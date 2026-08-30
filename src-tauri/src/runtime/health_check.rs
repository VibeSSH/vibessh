//! The shared logic behind every runtime's `health_check()` beyond "is the
//! process still running" - see `HealthCheckSpec`'s own doc comment
//! (`runtime::mod`) for what each variant means. Exists as its own module so
//! the actual probing logic (a TCP connect, an HTTP GET, a Minecraft Server
//! List Ping) is written once instead of once per `ApplicationRuntime`
//! implementation.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationLocation, ApplicationStatus};

use super::{HealthCheckSpec, HealthStatus, RuntimeContext};

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Every runtime's `health_check()` boils down to this: check status first
/// (the one thing every runtime already knows how to do), and only probe
/// further if the process is actually `Running` - a stopped or unknown
/// process reports `Unknown` regardless of `spec`, and a `Failed` one
/// reports `Unhealthy` with `failed_reason` without attempting a network
/// probe against a process that isn't there. `failed_reason` is `None` for
/// `RemoteProcessRuntime`, which has no way to observe a `Failed` state at
/// all (see that module's own doc comment) and so never calls this with
/// `status == Failed` in the first place.
pub async fn default_health_check(
    ctx: &RuntimeContext<'_>,
    spec: &HealthCheckSpec,
    status: ApplicationStatus,
    failed_reason: Option<&str>,
) -> AppResult<HealthStatus> {
    match status {
        ApplicationStatus::Failed => {
            return Ok(HealthStatus::Unhealthy { reason: failed_reason.unwrap_or("the application failed").to_string() });
        }
        ApplicationStatus::Running => {}
        _ => return Ok(HealthStatus::Unknown),
    }
    match spec {
        HealthCheckSpec::Process => Ok(HealthStatus::Healthy),
        HealthCheckSpec::Tcp { port } => check_tcp(ctx, *port).await,
        HealthCheckSpec::Http { port, path } => check_http(ctx, *port, path).await,
        HealthCheckSpec::MinecraftStatus { host, port } => check_minecraft_status(host, *port).await,
    }
}

/// Dialed from wherever the application actually runs, not from the VibeSSH
/// desktop - a health port bound to loopback (common; it often isn't meant
/// to be reachable from outside) would otherwise always read as closed for
/// a Remote application. Local dials `127.0.0.1` directly; Remote reuses the
/// application's own SSH connection and asks the remote shell to dial its
/// own loopback via bash's `/dev/tcp` device (needs a bash with that
/// built-in on the remote host - a documented requirement, not silently
/// assumed away).
pub async fn check_tcp(ctx: &RuntimeContext<'_>, port: u16) -> AppResult<HealthStatus> {
    match ctx.application.location() {
        ApplicationLocation::Local => Ok(tcp_connect_local(port).await),
        ApplicationLocation::Remote => tcp_connect_remote(ctx, port).await,
    }
}

async fn tcp_connect_local(port: u16) -> HealthStatus {
    match timeout(CHECK_TIMEOUT, tokio::net::TcpStream::connect(("127.0.0.1", port))).await {
        Ok(Ok(_)) => HealthStatus::Healthy,
        Ok(Err(err)) => HealthStatus::Unhealthy { reason: format!("couldn't connect to port {port}: {err}") },
        Err(_) => HealthStatus::Unhealthy { reason: format!("timed out connecting to port {port}") },
    }
}

async fn tcp_connect_remote(ctx: &RuntimeContext<'_>, port: u16) -> AppResult<HealthStatus> {
    let connection = connection_ref(ctx)?;
    let command = format!("timeout 5 bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port}' 2>/dev/null");
    let output = connection.execute_command(&command).await?;
    Ok(if output.exit_code == 0 {
        HealthStatus::Healthy
    } else {
        HealthStatus::Unhealthy { reason: format!("couldn't connect to port {port} on the remote host") }
    })
}

