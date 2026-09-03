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

/// Loads `.env`, if there is one.
///
/// `.env.example` has always said "copy to .env for local development", and
/// until now nothing read it - so the variables lived only in whichever
/// shell happened to export them, and restarting the backend meant finding
/// that shell again or losing the configuration. That is not a small
/// inconvenience: it is the difference between a restart anybody can do and
/// a restart only one terminal window can.
///
/// Both locations are tried because the binary is run from either place:
/// `cargo run` from `backend/`, and `./target/release/vibessh-backend.exe`
/// from the repository root.
///
/// A real environment variable always wins over the file - that is
/// `dotenvy`'s own rule, and the right one: a value exported deliberately
/// for one run should not be overridden by a file somebody forgot about.
/// A missing file is not an error; the variables may well be set already.
fn load_dotenv() {
    for candidate in [".env", "backend/.env"] {
        if let Ok(path) = dotenvy::from_filename(candidate) {
            log::info!("loaded environment from {}", path.display());
            return;
        }
    }
}

async fn run() -> Result<(), String> {
    load_dotenv();

    let database_url = std::env::var("DATABASE_URL").map_err(|_| {
        "DATABASE_URL is not set - export it, or copy backend/.env.example to backend/.env and fill it in".to_string()
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

    // Built-in roles mean "every permission", so they are brought up to the
    // current catalog before anything serves a request - see
    // `roles::backfill_system_role_permissions` for why this is a boot step
    // rather than a migration.
    match vibessh_backend::roles::backfill_system_role_permissions(&db).await {
        Ok(0) => {}
        Ok(added) => log::info!("granted {added} newly-catalogued permission(s) to built-in roles"),
        Err(err) => return Err(format!("couldn't bring built-in roles up to the permission catalog: {err}")),
    }
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
