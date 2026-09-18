//! A door Claude can knock on, so it can answer questions about your
//! servers instead of guessing.
//!
//! **What this is.** A Model Context Protocol server, spoken over HTTP on
//! loopback, served by the running desktop app. An MCP client - Claude Code,
//! or Claude inside an IDE - points at `http://127.0.0.1:7422/mcp` with a
//! bearer token and gets a set of tools: what servers exist, what
//! applications are on them, what a log says, and - only where that has been
//! allowed - restart one.
//!
//! **Why the app serves it rather than a separate binary.** A standalone
//! command would have to open its own SSH session on every call and read the
//! same SQLite file the desktop already has open. The desktop is running
//! while somebody is working anyway, and it already holds live sessions, so
//! the door belongs on it. A stdio bridge for clients that cannot speak HTTP
//! is a thin thing to add later and needs nothing here to change.
//!
//! **What it deliberately cannot do.**
//!
//! - **It is never reachable off this machine.** The bind address is not a
//!   setting. "It is only my LAN" is how a listening socket that lists
//!   somebody's production servers ends up reachable by somebody else.
//! - **It carries no secrets.** Applications are read through
//!   `ApplicationRepository`, which already returns secret environment values
//!   as empty strings - the real ones exist only inside a runtime about to
//!   start something. Servers are returned without passwords, passphrases or
//!   key material; a private key is referenced by path in this app and the
//!   path is not sent either.
//! - **It changes nothing unless asked twice.** Turning the endpoint on and
//!   allowing changes are two settings, because "may Claude see my servers"
//!   and "may Claude restart them" are different questions with different
//!   right answers.
//!
//! **The token is not decoration.** Any process on this machine can reach a
//! loopback port, including a web page's own scripts in some configurations.
//! The token is generated once, kept in the OS keyring beside every other
//! VibeSSH secret, and required on every request.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::storage::credentials::{self, SecretKind};
use crate::storage::mcp_config::McpConfig;

/// The keyring namespace for secrets that belong to this installation rather
/// than to any one server - `store_secret` takes a `Uuid` as a namespace and
/// does not care that this one names no row.
const INSTALL_SCOPE: Uuid = Uuid::nil();

/// The version of MCP this speaks. Sent back from `initialize`; a client
/// that wants a different one is told what it is talking to rather than
/// being guessed at.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Handle on the running endpoint, so turning the setting off actually
/// closes the socket instead of leaving it open until the app exits.
#[derive(Default)]
pub struct McpState {
    running: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl McpState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

/// The token, made on first use and kept in the OS keyring.
///
/// Read back rather than regenerated on every start: an MCP client stores it
/// in its own configuration, and a token that changed on each launch would
/// be a feature that works until you restart VibeSSH.
pub fn token() -> AppResult<String> {
    if let Some(existing) = credentials::load_secret(INSTALL_SCOPE, SecretKind::McpToken)? {
        return Ok(existing);
    }
    let fresh: [u8; 32] = rand::random();
    let token = fresh.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    credentials::store_secret(INSTALL_SCOPE, SecretKind::McpToken, &token)?;
    Ok(token)
}

/// Throws the current token away and issues another.
///
/// The answer to "I pasted it into the wrong window". Every client
/// configured with the old one stops working, which is the point.
pub fn rotate_token() -> AppResult<String> {
    credentials::delete_secret(INSTALL_SCOPE, SecretKind::McpToken)?;
    token()
}

#[derive(Clone)]
struct Endpoint {
    app: tauri::AppHandle,
    token: String,
    allow_changes: bool,
}

/// Starts or stops the endpoint to match `config`.
///
/// Called on startup and whenever the setting changes, and safe to call with
/// the state it is already in - a request to enable what is already running
/// restarts it, which is what makes a changed port or a rotated token take
/// effect without an app restart.
pub async fn apply(app: &tauri::AppHandle, state: &McpState, config: McpConfig) -> AppResult<()> {
    stop(state).await;
    if !config.enabled {
        return Ok(());
    }

    let endpoint = Endpoint { app: app.clone(), token: token()?, allow_changes: config.allow_changes };
    let router = Router::new()
        .route("/mcp", post(handle))
        // Not an MCP tool, on purpose. This one carries a build artifact -
        // a plugin jar is megabytes - and MCP would mean base64 inside a
        // JSON-RPC envelope. Its caller is a build, not a model: a Gradle
        // task that has just produced a jar and wants it on the server.
        .route("/deploy", post(deploy).layer(axum::extract::DefaultBodyLimit::max(MAX_DEPLOY_BYTES)))
        // A GET, and the one route here that never ends on its own: it holds
        // the connection open and writes a line whenever the application
        // does, until the reader goes away.
        .route("/logs", axum::routing::get(logs))
        .with_state(endpoint);

    // Loopback, not a setting - see the module doc.
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|err| AppError::Connection(format!("couldn't open the local endpoint on {address}: {err} - is another program using port {}?", config.port)))?;

