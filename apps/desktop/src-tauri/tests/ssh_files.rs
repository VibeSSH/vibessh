//! Drives `SshSession::list_directory`/`read_file`/`write_file` against a
//! real, local `russh::server` running the real `russh_sftp` server-side
//! protocol handling (backed by an in-memory "filesystem" for this test) -
//! proves the SFTP subsystem negotiation and the client-side calls in
//! `ssh/sftp.rs` actually round-trip, not just that they compile.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Handler as ServerHandler, Msg, Server as _, Session};
use russh::{Channel, ChannelId, Preferred};
use russh_sftp::protocol::{
    Data, ExtendedReply, File, FileAttributes, Handle, Name, OpenFlags, Packet, Status, StatusCode,
    Version,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex as TokioMutex;
use tokio::time::timeout;

use vibessh_lib::ssh::{connect, SshAuth, SshCredentials};

const TEST_USER: &str = "tester";
const TEST_PASSWORD: &str = "correct-horse-battery-staple";

#[derive(Default, Clone)]
struct InMemoryFs {
    files: Arc<TokioMutex<HashMap<String, Vec<u8>>>>,
    dirs: Arc<TokioMutex<std::collections::HashSet<String>>>,
}

#[derive(Clone)]
struct MockServer {
    fs: InMemoryFs,
    /// How many handles this server admits to allowing, or nothing to stay
    /// quiet about it - see `refuses_nothing_after_many_transfers`.
    handle_limit: Option<u64>,
}

impl russh::server::Server for MockServer {
    type Handler = MockSshHandler;
    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> MockSshHandler {
        MockSshHandler {
            fs: self.fs.clone(),
            handle_limit: self.handle_limit,
            pending_channel: None,
        }
    }
}

struct MockSshHandler {
    fs: InMemoryFs,
    handle_limit: Option<u64>,
    pending_channel: Option<Channel<Msg>>,
}

impl ServerHandler for MockSshHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == TEST_USER && password == TEST_PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.pending_channel = Some(channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(&mut self, channel_id: ChannelId, name: &str, session: &mut Session) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel_id)?;
            return Ok(());
        }
        let Some(channel) = self.pending_channel.take() else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };
        session.channel_success(channel_id)?;
        let handler = MockSftpHandler::new(self.fs.clone(), self.handle_limit);
        russh_sftp::server::run(channel.into_stream(), handler).await;
        Ok(())
    }
}

struct MockSftpHandler {
    fs: InMemoryFs,
    handle_limit: Option<u64>,
    open_files: HashMap<String, String>,
    open_dirs: HashMap<String, (String, bool)>,
    next_handle: u64,
}

impl MockSftpHandler {
    fn new(fs: InMemoryFs, handle_limit: Option<u64>) -> Self {
        Self {
            fs,
            handle_limit,
            open_files: HashMap::new(),
            open_dirs: HashMap::new(),
            next_handle: 0,
        }
    }

    fn alloc_handle(&mut self) -> String {
        self.next_handle += 1;
        format!("h{}", self.next_handle)
    }
}

