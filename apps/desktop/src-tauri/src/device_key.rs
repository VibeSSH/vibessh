//! This device's own SSH keypair, used to reach Nodes a team has shared.
//!
//! **Why a key per device rather than per account.** Somebody with a laptop
//! and a desktop gets two, and revoking the laptop leaves the desktop alone.
//! A single key shared between machines would mean losing one machine costs
//! access on all of them - and would make the Node's own auth log unable to
//! say which machine connected.
//!
//! **Why a file rather than the keyring.** This project already decided that
//! private keys are files referenced by path, with only their passphrase in
//! the OS keyring - see `SecretKind::SshKeyPassphrase` and
//! `Server::private_key_path`. Following that keeps one convention instead
//! of two, and has the side benefit that the key works with plain `ssh` when
//! somebody needs to get in without the app.
//!
//! The key is generated with no passphrase. A passphrase VibeSSH would have
//! to store somewhere in order to connect unattended is not a passphrase; it
//! is a second copy of the same secret with extra steps. The file's mode is
//! what protects it, exactly as it protects `~/.ssh/id_ed25519`.

use std::path::{Path, PathBuf};

use rand010::rngs::SysRng;
use russh::keys::ssh_key::rand_core::UnwrapErr;
use russh::keys::ssh_key::{Algorithm, LineEnding, PrivateKey};

use crate::errors::{AppError, AppResult};

/// Both halves, as the rest of the app needs them: a path to authenticate
/// with, and one line to publish.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceKey {
    /// Where the private half lives. Handed to the SSH layer, never read
    /// into this process's own memory.
    pub private_key_path: String,
    /// The `ssh-ed25519 AAAA... label` line, which is what goes to the
    /// backend and into a Node's `authorized_keys`.
    pub public_key: String,
    /// What this machine is called, so somebody revoking a device later can
    /// tell which one it was.
    pub label: String,
}

fn private_key_path(config_dir: &Path) -> PathBuf {
    config_dir.join("device_key")
}

/// The machine's own name, falling back to something honest rather than
/// something invented.
///
/// A blank or unknown label is worse than useless at the moment it matters -
/// somebody looking at three devices deciding which to revoke - so the
/// fallback says what it is rather than inventing a name.
///
/// Read from the environment rather than through a crate: the two variables
/// below are what Windows and the shells on Unix already set, and
/// `/etc/hostname` covers the case where neither is exported.
fn machine_label() -> String {
    for variable in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(variable) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    if let Ok(contents) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = contents.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "unnamed device".to_string()
}

/// Loads this device's key, generating it the first time.
///
/// Idempotent, and deliberately not regenerating when the file is present:
/// a new key would have to be installed on every Node again, and every
/// existing `authorized_keys` entry would become a key nobody holds - which
/// nothing would ever clean up.
pub fn ensure(config_dir: &Path) -> AppResult<DeviceKey> {
    let path = private_key_path(config_dir);
    let label = machine_label();

    if path.exists() {
        let key = PrivateKey::read_openssh_file(&path)
            .map_err(|err| AppError::Storage(format!("couldn't read this device's key at {}: {err}", path.display())))?;
        return Ok(DeviceKey {
            private_key_path: path.to_string_lossy().into_owned(),
            public_key: public_line(&key, &label)?,
            label,
        });
    }

    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("couldn't create {}: {err}", config_dir.display())))?;

    // Ed25519: small, fast, and the only algorithm every current OpenSSH
    // accepts without argument.
    let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)
        .map_err(|err| AppError::Internal(format!("couldn't generate this device's key: {err}")))?;
    // `write_openssh_file` creates the file 0600 on Unix, which is the whole
    // protection this key has - there is no passphrase by design.
    key.write_openssh_file(&path, LineEnding::LF)
        .map_err(|err| AppError::Storage(format!("couldn't write this device's key to {}: {err}", path.display())))?;

    Ok(DeviceKey { private_key_path: path.to_string_lossy().into_owned(), public_key: public_line(&key, &label)?, label })
}

/// The public half as one `authorized_keys` line, with this machine's name
/// as the comment.
///
/// The comment is cosmetic to SSH and is not cosmetic to a person: it is
/// what they read in a Node's `authorized_keys` when working out whose key
/// that is.
fn public_line(key: &PrivateKey, label: &str) -> AppResult<String> {
    let public = key
        .public_key()
        .to_openssh()
        .map_err(|err| AppError::Internal(format!("couldn't render this device's public key: {err}")))?;
    // A label with whitespace in it would become extra fields. The comment
    // is the last field and free-form, so collapsing whitespace keeps it one
    // field without rejecting a machine name somebody actually has.
    let comment = label.split_whitespace().collect::<Vec<_>>().join("-");
    let comment = if comment.is_empty() { "vibessh".to_string() } else { comment };
    Ok(format!("{} vibessh-{comment}", public.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibessh-device-key-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_key_is_generated_once_and_reused_afterwards() {
        let dir = temp_dir();
        let first = ensure(&dir).unwrap();
        let second = ensure(&dir).unwrap();
        // Regenerating would orphan every authorized_keys entry already
        // installed on a Node, and nothing would ever clean those up.
        assert_eq!(first.public_key, second.public_key);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_public_half_is_one_authorized_keys_line() {
        let dir = temp_dir();
        let key = ensure(&dir).unwrap();
        assert!(key.public_key.starts_with("ssh-ed25519 "), "{}", key.public_key);
        assert!(!key.public_key.contains('\n'), "a key with a line break is two entries: {}", key.public_key);
        // Type, body, comment - and nothing else, or the backend's own
        // validation will refuse it.
        assert_eq!(key.public_key.split_whitespace().count(), 3, "{}", key.public_key);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A machine called "Kryspin's PC" must not become three fields.
    #[test]
    fn a_machine_name_with_spaces_stays_one_comment_field() {
        let dir = temp_dir();
        let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519).unwrap();
        let line = public_line(&key, "Kryspin's PC").unwrap();
        assert_eq!(line.split_whitespace().count(), 3, "{line}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn the_private_key_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        ensure(&dir).unwrap();
        let mode = std::fs::metadata(private_key_path(&dir)).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the key is readable by somebody else: {mode:o}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
