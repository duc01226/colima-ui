use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::PathBuf;
use std::process::Command;

/// Common binary search paths on macOS, Linux, and Windows
#[cfg(not(windows))]
const EXTRA_PATHS: &[&str] = &[
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/sbin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
    // Nix paths
    "/nix/var/nix/profiles/default/bin",
    "/run/current-system/sw/bin",
    // Linuxbrew
    "/home/linuxbrew/.linuxbrew/bin",
];

/// Nix user-level paths that need $HOME expansion
#[cfg(not(windows))]
const NIX_USER_PATHS: &[&str] = &[
    ".nix-profile/bin",
    ".local/state/nix/profiles/profile/bin",
    ".nix-defexpr/channels/bin",
];

/// Version manager / user-local paths that need $HOME expansion
#[cfg(not(windows))]
const USER_PATHS: &[&str] = &[
    // asdf version manager (manages node, kubectl, helm, etc.)
    ".asdf/shims",
    ".asdf/bin",
    // Cargo (Rust)
    ".cargo/bin",
    // Go
    "go/bin",
    // Local bin
    ".local/bin",
    // Rancher Desktop
    ".rd/bin",
    // Krew (kubectl plugin manager)
    ".krew/bin",
];

/// The computed PATH, stored for use by `apply_path_to_cmd()` in late-spawned contexts.
static COMPUTED_PATH: std::sync::OnceLock<OsString> = std::sync::OnceLock::new();

/// `~/.colima-ui` — where this app keeps everything it writes.
///
/// Extracted when a third caller appeared (crash reports). Two ad-hoc copies of
/// a path is a coincidence; three is a convention that should live in one place,
/// so a future move does not leave one directory behind.
///
/// An empty `HOME` counts as missing, not as a valid prefix. `env::var` returns
/// `Ok("")` for a variable that is set but blank, and
/// `PathBuf::from("").join(".colima-ui")` is the *relative* path `.colima-ui` —
/// which would put the knowledge bank and the crash reports in whatever
/// directory the app happened to be launched from, silently, a different one
/// each time.
///
/// The `/tmp` fallback matches what the two original callers did. A process with
/// no `HOME` is not a normal desktop session, and losing the knowledge bank
/// there is better than refusing to start.
pub fn app_data_dir() -> PathBuf {
    try_app_data_dir().expect("Cannot resolve ColimaUI user data directory")
}

/// Windows uses the native per-user Known Folder. Startup checks this before
/// opening storage. POSIX retains its existing HOME/.colima-ui location.
pub fn try_app_data_dir() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        crate::windows_host::app_data_dir()
    }
    #[cfg(not(windows))]
    {
        Ok(posix_app_data_dir(env::var_os("HOME").as_deref()))
    }
}

#[cfg(not(windows))]
fn posix_app_data_dir(home: Option<&OsStr>) -> PathBuf {
    PathBuf::from(home.filter(|h| !h.is_empty()).unwrap_or(OsStr::new("/tmp"))).join(".colima-ui")
}

