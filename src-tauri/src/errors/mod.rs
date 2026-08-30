use serde::Serialize;
use thiserror::Error;

/// Single error type shared by every backend module. New modules should add a
/// variant here (or a `#[from]` conversion) instead of inventing their own.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// Tauri serializes command errors to the frontend as JSON, so AppError needs
/// to implement Serialize. We flatten it to `{ kind, message }` rather than
/// deriving it, since the variants carry plain strings we already format via
/// `Display`.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let kind = match self {
            AppError::NotFound(_) => "not_found",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::Storage(_) => "storage",
            AppError::Connection(_) => "connection",
            AppError::Internal(_) => "internal",
        };
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}
