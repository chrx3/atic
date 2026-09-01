//! Watcher del JSONL vivo de Codex TUI.
//!
//! Los rollouts viven en `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//! `task_started` / `user_message` abren trabajo; `task_complete` lo cierra
//! con `last_agent_message`. No hay línea de permiso: `waiting` sigue siendo
//! solo del hook. Se ignoran `atic`, Codex Desktop y subagentes.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tauri::AppHandle;

use super::presence::{self, AgentPresence, PresenceSource, PresenceStatus};
use super::watch_claude::{consume, Tail};

const POLL: Duration = Duration::from_secs(1);
const LIVE_WINDOW_SECS: u64 = 15 * 60;
const DISAPPEAR_SECS: i64 = 30 * 60;
const PREVIEW_MAX: usize = 120;
const FIRST_READ_TAIL: u64 = 256 * 1024;
const HEAD_PEEK: usize = 16 * 1024;
const BACKEND_ID: &str = "codex";
const BACKEND_NAME: &str = "Codex";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Pending,
    Tui,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Ignore,
    Prompt,
    EndTurn { preview: Option<String> },
    Activity,
}

#[derive(Debug)]
struct Tracked {
    path: PathBuf,
    tail: Tail,
    cwd: String,
    status: PresenceStatus,
    preview: Option<String>,
    updated_at: i64,
    origin: Origin,
}

#[derive(Debug, Default)]
pub struct WatchState {
    tracked: HashMap<String, Tracked>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mtime_secs(path: &Path) -> u64 {
    super::claude_sessions::mtime_secs(path)
}

pub fn sessions_root() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".codex").join("sessions"))
}

/// `rollout-YYYY-MM-DDTHH-MM-SS-<uuid>` → uuid.
pub fn session_id_from_stem(stem: &str) -> Option<&str> {
    let rest = stem.strip_prefix("rollout-")?;
    if rest.len() > 20 && rest.as_bytes().get(19) == Some(&b'-') {
        Some(&rest[20..])
    } else {
        None
    }
}

