//! SSH tunnels (`ssh -L`/`-R`/`-D`) riding on an existing `SshSession`. Only
//! the raw protocol calls that need `self.handle`/`self.forward_registry`
//! directly live on `SshSession` itself (in `client.rs`, since
//! `client::Handle<TofuHandler>` can't be named outside that file -
//! `TofuHandler` is private); everything else - the accept loops, the byte
//! pump, the SOCKS5 handshake - lives here, working only through that small
//! `pub(super)` surface.

use std::net::SocketAddr;
use std::sync::Arc;

use russh::client;
use russh::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use super::client::SshSession;
use crate::errors::{AppError, AppResult};

/// A live tunnel's stop switch and the port it actually ended up bound to
/// (meaningful when `0` was requested). Dropping this - or letting it drop
/// when removed from `state::PortForwardManager` - closes the `oneshot`
/// sender, which is what the accept loop's `tokio::select!` is waiting on;
/// same "drop ends it" idiom `TerminalHandle` already uses.
pub struct PortForwardHandle {
    pub actual_port: u16,
    stop_tx: oneshot::Sender<()>,
}

impl PortForwardHandle {
    /// Stops the forward: closes the listener (Local/Dynamic) or cancels
    /// the Node-side listener (Remote), so no *new* connection is accepted
    /// from this point on. Already-open tunneled connections are left to
    /// finish on their own rather than force-closed - see `run_accept_loop`'s
    /// own doc comment for why. Consuming `self` is what actually signals
    /// the stop (dropping `stop_tx` resolves the accept loop's `stop_rx`) -
    /// letting a `PortForwardHandle` simply go out of scope (e.g. removed
    /// from `state::PortForwardManager`'s map) does the exact same thing.
    pub fn stop(self) {
        drop(self.stop_tx);
    }
}

impl SshSession {
    /// `ssh -L`: listens on the desktop, tunnels each connection to a fixed
    /// `target_host:target_port` reachable from the Node's side of the SSH
    /// connection.
    pub async fn start_local_forward(
        session: Arc<SshSession>,
        bind_address: &str,
        bind_port: u16,
        target_host: String,
        target_port: u16,
    ) -> AppResult<PortForwardHandle> {
        let listener = bind_listener(bind_address, bind_port).await?;
        let actual_port = local_port(&listener)?;
        let (stop_tx, stop_rx) = oneshot::channel();

        tokio::spawn(async move {
            run_accept_loop(listener, stop_rx, move |stream, peer| {
                let session = session.clone();
                let target_host = target_host.clone();
                async move {
                    pipe_to_target(&session, stream, &target_host, target_port, peer).await;
                }
            })
            .await;
        });

        Ok(PortForwardHandle { actual_port, stop_tx })
    }

    /// `ssh -D`: listens on the desktop as a minimal SOCKS5 proxy (no auth,
    /// CONNECT only) - the target is whatever each SOCKS client asks for,
    /// not a fixed host:port.
    pub async fn start_dynamic_forward(session: Arc<SshSession>, bind_address: &str, bind_port: u16) -> AppResult<PortForwardHandle> {
        let listener = bind_listener(bind_address, bind_port).await?;
        let actual_port = local_port(&listener)?;
        let (stop_tx, stop_rx) = oneshot::channel();

        tokio::spawn(async move {
            run_accept_loop(listener, stop_rx, move |mut stream, peer| {
                let session = session.clone();
                async move {
                    let target = match socks5_handshake(&mut stream).await {
                        Ok(target) => target,
                        Err(err) => {
                            log::warn!("SOCKS5 handshake from {peer} failed: {err}");
                            return;
                        }
                    };
                    pipe_to_target(&session, stream, &target.host, target.port, peer).await;
                }
            })
            .await;
        });

        Ok(PortForwardHandle { actual_port, stop_tx })
    }

    /// `ssh -R`: asks the Node to listen on its own side, tunnels each
    /// connection it accepts back to a fixed `target_host:target_port`
    /// reachable from the desktop.
    pub async fn start_remote_forward(
        session: Arc<SshSession>,
        bind_address: &str,
        bind_port: u16,
        target_host: String,
        target_port: u16,
    ) -> AppResult<PortForwardHandle> {
        let (actual_port, mut incoming) = session.register_remote_forward(bind_address, bind_port).await?;
        let (stop_tx, mut stop_rx) = oneshot::channel();

        let bind_address = bind_address.to_string();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    channel = incoming.recv() => {
                        let Some(channel) = channel else { break };
                        let target_host = target_host.clone();
                        tokio::spawn(async move {
                            pipe_forwarded_channel(channel, &target_host, target_port).await;
                        });
                    }
                }
            }
            session.unregister_remote_forward(&bind_address, actual_port).await;
        });

        Ok(PortForwardHandle { actual_port, stop_tx })
    }
}

