//! Scheduled power actions - see `services::schedule_service`.

use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{ApplicationSchedule, ApplicationSchedules, ScheduleInput};
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::application_schedule_repository::ApplicationScheduleRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub async fn list_application_schedules(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    schedule_repo: State<'_, ApplicationScheduleRepository>,
    id: Uuid,
) -> AppResult<ApplicationSchedules> {
    services::list_schedules(&repo, &server_repo, &sessions, &schedule_repo, id).await
}

#[tauri::command]
pub async fn create_application_schedule(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    schedule_repo: State<'_, ApplicationScheduleRepository>,
    id: Uuid,
    input: ScheduleInput,
) -> AppResult<ApplicationSchedule> {
    services::create_schedule(&repo, &server_repo, &sessions, &schedule_repo, id, &input).await
}

#[tauri::command]
pub async fn update_application_schedule(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    schedule_repo: State<'_, ApplicationScheduleRepository>,
    schedule_id: Uuid,
    input: ScheduleInput,
) -> AppResult<ApplicationSchedule> {
    services::update_schedule(&repo, &server_repo, &sessions, &schedule_repo, schedule_id, &input).await
}

#[tauri::command]
pub async fn delete_application_schedule(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    schedule_repo: State<'_, ApplicationScheduleRepository>,
    schedule_id: Uuid,
) -> AppResult<()> {
    services::delete_schedule(&repo, &server_repo, &sessions, &schedule_repo, schedule_id).await
}

#[tauri::command]
pub async fn run_application_schedule_now(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    schedule_repo: State<'_, ApplicationScheduleRepository>,
    schedule_id: Uuid,
) -> AppResult<()> {
    services::run_schedule_now(&repo, &server_repo, &sessions, &schedule_repo, schedule_id).await
}

/// Installs cron on a Node that has none - the button a `cron_missing` error offers.
#[tauri::command]
pub async fn install_cron(server_repo: State<'_, ServerRepository>, sessions: State<'_, SshSessionManager>, server_id: Uuid) -> AppResult<()> {
    services::schedule_service::install_cron(&server_repo, &sessions, server_id).await
}

/// Sets or clears an Application's disk limit, enforced by the Node - see
/// `services::schedule_service::set_disk_limit`.
#[tauri::command]
pub async fn set_application_disk_limit(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    schedule_repo: State<'_, ApplicationScheduleRepository>,
    id: Uuid,
    limit_mb: Option<u64>,
) -> AppResult<crate::models::ApplicationDetail> {
    services::schedule_service::set_disk_limit(&repo, &server_repo, &sessions, &schedule_repo, id, limit_mb).await
}

/// What the Node's last disk check found, or null before the first one.
#[tauri::command]
pub async fn get_application_disk_usage(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<Option<crate::models::DiskUsage>> {
    services::schedule_service::disk_usage(&repo, &server_repo, &sessions, id).await
}
