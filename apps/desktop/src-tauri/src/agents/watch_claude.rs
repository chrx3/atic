//! Watcher del JSONL vivo de Claude Code.
//!
//! Poll ~1 s, sin crate `notify`. El archivo se anexa durante el turno: un
//! `user` con `promptSource` abre trabajo; un `assistant` con `stop_reason`
//! distinto de `tool_use` lo cierra. No hay línea que diga «espero permiso».

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tauri::AppHandle;

use super::presence::{
    self, ActivityKind, AgentPresence, PresenceActivity, PresenceSource, PresenceStatus,
};

const POLL: Duration = Duration::from_secs(1);
const LIVE_WINDOW_SECS: u64 = 15 * 60;
const DISAPPEAR_SECS: i64 = 30 * 60;
const PREVIEW_MAX: usize = 120;
/// Tope del detalle de la actividad (archivo, comando): la pill lo recorta más.
const DETAIL_MAX: usize = 48;
const FIRST_READ_TAIL: u64 = 256 * 1024;
const BACKEND_ID: &str = "claude-code";
const BACKEND_NAME: &str = "Claude Code";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Ignore,
    Prompt,
    EndTurn { preview: Option<String> },
    Activity,
}

#[derive(Debug, Default)]
pub struct Tail {
    pub offset: u64,
    pub carry: Vec<u8>,
    /// Al abrir a mitad de archivo, el primer fragmento es basura: se descarta.
    pub skip_head: bool,
}

#[derive(Debug)]
struct Tracked {
    path: PathBuf,
    tail: Tail,
    cwd: String,
    status: PresenceStatus,
    preview: Option<String>,
    updated_at: i64,
    activity: Option<PresenceActivity>,
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

pub fn classify(v: &Value) -> LineKind {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return LineKind::Ignore;
    }
    match v.get("type").and_then(Value::as_str) {
        Some("user") if v.get("promptSource").is_some() => LineKind::Prompt,
        Some("assistant") => {
            let stop = v.pointer("/message/stop_reason").and_then(Value::as_str);
            match stop {
                Some("tool_use") | None => LineKind::Activity,
                Some(_) => LineKind::EndTurn {
                    preview: extract_assistant_preview(v),
                },
            }
        }
        Some("user") => LineKind::Activity,
        _ => LineKind::Ignore,
    }
}

/// Qué está haciendo, según la línea.
///
/// Del mensaje del asistente cuenta el ÚLTIMO bloque: es lo que está pasando
/// ahora. Un `tool_result` (línea `user` sin prompt) es el agente leyendo lo
/// que devolvió la herramienta, o sea pensando el paso siguiente.
pub fn activity_of(v: &Value) -> Option<PresenceActivity> {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    match v.get("type").and_then(Value::as_str) {
        Some("user") if v.get("promptSource").is_none() => {
            Some(activity(ActivityKind::Thinking, None))
        }
        Some("assistant") => {
            let blocks = v.pointer("/message/content")?.as_array()?;
            let block = blocks.iter().rev().find(|b| {
                matches!(
                    b.get("type").and_then(Value::as_str),
                    Some("tool_use" | "thinking" | "text")
                )
            })?;
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => Some(tool_activity(block)),
                Some("thinking") => Some(activity(ActivityKind::Thinking, None)),
                _ => Some(activity(ActivityKind::Writing, None)),
            }
        }
        _ => None,
    }
}

fn activity(kind: ActivityKind, detail: Option<String>) -> PresenceActivity {
    PresenceActivity { kind, detail }
}

