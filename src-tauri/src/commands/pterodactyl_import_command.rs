//! Running an agreed Pterodactyl migration.
//!
//! **The plan is rebuilt here rather than accepted from the interface.** The
//! frontend sends which servers to import, not what to import them as. If it
//! sent the plan itself, anything running in the webview could ask this
//! command to create an Application with an image and a startup command of
//! its choosing on any Node - the plan would have become an instruction
//! channel. Rebuilding it from the panel costs one round of requests and
//! removes that entirely; it also means an import always acts on what the
//! panel says now rather than on what it said when the operator opened the
//! screen.
//!
//! Progress is emitted per server on `pterodactyl://import/progress`, the
//! same per-operation event shape `terminal_commands` and `ai_commands`
//! already use.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::{AppError, AppResult};
use crate::pterodactyl::mapping::PlanNote;
use crate::pterodactyl::PterodactylClient;
use crate::services::{self, PterodactylImportOutcome, PterodactylImportStep, PterodactylNodeOverride};
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// One import at a time, for the whole install.
///
/// Not a nicety: two runs of the same plan create every Application twice,
/// and the second run's copy would be reading files out from under the
/// first's. A disabled button in the interface is not enough on its own,
/// because the interface is not the only way this command can be reached.
static IMPORT_RUNNING: AtomicBool = AtomicBool::new(false);

/// Released on every exit path, including the error ones - a guard rather
/// than a `store(false)` at the end, which a `?` would skip.
struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        IMPORT_RUNNING.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    source_id: i64,
    name: String,
    step: PterodactylImportStep,
    /// How far through the selected servers this one is, so the interface
    /// can say "3 of 12" without counting events itself.
    index: usize,
    total: usize,
}

/// `target_server_id` is the Node to import onto. When absent, each server
/// goes to whichever Node its own Pterodactyl node was matched to - the
/// common case, and the one the plan already showed.
///
/// `target_database_host_id` is the VibeSSH database host the panel schemas
/// are loaded into. Absent means "do not move databases", which is a real
/// choice - a panel whose servers have none needs no answer here - and is
/// recorded as a warning on any server that did have them rather than
/// passed over in silence.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pterodactyl_import(
    app: AppHandle,
    base_url: String,
    source_ids: Vec<i64>,
    target_server_id: Option<Uuid>,
    target_database_host_id: Option<Uuid>,
    node_overrides: Option<Vec<PterodactylNodeOverride>>,
    applications: State<'_, ApplicationRepository>,
    registry: State<'_, BlueprintRegistry>,
    servers: State<'_, ServerRepository>,
    networks: State<'_, NodeNetworkRepository>,
    firewall_rules: State<'_, FirewallRuleRepository>,
    databases: State<'_, DatabaseRepository>,
    sessions: State<'_, SshSessionManager>,
    java_root: State<'_, crate::state::JavaRoot>,
) -> AppResult<Vec<PterodactylImportOutcome>> {
    if source_ids.is_empty() {
        return Err(AppError::InvalidInput("no servers were selected to import".to_string()));
    }
    if IMPORT_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::InvalidInput("a migration is already running".to_string()));
    }
    let _guard = RunningGuard;

    let key = credentials::load_pterodactyl_api_key()?
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| AppError::Unauthorized("no Pterodactyl API key is stored - connect to the panel again".to_string()))?;
    let client = PterodactylClient::new(&base_url, &key)?;
    let plan = services::build_pterodactyl_plan(&client, &servers, &applications, &node_overrides.unwrap_or_default()).await?;

    let selected: Vec<_> = plan.servers.iter().filter(|server| source_ids.contains(&server.source_id)).collect();
    let total = selected.len();
    let mut outcomes = Vec::with_capacity(total);

    for (index, planned) in selected.into_iter().enumerate() {
        let Some(target) = target_server_id.or(planned.source.matched_server_id) else {
            // Nowhere to put it, and the plan already said so. Recorded as a
            // failed outcome rather than skipped silently, so the summary
            // accounts for every server that was asked for.
            outcomes.push(PterodactylImportOutcome {
                source_id: planned.source_id,
                name: planned.name.clone(),
                application_id: None,
                files_copied: 0,
                databases_moved: Vec::new(),
                warnings: Vec::new(),
                failed: Some(PlanNote::with("nodeUnknown", &[("fqdn", &planned.source.fqdn)])),
            });
            continue;
        };

        let emitter = app.clone();
        let name = planned.name.clone();
        let source_id = planned.source_id;
        let on_step = move |step: PterodactylImportStep| {
            let _ = emitter.emit("pterodactyl://import/progress", ProgressEvent { source_id, name: name.clone(), step, index, total });
        };

        outcomes.push(
            services::import_pterodactyl_server(
                &applications,
                &registry,
                &servers,
                &networks,
                &firewall_rules,
                &databases,
                &sessions,
                &java_root.0,
                planned,
                target,
                target_database_host_id,
                &on_step,
            )
            .await,
        );
    }

    Ok(outcomes)
}
