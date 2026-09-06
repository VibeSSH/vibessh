//! Typing into a container running on this machine.
//!
//! **Why this is not the SSH mechanism.** Over SSH the console is a named
//! pipe: a background `docker attach` reads from a FIFO, and each keystroke
//! is a fresh `printf ... | tee` into it. That shape exists because an SSH
//! exec channel cannot be held open for the console's whole life, so the
//! *pipe* is what persists rather than the connection.
//!
//! None of that constraint applies here, and the pieces it is built from do
//! not exist on Windows - `mkfifo` has no counterpart. So the local console
//! keeps what the remote one could not: one long-lived `docker attach` per
//! container, with its stdin held open, exactly the way
//! `LocalProcessManager` holds a launched process's.
//!
//! One attach per container, kept in a process-wide map, because a console is
//! created afresh every time somebody opens the tab and a new `docker attach`
//! for each of those would leave a pile of them feeding the same stdin.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

type Attachments = Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>;

fn attachments() -> &'static Attachments {
    static ATTACHMENTS: OnceLock<Attachments> = OnceLock::new();
    ATTACHMENTS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// A writer into one container's stdin, starting the `docker attach` behind
/// it if this is the first time it has been asked for - or if the previous
/// one has since exited, which is what happens whenever the container is
/// restarted.
pub async fn attach(application_id: Uuid, container: &str) -> AppResult<mpsc::UnboundedSender<Vec<u8>>> {
    let mut map = attachments().lock().await;

    if let Some(existing) = map.get(&application_id) {
        // A closed channel means the task behind it is gone - the container
        // was restarted, or docker exited. Falls through to a fresh attach
        // rather than handing back a sender whose writes go nowhere.
        if !existing.is_closed() {
            return Ok(existing.clone());
        }
    }

    let mut command = tokio::process::Command::new("docker");
    command
        // Without this, Ctrl-C in whatever started VibeSSH would be forwarded
        // into the container - the console is for typing commands, not for
        // sharing this process's signals.
        .args(["attach", "--sig-proxy=false", container])
        .stdin(std::process::Stdio::piped())
        // The Logs tab reads `docker logs`, so anything this prints is a
        // duplicate of what is already shown there.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(false);

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            super::docker_command::docker_missing()
        } else {
            AppError::Internal(format!("couldn't attach to the container: {err}"))
        }
    })?;

    let mut stdin = child.stdin.take().ok_or_else(|| AppError::Internal("docker attach gave no stdin".into()))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            // A failed write means the container is gone. Ending the loop
            // drops the receiver, which closes the sender, which is how the
            // next `attach` learns to start a new one.
            if stdin.write_all(&data).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
        // Reap it rather than leaving a zombie holding the container's stdin.
        let _ = child.kill().await;
    });

    map.insert(application_id, tx.clone());
    Ok(tx)
}

/// Forgets an Application's attachment, ending the process behind it.
pub async fn detach(application_id: Uuid) {
    attachments().lock().await.remove(&application_id);
}
