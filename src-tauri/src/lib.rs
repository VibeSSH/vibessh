// This codebase's doc comments lean heavily on multi-line prose under a
// bullet, which `doc_lazy_continuation` wants indented to keep rustdoc's
// rendering exact. The rendering difference is cosmetic, the comments are
// the main way design decisions are recorded here, and reflowing 49 of them
// would bury real changes in whitespace diffs. Allowed deliberately, at the
// crate root, so the choice is visible rather than implicit.
#![allow(clippy::doc_lazy_continuation)]

// `pub` (not just `mod`) so the integration test in `tests/agent_client.rs`
// can drive it directly - everything else here only needs in-crate tests
// (pairing_commands' own test lives inside that module, see its file for why).
pub mod agent_client;
// The Vibe AI assistant's mechanism half - provider transport,
// context collection, redaction and the system prompt. The
// orchestration that ties them together is `services::ai_service`,
// matching the split every other subsystem here already uses
// (`runtime`/`firewall`/`network` are mechanism; `services` decides).
// `pub` so `tests/ai_assistant.rs` can drive the sanitizer and the
// service against a mock provider without going through Tauri.
pub mod ai;
mod blueprints;
pub mod cloud_client;
mod commands;
// `pub` for the same reason as `files`/`runtime` above - the file-operation
// helper's install/provisioning logic (`files::sudo_user`) needs this, and a
// future real-server integration test would too.
pub mod dedicated_user;
// `pub` for the integration tests: `tests/concurrency.rs` asserts that a
// loser in a port race gets `PortInUse` naming the winner rather than a bare
// storage error, and that distinction is the entire point of the transaction
// behaviour it is testing. There is nothing here a consumer of this crate
// would use - the visibility exists so the property can be asserted.
pub mod errors;
// `pub` for the same reason as `runtime`/`ssh` above - a real-server
// integration test (`tests/firewall_ufw.rs`) drives `firewall::ufw::UfwProvider`
// directly against a live, real ufw installation.
pub mod firewall;
// `pub` for the same reason as `agent_client`/`ssh` above - a real-server
// integration test (`tests/application_files_sftp.rs`) drives
// `SftpApplicationFileProvider` directly against a live SSH host.
pub mod files;
// `pub` for the same reason as `agent_client`/`files`/`ssh` below - the
// `tests/docker_runtime.rs` real-server test needs to build a real
// `Application`/`ApplicationPort` to drive `runtime::docker` with.
pub mod models;
// Turning arbitrary user text into names DNS and Docker will accept - the
// same conversion was written twice before this existed.
mod naming;
// Where VibeSSH may put working files on a managed Node. Shared by
// `network::wireguard` and `runtime::docker`, both of which used to reach
// for a predictable `/tmp` path instead - see the module's own doc
// comment for the two vulnerabilities that caused.
mod node_paths;
// `pub` for the same reason as `firewall`/`runtime` above - a real-server
// integration test (`tests/vibe_network.rs`) drives
// `network::wireguard` directly against a real WireGuard installation.
pub mod network;
// `pub` for the same reason as `agent_client`/`files`/`ssh` above - a
// real-server integration test (`tests/docker_runtime.rs`) drives
// `runtime::docker::DockerRuntime` directly against a live Docker daemon.
pub mod runtime;
// SigV4 client for the S3-compatible backup destination
// (`services::application_backup_service`) - see `s3::mod`'s own doc
// comment for why this is hand-rolled rather than `aws-sdk-s3`.
mod s3;
// `pub` for the same reason as `runtime`/`network` above - a real-server
// integration test (`tests/vibe_network.rs`) drives the service-layer
// orchestration (`join_node`, `sync_dns`, ...) directly, not just the
// mechanism underneath it.
pub mod services;
// `pub` for the same reason as `agent_client` above - `tests/ssh_client.rs`
// drives `ssh::connect` directly against a local mock SSH server.
pub mod ssh;
// `pub` for the same reason as `services` above - `tests/vibe_network.rs`
// needs a real `SshSessionManager`.
pub mod state;
// `pub` for the same reason as `services`/`state` above - `tests/vibe_network.rs`
// opens real repositories directly against a temp SQLite file.
pub mod storage;
mod transport;

