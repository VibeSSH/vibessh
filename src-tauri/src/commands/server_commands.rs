use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{NodeCapabilities, Server, ServerInput};
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub fn create_server(repo: State<ServerRepository>, input: ServerInput) -> AppResult<Server> {
    services::create_server(&repo, input)
}

#[tauri::command]
pub fn update_server(repo: State<ServerRepository>, id: Uuid, input: ServerInput) -> AppResult<Server> {
    services::update_server(&repo, id, input)
}

#[tauri::command]
pub fn delete_server(repo: State<ServerRepository>, id: Uuid) -> AppResult<()> {
    services::delete_server(&repo, id)
}

#[tauri::command]
pub fn get_server(repo: State<ServerRepository>, id: Uuid) -> AppResult<Server> {
    services::get_server(&repo, id)
}

#[tauri::command]
pub fn list_servers(repo: State<ServerRepository>) -> AppResult<Vec<Server>> {
    services::list_servers(&repo)
}

#[tauri::command]
pub fn upsert_agent_server(
    repo: State<ServerRepository>,
    name: String,
    host: String,
    agent_id: Uuid,
    docker_capable: Option<bool>,
) -> AppResult<Server> {
    services::upsert_agent_server(&repo, &name, &host, agent_id, docker_capable)
}

/// SSH-mode only (see `services::probe_node_capabilities`'s own doc
/// comment) - the Create Application wizard calls this per SSH-mode server
/// as its Node picker loads, Etap M1.
#[tauri::command]
pub async fn probe_server_capabilities(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<NodeCapabilities> {
    services::probe_node_capabilities(&repo, &sessions, id).await
}
