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
}
