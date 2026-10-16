//! Long-running CLI commands whose output must not sit in memory.
//!
//! `helpers::run_cmd` provides bounded short captures. Multi-gigabyte
//! archives need a file sink rather than resident capture. This module streams instead: stdout goes straight to a file descriptor
//! or is consumed line by line, so resident memory stays flat regardless of how
//! much data passes through.
//!
//! It also owns what a plain `Stdio::piped()` would not give us: cancelling a
//! command mid-flight, reporting progress, cleaning up half-written output, and
//! killing every child on the way out of the app.
//!
//! Environment setup and the "not installed" message are shared with `run_cmd`
//! via `helpers::build_cmd` / `helpers::describe_exec_error` — deliberately not
//! reimplemented here.
//!
//! **Path confinement is the caller's job.** This module writes wherever it is
//! told. See the base-directory policy documented on
//! `validation::assert_path_within`.

use crate::child_process::ChildProcess;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

use crate::helpers::{build_cmd, describe_exec_error};

/// How often the wait loop reports progress and checks for cancellation.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How long to wait for a killed command's stderr before giving up on it.
///
/// A grandchild that escaped the process-group kill still holds the pipe's write
/// end open, and reading it to EOF would then block for as long as that process
/// lives. Cancellation must not inherit somebody else's lifetime.
const MAX_LINE_BYTES: usize = 1024 * 1024;

const STDERR_TIMEOUT: Duration = Duration::from_secs(2);

