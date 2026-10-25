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
//! reanuda en una terminal nueva. Una sesión que vive en Atic Code (su id es
//! el de una conversación, `code::link`) no tiene terminal: el clic y «Ver»
//! abren esa conversación, «Aceptar» la deja leída y el mensaje rápido le llega.
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
    EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, KeyBinding,
    SharedString, Window,
};
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
const FOOTER_H: f32 = 26.0;
/// Filas de «En curso» antes de cortar: el notch no debe tapar media pantalla.
const MAX_ROWS: usize = 5;
/// Filas de «Por revisar» antes de cortar; las demás se cuentan.
const MAX_INBOX_ROWS: usize = 4;
const MORE_H: f32 = 22.0;
/// Lo más alto que llega el notch con Agentes.
const MAX_HEIGHT: f32 = PANEL_H + 60.0;
/// El mini reproductor del pie, y el aire sobre él.
const PLAYER_H: f32 = 44.0;
const PLAYER_GAP: f32 = 6.0;
/// El aire sobre la tarjeta para contestar.
const COMPOSER_GAP: f32 = 4.0;
/// La tarjeta para contestar un mensaje del celular, arriba del panel.
const REPLY_H: f32 = 132.0;
const REFRESH_EVERY: Duration = Duration::from_millis(1000);
/// Lo que suena el celular al buscarlo: después vuelve solo a «Buscar».
const RING_FOR: Duration = Duration::from_secs(60);

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
    /// Cómo se instala: una línea de PowerShell en Windows y de shell en
    /// macOS (las mismas de Atic, `agentCatalog.ts`).
    pub(crate) install: &'static str,
    /// Cómo se retoma una sesión por su id.
    resume: Option<&'static str>,
}

/// Lo de Windows o lo de macOS.
const fn os(windows: &'static str, macos: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        macos
    } else {
        windows
    }
}

pub const AGENTS: [Agent; 6] = [
    Agent {
        cli: "claude",
        name: "Claude Code",
        logo: "icons/agents/claude.svg",
        exe: "claude.exe",
        install: os("irm https://claude.ai/install.ps1 | iex", "curl -fsSL https://claude.ai/install.sh | bash"),
        resume: Some("claude --resume {id}"),
    },
    Agent {
        cli: "opencode",
        name: "OpenCode",
        logo: "icons/agents/opencode.svg",
        exe: "opencode.exe",
        install: os("npm install -g opencode-ai", "curl -fsSL https://opencode.ai/install | bash"),
        resume: None,
    },
    Agent {
        cli: "codex",
        name: "Codex",
        logo: "icons/agents/openai.svg",
        exe: "codex.exe",
        install: os("npm install -g @openai/codex", "npm install -g @openai/codex"),
        resume: Some("codex resume {id}"),
    },
    Agent {
        cli: "cursor-agent",
        name: "Cursor",
        logo: "icons/agents/cursor.svg",
        exe: "cursor-agent.exe",
        install: os("irm 'https://cursor.com/install?win32=true' | iex", "curl https://cursor.com/install -fsS | bash"),
        resume: None,
    },
    Agent {
        cli: "agy",
        name: "Antigravity",
        logo: "icons/agents/antigravity.svg",
        exe: "agy.exe",
        install: os("irm https://antigravity.google/cli/install.ps1 | iex", "curl -fsSL https://antigravity.google/cli/install.sh | bash"),
        resume: None,
    },
    Agent {
        cli: "grok",
        name: "Grok",
        logo: "icons/agents/grok.svg",
        exe: "grok.exe",
        install: os("irm https://x.ai/cli/install.ps1 | iex", "curl -fsSL https://x.ai/cli/install.sh | bash"),
        resume: None,
    },
];

const CLAUDE: usize = 0;
const OPENCODE: usize = 1;
const CODEX: usize = 2;
const CURSOR: usize = 3;

actions!(agents_panel, [Dismiss, Launch]);

const KEY_CONTEXT: &str = "AgentsPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("enter", Launch, context),
    ]);
}

