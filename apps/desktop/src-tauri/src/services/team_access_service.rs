//! Giving a team's members their own account on a Node, and taking it away.
//!
//! This is the point where the two halves meet: the backend knows who is in
//! the team and which public keys their devices published, and this install
//! is the one that can reach the Node. Nothing secret crosses between them -
//! only public keys, which is the whole reason the design chose per-member
//! accounts. See `docs/planning/team-access-design.md`.
//!
//! **What a member can do once this has run.** What their role says, and no
//! more: `member_sudoers` turns their effective permissions into the sudo
//! rules their account carries. A role granting nothing privileged leaves an
//! account that cannot run `sudo` on that Node. A role granting something
//! that cannot be narrowed without lying about it - a shell, package
//! installation, creating containers - still gets the blanket rule, and the
//! interface says which permission decided that.
//!
//! **Why one sync rather than a grant button and a revoke button.** What a
//! Node should hold is stated entirely by the team: these members, with
//! these published keys, and these people no longer. Applying it as one
//! operation is what makes the two halves agree - and it is why removing a
//! device, or removing a member, actually reaches the machine at all.
//! `authorized_keys` is written whole, so a key that is no longer published
//! disappears on the next sync without anybody having to ask for that
//! separately. The same shape `firewall_service` uses: describe the desired
//! state, apply it, report what really happened.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::member_account;
use crate::member_sudoers;
use crate::models::CloudMemberAccess;
use crate::services::ssh_service::get_or_connect;
use crate::state::{CloudState, SshSessionManager};
use crate::storage::server_repository::ServerRepository;

/// What happened for one member, so the interface can report per person
/// rather than one verdict for the whole team.
///
/// A team where four of five worked is not a success and is not a failure;
/// it is four and one, and the person reading needs to know which one.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberAccessResult {
    pub user_id: Uuid,
    pub email: String,
    pub node_username: String,
    /// `false` when this member has not opened VibeSSH on any device yet, so
    /// there is no key to install. Not an error - there is simply nothing to
    /// do until they do.
    pub has_key: bool,
    pub granted: bool,
    pub error: Option<String>,
    /// Per-application permissions that were asked for and could not be
    /// written - a folder a rule cannot name, a console a service does not
    /// have. Not a failed grant, and not something to leave unsaid.
    pub notes: Vec<member_sudoers::SkippedGrant>,
}

/// What happened to one person's access that the team has taken away.
///
/// `completed` is what the Node did, not what was asked for. A revocation
/// that failed here stays pending in the backend and stays pending on the
/// screen, because the person's key is still in a file on that machine and
/// saying otherwise would be the worst available mistake.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevocationResult {
    pub id: Uuid,
    pub email: String,
    pub node_username: String,
    pub completed: bool,
    pub error: Option<String>,
}

/// Everything one sync did to one Node.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAccessSync {
    pub members: Vec<MemberAccessResult>,
    pub revocations: Vec<RevocationResult>,
}

