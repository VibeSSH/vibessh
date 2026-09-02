//! What the Vibe AI assistant actually sends, against real repositories.
//!
//! The unit tests in `ai::sanitizer` prove the redaction rules in
//! isolation, and the ones in `services::ai_service` prove the
//! orchestration against a mock provider. Neither proves the join: that a
//! context assembled from a real SQLite database, with a real Application
//! carrying real secret-looking values, comes out clean. That is what this
//! file is for, and it is the test worth having, because the failure it
//! guards against is a disclosure rather than a crash.
//!
//! Nothing here touches the network. The Node used for the Node-context
//! case points at `127.0.0.1:1`, which refuses instantly - that exercises
//! the "a probe failed, record it as a gap" path without waiting on a
//! timeout, and without sending a packet anywhere but the loopback.

use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;
use vibessh_lib::errors::ErrorCode;
use vibessh_lib::models::{
    AiContextRef, AiMode, AuthenticationType, CreateApplicationInput, EnvironmentVariable, PortInput, PortProtocol, PortVisibility,
    RuntimeType, ServerInput,
};
use vibessh_lib::runtime::local_process::LocalProcessManager;
use vibessh_lib::services;
use vibessh_lib::state::{CloudState, SshSessionManager};
use vibessh_lib::storage::application_repository::ApplicationRepository;
use vibessh_lib::storage::firewall_rule_repository::FirewallRuleRepository;
use vibessh_lib::storage::log_capture::LogCaptureStore;
use vibessh_lib::storage::node_network_repository::NodeNetworkRepository;
use vibessh_lib::storage::server_repository::ServerRepository;

/// The values that must never appear in a collected context. Each one is
/// planted somewhere different, so a single assertion loop covers every
/// route a secret could take out of the database.
const PLANTED_SECRETS: &[&str] = &[
    "keyring-root-pw",       // a secret-flagged environment variable
    "looks-ordinary-token",  // a plain variable with a secret-sounding name
    "requirepass-value",     // a value inside a blueprint command array
    "connstring-password",   // a password inside a connection string
];

struct Fixture {
    db: PathBuf,
    logs: PathBuf,
    applications: ApplicationRepository,
    servers: ServerRepository,
    networks: NodeNetworkRepository,
    firewall_rules: FirewallRuleRepository,
    ssh_sessions: SshSessionManager,
    local_processes: Arc<LocalProcessManager>,
    log_capture: LogCaptureStore,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let db = std::env::temp_dir().join(format!("vibessh-ai-{label}-{}.sqlite3", Uuid::new_v4()));
        let logs = std::env::temp_dir().join(format!("vibessh-ai-logs-{}", Uuid::new_v4()));
        Self {
            applications: ApplicationRepository::open(&db).unwrap(),
            servers: ServerRepository::open(&db).unwrap(),
            networks: NodeNetworkRepository::open(&db).unwrap(),
            firewall_rules: FirewallRuleRepository::open(&db).unwrap(),
            ssh_sessions: SshSessionManager::new(),
            local_processes: Arc::new(LocalProcessManager::new()),
            log_capture: LogCaptureStore::new(logs.clone()).unwrap(),
            db,
            logs,
        }
    }

    async fn context(&self, mode: AiMode, reference: Option<AiContextRef>) -> Option<vibessh_lib::models::AiContextBundle> {
        services::build_ai_context(
            &self.applications,
            &self.servers,
            &self.networks,
            &self.firewall_rules,
            &self.ssh_sessions,
            &self.local_processes,
            &self.log_capture,
            mode,
            reference,
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.db).ok();
        std::fs::remove_dir_all(&self.logs).ok();
    }
}

