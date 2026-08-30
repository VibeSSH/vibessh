//! The local pairing control endpoint. `vibe-agent pair <code>` (running as
//! a separate CLI invocation on the same machine) POSTs here to register a
//! code with the already-running daemon - it's the only way a pairing code
//! becomes valid.
//!
//! This is deliberately never bound to anything but 127.0.0.1: unlike the
//! public `/ws` endpoint, there is no authentication on this route at all.
//! Its security model is "whoever can reach loopback on this machine is
//! already as trusted as someone with a shell here" - true for a
//! single-tenant VPS, not for a shared multi-tenant host. `main.rs` hardcodes
//! the bind address rather than reading it from the same env var as the
//! public server, specifically so this can't be widened by accident.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use vibessh_protocol::PAIRING_CODE_TTL;

use super::SharedState;

pub fn router(state: SharedState) -> Router {
    Router::new().route("/internal/pair", post(pair)).with_state(state)
}

#[derive(Deserialize)]
struct PairRequest {
    code: String,
}

#[derive(Serialize)]
struct PairResponse {
    ok: bool,
    message: String,
}

async fn pair(State(state): State<SharedState>, Json(request): Json<PairRequest>) -> Json<PairResponse> {
    state.pairing.register(request.code, PAIRING_CODE_TTL);
    log::info!("agent: pairing code registered locally, valid for 5 minutes");
    Json(PairResponse {
        ok: true,
        message: "Pairing code active. Waiting for the desktop to connect...".to_string(),
    })
}
