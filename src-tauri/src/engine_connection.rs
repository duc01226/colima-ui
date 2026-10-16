//! One Windows engine origin for a process lifetime. Endpoint/config/executable
//! are captured once; preferences are only the next launch's input. No global
//! Docker context is modified, and an address never substitutes for identity.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

pub const CONTEXT_SETTING: &str = "docker.context";
const CONFLICTING_ENV: &[&str] = &[
    "DOCKER_CONTEXT",
    "DOCKER_HOST",
    "DOCKER_TLS",
    "DOCKER_TLS_VERIFY",
    "DOCKER_CERT_PATH",
    "DOCKER_API_VERSION",
];

#[derive(Clone)]
pub struct EngineTarget {
    context: String,
    endpoint: String,
    executable: PathBuf,
    config: PathBuf,
}

impl EngineTarget {
    pub fn new(
        context: String,
        endpoint: String,
        executable: PathBuf,
        config: PathBuf,
    ) -> Result<Self, String> {
        validate_context_name(&context)?;
        let endpoint = local_pipe(&endpoint)?;
        if !executable.is_absolute() || !config.is_absolute() {
            return Err("Docker paths must be absolute".into());
        }
        Ok(Self {
            context,
            endpoint,
            executable,
            config,
        })
    }
    pub fn context(&self) -> &str {
        &self.context
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        self.apply_environment(&mut command);
        command.args(["--host", &self.endpoint]);
        command
    }
    pub fn apply_environment(&self, command: &mut Command) {
        for name in CONFLICTING_ENV {
            command.env_remove(name);
        }
        command
            .env("DOCKER_CONFIG", &self.config)
            .env("DOCKER_HOST", &self.endpoint);
        crate::path_util::apply_path_to_cmd(command);
    }
}

pub fn validate_context_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || "._-".contains(c))
    {
        return Err("Invalid Docker context name".into());
    }
    Ok(())
}

fn local_pipe(endpoint: &str) -> Result<String, String> {
    let normalized = endpoint.replace('\\', "/");
    let name = normalized.strip_prefix("npipe:////./pipe/")
        .ok_or_else(|| "Select a local Docker Desktop or Rancher Moby named-pipe context; remote and WSL transports are unavailable".to_string())?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("Unsupported local Docker pipe".into());
    }
    Ok(normalized)
}

