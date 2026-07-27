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

/// Conversaciones con agentes.
///
/// Los turnos van como **JSON en una columna** y no repartidos en tablas de
/// turnos e items. Dos razones: un hilo se lee y se escribe entero —no hay
/// consulta que pida «los items de tipo herramienta del martes»—, y el modelo
/// canónico vive en la app de escritorio, así que normalizarlo acá obligaría a
/// este crate a conocer un tipo que no le corresponde y a migrarlo cada vez que
/// se le agregue una variante.
///
/// Lo que SÍ sale a columnas es lo que se usa para listar sin abrir nada:
/// backend, carpeta, modelo y cuándo se tocó por última vez.
const MIGRATION_2: &str = r#"
CREATE TABLE agent_threads (
    id               TEXT PRIMARY KEY,
    backend_id       TEXT NOT NULL,
    backend_name     TEXT NOT NULL,
    provider_session TEXT,
    cwd              TEXT NOT NULL DEFAULT '',
    model            TEXT NOT NULL DEFAULT '',
    updated_at       INTEGER NOT NULL,
    turns            TEXT NOT NULL
);
CREATE INDEX agent_threads_updated ON agent_threads (updated_at DESC);
"#;

/// Un hilo de agente tal como se guarda.
#[derive(Debug, Clone)]
pub struct AgentThreadRow {
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
    /// Id con el que el CLI reanuda la conversación. Distinto de `id`.
    pub provider_session: Option<String>,
    pub cwd: String,
    pub model: String,
    /// Segundos desde epoch.
    pub updated_at: i64,
    /// Los turnos, serializados. Esta capa no los interpreta.
    pub turns: String,
}

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
        if current < 2 {
            self.conn.execute_batch(MIGRATION_2)?;
            self.conn
                .execute("INSERT INTO schema_version (version) VALUES (2)", [])?;
        }
        Ok(())
    }

    /// Guarda un hilo de agente, pisando el anterior si ya existía.
    ///
    /// Se llama en los bordes del turno y no con cada delta: los trozos de texto
    /// llegan cada pocos milisegundos y reescribir el hilo entero con cada uno
    /// sería costoso y no aportaría nada — el texto autoritativo llega igual al
    /// cerrar el bloque.
    pub fn save_agent_thread(&self, t: &AgentThreadRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO agent_threads
               (id, backend_id, backend_name, provider_session, cwd, model, updated_at, turns)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
               backend_name     = excluded.backend_name,
               provider_session = excluded.provider_session,
               cwd              = excluded.cwd,
               model            = excluded.model,
               updated_at       = excluded.updated_at,
               turns            = excluded.turns",
            params![
                t.id,
                t.backend_id,
                t.backend_name,
                t.provider_session,
                t.cwd,
                t.model,
                t.updated_at,
                t.turns,
            ],
        )?;
        Ok(())
    }

    /// Los hilos guardados, del más reciente al más viejo.
    ///
    /// `limit` existe porque esto alimenta una lista que se abre y se mira: sin
    /// tope, una máquina con meses de uso cargaría megabytes de conversación
    /// para mostrar diez líneas.
    pub fn list_agent_threads(&self, limit: u32) -> Result<Vec<AgentThreadRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, backend_id, backend_name, provider_session, cwd, model, updated_at, turns
             FROM agent_threads ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], agent_thread_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_agent_thread(&self, id: &str) -> Result<Option<AgentThreadRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, backend_id, backend_name, provider_session, cwd, model, updated_at, turns
             FROM agent_threads WHERE id = ?1",
        )?;
        Ok(stmt.query_row([id], agent_thread_from_row).optional()?)
    }

    pub fn delete_agent_thread(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM agent_threads WHERE id = ?1", [id])?;
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

fn agent_thread_from_row(row: &Row<'_>) -> rusqlite::Result<AgentThreadRow> {
    Ok(AgentThreadRow {
        id: row.get("id")?,
        backend_id: row.get("backend_id")?,
        backend_name: row.get("backend_name")?,
        provider_session: row.get("provider_session")?,
        cwd: row.get("cwd")?,
        model: row.get("model")?,
        updated_at: row.get("updated_at")?,
        turns: row.get("turns")?,
    })
}

#[cfg(test)]
mod agent_thread_tests {
    use super::*;

    fn db() -> Db {
        let dir = std::env::temp_dir().join(format!(
            "atic-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Db::open(&dir.join("atic.db3")).unwrap()
    }

    fn row(id: &str, at: i64) -> AgentThreadRow {
        AgentThreadRow {
            id: id.into(),
            backend_id: "claude-code".into(),
            backend_name: "Claude Code".into(),
            provider_session: Some("s1".into()),
            cwd: "C:/p".into(),
            model: "opus".into(),
            updated_at: at,
            turns: r#"[{"id":"t1"}]"#.into(),
        }
    }

    /// Lo que el registro plano no permitía: la conversación vuelve del disco.
    #[test]
    fn un_hilo_va_y_vuelve_entero() {
        let db = db();
        db.save_agent_thread(&row("h1", 100)).unwrap();

        let back = db.get_agent_thread("h1").unwrap().unwrap();
        assert_eq!(back.backend_id, "claude-code");
        assert_eq!(back.provider_session.as_deref(), Some("s1"));
        assert_eq!(back.cwd, "C:/p");
        assert_eq!(back.turns, r#"[{"id":"t1"}]"#);
    }

    /// Guardar dos veces la misma sesión la ACTUALIZA. Sin esto, cada borde de
    /// turno insertaría una fila nueva y la lista se llenaría de duplicados.
    #[test]
    fn volver_a_guardar_pisa_en_vez_de_duplicar() {
        let db = db();
        db.save_agent_thread(&row("h1", 100)).unwrap();
        let mut segunda = row("h1", 200);
        segunda.turns = r#"[{"id":"t1"},{"id":"t2"}]"#.into();
        db.save_agent_thread(&segunda).unwrap();

        assert_eq!(db.list_agent_threads(10).unwrap().len(), 1);
        let back = db.get_agent_thread("h1").unwrap().unwrap();
        assert_eq!(back.updated_at, 200);
        assert_eq!(back.turns, r#"[{"id":"t1"},{"id":"t2"}]"#);
    }

    /// Del más reciente al más viejo: es el orden en el que se buscan.
    #[test]
    fn se_listan_por_actividad_reciente() {
        let db = db();
        db.save_agent_thread(&row("viejo", 100)).unwrap();
        db.save_agent_thread(&row("nuevo", 300)).unwrap();
        db.save_agent_thread(&row("medio", 200)).unwrap();

        let ids: Vec<_> = db
            .list_agent_threads(10)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["nuevo", "medio", "viejo"]);
    }

    #[test]
    fn el_limite_recorta_la_lista() {
        let db = db();
        for i in 0..5 {
            db.save_agent_thread(&row(&format!("h{i}"), i)).unwrap();
        }
        assert_eq!(db.list_agent_threads(2).unwrap().len(), 2);
    }

    #[test]
    fn un_hilo_que_no_existe_no_es_un_error() {
        assert!(db().get_agent_thread("nada").unwrap().is_none());
    }

    #[test]
    fn se_puede_borrar() {
        let db = db();
        db.save_agent_thread(&row("h1", 1)).unwrap();
        db.delete_agent_thread("h1").unwrap();
        assert!(db.get_agent_thread("h1").unwrap().is_none());
    }

    /// La migración 2 corre sobre una base que ya tenía la 1, sin tocarla.
    #[test]
    fn la_migracion_convive_con_las_grabaciones() {
        let db = db();
        assert!(db.list_recordings().unwrap().is_empty());
        db.save_agent_thread(&row("h1", 1)).unwrap();
        assert_eq!(db.list_agent_threads(10).unwrap().len(), 1);
    }
}
