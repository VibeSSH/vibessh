//! Resource metrics and a process list over plain SSH - no agent, no
//! `sysinfo` (that crate only reads the *local* machine), so this reads the
//! same `/proc` files and runs the same `ps`/`df` a human would at a shell.
//! CPU% and network throughput aren't single-point-in-time values - both
//! are a delta between two samples divided by the elapsed time, the same
//! idea `agent::metrics::MetricsCollector` uses locally via `sysinfo`, just
//! computed from two remote `/proc/stat`/`/proc/net/dev` reads instead of
//! two local ones. The first call after connecting has no prior sample to
//! diff against, so it reports 0% CPU and 0 bytes/sec - not a meaningful
//! number, but not a fabricated one either.

use std::collections::HashMap;
use std::time::Instant;

use vibessh_protocol::{ProcessSummary, ServerMetrics};

use super::client::{MetricsSample, SshSession};
use crate::errors::AppResult;

const METRICS_COMMAND: &str = r#"echo ===CPU===
head -1 /proc/stat
echo ===MEM===
cat /proc/meminfo
echo ===DISK===
df -B1 --output=size,used / | tail -n 1
echo ===LOAD===
cat /proc/loadavg
echo ===UPTIME===
cat /proc/uptime
echo ===NET===
cat /proc/net/dev"#;

impl SshSession {
    pub async fn get_metrics(&self) -> AppResult<ServerMetrics> {
        let output = self.execute_command(METRICS_COMMAND).await?;
        let sections = split_sections(&output.stdout);
        let section = |name: &str| sections.get(name).map(String::as_str).unwrap_or("");

        let (cpu_idle_jiffies, cpu_total_jiffies) = parse_cpu_line(section("CPU")).unwrap_or((0, 0));
        let (ram_used_bytes, ram_total_bytes) = parse_meminfo(section("MEM")).unwrap_or((0, 0));
        let (disk_total_bytes, disk_used_bytes) = parse_disk(section("DISK")).unwrap_or((0, 0));
        let load_average_1m = parse_loadavg(section("LOAD")).unwrap_or(0.0);
        let uptime_seconds = parse_uptime(section("UPTIME")).unwrap_or(0);
        let (rx_bytes, tx_bytes) = parse_net_dev(section("NET"));

        let now = Instant::now();
        let previous = self.swap_metrics_sample(MetricsSample {
            cpu_idle_jiffies,
            cpu_total_jiffies,
            rx_bytes,
            tx_bytes,
            at: now,
        });

        let (cpu_usage_percent, network_rx_bytes_per_sec, network_tx_bytes_per_sec) = match previous {
            Some(prev) => {
                let elapsed_secs = now.duration_since(prev.at).as_secs_f64().max(0.001);
                let total_delta = cpu_total_jiffies.saturating_sub(prev.cpu_total_jiffies);
                let idle_delta = cpu_idle_jiffies.saturating_sub(prev.cpu_idle_jiffies);
                let cpu_pct = if total_delta > 0 {
                    (1.0 - (idle_delta as f64 / total_delta as f64)) * 100.0
                } else {
                    0.0
                };
                let rx_rate = (rx_bytes.saturating_sub(prev.rx_bytes) as f64 / elapsed_secs) as u64;
                let tx_rate = (tx_bytes.saturating_sub(prev.tx_bytes) as f64 / elapsed_secs) as u64;
                (cpu_pct as f32, rx_rate, tx_rate)
            }
            None => (0.0, 0, 0),
        };

        Ok(ServerMetrics {
            cpu_usage_percent,
            ram_used_bytes,
            ram_total_bytes,
            disk_used_bytes,
            disk_total_bytes,
            load_average_1m,
            uptime_seconds,
            network_rx_bytes_per_sec,
            network_tx_bytes_per_sec,
        })
    }

    pub async fn list_processes(&self) -> AppResult<Vec<ProcessSummary>> {
        let output = self.execute_command("ps -eo pid,user,pcpu,rss,comm --no-headers").await?;
        Ok(parse_ps_output(&output.stdout))
    }
}

/// Parses `ps -eo pid,user,pcpu,rss,comm --no-headers` output. `comm` (not
/// `command`/`args`) is a single token with no embedded spaces, so a plain
/// `split_whitespace` is enough - no shell-quoting-aware parsing needed.
fn parse_ps_output(stdout: &str) -> Vec<ProcessSummary> {
    let mut processes = Vec::new();

    for line in stdout.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(user), Some(cpu_percent), Some(rss_kb)) = (
            fields.next().and_then(|f| f.parse::<u32>().ok()),
            fields.next().map(str::to_string),
            fields.next().and_then(|f| f.parse::<f32>().ok()),
            fields.next().and_then(|f| f.parse::<u64>().ok()),
        ) else {
            continue;
        };
        let command: String = fields.collect::<Vec<_>>().join(" ");
        if command.is_empty() {
            continue;
        }

        processes.push(ProcessSummary {
            pid,
            user,
            cpu_percent,
            ram_bytes: rss_kb * 1024,
            command,
        });
    }

    processes
}