pub enum AgentsEvent {
    /// Cerrar devolviendo el foco a la app de antes.
    Close,
    /// Otra ventana se llevó el foco (la terminal): cerrar sin devolverlo.
    Left,
    /// Reanudar la sesión de un agente que no es Claude en la terminal de Atic
    /// Code: `line` corre en `dir`.
    TerminalResume { line: String, dir: PathBuf, title: String },
    /// Mostrar Atic Code.
    Code,
    /// Mostrar Atic Code con esa conversación (su clave): la sesión de la fila
    /// vive ahí y no en una terminal (`code::link`).
    CodeChat(String),
    /// Reanudar en Atic Code una sesión de Claude que no tiene conversación ahí:
    /// su id, la carpeta donde corrió y un título para la lista.
    CodeResume { session: String, cwd: std::path::PathBuf, title: String },
    /// No se pudo traer la terminal: abrir el notch en Agentes, que dice por qué
    /// (cuando «Ver» se tocó en el vistazo).
    Explain,
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
                self.activity = Some(crate::i18n::t("pill.activity.thinking").into());
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

pub(crate) fn home() -> Option<PathBuf> {
    crate::paths::home().map(std::ffi::OsString::from)
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
        Some("user") if v.get("promptSource").is_none() => Some(crate::i18n::t("pill.activity.thinking").into()),
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
                Some("thinking") => Some(crate::i18n::t("pill.activity.thinking").into()),
                _ => Some(crate::i18n::t("pill.activity.writing").into()),
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
            crate::i18n::t("pill.agentsList.verb.editing"),
            field("file_path").or_else(|| field("notebook_path")).map(file),
        ),
        "Read" => with(crate::i18n::t("pill.agentsList.verb.reading"), field("file_path").map(file)),
        "Grep" | "Glob" | "LS" => with(crate::i18n::t("pill.agentsList.verb.searching"), field("pattern").map(short)),
        "WebSearch" => with(crate::i18n::t("pill.agentsList.verb.searching"), field("query").map(short)),
        "WebFetch" => with(crate::i18n::t("pill.agentsList.verb.reading"), field("url").map(short)),
        "Bash" | "PowerShell" => with(
            crate::i18n::t("pill.agentsList.verb.running"),
            field("command").map(|command| {
                let first = command.lines().next().unwrap_or(command);
                short(&first.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
            }),
        ),
        "Task" | "Agent" => with(crate::i18n::t("pill.agentsList.verb.delegating"), field("description").map(short)),
        "TodoWrite" => crate::i18n::t("pill.activity.thinking").into(),
        other => with(crate::i18n::t("pill.agentsList.verb.using"), Some(short(other.rsplit("__").next().unwrap_or(other)))),
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

// --- Mirar en vivo -----------------------------------------------------------

/// Lo que escribe una sesión, en líneas para leer: el celular la mira en vivo
/// (`phone.rs`, `PhoneRequest::Watch`). Sale de su registro (`.jsonl` de
/// Claude Code o el rollout de Codex), así sirve igual para las consolas y
/// para las conversaciones de Atic Code.
pub struct Transcript {
    path: PathBuf,
    codex: bool,
    tail: Tail,
}

/// Cuánto se lee hacia atrás al empezar a mirar: alcanza para las últimas
/// ~200 líneas aunque haya resultados de herramientas largos en medio.
const WATCH_BACK: u64 = 2 * 1024 * 1024;

impl Transcript {
    /// El registro de la sesión `id`, si se encuentra.
    pub fn open(id: &str) -> Option<Self> {
        let (path, codex) = find_transcript(id)?;
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let tail = if len > WATCH_BACK {
            Tail { offset: len - WATCH_BACK, carry: Vec::new(), skip_head: true }
        } else {
            Tail::default()
        };
        Some(Self { path, codex, tail })
    }

    /// Las líneas nuevas desde la última vez.
    pub fn read(&mut self) -> Vec<String> {
        read_new_lines(&self.path, &mut self.tail)
            .iter()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .flat_map(|v| if self.codex { codex_lines(&v) } else { claude_lines(&v) })
            .collect()
    }
}

fn find_transcript(id: &str) -> Option<(PathBuf, bool)> {
    if id.is_empty() || id.contains(['/', '\\']) {
        return None;
    }
    if let Some(root) = claude_root() {
        let found = std::fs::read_dir(root).ok()?.flatten().map(|project| project.path().join(format!("{id}.jsonl"))).find(|path| path.is_file());
        if let Some(path) = found {
            return Some((path, false));
        }
    }
    // Codex: la carpeta del día; una sesión mirada en vivo es de la última semana.
    let root = codex_root()?;
    for back in 0..7 {
        let day = chrono::Local::now() - chrono::Duration::days(back);
        let dir = root.join(day.format("%Y").to_string()).join(day.format("%m").to_string()).join(day.format("%d").to_string());
        let Ok(files) = std::fs::read_dir(dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.file_stem().and_then(|s| s.to_str()).and_then(codex_id) == Some(id) {
                return Some((path, true));
            }
        }
    }
    None
}

/// `› ` antes de lo que escribiste; `• ` antes de cada herramienta.
fn prompt_lines(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![String::new()];
    lines.extend(text.lines().enumerate().map(|(i, line)| if i == 0 { format!("› {line}") } else { format!("  {line}") }));
    lines
}

fn text_lines(text: &str) -> Vec<String> {
    let text = text.trim_end();
    if text.trim().is_empty() {
        return Vec::new();
    }
    text.lines().map(str::to_string).collect()
}

fn claude_lines(v: &Value) -> Vec<String> {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return Vec::new();
    }
    let content = v.pointer("/message/content");
    match v.get("type").and_then(Value::as_str) {
        Some("user") => match content {
            Some(Value::String(text)) => prompt_lines(text),
            // Los resultados de herramientas no: son largos y ya se ve qué se usó.
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .filter(|t| !t.starts_with('<'))
                .flat_map(prompt_lines)
                .collect(),
            _ => Vec::new(),
        },
        Some("assistant") => content
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|block| match block.get("type").and_then(Value::as_str) {
                Some("text") => text_lines(block.get("text").and_then(Value::as_str).unwrap_or("")),
                Some("tool_use") => vec![format!("• {}", tool_activity(block))],
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn codex_lines(v: &Value) -> Vec<String> {
    let payload = |key: &str| v.pointer(&format!("/payload/{key}")).and_then(Value::as_str);
    match (v.get("type").and_then(Value::as_str), payload("type")) {
        (Some("event_msg"), Some("user_message")) => prompt_lines(payload("message").unwrap_or("")),
        (Some("event_msg"), Some("agent_message")) => text_lines(payload("message").unwrap_or("")),
        (Some("response_item"), Some("function_call" | "custom_tool_call" | "mcp_tool_call")) => {
            let name = payload("name").unwrap_or("herramienta");
            // `shell`: el comando, como en Claude.
            let command = payload("arguments")
                .and_then(|args| serde_json::from_str::<Value>(args).ok())
                .and_then(|args| match args.get("command") {
                    Some(Value::Array(parts)) => Some(parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")),
                    Some(Value::String(command)) => Some(command.clone()),
                    _ => None,
                });
            match command {
                Some(command) => vec![crate::i18n::tf("pill.agentsList.bulletRunning", &[("command", &short(command.lines().next().unwrap_or("")))])],
                None => vec![crate::i18n::tf("pill.agentsList.bulletUsing", &[("name", &name)])],
            }
        }
        _ => Vec::new(),
    }
}

// --- Carpetas --------------------------------------------------------------

pub(crate) fn folder_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

// --- Terminal -----------------------------------------------------------------

/// Si el CLI está en el PATH (o donde lo dejan los instaladores nativos).
pub(crate) fn on_path(cli: &str) -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    if let Some(home) = home() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".grok").join("bin"));
    }
    if !cfg!(windows) {
        // macOS: sin extensión, y además donde los dejan Homebrew, nvm y los
        // instaladores (la app abierta desde el Finder no hereda ese PATH).
        return atic_agents::exe::resolve(cli).is_some() || dirs.iter().any(|dir| dir.join(cli).is_file());
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
        return Err(crate::i18n::tf("pill.agentsList.notOpen", &[("name", &agent.name)]));
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

/// Lo lento de buscar la terminal de un agente, para tomarlo fuera del hilo de
/// la UI: en macOS, los procesos de `ps`. En Windows la foto de procesos es
/// rápida y se toma al buscar.
#[cfg(target_os = "macos")]
type Processes = Vec<(u32, u32, String)>;
#[cfg(not(target_os = "macos"))]
type Processes = ();

#[cfg(target_os = "macos")]
fn processes() -> Processes {
    process_snapshot()
}

#[cfg(not(target_os = "macos"))]
fn processes() -> Processes {}

/// Trae al frente la ventana de la terminal donde corre el agente, si hay
/// una sola posible. `Err` dice por qué no.
#[cfg(windows)]
fn focus_agent(agent: &Agent, _: &Processes) -> Result<(), String> {
    let found = agent_windows(agent)?;
    match found.len() {
        0 => Err(crate::i18n::tf("pill.agentsList.noWindow", &[("name", &agent.name)])),
        1 => {
            let hwnd = *found.iter().next().unwrap_or(&0);
            restore_if_minimized(hwnd);
            crate::paste::force_foreground(hwnd);
            Ok(())
        }
        n => Err(crate::i18n::tf("pill.agentsList.manyWindows", &[("name", &agent.name), ("n", &n)])),
    }
}

/// macOS: los procesos con `ps`, y de ahí hacia arriba hasta la primera app
/// con Dock (Terminal, iTerm, Ghostty, el editor…), que es la que se trae
/// al frente. Una app con varias pestañas sale con la última que se usó.
#[cfg(target_os = "macos")]
fn agent_apps(agent: &Agent, processes: &[(u32, u32, String)]) -> Result<HashSet<i32>, String> {
    let parents: HashMap<u32, (u32, &str)> =
        processes.iter().map(|(pid, ppid, name)| (*pid, (*ppid, name.as_str()))).collect();
    let own = std::process::id();
    let stem = agent.exe.trim_end_matches(".exe");
    let pids: Vec<u32> = processes
        .iter()
        .filter(|(_, _, name)| name == stem)
        .map(|(pid, _, _)| *pid)
        .filter(|&pid| ancestors(pid, &parents).all(|(id, name)| id != own && name != "atic-desktop"))
        .collect();
    if pids.is_empty() {
        return Err(crate::i18n::tf("pill.agentsList.notOpen", &[("name", &agent.name)]));
    }
    let regular = regular_apps();
    Ok(pids
        .iter()
        .filter_map(|&pid| ancestors(pid, &parents).find_map(|(id, _)| regular.contains(&(id as i32)).then_some(id as i32)))
        .collect())
}

/// Los pids de las apps con Dock.
#[cfg(target_os = "macos")]
fn regular_apps() -> HashSet<i32> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSArray;
    objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: Retained<NSArray<AnyObject>> = msg_send![&workspace, runningApplications];
        apps.iter()
            .filter(|app| {
                let policy: isize = msg_send![&**app, activationPolicy];
                policy == 0
            })
            .map(|app| msg_send![&*app, processIdentifier])
            .collect()
    })
}