/// Where a command's stdout goes.
pub enum OutputSink {
    /// Straight to this file. Nothing passes through our address space, so this
    /// is the sink for multi-gigabyte payloads like `docker save`.
    ToFile(PathBuf),
    /// Publish only if the destination is still absent at the commit boundary.
    ToNewFile(PathBuf),
    /// One message per line of stdout. For chatty-but-small output that the UI
    /// wants to display as it arrives.
    ByLine(SyncSender<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct StreamOutcome {
    /// Bytes written to the file, or bytes of stdout consumed, whichever applies.
    pub bytes: u64,
    /// True when the run ended because someone called [`cancel_stream`].
    pub cancelled: bool,
}

#[derive(Clone)]
struct RunningCmd {
    child: Arc<Mutex<ChildProcess>>,
    cancelled: Arc<AtomicBool>,
    published: Arc<AtomicBool>,
    publication: Arc<Mutex<()>>,
}

/// Every stream currently in flight, so they can be cancelled by id and killed
/// wholesale when the app exits.
static STOPPING: AtomicBool = AtomicBool::new(false);

static RUNNING: LazyLock<Mutex<HashMap<String, RunningCmd>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Run `program` with output streamed to `sink`.
///
/// Blocks until the command finishes, is cancelled, or fails — call it from a
/// blocking context (`helpers::run_blocking`). `on_progress` receives the running
/// byte count roughly every [`POLL_INTERVAL`].
///
/// `job_id` must be unique among running streams; a collision is refused rather
/// than resolved. Note that this says nothing about *destinations* — this module
/// writes wherever it is told and two jobs may name one path. The scratch file is
/// therefore keyed by job id as well, so concurrent writers cannot interleave into
/// one file. (`commands::file_transfer` refuses that case a layer up, where it can
/// see the other job; nothing here depends on it having done so.)
///
/// A `ToFile` destination is only ever written by renaming a completed scratch file
/// onto it, so cancellation and non-zero exits publish nothing and leave whatever
/// was already at that path alone. A truncated tar is worse than no tar: it looks
/// loadable and is not.
pub fn run_cmd_streaming(
    job_id: &str,
    program: &str,
    args: &[&str],
    sink: OutputSink,
    on_progress: impl FnMut(u64),
) -> Result<StreamOutcome, String> {
    run_streaming(
        job_id,
        build_cmd(program, args)?,
        program,
        sink,
        on_progress,
    )
}

/// Stream an already-built command.
///
/// Container-runtime callers must use this rather than [`run_cmd_streaming`]:
/// `commands::runtime::get_runtime_cmd` is what resolves docker vs podman vs the
/// `colima nerdctl` wrapper, along with the binary path and `DOCKER_HOST`.
/// Rebuilding that from a program name would work for docker and quietly fail for
/// everyone else.
///
/// `program_label` only names the command in error messages.
pub fn run_streaming(
    job_id: &str,
    cmd: Command,
    program_label: &str,
    sink: OutputSink,
    on_progress: impl FnMut(u64),
) -> Result<StreamOutcome, String> {
    run_streaming_with_commit(
        job_id,
        cmd,
        program_label,
        sink,
        on_progress,
        |partial, destination, overwrite| {
            publish_file(partial, destination, overwrite)
                .map(Some)
                .map_err(|e| format!("Cannot write {}: {e}", destination.display()))
        },
    )
}

/// A consumer may atomically admit publication with its own cancellation and
/// outcome state. None means cancellation won; no consumer policy lives here.
pub(crate) fn run_streaming_with_commit(
    job_id: &str,
    cmd: Command,
    program_label: &str,
    sink: OutputSink,
    on_progress: impl FnMut(u64),
    commit: impl FnOnce(&Path, &Path, bool) -> Result<Option<u64>, String>,
) -> Result<StreamOutcome, String> {
    if STOPPING.load(Ordering::SeqCst) {
        return Err("Application is closing".into());
    }
    match sink {
        OutputSink::ToFile(path) => {
            stream_to_file(job_id, cmd, program_label, &path, true, on_progress, commit)
        }
        OutputSink::ToNewFile(path) => stream_to_file(
            job_id,
            cmd,
            program_label,
            &path,
            false,
            on_progress,
            commit,
        ),
        OutputSink::ByLine(tx) => stream_by_line(job_id, cmd, program_label, tx, on_progress),
    }
}

/// Child ownership is shared with short captures: Unix groups and Windows Jobs.
fn spawn(mut cmd: Command, stdout: Stdio) -> Result<ChildProcess, std::io::Error> {
    cmd.stdout(stdout).stderr(Stdio::piped());
    ChildProcess::spawn(cmd)
}

fn kill_group(child: &mut ChildProcess) {
    child.terminate();
}

/// Ask a running stream to stop. Returns false if no such stream is running.
///
/// Marks the job cancelled *before* killing, so the waiting thread reports
/// cancellation rather than mistaking the kill for a command failure.
pub fn cancel_stream(job_id: &str) -> bool {
    let entry = RUNNING.lock().ok().and_then(|map| map.get(job_id).cloned());
    let Some(entry) = entry else {
        return false;
    };
    // Publication starts only after child completion. Never hold the global
    // map across I/O or wait for another job's final filesystem operation.
    let Ok(_publication) = entry.publication.try_lock() else {
        return false;
    };
    if entry.published.load(Ordering::SeqCst) {
        return false;
    }
    entry.cancelled.store(true, Ordering::SeqCst);
    if let Ok(mut child) = entry.child.lock() {
        kill_group(&mut child);
    }
    true
}

/// Kill every running stream. Called when the app is exiting: a `docker save`
/// started from a window that no longer exists has nothing left to report to.
///
/// Returns how many were killed.
pub fn kill_all_streams() -> usize {
    let entries = RUNNING
        .lock()
        .map(|map| map.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut killed = 0;
    for entry in entries {
        let Ok(_publication) = entry.publication.try_lock() else {
            continue;
        };
        if entry.published.load(Ordering::SeqCst) {
            continue;
        }
        entry.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut child) = entry.child.lock() {
            kill_group(&mut child);
            killed += 1;
        }
    }
    killed
}

pub fn shutdown_streams() -> usize {
    STOPPING.store(true, Ordering::SeqCst);
    kill_all_streams()
}

pub fn active_stream_count() -> usize {
    RUNNING.lock().map_or(0, |m| m.len())
}

/// Job ids cancelled before their child existed.
///
/// `cancel_stream` can only kill a process that is already registered, and there is
/// a real gap before that: the caller holds the job id as soon as the transfer is
/// accepted, but the child is spawned on a blocking task that may not have been
/// scheduled yet. A cancel landing in that gap used to be dropped, and the job ran
/// to completion after the user stopped it. [`arm_cancel`] records the intent here
/// instead, and [`register`] honours it the moment the child appears.
static PENDING_CANCEL: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// Cancel a stream, or arm the cancel if its child does not exist yet.
///
/// Unlike [`cancel_stream`] this is for callers that already know the job is real —
/// otherwise a typo would leave an entry armed forever. `commands::file_transfer`
/// checks its own registry first.
///
/// Returns true when a running child was killed outright.
pub fn arm_cancel(job_id: &str) -> bool {
    if let Ok(mut pending) = PENDING_CANCEL.lock() {
        pending.insert(job_id.to_string());
    }
    cancel_stream(job_id)
}

/// Forget an armed cancel that was never consumed.
///
/// A job that finished on its own leaves its entry behind; without this the set
/// would grow for the life of the process and a later job reusing the id — which
/// cannot happen today, but ids are only a counter — would be cancelled at birth.
pub fn disarm_cancel(job_id: &str) {
    if let Ok(mut pending) = PENDING_CANCEL.lock() {
        pending.remove(job_id);
    }
}

/// Register a spawned child, refusing a duplicate id.
///
/// The returned flag starts out true when the job was cancelled while it was still
/// starting, so the wait loop stops it on its first tick rather than running a
/// command nobody wants any more.
fn register(
    job_id: &str,
    child: ChildProcess,
) -> Result<(Arc<Mutex<ChildProcess>>, Arc<AtomicBool>), String> {
    // Publish under RUNNING before arm_cancel can miss us. Read pending while
    // holding that lock; the opposite order leaves a lost-cancel gap.
    let mut map = RUNNING
        .lock()
        .map_err(|_| "Stream registry is poisoned".to_string())?;
    let armed = PENDING_CANCEL
        .lock()
        .map_err(|_| "Cancellation registry is poisoned".to_string())?
        .remove(job_id);
    if STOPPING.load(Ordering::SeqCst) {
        return Err("Application is closing".into());
    }
    if map.contains_key(job_id) {
        return Err(format!("A stream with id {} is already running", job_id));
    }
    let handle = Arc::new(Mutex::new(child));
    let cancelled = Arc::new(AtomicBool::new(armed));
    map.insert(
        job_id.to_string(),
        RunningCmd {
            child: handle.clone(),
            cancelled: cancelled.clone(),
            published: Arc::new(AtomicBool::new(false)),
            publication: Arc::new(Mutex::new(())),
        },
    );
    Ok((handle, cancelled))
}

struct Registration {
    id: String,
    active: bool,
}
impl Registration {
    fn pending(id: &str) -> Self {
        Self {
            id: id.into(),
            active: false,
        }
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if self.active {
            unregister(&self.id);
        }
    }
}

fn unregister(job_id: &str) {
    if let Ok(mut map) = RUNNING.lock() {
        map.remove(job_id);
    }
    disarm_cancel(job_id);
}

/// Drain stderr on its own thread.
///
/// Two reasons this is a thread and not an inline read: the child blocks once it
/// has filled a pipe buffer's worth of stderr while we are still waiting on
/// stdout, and the result must be retrievable *with a deadline* — hence a channel
/// rather than a `JoinHandle`, which can only be joined unboundedly.
fn drain_stderr(child: &mut ChildProcess) -> Option<Receiver<String>> {
    let stderr = child.stderr.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut reader = stderr;
        let mut chunk = [0; 8192];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let keep = n.min(MAX_LINE_BYTES.saturating_sub(buf.len()));
            buf.extend_from_slice(&chunk[..keep]);
        }
        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
    });
    Some(rx)
}