fn tool_activity(block: &Value) -> PresenceActivity {
    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
    let input = block.get("input");
    let field = |key: &str| {
        input
            .and_then(|i| i.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match name {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => activity(
            ActivityKind::Editing,
            field("file_path")
                .or_else(|| field("notebook_path"))
                .map(file_name),
        ),
        "Read" => activity(ActivityKind::Reading, field("file_path").map(file_name)),
        "Grep" | "Glob" | "LS" => activity(ActivityKind::Searching, field("pattern").map(short)),
        "WebSearch" => activity(ActivityKind::Searching, field("query").map(short)),
        "WebFetch" => activity(ActivityKind::Reading, field("url").map(short)),
        "Bash" | "PowerShell" => {
            activity(ActivityKind::Running, field("command").map(command_head))
        }
        "Task" | "Agent" => activity(ActivityKind::Delegating, field("description").map(short)),
        "TodoWrite" => activity(ActivityKind::Thinking, None),
        _ => activity(ActivityKind::Tool, Some(short(tool_label(name)))),
    }
}

/// Solo el nombre del archivo: la ruta entera no entra y dice lo mismo.
fn file_name(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    short(name)
}

/// Las dos primeras palabras del comando: `pnpm test`, `git status`.
fn command_head(command: &str) -> String {
    let first_line = command.lines().next().unwrap_or(command);
    short(
        &first_line
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// `mcp__atlassian__search` → `search`: el prefijo del servidor no aporta.
fn tool_label(name: &str) -> &str {
    name.rsplit("__").next().unwrap_or(name)
}

fn short(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= DETAIL_MAX {
        return text.to_string();
    }
    let mut out: String = text.chars().take(DETAIL_MAX - 1).collect();
    out.push('…');
    out
}

fn extract_assistant_preview(v: &Value) -> Option<String> {
    let arr = v.pointer("/message/content")?.as_array()?;
    let mut last: Option<&str> = None;
    for block in arr {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = block.get("text").and_then(Value::as_str) else {
            continue;
        };
        if !text.trim().is_empty() {
            last = Some(text);
        }
    }
    let raw = last?;
    let line = raw
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(raw)
        .trim();
    if line.is_empty() {
        return None;
    }
    Some(line.chars().take(PREVIEW_MAX).collect())
}

/// Líneas completas nuevas. La última sin newline queda en `carry` y no avanza.
pub fn consume(tail: &mut Tail, new_bytes: &[u8]) -> Vec<String> {
    let mut data = std::mem::take(&mut tail.carry);
    data.extend_from_slice(new_bytes);
    let skip_head = tail.skip_head;
    tail.skip_head = false;

    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut first = true;
    for i in 0..data.len() {
        if data[i] != b'\n' {
            continue;
        }
        let mut end = i;
        if end > start && data[end - 1] == b'\r' {
            end -= 1;
        }
        let slice = &data[start..end];
        start = i + 1;
        if first && skip_head {
            first = false;
            continue;
        }
        first = false;
        if !slice.is_empty() {
            lines.push(String::from_utf8_lossy(slice).into_owned());
        }
    }
    tail.carry = data[start..].to_vec();
    lines
}

fn apply_kind(tracked: &mut Tracked, kind: LineKind, cwd: &str, now: i64) {
    if !cwd.is_empty() {
        tracked.cwd = cwd.to_string();
    }
    match kind {
        LineKind::Ignore => {}
        LineKind::Prompt => {
            tracked.status = PresenceStatus::Working;
            tracked.activity = Some(activity(ActivityKind::Thinking, None));
            tracked.updated_at = now;
        }
        LineKind::EndTurn { preview } => {
            tracked.status = PresenceStatus::Ready;
            tracked.activity = None;
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
        activity: t.activity.clone(),
    })
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

fn session_id_of(v: &Value, fallback: &str) -> String {
    v.get("sessionId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn cwd_of(v: &Value) -> String {
    v.get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn should_keep(t: &Tracked, now: i64) -> bool {
    match t.status {
        PresenceStatus::Working | PresenceStatus::Waiting => true,
        PresenceStatus::Ready | PresenceStatus::Idle => now - t.updated_at < DISAPPEAR_SECS,
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

    let Ok(projects) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    for project in projects.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&project_path) else {
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
            if ignore.contains(stem) {
                state.tracked.remove(stem);
                continue;
            }
            let mtime = mtime_secs(&path);
            let already = state.tracked.contains_key(stem);
            if !already && mtime < live_cutoff {
                continue;
            }
            seen.insert(stem.to_string());
            let tracked = state
                .tracked
                .entry(stem.to_string())
                .or_insert_with(|| Tracked {
                    path: path.clone(),
                    tail: open_tail(&path),
                    cwd: String::new(),
                    status: PresenceStatus::Idle,
                    preview: None,
                    updated_at: now,
                    activity: None,
                });
            if tracked.path != path {
                tracked.path = path.clone();
            }
            let lines = read_new_lines(&path, &mut tracked.tail);
            for line in lines {
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let kind = classify(&v);
                if kind == LineKind::Ignore {
                    continue;
                }
                let id = session_id_of(&v, stem);
                if ignore.contains(&id) {
                    continue;
                }
                let is_activity = kind == LineKind::Activity;
                apply_kind(tracked, kind, &cwd_of(&v), now);
                if is_activity && tracked.status == PresenceStatus::Working {
                    if let Some(next) = activity_of(&v) {
                        tracked.activity = Some(next);
                    }
                }
            }
        }
    }

    state.tracked.retain(|id, t| {
        if ignore.contains(id) || !t.path.exists() {
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
        .filter(|(_, t)| t.status != PresenceStatus::Idle)
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
        .name("atic-watch-claude".into())
        .spawn(move || {
            let mut state = WatchState::default();
            loop {
                std::thread::sleep(POLL);
                let Some(root) = super::claude_sessions::projects_root() else {
                    continue;
                };
                let ignore = super::bridge::live_session_ids();
                crate::clipboard_history::remember_paste_target();
                super::ping::drain();
                let list = tick(&root, now_secs(), &mut state, &ignore);
                sync_registry(&list);
                super::focus::attach_unique_claude();
                presence::publish(&handle);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, input: Value) -> Value {
        json!({
            "type": "assistant",
            "message": {
                "stop_reason": "tool_use",
                "content": [{ "type": "text", "text": "voy" }, { "type": "tool_use", "name": name, "input": input }]
            }
        })
    }

    #[test]
    fn editar_dice_el_archivo_no_la_ruta() {
        let a = activity_of(&tool(
            "Edit",
            json!({ "file_path": r"C:\repo\src\PillSurface.svelte" }),
        ))
        .unwrap();
        assert_eq!(a.kind, ActivityKind::Editing);
        assert_eq!(a.detail.as_deref(), Some("PillSurface.svelte"));
    }

    #[test]
    fn comando_dice_sus_dos_primeras_palabras() {
        let a = activity_of(&tool(
            "Bash",
            json!({ "command": "pnpm test --run\necho hi" }),
        ))
        .unwrap();
        assert_eq!(a.kind, ActivityKind::Running);
        assert_eq!(a.detail.as_deref(), Some("pnpm test"));
    }

    #[test]
    fn manda_el_ultimo_bloque() {
        let v = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "thinking", "thinking": "..." }, { "type": "text", "text": "Hola" }] }
        });
        assert_eq!(activity_of(&v).unwrap().kind, ActivityKind::Writing);
        let v = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "thinking", "thinking": "..." }] }
        });
        assert_eq!(activity_of(&v).unwrap().kind, ActivityKind::Thinking);
    }

    #[test]
    fn resultado_de_herramienta_es_pensar_y_el_prompt_no_cuenta() {
        let result = json!({ "type": "user", "message": { "content": [] } });
        assert_eq!(activity_of(&result).unwrap().kind, ActivityKind::Thinking);
        let prompt = json!({ "type": "user", "promptSource": "cli", "message": {} });
        assert!(activity_of(&prompt).is_none());
    }

    #[test]
    fn herramienta_mcp_sin_prefijo_y_detalle_largo_recortado() {
        let a = activity_of(&tool("mcp__atlassian__searchJiraIssuesUsingJql", json!({}))).unwrap();
        assert_eq!(a.kind, ActivityKind::Tool);
        assert_eq!(a.detail.as_deref(), Some("searchJiraIssuesUsingJql"));
        let long = "x".repeat(80);
        let a = activity_of(&tool("Grep", json!({ "pattern": long }))).unwrap();
        assert_eq!(a.detail.unwrap().chars().count(), DETAIL_MAX);
    }

    #[test]
    fn fin_de_turno_borra_la_actividad() {
        let mut t = Tracked {
            path: PathBuf::from("x.jsonl"),
            tail: Tail::default(),
            cwd: String::new(),
            status: PresenceStatus::Idle,
            preview: None,
            updated_at: 0,
            activity: None,
        };
        apply_kind(&mut t, LineKind::Prompt, "/repo", 1);
        assert_eq!(
            t.activity.as_ref().map(|a| a.kind),
            Some(ActivityKind::Thinking)
        );
        apply_kind(&mut t, LineKind::EndTurn { preview: None }, "/repo", 2);
        assert!(t.activity.is_none());
    }

    fn parse(v: Value) -> LineKind {
        classify(&v)
    }

    #[test]
    fn prompt_abre_turno() {
        let kind = parse(json!({
            "type": "user",
            "promptSource": "typed",
            "sessionId": "s1",
            "message": { "content": [{ "type": "text", "text": "hola" }] }
        }));
        assert_eq!(kind, LineKind::Prompt);
    }

    #[test]
    fn user_sin_prompt_source_es_activity() {
        let kind = parse(json!({
            "type": "user",
            "sessionId": "s1",
            "message": { "content": [{ "type": "tool_result", "content": "ok" }] }
        }));
        assert_eq!(kind, LineKind::Activity);
    }

    #[test]
    fn sidechain_se_ignora() {
        let kind = parse(json!({
            "type": "assistant",
            "isSidechain": true,
            "message": {
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "no" }]
            }
        }));
        assert_eq!(kind, LineKind::Ignore);
    }

    #[test]
    fn stop_sequence_cierra_turno() {
        let kind = parse(json!({
            "type": "assistant",
            "message": {
                "stop_reason": "stop_sequence",
                "content": [{ "type": "text", "text": "seq" }]
            }
        }));
        assert_eq!(
            kind,
            LineKind::EndTurn {
                preview: Some("seq".into())
            }
        );
    }

    #[test]
    fn tool_use_no_cierra() {
        let kind = parse(json!({
            "type": "assistant",
            "message": { "stop_reason": "tool_use", "content": [] }
        }));
        assert_eq!(kind, LineKind::Activity);
    }

    #[test]
    fn linea_ilegible_se_salta() {
        let mut tail = Tail::default();
        let lines = consume(
            &mut tail,
            b"esto no es json\n{\"type\":\"user\",\"promptSource\":\"typed\"}\n",
        );
        assert_eq!(lines.len(), 2);
        let kinds: Vec<_> = lines
            .iter()
            .map(|l| match serde_json::from_str::<Value>(l) {
                Ok(v) => classify(&v),
                Err(_) => LineKind::Ignore,
            })
            .collect();
        assert_eq!(kinds[0], LineKind::Ignore);
        assert_eq!(kinds[1], LineKind::Prompt);
    }

    #[test]
    fn linea_truncada_no_avanza_offset() {
        let mut tail = Tail::default();
        let first = b"{\"type\":\"user\",\"promptSource\":\"typed\"}\n{\"type\":\"assi";
        let lines = consume(&mut tail, first);
        assert_eq!(lines.len(), 1);
        assert!(!tail.carry.is_empty());
        let consumed = first.len() as u64 - tail.carry.len() as u64;
        assert_eq!(
            consumed,
            b"{\"type\":\"user\",\"promptSource\":\"typed\"}\n".len() as u64
        );

        let rest = b"stant\",\"message\":{\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n";
        let lines = consume(&mut tail, rest);
        assert_eq!(lines.len(), 1);
        assert!(tail.carry.is_empty());
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(
            classify(&v),
            LineKind::EndTurn {
                preview: Some("ok".into())
            }
        );
    }

    #[test]
    fn apply_prompt_luego_end_turn() {
        let mut t = Tracked {
            path: PathBuf::from("x.jsonl"),
            tail: Tail::default(),
            cwd: String::new(),
            status: PresenceStatus::Idle,
            preview: None,
            updated_at: 0,
            activity: None,
        };
        apply_kind(&mut t, LineKind::Prompt, "/repo", 10);
        assert_eq!(t.status, PresenceStatus::Working);
        apply_kind(&mut t, LineKind::Activity, "/repo", 11);
        assert_eq!(t.status, PresenceStatus::Working);
        apply_kind(
            &mut t,
            LineKind::EndTurn {
                preview: Some("listo".into()),
            },
            "/repo",
            12,
        );
        assert_eq!(t.status, PresenceStatus::Ready);
        assert_eq!(t.preview.as_deref(), Some("listo"));
        apply_kind(&mut t, LineKind::Activity, "/repo", 13);
        assert_eq!(t.status, PresenceStatus::Ready);
        assert_eq!(t.preview.as_deref(), Some("listo"));
    }

    #[test]
    fn tick_lee_solo_lo_nuevo() {
        let nonce = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("atic-watch-claude-{nonce}"));
        let proj = root.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("sess-1.jsonl");
        std::fs::write(
            &file,
            "{\"type\":\"user\",\"promptSource\":\"typed\",\"sessionId\":\"sess-1\",\"cwd\":\"/a\"}\n",
        )
        .unwrap();

        let mut state = WatchState::default();
        let ignore = HashSet::new();
        tick(&root, now_secs(), &mut state, &ignore);
        assert_eq!(state.tracked["sess-1"].status, PresenceStatus::Working);

        std::fs::write(
            &file,
            concat!(
                "{\"type\":\"user\",\"promptSource\":\"typed\",\"sessionId\":\"sess-1\",\"cwd\":\"/a\"}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"sess-1\",\"message\":{\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"hecho\"}]}}\n",
            ),
        )
        .unwrap();
        tick(&root, now_secs(), &mut state, &ignore);
        assert_eq!(state.tracked["sess-1"].status, PresenceStatus::Ready);
        assert_eq!(state.tracked["sess-1"].preview.as_deref(), Some("hecho"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
