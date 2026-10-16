use crate::api_server::*;
use crate::commands::autostart;
use crate::commands::containers;
use crate::commands::system;
use crate::routes::payloads::*;
use axum::{extract::Query, http::StatusCode, response::Json};

pub async fn api_check_system() -> (StatusCode, Json<ApiResponse<system::SystemInfo>>) {
    let info = SYSTEM_INFO_CACHE.lock().map_or_else(
        |_| load_system_info(),
        |mut cache| cache.get_or_init(load_system_info),
    );
    ok(info)
}

pub async fn api_get_version() -> (StatusCode, Json<ApiResponse<String>>) {
    let info = SYSTEM_INFO_CACHE.lock().map_or_else(
        |_| load_system_info(),
        |mut cache| cache.get_or_init(load_system_info),
    );
    ok(info.colima_version)
}

pub async fn api_check_homebrew() -> (StatusCode, Json<ApiResponse<system::HomebrewStatus>>) {
    match run_blocking(|| Ok(system::homebrew_status_blocking())).await {
        Ok(status) => ok(status),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_configure_autostart(
    Json(req): Json<AutostartRequest>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    match run_blocking(move || autostart::configure_autostart_blocking(req.enable)).await {
        Ok(msg) => ok(msg),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_get_autostart_status(
) -> (StatusCode, Json<ApiResponse<autostart::AutostartStatus>>) {
    match run_blocking(|| Ok(autostart::autostart_status_blocking())).await {
        Ok(status) => ok(status),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_check_tool(
    Query(q): Query<ToolQuery>,
) -> (StatusCode, Json<ApiResponse<ToolStatus>>) {
    match system::check_tool(q.name).await {
        Ok(value) => ok(ToolStatus {
            installed: value["installed"].as_bool().unwrap_or(false),
            version: value["version"].as_str().unwrap_or_default().into(),
        }),
        Err(e) => err(e),
    }
}

pub async fn api_get_platform() -> (StatusCode, Json<ApiResponse<PlatformInfo>>) {
    match run_blocking(|| Ok(detect_platform())).await {
        Ok(info) => ok(info),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_host_specs() -> (StatusCode, Json<ApiResponse<system::HostSpecs>>) {
    match run_blocking(system::detect_host_specs).await {
        Ok(specs) => ok(specs),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_install_dep(
    Json(req): Json<InstallDepRequest>,
) -> (StatusCode, Json<ApiResponse<system::InstallResult>>) {
    match run_blocking(move || system::install_dependency_blocking(&req.name, &req.method)).await {
        Ok(result) => ok(result),
        Err(e) => err(e.to_string()),
    }
}

/// Delegates to commands layer for system prune
pub async fn api_system_prune(
    Query(q): Query<PruneQuery>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let all = q.all.unwrap_or(false);
    match containers::system_prune(all).await {
        Ok(out) => ok(out),
        Err(e) => err(e.to_string()),
    }
}

/// Live CPU / memory / disk figures for the active container engine.
pub async fn api_engine_resources() -> (
    StatusCode,
    Json<ApiResponse<crate::commands::engine_resources::EngineResources>>,
) {
    match crate::commands::engine_resources::engine_resources().await {
        Ok(res) => ok(res),
        Err(e) => err(e.to_string()),
    }
}

/// Delegates to commands layer for system df
pub async fn api_system_df() -> (StatusCode, Json<ApiResponse<String>>) {
    match containers::system_df().await {
        Ok(out) => ok(out),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_docker_df() -> (StatusCode, Json<ApiResponse<String>>) {
    match containers::system_df().await {
        Ok(out) => ok(out),
        Err(e) => err(e.to_string()),
    }
}

pub async fn api_docker_prune(
    Query(q): Query<DockerPruneQuery>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Fix #15: Require explicit confirmation for destructive prune operation
    if !q.confirm {
        return err("This will remove all unused images, containers, networks, and volumes. Pass ?confirm=true to proceed.".to_string());
    }
    match containers::system_prune(true).await {
        Ok(out) => ok(out),
        Err(e) => err(e.to_string()),
    }
}

// `api_auth_token` used to be here, serving `GET /api/auth/token` from the
// public router. It claimed CORS as its protection, which was never true of
// anything but a browser: `curl` from any local process got the token and with
// it the entire API. Clients now obtain the token through `auth::api_token`
// (IPC) or a URL fragment handed over by the app.

/// Simple health check endpoint (unauthenticated).
pub async fn api_health() -> (StatusCode, Json<ApiResponse<String>>) {
    ok("ok".to_string())
}

/// Set the live-metrics sampling period.
///
/// Browser-mode twin of the `set_metrics_interval` command. The collector clamps
/// the value and returns what it actually used, so the UI can show the truth
/// rather than what it asked for.
pub async fn api_set_metrics_interval(
    Json(body): Json<crate::routes::payloads::MetricsIntervalBody>,
) -> (StatusCode, Json<ApiResponse<u64>>) {
    crate::commands::metrics_collector::set_interval_ms(body.ms);
    ok(crate::commands::metrics_collector::interval_ms())
}

pub async fn api_metrics_status() -> (
    StatusCode,
    Json<ApiResponse<crate::commands::metrics_collector::CollectorStatus>>,
) {
    match crate::commands::metrics_collector::collector_status() {
        Ok(status) => ok(status),
        Err(error) => err(error),
    }
}
