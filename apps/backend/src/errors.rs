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
    /// Not authenticated at all, or the credentials/token presented are
    /// invalid - a client seeing this is expected to try to re-authenticate.
    Unauthorized(String),
    /// Authenticated as someone real, but that person isn't allowed to do
    /// this - re-authenticating would never fix it, so this must never be
    /// confused with Unauthorized (a client that retries a 401 by
    /// refreshing tokens would just get the same 403 again).
    Forbidden(String),
    Conflict(String),
    /// The configured upstream AI provider failed or refused us.
    ///
    /// Not `Internal`: nothing in this service went wrong, and reporting it
    /// as a 500 tells an operator's monitoring to look in the wrong place.
    /// It is also not the caller's fault, which is why the message they get
    /// stays generic while the detail goes to this server's log.
    UpstreamFailure(String),
    /// A per-account allowance is spent. Its own variant rather than
    /// `Forbidden` because the two mean opposite things to a client: a 403
    /// will never succeed however long you wait, and this one succeeds
    /// tomorrow. Introduced for the hosted AI assistant's daily question
    /// limit (`ai::chat`).
    TooManyRequests(String),
    /// Authenticated, but the account still holds the password somebody
    /// else set for it and must replace it before doing anything.
    ///
    /// Its own variant rather than `Forbidden`, for the same reason
    /// `TooManyRequests` is: a client seeing a 403 should stop, and a client
    /// seeing this should send the user to one specific screen and then
    /// carry on. Collapsing them would leave the desktop app guessing from
    /// the message text.
    PasswordChangeRequired(String),
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
            ApiError::Forbidden(msg) => write!(f, "forbidden: {msg}"),
            ApiError::PasswordChangeRequired(msg) => write!(f, "password change required: {msg}"),
            ApiError::Conflict(msg) => write!(f, "conflict: {msg}"),
            ApiError::TooManyRequests(msg) => write!(f, "too many requests: {msg}"),
            ApiError::UpstreamFailure(msg) => write!(f, "upstream failure: {msg}"),
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
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg),
            // 403 as well, because it is a refusal that re-authenticating
            // will not lift - only changing the password will. The `kind`
            // is what tells the two apart.
            ApiError::PasswordChangeRequired(msg) => (StatusCode::FORBIDDEN, "password_change_required", msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            // The message is safe to show: it names the limit, which is the
            // one thing the user needs in order to understand the refusal.
            ApiError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests", msg),
            // 502, because that is what it is: a gateway got an unusable
            // answer from the service behind it. The message is withheld
            // like `Internal`'s - an upstream body is where a key gets
            // echoed back.
            ApiError::UpstreamFailure(msg) => {
                log::error!("{msg}");
                (StatusCode::BAD_GATEWAY, "upstream_failure", "the AI provider could not be used".to_string())
            }
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
