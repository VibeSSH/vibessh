//! The Vibe AI assistant's mechanism half.
//!
//! The split here mirrors the one the rest of this codebase already uses:
//! `runtime`, `firewall` and `network` are mechanism, and `services`
//! decides when to use them. So this module knows how to talk to a model
//! endpoint, how to collect a snapshot, what to redact and what the
//! standing instruction says - and nothing about when any of that should
//! happen. `services::ai_service` is where that lives.
//!
//! Read `sanitizer` first. It is the only part of this feature whose
//! mistakes cannot be taken back.

pub mod context;
pub mod knowledge;
pub mod openai_compatible;
pub mod prompt;
pub mod provider;
pub mod sanitizer;
