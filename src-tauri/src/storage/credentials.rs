//! Secrets in the OS credential store (Windows Credential Manager / macOS
//! Keychain / Linux Secret Service) via the `keyring` crate - never written
//! to a plain file. One `keyring::Entry` per `(server_id, SecretKind)`, so
//! an agent credential and an SSH password for the *same* server id never
//! collide even though they're both keyed off it.

use keyring::Entry;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

const SERVICE_NAME: &str = "VibeSSH";

/// What a stored secret is for. Distinguishes entries for the same server
/// id in the same keyring service - not itself sensitive, just a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// Etap E's `issued_credential` - proves this desktop to a paired agent.
    AgentCredential,
    /// SSH password auth (Etap 2). Never the private key itself - see
    /// `models::Server::private_key_path` for why keys are referenced by
    /// path, not stored here.
    SshPassword,
    /// Passphrase for an SSH private key file, when the key has one.
    SshKeyPassphrase,
    /// A `DatabaseHost`'s admin password (Phase 11 foundation - see
    /// `models::database`) - keyed by the `DatabaseHost`'s own `id`, not a
    /// server id, but `store_secret`/`load_secret`/`delete_secret` take any
    /// `Uuid` as that namespace regardless of what kind of row it names.
    DatabaseHostAdmin,
    /// A generated `ApplicationDatabase` user's password (Phase 11
    /// foundation) - keyed by the `ApplicationDatabase`'s own `id`.
    ApplicationDatabaseUser,
}

impl SecretKind {
    fn suffix(self) -> &'static str {
        match self {
            SecretKind::AgentCredential => "agent-credential",
            SecretKind::SshPassword => "ssh-password",
            SecretKind::SshKeyPassphrase => "ssh-key-passphrase",
            SecretKind::DatabaseHostAdmin => "database-host-admin",
            SecretKind::ApplicationDatabaseUser => "application-database-user",
        }
    }
}

/// Deletes a secret, logging rather than returning on failure.
///
/// Every caller is a cleanup step inside a larger operation that has
/// already succeeded - a Server was deleted, an Application's environment
/// no longer has that key - so failing the whole operation because the
/// keyring was momentarily unavailable would be worse than the leftover.
/// But discarding the result with `let _ =`, which is what these call sites
/// used to do, means a credential silently outlives the thing it belonged
/// to with nothing anywhere recording that it happened. This is the middle
/// ground: never fatal, never invisible.
pub fn forget_secret(server_id: Uuid, kind: SecretKind) {
    if let Err(err) = delete_secret(server_id, kind) {
        log::warn!("couldn't remove the stored {} for {server_id}: {err}", kind.suffix());
    }
}

pub fn store_secret(server_id: Uuid, kind: SecretKind, value: &str) -> AppResult<()> {
    entry_for(server_id, kind)?
        .set_password(value)
        .map_err(|err| AppError::Storage(format!("failed to store {}: {err}", kind.suffix())))
}

pub fn load_secret(server_id: Uuid, kind: SecretKind) -> AppResult<Option<String>> {
    match entry_for(server_id, kind)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Storage(format!("failed to read {}: {err}", kind.suffix()))),
    }
}

pub fn delete_secret(server_id: Uuid, kind: SecretKind) -> AppResult<()> {
    match entry_for(server_id, kind)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Storage(format!("failed to delete {}: {err}", kind.suffix()))),
    }
}

fn entry_for(server_id: Uuid, kind: SecretKind) -> AppResult<Entry> {
    Entry::new(SERVICE_NAME, &format!("{server_id}:{}", kind.suffix()))
        .map_err(|err| AppError::Storage(format!("failed to access the OS credential store: {err}")))
}

/// The cloud backend's refresh token - not keyed by server id like
/// everything else in this file, since it belongs to the signed-in account
/// as a whole, not to any one server. A separate fixed entry name rather
/// than reusing `entry_for` with a fabricated placeholder id.
const CLOUD_REFRESH_TOKEN_ENTRY: &str = "cloud-refresh-token";

fn cloud_refresh_token_entry() -> AppResult<Entry> {
    Entry::new(SERVICE_NAME, CLOUD_REFRESH_TOKEN_ENTRY)
        .map_err(|err| AppError::Storage(format!("failed to access the OS credential store: {err}")))
}

