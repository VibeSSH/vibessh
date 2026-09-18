// This codebase's doc comments lean heavily on multi-line prose under a
// bullet, which `doc_lazy_continuation` wants indented to keep rustdoc's
// rendering exact. The rendering difference is cosmetic, the comments are
// the main way design decisions are recorded here, and reflowing 49 of them
// would bury real changes in whitespace diffs. Allowed deliberately, at the
// crate root, so the choice is visible rather than implicit.
#![allow(clippy::doc_lazy_continuation)]

mod capabilities;
mod commands;
mod dto;
mod error;
mod events;
mod handshake;
mod pairing;

pub use capabilities::AgentCapabilities;
pub use commands::{DesktopCommand, NodeDesiredState};
pub use dto::{CommandOutput, ContainerSummary, MinecraftMetrics, ProcessSummary, RemoteFileEntry, ServerMetrics, ServiceSummary};
pub use error::ProtocolErrorCode;
pub use events::{LogLine, QuickActionProgress, ServerEvent, TerminalClosed, TerminalOutput};
pub use handshake::{HandshakeRequest, HandshakeResponse, PROTOCOL_VERSION};
pub use pairing::{generate_pairing_code, PAIRING_CODE_TTL};
