//! La pestaña «Asistente» del editor: un chat con un agente de IA (Claude, Codex, OpenCode,
//! Cursor, Grok…) que ve la grabación y la edita con las herramientas del editor
//! (`agent_bridge.rs`, servidas por `atic-mcp --editor`). Diseño en `docs/EDITOR_AGENTE.md`.
//!
//! Reusa el motor de Atic Code: Claude va por el sidecar (`code/sidecar.rs`) y el resto por ACP
//! (`code/acp.rs`); lo que llega se vuelve las mismas filas (`code::chat::Chat`). La sesión corre
//! en la carpeta de la grabación y arranca con el servidor `atic-editor` como MCP. Lo que el
//! agente le pide a las herramientas del editor se aprueba solo (es lo que el usuario vino a
//! hacer, y todo se puede deshacer de una vez); lo demás (leer archivos, correr comandos) pide
//! permiso aquí mismo.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, svg, AnyElement, ClickEvent, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla, KeyDownEvent, ScrollHandle,
    SharedString, Window,
};
use serde_json::{json, Value};

use super::palette::Ed;
use crate::code::acp;
use crate::code::chat::{Chat, Item, ToolCall};
use crate::code::sidecar::{Incoming, Sidecar};
use crate::hover::HoverExt;
use crate::i18n::{t, tf};
use crate::text_input::TextInput;

/// El nombre con que se registra el MCP del editor en el agente.
const SERVER: &str = "atic-editor";
/// El agente que se usó por última vez en este proceso (el próximo editor empieza con él).
static LAST_AGENT: Mutex<Option<String>> = Mutex::new(None);

const GAP: f32 = 8.0;
const COMPOSER_H: f32 = 38.0;
const AGENT_ROW_H: f32 = 30.0;

/// Un agente que se puede elegir: `None` en `cli` es Claude (por el sidecar).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Agent {
    cli: Option<&'static str>,
    name: &'static str,
}

impl Agent {
    fn key(self) -> &'static str {
        self.cli.unwrap_or("claude")
    }
}

/// Los agentes instalados en este equipo, Claude primero.
fn installed_agents() -> Vec<Agent> {
    let mut list = Vec::new();
    if crate::code::sidecar::node_path().is_some() && crate::code::sidecar::claude_path().is_some() {
        list.push(Agent { cli: None, name: "Claude" });
    }
    list.extend(acp::AGENTS.iter().filter(|a| crate::agents::on_path(a.cli)).map(|a| Agent { cli: Some(a.cli), name: a.name }));
    list
}

pub(super) struct Assistant {
    video: PathBuf,
    title: String,
    sidecar: Option<Arc<Sidecar>>,
    hub: Option<Arc<acp::Hub>>,
    chat: Option<Chat>,
    /// Para la clave de cada conversación.
    seq: u64,
    agents: Vec<Agent>,
    agent: usize,
    /// La lista de agentes abierta.
    picking: bool,
    /// La lista de modelos del agente abierta.
    picking_model: bool,
    /// El modelo elegido para el agente de esta ventana: (clave del agente, `configId`,
    /// valor). Se aplica al abrir la próxima sesión (`want`) o en caliente
    /// (`setConfig`). No se guarda entre ventanas: un modelo retirado dejaría la
    /// sesión sin abrir y sin lista para elegir otro.
    want: Option<(String, String, String)>,
    /// Ya se mandó el primer mensaje de esta sesión (lleva las instrucciones del editor).
    primed: bool,
    composer: Entity<TextInput>,
    scroll: ScrollHandle,
}

impl Assistant {
    pub fn new(video: PathBuf, title: String, cx: &mut Context<Self>) -> Self {
        let pal = Ed::current();
        let composer = cx.new(|cx| TextInput::new(t("pill.editor.ai.placeholder"), pal.text, pal.sub, pal.accent, cx));
        let agents = installed_agents();
        let last = LAST_AGENT.lock().ok().and_then(|l| l.clone());
        let agent = last.and_then(|key| agents.iter().position(|a| a.key() == key)).unwrap_or(0);
        Self {
            video,
            title,
            sidecar: None,
            hub: None,
            chat: None,
            seq: 0,
            agents,
            agent,
            picking: false,
            picking_model: false,
            want: None,
            primed: false,
            composer,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn composer_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.composer.read(cx).focus_handle(cx)
    }

    /// El campo tiene el foco: las teclas son suyas y no atajos del editor.
    pub fn composer_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.composer_focus(cx).is_focused(window)
    }