/// A local Application, so no probe reaches for SSH and the test stays fast.
fn application_with_planted_secrets() -> CreateApplicationInput {
    CreateApplicationInput {
        server_id: None,
        name: "redis-cache".to_string(),
        description: Some("session store".to_string()),
        blueprint_id: "redis".to_string(),
        blueprint_version: 1,
        runtime_type: RuntimeType::LocalProcess,
        working_directory: "/srv/redis-cache".to_string(),
        environment: vec![
            EnvironmentVariable { key: "MYSQL_ROOT_PASSWORD".into(), value: "keyring-root-pw".into(), is_secret: true },
            // Not flagged secret by the user, but named like one. This is
            // the case the name-based rule exists for.
            EnvironmentVariable { key: "SOME_TOKEN".into(), value: "looks-ordinary-token".into(), is_secret: false },
            EnvironmentVariable { key: "MYSQL_DATABASE".into(), value: "appdb".into(), is_secret: false },
            EnvironmentVariable {
                key: "REPORTING_DSN".into(),
                value: "mysql://reporter:connstring-password@db.internal/reporting".into(),
                is_secret: false,
            },
        ],
        ports: vec![PortInput {
            name: "redis".to_string(),
            protocol: PortProtocol::Tcp,
            bind_address: "127.0.0.1".to_string(),
            internal_port: 6379,
            external_port: Some(6379),
            visibility: PortVisibility::Localhost,
            required: true,
        }],
        // The shape `RedisBlueprint::render_runtime_config` actually
        // produces, secret and all.
        runtime_config: serde_json::json!({
            "image": "redis:7",
            "command": ["redis-server", "--appendonly", "yes", "--requirepass", "requirepass-value"],
            "memoryLimitMb": 512,
            "cpuLimitCores": 1.0
        }),
        metadata: serde_json::json!({}),
    }
}

#[tokio::test]
async fn an_application_context_carries_the_configuration_and_none_of_the_secrets() {
    let fixture = Fixture::new("app");
    let created = fixture.applications.create(&application_with_planted_secrets()).unwrap();

    let bundle = fixture
        .context(AiMode::Diagnose, Some(AiContextRef::Application { id: created.application.id }))
        .await
        .expect("Diagnose with an Application reference must produce a context");

    let everything = format!("{}\n{}", bundle.summary, bundle.notes.join("\n"));

    for secret in PLANTED_SECRETS {
        assert!(!everything.contains(secret), "the context leaked {secret}:\n{everything}");
    }

    // The other half of the bargain: having redacted the secrets, the
    // context must still be worth sending.
    assert!(everything.contains("redis-cache"), "the Application's name is missing");
    assert!(everything.contains("redis"), "the blueprint is missing");
    assert!(everything.contains("6379"), "the port is missing");
    assert!(everything.contains("512"), "the memory limit is missing");
    assert!(everything.contains("MYSQL_DATABASE=appdb"), "a non-secret variable was over-redacted");
    // Present-but-withheld, not absent - "no root password is set" is a
    // different diagnosis from "one is set and you cannot see it".
    assert!(everything.contains("MYSQL_ROOT_PASSWORD=***"), "the secret variable should still be listed");
    // The username survives the connection string; only the password goes.
    assert!(everything.contains("reporter"), "the DSN's user should survive redaction");
}

#[tokio::test]
async fn a_node_context_reports_what_it_could_not_reach_instead_of_inventing_it() {
    let fixture = Fixture::new("node");
    let server = fixture
        .servers
        .create(&ServerInput {
            name: "unreachable-node".to_string(),
            // Refuses immediately rather than hanging: this exercises the
            // failure path without a timeout and without leaving the machine.
            host: "127.0.0.1".into(),
            ssh_port: 1,
            username: "root".into(),
            authentication_type: AuthenticationType::PrivateKey,
            private_key_path: Some("/home/someone/.ssh/id_ed25519_production".into()),
            group_id: None,
            password: None,
            key_passphrase: None,
        })
        .unwrap();

    let bundle = fixture.context(AiMode::Diagnose, Some(AiContextRef::Node { id: server.id })).await.expect("a Node context");
    let everything = format!("{}\n{}", bundle.summary, bundle.notes.join("\n"));

    assert!(everything.contains("unreachable-node"));
    // A probe that could not run is a recorded gap, not a silent omission -
    // this is what lets the model say what it does not know.
    assert!(!bundle.notes.is_empty(), "an unreachable Node should have produced notes:\n{everything}");

    // The path to a private key is not a secret, and is deliberately still
    // not sent: it names the user's home directory and the key's filename,
    // and it answers no support question. Only its presence is reported.
    assert!(!everything.contains("id_ed25519_production"), "the key path must not be sent:\n{everything}");
    assert!(!everything.contains("/home/someone"), "the key path must not be sent:\n{everything}");
    assert!(everything.contains("Private key file configured: yes"));

    // "Never probed" must not read as "not available".
    assert!(everything.contains("never probed"), "capabilities were never probed and should say so");
}