pub fn store_cloud_refresh_token(value: &str) -> AppResult<()> {
    cloud_refresh_token_entry()?.set_password(value).map_err(|err| AppError::Storage(format!("failed to store the cloud session: {err}")))
}

pub fn load_cloud_refresh_token() -> AppResult<Option<String>> {
    match cloud_refresh_token_entry()?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Storage(format!("failed to read the cloud session: {err}"))),
    }
}

pub fn delete_cloud_refresh_token() -> AppResult<()> {
    match cloud_refresh_token_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Storage(format!("failed to clear the cloud session: {err}"))),
    }
}

/// The S3-compatible backup destination's secret access key - not keyed by
/// any id, same as the cloud refresh token above, since there's exactly
/// one destination for the whole install (see `models::BackupDestinationConfig`'s
/// own doc comment).
const BACKUP_DESTINATION_SECRET_ENTRY: &str = "backup-destination-secret-access-key";

fn backup_destination_secret_entry() -> AppResult<Entry> {
    Entry::new(SERVICE_NAME, BACKUP_DESTINATION_SECRET_ENTRY)
        .map_err(|err| AppError::Storage(format!("failed to access the OS credential store: {err}")))
}

pub fn store_backup_destination_secret(value: &str) -> AppResult<()> {
    backup_destination_secret_entry()?.set_password(value).map_err(|err| AppError::Storage(format!("failed to store the backup destination secret: {err}")))
}

pub fn load_backup_destination_secret() -> AppResult<Option<String>> {
    match backup_destination_secret_entry()?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Storage(format!("failed to read the backup destination secret: {err}"))),
    }
}

pub fn delete_backup_destination_secret() -> AppResult<()> {
    match backup_destination_secret_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Storage(format!("failed to clear the backup destination secret: {err}"))),
    }
}

// Named wrappers for the one call site (pairing_commands.rs) that predates
// SecretKind - self-documenting at the call site, same implementation.
pub fn store_agent_credential(server_id: Uuid, credential: &str) -> AppResult<()> {
    store_secret(server_id, SecretKind::AgentCredential, credential)
}

pub fn load_agent_credential(server_id: Uuid) -> AppResult<Option<String>> {
    load_secret(server_id, SecretKind::AgentCredential)
}

pub fn delete_agent_credential(server_id: Uuid) -> AppResult<()> {
    delete_secret(server_id, SecretKind::AgentCredential)
}

/// A secret `EnvironmentVariable`'s real value (`models::EnvironmentVariable::value`'s
/// own doc comment) - keyed by `(application_id, key)` rather than through
/// `SecretKind`/`entry_for`, since an Application can have any number of
/// secret variables (not one fixed secret per id the way every other
/// `SecretKind` is). Only `services::application_service` calls these -
/// see that module's `resolve_environment_secrets`/
/// `store_secret_environment_values`.
fn environment_secret_entry(application_id: Uuid, key: &str) -> AppResult<Entry> {
    Entry::new(SERVICE_NAME, &format!("{application_id}:env:{key}"))
        .map_err(|err| AppError::Storage(format!("failed to access the OS credential store: {err}")))
}

pub fn store_environment_secret(application_id: Uuid, key: &str, value: &str) -> AppResult<()> {
    environment_secret_entry(application_id, key)?
        .set_password(value)
        .map_err(|err| AppError::Storage(format!("failed to store the '{key}' secret: {err}")))
}

pub fn load_environment_secret(application_id: Uuid, key: &str) -> AppResult<Option<String>> {
    match environment_secret_entry(application_id, key)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Storage(format!("failed to read the '{key}' secret: {err}"))),
    }
}

pub fn delete_environment_secret(application_id: Uuid, key: &str) -> AppResult<()> {
    match environment_secret_entry(application_id, key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Storage(format!("failed to delete the '{key}' secret: {err}"))),
    }
}