    let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
    *state.running.lock().await = Some(shutdown);
    tauri::async_runtime::spawn(async move {
        let served = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
        if let Err(err) = served {
            log::warn!("the local MCP endpoint stopped: {err}");
        }
    });
    log::info!("the local MCP endpoint is listening on http://{address}/mcp");
    Ok(())
}

pub async fn stop(state: &McpState) {
    if let Some(shutdown) = state.running.lock().await.take() {
        let _ = shutdown.send(());
    }
}

async fn handle(State(endpoint): State<Endpoint>, headers: HeaderMap, body: Json<Value>) -> Response {
    if !authorized(&headers, &endpoint.token) {
        // No detail: a caller who got the token wrong learns only that.
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }

    let id = body.get("id").cloned();
    let method = body.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = body.get("params").cloned().unwrap_or(Value::Null);

    // A notification carries no id and expects no answer - `initialized` is
    // the one every client sends, and replying to it is a protocol error.
    if id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let result = match protocol_response(method, endpoint.allow_changes) {
        Some(answer) => answer,
        // The only method that needs the application itself, and so the only
        // one that cannot be answered by a pure function.
        None => call_tool(&endpoint, &params).await,
    };

    match result {
        Ok(value) => Json(json!({ "jsonrpc": "2.0", "id": id, "result": value })).into_response(),
        // Reported as a JSON-RPC error rather than an HTTP one: the request
        // was understood and answered, and a client that sees a 500 tends to
        // retry rather than show the reason.
        Err(err) => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32000, "message": err.to_string() },
        }))
        .into_response(),
    }
}

/// The largest thing `/deploy` will accept.
///
/// Generous because the realistic payload is a shaded jar, and mean enough
/// that a mistake - pointing the task at a directory archive, say - fails at
/// the door instead of after two minutes of SFTP.
const MAX_DEPLOY_BYTES: usize = 128 * 1024 * 1024;

// Checked when the crate is built rather than when its tests run: this is a
// constant, so the question "is it still sane?" has an answer at compile
// time and a test asserting it would only ever be true.
const _: () = assert!(MAX_DEPLOY_BYTES >= 64 * 1024 * 1024, "a shaded jar is tens of megabytes");
const _: () = assert!(MAX_DEPLOY_BYTES <= 512 * 1024 * 1024, "this is a plugin, not a disk image");

#[derive(serde::Deserialize)]
struct DeployParams {
    application: Uuid,
    /// Where to put it, relative to the application's working directory -
    /// `plugins/myplugin.jar`. Not validated here: both file providers
    /// already refuse anything that escapes that directory, and there is a
    /// test for it at the service layer. A second copy of that rule here
    /// would be a second thing to keep right.
    path: String,
    #[serde(default)]
    restart: bool,
}

