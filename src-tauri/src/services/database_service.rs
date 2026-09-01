//! Application Databases (Phase 11's real implementation, on top of the
//! schema+types foundation in `models::database`/`storage::database_repository`)
//! - see docs/APPLICATIONS_ARCHITECTURE.md Section 12.1 for the full design
//! this follows. Provisioning never speaks the MySQL wire protocol
//! directly from the desktop (a `database_hosts.host` like `"127.0.0.1"` is
//! only reachable from *that host's own shell*, not from VibeSSH's own
//! network stack) - every provisioning call runs the `mysql` client already
//! present on the host, over the same `SshSession::execute_command` every
//! other Remote feature in this codebase already uses.
//!
//! `database_name`/`username` are always machine-generated
//! (`generate_identifier`), never taken from user input - there's no
//! free-text SQL identifier to validate against injection in the first
//! place, same "don't accept what you don't have to" stance
//! `ssh::systemd::validate_unit_name` already takes. The generated
//! password is also machine-generated and alphanumeric-only by
//! construction, so it never needs SQL-string-literal escaping either.

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationDatabase, CreateApplicationDatabaseInput, CreateDatabaseHostInput, DatabaseHost};
use crate::services::ssh_service::{get_or_connect, retry_on_connection_failure};
use crate::ssh::{write_private_file, SshSession};
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials::{self, SecretKind};
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::server_repository::ServerRepository;

const MAX_DATABASE_NAME_LEN: usize = 64;
// Conservative single-byte MySQL username limit (older MySQL/MariaDB use a
// 32-character `User` column; 8.0+ extends it to 128, but there's no way to
// detect which from here, so this targets the widest-compatible bound).
const MAX_USERNAME_LEN: usize = 32;
const RANDOM_SUFFIX_LEN: usize = 6;
const GENERATED_PASSWORD_LEN: usize = 24;
/// The host pattern a generated database user may connect from.
///
/// **This used to be `%` - connectable from anywhere on the internet.** A
/// generated user's only protection was then a 24-character password, on a
/// server this same module had just reconfigured to listen on every
/// interface. Narrowing it costs nothing: an Application reaching its
/// database is either a container (arriving over a Docker bridge, always
/// inside Docker's default `172.16.0.0/12` pool) or a plain process on the
/// Node itself (arriving over loopback). Nothing legitimate connects from
/// anywhere else.
///
/// MySQL host patterns are string wildcards, not CIDRs, so `172.%` is the
/// closest expressible form. It is wider than Docker's pool by the
/// `172.0.*`-`172.15.*` range, but narrower than `%` by the entire rest of
/// the internet, and it is paired with a bind address that does not accept
/// connections from outside the Node in the first place.
const CONNECTIONS_FROM: &str = "172.%";

/// Every host pattern a generated user is created for. `CONNECTIONS_FROM`
/// is the one recorded on the row (containers are the common case);
/// `localhost` covers an Application that runs as a plain process or a
/// systemd unit directly on the Node, which arrives over loopback.
fn grant_hosts() -> [&'static str; 2] {
    [CONNECTIONS_FROM, "localhost"]
}

// ---- Database hosts ----

pub fn list_database_hosts(repo: &DatabaseRepository) -> AppResult<Vec<DatabaseHost>> {
    repo.list_hosts()
}

pub fn create_database_host(repo: &DatabaseRepository, input: CreateDatabaseHostInput) -> AppResult<DatabaseHost> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("a name is required".into()));
    }
    let host = input.host.trim();
    if host.is_empty() {
        return Err(AppError::InvalidInput("a host is required".into()));
    }
    let admin_username = input.admin_username.trim();
    if admin_username.is_empty() {
        return Err(AppError::InvalidInput("an admin username is required".into()));
    }
    if input.admin_password.is_empty() {
        return Err(AppError::InvalidInput("an admin password is required".into()));
    }

    let created = repo.create_host(&CreateDatabaseHostInput {
        server_id: input.server_id,
        name: name.to_string(),
        engine: input.engine,
        host: host.to_string(),
        port: input.port,
        admin_username: admin_username.to_string(),
        admin_password: input.admin_password.clone(),
    })?;

    if let Err(err) = credentials::store_secret(created.id, SecretKind::DatabaseHostAdmin, &input.admin_password) {
        // The row can't be left behind referencing a password that was
        // never actually stored - best-effort compensating delete, same
        // "don't leave a half-created row around" reasoning
        // `create_application_database` below applies to its own rollback.
        let _ = repo.delete_host(created.id);
        return Err(err);
    }
    Ok(created)
}

/// `ON DELETE RESTRICT` (see `storage::migrations`) rejects this while any
/// `ApplicationDatabase` still references the host - surfaced by
/// `DatabaseRepository::delete_host` itself, nothing extra needed here.
pub fn delete_database_host(repo: &DatabaseRepository, id: Uuid) -> AppResult<()> {
    repo.delete_host(id)?;
    credentials::forget_secret(id, SecretKind::DatabaseHostAdmin);
    Ok(())
}

/// Links (or unlinks, `application_id: None`) the built-in phpMyAdmin
/// instance deployed for this host - see docs/APPLICATIONS_ARCHITECTURE.md
/// Section 12.3. No new Blueprint is needed for phpMyAdmin itself:
/// `blueprints::GenericDockerBlueprint` (image `phpmyadmin/phpmyadmin`,
/// `PMA_HOST`/`PMA_PORT` set through the Application's own existing
/// environment-variable step) already covers it - this just records which
/// already-deployed Application is that instance.
pub fn set_database_host_phpmyadmin(repo: &DatabaseRepository, id: Uuid, application_id: Option<Uuid>) -> AppResult<DatabaseHost> {
    repo.update_phpmyadmin_application(id, application_id)
}

// ---- Application databases ----

pub fn list_application_databases(repo: &DatabaseRepository, application_id: Uuid) -> AppResult<Vec<ApplicationDatabase>> {
    repo.list_databases(application_id)
}

