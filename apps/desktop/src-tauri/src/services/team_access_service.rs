//! Giving a team's members their own account on a Node, and taking it away.
//!
//! This is the point where the two halves meet: the backend knows who is in
//! the team and which public keys their devices published, and this install
//! is the one that can reach the Node. Nothing secret crosses between them -
//! only public keys, which is the whole reason the design chose per-member
//! accounts. See `docs/planning/team-access-design.md`.
//!
//! **What a member can do once this has run.** Everything the app can do on
//! that Node. The account is theirs and the log names them, but until
//! role-derived sudoers lands (stage 3) it is as privileged as the owner's.
//! Anything in the interface that offers this has to say so.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::member_account;
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
}

/// Gives every member of `team_id` an account on `server_id`.
///
/// Runs per member and keeps going after one fails, because the alternative
/// - stopping at the first problem - leaves the team in a state nobody can
/// describe: some people have access, some do not, and the error names only
/// the first.
pub async fn grant_team_access(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    cloud: &CloudState,
    server_id: Uuid,
    team_id: Uuid,
) -> AppResult<Vec<MemberAccessResult>> {
    let members = crate::services::cloud_service::list_team_access(cloud, team_id).await?;
    let connection = get_or_connect(server_repo, sessions, server_id).await?;

    let mut results = Vec::new();
    for member in members {
        let has_key = !member.public_keys.is_empty();
        if !has_key {
            results.push(MemberAccessResult {
                user_id: member.user_id,
                email: member.email,
                node_username: member.node_username,
                has_key: false,
                granted: false,
                error: None,
            });
            continue;
        }

        let outcome = grant_one(&connection, &member).await;
        results.push(MemberAccessResult {
            user_id: member.user_id,
            email: member.email,
            node_username: member.node_username,
            has_key: true,
            granted: outcome.is_ok(),
            error: outcome.err().map(|err| err.to_string()),
        });
    }
    Ok(results)
}

async fn grant_one(connection: &crate::ssh::SshSession, member: &CloudMemberAccess) -> AppResult<()> {
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
    member_account::run(connection, &member_account::sudoers_script(username)?, "grant the member sudo").await
}

/// Takes one member's access to one Node away.
///
/// Separate from removing them from the team, and deliberately so: the team
/// record lives in the backend and the account lives on the Node, and only
/// an install that can reach the Node can do the second. Until stage 4
/// reconciles the two, whatever offers this must say which of them happened.
pub async fn revoke_member_access(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    node_username: &str,
) -> AppResult<()> {
    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    member_account::run(connection.as_ref(), &member_account::revoke_script(node_username)?, "revoke the member's access").await
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
