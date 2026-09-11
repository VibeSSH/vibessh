//! Drives `files::sftp::SftpApplicationFileProvider` against the real,
//! dedicated VibeSSH test server rather than only ever against local temp
//! directories in `files::local`'s own unit tests - the SFTP `REALPATH`-
//! based sandbox jail, and every provider method's actual wire behavior
//! against a genuine OpenSSH server (as opposed to `russh_sftp`'s
//! documented-but-unverified-here semantics), is exactly what those local
//! tests can't cover.
//!
//! `#[ignore]` - needs network access to one specific real host plus one
//! specific local private key file, neither of which exists in CI or on a
//! fresh clone. Run explicitly:
//! `cargo test --test application_files_sftp -- --ignored --test-threads=1`.
//!
//! **This server also runs several unrelated production services**
//! (Docker, MariaDB, nginx, a game panel, WireGuard proxies, ...) that must
//! never be touched. Everything this test does lives under one freshly
//! created, uniquely named directory under the test account's home
//! directory - removed again at the end, on every path (pass, assertion
//! failure, or panic - see `outcome` below), never left behind.

use std::sync::Arc;

use vibessh_lib::files::sftp::SftpApplicationFileProvider;
use vibessh_lib::files::ApplicationFileProvider;
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

#[tokio::test]
#[ignore]
async fn sftp_provider_full_crud_round_trips_against_the_real_server() {
    let session = connect_test_session().await;
    let root = format!("/root/vibessh-files-test-{}", uuid::Uuid::new_v4());
    session.execute_command(&format!("mkdir -p {root}")).await.expect("couldn't create the test root directory");

    // Run the actual assertions on a spawned task so a panic (a failed
    // assert_eq!/unwrap partway through) is captured as a JoinError rather
    // than aborting this test function before the cleanup below runs.
    let session_for_task = session.clone();
    let root_for_task = root.clone();
    let outcome = tokio::spawn(async move { run_full_crud(session_for_task, root_for_task).await }).await;

    // Always clean up the real server - pass, failure, or panic.
    let _ = session.execute_command(&format!("rm -rf {root}")).await;

    outcome.expect("the CRUD round trip against the real server panicked (see the captured message above)");
}

async fn run_full_crud(session: Arc<SshSession>, root: String) {
    let provider = SftpApplicationFileProvider::new(session, root);

    // write + read
    provider.write_file("server.properties", b"motd=hello").await.unwrap();
    assert_eq!(provider.read_file("server.properties").await.unwrap(), b"motd=hello");

    // list
    let listing = provider.list_directory(".").await.unwrap();
    assert_eq!(listing.len(), 1, "{listing:?}");
    assert_eq!(listing[0].name, "server.properties");
    assert!(!listing[0].is_dir);

    // mkdir + nested write
    provider.create_directory("plugins").await.unwrap();
    provider.write_file("plugins/MyPlugin.jar", b"jar-bytes").await.unwrap();

    // rename
    provider.rename("server.properties", "server.properties.bak").await.unwrap();
    assert!(provider.read_file("server.properties").await.is_err());
    assert_eq!(provider.read_file("server.properties.bak").await.unwrap(), b"motd=hello");

    // recursive copy
    provider.copy("plugins", "plugins-copy").await.unwrap();
    assert_eq!(provider.read_file("plugins-copy/MyPlugin.jar").await.unwrap(), b"jar-bytes");

    // metadata + chmod
    let meta = provider.metadata("plugins/MyPlugin.jar").await.unwrap();
    assert!(!meta.is_dir);
    assert_eq!(meta.size, 9);
    provider.set_permissions("plugins/MyPlugin.jar", 0o640).await.unwrap();
    let meta_after = provider.metadata("plugins/MyPlugin.jar").await.unwrap();
    assert_eq!(meta_after.permissions.map(|p| p & 0o777), Some(0o640), "{meta_after:?}");

    // download then re-upload, through a real local temp file, with
    // progress callbacks actually reporting the full byte count.
    let local_path = std::env::temp_dir().join(format!("vibessh-sftp-real-server-test-{}.jar", uuid::Uuid::new_v4()));
    let mut downloaded = 0u64;
    provider.download_file("plugins/MyPlugin.jar", &local_path, &mut |n| downloaded += n).await.unwrap();
    assert_eq!(downloaded, 9);
    assert_eq!(std::fs::read(&local_path).unwrap(), b"jar-bytes");

    let mut uploaded = 0u64;
    provider.upload_file(&local_path, "plugins/MyPlugin-reuploaded.jar", &mut |n| uploaded += n).await.unwrap();
    assert_eq!(uploaded, 9);
    assert_eq!(provider.read_file("plugins/MyPlugin-reuploaded.jar").await.unwrap(), b"jar-bytes");
    let _ = std::fs::remove_file(&local_path);

    // Path traversal rejected against the real server's own REALPATH
    // resolution - not a local-filesystem stand-in for it.
    assert!(provider.read_file("../../../etc/passwd").await.is_err());
    assert!(provider.write_file("../escape.txt", b"x").await.is_err());

    // recursive delete
    provider.delete("plugins-copy").await.unwrap();
    assert!(provider.list_directory("plugins-copy").await.is_err());
}

/// A real symlink, planted directly on the server (outside anything this
/// provider's own `resolve()` created), whose target resolves outside the
/// sandbox root - proves the SFTP `REALPATH`-based escape check works
/// against a real OpenSSH server's actual symlink resolution, not an
/// assumption about how it behaves.
#[tokio::test]
#[ignore]
async fn a_real_symlink_pointing_outside_the_sandbox_is_blocked_on_the_real_server() {
    let session = connect_test_session().await;
    let root = format!("/root/vibessh-files-test-{}", uuid::Uuid::new_v4());
    let outside = format!("/root/vibessh-files-test-outside-{}", uuid::Uuid::new_v4());
    session.execute_command(&format!("mkdir -p {root} {outside} && echo 'top secret' > {outside}/secret.txt")).await.unwrap();

    let session_for_task = session.clone();
    let root_for_task = root.clone();
    let outside_for_task = outside.clone();
    let outcome = tokio::spawn(async move {
        session_for_task.execute_command(&format!("ln -s {outside_for_task}/secret.txt {root_for_task}/link.txt")).await.unwrap();

        let provider = SftpApplicationFileProvider::new(session_for_task, root_for_task);
        let listing = provider.list_directory(".").await.unwrap();
        assert_eq!(listing.len(), 1);
        assert!(listing[0].is_symlink, "{listing:?}");

        let result = provider.read_file("link.txt").await;
        assert!(result.is_err(), "reading through an escaping symlink must be blocked, got {result:?}");
    })
    .await;

    let _ = session.execute_command(&format!("rm -rf {root} {outside}")).await;
    outcome.expect("the symlink-escape test panicked (see the captured message above)");
}
