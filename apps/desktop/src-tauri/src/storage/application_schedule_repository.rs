//! SQLite-backed storage for `application_schedules` - see
//! `storage::migrations` (migration 18) and `services::schedule_service`.
//! Its own connection to the same db file, the pattern every other
//! per-concern repository here follows.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationSchedule, ScheduleAction, ScheduleInput};

pub struct ApplicationScheduleRepository {
    conn: Mutex<Connection>,
}

const COLUMNS: &str = "id, application_id, name, cron, action, enabled, created_at";

impl ApplicationScheduleRepository {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        let mut conn = super::open_connection(db_path, "application schedule")?;
        super::schema::migrate(&mut conn, db_path, "schedules")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("schedules repository connection mutex poisoned")
    }

    pub fn list(&self, application_id: Uuid) -> AppResult<Vec<ApplicationSchedule>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(&format!("SELECT {COLUMNS} FROM application_schedules WHERE application_id = ?1 ORDER BY created_at"))
            .map_err(|err| AppError::Storage(format!("failed to prepare the schedule list query: {err}")))?;
        let rows = stmt
            .query_map(params![application_id.to_string()], row_to_schedule)
            .map_err(|err| AppError::Storage(format!("failed to list schedules: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a schedule row: {err}")))
    }

    pub fn get(&self, id: Uuid) -> AppResult<Option<ApplicationSchedule>> {
        self.lock()
            .query_row(&format!("SELECT {COLUMNS} FROM application_schedules WHERE id = ?1"), params![id.to_string()], row_to_schedule)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the schedule: {err}")))
    }

    /// Stores `input` as-is. Validating it is the service's job, which knows
    /// the cron rules; this only refuses what the schema itself refuses.
    pub fn create(&self, application_id: Uuid, input: &ScheduleInput) -> AppResult<ApplicationSchedule> {
        let schedule = ApplicationSchedule {
            id: Uuid::new_v4(),
            application_id,
            name: input.name.clone(),
            cron: input.cron.clone(),
            action: input.action,
            enabled: input.enabled,
            created_at: Utc::now(),
        };
        self.insert(&schedule)?;
        Ok(schedule)
    }

    /// Writes a whole row, id and creation time included - what `create`
    /// uses, and what puts a deleted row back exactly as it was.
    pub fn insert(&self, schedule: &ApplicationSchedule) -> AppResult<()> {
        self.lock()
            .execute(
                &format!("INSERT INTO application_schedules ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"),
                params![
                    schedule.id.to_string(),
                    schedule.application_id.to_string(),
                    schedule.name,
                    schedule.cron,
                    schedule.action.as_str(),
                    schedule.enabled,
                    schedule.created_at.to_rfc3339()
                ],
            )
            .map_err(|err| AppError::Storage(format!("failed to save the schedule: {err}")))?;
        Ok(())
    }

    pub fn update(&self, id: Uuid, input: &ScheduleInput) -> AppResult<()> {
        let changed = self
            .lock()
            .execute(
                "UPDATE application_schedules SET name = ?2, cron = ?3, action = ?4, enabled = ?5 WHERE id = ?1",
                params![id.to_string(), input.name, input.cron, input.action.as_str(), input.enabled],
            )
            .map_err(|err| AppError::Storage(format!("failed to update the schedule: {err}")))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("schedule {id}")));
        }
        Ok(())
    }

    pub fn delete(&self, id: Uuid) -> AppResult<()> {
        self.lock()
            .execute("DELETE FROM application_schedules WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete the schedule: {err}")))?;
        Ok(())
    }

    /// Hands every schedule of `from` to `to` - what a migration does, since
    /// the migrated Application is a new row with a new id.
    pub fn reassign(&self, from: Uuid, to: Uuid) -> AppResult<usize> {
        self.lock()
            .execute("UPDATE application_schedules SET application_id = ?2 WHERE application_id = ?1", params![from.to_string(), to.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to move the schedules: {err}")))
    }
}

fn row_to_schedule(row: &Row<'_>) -> rusqlite::Result<ApplicationSchedule> {
    let text = |index: usize| row.get::<_, String>(index);
    let parse_uuid = |index: usize, value: String| {
        Uuid::parse_str(&value).map_err(|err| rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err)))
    };
    let action = text(4)?;
    let created_at = text(6)?;
    Ok(ApplicationSchedule {
        id: parse_uuid(0, text(0)?)?,
        application_id: parse_uuid(1, text(1)?)?,
        name: text(2)?,
        cron: text(3)?,
        action: ScheduleAction::parse(&action).map_err(|err| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(err)))?,
        enabled: row.get(5)?,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map(|at| at.with_timezone(&Utc))
            .map_err(|err| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(err)))?,
    })
}
