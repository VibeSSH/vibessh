//! A reachability check for the Servers page's status dot - not an ICMP
//! ping (that needs raw sockets, which means elevated privileges on
//! Windows), but a TCP connect timing against the server's SSH port. That's
//! both a more honest measurement for an SSH client (it answers "can I
//! actually open a connection here", not just "does the host answer ICMP",
//! which firewalls frequently block anyway) and it needs no credentials, so
//! it works for a server nothing has authenticated to yet.

use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::time::timeout;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::storage::server_repository::ServerRepository;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn ping_server(repo: &ServerRepository, server_id: Uuid) -> AppResult<u32> {
    let server = repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    ping(&server.host, server.ssh_port).await
}

async fn ping(host: &str, port: u16) -> AppResult<u32> {
    let start = Instant::now();
    match timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port))).await {
        Ok(Ok(_stream)) => Ok(start.elapsed().as_millis() as u32),
        Ok(Err(err)) => Err(AppError::Connection(format!("couldn't reach {host}:{port}: {err}"))),
        Err(_) => Err(AppError::Connection(format!("timed out reaching {host}:{port}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn pings_a_reachable_local_port_successfully() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                if listener.accept().await.is_err() {
                    break;
                }
            }
        });

        let latency = ping("127.0.0.1", port).await.expect("should reach the listening port");
        assert!(latency < 3000, "expected a fast local connect, got {latency}ms");
    }

    #[tokio::test]
    async fn errors_cleanly_on_a_port_nothing_is_listening_on() {
        // Bind then immediately drop - OS-assigned ephemeral port that was
        // just proven free, and now has nothing listening on it.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = ping("127.0.0.1", port).await;
        assert!(result.is_err(), "connecting to a closed port should fail, not hang or succeed");
    }
}
