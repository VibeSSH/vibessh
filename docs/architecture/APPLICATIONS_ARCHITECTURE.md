# Applications + Blueprints + Runtimes — architecture analysis (Phase 0)

Pre-implementation analysis for the Applications feature layer. No code
changes accompany this document - see the numbered "START" request this
answers. Findings are based on a full read of the relevant source (not
guessed): `src-tauri/src/{transport,ssh,agent_client,state,storage,
commands,services}`, `agent/src/*`, `protocol/src/*`, and the frontend
router/stores/navigation.

## 1. Executive summary — what the codebase actually supports today

Two findings shape every design decision below more than anything else in
this document, so they're stated up front rather than buried in Section 7.

**Finding A: `ServerConnection` (the abstraction the brief asks Runtime to
avoid re-inventing) already exists, but only one of its two intended
implementations does.** `src-tauri/src/transport/mod.rs` defines the trait
(`execute_command`, `get_metrics`, `list_processes`, the systemd six,
the Docker six, the SFTP five - 20 methods) with a doc comment naming
`SshTransport` and `AgentTransport` as its two implementations. Only
`impl ServerConnection for SshSession` (`ssh/transport.rs`) exists.
`agent_client/mod.rs`'s own doc comment: *"Not wired into `ServerConnection`
yet — that's Etap H."* This is a known, already-planned gap in the existing
codebase, not something this analysis discovered as a defect - but it
means every "Actions" command (`actions_commands.rs`), file command, and
monitor command today only works for `connectionMode: "ssh"` servers.
`ServerCard.tsx` hides Files/Monitor/Actions/Terminal buttons for
agent-mode cards specifically because of this (`{!isAgent && ...}`
throughout). **Agent-mode hosts currently do nothing after pairing except
show a live metrics push during the pairing flow itself.**

**Finding B: the Agent daemon has zero command-routing or process-spawning
capability, and this is a real gap, not a planned-but-unbuilt one.**
Exhaustive grep across `agent/` for `Child|spawn|tokio::process|Command::
new` returns nothing except async task spawns in its own test harness -
no `std::process::Command`, no `tokio::process`. `agent/Cargo.toml`'s
tokio doesn't even enable the `process` feature. The WebSocket connection
loop (`agent/src/transport/connection.rs`) only *sends* `Heartbeat` and
`MetricsUpdate`; every other `ServerEvent` variant (`ServiceUpdate`,
`ProcessUpdate`, `TerminalOutput`, `LogsLine`, `QuickActionProgress`) is
defined in `protocol/src/events.rs` but **nothing in the agent ever
constructs one**, and there is no message type at all for desktop→agent
commands beyond the initial handshake. The agent is a push-only metrics
source today.

**Consequence for this whole feature**: the brief's "jeśli Agent jest
dostępny, preferuj Agent-managed process" is the right long-term direction,
but Agent-managed process supervision requires *building the agent-side
process-management protocol from nothing* - new `ServerEvent`/desktop→agent
message types, a process supervisor inside `agent/src`, PID tracking that
survives agent restarts, log/console streaming over the WebSocket. That is
a substantial project in its own right, comparable in size to everything
else in this brief combined. Recommendation, expanded in Section 10: build
Remote support against **SSH first** (which already has everything needed
for both a request-response path and, importantly, a genuinely long-lived
streaming PTY path via `SshSession::open_terminal` - see Section 7.3), and
treat "Agent-managed process runtime" as a later, separately-scoped phase
once this foundation exists and is proven. This is exactly the kind of
limitation the brief's own Section 5 asks to be stated clearly rather than
hidden behind a naive implementation.

## 2. Reuse / Extend / New

### Usable without changes
- `ServerRepository` / SQLite migration framework (`storage/migrations.rs`,
  `rusqlite_migration`) - the pattern, not the schema; Applications gets
  its own tables via new `M::up(...)` entries appended to the same list.
- `SshSessionManager` - already caches one live authenticated `SshSession`
  per server id; Application commands against a remote server reuse it
  exactly as Actions/Monitor/Files do today (`get_or_connect`).
- `TerminalSessionManager` + `SshSession::open_terminal`'s background-task/
  `mpsc`/event-callback pattern - directly reusable *shape* for
  `ApplicationConsole` (Section 5.3).
- `ssh/systemd.rs` / `ssh/docker.rs`'s validation functions
  (`validate_unit_name`, `validate_container_ref`) and their
  no-shell-injection design - reused as-is for Application-managed units/
  containers, which use the exact same remote command surface.
- `AppError` (`errors/mod.rs`) - the `{kind, message}` serialization
  already exists; Section 9 proposes additive variants, not a parallel
  error type.
- Frontend design system in full: `Card`, `Button`, `IconButton`, `Badge`,
  `EmptyState`, `SkeletonRows`, modal system, `.page`/`.page-tabs` (added
  this session), design tokens from `globals.css`. Nothing here is Applications-
  specific enough to need a second system - see Section 8.4 and the "no
  second design system" constraint the brief also states.
