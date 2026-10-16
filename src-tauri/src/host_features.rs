//! Supported application features, independent of installed executable names.
//! An externally owned Windows engine does not grant VM or installer authority.

#[derive(Clone, Copy, Debug)]
pub enum HostFeature {
    ManagedVms,
    Kubernetes,
    RuntimeInstall,
}

impl HostFeature {
    pub fn require(self) -> Result<(), String> {
        self.require_on(cfg!(windows))
    }

    fn require_on(self, windows: bool) -> Result<(), String> {
        if !windows {
            return Ok(());
        }
        let feature = match self {
            Self::ManagedVms => "Colima/Lima VM management",
            Self::Kubernetes => "Kubernetes management",
            Self::RuntimeInstall => "Runtime installation",
        };
        Err(format!("{feature} is unavailable in the Windows Docker edition. Manage the existing engine in Docker Desktop or Rancher Desktop."))
    }
}

pub fn require_tool(program: &str) -> Result<(), String> {
    if let Some(feature) = tool_feature(program) {
        feature.require()?;
    }
    Ok(())
}

fn tool_feature(program: &str) -> Option<HostFeature> {
    let name = program.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "colima" | "limactl" | "nerdctl" | "qemu" | "qemu-img" | "krunkit" => {
            Some(HostFeature::ManagedVms)
        }
        "kubectl" | "kind" | "helm" => Some(HostFeature::Kubernetes),
        "brew" | "nix" | "nix-env" | "apt" | "apt-get" | "wsl" => Some(HostFeature::RuntimeInstall),
        _ => None,
    }
}

fn path_feature(path: &str) -> Option<HostFeature> {
    let under =
        |base: &str| path == base || path.strip_prefix(base).is_some_and(|s| s.starts_with('/'));
    if ["/api/instances", "/api/lima", "/api/models"]
        .iter()
        .any(|p| under(p))
        || path == "/api/diagnostics/logs"
    {
        Some(HostFeature::ManagedVms)
    } else if ["/api/k8s", "/api/kind"].iter().any(|p| under(p)) {
        Some(HostFeature::Kubernetes)
    } else if [
        "/api/system/install",
        "/api/system/homebrew",
        "/api/system/autostart",
    ]
    .contains(&path)
    {
        Some(HostFeature::RuntimeInstall)
    } else {
        None
    }
}

/// Direct HTTP owners include raw process paths, so guard before dispatch too.
pub async fn http_guard(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Some(feature) = path_feature(request.uri().path()) {
        if let Err(reason) = feature.require() {
            return (
                axum::http::StatusCode::NOT_IMPLEMENTED,
                axum::Json(crate::helpers::ApiResponse::<()> {
                    success: false,
                    data: None,
                    error: Some(reason.into()),
                }),
            )
                .into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // TC-WINUI-015/029: installed tools cannot broaden the Windows contract.
    #[test]
    fn unavailable_host_features_refuse_every_tool_and_direct_route_family() {
        for tool in [
            "colima",
            "C:\\tools\\COLIMA.EXE",
            "/usr/bin/limactl",
            "kubectl.exe",
            "kind",
            "helm",
            "brew",
            "wsl.exe",
        ] {
            let feature = tool_feature(tool).expect("unsupported host tool");
            assert!(feature.require_on(true).is_err(), "{tool}");
            assert!(feature.require_on(false).is_ok(), "POSIX behavior retained");
        }
        for path in [
            "/api/instances/start",
            "/api/instances/config/apply",
            "/api/lima/shell",
            "/api/k8s/pods/logs/stream",
            "/api/kind/create",
            "/api/models/pull",
            "/api/system/install",
        ] {
            assert!(
                path_feature(path).unwrap().require_on(true).is_err(),
                "{path}"
            );
        }
        for path in [
            "/api/containers/start",
            "/api/compose/up",
            "/api/ai/models",
            "/api/kb/articles",
            "/api/events",
            "/api/instances-extra",
        ] {
            assert!(path_feature(path).is_none(), "unrelated contract {path}");
        }
        assert!(tool_feature("docker.exe").is_none());
    }
}