/// `purpose` is the wizard's one free-text input (Section 12.2) - used only
/// to seed the generated name/username so they read as something
/// recognizable ("vibessh_myserver_a1b2c3"); an empty/missing purpose falls
/// back to the Application's own name. Everything else about the generated
/// identifiers/password is opaque and random.
pub async fn create_application_database(
    db_repo: &DatabaseRepository,
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    database_host_id: Uuid,
    purpose: Option<&str>,
) -> AppResult<ApplicationDatabase> {
    let host = load_host(db_repo, database_host_id)?;
    let application = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?.application;
    let admin_password = load_host_admin_password(&host)?;

    let seed = purpose.map(str::trim).filter(|p| !p.is_empty()).unwrap_or(&application.name);
    let database_name = generate_identifier(seed, MAX_DATABASE_NAME_LEN);
    let username = generate_identifier(seed, MAX_USERNAME_LEN);
    let password = generate_password();

    let mut sql = format!("CREATE DATABASE IF NOT EXISTS `{database_name}`; ");
    for grant_host in grant_hosts() {
        sql.push_str(&format!(
            "CREATE USER IF NOT EXISTS '{username}'@'{grant_host}' IDENTIFIED BY '{password}'; \
             GRANT ALL PRIVILEGES ON `{database_name}`.* TO '{username}'@'{grant_host}'; "
        ));
    }
    sql.push_str("FLUSH PRIVILEGES;");
    run_mysql_with_retry(server_repo, sessions, &host, &admin_password, &sql, &[&password]).await?;

    let record = match db_repo.create_database(&CreateApplicationDatabaseInput {
        application_id,
        database_host_id,
        database_name: database_name.clone(),
        username: username.clone(),
        connections_from: CONNECTIONS_FROM.to_string(),
    }) {
        Ok(record) => record,
        Err(err) => {
            // The row couldn't be saved (e.g. a freak random-suffix
            // collision) - the database/user just created on the remote
            // host would otherwise be orphaned (untracked, but real).
            // Best-effort undo rather than leaving that behind silently.
            let mut cleanup_sql = String::new();
            for grant_host in grant_hosts() {
                cleanup_sql.push_str(&format!("DROP USER IF EXISTS '{username}'@'{grant_host}'; "));
            }
            cleanup_sql.push_str(&format!("DROP DATABASE IF EXISTS `{database_name}`;"));
            if let Err(cleanup_err) = run_mysql_with_retry(server_repo, sessions, &host, &admin_password, &cleanup_sql, &[]).await {
                // The undo failed, so a real database and a real user with a
                // real password now exist on the host that nothing in
                // VibeSSH records - it will never appear in the UI and never
                // be dropped by any later teardown. Only the operator can
                // clear that, and only if they are told.
                log::error!("couldn't clean up the orphaned '{database_name}' database after a failed save: {cleanup_err}");
                return Err(AppError::Internal(format!(
                    "{err}. A '{database_name}' database and its user were also left behind on '{host_name}' and couldn't be removed \
                     automatically ({cleanup_err}) - drop them by hand before retrying",
                    host_name = host.host
                )));
            }
            return Err(err);
        }
    };

    // The database now exists both remotely and as a local row - storing
    // its password is the last step, not swallowed on failure: a rare
    // OS-keyring error here is surfaced as-is rather than reported as a
    // silent success with a password that could never be revealed again.
    credentials::store_secret(record.id, SecretKind::ApplicationDatabaseUser, &password)?;
    Ok(record)
}

pub async fn delete_application_database(
    db_repo: &DatabaseRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    id: Uuid,
) -> AppResult<()> {
    let database = load_database(db_repo, id)?;
    let host = load_host(db_repo, database.database_host_id)?;
    let admin_password = load_host_admin_password(&host)?;

    // Drops every host pattern the user could have been created for, plus
    // the one actually recorded on the row - a database created before
    // `grant_hosts` existed still carries the old `%` pattern, and leaving
    // that user behind would be an orphaned account with a live password.
    let mut sql = String::new();
    for grant_host in grant_hosts().iter().copied().chain(std::iter::once(database.connections_from.as_str())) {
        sql.push_str(&format!("DROP USER IF EXISTS '{}'@'{grant_host}'; ", database.username));
    }
    sql.push_str(&format!("DROP DATABASE IF EXISTS `{}`;", database.database_name));
    run_mysql_with_retry(server_repo, sessions, &host, &admin_password, &sql, &[]).await?;

    db_repo.delete_database(id)?;
    credentials::forget_secret(id, SecretKind::ApplicationDatabaseUser);
    Ok(())
}

/// The stored password, straight from the keyring - no SSH round trip,
/// matching the UI's "hidden by default, revealed on click" pattern
/// (Section 12.2): this is a local read, not a fresh probe of the database.
pub fn reveal_application_database_password(db_repo: &DatabaseRepository, id: Uuid) -> AppResult<String> {
    load_database(db_repo, id)?;
    credentials::load_secret(id, SecretKind::ApplicationDatabaseUser)?
        .ok_or_else(|| AppError::Internal(format!("database {id} has no stored password")))
}

pub async fn reset_application_database_password(
    db_repo: &DatabaseRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    id: Uuid,
) -> AppResult<String> {
    let database = load_database(db_repo, id)?;
    let host = load_host(db_repo, database.database_host_id)?;
    let admin_password = load_host_admin_password(&host)?;

    let new_password = generate_password();
    let mut sql = String::new();
    for grant_host in grant_hosts().iter().copied().chain(std::iter::once(database.connections_from.as_str())) {
        sql.push_str(&format!("ALTER USER IF EXISTS '{}'@'{grant_host}' IDENTIFIED BY '{new_password}'; ", database.username));
    }
    sql.push_str("FLUSH PRIVILEGES;");
    run_mysql_with_retry(server_repo, sessions, &host, &admin_password, &sql, &[&new_password]).await?;

    credentials::store_secret(id, SecretKind::ApplicationDatabaseUser, &new_password)?;
    Ok(new_password)
}

