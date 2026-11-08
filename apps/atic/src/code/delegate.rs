//! Delegar en otro agente desde Atic Code (como los subagentes de t3code): un agente de
//! una conversación llama a `atic_delegate` (de `atic-mcp`), el hub (`hub.rs`) le pasa
//! el pedido a la ventana y aquí se abre una conversación hija con el agente pedido
//! (Claude, Codex, Cursor, OpenCode o Grok), en la misma carpeta que la del padre.
//!
//! Por defecto no se espera: `atic_delegate` contesta `running` al tiro y, cuando el
//! hijo termina su turno, su última respuesta le llega sola a la conversación padre
//! como un mensaje `<atic-subagent>` (que la vista dibuja como tarjeta, no como burbuja),
//! que entra en el turno en curso o queda en la cola. Con `mode: "wait"` (o `/v1/wait`)
//! el pedido espera el resultado.
//!
//! Cada conversación registra `atic-mcp --delegate` con quién es (`ATIC_SESSION`), a
//! qué profundidad está (`ATIC_DELEGATE_DEPTH`) y cuál es su raíz (`ATIC_ROOT`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::chat::{AcpOption, Chat};
use super::hub::{self, HubCall, HubFault, HubReply};
use super::{acp, loose_dir, CodeView, LOOSE};
use crate::i18n::tf;

/// Lo que se le explica a cada agente de Atic Code sobre los subagentes.
pub const DELEGATION_GUIDE: &str = "## Subagentes en Atic Code

Las tareas comunes no necesitan estas herramientas.

- Tienes `atic_delegate` para encargarle trabajo a otro agente: Claude, Codex, Cursor, OpenCode o Grok. Usa tu herramienta nativa de subagentes (Agent o Task) cuando el hijo es de tu mismo proveedor y modelo, sin ajustes distintos de los tuyos. Usa `atic_delegate` cuando el usuario pida otro proveedor u otro modelo, o un agente por su nombre.
- Las herramientas de Atic pueden llegar diferidas o tarde: si no las ves, cárgalas con ToolSearch (`select:mcp__atic__atic_delegate,mcp__atic__atic_list_agents`) antes de darlas por ausentes. No lances a mano la CLI del otro agente (`codex exec`, `cursor-agent`…): delega con `atic_delegate`.
- Antes del primer `atic_delegate` de la conversación, llama una vez a `atic_list_agents` (y a `atic_list_models` si el usuario nombró un modelo). Si el agente o el modelo no está disponible, dilo. No cambies a otro sin que te lo pidan. Con OpenCode, `atic_list_agents` y `atic_list_models` traen tus `subscriptions` (Go, Zen, claves) con su estado, y cada modelo su `requires` y si está `available`: elige uno con `available: true`, de preferencia `opencode-go/…` (la suscripción); los `opencode/…` de pago cobran saldo de Zen.
- El hijo no ve esta conversación: recibe solo `text`. Escribe un encargo que baste solo: el objetivo, las rutas que importan, lo que ya descartaste, las restricciones y qué debe entregar.
- `atic_delegate` no espera por defecto: cuando el hijo termine, su resultado te llega solo como un mensaje `<atic-subagent>` con su `session`, `status` y `result`. Sigue su `hint`: termina tu turno. No llames `atic_wait` ni consultes en bucle. Usa `mode: \"wait\"` solo si este mismo turno necesita el resultado.
- Cada revisión o re-chequeo nuevo es un `atic_delegate` nuevo, con el encargo, los hallazgos y las objeciones. Los arreglos chicos que salgan de una revisión, hazlos tú. Si te piden seguir sin un delegado, o con menos, corta los que sobran con `atic_cancel`.
- Si eres un subagente (te delegaron esta tarea), hazla tú mismo: no delegues salvo que el encargo lo pida. Tu última respuesta es lo único que recibe quien te delegó: que sea completa y se entienda sola (qué hiciste, qué encontraste, qué archivos tocaste, qué quedó pendiente o sin verificar).";

/// El nombre del servidor MCP en cada conversación (Claude ve `mcp__atic__…`).
const SERVER: &str = "atic";
/// Quien pide con esta profundidad (o más) ya no puede delegar: hijo y nieto, nada más.
const MAX_DEPTH: u64 = 2;
/// Hijos trabajando a la vez por cada padre.
const MAX_CHILDREN: usize = 4;
/// El tope de la respuesta que viaja al padre.
const MAX_REPLY: usize = 16_000;
const CLIPPED: &str = "\n\n… (recortado; pide el resto con atic_prompt)";
/// Cuánto del input de una herramienta se muestra al pedir un permiso.
const MAX_INPUT: usize = 600;
/// El padre de un hijo pedido desde fuera de Atic Code (una CLI suelta).
const EXTERNAL: &str = "external";

const HINT_ASYNC: &str = "Cuando termine, su resultado te llega solo como mensaje. Termina tu turno: no lo esperes ni consultes su estado.";
const HINT_TIMEOUT: &str = "Sigue trabajando; su resultado te llegará solo como mensaje. Termina tu turno.";
const HINT_TIMEOUT_EXTERNAL: &str = "Sigue trabajando. Pide su resultado con atic_wait cuando lo necesites.";
const HINT_PERMISSION: &str = "El subagente espera un permiso. Contéstalo con atic_permission (allow, allow_always o deny) o pídele al usuario que lo conteste en Atic Code; después su resultado te llega solo como mensaje.";
const HINT_PERMISSION_EXTERNAL: &str = "El subagente espera un permiso. Contéstalo con atic_permission (allow, allow_always o deny) o pídele al usuario que lo conteste en Atic Code; después pide su resultado con atic_wait.";
const DENIED: &str = "Quien te delegó la tarea rechazó este permiso.";

/// El id de cada espera (`/v1/wait` o `mode: "wait"`), para soltarla al vencer.
static NEXT_WAITER: AtomicU64 = AtomicU64::new(1);

/// Una conversación abierta por otra para hacerle un encargo.
pub struct Delegation {
    /// La clave de la conversación que la delegó (o `external`).
    pub parent: String,
    /// La conversación con que empezó la cadena.
    pub root: String,
    /// 1 para un hijo, 2 para un nieto.
    pub depth: u8,
    pub role: Option<String>,
    /// `claude`, `codex`, `cursor`, `opencode` o `grok`.
    pub backend: String,
    /// El modo de permisos de un hijo Claude (con techo en el del padre).
    pub permission_mode: Option<String>,
    /// El padre no es una conversación de Atic Code: no hay a quién despertar.
    pub external: bool,
    /// Cuándo empezó el turno en curso.
    pub started: Instant,
    /// Al terminar el turno, su resultado despierta al padre.
    pub wake: bool,
    /// Los pedidos que esperan el resultado (`wait`).
    pub waiters: Vec<(u64, mpsc::Sender<HubReply>)>,
    /// El resultado del último turno terminado.
    pub last: Option<Value>,
}

impl Delegation {
    /// Hay alguien esperando el turno en curso.
    fn pending(&self) -> bool {
        self.wake || !self.waiters.is_empty()
    }

    /// Lo que se guarda en el daemon para rearmarla si la pill se reinicia (sin quienes
    /// esperan: esos pedidos mueren con la pill).
    pub fn to_tag(&self) -> Value {
        let now = unix_millis();
        let tag = DelegationTag {
            parent: self.parent.clone(),
            root: self.root.clone(),
            depth: self.depth,
            role: self.role.clone(),
            backend: self.backend.clone(),
            permission_mode: self.permission_mode.clone(),
            external: self.external,
            wake: self.wake,
            started_ms: now.saturating_sub(self.started.elapsed().as_millis() as u64),
            last: self.last.clone(),
        };
        serde_json::to_value(tag).unwrap_or(Value::Null)
    }

    /// La delegación guardada con `to_tag`; `None` si no viene o no se entiende.
    pub fn from_tag(tag: &Value) -> Option<Self> {
        let tag: DelegationTag = serde_json::from_value(tag.clone()).ok()?;
        let elapsed = Duration::from_millis(unix_millis().saturating_sub(tag.started_ms));
        Some(Self {
            parent: tag.parent,
            root: tag.root,
            depth: tag.depth,
            role: tag.role,
            backend: tag.backend,
            permission_mode: tag.permission_mode,
            external: tag.external,
            started: Instant::now().checked_sub(elapsed).unwrap_or_else(Instant::now),
            wake: tag.wake,
            waiters: Vec::new(),
            last: tag.last,
        })
    }
}

