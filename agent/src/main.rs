use std::sync::Arc;

use tokio::net::TcpListener;

use vibe_agent::config::AgentConfig;
use vibe_agent::identity::AgentIdentity;
use vibe_agent::info::AgentInfo;
use vibe_agent::transport::{self, SharedState};

/// No TLS and no enforced auth yet (Etap D/E territory) - binding to
/// loopback only until pairing (Etap E) and the security review (Etap K)
/// land keeps an accidentally-exposed dev instance from being reachable
/// over the network.
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:7420";

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = AgentConfig::resolve();
    let identity = match AgentIdentity::load_or_create(&config.data_dir) {
        Ok(identity) => identity,
        Err(err) => {
            log::error!("failed to load or create agent identity: {err}");
            std::process::exit(1);
        }
    };

    let info = Arc::new(AgentInfo::collect(&identity));
    log::info!("Vibe Agent starting");
    log::info!("  id:       {}", info.id);
    log::info!("  version:  {}", info.version);
    log::info!("  hostname: {}", info.hostname);
    log::info!("  os:       {}", info.os);
    log::info!("data dir:   {}", config.data_dir.display());
    log::info!("config dir: {}", config.config_dir.display());

    let bind_addr = std::env::var("VIBESSH_AGENT_BIND").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("failed to bind {bind_addr}: {err}");
            std::process::exit(1);
        }
    };
    log::info!("listening on {} (ws://.../ws)", listener.local_addr().unwrap());
    log::info!("No pairing yet (Etap E) - handshake accepts any client. Press Ctrl+C to stop.");

    let state = SharedState {
        info,
        heartbeat_interval: transport::DEFAULT_HEARTBEAT_INTERVAL,
    };

    tokio::select! {
        result = transport::serve(listener, state) => {
            if let Err(err) = result {
                log::error!("server error: {err}");
            }
        }
        _ = tokio::signal::ctrl_c() => {}
    }

    log::info!("shutting down");
}