/// Same "from wherever the app runs" reasoning as `check_tcp`. Local sends a
/// direct `reqwest` request; Remote shells out to `curl` over the same SSH
/// connection (needs `curl` on the remote host - again a documented
/// requirement, not assumed).
pub async fn check_http(ctx: &RuntimeContext<'_>, port: u16, path: &str) -> AppResult<HealthStatus> {
    match ctx.application.location() {
        ApplicationLocation::Local => Ok(http_get_local(port, path).await),
        ApplicationLocation::Remote => http_get_remote(ctx, port, path).await,
    }
}

async fn http_get_local(port: u16, path: &str) -> HealthStatus {
    let url = format!("http://127.0.0.1:{port}{path}");
    let client = match reqwest::Client::builder().timeout(CHECK_TIMEOUT).build() {
        Ok(client) => client,
        Err(err) => return HealthStatus::Unhealthy { reason: format!("couldn't build an HTTP client: {err}") },
    };
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() || response.status().is_redirection() => HealthStatus::Healthy,
        Ok(response) => HealthStatus::Unhealthy { reason: format!("{url} responded with HTTP {}", response.status()) },
        Err(err) => HealthStatus::Unhealthy { reason: format!("couldn't reach {url}: {err}") },
    }
}

async fn http_get_remote(ctx: &RuntimeContext<'_>, port: u16, path: &str) -> AppResult<HealthStatus> {
    let connection = connection_ref(ctx)?;
    let url = format!("http://127.0.0.1:{port}{path}");
    let command = format!("curl -s -o /dev/null -w '%{{http_code}}' --max-time 5 {}", shell_quote(&url));
    let output = connection.execute_command(&command).await?;
    let code: u32 = output.stdout.trim().parse().unwrap_or(0);
    Ok(if (200..400).contains(&code) {
        HealthStatus::Healthy
    } else {
        HealthStatus::Unhealthy { reason: format!("{url} responded with HTTP {code}") }
    })
}

fn connection_ref<'a>(ctx: &'a RuntimeContext<'_>) -> AppResult<&'a crate::ssh::SshSession> {
    ctx.connection.as_deref().ok_or_else(|| AppError::Internal("a remote health check requires a connection".into()))
}

/// POSIX single-quote shell escaping - see `runtime::remote_process`'s copy
/// of the same function for the full reasoning; duplicated rather than
/// shared, same as it's already duplicated across several modules in this
/// codebase.
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

/// The real Minecraft Server List Ping protocol (post-1.7, VarInt-framed) -
/// see https://minecraft.wiki/w/Java_Edition_protocol/Server_List_Ping for
/// the wire format this implements. Always dialed directly from the VibeSSH
/// desktop, never via SSH (unlike `check_tcp`/`check_http`) - Minecraft
/// servers are near-universally meant to accept connections from outside
/// the host they run on (that's the entire point of the protocol this
/// function speaks), so there's no "only reachable from localhost" concern
/// to route around. `host` is `127.0.0.1` for a Local application or the
/// target Server's own `host` for Remote, resolved by the caller
/// (`application_service::resolve_health_check_spec`), not here.
pub async fn check_minecraft_status(host: &str, port: u16) -> AppResult<HealthStatus> {
    match timeout(CHECK_TIMEOUT, ping_minecraft(host, port)).await {
        Ok(Ok(())) => Ok(HealthStatus::Healthy),
        Ok(Err(reason)) => Ok(HealthStatus::Unhealthy { reason }),
        Err(_) => Ok(HealthStatus::Unhealthy { reason: format!("timed out pinging {host}:{port}") }),
    }
}