/// Splits `METRICS_COMMAND`'s output on its `===NAME===` markers into
/// (name, body) pairs - simpler and more robust than trying to tell the
/// sections apart by their content shape.
fn split_sections(output: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_body = String::new();

    for line in output.lines() {
        if let Some(name) = line.strip_prefix("===").and_then(|s| s.strip_suffix("===")) {
            if let Some(prev_name) = current_name.take() {
                sections.insert(prev_name, std::mem::take(&mut current_body));
            }
            current_name = Some(name.to_string());
        } else if current_name.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(name) = current_name {
        sections.insert(name, current_body);
    }

    sections
}

/// `/proc/stat`'s first line: `cpu  user nice system idle iowait irq softirq steal ...`.
/// Returns (idle jiffies, total jiffies) - `total` deliberately only sums
/// the first 8 fields (through `steal`), since `guest`/`guest_nice` (fields
/// 9-10, present on newer kernels) are already included inside `user`/
/// `nice` and would double-count if added again - the same convention
/// `top` and other tools follow.
fn parse_cpu_line(section: &str) -> Option<(u64, u64)> {
    let line = section.lines().next()?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
    if values.len() < 4 {
        return None;
    }
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    let total: u64 = values.iter().take(8).sum();
    Some((idle, total))
}

/// Returns (used bytes, total bytes). Prefers `MemAvailable` (accounts for
/// reclaimable cache, what a human actually means by "how much RAM is
/// free") over `MemFree`, falling back to it only on kernels old enough not
/// to report `MemAvailable` (pre-3.14, unlikely on anything this app targets).
fn parse_meminfo(section: &str) -> Option<(u64, u64)> {
    let mut total_kb = None;
    let mut available_kb = None;
    let mut free_kb = None;

    for line in section.lines() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        let Some(value_kb) = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        match key {
            "MemTotal" => total_kb = Some(value_kb),
            "MemAvailable" => available_kb = Some(value_kb),
            "MemFree" => free_kb = Some(value_kb),
            _ => {}
        }
    }

    let total_kb = total_kb?;
    let used_kb = total_kb.saturating_sub(available_kb.or(free_kb).unwrap_or(0));
    Some((used_kb * 1024, total_kb * 1024))
}

/// `df -B1 --output=size,used /` (minus its header, stripped by `tail -n 1`
/// in `METRICS_COMMAND`): two byte counts, total then used.
fn parse_disk(section: &str) -> Option<(u64, u64)> {
    let mut fields = section.lines().next()?.split_whitespace();
    let size: u64 = fields.next()?.parse().ok()?;
    let used: u64 = fields.next()?.parse().ok()?;
    Some((size, used))
}

fn parse_loadavg(section: &str) -> Option<f32> {
    section.lines().next()?.split_whitespace().next()?.parse().ok()
}

fn parse_uptime(section: &str) -> Option<u64> {
    let seconds: f64 = section.lines().next()?.split_whitespace().next()?.parse().ok()?;
    Some(seconds as u64)
}

/// `/proc/net/dev`: two header lines (no `:`, safely skipped by the
/// `split_once` check below) then one `iface: rx... tx...` line per
/// interface - 16 numeric fields, receive bytes first, transmit bytes 9th.
/// Summed across every interface except loopback, matching
/// `agent::metrics`'s own `!name.starts_with("lo")` filter so SSH mode and
/// Agent mode report network throughput the same way.
fn parse_net_dev(section: &str) -> (u64, u64) {
    let mut rx_total = 0u64;
    let mut tx_total = 0u64;

    for line in section.lines() {
        let Some((iface, rest)) = line.split_once(':') else { continue };
        if iface.trim().starts_with("lo") {
            continue;
        }
        let fields: Vec<u64> = rest.split_whitespace().filter_map(|f| f.parse().ok()).collect();
        if fields.len() < 9 {
            continue;
        }
        rx_total += fields[0];
        tx_total += fields[8];
    }

    (rx_total, tx_total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_realistic_proc_stat_line() {
        let (idle, total) = parse_cpu_line("cpu  3357 12 4313 1362393 55 0 7 0 0 0\n").unwrap();
        assert_eq!(idle, 1362393 + 55);
        assert_eq!(total, 3357 + 12 + 4313 + 1362393 + 55 + 0 + 7 + 0);
    }

    #[test]
    fn parses_meminfo_preferring_mem_available() {
        let section = "MemTotal:       16384000 kB\nMemFree:         2000000 kB\nMemAvailable:    8000000 kB\n";
        let (used, total) = parse_meminfo(section).unwrap();
        assert_eq!(total, 16384000 * 1024);
        assert_eq!(used, (16384000 - 8000000) * 1024);
    }

    #[test]
    fn parses_disk_size_and_used() {
        let (total, used) = parse_disk("468500987904 16873893888\n").unwrap();
        assert_eq!(total, 468500987904);
        assert_eq!(used, 16873893888);
    }

    #[test]
    fn sums_network_bytes_across_interfaces_and_skips_loopback() {
        let section = concat!(
            "Inter-|   Receive                                                |  Transmit\n",
            " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
            "    lo: 999999       10    0    0    0     0          0         0   999999      10    0    0    0     0       0          0\n",
            "  eth0: 1000            5    0    0    0     0          0         0     2000       3    0    0    0     0       0          0\n",
        );
        let (rx, tx) = parse_net_dev(section);
        assert_eq!(rx, 1000);
        assert_eq!(tx, 2000);
    }

    #[test]
    fn parses_processes_from_ps_output() {
        let stdout = "  1234 root     0.5  10240 sshd\n  5678 www-data 12.3 204800 nginx\n";
        let processes = parse_ps_output(stdout);
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].pid, 1234);
        assert_eq!(processes[0].user, "root");
        assert_eq!(processes[0].ram_bytes, 10240 * 1024);
        assert_eq!(processes[1].command, "nginx");
    }
}