/// Read the drained stderr, giving up after [`STDERR_TIMEOUT`].
///
/// Only worth calling on the failure path — that is the only time the text is
/// used, and waiting for it otherwise would make a successful or cancelled run
/// hostage to whatever still holds the pipe open.
fn collect_stderr(rx: Option<Receiver<String>>) -> String {
    rx.and_then(|rx| rx.recv_timeout(STDERR_TIMEOUT).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

/// Sibling scratch file a streamed download is written to before it is published.
///
/// A sibling rather than a temp directory: the rename below is only atomic within
/// one filesystem, and `/tmp` frequently is not the same one as the folder the
/// user picked.
///
/// The job id is part of the name because two jobs can legitimately target one
/// destination — the registry dedups on job id, and `resolve_destination` cannot
/// see a conflict since the destination does not exist yet while the first job is
/// still writing. Keying only on the destination let both open the same scratch
/// file and interleave their bytes into it. Now each job writes its own and the
/// loser of the rename is simply overwritten by a complete archive.
///
/// Public so that tests observing a transfer in flight watch the file that is
/// actually growing, rather than re-deriving the convention and drifting from it.
pub fn partial_sink_path(path: &Path, job_id: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", sanitize_job_id(job_id)));
    path.with_file_name(name)
}

/// Keep a job id usable as part of a file name.
///
/// Ids are generated internally (`save-3`, `cp-out-12`) so this is belt and braces,
/// but the id reaches the filesystem here and a separator in one would place the
/// scratch file somewhere else entirely.
fn sanitize_job_id(job_id: &str) -> String {
    job_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Run `cmd` with stdout redirected into `path`.
///
/// The bytes land in a `.part` sibling and are renamed onto `path` only after the
/// command exits successfully. Writing straight to `path` meant `File::create`
/// truncated whatever was already there, and every abort path then removed it — so
/// cancelling an overwrite destroyed the file the user still had. Nothing touches
/// `path` until there is a complete artefact to put there.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Refuse an unexpected competing file atomically rather than checking exists
/// then renaming. Windows MoveFileEx without REPLACE_EXISTING; POSIX link+unlink
/// within the same filesystem leaves the existing directory entry untouched.
pub(crate) fn publish_file(
    partial: &Path,
    destination: &Path,
    overwrite: bool,
) -> std::io::Result<u64> {
    let bytes = std::fs::metadata(partial)?.len();
    if overwrite {
        std::fs::rename(partial, destination)?;
        return Ok(bytes);
    }
    #[cfg(windows)]
    {
        let source = crate::windows_host::wide_path(partial)?;
        let target = crate::windows_host::wide_path(destination)?;
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                0,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::hard_link(partial, destination)?;
        // Publication has committed; a scratch-unlink error cannot turn a valid
        // destination into a fictional failure. Scratch's Drop retries removal.
        let _ = std::fs::remove_file(partial);
    }
    Ok(bytes)
}

fn stream_to_file(
    job_id: &str,
    cmd: Command,
    program: &str,
    path: &Path,
    overwrite: bool,
    mut on_progress: impl FnMut(u64),
    commit: impl FnOnce(&Path, &Path, bool) -> Result<Option<u64>, String>,
) -> Result<StreamOutcome, String> {
    // Declared before Scratch so every early-return cleanup precedes release.
    let mut registration = Registration::pending(job_id);
    let partial = partial_sink_path(path, job_id);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|e| format!("Cannot create {}: {}", partial.display(), e))?;

    let _scratch = Scratch(partial.clone());
    let mut child = match spawn(cmd, Stdio::from(file)) {
        Ok(child) => child,
        Err(e) => {
            // We created the scratch file, so we remove it — a failed spawn must
            // not leave a zero-byte artefact behind.
            let _ = std::fs::remove_file(&partial);
            return Err(describe_exec_error(program, &e));
        }
    };

    let stderr = drain_stderr(&mut child);
    let (handle, cancelled) = match register(job_id, child) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };

    registration.active = true;

    // Cancelled while it was still starting. The flag alone stops nothing — the
    // wait below is waiting on the process, not on a boolean — so the kill that
    // `cancel_stream` could not deliver has to happen here.
    if cancelled.load(Ordering::SeqCst) {
        if let Ok(mut child) = handle.lock() {
            kill_group(&mut child);
        }
    }

    let status = wait_with_progress(&handle, || on_progress(file_len(&partial)));
    if let Ok(mut child) = handle.lock() {
        child.terminate();
    }

    if cancelled.load(Ordering::SeqCst) {
        // stderr is deliberately not collected here: it is unused on this path,
        // and waiting for it would make cancellation as slow as the process that
        // holds the pipe.
        let _ = std::fs::remove_file(&partial);
        return Ok(StreamOutcome {
            bytes: 0,
            cancelled: true,
        });
    }

    let status = status.map_err(|e| format!("Failed to wait for {}: {}", program, e))?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("{} failed: {}", program, collect_stderr(stderr)));
    }

    let bytes = {
        let entry = RUNNING
            .lock()
            .map_err(|_| "Stream registry unavailable")?
            .get(job_id)
            .cloned()
            .ok_or("Stream registration disappeared before publication")?;
        let _publication = entry
            .publication
            .lock()
            .map_err(|_| "Output publication unavailable")?;
        if entry.cancelled.load(Ordering::SeqCst) {
            None
        } else {
            let result = commit(&partial, path, overwrite)?;
            entry.published.store(result.is_some(), Ordering::SeqCst);
            result
        }
    };
    let Some(bytes) = bytes else {
        return Ok(StreamOutcome {
            bytes: 0,
            cancelled: true,
        });
    };
    on_progress(bytes);
    Ok(StreamOutcome {
        bytes,
        cancelled: false,
    })
}

