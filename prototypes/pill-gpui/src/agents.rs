//! Agentes como notch: arriba lo que está en curso, abajo abrir uno nuevo.
//!
//! **En curso** sale de los mismos archivos que vigila Atic
//! (`agents/watch_claude.rs`, `watch_codex.rs`): el JSONL vivo de Claude Code
//! (`~/.claude/projects/*/*.jsonl`) y los rollouts de Codex
//! (`~/.codex/sessions/AAAA/MM/DD/rollout-*.jsonl`). Un prompt abre trabajo y
//! el fin de turno lo cierra con su última línea como vista previa. Las mismas
//! reglas: solo archivos tocados en los últimos 15 min, y lo listo desaparece
//! a los 30 min. OpenCode (su SQLite) y Cursor (sus procesos) salen de
//! `atic_agents`, igual que en Atic.
//!
//! Clic en una fila trae al frente la terminal del agente si se puede saber
//! cuál es sin adivinar (como `focus.rs` de Atic: el JSONL no trae pid). Con
//! varias ventanas posibles no elige: lo dice, y la flecha de la fila lo
//! reanuda en una terminal nueva.
//!
//! **Nuevo** es el `AgentLauncher` de Atic: el agente y la carpeta. Se abre
//! como consola en el espacio (`space`), igual que «↗» al reanudar.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    actions, div, img, prelude::*, px, rgb, svg, AnyElement, App, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, KeyBinding, PathPromptOptions,
    SharedString, Window,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hover::{pin_button, HoverExt};
use crate::clipboard::{BAND_H, PANEL_H};
use crate::text_input::TextInput;
use crate::tray;

const MARK_GAP: f32 = 40.0;
const SIDE_PAD: f32 = 8.0;
const HEADER_H: f32 = 24.0;
const ROW_H: f32 = 44.0;
const EMPTY_H: f32 = 44.0;
const PICKER_H: f32 = 44.0;
const FOLDERS_H: f32 = 34.0;
const LAUNCH_H: f32 = 40.0;
const FOOTER_H: f32 = 26.0;
/// Filas de «En curso» antes de cortar: el notch no debe tapar media pantalla.
const MAX_ROWS: usize = 5;
/// Filas de «Por revisar» antes de cortar; las demás se cuentan.
const MAX_INBOX_ROWS: usize = 4;
const MORE_H: f32 = 22.0;
/// «Nuevo» plegado: una sola fila.
const NUEVO_ROW_H: f32 = 38.0;
/// Lo más alto que llega el notch con Agentes.
const MAX_HEIGHT: f32 = PANEL_H + 60.0;
/// El mini reproductor del pie, y el aire sobre él.
const PLAYER_H: f32 = 44.0;
const PLAYER_GAP: f32 = 6.0;
/// El mensaje rápido y el aire sobre él.
const COMPOSER_H: f32 = 40.0;
const COMPOSER_GAP: f32 = 4.0;
const MAX_FOLDERS: usize = 4;
const REFRESH_EVERY: Duration = Duration::from_millis(1000);

const LIVE_WINDOW_SECS: i64 = 15 * 60;
const DISAPPEAR_SECS: i64 = 30 * 60;
const FIRST_READ_TAIL: u64 = 256 * 1024;
/// Lo que se lee del principio de un rollout buscando `session_meta`: esa
/// línea trae las instrucciones base enteras.
const HEAD_PEEK: u64 = 1024 * 1024;
const PREVIEW_MAX: usize = 120;
const DETAIL_MAX: usize = 40;

/// Un agente de consola. Mismo catálogo y orden que `agentCatalog.ts`.
pub struct Agent {
    pub cli: &'static str,
    pub name: &'static str,
    pub(crate) logo: &'static str,
    /// El ejecutable que corre la TUI, para encontrar su ventana.
    exe: &'static str,
    install: &'static str,
    /// Cómo se retoma una sesión por su id.
    resume: Option<&'static str>,
}

pub const AGENTS: [Agent; 6] = [
    Agent {
        cli: "claude",
        name: "Claude Code",
        logo: "icons/agents/claude.svg",
        exe: "claude.exe",
        install: "irm https://claude.ai/install.ps1 | iex",
        resume: Some("claude --resume {id}"),
    },
    Agent {
        cli: "opencode",
        name: "OpenCode",
        logo: "icons/agents/opencode.svg",
        exe: "opencode.exe",
        install: "npm install -g opencode-ai",
        resume: None,
    },
    Agent {
        cli: "codex",
        name: "Codex",
        logo: "icons/agents/openai.svg",
        exe: "codex.exe",
        install: "npm install -g @openai/codex",
        resume: Some("codex resume {id}"),
    },
    Agent {
        cli: "cursor-agent",
        name: "Cursor",
        logo: "icons/agents/cursor.svg",
        exe: "cursor-agent.exe",
        install: "irm 'https://cursor.com/install?win32=true' | iex",
        resume: None,
    },
    Agent {
        cli: "agy",
        name: "Antigravity",
        logo: "icons/agents/antigravity.svg",
        exe: "agy.exe",
        install: "irm https://antigravity.google/cli/install.ps1 | iex",
        resume: None,
    },
    Agent {
        cli: "grok",
        name: "Grok",
        logo: "icons/agents/grok.svg",
        exe: "grok.exe",
        install: "irm https://x.ai/cli/install.ps1 | iex",
        resume: None,
    },
];

const CLAUDE: usize = 0;
const OPENCODE: usize = 1;
const CODEX: usize = 2;
const CURSOR: usize = 3;

actions!(agents_panel, [Dismiss, Launch, PrevAgent, NextAgent]);

const KEY_CONTEXT: &str = "AgentsPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("enter", Launch, context),
        KeyBinding::new("left", PrevAgent, context),
        KeyBinding::new("right", NextAgent, context),
    ]);
}

pub enum AgentsEvent {
    /// Cerrar devolviendo el foco a la app de antes.
    Close,
    /// Otra ventana se llevó el foco (la terminal): cerrar sin devolverlo.
    Left,
    /// Abrir una consola en el espacio (`space`).
    Open(crate::space::Open),
    /// Mostrar el espacio.
    Space,
}

// --- Sesiones en curso --------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Working,
    Ready,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub agent: usize,
    pub cwd: String,
    pub status: Status,
    pub preview: Option<String>,
    pub activity: Option<String>,
    pub updated: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LineKind {
    Ignore,
    Prompt,
    EndTurn { preview: Option<String> },
    Activity,
}

#[derive(Debug, Default)]
struct Tail {
    offset: u64,
    carry: Vec<u8>,
    /// Al abrir a mitad de archivo, el primer fragmento es basura.
    skip_head: bool,
}

#[derive(Debug)]
struct Tracked {
    path: PathBuf,
    tail: Tail,
    cwd: String,
    status: Option<Status>,
    preview: Option<String>,
    activity: Option<String>,
    updated: i64,
    /// Codex: el rollout es de la TUI (no de Codex Desktop ni un subagente).
    /// `None` mientras no aparece `session_meta`.
    tui: Option<bool>,
}

impl Tracked {
    fn new(path: PathBuf, tui: Option<bool>) -> Self {
        let tail = open_tail(&path);
        Self {
            path,
            tail,
            cwd: String::new(),
            status: None,
            preview: None,
            activity: None,
            updated: 0,
            tui,
        }
    }

    fn apply(&mut self, kind: LineKind, activity: Option<String>, cwd: &str, now: i64) {
        if !cwd.is_empty() {
            self.cwd = cwd.to_string();
        }
        match kind {
            LineKind::Ignore => {}
            LineKind::Prompt => {
                self.status = Some(Status::Working);
                self.activity = Some("Pensando…".into());
                self.updated = now;
            }
            LineKind::EndTurn { preview } => {
                self.status = Some(Status::Ready);
                self.activity = None;
                if preview.is_some() {
                    self.preview = preview;
                }
                self.updated = now;
            }
            LineKind::Activity => {
                if self.status != Some(Status::Ready) {
                    self.status = Some(Status::Working);
                    self.updated = now;
                    if activity.is_some() {
                        self.activity = activity;
                    }
                }
            }
        }
    }