/// `Delegation` como se guarda en el daemon (`setTag`).
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct DelegationTag {
    parent: String,
    root: String,
    depth: u8,
    #[serde(default)]
    role: Option<String>,
    backend: String,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    external: bool,
    #[serde(default)]
    wake: bool,
    /// Cuándo empezó el turno, en milisegundos Unix (un `Instant` no sobrevive a la pill).
    #[serde(default)]
    started_ms: u64,
    #[serde(default)]
    last: Option<Value>,
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Cómo terminó el turno de un hijo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    Done,
    Failed,
    Cancelled,
}

impl Ended {
    /// Del evento `result`: detenido, con error o bien.
    pub fn of(interrupted: bool, result: &Value) -> Self {
        if interrupted {
            Ended::Cancelled
        } else if result.get("isError").and_then(Value::as_bool).unwrap_or(false) {
            Ended::Failed
        } else {
            Ended::Done
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Ended::Done => "done",
            Ended::Failed => "failed",
            Ended::Cancelled => "cancelled",
        }
    }
}

// --- Piezas puras ------------------------------------------------------------------------

/// El id del proveedor de una conversación (`None` es Claude).
pub fn backend_of(cli: Option<&str>) -> &'static str {
    match cli {
        None => "claude",
        Some("cursor-agent") => "cursor",
        Some(cli) => acp::agent(cli).map_or("claude", |agent| agent.cli),
    }
}

/// El agente de un id de proveedor: `Some(None)` es Claude; `None`, uno que no existe.
fn backend_cli(backend: &str) -> Option<Option<&'static str>> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" => Some(None),
        "cursor" | "cursor-agent" => Some(Some("cursor-agent")),
        other => acp::agent(other).map(|agent| Some(agent.cli)),
    }
}

/// El rol en español, para el encargo; `None` si no hay o es general.
fn role_name(role: Option<&str>) -> Option<&'static str> {
    let role = role?.trim().to_lowercase();
    Some(match role.as_str() {
        "implementation" | "implement" | "implementer" | "implementación" | "implementacion" => "implementación",
        "research" | "investigation" | "explore" | "investigación" | "investigacion" => "investigación",
        "review" | "reviewer" | "revisión" | "revision" => "revisión",
        "design" | "designer" | "diseño" | "diseno" => "diseño",
        "test" | "tests" | "testing" | "tester" | "pruebas" => "pruebas",
        _ => return None,
    })
}

/// El encargo como lo recibe el hijo: con su rol delante, si tiene uno.
pub fn role_prefix(role: Option<&str>, text: &str) -> String {
    match role_name(role) {
        Some(name) => format!("Actúa como el subagente de {name} para esta tarea.\n\n{text}"),
        None => text.to_string(),
    }
}

/// Qué tanto deja hacer cada modo de permisos de Claude.
fn mode_rank(mode: &str) -> Option<u8> {
    match mode {
        "plan" => Some(0),
        "default" => Some(1),
        "acceptEdits" => Some(2),
        "bypassPermissions" => Some(3),
        _ => None,
    }
}

/// El modo de un hijo Claude: el pedido, si no deja hacer más que el del padre.
pub fn capped_mode(requested: Option<&str>, parent: &str) -> String {
    match (requested.and_then(mode_rank), mode_rank(parent)) {
        (Some(wanted), Some(ceiling)) if wanted <= ceiling => requested.unwrap_or(parent).to_string(),
        (Some(wanted), None) if wanted <= 1 => requested.unwrap_or(parent).to_string(),
        _ => parent.to_string(),
    }
}

/// La respuesta con su tope, cortada en un límite de carácter.
pub fn clip_reply(text: &str) -> String {
    if text.len() <= MAX_REPLY {
        return text.to_string();
    }
    let mut end = MAX_REPLY;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{CLIPPED}", &text[..end])
}

/// Un campo corto del despertar: sin `&` ni `<` sueltos.
fn escape_field(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

/// La respuesta del hijo dentro de `<result>`: queda legible (`Vec<String>` sigue igual) y
/// solo se escapan `&` y los cierres (`</…`), que podrían cortar la etiqueta antes de tiempo.
fn escape_result(text: &str) -> String {
    text.replace('&', "&amp;").replace("</", "&lt;/")
}

/// El mensaje que despierta al padre cuando un hijo termina: una etiqueta propia
/// (`chat::split_subagent_notes` la lee y la vista la dibuja como tarjeta) con una línea
/// legible para el modelo delante.
pub fn wake_message(key: &str, title: &str, backend: &str, role: Option<&str>, status: &str, elapsed: Duration, reply: &str) -> String {
    let reply = if reply.trim().is_empty() { "(no dejó respuesta)" } else { reply };
    let role = role.map(str::trim).filter(|r| !r.is_empty()).map(|r| format!("<role>{}</role>\n", escape_field(r))).unwrap_or_default();
    format!(
        "<atic-subagent>\nEl subagente {key} ({backend}) terminó su turno con estado {status}. Su respuesta va abajo, en result: úsala para seguir con tu tarea.\n<session>{key}</session>\n<title>{title}</title>\n<agent>{backend}</agent>\n{role}<status>{status}</status>\n<duration_ms>{ms}</duration_ms>\n<result>\n{reply}\n</result>\n</atic-subagent>",
        title = escape_field(title),
        ms = elapsed.as_millis(),
        reply = escape_result(reply),
    )
}

/// El nombre legible de un proveedor (`codex` → «Codex»).
pub fn agent_label(backend: &str) -> &'static str {
    match backend_cli(backend) {
        Some(Some(cli)) => acp::agent(cli).map_or("Claude", |agent| agent.name),
        _ => "Claude",
    }
}

/// El logo de un proveedor (el de la lista de agentes de la pill), si hay uno.
pub fn agent_logo(backend: &str) -> Option<&'static str> {
    let cli = backend_cli(backend)?.unwrap_or("claude");
    crate::agents::AGENTS.iter().find(|agent| agent.cli == cli).map(|agent| agent.logo)
}

/// El rol para mostrar (`review` → «revisión»), si es uno conocido.
pub fn role_label(role: Option<&str>) -> Option<&'static str> {
    let label = match role_name(role)? {
        "implementación" => crate::i18n::t("pill.code.delegate.roleImplementation"),
        "investigación" => crate::i18n::t("pill.code.delegate.roleResearch"),
        "revisión" => crate::i18n::t("pill.code.delegate.roleReview"),
        "diseño" => crate::i18n::t("pill.code.delegate.roleDesign"),
        _ => crate::i18n::t("pill.code.delegate.roleTest"),
    };
    Some(label)
}

/// Qué herramienta de Atic es una llamada: `mcp__atic__atic_delegate` (Claude),
/// `atic · atic_delegate` (Codex) o el nombre a secas (otros agentes ACP).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegateTool {
    Delegate,
    Prompt,
}

impl DelegateTool {
    pub fn of(name: &str) -> Option<Self> {
        let tail = ["__", " · ", "/", ".", ":"].iter().fold(name.trim(), |acc, sep| acc.rsplit(sep).next().unwrap_or(acc)).trim();
        match tail {
            "atic_delegate" => Some(DelegateTool::Delegate),
            "atic_prompt" => Some(DelegateTool::Prompt),
            _ => None,
        }
    }
}

/// Lo que devolvió una llamada a `atic_delegate` o `atic_prompt` (`{"session":"c2",…}`, el
/// `Outcome` de `atic-mcp`), aunque venga con texto alrededor.
pub fn delegate_outcome(result: &str) -> Option<Value> {
    let start = result.find('{')?;
    serde_json::Deserializer::from_str(&result[start..]).into_iter::<Value>().next()?.ok()
}

