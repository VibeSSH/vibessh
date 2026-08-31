//! Real `wg`/`wg-quick` CLI calls over SSH exec - no WireGuard-in-Rust
//! crate, no hand-rolled key generation, nothing this module didn't ask the
//! real tool to do. Mirrors `runtime::docker`'s own split: pure,
//! independently-testable command/script construction
//! (`build_apply_script`) separated from the actual SSH exec
//! (`apply`) - see that module's own doc comment for why.
//!
//! **The private key never leaves the Node.** `ensure_keypair` only ever
//! returns the *public* key - the private key is generated with `wg genkey`
//! directly on the Node's own filesystem and is read back into the
//! rendered config entirely within one remote shell invocation
//! (`build_apply_script`'s own `PRIVATE_KEY=$(cat ...)` line), so its
//! plaintext content is never part of any SSH command's stdout, never
//! touches this process's memory, and never gets logged.

use crate::errors::{AppError, AppResult};
use crate::ssh::SshSession;

/// A separate, clearly-namespaced interface - never the host's own
/// pre-existing `wg0` (if any; the project's own dedicated test server
/// already runs an unrelated WireGuard mesh on `wg0` for something else
/// entirely, see the `vibessh-test-server` memory). This module must never
/// read, write, or otherwise touch anything named plain `wg0`.
pub const INTERFACE: &str = "wg-vibessh0";
/// Deliberately NOT WireGuard's own conventional default (`51820`) - a real
/// run against the project's own dedicated test server proved exactly why:
/// that host already runs an unrelated, pre-existing WireGuard interface
/// (`wg0`, for its own Minecraft-backend mesh) whose `ListenPort` is
/// `51820`, and a second interface trying to bind the same UDP port fails
/// at the kernel level (`wg-quick up` errors with a generic-looking
/// `RTNETLINK answers: Address already in use` that's actually the bind
/// conflict, not an IP-address one). Any Node already running WireGuard for
/// something else is likely to be using the conventional default too, so
/// picking a different, VibeSSH-specific port here isn't just cosmetic
/// namespacing - it's what makes joining a Node that already has its own
/// WireGuard setup actually work.
pub const LISTEN_PORT: u16 = 54221;
const PRIVATE_KEY_PATH: &str = "/etc/wireguard/vibessh-privatekey";
const CONFIG_PATH: &str = "/etc/wireguard/wg-vibessh0.conf";
/// Any table-generating provider (Vibe Network's own IPAM here) allocates
/// well inside `/16` - `/16` (not `/24`) matches
/// `storage::node_network_repository`'s own CIDR choice.
const ADDRESS_CIDR_SUFFIX: &str = "/16";

/// One other mesh member, as seen from the Node this config is being
/// rendered for - `allowed_ip` is that peer's own `/32` (a full mesh: every
/// pair connects directly, so each peer only ever routes to its own single
/// address, never a wider subnet).
pub struct Peer {
    pub public_key: String,
    pub allowed_ip: String,
    pub endpoint: String,
}

pub async fn detect(connection: &SshSession) -> AppResult<bool> {
    let output = connection.execute_command("command -v wg wg-quick >/dev/null 2>&1 && echo yes || echo no").await?;
    Ok(output.stdout.trim() == "yes")
}

/// `apt-get`-only (Ubuntu/Debian) - matches every built-in blueprint's own
/// target OS. Real, not assumed: `detect` is always checked first by the
/// caller, and this itself checks `apt-get` exists before trying to use it,
/// erroring with a clear, actionable message on any other distro rather
/// than silently doing nothing.
pub async fn install_if_missing(connection: &SshSession) -> AppResult<()> {
    if detect(connection).await? {
        return Ok(());
    }
    let has_apt = connection.execute_command("command -v apt-get >/dev/null 2>&1 && echo yes || echo no").await?;
    if has_apt.stdout.trim() != "yes" {
        return Err(AppError::InvalidInput("WireGuard isn't installed on this Node and it doesn't look like a Debian/Ubuntu host - install wireguard-tools manually first".into()));
    }
    let output = connection.execute_command("DEBIAN_FRONTEND=noninteractive sudo apt-get update -qq && sudo apt-get install -y -qq wireguard-tools").await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        return Err(AppError::Connection(format!("couldn't install wireguard-tools: {}", if detail.is_empty() { "apt-get failed" } else { detail })));
    }
    Ok(())
}

