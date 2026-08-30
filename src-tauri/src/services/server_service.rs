//! Orchestrates `server_repository` (non-secret fields) and `credentials`
//! (password / key passphrase) so callers never have to remember to touch
//! both. Validation lives here too, not in the repository, so the SQLite
//! layer stays a plain CRUD store.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{AuthenticationType, Server, ServerInput};
use crate::storage::credentials::{self, SecretKind};
use crate::storage::server_repository::ServerRepository;

pub fn create_server(repo: &ServerRepository, input: ServerInput) -> AppResult<Server> {
    validate_common(&input)?;
    validate_secret_present_for_create(&input)?;
    let server = repo.create(&input)?;
    persist_secrets(server.id, &input)?;
    Ok(server)
}

pub fn update_server(repo: &ServerRepository, id: Uuid, input: ServerInput) -> AppResult<Server> {
    validate_common(&input)?;
    let server = repo.update(id, &input)?;
    // A blank password/passphrase means "leave it as it was" - the frontend
    // never has the existing secret to redisplay, so it can't resend it.
    persist_secrets(id, &input)?;
    Ok(server)
}

pub fn delete_server(repo: &ServerRepository, id: Uuid) -> AppResult<()> {
    repo.delete(id)?;
    // Best-effort: the row is already gone, and delete_secret already treats
    // "nothing to delete" as success, so these can't meaningfully fail in a
    // way the caller should roll back for.
    let _ = credentials::delete_secret(id, SecretKind::SshPassword);
    let _ = credentials::delete_secret(id, SecretKind::SshKeyPassphrase);
    Ok(())
}

pub fn get_server(repo: &ServerRepository, id: Uuid) -> AppResult<Server> {
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("server {id}")))
}

pub fn list_servers(repo: &ServerRepository) -> AppResult<Vec<Server>> {
    repo.list()
}

fn persist_secrets(id: Uuid, input: &ServerInput) -> AppResult<()> {
    if let Some(password) = non_blank(&input.password) {
        credentials::store_secret(id, SecretKind::SshPassword, password)?;
    }
    if let Some(passphrase) = non_blank(&input.key_passphrase) {
        credentials::store_secret(id, SecretKind::SshKeyPassphrase, passphrase)?;
    }
    Ok(())
}

fn non_blank(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn validate_common(input: &ServerInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::InvalidInput("server name cannot be empty".into()));
    }
    if input.host.trim().is_empty() {
        return Err(AppError::InvalidInput("host cannot be empty".into()));
    }
    if input.username.trim().is_empty() {
        return Err(AppError::InvalidInput("username cannot be empty".into()));
    }
    if input.ssh_port == 0 {
        return Err(AppError::InvalidInput("SSH port must be between 1 and 65535".into()));
    }
    if input.authentication_type == AuthenticationType::PrivateKey
        && non_blank(&input.private_key_path).is_none()
    {
        return Err(AppError::InvalidInput(
            "a private key file path is required for key-based authentication".into(),
        ));
    }
    Ok(())
}

fn validate_secret_present_for_create(input: &ServerInput) -> AppResult<()> {
    if input.authentication_type == AuthenticationType::Password && non_blank(&input.password).is_none() {
        return Err(AppError::InvalidInput(
            "a password is required for password authentication".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> ServerInput {
        ServerInput {
            name: "Production".to_string(),
            host: "203.0.113.10".to_string(),
            ssh_port: 22,
            username: "root".to_string(),
            authentication_type: AuthenticationType::Password,
            private_key_path: None,
            group_id: None,
            password: Some("hunter2".to_string()),
            key_passphrase: None,
        }
    }

    fn temp_repo() -> ServerRepository {
        let path = std::env::temp_dir().join(format!("vibessh-service-test-{}.sqlite3", Uuid::new_v4()));
        ServerRepository::open(&path).unwrap()
    }

    /// See `storage::credentials::KEYRING_TEST_LOCK` - any test here that
    /// goes through `create_server`/`update_server`/`delete_server` (and so
    /// touches the real OS keyring) takes this first.
    fn keyring_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::storage::credentials::KEYRING_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn create_rejects_an_empty_name() {
        let repo = temp_repo();
        let mut input = valid_input();
        input.name = "   ".to_string();
        let err = create_server(&repo, input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn create_rejects_password_auth_without_a_password() {
        let repo = temp_repo();
        let mut input = valid_input();
        input.password = None;
        let err = create_server(&repo, input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn create_rejects_key_auth_without_a_key_path() {
        let repo = temp_repo();
        let mut input = valid_input();
        input.authentication_type = AuthenticationType::PrivateKey;
        input.password = None;
        let err = create_server(&repo, input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn create_stores_the_password_in_the_keyring_and_delete_removes_it() {
        let _guard = keyring_lock();
        let repo = temp_repo();
        let server = create_server(&repo, valid_input()).unwrap();

        assert_eq!(
            credentials::load_secret(server.id, SecretKind::SshPassword).unwrap(),
            Some("hunter2".to_string())
        );

        delete_server(&repo, server.id).unwrap();
        assert_eq!(credentials::load_secret(server.id, SecretKind::SshPassword).unwrap(), None);
        assert!(get_server(&repo, server.id).is_err());
    }

    #[test]
    fn update_with_a_blank_password_keeps_the_existing_secret() {
        let _guard = keyring_lock();
        let repo = temp_repo();
        let server = create_server(&repo, valid_input()).unwrap();

        let mut update = valid_input();
        update.name = "Renamed".to_string();
        update.password = None;
        update_server(&repo, server.id, update).unwrap();

        assert_eq!(
            credentials::load_secret(server.id, SecretKind::SshPassword).unwrap(),
            Some("hunter2".to_string())
        );

        let _ = credentials::delete_secret(server.id, SecretKind::SshPassword);
    }

    #[test]
    fn list_returns_created_servers() {
        let _guard = keyring_lock();
        let repo = temp_repo();
        let server = create_server(&repo, valid_input()).unwrap();
        let servers = list_servers(&repo).unwrap();
        assert!(servers.iter().any(|s| s.id == server.id));
        let _ = credentials::delete_secret(server.id, SecretKind::SshPassword);
    }
}