- `ModulePicker` pattern (index route picks a server, `:id` route shows
  the module) - reused conceptually for Local vs Remote application
  creation, not literally reused as a component (Applications' own list
  page already shows both, per the brief's "grid/list" spec).

### Needs extension (not rewritten)
- `ServerConnection` trait: gains file-write-with-mkdir-parents and a
  streaming exec primitive it doesn't have yet (Section 7.3) - additive
  methods, existing ones untouched.
- `agent_client` / `protocol::events::ServerEvent`: eventually needs new
  variants for Agent-managed applications (Section 10, Phase "Agent
  process protocol", explicitly deferred past the brief's own Phase 3/4).
- `AgentCapabilities`: gains fields the frontend's capability-driven UI
  can key on once Agent-managed runtimes exist (not needed for the SSH-
  first phases).
- `AppState`/`lib.rs` `.manage()` list and `generate_handler!` list: every
  existing feature area is wired the same way (a `*Repository`/`*Manager`
  struct `.manage()`d, commands added to the handler list) - Applications
  follows the identical, already-established pattern, just adds entries.
- Frontend `serversStore`-adjacent pattern: a new `applicationsStore.ts`
  sibling, not a rework of the existing one (a `Server` and an
  `Application` are different domain objects - conflating them would be
  the "drugi równoległy system" the brief explicitly says not to build).

### New modules
Enumerated in full in Section 6 (directory structure) - headline new
pieces: `models/application.rs`, `storage/application_repository.rs`,
`runtime/` (trait + 3 initial implementations), `blueprint/` (schema +
built-in blueprints + validation), `commands/application_commands.rs`,
`services/application_service.rs`, frontend `src/pages/Applications.tsx` +
`ApplicationDetail.tsx` + a `src/components/applications/` tree mirroring
the `teams/` one added this session.

## 3. Dependencies between Applications and existing features

```
Application (new)
  ├─ server_id? ──────────────► Server (existing, server_repository)
  │                              nullable = Local application
  ├─ runtime_type ─┬─ LocalProcess  → NEW: src-tauri process management
  │                ├─ RemoteProcess → SshSession (existing) + NEW PTY-exec path
  │                ├─ Systemd       → SshSession::{list,restart,start,stop,
  │                │                  enable,disable}_service (existing,
  │                │                  extended with create/update/remove unit)
  │                └─ Docker        → SshSession::{list,restart,start,stop,
  │                                   remove}_container / container_logs
  │                                   (existing, extended with create)
  ├─ blueprint_id/version ────► Blueprint (new, versioned, built-in + custom)
  ├─ ApplicationConsole ──────► SshSession::open_terminal's pattern (existing
  │                              *pattern*, new *instance* keyed by
  │                              application_id not server_id)
  ├─ Files tab ────────────────► SftpFileProvider wraps existing
  │                              list/read/write/download/upload_remote_*
  │                              (existing), rooted at working_directory
  │                              (new: root-jailing, see Section 9)
  │                              LocalFileProvider is genuinely new (existing
  │                              Files module has never touched the local
  │                              filesystem - it's 100% SFTP today)
  └─ audit events ────────────► backend/src/audit.rs's pattern (existing,
                                 but that's the *cloud* team backend, a
                                 separate Postgres service - Applications
                                 is a *local* SQLite feature. See Section 9:
                                 Application audit events are local-only
                                 unless/until Applications become a
                                 team-shared resource, which is out of
                                 scope here.)
```

The important edge already flagged in the brief (Section 33) and confirmed
by reading `server_repository.rs::delete`: **deleting a `Server` today does
a bare `DELETE FROM servers WHERE id = ?1` with no foreign-key awareness of
anything else** (SQLite FKs aren't even enabled via `PRAGMA foreign_keys`
in the current connection setup - checked, absent). Once `applications.
server_id` references `servers.id`, this needs handling explicitly - see
Section 9.

## 4. Application domain model

```rust
// src-tauri/src/models/application.rs

pub enum ApplicationLocation { Local, Remote }  // derived, not stored: Local ⇔ server_id.is_none()

pub enum RuntimeType { LocalProcess, RemoteProcess, Systemd, Docker }

/// Last-known status, refreshed FROM the runtime - never the sole source of
/// truth for "is this actually running" (brief Section 2's explicit rule).
/// Persisted so the UI has something to show before the first live refresh
/// completes, not because it's trusted on its own.
pub enum ApplicationStatus { Unknown, Starting, Running, Stopping, Stopped, Failed }

pub struct Application {
    pub id: Uuid,
    pub server_id: Option<Uuid>,          // None = Local
    pub name: String,
    pub description: Option<String>,
    pub blueprint_id: String,
    pub blueprint_version: i32,
    pub runtime_type: RuntimeType,
    pub working_directory: String,
    pub status: ApplicationStatus,        // last known, see above
    pub last_status_check_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Everything else the brief lists (`environment`, `startupConfig`,
`resourceLimits`, `ports`, `metadata`) is **not** flattened onto this
struct - see Section 6's schema for why (query/update granularity: an
environment-variable edit shouldn't rewrite a JSON blob containing ports,
and vice versa - the brief's own Section 32 rule).

## 5. Runtime abstraction

### 5.1 The trait

```rust
// src-tauri/src/runtime/mod.rs

#[async_trait::async_trait]
pub trait ApplicationRuntime: Send + Sync {
    /// Checked before create/start - e.g. binary exists, directory
    /// writable, Docker actually present on this host. Distinct from
    /// health_check, which checks a *running* application.
    async fn validate(&self, ctx: &RuntimeContext) -> AppResult<()>;

    async fn start(&self, ctx: &RuntimeContext) -> AppResult<()>;
    /// `graceful`: false ⇒ this runtime's normal stop signal (SIGTERM /
    /// `systemctl stop` / `docker stop`); the UI's separate "Kill" action
    /// maps to `kill()` below, not `stop(graceful=false)`.
    async fn stop(&self, ctx: &RuntimeContext, graceful: bool) -> AppResult<()>;
    async fn restart(&self, ctx: &RuntimeContext) -> AppResult<()>;
    async fn kill(&self, ctx: &RuntimeContext) -> AppResult<()>;

    async fn status(&self, ctx: &RuntimeContext) -> AppResult<ApplicationStatus>;
    async fn resource_usage(&self, ctx: &RuntimeContext) -> AppResult<ResourceUsage>;
    async fn health_check(&self, ctx: &RuntimeContext) -> AppResult<HealthStatus>;

    /// None if this runtime/application doesn't support one (e.g. a
    /// systemd unit with no interactive stdin - brief Section: "Console").
    async fn console(&self, ctx: &RuntimeContext) -> AppResult<Option<Box<dyn ApplicationConsole>>>;
    async fn logs(&self, ctx: &RuntimeContext) -> AppResult<Box<dyn LogProvider>>;
}

pub struct ResourceUsage { pub cpu_percent: Option<f32>, pub ram_bytes: Option<u64>, pub uptime_seconds: Option<u64> }
pub enum HealthStatus { Healthy, Unhealthy(String), Unknown }

/// Everything a runtime call needs, resolved once per command rather than
/// each method re-deriving it: the Application row, its typed runtime
/// config, and (Remote only) the cached SshSession. Mirrors how
/// actions_commands.rs already resolves `(repo, sessions, server_id)` once
/// per command today.
pub struct RuntimeContext<'a> {
    pub application: &'a Application,
    pub config: &'a RuntimeConfig,        // enum, one variant per RuntimeType, Section 6
    pub connection: Option<Arc<SshSession>>,  // None for Local
}
```

Implementations: `LocalProcessRuntime`, `RemoteProcessRuntime`,
`SystemdRuntime`, `DockerRuntime` (all in `runtime/`, one file each). A
`fn runtime_for(application: &Application) -> Box<dyn ApplicationRuntime>`
factory is the **one and only place** that matches on `RuntimeType` -
exactly the brief's "nie chcę `if runtime == docker` rozsianego po całym
projekcie" requirement. Frontend never sees `RuntimeType` used for
branching either - see Section 8.4.

### 5.2 `ApplicationConsole`

```rust
#[async_trait::async_trait]
pub trait ApplicationConsole: Send + Sync {
    async fn write(&self, input: &str) -> AppResult<()>;
    fn close(&self);
    /// false for a read-only fallback (systemd unit with no stdin, or a
    /// Docker container started without -i) - UI must disable the input
    /// box and say so, never silently swallow keystrokes.
    fn supports_input(&self) -> bool;
}
```

Registered in a new `ApplicationConsoleManager` (`Mutex<HashMap<Uuid /*
application_id */, ConsoleHandle>>`), structurally identical to
`TerminalSessionManager` but keyed by `application_id`. Output streams
over a per-application Tauri event (`application://{id}/console-output`),
same naming convention as `terminal://{id}/output`.

### 5.3 Concrete runtime designs

**`LocalProcessRuntime`** — `tokio::process::Command` (needs the `process`
tokio feature enabled, currently absent - see Section 11). Spawns with
piped stdin/stdout/stderr, `tokio::spawn`s a pump task per stream (mirrors
`open_terminal`'s background-task shape), tracks `Child` + PID in a new
`LocalProcessManager` state struct. `resource_usage` needs a new
dependency - `sysinfo` (already used by the *agent* crate for the same
purpose; not currently a `src-tauri` dependency) - to read CPU/RAM for an
arbitrary local PID cross-platform. Graceful stop is OS-specific: POSIX
`SIGTERM` via `nix`/`libc`, Windows has no signal equivalent - closing
stdin and waiting, then falling back to `kill()` (`Child::kill()`,
cross-platform) after a timeout is the honest cross-platform "graceful"
approximation; this asymmetry must be documented in the UI copy, not
hidden (brief Section 21's "nie udawaj identycznych capabilities").

**`RemoteProcessRuntime`** (SSH, since Agent-managed doesn't exist - see
Finding B) — the brief explicitly warns against a naive
`execute_command()`-per-request approach. Two real options surveyed:

  - *nohup + PID + FIFO* (recommended default): `nohup <command> >
    <workdir>/.vibessh-app.log 2>&1 & echo $!` captures a PID that survives
    the SSH session's own disconnect (that's what `nohup` is for); a named
    pipe (`mkfifo <workdir>/.vibessh-app.stdin`) started as the process's
    stdin lets later SSH sessions send console input via `echo "cmd" >
    fifo`. Logs/console output follow via a long-lived `tail -f` exec
    channel (same request shape as `container_logs`, just never-ending -
    reuses `SshSession`'s channel machinery, doesn't need `open_terminal`'s
    PTY specifically). **Real limitations to state in the UI, not hide**:
    no true interactive TTY (a program that behaves differently when it
    detects a TTY vs a pipe, e.g. disables color output, will notice); the
    PID file can go stale if the host reboots without VibeSSH knowing;
    orphan cleanup on `Application` delete needs an explicit `kill -9
    $(cat pidfile)` step.
  - *tmux* (documented alternative, not built first): stronger fit for a
    "reattach and see full scrollback" UX, but requires tmux installed on
    the remote host (not guaranteed) and scrollback capture needs `tmux
    pipe-pane`, which is real but adds a second moving part. Worth
    revisiting once nohup+FIFO's limitations are felt in practice, not
    before.

  Both are inherently more limited than SSH's own `open_terminal` PTY
  path, which genuinely could run the target command directly (`channel.
  exec(true, command)` with a PTY requested) for as long as the *desktop
  app* stays open and the SSH connection holds - this is actually the
  *better* UX for "watch it start up and use the console right now" but
  does **not** survive an app restart, unlike nohup+PID. Recommendation:
  offer the PTY-backed path as the live "attach console" experience while
  the app is open, backed by the nohup+PID+FIFO row for persistence -
  attaching writes into the same FIFO instead of a fresh PTY once the
  process is already running detached. This needs prototyping in Phase 3,
  not fully committed to on paper.

**`SystemdRuntime`** — extends `ssh/systemd.rs` (list/start/stop/restart/
enable/disable already exist and are reused verbatim) with unit
create/update/remove, which don't exist yet. **Critical constraint found
in `docs/security/agent-privileges.md`, directly relevant here**: Agent-mode
systemd is gated by a root-owned `managed-units.conf` allowlist with *no
automated way to add an entry yet* ("an admin edits it by hand... an
intentional v1 limitation"). SSH-mode has no such gate (runs as whatever
user authenticated, full privileges of that user) - so `SystemdRuntime`
for Applications must go through **SSH only** for the initial phases,
exactly like the existing systemd module does; Agent-managed
`SystemdRuntime` inherits the same "needs the allowlist automation built
first" blocker as Quick Actions already has, unrelated to this feature.
Managed units are named `vibessh-app-<uuid>.service` (brief's own
suggestion) so they're unambiguously VibeSSH-owned and never collide with
or accidentally touch a pre-existing unit (brief Section 6's explicit
rule) - `validate_unit_name` already accepts this shape unchanged.

**`DockerRuntime`** — extends `ssh/docker.rs` (list/start/stop/restart/
remove/logs already exist) with `docker create`/`docker run`. **Same kind
of constraint as systemd, worth deciding now rather than deferring again**:
`docs/security/agent-privileges.md` explicitly deferred Agent's `docker` group
membership because granting it "before any Docker feature exists to use
it" would hand out root-equivalent access speculatively. That feature now
exists (this brief). Recommendation: **keep Docker SSH-only for this
phase** too (reuses the SSH user's own already-granted docker group
membership, same accountability model already established for existing
Docker Quick Actions) and treat "Agent may manage Docker" as its own
explicit decision for later, made with a real feature in hand rather than
on spec - matching the doc's own reasoning almost exactly. This is a
decision this document is flagging for confirmation, not silently making;
see Section 11.

## 6. Persistence / migrations

New migration (`M::up`, appended - never edit an existing shipped one, per
`storage/migrations.rs`'s own rule):

```sql
CREATE TABLE applications (
    id                  TEXT PRIMARY KEY,
    server_id           TEXT REFERENCES servers(id) ON DELETE RESTRICT,
    name                TEXT NOT NULL,
    description         TEXT,
    blueprint_id        TEXT NOT NULL,
    blueprint_version   INTEGER NOT NULL,
    runtime_type        TEXT NOT NULL,
    working_directory   TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'unknown',
    last_status_check_at TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
-- ON DELETE RESTRICT (not CASCADE, not SET NULL): a Server with
-- applications attached must not be silently deletable - see Section 9,
-- this is the brief's Section 33 rule enforced at the schema level, not
-- just in application code that could be bypassed.

CREATE TABLE application_environment (
    application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    key             TEXT NOT NULL,
    value           TEXT NOT NULL,
    PRIMARY KEY (application_id, key)
);

CREATE TABLE application_ports (
    id              TEXT PRIMARY KEY,
    application_id  TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    protocol        TEXT NOT NULL,        -- 'tcp' | 'udp'
    bind_address    TEXT NOT NULL,
    internal_port   INTEGER NOT NULL,
    external_port   INTEGER,              -- Docker host-mapped port; NULL elsewhere
    required        BOOLEAN NOT NULL DEFAULT 0,  -- blueprint-declared, not user-removable
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- Runtime-specific config: genuinely variable shape per RuntimeType (JVM
-- flags vs. Docker image/volumes vs. systemd ExecStart/user) - this is the
-- brief's own "JSON może być użyty tam, gdzie ma sens" case, not a
-- shortcut. One row per application (not per-field columns, since the
-- field set differs entirely by runtime_type).
CREATE TABLE application_runtime_config (
    application_id  TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
    config_json     TEXT NOT NULL
);

-- Blueprint-specific / UI-specific extra data (e.g. Paper's detected
-- server.jar version) that doesn't belong in the domain model - also
-- genuinely variable per blueprint, also JSON for the same reason.
CREATE TABLE application_metadata (
    application_id  TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
    metadata_json   TEXT NOT NULL
);

-- A MySQL/MariaDB engine VibeSSH can provision databases on - almost
-- always the same VibeSSH-managed Server the application itself runs on
-- (server_id set), matching the Pterodactyl reference screenshot's own
-- 127.0.0.1:3306 pattern (the DB engine bound to loopback on that host,
-- not exposed publicly). server_id nullable so a shared/external DB host
-- (not itself a VibeSSH server) stays representable.
CREATE TABLE database_hosts (
    id               TEXT PRIMARY KEY,
    server_id        TEXT REFERENCES servers(id) ON DELETE RESTRICT,
    name             TEXT NOT NULL,
    engine           TEXT NOT NULL,          -- 'mysql' | 'mariadb' (wire-compatible, same provisioning path)
    host             TEXT NOT NULL,          -- as reachable from THIS host's own shell, e.g. "127.0.0.1"
    port             INTEGER NOT NULL DEFAULT 3306,
    admin_username   TEXT NOT NULL,          -- CREATE DATABASE/CREATE USER/GRANT privileges
    phpmyadmin_application_id TEXT REFERENCES applications(id) ON DELETE SET NULL,
    -- ^ set once a phpMyAdmin Blueprint instance is deployed for this host
    -- (Section 12.3) - NULL until then, never a manually-typed external URL
    -- (see Section 12.3 for why).
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
-- admin_password lives in the OS keyring (storage::credentials, new
-- SecretKind::DatabaseHostAdmin), same as every other secret in this
-- codebase - never this row.

CREATE TABLE application_databases (
    id                TEXT PRIMARY KEY,
    application_id    TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    database_host_id  TEXT NOT NULL REFERENCES database_hosts(id) ON DELETE RESTRICT,
    database_name     TEXT NOT NULL,         -- generated, prefixed per application (Section 12.1)
    username          TEXT NOT NULL,         -- generated, scoped to only this database
    connections_from  TEXT NOT NULL,         -- bind pattern, e.g. "%" or a specific host
    created_at        TEXT NOT NULL,
    UNIQUE(database_host_id, database_name)
);
-- Same rule as before (brief Section 28): no password column here either -
-- the generated user's password lives in the OS keyring, keyed by this
-- row's own id, shown to the UI once via the same reveal-on-demand pattern
-- already built for Rail's host-masking this session (Section 12.2).

CREATE TABLE blueprints (
    id               TEXT PRIMARY KEY,   -- e.g. "paper"
    schema_version    INTEGER NOT NULL,
    blueprint_version INTEGER NOT NULL,
    definition_json   TEXT NOT NULL,     -- the full parsed+validated Blueprint, Section 7
    is_builtin        BOOLEAN NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE TABLE custom_blueprints (
    id               TEXT PRIMARY KEY,
    source_path      TEXT,               -- where it was imported from, for re-import/update
    imported_at      TEXT NOT NULL,
    FOREIGN KEY (id) REFERENCES blueprints(id) ON DELETE CASCADE
);
```

Ports get their own table (not JSON) specifically because the brief's own
Ports tab needs real per-row CRUD + collision validation queries (`SELECT
... WHERE application_id != ? AND internal_port = ? AND bind_address =
?`) - exactly the "nie twórz jednej kolumny JSON, jeśli dane będą często
query/update" rule. Environment variables likewise get a real table (per-
key edit/delete without rewriting a blob, and so a future "reveal this one
secret-looking value" UI doesn't need to parse JSON client-side).

## 7. Integration with existing modules

### 7.1 Systemd / Docker
Already covered in Section 5.3 - `SystemdRuntime`/`DockerRuntime` call the
existing `SshSession` inherent methods directly, adding only
create/update/remove-unit and `docker create`/`run`. No changes to
`ssh/systemd.rs`'s or `ssh/docker.rs`'s existing functions.

### 7.2 SFTP / Files
`FileProvider` trait (brief's own Section 13):
```rust
#[async_trait::async_trait]
pub trait FileProvider: Send + Sync {
    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>>;
    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>>;
    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()>;
    async fn create_directory(&self, path: &str) -> AppResult<()>;
    async fn download_file(&self, remote_path: &str, local_path: &Path) -> AppResult<()>;
    async fn upload_file(&self, local_path: &Path, remote_path: &str) -> AppResult<()>;
}
```
`SftpFileProvider` wraps an `Arc<SshSession>` and delegates to the exact
methods `ServerConnection` already has - genuinely a thin adapter, not a
reimplementation. `LocalFileProvider` is **new work**, since the existing
Files module has never touched the local filesystem (100% SFTP today) -
implemented directly against `tokio::fs`, same method surface. Both are
**rooted**: every path argument is joined against and validated to stay
under `application.working_directory` before use (brief's explicit "Files
ma być rootowane" rule) - this is new validation logic, not present
anywhere today since the existing Files module already trusts the whole
remote filesystem is fair game (it's a host-level tool, not scoped to one
app). `AgentFileProvider` is a stub returning
`AppError::Connection("Agent-managed file access isn't implemented yet")`
until Finding B's agent-protocol work lands - never silently falls back to
something else.

### 7.3 Terminal / Console
Covered in Section 5.2-5.3. The one true reuse opportunity: `SshSession::
open_terminal` already does everything a "console attached to a specific
long-running remote command" needs *except* let the caller specify the
initial command instead of a login shell. Proposed additive method:
```rust
// ssh/client.rs, alongside the existing open_terminal
pub async fn open_terminal_running(&self, command: &str, cols: u32, rows: u32, on_output: ..., on_closed: ...) -> AppResult<TerminalHandle>
```
identical internals, `channel.exec(true, command)` with a PTY requested
instead of `channel.request_shell()` - existing `open_terminal` stays
untouched, this is a sibling, not a modification.

### 7.4 Monitoring
`resource_usage()` reuses `ssh/monitor.rs`'s existing `ps`-parsing
machinery filtered to the application's own PID (SSH/systemd/local
process) or `docker stats` (Docker) - no new remote-side mechanism needed
for Remote runtimes. Local needs `sysinfo` (new dependency, Section 11).

### 7.5 Capabilities
Blueprint `features`/`capabilities` (brief Section 8-9) are a **different
concept** from `AgentCapabilities` (host-level: does this host have
Docker/systemd at all) - related but not the same struct. Blueprint
capabilities describe what *this application* exposes in its UI (console,
players, plugins...); host `AgentCapabilities` gate what the Create
Application wizard even offers as a runtime choice (brief Section 16:
"Nie pokazuj Docker options, jeśli host nie posiada Docker"). Both feed the
same capability-driven tab renderer (Section 8.4) but from different
sources - Application tabs are `blueprint.features ∩ (host capabilities
the chosen runtime actually needs)`.

## 8. Local vs Remote architecture

`server_id: Option<Uuid>` is the entire distinction at the data layer - no
separate `LocalApplication`/`RemoteApplication` types, matching the
brief's explicit "Nie rób osobnego 'Local Server UI'... Ta sama Application
UI, inny provider/runtime" rule for the frontend, mirrored on the backend:

| | Local | Remote |
|---|---|---|
| `server_id` | `None` | `Some(server_id)` |
| Runtimes offered | `LocalProcess` (+ future local Docker, brief Section 16) | `RemoteProcess`, `Systemd`, `Docker` |
| `FileProvider` | `LocalFileProvider` | `SftpFileProvider` (or `AgentFileProvider` once built) |
| `RuntimeContext.connection` | `None` | `Some(Arc<SshSession>)` from `SshSessionManager` |
| Console | local PTY/pipes | SSH PTY-exec or nohup+FIFO (Section 5.3) |

The Create Application wizard's Step 1 ("Location") is the only place this
ever gets decided explicitly by the user; everything downstream reads
`server_id`.

## 9. Data integrity & security

- **Server deletion**: `ON DELETE RESTRICT` at the schema level (Section
  6) makes a server-with-applications undeletable by construction, not
  just by a UI check that could be bypassed by calling the Tauri command
  directly. `delete_server` (`server_service.rs`) needs a pre-check that
  turns the resulting SQLite constraint violation into a clear
  `AppError::InvalidInput` listing the dependent applications, rather than
  a raw SQLite error reaching the frontend (brief Section 37's "structured
  error" rule) - and the frontend's delete-server flow needs to surface
  "N applications depend on this server" with a path to deal with them
  first, per brief Section 33.
- **Command injection**: every runtime that builds a remote shell command
  from user input (startup command, JVM args, env values, Docker
  image/command) must validate/escape the same way `validate_unit_name`/
  `validate_container_ref` already do for their narrower cases. Startup
  commands specifically are the highest-risk surface (brief Section 34) -
  recommend never interpolating a raw startup-command string into `sh -c`
  server-side; instead pass it to `channel.exec()`/`docker run`'s own
  argv-style invocation where the SSH/Docker layer itself handles
  argument boundaries, and validate blueprint variable substitution
  (`{{jvm_flags}}` etc.) against an allowlist of safe characters per
  variable *type* (a `memory` variable is numeric only, a `file` variable
  is a path with no shell metacharacters, etc.) before substitution ever
  happens.
- **Path traversal**: `FileProvider` root-jailing (Section 7.2) - every
  path argument resolved and checked to stay under `working_directory`
  before any read/write/list, both for `../../etc/passwd`-style traversal
  and for absolute paths that happen to start elsewhere.
- **Blueprint installer steps** (brief Section 18): `download_file`
  needs a URL scheme allowlist (https only) and a destination-path check
  against the same root-jail; `run_command` installer steps are the
  biggest risk surface for a community/custom blueprint and should be
  opt-in-per-step-confirmed for anything not built-in, matching brief
  Section 31's "pokazuj permissions/operations przed instalacją" -
  built-in blueprints (Generic/Generic Java/Paper/Velocity) ship as Rust-
  validated data, not files a user could tamper with, so this gate matters
  most for the *custom* blueprint import path.
- **Docker/systemd privilege boundaries**: covered in Section 5.3 - both
  runtimes are SSH-only for now, a decision this document is surfacing for
  confirmation rather than assuming (Section 11).
- **RBAC hook, not RBAC**: the brief's `applications.*` permission list
  (Section 35) has no team/cloud-backend attachment point yet -
  Applications is a **local, per-device SQLite feature**, entirely
  separate from the cloud Team/RBAC backend built earlier this session.
  Recommendation: every `application_service.rs` function takes no
  identity/permission parameter today (matching every existing local
  command - `server_service`, `ssh_service` etc. have no such concept
  either), but is written as a thin function of `(repo, ..., args)` that a
  future authorization layer can wrap without restructuring - not
  literally threading an unused `actor_id` through now, since local
  VibeSSH has no multi-user concept to check against yet, but keeping the
  service layer thin enough that wrapping it later is mechanical.

## 10. Phased roadmap (adjusted from the brief's own Section 43, per Finding B)

The brief's phase order is sound except for Phase 3, which bundles
"SystemdRuntime" (straightforward extension of existing, working code)
with "remote process strategy" (genuinely new, higher-risk design). Splits
that out:

1. **Phase 1** — `Application` model, migrations, `ApplicationRuntime`
   trait (empty/stub impls compiling), repository CRUD + tests. No UI yet.
2. **Phase 2** — `LocalProcessRuntime` (incl. `sysinfo`+`tokio::process`
   deps), `LocalFileProvider`, local `ApplicationConsole`. Fully testable
   without any remote host.
3. **Phase 3a** — `SystemdRuntime` (extends existing `ssh/systemd.rs`).
4. **Phase 3b** — `RemoteProcessRuntime` via SSH (nohup+PID+FIFO +
   PTY-exec console, Section 5.3) - called out as its own phase since it's
   genuinely new design, not an extension, and should land with its
   documented limitations, not rushed alongside 3a.
5. **Phase 4** — `DockerRuntime` (extends existing `ssh/docker.rs`).
6. **Phase 5** — Blueprint schema + validation + Generic + Generic Java.
7. **Phase 6** — Create Application wizard (capability-driven per Section
   16 - by now every runtime it can offer actually works).
8. **Phase 7** — Application list/detail UI, capability-driven tabs
   (Section 8.4), Ports tab full CRUD + validation.
9. **Phase 8** — Paper blueprint.
10. **Phase 9** — Velocity blueprint.
11. **Phase 10** — Health checks, resource limits mapping (Section 21 -
    Docker/systemd/local each expose different real limits, UI shows only
    what's genuinely supported per runtime).
12. **Phase 11** — Database-link + deployment-target *foundation only*
    (schema + types, no working Databases feature - none exists yet to
    link to).
13. **Phase 12** — Full regression/security pass against Section 9's list.
14. **(New, not in the brief's list)** Agent process-management protocol -
    explicitly separate, undertaken only once 1-13 are stable and only if
    Agent-managed applications are still wanted; this is the piece Finding
    B shows doesn't exist at all yet and deserves its own scoping pass
    rather than being squeezed into Phase 3/4.

## 11. Open decisions this document is surfacing, not making silently

**Resolved (confirmed by the user):** Docker/systemd start SSH-only (item
1) and phpMyAdmin ships as VibeSSH's own deployed Blueprint, Option A
(Section 12.3) - both recorded here as decided, not just recommended.

1. ~~**Docker/systemd via Agent**~~ **Decided: SSH-only for now** (Section
   5.3) - matches `docs/security/agent-privileges.md`'s own earlier deferral of
   Agent's `docker` group membership as "root-equivalent access on spec."
   Revisit once Agent-managed applications become their own phase.
2. **`sysinfo` and `tokio` `process` feature** as new dependencies for
   `LocalProcessRuntime` (Section 5.3/10) - both are small, standard,
   already-used-elsewhere-in-this-workspace choices (`sysinfo` is already
   an `agent` crate dependency), not exotic additions.
3. **RemoteProcessRuntime's persistence mechanism**: nohup+PID+FIFO
   recommended over tmux for the first pass (Section 5.3) - open to
   revisiting once real usage surfaces its limitations.
4. **Naming**: `application_id`/`applications` table names throughout -
   no collision with the existing `vibessh-app-<uuid>.service` unit-naming
   suggestion from the brief itself, both use "app" as the VibeSSH-owned
   marker consistently.
5. ~~**phpMyAdmin approach**~~ **Decided: Option A** (Section 12.3) -
   VibeSSH deploys its own phpMyAdmin as a built-in Docker Blueprint,
   rather than linking to an externally-managed instance.

## 12. Application Databases (self-service, per the Pterodactyl reference)

Fleshes out Section 6's `database_hosts`/`application_databases` tables
into a real design: a user opens an application's Databases tab, clicks
"New Database", and gets a working MySQL/MariaDB database + scoped user
without ever touching a shell - matching the reference screenshot's UX,
restyled onto VibeSSH's own design system (Section 12.2).

### 12.1 Provisioning mechanism

**Not a direct MySQL-protocol connection from the desktop.** The
reference screenshot's endpoint (`127.0.0.1:3306`) is exactly why: that's
loopback *on the remote host*, invisible to the desktop's own network
stack - VibeSSH's desktop process cannot open a TCP connection to it
directly, SSH'd in or not. Two ways to actually reach it were considered:

- SSH port-forwarding (`direct-tcpip` channel, which `russh` supports) a
  local port through to the remote `database_hosts.host:port`, then
  speaking real MySQL protocol through that tunnel (would need a new
  `sqlx` `mysql` feature dependency, consistent with `backend`'s existing
  `sqlx`/Postgres usage). More moving parts (tunnel lifecycle, a second
  wire protocol) for marginal benefit over the option below.
- **Recommended: validated remote CLI execution**, the same philosophy
  `ssh/systemd.rs`/`ssh/docker.rs` already use - `SshSession::
  execute_command` running the `mysql`/`mariadb` client already present
  on any host that runs the database engine itself. `CREATE DATABASE`/
  `CREATE USER`/`GRANT`/`DROP DATABASE` statements are built from
  **generated, not user-typed**, identifiers (database name and username
  are always machine-generated - see 12.2 - so there's no free-text
  identifier to validate against SQL-identifier-injection in the first
  place, same "don't accept what you don't have to" reasoning as
  `validate_unit_name`). The admin password is never placed on the command
  line (visible via `ps` to anyone else on that host) - passed via a
  `--defaults-extra-file` written to a private temp file for the duration
  of the one call, or `MYSQL_PWD` scoped to that single `execute_command`
  invocation, then discarded either way.

New `DatabaseRuntime`-adjacent service (not an `ApplicationRuntime` impl
itself - provisioning a database isn't starting/stopping a process, it's
its own small CRUD-shaped module: `services/database_service.rs` +
`storage/database_repository.rs`, following the exact same layering as
everything else in this codebase):
```rust
pub async fn create_database(host: &DatabaseHost, application: &Application, connections_from: &str) -> AppResult<ApplicationDatabase>
pub async fn delete_database(host: &DatabaseHost, database: &ApplicationDatabase) -> AppResult<()>
pub async fn reset_password(host: &DatabaseHost, database: &ApplicationDatabase) -> AppResult<String> // returns the new password once
```

### 12.2 UI (VibeSSH design system, not Pterodactyl's)

New "Databases" tab (capability-gated per Section 7.5 - only shown when
the blueprint declares `features.databases: true`, matching Paper/
Velocity/Generic Java's tab lists from the original brief). Built entirely
from components that already exist in this codebase, not new ones:

- List: `.server-list`/`.server-list-item` rows (same primitive Teams'
  Servers/Roles tabs already use this session) - icon, name (`s1_rank`
  style, generated), a two-line `server-list-main` showing `host:port` as
  the primary line and `username` as the secondary line (mirrors
  `ServersSection`'s own `user@host:port` line exactly).
- Password: **hidden by default behind an eye-toggle**, revealed on
  click - literally the same `IconButton` + local `revealed` state pattern
  built for Rail's host-masking this session, not a new mechanism. No
  password ever sits in the DOM in plaintext until the user asks for it,
  same reasoning as that feature (a casual screenshot/screen-share
  shouldn't leak it).
- "Connections from" as a `Badge` (small, informational - `%` reads as
  "any host", a specific value as itself).
- Remove: `IconButton danger`, same confirmation-free direct-delete
  convention already established for Roles/Team-Servers rows this
  session (no modal, matches the existing pattern in this exact feature
  area rather than introducing a new confirmation style here alone).
- "New Database" button (`Button`, primary) opens the same lightweight
  inline-form pattern `ServersSection`/`InvitationsSection` already use
  (not a separate modal) - the only input is an optional free-text
  "purpose"/suffix for the generated name; everything else is generated.
- phpMyAdmin: a per-row `Button variant="secondary"` "Otwórz w
  phpMyAdmin" - present only when `database_hosts.
  phpmyadmin_application_id` is set (Section 12.3). Opens the deployed
  instance's URL in the system browser (`tauri-plugin-shell`'s `open`, a
  new but tiny dependency - `tauri-plugin-dialog` is already the same
  kind of official Tauri plugin already used in this codebase) with the
  database name pre-filled via phpMyAdmin's own `db=` query parameter
  where its config allows it; login itself still happens in phpMyAdmin's
  own form using the credentials shown in this tab - true SSO would need
  phpMyAdmin's `signon` auth mode configured against something, which is
  real added complexity for a v1 and is called out as explicitly out of
  scope for the first pass, not silently attempted and half-working.

### 12.3 phpMyAdmin: which of two real shapes

**Option A - VibeSSH deploys its own phpMyAdmin** as a built-in Docker
Blueprint (official `phpmyadmin/phpmyadmin` image) onto the *same* server
a `database_hosts` row lives on. This is what makes `database_hosts.host
= "127.0.0.1"` actually reachable *from phpMyAdmin's own container* on
that host (Docker's own bridge network or `--network host` reaches
loopback fine, unlike VibeSSH's desktop trying to reach it directly) -
technically coherent with everything else in this design, and it's just
another `Application` using machinery this whole feature already builds
(nothing phpMyAdmin-specific needed beyond the one Blueprint definition).
Cost: needs Docker on that host (brief Section 7's own constraint - not
every host has it), and is a real deployed thing an admin has to choose to
add (one click once Applications basics exist, not automatic).

**Option B - link out to an already-running phpMyAdmin** the admin
manages themselves (a config field: paste a base URL). Zero new
deployment machinery, but only actually usable if that external
phpMyAdmin can *itself* reach `database_hosts.host:port` - true when
phpMyAdmin lives on the same host/network as the database, coincidentally
often true for a simple single-VPS setup, not guaranteed in general. Also
means VibeSSH is trusting a URL it doesn't control or verify.

Both are legitimate; A is more self-contained and "VibeSSH-native" (fits
the Blueprint system exactly as designed) but costs a Docker dependency
and an extra deployed component. B is simpler to ship but only reliably
correct in the same narrow case A always handles - not a fallback for
every setup A would work on. Recommend **A now, with the
`phpmyadmin_application_id` column already designed for exactly that
(Section 6/12) and B not built at all initially** - a manually-pasted
external URL with no reachability guarantee is the kind of "half-working"
feature the brief's Section 39 asks not to ship.

**Decided: Option A.** `database_hosts.phpmyadmin_application_id` (Section
6) is exactly the column this needs - set once an admin deploys the
built-in phpMyAdmin Blueprint for that host, `NULL` (button hidden, per
Section 12.2's capability gate) until then.

## Next step

All open decisions (Section 11) are resolved. Phase 1 implementation
(Application model, migrations, `ApplicationRuntime` trait) begins next.
