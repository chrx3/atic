//! Watcher de OpenCode: SQLite ajeno, solo lectura.
//!
//! `~/.local/share/opencode/opencode.db`, en dos esquemas: el viejo
//! (`session`/`message`/`part`, con `json_extract(data,'$.role')` y
//! `step-finish`) y el de OpenCode v2 (`session_v2`/`session_message`, con la
//! columna `type` y el `time.completed` del mensaje). Se detecta cuál hay y se
//! lee ese; si el de v2 falla a mitad, se cae al viejo. Los subagentes
//! (`parent_id`) se ignoran. No se escribe nada.
//!
//! `waiting` no se emite: v2 tiene `session_pending`/`permission`, pero sin
//! filas reales para verificar qué `delivery`/`type` significan «esperando» —
//! un falso waiting encendería la tarjeta de permisos. Queda para un corte
//! siguiente, con la consulta validada contra una sesión que pida permiso.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use tauri::AppHandle;

use super::presence::{self, AgentPresence, PresenceSource, PresenceStatus};

const POLL: Duration = Duration::from_secs(1);
const LIVE_WINDOW_MS: i64 = 15 * 60 * 1000;
const DISAPPEAR_SECS: i64 = 30 * 60;
const PREVIEW_MAX: usize = 120;
const BACKEND_ID: &str = "opencode";
const BACKEND_NAME: &str = "OpenCode";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcStatus {
    Working,
    Ready {
        preview: Option<String>,
    },
    /// Reserva del esquema viejo/otros orígenes: en v2 el `idle` es el cierre
    /// del turno y clasifica como `Ready` con el preview del último mensaje.
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartHint {
    pub kind: String,
    pub reason: Option<String>,
    pub tool_status: Option<String>,
    pub text: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn db_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| {
            PathBuf::from(h)
                .join(".local")
                .join("share")
                .join("opencode")
                .join("opencode.db")
        })
}

fn first_line(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(text)
        .trim();
    if line.is_empty() {
        None
    } else {
        Some(line.chars().take(PREVIEW_MAX).collect())
    }
}

pub fn hint_from_part(data: &Value) -> PartHint {
    PartHint {
        kind: data
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        reason: data
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_status: data
            .pointer("/state/status")
            .and_then(Value::as_str)
            .map(str::to_string),
        text: data.get("text").and_then(Value::as_str).map(str::to_string),
    }
}

/// `parts` van de más nuevo a más viejo.
pub fn classify_parts(last_role: Option<&str>, parts: &[PartHint]) -> OcStatus {
    if last_role == Some("user") {
        return OcStatus::Working;
    }
    let preview = parts.iter().find_map(|p| {
        if p.kind == "text" {
            p.text.as_deref().and_then(first_line)
        } else {
            None
        }
    });
    for p in parts {
        match p.kind.as_str() {
            "step-finish" if p.reason.as_deref() == Some("stop") => {
                return OcStatus::Ready { preview };
            }
            "step-finish" | "step-start" => return OcStatus::Working,
            "tool" if p.tool_status.as_deref().is_some_and(|s| s != "completed") => {
                return OcStatus::Working;
            }
            _ => {}
        }
    }
    OcStatus::Working
}

fn open_ro(path: &Path) -> Option<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

/// Los dos esquemas que hay en la calle.
#[derive(Debug, Clone, Copy)]
enum Schema {
    /// `session` + `message` + `part` (OpenCode viejo).
    V1,
    /// `session_v2` + `session_message` (OpenCode v2).
    V2,
}

/// ¿Está la tabla de sesiones de v2? Nada más que eso decide el camino; si el
/// v2 falla después (columna que no está), `tick_db` cae al viejo.
fn schema_of(conn: &Connection) -> Schema {
    let has_v2 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('session_v2') WHERE name = 'id'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    if has_v2 {
        Schema::V2
    } else {
        Schema::V1
    }
}

struct SessionRow {
    id: String,
    cwd: String,
    updated_ms: i64,
}

fn load_sessions(conn: &Connection, cutoff_ms: i64) -> Vec<SessionRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, directory, time_updated FROM session
         WHERE parent_id IS NULL
           AND time_archived IS NULL
           AND time_updated >= ?",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map([cutoff_ms], |row| {
        Ok(SessionRow {
            id: row.get(0)?,
            cwd: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            updated_ms: row.get(2)?,
        })
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.flatten().collect()
}

