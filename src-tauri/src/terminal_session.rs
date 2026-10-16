//! Terminal session manager backed by real pseudo-terminals.
//!
//! Previously each session was `script -q /dev/null ssh …` with piped stdio.
//! `script(1)` exists only to make the child believe it has a TTY, which buys
//! character echo and nothing else: no window size, no `SIGWINCH`, no job
//! control, and flag spellings that differ between BSD and GNU. Everything that
//! makes a terminal a terminal — resize, `Ctrl+C`, `vim` — needs an actual pty,
//! so this module opens one via `portable-pty` and spawns onto its slave.
//!
//! Output is buffered as **bytes**. The previous implementation decoded each
//! chunk with `from_utf8_lossy`, which corrupts any multi-byte character that
//! happens to straddle a read boundary. The flush thread uses
//! [`split_on_char_boundary`] to publish whole characters and retains the
//! trailing partial sequence for its next chunk.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::PtySize;
#[cfg(not(windows))]
use portable_pty::{CommandBuilder, NativePtySystem, PtyPair, PtySystem};

use crate::validation::{ensure_valid_profile, is_valid_k8s_name};

/// Maximum buffered output per session (1 MB). A shell can produce output far
/// faster than the UI drains it, so the buffer is bounded and drops from the
/// oldest end rather than growing until the process is killed by the OS.
const MAX_BUFFER_BYTES: usize = 1024 * 1024;

/// How much to keep when the cap is hit. Trimming back to exactly the cap makes
/// every subsequent read shift the whole buffer — with `yes` running that is a
/// ~1 MB memmove per 8 KB chunk, under the mutex the reader and the UI share.
/// Dropping down to this instead amortises the cost to roughly nothing.
const BUFFER_TRIM_TO_BYTES: usize = 512 * 1024;

/// How often buffered output is pushed to the UI.
///
/// Sending every pty read straight through would put one IPC message per 8 KB
/// chunk on the wire; a build log would drown the webview in messages it cannot
/// render faster than one frame anyway. Coalescing on a frame boundary keeps
/// latency imperceptible while collapsing bursts into a single message.
const FLUSH_INTERVAL: Duration = Duration::from_millis(16);

/// Fallback grid used between `create` and the first resize from the frontend.
/// Nothing renders at 0×0 and some shells abort on it, so never start there.
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
pub(crate) const MAX_SESSIONS: usize = 16;

type OutputBuffer = Arc<Mutex<Vec<u8>>>;

/// A live pty plus the child attached to it.
pub struct TerminalSession {
    /// Master side only — see `create` for why the slave is not kept.
    #[cfg(not(windows))]
    master: Box<dyn portable_pty::MasterPty + Send>,
    #[cfg(windows)]
    master: crate::windows_pty::Master,
    writer: Box<dyn Write + Send>,
    #[cfg(not(windows))]
    child: Box<dyn portable_pty::Child + Send + Sync>,
    #[cfg(windows)]
    child: crate::windows_pty::Child,
    #[cfg(windows)]
    origin: Option<(Arc<crate::engine_connection::EngineConnection>, u64)>,
    /// Cleared on close so the reader and flush threads unwind.
    running: Arc<AtomicBool>,
    /// Last time bytes moved in either direction. Shared with the flush thread
    /// so output counts as activity too — otherwise a `tail -f` left running
    /// would be reaped as idle while it is visibly working.
    last_activity: Arc<Mutex<Instant>>,
    closed: bool,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn trim_output(bytes: &mut Vec<u8>) {
    let mut start = bytes.len().saturating_sub(BUFFER_TRIM_TO_BYTES);
    while start < bytes.len() && bytes[start] & 0xc0 == 0x80 {
        start += 1;
    }
    bytes.drain(..start);
}

/// What a session is attached to.
///
/// All three spawn onto the same pty and share transport, resize, reaping and
/// the session cap; only the argv differs. Adding a fourth target should mean
/// adding a variant here, not a parallel code path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionKind {
    ContainerExec {
        container_id: String,
        #[serde(default)]
        engine: String,
    },
    Colima {
        profile: String,
    },
    Lima {
        instance: String,
    },
    K8sExec {
        namespace: String,
        pod: String,
        #[serde(default)]
        container: String,
    },
}

/// A resolved command line: program plus argv, never a shell string.
#[derive(Debug, PartialEq, Eq)]
pub struct Spawn {
    pub program: String,
    pub args: Vec<String>,
}

/// Per-profile history so `↑` survives across sessions and profiles do not
/// share a list.
///
/// `PROMPT_COMMAND='history -a'` matters because several tabs on one profile now
/// share this file: bash's default append-on-exit would keep only the last tab
/// to close. `HISTCONTROL=ignorespace` lets a user keep a secret out of the file
/// by prefixing the command with a space.
///
/// A login rc that assigns `HISTFILE` or `PROMPT_COMMAND` itself still wins —
/// there is no way to stop that from out here.
fn history_preamble(name: &str) -> String {
    format!(
        "mkdir -p \"$HOME/.colima-ui\"; \
         export HISTFILE=\"$HOME/.colima-ui/history-{name}\" \
         HISTCONTROL=ignorespace \
         PROMPT_COMMAND='history -a'; \
         exec \"$SHELL\" -l"
    )
}

