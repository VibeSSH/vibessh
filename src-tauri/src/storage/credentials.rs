//! Agent device credentials, one per paired server, in the OS credential
//! store (Windows Credential Manager / macOS Keychain / Linux Secret
//! Service) via the `keyring` crate - never written to a plain file. This
//! is what Etap E's `issued_credential` gets handed to once a handshake
//! surfaces it.

use keyring::Entry;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

const SERVICE_NAME: &str = "VibeSSH";

pub fn store_agent_credential(server_id: Uuid, credential: &str) -> AppResult<()> {
    entry_for(server_id)?
        .set_password(credential)
        .map_err(|err| AppError::Storage(format!("failed to store agent credential: {err}")))
}

pub fn load_agent_credential(server_id: Uuid) -> AppResult<Option<String>> {
    match entry_for(server_id)?.get_password() {
        Ok(credential) => Ok(Some(credential)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Storage(format!("failed to read agent credential: {err}"))),
    }
}

pub fn delete_agent_credential(server_id: Uuid) -> AppResult<()> {
    match entry_for(server_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Storage(format!("failed to delete agent credential: {err}"))),
    }
}

fn entry_for(server_id: Uuid) -> AppResult<Entry> {
    Entry::new(SERVICE_NAME, &server_id.to_string())
        .map_err(|err| AppError::Storage(format!("failed to access the OS credential store: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cleans up the real OS credential store entry even if an assertion
    /// below panics - this test writes to the user's actual Windows
    /// Credential Manager / keychain, not a mock.
    struct Cleanup(Uuid);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = delete_agent_credential(self.0);
        }
    }

    #[test]
    fn stores_loads_and_deletes_a_credential_via_the_real_os_keyring() {
        let server_id = Uuid::new_v4();
        let _cleanup = Cleanup(server_id);

        assert_eq!(load_agent_credential(server_id).unwrap(), None);

        store_agent_credential(server_id, "test-secret-value").unwrap();
        assert_eq!(
            load_agent_credential(server_id).unwrap(),
            Some("test-secret-value".to_string())
        );

        delete_agent_credential(server_id).unwrap();
        assert_eq!(load_agent_credential(server_id).unwrap(), None);
    }
}
