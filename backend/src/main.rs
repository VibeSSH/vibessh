//! VibeSSH Backend entry point.
//!
//! This is the P0 foundation's accounts stage on top of last stage's
//! scaffold: connects to Postgres, runs migrations, and serves
//! register/login/refresh/logout/me alongside the existing health check.
//! Router-building logic lives in `lib.rs` so it's directly testable (see
//! `tests/`) without going through a spawned process.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use vibessh_backend::{build_router, connect_and_migrate};

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let database_url = std::env::var("DATABASE_URL").map_err(|_| {
        "DATABASE_URL is not set - e.g. postgres://vibessh_app:PASSWORD@localhost:5432/vibessh".to_string()
    })?;
    let jwt_secret = std::env::var("JWT_SECRET").map_err(|_| {
        "JWT_SECRET is not set - generate one with e.g. `openssl rand -base64 48`".to_string()
    })?;
    if jwt_secret.len() < 32 {
        return Err("JWT_SECRET is too short - use at least 32 bytes of real randomness".to_string());
    }
    let bind_addr: SocketAddr = std::env::var("VIBESSH_BACKEND_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
        .parse()
        .map_err(|err| format!("invalid VIBESSH_BACKEND_BIND: {err}"))?;

    log::info!("connecting to the database...");
    let db = connect_and_migrate(&database_url).await?;
    let app = build_router(db, Arc::from(jwt_secret.into_bytes()));

    log::info!("listening on http://{bind_addr}");
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|err| format!("failed to bind {bind_addr}: {err}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| format!("server error: {err}"))?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    log::info!("shutting down");
}