/// Builds the URL a "Open in phpMyAdmin" button opens in the system browser
/// (docs/APPLICATIONS_ARCHITECTURE.md Section 12.2) - `Err` when there's
/// nothing actually openable yet (no phpMyAdmin linked, or it has no
/// published port), rather than handing back a URL that would just fail to
/// load. `database_name` is optional and only pre-fills phpMyAdmin's own
/// `db=` query parameter where its own configuration allows it - login
/// itself always happens in phpMyAdmin's own form (Section 12.2 explicitly
/// scopes real SSO out of a first pass).
pub fn phpmyadmin_url(
    db_repo: &DatabaseRepository,
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    database_host_id: Uuid,
    database_name: Option<&str>,
) -> AppResult<String> {
    let host = load_host(db_repo, database_host_id)?;
    let Some(application_id) = host.phpmyadmin_application_id else {
        return Err(AppError::InvalidInput("no phpMyAdmin application is linked to this database host".into()));
    };
    let detail = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    // The first declared port with a published (external) port - a
    // phpMyAdmin application only ever declares the one web port, so
    // there's no ambiguity to resolve among several in practice.
    let Some(port) = detail.ports.iter().find_map(|p| p.external_port) else {
        return Err(AppError::InvalidInput("the linked phpMyAdmin application has no published port - add one on its Ports tab".into()));
    };
    let address = match detail.application.server_id {
        None => "127.0.0.1".to_string(),
        Some(server_id) => server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?.host,
    };

    let mut url = format!("http://{address}:{port}/");
    if let Some(name) = database_name {
        // Percent-encoded even though `generate_identifier` only ever
        // produces `[a-z0-9_]`. This value arrives from the frontend as a
        // free-form string, so "it is always machine-generated" is an
        // assumption about a caller rather than something this function can
        // see - and an unencoded `&` or `#` here silently truncates the
        // parameter rather than failing.
        url.push_str("?db=");
        url.push_str(&percent_encode_query_value(name));
    }
    Ok(url)
}

// ---- Shared helpers ----

/// Percent-encodes everything outside the unreserved set from RFC 3986.
/// Deliberately conservative - encoding a character that did not strictly
/// need it is harmless, missing one is not.
fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn load_host(db_repo: &DatabaseRepository, id: Uuid) -> AppResult<DatabaseHost> {
    db_repo.get_host(id)?.ok_or_else(|| AppError::NotFound(format!("database host {id}")))
}

fn load_database(db_repo: &DatabaseRepository, id: Uuid) -> AppResult<ApplicationDatabase> {
    db_repo.get_database(id)?.ok_or_else(|| AppError::NotFound(format!("database {id}")))
}

fn load_host_admin_password(host: &DatabaseHost) -> AppResult<String> {
    credentials::load_secret(host.id, SecretKind::DatabaseHostAdmin)?
        .ok_or_else(|| AppError::Internal(format!("database host {} has no stored admin password", host.id)))
}

/// `None` `server_id` (a shared/external DB host VibeSSH doesn't itself
/// manage) has no SSH connection VibeSSH can run commands through - a real,
/// documented limitation (Section 12.1's whole provisioning design is built
/// on `SshSession::execute_command`), not silently attempted and failing
/// somewhere deeper.
/// Resolves the host's connection and runs `sql` against it, retrying once
/// - with the cached session dropped first - if the connection turns out to
/// be dead (idle timeout, network blip). Same dead-cached-session recovery
/// `ssh_service::execute_command` already does for a single command,
/// generalized here since provisioning talks to `SshSession::execute_command`
/// through `run_mysql` (a `mysql` client invocation), not through that
/// plain-command wrapper directly.
async fn run_mysql_with_retry(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    host: &DatabaseHost,
    admin_password: &str,
    sql: &str,
    redact: &[&str],
) -> AppResult<()> {
    retry_on_connection_failure(sessions, host.server_id, || async {
        let connection = connect_to_host(server_repo, sessions, host).await?;
        ensure_mysql_client_installed(&connection).await;
        // Was: silently `apt-get install mariadb-server`, enable it, and
        // grant this host's admin user `ALL PRIVILEGES ... WITH GRANT
        // OPTION` - on any operation that happened to touch a loopback
        // host, with every error discarded (S-006/S-033). Installing a
        // database server and creating a superuser on it is not a
        // reasonable side effect of asking for an application database. Now
        // it is refused with a code the UI turns into an offer.
        require_database_server(&connection, host).await?;
        run_mysql(&connection, host, admin_password, sql, redact).await
    })
    .await
}

/// "Plug and play, no manual server prep" (the same standing bar
/// `server_service::install_docker` already meets for Docker) applies here
/// too: a Database Host's own MySQL/MariaDB *server* is always assumed
/// already running (its `host`/`port`/admin credentials are all supplied by
/// the user when they link the host - this module never provisions a
/// server), but the `mysql` *client* binary this whole flow shells out to
/// isn't guaranteed to be on a fresh Node's PATH just because a database
/// server is reachable from it. Probes first (`command -v`) so a Node that
/// already has it never pays for an `apt-get` round trip on every single
/// provisioning call. Best-effort and silent either way: if the install
/// fails (a non-apt distro, no network egress, whatever), the `mysql`
/// invocation right after this still runs and surfaces its own
/// "command not found" error same as before - this only ever removes that
/// error for the common case, never hides a real one.
async fn ensure_mysql_client_installed(connection: &SshSession) {
    let probe = connection.execute_command("command -v mysql >/dev/null 2>&1 && echo yes || echo no").await;
    if matches!(probe, Ok(ref output) if output.stdout.trim() == "yes") {
        return;
    }
    let _ = connection
        .execute_command(
            "sudo apt-get update -qq && \
             (sudo DEBIAN_FRONTEND=noninteractive apt-get install -y default-mysql-client \
             || sudo DEBIAN_FRONTEND=noninteractive apt-get install -y mysql-client)",
        )
        .await;
}

