//! The shared vocabulary of remote-server types, re-exported from
//! `vibessh_protocol` so commands and services import them from one place
//! rather than reaching into the protocol crate individually.
//!
//! **This module used to also define a `ServerConnection` trait** - twenty
//! methods covering commands, metrics, processes, systemd units, containers
//! and files - documented as "the abstraction Etap B exists to introduce",
//! with `SshTransport` and `AgentTransport` named as its two
//! implementations.
//!
//! It was removed because none of that was true. Only one implementation
//! ever existed (`impl ServerConnection for SshSession`), and the compiler
//! confirmed that **not one of its twenty methods was ever called** - every
//! call site used `SshSession` concretely. It was not scaffolding for a
//! second transport; it was a guess at what a second transport would need,
//! made without one to check against, and it cost a 147-line trait plus a
//! 94-line impl to keep compiling while actively misleading anyone reading
//! it. `models::server`'s own doc comment claimed services and the frontend
//! "work through `ServerConnection`", which was simply not so.
//!
//! When Agent Mode grows real capabilities (AUDIT A-002 - today an
//! agent-paired Node can do essentially nothing after pairing), the seam
//! should be shaped around what those two transports actually turn out to
//! share. Until then this file is what it always really was: a set of type
//! re-exports.

pub use vibessh_protocol::{CommandOutput, ContainerSummary, ProcessSummary, RemoteFileEntry, ServerMetrics, ServiceSummary};
