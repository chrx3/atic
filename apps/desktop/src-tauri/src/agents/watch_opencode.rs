//! Watcher de OpenCode. La lectura de su SQLite vive en
//! `atic_agents::opencode` (la comparte la pill GPUI); acá se pasa a la
//! presencia.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atic_agents::seen::{Seen, SeenStatus};
use tauri::AppHandle;

use super::presence::{self, AgentPresence, PresenceSource, PresenceStatus};

pub use atic_agents::opencode::db_path;

const POLL: Duration = Duration::from_secs(1);
const BACKEND_ID: &str = "opencode";
const BACKEND_NAME: &str = "OpenCode";

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn to_presence(seen: Seen) -> AgentPresence {
    presence::normalize(AgentPresence {
        id: seen.id,
        backend_id: BACKEND_ID.into(),
        backend_name: BACKEND_NAME.into(),
        cwd: seen.cwd,
        status: match seen.status {
            SeenStatus::Working => PresenceStatus::Working,
            SeenStatus::Ready => PresenceStatus::Ready,
            SeenStatus::Idle => PresenceStatus::Idle,
        },
        preview: seen.preview,
        updated_at: seen.updated,
        window: None,
        source: PresenceSource::Jsonl,
        activity: None,
    })
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
            let list: Vec<AgentPresence> = atic_agents::opencode::sessions(&path, now_secs(), &ignore)
                .into_iter()
                .map(to_presence)
                .collect();
            sync_registry(&list);
            super::focus::attach_unique_backend(BACKEND_ID);
            presence::publish(&handle);
        });
}