/// `host.host` values meaning "this Database Host's own server lives on the
/// exact same Node it's linked to" - the only case `ensure_mysql_server_installed`
/// below is safe to act on, never a value pointing at some other, already-
/// managed MySQL server elsewhere that this codebase has no business
/// installing a *server* onto.
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Single-quote SQL string-literal escaping (doubling an embedded `'`, the
/// ANSI-SQL/MySQL-standard way) - separate from `shell_quote` above, which
/// escapes for the *shell* wrapping a `mysql -e` invocation, not for SQL
/// syntax itself. Needed here (unlike the rest of this module, whose own
/// doc comment explains why `database_name`/`username` never need it -
/// they're always machine-generated) because `host.admin_username`/
/// `host.host` embedded below are free-text the user typed into the
/// Database Host form.
/// A MySQL string literal.
///
/// **Backslashes have to be escaped too, not just quotes.** Doubling `'`
/// alone is correct only under `NO_BACKSLASH_ESCAPES`, which is not the
/// default in MySQL or MariaDB. Without escaping the backslash, a value
/// ending in one breaks out: `x\` renders as `'x\''...'`, the server reads
/// `\'` as a literal quote, the string closes early, and whatever follows
/// runs as SQL. That matters here because `admin_username` and `host` are
/// free text and the statement they land in runs through `sudo mysql` as
/// the database superuser.
///
/// Backslash first, deliberately: escaping quotes first would then double
/// the backslashes this step introduces.
fn sql_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', r"\\").replace('\'', "''"))
}

/// "Plug and play, no manual server prep" (`server_service::install_docker`'s
/// same standing bar) extended to a self-hosted Database Host: a Node with
/// no MySQL/MariaDB server reachable at all (the common case for a fresh
/// Database Host pointed at its own Node's `127.0.0.1`, before anything's
/// ever been installed there) gets one auto-installed and started rather
/// than surfacing a bare "Can't connect to MySQL server" for the user to
/// puzzle out and go fix by hand over a separate SSH session. Skipped
/// entirely for a non-loopback `host.host` (see `is_loopback_host`) - this
/// only ever provisions a server on the exact Node this Database Host is
/// already linked to, never anywhere else.
///
/// Debian/Ubuntu's `mariadb-server` package leaves `root@localhost`
/// authenticating via the `unix_socket` plugin (no password, but only
/// usable from a local shell as the Linux `root` user - not over the TCP
/// connection this whole flow always uses, see `build_mysql_command`'s own
/// doc comment on why). **Must actually change that exact account, not just
/// add a separate one**: with name resolution on (the default), MySQL/
/// MariaDB resolve a TCP connection from `127.0.0.1` back to `localhost`
/// (via `/etc/hosts`) *before* matching it against `mysql.user`, so a
/// connection to `-h 127.0.0.1` is checked against `'<user>'@'localhost'`,
/// never a separately-created `'<user>'@'127.0.0.1'` row - granting only the
/// latter (an earlier version of this function's own mistake) leaves the
/// original `unix_socket`-only account in place and every TCP login still
/// gets rejected. `ALTER USER` (not just `CREATE USER IF NOT EXISTS`, which
/// no-ops when the account already exists) is what actually swaps that
/// account's auth method to a password. Sets it on both the `localhost` and
/// literal `host.host` forms, over the same local socket (`sudo mysql`,
/// authenticating as the Linux root user, no TCP/password needed for *this*
/// one-time step) - covers a server with name resolution off too, where the
/// literal-host row is the one actually consulted.
///
/// The install step is skipped once the server's already active (no point
/// re-running `apt-get` every single provisioning call) - but the grant
/// step below always runs regardless, cheap and idempotent (`ALTER USER`
/// to the same password is a no-op), so a Node whose server was already
/// installed *before* this function knew to fix the `localhost` account
/// still gets self-healed on its very next provisioning attempt, not only
/// on a fresh install.
/// Whether a database server is actually running on this Node.
///
/// Only ever asked of a loopback host - a remote host's server is somebody
/// else's business and this Node's `systemctl` says nothing about it.
async fn database_server_running(connection: &SshSession) -> AppResult<bool> {
    let active = connection.execute_command("systemctl is-active --quiet mariadb || systemctl is-active --quiet mysql").await?;
    Ok(active.exit_code == 0)
}

/// Refuses the operation, with an actionable code, when a loopback Database
/// Host has no server behind it.
///
/// Deliberately a refusal rather than an install: see the call site. A
/// remote host is left alone entirely - if it is unreachable, `run_mysql`'s
/// own error says so far better than a guess from here would.
async fn require_database_server(connection: &SshSession, host: &DatabaseHost) -> AppResult<()> {
    if !is_loopback_host(&host.host) {
        return Ok(());
    }
    if database_server_running(connection).await? {
        return Ok(());
    }
    Err(AppError::DatabaseServerUnavailable { host: host.name.clone() })
}

/// Installs MariaDB on the Node behind a loopback Database Host, and gives
/// that host's configured admin user the privileges VibeSSH then relies on.
///
/// **Explicit and consented** - this is the whole of A.4.3. It used to run
/// as an unannounced side effect of creating an application database, which
/// meant an `apt-get install`, an enabled system service, a new superuser
/// with `WITH GRANT OPTION`, and a rewritten bind address all appearing on
/// somebody's machine because they clicked "New database". Every step's
/// error was discarded, so when any of it failed the operator saw an
/// "access denied" from a later query instead.
///
/// Every step now propagates. A half-installed database server is worth
/// stopping on: the next step's failure would otherwise be reported against
/// whatever the operator does next, hours later.
pub async fn install_database_server(
    repo: &DatabaseRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    host_id: Uuid,
) -> AppResult<()> {
    let host = repo.get_host(host_id)?.ok_or_else(|| AppError::NotFound(format!("database host {host_id}")))?;
    if !is_loopback_host(&host.host) {
        return Err(AppError::InvalidInput(format!(
            "'{}' points at {}, not at this node itself - VibeSSH only installs a database server on a node it manages",
            host.name, host.host
        )));
    }
    let admin_password = load_host_admin_password(&host)?;
    let connection = connect_to_host(server_repo, sessions, &host).await?;

    if !database_server_running(&connection).await? {
        let install = connection
            .execute_command(
                "sudo apt-get update -qq \
                 && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y mariadb-server \
                 && sudo systemctl enable --now mariadb",
            )
            .await?;
        if install.exit_code != 0 {
            let detail = install.stderr.trim();
            let detail = if detail.is_empty() { "the install command failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't install a database server on this node: {detail}")));
        }
    }
    grant_admin_user(&connection, &host, &admin_password).await?;
    ensure_mysql_reachable_from_containers(&connection, host.port).await;
    Ok(())
}