/// Build the command for a session kind. Pure, so the injection guards below
/// are testable without opening a pty.
pub fn build_spawn(kind: &SessionKind, home: &str) -> Result<Spawn, String> {
    match kind {
        SessionKind::ContainerExec {
            container_id,
            engine,
        } => {
            if !crate::validation::is_stable_container_id(container_id) {
                return Err("Invalid container ID format".into());
            }
            if !engine.is_empty() {
                crate::engine_connection::validate_context_name(engine)?;
            }
            Ok(Spawn {
                program: "docker".into(),
                args: vec![
                    "exec".into(),
                    "-it".into(),
                    container_id.clone(),
                    "sh".into(),
                    "-c".into(),
                    "command -v bash >/dev/null 2>&1 && exec bash -l || exec sh".into(),
                ],
            })
        }
        SessionKind::Colima { profile } | SessionKind::Lima { instance: profile } => {
            ensure_valid_profile(profile)?;
            let name = if profile.is_empty() {
                "default"
            } else {
                profile
            };

            let (ssh_config, host) = match kind {
                SessionKind::Lima { .. } => (
                    format!("{home}/.lima/{name}/ssh.config"),
                    format!("lima-{name}"),
                ),
                _ => {
                    let lima_name = if name == "default" {
                        "colima".to_string()
                    } else {
                        format!("colima-{name}")
                    };
                    (
                        format!("{home}/.colima/_lima/{lima_name}/ssh.config"),
                        format!("lima-{lima_name}"),
                    )
                }
            };

            Ok(Spawn {
                program: "ssh".to_string(),
                args: vec![
                    "-tt".to_string(),
                    "-o".to_string(),
                    "LogLevel=QUIET".to_string(),
                    "-F".to_string(),
                    ssh_config,
                    host,
                    history_preamble(name),
                ],
            })
        }

        SessionKind::K8sExec {
            namespace,
            pod,
            container,
        } => {
            // Same validators the k8s HTTP routes use — these land in argv.
            if !is_valid_k8s_name(namespace) {
                return Err(format!("Invalid namespace: {namespace}"));
            }
            if !is_valid_k8s_name(pod) {
                return Err(format!("Invalid pod name: {pod}"));
            }
            if !container.is_empty() && !is_valid_k8s_name(container) {
                return Err(format!("Invalid container name: {container}"));
            }

            let mut args = vec![
                "exec".to_string(),
                "-it".to_string(),
                "-n".to_string(),
                namespace.clone(),
                pod.clone(),
            ];
            if !container.is_empty() {
                args.push("-c".to_string());
                args.push(container.clone());
            }
            // Prefer bash, fall back to sh.
            //
            // Note what this does *not* cover: the chain is evaluated by `sh`,
            // so it only helps an image that already has `sh`. An image built
            // FROM scratch — coredns, and most distroless bases — has no shell
            // at all, and kubectl exits 127 with a raw OCI error. That case is
            // caught by exit-code, in `exitHint` on the frontend; there is no
            // argv that can conjure a shell into an image which lacks one.
            args.push("--".to_string());
            args.push("sh".to_string());
            args.push("-c".to_string());
            args.push("command -v bash >/dev/null 2>&1 && exec bash -l || exec sh".to_string());

            Ok(Spawn {
                program: "kubectl".to_string(),
                args,
            })
        }
    }
}

/// Validated external preparation owns no PTY and never needs the shared map.
/// Keep daemon/tool/config probes here; commit performs only local authority
/// checks and PTY installation while holding the session manager.
struct PreparedSession {
    command: std::process::Command,
    #[cfg(windows)]
    origin: Option<(Arc<crate::engine_connection::EngineConnection>, u64)>,
}

impl PreparedSession {
    fn prepare(kind: &SessionKind) -> Result<Self, String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        // Validation lives in `build_spawn`, so it cannot be skipped by a caller
        // that builds a command some other way.
        let spawn = build_spawn(kind, &home)?;
        #[cfg(windows)]
        if !matches!(kind, SessionKind::ContainerExec { .. }) {
            return Err("Windows terminals support Docker containers only".into());
        }
        let mut command = if let SessionKind::ContainerExec {
            container_id,
            engine,
        } = kind
        {
            let (command, current) = container_command(container_id)?;
            if engine != &current {
                return Err(
                    "Terminal target differs from the active engine. Reopen it from Containers."
                        .into(),
                );
            }
            command
        } else {
            std::process::Command::new(crate::path_util::resolve_binary(&spawn.program))
        };
        #[cfg(windows)]
        let origin = if let SessionKind::ContainerExec { engine, .. } = kind {
            let connection = crate::engine_connection::active()?;
            if engine.is_empty() || engine != connection.target().context() {
                return Err("Terminal target differs from the active Docker engine. Reopen it from Containers.".into());
            }
            Some((connection.clone(), connection.read_generation()?))
        } else {
            None
        };
        command.args(&spawn.args).env("TERM", "xterm-256color");

        // Fail with something the user can act on rather than letting ssh emit
        // a config-file error into the terminal.
        if let Some(cfg) = spawn.args.iter().position(|a| a == "-F") {
            let path = &spawn.args[cfg + 1];
            if !std::path::Path::new(path).exists() {
                return Err(format!(
                    "SSH config not found: {path}. Is the instance running?"
                ));
            }
        }

        Ok(Self {
            command,
            #[cfg(windows)]
            origin,
        })
    }

    fn ensure_current(&self) -> Result<(), String> {
        #[cfg(windows)]
        if let Some((connection, generation)) = &self.origin {
            connection.ensure_generation(*generation)?;
        }
        Ok(())
    }
}

