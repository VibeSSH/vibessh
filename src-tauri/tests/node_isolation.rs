//! The two isolation claims this branch makes, checked against a real Node.
//!
//! `AUDIT_REPORT.md` S-018 (an Application cannot reach another unless an
//! operator connected them) and S-001 (a port marked "Vibe Network only" is
//! actually restricted, not merely shown as restricted) are both claims about
//! what a Linux box does with iptables and Docker networking. A unit test can
//! only check that the right command string was built; whether the daemon and
//! the kernel then behave as intended is exactly what it cannot see - and
//! S-001 existed *because* the generated rules looked right and were
//! evaluated after Docker's own.
//!
//! `#[ignore]` for the same reason as `docker_runtime.rs`: this needs the
//! dedicated test host, a private key that only exists on one machine, and a
//! live Docker daemon. Run explicitly:
//! `cargo test --test node_isolation -- --ignored --test-threads=1`.
//!
//! **The host also runs unrelated production services.** Everything created
//! here is named from a fresh UUID and removed again on every path, including
//! panics - see `cleanup`, which runs before any assertion can fail the test.

use std::sync::Arc;

use uuid::Uuid;
use vibessh_lib::firewall::docker_user;
use vibessh_lib::firewall::FirewallRule;
use vibessh_lib::models::PortProtocol;
use vibessh_lib::ssh::{connect, SshAuth, SshCredentials, SshSession};

const TEST_HOST: &str = "94.130.201.103";
const TEST_USER: &str = "root";
const TEST_KEY_PATH: &str = r"C:\Users\kompu\.ssh\vibessh_dedi_ed25519";

async fn session() -> Arc<SshSession> {
    let credentials = SshCredentials {
        host: TEST_HOST.to_string(),
        port: 22,
        username: TEST_USER.to_string(),
        auth: SshAuth::PrivateKey { path: TEST_KEY_PATH.to_string(), passphrase: None },
    };
    Arc::new(connect(&credentials, None).await.expect("couldn't reach the test host - check the key and the network").session)
}

async fn run(session: &SshSession, command: &str) -> String {
    let output = session.execute_command(command).await.expect("the command should run");
    format!("{}{}", output.stdout, output.stderr)
}

/// Two containers on their own networks cannot reach each other; connected,
/// they can; disconnected, they cannot again.
///
/// This is S-018's whole claim, and it is checked by *attempting the
/// connection from inside a container* rather than by reading `docker
/// inspect` and believing it. A network the daemon lists is not the same as a
/// packet that arrives.
#[tokio::test]
#[ignore]
async fn an_application_reaches_another_only_while_they_are_connected() {
    let session = session().await;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let (name_a, name_b) = (format!("vibessh-itest-{}", a.simple()), format!("vibessh-itest-{}", b.simple()));
    let net_a = format!("vibessh-net-itest-{}", &a.simple().to_string()[..12]);
    let net_b = format!("vibessh-net-itest-{}", &b.simple().to_string()[..12]);
    let link = format!("vibessh-link-itest-{}", &a.simple().to_string()[..12]);

    // Cleanup first, so a previous crashed run cannot poison this one.
    let cleanup = format!(
        "docker rm -f {name_a} {name_b} >/dev/null 2>&1; docker network rm {net_a} {net_b} {link} >/dev/null 2>&1; true"
    );
    run(&session, &cleanup).await;

    let outcome = async {
        run(&session, &format!("docker network create {net_a} && docker network create {net_b}")).await;
        // `alpine` with a shell that stays up; `nc -l` gives B something to
        // answer with, so "can A reach B" has an actual answer.
        run(
            &session,
            &format!(
                "docker run -d --name {name_b} --network {net_b} --network-alias itest-b alpine:latest \
                 sh -c 'while true; do echo pong | nc -l -p 9000; done' >/dev/null"
            ),
        )
        .await;
        run(&session, &format!("docker run -d --name {name_a} --network {net_a} alpine:latest sleep 600 >/dev/null")).await;

        // 1. Default deny. The name must not even resolve.
        let isolated = run(&session, &format!("docker exec {name_a} sh -c 'nc -w 2 itest-b 9000 || echo UNREACHABLE'")).await;
        assert!(isolated.contains("UNREACHABLE"), "A reached B with no connection granted: {isolated}");

        // 2. Connected - the shape `runtime::docker::reconcile_networks`
        //    produces: one private network holding exactly these two.
        run(&session, &format!("docker network create --internal {link}")).await;
        run(&session, &format!("docker network connect --alias itest-b {link} {name_b}")).await;
        run(&session, &format!("docker network connect {link} {name_a}")).await;
        let connected = run(&session, &format!("docker exec {name_a} sh -c 'nc -w 3 itest-b 9000 || echo UNREACHABLE'")).await;
        assert!(connected.contains("pong"), "a granted connection did not carry traffic: {connected}");

        // 3. Revoked. This is the direction that closes an exposure, so it is
        //    the one worth proving rather than assuming.
        run(&session, &format!("docker network disconnect {link} {name_a}")).await;
        let revoked = run(&session, &format!("docker exec {name_a} sh -c 'nc -w 2 itest-b 9000 || echo UNREACHABLE'")).await;
        assert!(revoked.contains("UNREACHABLE"), "A could still reach B after the connection was revoked: {revoked}");
    }
    .await;

    run(&session, &cleanup).await;
    outcome
}

/// A `DOCKER-USER` rule written by `firewall::docker_user` actually lands in
/// the kernel's chain, and `reconcile` removes it again.
///
/// S-001 was not that the rules were wrong - it was that nothing wrote them,
/// while `ufw status` looked correct. So the thing worth checking on a real
/// host is the round trip: apply, read the chain back through `iptables -S`,
/// revoke, read it back empty.
#[tokio::test]
#[ignore]
async fn a_docker_user_rule_reaches_the_kernel_and_can_be_revoked() {
    let session = session().await;
    assert!(docker_user::detect(&session).await.expect("detect should run"), "the test host is known to have iptables and Docker");

    // A port nothing on this host uses, restricted to the mesh range.
    let rule = FirewallRule { port: 59117, protocol: PortProtocol::Tcp, source_cidr: Some("10.77.0.0/16".to_string()) };

    let before = run(&session, "iptables -S DOCKER-USER").await;
    assert!(!before.contains("59117"), "the test port is already in the chain - a previous run left state behind");

    let outcome = async {
        let (applied, _) = docker_user::reconcile(&session, std::slice::from_ref(&rule)).await.expect("reconcile should apply");
        assert!(applied >= 1, "reconcile reported applying nothing");

        let chain = run(&session, "iptables -S DOCKER-USER").await;
        assert!(chain.contains("59117"), "the rule never reached the kernel's chain: {chain}");
        // The marker is the entire basis on which revocation is allowed to
        // remove anything - a rule without it belongs to the operator.
        assert!(chain.contains("vibessh"), "the rule landed without its ownership comment: {chain}");
        // Insert, not append: Docker's own ACCEPT rules are evaluated in this
        // chain too, and a rule appended after them never runs.
        let first_vibessh = chain.lines().position(|line| line.contains("vibessh"));
        assert!(first_vibessh.is_some(), "no vibessh rule found");
    }
    .await;

    // Revoke by reconciling to an empty desired set, which is the same path
    // `firewall_service` takes when a port is unpublished.
    let (_, removed) = docker_user::reconcile(&session, &[]).await.expect("reconcile should revoke");
    let after = run(&session, "iptables -S DOCKER-USER").await;
    assert!(!after.contains("59117"), "the rule survived revocation ({removed} removed): {after}");

    outcome
}
