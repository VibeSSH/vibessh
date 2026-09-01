// This codebase's doc comments lean heavily on multi-line prose under a
// bullet, which `doc_lazy_continuation` wants indented to keep rustdoc's
// rendering exact. The rendering difference is cosmetic, the comments are
// the main way design decisions are recorded here, and reflowing 49 of them
// would bury real changes in whitespace diffs. Allowed deliberately, at the
// crate root, so the choice is visible rather than implicit.
#![allow(clippy::doc_lazy_continuation)]

//! Library half of the `vibe-agent` crate. `src/main.rs` is a thin binary
//! wrapper around this; splitting it out lets integration tests in `tests/`
//! drive the real router/handshake code instead of a reimplementation.

pub mod capabilities;
pub mod cli;
pub mod config;
pub mod errors;
pub mod identity;
pub mod info;
pub mod metrics;
pub mod pairing;
pub mod tls;
pub mod transport;

/// Bind address for the local pairing control endpoint. Always loopback,
/// never read from the same env var as the public server - see
/// `transport::control` for why. `cli::pair` and `main`'s daemon startup
/// both need this constant, hence it living here rather than in either one.
pub const DEFAULT_CONTROL_BIND: &str = "127.0.0.1:7421";
