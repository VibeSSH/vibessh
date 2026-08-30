//! Single error type for every HTTP handler in this service - mirrors
//! src-tauri's own AppError (same {kind, message} JSON shape), but maps to
//! real HTTP status codes instead of being handed back through a Tauri
//! command channel.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    InvalidInput(String),
    Unauthorized(String),
    Conflict(String),
    NotFound(String),
    /// The message here is for the server log only - `IntoResponse` never
    /// sends it to the client (see the security note on Error Handling in
    /// the production roadmap: never show the user a raw internal error).
    Internal(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            ApiError::Unauthorized(msg) => write!(f, "unauthorized: {msg}"),
            ApiError::Conflict(msg) => write!(f, "conflict: {msg}"),
            ApiError::NotFound(msg) => write!(f, "not found: {msg}"),
            ApiError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Serialize)]
struct ErrorBody {
    kind: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, message) = match self {
            ApiError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, "invalid_input", msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "unauthorized", msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg),
            ApiError::Internal(msg) => {
                log::error!("{msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", "an internal error occurred".to_string())
            }
        };
        (status, axum::Json(ErrorBody { kind, message })).into_response()
    }
}

/// Any `sqlx::Error` that reaches a handler is an internal error - handlers
/// map the specific cases they care about (e.g. a unique-constraint
/// violation on register) to a real `ApiError` variant before this
/// conversion would ever apply.
impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Internal(err.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
