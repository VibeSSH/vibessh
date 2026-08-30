use std::sync::Arc;

use tokio::net::TcpListener;

use vibe_agent::cli;
use vibe_agent::config::AgentConfig;
use vibe_agent::identity::AgentIdentity;
use vibe_agent::info::AgentInfo;
use vibe_agent::pairing::PairingRegistry;
use vibe_agent::transport::{self, SharedState};
use vibe_agent::DEFAULT_CONTROL_BIND;

/// No TLS and no enforced auth yet (Etap D/E territory) - binding to
/// loopback only until pairing (Etap E) and the security review (Etap K)
/// land keeps an accidentally-exposed dev instance from being reachable
/// over the network.
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:7420";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("pair") {
        cli::pair(args.get(2).map(String::as_str).unwrap_or_default());
        return;
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the tokio runtime")
        .block_on(run_daemon());
}

async fn run_daemon() {
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

    let control_addr =
        std::env::var("VIBESSH_AGENT_CONTROL_BIND").unwrap_or_else(|_| DEFAULT_CONTROL_BIND.to_string());
    let control_listener = match TcpListener::bind(&control_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("failed to bind control endpoint {control_addr}: {err}");
            std::process::exit(1);
        }
    };

    log::info!("listening on {} (ws://.../ws)", listener.local_addr().unwrap());
    log::info!(
        "pairing control on {} (loopback only)",
        control_listener.local_addr().unwrap()
    );
    if vibe_agent::pairing::has_paired_credential(&config.data_dir) {
        log::info!("already paired - reconnecting desktops must present the stored credential");
    } else {
        log::info!("not paired yet - run `vibe-agent pair <code>` with the code shown on the desktop");
    }
    log::info!("Press Ctrl+C to stop.");

    let state = SharedState {
        info,
        data_dir: config.data_dir,
        pairing: PairingRegistry::new(),
        heartbeat_interval: transport::DEFAULT_HEARTBEAT_INTERVAL,
    };

    tokio::select! {
        result = transport::serve(listener, state.clone()) => {
            if let Err(err) = result {
                log::error!("server error: {err}");
            }
        }
        result = transport::serve_control(control_listener, state) => {
            if let Err(err) = result {
                log::error!("control server error: {err}");
            }
        }
        _ = tokio::signal::ctrl_c() => {}
    }

    log::info!("shutting down");
}