    fn keep(&self, now: i64) -> bool {
        match self.status {
            Some(Status::Working) => true,
            Some(Status::Ready) => now - self.updated < DISAPPEAR_SECS,
            None => false,
        }
    }
}

/// Lo que se sabe de cada archivo entre un barrido y el siguiente: así solo se
/// leen las líneas nuevas.
#[derive(Default)]
pub struct Watch {
    claude: HashMap<String, Tracked>,
    codex: HashMap<String, Tracked>,
}

impl Watch {
    pub fn tick(&mut self, now: i64) -> Vec<Session> {
        if let Some(root) = claude_root() {
            self.tick_claude(&root, now);
        }
        if let Some(root) = codex_root() {
            self.tick_codex(&root, now);
        }
        let mut sessions: Vec<Session> = self
            .claude
            .iter()
            .map(|(id, t)| (CLAUDE, id, t))
            .chain(self.codex.iter().map(|(id, t)| (CODEX, id, t)))
            .filter(|(_, _, t)| t.keep(now) && t.tui != Some(false))
            .filter_map(|(agent, id, t)| {
                Some(Session {
                    id: id.clone(),
                    agent,
                    cwd: t.cwd.clone(),
                    status: t.status?,
                    preview: t.preview.clone(),
                    activity: t.activity.clone(),
                    updated: t.updated,
                })
            })
            .collect();
        sessions.extend(others(now));
        sort_sessions(&mut sessions);
        sessions
    }

    fn tick_claude(&mut self, root: &Path, now: i64) {
        let cutoff = now - LIVE_WINDOW_SECS;
        let Ok(projects) = std::fs::read_dir(root) else {
            return;
        };
        for project in projects.flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !self.claude.contains_key(id) && mtime_secs(&path) < cutoff {
                    continue;
                }
                let tracked = self
                    .claude
                    .entry(id.to_string())
                    .or_insert_with(|| Tracked::new(path.clone(), None));
                for line in read_new_lines(&tracked.path, &mut tracked.tail) {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let updated = line_time(&v).unwrap_or(now);
                    tracked.apply(claude_kind(&v), claude_activity(&v), &cwd_of(&v), updated);
                }
            }
        }
        // Se sigue lo que todavía se escribe aunque ya no se muestre: así no se
        // vuelve a leer su cola entera en cada barrido.
        self.claude.retain(|_, t| t.keep(now) || mtime_secs(&t.path) >= cutoff);
    }

    fn tick_codex(&mut self, root: &Path, now: i64) {
        let cutoff = now - LIVE_WINDOW_SECS;
        // Basta con los días de hoy y ayer: un rollout más viejo no está vivo.
        let mut days = Vec::new();
        for back in 0..2 {
            let day = chrono::Local::now() - chrono::Duration::days(back);
            days.push(root.join(day.format("%Y").to_string())
                .join(day.format("%m").to_string())
                .join(day.format("%d").to_string()));
        }
        for dir in days {
            let Ok(files) = std::fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(id) = codex_id(stem) else {
                    continue;
                };
                if !self.codex.contains_key(id) && mtime_secs(&path) < cutoff {
                    continue;
                }
                let tracked = self
                    .codex
                    .entry(id.to_string())
                    .or_insert_with(|| Tracked::new(path.clone(), peek_codex_origin(&path)));
                if tracked.tui.is_none() {
                    tracked.tui = peek_codex_origin(&tracked.path);
                }
                if tracked.tui == Some(false) {
                    continue;
                }
                for line in read_new_lines(&tracked.path, &mut tracked.tail) {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let updated = line_time(&v).unwrap_or(now);
                    tracked.apply(codex_kind(&v), None, &codex_cwd(&v), updated);
                }
            }
        }
        self.codex.retain(|_, t| t.keep(now) || mtime_secs(&t.path) >= cutoff);
    }
}

/// Lo que trabaja primero; después, lo más reciente.
fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        let working = |s: &Session| s.status == Status::Working;
        working(b)
            .cmp(&working(a))
            .then(b.updated.cmp(&a.updated))
    });
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn claude_root() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => Some(PathBuf::from(dir).join("projects")),
        None => Some(home()?.join(".claude").join("projects")),
    }
}

fn codex_root() -> Option<PathBuf> {
    Some(home()?.join(".codex").join("sessions"))
}

pub(crate) fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mtime_secs(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// La hora de la línea (`timestamp` en ISO 8601): al leer la cola de un
/// archivo por primera vez, «hace cuánto» debe ser de la línea y no de ahora.
fn line_time(v: &Value) -> Option<i64> {
    let stamp = v.get("timestamp")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|t| t.timestamp())
}

fn cwd_of(v: &Value) -> String {
    v.get("cwd").and_then(Value::as_str).unwrap_or("").to_string()
}

