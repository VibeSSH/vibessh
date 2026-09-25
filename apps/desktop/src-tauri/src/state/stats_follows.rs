use std::collections::{HashMap, HashSet};

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::ssh::client::FollowHandle;

/// Holds the live `docker stats` stream behind each open Application page,
/// **one per Application**.
///
/// Its own registry, not a slot in `LogFollowManager`. That one is keyed by
/// Application too, and "replace whatever was here" is its whole point - so
/// sharing it would have made opening the stats stream end the console's log
/// stream, and the other way round.
///
/// Keyed by Application for the reason `LogFollowManager` spells out: every
/// stranded follow holds one of the ten sessions `sshd` allows per
/// connection, and a registry that never replaces anything leaks them until
/// every SSH operation on the Node fails. Opening a stream here ends
/// whichever one the same Application had before.
///
/// Stopping names the stream as well as the Application. A page that
/// unmounts and remounts at once - StrictMode, a quick tab switch - starts
/// its new stream and stops its old one concurrently, and a stop keyed on the
/// Application alone could land after the start and end the stream that was
/// meant to survive.
///
/// A stop can also arrive before its stream has finished opening - the page
/// left while the channel was still being set up. That stop is remembered,
/// and the stream is closed the moment it registers, instead of being kept
/// by a registry nobody will ask to stop it again.
#[derive(Default)]
pub struct StatsFollowManager {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    follows: HashMap<Uuid, (String, FollowHandle)>,
    stopped_before_start: HashSet<String>,
}

/// More early stops than any real use produces; past it the oldest is not
/// worth tracking - the set only guards a race measured in milliseconds.
const EARLY_STOP_CAP: usize = 64;

impl StatsFollowManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers this Application's stream. The one it replaced, if any, is
    /// dropped here - for an SSH follow that is what closes its channel. A
    /// stream whose stop already came in is dropped instead of registered.
    pub async fn replace(&self, application_id: Uuid, stream_id: String, follow: FollowHandle) {
        let mut inner = self.inner.lock().await;
        if inner.stopped_before_start.remove(&stream_id) {
            drop(follow);
            return;
        }
        inner.follows.insert(application_id, (stream_id, follow));
    }

    /// Ends this Application's stream if it is still `stream_id`. `false`
    /// means it was not running (yet) or has since been replaced - a race,
    /// not an error.
    pub async fn stop(&self, application_id: Uuid, stream_id: &str) -> bool {
        let mut inner = self.inner.lock().await;
        match inner.follows.get(&application_id) {
            Some((current, _)) if current == stream_id => inner.follows.remove(&application_id).is_some(),
            _ => {
                if inner.stopped_before_start.len() >= EARLY_STOP_CAP {
                    inner.stopped_before_start.clear();
                }
                inner.stopped_before_start.insert(stream_id.to_string());
                false
            }
        }
    }
}