#[cfg(target_os = "macos")]
fn focus_agent(agent: &Agent, processes: &Processes) -> Result<(), String> {
    let found = agent_apps(agent, processes)?;
    match found.len() {
        0 => Err(crate::i18n::tf("pill.agentsList.noWindow", &[("name", &agent.name)])),
        1 => {
            crate::paste::force_foreground(*found.iter().next().unwrap_or(&0) as isize);
            Ok(())
        }
        n => Err(crate::i18n::tf("pill.agentsList.manyApps", &[("name", &agent.name), ("n", &n)])),
    }
}

#[cfg(target_os = "macos")]
fn process_snapshot() -> Vec<(u32, u32, String)> {
    let out = std::process::Command::new("ps").args(["-axww", "-o", "pid=,ppid=,comm="]).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let exe = parts.collect::<Vec<_>>().join(" ");
            let name = exe.rsplit('/').next().unwrap_or(&exe).to_lowercase();
            Some((pid, ppid, name))
        })
        .collect()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn focus_agent(agent: &Agent, _: &Processes) -> Result<(), String> {
    Err(crate::i18n::tf("pill.agentsList.notOpen", &[("name", &agent.name)]))
}

/// Si el usuario está mirando la terminal de ese agente ahora mismo: un turno
/// que termina delante de él no necesita avisarle (`tray.rs`).
#[cfg(windows)]
pub(crate) fn agent_in_front(agent: usize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let front = unsafe { GetForegroundWindow() } as isize;
    front != 0 && agent_windows(&AGENTS[agent]).is_ok_and(|found| found.contains(&front))
}

#[cfg(target_os = "macos")]
pub(crate) fn agent_in_front(agent: usize) -> bool {
    let Some(front) = crate::paste::dictation_target() else {
        return false;
    };
    agent_apps(&AGENTS[agent], &process_snapshot()).is_ok_and(|found| found.contains(&(front as i32)))
}

#[cfg(not(any(windows, target_os = "macos")))]
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
    #[cfg(any(windows, target_os = "macos"))]
    {
        use atic_agents::cursor;
        // Un `cursor-agent` hijo del IDE no es el TUI. En macOS los procesos
        // van sin `.exe` (`process_snapshot` los da en minúsculas).
        let (exe, ide) = if cfg!(windows) { (cursor::EXE, "cursor.exe") } else { ("cursor-agent", "cursor") };
        let pids = cursor::pids_outside(&process_snapshot(), exe, &[ide]);
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
    /// El chat rápido con Atic Code (`notch_chat.rs`): su alto y si hay una
    /// conversación, leídos cada vez que cambia.
    chat: Entity<crate::notch_chat::NotchChat>,
    chat_h: f32,
    chat_active: bool,
    _chat_subs: [gpui::Subscription; 2],
    sessions: Vec<Session>,
    /// Lo último que pasó (un error, «no sé cuál ventana es»), en el pie.
    notice: Option<SharedString>,
    pub pinned: bool,
    /// El tope de alto del panel: en un costado, casi toda la pantalla (lo
    /// pone la pill); si no, el de siempre.
    pub max_height: Option<f32>,
    /// Lo que espera al usuario: turnos terminados y permisos (`tray.rs`).
    inbox: crate::tray::Inbox,
    /// Lo que suena (`media.rs`): si hay algo, el pie lleva su mini reproductor.
    pub media: Option<crate::media::Media>,
    /// El mensaje rápido a las conversaciones de Atic Code por revisar.
    composer: Entity<TextInput>,
    /// El mensaje del celular que se está contestando (id de la fila) y dónde
    /// se escribe la respuesta.
    reply: Option<u64>,
    reply_input: Entity<TextInput>,
    /// La sección «Celular»: el mismo panel con solo lo del celular (avisos y
    /// llamada). Sin ella, Agentes no los muestra.
    phone_mode: bool,
    /// «Buscar celular» lo tiene sonando. El celular no avisa cuando para:
    /// se vuelve a «Buscar» al minuto, lo que dura allá. `ring_seq` descarta
    /// el regreso de un toque anterior.
    ringing: bool,
    ring_seq: u64,
    /// Las consolas de la demo de la bandeja (`tray::Inbox::demo_consoles`).
    consoles: usize,
    /// Las sesiones que viven en una conversación de Atic Code (se relee cada
    /// segundo): esas filas abren su chat en vez de buscar una terminal.
    code_links: Vec<crate::code::link::ChatLink>,
    colors: Colors,
}