/// `Ask` is a promise that nothing is read from the user's infrastructure.
/// It has to hold even when the frontend passes a context reference - the
/// mode is the authority, not the caller.
#[tokio::test]
async fn ask_mode_collects_nothing_even_when_handed_a_reference() {
    let fixture = Fixture::new("ask");
    let created = fixture.applications.create(&application_with_planted_secrets()).unwrap();

    let bundle = fixture.context(AiMode::Ask, Some(AiContextRef::Application { id: created.application.id })).await;
    assert!(bundle.is_none(), "Ask mode must not collect a context");
}

#[tokio::test]
async fn a_reference_to_something_deleted_produces_a_note_rather_than_a_failure() {
    let fixture = Fixture::new("missing");

    let bundle = fixture.context(AiMode::Diagnose, Some(AiContextRef::Application { id: Uuid::new_v4() })).await.unwrap();
    assert!(bundle.summary.is_empty());
    assert!(!bundle.notes.is_empty());

    let bundle = fixture.context(AiMode::Diagnose, Some(AiContextRef::Node { id: Uuid::new_v4() })).await.unwrap();
    assert!(bundle.notes.iter().any(|note| note.contains("no longer in the local database")));
}

/// With nothing configured, the assistant refuses before it can read a key,
/// build a client or resolve a hostname.
#[tokio::test]
async fn a_disabled_assistant_refuses_before_it_touches_a_key_or_a_session() {
    let dir = std::env::temp_dir().join(format!("vibessh-ai-config-{}", Uuid::new_v4()));
    // A signed-out cloud state, which is what a fresh install has. The
    // refusal must come from `enabled` being false, before anything reaches
    // for a token - otherwise a disabled assistant would report "sign in"
    // rather than "set me up".
    let cloud = CloudState::new("http://localhost:8787".to_string());
    // `Box<dyn AiProvider>` is not Debug, so the Ok side cannot be unwrapped
    // by `expect_err`; matched by hand instead.
    let error = match services::resolve_ai_provider(&dir, &cloud).await {
        Err(error) => error,
        Ok(_) => panic!("a fresh install must not resolve a provider"),
    };
    assert_eq!(error.code(), ErrorCode::AiNotConfigured);
}

/// The included model needs an account, because the daily allowance is per
/// account. Signed out, that has to read as "sign in", not as a
/// configuration problem - the two send the user to different places.
#[tokio::test]
async fn the_included_model_refuses_with_unauthorized_when_signed_out() {
    let dir = std::env::temp_dir().join(format!("vibessh-ai-config-{}", Uuid::new_v4()));
    vibessh_lib::storage::ai_config::save_ai_config(
        &dir,
        &vibessh_lib::models::AiConfig {
            enabled: true,
            provider: vibessh_lib::models::AiProviderKind::VibeSshHosted,
            base_url: String::new(),
            model: String::new(),
        },
    )
    .unwrap();

    let cloud = CloudState::new("http://localhost:8787".to_string());
    let error = match services::resolve_ai_provider(&dir, &cloud).await {
        Err(error) => error,
        Ok(_) => panic!("the hosted provider must not resolve without a session"),
    };
    assert_eq!(error.code(), ErrorCode::Unauthorized);
    std::fs::remove_dir_all(&dir).ok();
}
