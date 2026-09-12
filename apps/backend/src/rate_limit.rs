//! How often one caller may try to authenticate.
//!
//! **Why this is needed at all.** Passwords are hashed with Argon2id, which
//! is deliberately expensive - that is what makes a stolen hash hard to
//! crack. It also makes an unauthenticated endpoint that runs it an
//! expensive thing to call: a few hundred concurrent sign-in attempts with
//! any password at all will saturate the CPU of a small VPS, and the cost is
//! paid before the credentials are found to be wrong. Alongside that, an
//! unlimited `/auth/login` is an open invitation to credential stuffing,
//! where the attacker already has the passwords and only needs somewhere to
//! try them.
//!
//! **Two counters, because they catch different attacks.** A per-account
//! limit stops someone working through a list of passwords against one
//! email. A per-address limit stops them working through a list of emails
//! with one password, which no per-account counter would ever notice.
//!
//! **In memory, and honest about it.** The window lives in this process, so
//! it resets on restart and is not shared between instances. That is the
//! right trade for a deployment that runs one instance: a database round
//! trip per attempt would add the very cost this is meant to contain, and a
//! limiter that forgets on restart still removes the sustained attack, which
//! is the one that matters. If this ever runs behind a load balancer across
//! several instances, this is the piece that has to move.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Attempts allowed within `WINDOW` for one key.
const MAX_ATTEMPTS: usize = 10;
const WINDOW: Duration = Duration::from_secs(5 * 60);

/// Stops the map growing without bound when an attacker varies the key -
/// which is exactly what spraying across email addresses does. Well above
/// any real deployment's number of distinct recent callers.
const MAX_TRACKED_KEYS: usize = 50_000;

#[derive(Debug, Default)]
pub struct RateLimiter {
    attempts: Mutex<HashMap<String, Vec<Instant>>>,
}

/// Whether the caller may proceed, and how long until they may again.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Seconds the caller should wait. Reported to them, because a limit
    /// somebody cannot see the shape of is indistinguishable from a fault.
    RetryAfter(u64),
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an attempt against `key` and says whether it is allowed.
    ///
    /// Counted on every call rather than only on failures. Counting failures
    /// alone lets an attacker who lands one correct password carry on
    /// unmetered, and the expense being contained here - the Argon2
    /// verification - is paid whether or not the password turns out to be
    /// right.
    pub fn check(&self, key: &str) -> Decision {
        let now = Instant::now();
        let mut attempts = match self.attempts.lock() {
            Ok(guard) => guard,
            // A poisoned mutex means another thread panicked while holding
            // it. Refusing to authenticate anyone for the life of the
            // process would be a worse outcome than an unmetered window, so
            // this recovers rather than propagating.
            Err(poisoned) => poisoned.into_inner(),
        };

        if attempts.len() > MAX_TRACKED_KEYS {
            attempts.retain(|_, seen| seen.iter().any(|at| now.duration_since(*at) < WINDOW));
        }

        let seen = attempts.entry(key.to_string()).or_default();
        seen.retain(|at| now.duration_since(*at) < WINDOW);

        if seen.len() >= MAX_ATTEMPTS {
            let oldest = seen.first().copied().unwrap_or(now);
            let elapsed = now.duration_since(oldest);
            let remaining = WINDOW.saturating_sub(elapsed);
            return Decision::RetryAfter(remaining.as_secs().max(1));
        }

        seen.push(now);
        Decision::Allow
    }
}

/// The key a per-address limit counts against.
///
/// **Why the socket address is not simply used.** In the hosted deployment
/// the service sits behind a tunnel, so every request arrives from
/// `127.0.0.1` and a per-address limit would be one global limit - locking
/// every user out the moment one attacker started. The real address is in a
/// forwarded header there.
///
/// **Why that header is not simply trusted.** It is set by the client, so a
/// service reachable directly lets an attacker vary it per request and evade
/// the limit entirely. Trusting it is therefore a deployment decision, not a
/// default: `TRUST_FORWARDED_FOR` says the operator has put something in
/// front that overwrites the header. The per-account limit does not depend
/// on any of this and holds either way.
pub fn client_key(forwarded_for: Option<&str>, peer: Option<IpAddr>) -> String {
    let trust_header = std::env::var("TRUST_FORWARDED_FOR").map(|value| value == "1" || value.eq_ignore_ascii_case("true")).unwrap_or(false);
    if trust_header {
        if let Some(value) = forwarded_for {
            // The leftmost entry is the original client; the rest were added
            // by each proxy in turn.
            if let Some(first) = value.split(',').next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return format!("ip:{trimmed}");
                }
            }
        }
    }
    match peer {
        Some(address) => format!("ip:{address}"),
        None => "ip:unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_refuses() {
        let limiter = RateLimiter::new();
        for attempt in 0..MAX_ATTEMPTS {
            assert_eq!(limiter.check("account:someone@example.com"), Decision::Allow, "attempt {attempt} should be allowed");
        }
        assert!(matches!(limiter.check("account:someone@example.com"), Decision::RetryAfter(_)));
    }

    /// The per-account and per-address counters must not be the same
    /// counter: one account being attacked would otherwise lock out every
    /// other caller from that address, and vice versa.
    #[test]
    fn separate_keys_are_counted_separately() {
        let limiter = RateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            limiter.check("account:someone@example.com");
        }
        assert_eq!(limiter.check("account:another@example.com"), Decision::Allow);
        assert_eq!(limiter.check("ip:198.51.100.7"), Decision::Allow);
    }

    #[test]
    fn the_forwarded_header_is_ignored_unless_the_deployment_says_otherwise() {
        std::env::remove_var("TRUST_FORWARDED_FOR");
        let key = client_key(Some("203.0.113.9"), Some("127.0.0.1".parse().unwrap()));
        assert_eq!(key, "ip:127.0.0.1", "a client-set header must not decide the limit by default");
    }

    #[test]
    fn the_forwarded_header_is_used_when_the_deployment_says_so() {
        std::env::set_var("TRUST_FORWARDED_FOR", "1");
        let key = client_key(Some("203.0.113.9, 10.0.0.1"), Some("127.0.0.1".parse().unwrap()));
        std::env::remove_var("TRUST_FORWARDED_FOR");
        assert_eq!(key, "ip:203.0.113.9", "the leftmost entry is the original client");
    }
}
