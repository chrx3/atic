//! Atic Code: trabajar con Claude Code en una ventana, a la manera de la referencia.
//!
//! A la izquierda, los espacios de trabajo (un nombre y varias carpetas, los
//! mismos de `space/workspaces.rs`) con sus conversaciones: las abiertas y las
//! guardadas por Claude Code. Al centro, el chat. A la derecha, el árbol de
//! archivos y lo que cambió en git, con un visor de solo lectura.
//!
//! Las conversaciones las maneja el sidecar (`sidecar.rs`): un Node con el
//! Claude Agent SDK. Cada una arranca en la primera carpeta del espacio y
//! recibe las demás como `additionalDirectories`. La configuración de Claude
//! (modelo, permisos, esfuerzo, razonamiento) es por espacio (`config.rs`).
//!
//! `CODE_ALONE=1` abre solo esta ventana, sin la pill. `CODE_DEMO=1` abre
//! además una conversación de ejemplo (`demo.rs`).

mod agent_menu;
mod chat;
mod config;
mod demo;
mod git;
mod menus;
mod palette;
mod permissions;
mod settings_m3;
mod sidebar;
mod sidecar;
mod style;
mod tools;
mod usage;
mod view;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use base64::Engine;
use gpui::{
    actions, prelude::*, px, size, App, Bounds, ClipboardEntry, Context, Entity, EntityInputHandler, FocusHandle,
    Focusable, ImageFormat, KeyBinding, PathPromptOptions, Pixels, Point, ScrollHandle, Window,
    WindowBackgroundAppearance,
};
use serde_json::{json, Value};

use crate::space::explorer::Explorer;
use crate::space::viewer::Doc;
use crate::space::workspaces::Workspaces;
use crate::text_area::TextArea;

use chat::Chat;
use config::{ClaudeConfig, Configs};
use sidecar::{Incoming, Reply, Sidecar};

actions!(atic_code, [Send, PasteAttach, CloseMenu, OpenPalette, NewConversation, ToggleSidebar, ToggleSettings, Attach]);

/// El contexto de teclas de la caja del chat: Enter manda, Mayús+Enter baja de línea.
const COMPOSER: &str = "CodeComposer";
/// Cada cuánto se mira qué cambió en git.
const CHANGES_EVERY: Duration = Duration::from_secs(3);

pub fn bind_keys(cx: &mut App) {
    let context = Some("CodeComposer > TextArea");
    cx.bind_keys([
        KeyBinding::new("enter", Send, context),
        KeyBinding::new("shift-enter", crate::text_area::Newline, context),
        // Pegar una imagen la adjunta; el texto se pega como siempre.
        KeyBinding::new("ctrl-v", PasteAttach, context),
        KeyBinding::new("escape", CloseMenu, Some("AticCode")),
        // Los atajos de la referencia (Ctrl en Windows).
        KeyBinding::new("ctrl-k", OpenPalette, Some("AticCode")),
        KeyBinding::new("ctrl-p", OpenPalette, Some("AticCode")),
        KeyBinding::new("ctrl-n", NewConversation, Some("AticCode")),
        KeyBinding::new("ctrl-b", ToggleSidebar, Some("AticCode")),
        KeyBinding::new("ctrl-,", ToggleSettings, Some("AticCode")),
        KeyBinding::new("ctrl-u", Attach, Some("AticCode")),
    ]);
}

/// Los menús de la caja de texto y de la barra de estado.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// Modelo y esfuerzo.
    Model,
    /// «/»: comandos de Claude Code y acciones.
    Actions,
    /// El modo de permisos.
    Mode,
    /// El proyecto, desde la pantalla de inicio.
    Project,
}

/// Lo que va con el próximo mensaje: una imagen (se manda como imagen) o un
/// archivo (se manda su ruta, para que Claude lo lea).
#[derive(Clone)]
pub enum Attachment {
    Image { name: String, media_type: &'static str, data: String },
    File(PathBuf),
}

fn image_type(path: &std::path::Path) -> Option<&'static str> {
    match path.extension()?.to_string_lossy().to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// El texto que se manda con los archivos adjuntos, como en la referencia.
fn with_files(text: &str, files: &[String]) -> String {
    if files.is_empty() {
        return text.to_string();
    }
    let base = if text.is_empty() { "Revisa los archivos adjuntos." } else { text };
    let list: Vec<String> = files.iter().map(|f| format!("@{f}")).collect();
    format!("{base}\n\n(Archivos adjuntos: {})", list.join(" "))
}

#[derive(Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub title: String,
    /// La última actividad (segundos o milisegundos desde 1970).
    pub modified: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Files,
    Changes,
}

