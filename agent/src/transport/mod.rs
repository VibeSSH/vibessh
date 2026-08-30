//! Desktop <-> Agent transport (Etap D). Plain HTTP for now — TLS is
//! deliberately deferred to the Etap K security review rather than bolted
//! on here with a self-signed dev cert. axum covers both the WebSocket
//! realtime channel this stage implements and the request/response HTTP
//! endpoints later stages will add, so there's one server, not two.

mod connection;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

use crate::info::AgentInfo;

pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT_SECS: u64 = 10;

#[derive(Clone)]
pub struct SharedState {
    pub info: Arc<AgentInfo>,
    /// A field (not a const) so tests can use a short interval instead of
    /// waiting out the real production cadence.
    pub heartbeat_interval: Duration,
}

pub fn router(state: SharedState) -> Router {
    Router::new().route("/ws", get(ws_handler)).with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<SharedState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| connection::handle(socket, state))
}

/// Serves forever on an already-bound listener. Splitting bind from serve
/// lets tests bind `127.0.0.1:0`, read back the OS-assigned port via
/// `TcpListener::local_addr()`, and only then start accepting connections.
pub async fn serve(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}