/// Creates (or re-points) the Database Host's configured admin user on a
/// freshly installed server, so the credentials the operator typed when they
/// linked the host actually work against it.
async fn grant_admin_user(connection: &SshSession, host: &DatabaseHost, admin_password: &str) -> AppResult<()> {
    let user = sql_quote(&host.admin_username);
    let addr = sql_quote(&host.host);
    let local = sql_quote("localhost");
    let pass = sql_quote(admin_password);
    let grant_sql = format!(
        "CREATE USER IF NOT EXISTS {user}@{addr} IDENTIFIED BY {pass}; \
         CREATE USER IF NOT EXISTS {user}@{local} IDENTIFIED BY {pass}; \
         ALTER USER {user}@{addr} IDENTIFIED BY {pass}; \
         ALTER USER {user}@{local} IDENTIFIED BY {pass}; \
         GRANT ALL PRIVILEGES ON *.* TO {user}@{addr} WITH GRANT OPTION; \
         GRANT ALL PRIVILEGES ON *.* TO {user}@{local} WITH GRANT OPTION; \
         FLUSH PRIVILEGES;"
    );
    // Through a file, not `-e`: this statement contains the admin password
    // in plaintext, and a command string is visible in `ps` to every local
    // account on the Node while it runs.
    let sql_file = format!(".vibessh-grant-{}.sql", Uuid::new_v4());
    write_private_file(connection, &sql_file, grant_sql.as_bytes()).await?;
    // Propagates, unlike before. Every later `mysql` call authenticates as
    // this user, so a grant that quietly did not land turns into "access
    // denied" against whatever the operator does next - an error that names
    // neither this step nor the install that triggered it.
    let applied = connection.execute_command(&format!("sudo mysql < {file}; rm -f {file}", file = shell_quote(&sql_file))).await?;
    if applied.exit_code != 0 {
        let detail = applied.stderr.trim().replace(admin_password, "[redacted]");
        let detail = if detail.is_empty() { "the grant statement failed".to_string() } else { detail };
        return Err(AppError::Connection(format!("the database server was installed but its admin user couldn't be created: {detail}")));
    }
    Ok(())
}

/// Makes a self-hosted MariaDB reachable from an Application's container
/// without making it reachable from the internet.
///
/// **Why anything is needed at all.** Debian/Ubuntu's `mariadb-server`
/// package ships `bind-address = 127.0.0.1`. A container's connection
/// arrives over a Docker bridge interface, never `lo`, and a socket bound
/// specifically to `127.0.0.1` never accepts a connection arriving on any
/// other interface - no firewall rule changes that.
///
/// **Why this no longer writes `0.0.0.0`.** It used to, unconditionally and
/// with its errors discarded, which silently converted a correctly
/// loopback-only database into one listening on the Node's public
/// interface. Paired with the `'user'@'%'` grants this module also used to
/// create, that put the database on the internet behind nothing but a
/// generated password. Only `ufw` stood in the way, and only when `ufw`
/// happened to already be active.
///
/// Instead it binds loopback **plus the Docker bridge address specifically**
/// (`bind-address = 127.0.0.1,172.17.0.1`, multi-address support present in
/// MariaDB 10.11+ and MySQL 8.0.13+). The public interface is never bound,
/// so exposure does not depend on a firewall being installed, enabled, or
/// correctly configured.
///
/// **If the server cannot start with that config** - an older MariaDB
/// without multi-address support - the drop-in is removed again and the
/// server restarted, leaving the package default in place. That means
/// container access does not work on those versions, which is a visible,
/// fixable limitation; silently falling back to `0.0.0.0` would trade a
/// broken feature for an exposed database, which is the wrong trade.
///
/// Only restarts when the drop-in's content actually needs to change.
async fn ensure_mysql_reachable_from_containers(connection: &SshSession, port: u16) {
    let _ = port;
    let script = r#"set -e
path=/etc/mysql/mariadb.conf.d/99-vibessh-bind.cnf
bridge=$(ip -4 -o addr show docker0 2>/dev/null | awk '{print $4}' | cut -d/ -f1)
if [ -z "$bridge" ]; then
    # No Docker bridge on this Node yet - nothing to widen the bind for,
    # and widening it "just in case" is exactly the mistake this replaced.
    exit 0
fi
desired=$(printf '[mysqld]\nbind-address = 127.0.0.1,%s\n' "$bridge")
current=$(sudo cat "$path" 2>/dev/null || true)
if [ "$current" = "$desired" ]; then
    exit 0
fi
printf '%s' "$desired" | sudo tee "$path" >/dev/null
if ! sudo systemctl restart mariadb; then
    # This MariaDB cannot parse a multi-address bind. Roll back rather than
    # fall back to 0.0.0.0 - a database that is unreachable from containers
    # is a fixable inconvenience; one that is reachable from the internet is
    # not.
    sudo rm -f "$path"
    sudo systemctl restart mariadb || true
    echo "vibessh: this MariaDB does not support a multi-address bind-address; containers cannot reach it" >&2
    exit 1
fi
"#;
    match connection.execute_command(script).await {
        Ok(output) if output.exit_code == 0 => {}
        Ok(output) => log::warn!("couldn't make MariaDB reachable from containers: {}", output.stderr.trim()),
        Err(err) => log::warn!("couldn't make MariaDB reachable from containers: {err}"),
    }
}