fn codex_cwd(v: &Value) -> String {
    v.pointer("/payload/cwd")
        .or_else(|| v.get("cwd"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// `rollout-AAAA-MM-DDTHH-MM-SS-<uuid>` → uuid.
fn codex_id(stem: &str) -> Option<&str> {
    let rest = stem.strip_prefix("rollout-")?;
    (rest.len() > 20 && rest.as_bytes().get(19) == Some(&b'-')).then(|| &rest[20..])
}

fn first_line(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(text)
        .trim();
    (!line.is_empty()).then(|| line.chars().take(PREVIEW_MAX).collect())
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

/// Claude Code: un `user` con `promptSource` abre trabajo; un `assistant`
/// con `stop_reason` distinto de `tool_use` lo cierra.
fn claude_kind(v: &Value) -> LineKind {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return LineKind::Ignore;
    }
    match v.get("type").and_then(Value::as_str) {
        Some("user") if v.get("promptSource").is_some() => LineKind::Prompt,
        Some("assistant") => match v.pointer("/message/stop_reason").and_then(Value::as_str) {
            Some("tool_use") | None => LineKind::Activity,
            Some(_) => LineKind::EndTurn {
                preview: claude_preview(v),
            },
        },
        Some("user") => LineKind::Activity,
        _ => LineKind::Ignore,
    }
}

fn claude_preview(v: &Value) -> Option<String> {
    let blocks = v.pointer("/message/content")?.as_array()?;
    let text = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .filter(|t| !t.trim().is_empty())
        .last()?;
    first_line(text)
}

/// Qué está haciendo, del último bloque del mensaje: es lo que pasa ahora.
fn claude_activity(v: &Value) -> Option<String> {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    match v.get("type").and_then(Value::as_str) {
        Some("user") if v.get("promptSource").is_none() => Some("Pensando…".into()),
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
                Some("thinking") => Some("Pensando…".into()),
                _ => Some("Escribiendo…".into()),
            }
        }
        _ => None,
    }
}

fn tool_activity(block: &Value) -> String {
    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
    let field = |key: &str| {
        block
            .pointer(&format!("/input/{key}"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let file = |path: &str| short(path.rsplit(['/', '\\']).next().unwrap_or(path));
    let with = |verb: &str, detail: Option<String>| match detail {
        Some(detail) => format!("{verb} {detail}"),
        None => format!("{verb}…"),
    };
    match name {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => with(
            "Editando",
            field("file_path").or_else(|| field("notebook_path")).map(file),
        ),
        "Read" => with("Leyendo", field("file_path").map(file)),
        "Grep" | "Glob" | "LS" => with("Buscando", field("pattern").map(short)),
        "WebSearch" => with("Buscando", field("query").map(short)),
        "WebFetch" => with("Leyendo", field("url").map(short)),
        "Bash" | "PowerShell" => with(
            "Ejecutando",
            field("command").map(|command| {
                let first = command.lines().next().unwrap_or(command);
                short(&first.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
            }),
        ),
        "Task" | "Agent" => with("Delegando", field("description").map(short)),
        "TodoWrite" => "Pensando…".into(),
        other => with("Usando", Some(short(other.rsplit("__").next().unwrap_or(other)))),
    }
}

/// Codex: `task_started`/`user_message` abren trabajo; `task_complete` lo
/// cierra con `last_agent_message`.
fn codex_kind(v: &Value) -> LineKind {
    match v.get("type").and_then(Value::as_str) {
        Some("event_msg") => match v.pointer("/payload/type").and_then(Value::as_str) {
            Some("task_started" | "user_message") => LineKind::Prompt,
            Some("task_complete" | "turn_aborted") => LineKind::EndTurn {
                preview: v
                    .pointer("/payload/last_agent_message")
                    .and_then(Value::as_str)
                    .and_then(first_line),
            },
            _ => LineKind::Ignore,
        },
        Some("response_item") => match v.pointer("/payload/type").and_then(Value::as_str) {
            Some("function_call" | "custom_tool_call" | "mcp_tool_call") => LineKind::Activity,
            _ => LineKind::Ignore,
        },
        _ => LineKind::Ignore,
    }
}

/// Si el rollout es de la TUI de Codex. `None` si todavía no se sabe.
fn peek_codex_origin(path: &Path) -> Option<bool> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file.take(HEAD_PEEK));
    let mut raw = Vec::new();
    let mut read = 0u64;
    loop {
        raw.clear();
        let n = reader.read_until(b'\n', &mut raw).unwrap_or(0);
        if n == 0 || raw.last() != Some(&b'\n') {
            break;
        }
        read += n as u64;
        let Ok(v) = serde_json::from_slice::<Value>(raw.trim_ascii_end()) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) == Some("session_meta") {
            let payload = v.get("payload").unwrap_or(&v);
            let tui = payload.get("originator").and_then(Value::as_str) == Some("codex-tui")
                && !payload.get("source").is_some_and(Value::is_object);
            return Some(tui);
        }
    }
    (read >= HEAD_PEEK).then_some(false)
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

/// Líneas completas nuevas; la última sin salto de línea espera en `carry`.
fn consume(tail: &mut Tail, new_bytes: &[u8]) -> Vec<String> {
    let mut data = std::mem::take(&mut tail.carry);
    data.extend_from_slice(new_bytes);
    let mut skip = std::mem::take(&mut tail.skip_head);
    let mut lines = Vec::new();
    let mut start = 0;
    for i in 0..data.len() {
        if data[i] != b'\n' {
            continue;
        }
        let slice = data[start..i].strip_suffix(b"\r").unwrap_or(&data[start..i]);
        start = i + 1;
        if std::mem::take(&mut skip) {
            continue;
        }
        if !slice.is_empty() {
            lines.push(String::from_utf8_lossy(slice).into_owned());
        }
    }
    tail.carry = data[start..].to_vec();
    lines
}

fn read_new_lines(path: &Path, tail: &mut Tail) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len < tail.offset {
        *tail = Tail::default();
    }
    if len == tail.offset || file.seek(SeekFrom::Start(tail.offset)).is_err() {
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

// --- Carpetas --------------------------------------------------------------

/// Las carpetas donde se usaron agentes hace poco: el `cwd` de la sesión más
/// reciente de cada proyecto de Claude Code, de la más nueva a la más vieja.
fn recent_folders(limit: usize) -> Vec<PathBuf> {
    let Some(root) = claude_root() else {
        return Vec::new();
    };
    let Ok(projects) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut latest: Vec<(i64, PathBuf)> = projects
        .flatten()
        .filter_map(|project| {
            std::fs::read_dir(project.path())
                .ok()?
                .flatten()
                .map(|f| f.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
                .map(|p| (mtime_secs(&p), p))
                .max_by_key(|(t, _)| *t)
        })
        .collect();
    latest.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen = HashSet::new();
    latest
        .into_iter()
        .filter_map(|(_, path)| session_cwd(&path))
        .filter(|dir| dir.is_dir() && seen.insert(dir.to_string_lossy().to_lowercase()))
        .take(limit)
        .collect()
}

/// El `cwd` de una sesión: viene en casi todas las líneas, basta la primera.
fn session_cwd(path: &Path) -> Option<PathBuf> {
    let reader = BufReader::new(File::open(path).ok()?.take(256 * 1024));
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find_map(|v| Some(PathBuf::from(v.get("cwd")?.as_str()?)))
}

pub(crate) fn folder_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

/// Lo elegido la última vez, en `<datos de Atic>\pill\agents.json`.
#[derive(Default, Serialize, Deserialize)]
struct Prefs {
    agent: Option<String>,
    folder: Option<PathBuf>,
}

fn prefs_file() -> Option<PathBuf> {
    crate::paths::file("agents.json")
}

impl Prefs {
    fn load() -> Self {
        prefs_file()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let Some(path) = prefs_file() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

// --- Terminal -----------------------------------------------------------------

/// Si el CLI está en el PATH (o donde lo dejan los instaladores nativos).
fn on_path(cli: &str) -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    if let Some(home) = home() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".grok").join("bin"));
    }
    dirs.iter().any(|dir| {
        ["exe", "cmd", "bat", "com"]
            .iter()
            .any(|ext| dir.join(format!("{cli}.{ext}")).is_file())
    })
}

/// Las ventanas donde corre el agente (la terminal que lo aloja). `Err` dice
/// por qué no se sabe.
#[cfg(windows)]
fn agent_windows(agent: &Agent) -> Result<HashSet<isize>, String> {
    let processes = process_snapshot();
    let parents: HashMap<u32, (u32, &str)> = processes
        .iter()
        .map(|(pid, ppid, name)| (*pid, (*ppid, name.as_str())))
        .collect();
    let own = std::process::id();
    // Las TUI de agente que no cuelgan de Atic ni de este prototipo.
    let pids: Vec<u32> = processes
        .iter()
        .filter(|(_, _, name)| name == agent.exe)
        .map(|(pid, _, _)| *pid)
        .filter(|&pid| {
            ancestors(pid, &parents).all(|(id, name)| {
                id != own && !matches!(name, "atic.exe" | "atic-desktop.exe")
            })
        })
        .collect();
    if pids.is_empty() {
        return Err(format!("{} no está abierto", agent.name));
    }
    let windows = top_windows();
    let found: HashSet<isize> = pids
        .iter()
        .filter_map(|&pid| {
            ancestors(pid, &parents)
                .take_while(|(_, name)| *name != "explorer.exe")
                .find_map(|(id, _)| windows.get(&id).copied())
        })
        .collect();
    Ok(found)
}

/// Trae al frente la ventana de la terminal donde corre el agente, si hay
/// una sola posible. `Err` dice por qué no.
#[cfg(windows)]
fn focus_agent(agent: &Agent) -> Result<(), String> {
    let found = agent_windows(agent)?;
    match found.len() {
        0 => Err(format!("No encontré la ventana de {}", agent.name)),
        1 => {
            let hwnd = *found.iter().next().unwrap_or(&0);
            restore_if_minimized(hwnd);
            crate::paste::force_foreground(hwnd);
            Ok(())
        }
        n => Err(format!("{} está en {n} ventanas: no sé cuál es", agent.name)),
    }
}

#[cfg(not(windows))]
fn focus_agent(agent: &Agent) -> Result<(), String> {
    Err(format!("{} no está abierto", agent.name))
}

/// Si el usuario está mirando la terminal de ese agente ahora mismo: un turno
/// que termina delante de él no necesita avisarle (`tray.rs`).
#[cfg(windows)]
pub(crate) fn agent_in_front(agent: usize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let front = unsafe { GetForegroundWindow() } as isize;
    front != 0 && agent_windows(&AGENTS[agent]).is_ok_and(|found| found.contains(&front))
}

#[cfg(not(windows))]
pub(crate) fn agent_in_front(_: usize) -> bool {
    false
}

/// El proceso y sus padres, en orden.
fn ancestors<'a>(
    pid: u32,
    parents: &'a HashMap<u32, (u32, &'a str)>,
) -> impl Iterator<Item = (u32, &'a str)> + 'a {
    let mut current = Some(pid);
    let mut seen = HashSet::new();
    std::iter::from_fn(move || {
        let pid = current.take()?;
        if !seen.insert(pid) {
            return None;
        }
        let (ppid, name) = *parents.get(&pid)?;
        if ppid != 0 && ppid != pid {
            current = Some(ppid);
        }
        Some((pid, name))
    })
}

/// OpenCode (su SQLite) y Cursor (sus procesos), con las reglas de Atic
/// (`atic_agents`). No tienen JSONL que seguir línea a línea.
fn others(now: i64) -> Vec<Session> {
    use atic_agents::seen::{Seen, SeenStatus};
    let session = |agent: usize, seen: Seen| {
        let status = match seen.status {
            SeenStatus::Working => Status::Working,
            SeenStatus::Ready => Status::Ready,
            SeenStatus::Idle => return None,
        };
        Some(Session {
            id: seen.id,
            agent,
            cwd: seen.cwd,
            status,
            preview: seen.preview,
            activity: None,
            updated: seen.updated,
        })
    };
    let mut out = Vec::new();
    if let Some(db) = atic_agents::opencode::db_path().filter(|path| path.exists()) {
        out.extend(
            atic_agents::opencode::sessions(&db, now, &HashSet::new())
                .into_iter()
                .filter_map(|seen| session(OPENCODE, seen)),
        );
    }
    #[cfg(windows)]
    {
        use atic_agents::cursor;
        // Un `cursor-agent` hijo del IDE no es el TUI.
        let pids = cursor::pids_outside(&process_snapshot(), cursor::EXE, &["cursor.exe"]);
        if !pids.is_empty() {
            let cwd = cursor::acp_root().and_then(|root| cursor::recent_cwd(&root));
            out.extend(
                cursor::sessions(&pids, cwd.as_deref(), now)
                    .into_iter()
                    .filter_map(|seen| session(CURSOR, seen)),
            );
        }
    }
    out
}

/// (pid, ppid, ejecutable en minúsculas) de todos los procesos.
#[cfg(windows)]
fn process_snapshot() -> Vec<(u32, u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                out.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// La primera ventana de usuario (visible, sin dueño, con título) de cada pid.
#[cfg(windows)]
fn top_windows() -> HashMap<u32, isize> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
        GW_OWNER,
    };
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
        let found = &mut *(data as *mut HashMap<u32, isize>);
        if IsWindowVisible(hwnd) != 0
            && GetWindow(hwnd, GW_OWNER).is_null()
            && GetWindowTextLengthW(hwnd) > 0
        {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            found.entry(pid).or_insert(hwnd as isize);
        }
        1
    }
    let mut found = HashMap::new();
    unsafe {
        EnumWindows(Some(each), &mut found as *mut _ as LPARAM);
    }
    found
}