/// Only Docker's global selector region is inspected. After the subcommand,
/// `--host` can be legitimate container/shell data rather than an engine flag.
pub fn validate_argv(args: &[String]) -> Result<(), String> {
    for arg in args {
        if arg == "--" || !arg.starts_with('-') {
            break;
        }
        // Reject unknown/value-taking global flags as well: treating their
        // value as a subcommand could hide a later --host or --context override.
        if !matches!(arg.as_str(), "--debug" | "-D" | "--help" | "--version") {
            return Err("Docker target overrides are unavailable; change the next-launch connection in Settings".into());
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct VerifiedIdentity {
    original: Option<String>,
    ready: bool,
    error: Option<String>,
    issued_check: u64,
    applied_check: u64,
    generation: u64,
}

impl VerifiedIdentity {
    pub fn begin_check(&mut self) -> u64 {
        self.issued_check = self.issued_check.saturating_add(1);
        self.issued_check
    }
    pub fn observe(&mut self, id: Option<&str>, os: Option<&str>) -> Result<(), String> {
        let check = self.begin_check();
        self.observe_at(check, id, os)
    }

    pub fn observe_at(
        &mut self,
        check: u64,
        id: Option<&str>,
        os: Option<&str>,
    ) -> Result<(), String> {
        if check < self.applied_check {
            return Err("Engine check was superseded; recheck".into());
        }
        self.applied_check = check;
        let result = match (id.filter(|id| !id.is_empty()), os) {
            (Some(id), Some("linux")) if self.original.as_deref().is_none_or(|old| old == id) => {
                self.original.get_or_insert_with(|| id.to_string());
                Ok(())
            }
            (_, Some(os)) if os != "linux" => {
                Err("Windows containers and non-Linux engines are unavailable".into())
            }
            (Some(_), Some("linux")) => {
                Err("The engine identity changed. Restart before operating this engine".into())
            }
            _ => Err("The selected engine identity could not be verified".into()),
        };
        if self.ready != result.is_ok() {
            self.generation = self.generation.saturating_add(1);
        }
        self.ready = result.is_ok();
        self.error = result.clone().err();
        result
    }
    pub fn lost(&mut self, reason: String) {
        let check = self.begin_check();
        self.lost_at(check, reason);
    }
    pub fn lost_at(&mut self, check: u64, reason: String) {
        if check < self.applied_check {
            return;
        }
        self.applied_check = check;
        if self.ready {
            self.generation = self.generation.saturating_add(1);
        }
        self.ready = false;
        self.error = Some(reason);
    }
    pub fn original(&self) -> Option<&str> {
        self.original.as_deref()
    }
    pub fn read_generation(&self) -> Result<u64, String> {
        if self.ready {
            Ok(self.generation)
        } else {
            Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Selected engine unavailable".into()))
        }
    }
    pub fn ensure_generation(&self, generation: u64) -> Result<(), String> {
        if self.read_generation()? == generation {
            Ok(())
        } else {
            Err("Engine availability changed; recheck the selected engine".into())
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EngineStatus {
    pub generation: Option<u64>,
    pub context: Option<String>,
    pub endpoint: Option<String>,
    pub engine_id: Option<String>,
    pub ready: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EngineContextChoice {
    pub name: String,
    pub supported: bool,
    pub reason: Option<String>,
}

#[cfg(windows)]
mod native {
    use super::*;
    use bollard::Docker;
    use rusqlite::{Connection, OpenFlags};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
    const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
    const PROBE_LIMIT: usize = 1_048_576;

    struct Session {
        context: Option<String>,
        preference_error: Option<String>,
        connection: Option<Arc<EngineConnection>>,
        error: Option<String>,
        retry_after: Instant,
    }

    pub struct EngineConnection {
        target: EngineTarget,
        client: Docker,
        identity: Mutex<VerifiedIdentity>,
    }

    fn session() -> &'static Mutex<Session> {
        SESSION.get_or_init(|| {
            let preference = read_preference();
            Mutex::new(Session {
                context: preference.as_ref().ok().cloned().flatten(),
                preference_error: preference.err(),
                connection: None,
                error: None,
                retry_after: Instant::now(),
            })
        })
    }

    fn read_preference() -> Result<Option<String>, String> {
        let path = crate::path_util::try_app_data_dir()
            .map_err(|_| "User data location unavailable")?
            .join("knowledge.db");
        match std::fs::metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Saved Docker context could not be read".into()),
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| "Saved Docker context could not be read")?;
        crate::app_settings::read(&conn, CONTEXT_SETTING)
            .map_err(|_| "Saved Docker context could not be read".into())
    }

    fn cli() -> Result<(PathBuf, PathBuf), String> {
        let executable = crate::path_util::resolve_executable("docker")
            .ok_or_else(|| "Docker CLI is not installed or is missing from PATH. Install Docker Desktop or Rancher Desktop Moby, then recheck".to_string())?;
        let config = match std::env::var_os("DOCKER_CONFIG").filter(|v| !v.is_empty()) {
            Some(path) => PathBuf::from(path),
            None => crate::windows_host::profile_dir()
                .map_err(|_| "Docker user directory unavailable")?
                .join(".docker"),
        };
        if !config.is_absolute() {
            return Err("Docker configuration directory must be absolute".into());
        }
        // Existing paths are canonical before they become a session identity;
        // an absent default directory is valid for an unconfigured Docker CLI.
        let config = if config.exists() {
            config
                .canonicalize()
                .map_err(|_| "Docker configuration directory unavailable")?
        } else {
            config
        };
        Ok((executable, config))
    }

    fn metadata_command(executable: &std::path::Path, config: &std::path::Path) -> Command {
        let mut command = Command::new(executable);
        for key in CONFLICTING_ENV {
            command.env_remove(key);
        }
        command.env("DOCKER_CONFIG", config);
        crate::path_util::apply_path_to_cmd(&mut command);
        command
    }

    fn probe(command: Command) -> Result<Vec<u8>, String> {
        let out =
            crate::child_process::capture(command, PROBE_TIMEOUT, PROBE_LIMIT).map_err(|e| {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    "Selected Docker engine check timed out".to_string()
                } else {
                    "Could not run the selected Docker CLI".to_string()
                }
            })?;
        if !out.status.success() {
            return Err(
                "The selected Docker context or engine is unavailable. Check it and recheck".into(),
            );
        }
        Ok(out.stdout)
    }

    fn discover(
        context: &str,
        executable: PathBuf,
        config: PathBuf,
    ) -> Result<EngineConnection, String> {
        validate_context_name(context)?;
        let mut command = metadata_command(&executable, &config);
        command.args(["context", "inspect", context, "--format", "{{json .}}"]);
        let json: serde_json::Value = serde_json::from_slice(&probe(command)?)
            .map_err(|_| "Invalid selected Docker context metadata")?;
        if json["Name"].as_str() != Some(context) {
            return Err("Selected Docker context metadata does not match its name".into());
        }
        let endpoint = json
            .pointer("/Endpoints/docker/Host")
            .and_then(serde_json::Value::as_str)
            .ok_or("Selected context has no Docker endpoint")?
            .to_string();
        let target = EngineTarget::new(context.to_string(), endpoint, executable, config)?;
        let client =
            Docker::connect_with_named_pipe(target.endpoint(), 5, bollard::API_DEFAULT_VERSION)
                .map_err(|_| "Cannot open the selected Docker pipe")?;
        Ok(EngineConnection {
            target,
            client,
            identity: Mutex::new(VerifiedIdentity::default()),
        })
    }

    pub fn active() -> Result<Arc<EngineConnection>, String> {
        let mut session = session().lock().map_err(|_| "Docker session unavailable")?;
        if let Some(error) = &session.preference_error {
            return Err(error.clone());
        }
        if let Some(connection) = &session.connection {
            return Ok(connection.clone());
        }
        if Instant::now() < session.retry_after {
            return Err(session
                .error
                .clone()
                .unwrap_or_else(|| "Selected Docker engine unavailable".into()));
        }
        let result = (|| {
            let (executable, config) = cli()?;
            if session.context.is_none() {
                let mut command = metadata_command(&executable, &config);
                command.args(["context", "show"]);
                let name = String::from_utf8(probe(command)?)
                    .map_err(|_| "Invalid Docker context name")?
                    .trim()
                    .to_string();
                validate_context_name(&name)?;
                session.context = Some(name);
            }
            discover(
                session
                    .context
                    .as_deref()
                    .ok_or("No Docker context selected")?,
                executable,
                config,
            )
            .map(Arc::new)
        })();
        match &result {
            Ok(connection) => {
                session.connection = Some(connection.clone());
                session.error = None;
            }
            Err(error) => {
                session.error = Some(format!(
                    "{}: {error}",
                    session.context.as_deref().unwrap_or("Docker")
                ));
                session.retry_after = Instant::now() + Duration::from_secs(2);
            }
        }
        result.map_err(|_| {
            session
                .error
                .clone()
                .unwrap_or_else(|| "Docker session unavailable".into())
        })
    }

    impl EngineConnection {
        /// In-memory identity for native boundary tests. Constructing Bollard's
        /// client does not connect; no discovery, probe or global session use.
        #[cfg(test)]
        pub(crate) fn identity_fixture() -> Arc<Self> {
            let root = std::env::current_dir().unwrap().canonicalize().unwrap();
            let target = EngineTarget::new(
                "unit-fixture".into(),
                "npipe:////./pipe/colimaui-unit-fixture".into(),
                std::env::current_exe().unwrap(),
                root,
            )
            .unwrap();
            let client =
                Docker::connect_with_named_pipe(target.endpoint(), 5, bollard::API_DEFAULT_VERSION)
                    .unwrap();
            let mut identity = VerifiedIdentity::default();
            identity
                .observe(Some("fixture-daemon"), Some("linux"))
                .unwrap();
            Arc::new(Self {
                target,
                client,
                identity: Mutex::new(identity),
            })
        }

        pub fn read_generation(&self) -> Result<u64, String> {
            self.identity
                .lock()
                .map_err(|_| "Docker identity unavailable")?
                .read_generation()
        }
        pub fn ensure_generation(&self, generation: u64) -> Result<(), String> {
            self.identity
                .lock()
                .map_err(|_| "Docker identity unavailable")?
                .ensure_generation(generation)
        }
        pub fn target(&self) -> &EngineTarget {
            &self.target
        }

        fn begin_check(&self) -> Result<u64, String> {
            Ok(self
                .identity
                .lock()
                .map_err(|_| "Docker identity unavailable")?
                .begin_check())
        }
        fn observe(&self, check: u64, id: Option<&str>, os: Option<&str>) -> Result<(), String> {
            self.identity
                .lock()
                .map_err(|_| "Docker identity unavailable")?
                .observe_at(check, id, os)
                .map_err(|error| format!("{}: {error}", self.target.context()))
        }

        fn lost(&self, check: u64, reason: String) {
            if let Ok(mut identity) = self.identity.lock() {
                identity.lost_at(check, reason);
            }
        }

        pub fn command(&self) -> Result<Command, String> {
            let check_id = self.begin_check()?;
            let mut check = self.target.command();
            check.args(["info", "--format", "{{json .}}"]);
            let result: Result<Command, String> = (|| {
                let value: serde_json::Value = serde_json::from_slice(&probe(check)?)
                    .map_err(|_| "Invalid selected engine identity")?;
                self.observe(check_id, value["ID"].as_str(), value["OSType"].as_str())?;
                Ok(self.target.command())
            })();
            if let Err(error) = &result {
                self.lost(check_id, error.clone());
            }
            result
        }

        pub async fn verified_client(&self) -> Result<Docker, String> {
            let check_id = self.begin_check()?;
            let result: Result<Docker, String> = async {
                let info = tokio::time::timeout(PROBE_TIMEOUT, async {
                    let client = self.client.clone().negotiate_version().await?;
                    client.info().await
                })
                .await
                .map_err(|_| "Selected Docker engine check timed out")?
                .map_err(|_| "Selected Docker engine unavailable")?;
                self.observe(check_id, info.id.as_deref(), info.os_type.as_deref())?;
                Ok(self.client.clone())
            }
            .await;
            if let Err(error) = &result {
                self.lost(check_id, error.clone());
            }
            result
        }

        pub fn status(&self) -> EngineStatus {
            match self.identity.lock() {
                Ok(identity) => EngineStatus {
                    generation: identity.read_generation().ok(),
                    context: Some(self.target.context.clone()),
                    endpoint: Some(self.target.endpoint.clone()),
                    engine_id: identity.original.clone(),
                    ready: identity.ready,
                    error: identity.error.clone(),
                },
                Err(_) => EngineStatus {
                    generation: None,
                    context: Some(self.target.context.clone()),
                    endpoint: None,
                    engine_id: None,
                    ready: false,
                    error: Some("Docker identity unavailable".into()),
                },
            }
        }
    }

    /// Observation only: unlike session(), this never initializes discovery.
    pub fn cached_status() -> Option<EngineStatus> {
        SESSION
            .get()?
            .lock()
            .ok()?
            .connection
            .as_ref()
            .map(|connection| connection.status())
    }

    pub fn status() -> EngineStatus {
        let Ok(session) = session().lock() else {
            return EngineStatus {
                generation: None,
                context: None,
                endpoint: None,
                engine_id: None,
                ready: false,
                error: Some("Docker session unavailable".into()),
            };
        };
        if let Some(connection) = &session.connection {
            return connection.status();
        }
        EngineStatus {
            generation: None,
            context: session.context.clone(),
            endpoint: None,
            engine_id: None,
            ready: false,
            error: session
                .preference_error
                .clone()
                .or_else(|| session.error.clone()),
        }
    }

    pub fn read_generation() -> Result<u64, String> {
        active()?.read_generation()
    }
    pub fn ensure_generation(generation: u64) -> Result<(), String> {
        active()?.ensure_generation(generation)
    }

    pub fn list_contexts() -> Result<Vec<EngineContextChoice>, String> {
        let (executable, config) = cli()?;
        let mut command = metadata_command(&executable, &config);
        command.args(["context", "ls", "--format", "{{json .}}"]);
        let bytes = probe(command)?;
        let text = String::from_utf8(bytes).map_err(|_| "Invalid Docker context list")?;
        let mut choices = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            if choices.len() >= 128 {
                return Err(
                    "Too many Docker contexts to display; reduce the context list and recheck"
                        .into(),
                );
            }
            let item: serde_json::Value =
                serde_json::from_str(line).map_err(|_| "Invalid Docker context list")?;
            let name = item["Name"]
                .as_str()
                .ok_or("Docker context has no name")?
                .to_string();
            validate_context_name(&name)?;
            let result = local_pipe(item["DockerEndpoint"].as_str().unwrap_or(""));
            choices.push(EngineContextChoice {
                name,
                supported: result.is_ok(),
                reason: result.err(),
            });
        }
        Ok(choices)
    }

    pub fn validate_selection(name: &str) -> Result<EngineStatus, String> {
        let (executable, config) = cli()?;
        let candidate = discover(name, executable, config)?;
        candidate.command()?;
        Ok(candidate.status())
    }
}

#[cfg(windows)]
pub use native::{
    active, cached_status, ensure_generation, list_contexts, read_generation, status,
    validate_selection, EngineConnection,
};

#[cfg(test)]
mod tests {
    use super::*;

    // TestSpec=TC-WINUI-026: partial source invariant at the identity owner;
    // native replacement, retained-resource effects and UI remain separate.
    #[test]
    fn recovery_preserves_original_identity_across_distinct_id_pairs() {
        let ids = [
            "engine-A",
            "engine-B",
            "0123456789abcdef",
            "origin:with:segments",
        ];
        for original in ids {
            for replacement in ids.into_iter().filter(|id| *id != original) {
                let mut identity = VerifiedIdentity::default();
                identity.observe(Some(original), Some("linux")).unwrap();
                let before_loss = identity.read_generation().unwrap();
                identity.lost("fixture loss".into());
                assert!(identity.ensure_generation(before_loss).is_err());

                // Invalid observations must not replace the retained identity,
                // even when they reuse the same endpoint/resource identities.
                for (id, os) in [
                    (Some(replacement), Some("linux")),
                    (Some(original), Some("windows")),
                    (Some(original), None),
                    (None, Some("linux")),
                    (Some(""), Some("linux")),
                ] {
                    assert!(identity.observe(id, os).is_err());
                    assert_eq!(identity.original(), Some(original));
                    assert!(identity.read_generation().is_err());
                }

                identity.observe(Some(original), Some("linux")).unwrap();
                let recovered = identity.read_generation().unwrap();
                assert_ne!(before_loss, recovered);
                assert!(identity.ensure_generation(before_loss).is_err());
                identity.observe(Some(original), Some("linux")).unwrap();
                assert_eq!(identity.read_generation().unwrap(), recovered);
                assert_eq!(identity.original(), Some(original));
            }
        }
    }

    // TestSpec=TC-WINUI-004; TestSpec=TC-WINUI-025 — emitted targeting controls only.
    // TC-WINUI-003/004/005/025/031: explicit target beats ambient metadata;
    // metadata and pending choices cannot mutate the captured value.
    #[test]
    fn target_capture_controls_endpoint_executable_and_environment() {
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let target = EngineTarget::new(
            "chosen".into(),
            "npipe:////./pipe/docker_engine".into(),
            root.join("docker.exe"),
            root.join("config"),
        )
        .unwrap();
        let original = target.endpoint().to_string();
        let command = target.command();
        assert_eq!(command.get_program(), root.join("docker.exe"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--host", original.as_str()]
        );
        let values: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            values
                .get(std::ffi::OsStr::new("DOCKER_CONFIG"))
                .copied()
                .flatten(),
            Some(root.join("config").as_os_str())
        );
        assert_eq!(
            values
                .get(std::ffi::OsStr::new("DOCKER_HOST"))
                .unwrap()
                .unwrap(),
            original.as_str()
        );
        for key in [
            "DOCKER_CONTEXT",
            "DOCKER_TLS",
            "DOCKER_TLS_VERIFY",
            "DOCKER_CERT_PATH",
            "DOCKER_API_VERSION",
        ] {
            assert_eq!(values.get(std::ffi::OsStr::new(key)), Some(&None));
        }
        let _next = EngineTarget::new(
            "next".into(),
            "npipe:////./pipe/another_engine".into(),
            root.join("other.exe"),
            root.join("elsewhere"),
        )
        .unwrap();
        assert_eq!(target.context(), "chosen");
        assert_eq!(target.endpoint(), original);
    }

    #[test]
    fn unsupported_transports_and_target_overrides_fail_closed() {
        for endpoint in [
            "tcp://localhost:2375",
            "ssh://host",
            "unix:///wsl/docker.sock",
            "npipe:////remote/pipe/docker_engine",
            "npipe:////./pipe/../other",
            "npipe:////./pipe/",
        ] {
            assert!(local_pipe(endpoint).is_err(), "{endpoint}");
        }
        for args in [
            vec!["--host=other", "ps"],
            vec!["-Hother", "ps"],
            vec!["--context", "other", "ps"],
            vec!["--config=other", "ps"],
            vec!["--log-level", "debug", "--host", "other", "ps"],
        ] {
            assert!(
                validate_argv(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
        assert!(validate_argv(&[
            "exec".into(),
            "container".into(),
            "echo".into(),
            "--host".into()
        ])
        .is_ok());
        for name in ["", "--help", "../other", "bad\nname"] {
            assert!(validate_context_name(name).is_err());
        }
    }

    // TC-WINUI-007/026/034/035: loss/replacement cannot authorize retained IDs.
    #[test]
    fn only_verified_original_linux_identity_can_be_ready() {
        let mut identity = VerifiedIdentity::default();
        assert!(identity.observe(None, Some("linux")).is_err());
        assert!(!identity.ready);
        assert!(identity.observe(Some("A"), Some("windows")).is_err());
        assert!(identity.original().is_none());
        identity.observe(Some("A"), Some("linux")).unwrap();
        assert!(identity.ready);
        identity.lost("unavailable".into());
        assert!(!identity.ready);
        assert_eq!(identity.original(), Some("A"));
        assert!(identity.observe(Some("B"), Some("linux")).is_err());
        assert!(!identity.ready);
        assert_eq!(identity.original(), Some("A"));
        identity.observe(Some("A"), Some("linux")).unwrap();
        assert!(identity.ready);
    }

    #[test]
    fn late_checks_and_pre_loss_snapshots_cannot_restore_old_readiness() {
        let mut identity = VerifiedIdentity::default();
        identity.observe(Some("A"), Some("linux")).unwrap();
        let old_generation = identity.read_generation().unwrap();
        let old = identity.begin_check();
        let newer = identity.begin_check();
        assert!(identity
            .observe_at(newer, Some("B"), Some("linux"))
            .is_err());
        assert!(identity.observe_at(old, Some("A"), Some("linux")).is_err());
        assert!(identity.ensure_generation(old_generation).is_err());
        identity.observe(Some("A"), Some("linux")).unwrap();
        assert!(identity.ensure_generation(old_generation).is_err());
        let current = identity.read_generation().unwrap();
        identity.lost_at(old, "late old failure".into());
        identity.ensure_generation(current).unwrap();
    }
}
