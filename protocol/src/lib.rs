mod capabilities;
mod dto;
mod error;
mod events;
mod handshake;
mod pairing;

pub use capabilities::AgentCapabilities;
pub use dto::{CommandOutput, ContainerSummary, ProcessSummary, RemoteFileEntry, ServerMetrics, ServiceSummary};
pub use error::ProtocolErrorCode;
pub use events::{LogLine, QuickActionProgress, ServerEvent, TerminalClosed, TerminalOutput};
pub use handshake::{HandshakeRequest, HandshakeResponse, PROTOCOL_VERSION};
pub use pairing::{generate_pairing_code, PAIRING_CODE_TTL};
