//! Live server metrics pushed to the interface, rather than pulled on a timer.
//!
//! The Dashboard used to poll `get_server_metrics` every few seconds, opening
//! a fresh SSH channel each tick from the frontend. This keeps one background
//! task per watched server that samples on the server's existing session and
//! emits the reading as a `metrics://<id>` event, so the interface only
//! listens. The CPU figure is a delta between consecutive `/proc/stat` reads
//! (see `ssh::monitor`), so sampling on a short interval here is exactly what
//! makes the number both live and meaningful.
//!
//! A stream runs only while something is subscribed: the Dashboard starts one
//! per SSH-mode server when it opens and stops them when it closes or the
//! window is hidden, so nothing samples a server nobody is looking at.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tauri::async_runtime::{spawn, JoinHandle};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;

/// How often a stream samples. Each sample is one `get_metrics` round trip on
/// the server's existing SSH session, and the window the CPU delta is averaged
/// over - short enough to feel live, long enough to be a stable reading and to
/// keep the channel churn modest.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(1500);

/// One background stream per subscribed server. Starting is idempotent;
/// stopping aborts the task and drops it.
#[derive(Default)]
pub struct MetricsStreamManager {
    streams: Mutex<HashMap<Uuid, JoinHandle<()>>>,
}

impl MetricsStreamManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begins streaming `metrics://<server_id>` if it is not already. Holds an
    /// `AppHandle` rather than the state references so the task outlives the
    /// command that started it and reaches the managed state each sample.
    pub fn start(&self, app: AppHandle, server_id: Uuid) {
        let mut streams = self.streams.lock().expect("metrics stream lock");
        if streams.contains_key(&server_id) {
            return;
        }
        let handle = spawn(async move {
            let event = format!("metrics://{server_id}");
            loop {
                let reading = services::get_server_metrics(
                    &app.state::<ServerRepository>(),
                    &app.state::<SshSessionManager>(),
                    server_id,
                )
                .await;
                // A failed sample is skipped, not emitted: the interface keeps
                // its last reading and its "updated N s ago" label climbs,
                // which is the honest signal that the stream has gone quiet.
                if let Ok(metrics) = reading {
                    let _ = app.emit(&event, metrics);
                }
                tokio::time::sleep(SAMPLE_INTERVAL).await;
            }
        });
        streams.insert(server_id, handle);
    }

    /// Stops the stream for one server, if any is running.
    pub fn stop(&self, server_id: Uuid) {
        if let Some(handle) = self.streams.lock().expect("metrics stream lock").remove(&server_id) {
            handle.abort();
        }
    }
}