#[cfg(windows)]
fn restore_if_minimized(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, ShowWindow, SW_RESTORE};
    unsafe {
        if IsIconic(hwnd as _) != 0 {
            ShowWindow(hwnd as _, SW_RESTORE);
        }
    }
}

// --- Panel ------------------------------------------------------------------

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    working: Hsla,
    ready: Hsla,
    claude: Hsla,
}

pub struct AgentsPanel {
    focus: FocusHandle,
    sessions: Vec<Session>,
    available: [bool; AGENTS.len()],
    agent: usize,
    folders: Vec<PathBuf>,
    folder: Option<PathBuf>,
    /// Lo último que pasó (un error, «no sé cuál ventana es»), en el pie.
    notice: Option<SharedString>,
    pub pinned: bool,
    /// El tope de alto del panel: en un costado, casi toda la pantalla (lo
    /// pone la pill); si no, el de siempre.
    pub max_height: Option<f32>,
    /// Lo que espera al usuario: turnos terminados y permisos (`tray.rs`).
    inbox: crate::tray::Inbox,
    /// «Nuevo» desplegado aunque haya filas por revisar.
    nuevo_open: bool,
    /// Lo que suena (`media.rs`): si hay algo, el pie lleva su mini reproductor.
    pub media: Option<crate::media::Media>,
    /// El mensaje rápido a los agentes abiertos en el Espacio.
    composer: Entity<TextInput>,
    /// Cuántos agentes hay abiertos en el Espacio (se relee cada segundo).
    consoles: usize,
    colors: Colors,
}

impl EventEmitter<AgentsEvent> for AgentsPanel {}

