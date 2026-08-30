//! Library half of the `vibe-agent` crate. `src/main.rs` is a thin binary
//! wrapper around this; splitting it out lets integration tests in `tests/`
//! drive the real router/handshake code instead of a reimplementation.

pub mod capabilities;
pub mod config;
pub mod errors;
pub mod identity;
pub mod info;
pub mod pairing;
pub mod transport;
