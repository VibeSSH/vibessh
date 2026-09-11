//! Tauri command bridge for the Port Forwarding module (`ssh -L`/`-R`/`-D`).
//! Same split as `terminal_commands`: `services::start_port_forward` only
//! resolves the connection and starts the tunnel, this command inserts the
//! result into `state::PortForwardManager` - `stop`/`list` talk to that
//! manager directly, no service-layer indirection needed for either.

use tauri::State;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{PortForwardStatus, StartPortForwardInput};
use crate::services;
use crate::state::{PortForwardManager, SshSessionManager};
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub async fn start_port_forward(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    forwards: State<'_, PortForwardManager>,
    input: StartPortForwardInput,
) -> AppResult<PortForwardStatus> {
    let (status, handle) = services::start_port_forward(&repo, &sessions, &input).await?;
    forwards.insert(status.clone(), handle).await;
    Ok(status)
}

#[tauri::command]
pub async fn list_port_forwards(forwards: State<'_, PortForwardManager>) -> AppResult<Vec<PortForwardStatus>> {
    Ok(forwards.list().await)
}

#[tauri::command]
pub async fn stop_port_forward(forwards: State<'_, PortForwardManager>, id: Uuid) -> AppResult<()> {
    if forwards.stop(id).await {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("port forward {id}")))
    }
}