/// Makes one Node hold exactly the access the team describes.
///
/// Two halves, in this order. Every current member gets their account and
/// their currently published keys - written whole, so a device somebody
/// revoked stops being able to log in here even though nobody asked for that
/// specifically. Then every revocation the team is still owed on *this*
/// machine is carried out and reported back as done.
///
/// Members first, deliberately. If the connection dies part way, the half
/// that ran has given people access they are supposed to have; the other
/// order would leave a removed person's key in place and a report saying the
/// sync was interrupted, which reads like nothing happened.
///
/// Runs per member and keeps going after one fails, because the alternative
/// - stopping at the first problem - leaves the team in a state nobody can
/// describe: some people have access, some do not, and the error names only
/// the first.
pub async fn sync_team_access(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    cloud: &CloudState,
    server_id: Uuid,
    team_id: Uuid,
    team_server_id: Uuid,
) -> AppResult<NodeAccessSync> {
    let members = crate::services::cloud_service::list_team_access(cloud, team_id).await?;
    let connection = get_or_connect(server_repo, sessions, server_id).await?;

    // Each member's per-application rules, for the Applications on this Node
    // only - the grants cover every Node the team shares.
    let rules_by_member: Vec<Vec<member_sudoers::ApplicationRules>> =
        members.iter().map(|member| application_rules_on(member, team_server_id)).collect();

    // Installed before anybody is granted it, so a console rule never names
    // a writer that is not there yet.
    let console_writer = if members.iter().zip(&rules_by_member).any(|(member, rules)| needs_console_writer(member, rules)) {
        ensure_console_writer_installed(&connection).await
    } else {
        Ok(())
    };
    // The same for the firewall: a role's node.firewall reaches `iptables`
    // only through the DOCKER-USER helper, which has to be there first.
    let firewall_helper = if members.iter().any(needs_firewall_helper) {
        crate::firewall::docker_user::ensure_helper_installed(&connection).await
    } else {
        Ok(())
    };

    let mut results = Vec::new();
    for (member, rules) in members.into_iter().zip(rules_by_member) {
        let notes: Vec<member_sudoers::SkippedGrant> = rules.iter().flat_map(|rules| rules.skipped.iter().cloned()).collect();
        let has_key = !member.public_keys.is_empty();
        if !has_key {
            results.push(MemberAccessResult {
                user_id: member.user_id,
                email: member.email,
                node_username: member.node_username,
                has_key: false,
                granted: false,
                error: None,
                notes,
            });
            continue;
        }

        let outcome = match (&console_writer, &firewall_helper) {
            // Nothing is granted on top of a helper that failed to install:
            // its rule would name a missing file, and the rest of the rules
            // would read as a complete grant.
            (Err(err), _) if needs_console_writer(&member, &rules) => Err(AppError::Connection(err.to_string())),
            (_, Err(err)) if needs_firewall_helper(&member) => Err(AppError::Connection(err.to_string())),
            _ => grant_one(&connection, &member, &rules).await,
        };
        results.push(MemberAccessResult {
            user_id: member.user_id,
            email: member.email,
            node_username: member.node_username,
            has_key: true,
            granted: outcome.is_ok(),
            error: outcome.err().map(|err| err.to_string()),
            notes,
        });
    }

    let revocations = complete_revocations(cloud, &connection, team_id, team_server_id).await?;
    Ok(NodeAccessSync { members: results, revocations })
}

/// A Node's access that the automatic sweep could not bring in line.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessSweepProblem {
    pub team_server_id: Uuid,
    pub server_name: String,
    /// The sync as a whole failed - the Node was unreachable, say.
    pub error: Option<String>,
    /// Members whose grant or revocation failed, by email.
    pub members: Vec<String>,
}

/// What each Node was last brought in line with, as a fingerprint of the
/// team's access list and pending revocations. Held for this run of the app:
/// after a restart every Node is synced once more, which costs a few
/// commands and settles anything that changed while the app was closed.
static LAST_SWEPT: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, u64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn fingerprint(members: &[CloudMemberAccess], revocations: &[crate::models::CloudNodeRevocation], team_server_id: Uuid, local_server_id: Uuid) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    team_server_id.hash(&mut hasher);
    local_server_id.hash(&mut hasher);
    serde_json::to_string(members).unwrap_or_default().hash(&mut hasher);
    for revocation in revocations.iter().filter(|row| row.team_server_id == team_server_id) {
        revocation.id.hash(&mut hasher);
    }
    hasher.finish()
}

