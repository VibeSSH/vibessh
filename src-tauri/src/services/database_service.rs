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
use crate::ssh::SshSession;
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
/// Every generated database gets this bind pattern - not exposed as a wizard
/// field (Section 12.2: "the only input is an optional free-text purpose;
/// everything else is generated").
const CONNECTIONS_FROM: &str = "%";

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
    let _ = credentials::delete_secret(id, SecretKind::DatabaseHostAdmin);
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

    let sql = format!(
        "CREATE DATABASE IF NOT EXISTS `{database_name}`; \
         CREATE USER IF NOT EXISTS '{username}'@'{CONNECTIONS_FROM}' IDENTIFIED BY '{password}'; \
         GRANT ALL PRIVILEGES ON `{database_name}`.* TO '{username}'@'{CONNECTIONS_FROM}'; \
         FLUSH PRIVILEGES;"
    );
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
            let cleanup_sql = format!("DROP USER IF EXISTS '{username}'@'{CONNECTIONS_FROM}'; DROP DATABASE IF EXISTS `{database_name}`;");
            let _ = run_mysql_with_retry(server_repo, sessions, &host, &admin_password, &cleanup_sql, &[]).await;
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

    let sql = format!(
        "DROP USER IF EXISTS '{}'@'{}'; DROP DATABASE IF EXISTS `{}`;",
        database.username, database.connections_from, database.database_name
    );
    run_mysql_with_retry(server_repo, sessions, &host, &admin_password, &sql, &[]).await?;

    db_repo.delete_database(id)?;
    let _ = credentials::delete_secret(id, SecretKind::ApplicationDatabaseUser);
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
    let sql = format!("ALTER USER '{}'@'{}' IDENTIFIED BY '{new_password}'; FLUSH PRIVILEGES;", database.username, database.connections_from);
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
        // Always machine-generated (alphanumeric + underscore only, see
        // `generate_identifier`) - safe to embed directly in a query
        // string, no percent-encoding needed.
        url.push_str("?db=");
        url.push_str(name);
    }
    Ok(url)
}

// ---- Shared helpers ----

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
        ensure_mysql_server_installed(&connection, host, admin_password).await;
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
fn sql_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
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
async fn ensure_mysql_server_installed(connection: &SshSession, host: &DatabaseHost, admin_password: &str) {
    if !is_loopback_host(&host.host) {
        return;
    }
    let active = connection.execute_command("systemctl is-active --quiet mariadb || systemctl is-active --quiet mysql").await;
    if !matches!(active, Ok(ref output) if output.exit_code == 0) {
        let install = connection
            .execute_command(
                "sudo apt-get update -qq \
                 && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y mariadb-server \
                 && sudo systemctl enable --now mariadb",
            )
            .await;
        if !matches!(install, Ok(ref output) if output.exit_code == 0) {
            return;
        }
    }
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
    let _ = connection.execute_command(&format!("sudo mysql -e {}", shell_quote(&grant_sql))).await;
    ensure_mysql_listens_on_all_interfaces(connection, host.port).await;
}

