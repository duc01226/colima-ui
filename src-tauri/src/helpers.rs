//! Shared helper utilities for the API server.
//!
//! Generic response wrappers, blocking task runner, CLI command runner,
//! timed caching, and global state (port forwards, system info cache).

use axum::{http::StatusCode, response::Json};
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use crate::commands::system;

// ===== API Response Wrappers =====

/// Generic API response wrapper.
///
/// `error` carries the same structured payload the Tauri IPC path returns, so
/// browser mode and desktop mode see an identical error contract.
#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<crate::error::ColimaError>,
}

pub fn ok<T: Serialize>(data: T) -> (StatusCode, Json<ApiResponse<T>>) {
    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(data),
            error: None,
        }),
    )
}

/// Build an error response.
///
/// Takes anything convertible into [`crate::error::ColimaError`], which is what
/// lets the ~115 existing `err(e.to_string())` call sites keep working while
/// gaining classification — `From<String>` classifies the message.
pub fn err<T: Serialize>(
    e: impl Into<crate::error::ColimaError>,
) -> (StatusCode, Json<ApiResponse<T>>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.into()),
        }),
    )
}

/// Build a [`crate::error::ColimaError`] from a finished process, attaching the
/// command and its exit status.
///
/// Prefer this over `format!("... {}", stderr)` at any site that has the
/// `Output` in hand: the classification is the same, but the user also learns
/// which command failed and how.
pub fn error_from_output(
    command: &str,
    output: &std::process::Output,
) -> crate::error::ColimaError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = if stderr.trim().is_empty() {
        format!("`{}` failed with no error output", command)
    } else {
        stderr.trim().to_string()
    };

    let mut e = crate::error::ColimaError::from(detail).with_command(command);
    if let Some(code) = output.status.code() {
        e = e.with_exit_code(code);
    }
    // A process that ran and failed is a command failure unless the message
    // says something more specific.
    if e.code == crate::error::ErrorCode::Unknown {
        e.code = crate::error::ErrorCode::CommandFailed;
    }
    e
}

// ===== Blocking Task Runner =====

/// Run a blocking closure on the thread pool to avoid starving the tokio reactor.
/// All colima/docker/system commands use std::process::Command::output() which blocks.
pub async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

// ===== CLI Command Runner =====

/// Build a `Command` with the environment every CLI path in this app needs.
///
/// Shared by `run_cmd` and `streaming_cmd::run_cmd_streaming` on purpose: two
/// independently-built command paths drift, and a `docker` invocation that
/// silently loses `DOCKER_HOST` talks to the wrong daemon.
pub fn build_cmd(program: &str, args: &[&str]) -> Result<Command, String> {
    crate::host_features::require_tool(program)?;
    #[cfg(windows)]
    {
        let name = std::path::Path::new(program)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");
        if name.eq_ignore_ascii_case("docker")
            || name.eq_ignore_ascii_case("docker.exe")
            || name.eq_ignore_ascii_case("docker-compose")
            || name.eq_ignore_ascii_case("docker-compose.exe")
        {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            crate::engine_connection::validate_argv(&args)?;
            let mut command = crate::commands::runtime::get_runtime_cmd()?;
            if name.to_ascii_lowercase().starts_with("docker-compose") {
                command.arg("compose");
            }
            command.args(args);
            return Ok(command);
        }
    }
    let mut cmd = Command::new(program);
    cmd.args(args);

    // Auto-set DOCKER_HOST for docker/docker-compose/kind commands
    if program == "docker" || program == "docker-compose" || program == "kind" {
        if let Some((host, _)) = crate::path_util::detect_docker_host() {
            cmd.env("DOCKER_HOST", host);
        }
    }

    crate::path_util::apply_path_to_cmd(&mut cmd);
    Ok(cmd)
}

