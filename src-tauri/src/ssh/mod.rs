//! Real SSH transport (Etap 3). `client` is pure protocol mechanics (connect,
//! TOFU host key check, auth, exec) with no knowledge of `Server`/keyring/
//! SQLite; `transport` adapts it to the app's `ServerConnection` trait.
//! `ssh_service` (in `services/`) is what actually resolves a `Server` row
//! and its keyring secret into the `SshCredentials` this module needs.

pub mod client;
// The single place untrusted values become pieces of a remote command -
// `pub(crate)` because every module that builds a command string (services,
// runtime, files, network, firewall) must reach it, and none of them should
// keep a private copy. See its own doc comment for why the copies it
// replaced were a security problem, not just duplication.
pub(crate) mod command;
// `pub(crate)` (not just `mod`) so `runtime::docker` (Applications) can
// reuse `validate_container_ref` for its own `docker create`/`inspect`/
// `stats` calls, not only the calls this module's own methods already
// validate.
pub(crate) mod docker;
mod monitor;
mod port_forward;
mod sftp;
// `pub(crate)` (not just `mod`) so `runtime::systemd` (Applications) can
// reuse `validate_unit_name` for its own unit-file writes, not only the
// `systemctl` calls this module's own methods already validate.
pub(crate) mod systemd;
mod transport;

pub use client::{connect, SshAuth, SshCredentials, SshSession, TerminalHandle};
pub use port_forward::PortForwardHandle;

use crate::errors::{AppError, AppResult};

/// Writes `contents` to `path` on the Node, mode 0600 from the moment the
/// file exists.
///
/// The point is to get a secret onto a Node **without it ever appearing in
/// a command string**. Anything in a command string is visible in `ps` to
/// every local account for as long as the command runs, which is how both
/// the MySQL admin password and the container-registry password used to
/// leak (AUDIT S-008).
///
/// `install -m 600 /dev/null` creates the file with the right mode *first*,
/// then SFTP writes into it - SFTP preserves an existing file's mode, so
/// there is no window where the file exists world-readable. The content
/// itself travels over the SFTP channel and never touches a shell.
pub(crate) async fn write_private_file(connection: &SshSession, path: &str, contents: &[u8]) -> AppResult<()> {
    let output = connection.execute_command(&format!("install -m 600 /dev/null {}", command::quote(path))).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "couldn't create a private file".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(detail));
    }
    connection.write_file(path, contents).await
}