impl Focusable for AgentsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AgentsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let prefs = Prefs::load();
        let agent = prefs
            .agent
            .as_deref()
            .and_then(|cli| AGENTS.iter().position(|a| a.cli == cli))
            .unwrap_or(CLAUDE);
        Self::refresh_loop(Arc::new(Mutex::new(Watch::default())), cx);
        let composer = cx.new(|cx| {
            TextInput::new(
                "Mensaje rápido…",
                rgb(0xf0f0ea).into(),
                rgb(0x9a9a90).into(),
                rgb(0xf0f0ea).into(),
                cx,
            )
        });
        // `PILL_TRAY_SEND=texto`: a los 5 s lo manda como mensaje rápido, para
        // probar el camino hasta el espacio sin teclear.
        if let Ok(text) = std::env::var("PILL_TRAY_SEND") {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let _ = this.update(cx, |panel, cx| {
                    panel.composer.update(cx, |input, cx| input.set_text(text, cx));
                    panel.send(cx);
                });
            })
            .detach();
        }
        Self {
            focus: cx.focus_handle(),
            sessions: Vec::new(),
            available: [true; AGENTS.len()],
            agent,
            folders: Vec::new(),
            folder: prefs.folder,
            notice: None,
            pinned: false,
            max_height: None,
            inbox: crate::tray::Inbox::from_env(),
            nuevo_open: false,
            media: None,
            composer,
            consoles: 0,
            colors: Colors {
                text: rgb(0xf0f0ea).into(),
                muted: rgb(0x9a9a90).into(),
                faint: rgb(0x6e6e66).into(),
                working: rgb(0xe8b04b).into(),
                ready: rgb(0x6cc48a).into(),
                claude: rgb(0xd97757).into(),
            },
        }
    }

    /// Lee las sesiones cada segundo, aunque el notch esté cerrado: así al
    /// abrirlo ya están al día. Solo se leen las líneas nuevas.
    fn refresh_loop(watch: Arc<Mutex<Watch>>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            let watch = watch.clone();
            let sessions = cx
                .background_spawn(async move {
                    watch.lock().map(|mut w| w.tick(now_secs())).unwrap_or_default()
                })
                .await;
            if this
                .update(cx, |panel, cx| {
                    panel.inbox.observe(&sessions, now_secs(), &agent_in_front);
                    let waiting = crate::agent_prompts::waiting();
                    panel.inbox.sync_prompts(&waiting, now_secs());
                    crate::phone::set_agents(&sessions, &waiting);
                    panel.sessions = sessions;
                    panel.consoles = crate::space::agent_consoles(cx).max(panel.inbox.demo_consoles());
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
            cx.background_executor().timer(REFRESH_EVERY).await;
        })
        .detach();
    }

    /// Al abrir: qué agentes están instalados y las carpetas recientes.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.nuevo_open = false;
        cx.spawn(async move |this, cx| {
            let (available, folders) = cx
                .background_spawn(async move {
                    let available = AGENTS.each_ref().map(|agent| on_path(agent.cli));
                    (available, recent_folders(MAX_FOLDERS + 2))
                })
                .await;
            let _ = this.update(cx, |panel, cx| {
                panel.available = available;
                panel.folders = folders;
                if panel.folder.as_ref().is_none_or(|dir| !dir.is_dir()) {
                    panel.folder = panel.folders.first().cloned();
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Las sesiones de «En curso», con su lugar en `sessions`. Las que ya están
    /// en «Por revisar» no se repiten, y caben las que dejen los demás bloques
    /// dentro del alto máximo del notch.
    fn running_rows(&self) -> Vec<(usize, &Session)> {
        let room = ((MAX_HEIGHT - self.fixed_height()) / ROW_H).floor().max(1.0) as usize;
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| !self.inbox.has_session(&session.id))
            .take(MAX_ROWS.min(room))
            .collect()
    }

    /// «Nuevo» va abierto como siempre; con filas por revisar se pliega a una.
    fn nuevo_expanded(&self) -> bool {
        self.inbox.is_empty() || self.nuevo_open
    }

    /// Lo que mide «Por revisar» sin su título.
    fn pending_height(&self) -> f32 {
        let items = self.inbox.ordered();
        if items.is_empty() {
            return EMPTY_H;
        }
        let rows: f32 = items.iter().take(MAX_INBOX_ROWS).map(|item| tray::row_height(item.kind)).sum();
        rows + if items.len() > MAX_INBOX_ROWS { MORE_H } else { 0.0 }
    }

    /// Lo del notch que no es «En curso»: franja, bandeja, «Nuevo» y pie.
    fn fixed_height(&self) -> f32 {
        let new = if self.nuevo_expanded() {
            HEADER_H + PICKER_H + FOLDERS_H + LAUNCH_H + 6.0
        } else {
            NUEVO_ROW_H
        };
        BAND_H
            + HEADER_H
            + self.pending_height()
            + HEADER_H
            + new
            + self.composer_height()
            + self.player_height()
            + FOOTER_H
            + SIDE_PAD
    }

    /// El mensaje rápido solo está si hay a quién escribirle.
    fn composer_height(&self) -> f32 {
        if self.consoles > 0 {
            COMPOSER_H + COMPOSER_GAP
        } else {
            0.0
        }
    }

    /// Enter en el mensaje rápido: va a los agentes abiertos en el Espacio.
    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let sent = crate::space::send_to_agents(cx, &text);
        println!("bandeja → mensaje a {sent} agentes: {text}");
        self.notice = Some(if sent == 0 {
            "No hay agentes abiertos en el Espacio".into()
        } else {
            self.composer.update(cx, |input, cx| input.clear(cx));
            let who = if sent == 1 { "agente" } else { "agentes" };
            format!("Enviado a {sent} {who}").into()
        });
        cx.notify();
    }

    /// El mensaje rápido: una línea que se escribe a todos los agentes abiertos
    /// en el Espacio. Sin ninguno no hay a quién: no se muestra.
    fn render_composer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.consoles == 0 {
            return None;
        }
        let text = self.colors.text;
        let cream: Hsla = rgb(0xe8e8e0).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let target = if self.consoles == 1 {
            "1 agente".to_string()
        } else {
            format!("Todos {}", self.consoles)
        };
        Some(
            div()
                .h(px(COMPOSER_H))
                .flex_none()
                .mx(px(SIDE_PAD))
                .mt(px(COMPOSER_GAP))
                .px(px(5.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(COMPOSER_H / 2.))
                .bg(text.opacity(0.07))
                .child(
                    div()
                        .h(px(30.))
                        .px(px(12.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .rounded(px(15.))
                        .bg(cream)
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ink)
                        .child(SharedString::from(target)),
                )
                .child(div().flex_1().min_w_0().text_size(px(12.)).child(self.composer.clone()))
                .child(
                    div()
                        .id("tray-send")
                        .size(px(30.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(15.))
                        .bg(cream)
                        .cursor_pointer()
                        .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.send(cx)))
                        .child(svg().path("icons/arrow-up.svg").size(px(15.)).text_color(ink)),
                )
                .into_any_element(),
        )
    }

    /// Lo que suena ahora, si algo suena (o quedó en pausa).
    fn track(&self) -> Option<crate::media::Track> {
        self.media.as_ref()?.track()
    }

    fn player_height(&self) -> f32 {
        if self.track().is_some() {
            PLAYER_H + PLAYER_GAP
        } else {
            0.0
        }
    }

    /// El mini reproductor: lo que suena y anterior, pausa o siguiente. La
    /// música no desaparece al abrir el notch en Agentes. «Ahora suena» entero
    /// sigue en la carátula del tab.
    fn render_player(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::media::Control;
        let track = self.track()?;
        let (text, muted) = (self.colors.text, self.colors.muted);
        let cream: Hsla = rgb(0xe8e8e0).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let art = match track.art.clone() {
            Some(art) => img(art).size(px(28.)).flex_none().rounded(px(6.)).into_any_element(),
            None => div()
                .size(px(28.))
                .flex_none()
                .rounded(px(6.))
                .bg(text.opacity(0.08))
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path("icons/audio-lines.svg").size(px(14.)).text_color(muted))
                .into_any_element(),
        };
        let button = |id: &'static str,
                      icon: &'static str,
                      size: f32,
                      big: bool,
                      control: fn() -> Control,
                      cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(size))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(size / 2.))
                .cursor_pointer()
                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(media) = &panel.media {
                        media.control(control());
                    }
                    cx.notify();
                }))
                .child(svg().path(icon).size(px(15.)).text_color(if big { ink } else { text }))
                .hover_bg(
                    id,
                    if big { cream } else { text.opacity(0.0) },
                    if big { cream.opacity(0.86) } else { text.opacity(0.1) },
                )
        };
        let progress = track
            .position_now()
            .map(|(pos, end)| (pos / end.max(1.0)).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        // El artista y la app que suena; lo que no se sepa, no deja un « · » colgando.
        let source = crate::media::source_label(&track.source);
        let by = [track.artist.as_str(), source.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        Some(
            div()
                .relative()
                .h(px(PLAYER_H))
                .flex_none()
                .mx(px(SIDE_PAD))
                .mt(px(PLAYER_GAP))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(12.))
                .bg(text.opacity(0.05))
                .child(art)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().truncate().text_size(px(12.)).child(SharedString::from(track.title.clone())))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child(SharedString::from(by)),
                        ),
                )
                .child(button("tray-prev", "icons/skip-back.svg", 28., false, || Control::Previous, cx))
                .child(button(
                    "tray-toggle",
                    if track.playing { "icons/pause.svg" } else { "icons/play.svg" },
                    32.,
                    true,
                    || Control::Toggle,
                    cx,
                ))
                .child(button("tray-next", "icons/skip-forward.svg", 28., false, || Control::Next, cx))
                // La línea de progreso, al pie de la franja, bajo el título.
                .child(
                    div()
                        .absolute()
                        .left(px(46.))
                        .right(px(116.))
                        .bottom(px(0.))
                        .h(px(2.))
                        .rounded(px(1.))
                        .bg(text.opacity(0.12))
                        .child(div().h(px(2.)).rounded(px(1.)).bg(text).w(gpui::relative(progress))),
                )
                .into_any_element(),
        )
    }

    /// Lo que espera al usuario, para los contadores del tab y el vistazo.
    pub fn inbox(&self) -> &tray::Inbox {
        &self.inbox
    }

    /// «Ver»: lleva al frente la terminal del agente de esa fila y la descarta.
    /// Dice si lo logró; si no, el motivo queda en el pie.
    pub fn tray_view(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let Some(item) = self.inbox.get(id).cloned() else {
            return false;
        };
        if item.origin == tray::Origin::Demo {
            println!("bandeja → ver: {}", item.title);
            self.inbox.accept(id);
            cx.notify();
            return true;
        }
        match focus_agent(&AGENTS[item.agent]) {
            Ok(()) => {
                self.inbox.accept(id);
                cx.emit(AgentsEvent::Left);
                cx.notify();
                true
            }
            Err(error) => {
                self.notice = Some(error.into());
                cx.notify();
                false
            }
        }
    }

    /// «Aceptar»: la fila se descarta sin abrir nada.
    pub fn tray_accept(&mut self, id: u64, cx: &mut Context<Self>) {
        self.inbox.accept(id);
        cx.notify();
    }

    /// «Permitir» o «Negar»: se teclea en la consola del agente
    /// (`agent_prompts`). Si ya no se puede, el motivo queda en el pie.
    pub fn tray_decide(&mut self, id: u64, allow: bool, cx: &mut Context<Self>) {
        if let Some(tray::Origin::Prompt { session, id: prompt }) = self.inbox.get(id).map(|item| item.origin.clone()) {
            if let Err(error) = crate::agent_prompts::decide(&session, &prompt, allow) {
                self.notice = Some(error.into());
            }
        }
        self.inbox.accept(id);
        cx.notify();
    }

    /// Las carpetas como chips: la elegida primero, después las recientes.
    fn folder_chips(&self) -> Vec<PathBuf> {
        let mut chips: Vec<PathBuf> = self.folder.iter().cloned().collect();
        let same = |a: &Path, b: &Path| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy());
        for dir in &self.folders {
            if chips.len() >= MAX_FOLDERS {
                break;
            }
            if !chips.iter().any(|chip| same(chip, dir)) {
                chips.push(dir.clone());
            }
        }
        chips
    }

    /// El alto que necesita el notch, con la franja.
    pub fn desired_height(&self) -> f32 {
        let rows = self.running_rows().len();
        let running = if rows == 0 {
            EMPTY_H
        } else {
            rows as f32 * ROW_H
        };
        (self.fixed_height() + running).min(self.max_height.unwrap_or(MAX_HEIGHT))
    }

    fn save_prefs(&self) {
        Prefs {
            agent: Some(AGENTS[self.agent].cli.into()),
            folder: self.folder.clone(),
        }
        .save();
    }

    fn pick_agent(&mut self, index: usize, cx: &mut Context<Self>) {
        self.agent = index % AGENTS.len();
        self.save_prefs();
        cx.notify();
    }

    fn pick_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.folder = Some(dir);
        self.save_prefs();
        cx.notify();
    }

    fn browse_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Abrir el agente aquí".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(dir) = paths.into_iter().next() {
                    let _ = this.update(cx, |panel, cx| panel.pick_folder(dir, cx));
                }
            }
        })
        .detach();
    }

    fn launch(&mut self, cx: &mut Context<Self>) {
        let agent = &AGENTS[self.agent];
        let Some(dir) = self.folder.clone().or_else(home) else {
            self.notice = Some("Elige una carpeta".into());
            cx.notify();
            return;
        };
        println!("agentes → abrir {} en {}", agent.cli, dir.display());
        let open = if self.available[self.agent] {
            crate::space::Open::agent(agent.cli, agent.name, agent.cli, Some(dir))
        } else {
            crate::space::Open {
                label: format!("Instalar {}", agent.name),
                program: "powershell.exe".into(),
                args: vec!["-NoExit".into(), "-Command".into(), agent.install.into()],
                cwd: Some(dir),
                agent: Some(agent.cli),
            }
        };
        cx.emit(AgentsEvent::Open(open));
    }

    fn focus_session(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get(row) else {
            return;
        };
        match focus_agent(&AGENTS[session.agent]) {
            Ok(()) => cx.emit(AgentsEvent::Left),
            Err(error) => {
                let hint = if AGENTS[session.agent].resume.is_some() {
                    " · ↗ la reanuda aparte"
                } else {
                    ""
                };
                self.notice = Some(format!("{error}{hint}").into());
                cx.notify();
            }
        }
    }

    fn resume_session(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get(row) else {
            return;
        };
        let agent = &AGENTS[session.agent];
        let Some(template) = agent.resume else {
            return;
        };
        let dir = PathBuf::from(&session.cwd);
        let dir = if dir.is_dir() { dir } else { home().unwrap_or(dir) };
        let line = template.replace("{id}", &session.id);
        let title = format!("{} · {}", agent.name, folder_name(&session.cwd));
        println!("agentes → reanudar {} {}", agent.cli, session.id);
        cx.emit(AgentsEvent::Open(crate::space::Open::agent(agent.cli, &title, &line, Some(dir))));
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(AgentsEvent::Close);
    }

    fn launch_action(&mut self, _: &Launch, window: &mut Window, cx: &mut Context<Self>) {
        // Con el cursor en el mensaje rápido, Enter lo manda en vez de abrir
        // un agente nuevo.
        if self.composer.focus_handle(cx).is_focused(window) {
            self.send(cx);
            return;
        }
        self.launch(cx);
    }

    fn prev_agent(&mut self, _: &PrevAgent, _: &mut Window, cx: &mut Context<Self>) {
        self.pick_agent(self.agent + AGENTS.len() - 1, cx);
    }

    fn next_agent(&mut self, _: &NextAgent, _: &mut Window, cx: &mut Context<Self>) {
        self.pick_agent(self.agent + 1, cx);
    }

    fn logo(&self, agent: usize, size: f32, color: Hsla) -> impl IntoElement {
        // El logo de Claude va con su color, como en Atic; los demás, del
        // color del texto.
        let color = if agent == CLAUDE { self.colors.claude } else { color };
        svg()
            .path(AGENTS[agent].logo)
            .size(px(size))
            .flex_none()
            .text_color(color)
    }

    fn render_band(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, muted) = (self.colors.text, self.colors.muted);
        let counts = self.inbox.counts();
        let summary = match (counts.working, counts.waiting()) {
            (0, 0) => String::new(),
            (w, 0) => format!("{w} trabajando"),
            (0, r) => format!("{r} por revisar"),
            (w, r) => format!("{w} trabajando · {r} por revisar"),
        };
        div()
            .h(px(BAND_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pr(px(SIDE_PAD))
            .child(
                div()
                    .id("agents-mark")
                    .w(px(MARK_GAP))
                    .h_full()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(AgentsEvent::Close))),
            )
            .child(div().text_size(px(12.)).child("Agentes"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(summary),
            )
            .child(
                div()
                    .id("agents-space")
                    .h(px(24.))
                    .px(px(10.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .rounded(px(12.))
                    .text_size(px(11.))
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(AgentsEvent::Space)))
                    .child("Espacio")
                    .fx("agents-space", move |el, h| {
                        el.text_color(h.mix(muted, text))
                            .bg(h.mix(text.opacity(0.0), text.opacity(0.08)))
                    }),
            )
            .child(pin_button("agents-pin", self.pinned, text, self.colors.faint, cx.listener(
                |panel, _: &ClickEvent, _, cx| {
                    panel.pinned = !panel.pinned;
                    cx.notify();
                },
            )))
    }

    fn header(&self, label: &'static str) -> impl IntoElement {
        div()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_end()
            .pb(px(4.))
            .px(px(SIDE_PAD + 8.))
            .text_size(px(10.))
            .text_color(self.colors.faint)
            .child(label)
    }

    fn render_session(&self, row: usize, session: &Session, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, muted, faint) = (self.colors.text, self.colors.muted, self.colors.faint);
        let working = session.status == Status::Working;
        let dot = if working { self.colors.working } else { self.colors.ready };
        let detail = if working {
            session.activity.clone().unwrap_or_else(|| "Trabajando…".into())
        } else {
            session.preview.clone().unwrap_or_else(|| "Listo".into())
        };
        let can_resume = AGENTS[session.agent].resume.is_some();
        let resume = cx.listener(move |panel, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
            panel.resume_session(row, cx)
        });
        div()
            .id(("agent-row", row))
            .h(px(ROW_H))
            .mx(px(SIDE_PAD))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(10.))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.focus_session(row, cx)))
            .child(self.logo(session.agent, 16., muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().size(px(6.)).flex_none().rounded(px(3.)).bg(dot))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .child(folder_name(&session.cwd)),
                            ),
                    )
                    // Como en las filas del Clipboard: el texto recortado va
                    // dentro de una fila flex, si no se encoge a cero.
                    .child(
                        div().w_full().flex().child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(if working { muted } else { faint })
                                .child(detail),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(faint)
                    .child(ago(now - session.updated)),
            )
            .fx(("agent-row-fx", row), move |el, h| {
                // Retomar aparece con el cursor sobre la fila, fundiéndose.
                el.bg(h.mix(text.opacity(0.0), text.opacity(0.06))).when(can_resume, |el| {
                    el.child(
                        div()
                            .id(("agent-resume", row))
                            .size(px(24.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded(px(12.))
                            .opacity(h.t)
                            .when(h.t < 0.05, |el| el.invisible())
                            .on_click(resume)
                            .child(
                                svg()
                                    .path("icons/arrow-up-right.svg")
                                    .size(px(12.))
                                    .text_color(muted),
                            )
                            .hover_bg(("agent-resume-fx", row), text.opacity(0.0), text.opacity(0.1)),
                    )
                })
            })
    }

    /// Una fila de «Por revisar»: lo que pasó y los botones para responder.
    /// Las decisiones llevan un fondo azul suave; el resto, solo el realce al
    /// pasar el cursor.
    fn render_item(&self, index: usize, item: &tray::Item, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, kind) = (item.id, item.kind);
        let (wl, wr) = tray::button_widths(kind);
        let text = self.colors.text;
        let blue: Hsla = rgb(0x86b6ff).into();
        let review = kind == tray::Kind::Review;
        let left = tray::button(tray::left_label(kind), wl, tray::left_is_primary(kind), false)
            .id(("tray-left", index))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if review {
                    panel.tray_view(id, cx);
                } else {
                    panel.tray_decide(id, false, cx);
                }
            }))
            .fx(("tray-left-fx", index), |el, h| el.opacity(1.0 - 0.15 * h.t - 0.1 * h.press));
        let right = tray::button(tray::right_label(kind), wr, !tray::left_is_primary(kind), false)
            .id(("tray-right", index))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if review {
                    panel.tray_accept(id, cx);
                } else {
                    panel.tray_decide(id, true, cx);
                }
            }))
            .fx(("tray-right-fx", index), |el, h| el.opacity(1.0 - 0.15 * h.t - 0.1 * h.press));
        div()
            .id(("tray-item", index))
            .h(px(tray::row_height(kind)))
            .mx(px(SIDE_PAD))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(10.))
            .when(review, |el| {
                el.cursor_pointer().on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                    panel.tray_view(id, cx);
                }))
            })
            .child(tray::row_body(item, now))
            .child(left)
            .child(right)
            .fx(("tray-item-fx", index), move |el, h| {
                if review {
                    el.bg(h.mix(text.opacity(0.0), text.opacity(0.06)))
                } else {
                    el.bg(h.mix(blue.opacity(0.10), blue.opacity(0.14)))
                }
            })
    }

    /// «Nuevo» plegado: una fila que lo despliega.
    fn render_nuevo_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, muted) = (self.colors.text, self.colors.muted);
        div()
            .id("agents-nuevo")
            .h(px(NUEVO_ROW_H - 2.))
            .mt(px(2.))
            .mx(px(SIDE_PAD))
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .rounded(px(10.))
            .text_size(px(12.))
            .cursor_pointer()
            .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| {
                panel.nuevo_open = true;
                cx.notify();
            }))
            .fx("agents-nuevo-fx", move |el, h| {
                let fg = h.mix(muted, text);
                el.text_color(fg)
                    .bg(h.mix(text.opacity(0.0), text.opacity(0.06)))
                    .child(svg().path("icons/plus.svg").size(px(14.)).text_color(fg))
                    .child("Nuevo agente…")
            })
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.colors.text;
        div()
            .h(px(PICKER_H))
            .flex_none()
            .flex()
            .gap(px(4.))
            .px(px(SIDE_PAD))
            .children((0..AGENTS.len()).map(|index| {
                let selected = index == self.agent;
                let (logo_on, logo_off) = (
                    self.logo(index, 20., text).into_any_element(),
                    self.logo(index, 20., self.colors.muted).into_any_element(),
                );
                div()
                    .id(("agent-pick", index))
                    .flex_1()
                    .h(px(PICKER_H - 4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .when(selected, |el| el.border_1().border_color(text.opacity(0.18)))
                    .when(!self.available[index], |el| el.opacity(0.42))
                    .cursor_pointer()
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                        panel.pick_agent(index, cx)
                    }))
                    .fx(("agent-pick-fx", index), move |el, h| {
                        let (rest, over) = if selected { (0.12, 0.14) } else { (0.0, 0.08) };
                        // El logo se enciende con el cursor, como en la web.
                        el.bg(h.mix(text.opacity(rest), text.opacity(over)))
                            .child(if selected || h.t > 0.5 { logo_on } else { logo_off })
                    })
            }))
    }

    fn render_folders(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, muted) = (self.colors.text, self.colors.muted);
        let chosen = self.folder.clone();
        div()
            .h(px(FOLDERS_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(SIDE_PAD))
            .children(self.folder_chips().into_iter().enumerate().map(|(index, dir)| {
                let selected = chosen.as_ref() == Some(&dir);
                let name = folder_name(&dir.to_string_lossy());
                div()
                    .id(("agent-folder", index))
                    .h(px(26.))
                    .max_w(px(120.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(13.))
                    .text_size(px(11.))
                    .truncate()
                    .cursor_pointer()
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                        panel.pick_folder(dir.clone(), cx)
                    }))
                    .child(name)
                    .fx(("agent-folder-fx", index), move |el, h| {
                        let (rest, over) = if selected { (0.12, 0.14) } else { (0.0, 0.08) };
                        el.text_color(if selected { text } else { h.mix(muted, text) })
                            .bg(h.mix(text.opacity(rest), text.opacity(over)))
                    })
            }))
            .child(
                div()
                    .id("agent-browse")
                    .h(px(26.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(13.))
                    .text_size(px(11.))
                    .cursor_pointer()
                    .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.browse_folder(cx)))
                    .fx("agent-browse-fx", move |el, h| {
                        let fg = h.mix(muted, text);
                        el.text_color(fg)
                            .bg(h.mix(text.opacity(0.0), text.opacity(0.08)))
                            .child(svg().path("icons/folder.svg").size(px(12.)).text_color(fg))
                            .child("Otra…")
                    }),
            )
    }

    fn render_launch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.colors.text;
        let agent = &AGENTS[self.agent];
        let folder = self
            .folder
            .as_ref()
            .map(|dir| folder_name(&dir.to_string_lossy()))
            .unwrap_or_else(|| "tu carpeta".into());
        let label = if self.available[self.agent] {
            format!("Abrir {} en {folder}", agent.name)
        } else {
            format!("Instalar {}", agent.name)
        };
        div()
            .id("agent-launch")
            .h(px(LAUNCH_H - 6.))
            .mx(px(SIDE_PAD))
            .mt(px(6.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .rounded(px(12.))
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(rgb(0x1a1a18))
            .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.launch(cx)))
            .child(div().truncate().child(label))
            .fx("agent-launch-fx", move |el, h| {
                // Como `.launch` en la web: se aclara y sube un pixel; al
                // apretar vuelve a su sitio.
                let lift = (h.t - h.press).max(0.0);
                el.bg(h.mix(text.opacity(0.92), text)).mt(px(6. - lift)).mb(px(lift))
            })
    }
}

