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
}

impl SecretKind {
    fn suffix(self) -> &'static str {
        match self {
            SecretKind::AgentCredential => "agent-credential",
            SecretKind::SshPassword => "ssh-password",
            SecretKind::SshKeyPassphrase => "ssh-key-passphrase",
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
}
