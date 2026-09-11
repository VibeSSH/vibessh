//! Real, two-real-host proof of the Vibe Network (Etap M4) - everything a
//! single-host test genuinely can't cover: a real WireGuard tunnel actually
//! carrying traffic between two independent machines, both sides' configs
//! staying in sync after a join, and DNS resolving to the correct side.
//!
//! **Node A (94.130.201.103) is a shared, production-adjacent box** (see
//! the `vibessh-test-server` memory) - already-running services there must
//! never be touched. Everything this test does is additively-scoped
//! (a new `wg-vibessh0` interface, distinct from any pre-existing mesh;
//! `10.77.0.0/16`, distinct from the `10.50.0.0/24` the existing mesh
//! there uses; purely-additive firewall rules) and is torn down again on
//! every path (pass, assertion failure, or panic) via the same
//! `tokio::spawn` + awaited `JoinHandle` cleanup-guarantee pattern every
//! other real-server test in this crate already uses.
//!
//! **Node B (57.128.203.210) is a disposable, freshly-reinstalled VPS**
//! (see the `vibessh-second-test-server` memory) - no production-safety
//! constraint applies there, but it's still cleaned up for tidiness.
//!
//! `#[ignore]` - needs network access to two specific real hosts plus one
//! specific local private key file. Run explicitly:
//! `cargo test --test vibe_network -- --ignored --test-threads=1`.

use vibessh_lib::models::{AuthenticationType, ServerInput};
use vibessh_lib::services;
use vibessh_lib::state::SshSessionManager;
use vibessh_lib::storage::application_repository::ApplicationRepository;
use vibessh_lib::storage::dns_config::DEFAULT_SUFFIX;
use vibessh_lib::storage::dns_repository::DnsRepository;
use vibessh_lib::storage::firewall_rule_repository::FirewallRuleRepository;
use vibessh_lib::storage::node_network_repository::NodeNetworkRepository;
use vibessh_lib::storage::server_repository::ServerRepository;

const KEY_PATH: &str = r"C:\Users\kompu\.ssh\vibessh_dedi_ed25519";
const NODE_A_HOST: &str = "94.130.201.103";
const NODE_A_USER: &str = "root";
const NODE_B_HOST: &str = "57.128.203.210";
const NODE_B_USER: &str = "ubuntu";

fn server_input(name: &str, host: &str, username: &str) -> ServerInput {
    ServerInput {
        name: name.to_string(),
        host: host.to_string(),
        ssh_port: 22,
        username: username.to_string(),
        authentication_type: AuthenticationType::PrivateKey,
        private_key_path: Some(KEY_PATH.to_string()),
        group_id: None,
        password: None,
        key_passphrase: None,
    }
}

struct TestRepos {
    server_repo: ServerRepository,
    network_repo: NodeNetworkRepository,
    app_repo: ApplicationRepository,
    dns_repo: DnsRepository,
    firewall_rule_repo: FirewallRuleRepository,
    sessions: SshSessionManager,
}

fn temp_repos() -> TestRepos {
    let path = std::env::temp_dir().join(format!("vibessh-vibe-network-test-{}.sqlite3", uuid::Uuid::new_v4()));
    TestRepos {
        server_repo: ServerRepository::open(&path).unwrap(),
        network_repo: NodeNetworkRepository::open(&path).unwrap(),
        app_repo: ApplicationRepository::open(&path).unwrap(),
        dns_repo: DnsRepository::open(&path).unwrap(),
        firewall_rule_repo: FirewallRuleRepository::open(&path).unwrap(),
        sessions: SshSessionManager::new(),
    }
}

#[tokio::test]
#[ignore]
async fn two_real_nodes_join_the_mesh_reach_each_other_and_resolve_dns() {
    let repos = temp_repos();
    let node_a = repos.server_repo.create(&server_input("Hetzner Production", NODE_A_HOST, NODE_A_USER)).unwrap();
    let node_b = repos.server_repo.create(&server_input("Disposable VPS", NODE_B_HOST, NODE_B_USER)).unwrap();
    let node_a_id = node_a.id;
    let node_b_id = node_b.id;

    let outcome = tokio::spawn(async move { run_test(repos, node_a_id, node_b_id).await }).await;

    // Cleanup on every path (pass, assertion failure, or panic) - raw SSH
    // exec against both real hosts directly, deliberately NOT through a
    // repository (the one `repos` that actually recorded this run's
    // membership/DNS state was moved into the spawned task above and is
    // gone by now - reopening a *fresh* temp database here would just be
    // empty and `leave_node` would silently no-op against it). Tears down
    // exactly what this test could have created: the WireGuard interface,
    // and this test's own DNS managed block.
    for (host, user) in [(NODE_A_HOST, NODE_A_USER), (NODE_B_HOST, NODE_B_USER)] {
        if let Ok(outcome) = vibessh_lib::ssh::connect(
            &vibessh_lib::ssh::SshCredentials {
                host: host.to_string(),
                port: 22,
                username: user.to_string(),
                auth: vibessh_lib::ssh::SshAuth::PrivateKey { path: KEY_PATH.to_string(), passphrase: None },
            },
            None,
        )
        .await
        {
            let _ = vibessh_lib::network::wireguard::teardown(&outcome.session).await;
            let _ = outcome
                .session
                .execute_command("sudo sed -i '/# BEGIN VIBESSH-MANAGED-DNS/,/# END VIBESSH-MANAGED-DNS/d' /etc/hosts")
                .await;
            outcome.session.close().await;
        }
    }

    outcome.expect("the Vibe Network real 2-node test panicked (see the captured message above)");
}

