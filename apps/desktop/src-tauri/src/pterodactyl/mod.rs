//! Reading a Pterodactyl panel, and working out what it would become in
//! VibeSSH.
//!
//! Pterodactyl is this project's reference point throughout - per-application
//! databases, eggs, allocations and the Java-version picker are all shaped
//! after it (see `docs/architecture/APPLICATIONS_ARCHITECTURE.md`). That is why an
//! importer is worth having at all: the two models line up almost field for
//! field, so a migration is mostly translation rather than reconstruction.
//!
//! **Nothing in this module writes anywhere.** It reads the panel, decides
//! what each server should become, and hands back a plan for a person to
//! agree to. Creating Applications, copying files and moving databases happen
//! outside it, against that agreed plan - which is what makes the plan worth
//! showing.

pub mod client;
pub mod mapping;
pub mod models;

pub use client::PterodactylClient;
pub use mapping::{map_server, ImagePlan};
