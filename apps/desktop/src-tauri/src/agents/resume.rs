//! La conversación que corre en una consola de la pizarra, para retomarla al
//! reabrir una sesión guardada.
//!
//! Cada CLI deja una pista distinta de qué conversación lleva cada proceso:
//!
//! - Claude Code: `~/.claude/sessions/<pid>.json` con su `sessionId`.
//! - Grok: `~/.grok/active_sessions.json`, una lista de `{ pid, session_id }`.
//! - Codex: mantiene abierto su rollout (`~/.codex/sessions/…/rollout-*-<id>.jsonl`).
//! - Antigravity: mantiene abierto `~/.gemini/antigravity-cli/presence/<id>.lock`.
//! - Cursor: crea `~/.cursor/chats/<hash>/<id>/meta.json` con `cwd` y
//!   `createdAtMs` al arrancar; no queda nada abierto.
//! - OpenCode: una sola base (`opencode.db`) para todas sus sesiones, con
//!   `directory` y `time_created`.
//!
//! Los cuatro primeros se ligan al proceso exacto, subiendo por el árbol hasta
//! la raíz de la consola. Cursor y OpenCode, por carpeta y hora: lo que nació
//! en esa carpeta después de que arrancó la consola y antes de que arrancara la
//! siguiente del mismo agente ahí.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Lo que se sabe de una consola para buscar su conversación.
pub struct ConsoleInfo {
    /// Raíz del PTY (`cmd /K` o la shell).
    pub pid: u32,
    pub cwd: String,
    pub started_ms: i64,
    /// Cuándo arrancó la siguiente consola del mismo agente en la misma
    /// carpeta: lo que nace desde ahí es de esa otra.
    pub until_ms: Option<i64>,
}

/// El id de la conversación que corre en la consola, si se encuentra.
pub fn agent_session(cli: &str, console: &ConsoleInfo) -> Option<String> {
    let parents = super::focus::parent_map();
    let inside = |pid: u32| is_under(pid, console.pid, &parents);
    match cli {
        "claude" => claude_session(&inside),
        "grok" => grok_session(&inside),
        "codex" => codex_session(&inside),
        "agy" => antigravity_session(&inside),
        "cursor-agent" => cursor_session(console),
        "opencode" => opencode_session(console),
        _ => None,
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// ¿`pid` corre dentro del árbol de `root`?
fn is_under(pid: u32, root: u32, parents: &HashMap<u32, u32>) -> bool {
    let mut current = pid;
    for _ in 0..16 {
        if current == root {
            return true;
        }
        match parents.get(&current) {
            Some(&parent) if parent != 0 && parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

/// Las carpetas se comparan sin mayúsculas y con `/`: OpenCode las guarda con
/// `/`, Cursor y Atic con `\`.
fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    !a.is_empty() && norm(a) == norm(b)
}

fn claude_session(inside: &dyn Fn(u32) -> bool) -> Option<String> {
    let dir = super::skills::config_dir()?.join("sessions");
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let text = std::fs::read_to_string(entry.path()).ok()?;
        let (pid, id) = claude_pid_session(&text)?;
        inside(pid).then_some(id)
    })
}

fn claude_pid_session(text: &str) -> Option<(u32, String)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
    Some((pid, v.get("sessionId")?.as_str()?.to_string()))
}

fn grok_session(inside: &dyn Fn(u32) -> bool) -> Option<String> {
    let text = std::fs::read_to_string(home()?.join(".grok").join("active_sessions.json")).ok()?;
    grok_active(&text)
        .into_iter()
        .find_map(|(pid, id)| inside(pid).then_some(id))
}

fn grok_active(text: &str) -> Vec<(u32, String)> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let pid = u32::try_from(item.get("pid")?.as_u64()?).ok()?;
            Some((pid, item.get("session_id")?.as_str()?.to_string()))
        })
        .collect()
}

/// Rollouts tocados en el último mes: una sesión abierta hace más que eso ya
/// no está en ninguna consola.
fn codex_session(inside: &dyn Fn(u32) -> bool) -> Option<String> {
    const RECENT_SECS: u64 = 30 * 24 * 3600;
    let root = super::watch_codex::sessions_root()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut found = None;
    super::watch_codex::for_each_rollout(&root, |path, id| {
        if found.is_some() {
            return;
        }
        let mtime = super::claude_sessions::mtime_secs(path);
        if now.saturating_sub(mtime) > RECENT_SECS {
            return;
        }
        if pids_holding_file(path).into_iter().any(inside) {
            found = Some(id.to_string());
        }
    });
    found
}