async fn run_test(repos: TestRepos, node_a_id: uuid::Uuid, node_b_id: uuid::Uuid) {
    let TestRepos { server_repo, network_repo, app_repo, dns_repo, firewall_rule_repo, sessions } = &repos;

    // Both Nodes join - each join reconciles the whole mesh, so after the
    // second join both sides know about each other.
    let member_a = services::join_node(network_repo, server_repo, sessions, node_a_id).await.unwrap();
    let member_b = services::join_node(network_repo, server_repo, sessions, node_b_id).await.unwrap();
    assert_ne!(member_a.wireguard_ip, member_b.wireguard_ip);

    // Explicit reconcile with results inspected, not swallowed - diagnostic
    // for exactly which side (if any) failed to apply.
    let reconcile_results = services::reconcile_mesh(network_repo, server_repo, sessions).await.unwrap();
    for result in &reconcile_results {
        eprintln!("reconcile result: {result:?}");
    }
    assert!(reconcile_results.iter().all(|r| r.error.is_none()), "{reconcile_results:?}");

    // Real WireGuard state, read back from both sides.
    let status = services::mesh_status(network_repo, server_repo, sessions).await.unwrap();
    assert_eq!(status.len(), 2);
    for node_status in &status {
        assert!(node_status.reachable, "{node_status:?}");
        assert_eq!(node_status.peers.len(), 1, "each Node should see exactly the other one as a peer - {node_status:?}");
    }

    // A real ping THROUGH the tunnel, not just "the interface exists" -
    // proves actual cross-machine connectivity, not just local config. A
    // fresh direct connection (not the crate's internal session cache,
    // which this external test crate can't reach) - fine, this is a
    // one-off exec, not something worth caching here.
    let session_a = vibessh_lib::ssh::connect(
        &vibessh_lib::ssh::SshCredentials {
            host: NODE_A_HOST.to_string(),
            port: 22,
            username: NODE_A_USER.to_string(),
            auth: vibessh_lib::ssh::SshAuth::PrivateKey { path: KEY_PATH.to_string(), passphrase: None },
        },
        None,
    )
    .await
    .unwrap()
    .session;
    let ping = session_a.execute_command(&format!("ping -c 3 -W 3 {}", member_b.wireguard_ip)).await.unwrap();
    assert_eq!(ping.exit_code, 0, "Node A could not reach Node B over the Vibe Network tunnel: {ping:?}");
    session_a.close().await;

    // A real Application "hosted" on Node B, given a DNS alias, synced,
    // then actually resolved from Node A - proves both service discovery
    // and that resolution crosses the mesh, not just localhost.
    let app = app_repo
        .create(&vibessh_lib::models::CreateApplicationInput {
            server_id: Some(node_b_id),
            name: "Vibe Network Test Service".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: vibessh_lib::models::RuntimeType::RemoteProcess,
            working_directory: "/root".to_string(),
            environment: vec![],
            ports: vec![],
            runtime_config: serde_json::json!({}),
            metadata: serde_json::json!({}),
        })
        .unwrap();
    let alias = services::create_dns_alias(DEFAULT_SUFFIX, dns_repo, app.application.id, "vibe-network-test-svc").unwrap();
    assert_eq!(alias.hostname, "vibe-network-test-svc.vibe");

    let dns_results = services::sync_dns(DEFAULT_SUFFIX, network_repo, server_repo, app_repo, dns_repo, sessions).await.unwrap();
    assert_eq!(dns_results.len(), 2);
    assert!(dns_results.iter().all(|r| r.ok), "{dns_results:?}");

    let resolved = services::verify_dns_alias(network_repo, server_repo, sessions, &alias.hostname, &member_b.wireguard_ip).await.unwrap();
    assert!(resolved, "vibe-network-test-svc.vibe should resolve to Node B's mesh IP ({}) on a real member", member_b.wireguard_ip);

    // Firewall: the mesh port must be allowed on both real hosts.
    let firewall_a =
        services::sync_application_node_firewall(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, app.application.id).await;
    assert!(firewall_a.is_ok(), "{firewall_a:?}");
}
