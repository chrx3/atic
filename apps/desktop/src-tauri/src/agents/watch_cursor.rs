//! Watcher de Cursor TUI (`cursor-agent`). Las reglas viven en
//! `atic_agents::cursor` (las comparte la pill GPUI).
//!
//! `~/.cursor/chats` es el IDE: no se mira. `acp-sessions` es un store de
//! blobs (a veces cifrado) sin marcador de fin de turno, así que el estado
//! honesto es «proceso vivo». Un `cursor-agent` hijo de `Cursor.exe` se ignora.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

pub use atic_agents::cursor::{acp_root, recent_cwd};

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
                activity: None,
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
}