/// Brings every Node this install administers for a team in line with that
/// team, whenever what the team says has changed.
///
/// **This is what makes adding a member enough.** Access used to be written
/// to a Node only when somebody pressed Sync on it in Teams -> Servers, so a
/// new member - or a new device, a changed role, an Application just shared
/// - reached nobody until then, and a removed member kept their key. Run
/// on a timer by the app: a Node is touched only when the team's access list
/// or its pending revocations differ from what it was last synced with.
///
/// **Only where this install is the administrator.** A Node counts when this
/// install has an SSH entry for it under an account of its own - not a
/// `vibessh-m-` member account, whose rules could not write any of this.
/// That is the whole test, and it is the right one: an install that can log
/// in to the Node as its administrator can already do everything a sync
/// does, so running it there grants nobody anything new.
///
/// **Every application on a team's Node is shared with the team.** Sharing
/// the Node is the decision; the applications on it follow, new ones
/// included, and a role decides what each member may do with them. They
/// used to be shared one by one from each application's Users tab, so a
/// member added to a team saw its Node and none of what ran there - and one
/// shared before the projection recorded its Node was inert until somebody
/// happened to open that tab. The Users tab still narrows one application
/// down to named people.
///
/// It needs this app running. The backend holds no credentials for any
/// Node, by design, so there is nothing else that could do it.
pub async fn sync_administered_team_access(
    server_repo: &ServerRepository,
    app_repo: &crate::storage::application_repository::ApplicationRepository,
    sessions: &SshSessionManager,
    cloud: &CloudState,
) -> AppResult<Vec<AccessSweepProblem>> {
    if crate::services::cloud_service::session_info(cloud).await.is_none() {
        return Ok(vec![]);
    }
    let teams = crate::services::cloud_service::list_teams(cloud).await?;
    let local_servers = server_repo.list()?;
    let mut problems = Vec::new();

    for team in &teams {
        let read = async {
            let servers = crate::services::cloud_service::list_servers(cloud, team.id).await?;
            let members = crate::services::cloud_service::list_team_access(cloud, team.id).await?;
            let revocations = crate::services::cloud_service::list_pending_revocations(cloud, team.id).await?;
            let shared = crate::services::cloud_service::list_team_applications(cloud, team.id).await?;
            AppResult::Ok((servers, members, revocations, shared))
        }
        .await;
        // Left for the next run: a team that could not be read says nothing
        // about what its Nodes should hold.
        let Ok((servers, members, revocations, shared)) = read else { continue };

        for team_server in &servers {
            let Ok(port) = u16::try_from(team_server.ssh_port) else { continue };
            let Some(local) = local_servers.iter().find(|local| {
                local.host == team_server.host
                    && local.ssh_port == port
                    && !local.username.starts_with("vibessh-m-")
                    && local.connection_mode == crate::models::ConnectionMode::Ssh
            }) else {
                continue;
            };

            // This install's own applications on the Node that the team does
            // not have yet, or has without knowing which Node they are on.
            // An application somebody else shared with this account is theirs
            // to share, not this install's.
            let mut share_failures = Vec::new();
            for application in app_repo.list_by_server(local.id)? {
                if app_repo.shared_access(application.id)?.is_some() {
                    continue;
                }
                let projection = shared.iter().find(|projection| projection.local_id == application.id);
                if projection.is_some_and(|projection| projection.team_server_id == Some(team_server.id)) {
                    continue;
                }
                if let Err(err) = crate::services::team_application_service::share_application(app_repo, server_repo, cloud, team.id, application.id, Some(team_server.id)).await {
                    log::warn!("couldn't share {} with team {}: {err}", application.name, team.name);
                    share_failures.push(application.name.clone());
                }
            }
            if !share_failures.is_empty() {
                problems.push(AccessSweepProblem {
                    team_server_id: team_server.id,
                    server_name: team_server.name.clone(),
                    error: Some(format!("couldn't share {} with the team", share_failures.join(", "))),
                    members: vec![],
                });
            }

            let wanted = fingerprint(&members, &revocations, team_server.id, local.id);
            if LAST_SWEPT.lock().expect("access sweep mutex poisoned").get(&team_server.id) == Some(&wanted) {
                continue;
            }

            match sync_team_access(server_repo, sessions, cloud, local.id, team.id, team_server.id).await {
                Ok(outcome) => {
                    let failed: Vec<String> = outcome
                        .members
                        .iter()
                        .filter(|member| member.error.is_some())
                        .map(|member| member.email.clone())
                        .chain(outcome.revocations.iter().filter(|revocation| !revocation.completed).map(|revocation| revocation.email.clone()))
                        .collect();
                    if failed.is_empty() {
                        // Remembered only when everything landed, so a
                        // member whose grant failed is tried again next time.
                        LAST_SWEPT.lock().expect("access sweep mutex poisoned").insert(team_server.id, wanted);
                    } else {
                        problems.push(AccessSweepProblem { team_server_id: team_server.id, server_name: team_server.name.clone(), error: None, members: failed });
                    }
                }
                Err(err) => {
                    log::warn!("couldn't sync team access on {}: {err}", team_server.name);
                    problems.push(AccessSweepProblem { team_server_id: team_server.id, server_name: team_server.name.clone(), error: Some(err.to_string()), members: vec![] });
                }
            }
        }
    }
    Ok(problems)
}

/// Carries out every revocation this team is owed on this Node.
///
/// Only this Node: the list covers every machine the team shares, and this
/// install may be able to reach exactly one of them. Matching on the team
/// server's own id rather than on its address, because the address is what
/// the interface used to find a local server and re-deriving it here would
/// be a second answer to a question already settled.
///
/// The backend is told a revocation landed only after the Node's own exit
/// code said so. A failure leaves the row pending, which is correct: their
/// key is still there.
async fn complete_revocations(
    cloud: &CloudState,
    connection: &crate::ssh::SshSession,
    team_id: Uuid,
    team_server_id: Uuid,
) -> AppResult<Vec<RevocationResult>> {
    let pending = crate::services::cloud_service::list_pending_revocations(cloud, team_id).await?;

    let mut results = Vec::new();
    for revocation in pending.into_iter().filter(|row| row.team_server_id == team_server_id) {
        let outcome = revoke_one(cloud, connection, team_id, &revocation).await;
        results.push(RevocationResult {
            id: revocation.id,
            email: revocation.email,
            node_username: revocation.node_username,
            completed: outcome.is_ok(),
            error: outcome.err().map(|err| err.to_string()),
        });
    }
    Ok(results)
}