impl russh_sftp::server::Handler for MockSftpHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        let mut version = Version::new();

        // A real OpenSSH server announces `limits@openssh.com` and the client
        // then holds itself to those numbers. Announced only when a test asks
        // for it, so the other tests keep negotiating exactly what they did
        // before this one existed.
        if self.handle_limit.is_some() {
            version
                .extensions
                .insert(russh_sftp::extensions::LIMITS.to_string(), "1".to_string());
        }

        Ok(version)
    }

    async fn extended(&mut self, id: u32, request: String, _data: Vec<u8>) -> Result<Packet, Self::Error> {
        let Some(handles) = self.handle_limit else {
            return Err(StatusCode::OpUnsupported);
        };

        if request != russh_sftp::extensions::LIMITS {
            return Err(StatusCode::OpUnsupported);
        }

        let limits = russh_sftp::extensions::LimitsExtension {
            max_packet_len: 32 * 1024,
            max_read_len: 32 * 1024,
            max_write_len: 32 * 1024,
            max_open_handles: handles,
        };

        Ok(Packet::ExtendedReply(ExtendedReply {
            id,
            data: russh_sftp::ser::to_bytes(&limits)
                .map_err(|_| StatusCode::Failure)?
                .to_vec(),
        }))
    }

    async fn open(&mut self, id: u32, filename: String, pflags: OpenFlags, _attrs: FileAttributes) -> Result<Handle, Self::Error> {
        {
            let mut files = self.fs.files.lock().await;
            if pflags.contains(OpenFlags::CREATE) {
                let entry = files.entry(filename.clone()).or_default();
                if pflags.contains(OpenFlags::TRUNCATE) {
                    entry.clear();
                }
            } else if !files.contains_key(&filename) {
                return Err(StatusCode::NoSuchFile);
            }
        }
        let handle = self.alloc_handle();
        self.open_files.insert(handle.clone(), filename);
        Ok(Handle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.open_files.remove(&handle);
        self.open_dirs.remove(&handle);
        Ok(ok_status(id))
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let path = self.open_files.get(&handle).cloned().ok_or(StatusCode::Failure)?;
        let files = self.fs.files.lock().await;
        let content = files.get(&path).ok_or(StatusCode::NoSuchFile)?;
        let offset = offset as usize;
        if offset >= content.len() {
            return Err(StatusCode::Eof);
        }
        let end = (offset + len as usize).min(content.len());
        Ok(Data {
            id,
            data: content[offset..end].to_vec(),
        })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let path = self.open_files.get(&handle).cloned().ok_or(StatusCode::Failure)?;
        let mut files = self.fs.files.lock().await;
        let content = files.entry(path).or_default();
        let offset = offset as usize;
        if content.len() < offset + data.len() {
            content.resize(offset + data.len(), 0);
        }
        content[offset..offset + data.len()].copy_from_slice(&data);
        Ok(ok_status(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let handle = self.alloc_handle();
        self.open_dirs.insert(handle.clone(), (path, false));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let (path, exhausted) = self.open_dirs.get(&handle).cloned().ok_or(StatusCode::Failure)?;
        if exhausted {
            return Err(StatusCode::Eof);
        }
        self.open_dirs.insert(handle, (path.clone(), true));

        let prefix = if path.ends_with('/') { path } else { format!("{path}/") };
        let files = self.fs.files.lock().await;
        let dirs = self.fs.dirs.lock().await;
        let mut entries: Vec<File> = files
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix) && !p[prefix.len()..].contains('/'))
            .map(|(p, content)| {
                let name = p[prefix.len()..].to_string();
                let mut attrs = FileAttributes::empty();
                attrs.size = Some(content.len() as u64);
                attrs.permissions = Some(0o100644); // regular file
                File::new(name, attrs)
            })
            .collect();
        entries.extend(dirs.iter().filter(|p| p.starts_with(&prefix) && !p[prefix.len()..].contains('/')).map(|p| {
            let name = p[prefix.len()..].to_string();
            let mut attrs = FileAttributes::empty();
            attrs.permissions = Some(0o040755); // directory
            File::new(name, attrs)
        }));
        Ok(Name { id, files: entries })
    }

    async fn mkdir(&mut self, id: u32, path: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
        self.fs.dirs.lock().await.insert(path);
        Ok(ok_status(id))
    }
}

fn ok_status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_string(),
        language_tag: "en".to_string(),
    }
}

async fn spawn_mock_server(fs: InMemoryFs) -> u16 {
    spawn_mock_server_with_limit(fs, None).await
}

async fn spawn_mock_server_with_limit(fs: InMemoryFs, handle_limit: Option<u64>) -> u16 {
    let config = Arc::new(russh::server::Config {
        keys: vec![PrivateKey::random(&mut rand010::rng(), Algorithm::Ed25519).unwrap()],
        preferred: Preferred::default(),
        ..Default::default()
    });
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();

    tokio::spawn(async move {
        let mut server = MockServer { fs, handle_limit };
        let _ = server.run_on_socket(config, &socket).await;
    });

    port
}

fn credentials(port: u16) -> SshCredentials {
    SshCredentials {
        host: "127.0.0.1".to_string(),
        port,
        username: TEST_USER.to_string(),
        auth: SshAuth::Password(TEST_PASSWORD.to_string()),
    }
}