fn antigravity_session(inside: &dyn Fn(u32) -> bool) -> Option<String> {
    let dir = home()?
        .join(".gemini")
        .join("antigravity-cli")
        .join("presence");
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lock") {
            return None;
        }
        let id = path.file_stem()?.to_str()?.to_string();
        pids_holding_file(&path)
            .into_iter()
            .any(inside)
            .then_some(id)
    })
}

/// Un chat de Cursor que pudo nacer en esta consola.
#[derive(Debug, PartialEq)]
struct CursorChat {
    id: String,
    cwd: String,
    created_ms: i64,
    updated_ms: i64,
    has_conversation: bool,
}

fn cursor_chat(id: &str, meta: &str) -> Option<CursorChat> {
    let v: Value = serde_json::from_str(meta).ok()?;
    Some(CursorChat {
        id: id.to_string(),
        cwd: v.get("cwd")?.as_str()?.to_string(),
        created_ms: v.get("createdAtMs")?.as_i64()?,
        updated_ms: v.get("updatedAtMs").and_then(Value::as_i64).unwrap_or(0),
        has_conversation: v
            .get("hasConversation")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Margen para un chat que el CLI crea apenas antes de que se registre la
/// hora de la consola.
const START_SLACK_MS: i64 = 5_000;

fn in_window(created_ms: i64, console: &ConsoleInfo) -> bool {
    created_ms >= console.started_ms - START_SLACK_MS
        && console.until_ms.is_none_or(|until| created_ms < until)
}

/// El chat con conversación más reciente de los que nacieron en la ventana
/// de la consola: tras un `/new` hay otro.
fn pick_cursor_chat(chats: Vec<CursorChat>, console: &ConsoleInfo) -> Option<String> {
    chats
        .into_iter()
        .filter(|c| {
            c.has_conversation && same_dir(&c.cwd, &console.cwd) && in_window(c.created_ms, console)
        })
        .max_by_key(|c| c.updated_ms)
        .map(|c| c.id)
}

fn cursor_session(console: &ConsoleInfo) -> Option<String> {
    let root = home()?.join(".cursor").join("chats");
    let mut chats = Vec::new();
    for project in std::fs::read_dir(root).ok()?.flatten() {
        let Ok(entries) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for chat in entries.flatten() {
            let path = chat.path();
            let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(meta) = std::fs::read_to_string(path.join("meta.json")) else {
                continue;
            };
            if let Some(found) = cursor_chat(id, &meta) {
                chats.push(found);
            }
        }
    }
    pick_cursor_chat(chats, console)
}

fn opencode_session(console: &ConsoleInfo) -> Option<String> {
    use rusqlite::{Connection, OpenFlags};
    let path = super::watch_opencode::db_path()?;
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    // v2 primero; si no está, el esquema viejo.
    ["session_v2", "session"]
        .iter()
        .find_map(|table| opencode_in(&conn, table, console))
}

fn opencode_in(conn: &rusqlite::Connection, table: &str, console: &ConsoleInfo) -> Option<String> {
    let sql = format!(
        "SELECT id, directory, time_created FROM {table}
         WHERE parent_id IS NULL AND time_created >= ?
         ORDER BY time_updated DESC"
    );
    let mut stmt = conn.prepare(&sql).ok()?;
    let rows = stmt
        .query_map([console.started_ms - START_SLACK_MS], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, i64>(2)?,
            ))
        })
        .ok()?;
    for (id, dir, created) in rows.flatten() {
        if same_dir(&dir, &console.cwd) && in_window(created, console) {
            return Some(id);
        }
    }
    None
}

