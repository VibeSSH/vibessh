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
    /// Nobody is signed in to the account backend. Its own code because
    /// the fix is a specific action - sign in, or point the app at a
    /// backend - and because it is the one an ordinary user meets most.
    NotSignedIn,

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
    /// A Database Host points at this Node's own loopback address and no
    /// database server is running there. Its own code for the same reason
    /// `DockerUnavailable` has one: the fix is an install, and installing a
    /// database server is a big enough thing to happen to somebody's machine
    /// that it has to be their decision rather than a side effect of asking
    /// for a database.
    DatabaseServerUnavailable,
    /// The operation ran too long and was given up on - always worth
    /// offering a retry, never worth showing as a hard failure.
    Timeout,
    /// The Node's host key changed. Deliberately its own code: this is the
    /// one error where the right UI is a warning, not a retry button.
    HostKeyMismatch,
    PasswordRequired,
    /// The Node answered and refused the login. Not `InvalidInput`: nothing
    /// typed into a form was malformed - the credentials are the wrong ones,
    /// and the sentence has to name the account that was refused.
    SshAuthRejected,
    /// A Node without cron, asked to keep a schedule. Its own code so the
    /// interface can offer to install it rather than only say so.
    CronMissing,
    /// Deleting a server that still has applications on it.
    ServerHasApplications,
    /// Starting an Application whose directory holds more than its disk limit.
    DiskLimitExceeded,
    /// The local database was migrated by a newer build than this one. Its
    /// own code because the fix - install the newer version again - is a
    /// button, and the first place it shows is the startup-failure screen.
    DatabaseFromNewerVersion,
    /// Somebody else's shared Application, asked to do something this
    /// account was not given - by this install, or refused by the Node.
    SharedActionNotAllowed,
    /// Migrating an Application that has databases on its current Node,
    /// which migration does not move.
    MigrationHasDatabases,

    // ---- Vibe AI ----
    //
    // Four codes rather than one, because the fix differs in every
    // case and the request brief for this feature was explicit that a
    // user must never be shown a raw provider JSON body. The technical
    // detail is logged; only these reach the interface.
    /// The assistant is off, or has no endpoint/model/key yet. The fix is
    /// in Settings, so the UI can link straight there.
    AiNotConfigured,
    /// The provider rejected the key. Not a connection problem and not a
    /// retry: the stored key is wrong, expired or revoked.
    AiAuthFailed,

    // ---- Pterodactyl migration ----
    //
    // Two codes rather than one, because 401 and 403 from a panel have
    // completely different fixes and reading them as the same thing sends
    // somebody looking for a new key when the one they have is correct.
    /// A database account that cannot be used over the network at all,
    /// because it authenticates through the local socket. Its own code
    /// because the fix is a different account, not a different password.
    DatabaseSocketAuthOnly,
    /// The panel refused the Application API key outright.
    PterodactylKeyRejected,
    /// The key was accepted, but has none of the resource permissions the
    /// import needs. The key is right; its checkboxes are not.
    PterodactylKeyForbidden,
    /// The endpoint answered, but does not serve the configured model.
    /// Carries the model name so the message can say which one.
    AiModelUnavailable,
    /// The provider's own rate or quota limit. Worth retrying later,
    /// unlike the two above.
    AiRateLimited,
    /// The endpoint could not be reached, failed on its own side, or
    /// answered with something this client could not parse. One code for
    /// the three because the user's next step is the same for all of
    /// them, and none of them is their fault.
    AiProviderUnavailable,
    /// The included model's own provider failed or refused the backend.
    ///
    /// Separate from `AiProviderUnavailable`, which tells the user to check
    /// the endpoint address and their connection - correct advice when it
    /// is *their* endpoint, and actively misleading here, where the address
    /// and the key are the backend operator's and were reached perfectly
    /// well before being refused. Nothing in the user's own configuration
    /// can fix this; their only move is a personal API key.
    AiHostedFailed,
    /// This VibeSSH backend has no included model configured. Not the
    /// user's problem to fix and not a connection failure: the backend
    /// answered perfectly well and said it offers no model. Their only
    /// move is a personal API key, which is what the message says.
    AiHostedUnavailable,
    /// The account's daily allowance for the *included* model is spent.
    /// Distinct from `AiRateLimited`, which is the upstream provider
    /// throttling and clears in minutes: this one clears at midnight and
    /// the remedy is either waiting or configuring your own provider.
    AiQuotaExhausted,
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

    #[error("no database server is running on {host}")]
    DatabaseServerUnavailable { host: String },

    #[error("{operation} didn't finish within {seconds} seconds")]
    Timeout { operation: &'static str, seconds: u64 },

    #[error("the Node's host key doesn't match the one VibeSSH saw before")]
    HostKeyMismatch {
        host: String,
        /// Filled in by `ssh_service::get_or_connect`, which knows which
        /// server it was connecting to - so the prompt can offer to trust the
        /// new key for that server.
        server_id: Option<uuid::Uuid>,
        /// The SHA-256 fingerprint recorded before, and the one the Node just
        /// presented - both shown to the person, who compares the new one
        /// with what the Node itself reports before trusting it.
        expected: Option<String>,
        presented: Option<String>,
    },

    /// This Node authenticates with a password and none is available - the
    /// keyring has none stored, or could not be reached at all.
    ///
    /// Its own variant because the UI does something specific with it: ask,
    /// and try again. Anything else would be a dead end on a machine with no
    /// working Secret Service, which is most of what "Linux" turns out to
    /// mean in practice - and the alternative, writing the password
    /// somewhere else, is the thing `storage::credentials` exists to refuse.
    #[error("a password is needed to connect to this Node")]
    PasswordRequired { server_id: uuid::Uuid },

    /// The Node refused the SSH login for `username`. It was `InvalidInput`,
    /// which printed "invalid input: ..." in English under a translated
    /// frame - wrong on both counts, since nothing was malformed.
    ///
    /// `method` is "password" or "key", because the remedy differs: a password
    /// can be typed again on the spot, a key has to be fixed in the server's
    /// settings. `server_id` is filled in by `ssh_service::get_or_connect` -
    /// the connect itself does not know it, and a connection test from the
    /// Add Server form has none.
    #[error("the Node rejected the SSH login for {username} - check the username, password or key")]
    SshAuthRejected { username: String, method: &'static str, server_id: Option<uuid::Uuid> },

    /// Schedules are written to the Node's cron, and this Node has none.
    /// `server_id` is what the interface's install button installs it on.
    #[error("this Node has no cron, which schedules need to run")]
    CronMissing { server_id: uuid::Uuid },

    /// A server with applications on it is not deleted: their containers
    /// would go on running on a Node nothing manages any more. Names them,
    /// because "still has applications" alone leaves somebody hunting.
    #[error("{server} still has applications on it ({applications}) - delete them or migrate them to another Node first")]
    ServerHasApplications { server: String, applications: String },

    /// The directory is over its disk limit, so it is not started - the same
    /// rule the Node's own check enforces by stopping it.
    #[error("this application uses {used_mb} MB, over its {limit_mb} MB disk limit - free some space or raise the limit")]
    DiskLimitExceeded { used_mb: u64, limit_mb: u64 },

    /// A database whose schema is ahead of this build: a newer VibeSSH
    /// opened it, then an older one was started. Nothing is damaged or lost,
    /// and the message has to say so, because "migration failed" reads like
    /// the opposite.
    #[error(
        "this {what} database was created by a newer version of VibeSSH (its schema is at version {found}, this build \
         understands {supported}). Nothing has been lost and the database is not damaged - update VibeSSH to open it again, or \
         restore one of the .bak files next to it if you meant to go back."
    )]
    DatabaseFromNewerVersion { what: String, found: usize, supported: usize },

    /// A shared Application, and an action this account may not take on it.
    /// `action` is the permission it would take, so the message can say
    /// which one to ask the owner for. Raised both before anything is sent
    /// (this install knows the grant) and when the Node's sudo refuses
    /// (the grant changed and was synced since).
    #[error("your account may not do that with this shared application ({action}) - ask its owner for it in the application's Users tab")]
    SharedActionNotAllowed { action: String },

    /// Refused before anything is stopped or copied: the Application uses
    /// databases hosted on its current Node, migration does not move them,
    /// and it used to go ahead anyway - dropping their records, leaving the
    /// databases behind untracked, and starting the Application somewhere
    /// its database connection could no longer reach.
    #[error("this application uses databases on its current Node ({databases}), which migration doesn't move - back them up and remove them from the application first")]
    MigrationHasDatabases { databases: String },

    // ---- Vibe AI ----
    //
    // Every message here is a plain sentence, and none of them carries a
    // provider response body. That is not tidiness: a provider error body
    // is attacker-influenced text from a third-party endpoint, and it is
    // also where an echoed request would put the API key. It goes to
    // `log::warn!` in `ai::openai_compatible` and no further.
    #[error("the AI assistant isn't configured yet")]
    AiNotConfigured,

    #[error("the AI provider rejected the API key")]
    AiAuthFailed,

    #[error("the AI provider doesn't offer the model {model}")]
    AiModelUnavailable { model: String },

    #[error("the AI provider's rate limit was reached")]
    AiRateLimited,

    #[error("couldn't reach the AI provider")]
    AiProviderUnavailable,

    #[error("this VibeSSH backend doesn't offer an included AI model")]
    AiHostedUnavailable,

    #[error("the included AI model couldn't be used")]
    AiHostedFailed,

    #[error("today's allowance for the included AI model is used up")]
    AiQuotaExhausted,

    /// A refusal the cloud backend described in its own terms.
    ///
    /// `kind` is the backend's coarse class, which decides this error's
    /// `ErrorCode` and therefore how the app behaves - a `401` must still
    /// look like `Unauthorized` to the token-refresh logic. `code` is the
    /// backend's specific identifier, which decides only what sentence the
    /// user reads. `params` fill that sentence, and `message` is the
    /// backend's English prose, kept as the fallback for a code this build
    /// has no translation for.
    ///
    /// One variant rather than forty: the backend already publishes a stable
    /// code per refusal, and copying that list into this enum would mean a
    /// desktop release before any new backend error could be shown in the
    /// user's language.
    #[error("{message}")]
    Cloud { kind: String, code: String, params: serde_json::Value, message: String },

    /// `user` is the account as MySQL named it, e.g. `root@localhost`.
    #[error("{user} authenticates through the local socket and cannot be used with a password")]
    DatabaseSocketAuthOnly { user: String },

    // ---- Pterodactyl migration ----
    #[error("the Pterodactyl panel rejected the Application API key")]
    PterodactylKeyRejected,

    /// `resource` is the panel path that was refused, which is what says
    /// *which* permission is missing - "servers" and "nodes" are separate
    /// checkboxes on a Pterodactyl key.
    #[error("the Pterodactyl API key has no permission for {resource}")]
    PterodactylKeyForbidden { resource: String },
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
            AppError::Unauthorized(message) if message.starts_with("not signed in") => ErrorCode::NotSignedIn,
            AppError::Unauthorized(_) => ErrorCode::Unauthorized,
            AppError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
            AppError::PortInUse { .. } => ErrorCode::PortInUse,
            AppError::DockerUnavailable => ErrorCode::DockerUnavailable,
            AppError::DatabaseServerUnavailable { .. } => ErrorCode::DatabaseServerUnavailable,
            AppError::Timeout { .. } => ErrorCode::Timeout,
            AppError::HostKeyMismatch { .. } => ErrorCode::HostKeyMismatch,
            AppError::PasswordRequired { .. } => ErrorCode::PasswordRequired,
            AppError::SshAuthRejected { .. } => ErrorCode::SshAuthRejected,
            AppError::CronMissing { .. } => ErrorCode::CronMissing,
            AppError::ServerHasApplications { .. } => ErrorCode::ServerHasApplications,
            AppError::DiskLimitExceeded { .. } => ErrorCode::DiskLimitExceeded,
            AppError::DatabaseFromNewerVersion { .. } => ErrorCode::DatabaseFromNewerVersion,
            AppError::SharedActionNotAllowed { .. } => ErrorCode::SharedActionNotAllowed,
            AppError::MigrationHasDatabases { .. } => ErrorCode::MigrationHasDatabases,
            AppError::AiNotConfigured => ErrorCode::AiNotConfigured,
            AppError::DatabaseSocketAuthOnly { .. } => ErrorCode::DatabaseSocketAuthOnly,
            AppError::PterodactylKeyRejected => ErrorCode::PterodactylKeyRejected,
            AppError::PterodactylKeyForbidden { .. } => ErrorCode::PterodactylKeyForbidden,
            AppError::AiAuthFailed => ErrorCode::AiAuthFailed,
            AppError::AiModelUnavailable { .. } => ErrorCode::AiModelUnavailable,
            AppError::AiRateLimited => ErrorCode::AiRateLimited,
            AppError::AiProviderUnavailable => ErrorCode::AiProviderUnavailable,
            AppError::AiHostedUnavailable => ErrorCode::AiHostedUnavailable,
            AppError::AiHostedFailed => ErrorCode::AiHostedFailed,
            AppError::AiQuotaExhausted => ErrorCode::AiQuotaExhausted,
            // The backend's coarse class, mapped to the same codes a local
            // failure of that class would produce. Anything the desktop
            // branches on - forgetting a dead token, telling the user to
            // sign in - keeps working without knowing the backend's
            // vocabulary.
            AppError::Cloud { kind, .. } => match kind.as_str() {
                "unauthorized" | "password_change_required" | "forbidden" => ErrorCode::Unauthorized,
                "not_found" => ErrorCode::NotFound,
                "invalid_input" | "conflict" => ErrorCode::InvalidInput,
                _ => ErrorCode::Internal,
            },
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
            AppError::HostKeyMismatch { host, server_id, expected, presented } => {
                serde_json::json!({ "host": host, "serverId": server_id, "expected": expected, "presented": presented })
            }
            AppError::PasswordRequired { server_id } => serde_json::json!({ "serverId": server_id }),
            AppError::SshAuthRejected { username, method, server_id } => {
                serde_json::json!({ "username": username, "method": method, "serverId": server_id })
            }
            AppError::CronMissing { server_id } => serde_json::json!({ "serverId": server_id }),
            AppError::ServerHasApplications { server, applications } => serde_json::json!({ "server": server, "applications": applications }),
            AppError::DiskLimitExceeded { used_mb, limit_mb } => serde_json::json!({ "usedMb": used_mb, "limitMb": limit_mb }),
            AppError::DatabaseFromNewerVersion { what, found, supported } => {
                serde_json::json!({ "what": what, "found": found, "supported": supported })
            }
            AppError::SharedActionNotAllowed { action } => serde_json::json!({ "action": action }),
            AppError::MigrationHasDatabases { databases } => serde_json::json!({ "databases": databases }),
            AppError::AiModelUnavailable { model } => serde_json::json!({ "model": model }),
            AppError::PterodactylKeyForbidden { resource } => serde_json::json!({ "resource": resource }),
            AppError::DatabaseSocketAuthOnly { user } => serde_json::json!({ "user": user }),
            // The coarse variants carry their own English detail.
            //
            // Without this the frontend had nothing to put in a translated
            // sentence, so `errorMessage` fell through to `Display` and the
            // user saw the whole thing in English - which is what somebody
            // reported. A translated frame around the detail is not a full
            // translation of the detail, but it is the difference between an
            // interface that speaks their language and one that does not.
            // The backend's own params, plus the code the interface picks
            // its sentence by and the English text to fall back on.
            AppError::Cloud { code, params, message, .. } => {
                let mut object = params.as_object().cloned().unwrap_or_default();
                object.insert("backendCode".to_string(), serde_json::Value::String(code.clone()));
                object.insert("message".to_string(), serde_json::Value::String(message.clone()));
                serde_json::Value::Object(object)
            }
            AppError::NotFound(message)
            | AppError::InvalidInput(message)
            | AppError::Storage(message)
            | AppError::Connection(message)
            | AppError::Internal(message)
            | AppError::Unauthorized(message) => serde_json::json!({ "message": message }),
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
            ErrorCode::Unauthorized | ErrorCode::NotSignedIn => "unauthorized",
            // The specific codes did not exist when `kind` was the only
            // discriminator, so each maps onto the coarse bucket a reader
            // of `kind` would previously have seen.
            ErrorCode::PermissionDenied | ErrorCode::PortInUse | ErrorCode::DockerUnavailable | ErrorCode::DatabaseServerUnavailable => {
                "invalid_input"
            }
            ErrorCode::Timeout | ErrorCode::HostKeyMismatch => "connection",
            // Coarsely an input problem: something the person can supply.
            ErrorCode::PasswordRequired => "invalid_input",
            // Was literally `InvalidInput` until it had a code of its own, so
            // a `kind` reader keeps seeing exactly what it saw before.
            ErrorCode::SshAuthRejected => "invalid_input",
            // Something the operator can fix on the Node - an input problem.
            ErrorCode::CronMissing => "invalid_input",
            ErrorCode::ServerHasApplications => "invalid_input",
            ErrorCode::DiskLimitExceeded => "invalid_input",
            // Was a plain `Storage` error until it had a code of its own.
            ErrorCode::DatabaseFromNewerVersion => "storage",
            // A permission problem, as a reader of `kind` would have seen it.
            ErrorCode::SharedActionNotAllowed => "invalid_input",
            ErrorCode::MigrationHasDatabases => "invalid_input",
            // Same rule as the block above: each new code degrades to the
            // coarse bucket a `kind` reader would have seen before it
            // existed. Not configured and a rejected key are input
            // problems the user can fix; the rest are the network.
            ErrorCode::DatabaseSocketAuthOnly => "invalid_input",
            ErrorCode::PterodactylKeyRejected | ErrorCode::PterodactylKeyForbidden => "unauthorized",
            ErrorCode::AiNotConfigured | ErrorCode::AiAuthFailed | ErrorCode::AiModelUnavailable => "invalid_input",
            ErrorCode::AiRateLimited | ErrorCode::AiProviderUnavailable => "connection",
            ErrorCode::AiHostedUnavailable | ErrorCode::AiQuotaExhausted => "invalid_input",
            ErrorCode::AiHostedFailed => "connection",
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
            AppError::HostKeyMismatch { host: "node.example.com".into(), server_id: None, expected: None, presented: None },
            AppError::AiNotConfigured,
            AppError::AiAuthFailed,
            AppError::AiModelUnavailable { model: "gpt-4o-mini".into() },
            AppError::AiRateLimited,
            AppError::AiProviderUnavailable,
            AppError::AiHostedUnavailable,
            AppError::AiHostedFailed,
            AppError::AiQuotaExhausted,
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
        assert_eq!(json(&AppError::HostKeyMismatch { host: "h".into(), server_id: None, expected: None, presented: None })["kind"], "connection");
        assert_eq!(json(&AppError::Unauthorized("x".into()))["kind"], "unauthorized");
    }

    /// A variant whose message says everything - there is nothing to fill a
    /// slot with, and the translated sentence stands on its own.
    #[test]
    fn errors_with_nothing_to_interpolate_carry_null_params() {
        assert!(json(&AppError::DockerUnavailable)["params"].is_null());
        assert!(json(&AppError::AiRateLimited)["params"].is_null());
    }

    /// The coarse variants carry theirs, and this is why.
    ///
    /// Their message is written at the call site in English and there are
    /// hundreds of them, so the interface cannot translate the detail - but
    /// it can translate the sentence around it, and that needs the detail as
    /// a parameter. Before this, `errorMessage` had nothing to interpolate,
    /// fell through to the English `Display`, and a Polish interface showed
    /// "unauthorized: not signed in to the VibeSSH cloud backend".
    #[test]
    fn a_coarse_error_carries_its_own_detail_for_the_translated_frame() {
        for error in [
            AppError::InvalidInput("a name is required".into()),
            AppError::NotFound("application 1".into()),
            AppError::Storage("failed to write".into()),
            AppError::Connection("host is down".into()),
            AppError::Internal("unreachable".into()),
            AppError::Unauthorized("refresh token revoked".into()),
        ] {
            let params = json(&error)["params"].clone();
            assert!(params["message"].is_string(), "{error:?} should carry its message: {params}");
        }
    }

    /// Somebody who is simply not signed in gets a sentence of their own
    /// rather than a frame around English - it is the error an ordinary user
    /// meets most, and its detail adds nothing they can act on.
    #[test]
    fn not_being_signed_in_has_its_own_code() {
        assert_eq!(json(&AppError::Unauthorized("not signed in".into()))["code"], "not_signed_in");
        assert_eq!(json(&AppError::Unauthorized("not signed in to the VibeSSH cloud backend".into()))["code"], "not_signed_in");
        // Anything else authorisation-related keeps the coarse code.
        assert_eq!(json(&AppError::Unauthorized("refresh token revoked".into()))["code"], "unauthorized");
    }
}
