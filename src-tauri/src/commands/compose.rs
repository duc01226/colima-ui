use serde::{Deserialize, Serialize};

use crate::commands::runtime;

/// Docker Compose project info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeProject {
    #[serde(alias = "Name")]
    pub name: String,
    #[serde(alias = "Status")]
    pub status: String,
    #[serde(alias = "ConfigFiles", default)]
    pub config_files: String,
}

/// List Docker Compose projects
#[tauri::command]
pub async fn list_compose_projects() -> Result<Vec<ComposeProject>, crate::error::ColimaError> {
    async move {
        let output = runtime::run(
            vec![
                "compose".into(),
                "ls".into(),
                "--format".into(),
                "json".into(),
                "-a".into(),
            ],
            runtime::DEFAULT_TIMEOUT,
        )
        .await?;

        if !output.status.success() {
            return Err(format!(
                "docker compose ls failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            return Ok(vec![]);
        }

        // docker compose ls --format json returns a JSON array
        let projects: Vec<ComposeProject> = match serde_json::from_str(stdout.trim()) {
            Ok(projects) => projects,
            Err(_) => stdout
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    serde_json::from_str(line)
                        .map_err(|_| "Invalid Docker Compose project response".to_string())
                })
                .collect::<Result<_, _>>()?,
        };

        Ok(projects)
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// Start a Docker Compose project
#[tauri::command]
pub async fn compose_up(
    project_dir: String,
    detach: bool,
) -> Result<String, crate::error::ColimaError> {
    async move {
        let mut args = vec!["compose".to_string()];
        args.extend(compose_location(&project_dir)?);
        args.push("up".to_string());
        if detach {
            args.push("-d".to_string());
        }

        let output = runtime::run(args, runtime::DEFAULT_TIMEOUT).await?;

        if !output.status.success() {
            return Err(format!(
                "docker compose up failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok("Compose project started".to_string())
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// Stop a Docker Compose project
#[tauri::command]
pub async fn compose_down(project_name: String) -> Result<String, crate::error::ColimaError> {
    async move {
        let mut args = project_args(&project_name).await?;
        args.push("down".into());
        let output = runtime::run(args, runtime::DEFAULT_TIMEOUT).await?;

        if !output.status.success() {
            return Err(format!(
                "docker compose down failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(format!("Compose project '{}' stopped", project_name))
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// Restart a Docker Compose project
#[tauri::command]
pub async fn compose_restart(project_name: String) -> Result<String, crate::error::ColimaError> {
    async move {
        let mut args = project_args(&project_name).await?;
        args.push("restart".into());
        let output = runtime::run(args, runtime::DEFAULT_TIMEOUT).await?;

        if !output.status.success() {
            return Err(format!(
                "docker compose restart failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(format!("Compose project '{}' restarted", project_name))
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// Stop a Docker Compose project (stops containers without removing them)
#[tauri::command]
pub async fn compose_stop(project_name: String) -> Result<String, crate::error::ColimaError> {
    async move {
        let mut args = project_args(&project_name).await?;
        args.push("stop".into());
        let output = runtime::run(args, runtime::DEFAULT_TIMEOUT).await?;

        if !output.status.success() {
            return Err(format!(
                "docker compose stop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(format!("Compose project '{}' stopped", project_name))
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// Get compose project logs
#[tauri::command]
pub async fn compose_logs(
    project_name: String,
    lines: u32,
) -> Result<String, crate::error::ColimaError> {
    async move {
        let tail = lines.to_string();
        let mut args = project_args(&project_name).await?;
        args.extend(["logs".into(), "--tail".into(), tail, "--no-color".into()]);
        let output = runtime::run(args, runtime::DEFAULT_TIMEOUT).await?;
        if !output.status.success() {
            return Err(format!(
                "docker compose logs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let combined = if stdout.is_empty() {
            stderr.to_string()
        } else {
            stdout.to_string()
        };

        Ok(combined)
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// List services in a compose project
#[tauri::command]
pub async fn compose_ps(project_name: String) -> Result<String, crate::error::ColimaError> {
    async move {
        let mut args = project_args(&project_name).await?;
        args.extend(["ps".into(), "--format".into(), "json".into()]);
        let output = runtime::run(args, runtime::DEFAULT_TIMEOUT).await?;

        if !output.status.success() {
            return Err(format!(
                "docker compose ps failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
    .await
    .map_err(|e: String| crate::error::ColimaError::from(e))
}

/// The legacy project_dir field accepts either an existing Compose file or a
/// directory. Keep the argv meaning explicit; never infer a daemon mount path.
fn compose_location(location: &str) -> Result<Vec<String>, String> {
    if location.is_empty() {
        #[cfg(windows)]
        return Err("Choose an existing Compose file or directory".into());
        #[cfg(not(windows))]
        return Ok(vec![]);
    }
    let path = std::path::Path::new(location)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve Compose location: {e}"))?;
    let flag = if path.is_file() {
        "-f"
    } else if path.is_dir() {
        "--project-directory"
    } else {
        return Err("Compose location must be a file or directory".into());
    };
    Ok(vec![
        flag.into(),
        path.into_os_string()
            .into_string()
            .map_err(|_| "Compose location is not valid text")?,
    ])
}

// The Windows branch performs asynchronous engine verification.
#[cfg_attr(not(windows), allow(clippy::unused_async))]
async fn project_args(name: &str) -> Result<Vec<String>, String> {
    if !crate::validation::is_valid_resource_name(name) {
        return Err("Invalid Compose project name".into());
    }
    // POSIX retains the existing launch-directory behavior. Windows needs the
    // selected project's client-side configuration rather than the app directory.
    #[cfg(not(windows))]
    return Ok(vec!["compose".into(), "-p".into(), name.into()]);
    #[cfg(windows)]
    {
        let projects = list_compose_projects().await.map_err(|e| e.to_string())?;
        let project = projects
            .iter()
            .find(|p| p.name == name)
            .ok_or("Compose project is no longer listed")?;
        let mut args = vec!["compose".into(), "-p".into(), name.into()];
        if project.config_files.is_empty() {
            return Err("The selected project has no readable client-side Compose file".into());
        }
        for file in project.config_files.split(',') {
            let location = compose_location(file)?;
            if location.first().is_none_or(|flag| flag != "-f") {
                return Err("Compose project configuration must name existing files".into());
            }
            args.extend(location);
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compose_file_and_directory_keep_their_distinct_meaning() {
        let root = std::env::temp_dir().join(format!("compose-路径 space-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("compose.yaml");
        std::fs::write(&file, "services: {}\n").unwrap();
        let dir_args = compose_location(root.to_str().unwrap()).unwrap();
        let file_args = compose_location(file.to_str().unwrap()).unwrap();
        assert_eq!(
            dir_args,
            vec![
                "--project-directory".to_string(),
                root.canonicalize().unwrap().to_str().unwrap().to_string()
            ]
        );
        assert_eq!(
            file_args,
            vec![
                "-f".to_string(),
                file.canonicalize().unwrap().to_str().unwrap().to_string()
            ]
        );
        assert!(compose_location(root.join("missing").to_str().unwrap()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
