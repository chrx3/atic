use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{Error, Result};
use crate::models::{Recording, RecordingStatus};

const MIGRATION_1: &str = r#"
CREATE TABLE recordings (
    id            TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    started_at    TEXT NOT NULL,
    duration_secs INTEGER NOT NULL DEFAULT 0,
    mic_path      TEXT,
    system_path   TEXT,
    status        TEXT NOT NULL DEFAULT 'recorded'
);
"#;

/// Acceso a la base de datos SQLite de la aplicación.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Abre (o crea) la base de datos y aplica las migraciones pendientes.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);",
        )?;
        let current: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;

        if current < 1 {
            self.conn.execute_batch(MIGRATION_1)?;
            self.conn
                .execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }
        Ok(())
    }

    pub fn insert_recording(&self, rec: &Recording) -> Result<()> {
        self.conn.execute(
            "INSERT INTO recordings
                (id, title, started_at, duration_secs, mic_path, system_path, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                rec.id,
                rec.title,
                rec.started_at.to_rfc3339(),
                rec.duration_secs,
                rec.mic_path,
                rec.system_path,
                rec.status.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn list_recordings(&self) -> Result<Vec<Recording>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, started_at, duration_secs, mic_path, system_path, status
             FROM recordings
             ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_recording)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_recording(&self, id: &str) -> Result<Option<Recording>> {
        let rec = self
            .conn
            .query_row(
                "SELECT id, title, started_at, duration_secs, mic_path, system_path, status
                 FROM recordings WHERE id = ?1",
                params![id],
                row_to_recording,
            )
            .optional()?;
        Ok(rec)
    }

    pub fn update_status(&self, id: &str, status: RecordingStatus) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE recordings SET status = ?2 WHERE id = ?1",
            params![id, status.as_str()],
        )?;
        if changed == 0 {
            return Err(Error::RecordingNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn update_title(&self, id: &str, title: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE recordings SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        if changed == 0 {
            return Err(Error::RecordingNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_recording(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM recordings WHERE id = ?1", params![id])?;
        Ok(())
    }
}

fn row_to_recording(row: &Row<'_>) -> rusqlite::Result<Recording> {
    let started_raw: String = row.get("started_at")?;
    let started_at = DateTime::parse_from_rfc3339(&started_raw)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let status_raw: String = row.get("status")?;
    let status = RecordingStatus::parse(&status_raw).unwrap_or(RecordingStatus::Error);

    Ok(Recording {
        id: row.get("id")?,
        title: row.get("title")?,
        started_at,
        duration_secs: row.get("duration_secs")?,
        mic_path: row.get("mic_path")?,
        system_path: row.get("system_path")?,
        status,
    })
}
