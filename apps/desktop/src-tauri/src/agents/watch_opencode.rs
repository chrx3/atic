//! Watcher de OpenCode: SQLite ajeno, solo lectura.
//!
//! `~/.local/share/opencode/opencode.db`. El JSONL no existe. Un
//! `step-finish` con `reason: stop` cierra el turno; `tool-calls` o un
//! `user` al final lo dejan trabajando. Los subagentes (`parent_id`) se
//! ignoran. No se escribe nada.

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
    Ready { preview: Option<String> },
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

pub fn tick_db(path: &Path, now: i64, ignore: &HashSet<String>) -> Vec<AgentPresence> {
    let Some(conn) = open_ro(path) else {
        return Vec::new();
    };
    let cutoff = now.saturating_mul(1000).saturating_sub(LIVE_WINDOW_MS);
    let mut out = Vec::new();
    for row in load_sessions(&conn, cutoff) {
        if ignore.contains(&row.id) {
            continue;
        }
        let status = classify_parts(
            last_role(&conn, &row.id).as_deref(),
            &last_parts(&conn, &row.id),
        );
        let (st, preview) = match status {
            OcStatus::Working => (PresenceStatus::Working, None),
            OcStatus::Ready { preview } => (PresenceStatus::Ready, preview),
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
