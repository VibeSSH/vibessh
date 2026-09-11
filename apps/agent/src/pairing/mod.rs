//! Pairing (Etap E): a one-time code registered locally via `vibe-agent
//! pair <code>` (see `crate::transport`'s `/internal/pair` route, loopback
//! only) is consumed by the first WebSocket handshake that presents it,
//! which then gets a durable credential in return. Only that credential's
//! hash is ever persisted - see `credential`.

mod credential;

pub use credential::{has_paired_credential, issue_credential, verify_credential};

use std::sync::Mutex;

use chrono::{DateTime, Utc};

/// A pending code survives at most this many wrong guesses before it's
/// burned, even if it hasn't expired yet - a cheap floor under brute-force
/// attempts from a local process that skips normal network latency.
const MAX_FAILED_ATTEMPTS: u32 = 10;

struct PendingCode {
    code: String,
    expires_at: DateTime<Utc>,
    failed_attempts: u32,
}

#[derive(Default)]
struct RegistryState {
    pending: Option<PendingCode>,
}

/// Cheap to clone (an `Arc` under the hood) so both the control HTTP
/// handler (`register`) and the WebSocket handshake (`try_consume`) can
/// hold their own copy of the same underlying state.
#[derive(Clone, Default)]
pub struct PairingRegistry(std::sync::Arc<Mutex<RegistryState>>);

impl PairingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a freshly-generated code, replacing any still-pending one
    /// (only one pairing can be in flight at a time - simple, and matches
    /// the one-agent-being-paired-to-one-desktop flow this is built for).
    pub fn register(&self, code: String, ttl: chrono::Duration) {
        let mut state = self.0.lock().expect("pairing registry mutex poisoned");
        state.pending = Some(PendingCode {
            code,
            expires_at: Utc::now() + ttl,
            failed_attempts: 0,
        });
    }

    /// Checks `candidate` against the pending code. A match burns the code
    /// (single use) and returns `true`. A miss counts against the attempt
    /// budget and returns `false`; once the budget or the TTL runs out the
    /// pending code is cleared so a stale/attacked code can't linger.
    pub fn try_consume(&self, candidate: &str) -> bool {
        let mut state = self.0.lock().expect("pairing registry mutex poisoned");

        let Some(pending) = state.pending.as_mut() else {
            return false;
        };

        if Utc::now() > pending.expires_at {
            state.pending = None;
            return false;
        }

        // Constant-time, matching how the durable credential is already
        // compared one module over. The burn limit below caps guessing at
        // ten attempts, so a timing oracle is not the primary risk here -
        // but comparing a secret with `==` when the codebase already has a
        // correct helper is not a trade worth making, and the leak (how
        // many leading characters matched) is exactly what would make those
        // ten attempts enough.
        if credential::constant_time_eq(pending.code.as_bytes(), candidate.as_bytes()) {
            state.pending = None;
            return true;
        }

        pending.failed_attempts += 1;
        if pending.failed_attempts >= MAX_FAILED_ATTEMPTS {
            log::warn!("agent: pairing code burned after {MAX_FAILED_ATTEMPTS} failed attempts");
            state.pending = None;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_code_is_consumed_exactly_once() {
        let registry = PairingRegistry::new();
        registry.register("VIBE-TEST-CODE".into(), chrono::Duration::minutes(5));

        assert!(registry.try_consume("VIBE-TEST-CODE"));
        assert!(!registry.try_consume("VIBE-TEST-CODE"));
    }

    #[test]
    fn expired_code_is_rejected() {
        let registry = PairingRegistry::new();
        registry.register("VIBE-TEST-CODE".into(), chrono::Duration::seconds(-1));

        assert!(!registry.try_consume("VIBE-TEST-CODE"));
    }

    #[test]
    fn code_is_burned_after_too_many_wrong_guesses() {
        let registry = PairingRegistry::new();
        registry.register("VIBE-TEST-CODE".into(), chrono::Duration::minutes(5));

        for _ in 0..MAX_FAILED_ATTEMPTS {
            assert!(!registry.try_consume("wrong-guess"));
        }
        // Even the right code no longer works - it was burned by the attempts above.
        assert!(!registry.try_consume("VIBE-TEST-CODE"));
    }
}
