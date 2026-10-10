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

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use serde_json::Value;

use super::config::{model_name, same_model, ClaudeConfig};

/// Lo que se guarda del resultado de una herramienta.
const MAX_RESULT: usize = 6000;
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
            LongLevel::Soft => "Esta conversación ya es larga. Para mejores resultados y menos consumo, abre un chat nuevo.",
            LongLevel::Strong => "El contexto está casi lleno: Claude pronto compactará y puede perder detalles. Te recomiendo un chat nuevo.",
            LongLevel::Compacted => "Esta conversación ya se compactó varias veces y puede haber perdido detalles. Te recomiendo un chat nuevo.",
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
}

impl ToolCall {
    /// Lo más corto que dice qué hace: la ruta, el comando o el patrón.
    pub fn summary(&self) -> String {
        let Some(input) = &self.input else {
            return String::new();
        };
        let field = |name: &str| input.get(name).and_then(Value::as_str).map(str::to_string);
        field("file_path")
            .or_else(|| field("notebook_path"))
            .or_else(|| field("command"))
            .or_else(|| field("pattern"))
            .or_else(|| field("url"))
            .or_else(|| field("query"))
            .or_else(|| field("description"))
            .or_else(|| field("prompt"))
            .map(|text| text.lines().next().unwrap_or_default().to_string())
            .unwrap_or_default()
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
    /// Mensajes de subagentes.
    nested: HashSet<String>,
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
    Some(Arc::new(gpui::Image::from_bytes(format, bytes)))
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
    format!("{}…", &text[..end])
}

fn title_for(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("Conversación").trim();
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
            title: "Nueva conversación".into(),
            items: Vec::new(),
            busy: false,
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
            nested: HashSet::new(),
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
        self.notice(format!("Esta respuesta la está dando {got}, no {chosen} (el elegido). Se volvió a aplicar {chosen} para lo que sigue."), true);
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
        self.blocks.clear();
        self.msg_blocks.clear();
        self.nested.clear();
        self.tools.clear();
        self.chain.clear();
        self.pending_echo.clear();
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
            "remote" => {
                self.remote_on = data.get("on").and_then(Value::as_bool).unwrap_or(false);
                self.remote_url = if self.remote_on { data.get("url").and_then(Value::as_str).map(str::to_string) } else { None };
            }
            "system" => {
                if data.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                    self.items.push(Item::Compact);
                }
            }
            "closed" => {
                self.live = false;
                self.starting = false;
                self.applied = None;
                self.busy = false;
                self.permissions.clear();
                if let Some(error) = data.get("error").and_then(Value::as_str) {
                    self.notice(format!("La sesión se cerró: {error}"), true);
                }
            }
            "error" => {
                let message = data.get("message").and_then(Value::as_str).unwrap_or("Error del agente");
                self.notice(message, true);
            }
            _ => {}
        }
    }

    /// El historial de una conversación guardada (`getSessionMessages`); con
    /// `up_to`, hasta ese mensaje inclusive (la rama de un Rewind).
    pub fn load_history(&mut self, messages: &[Value], up_to: Option<&str>) {
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
        let nested = op.get("parent").is_some_and(|p| !p.is_null());
        match op.get("op").and_then(Value::as_str) {
            Some("message") => {
                if nested {
                    self.nested.insert(msg);
                } else {
                    // Responde (también lo que quedó en la cola tras otro turno).
                    self.busy = true;
                }
            }
            Some("start") if !nested && !self.nested.contains(&msg) => {
                let item = match op.get("kind").and_then(Value::as_str) {
                    Some("text") => Item::Text(String::new()),
                    Some("thinking") => Item::Thinking(String::new()),
                    Some("tool_use") => {
                        let id = op.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                        if self.tools.contains_key(&id) {
                            return;
                        }
                        self.tools.insert(id.clone(), self.items.len());
                        Item::Tool(ToolCall {
                            id,
                            name: op.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            partial: String::new(),
                            input: None,
                            result: None,
                            is_error: false,
                        })
                    }
                    _ => return,
                };
                let row = self.items.len();
                match &item {
                    Item::Text(_) => self.msg_blocks.entry(msg.clone()).or_default().push((false, row, false)),
                    Item::Thinking(_) => self.msg_blocks.entry(msg.clone()).or_default().push((true, row, false)),
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
        if data.get("parent").is_some_and(|p| !p.is_null()) {
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
                        self.items.push(Item::Tool(ToolCall { id: tool_id, name, partial: String::new(), input, result: None, is_error: false }));
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
        let synthetic = data.get("isSynthetic").and_then(Value::as_bool).unwrap_or(false);
        if results || synthetic {
            return;
        }
        let text = text_of(&content);
        if is_meta(&text) {
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
            self.notice("Detenido.", false);
        } else if is_error {
            let message = if errors.is_empty() {
                data.get("result").and_then(Value::as_str).filter(|r| !r.is_empty()).unwrap_or("Ocurrió un error.").to_string()
            } else {
                errors.join("\n")
            };
            self.notice(message, true);
        }
        let seconds = data.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0) / 1000.0;
        let mut summary = format!("{seconds:.1} s");
        if let Some(cost) = data.get("costUsd").and_then(Value::as_f64) {
            summary.push_str(&format!(" · US$ {cost:.4}"));
        }
        self.items.push(Item::Turn(summary));
    }
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
        assert_eq!(chat.items.last(), Some(&Item::Turn("1.5 s · US$ 0.0100".into())));
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
        assert_eq!(tool.summary(), r"C:\repo\a.rs");
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
        chat.load_history(&history(), None);
        assert_eq!(chat.title, "arregla el bug");
        assert_eq!(chat.items.len(), 5);
        assert_eq!(uuid_of(&chat.items[0]), Some("u1"));
        let Item::Tool(tool) = &chat.items[2] else { panic!("no es herramienta") };
        assert_eq!(tool.summary(), "cargo test");
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
    fn la_rama_de_un_rewind_llega_hasta_su_mensaje() {
        let mut chat = chat();
        chat.load_history(&history(), Some("a1"));
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
}