async fn revoke_one(
    cloud: &CloudState,
    connection: &crate::ssh::SshSession,
    team_id: Uuid,
    revocation: &crate::models::CloudNodeRevocation,
) -> AppResult<()> {
    member_account::run(
        connection,
        &member_account::revoke_script(&revocation.node_username)?,
        "revoke the member's access",
    )
    .await?;
    crate::services::cloud_service::complete_revocation(cloud, team_id, revocation.id).await
}

/// Whether this member's rules name the console writer - through a grant on
/// one Application, or through a role's team-wide console permission.
fn needs_console_writer(member: &CloudMemberAccess, rules: &[member_sudoers::ApplicationRules]) -> bool {
    rules.iter().any(|rules| rules.console.is_some()) || member.permissions.iter().any(|key| key == "applications.console")
}

/// Whether this member's rules name the DOCKER-USER helper - a role with
/// `node.firewall`, see `member_sudoers::privilege_for`.
fn needs_firewall_helper(member: &CloudMemberAccess) -> bool {
    member.permissions.iter().any(|key| key == "node.firewall")
}

/// The rules one member's per-application grants earn on this Node.
///
/// An Application with no `team_server_id`, or on another Node, earns
/// nothing here; one whose runtime has no container or unit to name is said
/// so in the notes rather than dropped.
fn application_rules_on(member: &CloudMemberAccess, team_server_id: Uuid) -> Vec<member_sudoers::ApplicationRules> {
    member
        .applications
        .iter()
        .filter(|application| application.team_server_id == Some(team_server_id))
        .map(|application| match member_sudoers::GrantRuntime::from_projection(&application.runtime_type) {
            Some(runtime) => member_sudoers::application_rules(&member_sudoers::ApplicationGrant {
                application_id: application.local_id,
                runtime,
                working_directory: application.working_directory.clone(),
                permissions: application.permissions.clone(),
            }),
            None => member_sudoers::ApplicationRules {
                skipped: vec![member_sudoers::SkippedGrant {
                    application_id: application.local_id,
                    reason: member_sudoers::SkipReason::NothingToName,
                }],
                ..Default::default()
            },
        })
        .collect()
}

/// Puts the console writer in place when it is missing or out of date - the
/// same compare-then-install the file helper and the schedule runner use.
async fn ensure_console_writer_installed(connection: &crate::ssh::SshSession) -> AppResult<()> {
    let writer = crate::ssh::command::quote(member_sudoers::CONSOLE_WRITER_PATH);
    let deployed = connection.execute_command(&format!("sudo cat {writer} 2>/dev/null")).await;
    if matches!(deployed, Ok(ref output) if output.stdout == member_sudoers::CONSOLE_WRITER_SCRIPT) {
        return Ok(());
    }
    // Staged in the admin's own home over SFTP, then moved into place as
    // root: nothing passes through a world-writable directory (AGENTS.md 4).
    let staging = format!(".vibessh-console-write-{}", Uuid::new_v4());
    connection.write_file(&staging, member_sudoers::CONSOLE_WRITER_SCRIPT.as_bytes()).await?;
    let staging = crate::ssh::command::quote(&staging);
    let output = connection
        .execute_command(&format!("sudo install -D -o root -g root -m 0755 {staging} {writer}; rc=$?; rm -f {staging}; exit $rc"))
        .await?;
    if output.exit_code != 0 {
        return Err(AppError::Connection(format!("couldn't install the console writer: {}", output.stderr.trim())));
    }
    Ok(())
}

async fn grant_one(connection: &crate::ssh::SshSession, member: &CloudMemberAccess, applications: &[member_sudoers::ApplicationRules]) -> AppResult<()> {
    let username = &member.node_username;
    member_account::run(connection, &member_account::provision_script(username)?, "create the member's account").await?;
    // The keys before the sudo rule. An account that can log in and do
    // nothing is a half-finished grant somebody can see; a sudo rule for an
    // account nobody can log into is one they cannot.
    member_account::run(
        connection,
        &member_account::authorized_keys_script(username, &member.public_keys)?,
        "install the member's keys",
    )
    .await?;
    // Last, and derived from their role rather than blanket. A member whose
    // role earns nothing privileged ends up with no sudoers file at all,
    // which is the whole of stage 3 - see `member_sudoers`.
    member_account::run(connection, &member_account::sudoers_script(username, &member.permissions, applications)?, "set the member's sudo rules").await
}