/// «ahora», «hace 3 min», «hace 2 h».
pub(crate) fn ago(seconds: i64) -> String {
    match seconds {
        s if s < 45 => "ahora".into(),
        s if s < 3600 => format!("hace {} min", (s + 30) / 60),
        s => format!("hace {} h", s / 3600),
    }
}

impl Render for AgentsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_secs();
        let sessions: Vec<(usize, Session)> =
            self.running_rows().into_iter().map(|(row, session)| (row, session.clone())).collect();
        let running = if sessions.is_empty() {
            div()
                .h(px(EMPTY_H))
                .flex_none()
                .flex()
                .items_center()
                .px(px(SIDE_PAD + 8.))
                .text_size(px(11.))
                .text_color(self.colors.muted)
                .child("Nada en curso. Lo que abras aparece aquí.")
                .into_any_element()
        } else {
            div()
                .flex_none()
                .flex()
                .flex_col()
                .children(
                    sessions
                        .iter()
                        .map(|(row, session)| self.render_session(*row, session, now, cx)),
                )
                .into_any_element()
        };
        let items = self.inbox.ordered();
        let pending = if items.is_empty() {
            div()
                .h(px(EMPTY_H))
                .flex_none()
                .flex()
                .items_center()
                .px(px(SIDE_PAD + 8.))
                .text_size(px(11.))
                .text_color(self.colors.muted)
                .child("Nada por revisar.")
                .into_any_element()
        } else {
            div()
                .flex_none()
                .flex()
                .flex_col()
                .children(
                    items
                        .iter()
                        .take(MAX_INBOX_ROWS)
                        .enumerate()
                        .map(|(index, item)| self.render_item(index, item, now, cx)),
                )
                .when(items.len() > MAX_INBOX_ROWS, |el| {
                    el.child(
                        div()
                            .h(px(MORE_H))
                            .flex_none()
                            .flex()
                            .items_center()
                            .px(px(SIDE_PAD + 8.))
                            .text_size(px(10.))
                            .text_color(self.colors.faint)
                            .child(SharedString::from(format!(
                                "{} más en la bandeja",
                                items.len() - MAX_INBOX_ROWS
                            ))),
                    )
                })
                .into_any_element()
        };
        // «Nuevo» va abierto como siempre; con filas por revisar se pliega.
        let new = if self.nuevo_expanded() {
            div()
                .flex_none()
                .flex()
                .flex_col()
                .child(self.header("NUEVO"))
                .child(self.render_picker(cx))
                .child(self.render_folders(cx))
                .child(self.render_launch(cx))
                .into_any_element()
        } else {
            self.render_nuevo_row(cx).into_any_element()
        };
        let footer = self.notice.clone().unwrap_or_else(|| {
            "Clic trae su terminal · ↗ la reanuda en el espacio · Enter abre · Esc cierra".into()
        });

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::launch_action))
            .on_action(cx.listener(Self::prev_agent))
            .on_action(cx.listener(Self::next_agent))
            .size_full()
            .flex()
            .flex_col()
            .pb(px(SIDE_PAD))
            .font_family("Segoe UI")
            .text_color(self.colors.text)
            .child(self.render_band(cx))
            .child(self.header("POR REVISAR"))
            .child(pending)
            .child(self.header("EN CURSO"))
            .child(running)
            .child(new)
            .children(self.render_composer(cx))
            .children(self.render_player(cx))
            .child(div().flex_1())
            .child(
                div()
                    .h(px(FOOTER_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(SIDE_PAD + 8.))
                    .text_size(px(10.))
                    .text_color(self.colors.faint)
                    .truncate()
                    .child(footer),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_prompt_abre_y_fin_de_turno_cierra() {
        let prompt = json!({"type": "user", "promptSource": "user", "message": {}});
        assert_eq!(claude_kind(&prompt), LineKind::Prompt);
        let tool = json!({"type": "assistant", "message": {"stop_reason": "tool_use"}});
        assert_eq!(claude_kind(&tool), LineKind::Activity);
        let end = json!({"type": "assistant", "message": {
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "\nListo: subí el cambio.\nMás detalle"}]
        }});
        assert_eq!(
            claude_kind(&end),
            LineKind::EndTurn {
                preview: Some("Listo: subí el cambio.".into())
            }
        );
        let side = json!({"type": "user", "promptSource": "user", "isSidechain": true});
        assert_eq!(claude_kind(&side), LineKind::Ignore);
    }

    #[test]
    fn actividad_dice_que_hace() {
        let edit = json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "voy"},
            {"type": "tool_use", "name": "Edit", "input": {"file_path": "C:\\a\\src\\main.rs"}}
        ]}});
        assert_eq!(claude_activity(&edit).as_deref(), Some("Editando main.rs"));
        let bash = json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test --locked -p x"}}
        ]}});
        assert_eq!(claude_activity(&bash).as_deref(), Some("Ejecutando cargo test"));
        let mcp = json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "mcp__atic__atic_list_agents", "input": {}}
        ]}});
        assert_eq!(claude_activity(&mcp).as_deref(), Some("Usando atic_list_agents"));
    }

    #[test]
    fn listo_no_vuelve_a_trabajando_por_actividad_suelta() {
        let mut t = Tracked::new(PathBuf::from("no-existe.jsonl"), None);
        t.apply(LineKind::Prompt, None, "C:\\x", 10);
        t.apply(LineKind::EndTurn { preview: None }, None, "", 20);
        t.apply(LineKind::Activity, Some("Leyendo a".into()), "", 30);
        assert_eq!(t.status, Some(Status::Ready));
        assert_eq!(t.updated, 20);
        assert_eq!(t.cwd, "C:\\x");
    }

    #[test]
    fn listo_desaparece_a_la_media_hora() {
        let mut t = Tracked::new(PathBuf::from("no-existe.jsonl"), None);
        t.apply(LineKind::EndTurn { preview: None }, None, "", 1000);
        assert!(t.keep(1000 + DISAPPEAR_SECS - 1));
        assert!(!t.keep(1000 + DISAPPEAR_SECS));
        t.apply(LineKind::Prompt, None, "", 1000);
        assert!(t.keep(1000 + DISAPPEAR_SECS * 10));
    }

    #[test]
    fn codex_tareas_y_rollouts() {
        assert_eq!(
            codex_kind(&json!({"type": "event_msg", "payload": {"type": "task_started"}})),
            LineKind::Prompt
        );
        assert_eq!(
            codex_kind(&json!({"type": "event_msg", "payload": {
                "type": "task_complete", "last_agent_message": "Hecho"
            }})),
            LineKind::EndTurn {
                preview: Some("Hecho".into())
            }
        );
        assert_eq!(
            codex_id("rollout-2026-10-03T10-11-12-0199aaaa-bbbb"),
            Some("0199aaaa-bbbb")
        );
        assert_eq!(codex_id("otro-archivo"), None);
    }

    #[test]
    fn lineas_cortadas_esperan_y_la_cabeza_se_descarta() {
        let mut tail = Tail {
            skip_head: true,
            ..Tail::default()
        };
        assert_eq!(consume(&mut tail, b"basura\r\n{\"a\":1}\n{\"b\""), vec!["{\"a\":1}"]);
        assert_eq!(consume(&mut tail, b":2}\n"), vec!["{\"b\":2}"]);
    }

    #[test]
    fn trabajando_va_primero() {
        let session = |id: &str, status, updated| Session {
            id: id.into(),
            agent: CLAUDE,
            cwd: String::new(),
            status,
            preview: None,
            activity: None,
            updated,
        };
        let mut list = vec![
            session("listo-nuevo", Status::Ready, 50),
            session("trabaja-viejo", Status::Working, 10),
            session("trabaja-nuevo", Status::Working, 40),
        ];
        sort_sessions(&mut list);
        let ids: Vec<_> = list.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["trabaja-nuevo", "trabaja-viejo", "listo-nuevo"]);
    }

    #[test]
    fn nombres_y_tiempos() {
        assert_eq!(folder_name("C:\\Users\\x\\Documents\\atic\\"), "atic");
        assert_eq!(folder_name("C:\\"), "C:");
        assert_eq!(ago(10), "ahora");
        assert_eq!(ago(170), "hace 3 min");
        assert_eq!(ago(7300), "hace 2 h");
    }
}