    pub fn busy(&self) -> bool {
        self.chat.as_ref().is_some_and(|c| c.busy)
    }

    fn current(&self) -> Option<Agent> {
        self.agents.get(self.agent).copied()
    }

    /// El agente y su canal de eventos se levantan con el primer mensaje.
    fn engine(&mut self, cx: &mut Context<Self>) -> (Arc<Sidecar>, Arc<acp::Hub>) {
        if let (Some(sidecar), Some(hub)) = (&self.sidecar, &self.hub) {
            return (sidecar.clone(), hub.clone());
        }
        let (sidecar, mut events) = Sidecar::new("recording", false);
        let hub = acp::Hub::new(sidecar.events());
        cx.spawn(async move |this, cx| {
            while let Some(incoming) = events.next().await {
                if this.update(cx, |view, cx| view.on_incoming(incoming, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.sidecar = Some(sidecar.clone());
        self.hub = Some(hub.clone());
        (sidecar, hub)
    }

    fn call(&self, method: &str, params: Value) -> Option<futures::channel::oneshot::Receiver<crate::code::sidecar::Reply>> {
        let acp = self.chat.as_ref().is_some_and(|c| c.acp.is_some());
        if acp && acp::handles(method) {
            self.hub.as_ref().map(|hub| hub.call(method, params))
        } else if acp {
            None
        } else {
            self.sidecar.as_ref().map(|sidecar| sidecar.call(method, params))
        }
    }

    fn fire(&self, method: &str, params: Value) {
        drop(self.call(method, params));
    }

    fn on_incoming(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_mut() else { return };
        match incoming {
            Incoming::Event { event, key, data } => {
                if key.as_deref() != Some(chat.key.as_str()) {
                    return;
                }
                chat.apply(&event, &data);
            }
            Incoming::Exit => {
                chat.live = false;
                chat.busy = false;
                chat.notice(t("pill.notchChat.agentExited"), true);
            }
        }
        // Lo que pide una herramienta del editor se aprueba solo.
        let key = chat.key.clone();
        let mut approved = Vec::new();
        chat.permissions.retain(|p| {
            let ours = is_editor_tool(&p.tool) || p.title.as_deref().is_some_and(is_editor_tool);
            if ours {
                approved.push((p.request_id.clone(), p.input.clone()));
            }
            !ours
        });
        for (request, input) in approved {
            self.fire("permission", json!({ "key": key, "requestId": request, "result": { "behavior": "allow", "updatedInput": input } }));
        }
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Cómo se registra el MCP del editor, en el formato de cada agente. `None` si falta `atic-mcp`.
    fn mcp_server(&self) -> Option<(PathBuf, Value, Value)> {
        let exe = crate::agents_settings::mcp_binary()?;
        // Claude (SDK): un mapa por nombre. ACP: una lista con `env` como pares.
        let claude = json!({ SERVER: { "type": "stdio", "command": exe, "args": ["--editor"], "env": {} } });
        let acp = json!([{ "name": SERVER, "command": exe, "args": ["--editor"], "env": [] }]);
        Some((exe, claude, acp))
    }

    /// Las instrucciones del editor, para el primer mensaje de cada sesión.
    fn primer(&self) -> String {
        format!(
            "Estás dentro del editor de video de Atic, con la grabación «{title}» ({path}) abierta. \
Tienes las herramientas atic_editor_*: atic_editor_state (el proyecto y sus índices), atic_editor_frames (ver el video: cuadros), \
atic_editor_transcript (lo que se dice), atic_editor_activity (clics, escritura, silencios), atic_editor_edit (cambios) y atic_editor_export. \
Antes de editar, mira: pide el estado y revisa el video (cuadros, transcripción o actividad según lo que pida el usuario). \
Edita con atic_editor_edit en lotes (cada lote es un solo paso de deshacer y es todo o nada); los tiempos son segundos del video original. \
No cambies archivos del disco ni abras otras apps: todo se hace con esas herramientas. Si algo es ambiguo, haz tu mejor propuesta y dila. \
Responde corto, en el idioma del usuario, y al terminar resume lo que cambiaste.",
            title = self.title,
            path = self.video.display()
        )
    }

    /// Enter o la flecha: el mensaje va a la conversación (la abre si no hay).
    pub fn send(&mut self, cx: &mut Context<Self>) {
        let typed = self.composer.read(cx).text().trim().to_string();
        if typed.is_empty() {
            return;
        }
        self.send_text(typed, cx);
    }

    fn send_text(&mut self, typed: String, cx: &mut Context<Self>) {
        let Some(agent) = self.current() else {
            cx.notify();
            return;
        };
        let cwd = self.video.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let mcp = self.mcp_server();
        // Lo elegido para el agente, antes de tomar prestado `self.chat`.
        let want = self.want.clone();
        if self.chat.is_none() {
            self.seq += 1;
            let key = format!("editor-ai-{}-{}", std::process::id(), self.seq);
            let mut chat = Chat::new(key, 0, cwd.clone());
            chat.acp = agent.cli;
            if mcp.is_none() {
                chat.notice(t("pill.editor.ai.noMcp"), true);
            }
            if agent.cli == Some("codex") && !codex_has_atic() {
                chat.notice(t("pill.editor.ai.codexHint"), false);
            }
            self.chat = Some(chat);
            self.primed = false;
        }
        self.picking = false;
        self.composer.update(cx, |input, cx| input.clear(cx));
        if let Ok(mut last) = LAST_AGENT.lock() {
            *last = Some(agent.key().to_string());
        }
        let primer_text = self.primer();
        let primer = (!self.primed).then(|| primer_text.clone());
        self.primed = true;
        let (sidecar, hub) = self.engine(cx);
        let Some(chat) = self.chat.as_mut() else { return };
        chat.push_user(&typed, Vec::new());
        let key = chat.key.clone();
        let start = (!chat.live).then(|| {
            chat.live = true;
            chat.starting = true;
            if let Some(cli) = chat.acp {
                let mut params = json!({ "key": key, "cwd": cwd, "agent": cli });
                if let Some((_, _, acp_servers)) = &mcp {
                    params["mcpServers"] = acp_servers.clone();
                }
                if let Some(session) = &chat.acp_session {
                    params["resume"] = json!(session);
                }
                // El modelo elegido para este agente viaja al abrir la sesión (el
                // agente lo aplica antes del primer turno).
                if let Some((agent_key, config, value)) = &want {
                    if agent_key == cli {
                        let mut configs = serde_json::Map::new();
                        configs.insert(config.clone(), json!(value));
                        params["want"] = json!({ "config": configs });
                    }
                }
                params
            } else {
                let mut params = json!({ "key": key, "cwd": cwd, "permissionMode": "default" });
                params["appendSystemPrompt"] = json!(primer_text);
                if let Some((_, claude_servers, _)) = &mcp {
                    params["mcpServers"] = claude_servers.clone();
                    params["allowedTools"] = json!([format!("mcp__{SERVER}")]);
                }
                if let Some(session) = &chat.session_id {
                    params["resume"] = json!(session);
                }
                params
            }
        });
        // Los agentes ACP no tienen «prompt de sistema»: las instrucciones van delante del primer mensaje.
        let acp = chat.acp.is_some();
        let text = match (&primer, acp) {
            (Some(primer), true) => format!("{primer}\n\n---\n\n{typed}"),
            _ => typed,
        };
        self.scroll.scroll_to_bottom();
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Some(params) = start {
                let started = if acp { hub.call("start", params) } else { sidecar.call("start", params) };
                if let Some(error) = failure(started.await) {
                    let _ = this.update(cx, |view, cx| view.fail(&key, error, cx));
                    return;
                }
            }
            let params = json!({ "key": key, "text": text, "images": [] });
            let sent = if acp { hub.call("send", params) } else { sidecar.call("send", params) };
            if let Some(error) = failure(sent.await) {
                let _ = this.update(cx, |view, cx| view.fail(&key, error, cx));
            }
        })
        .detach();
    }

    fn fail(&mut self, key: &str, error: String, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut().filter(|c| c.key == key) {
            chat.live = false;
            chat.busy = false;
            chat.notice(error, true);
            // La sesión no abrió: la próxima vuelve a llevar las instrucciones del editor.
            self.primed = false;
            cx.notify();
        }
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = &self.chat {
            self.fire("interrupt", json!({ "key": chat.key }));
        }
        cx.notify();
    }

    /// Permitir o rechazar la solicitud que espera.
    fn answer(&mut self, allow: bool, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_mut() else { return };
        if chat.permissions.is_empty() {
            return;
        }
        let permission = chat.permissions.remove(0);
        let result = if allow {
            json!({ "behavior": "allow", "updatedInput": permission.input })
        } else {
            json!({ "behavior": "deny", "message": "El usuario lo rechazó desde el editor de Atic." })
        };
        let key = chat.key.clone();
        self.fire("permission", json!({ "key": key, "requestId": permission.request_id, "result": result }));
        cx.notify();
    }

    /// «Nueva»: se cierra la conversación y queda lista otra.
    fn new_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.take().filter(|c| c.live) {
            let acp = chat.acp.is_some();
            let params = json!({ "key": chat.key });
            if acp {
                if let Some(hub) = &self.hub {
                    drop(hub.call("close", params));
                }
            } else if let Some(sidecar) = &self.sidecar {
                drop(sidecar.call("close", params));
            }
        }
        self.primed = false;
        cx.notify();
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.agent {
            self.new_chat(cx);
            self.agent = index;
            // Cada agente tiene sus modelos: lo elegido para el anterior ya no vale.
            self.want = None;
            self.picking_model = false;
            if let (Some(agent), Ok(mut last)) = (self.current(), LAST_AGENT.lock()) {
                *last = Some(agent.key().to_string());
            }
        }
        self.picking = false;
        cx.notify();
    }

    /// La opción de modelo del agente abierto: su `configId` y lo que el agente dice
    /// que está elegido. `None` mientras la sesión no abrió (los modelos llegan con
    /// ella) o si el agente no ofrece modelos.
    fn model_option(&self) -> Option<(String, String, String, Vec<(String, String)>)> {
        let option = self.chat.as_ref()?.acp_cfg.options.iter().find(|option| option.category == "model")?;
        let current = option.choices.iter().find(|choice| choice.value == option.current).map_or_else(|| option.current.clone(), |choice| choice.name.clone());
        let choices = option.choices.iter().map(|choice| (choice.value.clone(), choice.name.clone())).collect();
        Some((option.id.clone(), option.current.clone(), current, choices))
    }

    /// El nombre que muestra el chip: lo elegido acá aunque la sesión todavía diga
    /// otra cosa (se aplica al abrir).
    fn model_label(&self, current: &str, choices: &[(String, String)]) -> String {
        let Some((agent, _, value)) = &self.want else { return current.to_string() };
        if self.current().map_or(true, |a| a.key() != agent) {
            return current.to_string();
        }
        choices.iter().find(|(choice, _)| choice == value).map_or_else(|| current.to_string(), |(_, name)| name.clone())
    }

    /// El modelo elegido acá todavía no llegó a la sesión (se aplica al abrirla).
    fn model_pending(&self) -> bool {
        let Some((agent, config, value)) = &self.want else { return false };
        if self.current().map_or(true, |current| current.key() != agent) {
            return false;
        }
        self.chat.as_ref().and_then(|chat| chat.acp_cfg.options.iter().find(|option| &option.id == config)).is_some_and(|option| &option.current != value)
    }

    /// Elige el modelo: en caliente si la sesión está abierta, o para la próxima que
    /// se abra en esta ventana.
    fn choose_model(&mut self, config: String, value: String, cx: &mut Context<Self>) {
        let Some(agent) = self.current() else { return };
        let key = agent.key().to_string();
        self.want = Some((key, config.clone(), value.clone()));
        self.picking_model = false;
        if let Some(chat) = self.chat.as_ref().filter(|chat| chat.live) {
            let key = chat.key.clone();
            self.fire("setConfig", json!({ "key": key, "configId": config, "value": value }));
        }
        cx.notify();
    }
}

impl Drop for Assistant {
    /// Al cerrar la ventana, la sesión del agente se cierra con ella.
    fn drop(&mut self) {
        if let Some(chat) = self.chat.take().filter(|c| c.live) {
            if chat.acp.is_some() {
                if let Some(hub) = &self.hub {
                    drop(hub.call("close", json!({ "key": chat.key })));
                }
            } else if let Some(sidecar) = &self.sidecar {
                drop(sidecar.call("close", json!({ "key": chat.key })));
            }
        }
    }
}

fn is_editor_tool(name: &str) -> bool {
    // Las siete del MCP del editor (`atic_editor_list`, `atic_editor_state`…), con el
    // prefijo que les ponga cada agente («mcp__atic-editor__…», «atic-editor.…»).
    // Se mira solo el nombre de la herramienta: un texto suelto que mencione
    // «atic-editor» no basta, o cualquier tool de otro MCP se aprobaría sola.
    let tail = name.rsplit("__").next().unwrap_or(name);
    let tail = tail.rsplit('.').next().unwrap_or(tail);
    tail.starts_with("atic_editor_")
}

/// Un nombre de modelo que cabe en el chip: «MiniMax M2.5 Highspeed» → «MiniMax M2.5 High…».
fn short_model(name: &str) -> String {
    const MAX: usize = 18;
    if name.chars().count() <= MAX {
        return name.to_string();
    }
    let mut out: String = name.chars().take(MAX - 1).collect();
    out.push('…');
    out
}

/// ¿Tiene Codex el MCP de Atic en su configuración? (no se le puede pasar por sesión)
fn codex_has_atic() -> bool {
    atic_agents::mcp_install::instalado("codex").unwrap_or(false)
}

fn failure(reply: Result<crate::code::sidecar::Reply, futures::channel::oneshot::Canceled>) -> Option<String> {
    match reply {
        Ok(Ok(_)) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(t("pill.notchChat.agentExited").to_string()),
    }
}

/// Lo que dice una herramienta en una línea.
fn tool_line(call: &ToolCall) -> String {
    let input = call.input.as_ref();
    let name = call.name.rsplit("__").next().unwrap_or(&call.name);
    let editor = |what: &str| -> Option<String> {
        let input = input?;
        match what {
            "atic_editor_edit" => {
                let ops: Vec<&str> = input.get("ops")?.as_array()?.iter().filter_map(|op| op.get("op")?.as_str()).collect();
                Some(ops.join(", "))
            }
            "atic_editor_frames" => {
                let n = input.get("times").and_then(Value::as_array).map(|t| t.len()).or_else(|| input.get("count").and_then(Value::as_u64).map(|c| c as usize)).unwrap_or(6);
                Some(tf("pill.editor.ai.frames", &[("count", &n)]))
            }
            _ => None,
        }
    };
    if let Some(line) = editor(name) {
        return line;
    }
    let field = |key: &str| input.and_then(|i| i.get(key)).and_then(Value::as_str).map(str::to_string);
    let path = field("file_path").or_else(|| field("path")).map(|p| std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p));
    let detail = path.or_else(|| field("command")).or_else(|| field("pattern")).or_else(|| field("description")).unwrap_or_default();
    detail.lines().next().unwrap_or_default().chars().take(80).collect()
}

/// El nombre corto de una herramienta: sin el prefijo del servidor MCP.
fn tool_name(call: &ToolCall) -> String {
    call.name.rsplit("__").next().unwrap_or(&call.name).trim_start_matches("atic_editor_").to_string()
}

impl Assistant {
    fn render_item(&self, pal: &Ed, row: usize, chat: &Chat) -> Option<AnyElement> {
        if chat.hidden(row) {
            return None;
        }
        let element = match chat.items.get(row)? {
            Item::User { text, .. } => div()
                .flex()
                .flex_col()
                .items_end()
                .child(
                    div().max_w(gpui::relative(0.88)).px(px(10.)).py(px(6.)).rounded(px(12.)).bg(pal.control).text_size(px(12.)).text_color(pal.text).child(text.clone()),
                )
                .into_any_element(),
            Item::Text(said) => {
                let id = SharedString::from(format!("editor-ai-md-{}-{row}", chat.key));
                div()
                    .text_size(px(12.))
                    .child(gpui_m3::Markdown::new(id, said.clone()).on_link(|url, _, cx| {
                        if url.starts_with("https://") || url.starts_with("http://") {
                            cx.open_url(url);
                        }
                    }))
                    .into_any_element()
            }
            Item::Tool(call) => {
                let tone: Hsla = match (&call.result, call.is_error) {
                    (_, true) => gpui::rgb(0xe85a52).into(),
                    (None, _) => gpui::rgb(0xe8b04b).into(),
                    (Some(_), _) => gpui::rgb(0x6cc48a).into(),
                };
                div()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(11.))
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(tone))
                    .child(div().flex_none().text_color(pal.sub).child(tool_name(call)))
                    .child(div().flex_1().min_w_0().truncate().text_color(pal.muted).child(tool_line(call)))
                    .into_any_element()
            }
            Item::Notice { text, error } => div()
                .text_size(px(11.))
                .text_color(if *error { gpui::rgb(0xe85a52).into() } else { pal.sub })
                .child(text.clone())
                .into_any_element(),
            _ => return None,
        };
        Some(element)
    }

