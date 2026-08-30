//! Adapts `SshSession` (pure protocol mechanics, see `client.rs`/`sftp.rs`)
//! to the app's transport-agnostic `ServerConnection` trait.
//! `execute_command`, `list_directory`, `read_file`, and `write_file` are
//! real. `get_metrics`/`list_processes`/`restart_service` stay honest stubs
//! - process manager and systemd are later stages that need their own
//! remote-side mechanics (e.g. `sysinfo`-equivalent shell parsing) beyond
//! what's built so far, so they stay stubs until those stages land instead
//! of faking a shape nothing has verified yet.
use crate::errors::{AppError, AppResult};
use crate::ssh::client::SshSession;
use crate::transport::{CommandOutput, ProcessSummary, RemoteFileEntry, ServerConnection, ServerMetrics};

#[async_trait::async_trait]
impl ServerConnection for SshSession {
    async fn execute_command(&self, command: &str) -> AppResult<CommandOutput> {
        self.execute_command(command).await
    }

    async fn get_metrics(&self) -> AppResult<ServerMetrics> {
        Err(not_yet_implemented("get_metrics"))
    }

    async fn list_processes(&self) -> AppResult<Vec<ProcessSummary>> {
        Err(not_yet_implemented("list_processes"))
    }

    async fn restart_service(&self, _service_name: &str) -> AppResult<()> {
        Err(not_yet_implemented("restart_service"))
    }

    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>> {
        self.list_directory(path).await
    }

    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>> {
        self.read_file(path).await
    }

    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()> {
        self.write_file(path, contents).await
    }
}

fn not_yet_implemented(what: &str) -> AppError {
    AppError::Internal(format!("SSH {what} isn't implemented yet - Etap 3 only covers running commands"))
}
