//! Reading an Application's output, and writing to its console.
//!
//! Its own module because the merge between captured history and a live
//! window is subtle enough to deserve reading on its own - see
//! `merge_new_log_lines`, which had a real bug in how it detected the
//! overlap between the two.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::RuntimeType;
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::RuntimeContext;
use crate::services::ssh_service::retry_on_connection_failure;
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::server_repository::ServerRepository;

use super::*;

/// Starts a live follow of an Application's output.
///
/// Docker over SSH only, and that is deliberate rather than unfinished.
/// `docker logs -f` is a real follow the Node performs for us; a local
/// process has no equivalent to attach to after the fact, and systemd's
/// `journalctl -f` needs its own unit resolution. Anything else returns
/// `InvalidInput` and the console keeps polling, which is what it did
/// before this existed - the fallback is the old behaviour, not a failure.
///
/// The seeded tail comes from the same `--tail` the follow itself asks
/// for, so opening a console does not stare at nothing until the container
/// next says something.
pub async fn follow_application_logs(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    tail: u32,
    on_line: impl FnMut(String) + Send + 'static,
    on_closed: impl FnOnce(Option<String>) + Send + 'static,
) -> AppResult<crate::ssh::client::FollowHandle> {
    let (detail, connection, _runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("live output is only available for Docker applications".to_string()));
    }
    let connection = connection.ok_or_else(|| AppError::InvalidInput("live output needs a connection to the Node".to_string()))?;
    let container = format!("vibessh-app-{id}");
    connection.follow_container_logs(&container, tail, on_line, on_closed).await
}

/// The last `max_lines` lines available right now - a snapshot the Logs tab
/// fetches on open and on manual refresh, same "pull, not push" shape
/// `ContainerLogsPanel`'s existing `get_server_container_logs` already
/// uses. Not live-streamed - see `runtime::mod`'s own `LogProvider` doc
/// comment for why that's a pull-based API in the first place.
/// Merges a fresh live fetch into `log_capture` (see that module's own doc
/// comment for why this exists at all) before answering from the merged,
/// locally-persisted result rather than the live fetch directly - so a
/// Recreate's brand new, empty container log buffer never actually looks
/// empty to the user, and a briefly unreachable Node degrades to "whatever
/// was captured last time" instead of a hard error on a tab that's mostly
/// used to figure out *why* something just failed.
pub async fn application_logs(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    log_capture: &LogCaptureStore,
    id: Uuid,
    max_lines: u32,
) -> AppResult<Vec<String>> {
    let server_id = get_application(repo, id)?.application.server_id;
    let live_fetch = retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.logs(&ctx).await?.tail(max_lines).await
    })
    .await;

    if let Ok(live_lines) = live_fetch {
        let previous_tail = log_capture.tail(id, LOG_OVERLAP_ANCHOR_LINES).await?;
        let new_lines = merge_new_log_lines(&previous_tail, live_lines);
        log_capture.append(id, &new_lines).await?;
    }

    log_capture.tail(id, max_lines).await
}

/// How many already-captured lines are used to locate the overlap.
///
/// One line is not enough, which is what the previous implementation used.
/// Application logs repeat themselves constantly - a Minecraft server's
/// "Can't keep up!", a bot's reconnect notice, any periodic health line -
/// and matching a single repeated line against the *rightmost* occurrence
/// silently discarded every line between the true position and that last
/// occurrence. Matching a whole block of recent lines makes an accidental
/// match effectively impossible: it would take the same 32 consecutive
/// lines appearing twice.
const LOG_OVERLAP_ANCHOR_LINES: u32 = 32;

/// Finds where genuinely new output starts in a fresh live fetch.
///
/// Both inputs are *windows* onto the same stream: `previous_tail` is the
/// end of what has already been captured, `live_lines` is whatever
/// `docker logs --tail N` (or the equivalent) just returned. The new lines
/// are the part of `live_lines` that comes after wherever the two windows
/// overlap.
///
/// The overlap is found by matching the longest possible suffix of
/// `previous_tail` against a prefix of `live_lines`. Longest-first matters:
/// a shorter match can be a coincidence, the longest one is where the
/// windows genuinely line up.
///
/// No overlap at all - the first capture ever, or a container that was just
/// recreated and whose brand new buffer shares nothing with the old one -
/// means the whole live batch is new. It is appended after whatever is
/// already stored, so a Recreate only ever adds to history.
// `pub(super)` purely so the unit tests in `mod.rs` can reach it. The tests
// stayed in one place when this file was split - moving a thousand lines of
// shared setup into eight files would have been a second, larger change
// wearing the same commit.
pub(super) fn merge_new_log_lines(previous_tail: &[String], live_lines: Vec<String>) -> Vec<String> {
    let max_overlap = previous_tail.len().min(live_lines.len());
    for overlap in (1..=max_overlap).rev() {
        if previous_tail[previous_tail.len() - overlap..] == live_lines[..overlap] {
            return live_lines[overlap..].to_vec();
        }
    }
    live_lines
}

/// Sends one line of input to the application's stdin/console (a Minecraft
/// server's `say hello`, a generic process's own REPL, etc.) - `Ok(None)`
/// from `runtime.console` (a systemd unit with no stdin, see that trait
/// method's own doc comment) and a console that reports `supports_input() ==
/// false` are both surfaced as the same clear "read-only" error rather than
/// silently swallowing the keystrokes.
pub async fn application_console_write(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    input: &str,
) -> AppResult<()> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        let console = runtime
            .console(&ctx)
            .await?
            .ok_or_else(|| AppError::InvalidInput("this application has no interactive console".into()))?;
        if !console.supports_input() {
            return Err(AppError::InvalidInput("this application's console is read-only".into()));
        }
        console.write(input).await
    })
    .await
}
