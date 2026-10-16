//! Dedicated-engine/run ownership for native Windows application-boundary tests.
//! No production path override or global Docker context mutation is introduced.
#[cfg(windows)]
use crate::{child_process, engine_connection};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Output;
#[cfg(windows)]
use std::{sync::Arc, time::Duration};

pub struct Fixture {
    pub run: String,
    pub provider: String,
    pub context: String,
    pub engine_id: String,
    pub image: String,
    pub image_id: String,
    pub work: PathBuf,
    #[cfg(windows)]
    connection: Arc<engine_connection::EngineConnection>,
    owned: Vec<(&'static str, String)>,
    compose_files: Vec<PathBuf>,
}

#[cfg(windows)]
fn required(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("ENVIRONMENT-BLOCKED: {name} is required"))
}

impl Fixture {
    #[cfg(not(windows))]
    pub fn acquire() -> Result<Self, String> {
        Err("ENVIRONMENT-BLOCKED: provider qualification requires native Windows 11 x64".into())
    }

    #[cfg(windows)]
    pub fn acquire() -> Result<Self, String> {
        let engine_id = required("COLIMAUI_TEST_ENGINE_ID")?;
        if required("COLIMAUI_TEST_DESTRUCTIVE")? != format!("dedicated:{engine_id}")
            || required("COLIMAUI_TEST_PROFILE")? != "disposable-account"
        {
            return Err("ENVIRONMENT-BLOCKED: explicitly arrange a dedicated engine and disposable Windows account".into());
        }
        let provider = required("COLIMAUI_TEST_PROVIDER")?;
        if !["docker-desktop", "rancher-moby"].contains(&provider.as_str()) {
            return Err("ENVIRONMENT-BLOCKED: unsupported provider qualification target".into());
        }
        let context = required("COLIMAUI_TEST_CONTEXT")?;
        engine_connection::validate_context_name(&context)?;
        let run = required("COLIMAUI_TEST_RUN_ID")?;
        if run.len() != 36 || !run.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err("ENVIRONMENT-BLOCKED: use the qualification runner's unique run ID".into());
        }
        let image = required("COLIMAUI_TEST_IMAGE")?;
        let image_id = required("COLIMAUI_TEST_IMAGE_ID")?;
        if !image_id
            .strip_prefix("sha256:")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(
                "ENVIRONMENT-BLOCKED: fixture image must have an exact local content ID".into(),
            );
        }
        let connection = engine_connection::active()?;
        let _ = connection.command()?;
        let status = connection.status();
        if status.context.as_deref() != Some(context.as_str())
            || status.engine_id.as_deref() != Some(engine_id.as_str())
            || !status.ready
        {
            return Err("ENVIRONMENT-BLOCKED: app-selected context/engine differs from the explicitly dedicated target; select it in the disposable profile and restart".into());
        }
        let parent = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/reports/windows-native")
            .join(&run);
        std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
        let work = parent.join("work 空間");
        std::fs::create_dir(&work).map_err(|e| format!("Refuse reused work directory: {e}"))?;
        let fixture = Self {
            run,
            provider,
            context,
            engine_id,
            image,
            image_id,
            work: work.canonicalize().map_err(|e| e.to_string())?,
            connection,
            owned: vec![],
            compose_files: vec![],
        };
        for args in [
            vec!["ps", "-aq"],
            vec!["volume", "ls", "-q"],
            vec!["network", "ls", "--filter", "type=custom", "-q"],
        ] {
            if !fixture.text(&args)?.trim().is_empty() {
                return Err("ENVIRONMENT-BLOCKED: dedicated engine must have no existing containers, volumes or custom networks".into());
            }
        }
        let image = fixture.inspect("image", &fixture.image)?;
        if image[0]["Id"].as_str() != Some(fixture.image_id.as_str())
            || image[0]["Config"]["Labels"]["io.colimaui.fixture"].as_str() != Some("winui-v1")
            || image[0]["Config"]["Cmd"] != serde_json::json!(["sleep", "600"])
        {
            return Err("ENVIRONMENT-BLOCKED: local fixture digest/label/command does not match the declared fixture".into());
        }
        Ok(fixture)
    }

    pub fn name(&self, purpose: &str) -> String {
        format!("cui-{}-{purpose}", self.run)
    }

    pub fn output(&self, args: &[&str]) -> Result<Output, String> {
        #[cfg(windows)]
        {
            let mut command = self.connection.command()?;
            command.args(args);
            child_process::capture(command, Duration::from_secs(30), 4 * 1024 * 1024)
                .map_err(|e| format!("Dedicated-engine command failed: {e}"))
        }
        #[cfg(not(windows))]
        {
            let _ = args;
            Err("ENVIRONMENT-BLOCKED: native Windows fixture required".into())
        }
    }

    pub fn text(&self, args: &[&str]) -> Result<String, String> {
        let output = self.output(args)?;
        if !output.status.success() {
            return Err(crate::redact::redact(&String::from_utf8_lossy(
                &output.stderr,
            )));
        }
        String::from_utf8(output.stdout).map_err(|_| "Non-UTF8 fixture command output".into())
    }

    pub fn inspect(&self, kind: &str, name: &str) -> Result<Value, String> {
        serde_json::from_str(&self.text(&[kind, "inspect", "--", name])?).map_err(|e| e.to_string())
    }

    pub fn claim(&mut self, kind: &'static str, name: &str) {
        assert!(
            name.starts_with(&self.name("")),
            "fixture may claim only its unique run names"
        );
        self.owned.push((kind, name.into()));
    }

    pub fn claim_compose(&mut self, file: &Path) {
        assert!(file.starts_with(&self.work));
        self.compose_files.push(file.into());
    }

    pub fn cleanup(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        for file in self.compose_files.iter().rev() {
            let result = self.text(&[
                "compose",
                "-f",
                file.to_str().ok_or("Non-UTF8 fixture path")?,
                "down",
                "--remove-orphans",
            ]);
            if let Err(error) = result {
                failures.push(error);
            }
        }
        for (kind, name) in self.owned.iter().rev() {
            let inspected = self.output(&[kind, "inspect", "--", name])?;
            if !inspected.status.success() {
                let error = String::from_utf8_lossy(&inspected.stderr).to_lowercase();
                if !error.contains("no such") {
                    failures.push(format!("Cannot verify owned {kind} before cleanup"));
                }
                continue;
            }
            // These names were registered before their create attempt, on an
            // initially empty dedicated engine. Never prune or select by prefix.
            let result = self.text(&[kind, "rm", "--force", "--", name]);
            if let Err(error) = result {
                failures.push(error);
            }
        }
        if failures.is_empty() {
            self.owned.clear();
            self.compose_files.clear();
            Ok(())
        } else {
            Err(format!(
                "Owned fixture cleanup failed: {}",
                failures.join("; ")
            ))
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("{error}");
        }
        let _ = std::fs::remove_dir_all(&self.work);
    }
}
