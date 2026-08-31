//! Drives `runtime::docker::DockerRuntime` against the real, dedicated
//! VibeSSH test server rather than only ever against the pure
//! `build_create_command` string assertions in `runtime::docker`'s own unit
//! tests - whether the real Docker daemon actually *honors* the bind-mount
//! and restart-policy flags those strings contain (Etap M1) is exactly what
//! those unit tests can't cover.
//!
//! `#[ignore]` - needs network access to one specific real host plus one
//! specific local private key file, neither of which exists in CI or on a
//! fresh clone, and a live Docker daemon on that host. Run explicitly:
//! `cargo test --test docker_runtime -- --ignored --test-threads=1`.
//!
//! **This server also runs several unrelated production services**
//! (its own Docker containers among them) that must never be touched.
//! Everything this test creates - the working directory and the
//! `vibessh-app-<uuid>` container - is uniquely named and removed again at
//! the end, on every path (pass, assertion failure, or panic - see
//! `outcome` below), never left behind.

use std::sync::Arc;

use vibessh_lib::models::{Application, ApplicationStatus, HealthCheckType, RuntimeType};
use vibessh_lib::runtime::{docker::DockerRuntime, ApplicationRuntime, RuntimeContext};
use vibessh_lib::ssh::{connect, SshAuth, SshCredentials, SshSession};

const TEST_HOST: &str = "94.130.201.103";
const TEST_USER: &str = "root";
const TEST_KEY_PATH: &str = r"C:\Users\kompu\.ssh\vibessh_dedi_ed25519";

async fn connect_test_session() -> Arc<SshSession> {
    let credentials = SshCredentials {
        host: TEST_HOST.to_string(),
        port: 22,
        username: TEST_USER.to_string(),
        auth: SshAuth::PrivateKey { path: TEST_KEY_PATH.to_string(), passphrase: None },
    };
    let outcome = connect(&credentials, None).await.expect("couldn't connect to the real test server - check the key/network");
    Arc::new(outcome.session)
}

fn stub_application(id: uuid::Uuid, working_directory: String) -> Application {
    Application {
        id,
        server_id: Some(uuid::Uuid::new_v4()),
        name: "Docker Runtime Real-Server Test".to_string(),
        description: None,
        blueprint_id: "generic-docker".to_string(),
        blueprint_version: 1,
        runtime_type: RuntimeType::Docker,
        working_directory,
        status: ApplicationStatus::Unknown,
        last_status_check_at: None,
        health_check_type: HealthCheckType::Process,
        health_check_port_id: None,
        health_check_http_path: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// Same format `runtime::docker::container_name` uses internally - not
/// itself exported (it's a private implementation detail), but its exact
/// shape is locked in by that module's own
/// `container_name_is_stable_and_namespaced` unit test, so relying on it
/// here (to `docker exec` into the container this test creates, by name) is
/// safe.
fn container_name(application_id: uuid::Uuid) -> String {
    format!("vibessh-app-{application_id}")
}

#[tokio::test]
#[ignore]
async fn bind_mounted_working_directory_survives_a_destroy_and_recreate_on_the_real_server() {
    let session = connect_test_session().await;
    let application_id = uuid::Uuid::new_v4();
    let working_directory = format!("/root/vibessh-docker-test-{application_id}");
    session.execute_command(&format!("mkdir -p {working_directory}")).await.expect("couldn't create the test working directory");

    let session_for_task = session.clone();
    let working_directory_for_task = working_directory.clone();
    let outcome = tokio::spawn(async move { run_test(session_for_task, application_id, working_directory_for_task).await }).await;

    // Always clean up the real server - pass, failure, or panic. `docker rm
    // -f` is a no-op (exit code != 0, ignored via `let _`) if the container
    // was already removed by the test itself before it failed.
    let name = container_name(application_id);
    let _ = session.execute_command(&format!("docker rm -f {name}")).await;
    let _ = session.execute_command(&format!("rm -rf {working_directory}")).await;

    outcome.expect("the Docker runtime real-server test panicked (see the captured message above)");
}

async fn run_test(session: Arc<SshSession>, application_id: uuid::Uuid, working_directory: String) {
    let application = stub_application(application_id, working_directory.clone());
    let runtime_config = serde_json::json!({ "image": "alpine:latest", "command": ["sh", "-c", "sleep 3600"] });
    let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: Some(session.clone()) };
    let runtime = DockerRuntime::new();
    let name = container_name(application_id);

    // Create + start.
    runtime.start(&ctx).await.expect("start() should create and start the container");
    assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Running);

    // The restart policy flag was really applied by the daemon, not just
    // present in the command string `build_create_command` produced.
    let restart_policy = session
        .execute_command(&format!("docker inspect --format '{{{{.HostConfig.RestartPolicy.Name}}}}' {name}"))
        .await
        .unwrap();
    assert_eq!(restart_policy.stdout.trim(), "unless-stopped", "{restart_policy:?}");

    // A file written on the HOST side of the bind mount is visible INSIDE
    // the container at the same path - proves `-v dir:dir` (not just
    // `-w dir`) actually took effect.
    session.execute_command(&format!("echo marker-before-recreate > {working_directory}/marker.txt")).await.unwrap();
    let seen_inside = session.execute_command(&format!("docker exec {name} cat {working_directory}/marker.txt")).await.unwrap();
    assert_eq!(seen_inside.stdout.trim(), "marker-before-recreate", "{seen_inside:?}");

    // Recreate - the whole point of Etap M1: this must NOT destroy the
    // bind-mounted file, unlike the pre-M1 behavior where a container's
    // writable layer was the only place its state lived.
    runtime.destroy(&ctx).await.expect("destroy() should remove the container");
    assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Stopped, "destroy() must leave no container behind");
    runtime.start(&ctx).await.expect("start() should recreate the container after destroy()");
    assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Running);

    let seen_after_recreate = session.execute_command(&format!("docker exec {name} cat {working_directory}/marker.txt")).await.unwrap();
    assert_eq!(
        seen_after_recreate.stdout.trim(),
        "marker-before-recreate",
        "the bind-mounted working directory must survive a destroy+recreate - got {seen_after_recreate:?}"
    );

    runtime.destroy(&ctx).await.expect("final destroy() should succeed");
}
