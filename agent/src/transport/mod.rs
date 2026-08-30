//! Desktop <-> Agent transport (Etap D), TLS (Etap K), plus the local
//! pairing control endpoint (Etap E). axum covers both the WebSocket
//! realtime channel and the request/response HTTP endpoints, so there's
//! one server library, not two - though the *public* WS server and the
//! *local-only* control server below are still two separate listeners,
//! deliberately, so a change to the public bind address can never
//! accidentally expose pairing control. Only the public server is TLS -
//! the control endpoint never leaves loopback, so there's no network path
//! for TLS to protect there (see `main.rs`'s startup check, which refuses
//! to run at all if that assumption is ever violated).

mod connection;
mod control;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use tokio::net::TcpListener;

use crate::info::AgentInfo;
use crate::pairing::PairingRegistry;

pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// Etap J: "nie wysyłaj danych absurdalnie często" - every 5s is frequent
/// enough for a "realtime" dashboard to feel live without turning a chatty
/// connection into a de facto flood.
pub const DEFAULT_METRICS_INTERVAL: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT_SECS: u64 = 10;

#[derive(Clone)]
pub struct SharedState {
    pub info: Arc<AgentInfo>,
    pub data_dir: PathBuf,
    pub pairing: PairingRegistry,
    /// Fields (not consts) so tests can use short intervals instead of
    /// waiting out the real production cadence.
    pub heartbeat_interval: Duration,
    pub metrics_interval: Duration,
}

pub fn router(state: SharedState) -> Router {
    Router::new().route("/ws", get(ws_handler)).with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<SharedState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| connection::handle(socket, state))
}

/// Serves the public WebSocket endpoint over TLS forever on an
/// already-bound listener. Splitting bind from serve lets tests bind
/// `127.0.0.1:0`, read back the OS-assigned port via
/// `TcpListener::local_addr()`, and only then start accepting connections.
pub async fn serve(listener: TcpListener, state: SharedState, tls_config: RustlsConfig) -> std::io::Result<()> {
    let std_listener = listener.into_std()?;
    axum_server::from_tcp_rustls(std_listener, tls_config)
        .serve(router(state).into_make_service())
        .await
}

/// Serves the local-only pairing control endpoint. Callers must only ever
/// bind this to a loopback address - see `control::router` for why that's
/// not merely a convention here.
pub async fn serve_control(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    axum::serve(listener, control::router(state)).await
}