    fn pill_button(&self, pal: &Ed, id: &'static str, label: SharedString, primary: bool, act: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        let (text, control, accent, on_accent) = (pal.text, pal.control, pal.accent, pal.on_accent);
        div()
            .id(id)
            .h(px(26.))
            .px(px(11.))
            .flex()
            .flex_none()
            .items_center()
            .rounded(px(13.))
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| act(view, cx)))
            .child(label)
            .fx(id, move |el, h| {
                if primary {
                    el.text_color(on_accent).bg(accent.opacity(0.88 + 0.12 * h.t))
                } else {
                    el.text_color(text).bg(control.opacity(0.7 + 0.3 * h.t))
                }
            })
            .into_any_element()
    }

    fn render_head(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let (text, sub) = (pal.text, pal.sub);
        let name = self.current().map_or(t("pill.editor.ai.noAgent"), |a| a.name);
        let many = self.agents.len() > 1;
        // El modelo, si el agente deja elegir (los modelos llegan con su sesión).
        let model_chip = self.model_option().and_then(|(_, _, current, choices)| {
            (choices.len() > 1).then(|| {
                let label = short_model(&self.model_label(&current, &choices));
                let pending = self.model_pending();
                div()
                    .id("editor-ai-model")
                    .h(px(26.))
                    .pl(px(10.))
                    .pr(px(8.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(13.))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.picking_model = !view.picking_model;
                        view.picking = false;
                        cx.notify();
                    }))
                    .child(label)
                    .child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(sub))
                    .fx("editor-ai-model", {
                        let control = pal.control;
                        let (text, accent) = (pal.text, pal.accent);
                        move |el, h| el.text_color(if pending { accent } else { text }).bg(control.opacity(0.6 + 0.4 * h.t))
                    })
                    .into_any_element()
            })
        });
        let chip = div()
            .id("editor-ai-agent")
            .h(px(26.))
            .pl(px(10.))
            .pr(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(5.))
            .rounded(px(13.))
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .when(many, |el| {
                el.cursor_pointer().on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.picking = !view.picking;
                    cx.notify();
                }))
            })
            .child(name)
            .when(many, |el| el.child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(sub)))
            .fx("editor-ai-agent", {
                let control = pal.control;
                move |el, h| el.text_color(text).bg(control.opacity(0.6 + 0.4 * h.t))
            });
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(GAP))
            .pb(px(6.))
            .child(chip)
            .children(model_chip)
            .child(div().flex_1())
            .when(self.chat.as_ref().is_some_and(|c| !c.items.is_empty()), |el| {
                el.child(self.pill_button(pal, "editor-ai-new", t("pill.editor.ai.new").into(), false, |view, cx| view.new_chat(cx), cx))
            })
            .into_any_element()
    }

    fn render_picker(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let (text, sub, control) = (pal.text, pal.sub, pal.control);
        div()
            .flex_none()
            .flex()
            .flex_col()
            .mb(px(6.))
            .p(px(4.))
            .rounded(px(12.))
            .bg(pal.raised)
            .children(self.agents.iter().enumerate().map(|(index, agent)| {
                let on = index == self.agent;
                let id = SharedString::from(format!("editor-ai-pick-{index}"));
                div()
                    .id(id.clone())
                    .h(px(AGENT_ROW_H))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(9.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(index, cx)))
                    .child(div().size(px(6.)).rounded_full().bg(text.opacity(if on { 0.9 } else { 0.0 })))
                    .child(agent.name)
                    .fx(id, move |el, h| el.text_color(if on { text } else { h.mix(sub, text) }).bg(control.opacity(0.7 * h.t)))
            }))
            .into_any_element()
    }

    /// Los modelos del agente, en una lista que se desplaza (OpenCode trae decenas).
    fn render_model_picker(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let Some((config, current, _current_name, choices)) = self.model_option() else { return div().into_any_element() };
        let (text, sub, control) = (pal.text, pal.sub, pal.control);
        div()
            .id("editor-ai-models")
            .flex_none()
            .flex()
            .flex_col()
            .mb(px(6.))
            .p(px(4.))
            .rounded(px(12.))
            .bg(pal.raised)
            .max_h(px(232.))
            .overflow_y_scroll()
            .children(choices.into_iter().map(|(value, name)| {
                let on = value == current;
                let id = SharedString::from(format!("editor-ai-model-{value}"));
                let chosen = value.clone();
                let config_row = config.clone();
                div()
                    .id(id.clone())
                    .h(px(AGENT_ROW_H))
                    .px(px(10.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(9.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_model(config_row.clone(), chosen.clone(), cx)))
                    .child(div().size(px(6.)).rounded_full().bg(text.opacity(if on { 0.9 } else { 0.0 })))
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .fx(id, move |el, h| el.text_color(if on { text } else { h.mix(sub, text) }).bg(control.opacity(0.7 * h.t)))
            }))
            .into_any_element()
    }

    /// Lo que se ve antes de la primera pregunta: qué hace y unas ideas para empezar.
    fn render_intro(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let name = self.current().map_or(t("pill.editor.ai.theAgent"), |a| a.name);
        let ideas = [t("pill.editor.ai.idea1"), t("pill.editor.ai.idea2"), t("pill.editor.ai.idea3")];
        let (text, sub, control) = (pal.text, pal.sub, pal.control);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(text).child(t("pill.editor.ai.title")))
            .child(div().text_size(px(11.5)).text_color(sub).child(SharedString::from(tf("pill.editor.ai.intro", &[("agent", &name)]))))
            .when(self.current().is_none(), |el| el.child(div().text_size(px(11.5)).text_color(gpui::rgb(0xe8b04b)).child(t("pill.editor.ai.noAgentHint"))))
            .when(self.current().is_some(), |el| {
                el.child(div().flex().flex_col().gap(px(6.)).children(ideas.into_iter().enumerate().map(|(i, idea)| {
                    let id = SharedString::from(format!("editor-ai-idea-{i}"));
                    div()
                        .id(id.clone())
                        .px(px(11.))
                        .py(px(7.))
                        .rounded(px(12.))
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.send_text(idea.to_string(), cx)))
                        .child(idea)
                        .fx(id, move |el, h| el.text_color(h.mix(sub, text)).bg(control.opacity(0.45 + 0.4 * h.t)))
                })))
            })
            .into_any_element()
    }

    fn render_permission(&self, pal: &Ed, chat: &Chat, cx: &mut Context<Self>) -> Option<AnyElement> {
        let permission = chat.permissions.first()?;
        let detail = tool_line(&ToolCall::new(String::new(), permission.tool.clone(), Some(permission.input.clone())));
        let title = permission.title.clone().unwrap_or_else(|| tf("pill.notchChat.wantsTool", &[("tool", &permission.tool)]));
        Some(
            div()
                .flex_none()
                .mt(px(6.))
                .p(px(10.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .rounded(px(12.))
                .bg(Hsla::from(gpui::rgb(0xe8b04b)).opacity(0.14))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(pal.text).child(title))
                        .child(div().truncate().text_size(px(11.)).text_color(pal.sub).child(detail)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(self.pill_button(pal, "editor-ai-deny", t("pill.notchChat.deny").into(), false, |view, cx| view.answer(false, cx), cx))
                        .child(self.pill_button(pal, "editor-ai-allow", t("pill.notchChat.allow").into(), true, |view, cx| view.answer(true, cx), cx)),
                )
                .into_any_element(),
        )
    }

    fn render_composer(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.busy();
        let enabled = self.current().is_some();
        let (accent, on_accent) = (pal.accent, pal.on_accent);
        div()
            .h(px(COMPOSER_H))
            .flex_none()
            .mt(px(6.))
            .pl(px(14.))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(COMPOSER_H / 2.))
            .bg(pal.control)
            .opacity(if enabled { 1.0 } else { 0.5 })
            .child(div().flex_1().min_w_0().text_size(px(12.)).child(self.composer.clone()))
            .child(
                div()
                    .id("editor-ai-send")
                    .size(px(30.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(15.))
                    .bg(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| if busy { view.stop(cx) } else { view.send(cx) }))
                    .child(if busy {
                        div().size(px(10.)).rounded(px(2.)).bg(on_accent).into_any_element()
                    } else {
                        svg().path("icons/arrow-up.svg").size(px(15.)).text_color(on_accent).into_any_element()
                    }),
            )
            .into_any_element()
    }
}