fn stream_by_line(
    job_id: &str,
    cmd: Command,
    program: &str,
    tx: SyncSender<String>,
    mut on_progress: impl FnMut(u64),
) -> Result<StreamOutcome, String> {
    let mut registration = Registration::pending(job_id);
    let mut child = spawn(cmd, Stdio::piped()).map_err(|e| describe_exec_error(program, &e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{}: stdout was not captured", program))?;
    let stderr = drain_stderr(&mut child);
    let (handle, cancelled) = register(job_id, child)?;
    registration.active = true;

    // Cancelled during startup: deliver the kill `cancel_stream` could not, so the
    // read loop below ends on the resulting error rather than streaming a command
    // the user already stopped.
    if cancelled.load(Ordering::SeqCst) {
        if let Ok(mut child) = handle.lock() {
            kill_group(&mut child);
        }
    }

    // Drain on a separate thread so a helper holding stdout cannot hide its
    // parent's exit from the owner. Slow/closed consumers fail with bounded work.
    let bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let read_bytes = bytes.clone();
    let read_handle = handle.clone();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader
                .by_ref()
                .take((MAX_LINE_BYTES + 1) as u64)
                .read_until(b'\n', &mut line)
            {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    read_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    let error = if line.len() > MAX_LINE_BYTES {
                        Some("Output line exceeds the stream limit")
                    } else if tx
                        .try_send(String::from_utf8_lossy(&line).trim_end().to_string())
                        .is_err()
                    {
                        Some("Output consumer closed or fell behind")
                    } else {
                        None
                    };
                    if let Some(error) = error {
                        if let Ok(mut child) = read_handle.lock() {
                            child.terminate();
                        }
                        return Err(error.to_string());
                    }
                }
                Err(e) => {
                    if let Ok(mut child) = read_handle.lock() {
                        child.terminate();
                    }
                    return Err(format!("Cannot read output: {e}"));
                }
            }
        }
    });
    let status = wait_with_progress(&handle, || on_progress(bytes.load(Ordering::Relaxed)));
    if let Ok(mut child) = handle.lock() {
        child.terminate();
    }
    let read_result = reader
        .join()
        .map_err(|_| "Output reader failed".to_string())?;
    let bytes = bytes.load(Ordering::Relaxed);

    if cancelled.load(Ordering::SeqCst) {
        // See the note on the same branch in `stream_to_file`: stderr is not
        // collected on a cancelled run.
        return Ok(StreamOutcome {
            bytes,
            cancelled: true,
        });
    }

    read_result?;
    let status = status.map_err(|e| format!("Failed to wait for {}: {}", program, e))?;
    if !status.success() {
        return Err(format!("{} failed: {}", program, collect_stderr(stderr)));
    }

    on_progress(bytes);
    Ok(StreamOutcome {
        bytes,
        cancelled: false,
    })
}