/// Idempotent: reuses an already-generated key if this Node has joined
/// before (or was set up out of band), only generating a fresh one the
/// first time. Only the public key is ever returned - see this module's
/// own doc comment for why.
pub async fn ensure_keypair(connection: &SshSession) -> AppResult<String> {
    let exists = connection.execute_command(&format!("test -f {PRIVATE_KEY_PATH} && echo yes || echo no")).await?;
    if exists.stdout.trim() != "yes" {
        let generate = connection
            .execute_command(&format!("umask 077 && mkdir -p /etc/wireguard && wg genkey | sudo tee {PRIVATE_KEY_PATH} >/dev/null && sudo chmod 600 {PRIVATE_KEY_PATH}"))
            .await?;
        if generate.exit_code != 0 {
            let detail = generate.stderr.trim();
            return Err(AppError::Connection(format!("couldn't generate a WireGuard keypair: {}", if detail.is_empty() { "wg genkey failed" } else { detail })));
        }
    }
    let pubkey = connection.execute_command(&format!("sudo cat {PRIVATE_KEY_PATH} | wg pubkey")).await?;
    if pubkey.exit_code != 0 || pubkey.stdout.trim().is_empty() {
        return Err(AppError::Connection("couldn't derive the WireGuard public key".into()));
    }
    Ok(pubkey.stdout.trim().to_string())
}

/// A raw newline or the heredoc's own delimiter in an interpolated value
/// could inject extra shell/config statements into the generated script -
/// rejected outright, same stance `runtime::docker::reject_newlines`
/// already takes for the same reason.
fn reject_unsafe(value: &str, field: &str) -> AppResult<()> {
    if value.contains('\n') || value.contains('\r') || value.contains("VIBESSH_WG_EOF") {
        return Err(AppError::InvalidInput(format!("{field} contains characters that aren't allowed in a WireGuard config")));
    }
    Ok(())
}

/// Pure script construction, separated from `apply`'s actual SSH exec so
/// the rendered shape can be unit tested without a live connection - same
/// split `runtime::docker::build_create_command` uses for the same reason.
/// The private key is read from disk *inside* this script (`$(cat ...)`),
/// never passed in as a parameter - see this module's own doc comment.
fn build_apply_script(own_ip: &str, peers: &[Peer]) -> AppResult<String> {
    reject_unsafe(own_ip, "the Node's own mesh IP")?;
    for peer in peers {
        reject_unsafe(&peer.public_key, "a peer's public key")?;
        reject_unsafe(&peer.allowed_ip, "a peer's allowed IP")?;
        reject_unsafe(&peer.endpoint, "a peer's endpoint")?;
    }

    let mut config = format!("[Interface]\nAddress = {own_ip}{ADDRESS_CIDR_SUFFIX}\nPrivateKey = $PRIVATE_KEY\nListenPort = {LISTEN_PORT}\n");
    for peer in peers {
        config.push_str(&format!(
            "\n[Peer]\nPublicKey = {}\nAllowedIPs = {}\nEndpoint = {}\nPersistentKeepalive = 25\n",
            peer.public_key, peer.allowed_ip, peer.endpoint
        ));
    }

    // `wg syncconf` needs a plain file argument, not a pipe - a temp file
    // rather than `<(...)` process substitution specifically so this
    // script only needs POSIX `sh` semantics, not bash, since which shell
    // actually runs an SSH exec command depends on the remote account's own
    // login shell (`dash` is Ubuntu's default `/bin/sh`, which has no
    // process substitution).
    Ok(format!(
        "set -e\nmkdir -p /etc/wireguard\nPRIVATE_KEY=$(sudo cat {PRIVATE_KEY_PATH})\nsudo tee {CONFIG_PATH} >/dev/null <<VIBESSH_WG_EOF\n{config}VIBESSH_WG_EOF\nsudo chmod 600 {CONFIG_PATH}\nif sudo ip link show {INTERFACE} >/dev/null 2>&1; then\n  sudo wg-quick strip {INTERFACE} | sudo tee /tmp/vibessh-wg-strip.conf >/dev/null\n  sudo wg syncconf {INTERFACE} /tmp/vibessh-wg-strip.conf\n  sudo rm -f /tmp/vibessh-wg-strip.conf\nelse\n  sudo wg-quick up {INTERFACE}\nfi\n"
    ))
}

/// Renders this Node's own full peer set and applies it - a first-time
/// call brings the interface up (`wg-quick up`); every call after that
/// live-reloads it (`wg syncconf`, no drop) rather than a down+up cycle
/// that would briefly interrupt every existing tunnel to this Node just to
/// add one new peer. Idempotent and safe to call on every membership
/// change for every member, matching the same "always re-derive and
/// re-apply the full desired set" shape `firewall_service::reconcile_node`
/// already uses.
pub async fn apply(connection: &SshSession, own_ip: &str, peers: &[Peer]) -> AppResult<()> {
    let script = build_apply_script(own_ip, peers)?;
    let output = connection.execute_command(&script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        return Err(AppError::Connection(format!("couldn't apply the WireGuard config: {}", if detail.is_empty() { "wg-quick/wg syncconf failed" } else { detail })));
    }
    Ok(())
}

/// One peer's real, current state as `wg show ... dump` reports it -
/// `latest_handshake_unix` is a Unix timestamp (`0` = no handshake yet,
/// meaning the tunnel has never actually been used). This is genuine
/// WireGuard state, not a synthetic ping - a recent handshake is the
/// closest thing WireGuard itself has to "this peer is online."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerStatus {
    pub public_key: String,
    pub latest_handshake_unix: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Parses `wg show <interface> dump` - the interface's own summary line
