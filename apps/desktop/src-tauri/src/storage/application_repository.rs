//! SQLite-backed Application records - see docs/architecture/APPLICATIONS_ARCHITECTURE.md
//! for the full design. A separate repository struct from `ServerRepository`
//! (same one-concern-per-repository convention as `credentials`/
//! `cloud_config` being their own modules) even though both open the same
//! physical database file - `Application.server_id` is a real foreign key
//! into `servers`, which only means something because they share one file.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::protocol_name;
use crate::models::{
    Application, ApplicationDetail, ApplicationPort, ApplicationStatus, CreateApplicationInput, EnvironmentVariable, HealthCheckType,
    PortInput, PortProtocol, PortVisibility, RuntimeType, UpdateApplicationInput,
};

pub struct ApplicationRepository {
    conn: Mutex<Connection>,
}

impl ApplicationRepository {
    /// `db_path` is the same `servers.sqlite3` `ServerRepository` opens -
    /// `schema::migrate` is safe to call from both (it runs once per file
    /// once the file's `user_version` is already current), and each keeps
    /// its own `Connection` to it, same as any two independent SQLite
    /// clients of one file.
    pub fn open(db_path: &Path) -> AppResult<Self> {
        // Pragmas (WAL, busy timeout, foreign keys) live in one place -
        // see `storage::open_connection` for why they matter with nine
        // connections open on the same file.
        let mut conn = super::open_connection(db_path, "application")?;
        super::schema::migrate(&mut conn, db_path, "application")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn create(&self, input: &CreateApplicationInput) -> AppResult<ApplicationDetail> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(|err| AppError::Storage(format!("failed to start transaction: {err}")))?;

        let id = Uuid::new_v4();
        let now = Utc::now();
        tx.execute(
            "INSERT INTO applications (
                id, server_id, name, description, blueprint_id, blueprint_version,
                runtime_type, working_directory, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'unknown', ?9, ?9)",
            params![
                id.to_string(),
                input.server_id.map(|v| v.to_string()),
                input.name,
                input.description,
                input.blueprint_id,
                input.blueprint_version,
                runtime_type_to_str(input.runtime_type),
                input.working_directory,
                now.to_rfc3339(),
            ],
        )
        .map_err(|err| storage_or_fk_error(err, "server"))?;

        for env in &input.environment {
            // A secret's real value never reaches this column - see
            // `EnvironmentVariable::value`'s own doc comment. The caller
            // (`services::application_service`) is responsible for writing
            // the real value into the OS keyring once it knows this row's
            // generated `application_id`.
            let stored_value = if env.is_secret { "" } else { env.value.as_str() };
            tx.execute(
                "INSERT INTO application_environment (application_id, key, value, is_secret) VALUES (?1, ?2, ?3, ?4)",
                params![id.to_string(), env.key, stored_value, env.is_secret],
            )
            .map_err(|err| AppError::Storage(format!("failed to insert environment variable: {err}")))?;
        }

        for port in &input.ports {
            insert_port(&tx, id, port)?;
        }

        tx.execute(
            "INSERT INTO application_runtime_config (application_id, config_json) VALUES (?1, ?2)",
            params![id.to_string(), input.runtime_config.to_string()],
        )
        .map_err(|err| AppError::Storage(format!("failed to insert runtime config: {err}")))?;

        tx.execute(
            "INSERT INTO application_metadata (application_id, metadata_json) VALUES (?1, ?2)",
            params![id.to_string(), input.metadata.to_string()],
        )
        .map_err(|err| AppError::Storage(format!("failed to insert metadata: {err}")))?;

        tx.commit().map_err(|err| AppError::Storage(format!("failed to commit transaction: {err}")))?;
        drop(conn);