/// El hijo que abrió una llamada a `atic_delegate`: el `session` de lo que devolvió.
pub fn delegate_session(result: &str) -> Option<String> {
    let value = delegate_outcome(result)?;
    value.get("session").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Lo que hace ahora un hijo, para su tarjeta en el padre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildState {
    /// La llamada todavía no vuelve: el hijo se está abriendo.
    Starting,
    Working,
    /// Espera un permiso (la herramienta que lo pide).
    Permission(String),
    Done,
    Failed,
    Stopped,
    /// Creado y sin turno (`atic_spawn` sin `atic_prompt`).
    Idle,
    /// Ya no está entre las conversaciones abiertas (se cerró, o es de otra sesión de Atic).
    Gone,
}

/// El estado de un hijo según su conversación: si espera un permiso, si trabaja y cómo
/// terminó su último turno (`Delegation::last`).
pub fn child_state(busy: bool, permission: Option<&str>, last_status: Option<&str>) -> ChildState {
    if let Some(tool) = permission {
        return ChildState::Permission(tool.to_string());
    }
    if busy {
        return ChildState::Working;
    }
    match last_status {
        Some("failed") => ChildState::Failed,
        Some("cancelled") => ChildState::Stopped,
        Some(_) => ChildState::Done,
        None => ChildState::Idle,
    }
}

/// El nombre de la herramienta que pide un permiso, para «pide permiso: Bash».
pub fn permission_tool(permission: &super::chat::Permission) -> String {
    [permission.display_name.as_deref(), permission.title.as_deref(), Some(permission.tool.as_str())]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map_or_else(|| crate::i18n::t("pill.code.delegate.toolFallback").to_string(), str::to_string)
}

/// Los textos para `appendSystemPrompt`, juntos; `None` si no hay ninguno.
pub fn join_prompts(parts: &[String]) -> Option<String> {
    let parts: Vec<&str> = parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// El primer mensaje de una sesión ACP nueva, con las instrucciones delante (ACP no
/// tiene prompt de sistema).
pub fn with_guide(text: &str) -> String {
    format!("<instrucciones_de_atic>\n{DELEGATION_GUIDE}\n</instrucciones_de_atic>\n\n<pedido_del_usuario>\n{text}\n</pedido_del_usuario>")
}

/// El mensaje sin las instrucciones de `with_guide` (al rearmar el chat desde el daemon).
pub fn without_guide(text: &str) -> &str {
    if !text.starts_with("<instrucciones_de_atic>") {
        return text;
    }
    text.split_once("<pedido_del_usuario>\n")
        .and_then(|(_, rest)| rest.strip_suffix("\n</pedido_del_usuario>"))
        .unwrap_or(text)
}

/// Las variables con que `atic-mcp` sabe quién lo llama.
fn mcp_env(key: &str, delegation: Option<&Delegation>) -> [(&'static str, String); 3] {
    let depth = delegation.map_or(0, |d| d.depth);
    let root = delegation.map_or_else(|| key.to_string(), |d| d.root.clone());
    [("ATIC_SESSION", key.to_string()), ("ATIC_DELEGATE_DEPTH", depth.to_string()), ("ATIC_ROOT", root)]
}

/// `atic-mcp --delegate` en los dos formatos: Claude (un mapa por nombre, `env` como
/// objeto) y ACP (una lista, `env` como pares).
fn server_specs(exe: &Path, env: &[(&'static str, String)]) -> (Value, Value) {
    let as_map: serde_json::Map<String, Value> = env.iter().map(|(name, value)| (name.to_string(), json!(value))).collect();
    let as_pairs: Vec<Value> = env.iter().map(|(name, value)| json!({ "name": name, "value": value })).collect();
    let claude = json!({ SERVER: { "type": "stdio", "command": exe, "args": ["--delegate"], "env": as_map } });
    let acp = json!([{ "name": SERVER, "command": exe, "args": ["--delegate"], "env": as_pairs }]);
    (claude, acp)
}

/// El servidor para delegar de una conversación, si `atic-mcp` está instalado.
pub fn mcp_servers(key: &str, delegation: Option<&Delegation>) -> Option<(Value, Value)> {
    let exe = crate::agents_settings::mcp_binary()?;
    Some(server_specs(&exe, &mcp_env(key, delegation)))
}

/// Suma el servidor a los `mcpServers` de Claude y lo deja pasar sin preguntar.
pub fn add_claude_mcp(params: &mut Value, servers: &Value) {
    let mut merged = params.get("mcpServers").and_then(Value::as_object).cloned().unwrap_or_default();
    for (name, spec) in servers.as_object().into_iter().flatten() {
        merged.insert(name.clone(), spec.clone());
    }
    params["mcpServers"] = Value::Object(merged);
    let mut allowed = params.get("allowedTools").and_then(Value::as_array).cloned().unwrap_or_default();
    let tool = json!(format!("mcp__{SERVER}"));
    if !allowed.contains(&tool) {
        allowed.push(tool);
    }
    params["allowedTools"] = Value::Array(allowed);
}

/// Las primeras palabras de un encargo, para el título del hijo.
pub(super) fn first_words(text: &str) -> String {
    const WORDS: usize = 6;
    const CHARS: usize = 48;
    let words: Vec<&str> = text.split_whitespace().take(WORDS).collect();
    let mut title = words.join(" ");
    if title.chars().count() > CHARS {
        title = title.chars().take(CHARS).collect::<String>() + "…";
    } else if text.split_whitespace().count() > WORDS {
        title.push('…');
    }
    title
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect::<String>() + "…"
    }
}

/// ¿`from` es el padre de `key`, o el padre de su padre…? `parent_of` da el padre de cada
/// conversación delegada.
fn is_ancestor(from: &str, key: &str, parent_of: impl Fn(&str) -> Option<String>) -> bool {
    let mut current = parent_of(key);
    for _ in 0..8 {
        let Some(parent) = current else { return false };
        if parent == from {
            return true;
        }
        current = parent_of(&parent);
    }
    false
}

/// La respuesta de cada pedido sobre un hijo (`Outcome` de `atic-mcp`).
/// El orden de un modelo de OpenCode para un subagente: Go, Zen gratis, claves propias, Zen de pago.
fn opencode_rank(id: &str) -> u8 {
    match id.split_once('/').map_or("", |(p, _)| p) {
        "opencode-go" => 0,
        "opencode" if id.ends_with("-free") => 1,
        "opencode" => 3,
        _ => 2,
    }
}

/// El proveedor de un modelo de OpenCode y qué pide para responder.
fn opencode_needs(id: &str) -> (&str, &'static str) {
    let provider = id.split_once('/').map_or("", |(p, _)| p);
    let needs = match provider {
        "opencode-go" => "opencode-go-subscription",
        "opencode" if id.ends_with("-free") => "free",
        "opencode" => "opencode-zen-balance",
        _ => "provider-key",
    };
    (provider, needs)
}

/// El modelo pedido para un hijo de OpenCode, con su proveedor: `opencode/X` (Zen de pago)
/// pasa a `opencode-go/X` si está, y `X` sin proveedor toma el mejor que lo tenga.
fn prefer_subscription(model: &str, available: &[&str]) -> String {
    match model.split_once('/') {
        Some(("opencode", rest)) if !model.ends_with("-free") => {
            let go = format!("opencode-go/{rest}");
            if available.contains(&go.as_str()) { go } else { model.to_string() }
        }
        Some(_) => model.to_string(),
        None => available
            .iter()
            .filter(|id| id.split_once('/').is_some_and(|(_, m)| m.eq_ignore_ascii_case(model)))
            .min_by_key(|id| opencode_rank(id))
            .map_or(model.to_string(), |id| id.to_string()),
    }
}

fn outcome(session: &str, backend: &str, status: &str, text: &str, hint: Option<&str>, elapsed: Duration, permissions: Vec<Value>) -> Value {
    let mut out = json!({ "session": session, "backend": backend, "status": status, "text": text, "hint": hint, "elapsed_s": elapsed.as_secs() });
    if !permissions.is_empty() {
        out["permissions"] = Value::Array(permissions);
    }
    out
}

/// Los permisos que el hijo tiene esperando, como los lee `atic-mcp`.
fn pending_permissions(chat: &Chat) -> Vec<Value> {
    chat.permissions
        .iter()
        .map(|p| {
            let description = p.description.clone().or_else(|| p.title.clone()).unwrap_or_default();
            json!({ "id": p.request_id, "tool": p.tool, "description": description, "input": clip_chars(&p.input.to_string(), MAX_INPUT) })
        })
        .collect()
}

fn wait_for(seconds: Option<u64>) -> u64 {
    seconds.unwrap_or(hub::WAIT_DEFAULT).clamp(1, hub::WAIT_MAX)
}

fn bad_request(message: impl Into<String>) -> HubFault {
    HubFault::new("bad_request", message)
}

fn parse<T: for<'de> Deserialize<'de>>(body: Value) -> Result<T, HubFault> {
    serde_json::from_value(body).map_err(|e| bad_request(format!("el cuerpo no tiene la forma esperada: {e}")))
}

fn blurb(backend: &str) -> &'static str {
    match backend {
        "claude" => "Claude Code (Anthropic): el agente por defecto de Atic Code.",
        "codex" => "Codex (OpenAI), por su app-server.",
        "opencode" => "OpenCode: agente abierto con modelos de varios proveedores.",
        "cursor" => "Cursor Agent: el agente de Cursor por línea de comandos.",
        "grok" => "Grok Agent (xAI).",
        _ => "",
    }
}

// --- Cuerpos de los pedidos -------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct DelegateBody {
    backend: Option<String>,
    cwd: Option<String>,
    text: String,
    permission_mode: Option<String>,
    model: Option<String>,
    label: Option<String>,
    parent: Option<String>,
    depth: u64,
    root: Option<String>,
    wait_s: Option<u64>,
    mode: Option<String>,
    role: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PromptBody {
    session: String,
    text: String,
    from: Option<String>,
    wait_s: Option<u64>,
    mode: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WaitBody {
    session: String,
    wait_s: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SessionBody {
    session: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PermissionBody {
    session: String,
    permission_id: Option<String>,
    decision: String,
    from: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RenameBody {
    session: String,
    label: String,
}

// --- Atic Code ---------------------------------------------------------------------------

impl CodeView {
    /// Enciende el hub y atiende sus pedidos en el hilo de la ventana.
    pub(super) fn start_hub(&mut self, cx: &mut Context<Self>) {
        let Some((server, mut calls)) = hub::start() else {
            return;
        };
        self.delegate_hub = Some(server);
        cx.spawn(async move |this, cx| {
            while let Some(call) = calls.next().await {
                if this.update(cx, |view, cx| view.hub_call(call, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn hub_call(&mut self, call: HubCall, cx: &mut Context<Self>) {
        let HubCall { method, path, query, body, reply } = call;
        let answer = match path.as_str() {
            "/v1/agents" => Some(Ok(self.hub_agents())),
            "/v1/models" => Some(self.hub_models(query.get("backend").map(String::as_str).unwrap_or("claude"))),
            "/v1/sessions" => Some(Ok(self.hub_sessions(query.get("backend").map(String::as_str)))),
            "/v1/delegate" => {
                self.hub_delegate(body, reply, cx);
                cx.notify();
                return;
            }
            "/v1/prompt" => {
                self.hub_prompt(body, reply, cx);
                cx.notify();
                return;
            }
            "/v1/wait" => {
                self.hub_wait(body, reply, cx);
                return;
            }
            "/v1/cancel" => Some(self.hub_cancel(body, cx)),
            "/v1/close" => Some(self.hub_close(body, cx)),
            "/v1/permission" => Some(self.hub_permission(body, cx)),
            "/v1/spawn" => Some(self.hub_spawn(body)),
            "/v1/rename" => Some(self.hub_rename(body)),
            _ => None,
        };
        let answer = answer.unwrap_or_else(|| Err(HubFault::new("not_found", format!("no existe la ruta {method} {path}"))));
        let _ = reply.send(answer);
        cx.notify();
    }

    fn hub_agents(&self) -> Value {
        let mut claude = json!({
            "id": "claude",
            "name": "Claude",
            "available": self.setup.is_none_or(|s| s.node && s.claude),
            "blurb": blurb("claude"),
        });
        if let Some(setup) = self.setup {
            claude["signedIn"] = json!(setup.signed_in);
        }
        let mut agents = vec![claude];
        for agent in &acp::AGENTS {
            let id = backend_of(Some(agent.cli));
            let mut entry = json!({ "id": id, "name": agent.name, "available": self.acp_installed.contains(&agent.cli), "blurb": blurb(id) });
            // Con qué cuentas responde: el orquestador elige el modelo según esto.
            if agent.cli == "opencode" {
                entry["subscriptions"] = super::subscriptions::to_json(&super::subscriptions::opencode());
            }
            agents.push(entry);
        }
        json!({ "agents": agents })
    }

    /// Los modelos de un agente que ya se conocen (de Claude Code o de la sesión de prueba
    /// del agente ACP); vacío si todavía no se leyeron.
    fn hub_models(&self, backend: &str) -> HubReply {
        let cli = backend_cli(backend).ok_or_else(|| HubFault::new("unknown_backend", format!("no conozco el agente «{backend}»")))?;
        let models: Vec<Value> = match cli {
            None => self.models.iter().filter(|(id, _)| !id.is_empty()).map(|(id, name)| json!({ "id": id, "name": name })).collect(),
            Some(cli) => {
                let mut ids: Vec<(String, String)> = self
                    .acp_drafts
                    .get(cli)
                    .into_iter()
                    .flat_map(|draft| draft.options.iter().filter(|o| o.category == "model"))
                    .flat_map(|option| option.choices.iter())
                    .map(|choice| (choice.value.clone(), choice.name.clone()))
                    .collect();
                // OpenCode: lo de la suscripción primero (Go), después lo gratis de Zen, las
                // claves propias y al final lo de Zen que cobra saldo. El agente que delega
                // toma el primero que calce: con Zen primero elegía uno sin saldo y fallaba.
                // Y se revisan las suscripciones del usuario: lo que no responde (Go sin cupo,
                // Zen sin saldo) va al final y marcado.
                let subs = if cli == "opencode" { super::subscriptions::opencode() } else { Vec::new() };
                if cli == "opencode" {
                    ids.sort_by_key(|(id, _)| (!super::subscriptions::model_usable(id, &subs), opencode_rank(id)));
                }
                let models: Vec<Value> = ids
                    .iter()
                    .map(|(id, name)| {
                        let mut model = json!({ "id": id, "name": name });
                        if cli == "opencode" {
                            let (provider, needs) = opencode_needs(id);
                            model["provider"] = json!(provider);
                            model["requires"] = json!(needs);
                            model["available"] = json!(super::subscriptions::model_usable(id, &subs));
                        }
                        model
                    })
                    .collect();
                if cli == "opencode" {
                    return Ok(json!({ "models": models, "subscriptions": super::subscriptions::to_json(&subs) }));
                }
                models
            }
        };
        Ok(json!({ "models": models }))
    }

    fn hub_sessions(&self, backend: Option<&str>) -> Value {
        let sessions: Vec<Value> = self
            .chats
            .iter()
            .filter(|chat| backend.is_none_or(|b| b == backend_of(chat.acp)))
            .map(|chat| {
                let mut session = json!({ "session": chat.key, "backend": backend_of(chat.acp), "cwd": chat.cwd, "running": chat.busy, "label": chat.title });
                if let Some(delegation) = &chat.delegation {
                    session["parent"] = json!(delegation.parent);
                    session["role"] = json!(delegation.role);
                }
                session
            })
            .collect();
        json!({ "sessions": sessions })
    }

    /// Una conversación delegada (o error `not_found`).
    fn delegated_mut(&mut self, key: &str) -> Result<&mut Chat, HubFault> {
        self.chats
            .iter_mut()
            .find(|c| c.key == key && c.delegation.is_some())
            .ok_or_else(|| HubFault::new("not_found", format!("no hay una tarea delegada «{key}»")))
    }

    fn parent_of(&self, key: &str) -> Option<String> {
        self.chats.iter().find(|c| c.key == key).and_then(|c| c.delegation.as_ref()).map(|d| d.parent.clone())
    }

    /// Crea la conversación hija (sin mandarle nada todavía). Va en el espacio y la
    /// carpeta del padre; no cambia la conversación a la vista.
    fn create_child(&mut self, request: &DelegateBody) -> Result<String, HubFault> {
        if request.depth >= MAX_DEPTH {
            return Err(HubFault::new("depth_exceeded", "un subagente de un subagente no puede delegar más: haz la tarea tú mismo"));
        }
        let parent_key = request.parent.clone().filter(|p| !p.trim().is_empty());
        let parent = parent_key.as_deref().and_then(|key| self.chats.iter().find(|c| c.key == key));
        let (workspace, mut cwd, depth, root, parent_cli, external) = match parent {
            Some(chat) => {
                let own = chat.delegation.as_ref();
                let depth = own.map_or(0, |d| d.depth as u64).max(request.depth);
                let root = own.map_or_else(|| chat.key.clone(), |d| d.root.clone());
                (chat.workspace, chat.cwd.clone(), depth, root, chat.acp, false)
            }
            // Pedido desde una CLI suelta: en el proyecto abierto (o en los chats sueltos).
            None => {
                let place = self.workspaces.active().and_then(|w| w.main().cloned().map(|cwd| (w.id, cwd))).or_else(|| loose_dir().map(|dir| (LOOSE, dir)));
                let Some((workspace, cwd)) = place else {
                    return Err(bad_request("Atic Code no tiene una carpeta donde abrir la tarea: abre un proyecto"));
                };
                let root = request.root.clone().or_else(|| parent_key.clone()).unwrap_or_else(|| EXTERNAL.to_string());
                (workspace, cwd, request.depth, root, None, true)
            }
        };
        if depth >= MAX_DEPTH {
            return Err(HubFault::new("depth_exceeded", "un subagente de un subagente no puede delegar más: haz la tarea tú mismo"));
        }
        let parent_id = parent_key.unwrap_or_else(|| EXTERNAL.to_string());
        let working = self.chats.iter().filter(|c| c.busy && c.delegation.as_ref().is_some_and(|d| d.parent == parent_id)).count();
        if working >= MAX_CHILDREN {
            return Err(HubFault::new("too_many_children", format!("ya tienes {working} subagentes trabajando: espera sus resultados o corta alguno con atic_cancel")));
        }
        let cli = match request.backend.as_deref().map(str::trim).filter(|b| !b.is_empty() && *b != "auto") {
            None => parent_cli,
            Some(backend) => backend_cli(backend).ok_or_else(|| {
                let mut fault = HubFault::new("unknown_backend", format!("no conozco el agente «{backend}»; mira atic_list_agents"));
                fault.data = Some(json!({ "known": ["claude", "codex", "cursor", "opencode", "grok"] }));
                fault
            })?,
        };
        let available = match cli {
            None => self.setup.is_none_or(|s| s.node && s.claude),
            Some(cli) => self.acp_installed.contains(&cli),
        };
        if !available {
            return Err(HubFault::new("backend_unavailable", format!("{} no está instalado o no está listo en esta máquina", backend_of(cli))));
        }
        if let Some(dir) = request.cwd.as_deref().map(PathBuf::from).filter(|dir| dir.is_dir()) {
            cwd = dir;
        }
        let model = request.model.clone().filter(|m| !m.trim().is_empty());
        let key = self.new_key();
        let mut chat = Chat::new(key.clone(), workspace, cwd);
        chat.acp = cli;
        match cli {
            None => chat.want_model = model,
            Some(cli) => {
                let mut config = self.acp_drafts.get(cli).cloned().unwrap_or_default();
                if let Some(model) = model {
                    let found = config.options.iter().find(|o| o.category == "model").map(|o| (o.id.clone(), o.choices.clone()));
                    // OpenCode: el mismo modelo por Go en vez de por Zen (que cobra saldo), o con
                    // su proveedor si vino sin él.
                    let model = match (cli, &found) {
                        ("opencode", Some((_, choices))) => {
                            let ids: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
                            let model = prefer_subscription(&model, &ids);
                            // Antes de levantarlo, que su cuenta responda: si no, el mismo modelo
                            // por otra que sí, o se le dice al orquestador qué hay.
                            let subs = super::subscriptions::opencode();
                            if super::subscriptions::model_usable(&model, &subs) {
                                model
                            } else {
                                let name = model.split_once('/').map_or(model.as_str(), |(_, m)| m);
                                let other = ids
                                    .iter()
                                    .filter(|id| id.split_once('/').is_some_and(|(_, m)| m == name) && super::subscriptions::model_usable(id, &subs))
                                    .min_by_key(|id| opencode_rank(id));
                                match other {
                                    Some(other) => other.to_string(),
                                    None => {
                                        let mut fault = bad_request(format!(
                                            "la cuenta de «{model}» no responde ahora; elige un modelo con available=true en atic_list_models"
                                        ));
                                        fault.data = Some(json!({ "subscriptions": super::subscriptions::to_json(&subs) }));
                                        return Err(fault);
                                    }
                                }
                            }
                        }
                        _ => model,
                    };
                    match found {
                        Some((_, choices)) if !choices.is_empty() && !choices.iter().any(|c| c.value == model || c.name.eq_ignore_ascii_case(&model)) => {
                            let mut fault = bad_request(format!("{} no tiene el modelo «{model}»; mira atic_list_models", backend_of(Some(cli))));
                            fault.data = Some(json!({ "models": choices.iter().map(|c| c.value.clone()).collect::<Vec<_>>() }));
                            return Err(fault);
                        }
                        Some((id, choices)) => {
                            let value = choices.iter().find(|c| c.name.eq_ignore_ascii_case(&model)).map_or(model.clone(), |c| c.value.clone());
                            config.set(&id, &value);
                        }
                        // Sin la lista del agente todavía: se pide igual al abrir la sesión.
                        None => config.options.push(AcpOption { id: "model".into(), name: "Modelo".into(), category: "model".into(), current: model, choices: Vec::new() }),
                    }
                }
                chat.acp_cfg = config;
            }
        }
        let permission_mode = cli.is_none().then(|| capped_mode(request.permission_mode.as_deref(), &self.configs.get(workspace).permission_mode));
        chat.title = match request.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            Some(label) => label.to_string(),
            None => tf("pill.code.delegate.title", &[("words", &first_words(&request.text))]),
        };
        chat.delegation = Some(Delegation {
            parent: parent_id,
            root,
            depth: (depth + 1) as u8,
            role: request.role.clone(),
            backend: backend_of(cli).to_string(),
            permission_mode,
            external,
            started: Instant::now(),
            wake: false,
            waiters: Vec::new(),
            last: None,
        });
        self.chats.push(chat);
        Ok(key)
    }

    /// Le manda un turno al hijo: desde ahora su resultado despierta al padre.
    fn start_child_turn(&mut self, key: &str, text: String, cx: &mut Context<Self>) {
        let mut title = None;
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            title = Some(chat.title.clone());
            if let Some(delegation) = chat.delegation.as_mut() {
                delegation.started = Instant::now();
                delegation.wake = !delegation.external;
                delegation.last = None;
            }
        }
        self.dispatch_message(key.to_string(), text, Vec::new(), Vec::new(), cx);
        // `push_user` le pone de título el primer mensaje: queda el del subagente.
        if let (Some(chat), Some(title)) = (self.chats.iter_mut().find(|c| c.key == key), title) {
            chat.title = title;
        }
        // Si la pill se reinicia en este turno, al volver sabe a quién despertar.
        self.sync_tag(key);
    }

    /// Contesta un `delegate` o `prompt` recién lanzado: al tiro (`running`) o al terminar.
    fn answer_started(&mut self, key: &str, wait: Option<u64>, reply: mpsc::Sender<HubReply>, cx: &mut Context<Self>) {
        match wait {
            Some(seconds) => self.add_waiter(key, reply, seconds, cx),
            None => {
                let backend = self.chats.iter().find(|c| c.key == key).and_then(|c| c.delegation.as_ref()).map(|d| d.backend.clone()).unwrap_or_default();
                let _ = reply.send(Ok(outcome(key, &backend, "running", "", Some(HINT_ASYNC), Duration::ZERO, Vec::new())));
            }
        }
    }

    fn hub_delegate(&mut self, body: Value, reply: mpsc::Sender<HubReply>, cx: &mut Context<Self>) {
        let request: DelegateBody = match parse(body) {
            Ok(request) => request,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        if request.text.trim().is_empty() {
            let _ = reply.send(Err(bad_request("falta `text`: el encargo para el subagente")));
            return;
        }
        let key = match self.create_child(&request) {
            Ok(key) => key,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let external = self.chats.iter().find(|c| c.key == key).and_then(|c| c.delegation.as_ref()).is_some_and(|d| d.external);
        // Sin una conversación padre no hay a quién despertar: se espera sí o sí.
        let wait = (external || request.mode.as_deref() == Some("wait")).then(|| wait_for(request.wait_s));
        tracing::info!(key = %key, backend = ?request.backend, "atic code: tarea delegada");
        self.start_child_turn(&key, role_prefix(request.role.as_deref(), &request.text), cx);
        self.answer_started(&key, wait, reply, cx);
    }

    /// Otro turno a un hijo que ya existe (solo su padre o un ancestro).
    fn hub_prompt(&mut self, body: Value, reply: mpsc::Sender<HubReply>, cx: &mut Context<Self>) {
        let request: PromptBody = match parse(body) {
            Ok(request) => request,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let external = match self.check_prompt(&request) {
            Ok(external) => external,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let wait = (external || request.mode.as_deref() == Some("wait")).then(|| wait_for(request.wait_s));
        self.start_child_turn(&request.session, request.text, cx);
        self.answer_started(&request.session, wait, reply, cx);
    }

    /// Se le puede mandar otro turno al hijo: existe, está libre y `from` es su padre o un
    /// ancestro. Devuelve si su padre es externo.
    fn check_prompt(&mut self, request: &PromptBody) -> Result<bool, HubFault> {
        if request.text.trim().is_empty() {
            return Err(bad_request("falta `text`"));
        }
        let chat = self.delegated_mut(&request.session)?;
        let external = chat.delegation.as_ref().is_some_and(|d| d.external);
        if chat.busy {
            return Err(HubFault::new("busy", "el subagente sigue trabajando: espera su resultado o córtalo con atic_cancel"));
        }
        let from = request.from.as_deref().unwrap_or_default();
        if from.is_empty() || !is_ancestor(from, &request.session, |key| self.parent_of(key)) {
            return Err(HubFault::new("not_your_child", "esa tarea no la delegaste tú"));
        }
        Ok(external)
    }

    fn hub_wait(&mut self, body: Value, reply: mpsc::Sender<HubReply>, cx: &mut Context<Self>) {
        let request: WaitBody = match parse(body) {
            Ok(request) => request,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let chat = match self.delegated_mut(&request.session) {
            Ok(chat) => chat,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let Some(delegation) = chat.delegation.as_ref() else { return };
        let elapsed = delegation.started.elapsed();
        if !chat.permissions.is_empty() {
            let hint = if delegation.external { HINT_PERMISSION_EXTERNAL } else { HINT_PERMISSION };
            let _ = reply.send(Ok(outcome(&chat.key, &delegation.backend, "permission_required", "", Some(hint), elapsed, pending_permissions(chat))));
            return;
        }
        if chat.busy {
            self.add_waiter(&request.session, reply, wait_for(request.wait_s), cx);
            return;
        }
        let done = delegation.last.clone().unwrap_or_else(|| outcome(&chat.key, &delegation.backend, "done", &clip_reply(&chat.last_reply()), None, elapsed, Vec::new()));
        let _ = reply.send(Ok(done));
    }

    /// Espera el turno en curso del hijo hasta `seconds`; al vencer contesta `timeout`
    /// (el hijo sigue trabajando).
    fn add_waiter(&mut self, key: &str, reply: mpsc::Sender<HubReply>, seconds: u64, cx: &mut Context<Self>) {
        let id = NEXT_WAITER.fetch_add(1, Ordering::Relaxed);
        let Some(delegation) = self.chats.iter_mut().find(|c| c.key == key).and_then(|c| c.delegation.as_mut()) else {
            let _ = reply.send(Err(HubFault::new("not_found", format!("no hay una tarea delegada «{key}»"))));
            return;
        };
        delegation.waiters.push((id, reply));
        let key = key.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(seconds)).await;
            let _ = this.update(cx, |view, _| view.waiter_expired(&key, id));
        })
        .detach();
    }

    fn waiter_expired(&mut self, key: &str, id: u64) {
        let Some(delegation) = self.chats.iter_mut().find(|c| c.key == key).and_then(|c| c.delegation.as_mut()) else {
            return;
        };
        let Some(at) = delegation.waiters.iter().position(|(waiter, _)| *waiter == id) else {
            return;
        };
        let (_, reply) = delegation.waiters.remove(at);
        let hint = if delegation.external { HINT_TIMEOUT_EXTERNAL } else { HINT_TIMEOUT };
        let _ = reply.send(Ok(outcome(key, &delegation.backend, "timeout", "", Some(hint), delegation.started.elapsed(), Vec::new())));
    }

    /// Detiene el turno del hijo; su resultado ya no despierta al padre.
    fn hub_cancel(&mut self, body: Value, cx: &mut Context<Self>) -> HubReply {
        let request: SessionBody = parse(body)?;
        let chat = self.delegated_mut(&request.session)?;
        if let Some(delegation) = chat.delegation.as_mut() {
            delegation.wake = false;
        }
        // Los que esperan reciben `cancelled` al llegar el resultado.
        self.interrupt_chat(&request.session, cx);
        self.sync_tag(&request.session);
        Ok(json!({ "session": request.session, "status": "cancel_requested" }))
    }

    /// Cierra el proceso del hijo; queda en la lista para leerlo o retomarlo.
    fn hub_close(&mut self, body: Value, cx: &mut Context<Self>) -> HubReply {
        let request: SessionBody = parse(body)?;
        let chat = self.delegated_mut(&request.session)?;
        let live = chat.live;
        let Some(delegation) = chat.delegation.as_mut() else {
            return Err(HubFault::new("not_found", "no hay una tarea delegada con esa clave"));
        };
        delegation.wake = false;
        let waiters = std::mem::take(&mut delegation.waiters);
        let (backend, elapsed) = (delegation.backend.clone(), delegation.started.elapsed());
        for (_, waiter) in waiters {
            let _ = waiter.send(Ok(outcome(&request.session, &backend, "cancelled", "", None, elapsed, Vec::new())));
        }
        if live {
            self.retire(&request.session, cx);
        }
        Ok(json!({ "session": request.session, "status": "closed" }))
    }

    /// Contesta un permiso del hijo en nombre del usuario (solo su padre o un ancestro).
    fn hub_permission(&mut self, body: Value, cx: &mut Context<Self>) -> HubReply {
        let request: PermissionBody = parse(body)?;
        let from = request.from.as_deref().unwrap_or_default();
        self.delegated_mut(&request.session)?;
        if from.is_empty() || !is_ancestor(from, &request.session, |key| self.parent_of(key)) {
            return Err(HubFault::new("not_your_child", "esa tarea no la delegaste tú"));
        }
        let chat = self.delegated_mut(&request.session)?;
        let index = match request.permission_id.as_deref() {
            Some(id) => chat.permissions.iter().position(|p| p.request_id == id),
            None => (!chat.permissions.is_empty()).then_some(0),
        }
        .ok_or_else(|| HubFault::new("not_found", "el subagente no tiene ese permiso esperando"))?;
        let permission = chat.permissions.remove(index);
        let result = match request.decision.as_str() {
            "allow" => json!({ "behavior": "allow", "updatedInput": permission.input }),
            "allow_always" => json!({ "behavior": "allow", "updatedInput": permission.input, "updatedPermissions": permission.suggestions.clone().unwrap_or(json!([])) }),
            "deny" => json!({ "behavior": "deny", "message": DENIED }),
            other => {
                chat.permissions.insert(index, permission);
                return Err(bad_request(format!("`decision` es allow, allow_always o deny (llegó «{other}»)")));
            }
        };
        self.asks.remove(&permission.request_id);
        self.fire("permission", json!({ "key": request.session, "requestId": permission.request_id, "result": result }), cx);
        Ok(json!({ "ok": true }))
    }

    /// Crea el hijo sin mandarle nada; el primer turno va con `atic_prompt`.
    fn hub_spawn(&mut self, body: Value) -> HubReply {
        let request: DelegateBody = parse(body)?;
        let key = self.create_child(&request)?;
        Ok(json!({ "session": key }))
    }

    fn hub_rename(&mut self, body: Value) -> HubReply {
        let request: RenameBody = parse(body)?;
        let label = request.label.trim().to_string();
        if label.is_empty() {
            return Err(bad_request("falta `label`"));
        }
        self.delegated_mut(&request.session)?.title = label;
        Ok(json!({ "session": request.session, "label": request.label.trim() }))
    }

    /// Terminó el turno de una conversación: si es un hijo, su resultado va a quien lo
    /// espera o despierta al padre.
    pub(super) fn delegate_finished(&mut self, key: &str, ended: Ended, error: Option<String>, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key && c.delegation.is_some()) else {
            return;
        };
        let mut reply = clip_reply(&chat.last_reply());
        let title = chat.title.clone();
        let Some(delegation) = chat.delegation.as_mut() else {
            return;
        };
        // Sin respuesta y con un error: el padre necesita saber por qué (un modelo sin saldo,
        // una suscripción vencida…), no solo que falló.
        if reply.trim().is_empty() {
            let reason = error.or_else(|| (delegation.backend == "opencode").then(super::acp::opencode_reason).flatten());
            if let Some(reason) = reason {
                reply = format!("Error: {reason}");
            }
        }
        let result = outcome(key, &delegation.backend, ended.as_str(), &reply, None, delegation.started.elapsed(), Vec::new());
        delegation.last = Some(result.clone());
        let mut delivered = false;
        for (_, waiter) in delegation.waiters.drain(..) {
            delivered |= waiter.send(Ok(result.clone())).is_ok();
        }
        let wake = std::mem::take(&mut delegation.wake);
        if delivered || !wake {
            return;
        }
        let message = wake_message(key, &title, &delegation.backend, delegation.role.as_deref(), ended.as_str(), delegation.started.elapsed(), &reply);
        let parent = delegation.parent.clone();
        self.wake_parent(&parent, message, cx);
    }

    /// El proceso de un hijo se cerró a mitad de turno: lo que esperaba recibe `failed`.
    pub(super) fn delegate_lost(&mut self, key: &str, cx: &mut Context<Self>) {
        let pending = self.chats.iter().any(|c| c.key == key && c.delegation.as_ref().is_some_and(Delegation::pending));
        if pending {
            self.delegate_finished(key, Ended::Failed, None, cx);
        }
    }

    /// Las conversaciones delegadas con alguien esperando su turno.
    pub(super) fn delegations_pending(&self) -> Vec<String> {
        self.chats.iter().filter(|c| c.delegation.as_ref().is_some_and(Delegation::pending)).map(|c| c.key.clone()).collect()
    }

    /// El resultado de un hijo entra a la conversación padre como mensaje del usuario:
    /// dentro del turno en curso si se puede, a la cola si no, o como turno nuevo
    /// (que retoma la sesión si estaba cerrada).
    fn wake_parent(&mut self, parent: &str, message: String, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == parent) else {
            return;
        };
        if chat.busy && !chat.can_steer() {
            chat.enqueue(message, Vec::new(), Vec::new());
            return;
        }
        self.dispatch_message(parent.to_string(), message, Vec::new(), Vec::new(), cx);
    }

    /// Un hijo pide un permiso: lo saben quienes esperan su resultado. El usuario lo ve en
    /// la tarjeta del hijo en la conversación padre («pide permiso: …» con «Abrir para
    /// contestar»), que lee los permisos del hijo, y en su fila de la barra.
    pub(super) fn delegate_permission(&mut self, key: &str) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key && c.delegation.is_some()) else {
            return;
        };
        let permissions = pending_permissions(chat);
        let Some(delegation) = chat.delegation.as_mut() else {
            return;
        };
        let hint = if delegation.external { HINT_PERMISSION_EXTERNAL } else { HINT_PERMISSION };
        let result = outcome(key, &delegation.backend, "permission_required", "", Some(hint), delegation.started.elapsed(), permissions);
        for (_, waiter) in delegation.waiters.drain(..) {
            let _ = waiter.send(Ok(result.clone()));
        }
    }

    /// Se detuvo `parent`: sus hijos que trabajan también se detienen, y ninguno lo
    /// despierta después (el usuario cortó ese turno). Sigue con los nietos.
    pub(super) fn interrupt_children(&mut self, parent: &str, cx: &mut Context<Self>) {
        let working: Vec<String> = self
            .chats
            .iter_mut()
            .filter_map(|chat| {
                let delegation = chat.delegation.as_mut().filter(|d| d.parent == parent)?;
                delegation.wake = false;
                (chat.busy && chat.live).then(|| chat.key.clone())
            })
            .collect();
        for child in working {
            self.interrupt_chat(&child, cx);
        }
    }

    /// El hijo `session` de la conversación `parent`, si sigue abierto. Las claves (`c2`) son
    /// de esta sesión de Atic: por eso se pide que su padre sea `parent`, y no se confunde
    /// con otra conversación que reciba la misma clave después de reabrir Atic.
    pub(super) fn child_of(&self, parent: &str, session: &str) -> Option<&Chat> {
        self.chats.iter().find(|c| c.key == session && c.delegation.as_ref().is_some_and(|d| d.parent == parent))
    }

    /// Los hijos de una conversación que siguen abiertos, en el orden en que se crearon.
    pub(super) fn children_of(&self, parent: &str) -> Vec<&Chat> {
        self.chats.iter().filter(|c| c.delegation.as_ref().is_some_and(|d| d.parent == parent)).collect()
    }

    /// El estado de un hijo para su tarjeta (`Gone` si ya no está).
    pub(super) fn child_state_of(&self, child: Option<&Chat>) -> ChildState {
        let Some(chat) = child else {
            return ChildState::Gone;
        };
        let permission = chat.permissions.first().map(permission_tool);
        let last = chat.delegation.as_ref().and_then(|d| d.last.as_ref()).and_then(|v| v.get("status")).and_then(Value::as_str);
        child_state(chat.busy, permission.as_deref(), last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_rol_va_delante_del_encargo_en_espanol() {
        assert_eq!(role_prefix(Some("review"), "Revisa el módulo"), "Actúa como el subagente de revisión para esta tarea.\n\nRevisa el módulo");
        assert_eq!(role_prefix(Some("implementation"), "x"), "Actúa como el subagente de implementación para esta tarea.\n\nx");
        assert!(role_prefix(Some("research"), "x").contains("de investigación"));
        assert!(role_prefix(Some("Design"), "x").contains("de diseño"));
        assert!(role_prefix(Some("tests"), "x").contains("de pruebas"));
        // Sin rol, o general: solo el encargo.
        assert_eq!(role_prefix(None, "solo esto"), "solo esto");
        assert_eq!(role_prefix(Some("general"), "solo esto"), "solo esto");
    }

    #[test]
    fn el_hijo_no_tiene_mas_permisos_que_el_padre() {
        assert_eq!(capped_mode(Some("plan"), "acceptEdits"), "plan");
        assert_eq!(capped_mode(Some("acceptEdits"), "acceptEdits"), "acceptEdits");
        assert_eq!(capped_mode(Some("bypassPermissions"), "default"), "default");
        assert_eq!(capped_mode(Some("acceptEdits"), "plan"), "plan");
        // Sin pedir nada, o con un modo que no existe: el del padre.
        assert_eq!(capped_mode(None, "acceptEdits"), "acceptEdits");
        assert_eq!(capped_mode(Some("yolo"), "default"), "default");
    }

    #[test]
    fn la_respuesta_larga_se_recorta_en_un_caracter() {
        assert_eq!(clip_reply("corto"), "corto");
        let long = "ñ".repeat(MAX_REPLY);
        let clipped = clip_reply(&long);
        assert!(clipped.ends_with(CLIPPED));
        let head = clipped.strip_suffix(CLIPPED).unwrap();
        assert!(head.len() <= MAX_REPLY);
        assert!(head.chars().all(|c| c == 'ñ'));
    }

    #[test]
    fn el_despertar_dice_quien_termino_y_como() {
        let message = wake_message("c7", "Subagente: revisa hub", "codex", Some("review"), "done", Duration::from_millis(1500), "Listo: dos hallazgos.");
        assert!(message.starts_with("<atic-subagent>\nEl subagente c7 (codex) terminó su turno con estado done."));
        assert!(message.contains("\n<session>c7</session>\n<title>Subagente: revisa hub</title>\n<agent>codex</agent>\n<role>review</role>\n<status>done</status>\n<duration_ms>1500</duration_ms>\n"));
        assert!(message.ends_with("<result>\nListo: dos hallazgos.\n</result>\n</atic-subagent>"));
        // Sin respuesta ni rol.
        let empty = wake_message("c7", "t", "claude", None, "failed", Duration::ZERO, "  ");
        assert!(!empty.contains("<role>"));
        assert!(empty.contains("<result>\n(no dejó respuesta)\n</result>"));
    }

    #[test]
    fn el_despertar_escapa_lo_que_cortaria_la_etiqueta() {
        let message = wake_message("c2", "a <b> & c", "codex", None, "done", Duration::ZERO, "usa Vec<String> && cierra </result> y </atic-subagent>");
        assert!(message.contains("<title>a &lt;b> &amp; c</title>"));
        // `Vec<String>` queda legible; los cierres no cortan la etiqueta.
        assert!(message.contains("usa Vec<String> &amp;&amp; cierra &lt;/result> y &lt;/atic-subagent>"));
        assert_eq!(message.matches("</atic-subagent>").count(), 1);
        assert_eq!(message.matches("</result>").count(), 1);
    }

    #[test]
    fn las_llamadas_de_atic_se_reconocen_por_su_nombre() {
        assert_eq!(DelegateTool::of("mcp__atic__atic_delegate"), Some(DelegateTool::Delegate));
        assert_eq!(DelegateTool::of("atic · atic_delegate"), Some(DelegateTool::Delegate));
        assert_eq!(DelegateTool::of("atic_prompt"), Some(DelegateTool::Prompt));
        assert_eq!(DelegateTool::of("mcp__atic__atic_wait"), None);
        assert_eq!(DelegateTool::of("Bash"), None);
    }

    #[test]
    fn el_hijo_sale_del_resultado_de_la_llamada() {
        assert_eq!(delegate_session(r#"{"session":"c2","backend":"codex","status":"running"}"#).as_deref(), Some("c2"));
        assert_eq!(delegate_session("{\n  \"session\": \"c9\"\n}\n\nmás texto").as_deref(), Some("c9"));
        assert_eq!(delegate_session("no conozco el agente «gemini»"), None);
        assert_eq!(delegate_session(r#"{"session":""}"#), None);
    }

    #[test]
    fn el_estado_del_hijo_para_su_tarjeta() {
        assert_eq!(child_state(true, Some("Bash"), None), ChildState::Permission("Bash".into()));
        assert_eq!(child_state(true, None, None), ChildState::Working);
        // Un turno nuevo en curso pesa más que cómo terminó el anterior.
        assert_eq!(child_state(true, None, Some("failed")), ChildState::Working);
        assert_eq!(child_state(false, None, Some("done")), ChildState::Done);
        assert_eq!(child_state(false, None, Some("failed")), ChildState::Failed);
        assert_eq!(child_state(false, None, Some("cancelled")), ChildState::Stopped);
        assert_eq!(child_state(false, None, None), ChildState::Idle);
    }

    #[test]
    fn el_nombre_y_el_logo_de_cada_agente() {
        assert_eq!(agent_label("codex"), "Codex");
        assert_eq!(agent_label("cursor"), "Cursor");
        assert_eq!(agent_label("claude"), "Claude");
        assert_eq!(agent_logo("claude"), Some("icons/agents/claude.svg"));
        assert_eq!(agent_logo("codex"), Some("icons/agents/openai.svg"));
        assert_eq!(agent_logo("cursor"), Some("icons/agents/cursor.svg"));
        assert_eq!(agent_logo("gemini"), None);
    }

    #[test]
    fn las_instrucciones_de_sistema_se_juntan() {
        assert_eq!(join_prompts(&[]), None);
        assert_eq!(join_prompts(&["".into(), "  ".into()]), None);
        assert_eq!(join_prompts(&["suelto".into(), "varias carpetas".into()]).as_deref(), Some("suelto\n\nvarias carpetas"));
        let joined = join_prompts(&["varias carpetas".into(), DELEGATION_GUIDE.into()]).unwrap();
        assert!(joined.starts_with("varias carpetas\n\n## Subagentes en Atic Code"));
    }

    #[test]
    fn las_instrucciones_van_delante_del_primer_mensaje_acp() {
        let text = with_guide("hola");
        assert!(text.starts_with("<instrucciones_de_atic>\n## Subagentes en Atic Code"));
        assert!(text.ends_with("<pedido_del_usuario>\nhola\n</pedido_del_usuario>"));
    }

    #[test]
    fn el_mcp_lleva_quien_llama_en_los_dos_formatos() {
        let env = mcp_env("c3", None);
        assert_eq!(env[0], ("ATIC_SESSION", "c3".to_string()));
        assert_eq!(env[1], ("ATIC_DELEGATE_DEPTH", "0".to_string()));
        assert_eq!(env[2], ("ATIC_ROOT", "c3".to_string()));
        let (claude, acp) = server_specs(Path::new("atic-mcp"), &env);
        assert_eq!(claude["atic"]["args"], json!(["--delegate"]));
        assert_eq!(claude["atic"]["env"]["ATIC_SESSION"], "c3");
        assert_eq!(acp[0]["name"], "atic");
        assert_eq!(acp[0]["env"][1], json!({ "name": "ATIC_DELEGATE_DEPTH", "value": "0" }));
    }

    #[test]
    fn el_mcp_se_suma_a_los_de_claude() {
        let mut params = json!({ "mcpServers": { "otro": { "command": "x" } }, "allowedTools": ["Read"] });
        let (claude, _) = server_specs(Path::new("atic-mcp"), &mcp_env("c1", None));
        add_claude_mcp(&mut params, &claude);
        assert!(params["mcpServers"]["otro"].is_object());
        assert!(params["mcpServers"]["atic"].is_object());
        assert_eq!(params["allowedTools"], json!(["Read", "mcp__atic"]));
        // Dos veces no lo repite.
        add_claude_mcp(&mut params, &claude);
        assert_eq!(params["allowedTools"], json!(["Read", "mcp__atic"]));
        let mut empty = json!({});
        add_claude_mcp(&mut empty, &claude);
        assert_eq!(empty["allowedTools"], json!(["mcp__atic"]));
    }

    #[test]
    fn el_ancestro_se_busca_por_la_cadena() {
        let parents = |key: &str| match key {
            "c3" => Some("c2".to_string()),
            "c2" => Some("c1".to_string()),
            _ => None,
        };
        assert!(is_ancestor("c2", "c3", parents));
        assert!(is_ancestor("c1", "c3", parents));
        assert!(!is_ancestor("c9", "c3", parents));
        assert!(!is_ancestor("c3", "c1", parents));
    }

    #[test]
    fn los_proveedores_por_su_id() {
        assert_eq!(backend_of(None), "claude");
        assert_eq!(backend_of(Some("cursor-agent")), "cursor");
        assert_eq!(backend_of(Some("codex")), "codex");
        assert_eq!(backend_cli("Claude"), Some(None));
        assert_eq!(backend_cli("cursor"), Some(Some("cursor-agent")));
        assert_eq!(backend_cli("opencode"), Some(Some("opencode")));
        assert_eq!(backend_cli("gemini"), None);
    }

    #[test]
    fn el_resultado_tiene_la_forma_de_atic_mcp() {
        let running = outcome("c2", "codex", "running", "", Some(HINT_ASYNC), Duration::from_millis(1500), Vec::new());
        assert_eq!(running["elapsed_s"], 1);
        assert_eq!(running["hint"], HINT_ASYNC);
        assert!(running.get("permissions").is_none());
        let asked = outcome("c2", "codex", "permission_required", "", None, Duration::ZERO, vec![json!({ "id": "p1" })]);
        assert!(asked["hint"].is_null());
        assert_eq!(asked["permissions"][0]["id"], "p1");
    }

    #[test]
    fn el_titulo_son_las_primeras_palabras() {
        assert_eq!(first_words("Revisa el hub"), "Revisa el hub");
        assert_eq!(first_words("uno dos tres cuatro cinco seis siete"), "uno dos tres cuatro cinco seis…");
    }

    #[test]
    fn el_fin_del_turno_segun_el_resultado() {
        assert_eq!(Ended::of(true, &json!({ "isError": true })), Ended::Cancelled);
        assert_eq!(Ended::of(false, &json!({ "isError": true })), Ended::Failed);
        assert_eq!(Ended::of(false, &json!({ "subtype": "success" })), Ended::Done);
    }

    #[test]
    fn un_hijo_de_opencode_va_por_la_suscripcion_y_no_por_zen_de_pago() {
        let available = ["opencode/deepseek-v4.1-flash", "opencode-go/deepseek-v4.1-flash", "opencode/big-pickle-free", "kimi-for-coding/k2"];
        assert_eq!(prefer_subscription("opencode/deepseek-v4.1-flash", &available), "opencode-go/deepseek-v4.1-flash");
        assert_eq!(prefer_subscription("deepseek-v4.1-flash", &available), "opencode-go/deepseek-v4.1-flash");
        assert_eq!(prefer_subscription("opencode/big-pickle-free", &available), "opencode/big-pickle-free");
        assert_eq!(prefer_subscription("kimi-for-coding/k2", &available), "kimi-for-coding/k2");
        let mut sorted = available.to_vec();
        sorted.sort_by_key(|id| opencode_rank(id));
        assert_eq!(sorted, ["opencode-go/deepseek-v4.1-flash", "opencode/big-pickle-free", "kimi-for-coding/k2", "opencode/deepseek-v4.1-flash"]);
        assert_eq!(opencode_needs("opencode/deepseek-v4.1-flash").1, "opencode-zen-balance");
    }

    #[test]
    fn la_delegacion_se_guarda_en_el_daemon_y_vuelve_igual() {
        let delegation = Delegation {
            parent: "c1-abc".into(),
            root: "c1-abc".into(),
            depth: 1,
            role: Some("reviewer".into()),
            backend: "opencode".into(),
            permission_mode: None,
            external: false,
            started: Instant::now() - Duration::from_secs(30),
            wake: true,
            waiters: Vec::new(),
            last: None,
        };
        let tag = delegation.to_tag();
        // Viaja como JSON (el daemon lo guarda tal cual).
        let tag: Value = serde_json::from_str(&tag.to_string()).unwrap();
        let back = Delegation::from_tag(&tag).unwrap();
        assert_eq!((back.parent.as_str(), back.root.as_str(), back.depth), ("c1-abc", "c1-abc", 1));
        assert_eq!((back.role.as_deref(), back.backend.as_str()), (Some("reviewer"), "opencode"));
        assert!(back.wake && !back.external && back.waiters.is_empty());
        // El turno sigue contando desde cuando empezó, no desde que volvió.
        assert!(back.started.elapsed() >= Duration::from_secs(29));
        assert!(Delegation::from_tag(&Value::Null).is_none());
        assert!(Delegation::from_tag(&json!({ "parent": "c1" })).is_none(), "sin backend no se entiende");
    }

    #[test]
    fn el_mensaje_se_rearma_sin_las_instrucciones() {
        assert_eq!(without_guide(&with_guide("revisa el parser")), "revisa el parser");
        assert_eq!(without_guide("hola"), "hola");
    }
}
