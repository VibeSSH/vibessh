use thiserror::Error;

/// Mirrors the shape of the desktop's `AppError` (kind + message) but is its
/// own type: the agent is a headless daemon and must never depend on the
/// Tauri crate, so the two error types are deliberately not shared.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),
}

pub type AgentResult<T> = Result<T, AgentError>;
