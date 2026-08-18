//! Watcher de Cursor TUI (`cursor-agent`).
//!
//! `~/.cursor/chats` es el IDE: no se mira. `acp-sessions` es un store de
//! blobs (a veces cifrado) sin marcador de fin de turno, así que el estado
//! honesto es «proceso vivo». Un `cursor-agent` hijo de `Cursor.exe` se ignora.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tauri::AppHandle;

use super::presence::{self, AgentPresence, PresenceSource, PresenceStatus};

const POLL: Duration = Duration::from_secs(1);
const BACKEND_ID: &str = "cursor";
const BACKEND_NAME: &str = "Cursor";

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn acp_root() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".cursor").join("acp-sessions"))
}

pub fn cwd_from_meta(v: &Value) -> Option<String> {
    v.get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Sesiones ACP con `store.db` (las carpetas vacías son inicios fallidos).
pub fn recent_cwd(root: &Path) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    let mut best: Option<(u64, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let store = path.join("store.db");
        if !store.exists() {
            continue;
        }
        let mtime = super::claude_sessions::mtime_secs(&store);
        let meta_path = path.join("meta.json");
        let cwd = std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| cwd_from_meta(&v))
            .unwrap_or_default();
        if best.as_ref().is_none_or(|(t, _)| mtime >= *t) {
            best = Some((mtime, cwd));
        }
    }
    best.map(|(_, cwd)| cwd).filter(|s| !s.is_empty())
}

pub fn tick(pids: &[u32], cwd: Option<&str>, now: i64) -> Vec<AgentPresence> {
    pids.iter()
        .map(|pid| {
            presence::normalize(AgentPresence {
                id: format!("cursor-{pid}"),
                backend_id: BACKEND_ID.into(),
                backend_name: BACKEND_NAME.into(),
                cwd: cwd.unwrap_or("").to_string(),
                status: PresenceStatus::Working,
                preview: None,
                updated_at: now,
                window: None,
                source: PresenceSource::Process,
            })
        })
        .collect()
}

pub fn sync_registry(presences: &[AgentPresence]) {
    let ids: HashSet<String> = presences.iter().map(|p| p.id.clone()).collect();
    for p in presences {
        presence::upsert(p.clone());
    }
    presence::retain_backend(BACKEND_ID, &ids);
}

fn live_pids() -> Vec<u32> {
    super::focus::agent_tui_pids(BACKEND_ID)
}

pub fn start(app: &AppHandle) {
    if !super::PAGER_ENABLED {
        return;
    }
    let handle = app.clone();
    let _ = std::thread::Builder::new()
        .name("atic-watch-cursor".into())
        .spawn(move || loop {
            std::thread::sleep(POLL);
            let pids = live_pids();
            let cwd = acp_root().and_then(|root| recent_cwd(&root));
            let list = tick(&pids, cwd.as_deref(), now_secs());
            sync_registry(&list);
            super::focus::attach_unique_backend(BACKEND_ID);
            presence::publish(&handle);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_saca_cwd() {
        let v = serde_json::json!({ "schemaVersion": 1, "cwd": "C:\\\\repo" });
        assert_eq!(cwd_from_meta(&v).as_deref(), Some("C:\\\\repo"));
    }

    #[test]
    fn sin_proceso_no_hay_presencia() {
        assert!(tick(&[], Some("/x"), 10).is_empty());
    }

    #[test]
    fn un_pid_es_working() {
        let list = tick(&[42], Some("/repo"), 10);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "cursor-42");
        assert_eq!(list[0].status, PresenceStatus::Working);
        assert_eq!(list[0].source, PresenceSource::Process);
        assert_eq!(list[0].cwd, "/repo");
    }

    #[test]
    fn recent_cwd_ignora_sin_store() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("atic-watch-cursor-{nonce}"));
        let empty = root.join("empty-uuid");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("meta.json"), r#"{"cwd":"/skip"}"#).unwrap();
        let live = root.join("live-uuid");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("meta.json"), r#"{"cwd":"/tui"}"#).unwrap();
        std::fs::write(live.join("store.db"), b"x").unwrap();
        assert_eq!(recent_cwd(&root).as_deref(), Some("/tui"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
