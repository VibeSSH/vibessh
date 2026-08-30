use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{ApplicationDatabase, CreateDatabaseHostInput, DatabaseHost};
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub fn list_database_hosts(repo: State<DatabaseRepository>) -> AppResult<Vec<DatabaseHost>> {
    services::list_database_hosts(&repo)
}

#[tauri::command]
pub fn create_database_host(repo: State<DatabaseRepository>, input: CreateDatabaseHostInput) -> AppResult<DatabaseHost> {
    services::create_database_host(&repo, input)
}

#[tauri::command]
pub fn delete_database_host(repo: State<DatabaseRepository>, id: Uuid) -> AppResult<()> {
    services::delete_database_host(&repo, id)
}

#[tauri::command]
pub fn set_database_host_phpmyadmin(repo: State<DatabaseRepository>, id: Uuid, application_id: Option<Uuid>) -> AppResult<DatabaseHost> {
    services::set_database_host_phpmyadmin(&repo, id, application_id)
}

#[tauri::command]
pub fn list_application_databases(repo: State<DatabaseRepository>, application_id: Uuid) -> AppResult<Vec<ApplicationDatabase>> {
    services::list_application_databases(&repo, application_id)
}

#[tauri::command]
pub async fn create_application_database(
    db_repo: State<'_, DatabaseRepository>,
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    database_host_id: Uuid,
    purpose: Option<String>,
) -> AppResult<ApplicationDatabase> {
    services::create_application_database(&db_repo, &app_repo, &server_repo, &sessions, application_id, database_host_id, purpose.as_deref())
        .await
}

#[tauri::command]
pub async fn delete_application_database(
    db_repo: State<'_, DatabaseRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<()> {
    services::delete_application_database(&db_repo, &server_repo, &sessions, id).await
}

#[tauri::command]
pub fn reveal_application_database_password(db_repo: State<DatabaseRepository>, id: Uuid) -> AppResult<String> {
    services::reveal_application_database_password(&db_repo, id)
}

#[tauri::command]
pub fn get_phpmyadmin_url(
    db_repo: State<DatabaseRepository>,
    app_repo: State<ApplicationRepository>,
    server_repo: State<ServerRepository>,
    database_host_id: Uuid,
    database_name: Option<String>,
) -> AppResult<String> {
    services::phpmyadmin_url(&db_repo, &app_repo, &server_repo, database_host_id, database_name.as_deref())
}

#[tauri::command]
pub async fn reset_application_database_password(
    db_repo: State<'_, DatabaseRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<String> {
    services::reset_application_database_password(&db_repo, &server_repo, &sessions, id).await
}