pub struct CodeView {
    focus: FocusHandle,
    workspaces: Workspaces,
    configs: Configs,
    sidecar: Arc<Sidecar>,
    chats: Vec<Chat>,
    /// La clave de la conversación a la vista; ninguna es una nueva por empezar.
    active: Option<String>,
    history: HashMap<u64, Vec<SessionInfo>>,
    explorer: Explorer,
    /// Los repositorios del espacio con lo que cambió.
    repos: Vec<git::Repo>,
    /// El panel de la derecha, si está abierto.
    side: Option<Side>,
    doc: Option<Doc>,
    composer: Entity<TextArea>,
    thread: ScrollHandle,
    /// Seguir el final del chat mientras llega texto (se suelta al subir).
    follow: bool,
    /// Herramientas abiertas para ver su detalle.
    expanded: HashSet<String>,
    /// Los modelos que ofrece Claude Code (de `meta`), id y nombre.
    models: Vec<(String, String)>,
    settings_open: bool,
    /// En la configuración: también los modelos viejos de cada familia.
    more_models: bool,
    claude_path: Option<PathBuf>,
    /// El menú abierto y dónde se pidió.
    menu: Option<(Menu, Point<Pixels>)>,
    attachments: Vec<Attachment>,
    /// Los comandos de Claude Code (de `meta`): nombre y descripción.
    commands: Vec<(String, String)>,
    /// Con qué estilo y modo se armó la caja de texto (cambian sus colores).
    composer_style: (style::Style, bool),
    /// El menú de acciones de Expressive: pestaña, submenú y filtro.
    menu_tab: usize,
    menu_sub: Option<agent_menu::Sub>,
    menu_filter: Entity<gpui_m3::TextField>,
    /// Los servidores MCP de la conversación (nombre, estado); `None` mientras carga.
    mcp: Option<Vec<(String, String)>>,
    /// Lo que dice `meta`: subagentes (nombre, descripción), estilos de salida y
    /// la descripción de cada modelo.
    agents: Vec<(String, String)>,
    output_styles: Vec<String>,
    model_info: HashMap<String, String>,
    toast: Option<gpui::SharedString>,
    toast_gen: u64,
    /// Las respuestas de las tarjetas de permiso de Expressive: la indicación
    /// al rechazar, la del plan, lo elegido en cada pregunta y sus «Otro…».
    perm_feedback: Entity<gpui_m3::TextField>,
    plan_feedback: Entity<gpui_m3::TextField>,
    asks: HashMap<String, permissions::AskPicks>,
    ask_other: HashMap<(String, usize), Entity<gpui_m3::TextField>>,
    /// La paleta de comandos: la entidad, si está abierta, qué hace cada fila y
    /// el comando elegido que falta ejecutar.
    palette: Entity<gpui_m3::CommandPalette>,
    palette_open: bool,
    palette_acts: Vec<palette::PaletteAct>,
    pending_palette: Option<palette::PaletteAct>,
    /// Configuración de Expressive: la pestaña, lo que se sabe de Claude Code,
    /// su actualización y la cuenta (correo, plan, organización).
    settings_tab: usize,
    claude_info: Option<settings_m3::ClaudeInfo>,
    updating_claude: bool,
    update_log: String,
    update_result: Option<(bool, String)>,
    account: Option<(Option<String>, Option<String>, Option<String>)>,
    /// «Cuenta y uso» o el mapa de agentes, y lo que muestra el primero.
    pop: Option<usage::Pop>,
    usage: Option<usage::UsageInfo>,
    /// La barra lateral de Expressive: abierta o el riel, la página de
    /// historial, el menú contextual y el renombrar en el sitio.
    sidebar_open: bool,
    history_page: bool,
    session_menu: Option<(sidebar::SessionRef, Point<Pixels>)>,
    renaming: Option<sidebar::SessionRef>,
    rename_field: Entity<gpui_m3::TextField>,
    history_search: Entity<gpui_m3::TextField>,
    /// La conversación del historial con el «Eliminar» armado.
    confirm_delete: Option<String>,
    user_name: Option<String>,
    /// Mensajes enviados: cada uno hace despegar el botón de enviar.
    sends: u64,
    /// La caja de texto tiene el foco (en Expressive cambia de fondo).
    composer_focused: bool,
    /// Dónde quedó la caja de texto en el último cuadro: en Expressive los
    /// menús se abren sobre ella, como en la referencia.
    composer_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Un error del agente que no es de una conversación.
    error: Option<String>,
    next_key: u64,
    next_doc: u64,
}

