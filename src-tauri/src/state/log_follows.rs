use std::collections::HashMap;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::ssh::client::FollowHandle;

/// One live console stream, however it is being carried.
///
/// The two transports stop in genuinely different ways, which is why this
/// is an enum rather than a trait object: an SSH follow ends by dropping
/// its handle (that closes the channel), and an Agent follow ends by
/// telling the Agent to kill the process it started. Neither can be
/// expressed as the other.
pub enum LogFollow {
    /// `docker logs -f` over its own SSH channel. Dropping the handle
    /// closes the channel, which is what ends the remote command.
    Ssh(FollowHandle),
    /// A follow the Agent is running. Stopping needs a message, so the id
    /// of the Node to send it to is kept here.
    Agent { server_id: Uuid, follow_id: Uuid },
}

/// Holds the live stream behind each open console.
///
/// Unlike `FileTransferManager` and `AiTurnManager`, which abort a local
/// task, a forgotten entry here is a command left running on somebody's
/// server, writing into a socket nobody reads. Every path out of the
/// console has to reach `stop`.
#[derive(Default)]
pub struct LogFollowManager {
    follows: Mutex<HashMap<String, LogFollow>>,
}

impl LogFollowManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new stream, replacing (and so stopping) any previous one
    /// under the same id.
    ///
    /// Replacing rather than rejecting matters: a console that reconnects -
    /// the tab was closed and reopened, or the page remounted - would
    /// otherwise leave the first follow running forever, and the second
    /// stream would deliver every line twice.
    pub async fn insert(&self, follow_id: String, follow: LogFollow) -> Option<LogFollow> {
        self.follows.lock().await.insert(follow_id, follow)
    }

    /// Takes a stream out of the registry so the caller can end it.
    ///
    /// Returns it rather than dropping it here, because the Agent variant
    /// cannot be stopped by a drop - it needs a message sent, and sending
    /// it is the command layer's job.
    pub async fn take(&self, follow_id: &str) -> Option<LogFollow> {
        self.follows.lock().await.remove(follow_id)
    }
}