        self.get(id)?.ok_or_else(|| AppError::Internal(format!("application {id} vanished immediately after being created")))
    }

    pub fn update(&self, id: Uuid, input: &UpdateApplicationInput) -> AppResult<ApplicationDetail> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(|err| AppError::Storage(format!("failed to start transaction: {err}")))?;
        let now = Utc::now();

        let affected = tx
            .execute(
                "UPDATE applications SET name = ?2, description = ?3, working_directory = ?4, updated_at = ?5 WHERE id = ?1",
                params![id.to_string(), input.name, input.description, input.working_directory, now.to_rfc3339()],
            )
            .map_err(|err| AppError::Storage(format!("failed to update application: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }

        tx.execute(
            "UPDATE application_runtime_config SET config_json = ?2 WHERE application_id = ?1",
            params![id.to_string(), input.runtime_config.to_string()],
        )
        .map_err(|err| AppError::Storage(format!("failed to update runtime config: {err}")))?;
        tx.execute(
            "UPDATE application_metadata SET metadata_json = ?2 WHERE application_id = ?1",
            params![id.to_string(), input.metadata.to_string()],
        )
        .map_err(|err| AppError::Storage(format!("failed to update metadata: {err}")))?;

        tx.commit().map_err(|err| AppError::Storage(format!("failed to commit transaction: {err}")))?;
        drop(conn);

        self.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
    }

    /// Rewrites only the stored `runtime_config` JSON - unlike `update`,
    /// doesn't touch name/description/working_directory or bump
    /// `updated_at`. Used by `services::set_application_resource_limits`,
    /// which patches just the memory/CPU limit keys inside whatever shape
    /// the application's own runtime type already uses, not a full
    /// application edit.
    /// Changes only the name.
    ///
    /// Narrow on purpose. `update` rewrites the working directory, the runtime
    /// config and the metadata alongside the name, so routing a rename through
    /// it would make a one-word edit capable of resetting how the Application
    /// runs if any caller ever passed a stale copy of those.
    pub fn rename(&self, id: Uuid, name: &str) -> AppResult<ApplicationDetail> {
        let conn = self.lock();
        let affected = conn
            .execute(
                "UPDATE applications SET name = ?2, updated_at = ?3 WHERE id = ?1",
                params![id.to_string(), name, Utc::now().to_rfc3339()],
            )
            .map_err(|err| AppError::Storage(format!("failed to rename application: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }
        drop(conn);
        self.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
    }

    /// Changes which blueprint an Application is, and nothing else.
    ///
    /// Narrow for the same reason `rename` is: the caller has already
    /// rendered a new runtime config and written it, and routing this through
    /// `update` would let a stale copy of the working directory or the
    /// metadata ride along with a one-field change.
    pub fn set_blueprint(&self, id: Uuid, blueprint_id: &str, blueprint_version: i32) -> AppResult<()> {
        let conn = self.lock();
        let affected = conn
            .execute(
                "UPDATE applications SET blueprint_id = ?2, blueprint_version = ?3, updated_at = ?4 WHERE id = ?1",
                params![id.to_string(), blueprint_id, blueprint_version, Utc::now().to_rfc3339()],
            )
            .map_err(|err| AppError::Storage(format!("failed to change the application's blueprint: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }
        Ok(())
    }

    pub fn update_runtime_config(&self, id: Uuid, runtime_config: &serde_json::Value) -> AppResult<ApplicationDetail> {
        let conn = self.lock();
        let affected = conn
            .execute(
                "UPDATE application_runtime_config SET config_json = ?2 WHERE application_id = ?1",
                params![id.to_string(), runtime_config.to_string()],
            )
            .map_err(|err| AppError::Storage(format!("failed to update runtime config: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }
        drop(conn);
        self.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
    }

    /// Separate from `update` deliberately - refreshing status from a
    /// runtime happens far more often than editing an application's
    /// config, and shouldn't bump `updated_at` (which is meant to reflect
    /// a user's own edit, not a background poll) or go through the same
    /// full-form validation path.
    pub fn update_status(&self, id: Uuid, status: ApplicationStatus) -> AppResult<()> {
        let conn = self.lock();
        let affected = conn
            .execute(
                "UPDATE applications SET status = ?2, last_status_check_at = ?3 WHERE id = ?1",
                params![id.to_string(), status_to_str(status), Utc::now().to_rfc3339()],
            )
            .map_err(|err| AppError::Storage(format!("failed to update application status: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }
        Ok(())
    }

    /// If `port_id` is `Some`, it must be one of *this* application's own
    /// ports - pointing a health check at another application's port would
    /// silently check the wrong thing, not just be a dangling reference.
    pub fn set_health_check(
        &self,
        id: Uuid,
        health_check_type: HealthCheckType,
        port_id: Option<Uuid>,
        http_path: Option<&str>,
    ) -> AppResult<Application> {
        let conn = self.lock();
        if let Some(port_id) = port_id {
            let owns_port = self
                .list_ports_locked(&conn, id)?
                .iter()
                .any(|port| port.id == port_id);
            if !owns_port {
                return Err(AppError::InvalidInput(format!("port {port_id} doesn't belong to application {id}")));
            }
        }
        let affected = conn
            .execute(
                "UPDATE applications SET health_check_type = ?2, health_check_port_id = ?3, health_check_http_path = ?4 WHERE id = ?1",
                params![id.to_string(), health_check_type_to_str(health_check_type), port_id.map(|p| p.to_string()), http_path],
            )
            .map_err(|err| AppError::Storage(format!("failed to update health check config: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }
        conn.query_row(&format!("{APPLICATION_COLUMNS} FROM applications WHERE id = ?1"), params![id.to_string()], row_to_application)
            .map_err(|err| AppError::Storage(format!("failed to reload application: {err}")))
    }

    pub fn delete(&self, id: Uuid) -> AppResult<()> {
        let conn = self.lock();
        let affected = conn
            .execute("DELETE FROM applications WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete application: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("application {id}")));
        }
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> AppResult<Option<ApplicationDetail>> {
        let conn = self.lock();
        let Some(application) = conn
            .query_row(&format!("{APPLICATION_COLUMNS} FROM applications WHERE id = ?1"), params![id.to_string()], row_to_application)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to load application: {err}")))?
        else {
            return Ok(None);
        };

        let environment = self.list_environment_locked(&conn, id)?;
        let ports = self.list_ports_locked(&conn, id)?;
        let links = list_links_locked(&conn, id)?;
        let runtime_config = conn
            .query_row("SELECT config_json FROM application_runtime_config WHERE application_id = ?1", params![id.to_string()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| AppError::Storage(format!("failed to load runtime config: {err}")))?;
        let metadata = conn
            .query_row("SELECT metadata_json FROM application_metadata WHERE application_id = ?1", params![id.to_string()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| AppError::Storage(format!("failed to load metadata: {err}")))?;

        Ok(Some(ApplicationDetail {
            application,
            environment,
            ports,
            runtime_config: serde_json::from_str(&runtime_config).unwrap_or_default(),
            metadata: serde_json::from_str(&metadata).unwrap_or_default(),
            links,
        }))
    }

    /// The list/dashboard view - just the `applications` row, not the full
    /// detail (environment/ports/config), matching the brief's own card
    /// fields (name/blueprint/runtime/location/status/port summary), which
    /// don't need a full `ApplicationDetail` per row.
    pub fn list(&self) -> AppResult<Vec<Application>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(&format!("{APPLICATION_COLUMNS} FROM applications ORDER BY name COLLATE NOCASE"))
            .map_err(|err| AppError::Storage(format!("failed to prepare application list query: {err}")))?;
        let rows = stmt.query_map((), row_to_application).map_err(|err| AppError::Storage(format!("failed to list applications: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read an application row: {err}")))
    }

    /// Used before deleting a Server, to show the user what's still
    /// attached rather than let the FK constraint surface as a bare error
    /// (docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 9).
    pub fn list_by_server(&self, server_id: Uuid) -> AppResult<Vec<Application>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(&format!("{APPLICATION_COLUMNS} FROM applications WHERE server_id = ?1 ORDER BY name COLLATE NOCASE"))
            .map_err(|err| AppError::Storage(format!("failed to prepare application list query: {err}")))?;
        let rows = stmt
            .query_map(params![server_id.to_string()], row_to_application)
            .map_err(|err| AppError::Storage(format!("failed to list applications for server: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read an application row: {err}")))
    }

    pub fn set_environment(&self, application_id: Uuid, environment: &[EnvironmentVariable]) -> AppResult<()> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(|err| AppError::Storage(format!("failed to start transaction: {err}")))?;
        tx.execute("DELETE FROM application_environment WHERE application_id = ?1", params![application_id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to clear environment: {err}")))?;
        for env in environment {
            // Same rule as `create` - a secret's real value never lands in
            // this column.
            let stored_value = if env.is_secret { "" } else { env.value.as_str() };
            tx.execute(
                "INSERT INTO application_environment (application_id, key, value, is_secret) VALUES (?1, ?2, ?3, ?4)",
                params![application_id.to_string(), env.key, stored_value, env.is_secret],
            )
            .map_err(|err| AppError::Storage(format!("failed to insert environment variable: {err}")))?;
        }
        tx.commit().map_err(|err| AppError::Storage(format!("failed to commit transaction: {err}")))
    }

    /// The Applications `application_id` may reach over the Node's internal
    /// Docker networking. See `ApplicationDetail::links`.
    pub fn list_links(&self, application_id: Uuid) -> AppResult<Vec<Uuid>> {
        let conn = self.lock();
        list_links_locked(&conn, application_id)
    }

    /// Idempotent - granting a connection that already exists is a no-op
    /// rather than a constraint failure, so a retry after a failed *apply*
    /// (the Docker half, which is the half that can fail against a live
    /// Node) does not first have to undo the stored half.
    ///
    /// Rejects a self-link: a container is trivially able to reach itself,
    /// so storing one would only produce a Docker network with a single
    /// member and a row the UI would have to special-case.
    pub fn add_link(&self, a: Uuid, b: Uuid) -> AppResult<()> {
        if a == b {
            return Err(AppError::InvalidInput("an application can't be connected to itself".into()));
        }
        let (lo, hi) = ordered_pair(a, b);
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO application_links (application_id, peer_id, created_at) VALUES (?1, ?2, ?3)",
            params![lo.to_string(), hi.to_string(), Utc::now().to_rfc3339()],
        )
        .map_err(|err| AppError::Storage(format!("failed to store the connection: {err}")))?;
        Ok(())
    }

    /// Also idempotent, and for a sharper reason than `add_link`: revoking
    /// is the direction that closes an exposure, so "it was already gone"
    /// has to be a success or a partially-applied revoke could never be
    /// retried to completion.
    pub fn remove_link(&self, a: Uuid, b: Uuid) -> AppResult<()> {
        let (lo, hi) = ordered_pair(a, b);
        let conn = self.lock();
        conn.execute(
            "DELETE FROM application_links WHERE application_id = ?1 AND peer_id = ?2",
            params![lo.to_string(), hi.to_string()],
        )
        .map_err(|err| AppError::Storage(format!("failed to remove the connection: {err}")))?;
        Ok(())
    }

    pub fn list_ports(&self, application_id: Uuid) -> AppResult<Vec<ApplicationPort>> {
        let conn = self.lock();
        self.list_ports_locked(&conn, application_id)
    }

    /// `None` on success with no collision; `Some(existing port name)` if
    /// `bind_address`+`protocol`+`internal_port` already belongs to
    /// another port on this same application - the caller (service layer)
    /// turns that into a clear `AppError::InvalidInput` rather than a raw
    /// constraint failure, and callers checking availability against the
    /// *host* (not just this application) do that separately, since it
    /// needs a live check against the actual server, not a local query.
    pub fn add_port(&self, application_id: Uuid, port: &PortInput) -> AppResult<ApplicationPort> {
        let conn = self.lock();
        if let Some(collision) = self.find_port_collision_locked(&conn, application_id, None, port)? {
            return Err(AppError::InvalidInput(format!("port {} is already used by '{collision}' on this application", port.internal_port)));
        }
        let id = insert_port(&conn, application_id, port)?;
        self.list_ports_locked(&conn, application_id)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::Internal(format!("port {id} vanished immediately after being created")))
    }

    pub fn update_port(&self, application_id: Uuid, port_id: Uuid, port: &PortInput) -> AppResult<ApplicationPort> {
        let conn = self.lock();
        if let Some(collision) = self.find_port_collision_locked(&conn, application_id, Some(port_id), port)? {
            return Err(AppError::InvalidInput(format!("port {} is already used by '{collision}' on this application", port.internal_port)));
        }
        let affected = conn
            .execute(
                "UPDATE application_ports SET name = ?3, protocol = ?4, bind_address = ?5, internal_port = ?6,
                 external_port = ?7, visibility = ?8, updated_at = ?9 WHERE id = ?1 AND application_id = ?2",
                params![
                    port_id.to_string(),
                    application_id.to_string(),
                    port.name,
                    protocol_to_str(port.protocol),
                    port.bind_address,
                    port.internal_port,
                    port.external_port,
                    visibility_to_str(port.visibility),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|err| AppError::Storage(format!("failed to update port: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("port {port_id}")));
        }
        self.list_ports_locked(&conn, application_id)?
            .into_iter()
            .find(|p| p.id == port_id)
            .ok_or_else(|| AppError::NotFound(format!("port {port_id}")))
    }

    /// Blueprint-`required` ports can't be removed, only edited (brief's
    /// own rule) - enforced here, not just in the UI, so a direct command
    /// call can't bypass it either.
    pub fn remove_port(&self, application_id: Uuid, port_id: Uuid) -> AppResult<()> {
        let conn = self.lock();
        let required: Option<bool> = conn
            .query_row(
                "SELECT required FROM application_ports WHERE id = ?1 AND application_id = ?2",
                params![port_id.to_string(), application_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to look up port: {err}")))?;
        match required {
            None => Err(AppError::NotFound(format!("port {port_id}"))),
            Some(true) => Err(AppError::InvalidInput("this port is required by the application's blueprint and can't be removed".into())),
            Some(false) => {
                conn.execute("DELETE FROM application_ports WHERE id = ?1", params![port_id.to_string()])
                    .map_err(|err| AppError::Storage(format!("failed to remove port: {err}")))?;
                Ok(())
            }
        }
    }

    fn list_environment_locked(&self, conn: &Connection, application_id: Uuid) -> AppResult<Vec<EnvironmentVariable>> {
        let mut stmt = conn
            .prepare("SELECT key, value, is_secret FROM application_environment WHERE application_id = ?1 ORDER BY key")
            .map_err(|err| AppError::Storage(format!("failed to prepare environment query: {err}")))?;
        let rows = stmt
            .query_map(params![application_id.to_string()], |row| {
                Ok(EnvironmentVariable { key: row.get(0)?, value: row.get(1)?, is_secret: row.get(2)? })
            })
            .map_err(|err| AppError::Storage(format!("failed to list environment: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read an environment row: {err}")))
    }

    fn list_ports_locked(&self, conn: &Connection, application_id: Uuid) -> AppResult<Vec<ApplicationPort>> {
        let mut stmt = conn
            .prepare(&format!("{PORT_COLUMNS} FROM application_ports WHERE application_id = ?1 ORDER BY name COLLATE NOCASE"))
            .map_err(|err| AppError::Storage(format!("failed to prepare port query: {err}")))?;
        let rows =
            stmt.query_map(params![application_id.to_string()], row_to_port).map_err(|err| AppError::Storage(format!("failed to list ports: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a port row: {err}")))
    }

    /// `Some(application_name)` if `external_port`+`protocol` is already
    /// published by *some* port on this same Node (`server_id`) - a
    /// different Application's port, or a different port on this same
    /// Application - `None` if free. Deliberately scoped to one Node: two
    /// different Nodes publishing the same external port isn't a collision
    /// at all, each has its own network stack. This is the DB half of the
    /// design doc's "check other Applications, other Exit Ports" collision
    /// requirement - `services::firewall_service::listening_process` covers
    /// the other half (an actual live probe against the host).
    pub fn find_external_port_owner(
        &self,
        server_id: Uuid,
        excluding_port_id: Option<Uuid>,
        protocol: PortProtocol,
        external_port: u16,
    ) -> AppResult<Option<String>> {
        let conn = self.lock();
        find_external_port_owner_locked(&conn, server_id, excluding_port_id, protocol, external_port)
    }

    /// Checks for a collision and inserts the port in **one** transaction.
    ///
    /// `add_port` checks only against this same Application's other ports,
    /// and the cross-Application check on a published `external_port` lived
    /// in the service layer as a separate query before a separate insert.
    /// Between those two statements is a window where two concurrent adds -
    /// which is what a double-clicked button produces, since each Tauri
    /// command runs on the same async runtime - both see the port free and
    /// both write it (`AUDIT_REPORT.md` D-007).
    ///
    /// Double-booking a published port is not cosmetic: `docker create`
    /// fails on the second one, and because the collision check consults the
    /// database, the row that should never have been written then makes the
    /// *next* check report the port as taken by an Application that could
    /// not start.
    ///
    /// `TransactionBehavior::Immediate` rather than the default deferred
    /// one, and that is the whole fix. A deferred transaction lets both
    /// racers take a read lock, see the port free, and only then fight over
    /// the write - the loser gets a busy/snapshot error, having already
    /// decided the port was available. Taking the write lock up front makes
    /// the loser wait (`busy_timeout`), re-read, and correctly find the
    /// collision, so it returns `PortInUse` naming the winner rather than a
    /// storage error naming nothing.
    ///
    /// D-007 asked for a unique index instead. That is not expressible here:
    /// the uniqueness that matters is `(server_id, protocol, external_port)`
    /// and `server_id` lives on `applications`, across a join SQLite cannot
    /// constrain - see `FIX_PLAN.md` E.9. This closes the same window inside
    /// one process, which is where it is actually reachable.
    pub fn claim_external_port(&self, application_id: Uuid, server_id: Uuid, port: &PortInput) -> AppResult<ApplicationPort> {
        let Some(external_port) = port.external_port else {
            // Nothing published, so nothing to claim against another
            // Application - `add_port`'s own per-Application check is the
            // whole requirement.
            return self.add_port(application_id, port);
        };
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| AppError::Storage(format!("failed to start the port transaction: {err}")))?;

        if let Some(collision) = self.find_port_collision_locked(&tx, application_id, None, port)? {
            return Err(AppError::InvalidInput(format!("port {} is already used by '{collision}' on this application", port.internal_port)));
        }
        if let Some(owner) = find_external_port_owner_locked(&tx, server_id, None, port.protocol, external_port)? {
            return Err(AppError::PortInUse { port: external_port, protocol: protocol_name(port.protocol), owner: Some(owner) });
        }
        let id = insert_port(&tx, application_id, port)?;
        tx.commit().map_err(|err| AppError::Storage(format!("failed to commit the port: {err}")))?;

        self.list_ports_locked(&conn, application_id)?
            .into_iter()
            .find(|existing| existing.id == id)
            .ok_or_else(|| AppError::Internal(format!("port {id} vanished immediately after being created")))
    }

    fn find_port_collision_locked(
        &self,
        conn: &Connection,
        application_id: Uuid,
        excluding_port_id: Option<Uuid>,
        port: &PortInput,
    ) -> AppResult<Option<String>> {
        conn.query_row(
            "SELECT name FROM application_ports
             WHERE application_id = ?1 AND protocol = ?2 AND bind_address = ?3 AND internal_port = ?4
             AND id != ?5",
            params![
                application_id.to_string(),
                protocol_to_str(port.protocol),
                port.bind_address,
                port.internal_port,
                excluding_port_id.map(|id| id.to_string()).unwrap_or_default(),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| AppError::Storage(format!("failed to check for a port collision: {err}")))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("application database mutex poisoned")
    }
}

/// Which other Application on `server_id` has already published
/// `external_port` on this protocol, if any. Takes a `&Connection` so
/// `claim_external_port` can run it inside its own transaction rather than
/// re-locking - the point of that transaction is that this check and the
/// insert cannot be separated.
fn find_external_port_owner_locked(
    conn: &Connection,
    server_id: Uuid,
    excluding_port_id: Option<Uuid>,
    protocol: PortProtocol,
    external_port: u16,
) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT a.name FROM application_ports p
         JOIN applications a ON a.id = p.application_id
         WHERE a.server_id = ?1 AND p.protocol = ?2 AND p.external_port = ?3
         AND p.id != ?4",
        params![
            server_id.to_string(),
            protocol_to_str(protocol),
            external_port,
            excluding_port_id.map(|id| id.to_string()).unwrap_or_default(),
        ],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| AppError::Storage(format!("failed to check for an external port collision: {err}")))
}

fn insert_port(conn: &Connection, application_id: Uuid, port: &PortInput) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    conn.execute(
        "INSERT INTO application_ports (
            id, application_id, name, protocol, bind_address, internal_port, external_port, visibility, required, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            id.to_string(),
            application_id.to_string(),
            port.name,
            protocol_to_str(port.protocol),
            port.bind_address,
            port.internal_port,
            port.external_port,
            visibility_to_str(port.visibility),
            port.required,
            now.to_rfc3339(),
        ],
    )
    .map_err(|err| AppError::Storage(format!("failed to insert port: {err}")))?;
    Ok(id)
}

/// Maps a foreign-key failure on `applications.server_id` to a clear
/// "that server doesn't exist" - the only FK this table has going out
/// (its own id is the one other tables reference in, handled by their own
/// `ON DELETE CASCADE`/`RESTRICT`, not by this function).
fn storage_or_fk_error(err: rusqlite::Error, referenced: &str) -> AppError {
    // Message text, not `extended_code == SQLITE_CONSTRAINT_FOREIGNKEY` -
    // see server_repository::is_foreign_key_violation's own comment for
    // why that extended code isn't reliable across every FK-violation
    // shape (proven by a real test failure on the RESTRICT/DELETE case,
    // even though this particular INSERT case happens to report it
    // correctly).
    if let rusqlite::Error::SqliteFailure(ref sqlite_err, Some(ref message)) = err {
        if sqlite_err.code == rusqlite::ErrorCode::ConstraintViolation && message.contains("FOREIGN KEY") {
            return AppError::InvalidInput(format!("that {referenced} doesn't exist"));
        }
    }
    AppError::Storage(format!("failed to save application: {err}"))
}

const APPLICATION_COLUMNS: &str = "SELECT id, server_id, name, description, blueprint_id, blueprint_version, \
     runtime_type, working_directory, status, last_status_check_at, health_check_type, health_check_port_id, \
     health_check_http_path, created_at, updated_at";

/// `application_links` stores one row per unordered pair with
/// `application_id < peer_id` (a CHECK constraint enforces it), so every
/// read and write normalises the pair the same way first.
fn ordered_pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Both columns, because the pair is normalised on write: an Application is
/// as often the higher id as the lower one, and which side it landed on says
/// nothing about the relationship.
fn list_links_locked(conn: &Connection, application_id: Uuid) -> AppResult<Vec<Uuid>> {
    let id = application_id.to_string();
    let mut stmt = conn
        .prepare(
            "SELECT peer_id FROM application_links WHERE application_id = ?1
             UNION
             SELECT application_id FROM application_links WHERE peer_id = ?1",
        )
        .map_err(|err| AppError::Storage(format!("failed to prepare the connection query: {err}")))?;
    let rows = stmt
        .query_map(params![id], |row| row.get::<_, String>(0))
        .map_err(|err| AppError::Storage(format!("failed to list connections: {err}")))?;
    let mut links = Vec::new();
    for row in rows {
        let raw = row.map_err(|err| AppError::Storage(format!("failed to read a connection row: {err}")))?;
        // A row whose id no longer parses is a corrupted row, not a reason to
        // fail the whole Application load - but it must not silently become
        // "no connection", because a dropped link reads as *more* isolation
        // than there is. Log it and keep the rest.
        match Uuid::parse_str(&raw) {
            Ok(id) => links.push(id),
            Err(err) => log::warn!("ignoring an unparseable application_links row '{raw}': {err}"),
        }
    }
    Ok(links)
}

fn row_to_application(row: &rusqlite::Row) -> rusqlite::Result<Application> {
    Ok(Application {
        id: parse_uuid(row.get::<_, String>(0)?, 0)?,
        server_id: row.get::<_, Option<String>>(1)?.map(|v| parse_uuid(v, 1)).transpose()?,
        name: row.get(2)?,
        description: row.get(3)?,
        blueprint_id: row.get(4)?,
        blueprint_version: row.get(5)?,
        runtime_type: runtime_type_from_str(&row.get::<_, String>(6)?),
        working_directory: row.get(7)?,
        status: status_from_str(&row.get::<_, String>(8)?),
        last_status_check_at: row.get::<_, Option<String>>(9)?.map(|v| parse_timestamp(v, 9)).transpose()?,
        health_check_type: health_check_type_from_str(&row.get::<_, String>(10)?),
        health_check_port_id: row.get::<_, Option<String>>(11)?.map(|v| parse_uuid(v, 11)).transpose()?,
        health_check_http_path: row.get(12)?,
        created_at: parse_timestamp(row.get::<_, String>(13)?, 13)?,
        updated_at: parse_timestamp(row.get::<_, String>(14)?, 14)?,
    })
}

const PORT_COLUMNS: &str = "SELECT id, application_id, name, protocol, bind_address, internal_port, \
     external_port, visibility, required, created_at, updated_at";

fn row_to_port(row: &rusqlite::Row) -> rusqlite::Result<ApplicationPort> {
    Ok(ApplicationPort {
        id: parse_uuid(row.get::<_, String>(0)?, 0)?,
        application_id: parse_uuid(row.get::<_, String>(1)?, 1)?,
        name: row.get(2)?,
        protocol: protocol_from_str(&row.get::<_, String>(3)?),
        bind_address: row.get(4)?,
        internal_port: row.get(5)?,
        external_port: row.get(6)?,
        visibility: visibility_from_str(&row.get::<_, String>(7)?),
        required: row.get(8)?,
        created_at: parse_timestamp(row.get::<_, String>(9)?, 9)?,
        updated_at: parse_timestamp(row.get::<_, String>(10)?, 10)?,
    })
}

fn visibility_to_str(value: PortVisibility) -> &'static str {
    match value {
        PortVisibility::Public => "public",
        PortVisibility::VibeNetwork => "vibe_network",
        PortVisibility::Localhost => "localhost",
        PortVisibility::Custom => "custom",
    }
}

fn visibility_from_str(value: &str) -> PortVisibility {
    match value {
        "vibe_network" => PortVisibility::VibeNetwork,
        "localhost" => PortVisibility::Localhost,
        "custom" => PortVisibility::Custom,
        _ => PortVisibility::Public,
    }
}

/// Both of these used to `expect()`, on the reasoning that VibeSSH is the
/// only writer of this database. That does not survive contact with a
/// release build: the release profile sets `panic = "abort"`, so a single
/// malformed value - a half-written row after a power cut, a hand-edited
/// database, a file restored from a partial backup - aborts the entire
/// desktop app with no dialog and no log, on every launch, because these
/// run on the read path every list view uses. See
/// `server_repository::parse_uuid` for the same change and reasoning.
fn parse_uuid(value: String, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|err| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(err)))
}

fn parse_timestamp(value: String, column: usize) -> rusqlite::Result<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(err)))
}

fn runtime_type_to_str(value: RuntimeType) -> &'static str {
    match value {
        RuntimeType::LocalProcess => "local_process",
        RuntimeType::RemoteProcess => "remote_process",
        RuntimeType::Systemd => "systemd",
        RuntimeType::Docker => "docker",
    }
}

fn runtime_type_from_str(value: &str) -> RuntimeType {
    match value {
        "remote_process" => RuntimeType::RemoteProcess,
        "systemd" => RuntimeType::Systemd,
        "docker" => RuntimeType::Docker,
        _ => RuntimeType::LocalProcess,
    }
}

fn status_to_str(value: ApplicationStatus) -> &'static str {
    match value {
        ApplicationStatus::Unknown => "unknown",
        ApplicationStatus::Starting => "starting",
        ApplicationStatus::Running => "running",
        ApplicationStatus::Stopping => "stopping",
        ApplicationStatus::Stopped => "stopped",
        ApplicationStatus::Failed => "failed",
    }
}

fn status_from_str(value: &str) -> ApplicationStatus {
    match value {
        "starting" => ApplicationStatus::Starting,
        "running" => ApplicationStatus::Running,
        "stopping" => ApplicationStatus::Stopping,
        "stopped" => ApplicationStatus::Stopped,
        "failed" => ApplicationStatus::Failed,
        _ => ApplicationStatus::Unknown,
    }
}

fn health_check_type_to_str(value: HealthCheckType) -> &'static str {
    match value {
        HealthCheckType::Process => "process",
        HealthCheckType::Tcp => "tcp",
        HealthCheckType::Http => "http",
        HealthCheckType::MinecraftStatus => "minecraft_status",
    }
}

fn health_check_type_from_str(value: &str) -> HealthCheckType {
    match value {
        "tcp" => HealthCheckType::Tcp,
        "http" => HealthCheckType::Http,
        "minecraft_status" => HealthCheckType::MinecraftStatus,
        _ => HealthCheckType::Process,
    }
}

fn protocol_to_str(value: PortProtocol) -> &'static str {
    match value {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

fn protocol_from_str(value: &str) -> PortProtocol {
    match value {
        "udp" => PortProtocol::Udp,
        _ => PortProtocol::Tcp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ApplicationLocation;

    fn temp_repo() -> (ApplicationRepository, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("vibessh-app-test-{}.sqlite3", Uuid::new_v4()));
        (ApplicationRepository::open(&path).unwrap(), path)
    }

    fn local_input(name: &str) -> CreateApplicationInput {
        CreateApplicationInput {
            server_id: None,
            name: name.to_string(),
            description: None,
            blueprint_id: "generic-java".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::LocalProcess,
            working_directory: "/tmp/app".to_string(),
            environment: vec![EnvironmentVariable { key: "FOO".to_string(), value: "bar".to_string(), is_secret: false }],
            ports: vec![PortInput {
                name: "game".to_string(),
                protocol: PortProtocol::Tcp,
                bind_address: "0.0.0.0".to_string(),
                internal_port: 25565,
                external_port: None,
                visibility: crate::models::PortVisibility::Public,
                required: true,
            }],
            runtime_config: serde_json::json!({ "jar": "server.jar" }),
            metadata: serde_json::json!({}),
        }
    }

    /// Switching an Application's blueprint has to leave everything else
    /// alone - its ports and environment are the reason somebody adopted it
    /// rather than recreating it, and losing them to a type change would be
    /// exactly the data loss the switch is meant to avoid.
    #[test]
    fn set_blueprint_changes_the_blueprint_and_nothing_else() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("Adopted")).unwrap();

        repo.set_blueprint(detail.application.id, "paper", 3).unwrap();

        let after = repo.get(detail.application.id).unwrap().unwrap();
        assert_eq!(after.application.blueprint_id, "paper");
        assert_eq!(after.application.blueprint_version, 3);
        assert_eq!(after.application.name, "Adopted");
        assert_eq!(after.application.working_directory, "/tmp/app");
        assert_eq!(after.application.runtime_type, RuntimeType::LocalProcess);
        assert_eq!(after.ports.len(), 1);
        assert_eq!(after.environment.len(), 1);
        assert_eq!(after.runtime_config, serde_json::json!({ "jar": "server.jar" }));
    }

    #[test]
    fn set_blueprint_on_an_application_that_is_not_there_says_so() {
        let (repo, _path) = temp_repo();

        let err = repo.set_blueprint(Uuid::new_v4(), "paper", 1).unwrap_err();

        assert!(matches!(err, AppError::NotFound(_)), "expected NotFound, got {err:?}");
    }

    #[test]
    fn create_then_get_round_trips_every_field_including_related_tables() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("Paper Server")).unwrap();

        assert_eq!(detail.application.name, "Paper Server");
        assert_eq!(detail.application.location(), ApplicationLocation::Local);
        assert_eq!(detail.application.status, ApplicationStatus::Unknown);
        assert_eq!(detail.environment.len(), 1);
        assert_eq!(detail.environment[0].key, "FOO");
        assert_eq!(detail.ports.len(), 1);
        assert_eq!(detail.ports[0].internal_port, 25565);
        assert!(detail.ports[0].required);
        assert_eq!(detail.runtime_config["jar"], "server.jar");

        let loaded = repo.get(detail.application.id).unwrap().unwrap();
        assert_eq!(loaded.application.id, detail.application.id);
        assert_eq!(loaded.ports.len(), 1);
    }

    #[test]
    fn create_with_a_missing_server_id_is_a_clean_error_not_a_raw_constraint_failure() {
        let (repo, _path) = temp_repo();
        let mut input = local_input("Remote App");
        input.server_id = Some(Uuid::new_v4());
        input.runtime_type = RuntimeType::Systemd;

        let err = repo.create(&input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn update_status_does_not_touch_updated_at_the_same_way_update_does() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("App")).unwrap();
        let original_updated_at = detail.application.updated_at;

        repo.update_status(detail.application.id, ApplicationStatus::Running).unwrap();
        let after = repo.get(detail.application.id).unwrap().unwrap();
        assert_eq!(after.application.status, ApplicationStatus::Running);
        assert!(after.application.last_status_check_at.is_some());
        assert_eq!(after.application.updated_at, original_updated_at, "update_status must not bump updated_at");
    }

    #[test]
    fn adding_a_colliding_port_is_rejected() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("App")).unwrap();

        let collision = PortInput {
            name: "duplicate".to_string(),
            protocol: PortProtocol::Tcp,
            bind_address: "0.0.0.0".to_string(),
            internal_port: 25565,
            external_port: None,
            visibility: crate::models::PortVisibility::Public,
            required: false,
        };
        let err = repo.add_port(detail.application.id, &collision).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn a_required_port_cannot_be_removed() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("App")).unwrap();
        let required_port = &detail.ports[0];

        let err = repo.remove_port(detail.application.id, required_port.id).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn a_non_required_port_can_be_added_and_removed() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("App")).unwrap();

        let added = repo
            .add_port(
                detail.application.id,
                &PortInput {
                    name: "query".to_string(),
                    protocol: PortProtocol::Udp,
                    bind_address: "0.0.0.0".to_string(),
                    internal_port: 25566,
                    external_port: None,
                    visibility: crate::models::PortVisibility::Public,
                    required: false,
                },
            )
            .unwrap();
        assert_eq!(repo.list_ports(detail.application.id).unwrap().len(), 2);

        repo.remove_port(detail.application.id, added.id).unwrap();
        assert_eq!(repo.list_ports(detail.application.id).unwrap().len(), 1);
    }

    #[test]
    fn list_by_server_only_returns_that_servers_applications() {
        let (repo, path) = temp_repo();
        // A real server row is needed for the foreign key to succeed.
        let server_repo = crate::storage::server_repository::ServerRepository::open(&path).unwrap();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Host".to_string(),
                host: "203.0.113.10".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("hunter2".to_string()),
                key_passphrase: None,
            })
            .unwrap();

        let mut remote_input = local_input("Remote App");
        remote_input.server_id = Some(server.id);
        remote_input.runtime_type = RuntimeType::Systemd;
        let remote = repo.create(&remote_input).unwrap();
        repo.create(&local_input("Local App")).unwrap();

        let for_server = repo.list_by_server(server.id).unwrap();
        assert_eq!(for_server.len(), 1);
        assert_eq!(for_server[0].id, remote.application.id);
        assert_eq!(repo.list().unwrap().len(), 2);
    }

    #[test]
    fn deleting_an_application_cascades_its_related_rows() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("App")).unwrap();

        repo.delete(detail.application.id).unwrap();
        assert!(repo.get(detail.application.id).unwrap().is_none());

        let err = repo.delete(detail.application.id).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn set_environment_replaces_the_whole_set() {
        let (repo, _path) = temp_repo();
        let detail = repo.create(&local_input("App")).unwrap();

        repo.set_environment(
            detail.application.id,
            &[EnvironmentVariable { key: "NEW_KEY".to_string(), value: "1".to_string(), is_secret: false }],
        )
        .unwrap();

        let loaded = repo.get(detail.application.id).unwrap().unwrap();
        assert_eq!(loaded.environment.len(), 1);
        assert_eq!(loaded.environment[0].key, "NEW_KEY");
    }
}
