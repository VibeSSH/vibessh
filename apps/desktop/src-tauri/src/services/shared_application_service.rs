//! Putting the Applications teammates shared with this account on this
//! install - the member's half of per-application permissions.
//!
//! The owner's access sync gave this account rules on the Node naming each
//! Application it may act on (`member_sudoers`). This is what makes those
//! rules reachable from the app rather than only from a terminal: every
//! shared Application on a Node this install connects to *as its own member
//! account* gets a local record under the owner's id, marked as shared
//! (migration 20), and from then on the ordinary screens open it - through
//! `runtime::member` and the file helper, never through the owner's paths.
//!
//! **Only as the member account.** A Node this install reaches as some other
//! account - its owner's, root - is not one where those rules apply, and the
//! owner already has the real record. So the local server has to match the
//! team server's address *and* log in as this person's `vibessh-m-...`.
//!
//! **What they may do** is the union of their role's narrow permissions,
//! which apply to every Application, and what was ticked for them on this
//! one. Anything else a role grants is not something this path can do.
//!
//! **The backend is the list.** An Application no longer shared with this
//! account, or in a team it has left, loses its local record at the next
//! sync - the row only; the container is the owner's and is not touched.
//!
//! **The server entries are made here too.** Every Node a team shares gets
//! an entry on this install logging in as the member account with this
//! device's key. They used to come only from a button in Teams -> Servers,
//! so being added to a team showed nothing until somebody found it, and a
//! member who had instead added the Node by hand as `root` - which the
//! team list, showing the owner's login, all but suggested - got a password
//! prompt for an account that was never theirs. An entry this made is
//! removed when no team shares that Node with this account any more.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{CloudMemberAccess, RuntimeType};
use crate::state::CloudState;
use crate::storage::application_repository::{AdoptOutcome, AdoptedApplication, ApplicationRepository};
use crate::storage::server_repository::ServerRepository;

/// The permissions that can act on one Application - the backend's
/// `APPLICATION_SCOPED`. A role's other permissions do not reach this path.
const APPLICATION_SCOPED: &[&str] = &["applications.lifecycle", "applications.console", "applications.files.read", "applications.files.write"];

/// What one sync did, for the list screen to refresh on.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSyncReport {
    pub added: usize,
    pub refreshed: usize,
    pub removed: usize,
    /// Team Nodes that got an entry on this install.
    pub servers_added: usize,
    /// Entries this sync made earlier, for Nodes no team shares any more.
    pub servers_removed: usize,
}

/// Whether a local server entry is one this sync made: the member account
/// (a name only the backend hands out) and this device's own key. A Node
/// somebody added by hand, under any other login or key, is theirs and is
/// never touched.
fn is_team_entry(username: &str, private_key_path: Option<&str>, device_key_path: &str) -> bool {
    username.starts_with("vibessh-m-") && private_key_path == Some(device_key_path)
}

fn runtime_type_from_projection(name: &str) -> Option<RuntimeType> {
    match name {
        "docker" => Some(RuntimeType::Docker),
        "systemd" => Some(RuntimeType::Systemd),
        _ => None,
    }
}

/// What this account may do with one Application: its role's narrow
/// permissions and what was granted on the Application itself, in the
/// catalog's order.
fn effective_permissions(role: &[String], granted: &[String]) -> Vec<String> {
    APPLICATION_SCOPED
        .iter()
        .filter(|key| role.iter().chain(granted).any(|held| held == *key))
        .map(|key| (*key).to_string())
        .collect()
}

