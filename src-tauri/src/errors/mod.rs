use serde::Serialize;
use thiserror::Error;

/// What the frontend branches on.
///
/// **Why this exists.** `AppError` used to serialize as
/// `{ kind, message }` where `kind` was one of six coarse buckets and
/// `message` was `Display` output - so the only thing the UI could actually
/// show was a raw Rust string, in English, with no way to react to *what*
/// had gone wrong. The brief's own example of the problem was a user seeing
///
/// > invalid input: containing directory doesn't exist
///
/// which is not wrong, but tells someone managing a game server nothing
/// they can act on, and cannot be translated because it is assembled in
/// Rust.
///
/// A code is a promise the frontend can rely on: `port_in_use` always means
/// the same thing and always carries a port, so the UI can render a real
/// sentence in the user's own language and offer the right next step. The
/// six coarse codes are still here because 685 error construction sites
/// cannot be reclassified in one change, and a partial migration that
/// leaves the rest unroutable would be worse than a clear floor - anything
/// not yet specific degrades to exactly today's behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // The coarse floor, one per legacy variant.
    NotFound,
    InvalidInput,
    Storage,
    Connection,
    Internal,
    Unauthorized,

    // Specific enough that the UI can do something other than print text.
    /// The operation is understood and valid, but this account may not do
    /// it. Distinct from `InvalidInput` because the fix is different: not
    /// "type something else" but "grant something".
    PermissionDenied,
    /// Something else already holds the port. Carries the port so the UI can
    /// name it and offer to pick another.
    PortInUse,
    /// The Node has no usable Docker daemon. The fix is installing it, which
    /// the app can offer to do.
    DockerUnavailable,
    /// The operation ran too long and was given up on - always worth
    /// offering a retry, never worth showing as a hard failure.
    Timeout,
    /// The Node's host key changed. Deliberately its own code: this is the
    /// one error where the right UI is a warning, not a retry button.
    HostKeyMismatch,
}

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

    /// Not (or no longer) signed in to the cloud backend - the frontend
    /// should route this to a login prompt rather than a generic error
    /// toast.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    // ---- Specific failures, below ----
    //
    // Each of these exists because the UI genuinely does something
    // different with it. A variant that would only ever be rendered as its
    // own message belongs in one of the buckets above instead - the point
    // is routing, not taxonomy for its own sake.
    #[error("permission denied: {message}")]
    PermissionDenied { message: String },

    #[error("port {port}/{protocol} is already in use")]
    PortInUse {
        port: u16,
        protocol: &'static str,
        /// What is holding it, when that could be determined - a process
        /// name from `ss`, or another Application's name.
        owner: Option<String>,
    },

    #[error("Docker isn't available on this Node")]
    DockerUnavailable,

    #[error("{operation} didn't finish within {seconds} seconds")]
    Timeout { operation: &'static str, seconds: u64 },

    #[error("the Node's host key doesn't match the one VibeSSH saw before")]
    HostKeyMismatch { host: String },
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::NotFound(_) => ErrorCode::NotFound,
            AppError::InvalidInput(_) => ErrorCode::InvalidInput,
            AppError::Storage(_) => ErrorCode::Storage,
            AppError::Connection(_) => ErrorCode::Connection,
            AppError::Internal(_) => ErrorCode::Internal,
            AppError::Unauthorized(_) => ErrorCode::Unauthorized,
            AppError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
            AppError::PortInUse { .. } => ErrorCode::PortInUse,
            AppError::DockerUnavailable => ErrorCode::DockerUnavailable,
            AppError::Timeout { .. } => ErrorCode::Timeout,
            AppError::HostKeyMismatch { .. } => ErrorCode::HostKeyMismatch,
        }
    }

    /// The values a translated message needs, as a JSON object.
    ///
    /// Kept separate from the message rather than only formatted into it:
    /// the frontend has to be able to write its own sentence around these,
    /// in its own language and word order, and it cannot do that by parsing
    /// English prose back apart.
    fn params(&self) -> serde_json::Value {
        match self {
            AppError::PortInUse { port, protocol, owner } => {
                serde_json::json!({ "port": port, "protocol": protocol, "owner": owner })
            }
            AppError::Timeout { operation, seconds } => serde_json::json!({ "operation": operation, "seconds": seconds }),
            AppError::HostKeyMismatch { host } => serde_json::json!({ "host": host }),
            _ => serde_json::Value::Null,
        }
    }
}

