//! Tauri commands for interactive terminal sessions.
//!
//! These replaced `/api/terminal/*`. The HTTP version needed a token on every
//! keystroke, polled for output every 100 ms, and exposed a shell on a local
//! port. Over IPC there is no port to reach, no handshake to authenticate, and
//! output is pushed instead of polled.

use tauri::ipc::Channel;
use tauri::State;

use crate::terminal_session::{SessionKind, SharedSessionManager};

/// Carry a verified origin into the existing retained Terminal view. IPC only.
#[tauri::command]
pub async fn terminal_target(
    container_id: String,
) -> Result<SessionKind, crate::error::ColimaError> {
    tokio::task::spawn_blocking(move || -> Result<SessionKind, String> {
        let (_, engine) = crate::terminal_session::container_command(&container_id)?;
        Ok(SessionKind::ContainerExec {
            container_id,
            engine,
        })
    })
    .await
    .map_err(|e| crate::error::ColimaError::from(format!("Terminal task failed: {e}")))?
    .map_err(crate::error::ColimaError::from)
}

/// Note which session was opened, so the UI can offer "reopen last session".
///
/// Metadata only — kind and target. Session *content* is deliberately never
/// stored or logged: people type credentials into shells.
fn record_session_opened(kind: &SessionKind) {
    let (k, target) = match kind {
        SessionKind::ContainerExec {
            container_id,
            engine,
        } => ("containerExec", format!("{engine}/{container_id}")),
        SessionKind::Colima { profile } => ("colima", profile.clone()),
        SessionKind::Lima { instance } => ("lima", instance.clone()),
        SessionKind::K8sExec {
            namespace,
            pod,
            container,
        } => (
            "k8sExec",
            if container.is_empty() {
                format!("{namespace}/{pod}")
            } else {
                format!("{namespace}/{pod}/{container}")
            },
        ),
    };

    // Best-effort: failing to write history must never stop a shell opening.
    if let Ok(conn) = crate::commands::knowledge_bank::get_db().lock() {
        let _ = conn.execute(
            "INSERT INTO terminal_sessions (kind, target) VALUES (?1, ?2)",
            rusqlite::params![k, target],
        );
    }
}

/// Open a session and start streaming its output into `on_output`.
#[tauri::command]
pub async fn terminal_create(
    mgr: State<'_, SharedSessionManager>,
    session_id: String,
    kind: SessionKind,
    on_output: Channel<String>,
) -> Result<(), crate::error::ColimaError> {
    crate::terminal_session::create_shared_session(
        (*mgr).clone(),
        session_id,
        kind.clone(),
        Box::new(move |text| {
            // The session owns the channel lifetime; Close reaps its child.
            let _ = on_output.send(text);
        }),
    )
    .await
    .map_err(crate::error::ColimaError::from)?;
    record_session_opened(&kind);
    Ok(())
}

#[tauri::command]
pub async fn terminal_write(
    mgr: State<'_, SharedSessionManager>,
    session_id: String,
    data: String,
) -> Result<(), crate::error::ColimaError> {
    let mut m = mgr.lock().unwrap();
    m.write(&session_id, &data)
        .map_err(crate::error::ColimaError::from)
}

/// Push a new grid size, which delivers `SIGWINCH` to the child.
#[tauri::command]
pub async fn terminal_resize(
    mgr: State<'_, SharedSessionManager>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), crate::error::ColimaError> {
    let mut m = mgr.lock().unwrap();
    m.resize(&session_id, rows, cols)
        .map_err(crate::error::ColimaError::from)
}

/// Report the shell's exit code once it has died, so the UI stops pretending
/// it is connected. Output itself is pushed, not polled.
#[tauri::command]
pub async fn terminal_poll_exit(
    mgr: State<'_, SharedSessionManager>,
    session_id: String,
) -> Result<Option<u32>, crate::error::ColimaError> {
    let mut m = mgr.lock().unwrap();
    Ok(m.poll_exit(&session_id))
}

#[tauri::command]
pub async fn terminal_close(
    mgr: State<'_, SharedSessionManager>,
    session_id: String,
) -> Result<(), crate::error::ColimaError> {
    let mut m = mgr.lock().unwrap();
    m.close(&session_id)
        .map_err(crate::error::ColimaError::from)
}