/// Los procesos que tienen abierto un archivo, según el Restart Manager de
/// Windows. Codex y Antigravity mantienen abiertos sus archivos de sesión
/// mientras viven.
pub(crate) fn pids_holding_file(path: &Path) -> Vec<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
        RM_PROCESS_INFO,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    let mut handle = 0u32;
    if unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let files = [wide.as_ptr()];
    let mut out = Vec::new();
    let registered = unsafe {
        RmRegisterResources(
            handle,
            1,
            files.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        )
    };
    if registered == ERROR_SUCCESS {
        // Entre la consulta del tamaño y la lectura la lista puede crecer
        // (ERROR_MORE_DATA): se vuelve a intentar.
        for _ in 0..3 {
            let mut needed = 0u32;
            let mut count = 0u32;
            let mut reasons = 0u32;
            let first = unsafe {
                RmGetList(
                    handle,
                    &mut needed,
                    &mut count,
                    std::ptr::null_mut(),
                    &mut reasons,
                )
            };
            if first != ERROR_MORE_DATA {
                break;
            }
            let empty = unsafe { std::mem::zeroed::<RM_PROCESS_INFO>() };
            let mut info = vec![empty; needed as usize];
            count = needed;
            let read = unsafe {
                RmGetList(
                    handle,
                    &mut needed,
                    &mut count,
                    info.as_mut_ptr(),
                    &mut reasons,
                )
            };
            if read == ERROR_SUCCESS {
                out = info[..count as usize]
                    .iter()
                    .map(|p| p.Process.dwProcessId)
                    .collect();
                break;
            }
            if read != ERROR_MORE_DATA {
                break;
            }
        }
    }
    unsafe { RmEndSession(handle) };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console(started_ms: i64, until_ms: Option<i64>) -> ConsoleInfo {
        ConsoleInfo {
            pid: 1,
            cwd: r"C:\Users\x\GitHub".into(),
            started_ms,
            until_ms,
        }
    }

    #[test]
    fn sube_por_el_arbol_hasta_la_consola() {
        let parents = HashMap::from([(30, 20), (20, 10), (10, 4)]);
        assert!(is_under(30, 10, &parents));
        assert!(!is_under(30, 99, &parents));
    }

    #[test]
    fn carpetas_iguales_con_otra_barra_y_mayusculas() {
        assert!(same_dir("C:/Users/x/GitHub", r"c:\users\x\github\"));
        assert!(!same_dir("", ""));
        assert!(!same_dir("C:/Users/x/GitHub", r"C:\Users\x\GitHub2"));
    }

    #[test]
    fn lee_el_pid_de_claude_y_de_grok() {
        assert_eq!(
            claude_pid_session(r#"{"pid":26088,"sessionId":"abc","cwd":"C:\\x"}"#),
            Some((26088, "abc".into()))
        );
        let grok =
            r#"[{"session_id":"01a0","pid":16136,"cwd":"C:\\x","opened_at":"t"},{"pid":"no"}]"#;
        assert_eq!(grok_active(grok), vec![(16136, "01a0".into())]);
        assert!(grok_active("no es json").is_empty());
    }

    #[test]
    fn cursor_se_queda_con_el_chat_de_su_ventana() {
        let chat = |id: &str, created: i64, updated: i64, has: bool| CursorChat {
            id: id.into(),
            cwd: r"C:\Users\x\GitHub".into(),
            created_ms: created,
            updated_ms: updated,
            has_conversation: has,
        };
        let chats = || {
            vec![
                chat("antes", 500, 600, true),
                chat("vacio", 1_100, 1_100, false),
                chat("mio", 1_200, 1_900, true),
                chat("de-la-otra", 3_000, 3_500, true),
            ]
        };
        assert_eq!(
            pick_cursor_chat(chats(), &console(2_500 + START_SLACK_MS, None)),
            Some("de-la-otra".into())
        );
        let mine = ConsoleInfo {
            started_ms: 1_000 + START_SLACK_MS,
            ..console(0, Some(3_000))
        };
        assert_eq!(pick_cursor_chat(chats(), &mine), Some("mio".into()));
        assert_eq!(pick_cursor_chat(chats(), &console(50_000, None)), None);
    }

    #[test]
    fn lee_el_meta_de_cursor() {
        let meta = r#"{"schemaVersion":1,"createdAtMs":1790281042712,"hasConversation":true,"updatedAtMs":1790281043507,"cwd":"C:\\x"}"#;
        let chat = cursor_chat("fd47", meta).unwrap();
        assert_eq!(chat.created_ms, 1790281042712);
        assert!(chat.has_conversation);
    }

    #[test]
    fn opencode_toma_la_sesion_raiz_mas_reciente_de_su_carpeta() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (id TEXT, parent_id TEXT, directory TEXT,
                time_created INTEGER, time_updated INTEGER);
             INSERT INTO session_v2 VALUES
                ('vieja', NULL, 'C:/Users/x/GitHub', 100, 200),
                ('mia', NULL, 'C:/Users/x/GitHub', 20000, 30000),
                ('sub', 'mia', 'C:/Users/x/GitHub', 21000, 40000),
                ('otra-carpeta', NULL, 'C:/Users/x/atic', 20000, 50000),
                ('de-la-siguiente', NULL, 'C:/Users/x/GitHub', 60000, 60000);",
        )
        .unwrap();
        let me = console(10_000, Some(50_000));
        assert_eq!(opencode_in(&conn, "session_v2", &me), Some("mia".into()));
        assert_eq!(opencode_in(&conn, "session", &me), None);
    }
}
