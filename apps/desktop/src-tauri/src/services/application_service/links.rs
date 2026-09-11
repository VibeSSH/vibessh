//! Which Applications on a Node are allowed to reach each other.
//!
//! Reachability between Applications is default-deny (`AUDIT_REPORT.md`
//! S-018) and this is the whole of the allow-list: granting, revoking, and
//! pushing the result onto the Node's Docker networking.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::RuntimeType;
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::RuntimeContext;
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

use super::*;

/// Which other Applications this one is allowed to reach over the Node's
/// internal Docker networking.
pub fn list_application_links(repo: &ApplicationRepository, id: Uuid) -> AppResult<Vec<Uuid>> {
    // Through `get_application` rather than the repository directly, so a
    // missing Application is a `NotFound` rather than an empty list - "this
    // application can reach nothing" and "there is no such application" are
    // very different answers to give a UI about isolation.
    Ok(get_application(repo, id)?.links)
}

/// Lets two Applications on the same Node reach each other's ports.
///
/// Reachability is default-deny (`runtime::docker`'s `APP_NETWORK_PREFIX`),
/// so this is how a Velocity proxy is allowed to find its Paper backend, or
/// an app its self-hosted cache. It is symmetric, because the Docker bridge
/// network that implements it is: granting A→B also grants B→A, and the
/// storage layer refuses to record a direction it could not honour.
///
/// Both Applications must be Docker workloads on the *same* Node. A Docker
/// network does not span hosts, and a systemd unit or bare process is not on
/// one at all - for those, "who can reach this port" is the firewall's
/// question, not this one.
///
/// Writes the row first and applies second, and `disconnect_applications`
/// does the reverse, so that a half-completed change always errs towards
/// reporting *more* connectivity than exists rather than less. An operator
/// who is told two Applications are connected when they are not loses a
/// feature until the next start; one told they are isolated when they are
/// not has been given a false answer about a boundary.
pub async fn connect_applications(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    a: Uuid,
    b: Uuid,
) -> AppResult<()> {
    validate_connectable(repo, a, b)?;
    repo.add_link(a, b)?;
    apply_connections(repo, server_repo, sessions, local_process_manager, &[a, b]).await
}

/// Stops two Applications being able to reach each other.
///
/// Applies before it forgets: if taking the containers off their shared
/// network fails, the row goes back, because the connection is still live
/// and a UI that has stopped listing it would be claiming an isolation the
/// Node is not enforcing.
pub async fn disconnect_applications(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    a: Uuid,
    b: Uuid,
) -> AppResult<()> {
    repo.remove_link(a, b)?;
    match apply_connections(repo, server_repo, sessions, local_process_manager, &[a, b]).await {
        Ok(()) => Ok(()),
        Err(err) => {
            if let Err(restore) = repo.add_link(a, b) {
                // Now the stored state under-reports a live connection,
                // which is the one outcome this function is arranged to
                // avoid - it has to be loud.
                log::error!("failed to restore the connection row after a failed disconnect between {a} and {b}: {restore}");
            }
            Err(err)
        }
    }
}

/// Both Applications exist, both are Docker, and both are on the same Node.
///
/// Checked here rather than at the storage layer because it is a statement
/// about what the *runtime* can implement, not about what the table can
/// hold - and the error has to name which of the three conditions failed,
/// since an operator staring at two Applications side by side has no way to
/// tell from the UI that one of them is a systemd unit.
fn validate_connectable(repo: &ApplicationRepository, a: Uuid, b: Uuid) -> AppResult<()> {
    if a == b {
        return Err(AppError::InvalidInput("an application can't be connected to itself".into()));
    }
    let first = get_application(repo, a)?.application;
    let second = get_application(repo, b)?.application;
    for application in [&first, &second] {
        if application.runtime_type != RuntimeType::Docker {
            return Err(AppError::InvalidInput(format!(
                "'{}' isn't a Docker application, so there's no private network to connect it to",
                application.name
            )));
        }
    }
    match (first.server_id, second.server_id) {
        (Some(left), Some(right)) if left == right => Ok(()),
        // Both local is the same host as surely as both on one Node is, and
        // the same local Docker daemon holds both containers - refusing it
        // meant a phpMyAdmin and a MariaDB on somebody's own desktop could
        // never be connected at all, which is not what the rule below says.
        (None, None) => Ok(()),
        _ => Err(AppError::InvalidInput(format!(
            "'{}' and '{}' aren't on the same node - a Docker network doesn't span hosts",
            first.name, second.name
        ))),
    }
}

/// Re-applies the stored allow-list to each of `ids` on the Node.
///
/// Every id, not just the one that changed: a connection has two ends, and
/// applying it to one container without the other leaves a network with a
/// single member, which reaches nothing.
async fn apply_connections(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    ids: &[Uuid],
) -> AppResult<()> {
    for id in ids {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, *id).await?;
        let ctx = RuntimeContext {
            application: &detail.application,
            runtime_config: &detail.runtime_config,
            environment: &detail.environment,
            ports: &detail.ports,
            links: &detail.links,
            connection,
        };
        runtime.sync_connections(&ctx).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CreateApplicationInput;

    fn temp_repo() -> ApplicationRepository {
        let path = std::env::temp_dir().join(format!("vibessh-links-test-{}.sqlite3", uuid::Uuid::new_v4()));
        ApplicationRepository::open(&path).unwrap()
    }

    /// Local only: an Application on a Node needs a real `servers` row to
    /// satisfy the foreign key, and the Node half of this rule is not what
    /// changed.
    fn create(repo: &ApplicationRepository, name: &str, runtime_type: RuntimeType) -> Uuid {
        repo.create(&CreateApplicationInput {
            server_id: None,
            name: name.to_string(),
            description: None,
            blueprint_id: "generic-docker".to_string(),
            blueprint_version: 1,
            runtime_type,
            working_directory: "/tmp/app".to_string(),
            environment: vec![],
            ports: vec![],
            runtime_config: serde_json::json!({}),
            metadata: serde_json::json!({}),
        })
        .unwrap()
        .application
        .id
    }

    /// Two containers on the same local Docker daemon are as much on one
    /// host as two on a Node are. Refusing this meant a phpMyAdmin and a
    /// MariaDB on somebody's own desktop could never be connected at all.
    #[test]
    fn two_local_docker_applications_are_on_the_same_host() {
        let repo = temp_repo();
        let a = create(&repo, "phpMyAdmin", RuntimeType::Docker);
        let b = create(&repo, "MariaDB", RuntimeType::Docker);

        validate_connectable(&repo, a, b).unwrap();
    }

    /// There is no private network to put a bare process on, and the error
    /// has to say which of the two is the problem.
    #[test]
    fn something_that_is_not_a_container_has_no_network_to_join() {
        let repo = temp_repo();
        let container = create(&repo, "phpMyAdmin", RuntimeType::Docker);
        let process = create(&repo, "A script", RuntimeType::LocalProcess);

        let err = validate_connectable(&repo, container, process).unwrap_err();

        assert!(format!("{err}").contains("A script"), "the error should name the one that isn't a container: {err}");
    }
}
