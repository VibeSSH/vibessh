//! Tauri command bridge for the interactive Terminal module. Each open
//! terminal gets its own event pair (`terminal://{id}/output`,
//! `terminal://{id}/closed`) rather than one shared event name, since -
//! unlike pairing, which only ever has one attempt in flight - a user can
//! reasonably have several terminals open across different servers at once.

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::services;
use crate::state::{SshSessionManager, TerminalSessionManager};
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    repo: State<'_, ServerRepository>,
    ssh_sessions: State<'_, SshSessionManager>,
    terminal_sessions: State<'_, TerminalSessionManager>,
    server_id: Uuid,
    cols: u32,
    rows: u32,
) -> AppResult<Uuid> {
    let terminal_id = Uuid::new_v4();
    let output_event = format!("terminal://{terminal_id}/output");
    let closed_event = format!("terminal://{terminal_id}/closed");
    let app_for_closed = app.clone();

    let handle = services::open_ssh_terminal(
        &repo,
        &ssh_sessions,
        server_id,
        cols,
        rows,
        move |data| {
            let _ = app.emit(&output_event, data);
        },
        move |reason| {
            let _ = app_for_closed.emit(&closed_event, reason);
        },
    )
    .await?;

    terminal_sessions.insert(terminal_id, handle).await;
    Ok(terminal_id)
}

#[tauri::command]
pub async fn write_to_terminal(terminal_sessions: State<'_, TerminalSessionManager>, terminal_id: Uuid, data: String) -> AppResult<()> {
    if terminal_sessions.write(terminal_id, data.into_bytes()).await {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("terminal {terminal_id}")))
    }
}

#[tauri::command]
pub async fn resize_terminal(
    terminal_sessions: State<'_, TerminalSessionManager>,
    terminal_id: Uuid,
    cols: u32,
    rows: u32,
) -> AppResult<()> {
    if terminal_sessions.resize(terminal_id, cols, rows).await {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("terminal {terminal_id}")))
    }
}

#[tauri::command]
pub async fn close_terminal(terminal_sessions: State<'_, TerminalSessionManager>, terminal_id: Uuid) -> AppResult<()> {
    terminal_sessions.close(terminal_id).await;
    Ok(())
}
