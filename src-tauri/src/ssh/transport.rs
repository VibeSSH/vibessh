//! Adapts `SshSession` (pure protocol mechanics, see `client.rs`) to the
//! app's transport-agnostic `ServerConnection` trait. Only `execute_command`
//! is real in Etap 3 - metrics/processes/service restart/SFTP are later
//! stages (process manager, systemd, SFTP) that need their own remote-side
//! mechanics (e.g. `sysinfo`-equivalent shell parsing) beyond "run a
//! command," so they stay honest stubs until those stages land instead of
//! faking a shape nothing has verified yet.
use crate::errors::{AppError, AppResult};
use crate::ssh::client::SshSession;
use crate::transport::{CommandOutput, ProcessSummary, ServerConnection, ServerMetrics};

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

    async fn read_file(&self, _path: &str) -> AppResult<Vec<u8>> {
        Err(not_yet_implemented("read_file"))
    }

    async fn write_file(&self, _path: &str, _contents: &[u8]) -> AppResult<()> {
        Err(not_yet_implemented("write_file"))
    }
}

fn not_yet_implemented(what: &str) -> AppError {
    AppError::Internal(format!("SSH {what} isn't implemented yet - Etap 3 only covers running commands"))
}