#[tokio::test]
async fn writes_a_new_file_reads_it_back_and_lists_it_in_the_directory() {
    let fs = InMemoryFs::default();
    let port = spawn_mock_server(fs).await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    outcome
        .session
        .write_file("/uploads/notes.txt", b"hello from vibessh")
        .await
        .expect("writing a brand new file should succeed (create semantics)");

    let contents = outcome
        .session
        .read_file("/uploads/notes.txt")
        .await
        .expect("reading it back should succeed");
    assert_eq!(contents, b"hello from vibessh");

    let entries = outcome
        .session
        .list_directory("/uploads")
        .await
        .expect("listing the directory should succeed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "notes.txt");
    assert!(!entries[0].is_dir);
    assert_eq!(entries[0].size, "hello from vibessh".len() as u64);

    // Overwrite (truncate) semantics - saving shorter content shouldn't
    // leave stale bytes from the longer original past the new length.
    outcome
        .session
        .write_file("/uploads/notes.txt", b"short")
        .await
        .expect("overwriting an existing file should succeed");
    let contents = outcome.session.read_file("/uploads/notes.txt").await.unwrap();
    assert_eq!(contents, b"short");

    outcome.session.close().await;
}

#[tokio::test]
async fn downloads_and_uploads_stream_through_a_real_local_file_not_just_memory() {
    let fs = InMemoryFs::default();
    let port = spawn_mock_server(fs).await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    outcome
        .session
        .write_file("/uploads/report.csv", b"id,name\n1,alpha\n2,beta\n")
        .await
        .expect("seeding the remote file should succeed");

    let local_dir = std::env::temp_dir().join(format!("vibessh-sftp-test-{}-{}", std::process::id(), unique_suffix()));
    std::fs::create_dir_all(&local_dir).expect("create scratch dir");
    let download_target = local_dir.join("downloaded.csv");

    outcome
        .session
        .download_file("/uploads/report.csv", &download_target)
        .await
        .expect("download_file should succeed");
    let downloaded = std::fs::read(&download_target).expect("downloaded file should exist locally");
    assert_eq!(downloaded, b"id,name\n1,alpha\n2,beta\n");

    // Round trip: upload the file we just downloaded to a new remote path
    // and read it back over SFTP, proving upload_file's local-file-to-SFTP
    // stream works too, not just the download direction.
    outcome
        .session
        .upload_file(&download_target, "/uploads/report-copy.csv")
        .await
        .expect("upload_file should succeed");
    let reuploaded = outcome
        .session
        .read_file("/uploads/report-copy.csv")
        .await
        .expect("reading the reuploaded file back should succeed");
    assert_eq!(reuploaded, b"id,name\n1,alpha\n2,beta\n");

    std::fs::remove_dir_all(&local_dir).ok();
    outcome.session.close().await;
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after the epoch")
        .as_nanos()
}

#[tokio::test]
async fn creates_a_directory_and_lists_it_alongside_files() {
    let fs = InMemoryFs::default();
    let port = spawn_mock_server(fs).await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    outcome
        .session
        .write_file("/uploads/notes.txt", b"hello")
        .await
        .expect("seeding a file should succeed");
    outcome
        .session
        .create_directory("/uploads/backups")
        .await
        .expect("create_directory should succeed");

    let entries = outcome
        .session
        .list_directory("/uploads")
        .await
        .expect("listing the directory should succeed");
    assert_eq!(entries.len(), 2);
    let dir_entry = entries.iter().find(|e| e.name == "backups").expect("new directory should be listed");
    assert!(dir_entry.is_dir);
    let file_entry = entries.iter().find(|e| e.name == "notes.txt").expect("existing file should still be listed");
    assert!(!file_entry.is_dir);

    outcome.session.close().await;
}

#[tokio::test]
async fn reading_a_missing_file_is_a_clean_error_not_a_hang_or_panic() {
    let fs = InMemoryFs::default();
    let port = spawn_mock_server(fs).await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    let result = timeout(Duration::from_secs(5), outcome.session.read_file("/does/not/exist.txt"))
        .await
        .expect("timed out - a missing file must produce an error promptly, not hang");
    assert!(result.is_err());

    outcome.session.close().await;
}

/// The regression behind "Limit exceeded: handle limit reached" in the file
/// manager: nothing was open, and uploads still started failing.
///
/// `russh_sftp` keeps its own tally of open handles and checks it against the
/// server's advertised ceiling before every open. The tally drops only on a
/// close that is awaited; `File`'s `Drop` fires the close off without waiting,
/// which frees it on the server and leaves it counted here forever. With a
/// ceiling of two, the third transfer of any kind used to be refused by the
/// client before a single packet went out.
///
/// Two handles, ten transfers - four times over the old ceiling, in every
/// direction the module offers.
///
/// The library bug behind it: <https://github.com/AspectUnk/russh-sftp/issues/98>.
/// This test should keep passing after it is fixed upstream - it asserts the
/// behaviour, not the workaround.
#[tokio::test]
async fn keeps_transferring_after_more_files_than_the_handle_limit() {
    let fs = InMemoryFs::default();
    let port = spawn_mock_server_with_limit(fs, Some(2)).await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    let scratch = std::env::temp_dir().join(format!(
        "vibessh-sftp-handles-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");

    for round in 0..10 {
        let remote = format!("/uploads/round-{round}.txt");
        let body = format!("round {round}");

        outcome
            .session
            .write_file(&remote, body.as_bytes())
            .await
            .unwrap_or_else(|err| panic!("write {round} should succeed: {err}"));

        let read = outcome
            .session
            .read_file(&remote)
            .await
            .unwrap_or_else(|err| panic!("read {round} should succeed: {err}"));
        assert_eq!(read, body.as_bytes());

        let local = scratch.join(format!("round-{round}.txt"));
        outcome
            .session
            .download_file(&remote, &local)
            .await
            .unwrap_or_else(|err| panic!("download {round} should succeed: {err}"));

        outcome
            .session
            .upload_file(&local, &format!("/uploads/copy-{round}.txt"))
            .await
            .unwrap_or_else(|err| panic!("upload {round} should succeed: {err}"));

        let mut moved = 0u64;
        outcome
            .session
            .upload_file_with_progress(&local, &format!("/uploads/watched-{round}.txt"), &mut |bytes| {
                moved += bytes;
            })
            .await
            .unwrap_or_else(|err| panic!("watched upload {round} should succeed: {err}"));
        assert_eq!(moved, body.len() as u64);
    }

    std::fs::remove_dir_all(&scratch).ok();
    outcome.session.close().await;
}

/// The same leak on the path that is easy to miss: the remote file opens, and
/// then creating the *local* one fails.
///
/// That failure is ordinary - a download directory that is not there, a full
/// disk, a name Windows will not take, a file held open by something else -
/// and the early return used to drop the remote handle rather than close it,
/// which is exactly the bug the test above covers, one line further down.
///
/// A ceiling of two and three failed downloads: without the close, the third
/// never reaches the local filesystem at all, because the client refuses to
/// open the remote file first.
#[tokio::test]
async fn a_download_that_cannot_write_locally_still_gives_the_handle_back() {
    let fs = InMemoryFs::default();
    let port = spawn_mock_server_with_limit(fs, Some(2)).await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    outcome
        .session
        .write_file("/uploads/source.txt", b"body")
        .await
        .expect("writing the source should succeed");

    // A directory that does not exist, so `LocalFile::create` fails every
    // time and always for the same reason.
    let impossible = std::env::temp_dir()
        .join(format!("vibessh-sftp-missing-{}-{}", std::process::id(), unique_suffix()))
        .join("nowhere")
        .join("file.txt");

    for round in 0..3 {
        let error = outcome
            .session
            .download_file("/uploads/source.txt", &impossible)
            .await
            .expect_err("a download into a directory that is not there cannot succeed");
        // The failure has to be the local one every time. Once the handles
        // leak, the client refuses the *remote* open instead and the message
        // changes - which is the regression, dressed as the same failure.
        let message = error.to_string();
        assert!(
            message.contains("couldn't create"),
            "round {round} failed for the wrong reason: {message}"
        );
    }

    // And the session is still usable afterwards.
    let scratch = std::env::temp_dir().join(format!(
        "vibessh-sftp-recover-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let local = scratch.join("source.txt");
    outcome
        .session
        .download_file("/uploads/source.txt", &local)
        .await
        .expect("a download should still work after three local failures");
    assert_eq!(std::fs::read(&local).expect("read the downloaded file"), b"body");

    std::fs::remove_dir_all(&scratch).ok();
    outcome.session.close().await;
}