pub async fn sync_shared_applications(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    cloud: &CloudState,
    device_key_path: &str,
) -> AppResult<SharedSyncReport> {
    let mut report = SharedSyncReport::default();
    // Signed out is not "nothing is shared any more": the records stay until
    // a signed-in sync says otherwise.
    let Some(session) = crate::services::cloud_service::session_info(cloud).await else {
        return Ok(report);
    };
    let me = session.user.id;
    let teams = crate::services::cloud_service::list_teams(cloud).await?;
    let mut local_servers = server_repo.list()?;
    // Every Node a team shares with this account, as (host, port) - what a
    // team entry is kept for.
    let mut shared_nodes: HashSet<(String, u16)> = HashSet::new();

    let mut kept: HashSet<Uuid> = HashSet::new();
    // Teams this sync could read in full. Only their records are pruned: a
    // team whose listing failed says nothing about what is still shared.
    let mut settled_teams: HashSet<Uuid> = teams.iter().map(|team| team.id).collect();

    for team in &teams {
        let listed = async {
            let servers = crate::services::cloud_service::list_servers(cloud, team.id).await?;
            let applications = crate::services::cloud_service::list_team_applications(cloud, team.id).await?;
            let access = crate::services::cloud_service::list_team_access(cloud, team.id).await?;
            AppResult::Ok((servers, applications, access))
        }
        .await;
        let (servers, applications, access) = match listed {
            Ok(listed) => listed,
            Err(err) => {
                log::warn!("couldn't read team {} for shared applications, leaving its records as they are: {err}", team.id);
                settled_teams.remove(&team.id);
                continue;
            }
        };
        let Some(mine): Option<&CloudMemberAccess> = access.iter().find(|member| member.user_id == me) else { continue };

        for team_server in &servers {
            let Ok(port) = u16::try_from(team_server.ssh_port) else { continue };
            if team_server.host.trim().is_empty() {
                continue;
            }
            shared_nodes.insert((team_server.host.clone(), port));
            // Not for the team's owner, who reaches their own Nodes as their
            // administrator. Anybody else gets the member entry even if they
            // already have one of their own for the address: that one may be
            // exactly the hand-made `root` entry that never worked.
            if team.owner_id == me {
                continue;
            }
            let exists = local_servers.iter().any(|local| local.host == team_server.host && local.ssh_port == port && local.username == mine.node_username);
            if exists {
                continue;
            }
            let created = crate::services::server_service::create_server(
                server_repo,
                crate::models::ServerInput {
                    name: team_server.name.clone(),
                    host: team_server.host.clone(),
                    ssh_port: port,
                    username: mine.node_username.clone(),
                    authentication_type: crate::models::AuthenticationType::PrivateKey,
                    private_key_path: Some(device_key_path.to_string()),
                    group_id: None,
                    password: None,
                    key_passphrase: None,
                },
            )?;
            local_servers.push(created);
            report.servers_added += 1;
        }
        let granted: HashMap<Uuid, &Vec<String>> = mine.applications.iter().map(|grant| (grant.local_id, &grant.permissions)).collect();

        for application in &applications {
            let Some(runtime_type) = runtime_type_from_projection(&application.runtime_type) else { continue };
            let Some(team_server) = application.team_server_id.and_then(|id| servers.iter().find(|server| server.id == id)) else { continue };
            // This install's entry for that Node, logging in as this person's
            // own member account - see the module comment.
            let Some(local) = local_servers.iter().find(|local| {
                local.host == team_server.host && i32::from(local.ssh_port) == team_server.ssh_port && local.username == mine.node_username
            }) else {
                continue;
            };
            let permissions = effective_permissions(&mine.permissions, granted.get(&application.local_id).map(|p| p.as_slice()).unwrap_or(&[]));
            let adopted = AdoptedApplication {
                id: application.local_id,
                server_id: local.id,
                name: application.name.clone(),
                blueprint_id: application.blueprint_id.clone(),
                runtime_type,
                // Without a trailing slash, the form the Node's file rule
                // names it in.
                working_directory: application.working_directory.trim_end_matches('/').to_string(),
                team_id: team.id,
                permissions,
            };
            match app_repo.adopt_shared(&adopted)? {
                AdoptOutcome::Added => report.added += 1,
                AdoptOutcome::Refreshed => report.refreshed += 1,
                AdoptOutcome::OwnApplication => continue,
            }
            kept.insert(application.local_id);
        }
    }

    for (id, access) in app_repo.list_shared()? {
        let team_gone = !teams.iter().any(|team| team.id == access.team_id);
        if !kept.contains(&id) && (team_gone || settled_teams.contains(&access.team_id)) {
            app_repo.forget_shared(id)?;
            report.removed += 1;
        }
    }

    // Only when every team was read: a team whose listing failed says
    // nothing about which Nodes are still shared.
    if settled_teams.len() == teams.len() {
        for local in server_repo.list()? {
            if !is_team_entry(&local.username, local.private_key_path.as_deref(), device_key_path) || shared_nodes.contains(&(local.host.clone(), local.ssh_port)) {
                continue;
            }
            match crate::services::server_service::delete_server(server_repo, app_repo, local.id) {
                Ok(()) => report.servers_removed += 1,
                // Something of this account's own is on it. Kept, and said.
                Err(err) => log::warn!("{} is no longer shared with this account but couldn't be removed: {err}", local.name),
            }
        }
    }
    Ok(report)
}

/// Which of this install's Applications are shared ones, and what each
/// allows - for the list screen, which reads rows rather than details.
pub fn list_shared_access(app_repo: &ApplicationRepository) -> AppResult<Vec<SharedApplicationAccess>> {
    Ok(app_repo
        .list_shared()?
        .into_iter()
        .map(|(application_id, access)| SharedApplicationAccess { application_id, team_id: access.team_id, permissions: access.permissions })
        .collect())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedApplicationAccess {
    pub application_id: Uuid,
    pub team_id: Uuid,
    pub permissions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    /// A role's narrow permissions apply everywhere, a grant on one
    /// Application adds to them, and nothing outside the four reaches this
    /// path whatever a role says.
    #[test]
    fn a_role_and_a_grant_add_up_to_the_four_that_act_on_one_application() {
        assert_eq!(effective_permissions(&[], &keys(&["applications.lifecycle"])), keys(&["applications.lifecycle"]));
        assert_eq!(
            effective_permissions(&keys(&["applications.files.read", "node.terminal", "team.view"]), &keys(&["applications.console"])),
            keys(&["applications.console", "applications.files.read"])
        );
        assert!(effective_permissions(&keys(&["applications.delete", "applications.config"]), &[]).is_empty());
    }

    /// Removing a Node that stops being shared must never reach one the
    /// person added themselves - as root, or as the member account with a
    /// key of their own choosing.
    #[test]
    fn only_an_entry_this_sync_made_counts_as_one() {
        let device = "/cfg/device_key";
        assert!(is_team_entry("vibessh-m-0123456789ab", Some(device), device));
        assert!(!is_team_entry("root", Some(device), device));
        assert!(!is_team_entry("vibessh-m-0123456789ab", Some("/home/me/.ssh/id_ed25519"), device));
        assert!(!is_team_entry("vibessh-m-0123456789ab", None, device));
    }

    #[test]
    fn only_docker_and_systemd_are_adopted() {
        assert_eq!(runtime_type_from_projection("docker"), Some(RuntimeType::Docker));
        assert_eq!(runtime_type_from_projection("systemd"), Some(RuntimeType::Systemd));
        assert_eq!(runtime_type_from_projection("remoteprocess"), None);
    }
}
