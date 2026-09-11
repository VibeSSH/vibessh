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

/// Holds the live stream behind each open console, **one per Application**.
///
/// **Keyed by Application, not by follow.** An earlier version keyed on the
/// follow id, which the frontend generates fresh every time it opens a
/// stream - so "replace whatever was here" could never fire, because the
/// key was new every time. Nothing ever replaced anything, and every
/// abandoned follow stayed in the map with its channel open: React's
/// StrictMode double-mount left one behind on every mount, and every
/// reconnect left another.
///
/// That is not a slow leak. `sshd` allows ten sessions per connection by
/// default, and each stranded follow holds one - so after a few minutes of
/// opening a console, *every* SSH operation on that Node fails with
/// `ConnectFailed`, including the ones that have nothing to do with logs.
///
/// One Application can only be looked at through one console at a time, so
/// keying on it makes the replacement real: opening a stream ends whatever
/// the last one was, whether or not anybody remembered to stop it.
#[derive(Default)]
pub struct LogFollowManager {
    follows: Mutex<HashMap<Uuid, LogFollow>>,
}

impl LogFollowManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers this Application's stream and hands back the one it
    /// replaced, if any.
    ///
    /// The caller has to deal with what comes back: dropping it is enough
    /// for an SSH follow, and an Agent follow needs a message. Returning it
    /// rather than dropping it here is what makes that impossible to
    /// forget - the value is `#[must_use]`-shaped by being the only way to
    /// learn there was a previous one at all.
    pub async fn replace(&self, application_id: Uuid, follow: LogFollow) -> Option<LogFollow> {
        self.follows.lock().await.insert(application_id, follow)
    }

    /// Takes this Application's stream out of the registry so the caller
    /// can end it. `None` means there was nothing running, which is a race
    /// - a console closing twice - rather than an error.
    pub async fn take(&self, application_id: Uuid) -> Option<LogFollow> {
        self.follows.lock().await.remove(&application_id)
    }
}