async fn bind_listener(bind_address: &str, bind_port: u16) -> AppResult<TcpListener> {
    TcpListener::bind((bind_address, bind_port))
        .await
        .map_err(|err| AppError::InvalidInput(format!("couldn't listen on {bind_address}:{bind_port}: {err}")))
}

fn local_port(listener: &TcpListener) -> AppResult<u16> {
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|err| AppError::Internal(format!("couldn't read the bound port back: {err}")))
}

/// Shared accept loop for Local/Dynamic forward - both are "listen on the
/// desktop, hand each accepted connection to a per-connection handler",
/// differing only in how that handler decides the target (fixed vs. a
/// SOCKS5 handshake). Stops as soon as `stop_rx` resolves - already
/// in-flight connections are left to finish on their own rather than force-
/// closed, the same way closing a terminal's `TerminalHandle` doesn't reach
/// into an already-run remote command.
async fn run_accept_loop<F, Fut>(listener: TcpListener, mut stop_rx: oneshot::Receiver<()>, handle_connection: F)
where
    F: Fn(TcpStream, SocketAddr) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                tokio::spawn(handle_connection(stream, peer));
            }
        }
    }
}

/// Opens a `direct-tcpip` channel to `target_host:target_port` through the
/// SSH session and pumps bytes both ways until either side closes - the
/// common tail end of both Local and Dynamic forward's per-connection
/// handler.
async fn pipe_to_target(session: &SshSession, mut local: TcpStream, target_host: &str, target_port: u16, peer: SocketAddr) {
    let channel = match session.open_direct_tcpip(target_host, target_port, &peer.ip().to_string(), peer.port()).await {
        Ok(channel) => channel,
        Err(err) => {
            log::warn!("port forward: couldn't open a tunnel to {target_host}:{target_port}: {err}");
            return;
        }
    };
    let mut remote = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
}

/// The Remote-forward counterpart of `pipe_to_target`: the channel already
/// arrived from the Node (someone connected to the Node's forwarded port),
/// so this only needs to dial the local target and pump.
async fn pipe_forwarded_channel(channel: Channel<client::Msg>, target_host: &str, target_port: u16) {
    let mut remote = channel.into_stream();
    match TcpStream::connect((target_host, target_port)).await {
        Ok(mut local) => {
            let _ = tokio::io::copy_bidirectional(&mut remote, &mut local).await;
        }
        Err(err) => log::warn!("remote forward: couldn't reach {target_host}:{target_port}: {err}"),
    }
}

pub(super) struct Socks5Target {
    pub host: String,
    pub port: u16,
}

const SOCKS5_VERSION: u8 = 0x05;
const SOCKS5_CMD_CONNECT: u8 = 0x01;
const SOCKS5_ATYP_IPV4: u8 = 0x01;
const SOCKS5_ATYP_DOMAIN: u8 = 0x03;
const SOCKS5_ATYP_IPV6: u8 = 0x04;
/// SOCKS5 REP codes (RFC 1928 §6) this minimal server can actually produce.
const SOCKS5_REP_COMMAND_NOT_SUPPORTED: u8 = 0x07;
const SOCKS5_REP_ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;

