//! Tauri command bridge for the Vibe AI assistant.
//!
//! Each turn gets its own set of events - `ai://{turn_id}/delta`,
//! `ai://{turn_id}/done`, `ai://{turn_id}/error` and `ai://{turn_id}/phase` -
//! rather than one shared name, the same shape `terminal_commands` uses and
//! for the same reason: the panel can be left mid-answer while another turn
//! is started elsewhere, and two answers interleaving into one event name
//! would be unreadable.
//!
//! `phase` exists because a Diagnose turn does two very different things
//! before a single token arrives: it reads the Node over SSH, then it waits
//! on a model. Both used to render as "Thinking...", so a slow or
//! unresponsive Node was indistinguishable from a slow model - and the first
//! time that happened in practice, it was read as the model being slow when
//! the request had not reached one. The probes are bounded now
//! (`ai::context::PROBE_TIMEOUT`); this says which of the two is happening.
//!
//! **The provider is only ever reached from here inward.** The frontend can
//! ask for a turn; it cannot supply an endpoint, a key, a system prompt or a
//! model. Those come from the stored configuration on this side of the
//! bridge, which is what makes the redaction and the standing instruction
//! something the UI cannot route around.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai::knowledge::KeywordKnowledgeService;
use crate::errors::{AppError, AppResult};
use crate::models::{AiConfigView, AiContextBundle, AiContextRef, AiMode, AiTurnRequest, SetAiConfigInput};
use crate::runtime::local_process::LocalProcessManager;
use crate::services;
use crate::state::{AiTurnManager, CloudState, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// Emits, and says so in the log when it could not.
///
/// `let _ = app.emit(...)` is what the older command modules do, and
/// `AGENTS.md` §3 is explicit that discarding an error the user can see is a
/// bug. A dropped delta is a word missing from an answer; a dropped `done`
/// leaves the panel spinning forever. Neither should be invisible.
fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    if let Err(err) = app.emit(event, payload) {
        log::warn!("couldn't deliver {event} to the UI: {err}");
    }
}

fn config_dir(app: &AppHandle) -> AppResult<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .map_err(|err| AppError::Storage(format!("couldn't locate the config directory: {err}")))
}

#[tauri::command]
pub fn get_ai_config(app: AppHandle) -> AppResult<AiConfigView> {
    services::ai_config_view(&config_dir(&app)?)
}

#[tauri::command]
pub fn set_ai_config(app: AppHandle, input: SetAiConfigInput) -> AppResult<AiConfigView> {
    services::set_ai_config(&config_dir(&app)?, input)
}

#[tauri::command]
pub async fn test_ai_connection(app: AppHandle, cloud: State<'_, CloudState>) -> AppResult<()> {
    let dir = config_dir(&app)?;
    services::test_ai_connection(&dir, &cloud).await
}

/// How much of the included model's daily allowance is left, or `null`
/// when this install uses a personal API key and has no allowance.
#[tauri::command]
pub async fn get_ai_quota(app: AppHandle, cloud: State<'_, CloudState>) -> AppResult<Option<crate::models::CloudAiQuota>> {
    let dir = config_dir(&app)?;
    services::ai_quota(&dir, &cloud).await
}

/// The exact text that a Diagnose turn would send.
///
/// Its own command so the panel can show the user what is about to leave
/// their machine *before* it does - `AGENTS.md` §6: a boundary the interface
/// does not show is not a boundary. This calls the same builder the turn
/// itself calls, so the preview cannot drift from the payload.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn preview_ai_context(
    applications: State<'_, ApplicationRepository>,
    servers: State<'_, ServerRepository>,
    networks: State<'_, NodeNetworkRepository>,
    firewall_rules: State<'_, FirewallRuleRepository>,
    ssh_sessions: State<'_, SshSessionManager>,
    local_processes: State<'_, Arc<LocalProcessManager>>,
    log_capture: State<'_, LogCaptureStore>,
    mode: AiMode,
    context: Option<AiContextRef>,
) -> AppResult<Option<AiContextBundle>> {
    Ok(services::build_ai_context(
        &applications,
        &servers,
        &networks,
        &firewall_rules,
        &ssh_sessions,
        &local_processes,
        &log_capture,
        mode,
        context,
    )
    .await)
}

/// Starts one turn. Returns as soon as the work is spawned; the answer
/// arrives on the event triple for `turn_id`.
///
/// Returning early rather than awaiting the answer is what makes Stop
/// possible at all: the command's own future would otherwise hold the task,
/// and there would be nothing for `AiTurnManager` to abort.
#[tauri::command]
pub async fn send_ai_turn(
    app: AppHandle,
    turns: State<'_, AiTurnManager>,
    cloud: State<'_, CloudState>,
    turn_id: String,
    request: AiTurnRequest,
) -> AppResult<()> {
    // Resolved before spawning, so a misconfigured assistant fails the
    // command itself - the UI gets a real error to render instead of an
    // empty answer that ends in an error event.
    let dir = config_dir(&app)?;
    let (config, provider) = services::resolve_ai_provider(&dir, &cloud).await?;

    let phase_event = format!("ai://{turn_id}/phase");
    let delta_event = format!("ai://{turn_id}/delta");
    let done_event = format!("ai://{turn_id}/done");
    let error_event = format!("ai://{turn_id}/error");

    let task = tokio::spawn({
        let app = app.clone();
        let turn_id = turn_id.clone();
        async move {
            // Announced before the first SSH round trip rather than after, so
            // the panel is never silently in a phase it has not been told
            // about.
            emit(&app, &phase_event, "collecting");
            let context = services::build_ai_context(
                &app.state::<ApplicationRepository>(),
                &app.state::<ServerRepository>(),
                &app.state::<NodeNetworkRepository>(),
                &app.state::<FirewallRuleRepository>(),
                &app.state::<SshSessionManager>(),
                &app.state::<Arc<LocalProcessManager>>(),
                &app.state::<LogCaptureStore>(),
                request.mode,
                request.context,
            )
            .await;

            emit(&app, &phase_event, "waiting");

            let knowledge = app.state::<KeywordKnowledgeService>();
            let sink = {
                let app = app.clone();
                let delta_event = delta_event.clone();
                move |delta: String| emit(&app, &delta_event, delta)
            };

            let outcome =
                services::run_ai_turn(provider.as_ref(), knowledge.inner(), &config.model, context.as_ref(), &request, &sink).await;

            match outcome {
                Ok(answer) => emit(&app, &done_event, answer),
                // The serialized `AppError` carries `code` and `params`, so
                // the panel renders a translated sentence rather than the
                // English fallback - and never the provider's own body,
                // which never reaches this point (see `ai::openai_compatible`).
                Err(err) => {
                    log::warn!("the AI turn failed: {err}");
                    match serde_json::to_value(&err) {
                        Ok(payload) => emit(&app, &error_event, payload),
                        Err(encode) => {
                            log::error!("couldn't encode the AI error for the UI: {encode}");
                            emit(&app, &error_event, serde_json::json!({ "code": "internal", "message": err.to_string() }));
                        }
                    }
                }
            }

            app.state::<AiTurnManager>().clear(&turn_id).await;
        }
    });

    turns.register(turn_id, task.abort_handle()).await;
    Ok(())
}

/// Stops a running turn. `false` means it had already finished, which is a
/// race the user should never be shown as a failure.
#[tauri::command]
pub async fn stop_ai_turn(turns: State<'_, AiTurnManager>, turn_id: String) -> AppResult<bool> {
    Ok(turns.cancel(&turn_id).await)
}