/// Two independent things stand between a phpMyAdmin (or any other) Docker
/// container and a self-hosted MariaDB, and both have to be fixed for
/// `host.docker.internal` to actually work - fixing only one still times
/// out, it just times out for a different reason:
///
/// 1. **The bind address.** Debian/Ubuntu's `mariadb-server` package ships
///    `bind-address = 127.0.0.1` in `50-server.cnf` - loopback-only, by
///    design. A container's connection arrives over the `docker0` bridge
///    interface, never `lo`, and a socket bound specifically to
///    `127.0.0.1` never accepts a connection arriving on any other
///    interface, full stop - no firewall rule changes that. A drop-in
///    config file (`99-vibessh-bind.cnf`, loaded after `50-server.cnf` so
///    it wins) widens it to every interface.
/// 2. **The firewall.** A host with `ufw` active (this codebase's own
///    `firewall_service` sets exactly this kind of default-deny-incoming
///    policy up) drops - not rejects, which is exactly why this fails as a
///    silent connection *timeout* rather than an immediate refusal -
///    anything not explicitly allowed, and nothing before this ever taught
///    it about `docker0`. `ufw allow in on docker0 ... proto tcp` scopes
///    the allow to traffic arriving over that one interface specifically
///    (not a CIDR guess at Docker's bridge subnet, which varies) - the
///    public internet stays exactly as blocked from this port as it always
///    was, nothing here opens it there. Skipped entirely if `ufw` isn't
///    installed or isn't active, so this never *turns on* a firewall that
///    wasn't already managing this host's incoming traffic.
///
/// Only restarts MariaDB when the drop-in's content actually needs to
/// change, not on every provisioning call; the `ufw allow` is idempotent by
/// nature (re-adding an identical rule is a no-op) so it's safe to just run
/// every time too.
async fn ensure_mysql_listens_on_all_interfaces(connection: &SshSession, port: u16) {
    let script = format!(
        "path=/etc/mysql/mariadb.conf.d/99-vibessh-bind.cnf; \
         desired=$(printf '[mysqld]\\nbind-address = 0.0.0.0\\n'); \
         current=$(sudo cat \"$path\" 2>/dev/null || true); \
         if [ \"$current\" != \"$desired\" ]; then \
             printf '%s' \"$desired\" | sudo tee \"$path\" >/dev/null && sudo systemctl restart mariadb; \
         fi; \
         if command -v ufw >/dev/null 2>&1 && sudo ufw status | grep -q '^Status: active'; then \
             sudo ufw allow in on docker0 to any port {port} proto tcp >/dev/null; \
         fi"
    );
    let _ = connection.execute_command(&script).await;
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
    let command = build_mysql_command(host, admin_password, sql);
    let output = connection.execute_command(&command).await?;
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
fn build_mysql_command(host: &DatabaseHost, admin_password: &str, sql: &str) -> String {
    format!(
        "MYSQL_PWD={} mysql --protocol=TCP -h {} -P {} -u {} -e {}",
        shell_quote(admin_password),
        shell_quote(&host.host),
        host.port,
        shell_quote(&host.admin_username),
        shell_quote(sql),
    )
}

/// POSIX single-quote shell escaping - see `runtime::remote_process`'s copy
/// of the same function for the full reasoning; duplicated rather than
/// shared, matching how it's already duplicated across several modules in
/// this codebase.
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

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

    #[test]
    fn build_mysql_command_uses_mysql_pwd_not_a_visible_dash_p_flag() {
        let host = stub_host();
        let command = build_mysql_command(&host, "adminpass", "SELECT 1;");
        assert!(command.starts_with("MYSQL_PWD='adminpass' mysql --protocol=TCP"));
        assert!(!command.contains("-p'adminpass'"), "the password must never be passed as a -p flag");
        assert!(command.contains("-h '127.0.0.1'"));
        assert!(command.contains("-P 3306"));
        assert!(command.contains("-u 'root'"));
        assert!(command.contains("-e 'SELECT 1;'"));
    }

    #[test]
    fn build_mysql_command_forces_tcp_even_when_the_host_is_literally_localhost() {
        // The mysql client silently switches to a Unix socket - bypassing
        // MYSQL_PWD/-u auth entirely - whenever `-h` is exactly "localhost".
        // --protocol=TCP is what stops that from happening.
        let mut host = stub_host();
        host.host = "localhost".to_string();
        let command = build_mysql_command(&host, "adminpass", "SELECT 1;");
        assert!(command.contains("--protocol=TCP"), "{command}");
    }

    #[test]
    fn build_mysql_command_single_quote_escapes_every_embedded_value() {
        let mut host = stub_host();
        host.admin_username = "ro'ot".to_string();
        let command = build_mysql_command(&host, "pa'ss", "DROP DATABASE `x`;");
        // A raw embedded quote would otherwise close the shell string early.
        assert!(command.contains("MYSQL_PWD='pa'\\''ss'"));
        assert!(command.contains("-u 'ro'\\''ot'"));
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
}
