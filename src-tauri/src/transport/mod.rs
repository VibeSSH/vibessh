pub use vibessh_protocol::{CommandOutput, ProcessSummary, RemoteFileEntry, ServerMetrics};

use crate::errors::AppResult;

/// Everything the app can do to a remote server, independent of how it gets
/// there. `SshTransport` and `AgentTransport` are the two implementations;
/// commands and services hold a `Box<dyn ServerConnection>` and never know
/// or care which one they got. This is the abstraction Etap B exists to
/// introduce, so that SSH mode and Agent mode never require an
/// `if connection_mode == ...` outside of the one place that picks which
/// transport to construct.
#[async_trait::async_trait]
pub trait ServerConnection: Send + Sync {
    async fn execute_command(&self, command: &str) -> AppResult<CommandOutput>;
    async fn get_metrics(&self) -> AppResult<ServerMetrics>;
    async fn list_processes(&self) -> AppResult<Vec<ProcessSummary>>;
    async fn restart_service(&self, service_name: &str) -> AppResult<()>;
    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>>;
    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>>;
    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::AppError;

    /// Proves the trait is object-safe (`Box<dyn ServerConnection>` compiles)
    /// and usable from async code, without needing a real SSH or Agent
    /// implementation yet.
    struct StubConnection;

    #[async_trait::async_trait]
    impl ServerConnection for StubConnection {
        async fn execute_command(&self, _command: &str) -> AppResult<CommandOutput> {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: "stub".into(),
                stderr: String::new(),
            })
        }

        async fn get_metrics(&self) -> AppResult<ServerMetrics> {
            Err(AppError::Internal("not implemented in stub".into()))
        }

        async fn list_processes(&self) -> AppResult<Vec<ProcessSummary>> {
            Ok(vec![])
        }

        async fn restart_service(&self, _service_name: &str) -> AppResult<()> {
            Ok(())
        }

        async fn list_directory(&self, _path: &str) -> AppResult<Vec<RemoteFileEntry>> {
            Ok(vec![])
        }

        async fn read_file(&self, _path: &str) -> AppResult<Vec<u8>> {
            Ok(vec![])
        }

        async fn write_file(&self, _path: &str, _contents: &[u8]) -> AppResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn stub_connection_is_object_safe_and_callable() {
        let conn: Box<dyn ServerConnection> = Box::new(StubConnection);
        let output = conn.execute_command("echo hi").await.unwrap();
        assert_eq!(output.stdout, "stub");
    }
}