/// Same per-row keyring shape as `environment_secret_entry` (identical
/// reasoning: any number of registry credentials can exist, not one fixed
/// secret per id), keyed by the credential row's own id rather than an
/// Application id - see `storage::registry_credential_repository`, the only
/// caller.
fn registry_credential_entry(credential_id: Uuid) -> AppResult<Entry> {
    Entry::new(SERVICE_NAME, &format!("{credential_id}:registry-password"))
        .map_err(|err| AppError::Storage(format!("failed to access the OS credential store: {err}")))
}

pub fn store_registry_credential_password(credential_id: Uuid, password: &str) -> AppResult<()> {
    registry_credential_entry(credential_id)?
        .set_password(password)
        .map_err(|err| AppError::Storage(format!("failed to store the registry credential: {err}")))
}

pub fn load_registry_credential_password(credential_id: Uuid) -> AppResult<Option<String>> {
    match registry_credential_entry(credential_id)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Storage(format!("failed to read the registry credential: {err}"))),
    }
}

pub fn delete_registry_credential_password(credential_id: Uuid) -> AppResult<()> {
    match registry_credential_entry(credential_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Storage(format!("failed to delete the registry credential: {err}"))),
    }
}

/// Every test across this crate that touches the real OS credential store -
/// here and in `services::server_service`'s tests - takes this lock first.
/// Windows Credential Manager isn't reliably safe under concurrent access
/// from many threads in one process (observed: a write for one credential
/// name occasionally not showing up yet when a *different* credential name
/// is read back moments later from another thread) - `cargo test` runs
/// tests in parallel by default, so without this, keyring-touching tests
/// are flaky in a way that has nothing to do with the code under test.
#[cfg(test)]
pub(crate) static KEYRING_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        KEYRING_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Cleans up the real OS credential store entry even if an assertion
    /// below panics - these tests write to the user's actual Windows
    /// Credential Manager / keychain, not a mock.
    struct Cleanup(Uuid, SecretKind);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = delete_secret(self.0, self.1);
        }
    }

    #[test]
    fn stores_loads_and_deletes_a_credential_via_the_real_os_keyring() {
        let _guard = lock();
        let server_id = Uuid::new_v4();
        let _cleanup = Cleanup(server_id, SecretKind::AgentCredential);

        assert_eq!(load_agent_credential(server_id).unwrap(), None);

        store_agent_credential(server_id, "test-secret-value").unwrap();
        assert_eq!(
            load_agent_credential(server_id).unwrap(),
            Some("test-secret-value".to_string())
        );

        delete_agent_credential(server_id).unwrap();
        assert_eq!(load_agent_credential(server_id).unwrap(), None);
    }

    #[test]
    fn cloud_refresh_token_stores_loads_and_deletes_via_the_real_os_keyring() {
        let _guard = lock();
        struct CloudCleanup;
        impl Drop for CloudCleanup {
            fn drop(&mut self) {
                let _ = delete_cloud_refresh_token();
            }
        }
        let _cleanup = CloudCleanup;

        assert_eq!(load_cloud_refresh_token().unwrap(), None);
        store_cloud_refresh_token("real-refresh-token-value").unwrap();
        assert_eq!(load_cloud_refresh_token().unwrap(), Some("real-refresh-token-value".to_string()));
        delete_cloud_refresh_token().unwrap();
        assert_eq!(load_cloud_refresh_token().unwrap(), None);
    }

    #[test]
    fn different_secret_kinds_for_the_same_server_dont_collide() {
        let _guard = lock();
        let server_id = Uuid::new_v4();
        let _cleanup_password = Cleanup(server_id, SecretKind::SshPassword);
        let _cleanup_agent = Cleanup(server_id, SecretKind::AgentCredential);

        store_secret(server_id, SecretKind::SshPassword, "hunter2").unwrap();
        store_secret(server_id, SecretKind::AgentCredential, "agent-cred-value").unwrap();

        assert_eq!(
            load_secret(server_id, SecretKind::SshPassword).unwrap(),
            Some("hunter2".to_string())
        );
        assert_eq!(
            load_secret(server_id, SecretKind::AgentCredential).unwrap(),
            Some("agent-cred-value".to_string())
        );
        // The third kind was never written for this server id.
        assert_eq!(load_secret(server_id, SecretKind::SshKeyPassphrase).unwrap(), None);
    }

    #[test]
    fn database_secret_kinds_store_load_and_delete_via_the_real_os_keyring() {
        let _guard = lock();
        let host_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let _cleanup_host = Cleanup(host_id, SecretKind::DatabaseHostAdmin);
        let _cleanup_database = Cleanup(database_id, SecretKind::ApplicationDatabaseUser);

        store_secret(host_id, SecretKind::DatabaseHostAdmin, "admin-password").unwrap();
        store_secret(database_id, SecretKind::ApplicationDatabaseUser, "generated-user-password").unwrap();

        assert_eq!(load_secret(host_id, SecretKind::DatabaseHostAdmin).unwrap(), Some("admin-password".to_string()));
        assert_eq!(
            load_secret(database_id, SecretKind::ApplicationDatabaseUser).unwrap(),
            Some("generated-user-password".to_string())
        );

        delete_secret(host_id, SecretKind::DatabaseHostAdmin).unwrap();
        assert_eq!(load_secret(host_id, SecretKind::DatabaseHostAdmin).unwrap(), None);
    }

    #[test]
    fn environment_secrets_store_load_and_delete_via_the_real_os_keyring() {
        let _guard = lock();
        let application_id = Uuid::new_v4();
        struct EnvCleanup(Uuid, &'static str);
        impl Drop for EnvCleanup {
            fn drop(&mut self) {
                let _ = delete_environment_secret(self.0, self.1);
            }
        }
        let _cleanup = EnvCleanup(application_id, "DB_PASSWORD");

        assert_eq!(load_environment_secret(application_id, "DB_PASSWORD").unwrap(), None);

        store_environment_secret(application_id, "DB_PASSWORD", "hunter2").unwrap();
        assert_eq!(load_environment_secret(application_id, "DB_PASSWORD").unwrap(), Some("hunter2".to_string()));

        delete_environment_secret(application_id, "DB_PASSWORD").unwrap();
        assert_eq!(load_environment_secret(application_id, "DB_PASSWORD").unwrap(), None);
    }

    #[test]
    fn registry_credential_password_stores_loads_and_deletes_via_the_real_os_keyring() {
        let _guard = lock();
        let credential_id = Uuid::new_v4();
        struct Cleanup(Uuid);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_registry_credential_password(self.0);
            }
        }
        let _cleanup = Cleanup(credential_id);

        assert_eq!(load_registry_credential_password(credential_id).unwrap(), None);
        store_registry_credential_password(credential_id, "ghp_realtoken").unwrap();
        assert_eq!(load_registry_credential_password(credential_id).unwrap(), Some("ghp_realtoken".to_string()));
        delete_registry_credential_password(credential_id).unwrap();
        assert_eq!(load_registry_credential_password(credential_id).unwrap(), None);
    }

    #[test]
    fn backup_destination_secret_stores_loads_and_deletes_via_the_real_os_keyring() {
        let _guard = lock();
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_backup_destination_secret();
            }
        }
        let _cleanup = Cleanup;

        assert_eq!(load_backup_destination_secret().unwrap(), None);
        store_backup_destination_secret("s3-secret-value").unwrap();
        assert_eq!(load_backup_destination_secret().unwrap(), Some("s3-secret-value".to_string()));
        delete_backup_destination_secret().unwrap();
        assert_eq!(load_backup_destination_secret().unwrap(), None);
    }

    #[test]
    fn environment_secrets_for_different_keys_on_the_same_application_dont_collide() {
        let _guard = lock();
        let application_id = Uuid::new_v4();
        struct EnvCleanup(Uuid, &'static str);
        impl Drop for EnvCleanup {
            fn drop(&mut self) {
                let _ = delete_environment_secret(self.0, self.1);
            }
        }
        let _cleanup_a = EnvCleanup(application_id, "DB_PASSWORD");
        let _cleanup_b = EnvCleanup(application_id, "API_KEY");

        store_environment_secret(application_id, "DB_PASSWORD", "hunter2").unwrap();
        store_environment_secret(application_id, "API_KEY", "sk-real-value").unwrap();

        assert_eq!(load_environment_secret(application_id, "DB_PASSWORD").unwrap(), Some("hunter2".to_string()));
        assert_eq!(load_environment_secret(application_id, "API_KEY").unwrap(), Some("sk-real-value".to_string()));
    }
}
