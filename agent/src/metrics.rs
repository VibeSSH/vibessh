//! Realtime metrics collection (Etap J) - the first real Agent Mode data
//! feature. A `MetricsCollector` is long-lived per connection (see
//! `transport::connection`) and refreshed on each tick rather than
//! recreated, because both CPU% and the network rates below are computed
//! from the delta since the *previous* refresh - a fresh collector every
//! call would report a meaningless first-sample number (0% CPU, 0 bytes/sec)
//! forever instead of once.

use std::time::Instant;

use sysinfo::{Disks, Networks, System};
use vibessh_protocol::ServerMetrics;

pub struct MetricsCollector {
    system: System,
    networks: Networks,
    last_sample_at: Instant,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        Self {
            system,
            networks: Networks::new_with_refreshed_list(),
            last_sample_at: Instant::now(),
        }
    }

    pub fn collect(&mut self) -> ServerMetrics {
        let elapsed_secs = self.last_sample_at.elapsed().as_secs_f64().max(0.001);
        self.last_sample_at = Instant::now();

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.networks.refresh(true);

        let (disk_used_bytes, disk_total_bytes) = root_disk_usage();
        let load = System::load_average();

        // Networks::received()/transmitted() are already the delta since
        // the last refresh (not a running total), so summing across
        // non-loopback interfaces and dividing by elapsed time is a real
        // rate, not something derived from two cumulative counters here.
        let (rx_delta, tx_delta) = self
            .networks
            .iter()
            .filter(|(name, _)| !name.starts_with("lo"))
            .fold((0u64, 0u64), |(rx, tx), (_, data)| {
                (rx + data.received(), tx + data.transmitted())
            });

        ServerMetrics {
            cpu_usage_percent: self.system.global_cpu_usage(),
            ram_used_bytes: self.system.used_memory(),
            ram_total_bytes: self.system.total_memory(),
            disk_used_bytes,
            disk_total_bytes,
            load_average_1m: load.one as f32,
            uptime_seconds: System::uptime(),
            network_rx_bytes_per_sec: (rx_delta as f64 / elapsed_secs) as u64,
            network_tx_bytes_per_sec: (tx_delta as f64 / elapsed_secs) as u64,
        }
    }
}

/// The filesystem mounted at "/" is the meaningful "how full is this
/// server" number for the typical single-disk VPS this agent targets.
/// Falls back to whatever sysinfo lists first (e.g. on a dev machine
/// without a "/" mount point) rather than reporting nothing.
fn root_disk_usage() -> (u64, u64) {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .find(|d| d.mount_point().as_os_str() == "/")
        .or_else(|| disks.iter().next())
        .map(|d| (d.total_space() - d.available_space(), d.total_space()))
        .unwrap_or((0, 0))
}
