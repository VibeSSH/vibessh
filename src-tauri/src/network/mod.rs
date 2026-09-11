//! The Vibe Network - a WireGuard-based private overlay between Nodes
//! (Etap M4). See `docs/architecture/APPLICATIONS_ARCHITECTURE.md`'s own Etap L note and
//! this project's Etap M planning doc for the full design; this module is
//! the mechanism, `services::network_service` is the orchestration
//! (IPAM + membership + full-mesh reconcile) built on top of it.
//!
//! **Never hand-rolled crypto or tunneling** - every real operation here is
//! a call to the real `wg`/`wg-quick` CLI over SSH exec, the same
//! "piggyback on an existing, audited implementation" stance the project's
//! own Etap L note already committed to.

pub mod wireguard;