/// Returns `Ok(())` once a well-formed Status Response (a JSON object) comes
/// back - a health check only needs "is this actually a Minecraft server
/// that's alive," not the parsed player count/MOTD (there's no "server
/// info" UI to feed that to yet).
async fn ping_minecraft(host: &str, port: u16) -> Result<(), String> {
    let mut stream = tokio::net::TcpStream::connect((host, port)).await.map_err(|err| format!("couldn't connect to {host}:{port}: {err}"))?;

    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0x00); // packet id
    write_varint(&mut handshake, -1); // protocol version: -1 ("unknown") is universally accepted for a status-only ping
    write_string(&mut handshake, host);
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1); // next state: 1 = status
    write_framed_packet(&mut stream, &handshake).await.map_err(|err| format!("couldn't send handshake: {err}"))?;

    let mut status_request = Vec::new();
    write_varint(&mut status_request, 0x00); // packet id, empty body
    write_framed_packet(&mut stream, &status_request).await.map_err(|err| format!("couldn't send status request: {err}"))?;

    let packet = read_framed_packet(&mut stream).await.map_err(|err| format!("couldn't read status response: {err}"))?;
    let mut cursor = packet.as_slice();
    let packet_id = read_varint_from_slice(&mut cursor)?;
    if packet_id != 0x00 {
        return Err(format!("unexpected packet id {packet_id} in status response"));
    }
    let json = read_string_from_slice(&mut cursor)?;
    serde_json::from_str::<serde_json::Value>(&json).map_err(|err| format!("malformed status response: {err}"))?;
    Ok(())
}

async fn write_framed_packet<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, body: &[u8]) -> std::io::Result<()> {
    let mut framed = Vec::new();
    write_varint(&mut framed, body.len() as i32);
    framed.extend_from_slice(body);
    writer.write_all(&framed).await
}

async fn read_framed_packet<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let length = read_varint_async(reader).await?;
    if length < 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "negative packet length"));
    }
    let mut buf = vec![0u8; length as usize];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn read_varint_async<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<i32> {
    let mut result: i32 = 0;
    let mut shift = 0u32;
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).await?;
        let byte = byte[0];
        result |= ((byte & 0x7F) as i32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt is too long"));
        }
    }
    Ok(result)
}

/// VarInt encoding per the Minecraft protocol spec: 7 data bits per byte,
/// high bit set on every byte but the last. A negative `value` (only used
/// here for the handshake's "unknown protocol version" `-1`) encodes as its
/// 32-bit unsigned bit pattern, which always takes the full 5 bytes - that's
/// the spec's own defined behavior, not a bug.
fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_string(buf: &mut Vec<u8>, value: &str) {
    write_varint(buf, value.len() as i32);
    buf.extend_from_slice(value.as_bytes());
}

fn read_varint_from_slice(cursor: &mut &[u8]) -> Result<i32, String> {
    let mut result: i32 = 0;
    let mut shift = 0u32;
    loop {
        let (&byte, rest) = cursor.split_first().ok_or("VarInt truncated")?;
        *cursor = rest;
        result |= ((byte & 0x7F) as i32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err("VarInt is too long".to_string());
        }
    }
    Ok(result)
}

