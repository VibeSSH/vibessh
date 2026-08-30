//! `credentials` exists early because Etap E's pairing flow produces a
//! secret the moment it lands - "don't persist secrets in plaintext" isn't
//! optional just because the full server repository wasn't built yet.
//! `server_repository` (Etap 2) is the SQLite-backed store for server
//! records themselves; it never touches secrets, see `credentials`.

pub mod credentials;
pub mod migrations;
pub mod server_repository;
