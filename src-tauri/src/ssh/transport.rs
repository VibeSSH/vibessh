//! Adapts `SshSession` (pure protocol mechanics, see `client.rs`/`sftp.rs`/
//! `monitor.rs`/`systemd.rs`/`docker.rs`) to the app's transport-agnostic
//! `ServerConnection` trait. Every method is real - none of this is a stub
//! standing in for a later stage.
use crate::errors::AppResult;
use crate::ssh::client::SshSession;
use crate::transport::{
    CommandOutput, ContainerSummary, ProcessSummary, RemoteFileEntry, ServerConnection, ServerMetrics, ServiceSummary,
};

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

    async fn list_services(&self) -> AppResult<Vec<ServiceSummary>> {
        self.list_services().await
    }

    async fn restart_service(&self, service_name: &str) -> AppResult<()> {
        self.restart_service(service_name).await
    }

    async fn start_service(&self, service_name: &str) -> AppResult<()> {
        self.start_service(service_name).await
    }

    async fn stop_service(&self, service_name: &str) -> AppResult<()> {
        self.stop_service(service_name).await
    }

    async fn enable_service(&self, service_name: &str) -> AppResult<()> {
        self.enable_service(service_name).await
    }

    async fn disable_service(&self, service_name: &str) -> AppResult<()> {
        self.disable_service(service_name).await
    }

    async fn list_containers(&self) -> AppResult<Vec<ContainerSummary>> {
        self.list_containers().await
    }

    async fn restart_container(&self, container: &str) -> AppResult<()> {
        self.restart_container(container).await
    }

    async fn start_container(&self, container: &str) -> AppResult<()> {
        self.start_container(container).await
    }

    async fn stop_container(&self, container: &str) -> AppResult<()> {
        self.stop_container(container).await
    }

    async fn remove_container(&self, container: &str) -> AppResult<()> {
        self.remove_container(container).await
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