pub struct SessionManager {
    sessions: HashMap<String, TerminalSession>,
    pending: HashMap<String, u64>,
    next_pending: u64,
    closing: bool,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            pending: HashMap::new(),
            next_pending: 0,
            closing: false,
        }
    }

    fn reserve(&mut self, session_id: &str) -> Result<u64, String> {
        if self.closing {
            return Err("Application is closing".into());
        }
        if self.pending.contains_key(session_id) {
            return Err("Terminal session is already opening".into());
        }
        // Count the union: reconnecting a live id needs no additional slot,
        // while a slow open cannot allow another caller to oversubscribe.
        let occupied = self.sessions.len()
            + self
                .pending
                .keys()
                .filter(|id| !self.sessions.contains_key(*id))
                .count();
        check_session_limit(occupied, self.contains(session_id))?;
        self.next_pending = self
            .next_pending
            .checked_add(1)
            .ok_or("Terminal admission exhausted")?;
        self.pending
            .insert(session_id.to_owned(), self.next_pending);
        Ok(self.next_pending)
    }

    fn cancel_reservation(&mut self, session_id: &str, token: u64) {
        if self.pending.get(session_id) == Some(&token) {
            self.pending.remove(session_id);
        }
    }

    fn commit_prepared(
        &mut self,
        session_id: &str,
        token: u64,
        prepared: PreparedSession,
        on_output: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<(), String> {
        if self.closing || self.pending.get(session_id) != Some(&token) {
            return Err("Terminal opening was cancelled".into());
        }
        // No daemon subprocess here: reject obsolete prepared authority using
        // the connection's current generation before replacing a live session.
        prepared.ensure_current()?;
        self.cancel_reservation(session_id, token);
        if self.sessions.contains_key(session_id) {
            self.close(session_id)?;
        }
        let PreparedSession {
            command,
            #[cfg(windows)]
            origin,
        } = prepared;
        #[cfg(not(windows))]
        let (master, child, mut reader, writer) = {
            let pair = NativePtySystem::default()
                .openpty(PtySize {
                    rows: DEFAULT_ROWS,
                    cols: DEFAULT_COLS,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("Failed to open pty: {e}"))?;
            let mut cmd = CommandBuilder::new(command.get_program());
            cmd.args(command.get_args());
            for (key, value) in command.get_envs() {
                if let Some(value) = value {
                    cmd.env(key, value);
                } else {
                    cmd.env_remove(key);
                }
            }
            let PtyPair { master, slave } = pair;
            // Acquire fallible host I/O before the child can exist.
            let reader = master
                .try_clone_reader()
                .map_err(|e| format!("Failed to read pty: {e}"))?;
            let writer = master
                .take_writer()
                .map_err(|e| format!("Failed to write pty: {e}"))?;
            let child = slave
                .spawn_command(cmd)
                .map_err(|e| format!("Failed to spawn terminal: {e}"))?;
            drop(slave);
            (master, child, reader, writer)
        };
        #[cfg(windows)]
        let (master, child, mut reader, writer) = {
            let opened = crate::windows_pty::spawn(
                command,
                PtySize {
                    rows: DEFAULT_ROWS,
                    cols: DEFAULT_COLS,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .map_err(|e| format!("Failed to open Windows terminal: {e}"))?;
            (opened.master, opened.child, opened.reader, opened.writer)
        };

        // A pty merges stdout and stderr onto one stream, so one reader thread
        // replaces the two the piped version needed — and the interleaving is
        // now the kernel's, which is what the user actually typed at.
        let buffer: OutputBuffer = Arc::new(Mutex::new(Vec::new()));
        let running = Arc::new(AtomicBool::new(true));
        let last_activity = Arc::new(Mutex::new(Instant::now()));
        let trimmed = Arc::new(AtomicBool::new(false));
        let read_trimmed = trimmed.clone();

        let buf_clone = Arc::clone(&buffer);
        thread::spawn(move || {
            let mut tmp = [0u8; 8192];
            loop {
                match reader.read(&mut tmp) {
                    Ok(0) => break, // pty closed
                    Ok(n) => {
                        let mut buf = buf_clone.lock().unwrap();
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.len() > MAX_BUFFER_BYTES {
                            trim_output(&mut buf);
                            read_trimmed.store(true, Ordering::Release);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Flush thread: drains the buffer on a frame boundary and pushes text
        // to the UI. It owns the carry, so a character split across two pty
        // reads is completed on the next tick instead of being replaced.
        let flush_buf = Arc::clone(&buffer);
        let flush_running = Arc::clone(&running);
        let flush_activity = Arc::clone(&last_activity);
        thread::spawn(move || {
            let mut carry: Vec<u8> = Vec::new();
            loop {
                thread::sleep(FLUSH_INTERVAL);

                let (chunk, dropped) = {
                    let mut buf = flush_buf.lock().unwrap();
                    (
                        std::mem::take(&mut *buf),
                        trimmed.swap(false, Ordering::AcqRel),
                    )
                };
                if dropped {
                    carry.clear();
                }
                if chunk.is_empty() {
                    if !flush_running.load(Ordering::Relaxed) {
                        break;
                    }
                    continue;
                }

                let mut pending = std::mem::take(&mut carry);
                pending.extend_from_slice(&chunk);

                let (text, rest) = split_on_char_boundary(pending);
                carry = rest;
                if !text.is_empty() {
                    if let Ok(mut a) = flush_activity.lock() {
                        *a = Instant::now();
                    }
                    on_output(text);
                }
                if !flush_running.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        self.sessions.insert(
            session_id.to_string(),
            TerminalSession {
                master,
                writer,
                child,
                #[cfg(windows)]
                origin,
                running,
                last_activity,
                closed: false,
            },
        );

        Ok(())
    }

    /// Send input to the session.
    pub fn write(&mut self, session_id: &str, data: &str) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;

        if session.closed {
            return Err("Session is closed".to_string());
        }
        #[cfg(windows)]
        if let Some((connection, generation)) = &session.origin {
            if let Err(error) = connection.ensure_generation(*generation) {
                session.running.store(false, Ordering::Relaxed);
                let _ = session.child.kill();
                return Err(error);
            }
        }

        session
            .writer
            .write_all(data.as_bytes())
            .map_err(|e| format!("Write error: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("Flush error: {e}"))?;

        if let Ok(mut a) = session.last_activity.lock() {
            *a = Instant::now();
        }
        Ok(())
    }

    /// Close sessions with no traffic in either direction for `max_idle`.
    ///
    /// Multi-tab means nothing bounds pty count by construction: each tab is a
    /// real shell holding an ssh connection into the VM, and tabs are easy to
    /// open and forget. Returns the ids that were reaped so the caller can tell
    /// the UI.
    pub fn reap_idle(&mut self, max_idle: Duration) -> Vec<String> {
        let stale: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.last_activity.lock().is_ok_and(|a| a.elapsed() > max_idle))
            .map(|(id, _)| id.clone())
            .collect();

        for id in &stale {
            let _ = self.close(id);
        }
        stale
    }

    /// Tell the pty its new grid, which delivers `SIGWINCH` to the child.
    ///
    /// Without this a full-screen program keeps drawing into the size it saw at
    /// startup. Zero in either axis is rejected: it is never a real window and
    /// some shells abort on it.
    pub fn resize(&mut self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        if rows == 0 || cols == 0 {
            return Err("Refusing to resize to a zero-sized grid".to_string());
        }

        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;

        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Resize error: {e}"))
    }

    /// Report the exit code once, if the child has died.
    ///
    /// Output no longer needs polling — the flush thread pushes it — but the UI
    /// still has to learn that the shell is gone, or it sits on a dead prompt.
    /// Returns `Some` exactly once per session.
    pub fn poll_exit(&mut self, session_id: &str) -> Option<u32> {
        let session = self.sessions.get_mut(session_id)?;
        if session.closed {
            return None;
        }
        #[cfg(windows)]
        if session
            .origin
            .as_ref()
            .is_some_and(|(connection, generation)| {
                connection.ensure_generation(*generation).is_err()
            })
        {
            let _ = session.child.kill();
        }
        let status = session.child.try_wait().ok().flatten()?;
        let code = status.exit_code();
        let _ = self.close(session_id);
        Some(code)
    }

    /// Kill the session and reap it.
    ///
    /// `ssh -tt` is a single local process — the shell lives on the remote side
    /// and is hung up by sshd when the connection drops — so killing the child
    /// is enough and no process-group kill is needed. Dropping the master then
    /// closes the pty, which lets the reader thread see EOF and exit; clearing
    /// `running` unwinds the flush thread on its next tick.
    pub fn close(&mut self, session_id: &str) -> Result<(), String> {
        self.pending.remove(session_id);
        if let Some(mut session) = self.sessions.remove(session_id) {
            session.running.store(false, Ordering::Relaxed);
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
        Ok(())
    }

    /// Close every session. Called on app exit so no pty outlives the window.
    pub fn close_all(&mut self) {
        self.closing = true;
        self.pending.clear();
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let _ = self.close(&id);
        }
    }

    /// Number of live sessions. Used by the session cap.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether this id already has a session — a reconnect rather than a new tab.
    pub fn contains(&self, session_id: &str) -> bool {
        self.sessions.contains_key(session_id)
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

pub(crate) fn check_session_limit(count: usize, reconnect: bool) -> Result<(), String> {
    if !reconnect && count >= MAX_SESSIONS {
        Err(format!(
            "Too many terminal sessions open ({MAX_SESSIONS}). Close one first."
        ))
    } else {
        Ok(())
    }
}

/// Resolve readiness and origin at the resource boundary. No host shell or
/// automatic start is a recovery path for an absent/stopped/shell-less image.
pub(crate) fn container_command(id: &str) -> Result<(std::process::Command, String), String> {
    if !crate::validation::is_stable_container_id(id) {
        return Err("Invalid container ID format".into());
    }
    let mut command = crate::commands::runtime::get_runtime_cmd()?;
    #[cfg(windows)]
    let engine = crate::engine_connection::active()?
        .target()
        .context()
        .to_string();
    #[cfg(not(windows))]
    let engine = crate::commands::runtime::detect_runtime().1;
    command.args(["container", "inspect", "--format", "{{json .}}", id]);
    let output = crate::child_process::capture(command, Duration::from_secs(5), 1024 * 1024)
        .map_err(|e| format!("{engine}: cannot inspect terminal container: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{engine}: {}",
            crate::redact::redact(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Invalid terminal container response")?;
    if !value["Id"]
        .as_str()
        .is_some_and(|full| full.starts_with(id))
        || value["State"]["Running"].as_bool() != Some(true)
        || value["State"]["Paused"].as_bool() != Some(false)
    {
        return Err(format!(
            "{engine}: container is stopped, paused or no longer matches. Open it in Containers."
        ));
    }
    Ok((crate::commands::runtime::get_runtime_cmd()?, engine))
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Split pty output into decodable text plus any truncated trailing character.
///
/// A pty read can end mid-character; decoding that chunk on its own turns both
/// halves into `U+FFFD`. The tail is handed back so the caller can prepend it to
/// the next chunk.
///
/// Bytes that are invalid rather than merely incomplete are *not* carried — a
/// tail can be at most 3 bytes short of the 4-byte maximum, so anything longer
/// is real corruption and is replaced instead of being pushed back forever.
fn split_on_char_boundary(data: Vec<u8>) -> (String, Vec<u8>) {
    let mut out = String::new();
    let mut rest = &data[..];

    // Loop rather than inspecting only the first error. `from_utf8` reports one
    // error at a time, so a single stray byte early in the chunk would otherwise
    // force the whole remainder — including a legitimately truncated character
    // at the end — down the lossy path. That is reachable in normal operation:
    // the buffer cap cuts at an arbitrary offset, so a chunk read after an
    // overflow routinely *begins* mid-character.
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return (out, Vec::new());
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&rest[..valid]).unwrap_or_default());

                match e.error_len() {
                    // Truncated: keep it for the next chunk to complete. A
                    // prefix is at most 3 bytes short of the 4-byte maximum.
                    None => return (out, rest[valid..].to_vec()),
                    // Genuinely invalid: replace and keep scanning, so bad bytes
                    // never stall the stream.
                    Some(bad) => {
                        out.push(char::REPLACEMENT_CHARACTER);
                        rest = &rest[valid + bad..];
                    }
                }
            }
        }
    }
}

pub type SharedSessionManager = Arc<Mutex<SessionManager>>;

/// Production IPC acquisition: reserve briefly, prepare on a blocking worker
/// without the global map, then commit only the still-current reservation.
pub async fn create_shared_session(
    manager: SharedSessionManager,
    session_id: String,
    kind: SessionKind,
    on_output: Box<dyn Fn(String) + Send + 'static>,
) -> Result<(), String> {
    create_prepared_session(
        manager,
        session_id,
        move || PreparedSession::prepare(&kind),
        on_output,
    )
    .await
}

struct SessionReservation {
    manager: SharedSessionManager,
    id: String,
    token: u64,
}
impl Drop for SessionReservation {
    fn drop(&mut self) {
        if let Ok(mut manager) = self.manager.lock() {
            manager.cancel_reservation(&self.id, self.token);
        }
    }
}

async fn create_prepared_session<F>(
    manager: SharedSessionManager,
    id: String,
    prepare: F,
    on_output: Box<dyn Fn(String) + Send + 'static>,
) -> Result<(), String>
where
    F: FnOnce() -> Result<PreparedSession, String> + Send + 'static,
{
    let token = manager
        .lock()
        .map_err(|_| "Terminal manager unavailable")?
        .reserve(&id)?;
    let reservation = SessionReservation { manager, id, token };
    tokio::task::spawn_blocking(move || {
        let prepared = prepare()?;
        // The guard is dropped before the reservation's RAII cleanup, including
        // failed preparation/commit and a worker panic. Close cancels tokens.
        let mut manager = reservation
            .manager
            .lock()
            .map_err(|_| "Terminal manager unavailable")?;
        let outcome =
            manager.commit_prepared(&reservation.id, reservation.token, prepared, on_output);
        drop(manager);
        outcome
    })
    .await
    .map_err(|error| format!("Terminal task failed: {error}"))?
}

/// Sessions with no traffic for this long are closed.
///
/// Generous on purpose: output counts as activity, so this only fires on a
/// session that is genuinely doing nothing. Killing a shell someone is reading
/// would be worse than the leak it prevents.
const MAX_IDLE: Duration = Duration::from_secs(30 * 60);

/// How often to look for idle sessions. Coarse — the deadline is 30 minutes.
const IDLE_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

pub fn create_session_manager() -> SharedSessionManager {
    let mgr: SharedSessionManager = Arc::new(Mutex::new(SessionManager::new()));

    let sweeper = Arc::clone(&mgr);
    thread::spawn(move || loop {
        thread::sleep(IDLE_SWEEP_INTERVAL);
        if let Ok(mut m) = sweeper.lock() {
            m.reap_idle(MAX_IDLE);
        }
    });

    mgr
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/tester";

    // TestSpec=TC-WINUI-013: partial stream-text guard only, not installed IME,
    // terminal navigation, origin, cap or native ConPTY qualification.
    #[test]
    fn valid_utf8_round_trips_across_every_two_chunk_cut() {
        // OS reads can divide a character at any byte. Exhaust every pair of
        // cuts (including empty chunks) without a random generator or sleeps.
        for text in ["", "ASCII", "aé界🦀z", "e\u{301}\r\n日本語\u{3}"] {
            let bytes = text.as_bytes();
            for first in 0..=bytes.len() {
                for second in first..=bytes.len() {
                    let mut output = String::new();
                    let mut carry = Vec::new();
                    for chunk in [&bytes[..first], &bytes[first..second], &bytes[second..]] {
                        carry.extend_from_slice(chunk);
                        let (decoded, remainder) = split_on_char_boundary(carry);
                        output.push_str(&decoded);
                        carry = remainder;
                        assert!(carry.len() <= 3, "unbounded carry at {first}/{second}");
                    }
                    assert_eq!(output, text, "text changed at {first}/{second}");
                    assert!(carry.is_empty(), "valid completed text left a carried tail");
                }
            }
        }
    }

    // TC-WINUI-013/033: target data stays literal; no ID can become a host command.
    // TestSpec=TC-WINUI-013; TestSpec=TC-WINUI-033 — partial shared-owner contract.
    // TC013/033: a real owned PTY, not a host shell or personal Docker engine.
    #[test]
    fn terminal_fixture_child() {
        if std::env::var_os("COLIMAUI_TERMINAL_FIXTURE").is_none() {
            return;
        }
        use std::io::BufRead;
        println!("CUI-fixture-ready");
        std::io::stdout().flush().unwrap();
        for line in std::io::stdin().lock().lines() {
            match line.unwrap().as_str() {
                "fixture-input" => {
                    println!("CUI-child-accepted");
                    std::io::stdout().flush().unwrap();
                }
                "exit" => break,
                _ => {}
            }
        }
    }

    fn prepared_fixture() -> PreparedSession {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "terminal_session::tests::terminal_fixture_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("COLIMAUI_TERMINAL_FIXTURE", "1");
        PreparedSession {
            command,
            #[cfg(windows)]
            origin: None,
        }
    }

    fn fixture_session(
        manager: &mut SessionManager,
        id: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        let (sent, output) = std::sync::mpsc::channel();
        let token = manager.reserve(id).unwrap();
        manager
            .commit_prepared(
                id,
                token,
                prepared_fixture(),
                Box::new(move |text| {
                    let _ = sent.send(text);
                }),
            )
            .unwrap();
        output
    }

    fn expect_output(output: &std::sync::mpsc::Receiver<String>, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        while !text.contains(marker) {
            text.push_str(
                &output
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("owned PTY did not acknowledge input"),
            );
        }
    }

    struct HeldOpen {
        release: Option<std::sync::mpsc::Sender<()>>,
        worker: Option<std::thread::JoinHandle<Result<(), String>>>,
    }
    impl HeldOpen {
        fn finish(mut self) -> Result<(), String> {
            self.release.take().unwrap().send(()).unwrap();
            self.worker.take().unwrap().join().unwrap()
        }
    }
    impl Drop for HeldOpen {
        fn drop(&mut self) {
            // Release even on an assertion panic: a defective long lock or
            // current-thread-runtime stall produces a failed assertion, never
            // an unrecoverable test deadlock or runner-timeout "kill".
            if let Some(release) = self.release.take() {
                let _ = release.send(());
            }
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn held_open(
        manager: SharedSessionManager,
        id: &str,
        fail: bool,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Receiver<()>,
        HeldOpen,
    ) {
        let (entered, started) = std::sync::mpsc::channel();
        let (heartbeat, progress) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let id = id.to_owned();
        let worker = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let (signal, observed) = tokio::sync::oneshot::channel();
                    let opening = tokio::spawn(create_prepared_session(
                        manager,
                        id,
                        move || {
                            entered.send(()).unwrap();
                            let _ = signal.send(());
                            held.recv().unwrap();
                            if fail {
                                Err("fixture preflight rejected".into())
                            } else {
                                Ok(prepared_fixture())
                            }
                        },
                        Box::new(|_| {}),
                    ));
                    observed.await.unwrap();
                    heartbeat.send(()).unwrap();
                    opening.await.unwrap()
                })
        });
        (
            started,
            progress,
            HeldOpen {
                release: Some(release),
                worker: Some(worker),
            },
        )
    }

    #[test]
    fn slow_open_keeps_shared_controls_and_async_runtime_responsive() {
        for fail in [false, true] {
            let manager = Arc::new(Mutex::new(SessionManager::new()));
            let output = fixture_session(&mut manager.lock().unwrap(), "A");
            expect_output(&output, "CUI-fixture-ready");
            let (started, heartbeat, opening) = held_open(manager.clone(), "B", fail);
            started
                .recv_timeout(Duration::from_secs(5))
                .expect("prepare must start");
            heartbeat
                .recv_timeout(Duration::from_secs(2))
                .expect("async worker must progress while preparation is held");
            {
                let mut live = manager
                    .try_lock()
                    .expect("external preparation must not hold the shared manager");
                live.write("A", "fixture-input\n").unwrap();
                live.resize("A", 32, 100).unwrap();
                assert_eq!(live.poll_exit("A"), None);
            }
            expect_output(&output, "CUI-child-accepted");
            manager
                .try_lock()
                .expect("Close must remain available")
                .close("A")
                .unwrap();
            assert!(!manager.lock().unwrap().contains("A"));
            let result = opening.finish();
            assert_eq!(result.is_err(), fail);
            let mut live = manager.lock().unwrap();
            assert!(
                live.pending.is_empty(),
                "success and failure release admission"
            );
            assert_eq!(live.contains("B"), !fail);
            live.close_all();
            assert!(live.is_empty());
        }
    }

    #[test]
    fn close_and_shutdown_cancel_held_open_without_late_pty_or_output() {
        for shutdown in [false, true] {
            let manager = Arc::new(Mutex::new(SessionManager::new()));
            let (started, heartbeat, opening) = held_open(manager.clone(), "pending", false);
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            heartbeat.recv_timeout(Duration::from_secs(2)).unwrap();
            {
                let mut live = manager.try_lock().expect("close cannot wait for preflight");
                if shutdown {
                    live.close_all();
                } else {
                    live.close("pending").unwrap();
                }
                assert!(live.pending.is_empty());
            }
            assert_eq!(
                opening.finish().unwrap_err(),
                "Terminal opening was cancelled"
            );
            let mut live = manager.lock().unwrap();
            assert!(live.is_empty());
            if shutdown {
                assert!(live.reserve("late").is_err());
            } else {
                live.reserve("late").unwrap();
            }
        }
    }

    #[test]
    fn pending_capacity_duplicate_and_reconnect_share_one_admission_owner() {
        let mut manager = SessionManager::new();
        let _output = fixture_session(&mut manager, "live");
        for index in 0..15 {
            manager.reserve(&format!("pending-{index}")).unwrap();
        }
        assert!(manager.reserve("overflow").is_err());
        assert!(manager
            .reserve("pending-0")
            .unwrap_err()
            .contains("already opening"));
        let old = manager
            .reserve("live")
            .expect("live reconnect fits at cap16");
        assert!(
            manager.reserve("live").is_err(),
            "one in-flight prepare per id"
        );
        manager.close("live").unwrap();
        let new = manager.reserve("live").unwrap();
        assert_ne!(old, new);
        assert_eq!(
            manager
                .commit_prepared("live", old, prepared_fixture(), Box::new(|_| {}))
                .unwrap_err(),
            "Terminal opening was cancelled"
        );
        manager.cancel_reservation("live", old);
        assert_eq!(
            manager.pending.get("live"),
            Some(&new),
            "old cleanup cannot cancel a new open"
        );
        manager
            .commit_prepared("live", new, prepared_fixture(), Box::new(|_| {}))
            .unwrap();
        manager.close_all();
        assert!(manager.pending.is_empty());
        assert!(manager.is_empty());
    }

    #[test]
    fn failed_reconnect_keeps_the_existing_session_and_releases_pending_slot() {
        let manager = Arc::new(Mutex::new(SessionManager::new()));
        let output = fixture_session(&mut manager.lock().unwrap(), "live");
        expect_output(&output, "CUI-fixture-ready");
        let (started, heartbeat, opening) = held_open(manager.clone(), "live", true);
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        heartbeat.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(manager.lock().unwrap().contains("live"));
        assert!(opening.finish().is_err());
        let mut live = manager.lock().unwrap();
        assert!(live.pending.is_empty());
        live.write("live", "fixture-input\n").unwrap();
        drop(live);
        expect_output(&output, "CUI-child-accepted");
        manager.lock().unwrap().close_all();
    }

    #[tokio::test]
    async fn panicked_preparation_releases_admission_without_acquiring_a_session() {
        let manager = Arc::new(Mutex::new(SessionManager::new()));
        let result = create_prepared_session(
            manager.clone(),
            "panic-fixture".into(),
            || panic!("controlled preparation panic"),
            Box::new(|_| {}),
        )
        .await;
        assert!(result.unwrap_err().contains("Terminal task failed"));
        let mut live = manager.lock().unwrap();
        assert!(live.pending.is_empty());
        assert!(live.is_empty());
        live.reserve("panic-fixture").unwrap();
        live.close_all();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn prepared_origin_must_still_match_before_replacing_a_live_session() {
        let connection = crate::engine_connection::EngineConnection::identity_fixture();
        let generation = connection.read_generation().unwrap();
        let mut manager = SessionManager::new();
        let _output = fixture_session(&mut manager, "live");
        let token = manager.reserve("live").unwrap();
        let mut stale = prepared_fixture();
        stale.origin = Some((connection.clone(), generation + 1));
        assert!(manager
            .commit_prepared("live", token, stale, Box::new(|_| {}))
            .unwrap_err()
            .contains("availability changed"));
        assert!(
            manager.contains("live"),
            "obsolete preparation cannot close the existing session"
        );
        manager.cancel_reservation("live", token);
        let token = manager.reserve("live").unwrap();
        let mut current = prepared_fixture();
        current.origin = Some((connection, generation));
        manager
            .commit_prepared("live", token, current, Box::new(|_| {}))
            .unwrap();
        manager.close_all();
    }

    #[test]
    fn container_terminal_keeps_identity_as_one_literal_argument() {
        let kind = SessionKind::ContainerExec {
            container_id: "a".repeat(64),
            engine: "desktop-linux".into(),
        };
        let spawn = build_spawn(&kind, HOME).unwrap();
        assert_eq!(&spawn.args[..4], &["exec", "-it", &"a".repeat(64), "sh"]);
        let value = serde_json::to_value(&kind).unwrap();
        assert_eq!(value["kind"], "containerExec");
        assert_eq!(value["containerId"], "a".repeat(64));
        for id in ["--privileged", "a;whoami", "abc def", "../file", ""] {
            assert!(build_spawn(
                &SessionKind::ContainerExec {
                    container_id: id.into(),
                    engine: "desktop-linux".into()
                },
                HOME
            )
            .is_err());
        }
    }

    #[test]
    fn terminal_session_cap_refuses_growth_but_allows_existing_id_reconnect() {
        for count in [0, 15] {
            check_session_limit(count, false).unwrap();
        }
        for count in [16, 17] {
            assert!(check_session_limit(count, false).is_err());
            check_session_limit(count, true).unwrap();
        }
    }

    #[test]
    fn output_limit_keeps_a_recent_whole_utf8_window() {
        for length in [
            MAX_BUFFER_BYTES - 1,
            MAX_BUFFER_BYTES,
            MAX_BUFFER_BYTES + 1,
            2 * MAX_BUFFER_BYTES,
        ] {
            for text in ["é", "界", "🦀"] {
                let mut bytes = text.repeat(length.div_ceil(text.len())).into_bytes();
                let original = bytes.clone();
                if bytes.len() > MAX_BUFFER_BYTES {
                    trim_output(&mut bytes);
                }
                assert!(bytes.len() <= MAX_BUFFER_BYTES);
                if original.len() > MAX_BUFFER_BYTES {
                    assert!(bytes.len() <= BUFFER_TRIM_TO_BYTES);
                }
                assert!(std::str::from_utf8(&bytes).is_ok());
                assert!(original.ends_with(&bytes));
            }
        }
    }

    #[test]
    fn rejects_hostile_profile_names() {
        // Each of these would otherwise reach a config path or an ssh argv
        // position, where a leading `-` is read as a flag. Empty is deliberately
        // absent — it means "default VM", see the test below.
        for bad in ["../../etc/passwd", "-oProxyCommand=touch /tmp/pwn", "a b"] {
            let kind = SessionKind::Colima {
                profile: bad.to_string(),
            };
            let e = build_spawn(&kind, HOME).unwrap_err();
            assert!(
                e.contains("Invalid profile name"),
                "expected {bad:?} to be rejected, got: {e}"
            );
        }
    }

    #[test]
    fn rejects_hostile_k8s_names() {
        let hostile = "-o/etc/passwd";
        for kind in [
            SessionKind::K8sExec {
                namespace: hostile.to_string(),
                pod: "p".into(),
                container: String::new(),
            },
            SessionKind::K8sExec {
                namespace: "ns".into(),
                pod: hostile.to_string(),
                container: String::new(),
            },
            SessionKind::K8sExec {
                namespace: "ns".into(),
                pod: "p".into(),
                container: hostile.to_string(),
            },
        ] {
            assert!(
                build_spawn(&kind, HOME).is_err(),
                "hostile k8s name must not reach argv: {kind:?}"
            );
        }
    }

    #[test]
    fn k8s_exec_builds_argv_not_a_shell_string() {
        let spawn = build_spawn(
            &SessionKind::K8sExec {
                namespace: "kube-system".into(),
                pod: "coredns-75d478d48f-6c4v2".into(),
                container: "coredns".into(),
            },
            HOME,
        )
        .unwrap();

        assert_eq!(spawn.program, "kubectl");
        // Every user-supplied value is its own argv element, so nothing is
        // re-parsed and there is no quoting to get wrong.
        assert!(spawn.args.contains(&"kube-system".to_string()));
        assert!(spawn.args.contains(&"coredns-75d478d48f-6c4v2".to_string()));
        assert!(spawn.args.contains(&"coredns".to_string()));
        assert!(spawn.args.contains(&"-it".to_string()));
    }

    #[test]
    fn k8s_exec_omits_container_flag_when_unspecified() {
        let spawn = build_spawn(
            &SessionKind::K8sExec {
                namespace: "default".into(),
                pod: "p".into(),
                container: String::new(),
            },
            HOME,
        )
        .unwrap();

        // Only the args before `--` are kubectl's; `-c` also appears after it as
        // part of `sh -c`, so a naive `contains` check passes for the wrong
        // reason.
        let sep = spawn.args.iter().position(|a| a == "--").unwrap();
        assert!(
            !spawn.args[..sep].contains(&"-c".to_string()),
            "no container flag should be passed when none was requested"
        );
    }

    #[test]
    fn colima_and_lima_resolve_different_ssh_configs() {
        let colima = build_spawn(
            &SessionKind::Colima {
                profile: "default".into(),
            },
            HOME,
        )
        .unwrap();
        let lima = build_spawn(
            &SessionKind::Lima {
                instance: "ubuntu".into(),
            },
            HOME,
        )
        .unwrap();

        assert!(colima
            .args
            .iter()
            .any(|a| a.contains(".colima/_lima/colima/")));
        assert!(lima.args.iter().any(|a| a.contains(".lima/ubuntu/")));
    }

    #[test]
    fn history_file_is_per_profile() {
        let a = build_spawn(
            &SessionKind::Colima {
                profile: "a".into(),
            },
            HOME,
        )
        .unwrap();
        let b = build_spawn(
            &SessionKind::Colima {
                profile: "b".into(),
            },
            HOME,
        )
        .unwrap();

        let hist = |s: &Spawn| s.args.last().unwrap().clone();
        assert!(hist(&a).contains("history-a"));
        assert!(hist(&b).contains("history-b"));
        assert_ne!(hist(&a), hist(&b), "profiles must not share a history file");
        // Multiple tabs on one profile share the file, so appending per prompt
        // is what stops all but the last-closed tab losing its history.
        assert!(hist(&a).contains("history -a"));
    }

    #[test]
    fn refuses_zero_sized_resize() {
        let mut m = SessionManager::new();
        assert!(m.resize("missing", 0, 80).is_err());
        assert!(m.resize("missing", 24, 0).is_err());
    }

    #[test]
    fn whole_characters_decode_untouched() {
        let (text, carry) = split_on_char_boundary("hello ☃".as_bytes().to_vec());
        assert_eq!(text, "hello ☃");
        assert!(carry.is_empty());
    }

    #[test]
    fn truncated_character_is_carried_not_mangled() {
        let full = "ab☃".as_bytes().to_vec();
        // Cut one byte off the snowman: the old lossy decode produced "ab\u{FFFD}".
        let split = full[..full.len() - 1].to_vec();

        let (text, carry) = split_on_char_boundary(split);
        assert_eq!(text, "ab");
        assert_eq!(carry, vec![0xE2, 0x98]);

        // Feeding the carry back with the missing byte reconstitutes the char.
        let mut next = carry;
        next.push(0x83);
        let (rest, carry2) = split_on_char_boundary(next);
        assert_eq!(rest, "☃");
        assert!(carry2.is_empty());
    }

    /// Regression: an invalid byte *before* a truncated character used to
    /// disable the carry for the rest of the chunk, because `from_utf8` only
    /// reports its first error. The buffer cap cuts at an arbitrary offset, so
    /// chunks that start with a broken byte and end mid-character are normal
    /// after an overflow — not a contrived input.
    #[test]
    fn invalid_byte_does_not_disable_carry_for_later_truncation() {
        let (text, carry) = split_on_char_boundary(vec![0xFF, b'a', 0xE2, 0x98]);
        assert_eq!(text, "\u{FFFD}a");
        assert_eq!(
            carry,
            vec![0xE2, 0x98],
            "truncated tail must still be carried when an invalid byte precedes it"
        );
    }

    #[test]
    fn empty_profile_means_default_not_rejected() {
        // Empty is "the default VM" everywhere else in the codebase, so it must
        // resolve rather than error.
        let spawn = build_spawn(
            &SessionKind::Colima {
                profile: String::new(),
            },
            HOME,
        )
        .unwrap();
        assert!(spawn.args.iter().any(|a| a.contains("_lima/colima/")));
    }

    #[test]
    fn invalid_bytes_are_replaced_rather_than_carried_forever() {
        let (text, carry) = split_on_char_boundary(vec![b'o', b'k', 0xFF, 0xFE, b'!']);
        assert!(text.starts_with("ok"));
        assert!(
            carry.is_empty(),
            "invalid bytes must not be pushed back, or the stream stalls"
        );
    }
}
