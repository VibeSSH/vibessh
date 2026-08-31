//! Drives `firewall::ufw::UfwProvider` against the real, dedicated VibeSSH
//! test server - **read-only**, deliberately. That server's `ufw` is real,
//! actively-managed production security infrastructure (Cloudflare-scoped
//! HTTP(S) rules, a WireGuard-based backend mesh, Pterodactyl container
//! ports, and a long hand-curated anti-abuse `DENY FWD` blocklist against
//! real attacker IPs) - not a disposable rule set safe to mutate for a
//! test, even additively. What THIS test verifies instead: that
//! `detect`/`current_rules`/`is_active` behave correctly against genuinely
//! messy, real-world `ufw status` output (IPv6 duplicates, CIDR sources,
//! named-interface rules, port ranges, trailing `# comment`s) rather than
//! only the small hand-written fixtures in `ufw.rs`'s own unit tests -
//! exactly the gap a real-server test is for. `apply_rules`/`enable` (the
//! only mutating methods) are never called here.
//!
//! `#[ignore]` - needs network access to one specific real host plus one
//! specific local private key file. Run explicitly:
//! `cargo test --test firewall_ufw -- --ignored`.

use vibessh_lib::firewall::ufw::UfwProvider;
use vibessh_lib::firewall::FirewallProvider;
use vibessh_lib::ssh::{connect, SshAuth, SshCredentials};

const TEST_HOST: &str = "94.130.201.103";
const TEST_USER: &str = "root";
const TEST_KEY_PATH: &str = r"C:\Users\kompu\.ssh\vibessh_dedi_ed25519";

#[tokio::test]
#[ignore]
async fn detect_and_read_the_real_servers_actual_ufw_state() {
    let credentials = SshCredentials {
        host: TEST_HOST.to_string(),
        port: 22,
        username: TEST_USER.to_string(),
        auth: SshAuth::PrivateKey { path: TEST_KEY_PATH.to_string(), passphrase: None },
    };
    let outcome = connect(&credentials, None).await.expect("couldn't connect to the real test server - check the key/network");
    let session = outcome.session;

    assert!(UfwProvider::detect(&session).await.unwrap(), "ufw is known to be installed on this server");

    let provider = UfwProvider;
    assert!(provider.is_active(&session).await.unwrap(), "ufw is known to be active on this server");

    // The real rule set is large, messy, and dual-stack - this must not
    // panic on any of it, and must correctly recognize at least the plain
    // `port/proto` rules this server is known to have (the SSH allow rule
    // itself, present on every real host this codebase manages).
    let rules = provider.current_rules(&session).await.unwrap();
    assert!(!rules.is_empty(), "a server with dozens of real ufw rules must parse to at least one");
    assert!(
        rules.contains(&vibessh_lib::firewall::FirewallRule { port: 22, protocol: vibessh_lib::models::PortProtocol::Tcp, source_cidr: None }),
        "the real SSH allow rule (22/tcp) must be recognized - got {rules:?}"
    );

    session.close().await;
}