async fn connect_to_host(server_repo: &ServerRepository, sessions: &SshSessionManager, host: &DatabaseHost) -> AppResult<Arc<SshSession>> {
    let Some(server_id) = host.server_id else {
        return Err(AppError::InvalidInput("this database host has no linked Server - VibeSSH can't run commands on it".into()));
    };
    get_or_connect(server_repo, sessions, server_id).await
}

/// `redact` is embedded into the executed SQL (e.g. a generated password in
/// a `CREATE USER`/`ALTER USER` statement) - if `mysql` ever echoes part of
/// the query back in an error message (a syntax-error snippet, say), this
/// keeps that secret out of the `AppError` a caller might display or log.
/// `admin_password` is always redacted too, defensively, even though it
/// isn't embedded in the SQL body itself (only used to authenticate the
/// connection).
async fn run_mysql(connection: &SshSession, host: &DatabaseHost, admin_password: &str, sql: &str, redact: &[&str]) -> AppResult<()> {
    // The password reaches the client through a mode-0600 defaults file
    // written over SFTP, never through the command string. Anything in a
    // command string is visible in `ps` to every local account on the Node
    // for as long as the client runs, and `MYSQL_PWD` - what this used to
    // do - is documented by MySQL itself as insecure for that reason.
    let defaults_file = format!(".vibessh-my-{}.cnf", Uuid::new_v4());
    write_private_file(connection, &defaults_file, defaults_file_contents(admin_password).as_bytes()).await?;

    let command = build_mysql_command(&defaults_file, host, sql);
    let attempt = connection.execute_command(&command).await;
    let _ = connection.execute_command(&format!("rm -f {}", shell_quote(&defaults_file))).await;
    let output = attempt?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let mut detail = if detail.is_empty() { "mysql command failed".to_string() } else { detail.to_string() };
        for secret in redact.iter().copied().chain(std::iter::once(admin_password)) {
            detail = detail.replace(secret, "[redacted]");
        }
        return Err(AppError::Connection(format!("database provisioning failed: {detail}")));
    }
    Ok(())
}

/// Pure command-string construction, separated from `run_mysql`'s actual
/// SSH exec so it's unit-testable without a live connection - same split
/// `runtime::docker::build_create_command`/`runtime::systemd::render_unit_file`
/// already use for the same reason. `MYSQL_PWD` (not `-p<password>`) so the
/// password never appears in a `ps`-visible argument list, only in this
/// one exec channel's own environment - see `docs/APPLICATIONS_ARCHITECTURE.md`
/// Section 12.1.
///
/// `--protocol=TCP` is required, not cosmetic: the `mysql` client silently
/// switches to a local Unix socket - ignoring `-P`/`-h` and, critically,
/// the `MYSQL_PWD`/`-u` auth this whole flow depends on - whenever `-h` is
/// literally `"localhost"` (as opposed to `"127.0.0.1"` or any other
/// value). A socket connection then authenticates as whatever OS user is
/// running the SSH session, not `admin_username`, which fails outright on
/// a `root` account still using the `auth_socket`/`unix_socket` plugin
/// (the default on most Debian/Ubuntu MySQL/MariaDB installs) - exactly
/// the confusing "Access denied for user 'root'@'localhost'" this forces
/// a real TCP connection to avoid, regardless of what `host.host` is set to.
/// The `[client]` section `--defaults-extra-file` reads. Only the password
/// goes here - everything else stays on the command line, where it is not
/// sensitive and is much easier to read back from a log.
fn defaults_file_contents(admin_password: &str) -> String {
    format!("[client]\npassword={}\n", option_file_value(admin_password))
}

/// Quotes a value for a MySQL option file.
///
/// An unquoted value cannot be used here: MySQL option files treat `#` as
/// the start of a comment and strip trailing whitespace, so an admin
/// password containing either would be silently truncated and
/// authentication would fail with a confusing "access denied" that has
/// nothing to do with the password being wrong. Double quotes disable both.
///
/// Inside a quoted value MySQL recognizes backslash escape sequences, so
/// `\` and `"` have to be escaped themselves - backslash first, or the
/// escaping would double the backslashes it just introduced.
fn option_file_value(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', r"\\").replace('"', "\\\""))
}

/// `--defaults-extra-file` must be the first option: MySQL clients reject
/// it anywhere else.
fn build_mysql_command(defaults_file: &str, host: &DatabaseHost, sql: &str) -> String {
    format!(
        "mysql --defaults-extra-file={} --protocol=TCP -h {} -P {} -u {} -e {}",
        shell_quote(defaults_file),
        shell_quote(&host.host),
        host.port,
        shell_quote(&host.admin_username),
        shell_quote(sql),
    )
}