fn read_string_from_slice(cursor: &mut &[u8]) -> Result<String, String> {
    let length = read_varint_from_slice(cursor)?;
    if length < 0 || (length as usize) > cursor.len() {
        return Err("string length out of bounds".to_string());
    }
    let (bytes, rest) = cursor.split_at(length as usize);
    *cursor = rest;
    String::from_utf8(bytes.to_vec()).map_err(|err| format!("invalid UTF-8 in string: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Application, HealthCheckType, RuntimeType};
    use tokio::net::TcpListener;

    fn stub_application(server_id: Option<uuid::Uuid>) -> Application {
        Application {
            id: uuid::Uuid::new_v4(),
            server_id,
            name: "Stub".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::LocalProcess,
            working_directory: "/tmp".to_string(),
            status: ApplicationStatus::Unknown,
            last_status_check_at: None,
            health_check_type: HealthCheckType::Process,
            health_check_port_id: None,
            health_check_http_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn varint_round_trips_through_write_and_slice_read_for_representative_values() {
        for value in [0i32, 1, 127, 128, 255, 2097151, 25565, i32::MAX, -1, i32::MIN] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            let mut cursor = buf.as_slice();
            assert_eq!(read_varint_from_slice(&mut cursor).unwrap(), value);
            assert!(cursor.is_empty(), "value {value} left {} unread bytes", cursor.len());
        }
    }

    #[test]
    fn string_round_trips_through_write_and_slice_read() {
        let mut buf = Vec::new();
        write_string(&mut buf, "play.example.com");
        let mut cursor = buf.as_slice();
        assert_eq!(read_string_from_slice(&mut cursor).unwrap(), "play.example.com");
        assert!(cursor.is_empty());
    }

    #[tokio::test]
    async fn default_health_check_reports_unknown_without_probing_when_the_process_isnt_running() {
        let application = stub_application(None);
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        // `Tcp { port: 1 }` would fail to connect if actually probed (nothing
        // listens on port 1) - reaching `Unknown` here instead of
        // `Unhealthy` proves the probe was skipped entirely.
        let result = default_health_check(&ctx, &HealthCheckSpec::Tcp { port: 1 }, ApplicationStatus::Stopped, None).await.unwrap();
        assert!(matches!(result, HealthStatus::Unknown));
    }

    #[tokio::test]
    async fn default_health_check_reports_unhealthy_with_the_given_reason_when_failed() {
        let application = stub_application(None);
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        let result = default_health_check(&ctx, &HealthCheckSpec::Process, ApplicationStatus::Failed, Some("exited with status 1")).await.unwrap();
        match result {
            HealthStatus::Unhealthy { reason } => assert_eq!(reason, "exited with status 1"),
            other => panic!("expected Unhealthy, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn default_health_check_reports_healthy_for_a_running_process_check() {
        let application = stub_application(None);
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        let result = default_health_check(&ctx, &HealthCheckSpec::Process, ApplicationStatus::Running, None).await.unwrap();
        assert!(matches!(result, HealthStatus::Healthy));
    }

    #[tokio::test]
    async fn check_tcp_locally_reports_healthy_against_a_real_listening_port_and_unhealthy_against_a_closed_one() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let application = stub_application(None);
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };

        assert!(matches!(check_tcp(&ctx, port).await.unwrap(), HealthStatus::Healthy));

        // A port nothing is listening on (bind-then-immediately-drop to get
        // a real, currently-unused ephemeral port rather than guessing one).
        let closed_port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        assert!(matches!(check_tcp(&ctx, closed_port).await.unwrap(), HealthStatus::Unhealthy { .. }));
    }

    #[tokio::test]
    async fn check_http_locally_reports_healthy_against_a_real_200_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
        });

        let application = stub_application(None);
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        assert!(matches!(check_http(&ctx, port, "/health").await.unwrap(), HealthStatus::Healthy));
    }

    /// A minimal fake Minecraft server: reads (and discards) the handshake
    /// and status-request packets a real client sends, then writes back one
    /// framed Status Response packet with a canned JSON body - exactly the
    /// wire shape `ping_minecraft` expects, built with the same
    /// `write_framed_packet`/`write_varint`/`write_string` helpers under
    /// test, so a passing test here proves the client and server sides
    /// agree on the framing.
    async fn spawn_fake_minecraft_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let _ = read_framed_packet(&mut socket).await; // handshake
            let _ = read_framed_packet(&mut socket).await; // status request

            let mut body = Vec::new();
            write_varint(&mut body, 0x00);
            write_string(&mut body, r#"{"version":{"name":"1.21","protocol":767},"players":{"max":20,"online":0},"description":{"text":"A test server"}}"#);
            let _ = write_framed_packet(&mut socket, &body).await;
        });
        port
    }

    #[tokio::test]
    async fn check_minecraft_status_reports_healthy_against_a_real_status_response() {
        let port = spawn_fake_minecraft_server().await;
        assert!(matches!(check_minecraft_status("127.0.0.1", port).await.unwrap(), HealthStatus::Healthy));
    }

    #[tokio::test]
    async fn check_minecraft_status_reports_unhealthy_when_nothing_is_listening() {
        let closed_port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        assert!(matches!(check_minecraft_status("127.0.0.1", closed_port).await.unwrap(), HealthStatus::Unhealthy { .. }));
    }

    #[tokio::test]
    async fn tcp_and_http_checks_on_a_remote_application_without_a_connection_are_an_internal_error_not_a_panic() {
        let application = stub_application(Some(uuid::Uuid::new_v4()));
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        assert!(check_tcp(&ctx, 25565).await.is_err());
        assert!(check_http(&ctx, 8080, "/").await.is_err());
    }
}
