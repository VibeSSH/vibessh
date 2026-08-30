//! Server repository storage (SQLite for server records) is still reserved
//! for Etap 2. `credentials` exists early because Etap E's pairing flow
//! produces a secret the moment it lands - "don't persist secrets in
//! plaintext" isn't optional just because the full repository isn't built
//! yet.

pub mod credentials;
