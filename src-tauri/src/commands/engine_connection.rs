//! Connection preferences are next-launch input; inspecting/saving never swaps
//! the process's captured origin. Restart cleanup precedes Tauri's restart code.

use crate::engine_connection::{EngineContextChoice, EngineStatus, CONTEXT_SETTING};
use serde::Serialize;

#[derive(Serialize)]
pub struct ConnectionSettings {
    active: EngineStatus,
    pending: Option<String>,
    contexts: Vec<EngineContextChoice>,
    discovery_error: Option<String>,
}

#[tauri::command]
pub async fn get_engine_connection() -> Result<ConnectionSettings, String> {
    #[cfg(windows)]
    {
        let pending = super::knowledge_bank::get_setting(CONTEXT_SETTING.into())
            .await
            .map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || {
            // Recheck only the captured origin, never a saved pending choice.
            if let Ok(connection) = crate::engine_connection::active() {
                let _ = connection.command();
            }
            let choices = crate::engine_connection::list_contexts();
            ConnectionSettings {
                active: crate::engine_connection::status(),
                pending,
                discovery_error: choices.as_ref().err().cloned(),
                contexts: choices.unwrap_or_default(),
            }
        })
        .await
        .map_err(|_| "Connection check could not finish".into())
    }
    #[cfg(not(windows))]
    {
        Err("Connection preferences are available in the Windows Docker edition".into())
    }
}

#[tauri::command]
pub async fn check_engine_connection(context: String) -> Result<EngineStatus, String> {
    crate::engine_connection::validate_context_name(&context)?;
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || crate::engine_connection::validate_selection(&context))
            .await
            .map_err(|_| "Connection check could not finish")?
    }
    #[cfg(not(windows))]
    {
        Err("Connection preferences are available in the Windows Docker edition".into())
    }
}

#[tauri::command]
pub async fn save_engine_connection(context: String) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Connection preferences are available in the Windows Docker edition".into());
    }
    // The settings writer also validates alternate HTTP/AI writers of this key.
    super::knowledge_bank::set_setting(CONTEXT_SETTING.into(), context)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restart_for_engine(
    app: tauri::AppHandle,
    cancel_transfers: bool,
) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Connection preferences are available in the Windows Docker edition".into());
    }
    use tauri::Manager;
    let manager = (*app.state::<crate::terminal_session::SharedSessionManager>()).clone();
    tokio::task::spawn_blocking(move || {
        crate::transfer_registry::begin_shutdown(cancel_transfers)?;
        // Tauri ignores prevent_exit for RESTART_EXIT_CODE. No request is sent
        // until every owned operation has settled. Refusal leaves this process.
        manager
            .lock()
            .map_err(|_| "Terminal cleanup unavailable")?
            .close_all();
        super::file_transfer::shutdown_transfers(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|_| "Restart cleanup could not finish")??;
    app.request_restart();
    Ok(())
}
