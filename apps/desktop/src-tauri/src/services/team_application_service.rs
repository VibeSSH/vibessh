//! Projecting a local Application so the rest of its team can see it.
//!
//! The local SQLite record stays authoritative: it is what the runtime acts
//! on and what this install edits. What goes to the backend is a snapshot,
//! refreshed by pushing again - see
//! `apps/backend/migrations/0011_team_applications.sql`.
//!
//! **Secret values do not travel, and their existence does.** A secret
//! variable is projected with an empty value and a flag. Dropping it
//! entirely would read as "not configured", which would be wrong and would
//! send somebody to set a variable that is already set. The value itself is
//! on the Node, in the Application's own environment file, which is where
//! the process reads it from.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    CloudApplication, CloudApplicationEnvironment, CloudApplicationMember, CloudApplicationPort, EnvironmentVariable, PortProtocol, PortVisibility,
};
use crate::state::CloudState;
use crate::storage::application_repository::ApplicationRepository;

fn protocol_name(protocol: PortProtocol) -> &'static str {
    match protocol {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

fn visibility_name(visibility: PortVisibility) -> &'static str {
    match visibility {
        PortVisibility::Public => "public",
        PortVisibility::VibeNetwork => "vibeNetwork",
        PortVisibility::Localhost => "localhost",
        PortVisibility::Custom => "custom",
    }
}

/// The environment as the team should see it.
///
/// A plain repository read already blanks a secret row's value (see
/// `EnvironmentVariable::value`), so this cannot leak one even by mistake.
/// It is written as an explicit blank anyway: relying on a guarantee made
/// somewhere else, silently, is how the guarantee gets removed by somebody
/// who does not know this depends on it.
fn project_environment(environment: &[EnvironmentVariable]) -> Vec<CloudApplicationEnvironment> {
    environment
        .iter()
        .map(|variable| CloudApplicationEnvironment {
            key: variable.key.clone(),
            value: if variable.is_secret { String::new() } else { variable.value.clone() },
            is_secret: variable.is_secret,
        })
        .collect()
}

/// Publishes one Application to a team, replacing any earlier snapshot.
pub async fn share_application(
    app_repo: &ApplicationRepository,
    cloud: &CloudState,
    team_id: Uuid,
    application_id: Uuid,
    team_server_id: Option<Uuid>,
) -> AppResult<CloudApplication> {
    let detail = app_repo
        .get(application_id)?
        .ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;

    let ports: Vec<CloudApplicationPort> = detail
        .ports
        .iter()
        .map(|port| CloudApplicationPort {
            name: port.name.clone(),
            protocol: protocol_name(port.protocol).to_string(),
            internal_port: i32::from(port.internal_port),
            external_port: port.external_port.map(i32::from),
            visibility: visibility_name(port.visibility).to_string(),
        })
        .collect();

    crate::services::cloud_service::push_team_application(
        cloud,
        team_id,
        detail.application.id,
        team_server_id,
        &detail.application.name,
        &detail.application.blueprint_id,
        &format!("{:?}", detail.application.runtime_type).to_lowercase(),
        &detail.application.working_directory,
        &ports,
        &project_environment(&detail.environment),
    )
    .await
}

pub async fn list_shared_applications(cloud: &CloudState, team_id: Uuid) -> AppResult<Vec<CloudApplication>> {
    crate::services::cloud_service::list_team_applications(cloud, team_id).await
}

/// Stops sharing. The Application keeps running and this install keeps its
/// own record - only the team's copy goes.
pub async fn unshare_application(cloud: &CloudState, team_id: Uuid, application_id: Uuid) -> AppResult<()> {
    crate::services::cloud_service::remove_team_application(cloud, team_id, application_id).await
}

/// Who, of a team's members, may see one shared Application.
pub async fn list_application_members(cloud: &CloudState, team_id: Uuid, application_id: Uuid) -> AppResult<Vec<CloudApplicationMember>> {
    crate::services::cloud_service::list_application_members(cloud, team_id, application_id).await
}

/// Grants one member access. The first grant restricts the Application to its
/// allow-list; before that a shared Application is visible to the whole team.
pub async fn add_application_member(cloud: &CloudState, team_id: Uuid, application_id: Uuid, user_id: Uuid) -> AppResult<()> {
    crate::services::cloud_service::add_application_member(cloud, team_id, application_id, user_id).await
}

/// Replaces what one member may do with a shared Application they can see.
/// The backend refuses anything a Node could not hold to one Application,
/// and anything the caller does not hold themselves.
pub async fn set_application_member_permissions(
    cloud: &CloudState,
    team_id: Uuid,
    application_id: Uuid,
    user_id: Uuid,
    permissions: &[String],
) -> AppResult<()> {
    crate::services::cloud_service::set_application_member_permissions(cloud, team_id, application_id, user_id, permissions).await
}

/// Revokes one member's access. Emptying the list returns the Application to
/// being visible to the whole team.
pub async fn remove_application_member(cloud: &CloudState, team_id: Uuid, application_id: Uuid, user_id: Uuid) -> AppResult<()> {
    crate::services::cloud_service::remove_application_member(cloud, team_id, application_id, user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(key: &str, value: &str, is_secret: bool) -> EnvironmentVariable {
        EnvironmentVariable { key: key.into(), value: value.into(), is_secret }
    }

    /// The one that matters: a secret's value must not be in the projection,
    /// even if a repository read ever stopped blanking it.
    #[test]
    fn a_secret_value_is_never_projected() {
        let projected = project_environment(&[variable("MYSQL_ROOT_PASSWORD", "hunter2", true)]);
        assert_eq!(projected[0].value, "", "{projected:?}");
        assert!(projected[0].is_secret);
    }

    /// And its existence is. An omitted variable reads as "not configured",
    /// which would send somebody to set one that is already set.
    #[test]
    fn a_secret_variable_still_appears_by_name() {
        let projected = project_environment(&[variable("MYSQL_ROOT_PASSWORD", "", true)]);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].key, "MYSQL_ROOT_PASSWORD");
    }

    #[test]
    fn an_ordinary_variable_keeps_its_value() {
        let projected = project_environment(&[variable("PORT", "25565", false)]);
        assert_eq!(projected[0].value, "25565");
        assert!(!projected[0].is_secret);
    }
}