/// Puts a freshly built file onto a server and, if asked, restarts what
/// runs it.
///
/// **The whole point of the IntelliJ story.** `./gradlew build` produces a
/// jar; this is how it gets to `plugins/` and the server comes back with it
/// loaded, without leaving the editor.
///
/// Behind `allow_changes` like every other tool that writes: this one both
/// writes to a server and can restart it, which is further than reading a
/// log.
async fn deploy(State(endpoint): State<Endpoint>, headers: HeaderMap, params: axum::extract::Query<DeployParams>, body: axum::body::Bytes) -> Response {
    if !authorized(&headers, &endpoint.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    if !endpoint.allow_changes {
        return (
            StatusCode::FORBIDDEN,
            "changes from the local endpoint are switched off - turn on 'Allow changes' in VibeSSH's settings",
        )
            .into_response();
    }

    match deploy_inner(&endpoint, &params, &body).await {
        Ok(value) => Json(value).into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn deploy_inner(endpoint: &Endpoint, params: &DeployParams, body: &[u8]) -> AppResult<Value> {
    use tauri::Manager;

    let app_repo = endpoint.app.state::<crate::storage::application_repository::ApplicationRepository>();
    let server_repo = endpoint.app.state::<crate::storage::server_repository::ServerRepository>();
    let sessions = endpoint.app.state::<crate::state::SshSessionManager>();

    crate::services::write_application_file(&app_repo, &server_repo, &sessions, params.application, &params.path, body).await?;
    log::info!("the local endpoint wrote {} bytes to {} of application {}", body.len(), params.path, params.application);

    if !params.restart {
        return Ok(json!({ "bytes": body.len(), "path": params.path, "restarted": false }));
    }

    let local = endpoint.app.state::<Arc<crate::runtime::local_process::LocalProcessManager>>();
    let status = crate::services::restart_application(&app_repo, &server_repo, &sessions, &local, params.application).await?;
    Ok(json!({ "bytes": body.len(), "path": params.path, "restarted": true, "status": format!("{status:?}") }))
}

#[derive(serde::Deserialize)]
struct LogParams {
    application: Uuid,
    /// How much history to send before going live, so a console that has
    /// just opened is not staring at nothing until the server next speaks.
    #[serde(default = "default_tail")]
    tail: u32,
}

fn default_tail() -> u32 {
    200
}

/// Follows an application's output for as long as the caller keeps reading.
///
/// **Plain lines, not an event format.** The reader is an IDE console
/// appending text; server-sent events would mean framing on this side and
/// unframing on the other for no gain. One line per line, and the connection
/// ending is the end.
///
/// **Reading, so it needs no permission to change.** Following a log is the
/// same act as `application_logs`, held open - it writes nothing and
/// restarts nothing.
///
/// **The follow stops when the reader goes.** `FollowHandle` stops the
/// remote `docker logs -f` when it is dropped, so it is moved into the
/// stream: an IDE that closes the console, or a laptop that sleeps, ends the
/// process on the server rather than leaving it attached forever.
async fn logs(State(endpoint): State<Endpoint>, headers: HeaderMap, params: axum::extract::Query<LogParams>) -> Response {
    use tauri::Manager;

    if !authorized(&headers, &endpoint.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }

    let app_repo = endpoint.app.state::<crate::storage::application_repository::ApplicationRepository>();
    let server_repo = endpoint.app.state::<crate::storage::server_repository::ServerRepository>();
    let sessions = endpoint.app.state::<crate::state::SshSessionManager>();
    let local = endpoint.app.state::<Arc<crate::runtime::local_process::LocalProcessManager>>();

    let (lines_tx, lines_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let closed_tx = lines_tx.clone();
    let follow = crate::services::follow_application_logs(
        &app_repo,
        &server_repo,
        &sessions,
        &local,
        params.application,
        params.tail,
        move |line| {
            let _ = lines_tx.send(line);
        },
        move |reason| {
            // Said in the stream rather than swallowed: a console that stops
            // filling looks the same as one whose application went quiet,
            // and those are very different situations.
            if let Some(reason) = reason {
                let _ = closed_tx.send(format!("--- VibeSSH: podglad zakonczony: {reason}"));
            } else {
                let _ = closed_tx.send("--- VibeSSH: podglad zakonczony".to_string());
            }
        },
    )
    .await;

    let handle = match follow {
        Ok(handle) => handle,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    // The handle rides along in the stream's state, so it lives exactly as
    // long as somebody is reading and not one moment longer.
    let stream = futures_util::stream::unfold((lines_rx, handle), |(mut rx, handle)| async move {
        let line = rx.recv().await?;
        Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(format!("{line}\n"))), (rx, handle)))
    });

    (
        [
            (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            // Nothing between here and the reader, but both are ways a proxy
            // or a client library decides to buffer a response until it ends
            // - and this one does not end.
            (axum::http::header::CACHE_CONTROL, "no-cache"),
            (axum::http::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

/// Everything a client asks for that does not need the application behind
/// it - `None` means "this is `tools/call`, go and do the work".
///
/// Split out so the shapes a client actually depends on can be asserted
/// without a running desktop: `initialize`'s answer is what an MCP client
/// reads before it will speak at all, and getting a field name wrong there
/// is a feature that fails at the handshake with nothing in any log.
fn protocol_response(method: &str, allow_changes: bool) -> Option<AppResult<Value>> {
    match method {
        "initialize" => Some(Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "vibessh", "version": env!("CARGO_PKG_VERSION") },
        }))),
        "tools/list" => Some(Ok(json!({ "tools": tool_definitions(allow_changes) }))),
        "tools/call" => None,
        other => Some(Err(AppError::InvalidInput(format!("unknown method: {other}")))),
    }
}

/// Constant-time enough for a 32-byte hex string compared on loopback: the
/// comparison is on length first, and a timing oracle over a local socket
/// against a value with 256 bits of entropy is not the way in.
fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers.get("authorization").and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(presented) = value.strip_prefix("Bearer ") else {
        return false;
    };
    presented.len() == expected.len() && presented.bytes().zip(expected.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// What the client is told it can do.
///
/// The changing tools are absent rather than present-and-refusing when
/// changes are not allowed: a tool a model can see is a tool it will try,
/// and an assistant that keeps proposing a restart it cannot perform is
/// worse than one that never offers.
fn tool_definitions(allow_changes: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "list_servers",
            "description": "List the servers VibeSSH manages: name, address, port and connection mode. No credentials of any kind are returned.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "list_applications",
            "description": "List the applications VibeSSH manages, with their runtime type, working directory, last known status and which server each runs on (absent means this computer).",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "application_logs",
            "description": "The most recent log lines from one application. Use list_applications first to get its id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "application_id": { "type": "string", "description": "The application's id, as given by list_applications." },
                    "lines": { "type": "integer", "description": "How many lines from the end. Defaults to 100, capped at 1000." },
                },
                "required": ["application_id"],
            },
        }),
    ];
    if allow_changes {
        tools.push(json!({
            "name": "restart_application",
            "description": "Restart one application. Only available because the user has allowed changes from this endpoint.",
            "inputSchema": {
                "type": "object",
                "properties": { "application_id": { "type": "string" } },
                "required": ["application_id"],
            },
        }));
    }
    tools
}

