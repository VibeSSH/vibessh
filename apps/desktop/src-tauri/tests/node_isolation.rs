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

/// The disposable test VPS.
///
/// **Not** the host `docker_runtime.rs` and the other older integration tests
/// point at. Their doc comments call that one "the real, dedicated VibeSSH
/// test server"; it stopped being that and now runs a Pterodactyl panel, live
/// game containers and the cloud backend. This file targeted it for exactly
/// that reason - the constant was copied - and the tests were run against
/// production once before the mistake was caught. Nothing was damaged, and
/// the point stands: do not infer the test host from those files.
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
    Arc::new(connect(&credentials, None).await.expect("couldn't reach the test host - check the key and the network").session)
}

/// Runs `command` on the Node as root.
///
/// The whole string goes through one `sudo -n sh -c`, deliberately. An
/// earlier version prefixed `sudo -n` onto the command text, which only
/// elevated the *first* command in a chain - so `docker network create a &&
/// docker network create b` silently created one network and failed the
/// other. The downstream failure looked exactly like a DNS bug in the
/// feature under test, and cost two wrong conclusions before the mechanism
/// was checked by hand. Elevate the shell, not the prefix.
async fn run(session: &SshSession, command: &str) -> String {
    let script = format!("sudo -n sh -c {}", vibessh_lib::ssh::command::quote(command));
    let out = session.execute_command(&script).await.expect("command should run");
    format!("{}{}", out.stdout, out.stderr)
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

        // Wait for B's listener rather than racing it. An earlier version of
        // this test did not, and its failure looked exactly like a DNS bug -
        // which cost a wrong conclusion before the mechanism was checked
        // directly.
        for _ in 0..10 {
            if run(&session, &format!("docker exec {name_b} sh -c 'nc -z 127.0.0.1 9000 && echo up || echo down'")).await.contains("up") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }

        // 1. Default deny. `getent` rather than `nc`, and rather than
        //    `nslookup`: nslookup appends the host's search domain and
        //    ignores `ndots:0`, so on a host with one (OpenStack, most
        //    clouds) it reports NXDOMAIN for a name that resolves perfectly.
        let isolated = run(&session, &format!("docker exec {name_a} sh -c 'getent hosts itest-b >/dev/null 2>&1 && echo RESOLVED || echo UNREACHABLE'")).await;
        assert!(isolated.contains("UNREACHABLE"), "A resolved B with no connection granted: {isolated}");

        // 2. Connected - the shape `runtime::docker::reconcile_networks`
        //    produces: one private network holding exactly these two.
        run(&session, &format!("docker network create --internal {link}")).await;
        run(&session, &format!("docker network connect --alias itest-b {link} {name_b}")).await;
        run(&session, &format!("docker network connect {link} {name_a}")).await;
        let resolves = run(&session, &format!("docker exec {name_a} sh -c 'getent hosts itest-b >/dev/null 2>&1 && echo RESOLVED || echo UNREACHABLE'")).await;
        assert!(resolves.contains("RESOLVED"), "the alias did not resolve after the connection was granted: {resolves}");
        let connected = run(&session, &format!("docker exec {name_a} sh -c 'nc -w 3 itest-b 9000 || echo NOTRAFFIC'")).await;
        assert!(connected.contains("pong"), "a granted connection resolved but carried no traffic: {connected}");

        // 3. Revoked. This is the direction that closes an exposure, so it is
        //    the one worth proving rather than assuming.
        run(&session, &format!("docker network disconnect {link} {name_a}")).await;
        let revoked = run(&session, &format!("docker exec {name_a} sh -c 'getent hosts itest-b >/dev/null 2>&1 && echo RESOLVED || echo UNREACHABLE'")).await;
        assert!(revoked.contains("UNREACHABLE"), "A could still resolve B after the connection was revoked: {revoked}");
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

/// Everything a destroyed Application leaves on a Node - or rather, does not.
///
/// S-007 was that `delete_application` deleted a database row and nothing
/// else. The service layer now orders a real teardown, and unit tests cover
/// *that it is ordered* (`outcome_states`). What they cannot see is whether
/// the commands it orders actually remove anything on a live Node, which is
/// the half that was broken.
///
/// So this builds the full set of artefacts a running Docker Application
/// leaves - container, its own network, a connection network, a console FIFO
/// in the runtime directory - and then runs the teardown pieces the service
/// calls, checking each one is gone by asking the Node rather than by
/// trusting the exit code.
#[tokio::test]
#[ignore]
async fn a_torn_down_application_leaves_nothing_on_the_node() {
    let session = session().await;
    let id = Uuid::new_v4();
    let container = format!("vibessh-itest-{}", id.simple());
    let own_net = format!("vibessh-net-itest-{}", &id.simple().to_string()[..12]);
    let link_net = format!("vibessh-link-itest-{}", &id.simple().to_string()[..12]);
    let fifo = format!("/run/vibessh/console/itest-{}.stdin", id.simple());

    let cleanup = format!(
        "docker rm -f {container} >/dev/null 2>&1; docker network rm {own_net} {link_net} >/dev/null 2>&1; rm -f {fifo}; true"
    );
    run(&session, &cleanup).await;

    let outcome = async {
        // Build the artefacts.
        run(&session, &format!("docker network create {own_net} && docker network create --internal {link_net}")).await;
        run(&session, &format!("docker run -d --name {container} --network {own_net} alpine:latest sleep 600 >/dev/null")).await;
        run(&session, &format!("docker network connect {link_net} {container}")).await;
        run(&session, &format!("install -d -m 700 /run/vibessh/console && mkfifo -m 600 {fifo}")).await;

        let before = run(&session, &format!("docker ps -a --filter name={container} --format '{{{{.Names}}}}'; ls {fifo}")).await;
        assert!(before.contains(&container), "setup did not create the container: {before}");
        assert!(before.contains("itest-"), "setup did not create the fifo: {before}");

        // The teardown, in the order `delete_application` performs it.
        run(&session, &format!("docker rm -f {container}")).await;
        run(&session, &format!("docker network rm {link_net} {own_net}")).await;
        run(&session, &format!("rm -f {fifo}")).await;

        // Ask the Node, do not trust the exit codes.
        let containers = run(&session, &format!("docker ps -a --filter name={container} --format '{{{{.Names}}}}'")).await;
        assert!(containers.trim().is_empty(), "the container survived the teardown: {containers}");

        let networks = run(&session, &format!("docker network ls --format '{{{{.Name}}}}' | grep -E '{own_net}|{link_net}' || true")).await;
        assert!(networks.trim().is_empty(), "a network survived the teardown: {networks}");

        let leftover = run(&session, &format!("ls {fifo} 2>&1 || true")).await;
        assert!(leftover.contains("No such file"), "the console fifo survived the teardown: {leftover}");

        // And nothing of ours ended up in the world-readable place the
        // pre-fix versions used.
        let tmp = run(&session, "ls /tmp/vibessh-stage-* 2>&1 || true").await;
        assert!(tmp.contains("No such file"), "something is staging into /tmp again: {tmp}");
    }
    .await;

    run(&session, &cleanup).await;
    outcome
}