fn item_text(v: &Value) -> Option<&str> {
    v.pointer("/payload/item/content")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|c| {
            if c.get("type").and_then(Value::as_str) == Some("Text") {
                c.get("text").and_then(Value::as_str)
            } else {
                None
            }
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

pub fn classify(v: &Value) -> LineKind {
    match v.get("type").and_then(Value::as_str) {
        Some("event_msg") => match v.pointer("/payload/type").and_then(Value::as_str) {
            Some("task_started") | Some("user_message") => LineKind::Prompt,
            Some("task_complete") | Some("turn_aborted") => LineKind::EndTurn {
                preview: v
                    .pointer("/payload/last_agent_message")
                    .and_then(Value::as_str)
                    .and_then(first_line),
            },
            Some("item_completed") => match v.pointer("/payload/item/type").and_then(Value::as_str)
            {
                Some("UserMessage") => LineKind::Prompt,
                Some("AgentMessage") => {
                    if v.pointer("/payload/item/phase").and_then(Value::as_str)
                        == Some("final_answer")
                    {
                        LineKind::EndTurn {
                            preview: item_text(v).and_then(first_line),
                        }
                    } else {
                        LineKind::Ignore
                    }
                }
                _ => LineKind::Ignore,
            },
            _ => LineKind::Ignore,
        },
        Some("response_item") => match v.pointer("/payload/type").and_then(Value::as_str) {
            Some("function_call") | Some("custom_tool_call") | Some("mcp_tool_call") => {
                LineKind::Activity
            }
            _ => LineKind::Ignore,
        },
        _ => LineKind::Ignore,
    }
}

pub fn origin_of(v: &Value) -> Option<bool> {
    if v.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = v.get("payload").unwrap_or(v);
    let originator = payload
        .get("originator")
        .and_then(Value::as_str)
        .unwrap_or("");
    if originator != "codex-tui" {
        return Some(false);
    }
    if payload.get("source").is_some_and(Value::is_object) {
        return Some(false);
    }
    Some(true)
}

fn cwd_of(v: &Value) -> String {
    v.pointer("/payload/cwd")
        .or_else(|| v.get("cwd"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn peek_origin(path: &Path) -> Origin {
    let Ok(mut file) = File::open(path) else {
        return Origin::Pending;
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = vec![0u8; HEAD_PEEK];
    let n = file.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    for raw in buf.split(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if let Some(keep) = origin_of(&v) {
            return if keep { Origin::Tui } else { Origin::Skip };
        }
    }
    if len > HEAD_PEEK as u64 {
        Origin::Skip
    } else {
        Origin::Pending
    }
}

fn apply_kind(tracked: &mut Tracked, kind: LineKind, cwd: &str, now: i64) {
    if !cwd.is_empty() {
        tracked.cwd = cwd.to_string();
    }
    match kind {
        LineKind::Ignore => {}
        LineKind::Prompt => {
            tracked.status = PresenceStatus::Working;
            tracked.updated_at = now;
        }
        LineKind::EndTurn { preview } => {
            tracked.status = PresenceStatus::Ready;
            if let Some(p) = preview {
                tracked.preview = Some(p);
            }
            tracked.updated_at = now;
        }
        LineKind::Activity => {
            if tracked.status != PresenceStatus::Ready {
                tracked.status = PresenceStatus::Working;
                tracked.updated_at = now;
            }
        }
    }
}

fn to_presence(id: &str, t: &Tracked) -> AgentPresence {
    presence::normalize(AgentPresence {
        id: id.to_string(),
        backend_id: BACKEND_ID.into(),
        backend_name: BACKEND_NAME.into(),
        cwd: t.cwd.clone(),
        status: t.status,
        preview: t.preview.clone(),
        updated_at: t.updated_at,
        window: None,
        source: PresenceSource::Jsonl,
    })
}

fn open_tail(path: &Path) -> Tail {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len > FIRST_READ_TAIL {
        Tail {
            offset: len - FIRST_READ_TAIL,
            carry: Vec::new(),
            skip_head: true,
        }
    } else {
        Tail::default()
    }
}

fn read_new_lines(path: &Path, tail: &mut Tail) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(meta) = file.metadata() else {
        return Vec::new();
    };
    let len = meta.len();
    if len < tail.offset {
        tail.offset = 0;
        tail.carry.clear();
        tail.skip_head = false;
    }
    if file.seek(SeekFrom::Start(tail.offset)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let lines = consume(tail, &buf);
    tail.offset = len.saturating_sub(tail.carry.len() as u64);
    lines
}

fn should_keep(t: &Tracked, now: i64) -> bool {
    if t.origin == Origin::Skip {
        return false;
    }
    match t.status {
        PresenceStatus::Working | PresenceStatus::Waiting => true,
        PresenceStatus::Ready | PresenceStatus::Idle => now - t.updated_at < DISAPPEAR_SECS,
    }
}

fn for_each_rollout(root: &Path, mut visit: impl FnMut(&Path, &str)) {
    let Ok(years) = std::fs::read_dir(root) else {
        return;
    };
    for year in years.flatten() {
        let year_path = year.path();
        if !year_path.is_dir() {
            continue;
        }
        let Ok(months) = std::fs::read_dir(&year_path) else {
            continue;
        };
        for month in months.flatten() {
            let month_path = month.path();
            if !month_path.is_dir() {
                continue;
            }
            let Ok(days) = std::fs::read_dir(&month_path) else {
                continue;
            };
            for day in days.flatten() {
                let day_path = day.path();
                if !day_path.is_dir() {
                    continue;
                }
                let Ok(files) = std::fs::read_dir(&day_path) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let Some(id) = session_id_from_stem(stem) else {
                        continue;
                    };
                    visit(&path, id);
                }
            }
        }
    }
}

/// Un barrido. No emite: eso lo hace [`presence::publish`].
pub fn tick(
    root: &Path,
    now: i64,
    state: &mut WatchState,
    ignore: &HashSet<String>,
) -> Vec<AgentPresence> {
    let live_cutoff = now.saturating_sub(LIVE_WINDOW_SECS as i64) as u64;
    let mut seen = HashSet::new();

    for_each_rollout(root, |path, id| {
        if ignore.contains(id) {
            state.tracked.remove(id);
            return;
        }
        let mtime = mtime_secs(path);
        let already = state.tracked.contains_key(id);
        if !already && mtime < live_cutoff {
            return;
        }
        seen.insert(id.to_string());
        let tracked = state.tracked.entry(id.to_string()).or_insert_with(|| {
            let origin = peek_origin(path);
            Tracked {
                path: path.to_path_buf(),
                tail: open_tail(path),
                cwd: String::new(),
                status: PresenceStatus::Idle,
                preview: None,
                updated_at: now,
                origin,
            }
        });
        if tracked.origin == Origin::Skip {
            return;
        }
        if tracked.origin == Origin::Pending {
            tracked.origin = peek_origin(path);
            if tracked.origin == Origin::Skip {
                return;
            }
        }
        let lines = read_new_lines(path, &mut tracked.tail);
        for line in lines {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(keep) = origin_of(&v) {
                tracked.origin = if keep { Origin::Tui } else { Origin::Skip };
                let cwd = cwd_of(&v);
                if !cwd.is_empty() {
                    tracked.cwd = cwd;
                }
                if tracked.origin == Origin::Skip {
                    break;
                }
                continue;
            }
            if tracked.origin == Origin::Skip {
                break;
            }
            let kind = classify(&v);
            if kind == LineKind::Ignore {
                let cwd = cwd_of(&v);
                if !cwd.is_empty() {
                    tracked.cwd = cwd;
                }
                continue;
            }
            apply_kind(tracked, kind, &cwd_of(&v), now);
        }
    });

    state.tracked.retain(|id, t| {
        if ignore.contains(id) || !t.path.exists() || t.origin == Origin::Skip {
            return false;
        }
        if !seen.contains(id) && t.status != PresenceStatus::Working {
            return should_keep(t, now);
        }
        should_keep(t, now)
    });

    state
        .tracked
        .iter()
        .filter(|(_, t)| t.origin == Origin::Tui && t.status != PresenceStatus::Idle)
        .map(|(id, t)| to_presence(id, t))
        .collect()
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
        .name("atic-watch-codex".into())
        .spawn(move || {
            let mut state = WatchState::default();
            loop {
                std::thread::sleep(POLL);
                let Some(root) = sessions_root() else {
                    continue;
                };
                let ignore = super::bridge::live_session_ids();
                let list = tick(&root, now_secs(), &mut state, &ignore);
                sync_registry(&list);
                super::focus::attach_unique_backend(BACKEND_ID);
                presence::publish(&handle);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stem_saca_el_uuid() {
        assert_eq!(
            session_id_from_stem(
                "rollout-2026-08-07T10-36-21-019fdca7-52b0-74e3-9f13-0e43d1b600a7"
            ),
            Some("019fdca7-52b0-74e3-9f13-0e43d1b600a7")
        );
    }

    #[test]
    fn task_started_abre_turno() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": { "type": "task_started", "turn_id": "t1" }
            })),
            LineKind::Prompt
        );
    }

    #[test]
    fn user_message_abre_turno() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": { "type": "user_message", "message": "hola" }
            })),
            LineKind::Prompt
        );
    }

    #[test]
    fn task_complete_cierra_con_preview() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_complete",
                    "last_agent_message": "listo\nsegunda"
                }
            })),
            LineKind::EndTurn {
                preview: Some("listo".into())
            }
        );
    }

    #[test]
    fn item_completed_final_answer_cierra_con_preview() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": {
                    "type": "item_completed",
                    "item": {
                        "type": "AgentMessage",
                        "content": [{ "type": "Text", "text": "chiste\nsegunda" }],
                        "phase": "final_answer"
                    }
                }
            })),
            LineKind::EndTurn {
                preview: Some("chiste".into())
            }
        );
    }

    #[test]
    fn item_completed_user_message_abre_turno() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": {
                    "type": "item_completed",
                    "item": { "type": "UserMessage", "content": [{ "type": "Text", "text": "hola" }] }
                }
            })),
            LineKind::Prompt
        );
    }

    #[test]
    fn agent_message_no_parpadea() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": { "type": "agent_message", "message": "pensando" }
            })),
            LineKind::Ignore
        );
    }

    #[test]
    fn tool_es_activity() {
        assert_eq!(
            classify(&json!({
                "type": "response_item",
                "payload": { "type": "function_call", "name": "shell" }
            })),
            LineKind::Activity
        );
    }

    #[test]
    fn token_count_se_ignora() {
        assert_eq!(
            classify(&json!({
                "type": "event_msg",
                "payload": { "type": "token_count" }
            })),
            LineKind::Ignore
        );
    }

    #[test]
    fn origin_tui_se_queda() {
        assert_eq!(
            origin_of(&json!({
                "type": "session_meta",
                "payload": { "originator": "codex-tui", "source": "cli", "cwd": "/x" }
            })),
            Some(true)
        );
    }

    #[test]
    fn origin_atic_se_salta() {
        assert_eq!(
            origin_of(&json!({
                "type": "session_meta",
                "payload": { "originator": "atic", "source": "cli" }
            })),
            Some(false)
        );
    }

    #[test]
    fn origin_desktop_se_salta() {
        assert_eq!(
            origin_of(&json!({
                "type": "session_meta",
                "payload": { "originator": "Codex Desktop", "source": "vscode" }
            })),
            Some(false)
        );
    }

    #[test]
    fn origin_subagente_se_salta() {
        assert_eq!(
            origin_of(&json!({
                "type": "session_meta",
                "payload": {
                    "originator": "codex-tui",
                    "source": { "subagent": { "depth": 1 } }
                }
            })),
            Some(false)
        );
    }

    #[test]
    fn tick_tui_lee_solo_lo_nuevo() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("atic-watch-codex-{nonce}"));
        let day = root.join("2026").join("08").join("07");
        std::fs::create_dir_all(&day).unwrap();
        let id = "019fdca7-52b0-74e3-9f13-0e43d1b600a7";
        let file = day.join(format!("rollout-2026-08-07T10-36-21-{id}.jsonl"));
        let meta = concat!(
            r#"{"type":"session_meta","payload":{"session_id":"019fdca7-52b0-74e3-9f13-0e43d1b600a7","originator":"codex-tui","source":"cli","cwd":"/repo"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
            "\n",
        );
        std::fs::write(&file, meta).unwrap();

        let mut state = WatchState::default();
        let ignore = HashSet::new();
        let now = now_secs();
        tick(&root, now, &mut state, &ignore);
        assert_eq!(state.tracked[id].status, PresenceStatus::Working);
        assert_eq!(state.tracked[id].cwd, "/repo");

        let mut all = String::from(meta);
        all.push_str(
            r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"hecho"}}"#,
        );
        all.push('\n');
        std::fs::write(&file, all).unwrap();
        tick(&root, now, &mut state, &ignore);
        assert_eq!(state.tracked[id].status, PresenceStatus::Ready);
        assert_eq!(state.tracked[id].preview.as_deref(), Some("hecho"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tick_atic_no_entra() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("atic-watch-codex-skip-{nonce}"));
        let day = root.join("2026").join("08").join("07");
        std::fs::create_dir_all(&day).unwrap();
        let id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let file = day.join(format!("rollout-2026-08-07T10-36-21-{id}.jsonl"));
        std::fs::write(
            &file,
            concat!(
                r#"{"type":"session_meta","payload":{"originator":"atic","source":"cli","cwd":"/x"}}"#,
                "\n",
                r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
                "\n",
            ),
        )
        .unwrap();

        let mut state = WatchState::default();
        let list = tick(&root, now_secs(), &mut state, &HashSet::new());
        assert!(list.is_empty());
        assert!(!state.tracked.contains_key(id));

        let _ = std::fs::remove_dir_all(&root);
    }
}