/// Wait for the child, running `tick` between polls.
///
/// The lock is held only for `try_wait` and released before sleeping, so
/// `cancel_stream` is never blocked waiting for this loop.
fn wait_with_progress(
    handle: &Arc<Mutex<ChildProcess>>,
    mut tick: impl FnMut(),
) -> std::io::Result<std::process::ExitStatus> {
    loop {
        {
            let mut child = handle
                .lock()
                .map_err(|_| std::io::Error::other("child handle is poisoned"))?;
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
        }
        tick();
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// The registry is process-global, and so is `kill_all_streams` — by design,
    /// since app exit must reap everything. That makes these tests mutually
    /// exclusive: cargo runs them on parallel threads in one process, where one
    /// test's `kill_all_streams` would cancel another's running command. Every
    /// test in this module takes this guard first.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    /// Take the serialization guard, ignoring poisoning from an earlier failure —
    /// one failing test should not cascade into false failures in the rest.
    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn job(name: &str) -> String {
        format!("test-{}-{:?}", name, std::thread::current().id())
    }

    fn temp_path(name: &str) -> PathBuf {
        static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
            let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp");
            std::fs::create_dir_all(&parent).unwrap();
            let root = parent.join(format!(
                "stream-fixture-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            root.canonicalize().unwrap()
        });
        ROOT.join(format!("{}-{:?}", name, std::thread::current().id()))
    }

    fn portable_fixture(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "streaming_cmd::tests::stream_fixture",
            "--ignored",
            "--nocapture",
        ]);
        command.env("COLIMA_STREAM_MODE", mode);
        command
    }

    fn node_fixture(mode: &str) -> Command {
        let mut command = crate::helpers::build_cmd("node", &[])
            .expect("pinned Node is required for portable process tests");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/process.mjs"))
            .arg(mode);
        command
    }

    fn run_node_fixture(
        job_id: &str,
        mode: &str,
        sink: OutputSink,
        on_progress: impl FnMut(u64),
    ) -> Result<StreamOutcome, String> {
        run_streaming(job_id, node_fixture(mode), "node", sink, on_progress)
    }

    #[test]
    #[ignore = "owned subprocess fixture invoked by guarding stream tests"]
    fn stream_fixture() {
        use std::io::Write;
        match std::env::var("COLIMA_STREAM_MODE").unwrap().as_str() {
            "release" => {
                let gate = PathBuf::from(std::env::var_os("COLIMA_STREAM_GATE").unwrap());
                let until = Instant::now() + Duration::from_secs(5);
                while !gate.exists() {
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(10));
                }
                println!("released payload");
            }
            "long" => {
                std::io::stdout()
                    .write_all(&vec![b'x'; MAX_LINE_BYTES + 2])
                    .unwrap();
            }
            "failure" => {
                print!("partial");
                eprintln!("known-stream-failure");
                std::process::exit(3);
            }
            "hold" => {
                println!("READY");
                std::io::stdout().flush().unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            _ => println!("PAYLOAD:archive 工具 with spaces"),
        }
    }

    // TC-WINUI-011/012: these exercise native executable/process/file seams on
    // Windows and POSIX; older shell-specific fixtures remain separate evidence.
    #[test]
    fn portable_stream_publishes_complete_bytes_and_preserves_prior_output_on_failure() {
        let _guard = exclusive();
        let path = temp_path("portable");
        let id = job("portable");
        let outcome = run_streaming(
            &id,
            portable_fixture("normal"),
            "fixture",
            OutputSink::ToFile(path.clone()),
            |_| {},
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("PAYLOAD:archive 工具 with spaces"));
        assert_eq!(outcome.bytes, bytes.len() as u64);
        let error = run_streaming(
            &id,
            portable_fixture("failure"),
            "fixture",
            OutputSink::ToFile(path.clone()),
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("known-stream-failure"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!partial_sink_path(&path, &id).exists());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn oversized_lines_and_stalled_consumers_fail_with_bounded_output() {
        let _guard = exclusive();
        let (tx, _rx) = mpsc::sync_channel(64);
        let error = run_streaming(
            &job("long"),
            portable_fixture("long"),
            "fixture",
            OutputSink::ByLine(tx),
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("line exceeds"));
        let (tx, _rx) = mpsc::sync_channel(0);
        let error = run_streaming(
            &job("slow-reader"),
            portable_fixture("normal"),
            "fixture",
            OutputSink::ByLine(tx),
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("closed or fell behind"));
    }

    #[test]
    fn a_destination_created_while_streaming_is_never_overwritten_without_permission() {
        let _guard = exclusive();
        let path = temp_path("competing-destination");
        let gate = temp_path("competing-gate");
        let mut command = portable_fixture("release");
        command.env("COLIMA_STREAM_GATE", &gate);
        let mut arranged = false;
        let result = run_streaming(
            &job("competing"),
            command,
            "fixture",
            OutputSink::ToNewFile(path.clone()),
            |_| {
                if !arranged {
                    std::fs::write(&path, b"created by another writer").unwrap();
                    std::fs::write(&gate, b"release").unwrap();
                    arranged = true;
                }
            },
        );
        assert!(arranged && result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"created by another writer");
        assert!(!partial_sink_path(&path, &job("competing")).exists());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&gate).unwrap();
    }

    #[test]
    fn an_existing_scratch_entry_is_never_truncated_or_claimed() {
        let _guard = exclusive();
        let path = temp_path("scratch-owner");
        let id = job("scratch-owner");
        let scratch = partial_sink_path(&path, &id);
        std::fs::write(&scratch, b"belongs to another operation").unwrap();
        assert!(run_streaming(
            &id,
            portable_fixture("normal"),
            "fixture",
            OutputSink::ToFile(path.clone()),
            |_| {}
        )
        .is_err());
        assert_eq!(
            std::fs::read(&scratch).unwrap(),
            b"belongs to another operation"
        );
        assert!(!path.exists());
        std::fs::remove_file(scratch).unwrap();
    }

    #[test]
    fn portable_cancel_discards_scratch_and_releases_registry_after_cleanup() {
        let _guard = exclusive();
        let path = temp_path("portable-cancel");
        let id = job("portable-cancel");
        let cancel_id = id.clone();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !cancel_stream(&cancel_id) {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let outcome = run_streaming(
            &id,
            portable_fixture("hold"),
            "fixture",
            OutputSink::ToFile(path.clone()),
            |_| {},
        )
        .unwrap();
        worker.join().unwrap();
        assert!(outcome.cancelled);
        assert!(!path.exists());
        assert!(!partial_sink_path(&path, &id).exists());
        assert!(!cancel_stream(&id));
    }

    #[test]
    fn writes_stdout_to_file_and_reports_progress() {
        let _guard = exclusive();
        let path = temp_path("to-file");
        let mut reports = Vec::new();
        let outcome = run_node_fixture(
            &job("to-file"),
            "hello",
            OutputSink::ToFile(path.clone()),
            |b| reports.push(b),
        )
        .expect("command should succeed");

        assert_eq!(outcome.bytes, 11);
        assert!(!outcome.cancelled);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
        assert_eq!(reports.last(), Some(&11));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn yields_stdout_line_by_line() {
        let _guard = exclusive();
        let (tx, rx) = mpsc::sync_channel(64);
        let outcome = run_node_fixture(&job("by-line"), "lines", OutputSink::ByLine(tx), |_| {})
            .expect("command should succeed");

        let lines: Vec<String> = rx.iter().collect();
        assert_eq!(lines, vec!["a", "b", "c"]);
        assert_eq!(outcome.bytes, 6);
    }

    #[test]
    fn failed_command_reports_stderr_and_removes_partial_file() {
        let _guard = exclusive();
        let path = temp_path("failing");
        let err = run_node_fixture(
            &job("failing"),
            "failure",
            OutputSink::ToFile(path.clone()),
            |_| {},
        )
        .expect_err("non-zero exit must be an error");

        assert!(
            err.contains("went wrong"),
            "stderr should reach the caller: {}",
            err
        );
        // A truncated archive that looks complete is worse than none at all.
        assert!(!path.exists(), "partial output must not survive a failure");
    }

    #[test]
    fn cancel_kills_the_child_and_discards_partial_output() {
        let _guard = exclusive();
        let path = temp_path("cancel");
        let id = job("cancel");
        let id_for_thread = id.clone();

        // Cancel from another thread while the command is still running.
        let canceller = std::thread::spawn(move || {
            for _ in 0..100 {
                if cancel_stream(&id_for_thread) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        });

        let outcome = run_node_fixture(&id, "hold", OutputSink::ToFile(path.clone()), |_| {})
            .expect("cancellation is not a failure");

        assert!(
            canceller.join().unwrap(),
            "cancel_stream should have found the job"
        );
        assert!(outcome.cancelled);
        assert!(!path.exists(), "cancelled output must be cleaned up");
        assert!(
            !partial_sink_path(&path, &id).exists(),
            "the scratch file must go too"
        );
        // No orphan: the registry entry is gone and the child was reaped.
        assert!(!cancel_stream(&id), "job should no longer be registered");
    }

    /// Cancelling an overwrite must leave the file the user already had.
    ///
    /// Output used to be written straight to the destination, so `File::create`
    /// truncated it on the first byte and the abort path then deleted it. Cancelling
    /// an export over an existing archive therefore destroyed that archive — the
    /// user lost a file by changing their mind. Nothing touches the destination now
    /// until there is a complete artefact to rename onto it.
    #[test]
    fn cancelling_does_not_destroy_an_existing_destination() {
        let _guard = exclusive();
        let path = temp_path("cancel-keeps-existing");
        std::fs::write(&path, b"the file the user already had").unwrap();

        let id = job("cancel-keeps-existing");
        let id_for_thread = id.clone();
        let canceller = std::thread::spawn(move || {
            for _ in 0..100 {
                if cancel_stream(&id_for_thread) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        });

        let outcome =
            run_node_fixture(&id, "replacement", OutputSink::ToFile(path.clone()), |_| {})
                .expect("cancellation is not a failure");

        assert!(
            canceller.join().unwrap(),
            "cancel_stream should have found the job"
        );
        assert!(outcome.cancelled);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"the file the user already had",
            "cancelling must not touch the destination"
        );
        assert!(!partial_sink_path(&path, &id).exists());

        let _ = std::fs::remove_file(&path);
    }

    /// Two jobs aimed at one destination must not write into the same scratch file.
    ///
    /// The registry dedups on job id, and the destination does not exist while the
    /// first job is running — so nothing upstream refuses this. Keying the scratch
    /// file on the destination alone let both processes open it and interleave
    /// their bytes into a single corrupt archive.
    #[test]
    fn concurrent_jobs_to_one_destination_use_separate_scratch_files() {
        let path = temp_path("shared-destination");
        let a = partial_sink_path(&path, "save-1");
        let b = partial_sink_path(&path, "save-2");

        assert_ne!(a, b, "two jobs must not share a scratch file");
        assert_eq!(a.parent(), path.parent(), "scratch must stay a sibling");
    }

    /// A job id cannot place the scratch file outside the destination's folder.
    #[test]
    fn a_separator_in_a_job_id_cannot_escape_the_folder() {
        let path = temp_path("escape-attempt");
        let scratch = partial_sink_path(&path, "../../etc/passwd");

        assert_eq!(scratch.parent(), path.parent());
        assert!(!scratch.to_string_lossy().contains(".."));
    }

    /// A failing command must leave an existing destination alone as well.
    #[test]
    fn a_failed_command_does_not_destroy_an_existing_destination() {
        let _guard = exclusive();
        let path = temp_path("fail-keeps-existing");
        std::fs::write(&path, b"previous archive").unwrap();

        let id = job("fail-keeps-existing");
        let err = run_node_fixture(&id, "failure", OutputSink::ToFile(path.clone()), |_| {})
            .expect_err("a non-zero exit is a failure");
        assert!(!err.is_empty());

        assert_eq!(std::fs::read(&path).unwrap(), b"previous archive");
        assert!(!partial_sink_path(&path, &id).exists());

        let _ = std::fs::remove_file(&path);
    }

    /// Cancelling must return promptly even when the command forked a helper.
    ///
    /// The controlled fixture forks a helper that inherits its stderr pipe.
    /// Signalling only the direct child left the
    /// grandchild alive holding that pipe, and reading stderr to EOF then blocked
    /// for the grandchild's full 30 seconds — cancellation appeared to hang.
    #[test]
    fn cancel_returns_promptly_even_when_the_command_forked_a_child() {
        let _guard = exclusive();
        let path = temp_path("cancel-fast");
        let id = job("cancel-fast");
        let id_for_thread = id.clone();

        let canceller = std::thread::spawn(move || {
            for _ in 0..100 {
                if cancel_stream(&id_for_thread) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        });

        let started = Instant::now();
        let outcome = run_node_fixture(&id, "fork", OutputSink::ToFile(path.clone()), |_| {})
            .expect("cancellation is not a failure");
        let elapsed = started.elapsed();

        assert!(canceller.join().unwrap());
        assert!(outcome.cancelled);
        assert!(
            elapsed < Duration::from_secs(5),
            "cancel took {:?}; it is waiting on a process it does not own",
            elapsed
        );
        assert!(!path.exists());
    }

    /// The grandchild itself must die, not merely stop blocking us.
    #[test]
    fn cancel_leaves_no_orphan_process() {
        let _guard = exclusive();
        let path = temp_path("orphan");
        let id = job("orphan");
        let id_for_thread = id.clone();
        let marker = temp_path("orphan-child.pid");
        let marker_inner = marker.clone();

        let canceller = std::thread::spawn(move || {
            for _ in 0..100 {
                if let Ok(pid) = std::fs::read_to_string(&marker_inner) {
                    let pid = pid.parse().unwrap();
                    assert!(
                        process_running(pid),
                        "fixture descendant must exist before cancellation"
                    );
                    if cancel_stream(&id_for_thread) {
                        return true;
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        });

        let mut command = node_fixture("fork");
        command.arg(&marker);
        let outcome = run_streaming(
            &id,
            command,
            "node",
            OutputSink::ToFile(path.clone()),
            |_| {},
        )
        .expect("cancellation is not a failure");
        assert!(
            canceller.join().unwrap(),
            "descendant readiness/cancellation was never observed"
        );
        assert!(outcome.cancelled);

        let pid = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while process_running(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_running(pid),
            "owned descendant survived cancellation"
        );
        std::fs::remove_file(marker).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refuses_a_duplicate_job_id() {
        let _guard = exclusive();
        let id = job("dup");
        let path = temp_path("dup");
        let id_inner = id.clone();
        let path_inner = temp_path("dup-inner");

        // Hold one job open, then try to start a second under the same id.
        let blocker = std::thread::spawn(move || {
            run_node_fixture(&id_inner, "hold", OutputSink::ToFile(path_inner), |_| {})
        });

        let mut err = None;
        for _ in 0..100 {
            if active_stream_count() > 0 {
                err = run_node_fixture(&id, "byte", OutputSink::ToFile(path.clone()), |_| {}).err();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        cancel_stream(&id);
        let _ = blocker.join();

        let err = err.expect("second start with the same id must fail");
        assert!(err.contains("already running"), "unexpected error: {}", err);
        // The refusal must not clobber the running job's destination.
        assert!(!path.exists());
    }

    #[test]
    fn kill_all_streams_reaps_everything() {
        let _guard = exclusive();
        let id = job("kill-all");
        let id_inner = id.clone();
        let path = temp_path("kill-all");

        let runner = std::thread::spawn(move || {
            run_node_fixture(&id_inner, "hold", OutputSink::ToFile(path), |_| {})
        });

        let mut killed = 0;
        for _ in 0..100 {
            if active_stream_count() > 0 {
                killed = kill_all_streams();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let outcome = runner.join().unwrap().expect("kill is a cancellation");
        assert!(killed >= 1);
        assert!(outcome.cancelled);
        assert!(!cancel_stream(&id));
    }

    #[test]
    fn missing_program_reports_the_same_hint_as_run_cmd() {
        let _guard = exclusive();
        let err = run_cmd_streaming(
            &job("missing"),
            "colimaui-no-such-binary",
            &[],
            OutputSink::ToFile(temp_path("missing")),
            |_| {},
        )
        .expect_err("a missing binary must fail");

        assert_eq!(
            err,
            crate::helpers::describe_exec_error(
                "colimaui-no-such-binary",
                &std::io::Error::from(std::io::ErrorKind::NotFound)
            )
        );
        // The destination file we opened before spawning must not be left behind.
        assert!(!temp_path("missing").exists());
    }

    /// The reason this module exists: resident memory must not track the size of
    /// the payload. `Command::output()` would hold all 256 MB at once.
    #[test]
    fn resident_memory_does_not_track_payload_size() {
        let _guard = exclusive();
        let path = temp_path("rss");
        let before = resident_kb().expect("cannot read RSS on this platform");

        let outcome = run_node_fixture(
            &job("rss"),
            "large",
            OutputSink::ToFile(path.clone()),
            |_| {},
        )
        .expect("command should succeed");

        let after = resident_kb().expect("cannot read RSS on this platform");
        let _ = std::fs::remove_file(&path);

        assert_eq!(outcome.bytes, 256 * 1024 * 1024);
        let growth_kb = after.saturating_sub(before);
        assert!(
            growth_kb < 32 * 1024,
            "RSS grew {} KB while streaming 256 MB — output is being buffered",
            growth_kb
        );
    }

    #[cfg(unix)]
    fn resident_kb() -> Option<u64> {
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    #[cfg(windows)]
    fn resident_kb() -> Option<u64> {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of_val(&counters) as u32;
        let ok = unsafe {
            GetProcessMemoryInfo(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                &mut counters,
                std::mem::size_of_val(&counters) as u32,
            )
        };
        (ok != 0).then_some(counters.WorkingSetSize as u64 / 1024)
    }

    #[cfg(unix)]
    fn process_running(pid: u32) -> bool {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("POSIX ps required");
        output.status.success()
            && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
            && !String::from_utf8_lossy(&output.stdout)
                .trim()
                .starts_with('Z')
    }

    #[cfg(windows)]
    fn process_running(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{
            CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
        };
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            assert_eq!(
                unsafe { GetLastError() },
                ERROR_INVALID_PARAMETER,
                "cannot query owned descendant"
            );
            return false;
        }
        let mut code = 0;
        let ok = unsafe { GetExitCodeProcess(process, &mut code) };
        unsafe {
            CloseHandle(process);
        }
        assert_ne!(ok, 0, "cannot read owned descendant status");
        code == STILL_ACTIVE as u32
    }
}