/// Turn a spawn/exec failure into the message the user sees.
///
/// Shared with the streaming path so "not installed" reads identically wherever
/// the command was launched from.
pub fn describe_exec_error(program: &str, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        #[cfg(windows)]
        return format!("{program} is unavailable. Check the native executable and the selected Docker Desktop or Rancher Moby engine.");
        #[cfg(not(windows))]
        let hint = match program {
            "kind" => "Install with: brew install kind",
            "kubectl" => "Install with: brew install kubectl",
            "helm" => "Install with: brew install helm",
            "limactl" => "Install with: brew install lima",
            "colima" => "Install with: brew install colima",
            "docker" => "Install with: brew install docker",
            _ => "Please install it and try again",
        };
        #[cfg(not(windows))]
        {
            format!("'{}' is not installed. {}", program, hint)
        }
    } else {
        format!("Failed to execute {}: {}", program, e)
    }
}

pub fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let cmd = build_cmd(program, args)?;

    let output =
        crate::child_process::capture(cmd, std::time::Duration::from_secs(30), 16 * 1024 * 1024)
            .map_err(|e| describe_exec_error(program, &e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} failed: {}", program, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ===== Timed Cache =====

/// Fix #18: Timed system info cache — refreshes every 5 minutes.
/// Replaces the OnceLock (which never invalidated, showing stale versions).
pub struct TimedCache<T> {
    pub data: Option<T>,
    pub cached_at: std::time::Instant,
    pub ttl: std::time::Duration,
}

impl<T: Clone> TimedCache<T> {
    pub fn new(ttl: std::time::Duration) -> Self {
        Self {
            data: None,
            cached_at: std::time::Instant::now(),
            ttl,
        }
    }

    pub fn get_or_init(&mut self, f: impl FnOnce() -> T) -> T {
        if let Some(ref data) = self.data {
            if self.cached_at.elapsed() < self.ttl {
                return data.clone();
            }
        }
        let val = f();
        self.data = Some(val.clone());
        self.cached_at = std::time::Instant::now();
        val
    }
}

pub static SYSTEM_INFO_CACHE: std::sync::LazyLock<Mutex<TimedCache<system::SystemInfo>>> =
    std::sync::LazyLock::new(|| Mutex::new(TimedCache::new(std::time::Duration::from_secs(300))));

pub fn load_system_info() -> system::SystemInfo {
    #[cfg(windows)]
    {
        return system::system_info();
    }
    // Preserve the established POSIX HTTP version formatting and probe path.
    #[cfg(not(windows))]
    {
        let colima_version = run_cmd("colima", &["version"]).unwrap_or_default();
        let docker_version = run_cmd("docker", &["--version"]).unwrap_or_default();
        let lima_version = run_cmd("limactl", &["--version"]).unwrap_or_default();

        system::SystemInfo {
            colima_installed: !colima_version.is_empty(),
            colima_version: colima_version.lines().next().unwrap_or("").to_string(),
            docker_installed: !docker_version.is_empty(),
            docker_version: docker_version.trim().to_string(),
            lima_installed: !lima_version.is_empty(),
            lima_version: lima_version.trim().to_string(),
        }
    }
}

// ===== Global State =====

/// Port forward process tracking (pid by port key)
pub static PORT_FORWARDS: std::sync::LazyLock<Mutex<HashMap<String, u32>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
mod tests {
    use super::*;

    fn run_fixture(mode: &str) -> Result<String, String> {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/process.mjs");
        run_cmd("node", &[fixture.to_str().expect("fixture path"), mode])
    }

    /// `run_cmd`'s body was extracted into `build_cmd` + `describe_exec_error` so
    /// the streaming path could share it. These pin the observable behaviour that
    /// extraction had to preserve.
    #[test]
    fn run_cmd_returns_stdout() {
        let out = run_fixture("hello").expect("should succeed");
        assert_eq!(out, "hello world");
    }

    #[test]
    fn run_cmd_reports_a_failing_command_with_its_stderr() {
        let err = run_fixture("failure").expect_err("non-zero exit must be an error");
        assert!(err.contains("went wrong"), "unexpected error: {}", err);
    }

    #[test]
    fn run_cmd_hints_at_installation_for_a_missing_binary() {
        let err = run_cmd("colimaui-no-such-binary", &[]).expect_err("should fail");
        assert!(
            err.contains(if cfg!(windows) {
                "is unavailable"
            } else {
                "is not installed"
            }),
            "unexpected error: {}",
            err
        );
    }

    // TestSpec=TC-WINUI-002: partial prerequisite/error projection only.
    #[test]
    fn missing_tool_remediation_matches_the_host_and_other_errors_keep_their_cause() {
        let missing = describe_exec_error(
            "docker",
            &std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        if cfg!(windows) {
            assert!(missing.contains("Docker Desktop") && missing.contains("Rancher Moby"));
            assert!(!missing.contains("brew"));
        } else {
            assert!(missing.contains("brew install docker"));
        }
        let denied = describe_exec_error(
            "node",
            &std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture permission"),
        );
        assert!(denied.contains("Failed to execute node") && denied.contains("fixture permission"));
        assert!(!denied.contains("not installed") && !denied.contains("is unavailable"));
    }

    // TestSpec=TC-WINUI-002: actual native HTTP-handler/IPC prerequisite
    // projections with a usable CLI but no usable engine. No real Docker or
    // account activity database is acquired by this child fixture.
    #[cfg(windows)]
    #[test]
    fn native_prerequisite_projections_do_not_require_an_engine() {
        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/tooling-tests")
            .join(format!(
                "native-metadata-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::fs::create_dir(&root).unwrap();
        let scratch = Scratch(root.canonicalize().unwrap());
        let node = crate::path_util::resolve_executable("node").expect("pinned Node prerequisite");
        // Node's --version is a real successful local CLI probe; its context,
        // info and daemon commands are unusable. The alias can never reach an
        // operator's engine, including when the protected regression is seeded.
        std::fs::copy(node, scratch.0.join("docker.exe")).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args([
            "--exact",
            "helpers::tests::native_metadata_fixture",
            "--ignored",
            "--nocapture",
        ]);
        child
            .env("PATH", &scratch.0)
            .env("DOCKER_CONFIG", &scratch.0)
            .env("COLIMAUITEST_METADATA", "owned-cli-only")
            .env_remove("NODE_OPTIONS");
        let output =
            crate::child_process::capture(child, std::time::Duration::from_secs(20), 131_072)
                .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("METADATA-PROJECTIONS-OK"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "owned CLI-only subprocess selected by its nonignored parent"]
    fn native_metadata_fixture() {
        assert_eq!(
            std::env::var("COLIMAUITEST_METADATA").as_deref(),
            Ok("owned-cli-only")
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let ipc = runtime.block_on(system::check_system()).unwrap();
        assert!(ipc.docker_installed && !ipc.docker_version.is_empty());
        let (status, Json(http)) = runtime.block_on(crate::routes::system::api_check_system());
        assert_eq!(status, StatusCode::OK);
        assert!(http.success && http.error.is_none());
        let http = http.data.unwrap();
        assert_eq!(
            serde_json::to_value(&ipc).unwrap(),
            serde_json::to_value(&http).unwrap()
        );
        assert!(http.docker_installed);
        println!("METADATA-PROJECTIONS-OK");
    }

    #[cfg(not(windows))]
    #[test]
    fn docker_host_is_set_only_for_container_tooling() {
        // The env decision lives in `build_cmd` now; both callers depend on it
        // being made the same way.
        let docker = build_cmd("docker", &["version"]).unwrap();
        let other = build_cmd("sh", &["-c", "true"]).unwrap();
        let has_docker_host = |cmd: &Command| {
            cmd.get_envs()
                .any(|(k, v)| k == "DOCKER_HOST" && v.is_some())
        };
        // Only assert the negative unconditionally: whether DOCKER_HOST gets set
        // for docker depends on a socket existing on the machine running the test.
        assert!(!has_docker_host(&other));
        if crate::path_util::detect_docker_host().is_some() {
            assert!(has_docker_host(&docker));
        }
    }
}
