//! Una conversación con Claude Code: lo que llega del sidecar convertido en
//! filas para dibujar.
//!
//! El texto llega en trozos (`stream`) y después entero (`assistant`): el
//! entero reemplaza lo que llegó en trozos (como `onAssistant` de la referencia, que
//! busca el primer bloque del mismo tipo sin su versión final). Lo que no pasó
//! por el streaming (el historial de una conversación guardada) se agrega del
//! mensaje entero.
//!
//! De cada mensaje de la conversación principal se guarda su `uuid`, en orden
//! (`chain`): el Rewind corta la conversación en uno de ellos.
//!
//! Los subagentes (mensajes con `parent`) no se muestran: su trabajo aparece
//! como la herramienta que los lanzó.

use crate::i18n::{t, tf};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use serde_json::Value;

use super::config::{model_name, same_model, ClaudeConfig};

/// Lo que se guarda del resultado de una herramienta. la referencia no recorta; aquí
/// el resultado es un solo texto en memoria que se dibuja entero al abrirlo
/// (un `Read` de un archivo grande lo hace lento), así que se pone un tope
/// generoso y se avisa cuánto quedó fuera.
const MAX_RESULT: usize = 40_000;
/// Cuántas herramientas hijas recuerda un subagente (`slice(-19)` de la referencia).
const MAX_CHILDREN: usize = 20;
/// El aviso de conversación larga (`LONG_CHAT` de la referencia): la calidad baja
/// mucho antes de llenar la ventana y cada turno reenvía todo el contexto.
const LONG_SOFT: f64 = 0.5;
const LONG_STRONG: f64 = 0.8;
const LONG_TOKENS: u64 = 150_000;
const LONG_COMPACTIONS: usize = 2;

/// Qué tan larga es una conversación, para el aviso sobre la caja.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongLevel {
    Soft,
    Strong,
    Compacted,
}

impl LongLevel {
    pub fn text(self) -> &'static str {
        match self {
            LongLevel::Soft => t("pill.code.chat.longSoft"),
            LongLevel::Strong => t("pill.code.chat.longStrong"),
            LongLevel::Compacted => t("pill.code.chat.longCompacted"),
        }
    }
}

/// Un Artifact recién publicado que se ofrece abrir en el navegador.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactOffer {
    pub title: String,
    pub url: String,
    /// Cuál es (cada oferta entra con su animación).
    pub serial: u64,
}

/// El enlace de un Artifact en el resultado de su herramienta
/// (`https://claude.ai/[code/]artifact/<id>`, el `artifactUrl` de la referencia).
pub fn artifact_url(text: &str) -> Option<String> {
    const BASE: &str = "https://claude.ai/";
    let mut rest = text;
    while let Some(at) = rest.find(BASE) {
        let after = &rest[at + BASE.len()..];
        let path = after.strip_prefix("code/").unwrap_or(after);
        if let Some(id) = path.strip_prefix("artifact/") {
            let len = id.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(id.len());
            if len > 0 {
                let end = text.len() - id.len() + len;
                let start = text.len() - rest.len() + at;
                return Some(text[start..end].to_string());
            }
        }
        rest = after;
    }
    None
}

/// Solo publicar (no leer, listar ni subir recursos) deja un Artifact nuevo.
pub fn artifact_is_publish(input: Option<&Value>) -> bool {
    let action = input.and_then(|i| i.get("action")).and_then(Value::as_str);
    action.is_none_or(|a| a == "publish") && input.and_then(|i| i.get("asset")).is_none_or(Value::is_null)
}

/// El verbo de la tarjeta de un Artifact según su acción (`ArtifactCard` de la referencia).
pub fn artifact_verb(input: Option<&Value>) -> String {
    match input.and_then(|i| i.get("action")).and_then(Value::as_str) {
        None | Some("") | Some("publish") => "Artifact".into(),
        Some("read") => t("pill.code.chat.artifactRead").into(),
        Some("list") => t("pill.code.chat.artifactList").into(),
        Some(other) => format!("Artifact · {other}"),
    }
}

/// El título de un Artifact: el que se le dio o el nombre del archivo publicado.
pub fn artifact_title(input: Option<&Value>) -> String {
    let field = |name: &str| input.and_then(|i| i.get(name)).and_then(Value::as_str).unwrap_or_default().trim().to_string();
    let title = field("title");
    if !title.is_empty() {
        return title;
    }
    let file = field("file_path");
    let base = file.rsplit(['/', '\\']).next().unwrap_or_default();
    let lower = base.to_lowercase();
    let base = [".html", ".htm", ".md"].iter().find_map(|ext| lower.ends_with(ext).then(|| &base[..base.len() - ext.len()])).unwrap_or(base);
    if base.is_empty() { "Artifact".into() } else { base.to_string() }
}

/// Las herramientas que editan archivos: si el visor tiene uno abierto, se recarga.
const EDIT_TOOLS: [&str; 4] = ["Edit", "MultiEdit", "Write", "NotebookEdit"];

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// El JSON del input mientras llega en trozos.
    pub partial: String,
    pub input: Option<Value>,
    /// `None` mientras corre. Vacío si terminó sin resultado (historial) o no
    /// llegó a terminar (con `is_error`).
    pub result: Option<String>,
    pub is_error: bool,
    /// Un subagente (Task/Agent): los nombres de sus últimas herramientas y
    /// cuántas lleva en total.
    pub children: Vec<String>,
    pub child_total: usize,
}

impl ToolCall {
    pub fn new(id: String, name: String, input: Option<Value>) -> Self {
        Self { id, name, partial: String::new(), input, result: None, is_error: false, children: Vec::new(), child_total: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// Un mensaje del usuario; `uuid` llega con su eco (lo necesita el Rewind).
    /// Las imágenes que llevó se ven como miniaturas en su burbuja.
    User { text: String, uuid: Option<String>, images: Vec<Arc<gpui::Image>> },
    Text(String),
    Thinking(String),
    Tool(ToolCall),
    /// El cierre de un turno: cuánto tardó y cuánto costó.
    Turn(String),
    /// La marca de cambio de modelo («Cambiado a Opus 4.5»).
    Model(String),
    Notice { text: String, error: bool },
    /// Claude Code compactó el contexto (`compact_boundary`).
    Compact,
    /// Terminó un subagente o una tarea en segundo plano: Claude Code lo avisa
    /// con un mensaje del usuario (`<task-notification>`) que nadie escribió.
    Task(TaskNote),
}

/// El aviso de que terminó una tarea (`<task-notification>`), ya leído.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskNote {
    /// El del mensaje que lo trajo (para no repetirlo).
    pub uuid: Option<String>,
    pub task_id: String,
    pub tool_use_id: Option<String>,
    /// `completed`, `stopped`, `failed`… o `event` (lo que vio un Monitor).
    pub status: String,
    /// «Agent "Revisar X" finished».
    pub summary: String,
    /// Lo que respondió el subagente (Markdown) o lo que vio el Monitor, si lo trae.
    pub result: Option<String>,
    pub tokens: Option<u64>,
    pub tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
}

impl TaskNote {
    /// «Agente terminó: Revisar X», «Comando falló: npm test», «Monitor avisó: build».
    pub fn heading(&self) -> String {
        let summary = self.summary.trim();
        let kind = summary[..summary.find('"').unwrap_or(summary.len())].to_lowercase();
        let noun = if kind.contains("agent") {
            t("pill.code.chat.nounAgent")
        } else if kind.contains("monitor") {
            "Monitor"
        } else if kind.contains("command") || kind.contains("shell") {
            t("pill.code.chat.nounCommand")
        } else {
            t("pill.code.chat.nounTask")
        };
        let verb = match self.status.as_str() {
            "failed" | "error" => t("pill.code.chat.verbFailed"),
            "stopped" | "killed" | "cancelled" | "canceled" => t("pill.code.chat.verbStopped"),
            "event" => t("pill.code.chat.verbEvent"),
            _ => t("pill.code.chat.verbDone"),
        };
        // El nombre va entre comillas en el resumen; sin comillas, el resumen entero.
        let name = match (summary.find('"'), summary.rfind('"')) {
            (Some(start), Some(end)) if end > start + 1 => &summary[start + 1..end],
            _ => summary,
        };
        if name.is_empty() { format!("{noun} {verb}") } else { format!("{noun} {verb}: {name}") }
    }

    /// «3 min 16 s · 35 herramientas · 171.9k tok» (lo que traiga).
    pub fn meta(&self) -> String {
        [
            super::usage::duration(self.duration_ms),
            self.tool_uses.map(|n| if n == 1 { t("pill.code.chat.toolOne").to_string() } else { tf("pill.code.chat.toolMany", &[("n", &n)]) }).unwrap_or_default(),
            self.tokens.map(|n| format!("{} tok", super::usage::fmt(Some(n)))).unwrap_or_default(),
        ]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

const NOTE_OPEN: &str = "<task-notification>";
const NOTE_CLOSE: &str = "</task-notification>";

/// El contenido de `<name>…</name>` en `text`, sin espacios a los lados.
fn xml_tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{name}>"))? + start;
    Some(text[start..end].trim())
}

/// El texto de un campo sin los escapes de XML (`&gt;`, `&amp;`…).
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&amp;", "&")
}

/// Sin los códigos de color y de enlace de la terminal (`ESC[…m`, `ESC]8;;…ESC\\`).
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                while chars.next().is_some_and(|c| !c.is_ascii_alphabetic()) {}
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Un `<task-notification>` (lo de adentro); sin `<task-id>` ni `<status>` no es uno.
fn task_note(body: &str) -> Option<TaskNote> {
    // El resultado es Markdown libre: va de su primera apertura a su último cierre, y
    // las demás etiquetas se buscan fuera de él.
    let (result, rest) = match (body.find("<result>"), body.rfind("</result>")) {
        (Some(start), Some(end)) if end >= start + "<result>".len() => (
            Some(unescape(body[start + "<result>".len()..end].trim())).filter(|r| !r.is_empty()),
            format!("{}{}", &body[..start], &body[end + "</result>".len()..]),
        ),
        _ => (None, body.to_string()),
    };
    if xml_tag(&rest, "task-id").is_none() && xml_tag(&rest, "status").is_none() {
        return None;
    }
    let field = |name: &str| xml_tag(&rest, name).filter(|v| !v.is_empty()).map(unescape);
    let number = |name: &str| xml_tag(&rest, name).and_then(|v| v.parse::<u64>().ok());
    // Lo que vio un Monitor llega en `<event>`, sin estado: va como texto plano.
    let event = xml_tag(&rest, "event").map(|e| strip_ansi(&unescape(e))).filter(|e| !e.trim().is_empty());
    let status = field("status").unwrap_or_else(|| if event.is_some() { "event".into() } else { "completed".into() });
    let result = result.or_else(|| event.map(|e| format!("```text\n{}\n```", e.trim_end())));
    Some(TaskNote {
        uuid: None,
        task_id: field("task-id").unwrap_or_default(),
        tool_use_id: field("tool-use-id"),
        status,
        summary: field("summary").unwrap_or_default(),
        result: result.map(clip),
        tokens: number("subagent_tokens").or_else(|| number("total_tokens")),
        tool_uses: number("tool_uses"),
        duration_ms: number("duration_ms"),
    })
}

/// Separa de un mensaje del usuario los avisos de tareas que metió Claude Code
/// (con su envoltura `<system-reminder>` y «[SYSTEM NOTIFICATION - NOT USER
/// INPUT]», si la trae): lo que queda escrito por el usuario (vacío si nada) y
/// los avisos. Un aviso va al comienzo de una línea; uno citado a media frase
/// es texto.
pub fn split_task_notes(text: &str) -> (String, Vec<TaskNote>) {
    const REMINDER_OPEN: &str = "<system-reminder>";
    const REMINDER_CLOSE: &str = "</system-reminder>";
    let mut kept = String::new();
    let mut notes = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(NOTE_OPEN) {
        let Some(len) = rest[at..].find(NOTE_CLOSE) else {
            break;
        };
        let close = at + len + NOTE_CLOSE.len();
        let before = rest[..at].trim_end_matches([' ', '\t']);
        let note = task_note(&rest[at + NOTE_OPEN.len()..at + len]).filter(|_| before.is_empty() || before.ends_with('\n'));
        let Some(note) = note else {
            kept.push_str(&rest[..close]);
            rest = &rest[close..];
            continue;
        };
        notes.push(note);
        let (mut start, mut end) = (at, close);
        // Dentro de un recordatorio abierto: se va el recordatorio entero.
        if let Some(open) = rest[..at].rfind(REMINDER_OPEN).filter(|&open| !rest[open..at].contains(REMINDER_CLOSE)) {
            start = open;
            let tail = rest[close..].trim_start();
            if tail.starts_with(REMINDER_CLOSE) {
                end = rest.len() - tail.len() + REMINDER_CLOSE.len();
            }
        }
        kept.push_str(&rest[..start]);
        rest = &rest[end..];
    }
    kept.push_str(rest);
    if !notes.is_empty() {
        kept = kept.replace(REMINDER_CLOSE, "");
    }
    (kept.trim().to_string(), notes)
}

impl Item {
    #[cfg(test)]
    pub fn user(text: impl Into<String>) -> Self {
        Item::User { text: text.into(), uuid: None, images: Vec::new() }
    }

