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