async fn call_tool(endpoint: &Endpoint, params: &Value) -> AppResult<Value> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    let text = match name {
        "list_servers" => tools::list_servers(&endpoint.app)?,
        "list_applications" => tools::list_applications(&endpoint.app)?,
        "application_logs" => tools::application_logs(&endpoint.app, &arguments).await?,
        "restart_application" if endpoint.allow_changes => tools::restart_application(&endpoint.app, &arguments).await?,
        "restart_application" => {
            return Err(AppError::InvalidInput(
                "changes from the local endpoint are switched off - turn on 'Allow changes' in VibeSSH's settings to permit this".into(),
            ))
        }
        other => return Err(AppError::InvalidInput(format!("unknown tool: {other}"))),
    };

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

mod tools {
    use super::*;
    use tauri::Manager;

    fn application_id(arguments: &Value) -> AppResult<Uuid> {
        let raw = arguments
            .get("application_id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::InvalidInput("application_id is required".into()))?;
        Uuid::parse_str(raw).map_err(|_| AppError::InvalidInput(format!("'{raw}' is not an application id - list_applications gives them")))
    }

    /// Deliberately assembled field by field rather than serialising
    /// `Server` wholesale. A future field holding something private would
    /// otherwise start being sent the day somebody adds it, with nothing
    /// here changed and no test failing.
    pub fn list_servers(app: &tauri::AppHandle) -> AppResult<String> {
        let repo = app.state::<crate::storage::server_repository::ServerRepository>();
        let servers = crate::services::list_servers(&repo)?;
        let rows: Vec<Value> = servers
            .iter()
            .map(|server| {
                json!({
                    "id": server.id,
                    "name": server.name,
                    "host": server.host,
                    "port": server.ssh_port,
                    "username": server.username,
                })
            })
            .collect();
        serde_json::to_string_pretty(&rows).map_err(|err| AppError::Internal(err.to_string()))
    }

    /// Same rule as `list_servers`, and one more that matters here: the
    /// environment is not included at all. A secret row would come back
    /// empty anyway (see `EnvironmentVariable::value`), but a plain row can
    /// hold an address or a licence key somebody did not mark secret, and
    /// this tool has no business being the thing that publishes it.
    pub fn list_applications(app: &tauri::AppHandle) -> AppResult<String> {
        let repo = app.state::<crate::storage::application_repository::ApplicationRepository>();
        let applications = crate::services::list_applications(&repo)?;
        let rows: Vec<Value> = applications
            .iter()
            .map(|application| {
                json!({
                    "id": application.id,
                    "name": application.name,
                    "blueprint": application.blueprint_id,
                    "runtime": application.runtime_type,
                    "working_directory": application.working_directory,
                    "status": application.status,
                    "server_id": application.server_id,
                })
            })
            .collect();
        serde_json::to_string_pretty(&rows).map_err(|err| AppError::Internal(err.to_string()))
    }

    pub async fn application_logs(app: &tauri::AppHandle, arguments: &Value) -> AppResult<String> {
        let id = application_id(arguments)?;
        // Capped rather than trusted: a model asking for a million lines
        // would otherwise pull a million lines over SSH and post them into
        // its own context.
        let lines = arguments.get("lines").and_then(Value::as_u64).unwrap_or(100).clamp(1, 1000) as u32;

        let repo = app.state::<crate::storage::application_repository::ApplicationRepository>();
        let servers = app.state::<crate::storage::server_repository::ServerRepository>();
        let sessions = app.state::<crate::state::SshSessionManager>();
        let local = app.state::<Arc<crate::runtime::local_process::LocalProcessManager>>();
        let capture = app.state::<crate::storage::log_capture::LogCaptureStore>();

        let lines = crate::services::application_logs(&repo, &servers, &sessions, &local, &capture, id, lines).await?;
        Ok(lines.join("\n"))
    }

    pub async fn restart_application(app: &tauri::AppHandle, arguments: &Value) -> AppResult<String> {
        let id = application_id(arguments)?;
        let repo = app.state::<crate::storage::application_repository::ApplicationRepository>();
        let servers = app.state::<crate::storage::server_repository::ServerRepository>();
        let sessions = app.state::<crate::state::SshSessionManager>();
        let local = app.state::<Arc<crate::runtime::local_process::LocalProcessManager>>();

        let status = crate::services::restart_application(&repo, &servers, &sessions, &local, id).await?;
        // Said out loud in the log as well: a change made from outside the
        // interface should be visible to whoever is sitting in front of it.
        log::info!("the local MCP endpoint restarted application {id}");
        Ok(format!("restarted; status is now {status:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_str(value).unwrap());
        headers
    }

    /// The token is the only thing between this endpoint and every other
    /// process on the machine, so each of these is a way in if it passes.
    #[test]
    fn nothing_but_the_exact_token_is_authorized() {
        let expected = "a".repeat(64);
        assert!(authorized(&headers(&format!("Bearer {expected}")), &expected));

        assert!(!authorized(&HeaderMap::new(), &expected), "no header at all");
        assert!(!authorized(&headers(""), &expected), "empty header");
        assert!(!authorized(&headers(&expected), &expected), "the scheme is not optional");
        assert!(!authorized(&headers("Bearer "), &expected), "an empty token");
        assert!(!authorized(&headers(&format!("bearer {expected}")), &expected), "the scheme is case-sensitive here");
        assert!(!authorized(&headers(&format!("Bearer {}", "a".repeat(63))), &expected), "a prefix of the token");
        assert!(!authorized(&headers(&format!("Bearer {expected}x")), &expected), "the token plus something");
        assert!(!authorized(&headers(&format!("Bearer {}b", "a".repeat(63))), &expected), "one byte different");
    }

    /// A tool a model can see is a tool it will try. Hiding the changing
    /// ones is what stops an assistant proposing a restart that will be
    /// refused, over and over.
    #[test]
    fn the_changing_tools_are_absent_until_changes_are_allowed() {
        let names = |allow| {
            tool_definitions(allow)
                .iter()
                .map(|tool| tool["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };

        let read_only = names(false);
        assert!(read_only.contains(&"list_servers".to_string()));
        assert!(read_only.contains(&"application_logs".to_string()));
        assert!(!read_only.contains(&"restart_application".to_string()));

        let allowed = names(true);
        assert!(allowed.contains(&"restart_application".to_string()));
        // Everything the read-only set had is still there - allowing changes
        // adds, it does not swap one set for another.
        for name in &read_only {
            assert!(allowed.contains(name), "{name} disappeared once changes were allowed");
        }
    }

    /// The handshake every client performs before it will speak at all. A
    /// wrong field name here is a feature that fails silently at connect
    /// time, with nothing in any log to say why.
    #[test]
    fn initialize_answers_in_the_shape_a_client_reads() {
        let answer = protocol_response("initialize", false).expect("initialize is answered here").unwrap();
        assert_eq!(answer["protocolVersion"], PROTOCOL_VERSION);
        assert!(answer["capabilities"]["tools"].is_object(), "a client decides whether to ask for tools from this");
        assert_eq!(answer["serverInfo"]["name"], "vibessh");
        assert_eq!(answer["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    }

    /// `tools/call` is the one method that needs the running application, so
    /// it must fall through rather than being answered here - if it ever
    /// stopped doing so, every tool would silently do nothing.
    #[test]
    fn tools_call_is_the_only_method_left_to_the_application() {
        assert!(protocol_response("tools/call", false).is_none());
        assert!(protocol_response("initialize", false).is_some());
        assert!(protocol_response("tools/list", false).is_some());
    }

    /// Answered, not ignored: a client that mistypes a method gets told,
    /// rather than waiting for a reply that never comes.
    #[test]
    fn an_unknown_method_is_an_error_rather_than_silence() {
        let answer = protocol_response("tools/destroy_everything", true).expect("still answered");
        let err = answer.expect_err("an unknown method cannot succeed");
        assert!(err.to_string().contains("tools/destroy_everything"), "{err}");
    }

    /// `/deploy` writes to a server and can restart it, so it sits behind
    /// the same permission as the restarting tool. It is a separate route
    /// rather than an MCP tool, which is exactly how a check like this gets
    /// forgotten - hence a test rather than a comment.
    /// One handler's source, from its signature to the next item.
    ///
    /// Bounded by the next `fn` of any kind, not just the next `async fn`.
    /// The first version looked only for `async fn`, so a handler followed
    /// by a plain function swallowed everything up to the one after it -
    /// and the log test then failed on an `allow_changes` belonging to
    /// `call_tool`, which is the right place for one. Caught by the test
    /// it broke, which is the only reason this note exists.
    fn handler_source(name: &str) -> &'static str {
        let source = include_str!("mcp.rs");
        let start = source.find(name).unwrap_or_else(|| panic!("no handler called {name}"));
        let rest = &source[start + 1..];
        let end = [rest.find("\nasync fn "), rest.find("\nfn ")]
            .into_iter()
            .flatten()
            .min()
            .map(|at| start + 1 + at)
            .unwrap_or(source.len());
        &source[start..end]
    }

    #[test]
    fn deploy_is_behind_the_same_permission_as_restarting() {
        let body = handler_source("async fn deploy(");
        assert!(body.contains("authorized(&headers"), "deploy must check the token like every other route");
        assert!(body.contains("endpoint.allow_changes"), "deploy writes to a server - it cannot be gated as if it only read");
    }

    /// Following a log is reading, held open - it writes nothing and
    /// restarts nothing, so it must not sit behind the permission that
    /// exists for changing things. Asserted rather than commented because
    /// the two routes are next to each other and the gate is one line.
    #[test]
    fn following_a_log_needs_the_token_but_not_permission_to_change_things() {
        let body = handler_source("async fn logs(");
        assert!(body.contains("authorized(&headers"), "every route checks the token");
        assert!(!body.contains("allow_changes"), "reading a log is not a change and must not need that permission");
    }

    /// The remote `docker logs -f` has to stop when the reader goes away.
    /// The only thing that makes that true is the handle being owned by the
    /// stream, so that is what is asserted - a handle dropped early kills a
    /// live console, and one held forever leaks a process per open console.
    #[test]
    fn the_follow_handle_lives_exactly_as_long_as_the_stream() {
        let body = handler_source("async fn logs(");
        assert!(body.contains("unfold((lines_rx, handle)"), "the handle must be part of the stream's own state");
    }

    /// Every tool must declare a schema an MCP client can actually read; a
    /// tool with none is one the model will call with nothing.
    #[test]
    fn every_tool_declares_a_name_a_description_and_a_schema() {
        for tool in tool_definitions(true) {
            let name = tool["name"].as_str().expect("a tool without a name");
            assert!(!tool["description"].as_str().unwrap_or_default().is_empty(), "{name} has no description");
            assert_eq!(tool["inputSchema"]["type"], "object", "{name} has no object schema");
        }
    }
}
