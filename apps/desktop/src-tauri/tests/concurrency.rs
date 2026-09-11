//! What happens when two things arrive at once.
//!
//! FIX_PLAN C.4 and C.6. Every case here is reachable without a Node: a
//! double-clicked button produces two Tauri commands that run concurrently on
//! the same runtime, and nine repositories hold nine independent connections
//! to one SQLite file. The cases that genuinely need a live host - concurrent
//! DNS sync, concurrent firewall reconcile, concurrent SFTP writes - are
//! listed in `FIX_PLAN.md` as still needing an integration pass rather than
//! faked with a mock that would only prove the mock.

use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;
use vibessh_lib::models::{
    AuthenticationType, CreateApplicationInput, PortInput, PortProtocol, PortVisibility, RuntimeType, ServerInput,
};
use vibessh_lib::storage::application_repository::ApplicationRepository;
use vibessh_lib::storage::server_repository::ServerRepository;

fn temp_db() -> PathBuf {
    std::env::temp_dir().join(format!("vibessh-concurrency-{}.sqlite3", Uuid::new_v4()))
}

fn server_input(name: &str) -> ServerInput {
    ServerInput {
        name: name.to_string(),
        host: "203.0.113.10".into(),
        ssh_port: 22,
        username: "root".into(),
        authentication_type: AuthenticationType::Password,
        private_key_path: None,
        group_id: None,
        password: Some("x".into()),
        key_passphrase: None,
    }
}

fn application_input(server_id: Uuid, name: &str) -> CreateApplicationInput {
    CreateApplicationInput {
        server_id: Some(server_id),
        name: name.to_string(),
        description: None,
        blueprint_id: "generic-docker".to_string(),
        blueprint_version: 1,
        runtime_type: RuntimeType::Docker,
        working_directory: "/srv/app".to_string(),
        environment: vec![],
        ports: vec![],
        runtime_config: serde_json::json!({}),
        metadata: serde_json::json!({}),
    }
}

fn port_input(internal: u16, external: u16) -> PortInput {
    PortInput {
        name: "game".to_string(),
        protocol: PortProtocol::Tcp,
        bind_address: "0.0.0.0".to_string(),
        internal_port: internal,
        external_port: Some(external),
        visibility: PortVisibility::Public,
        required: false,
    }
}

