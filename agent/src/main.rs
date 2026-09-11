use std::sync::Arc;

use axum_server::tls_rustls::RustlsConfig;
use tokio::net::TcpListener;

use vibe_agent::cli;
use vibe_agent::config::AgentConfig;
use vibe_agent::identity::AgentIdentity;
use vibe_agent::info::AgentInfo;
use vibe_agent::pairing::PairingRegistry;
use vibe_agent::transport::{self, SharedState};
use vibe_agent::DEFAULT_CONTROL_BIND;

/// Binds to all interfaces by default: this is the endpoint a desktop
/// somewhere else on the internet actually needs to reach. TLS (Etap K)
/// and pairing-code/credential auth are what make that safe to expose, not
/// network placement.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:7420";

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

    // Etap K: the control endpoint has no authentication of its own at all
    // - its entire security model is "loopback-only, so reaching it already
    // implies shell-level trust" (see docs/security/agent-privileges.md). That was
    // previously only a comment; a typo'd or copy-pasted
    // VIBESSH_AGENT_CONTROL_BIND would have silently exposed pairing
    // control to the network with zero auth. Refusing to start is the fix.
    let control_ip = control_listener.local_addr().unwrap().ip();
    if !control_ip.is_loopback() {
        log::error!(
            "refusing to start: pairing control endpoint is bound to {control_ip}, which is not loopback. \
             This endpoint has no authentication - it must never be reachable from the network. \
             Check VIBESSH_AGENT_CONTROL_BIND."
        );
        std::process::exit(1);
    }

    let tls_paths = match vibe_agent::tls::load_or_create(&config.data_dir) {
        Ok(paths) => paths,
        Err(err) => {
            log::error!("failed to load or create the TLS certificate: {err}");
            std::process::exit(1);
        }
    };
    let tls_config = match RustlsConfig::from_pem_file(&tls_paths.cert_path, &tls_paths.key_path).await {
        Ok(config) => config,
        Err(err) => {
            log::error!("failed to load the TLS certificate into the server: {err}");
            std::process::exit(1);
        }
    };

    log::info!("listening on {} (wss://.../ws)", listener.local_addr().unwrap());
    log::info!(
        "pairing control on {} (loopback only, enforced)",
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
        metrics_interval: transport::DEFAULT_METRICS_INTERVAL,
    };

    tokio::select! {
        result = transport::serve(listener, state.clone(), tls_config) => {
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
