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
cat /proc/net/dev
echo ===OS===
cat /etc/os-release 2>/dev/null"#;

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
        let os_name = parse_os_release(section("OS"));

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
            os_name,
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

/// `PRETTY_NAME` out of `/etc/os-release`.
///
/// The value is shell-quoted in that file more often than not
/// (`PRETTY_NAME="Ubuntu 24.04.1 LTS"`), so the quotes come off; a
/// distribution that omits them is handled by the same code path rather
/// than by a second branch.
///
/// `None` rather than a guess when the file is absent or has no such line -
/// a container image without it is a real case, and "unknown" written out
/// as if it were a distribution name would be worse than a blank row.
fn parse_os_release(section: &str) -> Option<String> {
    let value = section.lines().find_map(|line| line.trim().strip_prefix("PRETTY_NAME="))?;
    let value = value.trim().trim_matches('"').trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
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
        assert_eq!(total, ((3357 + 12 + 4313 + 1362393 + 55) + 7));
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

#[cfg(test)]
mod os_release_tests {
    use super::*;

    #[test]
    fn reads_the_pretty_name_out_of_a_real_os_release() {
        let section = "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\n";
        assert_eq!(parse_os_release(section).as_deref(), Some("Ubuntu 24.04.1 LTS"));
    }

    /// Some distributions leave the value unquoted. Same code path, not a
    /// second branch.
    #[test]
    fn reads_an_unquoted_value() {
        assert_eq!(parse_os_release("PRETTY_NAME=Alpine Linux v3.20\n").as_deref(), Some("Alpine Linux v3.20"));
    }

    /// `NAME=` also ends in the same three letters, so a naive `contains`
    /// would match it and report the wrong string.
    #[test]
    fn is_not_fooled_by_a_line_that_merely_ends_in_the_same_letters() {
        let section = "NAME=\"Debian GNU/Linux\"\nPRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\n";
        assert_eq!(parse_os_release(section).as_deref(), Some("Debian GNU/Linux 12 (bookworm)"));
    }

    /// A container image without the file, or a distribution that does not
    /// ship the field. Nothing is better than a guess.
    #[test]
    fn says_nothing_when_there_is_nothing_to_say() {
        assert_eq!(parse_os_release(""), None);
        assert_eq!(parse_os_release("NAME=\"Something\"\n"), None);
        assert_eq!(parse_os_release("PRETTY_NAME=\"\"\n"), None);
    }
}

// ---- Minecraft server health over RCON --------------------------------
//
// The host metrics above answer "is the machine busy". These answer "is the
// game server healthy" - TPS, tick time, who is online - which only the JVM
// knows. Reached over a direct-tcpip channel to the server's own loopback, so
// the RCON port is never exposed; the password comes from the keyring, never
// a command line. Best-effort per command: a Spigot build without Paper's
// `/tps` still yields the player list rather than failing the whole poll.

impl SshSession {
    pub async fn get_minecraft_metrics(
        &self,
        rcon_port: u16,
        password: &str,
    ) -> AppResult<vibessh_protocol::MinecraftMetrics> {
        use tokio::io::AsyncWriteExt;

        let channel = self.open_direct_tcpip("127.0.0.1", rcon_port, "127.0.0.1", 0).await?;
        let mut stream = channel.into_stream();

        // Auth. On success the reply echoes our id; a wrong password is -1.
        const AUTH_ID: i32 = 1;
        stream
            .write_all(&super::minecraft_rcon::encode_packet(AUTH_ID, super::minecraft_rcon::RCON_TYPE_AUTH, password))
            .await
            .map_err(|e| crate::errors::AppError::Connection(format!("couldn't send RCON login: {e}")))?;
        let (auth_id, _, _) = rcon_read_packet(&mut stream).await?;
        if auth_id == super::minecraft_rcon::RCON_AUTH_FAILED {
            // Unauthorized, not Connection: the frontend tells a wrong
            // password apart from an unreachable port by the error code, and
            // "the password is wrong" and "RCON isn't listening" need
            // different advice.
            return Err(crate::errors::AppError::Unauthorized("RCON rejected the password".into()));
        }

        // Each is optional: a non-Paper server answers `list` but not `tps`.
        let tps = rcon_command(&mut stream, "tps").await.ok();
        let mspt = rcon_command(&mut stream, "mspt").await.ok();
        let list = rcon_command(&mut stream, "list").await.ok();

        let (tps_1m, tps_5m, tps_15m) = match tps.as_deref().and_then(super::minecraft_rcon::parse_tps) {
            Some((a, b, c)) => (Some(a), Some(b), Some(c)),
            None => (None, None, None),
        };
        let (mspt_avg, mspt_max) = match mspt.as_deref().and_then(super::minecraft_rcon::parse_mspt) {
            Some((a, b)) => (Some(a), Some(b)),
            None => (None, None),
        };
        let (players_online, players_max, player_names) =
            list.as_deref().and_then(super::minecraft_rcon::parse_list).unwrap_or((0, 0, Vec::new()));

        Ok(vibessh_protocol::MinecraftMetrics {
            tps_1m,
            tps_5m,
            tps_15m,
            mspt_avg,
            mspt_max,
            players_online,
            players_max,
            player_names,
        })
    }
}

/// Reads exactly one RCON packet, bounded by a timeout so a silent server
/// can't hang the poll. Used only for the auth reply, which is a single
/// packet; command replies use `rcon_command`, which handles multi-packet.
async fn rcon_read_packet<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> AppResult<(i32, i32, String)> {
    use tokio::io::AsyncReadExt;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        if let Some((id, kind, body, _used)) = super::minecraft_rcon::decode_packet(&buf) {
            return Ok((id, kind, body));
        }
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .map_err(|_| crate::errors::AppError::Connection("RCON timed out".into()))?
            .map_err(|e| crate::errors::AppError::Connection(format!("RCON read failed: {e}")))?;
        if read == 0 {
            return Err(crate::errors::AppError::Connection("RCON closed the channel early".into()));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
}

/// Runs one command and returns its full text reply. A reply can span several
/// packets, so a second empty command is sent as a sentinel: the server
/// processes in order and echoes the sentinel's id last, which is how the end
/// of the real reply is known without guessing at packet boundaries.
async fn rcon_command<S>(stream: &mut S, command: &str) -> AppResult<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    const CMD_ID: i32 = 10;
    const END_ID: i32 = 11;

    stream
        .write_all(&super::minecraft_rcon::encode_packet(CMD_ID, super::minecraft_rcon::RCON_TYPE_COMMAND, command))
        .await
        .map_err(|e| crate::errors::AppError::Connection(format!("couldn't send RCON command: {e}")))?;
    stream
        .write_all(&super::minecraft_rcon::encode_packet(END_ID, super::minecraft_rcon::RCON_TYPE_COMMAND, ""))
        .await
        .map_err(|e| crate::errors::AppError::Connection(format!("couldn't send RCON sentinel: {e}")))?;

    let mut body = String::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        while let Some((id, _kind, part, used)) = super::minecraft_rcon::decode_packet(&buf) {
            buf.drain(..used);
            if id == END_ID {
                return Ok(body);
            }
            if id == CMD_ID {
                body.push_str(&part);
            }
        }
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .map_err(|_| crate::errors::AppError::Connection("RCON timed out".into()))?
            .map_err(|e| crate::errors::AppError::Connection(format!("RCON read failed: {e}")))?;
        if read == 0 {
            return if body.is_empty() {
                Err(crate::errors::AppError::Connection("RCON closed the channel early".into()))
            } else {
                Ok(body)
            };
        }
        buf.extend_from_slice(&chunk[..read]);
    }
}