/// Deliberately **not** `naming::dns_label`, despite the similar name.
///
/// This produces part of a SQL identifier, where `-` is not a legal
/// character at all - so this strips every non-alphanumeric rather than
/// converting runs of them to dashes. Two conversions that look alike and
/// have to stay different; sharing them would break one or the other.
fn slugify(input: &str) -> String {
    input.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn random_alnum(len: usize) -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

/// `vibessh_<slug-of-seed>_<random>` - the fixed `vibessh_`/`_<random>`
/// parts are never truncated (the random suffix is the actual uniqueness
/// guarantee; the prefix is what makes a generated identifier
/// recognizable as VibeSSH-owned at a glance, same naming philosophy
/// `runtime::docker::container_name`/`runtime::systemd::unit_name` already
/// use) - only the seed-derived slug in the middle shrinks to fit
/// `max_len`. Always starts with a letter and contains only
/// `[a-z0-9_]`, valid as both a MySQL identifier and username regardless
/// of `seed`'s own content.
fn generate_identifier(seed: &str, max_len: usize) -> String {
    let slug = slugify(seed);
    let suffix = random_alnum(RANDOM_SUFFIX_LEN);
    let fixed = "vibessh__".len() + suffix.len();
    let slug_budget = max_len.saturating_sub(fixed).max(1);
    let slug: String = slug.chars().take(slug_budget).collect();
    format!("vibessh_{slug}_{suffix}")
}

fn generate_password() -> String {
    use rand::Rng;
    // Letters + digits only, upper/lower mixed for entropy - no symbols, so
    // this never needs escaping anywhere it's used (a SQL string literal,
    // a shell argument, a keyring value).
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..GENERATED_PASSWORD_LEN).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DatabaseEngine;

    fn stub_host() -> DatabaseHost {
        DatabaseHost {
            id: Uuid::new_v4(),
            server_id: None,
            name: "Main DB host".to_string(),
            engine: DatabaseEngine::Mysql,
            host: "127.0.0.1".to_string(),
            port: 3306,
            admin_username: "root".to_string(),
            phpmyadmin_application_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// `-u 'ro'\''ot'` - what POSIX single-quote escaping turns
    /// `ro'ot` into. Spelled out as a constant so the test's own
    /// expectation is readable rather than a wall of backslashes.
    const SHELL_ESCAPED_ROOT: &str = r"-u 'ro'\''ot'";

    /// The regression test for the password-in-argv finding: a command
    /// string is visible in `ps` to every local account on the Node for as
    /// long as the client runs, so the password must not appear in one -
    /// not as `-p`, and not as `MYSQL_PWD` either.
    #[test]
    fn build_mysql_command_never_carries_the_password() {
        let host = stub_host();
        let command = build_mysql_command(".vibessh-my-test.cnf", &host, "SELECT 1;");
        assert!(!command.contains("adminpass"), "{command}");
        assert!(!command.contains("MYSQL_PWD"), "{command}");
        // --defaults-extra-file has to be the first option or the client
        // rejects it.
        assert!(command.starts_with("mysql --defaults-extra-file='.vibessh-my-test.cnf'"), "{command}");
        assert!(command.contains("-h '127.0.0.1'"));
        assert!(command.contains("-P 3306"));
        assert!(command.contains("-u 'root'"));
        assert!(command.contains("-e 'SELECT 1;'"));
    }

    #[test]
    fn defaults_file_contents_is_a_client_section_with_only_the_password() {
        assert_eq!(defaults_file_contents("adminpass"), "[client]\npassword=\"adminpass\"\n");
    }

    /// The admin password is free text the operator typed. MySQL option
    /// files treat `#` as a comment and strip trailing whitespace, so an
    /// unquoted value would be silently truncated and surface as a
    /// confusing "access denied" rather than anything pointing at the real
    /// cause.
    #[test]
    fn option_file_value_survives_characters_an_option_file_would_otherwise_eat() {
        assert_eq!(option_file_value("p#ss"), "\"p#ss\"");
        assert_eq!(option_file_value("pass "), "\"pass \"");
        assert_eq!(option_file_value("a\"b"), "\"a\\\"b\"");
        assert_eq!(option_file_value(r"a\b"), "\"a\\\\b\"");
        // A quote preceded by a backslash must not let the value close early.
        assert_eq!(option_file_value("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn build_mysql_command_forces_tcp_even_when_the_host_is_literally_localhost() {
        // The mysql client silently switches to a Unix socket - bypassing
        // the defaults file and `-u` auth entirely - whenever `-h` is
        // exactly "localhost". --protocol=TCP is what stops that.
        let mut host = stub_host();
        host.host = "localhost".to_string();
        let command = build_mysql_command(".vibessh-my-test.cnf", &host, "SELECT 1;");
        assert!(command.contains("--protocol=TCP"), "{command}");
    }

    #[test]
    fn build_mysql_command_single_quote_escapes_every_embedded_value() {
        let mut host = stub_host();
        host.admin_username = "ro'ot".to_string();
        let command = build_mysql_command(".vibessh-my-test.cnf", &host, "DROP DATABASE `x`;");
        // A raw embedded quote would otherwise close the shell string early.
        assert!(command.contains(SHELL_ESCAPED_ROOT), "{command}");
    }

    /// The regression test for the SQL-injection finding. Doubling `'`
    /// alone is only correct under `NO_BACKSLASH_ESCAPES`, which is not the
    /// default - so a value ending in a backslash closed the string early
    /// and let everything after it run as SQL, through `sudo mysql`.
    #[test]
    fn sql_quote_escapes_backslashes_as_well_as_quotes() {
        assert_eq!(sql_quote("plain"), "'plain'");
        assert_eq!(sql_quote("it's"), "'it''s'");
        assert_eq!(sql_quote(r"x\"), r"'x\\'");
        // The breakout attempt: a trailing backslash followed by a quote.
        // Both must survive escaping as inert literal characters.
        let hostile = sql_quote(r"x\'; GRANT ALL PRIVILEGES ON *.* TO 'evil'@'%'; -- ");
        assert!(hostile.starts_with(r"'x\\''"), "{hostile}");
        assert!(hostile.ends_with('\''), "{hostile}");
    }

    /// A generated database user must never be reachable from the whole
    /// internet. `%` was the previous value, and the reason the bind-address
    /// exposure mattered as much as it did.
    #[test]
    fn generated_users_are_never_granted_to_every_host() {
        assert_ne!(CONNECTIONS_FROM, "%");
        for grant_host in grant_hosts() {
            assert_ne!(grant_host, "%", "a grant host of '%' is reachable from anywhere");
        }
        // Containers arrive over a Docker bridge; host processes over
        // loopback. Those are the only two legitimate sources.
        assert!(grant_hosts().contains(&"localhost"));
        assert!(grant_hosts().iter().any(|h| h.starts_with("172.")));
    }


    #[test]
    fn is_loopback_host_accepts_only_the_same_node_addresses() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
        assert!(!is_loopback_host("10.0.0.5"));
        assert!(!is_loopback_host("db.example.com"));
    }

    #[test]
    fn sql_quote_doubles_an_embedded_single_quote() {
        assert_eq!(sql_quote("root"), "'root'");
        assert_eq!(sql_quote("o'brien"), "'o''brien'");
    }

    #[test]
    fn generate_identifier_is_a_valid_mysql_identifier_within_the_length_budget() {
        // A short seed (fits entirely) and a long one (forces truncation of
        // the slug, never of the fixed "vibessh_"/random-suffix parts) -
        // both real max_lens this module actually uses.
        for seed in ["My Cool Server!! 🎮", "An Extremely Long Application Name That Would Never Fit As-Is In Any Identifier"] {
            for max_len in [MAX_USERNAME_LEN, MAX_DATABASE_NAME_LEN] {
                let id = generate_identifier(seed, max_len);
                assert!(id.len() <= max_len, "{id} ({}) exceeds max_len {max_len}", id.len());
                assert!(id.chars().next().unwrap().is_ascii_lowercase());
                assert!(id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "unexpected character in {id}");
                assert!(id.starts_with("vibessh_"));
            }
        }
    }

    #[test]
    fn generate_identifier_is_randomized_across_calls() {
        let a = generate_identifier("same seed", MAX_DATABASE_NAME_LEN);
        let b = generate_identifier("same seed", MAX_DATABASE_NAME_LEN);
        assert_ne!(a, b, "two generated identifiers for the same seed collided - RNG looks broken");
    }

    #[test]
    fn generate_identifier_handles_a_seed_with_no_alphanumeric_characters() {
        let id = generate_identifier("!!! 🎮 ???", MAX_USERNAME_LEN);
        assert!(id.len() <= MAX_USERNAME_LEN);
        assert!(id.starts_with("vibessh_"));
    }

    #[test]
    fn generate_password_is_alphanumeric_only_and_randomized() {
        let a = generate_password();
        let b = generate_password();
        assert_eq!(a.len(), GENERATED_PASSWORD_LEN);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b, "two generated passwords collided - RNG looks broken");
    }

    #[tokio::test]
    async fn create_application_database_fails_cleanly_for_a_host_with_no_linked_server() {
        let sessions = SshSessionManager::new();
        let server_repo = ServerRepository::open(&std::env::temp_dir().join(format!("vibessh-database-service-test-{}.sqlite3", Uuid::new_v4()))).unwrap();
        let host = stub_host();
        assert!(host.server_id.is_none());

        let result = connect_to_host(&server_repo, &sessions, &host).await;
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    fn temp_repos() -> (DatabaseRepository, ApplicationRepository, ServerRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-database-service-phpmyadmin-test-{}.sqlite3", Uuid::new_v4()));
        (DatabaseRepository::open(&path).unwrap(), ApplicationRepository::open(&path).unwrap(), ServerRepository::open(&path).unwrap())
    }

    fn docker_application_with_published_port(app_repo: &ApplicationRepository, external_port: Option<u16>) -> Uuid {
        use crate::models::{CreateApplicationInput, PortInput, PortProtocol, RuntimeType};
        let detail = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "phpMyAdmin".to_string(),
                description: None,
                blueprint_id: "generic-docker".to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "image": "phpmyadmin/phpmyadmin", "command": [] }),
                metadata: serde_json::json!({}),
            })
            .unwrap();
        app_repo
            .add_port(
                detail.application.id,
                &PortInput {
                    name: "web".to_string(),
                    protocol: PortProtocol::Tcp,
                    bind_address: "0.0.0.0".to_string(),
                    internal_port: 80,
                    external_port,
                    visibility: crate::models::PortVisibility::Public,
                    required: false,
                },
            )
            .unwrap();
        detail.application.id
    }

    #[test]
    fn phpmyadmin_url_is_rejected_when_no_application_is_linked() {
        let (db_repo, app_repo, server_repo) = temp_repos();
        let host = db_repo.create_host(&host_input_for_test()).unwrap();

        let result = phpmyadmin_url(&db_repo, &app_repo, &server_repo, host.id, None);
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn phpmyadmin_url_is_rejected_when_the_linked_application_has_no_published_port() {
        let (db_repo, app_repo, server_repo) = temp_repos();
        let host = db_repo.create_host(&host_input_for_test()).unwrap();
        let application_id = docker_application_with_published_port(&app_repo, None);
        db_repo.update_phpmyadmin_application(host.id, Some(application_id)).unwrap();

        let result = phpmyadmin_url(&db_repo, &app_repo, &server_repo, host.id, None);
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn phpmyadmin_url_builds_a_local_url_with_the_published_port_and_prefilled_database() {
        let (db_repo, app_repo, server_repo) = temp_repos();
        let host = db_repo.create_host(&host_input_for_test()).unwrap();
        let application_id = docker_application_with_published_port(&app_repo, Some(8080));
        db_repo.update_phpmyadmin_application(host.id, Some(application_id)).unwrap();

        let url = phpmyadmin_url(&db_repo, &app_repo, &server_repo, host.id, Some("vibessh_myapp_ab12cd")).unwrap();
        assert_eq!(url, "http://127.0.0.1:8080/?db=vibessh_myapp_ab12cd");
    }

    fn host_input_for_test() -> crate::models::CreateDatabaseHostInput {
        crate::models::CreateDatabaseHostInput {
            server_id: None,
            name: "Main DB host".to_string(),
            engine: DatabaseEngine::Mysql,
            host: "127.0.0.1".to_string(),
            port: 3306,
            admin_username: "root".to_string(),
            admin_password: "hunter2".to_string(),
        }
    }
    /// The value is machine-generated today, but it arrives here as a
    /// free-form string from the frontend - an unencoded `&` or `#` would
    /// silently truncate the parameter rather than failing.
    #[test]
    fn percent_encode_query_value_escapes_everything_outside_the_unreserved_set() {
        assert_eq!(percent_encode_query_value("vibessh_app_a1b2c3"), "vibessh_app_a1b2c3");
        assert_eq!(percent_encode_query_value("a-b.c~d"), "a-b.c~d");
        assert_eq!(percent_encode_query_value("a&b"), "a%26b");
        assert_eq!(percent_encode_query_value("a#b"), "a%23b");
        assert_eq!(percent_encode_query_value("a b"), "a%20b");
        assert_eq!(percent_encode_query_value("a/b?c=d"), "a%2Fb%3Fc%3Dd");
    }
}