    /// El tipo y el texto con que se marca (`flagId` de la referencia); solo los
    /// mensajes del usuario y las respuestas se pueden marcar.
    pub fn flag_key(&self) -> Option<(&'static str, &str)> {
        match self {
            Item::User { text, .. } => Some(("user", text)),
            Item::Text(text) => Some(("text", text)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Permission {
    pub request_id: String,
    pub tool: String,
    pub input: Value,
    pub title: Option<String>,
    pub description: Option<String>,
    pub decision_reason: Option<String>,
    pub display_name: Option<String>,
    pub blocked_path: Option<String>,
    /// Claude Code no ofrece «Permitir siempre» para esta solicitud.
    pub suppress_always: bool,
    pub suggestions: Option<Value>,
    /// La herramienta a la que pertenece la solicitud.
    pub tool_use_id: Option<String>,
    /// Claude Code sugiere rechazar: Enter no permite.
    pub default_to_no: bool,
}

impl Permission {
    fn from(data: &Value) -> Option<Self> {
        let text = |name: &str| data.get(name).and_then(Value::as_str).map(str::to_string);
        Some(Self {
            request_id: text("requestId")?,
            tool: text("toolName").unwrap_or_default(),
            input: data.get("input").cloned().unwrap_or(Value::Null),
            title: text("title"),
            description: text("description").or_else(|| text("decisionReason")),
            decision_reason: text("decisionReason"),
            display_name: text("displayName"),
            blocked_path: text("blockedPath"),
            suppress_always: data.get("suppressAlwaysAllowRule").and_then(Value::as_bool).unwrap_or(false),
            suggestions: data.get("suggestions").filter(|s| s.as_array().is_some_and(|a| !a.is_empty())).cloned(),
            tool_use_id: text("toolUseID"),
            default_to_no: data.get("defaultToNo").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

/// El modelo y el esfuerzo aplicados al proceso de una conversación (vacío:
/// los de Claude Code).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub model: String,
    pub effort: String,
}

/// Una opción que un agente ACP deja elegir (su modelo, el nivel de razonamiento…).
#[derive(Clone, Debug, PartialEq)]
pub struct AcpOption {
    pub id: String,
    pub name: String,
    /// `model`, `thought_level`, `mode`… (lo dice el agente).
    pub category: String,
    pub current: String,
    /// Cada valor, su nombre y su explicación (si el agente la manda).
    pub choices: Vec<AcpChoice>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcpChoice {
    pub value: String,
    pub name: String,
    pub description: String,
}

/// Las opciones de tipo lista que manda un agente ACP (`configOptions`); las opciones
/// agrupadas se aplanan.
pub fn acp_options(config: &Value) -> Vec<AcpOption> {
    let text = |value: &Value, name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    config
        .as_array()
        .into_iter()
        .flatten()
        .filter(|option| option.get("type").and_then(Value::as_str) == Some("select"))
        .map(|option| {
            let choice = |c: &Value| AcpChoice { value: text(c, "value"), name: text(c, "name"), description: text(c, "description") };
            let mut choices = Vec::new();
            for entry in option.get("options").and_then(Value::as_array).into_iter().flatten() {
                match entry.get("options").and_then(Value::as_array) {
                    Some(group) => choices.extend(group.iter().map(choice)),
                    None => choices.push(choice(entry)),
                }
            }
            AcpOption {
                id: text(option, "id"),
                name: text(option, "name"),
                category: text(option, "category"),
                current: option.get("currentValue").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default(),
                choices,
            }
        })
        .collect()
}

/// Lo que un agente ACP deja elegir: sus opciones (modelo, razonamiento…), sus modos y
/// sus comandos. Lo tiene la conversación (lo que dijo su sesión) y, antes de escribir,
/// el borrador de la próxima (lo leído del agente y lo que eligió el usuario).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AcpConfig {
    pub options: Vec<AcpOption>,
    /// Los modos (id, nombre, explicación). Llegan como `modes` o, en OpenCode y
    /// Cursor, como una opción de categoría `mode` (entonces `mode_config` es su id y
    /// el modo se cambia como opción).
    pub modes: Vec<(String, String, String)>,
    pub mode: Option<String>,
    pub mode_config: Option<String>,
    /// Los comandos con «/» (nombre, descripción).
    pub commands: Vec<(String, String)>,
}

impl AcpConfig {
    /// Lo que manda el sidecar ACP en `acp_config` (`configOptions` y `modes`); los
    /// comandos se conservan.
    pub fn read(&mut self, data: &Value) {
        let mut options = acp_options(data.get("configOptions").unwrap_or(&Value::Null));
        // El modo como opción (OpenCode, Cursor) va con los modos, no en la lista.
        let mode_option = options.iter().position(|o| o.category == "mode").map(|at| options.remove(at));
        self.options = options;
        let text = |value: &Value, name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        match mode_option {
            Some(option) => {
                self.modes = option.choices.iter().map(|c| (c.value.clone(), c.name.clone(), c.description.clone())).collect();
                self.mode = Some(option.current);
                self.mode_config = Some(option.id);
            }
            None => {
                let modes = data.get("modes").unwrap_or(&Value::Null);
                self.modes = modes
                    .get("availableModes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|m| (text(m, "id"), text(m, "name"), text(m, "description")))
                    .collect();
                self.mode = modes.get("currentModeId").and_then(Value::as_str).map(str::to_string);
                self.mode_config = None;
            }
        }
    }

    /// Los comandos de `acp_commands`.
    pub fn read_commands(&mut self, commands: &Value) {
        let text = |value: &Value, name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        self.commands = commands
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| (text(c, "name"), text(c, "description")))
            .filter(|(name, _)| !name.is_empty())
            .collect();
    }

    pub fn is_empty(&self) -> bool {
        self.options.is_empty() && self.modes.is_empty()
    }

    /// Conserva lo que el usuario eligió en `old` si en esta lista sigue existiendo (al
    /// volver a leer las opciones del agente).
    pub fn keep_choices(&mut self, old: &AcpConfig) {
        for previous in &old.options {
            if let Some(option) = self.options.iter_mut().find(|o| o.id == previous.id) {
                if option.choices.iter().any(|c| c.value == previous.current) {
                    option.current = previous.current.clone();
                }
            }
        }
        if let Some(mode) = old.mode.as_ref().filter(|m| self.modes.iter().any(|(id, _, _)| id == *m)) {
            self.mode = Some(mode.clone());
        }
    }

    /// Elige `value` en la opción `config` (el modo, si `config` es el suyo).
    pub fn set(&mut self, config: &str, value: &str) {
        if self.mode_config.as_deref() == Some(config) {
            self.mode = Some(value.to_string());
        }
        if let Some(option) = self.options.iter_mut().find(|o| o.id == config) {
            option.current = value.to_string();
        }
    }

    /// Lo elegido, para aplicarlo al abrir la sesión: `{ config: { id: valor }, mode }`.
    pub fn wanted(&self) -> Value {
        let mut config = serde_json::Map::new();
        for option in &self.options {
            config.insert(option.id.clone(), Value::String(option.current.clone()));
        }
        let mut mode = Value::Null;
        if let Some(current) = &self.mode {
            match &self.mode_config {
                Some(id) => {
                    config.insert(id.clone(), Value::String(current.clone()));
                }
                None => mode = Value::String(current.clone()),
            }
        }
        serde_json::json!({ "config": config, "mode": mode })
    }
}

/// Un mensaje escrito mientras el turno corre: sale tal cual cuando el turno termina,
/// o antes si el usuario elige «Enviar ahora» (interrumpe el turno).
pub struct Queued {
    /// Único en la conversación: el chip y sus botones lo usan aunque la cola se reordene.
    pub id: u64,
    pub text: String,
    pub images: Vec<Value>,
    pub thumbs: Vec<std::sync::Arc<gpui::Image>>,
}

pub struct Chat {
    /// La clave de la sesión en el sidecar.
    pub key: String,
    pub workspace: u64,
    pub cwd: PathBuf,
    pub session_id: Option<String>,
    pub title: String,
    pub items: Vec<Item>,
    /// Hay un turno en curso.
    pub busy: bool,
    /// Mensajes escritos mientras el turno corre: salen solos al terminar, en orden.
    pub queued: Vec<Queued>,
    /// El agente ACP de la conversación (`opencode`, `gemini`…); `None` es Claude.
    pub acp: Option<&'static str>,
    /// Su sesión en el agente, para retomarla (`session/load`).
    pub acp_session: Option<String>,
    /// Lo que el agente ACP deja elegir, según su sesión.
    pub acp_cfg: AcpConfig,
    /// El próximo `Queued::id`.
    queue_seq: u64,
    /// El usuario pidió «Enviar ahora»: el turno se interrumpió para mandar el primero
    /// de la cola, así que ese resultado sí la despacha.
    send_after_stop: bool,
    /// La sesión está abierta en el sidecar.
    pub live: bool,
    pub permissions: Vec<Permission>,
    pub model: Option<String>,
    /// Modelo y esfuerzo elegidos para esta conversación (cada chat tiene los
    /// suyos, como en la referencia). Sin elegir, los del espacio.
    pub want_model: Option<String>,
    pub want_effort: Option<String>,
    /// Lo que tiene de verdad el proceso de esta conversación: `None` mientras
    /// no esté abierto o `start` no haya respondido.
    pub applied: Option<Applied>,
    /// El modelo elegido con el que se envió el último mensaje: si cambia,
    /// aparece la marca.
    pub sent_model: Option<String>,
    /// Contexto usado y máximo, según Claude Code.
    pub context: Option<(u64, u64)>,
    /// Costo y tokens (entrada, salida, caché) de toda la conversación.
    pub total_cost: f64,
    pub tokens: (u64, u64, u64),
    /// Lo que tardó la última respuesta (la línea de estado).
    pub last_duration_ms: Option<u64>,
    /// Cuándo se miró o se le escribió por última vez (para cerrar las
    /// inactivas cuando hay demasiadas abiertas).
    pub used_at: std::time::Instant,
    /// Subagentes y tareas en segundo plano (el mapa de agentes).
    pub tasks: Vec<super::usage::Task>,
    /// Las tareas en segundo plano vivas según Claude Code
    /// (`background_tasks_changed`): mientras haya, su proceso no se cierra.
    pub background: Vec<String>,
    /// El turno en curso lo abrió Claude Code solo, al terminar una tarea en
    /// segundo plano: el aviso lo dice.
    pub auto_turn: bool,
    /// El estado del puente de Remote Control.
    pub remote_state: Option<String>,
    /// Terminó un turno mientras no se miraba (el punto de «sin leer»).
    pub unread: bool,
    /// Remote Control: si está conectada y su enlace.
    pub remote_on: bool,
    pub remote_url: Option<String>,
    /// Se pidió abrir la sesión y todavía no llegó `meta` («Conectando…»).
    pub starting: bool,
    /// Se pidió detener: el resultado se muestra como «Detenido.».
    pub interrupted: bool,
    /// Una rama del Rewind: la sesión de la que sale y el mensaje donde corta.
    /// Vale hasta que Claude Code le da su propia sesión.
    pub fork: Option<(String, String)>,
    /// Archivos que Claude editó desde la última vez que se miró (para el visor).
    pub edited: Vec<PathBuf>,
    /// Desde cuándo la ve Atic Code (el chip de duración, como en la referencia).
    pub seen_at: std::time::Instant,
    /// El último Artifact publicado aquí y si se sigue ofreciendo (se
    /// conserva mientras su aviso sale).
    pub artifact: Option<(ArtifactOffer, bool)>,
    /// Los niveles del aviso de conversación larga que se cerraron.
    pub long_dismissed: Vec<LongLevel>,
    /// (mensaje, índice del bloque) → fila.
    blocks: HashMap<(String, u64), usize>,
    /// Los bloques de texto y razonamiento de cada mensaje, en orden: (es
    /// razonamiento, fila, ya tiene su versión final). Claude Code manda cada
    /// bloque como un mensaje aparte con el mismo id.
    msg_blocks: HashMap<String, Vec<(bool, usize, bool)>>,
    /// Mensajes de subagentes: mensaje → la herramienta (Task) que los lanzó.
    nested: HashMap<String, String>,
    /// Las herramientas hijas ya contadas (llegan por el stream y otra vez completas).
    child_seen: HashSet<String>,
    /// Bloques de texto o razonamiento que todavía llegan en trozos.
    streaming: HashSet<usize>,
    /// id de la herramienta → fila.
    tools: HashMap<String, usize>,
    /// Los uuid de la conversación principal, en orden.
    chain: Vec<String>,
    /// Filas de mensajes escritos aquí que esperan su eco (y su uuid).
    pending_echo: Vec<usize>,
    /// Avisos de «responde otro modelo» ya dados (respondido, elegido).
    model_warned: HashSet<(String, String)>,
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| (b.get("type").and_then(Value::as_str) == Some("text")).then(|| b.get("text")?.as_str()))
            .flatten()
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Una imagen de un mensaje para su miniatura, si se puede leer.
pub fn image_from(media_type: &str, bytes: Vec<u8>) -> Option<Arc<gpui::Image>> {
    let format = match media_type {
        "image/png" => gpui::ImageFormat::Png,
        "image/jpeg" | "image/jpg" => gpui::ImageFormat::Jpeg,
        "image/gif" => gpui::ImageFormat::Gif,
        "image/webp" => gpui::ImageFormat::Webp,
        _ => return None,
    };
    let size = image_dimensions(&bytes);
    let image = thumbnail_of(format, bytes, size);
    if let Some((width, height)) = size {
        if let Ok(mut sizes) = image_sizes().lock() {
            sizes.insert(image.id(), (width as f32, height as f32));
        }
    }
    Some(image)
}

/// El lado mayor de las miniaturas: dos veces los 96 px en que se ven, por las
/// pantallas Retina.
const THUMB_PX: u32 = 192;

/// La miniatura de una imagen, de a lo más `THUMB_PX`. gpui decodifica y sube a la
/// GPU lo que recibe: con la captura entera, cada una dejaba ~28 MB retenidos (3360×2100
/// en RGBA) y el chat llegaba a varios GB. Los bytes originales van aparte, en base64,
/// a Claude. Se recuerda por el hash del original para no decodificarla de nuevo al
/// recargar el historial; gif y webp (sin decodificador aquí) quedan como vienen.
fn thumbnail_of(format: gpui::ImageFormat, bytes: Vec<u8>, size: Option<(u32, u32)>) -> Arc<gpui::Image> {
    use std::hash::{Hash, Hasher};
    static THUMBS: std::sync::OnceLock<std::sync::Mutex<HashMap<u64, Arc<gpui::Image>>>> = std::sync::OnceLock::new();
    let small = size.is_some_and(|(width, height)| width.max(height) <= THUMB_PX);
    if small || !matches!(format, gpui::ImageFormat::Png | gpui::ImageFormat::Jpeg) {
        return Arc::new(gpui::Image::from_bytes(format, bytes));
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let key = hasher.finish();
    let thumbs = THUMBS.get_or_init(Default::default);
    if let Some(thumb) = thumbs.lock().ok().and_then(|t| t.get(&key).cloned()) {
        return thumb;
    }
    let mut png = Vec::new();
    let Ok(()) = image::load_from_memory(&bytes)
        .map(|full| full.thumbnail(THUMB_PX, THUMB_PX))
        .and_then(|thumb| thumb.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png))
    else {
        return Arc::new(gpui::Image::from_bytes(format, bytes));
    };
    let thumb = Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png));
    if let Ok(mut thumbs) = thumbs.lock() {
        // Cada una pesa decenas de KB; el tope solo evita que crezca sin fin.
        if thumbs.len() >= 512 {
            thumbs.clear();
        }
        thumbs.insert(key, thumb.clone());
    }
    thumb
}

/// Por el id de la imagen, que es el hash de sus bytes: la misma imagen no suma otra
/// entrada y cada una ocupa 16 bytes, así que el mapa no se limpia.
type ImageSizes = std::collections::HashMap<u64, (f32, f32)>;

/// El tamaño de cada imagen creada aquí, por el id de su `gpui::Image`: gpui no lo
/// guarda y la miniatura lo necesita para conservar la proporción.
fn image_sizes() -> &'static std::sync::Mutex<ImageSizes> {
    static SIZES: std::sync::OnceLock<std::sync::Mutex<ImageSizes>> = std::sync::OnceLock::new();
    SIZES.get_or_init(Default::default)
}

/// El ancho y el alto de una imagen creada aquí (para la miniatura), si se conocen.
pub fn image_size(image: &Arc<gpui::Image>) -> Option<(f32, f32)> {
    image_sizes().lock().ok()?.get(&image.id()).copied()
}

/// El ancho y el alto de una imagen, mirando solo su cabecera.
fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// Las imágenes (en base64) de un mensaje del historial o de otro cliente.
fn images_of(content: &Value) -> Vec<Arc<gpui::Image>> {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("image"))
        .filter_map(|b| {
            let source = b.get("source")?;
            let media = source.get("media_type").and_then(Value::as_str)?;
            let data = source.get("data").and_then(Value::as_str)?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
            image_from(media, bytes)
        })
        .collect()
}

/// Deja en caché las miniaturas de las imágenes de un historial, para que
/// `load_history` las encuentre hechas. Pensado para correr en otro hilo.
pub fn warm_thumbnails(messages: &[Value]) {
    for message in messages {
        if let Some(content) = message.get("message").and_then(|m| m.get("content")) {
            images_of(content);
        }
    }
}

/// Las marcas «Cambiado a X» de un historial: uuid del mensaje del usuario →
/// modelo con el que respondió el turno, cuando difiere del turno anterior
/// (`loadHistory` de la referencia, `agent.ts:697`). Los subagentes no cuentan.
fn history_model_marks(messages: &[Value]) -> HashMap<String, String> {
    let mut marks = HashMap::new();
    let mut last: Option<String> = None;
    let mut pending: Option<String> = None;
    for message in messages {
        if message.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
            continue;
        }
        let inner = message.get("message").unwrap_or(&Value::Null);
        match message.get("type").and_then(Value::as_str) {
            Some("user") => {
                let text = text_of(inner.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() && !text.starts_with('<') && !split_task_notes(&text).0.is_empty() {
                    pending = message.get("uuid").and_then(Value::as_str).map(str::to_string);
                }
            }
            Some("assistant") => {
                let Some(model) = inner.get("model").and_then(Value::as_str).filter(|m| !m.is_empty() && !m.starts_with('<')) else {
                    continue;
                };
                if let (Some(prev), Some(user)) = (&last, pending.take()) {
                    if prev != model {
                        marks.insert(user, model.to_string());
                    }
                }
                pending = None;
                last = Some(model.to_string());
            }
            _ => {}
        }
    }
    marks
}

/// Lo que Claude Code mete como mensaje del usuario sin que lo haya escrito.
fn is_meta(text: &str) -> bool {
    let text = text.trim_start();
    text.is_empty()
        || text.starts_with("<command-")
        || text.starts_with("<local-command")
        || text.starts_with("<system-reminder>")
        || text.starts_with("Caveat:")
}

fn clip(text: String) -> String {
    if text.len() <= MAX_RESULT {
        return text;
    }
    let mut end = MAX_RESULT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    tf("pill.code.chat.clipped", &[("head", &&text[..end]), ("n", &text[end..].chars().count())])
}

/// Si `title` es el de una conversación recién creada (en cualquier idioma).
pub fn is_default_title(title: &str) -> bool {
    title == "Nueva conversación" || title == "New conversation"
}

fn title_for(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or(t("pill.code.chat.fallbackTitle")).trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

impl Chat {
    pub fn new(key: String, workspace: u64, cwd: PathBuf) -> Self {
        Self {
            key,
            workspace,
            cwd,
            session_id: None,
            title: t("pill.code.chat.newTitle").into(),
            items: Vec::new(),
            busy: false,
            queued: Vec::new(),
            acp: None,
            acp_session: None,
            acp_cfg: AcpConfig::default(),
            queue_seq: 0,
            send_after_stop: false,
            live: false,
            permissions: Vec::new(),
            model: None,
            want_model: None,
            want_effort: None,
            applied: None,
            sent_model: None,
            context: None,
            total_cost: 0.,
            tokens: (0, 0, 0),
            last_duration_ms: None,
            used_at: std::time::Instant::now(),
            tasks: Vec::new(),
            background: Vec::new(),
            auto_turn: false,
            remote_state: None,
            unread: false,
            remote_on: false,
            remote_url: None,
            starting: false,
            interrupted: false,
            fork: None,
            edited: Vec::new(),
            seen_at: std::time::Instant::now(),
            artifact: None,
            long_dismissed: Vec::new(),
            blocks: HashMap::new(),
            msg_blocks: HashMap::new(),
            nested: HashMap::new(),
            child_seen: HashSet::new(),
            streaming: HashSet::new(),
            tools: HashMap::new(),
            chain: Vec::new(),
            pending_echo: Vec::new(),
            model_warned: HashSet::new(),
        }
    }

    /// Donde se guardan sus marcas: la sesión o, antes del primer mensaje, la clave.
    pub fn flag_session(&self) -> &str {
        self.session_id.as_deref().unwrap_or(&self.key)
    }

    /// Quién responde en esta conversación: Claude o el agente ACP.
    pub fn agent_name(&self) -> &'static str {
        self.acp.and_then(super::acp::agent).map_or("Claude", |agent| agent.name)
    }

    /// Encola un mensaje para mandarlo apenas termine el turno en curso.
    pub fn enqueue(&mut self, text: String, images: Vec<Value>, thumbs: Vec<Arc<gpui::Image>>) -> u64 {
        self.queue_seq += 1;
        let id = self.queue_seq;
        self.queued.push(Queued { id, text, images, thumbs });
        id
    }

    /// Lo que sale de la cola al llegar un `result`. Si el usuario detuvo el turno, la
    /// cola espera (cada chip ofrece «Enviar»), salvo que lo haya detenido justamente
    /// con «Enviar ahora».
    pub fn next_queued(&mut self, interrupted: bool) -> Option<Queued> {
        if interrupted && !std::mem::take(&mut self.send_after_stop) {
            return None;
        }
        self.send_after_stop = false;
        (!self.busy && !self.queued.is_empty()).then(|| self.queued.remove(0))
    }

    /// Saca de la cola el mensaje `id`.
    pub fn remove_queued(&mut self, id: u64) -> Option<Queued> {
        let index = self.queued.iter().position(|m| m.id == id)?;
        Some(self.queued.remove(index))
    }

    /// «Enviar ahora» con el turno en curso: el mensaje `id` pasa al frente y el
    /// resultado de la interrupción lo despacha. `false` si ya no está en la cola.
    pub fn send_first_after_stop(&mut self, id: u64) -> bool {
        let Some(message) = self.remove_queued(id) else {
            return false;
        };
        self.queued.insert(0, message);
        self.send_after_stop = true;
        true
    }

    /// Suelta la cola cuando la sesión ya no puede mandarla (se cerró o no abrió), con
    /// un aviso para que no queden chips pegados que nunca salen.
    pub fn discard_queued(&mut self) {
        let pending = self.queued.len();
        if pending > 0 {
            self.queued.clear();
            self.send_after_stop = false;
            self.notice(tf("pill.code.chat.queuedLost", &[("pending", &pending)]), true);
        }
    }

    /// El modelo de esta conversación: el elegido en ella o, si no, el del espacio.
    pub fn chosen_model(&self, config: &ClaudeConfig) -> String {
        self.want_model.clone().unwrap_or_else(|| config.model.clone())
    }

    /// El esfuerzo de esta conversación: el elegido en ella o, si no, el del espacio.
    pub fn chosen_effort(&self, config: &ClaudeConfig) -> String {
        self.want_effort.clone().unwrap_or_else(|| config.effort.clone())
    }

    /// Al enviar con otro modelo que el del último mensaje, deja la marca
    /// «Cambiado a X» y recuerda el nuevo (`noteModelForSend` de la referencia).
    pub fn note_model_for_send(&mut self, list: &[(String, String)], model: &str) {
        if !self.items.is_empty() && self.sent_model.as_deref().is_some_and(|sent| !same_model(list, sent, model)) {
            self.items.push(Item::Model(model_name(list, model)));
        }
        self.sent_model = Some(model.to_string());
    }

    /// Se eligió otro modelo con la conversación ya empezada: el nombre que
    /// usará el próximo mensaje (`pendingModel` de la referencia).
    pub fn pending_model(&self, list: &[(String, String)], config: &ClaudeConfig) -> Option<String> {
        let sent = self.sent_model.as_deref()?;
        let chosen = self.chosen_model(config);
        (!self.items.is_empty() && !self.busy && !same_model(list, &chosen, sent)).then(|| model_name(list, &chosen))
    }

    /// Una respuesta de la conversación principal con otro modelo que el
    /// aplicado: avisa una vez y devuelve el modelo que hay que volver a
    /// aplicar (`checkModel` de la referencia).
    pub fn check_model(&mut self, list: &[(String, String)], model: &str) -> Option<String> {
        let want = self.applied.as_ref().map(|a| a.model.clone()).filter(|m| !m.is_empty() && m != "default")?;
        if model.is_empty() || model.starts_with('<') || same_model(list, model, &want) {
            return None;
        }
        if !self.model_warned.insert((model.to_string(), want.clone())) {
            return None;
        }
        let (got, chosen) = (model_name(list, model), model_name(list, &want));
        self.notice(tf("pill.code.chat.wrongModel", &[("got", &got), ("chosen", &chosen)]), true);
        Some(want)
    }

    /// Un mensaje escrito aquí. Con un turno en curso queda en la cola del
    /// sidecar y se responde después.
    pub fn push_user(&mut self, text: &str, images: Vec<Arc<gpui::Image>>) {
        if self.items.iter().all(|i| !matches!(i, Item::User { .. })) {
            self.title = title_for(text);
        }
        self.pending_echo.push(self.items.len());
        self.items.push(Item::User { text: text.to_string(), uuid: None, images });
        self.busy = true;
        self.auto_turn = false;
    }

    /// El mensaje anterior a `uuid` en la conversación (`uuidBefore` de la referencia):
    /// donde corta el Rewind de la conversación.
    pub fn uuid_before(&self, uuid: &str) -> Option<String> {
        let at = self.chain.iter().position(|u| u == uuid)?;
        at.checked_sub(1).map(|before| self.chain[before].clone())
    }

    /// Los mensajes del usuario a los que se puede volver, del más nuevo al más viejo.
    pub fn user_turns(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .rev()
            .filter_map(|i| match i {
                Item::User { text, uuid: Some(uuid), .. } => Some((uuid.clone(), text.clone())),
                _ => None,
            })
            .collect()
    }

    /// Deja la conversación vacía para volver a leerla de su archivo
    /// (`reloadParts` de la referencia); conserva la sesión, el título y lo elegido.
    pub fn reset_items(&mut self) {
        self.items.clear();
        self.permissions.clear();
        self.tasks.clear();
        self.background.clear();
        self.blocks.clear();
        self.msg_blocks.clear();
        self.nested.clear();
        self.child_seen.clear();
        self.streaming.clear();
        self.tools.clear();
        self.chain.clear();
        self.pending_echo.clear();
    }

    /// El bloque `row` sigue llegando en trozos (el «Razonando…» en vivo).
    pub fn is_streaming(&self, row: usize) -> bool {
        self.streaming.contains(&row)
    }

    /// Se muestra «Trabajando…»: hay un turno en curso, nada espera una
    /// respuesta del usuario y no está llegando texto o razonamiento (que ya
    /// muestra que trabaja). Como `working` de `Thread.tsx` de la referencia.
    pub fn working(&self) -> bool {
        if !self.busy || !self.permissions.is_empty() {
            return false;
        }
        !matches!(self.items.last(), Some(Item::Text(_) | Item::Thinking(_)) if self.is_streaming(self.items.len() - 1))
    }

    /// El razonamiento sin texto (cifrado, o que no llegó a escribir nada) no
    /// se dibuja salvo que esté llegando.
    pub fn hidden(&self, row: usize) -> bool {
        matches!(self.items.get(row), Some(Item::Thinking(text)) if text.trim().is_empty() && !self.is_streaming(row))
    }

    /// Una herramienta hija de un subagente: se cuenta bajo su Task en vez de
    /// llenar la conversación (`noteChild` de la referencia).
    fn note_child(&mut self, parent: &str, id: &str, name: &str) {
        let Some(&row) = self.tools.get(parent) else {
            return;
        };
        if !id.is_empty() && !self.child_seen.insert(id.to_string()) {
            return;
        }
        if let Item::Tool(tool) = &mut self.items[row] {
            tool.child_total += 1;
            tool.children.push(if name.is_empty() { "herramienta".to_string() } else { name.to_string() });
            let extra = tool.children.len().saturating_sub(MAX_CHILDREN);
            tool.children.drain(..extra);
        }
    }

    /// El aviso de conversación larga que toca, si no se cerró (`LongChatNotice`
    /// de la referencia): se cierra por nivel, así que vuelve si la conversación crece.
    pub fn long_level(&self) -> Option<LongLevel> {
        let level = if self.items.iter().filter(|i| matches!(i, Item::Compact)).count() >= LONG_COMPACTIONS {
            LongLevel::Compacted
        } else {
            let (used, max) = self.context.filter(|(_, max)| *max > 0)?;
            let share = used as f64 / max as f64;
            if share >= LONG_STRONG {
                LongLevel::Strong
            } else if share >= LONG_SOFT || used >= LONG_TOKENS {
                LongLevel::Soft
            } else {
                return None;
            }
        };
        (!self.long_dismissed.contains(&level)).then_some(level)
    }

    pub fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.items.push(Item::Notice { text: text.into(), error });
    }

    /// Un evento del sidecar para esta conversación.
    pub fn apply(&mut self, event: &str, data: &Value) {
        match event {
            "stream" => {
                for op in data.as_array().into_iter().flatten() {
                    self.stream(op);
                }
            }
            "assistant" => self.assistant(data),
            "user" => self.user(data, false),
            "result" => self.result(data),
            "permission" => {
                if let Some(permission) = Permission::from(data) {
                    self.permissions.push(permission);
                }
            }
            "permission_cancel" => {
                let id = data.get("requestId").and_then(Value::as_str);
                self.permissions.retain(|p| Some(p.request_id.as_str()) != id);
            }
            "session" => {
                self.session_id = data.get("sessionId").and_then(Value::as_str).map(str::to_string);
                if self.session_id.is_some() {
                    self.fork = None;
                }
            }
            "init" => {
                self.model = data.get("model").and_then(Value::as_str).map(str::to_string);
            }
            "meta" => self.starting = false,
            "acp_config" => self.acp_cfg.read(data),
            "acp_commands" => self.acp_cfg.read_commands(data.get("commands").unwrap_or(&Value::Null)),
            "remote" => {
                self.remote_on = data.get("on").and_then(Value::as_bool).unwrap_or(false);
                self.remote_url = if self.remote_on { data.get("url").and_then(Value::as_str).map(str::to_string) } else { None };
            }
            "system" => {
                if data.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                    self.items.push(Item::Compact);
                }
            }
            // Claude Code abrió un turno solo, tras una tarea en segundo plano.
            "turn" => {
                self.busy = true;
                self.auto_turn = true;
            }
            "closed" => {
                self.streaming.clear();
                self.background.clear();
                self.live = false;
                self.starting = false;
                self.applied = None;
                self.busy = false;
                self.permissions.clear();
                if let Some(error) = data.get("error").and_then(Value::as_str) {
                    self.notice(tf("pill.code.chat.sessionClosed", &[("error", &error)]), true);
                }
                self.discard_queued();
            }
            "error" => {
                let message = data.get("message").and_then(Value::as_str).unwrap_or(t("pill.code.chat.agentError"));
                self.notice(message, true);
            }
            _ => {}
        }
    }

    /// El historial de una conversación guardada (`getSessionMessages`); con
    /// `up_to`, hasta ese mensaje inclusive (la rama de un Rewind).
    pub fn load_history(&mut self, list: &[(String, String)], messages: &[Value], up_to: Option<&str>) {
        let marks = history_model_marks(messages);
        for message in messages {
            let parent = message.get("parent_tool_use_id").cloned().unwrap_or(Value::Null);
            let uuid = message.get("uuid").cloned().unwrap_or(Value::Null);
            let inner = message.get("message").cloned().unwrap_or(Value::Null);
            let content = inner.get("content").cloned().unwrap_or(Value::Null);
            match message.get("type").and_then(Value::as_str) {
                Some("assistant") => {
                    let id = inner.get("id").cloned().filter(|id| !id.is_null()).unwrap_or_else(|| uuid.clone());
                    let data = serde_json::json!({ "id": id, "uuid": uuid, "content": content, "parent": parent });
                    self.assistant(&data);
                }
                Some("user") => {
                    // La marca va antes del mensaje cuyo turno usó otro modelo (agent.ts:728).
                    if let Some(model) = uuid.as_str().and_then(|u| marks.get(u)) {
                        self.items.push(Item::Model(model_name(list, model)));
                    }
                    let data = serde_json::json!({ "uuid": uuid, "content": content, "parent": parent });
                    self.user(&data, true);
                }
                _ => {}
            }
            if up_to.is_some() && uuid.as_str() == up_to {
                break;
            }
        }
        // Lo que quedó sin resultado en el archivo ya terminó (agent.ts:743).
        for item in &mut self.items {
            if let Item::Tool(tool) = item {
                if tool.result.is_none() {
                    tool.result = Some(String::new());
                }
            }
        }
        if let Some(Item::User { text, .. }) = self.items.iter().find(|i| matches!(i, Item::User { .. })) {
            self.title = title_for(text);
        }
    }

    fn stream(&mut self, op: &Value) {
        let msg = op.get("msg").and_then(Value::as_str).unwrap_or_default().to_string();
        let index = op.get("index").and_then(Value::as_u64).unwrap_or(0);
        let own_parent = op.get("parent").and_then(Value::as_str).map(str::to_string);
        let nested = op.get("parent").is_some_and(|p| !p.is_null());
        match op.get("op").and_then(Value::as_str) {
            Some("message") => {
                if nested {
                    self.nested.insert(msg, own_parent.unwrap_or_default());
                } else {
                    // Responde (también lo que quedó en la cola tras otro turno).
                    self.busy = true;
                }
            }
            Some("start") => {
                // Lo de un subagente no se muestra: solo se cuentan sus herramientas.
                if let Some(parent) = own_parent.or_else(|| self.nested.get(&msg).cloned()) {
                    if op.get("kind").and_then(Value::as_str) == Some("tool_use") {
                        let id = op.get("id").and_then(Value::as_str).unwrap_or_default();
                        let name = op.get("name").and_then(Value::as_str).unwrap_or_default();
                        self.note_child(&parent, id, name);
                    }
                    return;
                }
                if nested {
                    return;
                }
                let item = match op.get("kind").and_then(Value::as_str) {
                    Some("text") => Item::Text(String::new()),
                    Some("thinking") => Item::Thinking(String::new()),
                    Some("tool_use") => {
                        let id = op.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                        if self.tools.contains_key(&id) {
                            return;
                        }
                        self.tools.insert(id.clone(), self.items.len());
                        Item::Tool(ToolCall::new(id, op.get("name").and_then(Value::as_str).unwrap_or_default().to_string(), None))
                    }
                    _ => return,
                };
                let row = self.items.len();
                match &item {
                    Item::Text(_) => {
                        self.msg_blocks.entry(msg.clone()).or_default().push((false, row, false));
                        self.streaming.insert(row);
                    }
                    Item::Thinking(_) => {
                        self.msg_blocks.entry(msg.clone()).or_default().push((true, row, false));
                        self.streaming.insert(row);
                    }
                    _ => {}
                }
                self.blocks.insert((msg, index), row);
                self.items.push(item);
            }
            Some("delta") => {
                let text = op.get("text").and_then(Value::as_str).unwrap_or_default();
                if let Some(&row) = self.blocks.get(&(msg, index)) {
                    match &mut self.items[row] {
                        Item::Text(t) | Item::Thinking(t) => t.push_str(text),
                        Item::Tool(tool) => tool.partial.push_str(text),
                        _ => {}
                    }
                }
            }
            Some("stop") => {
                if let Some(&row) = self.blocks.get(&(msg, index)) {
                    self.streaming.remove(&row);
                    if let Item::Tool(tool) = &mut self.items[row] {
                        if tool.input.is_none() {
                            tool.input = serde_json::from_str(&tool.partial).ok();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn assistant(&mut self, data: &Value) {
        if let Some(parent) = data.get("parent").and_then(Value::as_str) {
            for block in data.get("content").and_then(Value::as_array).into_iter().flatten() {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    let name = block.get("name").and_then(Value::as_str).unwrap_or_default();
                    self.note_child(parent, id, name);
                }
            }
            return;
        }
        if let Some(uuid) = data.get("uuid").and_then(Value::as_str) {
            self.chain.push(uuid.to_string());
        }
        let id = data.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        for block in data.get("content").and_then(Value::as_array).into_iter().flatten() {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let tool_id = block.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let input = block.get("input").cloned();
                    let name = block.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                    if let Some(&row) = self.tools.get(&tool_id) {
                        if let Item::Tool(tool) = &mut self.items[row] {
                            tool.input = input;
                            if !name.is_empty() {
                                tool.name = name;
                            }
                        }
                    } else {
                        self.tools.insert(tool_id.clone(), self.items.len());
                        self.items.push(Item::Tool(ToolCall::new(tool_id, name, input)));
                    }
                }
                Some(kind @ ("text" | "thinking")) => {
                    let thinking = kind == "thinking";
                    let text = block.get(if thinking { "thinking" } else { "text" }).and_then(Value::as_str).unwrap_or_default();
                    let list = self.msg_blocks.entry(id.clone()).or_default();
                    // El primer bloque del mismo tipo que aún no tiene su versión final es este.
                    if let Some(slot) = list.iter_mut().find(|(t, _, done)| *t == thinking && !done) {
                        slot.2 = true;
                        let row = slot.1;
                        self.streaming.remove(&row);
                        if !text.is_empty() {
                            if let Item::Text(t) | Item::Thinking(t) = &mut self.items[row] {
                                *t = text.to_string();
                            }
                        }
                        continue;
                    }
                    if text.trim().is_empty() {
                        continue;
                    }
                    list.push((thinking, self.items.len(), true));
                    self.items.push(if thinking { Item::Thinking(text.to_string()) } else { Item::Text(text.to_string()) });
                }
                _ => {}
            }
        }
    }

    fn user(&mut self, data: &Value, history: bool) {
        if data.get("parent").is_some_and(|p| !p.is_null()) {
            return;
        }
        let uuid = data.get("uuid").and_then(Value::as_str).map(str::to_string);
        if let Some(uuid) = &uuid {
            self.chain.push(uuid.clone());
        }
        let content = data.get("content").cloned().unwrap_or(Value::Null);
        let mut results = false;
        for block in content.as_array().into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            results = true;
            let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
            if let Some(&row) = self.tools.get(id) {
                if let Item::Tool(tool) = &mut self.items[row] {
                    let result = block.get("content").map(text_of).unwrap_or_default();
                    tool.result = Some(clip(result));
                    tool.is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    // Un Artifact recién publicado: se ofrece abrirlo (como la extensión de VS Code).
                    if !history && !tool.is_error && tool.name == "Artifact" && artifact_is_publish(tool.input.as_ref()) {
                        if let Some(url) = tool.result.as_deref().and_then(artifact_url) {
                            let serial = self.artifact.as_ref().map_or(0, |(o, _)| o.serial + 1);
                            self.artifact = Some((ArtifactOffer { title: artifact_title(tool.input.as_ref()), url, serial }, true));
                        }
                    }
                    // Claude editó un archivo: si está abierto en el visor, se recarga.
                    if !history && !tool.is_error && EDIT_TOOLS.contains(&tool.name.as_str()) {
                        let input = tool.input.as_ref();
                        let path = input.and_then(|i| i.get("file_path").or_else(|| i.get("notebook_path"))).and_then(Value::as_str);
                        if let Some(path) = path {
                            self.edited.push(PathBuf::from(path));
                        }
                    }
                }
            }
        }
        if results {
            return;
        }
        let text = text_of(&content);
        // Terminó una tarea: una fila propia, no una burbuja con el XML (ni cuenta como
        // mensaje del usuario para el título, el eco o el Rewind).
        let (text, notes) = if text.contains(NOTE_OPEN) { split_task_notes(&text) } else { (text, Vec::new()) };
        let seen = uuid.is_some() && self.items.iter().any(|i| matches!(i, Item::Task(n) if n.uuid == uuid));
        if !seen {
            for mut note in notes {
                note.uuid = uuid.clone();
                self.items.push(Item::Task(note));
            }
        }
        let synthetic = data.get("isSynthetic").and_then(Value::as_bool).unwrap_or(false);
        if synthetic || is_meta(&text) {
            return;
        }
        if history {
            self.items.push(Item::User { text, uuid, images: images_of(&content) });
            return;
        }
        // El eco de lo que se escribió aquí: se le pone su uuid (lo necesita el
        // Rewind). Los que se saltó (no tendrán eco) dejan de esperar.
        let replay = data.get("isReplay").and_then(Value::as_bool).unwrap_or(false);
        let matching = self.pending_echo.iter().position(|&row| matches!(&self.items[row], Item::User { text: t, .. } if *t == text));
        let pending = match matching {
            Some(at) => Some(at),
            None if replay && !self.pending_echo.is_empty() => Some(0),
            None => None,
        };
        if let Some(at) = pending {
            let row = self.pending_echo[at];
            self.pending_echo.drain(..=at);
            if let Item::User { uuid: slot, .. } = &mut self.items[row] {
                *slot = uuid;
            }
            return;
        }
        if replay {
            return;
        }
        // Un mensaje que no se escribió aquí (Remote Control, el móvil): se
        // muestra y la conversación pasa a «respondiendo».
        if uuid.is_some() && self.items.iter().any(|i| matches!(i, Item::User { uuid: u, .. } if *u == uuid)) {
            return;
        }
        self.items.push(Item::User { text, uuid, images: images_of(&content) });
        self.busy = true;
    }

    fn result(&mut self, data: &Value) {
        self.busy = false;
        self.streaming.clear();
        self.permissions.clear();
        let stopped = std::mem::take(&mut self.interrupted);
        let is_error = data.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let subtype = data.get("subtype").and_then(Value::as_str).unwrap_or("success");
        let errors: Vec<String> = data
            .get("errors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect();
        // Lo que seguía corriendo cuando el turno falló o se detuvo no terminó.
        if subtype != "success" {
            for item in &mut self.items {
                if let Item::Tool(tool) = item {
                    if tool.result.is_none() {
                        tool.result = Some(String::new());
                        tool.is_error = true;
                    }
                }
            }
        }
        if stopped || (subtype == "error_during_execution" && errors.is_empty()) {
            self.notice(t("pill.code.chat.stopped"), false);
        } else if is_error {
            let message = if errors.is_empty() {
                data.get("result").and_then(Value::as_str).filter(|r| !r.is_empty()).unwrap_or(t("pill.code.chat.errorOccurred")).to_string()
            } else {
                errors.join("\n")
            };
            self.notice(message, true);
        }
        self.items.push(Item::Turn(turn_summary(data)));
    }
}

/// La fila del turno: cuánto tardó y a qué velocidad escribió Claude. Con `speed` (lo mide
/// el sidecar entre el primer y el último token de cada mensaje), la velocidad real de
/// escritura y la espera hasta el primer token; sin él, tokens de salida por segundo de
/// API (o de la duración entera). Sin el costo: con una suscripción no se paga por turno
/// y el número confundía.
fn turn_summary(data: &Value) -> String {
    let seconds = data.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0) / 1000.0;
    let mut summary = format!("{seconds:.1} s");
    let speed = data.get("speed").filter(|speed| !speed.is_null());
    let tokens = speed.and_then(|speed| speed.get("tokens")).and_then(Value::as_f64).unwrap_or(0.0);
    let gen_ms = speed.and_then(|speed| speed.get("genMs")).and_then(Value::as_f64).unwrap_or(0.0);
    if tokens > 0.0 && gen_ms > 0.0 {
        summary.push_str(&format!(" · {:.0} tok/s", tokens / (gen_ms / 1000.0)));
        if let Some(ttft) = speed.and_then(|speed| speed.get("ttftMs")).and_then(Value::as_f64) {
            summary.push_str(&tf("pill.code.chat.toFirstToken", &[("s", &format!("{:.1}", ttft / 1000.0))]));
        }
        return summary;
    }
    let output = data.pointer("/usage/output_tokens").and_then(Value::as_f64).unwrap_or(0.0);
    let api = data.get("durationApiMs").and_then(Value::as_f64).filter(|ms| *ms > 0.0).map(|ms| ms / 1000.0).unwrap_or(seconds);
    if output > 0.0 && api > 0.0 {
        summary.push_str(&format!(" · {:.0} tok/s", output / api));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat() -> Chat {
        Chat::new("c1".into(), 0, PathBuf::from(r"C:\repo"))
    }

    fn models() -> Vec<(String, String)> {
        [("", "Predeterminado"), ("opus", "Opus 4.5"), ("sonnet", "Sonnet 4.5")].iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
    }

    fn uuid_of(item: &Item) -> Option<&str> {
        match item {
            Item::User { uuid, .. } => uuid.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn cada_conversacion_usa_su_modelo_o_el_del_espacio() {
        let config = ClaudeConfig { model: "sonnet".into(), effort: "high".into(), ..Default::default() };
        let mut chat = chat();
        assert_eq!((chat.chosen_model(&config), chat.chosen_effort(&config)), ("sonnet".into(), "high".into()));
        chat.want_model = Some("opus".into());
        chat.want_effort = Some("low".into());
        assert_eq!((chat.chosen_model(&config), chat.chosen_effort(&config)), ("opus".into(), "low".into()));
        // Elegir «Predeterminado» en la conversación no vuelve al del espacio.
        chat.want_model = Some(String::new());
        assert_eq!(chat.chosen_model(&config), "");
    }

    #[test]
    fn la_marca_de_modelo_aparece_al_enviar_con_otro() {
        let list = models();
        let config = ClaudeConfig::default();
        let mut chat = chat();
        chat.note_model_for_send(&list, "opus");
        assert!(chat.items.is_empty(), "el primer mensaje no lleva marca");
        chat.push_user("hola", Vec::new());
        chat.busy = false;
        // El id de la sesión retomada y el alias son el mismo modelo.
        chat.sent_model = Some("claude-opus-4-5-20251101".into());
        chat.want_model = Some("opus".into());
        assert_eq!(chat.pending_model(&list, &config), None);
        chat.note_model_for_send(&list, "opus");
        assert!(!chat.items.iter().any(|i| matches!(i, Item::Model(_))));
        chat.want_model = Some("sonnet".into());
        assert_eq!(chat.pending_model(&list, &config).as_deref(), Some("Sonnet 4.5"));
        chat.note_model_for_send(&list, "sonnet");
        assert_eq!(chat.items.last(), Some(&Item::Model("Sonnet 4.5".into())));
        assert_eq!(chat.pending_model(&list, &config), None);
    }

    #[test]
    fn otro_modelo_respondiendo_avisa_una_vez() {
        let list = models();
        let mut chat = chat();
        // Sin modelo aplicado (o el predeterminado) no hay con qué comparar.
        assert_eq!(chat.check_model(&list, "claude-sonnet-4-5"), None);
        chat.applied = Some(Applied { model: "opus".into(), effort: String::new() });
        assert_eq!(chat.check_model(&list, "claude-opus-4-5-20251101"), None);
        assert_eq!(chat.check_model(&list, "<synthetic>"), None);
        assert_eq!(chat.check_model(&list, "claude-sonnet-4-5").as_deref(), Some("opus"));
        assert_eq!(chat.check_model(&list, "claude-sonnet-4-5"), None);
        assert_eq!(chat.items.iter().filter(|i| matches!(i, Item::Notice { error: true, .. })).count(), 1);
    }

    #[test]
    fn el_texto_final_reemplaza_al_de_los_trozos() {
        let mut chat = chat();
        chat.push_user("hola", Vec::new());
        chat.apply(
            "stream",
            &json!([
                { "op": "message", "msg": "m1", "parent": null },
                { "op": "start", "msg": "m1", "index": 0, "kind": "text", "parent": null },
                { "op": "delta", "msg": "m1", "index": 0, "text": "Hola, " },
                { "op": "delta", "msg": "m1", "index": 0, "text": "¿qué hace" },
                { "op": "stop", "msg": "m1", "index": 0 }
            ]),
        );
        chat.apply("assistant", &json!({ "id": "m1", "uuid": "a1", "parent": null, "content": [{ "type": "text", "text": "Hola, ¿qué hacemos?" }] }));
        assert_eq!(chat.items, vec![Item::user("hola"), Item::Text("Hola, ¿qué hacemos?".into())]);
        assert!(chat.busy);
        chat.apply("result", &json!({ "subtype": "success", "isError": false, "durationMs": 1500, "costUsd": 0.01 }));
        assert!(!chat.busy);
        assert_eq!(chat.items.last(), Some(&Item::Turn("1.5 s".into())));
        // Con tokens de salida, la velocidad; con el tiempo de API, sobre ese tiempo.
        assert_eq!(turn_summary(&json!({ "durationMs": 2000, "usage": { "output_tokens": 80 } })), "2.0 s · 40 tok/s");
        assert_eq!(turn_summary(&json!({ "durationMs": 3000, "durationApiMs": 1000, "usage": { "output_tokens": 50 } })), "3.0 s · 50 tok/s");
        // Con la medición del sidecar manda la velocidad de escritura, no la del turno.
        let speed = json!({ "durationMs": 9000, "durationApiMs": 6000, "usage": { "output_tokens": 150 }, "speed": { "tokens": 150, "genMs": 3000, "ttftMs": 750 } });
        assert_eq!(turn_summary(&speed), "9.0 s · 50 tok/s · 0.8 s al primer token");
        // Un turno sin mensajes medibles (`speed: null`) vuelve al cálculo de antes.
        assert_eq!(turn_summary(&json!({ "durationMs": 2000, "usage": { "output_tokens": 80 }, "speed": null })), "2.0 s · 40 tok/s");
    }

    #[test]
    fn cada_bloque_del_mensaje_recibe_su_version_final() {
        // Claude Code manda cada bloque como un mensaje aparte con el mismo id.
        let mut chat = chat();
        chat.apply(
            "stream",
            &json!([
                { "op": "message", "msg": "m1", "parent": null },
                { "op": "start", "msg": "m1", "index": 0, "kind": "thinking", "parent": null },
                { "op": "delta", "msg": "m1", "index": 0, "text": "pien" },
                { "op": "start", "msg": "m1", "index": 1, "kind": "text", "parent": null },
                { "op": "delta", "msg": "m1", "index": 1, "text": "uno" }
            ]),
        );
        chat.apply("assistant", &json!({ "id": "m1", "parent": null, "content": [{ "type": "thinking", "thinking": "pienso" }] }));
        chat.apply("assistant", &json!({ "id": "m1", "parent": null, "content": [{ "type": "text", "text": "uno y dos" }] }));
        // Uno que no llegó en trozos se agrega.
        chat.apply("assistant", &json!({ "id": "m1", "parent": null, "content": [{ "type": "text", "text": "tres" }] }));
        assert_eq!(chat.items, vec![Item::Thinking("pienso".into()), Item::Text("uno y dos".into()), Item::Text("tres".into())]);
    }

    #[test]
    fn lee_los_avisos_de_tareas() {
        let done = "<task-notification>\n<task-id>a656</task-id>\n<tool-use-id>toolu_1</tool-use-id>\n<output-file>/private/tmp/a656.output</output-file>\n<status>completed</status>\n<summary>Agent \"Review macOS platform layer\" finished</summary>\n<note>leelo</note>\n<result>## Hallazgos\n\n- `a &amp;&amp; b` <b>dos</b>\n</result>\n<usage><subagent_tokens>171883</subagent_tokens><tool_uses>35</tool_uses><duration_ms>196005</duration_ms></usage>\n</task-notification>";
        let (rest, notes) = split_task_notes(done);
        assert_eq!(rest, "");
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.task_id, "a656");
        assert_eq!(note.tool_use_id.as_deref(), Some("toolu_1"));
        assert_eq!(note.status, "completed");
        assert_eq!(note.result.as_deref(), Some("## Hallazgos\n\n- `a && b` <b>dos</b>"));
        assert_eq!((note.tokens, note.tool_uses, note.duration_ms), (Some(171_883), Some(35), Some(196_005)));
        assert_eq!(note.heading(), "Agente terminó: Review macOS platform layer");
        assert_eq!(note.meta(), "3 min 16 s · 35 herramientas · 171.9k tok");

        // Detenida, sin resultado ni uso, dentro del recordatorio con que la manda Claude Code
        // y junto a un mensaje de verdad.
        let stopped = "<system-reminder>\n[SYSTEM NOTIFICATION - NOT USER INPUT]\nThis is an automated background-task event, NOT a message from the user.\n\n<task-notification>\n<task-id>b1</task-id>\n<status>stopped</status>\n<summary>Background agent \"Buscar\" didn&apos;t finish</summary>\n</task-notification>\n</system-reminder>\nsigue con lo otro";
        let (rest, notes) = split_task_notes(stopped);
        assert_eq!(rest, "sigue con lo otro");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].result, None);
        assert_eq!((notes[0].tokens, notes[0].tool_uses, notes[0].duration_ms), (None, None, None));
        assert_eq!(notes[0].heading(), "Agente se detuvo: Buscar");
        assert_eq!(notes[0].summary, "Background agent \"Buscar\" didn't finish");
        assert_eq!(notes[0].meta(), "");

        let failed = TaskNote { status: "failed".into(), summary: "Background command \"npm test\" failed with exit code 1".into(), ..Default::default() };
        assert_eq!(failed.heading(), "Comando falló: npm test");

        // Lo que vio un Monitor: sin estado, con su salida (sin los colores de la terminal).
        let event = "<task-notification>\n<task-id>m1</task-id>\n<summary>Monitor event: \"build\"</summary>\n<event>\u{1b}[1m\u{1b}[92mFinished\u{1b}[0m ok</event>\n</task-notification>";
        let (_, notes) = split_task_notes(event);
        assert_eq!(notes[0].heading(), "Monitor avisó: build");
        assert_eq!(notes[0].result.as_deref(), Some("```text\nFinished ok\n```"));

        // Un mensaje de verdad que cita la etiqueta no es un aviso.
        let quoted = "mira esto: <task-notification><task-id>x</task-id></task-notification>";
        assert_eq!(split_task_notes(quoted), (quoted.to_string(), Vec::new()));
        assert!(split_task_notes("<task-notification>\nhola\n</task-notification>").1.is_empty());
    }

    #[test]
    fn el_aviso_de_tarea_no_es_un_mensaje_del_usuario() {
        let mut chat = chat();
        let text = "<task-notification>\n<task-id>t1</task-id>\n<status>failed</status>\n<summary>Agent \"X\" failed</summary>\n</task-notification>";
        chat.apply("user", &json!({ "uuid": "n1", "parent": null, "content": text }));
        // Repetido (p. ej. su eco) no se duplica.
        chat.apply("user", &json!({ "uuid": "n1", "parent": null, "isReplay": true, "content": [{ "type": "text", "text": text }] }));
        assert_eq!(chat.items.len(), 1);
        assert!(matches!(&chat.items[0], Item::Task(note) if note.heading() == "Agente falló: X"));
        assert!(!chat.busy);
        chat.load_history(&[], &[json!({ "type": "user", "uuid": "n2", "parent_tool_use_id": null, "message": { "content": text } })], None);
        assert!(matches!(&chat.items[1], Item::Task(_)));
        assert_eq!(chat.title, "Nueva conversación");
    }

    #[test]
    fn el_eco_le_da_su_uuid_y_arma_la_cadena() {
        let mut chat = chat();
        chat.push_user("hola", Vec::new());
        chat.apply("user", &json!({ "uuid": "u1", "parent": null, "isReplay": true, "content": [{ "type": "text", "text": "hola" }] }));
        chat.apply("assistant", &json!({ "id": "m1", "uuid": "a1", "parent": null, "content": [{ "type": "text", "text": "Hola" }] }));
        // Los subagentes no entran en la cadena.
        chat.apply("assistant", &json!({ "id": "s1", "uuid": "x1", "parent": "t9", "content": [] }));
        chat.apply("result", &json!({ "subtype": "success" }));
        chat.push_user("sigue", Vec::new());
        chat.apply("user", &json!({ "uuid": "u2", "parent": null, "isReplay": true, "content": "sigue" }));
        assert_eq!(uuid_of(&chat.items[0]), Some("u1"));
        assert_eq!(chat.uuid_before("u2").as_deref(), Some("a1"));
        assert_eq!(chat.uuid_before("a1").as_deref(), Some("u1"));
        assert_eq!(chat.uuid_before("u1"), None);
        assert_eq!(chat.uuid_before("x1"), None);
        assert_eq!(chat.user_turns(), vec![("u2".into(), "sigue".into()), ("u1".into(), "hola".into())]);
        // El eco no se agrega como otro mensaje.
        assert_eq!(chat.items.iter().filter(|i| matches!(i, Item::User { .. })).count(), 2);
    }

    #[test]
    fn el_tamano_de_una_imagen_sale_de_su_cabecera() {
        let picture = image::RgbaImage::from_pixel(3, 2, image::Rgba([0, 0, 0, 0]));
        let mut png = Vec::new();
        picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert_eq!(image_dimensions(&png), Some((3, 2)));
    }

    #[test]
    fn una_captura_grande_queda_como_miniatura_con_su_tamano_original() {
        let picture = image::RgbaImage::from_pixel(1200, 600, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let image = image_from("image/png", png.clone()).unwrap();
        assert_eq!(image_dimensions(image.bytes()), Some((THUMB_PX, THUMB_PX / 2)));
        // La miniatura conserva la proporción del original.
        assert_eq!(image_size(&image), Some((1200., 600.)));
        // La misma imagen otra vez no se decodifica de nuevo.
        assert!(Arc::ptr_eq(&image, &image_from("image/png", png).unwrap()));
    }

    #[test]
    fn las_opciones_acp_se_leen_aunque_vengan_agrupadas() {
        let config = json!([
            { "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "b",
              "options": [{ "group": "x", "name": "X", "options": [{ "value": "a", "name": "A" }] }, { "value": "b", "name": "B" }] },
            { "id": "flag", "type": "boolean", "currentValue": true }
        ]);
        let options = acp_options(&config);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].current, "b");
        let names: Vec<&str> = options[0].choices.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn el_modo_como_opcion_va_con_los_modos() {
        let mut chat = Chat::new("c".into(), 1, std::path::PathBuf::from("."));
        chat.apply("acp_config", &json!({ "configOptions": [
            { "id": "model", "category": "model", "type": "select", "currentValue": "a", "options": [{ "value": "a", "name": "A" }] },
            { "id": "mode", "category": "mode", "type": "select", "currentValue": "build",
              "options": [{ "value": "build", "name": "Build", "description": "x" }, { "value": "plan", "name": "Plan" }] }
        ] }));
        assert_eq!(chat.acp_cfg.options.len(), 1);
        assert_eq!(chat.acp_cfg.modes.len(), 2);
        assert_eq!(chat.acp_cfg.mode.as_deref(), Some("build"));
        assert_eq!(chat.acp_cfg.mode_config.as_deref(), Some("mode"));
        // Lo elegido antes de abrir viaja como una opción más (el modo, por su id).
        chat.acp_cfg.set("mode", "plan");
        chat.acp_cfg.set("model", "a");
        let wanted = chat.acp_cfg.wanted();
        assert_eq!(wanted["config"]["mode"], "plan");
        assert_eq!(wanted["config"]["model"], "a");
        assert!(wanted["mode"].is_null());
    }

    #[test]
    fn la_cola_de_envio_espera_al_turno_y_es_en_orden() {
        let mut chat = Chat::new("c".into(), 1, std::path::PathBuf::from("."));
        chat.busy = true;
        chat.enqueue("uno".into(), Vec::new(), Vec::new());
        chat.enqueue("dos".into(), Vec::new(), Vec::new());
        // Con el turno en curso nada sale; al terminar, sale el primero.
        assert!(chat.next_queued(false).is_none());
        chat.busy = false;
        assert_eq!(chat.next_queued(false).map(|m| m.text), Some("uno".into()));
        // El turno del primero deja al segundo esperando otra vez.
        chat.busy = true;
        assert!(chat.next_queued(false).is_none());
        chat.busy = false;
        assert_eq!(chat.next_queued(false).map(|m| m.text), Some("dos".into()));
        assert!(chat.next_queued(false).is_none());
    }

    #[test]
    fn detener_deja_la_cola_esperando_y_enviar_ahora_la_despacha() {
        let mut chat = Chat::new("c".into(), 1, std::path::PathBuf::from("."));
        let uno = chat.enqueue("uno".into(), Vec::new(), Vec::new());
        let dos = chat.enqueue("dos".into(), Vec::new(), Vec::new());
        assert_ne!(uno, dos);
        // Detenido por el usuario: la cola no sale sola.
        assert!(chat.next_queued(true).is_none());
        assert_eq!(chat.queued.len(), 2);
        // «Enviar ahora» sobre el segundo: pasa al frente y sale con la interrupción.
        assert!(chat.send_first_after_stop(dos));
        assert_eq!(chat.next_queued(true).map(|m| m.text), Some("dos".into()));
        // La marca se usa una vez: la próxima interrupción vuelve a dejarla esperando.
        assert!(chat.next_queued(true).is_none());
        assert_eq!(chat.remove_queued(uno).map(|m| m.text), Some("uno".into()));
        assert!(!chat.send_first_after_stop(uno));
    }

    #[test]
    fn al_cerrarse_la_sesion_la_cola_se_suelta_con_aviso() {
        let mut chat = Chat::new("c".into(), 1, std::path::PathBuf::from("."));
        chat.busy = true;
        chat.enqueue("uno".into(), Vec::new(), Vec::new());
        chat.apply("closed", &json!({}));
        assert!(chat.queued.is_empty());
        assert!(matches!(chat.items.last(), Some(Item::Notice { .. })));
    }

    #[test]
    fn la_cola_recibe_sus_ecos_en_orden() {
        let mut chat = chat();
        chat.push_user("uno", Vec::new());
        // Mientras responde se escriben dos más: quedan en la cola del sidecar.
        chat.push_user("dos", Vec::new());
        chat.push_user("tres", Vec::new());
        assert!(chat.busy);
        chat.apply("user", &json!({ "uuid": "u1", "parent": null, "isReplay": true, "content": "uno" }));
        chat.apply("result", &json!({ "subtype": "success" }));
        assert!(!chat.busy);
        // El siguiente de la cola empieza a responder solo.
        chat.apply("stream", &json!([{ "op": "message", "msg": "m2", "parent": null }]));
        assert!(chat.busy);
        chat.apply("user", &json!({ "uuid": "u2", "parent": null, "isReplay": true, "content": "dos" }));
        chat.apply("user", &json!({ "uuid": "u3", "parent": null, "isReplay": true, "content": "tres" }));
        let uuids: Vec<_> = chat.items.iter().filter_map(uuid_of).collect();
        assert_eq!(uuids, vec!["u1", "u2", "u3"]);
    }

    #[test]
    fn un_mensaje_de_otro_cliente_aparece_y_responde() {
        let mut chat = chat();
        chat.apply("user", &json!({ "uuid": "r1", "parent": null, "content": "desde el móvil" }));
        assert_eq!(chat.items, vec![Item::User { text: "desde el móvil".into(), uuid: Some("r1".into()), images: Vec::new() }]);
        assert!(chat.busy);
        chat.apply("user", &json!({ "uuid": "r1", "parent": null, "content": "desde el móvil" }));
        assert_eq!(chat.items.len(), 1);
    }

    #[test]
    fn una_herramienta_junta_input_y_resultado() {
        let mut chat = chat();
        chat.apply(
            "stream",
            &json!([
                { "op": "message", "msg": "m1", "parent": null },
                { "op": "start", "msg": "m1", "index": 0, "kind": "tool_use", "id": "t1", "name": "Read", "parent": null },
                { "op": "delta", "msg": "m1", "index": 0, "text": "{\"file_path\":" },
                { "op": "delta", "msg": "m1", "index": 0, "text": "\"C:\\\\repo\\\\a.rs\"}" },
                { "op": "stop", "msg": "m1", "index": 0 }
            ]),
        );
        chat.apply("user", &json!({ "parent": null, "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": [{ "type": "text", "text": "fn main() {}" }] }] }));
        let Item::Tool(tool) = &chat.items[0] else { panic!("no es herramienta") };
        assert_eq!(tool.result.as_deref(), Some("fn main() {}"));
        assert!(!tool.is_error);
        // El resultado de una herramienta no es un mensaje del usuario.
        assert_eq!(chat.items.len(), 1);
        // Leer no cuenta como editar.
        assert!(chat.edited.is_empty());
    }

    #[test]
    fn editar_un_archivo_avisa_al_visor() {
        let mut chat = chat();
        chat.apply("assistant", &json!({ "id": "m1", "parent": null, "content": [{ "type": "tool_use", "id": "t1", "name": "Edit", "input": { "file_path": r"C:\repo\a.rs" } }] }));
        chat.apply("user", &json!({ "parent": null, "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "ok" }] }));
        assert_eq!(chat.edited, vec![PathBuf::from(r"C:\repo\a.rs")]);
    }

    #[test]
    fn al_detener_se_ve_detenido_y_lo_que_corria_falla() {
        let mut chat = chat();
        chat.push_user("hola", Vec::new());
        chat.apply("assistant", &json!({ "id": "m1", "parent": null, "content": [
            { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "sleep 99" } }
        ] }));
        chat.interrupted = true;
        chat.apply("result", &json!({ "subtype": "error_during_execution", "isError": true, "errors": ["[ede_diagnostic] algo"] }));
        let Item::Tool(tool) = &chat.items[1] else { panic!("no es herramienta") };
        assert!(tool.is_error && tool.result.is_some());
        assert!(chat.items.contains(&Item::Notice { text: "Detenido.".into(), error: false }));
        assert!(!chat.interrupted);
        // Sin pedirlo, el mismo error sin detalle también es «Detenido.».
        chat.apply("result", &json!({ "subtype": "error_during_execution", "isError": true, "errors": [] }));
        assert_eq!(chat.items.iter().filter(|i| matches!(i, Item::Notice { error: false, .. })).count(), 2);
        // Con detalle, es un error.
        chat.apply("result", &json!({ "subtype": "error_max_turns", "isError": true, "errors": ["Demasiados turnos"] }));
        assert!(chat.items.contains(&Item::Notice { text: "Demasiados turnos".into(), error: true }));
    }

    #[test]
    fn la_compactacion_deja_su_separador() {
        let mut chat = chat();
        chat.apply("system", &json!({ "subtype": "compact_boundary" }));
        chat.apply("system", &json!({ "subtype": "bridge_state", "state": "ready" }));
        assert_eq!(chat.items, vec![Item::Compact]);
    }

    #[test]
    fn conectando_hasta_meta_y_remote_control() {
        let mut chat = chat();
        chat.starting = true;
        chat.apply("meta", &json!({}));
        assert!(!chat.starting);
        chat.apply("remote", &json!({ "on": true, "url": "https://claude.ai/code/x" }));
        assert!(chat.remote_on);
        assert_eq!(chat.remote_url.as_deref(), Some("https://claude.ai/code/x"));
        chat.apply("remote", &json!({ "on": false }));
        assert!(!chat.remote_on && chat.remote_url.is_none());
    }

    #[test]
    fn los_subagentes_y_los_ecos_no_aparecen() {
        let mut chat = chat();
        chat.apply("stream", &json!([
            { "op": "message", "msg": "sub", "parent": "t9" },
            { "op": "start", "msg": "sub", "index": 0, "kind": "text", "parent": "t9" },
            { "op": "delta", "msg": "sub", "index": 0, "text": "interno" }
        ]));
        chat.apply("user", &json!({ "parent": null, "isReplay": true, "content": "hola" }));
        chat.apply("assistant", &json!({ "id": "x", "parent": "t9", "content": [{ "type": "text", "text": "interno" }] }));
        assert!(chat.items.is_empty());
    }

    fn history() -> Vec<Value> {
        vec![
            json!({ "type": "user", "uuid": "u0", "parent_tool_use_id": null, "message": { "role": "user", "content": "<command-name>/clear</command-name>" } }),
            json!({ "type": "user", "uuid": "u1", "parent_tool_use_id": null, "message": { "role": "user", "content": "arregla el bug\ndel login" } }),
            json!({ "type": "assistant", "uuid": "a1", "parent_tool_use_id": null, "message": { "id": "m1", "content": [
                { "type": "text", "text": "Voy." },
                { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "cargo test" } }
            ] } }),
            json!({ "type": "user", "uuid": "u2", "parent_tool_use_id": null, "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "ok", "is_error": true }] } }),
            json!({ "type": "assistant", "uuid": "a2", "parent_tool_use_id": null, "message": { "id": "m2", "content": [
                { "type": "tool_use", "id": "t2", "name": "Read", "input": { "file_path": "x" } }
            ] } }),
            json!({ "type": "user", "uuid": "u3", "parent_tool_use_id": null, "message": { "content": "otra cosa" } }),
        ]
    }

    #[test]
    fn el_historial_arma_la_conversacion_sin_lo_interno() {
        let mut chat = chat();
        chat.load_history(&[], &history(), None);
        assert_eq!(chat.title, "arregla el bug");
        assert_eq!(chat.items.len(), 5);
        assert_eq!(uuid_of(&chat.items[0]), Some("u1"));
        let Item::Tool(tool) = &chat.items[2] else { panic!("no es herramienta") };
        assert!(tool.is_error);
        // La que no tiene resultado en el archivo se da por hecha.
        let Item::Tool(tool) = &chat.items[3] else { panic!("no es herramienta") };
        assert_eq!((tool.result.as_deref(), tool.is_error), (Some(""), false));
        assert!(!chat.busy);
        assert_eq!(chat.uuid_before("u3").as_deref(), Some("a2"));
        // El historial no avisa al visor.
        assert!(chat.edited.is_empty());
    }

    #[test]
    fn el_historial_marca_el_cambio_de_modelo() {
        let reply = |uuid: &str, id: &str, model: &str| json!({ "type": "assistant", "uuid": uuid, "parent_tool_use_id": null, "message": { "id": id, "model": model, "content": [{ "type": "text", "text": "ok" }] } });
        let ask = |uuid: &str, text: &str| json!({ "type": "user", "uuid": uuid, "parent_tool_use_id": null, "message": { "content": text } });
        let messages = vec![
            ask("u1", "uno"),
            reply("a1", "m1", "claude-opus-4-5-20251101"),
            ask("u2", "dos"),
            reply("a2", "m2", "claude-opus-4-5-20251101"),
            ask("u3", "tres"),
            // Una respuesta sintética no cuenta como cambio.
            reply("a3", "m3", "<synthetic>"),
            reply("a4", "m4", "claude-sonnet-4-5"),
        ];
        let mut chat = chat();
        chat.load_history(&models(), &messages, None);
        let marks: Vec<_> = chat.items.iter().enumerate().filter_map(|(i, it)| if let Item::Model(m) = it { Some((i, m.clone())) } else { None }).collect();
        // Solo el tercer mensaje, justo antes de él; el primero no tiene con qué comparar.
        assert_eq!(marks.len(), 1);
        assert!(matches!(&chat.items[marks[0].0 + 1], Item::User { text, .. } if text == "tres"));
        assert_eq!(marks[0].1, "Sonnet 4.5");
        // Con `up_to` antes del cambio no aparece.
        let mut cut = Chat::new("k".into(), 1, PathBuf::new());
        cut.load_history(&models(), &messages, Some("a2"));
        assert!(cut.items.iter().all(|i| !matches!(i, Item::Model(_))));
    }

    #[test]
    fn la_rama_de_un_rewind_llega_hasta_su_mensaje() {
        let mut chat = chat();
        chat.load_history(&[], &history(), Some("a1"));
        assert_eq!(chat.items.len(), 3);
        assert_eq!(chat.user_turns(), vec![("u1".into(), "arregla el bug\ndel login".into())]);
        chat.reset_items();
        assert!(chat.items.is_empty() && chat.uuid_before("a1").is_none());
    }

    #[test]
    fn un_permiso_llega_y_se_cancela() {
        let mut chat = chat();
        chat.apply("permission", &json!({ "requestId": "p1", "toolName": "Bash", "input": { "command": "rm x" }, "suggestions": [], "toolUseID": "t1", "defaultToNo": true }));
        assert_eq!(chat.permissions.len(), 1);
        assert!(chat.permissions[0].suggestions.is_none());
        assert!(chat.permissions[0].default_to_no);
        assert_eq!(chat.permissions[0].tool_use_id.as_deref(), Some("t1"));
        chat.apply("permission_cancel", &json!({ "requestId": "p1" }));
        assert!(chat.permissions.is_empty());
    }

    #[test]
    fn solo_se_marcan_mensajes_y_respuestas() {
        assert_eq!(Item::user("hola").flag_key(), Some(("user", "hola")));
        assert_eq!(Item::Text("ok".into()).flag_key(), Some(("text", "ok")));
        assert_eq!(Item::Compact.flag_key(), None);
        let mut chat = chat();
        assert_eq!(chat.flag_session(), "c1");
        chat.apply("session", &json!({ "sessionId": "s1" }));
        assert_eq!(chat.flag_session(), "s1");
    }

    #[test]
    fn el_verbo_de_la_tarjeta_del_artifact() {
        assert_eq!(artifact_verb(None), "Artifact");
        assert_eq!(artifact_verb(Some(&json!({ "action": "publish" }))), "Artifact");
        assert_eq!(artifact_verb(Some(&json!({ "action": "read" }))), "Leer artifact");
        assert_eq!(artifact_verb(Some(&json!({ "action": "list" }))), "Listar artifacts");
        assert_eq!(artifact_verb(Some(&json!({ "action": "delete" }))), "Artifact · delete");
    }

    #[test]
    fn enlace_y_titulo_del_artifact() {
        let text = "Publicado en https://claude.ai/code/artifact/abc_12-x. Listo.";
        assert_eq!(artifact_url(text).as_deref(), Some("https://claude.ai/code/artifact/abc_12-x"));
        assert_eq!(artifact_url("https://claude.ai/artifact/").as_deref(), None);
        assert_eq!(artifact_title(Some(&json!({ "file_path": r"C:\docs\Notas.HTML" }))), "Notas");
        assert_eq!(artifact_title(Some(&json!({ "title": " Plan " }))), "Plan");
        assert!(artifact_is_publish(Some(&json!({ "action": "publish" }))));
        assert!(!artifact_is_publish(Some(&json!({ "action": "read" }))));
    }

    #[test]
    fn aviso_de_conversacion_larga_por_nivel() {
        let mut chat = chat();
        assert_eq!(chat.long_level(), None);
        chat.context = Some((60_000, 100_000));
        assert_eq!(chat.long_level(), Some(LongLevel::Soft));
        chat.long_dismissed.push(LongLevel::Soft);
        assert_eq!(chat.long_level(), None);
        chat.context = Some((85_000, 100_000));
        assert_eq!(chat.long_level(), Some(LongLevel::Strong));
        chat.items.extend([Item::Compact, Item::Compact]);
        assert_eq!(chat.long_level(), Some(LongLevel::Compacted));
    }

    #[test]
    fn los_hijos_de_un_subagente_se_cuentan_bajo_su_task() {
        let mut chat = chat();
        chat.apply("stream", &json!([{ "op": "start", "msg": "m1", "index": 0, "kind": "tool_use", "id": "task1", "name": "Task", "parent": null }]));
        // Un mensaje del subagente por el stream y otra vez entero: cada herramienta cuenta una vez.
        chat.apply("stream", &json!([{ "op": "message", "msg": "s1", "parent": "task1" }]));
        chat.apply("stream", &json!([{ "op": "start", "msg": "s1", "index": 0, "kind": "tool_use", "id": "r1", "name": "Read", "parent": null }]));
        chat.apply("assistant", &json!({ "id": "s1", "parent": "task1", "content": [{ "type": "tool_use", "id": "r1", "name": "Read", "input": {} }] }));
        for n in 0..7 {
            chat.apply(
                "assistant",
                &json!({ "id": format!("s{}", n + 2), "parent": "task1", "content": [{ "type": "tool_use", "id": format!("g{n}"), "name": "Grep", "input": {} }, { "type": "text", "text": "ignorado" }] }),
            );
        }
        // Nada del subagente llena la conversación.
        assert_eq!(chat.items.len(), 1);
        let Item::Tool(task) = &chat.items[0] else { panic!("la Task") };
        assert_eq!(task.child_total, 8);
        assert_eq!(task.children.len(), 8);
        assert_eq!(task.children[0], "Read");
        // Solo se recuerdan las últimas veinte.
        for n in 0..30 {
            chat.apply("assistant", &json!({ "id": "x", "parent": "task1", "content": [{ "type": "tool_use", "id": format!("w{n}"), "name": "Write", "input": {} }] }));
        }
        let Item::Tool(task) = &chat.items[0] else { panic!("la Task") };
        assert_eq!((task.child_total, task.children.len()), (38, 20));
        // Un padre desconocido no rompe nada.
        chat.apply("assistant", &json!({ "id": "y", "parent": "otra", "content": [{ "type": "tool_use", "id": "z", "name": "Bash", "input": {} }] }));
        assert_eq!(chat.items.len(), 1);
    }

    #[test]
    fn razonando_en_vivo_y_trabajando() {
        let mut chat = chat();
        chat.push_user("hola", Vec::new());
        assert!(chat.working(), "recién enviado: trabajando");
        chat.apply("stream", &json!([{ "op": "message", "msg": "m1", "parent": null }, { "op": "start", "msg": "m1", "index": 0, "kind": "thinking", "parent": null }]));
        // Razonamiento vacío pero llegando: se ve y ya muestra que trabaja.
        assert!(chat.is_streaming(1));
        assert!(!chat.hidden(1));
        assert!(!chat.working());
        chat.apply("stream", &json!([{ "op": "stop", "msg": "m1", "index": 0 }]));
        // Terminó sin texto (cifrado): se oculta, y vuelve «Trabajando…».
        assert!(!chat.is_streaming(1));
        assert!(chat.hidden(1));
        assert!(chat.working());
        chat.apply("stream", &json!([{ "op": "start", "msg": "m1", "index": 1, "kind": "text", "parent": null }, { "op": "delta", "msg": "m1", "index": 1, "text": "Hola" }]));
        assert!(!chat.working());
        chat.apply("result", &json!({ "subtype": "success" }));
        assert!(!chat.is_streaming(2) && !chat.working());
        assert!(!chat.hidden(2));
        // Un permiso pendiente tampoco muestra «Trabajando…».
        chat.busy = true;
        chat.permissions.push(Permission::from(&json!({ "requestId": "p", "toolName": "Bash" })).unwrap());
        assert!(!chat.working());
    }

    #[test]
    fn un_resultado_largo_se_recorta_y_avisa() {
        assert_eq!(clip("corto".into()), "corto");
        let long = "á".repeat(MAX_RESULT);
        let clipped = clip(long);
        assert!(clipped.contains("…\n(se recortó: "), "{}", &clipped[clipped.len() - 40..]);
        assert!(clipped.len() < 2 * MAX_RESULT + 60);
        let exact = "x".repeat(MAX_RESULT);
        assert_eq!(clip(exact.clone()), exact);
        let over = clip("x".repeat(MAX_RESULT + 5));
        assert!(over.ends_with("(se recortó: 5 caracteres más)"));
    }
}