/// Igual que `load_sessions` pero contra v2. `None` = la consulta no se pudo
/// armar o leer (ahí `tick_db` se queda con el camino viejo).
fn load_sessions_v2(conn: &Connection, cutoff_ms: i64) -> Option<Vec<SessionRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, directory, time_updated FROM session_v2
             WHERE parent_id IS NULL
               AND time_archived IS NULL
               AND time_updated >= ?",
        )
        .ok()?;
    let rows = stmt
        .query_map([cutoff_ms], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                cwd: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                updated_ms: row.get(2)?,
            })
        })
        .ok()?;
    Some(rows.flatten().collect())
}

fn last_role(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT json_extract(data, '$.role') FROM message
         WHERE session_id = ? ORDER BY time_created DESC LIMIT 1",
        [id],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

fn last_parts(conn: &Connection, id: &str) -> Vec<PartHint> {
    let Ok(mut stmt) = conn
        .prepare("SELECT data FROM part WHERE session_id = ? ORDER BY time_created DESC LIMIT 12")
    else {
        return Vec::new();
    };
    let rows = stmt.query_map([id], |row| row.get::<_, String>(0));
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.flatten()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .map(|v| hint_from_part(&v))
        .collect()
}

/// Última fila de `session_message`, por `seq`: el orden real de la sesión.
fn last_message_v2(conn: &Connection, id: &str) -> Option<(String, Value)> {
    let raw: (String, String) = conn
        .query_row(
            "SELECT type, data FROM session_message
             WHERE session_id = ? ORDER BY seq DESC LIMIT 1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()?;
    let data = serde_json::from_str::<Value>(&raw.1).ok()?;
    Some((raw.0, data))
}

/// Primer `content[]` de tipo `text`, primera línea.
pub fn preview_from_content(data: &Value) -> Option<String> {
    data.get("content")?
        .as_array()?
        .iter()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .and_then(|part| part.get("text").and_then(Value::as_str))
        .and_then(first_line)
}

/// Estado desde la última fila de `session_message` (esquema v2).
///
/// `type` ya trae el rol (`user`/`assistant`/`idle`/…); `data` trae
/// `time.created/streamed/completed` y `content[]`. Un `assistant` sin
/// `completed` sigue streameando: eso es «contestando», que la pill pinta
/// igual que trabajar.
///
/// El `idle` de v2 NO es «sin respuesta»: es el cierre del turno — el rol
/// describe el cierre, no el mensaje. Quien clasifica acá puede pedir el
/// preview del último `assistant` para no perder el inicio de la respuesta.
pub fn classify_v2(kind: &str, data: &Value) -> OcStatus {
    match kind {
        "user" => OcStatus::Working,
        "idle" => OcStatus::Ready { preview: None },
        "assistant" => {
            if data
                .pointer("/time/completed")
                .is_none_or(|completed| completed.is_null())
            {
                OcStatus::Working
            } else {
                OcStatus::Ready {
                    preview: preview_from_content(data),
                }
            }
        }
        _ => OcStatus::Working,
    }
}

/// Última fila `assistant` de la sesión, para el preview del cierre.
///
/// El `idle` llega después del assistant y no trae texto: sin esta mirada
/// atrás, «Listo» aparece sin decir qué contestó el agente.
fn last_assistant_preview_v2(conn: &Connection, id: &str) -> Option<String> {
    let raw: String = conn
        .query_row(
            "SELECT data FROM session_message
             WHERE session_id = ? AND type = 'assistant'
             ORDER BY seq DESC LIMIT 1",
            [id],
            |row| row.get(0),
        )
        .ok()?;
    let data = serde_json::from_str::<Value>(&raw).ok()?;
    preview_from_content(&data)
}

pub fn tick_db(path: &Path, now: i64, ignore: &HashSet<String>) -> Vec<AgentPresence> {
    let Some(conn) = open_ro(path) else {
        return Vec::new();
    };
    let cutoff = now.saturating_mul(1000).saturating_sub(LIVE_WINDOW_MS);
    // El esquema se decide una vez por tick: si el v2 no se puede leer, todo el
    // tick se hace con el viejo (columnas que sí están).
    let (schema, rows) = match schema_of(&conn) {
        Schema::V2 => match load_sessions_v2(&conn, cutoff) {
            Some(rows) => (Schema::V2, rows),
            None => (Schema::V1, load_sessions(&conn, cutoff)),
        },
        Schema::V1 => (Schema::V1, load_sessions(&conn, cutoff)),
    };
    let mut out = Vec::new();
    for row in rows {
        if ignore.contains(&row.id) {
            continue;
        }
        let status = match schema {
            Schema::V2 => match last_message_v2(&conn, &row.id) {
                // El `idle` es el cierre del turno: el preview vive en el
                // último `assistant`, una fila atrás.
                Some((kind, data)) if kind == "idle" => OcStatus::Ready {
                    preview: last_assistant_preview_v2(&conn, &row.id),
                },
                Some((kind, data)) => classify_v2(&kind, &data),
                // Sin mensajes todavía: recién abierta, se asume ocupada.
                None => OcStatus::Working,
            },
            Schema::V1 => classify_parts(
                last_role(&conn, &row.id).as_deref(),
                &last_parts(&conn, &row.id),
            ),
        };
        let (st, preview) = match status {
            OcStatus::Working => (PresenceStatus::Working, None),
            OcStatus::Ready { preview } => (PresenceStatus::Ready, preview),
            OcStatus::Idle => (PresenceStatus::Idle, None),
        };
        let updated_at = row.updated_ms / 1000;
        if st == PresenceStatus::Ready && now - updated_at >= DISAPPEAR_SECS {
            continue;
        }
        out.push(presence::normalize(AgentPresence {
            id: row.id,
            backend_id: BACKEND_ID.into(),
            backend_name: BACKEND_NAME.into(),
            cwd: row.cwd,
            status: st,
            preview,
            updated_at,
            window: None,
            source: PresenceSource::Jsonl,
            activity: None,
        }));
    }
    out
}

pub fn sync_registry(presences: &[AgentPresence]) {
    let ids: HashSet<String> = presences.iter().map(|p| p.id.clone()).collect();
    for p in presences {
        presence::upsert(p.clone());
    }
    presence::retain_backend(BACKEND_ID, &ids);
}

pub fn start(app: &AppHandle) {
    if !super::PAGER_ENABLED {
        return;
    }
    let handle = app.clone();
    let _ = std::thread::Builder::new()
        .name("atic-watch-opencode".into())
        .spawn(move || loop {
            std::thread::sleep(POLL);
            let Some(path) = db_path() else {
                continue;
            };
            let ignore = super::bridge::live_session_ids();
            let list = tick_db(&path, now_secs(), &ignore);
            sync_registry(&list);
            super::focus::attach_unique_backend(BACKEND_ID);
            presence::publish(&handle);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: &str, reason: Option<&str>, tool: Option<&str>, text: Option<&str>) -> PartHint {
        PartHint {
            kind: kind.into(),
            reason: reason.map(str::to_string),
            tool_status: tool.map(str::to_string),
            text: text.map(str::to_string),
        }
    }

    #[test]
    fn user_abre_trabajo() {
        assert_eq!(classify_parts(Some("user"), &[]), OcStatus::Working);
    }

    #[test]
    fn stop_cierra_con_preview() {
        let parts = vec![
            part("step-finish", Some("stop"), None, None),
            part("text", None, None, Some("listo\nmas")),
        ];
        assert_eq!(
            classify_parts(Some("assistant"), &parts),
            OcStatus::Ready {
                preview: Some("listo".into())
            }
        );
    }

    #[test]
    fn tool_calls_sigue_trabajando() {
        let parts = vec![part("step-finish", Some("tool-calls"), None, None)];
        assert_eq!(classify_parts(Some("assistant"), &parts), OcStatus::Working);
    }

    #[test]
    fn tool_pendiente_es_working() {
        let parts = vec![part("tool", None, Some("running"), None)];
        assert_eq!(classify_parts(Some("assistant"), &parts), OcStatus::Working);
    }

    /* ─── Esquema v2 (`session_v2`/`session_message`) ─────────────────────── */

    #[test]
    fn v2_user_abre_trabajo() {
        let data = serde_json::json!({"time":{"created":1}, "text":"hola"});
        assert_eq!(classify_v2("user", &data), OcStatus::Working);
    }

    #[test]
    fn v2_assistant_sin_completed_sigue_contestando() {
        // `streamed` sin `completed` = el stream está vivo.
        let data = serde_json::json!({
            "time":{"created":1,"streamed":2},
            "content":[{"type":"text","text":"voy por la mitad"}],
        });
        assert_eq!(classify_v2("assistant", &data), OcStatus::Working);
    }

    #[test]
    fn v2_assistant_completed_listo_con_preview() {
        let data = serde_json::json!({
            "time":{"created":1,"streamed":2,"completed":3},
            "content":[
                {"type":"reasoning","text":"pienso"},
                {"type":"text","text":"listo\nmas texto"},
            ],
        });
        assert_eq!(
            classify_v2("assistant", &data),
            OcStatus::Ready {
                preview: Some("listo".into())
            }
        );
    }

    #[test]
    fn v2_assistant_completed_sin_texto_no_tiene_preview() {
        // Turno cerrado en una herramienta: no hay respuesta que mostrar.
        let data = serde_json::json!({
            "time":{"completed":3},
            "content":[{"type":"tool","text":"x"}],
        });
        assert_eq!(
            classify_v2("assistant", &data),
            OcStatus::Ready { preview: None }
        );
    }

    #[test]
    fn v2_idle_clasifica_listo_y_toma_preview_del_assistant() {
        let data = serde_json::json!({"time":{"created":1},"outcome":"succeeded"});
        assert_eq!(
            classify_v2("idle", &data),
            OcStatus::Ready { preview: None }
        );
    }

    #[test]
    fn tick_db_lee_esquema_v2() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("atic-opencode-v2-{nonce}.db"));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (
                id TEXT, project_id TEXT, parent_id TEXT, directory TEXT,
                time_updated INTEGER, time_archived INTEGER
             );
             CREATE TABLE session_message (
                id TEXT, session_id TEXT, type TEXT, seq INTEGER,
                time_created INTEGER, time_updated INTEGER, data TEXT
             );
             CREATE TABLE session (id TEXT, directory TEXT, time_updated INTEGER);",
        )
        .unwrap();
        let now_ms = now_secs() * 1000;
        conn.execute(
            "INSERT INTO session_v2 VALUES ('ses_v2_live', NULL, NULL, '/repo', ?, NULL)",
            [now_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_v2 VALUES ('ses_v2_done', NULL, NULL, '/repo', ?, NULL)",
            [now_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_v2 VALUES ('ses_v2_child', NULL, 'ses_v2_live', '/repo', ?, NULL)",
            [now_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message VALUES ('m1', 'ses_v2_live', 'user', 1, ?, ?, ?)",
            rusqlite::params![now_ms, now_ms, r#"{"time":{"created":1}}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message VALUES ('m2', 'ses_v2_live', 'assistant', 2, ?, ?, ?)",
            rusqlite::params![
                now_ms,
                now_ms,
                r#"{"time":{"created":1,"streamed":2},"content":[{"type":"text","text":"voy"}]}"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message VALUES ('m3', 'ses_v2_done', 'assistant', 1, ?, ?, ?)",
            rusqlite::params![
                now_ms,
                now_ms,
                r#"{"time":{"created":1,"streamed":2,"completed":3},"content":[{"type":"text","text":"hecho"}]}"#
            ],
        )
        .unwrap();
        drop(conn);

        let mut list = tick_db(&path, now_secs(), &HashSet::new());
        list.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(list.len(), 2, "el subagente no se publica");
        assert_eq!(list[0].id, "ses_v2_done");
        assert_eq!(list[0].status, PresenceStatus::Ready);
        assert_eq!(list[0].preview.as_deref(), Some("hecho"));
        assert_eq!(list[0].cwd, "/repo");
        assert_eq!(list[1].id, "ses_v2_live");
        assert_eq!(list[1].status, PresenceStatus::Working);
        assert_eq!(list[1].preview, None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tick_db_lee_sesion_y_salta_subagente() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("atic-opencode-{nonce}.db"));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT, project_id TEXT, parent_id TEXT, directory TEXT,
                time_updated INTEGER, time_archived INTEGER
             );
             CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        let now_ms = now_secs() * 1000;
        conn.execute(
            "INSERT INTO session VALUES ('ses_live', NULL, NULL, '/repo', ?, NULL)",
            [now_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('ses_child', NULL, 'ses_live', '/repo', ?, NULL)",
            [now_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message VALUES ('m1', 'ses_live', ?, ?)",
            rusqlite::params![now_ms, r#"{"role":"assistant"}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part VALUES ('p1', 'ses_live', ?, ?)",
            rusqlite::params![now_ms, r#"{"type":"step-finish","reason":"stop"}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part VALUES ('p0', 'ses_live', ?, ?)",
            rusqlite::params![now_ms - 1, r#"{"type":"text","text":"hecho"}"#],
        )
        .unwrap();
        drop(conn);

        let list = tick_db(&path, now_secs(), &HashSet::new());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "ses_live");
        assert_eq!(list[0].status, PresenceStatus::Ready);
        assert_eq!(list[0].preview.as_deref(), Some("hecho"));
        let _ = std::fs::remove_file(&path);
    }
}