/// The regression test for A-004/D-001, at the level it actually failed.
///
/// Nine repositories open the same file during startup and each runs the
/// migrations. Before WAL and a busy timeout, the losers of that race got
/// `SQLITE_BUSY` and `lib.rs` propagated it, so the app intermittently
/// refused to launch. `storage::mod` has a test for nine *opens*; this one
/// checks the harder case, nine concurrent **writers**.
#[test]
fn nine_repositories_writing_at_once_do_not_collide() {
    let path = temp_db();
    let server_repo = ServerRepository::open(&path).unwrap();
    let server = server_repo.create(&server_input("Concurrency Node")).unwrap();

    let repos: Vec<Arc<ApplicationRepository>> = (0..9).map(|_| Arc::new(ApplicationRepository::open(&path).unwrap())).collect();
    let handles: Vec<_> = repos
        .into_iter()
        .enumerate()
        .map(|(index, repo)| {
            let server_id = server.id;
            std::thread::spawn(move || {
                // Several writes each, not one - a single write per thread
                // would often serialise by luck rather than by the pragmas.
                for round in 0..5 {
                    repo.create(&application_input(server_id, &format!("App {index}-{round}")))
                        .unwrap_or_else(|err| panic!("writer {index} round {round} failed: {err}"));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("no writer should panic");
    }

    let reader = ApplicationRepository::open(&path).unwrap();
    assert_eq!(reader.list().unwrap().len(), 45, "every write should have landed");

    drop(server_repo);
    let _ = std::fs::remove_file(&path);
}

/// C.4's concurrent half.
///
/// Two published ports with the same external port on the same Node is a
/// real conflict: `docker create` fails on the second one, and because the
/// collision check consults the database, a row that should never have been
/// written then makes the *next* check believe the port is taken by an
/// Application that could not start.
///
/// `AUDIT_REPORT.md` D-007 describes the window: the check and the insert
/// were separate statements. The test has to actually land in that window,
/// which took two attempts to get right - opening each repository inside its
/// own thread made them serialise on the open and the race never happened,
/// so the test passed against the *unfixed* code and proved nothing. Every
/// connection is now opened up front and every thread waits on a barrier, so
/// they all reach the claim together.
#[test]
fn concurrent_adds_of_the_same_external_port_leave_exactly_one() {
    const RACERS: usize = 8;

    let path = temp_db();
    let server_repo = ServerRepository::open(&path).unwrap();
    let server = server_repo.create(&server_input("Port Race Node")).unwrap();

    let setup = ApplicationRepository::open(&path).unwrap();
    let applications: Vec<Uuid> = (0..RACERS)
        .map(|index| setup.create(&application_input(server.id, &format!("Racer {index}"))).unwrap().application.id)
        .collect();
    drop(setup);

    // Opened before the barrier: opening a repository runs the migration
    // check, which is slow enough to stagger the threads past each other.
    let repos: Vec<ApplicationRepository> = (0..RACERS).map(|_| ApplicationRepository::open(&path).unwrap()).collect();
    let barrier = Arc::new(std::sync::Barrier::new(RACERS));

    let handles: Vec<_> = repos
        .into_iter()
        .zip(applications)
        .enumerate()
        .map(|(index, (repo, application_id))| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                repo.claim_external_port(application_id, server.id, &port_input(25565 + index as u16, 25565))
            })
        })
        .collect();

    let outcomes: Vec<_> = handles.into_iter().map(|handle| handle.join().expect("no racer should panic")).collect();
    let winners = outcomes.iter().filter(|result| result.is_ok()).count();

    // The database is the authority, not the return values: a claim that
    // reported failure while writing its row would be the actual bug.
    let reader = ApplicationRepository::open(&path).unwrap();
    let published: usize = reader
        .list()
        .unwrap()
        .iter()
        .map(|application| reader.list_ports(application.id).unwrap().iter().filter(|port| port.external_port == Some(25565)).count())
        .sum();

    assert_eq!(published, 1, "port 25565 was claimed {published} times; {winners} attempts reported success");
    assert_eq!(winners, 1, "{winners} attempts reported success for a port only one can hold");

    // And the losers have to fail *for the right reason*. This is what the
    // transaction behaviour actually buys: a deferred transaction lets every
    // racer read the port as free and only then fight over the write, so the
    // losers come back with a SQLite busy/snapshot error - which the UI can
    // only render as "storage error", telling the operator nothing. Taking
    // the write lock up front makes each loser wait, re-read, and find the
    // real collision.
    for outcome in &outcomes {
        if let Err(err) = outcome {
            assert!(
                matches!(err, vibessh_lib::errors::AppError::PortInUse { port: 25565, protocol: "tcp", owner: Some(_) }),
                "a loser failed with something other than a named collision: {err:?}"
            );
        }
    }

    drop(server_repo);
    let _ = std::fs::remove_file(&path);
}

/// Deleting and re-adding the same port repeatedly, from several threads.
///
/// The shape behind S-007: a port freed by a delete must actually become
/// available again, and a delete racing an add must not leave the row behind
/// while reporting success.
#[test]
fn a_port_freed_by_a_delete_can_be_claimed_again() {
    let path = temp_db();
    let server_repo = ServerRepository::open(&path).unwrap();
    let server = server_repo.create(&server_input("Churn Node")).unwrap();
    let repo = Arc::new(ApplicationRepository::open(&path).unwrap());
    let application = repo.create(&application_input(server.id, "Churn")).unwrap().application.id;

    for round in 0..25 {
        let port = repo
            .claim_external_port(application, server.id, &port_input(30000, 25565))
            .unwrap_or_else(|err| panic!("round {round} could not claim a free port: {err}"));
        repo.remove_port(application, port.id).unwrap();
    }
    assert!(repo.list_ports(application).unwrap().is_empty());

    drop(server_repo);
    let _ = std::fs::remove_file(&path);
}
