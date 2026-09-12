//! Single error type for every HTTP handler in this service - mirrors
//! src-tauri's own AppError (same {kind, message} JSON shape), but maps to
//! real HTTP status codes instead of being handed back through a Tauri
//! command channel.
//!
//! Every refusal a user can see also carries a stable `code` and, where the
//! sentence has a number or a name in it, the `params` to fill it with. The
//! English `message` is the fallback and the log line; it is not what the
//! desktop app shows. Without the code there was nothing for a client to
//! translate by, so a Polish interface rendered "Brak uprawnien: invalid
//! email or password" - half of one language, half of another.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A refusal the user will read: a stable code to translate by, an English
/// message to fall back to, and the values a translated sentence needs.
///
/// Deliberately not constructible from a bare `String`. Every site that
/// raises a user-visible error has to name a code, and the compiler is what
/// enforces that - a `From<String>` shortcut would let the next one slip
/// through untranslated, which is the bug this type exists to prevent.
#[derive(Debug)]
pub struct Detail {
    pub code: &'static str,
    pub message: String,
    params: serde_json::Map<String, serde_json::Value>,
}

impl Detail {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), params: serde_json::Map::new() }
    }

    /// One value for the translated sentence to interpolate, named as the
    /// locale file's `{{slot}}` names it.
    pub fn with(mut self, slot: &str, value: impl Into<serde_json::Value>) -> Self {
        self.params.insert(slot.to_string(), value.into());
        self
    }
}

impl std::fmt::Display for Detail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug)]
pub enum ApiError {
    InvalidInput(Detail),
    /// Not authenticated at all, or the credentials/token presented are
    /// invalid - a client seeing this is expected to try to re-authenticate.
    Unauthorized(Detail),
    /// Authenticated as someone real, but that person isn't allowed to do
    /// this - re-authenticating would never fix it, so this must never be
    /// confused with Unauthorized (a client that retries a 401 by
    /// refreshing tokens would just get the same 403 again).
    Forbidden(Detail),
    Conflict(Detail),
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
    TooManyRequests(Detail),
    /// Authenticated, but the account still holds the password somebody
    /// else set for it and must replace it before doing anything.
    ///
    /// Its own variant rather than `Forbidden`, for the same reason
    /// `TooManyRequests` is: a client seeing a 403 should stop, and a client
    /// seeing this should send the user to one specific screen and then
    /// carry on. Collapsing them would leave the desktop app guessing from
    /// the message text.
    PasswordChangeRequired(Detail),
    NotFound(Detail),
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
    /// Stable and machine-readable - what a client translates by. Absent
    /// only for the two errors whose detail is never shown to anyone.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    message: String,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    params: serde_json::Map<String, serde_json::Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, body) = match self {
            ApiError::InvalidInput(detail) => (StatusCode::BAD_REQUEST, "invalid_input", detail),
            ApiError::Unauthorized(detail) => (StatusCode::UNAUTHORIZED, "unauthorized", detail),
            ApiError::Forbidden(detail) => (StatusCode::FORBIDDEN, "forbidden", detail),
            // 403 as well, because it is a refusal that re-authenticating
            // will not lift - only changing the password will. The `kind`
            // is what tells the two apart.
            ApiError::PasswordChangeRequired(detail) => (StatusCode::FORBIDDEN, "password_change_required", detail),
            ApiError::Conflict(detail) => (StatusCode::CONFLICT, "conflict", detail),
            // The message is safe to show: it names the limit, which is the
            // one thing the user needs in order to understand the refusal.
            ApiError::TooManyRequests(detail) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests", detail),
            ApiError::NotFound(detail) => (StatusCode::NOT_FOUND, "not_found", detail),
            // 502, because that is what it is: a gateway got an unusable
            // answer from the service behind it. The message is withheld
            // like `Internal`'s - an upstream body is where a key gets
            // echoed back. No code either: there is nothing here specific
            // enough for a client to say more than "the provider failed".
            ApiError::UpstreamFailure(msg) => {
                log::error!("{msg}");
                let body = ErrorBody {
                    kind: "upstream_failure",
                    code: None,
                    message: "the AI provider could not be used".to_string(),
                    params: serde_json::Map::new(),
                };
                return (StatusCode::BAD_GATEWAY, axum::Json(body)).into_response();
            }
            ApiError::Internal(msg) => {
                log::error!("{msg}");
                let body = ErrorBody {
                    kind: "internal",
                    code: None,
                    message: "an internal error occurred".to_string(),
                    params: serde_json::Map::new(),
                };
                return (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(body)).into_response();
            }
        };
        let Detail { code, message, params } = body;
        (status, axum::Json(ErrorBody { kind, code: Some(code), message, params })).into_response()
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
