use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::ServerInput;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;
use crate::transport::CommandOutput;

#[tauri::command]
pub async fn test_ssh_connection(input: ServerInput) -> AppResult<()> {
    services::test_ssh_connection(&input).await
}

#[tauri::command]
pub async fn execute_ssh_command(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    command: String,
) -> AppResult<CommandOutput> {
    services::execute_ssh_command(&repo, &sessions, id, &command).await
}