/// A minimal SOCKS5 server handshake (RFC 1928): no-auth only, `CONNECT`
/// only - everything a desktop app or browser pointed at this proxy
/// actually sends in practice. `BND.ADDR`/`BND.PORT` in the success reply
/// are always `0.0.0.0:0`: there's no real local listener bound per
/// connection to report back (this proxy hands the connection straight to
/// an SSH `direct-tcpip` channel instead), and every SOCKS5 client this app
/// needs to support ignores that field once `REP == succeeded`.
pub(super) async fn socks5_handshake<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> AppResult<Socks5Target> {
    let mut greeting = [0u8; 2];
    read_exact_socks(stream, &mut greeting).await?;
    let [version, nmethods] = greeting;
    if version != SOCKS5_VERSION {
        return Err(AppError::InvalidInput(format!("unsupported SOCKS version {version} (only SOCKS5 is supported)")));
    }
    let mut methods = vec![0u8; nmethods as usize];
    read_exact_socks(stream, &mut methods).await?;
    stream
        .write_all(&[SOCKS5_VERSION, 0x00])
        .await
        .map_err(|err| AppError::Connection(format!("SOCKS5 handshake failed: {err}")))?;

    let mut request_header = [0u8; 4];
    read_exact_socks(stream, &mut request_header).await?;
    let [_version, cmd, _reserved, address_type] = request_header;

    let host = match address_type {
        SOCKS5_ATYP_IPV4 => {
            let mut octets = [0u8; 4];
            read_exact_socks(stream, &mut octets).await?;
            std::net::Ipv4Addr::from(octets).to_string()
        }
        SOCKS5_ATYP_DOMAIN => {
            let mut length = [0u8; 1];
            read_exact_socks(stream, &mut length).await?;
            let mut name = vec![0u8; length[0] as usize];
            read_exact_socks(stream, &mut name).await?;
            String::from_utf8(name).map_err(|_| AppError::InvalidInput("SOCKS5 request had a non-UTF-8 domain name".into()))?
        }
        SOCKS5_ATYP_IPV6 => {
            let mut octets = [0u8; 16];
            read_exact_socks(stream, &mut octets).await?;
            std::net::Ipv6Addr::from(octets).to_string()
        }
        other => {
            reply_socks5(stream, SOCKS5_REP_ADDRESS_TYPE_NOT_SUPPORTED).await;
            return Err(AppError::InvalidInput(format!("unsupported SOCKS5 address type {other}")));
        }
    };

    let mut port_bytes = [0u8; 2];
    read_exact_socks(stream, &mut port_bytes).await?;
    let port = u16::from_be_bytes(port_bytes);

    if cmd != SOCKS5_CMD_CONNECT {
        reply_socks5(stream, SOCKS5_REP_COMMAND_NOT_SUPPORTED).await;
        return Err(AppError::InvalidInput("only the SOCKS5 CONNECT command is supported".into()));
    }

    reply_socks5(stream, 0x00).await;
    Ok(Socks5Target { host, port })
}

async fn read_exact_socks<S: AsyncRead + Unpin>(stream: &mut S, buf: &mut [u8]) -> AppResult<()> {
    stream.read_exact(buf).await.map_err(|err| AppError::Connection(format!("SOCKS5 handshake failed: {err}")))?;
    Ok(())
}

async fn reply_socks5<S: AsyncWrite + Unpin>(stream: &mut S, rep: u8) {
    let _ = stream.write_all(&[SOCKS5_VERSION, rep, 0x00, SOCKS5_ATYP_IPV4, 0, 0, 0, 0, 0, 0]).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn socks5_handshake_parses_an_ipv4_connect_request() {
        let (mut client, mut server) = tokio::io::duplex(256);
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        client.write_all(&[0x05, 0x01, 0x00, 0x01, 93, 184, 216, 34, 0x00, 0x50]).await.unwrap();

        let target = socks5_handshake(&mut server).await.unwrap();
        assert_eq!(target.host, "93.184.216.34");
        assert_eq!(target.port, 80);

        let mut method_reply = [0u8; 2];
        client.read_exact(&mut method_reply).await.unwrap();
        assert_eq!(method_reply, [0x05, 0x00]);
        let mut connect_reply = [0u8; 10];
        client.read_exact(&mut connect_reply).await.unwrap();
        assert_eq!(connect_reply, [0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn socks5_handshake_parses_a_domain_name_connect_request() {
        let (mut client, mut server) = tokio::io::duplex(256);
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let domain = b"example.com";
        let mut request = vec![0x05, 0x01, 0x00, 0x03, domain.len() as u8];
        request.extend_from_slice(domain);
        request.extend_from_slice(&443u16.to_be_bytes());
        client.write_all(&request).await.unwrap();

        let target = socks5_handshake(&mut server).await.unwrap();
        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 443);
    }

    #[tokio::test]
    async fn socks5_handshake_rejects_a_non_connect_command() {
        let (mut client, mut server) = tokio::io::duplex(256);
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        // CMD 0x02 = BIND, not supported.
        client.write_all(&[0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x50]).await.unwrap();

        assert!(socks5_handshake(&mut server).await.is_err());
    }

    #[tokio::test]
    async fn socks5_handshake_rejects_a_pre_socks5_version() {
        let (mut client, mut server) = tokio::io::duplex(256);
        client.write_all(&[0x04, 0x01, 0x00]).await.unwrap();

        assert!(socks5_handshake(&mut server).await.is_err());
    }
}
