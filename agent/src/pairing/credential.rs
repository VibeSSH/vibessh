use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::errors::AgentResult;

const CREDENTIAL_FILE_NAME: &str = "paired_credential.json";

/// Only ever written to disk - the raw credential this is a hash of exists
/// only in memory for the moment it's generated and handed back over the
/// (still plaintext, Etap D/K note) WebSocket connection.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCredential {
    credential_hash: String,
    paired_at: DateTime<Utc>,
}

/// Generates a new 128-bit bearer credential, persists only its hash, and
/// returns the raw value so the caller can send it once to the newly
/// paired desktop.
pub fn issue_credential(data_dir: &Path) -> AgentResult<String> {
    let raw = Uuid::new_v4().simple().to_string();
    let record = PersistedCredential {
        credential_hash: hash(&raw),
        paired_at: Utc::now(),
    };
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(credential_path(data_dir), serde_json::to_vec_pretty(&record)?)?;
    Ok(raw)
}

/// `Ok(true)` only if a credential has been issued before and `candidate`
/// hashes to the same value. Any other outcome (no file yet, wrong value,
/// unreadable file) is a rejection, not an error the caller needs to
/// distinguish - a corrupt credential file should fail closed.
pub fn verify_credential(data_dir: &Path, candidate: &str) -> bool {
    let Ok(bytes) = std::fs::read(credential_path(data_dir)) else {
        return false;
    };
    let Ok(record) = serde_json::from_slice::<PersistedCredential>(&bytes) else {
        return false;
    };
    constant_time_eq(record.credential_hash.as_bytes(), hash(candidate).as_bytes())
}

pub fn has_paired_credential(data_dir: &Path) -> bool {
    credential_path(data_dir).exists()
}

fn credential_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(CREDENTIAL_FILE_NAME)
}

fn hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Avoids leaking the number of matching leading bytes through comparison
/// timing. Both inputs here are fixed-length hex digests, but this stays
/// correct even if that ever changes.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_credential_verifies_and_wrong_ones_dont() {
        let dir = std::env::temp_dir().join(format!("vibessh-agent-test-{}", Uuid::new_v4()));
        let raw = issue_credential(&dir).unwrap();

        assert!(verify_credential(&dir, &raw));
        assert!(!verify_credential(&dir, "not-the-credential"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_credential_file_means_verification_fails_closed() {
        let dir = std::env::temp_dir().join(format!("vibessh-agent-test-{}", Uuid::new_v4()));
        assert!(!verify_credential(&dir, "anything"));
        assert!(!has_paired_credential(&dir));
    }
}