/// Ensure common binary paths are in the PATH environment variable.
///
/// Call this once at app startup **before any threads are spawned** to fix
/// the PATH for all subsequent Command calls.
///
/// # Safety
/// This function calls `env::set_var` which is unsound in multi-threaded contexts.
/// It MUST be called from `main()` before `tauri::Builder::default()` (which spawns
/// the tokio runtime and other threads).
pub fn fix_path_env() {
    let current_path = env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = env::split_paths(&current_path).collect();

    #[cfg(not(windows))]
    {
        for extra in EXTRA_PATHS {
            append_existing_path(&mut paths, PathBuf::from(extra));
        }

        // Add Nix user-level paths (need $HOME expansion)
        if let Some(home) = env::var_os("HOME").filter(|h| !h.is_empty()) {
            for rel in NIX_USER_PATHS {
                append_existing_path(&mut paths, PathBuf::from(&home).join(rel));
            }

            // Add version-manager and user-local paths (asdf, cargo, go, etc.)
            for rel in USER_PATHS {
                append_existing_path(&mut paths, PathBuf::from(&home).join(rel));
            }
        }

        // Try to load user's shell PATH via login shell
        // Use zsh (macOS default) first, then fall back to sh
        let shell_cmds = [
            ("/bin/zsh", &["-l", "-c", "echo $PATH"][..]),
            ("/bin/sh", &["-l", "-c", "echo $PATH"][..]),
        ];
        for (shell, args) in &shell_cmds {
            if let Ok(output) = Command::new(shell).args(*args).output() {
                if output.status.success() {
                    let shell_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    for p in env::split_paths(&shell_path) {
                        if !p.as_os_str().is_empty() && !paths.contains(&p) {
                            paths.push(p);
                        }
                    }
                    break; // Use the first successful shell
                }
            }
        }
    }
    #[cfg(windows)]
    if let Ok(profile) = crate::windows_host::profile_dir() {
        append_existing_path(&mut paths, profile.join(".rd").join("bin"));
    }

    // These entries originated in PATH or existing directories, so each is a
    // native path component. Do not silently publish a truncated PATH.
    let new_path = env::join_paths(paths).expect("Invalid native PATH component");

    // Store for per-Command application in late-spawned threads
    let _ = COMPUTED_PATH.set(new_path.clone());

    // Fix #17: Guard against accidental use from multi-threaded context.
    // This function MUST be called exactly once from main(), before any threads exist.
    #[cfg(debug_assertions)]
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);
        assert!(
            !CALLED.swap(true, Ordering::SeqCst),
            "fix_path_env() must only be called once, from main() before the async runtime starts"
        );
    }

    // SAFETY: Called from main() before any threads are spawned (before tauri::Builder).
    // No concurrent readers/writers of the environment exist at this point.
    unsafe { env::set_var("PATH", &new_path) };
}

fn append_existing_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.is_dir() && !paths.contains(&candidate) {
        paths.push(candidate);
    }
}

/// Apply the computed PATH to a Command. Use this in contexts where the global
/// env may not have been inherited (e.g., spawned after fix_path_env).
#[allow(dead_code)]
pub fn apply_path_to_cmd(cmd: &mut Command) {
    if let Some(path) = COMPUTED_PATH.get() {
        cmd.env("PATH", path);
    }
}

/// Resolve the full path for a binary by checking PATH
#[allow(dead_code)]
pub fn resolve_binary(name: &str) -> String {
    #[cfg(windows)]
    {
        resolve_executable(name)
            .map_or_else(|| name.to_string(), |p| p.to_string_lossy().into_owned())
    }
    #[cfg(not(windows))]
    {
        // First check if the binary can be found via `which`
        if let Ok(output) = Command::new("/usr/bin/which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return path;
                }
            }
        }

        // Fall back to common locations
        for dir in EXTRA_PATHS {
            let candidate = PathBuf::from(dir).join(name);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }

        // Last resort: just use the name and hope PATH has it
        name.to_string()
    }
}

/// Shared by the desktop and standalone Windows CLI without mutating a running
/// async process's environment. Rancher's user bin is an actual second search
/// source, including when the CLI was launched outside the desktop bootstrap.
#[cfg(windows)]
pub fn resolve_executable(name: &str) -> Option<PathBuf> {
    let path = COMPUTED_PATH
        .get()
        .cloned()
        .or_else(|| env::var_os("PATH"))
        .unwrap_or_default();
    let mut paths: Vec<PathBuf> = env::split_paths(&path).collect();
    if let Ok(profile) = crate::windows_host::profile_dir() {
        append_existing_path(&mut paths, profile.join(".rd").join("bin"));
    }
    find_executable(name, Some(&env::join_paths(paths).ok()?))
}

/// Resolve a native executable without a shell, PATHEXT scripts, or a current
/// directory search. The connection owner retains this PathBuf losslessly.
#[cfg(windows)]
pub fn find_executable(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let requested = PathBuf::from(name);
    let file = if requested.extension().is_some() {
        requested
    } else {
        requested.with_extension("exe")
    };
    if !file
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return None;
    }
    if file.is_absolute() {
        return file.is_file().then(|| file.canonicalize().ok()).flatten();
    }
    if file.components().count() != 1 {
        return None;
    }
    env::split_paths(path?)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(&file))
        .find(|p| p.is_file())?
        .canonicalize()
        .ok()
}