/// Tauri serializes command errors to the frontend as JSON.
///
/// Three fields, and the split matters:
/// - `code` is what the UI branches on and translates by.
/// - `params` is what a translated sentence interpolates.
/// - `message` is the English `Display` output, kept as a fallback for a
///   code the frontend has no translation for yet, and as the "technical
///   details" a user can copy into a bug report. It is deliberately *not*
///   the primary thing to show.
///
/// `kind` is still emitted, unchanged, so nothing that reads it breaks
/// while call sites migrate to `code`.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let code = self.code();
        let kind = match code {
            ErrorCode::NotFound => "not_found",
            ErrorCode::InvalidInput => "invalid_input",
            ErrorCode::Storage => "storage",
            ErrorCode::Connection => "connection",
            ErrorCode::Internal => "internal",
            ErrorCode::Unauthorized => "unauthorized",
            // The specific codes did not exist when `kind` was the only
            // discriminator, so each maps onto the coarse bucket a reader
            // of `kind` would previously have seen.
            ErrorCode::PermissionDenied | ErrorCode::PortInUse | ErrorCode::DockerUnavailable => "invalid_input",
            ErrorCode::Timeout | ErrorCode::HostKeyMismatch => "connection",
        };
        let mut state = serializer.serialize_struct("AppError", 4)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("code", &code)?;
        state.serialize_field("params", &self.params())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(error: &AppError) -> serde_json::Value {
        serde_json::to_value(error).unwrap()
    }

    #[test]
    fn every_variant_serializes_a_code_and_a_message() {
        for error in [
            AppError::NotFound("server 1".into()),
            AppError::InvalidInput("a name is required".into()),
            AppError::Storage("database is locked".into()),
            AppError::Connection("connection reset".into()),
            AppError::Internal("unreachable".into()),
            AppError::Unauthorized("not signed in".into()),
            AppError::PermissionDenied { message: "can't write there".into() },
            AppError::PortInUse { port: 25565, protocol: "tcp", owner: None },
            AppError::DockerUnavailable,
            AppError::Timeout { operation: "the command", seconds: 600 },
            AppError::HostKeyMismatch { host: "node.example.com".into() },
        ] {
            let value = json(&error);
            assert!(value["code"].is_string(), "{value}");
            assert!(!value["message"].as_str().unwrap_or("").is_empty(), "{value}");
            assert!(value["kind"].is_string(), "{value}");
        }
    }

    /// The whole point of `params`: the frontend has to be able to build its
    /// own sentence in its own word order, which it cannot do by parsing
    /// English prose back apart.
    #[test]
    fn a_port_conflict_carries_the_port_separately_from_the_message() {
        let value = json(&AppError::PortInUse { port: 25565, protocol: "tcp", owner: Some("nginx".into()) });
        assert_eq!(value["code"], "port_in_use");
        assert_eq!(value["params"]["port"], 25565);
        assert_eq!(value["params"]["protocol"], "tcp");
        assert_eq!(value["params"]["owner"], "nginx");
    }

    #[test]
    fn a_timeout_carries_the_operation_and_the_limit() {
        let value = json(&AppError::Timeout { operation: "the command", seconds: 600 });
        assert_eq!(value["code"], "timeout");
        assert_eq!(value["params"]["seconds"], 600);
    }

    /// `kind` predates `code` and is still read by the frontend's existing
    /// handling, so the specific codes have to map onto the bucket a reader
    /// of `kind` would previously have seen rather than onto something new.
    #[test]
    fn kind_stays_backwards_compatible_for_the_new_codes() {
        assert_eq!(json(&AppError::DockerUnavailable)["kind"], "invalid_input");
        assert_eq!(json(&AppError::PortInUse { port: 1, protocol: "tcp", owner: None })["kind"], "invalid_input");
        assert_eq!(json(&AppError::Timeout { operation: "x", seconds: 1 })["kind"], "connection");
        assert_eq!(json(&AppError::HostKeyMismatch { host: "h".into() })["kind"], "connection");
        assert_eq!(json(&AppError::Unauthorized("x".into()))["kind"], "unauthorized");
    }

    #[test]
    fn errors_with_nothing_to_interpolate_carry_null_params() {
        assert!(json(&AppError::InvalidInput("x".into()))["params"].is_null());
        assert!(json(&AppError::DockerUnavailable)["params"].is_null());
    }
}