impl EventEmitter<AgentsEvent> for AgentsPanel {}

impl Focusable for AgentsPanel {
    /// Con una respuesta en curso, el foco va a su campo: abrir el notch para
    /// contestar deja escribiendo de una.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.reply.is_some() {
            self.reply_input.focus_handle(cx)
        } else if !self.phone_mode {
            // Abrir Agentes deja escribiendo en el chat con Atic Code.
            self.chat.read(cx).composer.focus_handle(cx)
        } else {
            self.focus.clone()
        }
    }
}

impl AgentsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::refresh_loop(Arc::new(Mutex::new(Watch::default())), cx);
        let composer = cx.new(|cx| {
            TextInput::new(
                crate::i18n::t("pill.agentsList.quickMessage"),
                rgb(0xf0f0ea).into(),
                rgb(0x9a9a90).into(),
                rgb(0xf0f0ea).into(),
                cx,
            )
        });
        let reply_input = cx.new(|cx| {
            TextInput::new(crate::i18n::t("pill.agentsList.replyPlaceholder"), rgb(0xf0f0ea).into(), rgb(0x9a9a90).into(), rgb(0xf0f0ea).into(), cx)
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
        let chat = cx.new(crate::notch_chat::NotchChat::new);
        let chat_subs = [
            cx.observe(&chat, |panel, chat, cx| {
                let chat = chat.read(cx);
                panel.chat_h = chat.height();
                panel.chat_active = chat.active();
                cx.notify();
            }),
            cx.subscribe(&chat, |_, _, event: &crate::notch_chat::NotchChatEvent, cx| match event {
                crate::notch_chat::NotchChatEvent::Resume { session, cwd, title } => cx.emit(AgentsEvent::CodeResume {
                    session: session.clone(),
                    cwd: cwd.clone(),
                    title: title.clone(),
                }),
            }),
        ];
        let chat_h = chat.read(cx).height();
        Self {
            focus: cx.focus_handle(),
            chat,
            chat_h,
            chat_active: false,
            _chat_subs: chat_subs,
            sessions: Vec::new(),
            notice: None,
            pinned: false,
            max_height: None,
            inbox: crate::tray::Inbox::from_env(),
            media: None,
            composer,
            reply: None,
            reply_input,
            phone_mode: false,
            ringing: false,
            ring_seq: 0,
            consoles: 0,
            code_links: Vec::new(),
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
                    // Un turno de Atic Code se está mirando si su conversación está a
                    // la vista; ahí no hay terminal que buscar (ni `ps` en macOS).
                    let links = crate::code::link::chat_links(cx);
                    // Solo lo que vive en Atic Code: las sesiones de Claude,
                    // Codex u otros en terminales sueltas ya no se muestran.
                    let sessions: Vec<Session> = sessions
                        .into_iter()
                        .filter(|session| crate::code::link::link_for(&links, &session.id).is_some())
                        .collect();
                    let in_front = |session: &Session| match crate::code::link::link_for(&links, &session.id) {
                        Some(link) => link.in_view,
                        None => agent_in_front(session.agent),
                    };
                    panel.inbox.observe(&sessions, now_secs(), &in_front);
                    panel.code_links = links;
                    let waiting = crate::agent_prompts::waiting();
                    panel.inbox.sync_prompts(&waiting, now_secs());
                    panel.inbox.sync_phone(&crate::phone::alerts(), now_secs());
                    // Una versión nueva de Atic también avisa en la bandeja.
                    panel.inbox.sync_update(crate::updater::available().map(|u| u.version).as_deref(), now_secs());
                    crate::phone::set_agents(&sessions, &waiting);
                    panel.sessions = sessions;
                    panel.consoles = panel.inbox.demo_consoles();
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

    /// Al abrir: sin el aviso de la vez anterior.
    /// Archivos soltados sobre el chat (desde el Portapapeles de al lado).
    pub fn chat_attach(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.chat.update(cx, |chat, cx| chat.attach(paths, cx));
    }

    /// Texto soltado sobre el chat: se suma a lo que haya en su campo.
    pub fn chat_insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.chat.update(cx, |chat, cx| chat.insert_text(text, cx));
    }

    /// El panel no se cierra al hacer clic fuera: fijado, o con Atic Code
    /// respondiendo en el chat (se puede ir a otra app mientras tanto).
    pub fn holds_open(&self, cx: &App) -> bool {
        self.pinned || self.chat.read(cx).busy()
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.chat.update(cx, |chat, cx| chat.refresh(cx));
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

    /// Lo que mide «Por revisar» sin su título.
    /// Las filas de la bandeja de esta sección: lo del celular en «Celular», lo
    /// demás en Agentes.
    fn shown_items(&self) -> Vec<tray::Item> {
        self.inbox.ordered().into_iter().filter(|item| item.phone.is_some() == self.phone_mode).collect()
    }

    /// Agentes o «Celular» (al abrir una u otra en el notch).
    pub fn set_phone_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.phone_mode != on {
            self.phone_mode = on;
            if !on {
                self.reply = None;
            }
            cx.notify();
        }
    }

    /// Hace sonar los celulares conectados, o los calla.
    fn toggle_ring(&mut self, cx: &mut Context<Self>) {
        let on = !self.ringing;
        let devices = crate::phone::status().devices.into_iter().filter(|(_, _, connected)| *connected);
        let sent = devices.fold(false, |any, (id, _, _)| crate::phone::ring_phone(&id, on) | any);
        if on && !sent {
            self.notice = Some(crate::i18n::t("pill.agentsList.phoneNoRing").into());
            cx.notify();
            return;
        }
        self.ringing = on;
        self.ring_seq += 1;
        if on {
            let seq = self.ring_seq;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RING_FOR).await;
                let _ = this.update(cx, |panel, cx| {
                    if panel.ring_seq == seq {
                        panel.ringing = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn pending_height(&self) -> f32 {
        let items = self.shown_items();
        if items.is_empty() {
            return EMPTY_H;
        }
        let rows: f32 = items.iter().take(MAX_INBOX_ROWS).map(|item| tray::row_height(item.kind)).sum();
        rows + if items.len() > MAX_INBOX_ROWS { MORE_H } else { 0.0 }
    }

    /// Lo del notch que no es «En curso»: franja, bandeja, «Nuevo» y pie.
    fn fixed_height(&self) -> f32 {
        BAND_H
            + self.reply_height()
            + HEADER_H
            + self.pending_height()
            + HEADER_H
            + self.chat_h
            + self.player_height()
            + FOOTER_H
            + SIDE_PAD
    }

    /// La conversación de Atic Code donde vive esa sesión, si es de Atic Code.
    fn chat_of(&self, session: &str) -> Option<String> {
        crate::code::link::link_for(&self.code_links, session).map(|link| link.key.clone())
    }

    /// Las conversaciones de Atic Code por revisar: el mensaje rápido también les
    /// llega, como respuesta a su turno.
    fn code_targets(&self) -> Vec<(u64, String)> {
        let mut targets: Vec<(u64, String)> = Vec::new();
        for item in self.inbox.ordered() {
            let tray::Origin::Session(session) = &item.origin else {
                continue;
            };
            if let Some(key) = self.chat_of(session) {
                if !targets.iter().any(|(_, k)| *k == key) {
                    targets.push((item.id, key));
                }
            }
        }
        targets
    }



    /// Enter en el mensaje rápido: va a las conversaciones de Atic Code por revisar.
    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut sent = 0;
        for (id, key) in self.code_targets() {
            if crate::code::link::send_to_chat(cx, &key, &text) {
                sent += 1;
                // Ya tiene respuesta: su fila se va.
                self.inbox.accept(id);
            }
        }
        println!("bandeja → mensaje a {sent} agentes: {text}");
        self.notice = Some(if sent == 0 {
            crate::i18n::t("pill.agentsList.noAgentsOpen").into()
        } else {
            self.composer.update(cx, |input, cx| input.clear(cx));
            crate::i18n::tf(
                if sent == 1 { "pill.agentsList.sentOne" } else { "pill.agentsList.sentMany" },
                &[("sent", &sent)],
            )
            .into()
        });
        cx.notify();
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
        // Un turno de Atic Code: se abre su conversación.
        if let Some(key) = match &item.origin {
            tray::Origin::Session(session) => self.chat_of(session),
            _ => None,
        } {
            self.inbox.accept(id);
            cx.emit(AgentsEvent::CodeChat(key));
            cx.notify();
            return true;
        }
        self.focus_terminal(item.agent, cx, move |panel, result, cx| {
            match result {
                Ok(()) => {
                    panel.inbox.accept(id);
                    cx.emit(AgentsEvent::Left);
                }
                Err(error) => {
                    panel.notice = Some(error.into());
                    cx.emit(AgentsEvent::Explain);
                }
            }
            cx.notify();
        });
        true
    }

    /// Trae al frente la terminal del agente sin trabar la UI: lo lento (en
    /// macOS, `ps`) corre aparte y el resultado llega a `done`.
    fn focus_terminal(
        &mut self,
        agent: usize,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            let processes = cx.background_spawn(async move { processes() }).await;
            let _ = this.update(cx, |panel, cx| {
                let result = focus_agent(&AGENTS[agent], &processes);
                done(panel, result, cx);
            });
        })
        .detach();
    }

    /// «Aceptar»: la fila se descarta sin abrir nada.
    /// «Luego» en la actualización: no vuelve a avisarse hasta otra versión.
    pub fn tray_skip_update(&mut self, id: u64, cx: &mut Context<Self>) {
        self.inbox.skip_update(id);
        cx.notify();
    }

    /// «Instalar»: descarga y comprueba el instalador como en Ajustes y cierra la
    /// pill; el instalador la vuelve a abrir al terminar.
    pub fn tray_install(&mut self, cx: &mut Context<Self>) {
        let Some(update) = crate::updater::available() else {
            return;
        };
        self.notice = Some(crate::i18n::t("pill.agentsList.downloading").into());
        cx.spawn(async move |panel, cx| {
            let ok = cx.background_spawn(async move { crate::updater::install(&update) }).await;
            if ok {
                let _ = cx.update(|cx| cx.quit());
            } else {
                let _ = panel.update(cx, |panel, cx| {
                    panel.notice = Some(crate::i18n::t("pill.agentsList.installFailed").into());
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// «Aceptar»: la fila se descarta sin abrir nada; si el turno es de Atic Code,
    /// su conversación también queda leída.
    pub fn tray_accept(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(tray::Origin::Session(session)) = self.inbox.get(id).map(|item| item.origin.clone()) {
            if let Some(key) = self.chat_of(&session) {
                crate::code::link::mark_read(cx, &key);
            }
        }
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

    /// Los botones de un aviso o de la llamada del celular. Aviso: «Descartar»
    /// (`left`, también en el celular) o «Listo» (solo aquí). Sonando: «Colgar»
    /// o «Contestar». En llamada: «Ocultar» o «Colgar».
    pub fn tray_phone(&mut self, id: u64, left: bool, cx: &mut Context<Self>) {
        let Some(item) = self.inbox.get(id).cloned() else {
            return;
        };
        match (&item.origin, item.kind, left) {
            // Descartar la conversación: todos sus avisos, también en el celular.
            (tray::Origin::PhoneNotice { device, .. }, _, true) => {
                for key in item.phone.as_ref().map(|p| p.keys.clone()).unwrap_or_default() {
                    crate::phone::notice_dismiss(device, &key);
                }
                self.inbox.accept(id);
            }
            (tray::Origin::PhoneNotice { .. }, _, false) => self.inbox.hide_phone(id),
            (tray::Origin::PhoneCall { device }, tray::Kind::Ringing, left) => crate::phone::call_action(device, !left),
            (tray::Origin::PhoneCall { .. }, _, true) => self.inbox.hide_phone(id),
            (tray::Origin::PhoneCall { device }, _, false) => crate::phone::call_action(device, false),
            _ => {}
        }
        cx.notify();
    }

    /// Pone al día las filas del celular ya (`phone::take_alerts_changed`).
    pub fn sync_phone_now(&mut self, cx: &mut Context<Self>) {
        self.inbox.sync_phone(&crate::phone::alerts(), now_secs());
        // El mensaje que se contestaba ya no está (se leyó en el celular).
        if self.reply.is_some_and(|id| self.inbox.get(id).is_none()) {
            self.reply = None;
        }
        cx.notify();
    }

    /// «Responder»: la tarjeta con el mensaje y el campo, arriba del panel.
    pub fn start_reply(&mut self, id: u64, cx: &mut Context<Self>) {
        self.reply = Some(id);
        self.notice = None;
        self.reply_input.update(cx, |input, cx| input.clear(cx));
        cx.notify();
    }

    fn reply_height(&self) -> f32 {
        if self.reply.is_some() { REPLY_H } else { 0.0 }
    }

    /// Enter en la respuesta: va al celular, que la manda con la respuesta
    /// rápida de la notificación. La fila se va y el notch se cierra solo.
    fn send_reply(&mut self, cx: &mut Context<Self>) {
        let text = self.reply_input.read(cx).text().trim().to_string();
        let Some(id) = self.reply else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let Some(item) = self.inbox.get(id).cloned() else {
            self.reply = None;
            cx.notify();
            return;
        };
        let sent = item
            .phone
            .as_ref()
            .and_then(|phone| phone.keys.last().map(|key| crate::phone::notice_reply(&phone.device, key, &text)))
            .unwrap_or(false);
        if !sent {
            self.notice = Some(crate::i18n::t("pill.agentsList.sendFailed").into());
            cx.notify();
            return;
        }
        self.reply = None;
        self.reply_input.update(cx, |input, cx| input.clear(cx));
        self.inbox.hide_phone(id);
        self.notice = Some(crate::i18n::tf("pill.agentsList.sentTo", &[("title", &item.title)]).into());
        cx.notify();
        // Se ve «Enviado» un momento y el notch se recoge.
        cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(Duration::from_millis(900)).await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.reply.is_none() {
                    cx.emit(AgentsEvent::Close);
                }
            });
        })
        .detach();
    }

    /// La tarjeta de la respuesta: de quién es, el último mensaje y el campo.
    fn render_reply(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let item = self.inbox.get(self.reply?)?.clone();
        let phone = item.phone.clone().unwrap_or_default();
        let (text, muted, faint) = (self.colors.text, self.colors.muted, self.colors.faint);
        let lead = match &phone.icon {
            Some(icon) => gpui::img(icon.clone()).size(px(30.)).flex_none().rounded_full().into_any_element(),
            None => div()
                .size(px(30.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(text.opacity(0.10))
                .child(svg().path("icons/smartphone.svg").size(px(15.)).text_color(text))
                .into_any_element(),
        };
        Some(
            div()
                .h(px(REPLY_H - COMPOSER_GAP))
                .flex_none()
                .mx(px(SIDE_PAD))
                .mt(px(COMPOSER_GAP))
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .rounded(px(16.))
                .bg(text.opacity(0.07))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(lead)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().truncate().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(SharedString::from(item.title.clone())))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_size(px(10.))
                                        .text_color(faint)
                                        .child(svg().path("icons/smartphone.svg").size(px(10.)).text_color(faint))
                                        .child(SharedString::from(phone.app.clone())),
                                ),
                        ),
                )
                .child(div().line_clamp(2).text_size(px(12.)).text_color(muted).child(SharedString::from(item.detail.clone())))
                .child(
                    div()
                        .h(px(34.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .rounded(px(17.))
                        .bg(text.opacity(0.08))
                        .text_size(px(12.))
                        .child(div().flex_1().min_w_0().child(self.reply_input.clone()))
                        .child(div().flex_none().text_size(px(10.)).text_color(faint).child(crate::i18n::t("pill.agentsList.replyHint"))),
                )
                .into_any_element(),
        )
    }

    /// El alto que necesita el notch, con la franja.
    pub fn desired_height(&self) -> f32 {
        if self.phone_mode {
            return (BAND_H + self.reply_height() + self.pending_height() + FOOTER_H + SIDE_PAD)
                .min(self.max_height.unwrap_or(MAX_HEIGHT));
        }
        // Conversando, el chat ocupa el lugar de las listas.
        if self.chat_active {
            return (BAND_H + self.reply_height() + self.chat_h + self.player_height() + FOOTER_H + SIDE_PAD)
                .min(self.max_height.unwrap_or(MAX_HEIGHT));
        }
        let rows = self.running_rows().len();
        let running = if rows == 0 {
            EMPTY_H
        } else {
            rows as f32 * ROW_H
        };
        (self.fixed_height() + running).min(self.max_height.unwrap_or(MAX_HEIGHT))
    }


    fn focus_session(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get(row) else {
            return;
        };
        // Vive en Atic Code: se abre su conversación, no hay terminal.
        if let Some(key) = self.chat_of(&session.id) {
            cx.emit(AgentsEvent::CodeChat(key));
            return;
        }
        let agent = session.agent;
        self.focus_terminal(agent, cx, move |panel, result, cx| match result {
            Ok(()) => cx.emit(AgentsEvent::Left),
            Err(error) => {
                let hint = if agent == CLAUDE {
                    crate::i18n::t("pill.agentsList.hintResumeCode")
                } else if AGENTS[agent].resume.is_some() {
                    crate::i18n::t("pill.agentsList.hintResumeApart")
                } else {
                    ""
                };
                panel.notice = Some(format!("{error}{hint}").into());
                cx.notify();
            }
        });
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
        // Atic Code es donde se trabaja: una sesión de Claude se retoma ahí, con su
        // historial; las de otros agentes, en su terminal.
        if session.agent == CLAUDE && dir.is_dir() {
            let title = session.preview.clone().unwrap_or_else(|| folder_name(&session.cwd));
            println!("agentes → reanudar en Atic Code {}", session.id);
            cx.emit(AgentsEvent::CodeResume { session: session.id.clone(), cwd: dir, title });
            return;
        }
        let dir = if dir.is_dir() { dir } else { home().unwrap_or(dir) };
        let line = template.replace("{id}", &session.id);
        let title = format!("{} · {}", agent.name, folder_name(&session.cwd));
        println!("agentes → reanudar {} {}", agent.cli, session.id);
        cx.emit(AgentsEvent::TerminalResume { line, dir, title });
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        // Esc con una respuesta a medias: se cancela y el notch se cierra.
        self.reply = None;
        cx.emit(AgentsEvent::Close);
    }

    fn launch_action(&mut self, _: &Launch, window: &mut Window, cx: &mut Context<Self>) {
        // Enter manda el mensaje del chat con Atic Code.
        if self.chat.read(cx).composer.focus_handle(cx).is_focused(window) {
            self.chat.update(cx, |chat, cx| chat.send(cx));
        }
        // O la respuesta al mensaje del celular.
        if self.reply.is_some() && self.reply_input.focus_handle(cx).is_focused(window) {
            self.send_reply(cx);
        }
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
        let summary = if self.phone_mode {
            let items = self.shown_items();
            let notices = items.iter().filter(|i| matches!(i.kind, tray::Kind::Notice | tray::Kind::Message)).count();
            let call = items.iter().find_map(|i| match i.kind {
                tray::Kind::Ringing => Some(crate::i18n::t("pill.agentsList.ringing")),
                tray::Kind::OnCall => Some(crate::i18n::t("pill.agentsList.onCall")),
                _ => None,
            });
            match (crate::phone::can_send_files(), notices, call) {
                (false, 0, None) => crate::i18n::t("pill.agentsList.disconnected").to_string(),
                (_, 0, None) => String::new(),
                (_, 0, Some(call)) => call.to_string(),
                (_, n, None) => notices_label(n),
                (_, n, Some(call)) => format!("{call} · {}", notices_label(n)),
            }
        } else {
            match (counts.working, counts.waiting()) {
                (0, 0) => String::new(),
                (w, 0) => crate::i18n::tf("pill.agentsList.working", &[("w", &w)]),
                (0, r) => crate::i18n::tf("pill.agentsList.toReview", &[("r", &r)]),
                (w, r) => format!(
                    "{} · {}",
                    crate::i18n::tf("pill.agentsList.working", &[("w", &w)]),
                    crate::i18n::tf("pill.agentsList.toReview", &[("r", &r)])
                ),
            }
        };
        let phone_mode = self.phone_mode;
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
            .child(div().text_size(px(12.)).child(if phone_mode { crate::i18n::t("settings.nav.phone") } else { crate::i18n::t("settings.agents.navAgents") }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(summary),
            )
            // Atic Code es donde se trabaja con los agentes: el botón se nota.
            .when(!phone_mode, |el| el.child(open_code_button("agents-code", text, cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(AgentsEvent::Code)))))
            // Con un celular conectado: hacerlo sonar para encontrarlo.
            .when(phone_mode && crate::phone::can_send_files(), |el| {
                let ringing = self.ringing;
                el.child(
                    div()
                        .id("phone-ring")
                        .h(px(24.))
                        .px(px(10.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(5.))
                        .rounded(px(12.))
                        .text_size(px(11.))
                        .cursor_pointer()
                        .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.toggle_ring(cx)))
                        .fx("phone-ring", move |el, h| {
                            let fg = if ringing { text } else { h.mix(muted, text) };
                            let bg = if ringing { text.opacity(0.14 + 0.04 * h.t) } else { h.mix(text.opacity(0.0), text.opacity(0.08)) };
                            el.text_color(fg)
                                .bg(bg)
                                .child(svg().path("icons/smartphone.svg").size(px(12.)).text_color(fg))
                                .child(if ringing { crate::i18n::t("pill.agentsList.stopRinging") } else { crate::i18n::t("pill.agentsList.findPhone") })
                        }),
                )
            })
            // Con Atic Code respondiendo el panel también queda abierto: el pin
            // se ve encendido mientras tanto (`holds_open`).
            .child(pin_button("agents-pin", self.pinned || self.chat.read(cx).busy(), text, self.colors.faint, cx.listener(
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
            session.activity.clone().unwrap_or_else(|| crate::i18n::t("page.agents.window.status.working").into())
        } else {
            session.preview.clone().unwrap_or_else(|| crate::i18n::t("pill.agentsList.ready").into())
        };
        // Una sesión de Atic Code no se reanuda en otra consola: ya tiene su chat.
        let can_resume = AGENTS[session.agent].resume.is_some() && self.chat_of(&session.id).is_none();
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
                match kind {
                    tray::Kind::Review => {
                        panel.tray_view(id, cx);
                    }
                    tray::Kind::Decision => panel.tray_decide(id, false, cx),
                    tray::Kind::Update => panel.tray_skip_update(id, cx),
                    tray::Kind::Notice | tray::Kind::Message | tray::Kind::Ringing | tray::Kind::OnCall => panel.tray_phone(id, true, cx),
                }
            }))
            .fx(("tray-left-fx", index), |el, h| el.opacity(1.0 - 0.15 * h.t - 0.1 * h.press));
        let right = tray::button(tray::right_label(kind), wr, !tray::left_is_primary(kind), false)
            .id(("tray-right", index))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                match kind {
                    tray::Kind::Review => panel.tray_accept(id, cx),
                    tray::Kind::Decision => panel.tray_decide(id, true, cx),
                    tray::Kind::Update => panel.tray_install(cx),
                    tray::Kind::Message => panel.start_reply(id, cx),
                    tray::Kind::Notice | tray::Kind::Ringing | tray::Kind::OnCall => panel.tray_phone(id, false, cx),
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

}

/// «1 aviso», «3 avisos».
fn notices_label(n: usize) -> String {
    crate::i18n::tf(
        if n == 1 { "pill.agentsList.noticeOne" } else { "pill.agentsList.noticeMany" },
        &[("n", &n)],
    )
}

/// «ahora», «hace 3 min», «hace 2 h».
pub(crate) fn ago(seconds: i64) -> String {
    match seconds {
        s if s < 45 => crate::i18n::t("pill.agentsList.agoNow").into(),
        s if s < 3600 => crate::i18n::tf("pill.agentsList.agoMin", &[("min", &((s + 30) / 60))]),
        s => crate::i18n::tf("pill.agentsList.agoHour", &[("h", &(s / 3600))]),
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
                .child(crate::i18n::t("pill.agentsList.emptyRunning"))
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
        let items = self.shown_items();
        let pending = if items.is_empty() {
            div()
                .h(px(EMPTY_H))
                .flex_none()
                .flex()
                .items_center()
                .px(px(SIDE_PAD + 8.))
                .text_size(px(11.))
                .text_color(self.colors.muted)
                .child(if self.phone_mode { crate::i18n::t("pill.agentsList.emptyPhone") } else { crate::i18n::t("pill.agentsList.emptyReview") })
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
                            .child(SharedString::from(crate::i18n::tf(
                                "pill.agentsList.moreInTray",
                                &[("n", &(items.len() - MAX_INBOX_ROWS))],
                            ))),
                    )
                })
                .into_any_element()
        };
        let footer = self.notice.clone().unwrap_or_else(|| {
            if !self.phone_mode {
                crate::i18n::t("pill.agentsList.footerAgents").into()
            } else if crate::phone::can_send_files() {
                crate::i18n::t("pill.agentsList.footerPhone").into()
            } else {
                crate::i18n::t("pill.agentsList.footerConnect").into()
            }
        });
        let footer = div()
            .h(px(FOOTER_H))
            .flex_none()
            .flex()
            .items_center()
            .px(px(SIDE_PAD + 8.))
            .text_size(px(10.))
            .text_color(self.colors.faint)
            .truncate()
            .child(footer);
        if self.phone_mode {
            return div()
                .key_context(KEY_CONTEXT)
                .track_focus(&self.focus)
                .on_action(cx.listener(Self::dismiss))
                .on_action(cx.listener(Self::launch_action))
                .size_full()
                .flex()
                .flex_col()
                .pb(px(SIDE_PAD))
                .font_family(crate::fonts::UI)
                .text_color(self.colors.text)
                .child(self.render_band(cx))
                .children(self.render_reply(cx))
                .child(pending)
                .child(div().flex_1())
                .child(footer);
        }

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::launch_action))
            .size_full()
            .flex()
            .flex_col()
            .pb(px(SIDE_PAD))
            .font_family(crate::fonts::UI)
            .text_color(self.colors.text)
            .child(self.render_band(cx))
            .children(self.render_reply(cx))
            // Conversando en el notch, el chat ocupa el lugar de las listas;
            // si no, las conversaciones de Atic Code y abajo, para escribir.
            .when(!self.chat_active, |el| {
                el.child(self.header(crate::i18n::t("pill.agentsList.headerReview")))
                    .child(pending)
                    .child(self.header(crate::i18n::t("pill.agentsList.headerRunning")))
                    .child(running)
            })
            .child(self.chat.clone())
            .children(self.render_player(cx))
            .child(div().flex_1())
            .child(footer)
    }
}