impl Focusable for CodeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

pub fn open_window(cx: &mut App) -> anyhow::Result<gpui::WindowHandle<CodeView>> {
    let configs = Configs::load();
    style::set(configs.style, configs.mode);
    style::apply_m3(cx);
    let options = gpui::WindowOptions {
        window_background: background(configs.style),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Atic Code".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_min_size: Some(size(px(900.), px(560.))),
        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(None, size(px(1360.), px(860.)), cx))),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        ..Default::default()
    };
    Ok(cx.open_window(options, |window, cx| {
        crate::space::chrome::setup(window);
        cx.new(|cx| CodeView::new(window, cx))
    })?)
}

/// Liquid Glass deja ver el escritorio desenfocado detrás de la ventana.
fn background(style: style::Style) -> WindowBackgroundAppearance {
    if style == style::Style::Glass {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// El campo de las tarjetas de permiso: 40 de alto sobre el fondo del panel.
fn feedback_field(placeholder: &'static str, cx: &mut Context<CodeView>) -> Entity<gpui_m3::TextField> {
    let field = cx.new(|cx| gpui_m3::TextField::new(cx).height(px(40.)).placeholder(placeholder));
    cx.subscribe(&field, |_, _, _: &gpui_m3::TextFieldEvent, cx| cx.notify()).detach();
    field
}

fn new_composer(text: &str, cx: &mut Context<CodeView>) -> Entity<TextArea> {
    let t = style::t();
    let area = cx.new(|cx| {
        let mut area = TextArea::new("Responde a Claude…", t.text, t.faint, t.accent, cx);
        area.set_text(text, cx);
        area
    });
    cx.subscribe(&area, CodeView::composer_changed).detach();
    area
}

impl CodeView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sidecar, mut events) = Sidecar::new();
        cx.spawn(async move |this, cx| {
            while let Some(message) = events.next().await {
                if this.update(cx, |view, cx| view.incoming(message, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let composer = new_composer("", cx);
        let t = style::t();
        let focus = cx.focus_handle();
        composer.read(cx).focus_handle(cx).focus(window);
        let menu_filter = cx.new(|cx| gpui_m3::TextField::new(cx).compact().placeholder("Filtrar acciones o comandos…").icon("search"));
        cx.subscribe(&menu_filter, |_, _, _: &gpui_m3::TextFieldEvent, cx| cx.notify()).detach();
        let rename_field = cx.new(|cx| gpui_m3::TextField::new(cx).inline().select_all_on_focus(true).restore_on_cancel(true));
        cx.subscribe(&rename_field, |view: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
            gpui_m3::TextFieldEvent::Submitted(_) => view.commit_rename(cx),
            gpui_m3::TextFieldEvent::Cancelled => view.cancel_rename(cx),
            gpui_m3::TextFieldEvent::Changed(_) => {}
        })
        .detach();
        let history_search = cx.new(|cx| gpui_m3::TextField::new(cx).placeholder("Buscar conversaciones").icon("search"));
        cx.subscribe(&history_search, |_, _, _: &gpui_m3::TextFieldEvent, cx| cx.notify()).detach();

        let mut view = Self {
            focus,
            workspaces: Workspaces::load(),
            configs: Configs::load(),
            sidecar,
            chats: Vec::new(),
            active: None,
            history: HashMap::new(),
            explorer: Explorer::default(),
            repos: Vec::new(),
            side: None,
            doc: None,
            composer,
            thread: ScrollHandle::new(),
            follow: true,
            expanded: HashSet::new(),
            models: config::MODELS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect(),
            settings_open: false,
            more_models: false,
            claude_path: sidecar::claude_path(),
            menu: None,
            attachments: Vec::new(),
            commands: Vec::new(),
            composer_style: (t.style, t.light),
            menu_tab: 0,
            menu_sub: None,
            menu_filter,
            mcp: None,
            agents: Vec::new(),
            output_styles: Vec::new(),
            model_info: HashMap::new(),
            toast: None,
            toast_gen: 0,
            sends: 0,
            perm_feedback: feedback_field("Decirle a Claude qué hacer en cambio", cx),
            plan_feedback: feedback_field("Decirle a Claude qué cambiar del plan", cx),
            asks: HashMap::new(),
            settings_tab: 0,
            claude_info: None,
            updating_claude: false,
            update_log: String::new(),
            update_result: None,
            account: None,
            palette: palette::new_palette(cx),
            palette_open: false,
            palette_acts: Vec::new(),
            pending_palette: None,
            pop: None,
            usage: None,
            sidebar_open: true,
            history_page: false,
            session_menu: None,
            renaming: None,
            rename_field,
            history_search,
            confirm_delete: None,
            user_name: sidebar::git_user_name(),
            ask_other: HashMap::new(),
            composer_focused: false,
            composer_bounds: Rc::new(Cell::new(None)),
            error: None,
            next_key: 0,
            next_doc: 0,
        };
        view.load_history(cx);
        view.watch_changes(cx);
        if std::env::var_os("CODE_DEMO").is_some() {
            view.open_demo();
        }
        view
    }

    // --- Sidecar -----------------------------------------------------------------

    /// Una petición al sidecar; `done` recibe la respuesta en el hilo de la vista.
    fn request(
        &self,
        method: &str,
        params: Value,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Reply, &mut Context<Self>) + 'static,
    ) {
        let reply = self.sidecar.call(method, params);
        cx.spawn(async move |this, cx| {
            let reply = reply.await.unwrap_or_else(|_| Err("El agente no respondió".into()));
            let _ = this.update(cx, |view, cx| {
                done(view, reply, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Una petición de la que solo importa si falló.
    fn fire(&self, method: &str, params: Value, cx: &mut Context<Self>) {
        self.request(method, params, cx, |view, reply, _| {
            if let Err(error) = reply {
                view.error = Some(error);
            }
        });
    }

    fn incoming(&mut self, message: Incoming, cx: &mut Context<Self>) {
        match message {
            Incoming::Exit => {
                for chat in &mut self.chats {
                    if chat.live {
                        chat.live = false;
                        chat.busy = false;
                        chat.permissions.clear();
                        chat.notice("El agente se detuvo. Al escribir de nuevo se retoma la conversación.", true);
                    }
                }
            }
            Incoming::Event { event, key, data } => {
                match event.as_str() {
                    "meta" => {
                        self.read_models(&data);
                        self.read_commands(&data);
                        self.read_meta(&data);
                    }
                    "claudeUpdate" => {
                        if let Some(text) = data.get("text").and_then(Value::as_str) {
                            self.update_log.push_str(text);
                        }
                    }
                    "fatal" => {
                        self.error = data.get("message").and_then(Value::as_str).map(|m| m.lines().next().unwrap_or(m).to_string());
                    }
                    _ => {}
                }
                let Some(key) = key else {
                    cx.notify();
                    return;
                };
                let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
                    return;
                };
                chat.track(&event, &data);
                if event == "remote" {
                    chat.remote_url = data.get("url").and_then(Value::as_str).map(str::to_string);
                }
                chat.apply(&event, &data);
                let workspace = chat.workspace;
                if event == "permission" {
                    self.prepare_permission(&key, cx);
                }
                if event == "result" {
                    let seen = self.active.as_deref() == Some(key.as_str());
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
                        chat.unread = !seen;
                    }
                    self.load_history_for(workspace, cx);
                    self.refresh_context(&key, cx);
                }
                if self.active.as_deref() == Some(key.as_str()) && self.follow {
                    self.thread.scroll_to_bottom();
                }
            }
        }
        cx.notify();
    }

    fn read_commands(&mut self, meta: &Value) {
        let Some(list) = meta.get("commands").and_then(Value::as_array) else {
            return;
        };
        self.commands = list
            .iter()
            .filter_map(|c| {
                let name = c.get("name").and_then(Value::as_str)?;
                let description = c.get("description").and_then(Value::as_str).unwrap_or_default();
                Some((name.to_string(), description.to_string()))
            })
            .collect();
    }

    /// Subagentes y estilos de salida, para el menú de acciones.
    fn read_meta(&mut self, meta: &Value) {
        let text = |value: &Value, name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        if let Some(list) = meta.get("agents").and_then(Value::as_array) {
            self.agents = list.iter().map(|a| (text(a, "name"), text(a, "description"))).filter(|(name, _)| !name.is_empty()).collect();
        }
        if let Some(account) = meta.get("account") {
            let field = |name: &str| account.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
            self.account = Some((field("email"), field("subscriptionType"), field("organization")));
        }
        if let Some(list) = meta.get("available_output_styles").and_then(Value::as_array) {
            self.output_styles = list.iter().filter_map(Value::as_str).map(str::to_string).collect();
        }
    }

    fn read_models(&mut self, meta: &Value) {
        let Some(list) = meta.get("models").and_then(Value::as_array) else {
            return;
        };
        let mut models = vec![(String::new(), "Predeterminado".to_string())];
        for model in list {
            let Some(id) = model.get("value").and_then(Value::as_str) else {
                continue;
            };
            let description = model.get("description").and_then(Value::as_str).filter(|d| !d.is_empty());
            // El predeterminado de Claude Code es el vacío de Atic Code, con su nombre y descripción.
            if id.is_empty() || id == "default" {
                if let Some(name) = model.get("displayName").and_then(Value::as_str) {
                    models[0].1 = name.to_string();
                }
                if let Some(description) = description {
                    self.model_info.insert(String::new(), description.to_string());
                }
                continue;
            }
            let name = model.get("displayName").and_then(Value::as_str).unwrap_or(id);
            if let Some(description) = description {
                self.model_info.insert(id.to_string(), description.to_string());
            }
            models.push((id.to_string(), name.to_string()));
        }
        if models.len() > 1 {
            self.models = models;
        }
    }

    // --- Espacios e historial --------------------------------------------------------

    fn active_workspace(&self) -> Option<u64> {
        self.workspaces.active_id()
    }

    fn load_history(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.active_workspace() {
            self.load_history_for(id, cx);
        }
    }

    fn load_history_for(&mut self, workspace: u64, cx: &mut Context<Self>) {
        let Some(dir) = self.workspaces.get(workspace).and_then(|w| w.main().cloned()) else {
            return;
        };
        let params = json!({ "dir": dir, "limit": 40 });
        self.request("listSessions", params, cx, move |view, reply, _| match reply {
            Ok(list) => {
                let sessions = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        let text = |name: &str| s.get(name).and_then(Value::as_str).filter(|t| !t.trim().is_empty());
                        Some(SessionInfo {
                            session_id: text("sessionId")?.to_string(),
                            title: text("customTitle")
                                .or_else(|| text("summary"))
                                .or_else(|| text("firstPrompt"))
                                .unwrap_or("Conversación")
                                .lines()
                                .next()
                                .unwrap_or_default()
                                .to_string(),
                            modified: sidebar::modified_of(s),
                        })
                    })
                    .collect();
                view.history.insert(workspace, sessions);
            }
            Err(error) => view.error = Some(error),
        });
    }

    fn select_workspace(&mut self, id: u64, cx: &mut Context<Self>) {
        self.workspaces.select(id);
        if self.active_chat().is_some_and(|c| c.workspace != id) {
            self.active = None;
        }
        self.doc = None;
        self.repos.clear();
        self.explorer.refresh();
        self.load_history_for(id, cx);
        self.refresh_changes(cx);
        cx.notify();
    }

    fn pick_folders(&mut self, add_to: Option<u64>, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Elegir".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            if paths.is_empty() {
                return;
            }
            let _ = this.update(cx, |view, cx| {
                let id = match add_to {
                    Some(id) => {
                        view.workspaces.add_folders(id, paths);
                        id
                    }
                    None => view.workspaces.create(paths),
                };
                view.select_workspace(id, cx);
            });
        })
        .detach();
    }

    fn remove_workspace(&mut self, id: u64, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.chats.iter().filter(|c| c.workspace == id).map(|c| c.key.clone()).collect();
        for key in keys {
            self.close_chat(&key, cx);
        }
        self.workspaces.remove(id);
        self.history.remove(&id);
        if let Some(active) = self.active_workspace() {
            self.select_workspace(active, cx);
        }
        cx.notify();
    }

    // --- Conversaciones ---------------------------------------------------------------

    fn active_chat(&self) -> Option<&Chat> {
        let key = self.active.as_deref()?;
        self.chats.iter().find(|c| c.key == key)
    }

    fn open_demo(&mut self) {
        let Some(workspace) = self.workspaces.active() else {
            return;
        };
        let Some(cwd) = workspace.main().cloned() else {
            return;
        };
        let id = workspace.id;
        let key = self.new_key();
        self.chats.push(demo::chat(key.clone(), id, cwd));
        self.active = Some(key);
        self.side = Some(Side::Changes);
    }

    fn new_key(&mut self) -> String {
        self.next_key += 1;
        format!("c{}", self.next_key)
    }

    fn new_chat(&mut self, workspace: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_workspace() != Some(workspace) {
            self.select_workspace(workspace, cx);
        }
        self.active = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.read(cx).focus_handle(cx).focus(window);
    }

    fn select_chat(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.unread = false;
        }
        self.active = Some(key);
        self.follow = true;
        self.thread.scroll_to_bottom();
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Abre una conversación guardada: su historial ahora, la sesión al escribir.
    fn open_session(&mut self, workspace: u64, info: SessionInfo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = self.chats.iter().find(|c| c.session_id.as_deref() == Some(info.session_id.as_str())) {
            let key = chat.key.clone();
            self.select_chat(key, window, cx);
            return;
        }
        let Some(cwd) = self.workspaces.get(workspace).and_then(|w| w.main().cloned()) else {
            return;
        };
        let key = self.new_key();
        let mut chat = Chat::new(key.clone(), workspace, cwd.clone());
        chat.session_id = Some(info.session_id.clone());
        chat.title = info.title.clone();
        self.chats.push(chat);
        self.select_chat(key.clone(), window, cx);
        let params = json!({ "sessionId": info.session_id, "dir": cwd });
        self.request("sessionMessages", params, cx, move |view, reply, _| {
            let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) else {
                return;
            };
            match reply {
                Ok(messages) => chat.load_history(messages.as_array().map(Vec::as_slice).unwrap_or_default()),
                Err(error) => chat.notice(format!("No se pudo leer la conversación: {error}"), true),
            }
            if view.active.as_deref() == Some(key.as_str()) {
                view.thread.scroll_to_bottom();
            }
        });
    }

    fn close_chat(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(chat) = self.chats.iter().find(|c| c.key == key) {
            if chat.live {
                self.fire("close", json!({ "key": key }), cx);
            }
        }
        self.chats.retain(|c| c.key != key);
        if self.active.as_deref() == Some(key) {
            self.active = None;
        }
        cx.notify();
    }

    /// Abre la sesión en el sidecar si no está abierta (nueva o retomada).
    fn ensure_live(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter().find(|c| c.key == key) else {
            return;
        };
        if chat.live {
            return;
        }
        let workspace = self.workspaces.get(chat.workspace);
        let extras = workspace.map(|w| w.extras(&chat.cwd)).unwrap_or_default();
        let names: Vec<String> = workspace.map(|w| w.folders.iter().map(|f| f.display().to_string()).collect()).unwrap_or_default();
        let mut params = json!({ "key": key, "cwd": chat.cwd });
        if !extras.is_empty() {
            params["additionalDirectories"] = json!(extras);
        }
        if names.len() > 1 {
            params["appendSystemPrompt"] =
                json!(format!("Este proyecto tiene varias carpetas; puedes leer y editar en todas:\n{}", names.join("\n")));
        }
        if let Some(session) = &chat.session_id {
            params["resume"] = json!(session);
        }
        self.configs.get(chat.workspace).start_params(&mut params);
        let key = key.to_string();
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.live = true;
        }
        self.request("start", params, cx, move |view, reply, _| {
            if let Err(error) = reply {
                if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
                    chat.live = false;
                    chat.busy = false;
                    chat.notice(format!("No se pudo abrir la sesión: {error}"), true);
                }
            }
        });
    }

    fn send(&mut self, _: &Send, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.composer.read(cx).text().trim().to_string();
        if typed.is_empty() && self.attachments.is_empty() {
            return;
        }
        let mut images = Vec::new();
        let mut files = Vec::new();
        for attachment in &self.attachments {
            match attachment {
                Attachment::Image { media_type, data, .. } => images.push(json!({ "mediaType": media_type, "data": data })),
                Attachment::File(path) => files.push(path.display().to_string()),
            }
        }
        let text = with_files(&typed, &files);
        if self.active_chat().is_some_and(|c| c.busy) {
            return;
        }
        // La animación de despegue del botón de enviar.
        self.sends += 1;
        let key = match self.active.clone() {
            Some(key) => key,
            None => {
                let Some(workspace) = self.workspaces.active() else {
                    return;
                };
                let Some(cwd) = workspace.main().cloned() else {
                    return;
                };
                let workspace = workspace.id;
                let key = self.new_key();
                self.chats.push(Chat::new(key.clone(), workspace, cwd));
                self.active = Some(key.clone());
                key
            }
        };
        self.ensure_live(&key, cx);
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.push_user(&text);
        }
        let params = json!({ "key": key, "text": text, "images": images });
        self.attachments.clear();
        let failed_key = key.clone();
        self.request("send", params, cx, move |view, reply, _| {
            if let Err(error) = reply {
                if let Some(chat) = view.chats.iter_mut().find(|c| c.key == failed_key) {
                    chat.busy = false;
                    chat.notice(format!("No se pudo mandar: {error}"), true);
                }
            }
        });
        self.composer.update(cx, |area, cx| area.set_text("", cx));
        self.follow = true;
        self.thread.scroll_to_bottom();
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn interrupt(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active.clone() else {
            return;
        };
        if self.active_chat().is_some_and(|c| c.live && c.busy) {
            self.fire("interrupt", json!({ "key": key }), cx);
        }
    }

    /// Responde un permiso: permitir, permitir siempre (con las sugerencias de
    /// Claude Code) o rechazar.
    fn answer(&mut self, key: &str, request_id: &str, allow: bool, always: bool, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
            return;
        };
        let Some(index) = chat.permissions.iter().position(|p| p.request_id == request_id) else {
            return;
        };
        let permission = chat.permissions.remove(index);
        let result = if allow {
            let mut result = json!({ "behavior": "allow", "updatedInput": permission.input });
            if always {
                if let Some(suggestions) = permission.suggestions {
                    result["updatedPermissions"] = suggestions;
                }
            }
            result
        } else {
            json!({ "behavior": "deny", "message": "El usuario rechazó este paso." })
        };
        self.fire("permission", json!({ "key": key, "requestId": request_id, "result": result }), cx);
        cx.notify();
    }

    // --- Adjuntos y menús -----------------------------------------------------------

    fn attach_paths(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            let image = image_type(&path).and_then(|media_type| {
                let bytes = std::fs::read(&path).ok()?;
                Some(Attachment::Image {
                    name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    media_type,
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                })
            });
            let attachment = image.unwrap_or(Attachment::File(path));
            if let Attachment::File(path) = &attachment {
                if self.attachments.iter().any(|a| matches!(a, Attachment::File(p) if p == path)) {
                    continue;
                }
            }
            self.attachments.push(attachment);
        }
    }

    fn pick_attachments(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Adjuntar".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                view.attach_paths(paths);
                cx.notify();
            });
        })
        .detach();
    }

    /// Ctrl+V en la caja: una imagen se adjunta; el texto se pega.
    fn paste(&mut self, _: &PasteAttach, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for entry in item.entries() {
            if let ClipboardEntry::Image(image) = entry {
                let media_type = match image.format() {
                    ImageFormat::Png => "image/png",
                    ImageFormat::Jpeg => "image/jpeg",
                    ImageFormat::Gif => "image/gif",
                    ImageFormat::Webp => "image/webp",
                    _ => continue,
                };
                let n = self.attachments.iter().filter(|a| matches!(a, Attachment::Image { .. })).count() + 1;
                self.attachments.push(Attachment::Image {
                    name: format!("Imagen {n}"),
                    media_type,
                    data: base64::engine::general_purpose::STANDARD.encode(image.bytes()),
                });
                cx.notify();
                return;
            }
        }
        if let Some(text) = item.text() {
            let text = text.replace("\r\n", "\n");
            self.composer.update(cx, |area, cx| area.replace_text_in_range(None, &text, window, cx));
        }
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    fn toggle_menu(&mut self, menu: Menu, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.menu = match self.menu {
            Some((open, _)) if open == menu => None,
            _ => Some((menu, at)),
        };
        // El menú de acciones abre siempre en su raíz y sin filtro.
        self.menu_sub = None;
        self.menu_filter.update(cx, |field, cx| field.set_text("", cx));
        cx.notify();
    }

    /// Esc: cierra lo que esté abierto o, si no hay nada, detiene la respuesta.
    fn close_menu(&mut self, _: &CloseMenu, _: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() || self.pop.take().is_some() || self.settings_open {
            self.settings_open = false;
            cx.notify();
        } else {
            self.interrupt(cx);
        }
    }

    fn open_palette_action(&mut self, _: &OpenPalette, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(window, cx);
    }

    fn new_conversation_action(&mut self, _: &NewConversation, window: &mut Window, cx: &mut Context<Self>) {
        self.new_conversation(window, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    fn toggle_settings(&mut self, _: &ToggleSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        cx.notify();
    }

    fn attach_action(&mut self, _: &Attach, _: &mut Window, cx: &mut Context<Self>) {
        self.pick_attachments(cx);
    }

    /// El chip del modo pasa al siguiente: Preguntar, Editar automáticamente,
    /// Plan, Omitir permisos.
    fn cycle_mode(&mut self, cx: &mut Context<Self>) {
        let current = self.config().permission_mode;
        let index = config::MODES.iter().position(|(id, _)| *id == current).map_or(0, |index| (index + 1) % config::MODES.len());
        let next = config::MODES[index].0.to_string();
        self.set_config(|c| c.permission_mode = next, cx);
    }

    /// Deja un texto en la caja, listo para seguir escribiendo (las sugerencias del inicio).
    fn prefill(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |area, cx| area.set_text(text, cx));
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// «/» como único texto abre el menú de acciones, como en la referencia.
    fn composer_changed(&mut self, area: Entity<TextArea>, _: &crate::text_input::Changed, cx: &mut Context<Self>) {
        if area.read(cx).text() == "/" {
            area.update(cx, |area, cx| area.set_text("", cx));
            self.toggle_menu(Menu::Actions, Point::default(), cx);
        }
    }

    /// Escribe `/comando ` en la caja, listo para completar o mandar.
    fn insert_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let text = format!("/{name} ");
        self.composer.update(cx, |area, cx| area.set_text(&text, cx));
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn set_appearance(&mut self, style: style::Style, mode: style::Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.configs.style = style;
        self.configs.mode = mode;
        self.configs.save();
        style::set(style, mode);
        window.set_background_appearance(background(style));
        cx.notify();
    }

    /// Si cambió el estilo o el modo, la caja de texto se rehace con sus colores
    /// (conserva lo escrito y el foco).
    fn sync_style(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t = style::t();
        if self.composer_style == (t.style, t.light) {
            return;
        }
        self.composer_style = (t.style, t.light);
        style::apply_m3(cx);
        let text = self.composer.read(cx).text().to_string();
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        self.composer = new_composer(&text, cx);
        if focused {
            self.focus_composer(window, cx);
        }
    }


    // --- Configuración -------------------------------------------------------------

    fn config(&self) -> ClaudeConfig {
        self.active_workspace().map(|id| self.configs.get(id)).unwrap_or_default()
    }

    /// Guarda la configuración del espacio activo y la aplica a sus sesiones abiertas.
    fn set_config(&mut self, change: impl FnOnce(&mut ClaudeConfig), cx: &mut Context<Self>) {
        let Some(workspace) = self.active_workspace() else {
            return;
        };
        let before = self.configs.get(workspace);
        let mut after = before.clone();
        change(&mut after);
        if after == before {
            return;
        }
        self.configs.set(workspace, after.clone());
        let live: Vec<String> = self.chats.iter().filter(|c| c.live && c.workspace == workspace).map(|c| c.key.clone()).collect();
        for key in live {
            if after.model != before.model {
                let model = (!after.model.is_empty()).then(|| after.model.clone());
                self.fire("setModel", json!({ "key": key, "model": model }), cx);
            }
            if after.permission_mode != before.permission_mode {
                self.fire("setPermissionMode", json!({ "key": key, "mode": after.permission_mode }), cx);
            }
            if after.thinking != before.thinking {
                self.fire("setThinking", json!({ "key": key, "on": after.thinking }), cx);
            }
            let flags = after.flags(Some(&before));
            if flags.as_object().is_some_and(|flags| !flags.is_empty()) {
                self.fire("applyFlags", json!({ "key": key, "settings": flags }), cx);
            }
            if after.remote_control != before.remote_control {
                let title = self.chats.iter().find(|c| c.key == key).map(|c| c.title.clone()).filter(|t| t != "Nueva conversación");
                self.fire("remoteControl", json!({ "key": key, "enabled": after.remote_control, "name": title }), cx);
            }
        }
        cx.notify();
    }

    fn model_label(&self, id: &str) -> String {
        self.models
            .iter()
            .find(|(model, _)| model == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| {
                // Hasta que llegue la lista de Claude Code: el id con mayúscula.
                let mut chars = id.chars();
                chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
            })
    }

    // --- Archivos y cambios -----------------------------------------------------------

    fn changed_files(&self) -> usize {
        self.repos.iter().map(|r| r.files.len()).sum()
    }

    fn toggle_side(&mut self, side: Side, cx: &mut Context<Self>) {
        self.side = if self.side == Some(side) && self.doc.is_none() { None } else { Some(side) };
        self.doc = None;
        cx.notify();
    }

    fn open_doc(&mut self, path: PathBuf, show_diff: bool, cx: &mut Context<Self>) {
        self.next_doc += 1;
        self.doc = Some(Doc::load(self.next_doc, &path, show_diff));
        cx.notify();
    }

    fn watch_changes(&mut self, cx: &mut Context<Self>) {
        self.refresh_changes(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(CHANGES_EVERY).await;
            if this.update(cx, |view, cx| view.refresh_changes(cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    fn refresh_changes(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspaces.active() else {
            return;
        };
        let folders = workspace.folders.clone();
        let id = workspace.id;
        let scan = cx.background_executor().spawn(async move { git::status(&folders) });
        cx.spawn(async move |this, cx| {
            let repos = scan.await;
            let _ = this.update(cx, |view, cx| {
                if view.active_workspace() == Some(id) {
                    view.repos = repos;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
