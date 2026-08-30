//! Real SSH transport (Etap 3). `client` is pure protocol mechanics (connect,
//! TOFU host key check, auth, exec) with no knowledge of `Server`/keyring/
//! SQLite; `transport` adapts it to the app's `ServerConnection` trait.
//! `ssh_service` (in `services/`) is what actually resolves a `Server` row
//! and its keyring secret into the `SshCredentials` this module needs.

pub mod client;
mod docker;
mod monitor;
mod sftp;
mod systemd;
mod transport;

pub use client::{connect, SshAuth, SshCredentials, SshSession, TerminalHandle};