use blueprints::BlueprintRegistry;
use runtime::local_process::LocalProcessManager;
use state::{AppState, BackupDestinationState, CloudState, DnsSuffixState, PairingSession, PortForwardManager, SshSessionManager, TerminalSessionManager};
use std::sync::Arc;
use storage::application_repository::ApplicationRepository;
use storage::server_repository::ServerRepository;
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new("VibeSSH", env!("CARGO_PKG_VERSION")))
        .manage(PairingSession::new())
        .manage(SshSessionManager::new())
        .manage(TerminalSessionManager::new())
        .manage(PortForwardManager::new())
        .manage(state::AgentSessionManager::new())
        .manage(state::FileTransferManager::new())
        .manage(state::AiTurnManager::new())
        .manage(state::LogFollowManager::new())
        .manage(state::MigrationLockManager::new())
        // Arc-wrapped (unlike the two managers above) because
        // `LocalProcessRuntime` needs an owned, cheaply-cloneable handle to
        // construct itself with, not just a borrow scoped to one command -
        // see runtime::local_process's own doc comment.
        .manage(Arc::new(LocalProcessManager::new()))
        // Read-only after construction (no interior mutability needed) -
        // see blueprints::mod's own doc comment for why this is the whole
        // persistence story for built-in blueprints in this phase.
        .manage(BlueprintRegistry::with_builtins())
        // The Vibe AI assistant's documentation corpus, split into
        // sections once at startup. Read-only afterwards, same as the
        // blueprint registry above - the documents are compiled into
        // the binary, so there is nothing to reload.
        .manage(ai::knowledge::KeywordKnowledgeService::with_builtin_docs())
        .setup(|app| {
            // Needs the resolved app data dir, which only exists once the
            // app is running - can't be built alongside the other .manage()
            // calls above.
            let db_path = app.path().app_data_dir()?.join("servers.sqlite3");
            app.manage(ServerRepository::open(&db_path)?);
            // Same physical file as ServerRepository above (Applications'
            // server_id is a real foreign key into servers, which only
            // means something within one SQLite file) - see
            // ApplicationRepository::open's own doc comment for why this
            // is a second independent Connection rather than a shared one.
            app.manage(ApplicationRepository::open(&db_path)?);
            // Same physical file again - Application Databases (Phase 11)
            // foreign keys into both `applications` and `servers`.
            app.manage(storage::database_repository::DatabaseRepository::open(&db_path)?);
            // Same physical file again - Etap M3's desired/applied state
            // revisioning foreign-keys into `servers`.
            app.manage(storage::node_state_repository::NodeStateRepository::open(&db_path)?);
            // Same physical file again - Etap M4's Vibe Network membership
            // (IPAM) and Private DNS records both foreign-key into
            // `servers`/`applications`.
            app.manage(storage::node_network_repository::NodeNetworkRepository::open(&db_path)?);
            app.manage(storage::dns_repository::DnsRepository::open(&db_path)?);
            // Same physical file again - Application backups foreign-key
            // into `applications`.
            app.manage(storage::application_backup_repository::ApplicationBackupRepository::open(&db_path)?);
            // Same physical file again - manual firewall rules foreign-key
            // into `servers`.
            app.manage(storage::firewall_rule_repository::FirewallRuleRepository::open(&db_path)?);
            // Same physical file again - one row per registry host, not
            // scoped to any particular server/application.
            app.manage(storage::registry_credential_repository::RegistryCredentialRepository::open(&db_path)?);

            let config_dir = app.path().app_config_dir()?;
            let backend_url = storage::cloud_config::load_backend_url(&config_dir)?;
            app.manage(CloudState::new(backend_url));

            app.manage(storage::log_capture::LogCaptureStore::new(config_dir.join("logs"))?);

            let backup_destination = storage::backup_destination_config::load_backup_destination(&config_dir)?;
            app.manage(BackupDestinationState::new(backup_destination));

            let dns_suffix = storage::dns_config::load_dns_suffix(&config_dir)?;
            app.manage(DnsSuffixState::new(dns_suffix));

            // Silently turns a keyring-stored refresh token from a previous
            // run back into a live session, if there is one - see
            // services::cloud_try_restore_session's own doc comment for why
            // this is fire-and-forget rather than something setup() waits on
            // or surfaces an error for. Spawned *after* app.manage() above,
            // not before - the spawned task looks the state up by type, so
            // it must already be registered before this can run.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<CloudState>();
                services::cloud_try_restore_session(&state).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_commands::get_app_info,
            commands::ai_commands::get_ai_config,
            commands::ai_commands::set_ai_config,
            commands::ai_commands::test_ai_connection,
            commands::ai_commands::get_ai_quota,
            commands::ai_commands::preview_ai_context,
            commands::ai_commands::send_ai_turn,
            commands::ai_commands::stop_ai_turn,
            commands::pairing_commands::generate_pairing_code,
            commands::pairing_commands::pairing_code_ttl_seconds,
            commands::pairing_commands::start_agent_pairing,
            commands::pairing_commands::cancel_agent_pairing,
            commands::application_commands::list_applications,
            commands::application_commands::list_paper_versions,
            commands::application_commands::list_velocity_versions,
            commands::application_commands::list_waterfall_versions,
            commands::application_commands::list_purpur_versions,
            commands::application_commands::list_application_ports,
            commands::application_commands::add_application_port,
            commands::application_commands::update_application_port,
            commands::application_commands::remove_application_port,
            commands::application_commands::sync_application_node_firewall,
            commands::application_commands::list_application_links,
            commands::application_commands::connect_applications,
            commands::application_commands::disconnect_applications,
            commands::application_commands::get_application,
            commands::application_commands::list_blueprints,
            commands::application_commands::create_application,
            commands::application_commands::update_application_config,
            commands::application_commands::follow_application_logs,
            commands::application_commands::stop_following_application_logs,
            commands::application_backup_commands::list_application_backups,
            commands::application_backup_commands::create_application_backup,
            commands::application_backup_commands::delete_application_backup,
            commands::application_backup_commands::restore_application_backup,
            commands::application_backup_commands::get_application_backup_schedule,
            commands::application_backup_commands::set_application_backup_schedule,
            commands::application_backup_commands::run_due_application_backups,
            commands::application_backup_commands::get_backup_destination,
            commands::application_backup_commands::set_backup_destination,
            commands::application_backup_commands::test_backup_destination,
            commands::application_commands::delete_application,
            commands::application_commands::start_application,
            commands::application_commands::stop_application,
            commands::application_commands::restart_application,
            commands::application_commands::recreate_application,
            commands::application_commands::kill_application,
            commands::application_commands::refresh_application_status,
            commands::application_commands::get_application_resource_usage,
            commands::application_commands::get_application_logs,
            commands::application_commands::write_application_console,
            commands::application_commands::get_application_health,
            commands::application_commands::set_application_health_check,
            commands::application_commands::set_application_resource_limits,
            commands::application_commands::set_application_environment,
            commands::application_commands::set_application_image,
            commands::application_commands::pull_application_image,
            commands::application_commands::list_registry_credentials,
            commands::application_commands::set_registry_credential,
            commands::application_commands::remove_registry_credential,
            commands::application_commands::detect_java_installations,
            commands::migration_commands::migrate_application,
            commands::database_commands::list_database_hosts,
            commands::database_commands::create_database_host,
            commands::database_commands::delete_database_host,
            commands::database_commands::set_database_host_phpmyadmin,
            commands::database_commands::install_database_server,
            commands::database_commands::list_application_databases,
            commands::database_commands::create_application_database,
            commands::database_commands::delete_application_database,
            commands::database_commands::reveal_application_database_password,
            commands::database_commands::reset_application_database_password,
            commands::application_file_commands::list_application_files,
            commands::application_file_commands::get_application_file_metadata,
            commands::application_file_commands::read_application_file,
            commands::application_file_commands::write_application_file,
            commands::application_file_commands::save_application_file,
            commands::application_file_commands::create_application_directory,
            commands::application_file_commands::delete_application_file,
            commands::application_file_commands::rename_application_file,
            commands::application_file_commands::copy_application_file,
            commands::application_file_commands::set_application_file_permissions,
            commands::application_file_commands::download_application_file,
            commands::application_file_commands::upload_application_file,
            commands::application_file_commands::cancel_application_file_transfer,
            commands::application_file_commands::extract_application_archive,
            commands::application_file_commands::list_application_file_history,
            commands::application_file_commands::restore_application_file_history,
            commands::application_file_commands::clear_application_file_history,
            commands::database_commands::get_phpmyadmin_url,
            commands::server_commands::create_server,
            commands::server_commands::update_server,
            commands::server_commands::set_server_icon,
            commands::server_commands::delete_server,
            commands::server_commands::get_server,
            commands::server_commands::list_servers,
            commands::server_commands::upsert_agent_server,
            commands::server_commands::upgrade_server_to_agent,
            commands::server_commands::probe_server_capabilities,
            commands::server_commands::install_docker,
            commands::server_commands::install_wireguard,
            commands::server_commands::install_ufw,
            commands::server_commands::preview_server_firewall_rules,
            commands::server_commands::enable_server_firewall,
            commands::server_commands::sync_node_firewall,
            commands::server_commands::get_node_firewall_overview,
            commands::server_commands::add_firewall_custom_rule,
            commands::server_commands::remove_firewall_custom_rule,
            commands::agent_session_commands::start_agent_session,
            commands::agent_session_commands::get_node_sync_status,
            commands::agent_session_commands::reconcile_agent_node,
            commands::network_commands::list_network_members,
            commands::network_commands::join_vibe_network,
            commands::network_commands::leave_vibe_network,
            commands::network_commands::get_vibe_network_status,
            commands::network_commands::list_node_endpoints,
            commands::network_commands::list_dns_records,
            commands::network_commands::create_dns_alias,
            commands::network_commands::update_dns_alias,
            commands::network_commands::delete_dns_alias,
            commands::network_commands::sync_vibe_dns,
            commands::network_commands::verify_dns_alias,
            commands::network_commands::resolve_dns_view,
            commands::network_commands::sync_vibe_network,
            commands::network_commands::get_dns_suffix,
            commands::network_commands::set_dns_suffix,
            commands::ssh_commands::test_ssh_connection,
            commands::ssh_commands::execute_ssh_command,
            commands::ssh_commands::ping_server,
            commands::terminal_commands::open_terminal,
            commands::terminal_commands::write_to_terminal,
            commands::terminal_commands::resize_terminal,
            commands::terminal_commands::close_terminal,
            commands::port_forward_commands::start_port_forward,
            commands::port_forward_commands::list_port_forwards,
            commands::port_forward_commands::stop_port_forward,
            commands::file_commands::list_remote_directory,
            commands::file_commands::create_remote_directory,
            commands::file_commands::read_remote_file,
            commands::file_commands::write_remote_file,
            commands::file_commands::download_remote_file,
            commands::file_commands::upload_remote_file,
            commands::file_commands::rename_remote_path,
            commands::file_commands::delete_remote_path,
            commands::file_commands::set_remote_permissions,
            commands::file_commands::extract_remote_archive,
            commands::file_commands::compress_remote_paths,
            commands::monitor_commands::get_server_metrics,
            commands::monitor_commands::list_server_processes,
            commands::actions_commands::list_server_services,
            commands::actions_commands::restart_server_service,
            commands::actions_commands::start_server_service,
            commands::actions_commands::stop_server_service,
            commands::actions_commands::enable_server_service,
            commands::actions_commands::disable_server_service,
            commands::actions_commands::list_server_containers,
            commands::actions_commands::restart_server_container,
            commands::actions_commands::start_server_container,
            commands::actions_commands::stop_server_container,
            commands::actions_commands::remove_server_container,
            commands::actions_commands::get_server_container_logs,
            commands::cloud_commands::cloud_register,
            commands::cloud_commands::cloud_login,
            commands::cloud_commands::cloud_logout,
            commands::cloud_commands::cloud_session_info,
            commands::cloud_commands::cloud_get_backend_url,
            commands::cloud_commands::cloud_set_backend_url,
            commands::cloud_commands::cloud_list_teams,
            commands::cloud_commands::cloud_create_team,
            commands::cloud_commands::cloud_list_members,
            commands::cloud_commands::cloud_get_team,
            commands::cloud_commands::cloud_list_permissions,
            commands::cloud_commands::cloud_list_roles,
            commands::cloud_commands::cloud_create_role,
            commands::cloud_commands::cloud_update_role,
            commands::cloud_commands::cloud_delete_role,
            commands::cloud_commands::cloud_list_member_roles,
            commands::cloud_commands::cloud_assign_role,
            commands::cloud_commands::cloud_unassign_role,
            commands::cloud_commands::cloud_list_servers,
            commands::cloud_commands::cloud_create_server,
            commands::cloud_commands::cloud_delete_server,
            commands::cloud_commands::cloud_my_permissions,
            commands::cloud_commands::cloud_remove_member,
            commands::cloud_commands::cloud_delete_team,
            commands::cloud_commands::cloud_list_invitations,
            commands::cloud_commands::cloud_create_invitation,
            commands::cloud_commands::cloud_revoke_invitation,
            commands::cloud_commands::cloud_accept_invitation,
            commands::cloud_commands::cloud_decline_invitation,
            commands::cloud_commands::cloud_list_audit_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeSSH");
}
