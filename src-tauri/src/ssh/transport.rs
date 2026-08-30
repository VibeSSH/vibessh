//! Adapts `SshSession` (pure protocol mechanics, see `client.rs`/`sftp.rs`/
//! `monitor.rs`) to the app's transport-agnostic `ServerConnection` trait.
//! Everything except `restart_service` is real now - systemd unit control
//! is a later stage (Quick Actions) that needs its own remote-side
//! mechanics beyond what's built so far, so it stays a stub until that
//! stage lands instead of faking a shape nothing has verified yet.
use crate::errors::{AppError, AppResult};
use crate::ssh::client::SshSession;
use crate::transport::{CommandOutput, ProcessSummary, RemoteFileEntry, ServerConnection, ServerMetrics};

#[async_trait::async_trait]
impl ServerConnection for SshSession {
    async fn execute_command(&self, command: &str) -> AppResult<CommandOutput> {
        self.execute_command(command).await
    }

    async fn get_metrics(&self) -> AppResult<ServerMetrics> {
        self.get_metrics().await
    }

    async fn list_processes(&self) -> AppResult<Vec<ProcessSummary>> {
        self.list_processes().await
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
