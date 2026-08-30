mod capabilities;
mod config;
mod errors;
mod identity;
mod info;
mod pairing;
mod transport;

use config::AgentConfig;
use identity::AgentIdentity;
use info::AgentInfo;

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

    let info = AgentInfo::collect(&identity);
    log::info!("Vibe Agent starting");
    log::info!("  id:       {}", info.id);
    log::info!("  version:  {}", info.version);
    log::info!("  hostname: {}", info.hostname);
    log::info!("  os:       {}", info.os);
    log::info!("  status:   {:?}", info.status);
    log::info!("data dir:   {}", config.data_dir.display());
    log::info!("config dir: {}", config.config_dir.display());
    log::info!("No pairing/transport yet (Etap D/E) - idling. Press Ctrl+C to stop.");

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    log::info!("shutting down");
}