/// «Abrir Atic Code»: claro sobre la piel oscura, con su marca, para que se
/// note entre lo demás. Lo usan el panel de Agentes y su vistazo.
pub(crate) fn open_code_button(
    id: &'static str,
    text: Hsla,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> impl IntoElement {
    code_button(id, crate::i18n::t("pill.agents.openCode"), text, on_click)
}

/// Un botón claro con la marca de Atic Code («Abrir Atic Code», «Seguir en
/// Atic Code»).
pub(crate) fn code_button(
    id: &'static str,
    label: &'static str,
    text: Hsla,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> impl IntoElement {
    let ink: Hsla = rgb(0x1a1a18).into();
    div()
        .id(id)
        .h(px(26.))
        .pl(px(6.))
        .pr(px(11.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(13.))
        .text_size(px(11.5))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(ink)
        .cursor_pointer()
        .on_click(on_click)
        .child(crate::kit::chrome::logo(16.))
        .child(label)
        .fx(id, move |el, h| el.bg(text.opacity(0.88 + 0.12 * h.t)))
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

    #[test]
    fn lo_que_escribe_un_agente_se_lee_en_lineas() {
        let prompt = json!({"type": "user", "promptSource": "x", "message": {"content": "arregla el test\nporfa"}});
        assert_eq!(claude_lines(&prompt), vec!["", "› arregla el test", "  porfa"]);
        let answer = json!({"type": "assistant", "message": {"content": [
            {"type": "thinking", "thinking": "hmm"},
            {"type": "text", "text": "Listo.\nCorrí los tests."},
            {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test --locked"}}
        ]}});
        assert_eq!(claude_lines(&answer), vec!["Listo.", "Corrí los tests.", "• Ejecutando cargo test"]);
        // Los resultados de herramientas y lo de los subagentes no se muestran.
        let result = json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "mucho"}]}});
        assert!(claude_lines(&result).is_empty());
        let side = json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "text", "text": "x"}]}});
        assert!(claude_lines(&side).is_empty());
        let codex = json!({"type": "response_item", "payload": {"type": "function_call", "name": "shell", "arguments": "{\"command\":[\"ls\",\"-la\"]}"}});
        assert_eq!(codex_lines(&codex), vec!["• Ejecutando ls -la"]);
        let said = json!({"type": "event_msg", "payload": {"type": "agent_message", "message": "Hecho"}});
        assert_eq!(codex_lines(&said), vec!["Hecho"]);
    }
}