/// What this person logs in as on a shared Node, and with which key.
///
/// Both halves are already decided - the account name by the backend, the
/// key by this install - and neither was anywhere a member could see it.
/// That was the whole of the gap: a teammate was given a real account on a
/// real machine and no way to learn its name, so the feature looked broken
/// while working exactly as designed.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyNodeAccess {
    /// The Linux account created for this person by a sync.
    pub node_username: String,
    /// The private half of the key a sync installs, on this machine.
    pub private_key_path: String,
    /// False until somebody has run a sync on the Node. The account name is
    /// known either way - it is derived, not assigned - so this says whether
    /// connecting will actually work yet rather than whether we can name it.
    pub published: bool,
}

/// Looks this person up in the team's own access list.
///
/// Derived from the backend's answer rather than re-derived here: the
/// account name comes from a rule in `device_keys::node_username`, and a
/// second copy of that rule on this side is a second thing to keep in step.
pub async fn my_node_access(cloud: &CloudState, team_id: Uuid, config_dir: &std::path::Path) -> AppResult<MyNodeAccess> {
    let session = crate::services::cloud_service::session_info(cloud)
        .await
        .ok_or_else(|| AppError::InvalidInput("you are not signed in".to_string()))?;

    let members = crate::services::cloud_service::list_team_access(cloud, team_id).await?;
    let me = members
        .into_iter()
        .find(|member| member.user_id == session.user.id)
        .ok_or_else(|| AppError::NotFound("you are not a member of this team".to_string()))?;

    let key = crate::device_key::ensure(config_dir)?;
    Ok(MyNodeAccess { node_username: me.node_username, private_key_path: key.private_key_path, published: !me.public_keys.is_empty() })
}

/// Publishes this device's public key, so other installs can put it in the
/// accounts they create.
///
/// Called after signing in rather than on a button, because a key nobody
/// published is a member nobody can grant access to - and the person would
/// have no way of knowing that was the missing step.
pub async fn publish_this_device(cloud: &CloudState, config_dir: &std::path::Path) -> AppResult<()> {
    let key = crate::device_key::ensure(config_dir)?;
    crate::services::cloud_service::publish_device_key(cloud, &key.public_key, &key.label)
        .await
        .map(|_| ())
        .map_err(|err| AppError::Connection(format!("couldn't publish this device's key: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(keys: &[&str], permissions: &[&str]) -> CloudMemberAccess {
        CloudMemberAccess {
            user_id: Uuid::nil(),
            email: "member@example.com".into(),
            display_name: "Member".into(),
            node_username: "vibessh-m-0123456789ab".into(),
            public_keys: keys.iter().map(|key| (*key).to_string()).collect(),
            permissions: permissions.iter().map(|permission| (*permission).to_string()).collect(),
            applications: vec![],
        }
    }

    fn revocation(team_server_id: Uuid) -> crate::models::CloudNodeRevocation {
        crate::models::CloudNodeRevocation {
            id: Uuid::new_v4(),
            team_server_id,
            server_name: "royalmc-dedyk".into(),
            host: "203.0.113.5".into(),
            ssh_port: 22,
            user_id: Uuid::nil(),
            node_username: "vibessh-m-0123456789ab".into(),
            email: "gone@example.com".into(),
            requested_at: chrono::Utc::now(),
        }
    }

    /// The sweep only touches a Node when this changes, so everything that
    /// should reach the Node has to change it: a member's first device key,
    /// a new permission, a removal waiting to be carried out.
    #[test]
    fn every_change_that_must_reach_the_node_changes_the_fingerprint() {
        let (team_server, local) = (Uuid::new_v4(), Uuid::new_v4());
        let base = fingerprint(&[member(&[], &[])], &[], team_server, local);

        assert_eq!(base, fingerprint(&[member(&[], &[])], &[], team_server, local), "an unchanged team must not trigger a sync");
        assert_ne!(base, fingerprint(&[member(&["ssh-ed25519 AAAA"], &[])], &[], team_server, local), "a newly published key");
        assert_ne!(base, fingerprint(&[member(&[], &["applications.console"])], &[], team_server, local), "a changed permission");
        assert_ne!(base, fingerprint(&[], &[], team_server, local), "a member removed");
        assert_ne!(base, fingerprint(&[member(&[], &[])], &[revocation(team_server)], team_server, local), "a revocation for this Node");
        assert_eq!(base, fingerprint(&[member(&[], &[])], &[revocation(Uuid::new_v4())], team_server, local), "another Node's revocation is not this Node's business");
    }
}
