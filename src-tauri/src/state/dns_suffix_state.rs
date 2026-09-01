//! In-memory copy of the Vibe Network's DNS suffix - loaded once from
//! `storage::dns_config` at startup, kept live here so `services::dns_service`
//! never has to re-read the config file on every single alias/Node hostname
//! computation. A plain `std::sync::RwLock`, not `tokio::sync::Mutex` like
//! `state::BackupDestinationState` - `services::dns_service`'s own
//! hostname-computing functions (`normalize_alias`, `node_alias`,
//! `resolve_dns_view`) are deliberately synchronous/pure (so they stay unit
//! testable without a runtime), and a sync `RwLock` read is what lets a
//! synchronous function call `.get()` without needing to become `async`
//! just to learn the current suffix.

use std::sync::RwLock;

pub struct DnsSuffixState {
    inner: RwLock<String>,
}

impl DnsSuffixState {
    pub fn new(suffix: String) -> Self {
        Self { inner: RwLock::new(suffix) }
    }

    pub fn get(&self) -> String {
        self.inner.read().expect("DNS suffix lock poisoned").clone()
    }

    pub fn set(&self, suffix: String) {
        *self.inner.write().expect("DNS suffix lock poisoned") = suffix;
    }
}
