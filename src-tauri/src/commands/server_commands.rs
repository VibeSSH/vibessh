use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{Server, ServerInput};
use crate::services;
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