/// Detect the Docker socket from a running Colima instance.
/// Returns something like "unix:///Users/mike/.colima/default/docker.sock".
///
/// This is critical for macOS .app bundles because:
/// 1. DOCKER_HOST env var is NOT inherited from the user's shell
/// 2. Colima doesn't create /var/run/docker.sock (no root access)
/// 3. Bollard's connect_with_defaults() only checks /var/run/docker.sock
#[cfg(windows)]
pub fn detect_docker_host() -> Option<(String, String)> {
    let connection = crate::engine_connection::active().ok()?;
    Some((
        connection.target().endpoint().to_string(),
        connection.target().context().to_string(),
    ))
}

#[cfg(not(windows))]
pub fn detect_docker_host() -> Option<(String, String)> {
    if let Ok(host) = std::env::var("DOCKER_HOST") {
        if !host.is_empty() {
            return Some((host, "default".to_string()));
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let colima_home = std::env::var("COLIMA_HOME").unwrap_or_else(|_| format!("{}/.colima", home));
    let colima_path = std::path::Path::new(&colima_home);

    if !colima_path.exists() {
        return None;
    }

    // Scan profiles for a running instance (has ha.sock in _lima dir)
    if let Ok(entries) = std::fs::read_dir(colima_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('_') || name.starts_with('.') || !entry.path().is_dir() {
                continue;
            }
            // Map profile to lima instance name
            let lima_name = if name == "default" {
                "colima".to_string()
            } else {
                format!("colima-{}", name)
            };
            let lima_dir = colima_path.join("_lima").join(&lima_name);

            if lima_dir.join("ha.sock").exists() || lima_dir.join("ha.pid").exists() {
                // Found running instance — return its docker socket
                let sock = colima_path.join(&name).join("docker.sock");
                if sock.exists() {
                    return Some((format!("unix://{}", sock.display()), name.clone()));
                }
            }
        }
    }

    // Fallback: the colima-level docker.sock symlink, which Colima repoints at
    // whichever profile started last. Reached when the scan above found nothing
    // running.
    let fallback = colima_path.join("docker.sock");
    if fallback.exists() {
        return Some((
            format!("unix://{}", fallback.display()),
            "default".to_string(),
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // TC-WINUI-001/020: native separators preserve names, and supported POSIX
    // data must not move when another OS is added.
    #[test]
    fn native_path_round_trip_preserves_spaces_unicode_and_order() {
        let paths = [PathBuf::from("tools with spaces"), PathBuf::from("工具")];
        let joined = env::join_paths(&paths).unwrap();
        assert_eq!(env::split_paths(&joined).collect::<Vec<_>>(), paths);
    }

    #[cfg(not(windows))]
    #[test]
    fn missing_or_empty_home_retains_posix_data_location() {
        assert_eq!(posix_app_data_dir(None), PathBuf::from("/tmp/.colima-ui"));
        assert_eq!(
            posix_app_data_dir(Some(OsStr::new(""))),
            PathBuf::from("/tmp/.colima-ui")
        );
        assert_eq!(
            posix_app_data_dir(Some(OsStr::new("/Users/工具 space"))),
            PathBuf::from("/Users/工具 space/.colima-ui")
        );
    }

    #[cfg(windows)]
    #[test]
    fn only_absolute_search_directories_and_exe_files_can_resolve() {
        let root = env::temp_dir().join(format!("colimaui-path-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("native.exe"), b"fixture").unwrap();
        std::fs::write(root.join("shell.cmd"), b"fixture").unwrap();
        let path = env::join_paths([&root]).unwrap();
        assert_eq!(
            find_executable("native", Some(&path)),
            Some(root.join("native.exe").canonicalize().unwrap())
        );
        assert!(find_executable("shell.cmd", Some(&path)).is_none());
        assert!(find_executable("..\\native", Some(&path)).is_none());
        assert!(find_executable("native", Some(OsStr::new("."))).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