impl gpui::Render for Assistant {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = Ed::current();
        let scope = crate::theme::scope_m3(cx);
        let body = match self.chat.as_ref().filter(|c| !c.items.is_empty()) {
            None => self.render_intro(&pal, cx),
            Some(chat) => {
                let rows: Vec<AnyElement> = (0..chat.items.len()).filter_map(|row| self.render_item(&pal, row, chat)).collect();
                let working = chat.working();
                let sub = pal.sub;
                div()
                    .id("editor-ai-log")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rows)
                    .when(working, |el| el.child(div().text_size(px(11.)).text_color(sub).child(t("pill.notchChat.working"))))
                    .into_any_element()
            }
        };
        let permission = self.chat.as_ref().and_then(|c| self.render_permission(&pal, c, cx));
        div()
            .id("editor-ai")
            .size_full()
            .flex()
            .flex_col()
            .pt(px(2.))
            .child(self.render_head(&pal, cx))
            .when(self.picking, |el| el.child(self.render_picker(&pal, cx)))
            .when(self.picking_model, |el| el.child(self.render_model_picker(&pal, cx)))
            .child(body)
            .children(permission)
            .child(self.render_composer(&pal, cx))
            .children(scope)
    }
}

/// Las teclas con el campo del chat enfocado: Intro envía y Esc devuelve el foco al editor.
/// `true` si la tecla ya se atendió (o es del campo): el editor no la trata como atajo.
pub(super) fn on_key(assistant: &Entity<Assistant>, event: &KeyDownEvent, window: &mut Window, editor_focus: &FocusHandle, cx: &mut gpui::App) -> bool {
    if !assistant.read(cx).composer_focused(window, cx) {
        return false;
    }
    match event.keystroke.key.as_str() {
        "enter" => assistant.update(cx, |view, cx| view.send(cx)),
        "escape" => window.focus(editor_focus),
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, input: Value) -> ToolCall {
        ToolCall::new("1".into(), name.into(), Some(input))
    }

    #[test]
    fn las_herramientas_del_editor_se_reconocen_por_su_nombre() {
        assert!(is_editor_tool("mcp__atic-editor__atic_editor_edit"));
        assert!(is_editor_tool("atic-editor.atic_editor_frames"));
        assert!(is_editor_tool("atic_editor_state"));
        assert!(!is_editor_tool("Bash"));
        assert!(!is_editor_tool("mcp__atic__atic_prompt"));
        // Un texto que solo menciona el servidor no alcanza: otra tool no se aprueba sola.
        assert!(!is_editor_tool("mcp__otro__atic-editor_destroy"));
        assert!(!is_editor_tool("atic-editor"));
    }

    #[test]
    fn el_nombre_del_modelo_cabe_en_el_chip() {
        assert_eq!(short_model("GPT-6 Luna"), "GPT-6 Luna");
        assert_eq!(short_model("MiniMax-M2.5-highspeed"), "MiniMax-M2.5-high…");
    }

    #[test]
    fn una_edicion_se_resume_por_sus_operaciones() {
        let edit = call("mcp__atic-editor__atic_editor_edit", json!({ "ops": [{ "op": "cut" }, { "op": "zoom" }] }));
        assert_eq!(tool_line(&edit), "cut, zoom");
        assert_eq!(tool_name(&edit), "edit");
        let frames = call("atic_editor_frames", json!({ "count": 4 }));
        assert!(tool_line(&frames).contains('4'), "{}", tool_line(&frames));
        let bash = call("Bash", json!({ "command": "ls -la\nmás" }));
        assert_eq!(tool_line(&bash), "ls -la");
    }
}
