//! The one operation in this codebase that can end with nobody able to reach
//! the machine again.
//!
//! `UfwProvider::enable` turns on a default-deny firewall over the very SSH
//! session that manages the Node. Its guard asks the Node which port the live
//! session actually arrived on (`$SSH_CONNECTION`'s fourth field) and refuses
//! unless the desired rules cover it - because VibeSSH's *stored* SSH port
//! goes stale the moment somebody moves sshd, or reaches the box through a
//! jump host.
//!
//! That guard had never been executed. It cannot be checked by a unit test:
//! the whole question is what a real `ufw` does to a real SSH session, and
//! the only way to find out is to construct the situation where a wrong
//! answer costs the connection.
//!
//! **These tests take the machine's firewall down and back up.** They run
//! only against the disposable test VPS, and they arm a `systemd-run` timer
//! that disables ufw unconditionally a few minutes later, so a genuine
//! lockout self-heals instead of needing the provider's console. Run:
//! `cargo test --test firewall_lockout -- --ignored --test-threads=1`.

use std::sync::Arc;

use vibessh_lib::firewall::{ufw::UfwProvider, FirewallProvider, FirewallRule};
use vibessh_lib::models::PortProtocol;
use vibessh_lib::ssh::{connect, SshAuth, SshCredentials, SshSession};

/// The disposable VPS, not the production host. `docker_runtime.rs` points
/// somewhere else and calls it "the dedicated test server"; that stopped
/// being true, and this file must not follow it.
const TEST_HOST: &str = "57.128.203.210";
const TEST_USER: &str = "ubuntu";
const TEST_KEY_PATH: &str = r"C:\Users\kompu\.ssh\vibessh_dedi_ed25519";

async fn session() -> Arc<SshSession> {
    let credentials = SshCredentials {
        host: TEST_HOST.to_string(),
        port: 22,
        username: TEST_USER.to_string(),
        auth: SshAuth::PrivateKey { path: TEST_KEY_PATH.to_string(), passphrase: None },
    };
    Arc::new(connect(&credentials, None).await.expect("couldn't reach the test VPS").session)
}

async fn run(session: &SshSession, command: &str) -> String {
    let out = session.execute_command(command).await.expect("command should run");
    format!("{}{}", out.stdout, out.stderr)
}

/// Disables ufw unconditionally after `seconds`, whatever else happens.
///
/// Armed *before* anything is enabled, so a lockout is temporary by
/// construction rather than by the test finishing cleanly. A panic between
/// the enable and the cleanup would otherwise leave a locked box.
async fn arm_rescue(session: &SshSession, seconds: u32) {
    run(session, "sudo systemctl stop ufw-rescue.timer 2>/dev/null; sudo systemctl reset-failed ufw-rescue.service 2>/dev/null; true").await;
    let armed = run(session, &format!("sudo systemd-run --on-active={seconds} --unit=ufw-rescue /usr/sbin/ufw --force disable")).await;
    assert!(armed.contains("ufw-rescue"), "the rescue timer did not arm - refusing to continue: {armed}");
}

async fn disarm_rescue(session: &SshSession) {
    run(session, "sudo systemctl stop ufw-rescue.timer 2>/dev/null; sudo systemctl reset-failed ufw-rescue.service 2>/dev/null; true").await;
}

fn tcp(port: u16) -> FirewallRule {
    FirewallRule { port, protocol: PortProtocol::Tcp, source_cidr: None }
}

/// The refusal. A rule set that does not cover the live SSH port must not
/// enable anything.
///
/// This is the case that matters: VibeSSH's stored SSH port is what builds
/// the desired rules, and it is exactly the value that goes stale. If the
/// guard is wrong here, the operator loses the machine.
#[tokio::test]
#[ignore]
async fn enabling_is_refused_when_no_rule_covers_the_live_ssh_port() {
    let session = session().await;
    arm_rescue(&session, 300).await;
    run(&session, "sudo ufw --force disable").await;

    let provider = UfwProvider;
    // Deliberately no rule for 22 - a plausible desired set for a Node whose
    // stored SSH port is wrong.
    let desired = vec![tcp(8080), tcp(25565)];
    let result = provider.enable(&session, &desired).await;

    let status = run(&session, "sudo ufw status").await;
    disarm_rescue(&session).await;

    let err = result.expect_err("enable must refuse when the live SSH port is uncovered");
    assert!(err.to_string().contains("port 22"), "the refusal should name the port it would have cut off: {err}");
    assert!(status.contains("inactive"), "ufw was enabled despite the refusal - this is the lockout: {status}");
}

/// The permitted case: a rule set covering the live SSH port enables the
/// firewall, and the session survives it.
///
/// "Survives" is checked by running another command over the *same* session
/// after the enable, and by opening a fresh connection - a rule that only
/// spares established connections would pass the first and fail the second.
#[tokio::test]
#[ignore]
async fn enabling_with_the_ssh_port_covered_keeps_the_node_reachable() {
    let session = session().await;
    arm_rescue(&session, 300).await;
    run(&session, "sudo ufw --force disable").await;

    let provider = UfwProvider;
    let desired = vec![tcp(22), tcp(8080), tcp(25565)];
    let enabled = provider.enable(&session, &desired).await;

    // The existing session first.
    let status = run(&session, "sudo ufw status verbose").await;
    // Then a brand new connection, which is what an operator reconnecting
    // tomorrow actually does.
    let fresh = connect(
        &SshCredentials {
            host: TEST_HOST.to_string(),
            port: 22,
            username: TEST_USER.to_string(),
            auth: SshAuth::PrivateKey { path: TEST_KEY_PATH.to_string(), passphrase: None },
        },
        None,
    )
    .await;

    disarm_rescue(&session).await;

    enabled.expect("enable should succeed when the SSH port is covered");
    assert!(status.contains("Status: active"), "ufw did not come up: {status}");
    assert!(status.contains("22/tcp"), "the SSH port is not in the active rule set: {status}");
    let fresh = fresh.expect("a fresh SSH connection must still be possible after enabling the firewall");
    fresh.session.close().await;
}