/// (private key, public key, listen port, fwmark - 4 tab-separated fields)
/// comes first and is skipped; every line after that is one peer (public
/// key, preshared key, endpoint, allowed IPs, latest handshake, rx, tx,
/// keepalive - 8 fields).
fn parse_wg_dump(output: &str) -> Vec<PeerStatus> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 7 {
                return None;
            }
            Some(PeerStatus {
                public_key: fields[0].to_string(),
                latest_handshake_unix: fields[4].parse().unwrap_or(0),
                rx_bytes: fields[5].parse().unwrap_or(0),
                tx_bytes: fields[6].parse().unwrap_or(0),
            })
        })
        .collect()
}

/// This Node's own live view of its peers - real `wg show` output, not a
/// synthetic probe. Empty (not an error) if the interface doesn't exist
/// yet (this Node hasn't joined, or hasn't been reconciled since joining).
pub async fn show_peers(connection: &SshSession) -> AppResult<Vec<PeerStatus>> {
    let output = connection.execute_command(&format!("sudo wg show {INTERFACE} dump 2>/dev/null")).await?;
    if output.exit_code != 0 {
        return Ok(vec![]);
    }
    Ok(parse_wg_dump(&output.stdout))
}

/// Tears the interface down and removes its config - used both by an
/// explicit "Leave Vibe Network" action and by every real-server test's own
/// cleanup, so a Node this module touched is left with no trace once it's
/// no longer a member. Deliberately does NOT remove the keypair - a Node
/// that rejoins later should get the same public key back, not a new
/// identity every time.
pub async fn teardown(connection: &SshSession) -> AppResult<()> {
    let _ = connection.execute_command(&format!("sudo wg-quick down {INTERFACE} 2>/dev/null; sudo rm -f {CONFIG_PATH}")).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(public_key: &str, allowed_ip: &str, endpoint: &str) -> Peer {
        Peer { public_key: public_key.to_string(), allowed_ip: allowed_ip.to_string(), endpoint: endpoint.to_string() }
    }

    #[test]
    fn build_apply_script_reads_the_private_key_from_disk_never_takes_it_as_a_parameter() {
        let script = build_apply_script("10.77.0.1", &[]).unwrap();
        assert!(script.contains(&format!("PRIVATE_KEY=$(sudo cat {PRIVATE_KEY_PATH})")), "{script}");
        assert!(script.contains("PrivateKey = $PRIVATE_KEY"), "{script}");
    }

    #[test]
    fn build_apply_script_renders_one_peer_block_per_peer_with_its_own_slash_32() {
        let script = build_apply_script(
            "10.77.0.1",
            &[peer("pubkeyA=", "10.77.0.2/32", "203.0.113.20:51820"), peer("pubkeyB=", "10.77.0.3/32", "203.0.113.30:51820")],
        )
        .unwrap();
        assert!(script.contains("PublicKey = pubkeyA=\nAllowedIPs = 10.77.0.2/32\nEndpoint = 203.0.113.20:51820"), "{script}");
        assert!(script.contains("PublicKey = pubkeyB=\nAllowedIPs = 10.77.0.3/32\nEndpoint = 203.0.113.30:51820"), "{script}");
    }

    #[test]
    fn build_apply_script_brings_the_interface_up_on_first_run_and_live_reloads_after() {
        let script = build_apply_script("10.77.0.1", &[]).unwrap();
        assert!(script.contains(&format!("wg-quick up {INTERFACE}")), "{script}");
        assert!(script.contains(&format!("wg syncconf {INTERFACE}")), "{script}");
    }

    #[test]
    fn build_apply_script_rejects_a_newline_smuggled_into_a_peer_endpoint() {
        let result = build_apply_script("10.77.0.1", &[peer("pubkeyA=", "10.77.0.2/32", "203.0.113.20:51820\nrm -rf /")]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_wg_dump_skips_the_interface_summary_line_and_reads_every_peer() {
        let output = "privkeyA=\tpubkeyA=\t51820\toff\npubkeyB=\t(none)\t203.0.113.20:51820\t10.77.0.2/32\t1700000000\t1024\t2048\t25\npubkeyC=\t(none)\t203.0.113.30:51820\t10.77.0.3/32\t0\t0\t0\t25\n";
        let peers = parse_wg_dump(output);
        assert_eq!(
            peers,
            vec![
                PeerStatus { public_key: "pubkeyB=".into(), latest_handshake_unix: 1700000000, rx_bytes: 1024, tx_bytes: 2048 },
                PeerStatus { public_key: "pubkeyC=".into(), latest_handshake_unix: 0, rx_bytes: 0, tx_bytes: 0 },
            ]
        );
    }

    #[test]
    fn build_apply_script_rejects_the_heredoc_delimiter_smuggled_into_a_public_key() {
        let result = build_apply_script("10.77.0.1", &[peer("VIBESSH_WG_EOF", "10.77.0.2/32", "203.0.113.20:51820")]);
        assert!(result.is_err());
    }
}
