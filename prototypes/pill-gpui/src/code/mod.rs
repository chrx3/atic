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
mod edits;
mod editor;
mod enter;
mod files;
mod git;
mod highlight;
mod marks;
mod mention;
mod menus;
mod notify;
mod overlay;
mod palette;
mod permissions;
mod profile;
mod rewind;
mod settings_flat;
mod settings_m3;
mod settings_search;
mod shortcuts;
mod sidebar;
mod sidecar;
mod split;
mod style;
mod terminal;
mod tools;
mod usage;
mod view;
mod vscode;

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

use chat::{Applied, Chat};
use config::{ClaudeConfig, Configs};
use sidecar::{Incoming, Reply, Sidecar};

actions!(
    atic_code,
    [Send, PasteAttach, CloseMenu, OpenPalette, NewConversation, ToggleSidebar, ToggleSettings, Attach, SubmitAnswers, ShowFiles, ShowChanges, OpenFolder, ClearConversation]
);

/// La CPU que lleva usada el hilo actual (el de GPUI), en milisegundos.
#[cfg(windows)]
fn main_thread_cpu_ms() -> f64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    if unsafe { GetThreadTimes(GetCurrentThread(), &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return 0.;
    }
    let ticks = |t: FILETIME| ((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64) as f64;
    (ticks(kernel) + ticks(user)) / 10_000.
}

#[cfg(not(windows))]
fn main_thread_cpu_ms() -> f64 {
    0.
}

/// El contexto de teclas de la caja del chat: Enter manda, Mayús+Enter baja de línea.
const COMPOSER: &str = "CodeComposer";
/// Cada cuánto se mira qué cambió en git.
const CHANGES_EVERY: Duration = Duration::from_secs(3);

/// Los atajos salen de `shortcuts::TABLE` con lo que el usuario cambió (Configuración > Atajos).
pub fn bind_keys(cx: &mut App) {
    shortcuts::bind_keys(cx, &Configs::load().shortcuts);
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
    /// `image` es la misma imagen ya lista para su miniatura.
    Image { name: String, media_type: &'static str, data: String, image: Arc<gpui::Image> },
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

/// Como mucho, las imágenes de un mensaje (como en la referencia).
const MAX_IMAGES: usize = 10;
/// Dos Esc seguidos dentro de este tiempo abren el Rewind.
const DOUBLE_ESC: Duration = Duration::from_millis(500);

/// El texto que se manda, como `send` de la referencia: el contexto del editor, los archivos
/// adjuntos como `@ruta` al final y, si solo hay imágenes, un texto por defecto.
fn compose_message(text: &str, context: Option<&str>, files: &[String], images: bool) -> String {
    let mut full = text.to_string();
    // El archivo y las líneas del editor a la vista (la referencia: `(Contexto: @ruta, líneas 3-9)`).
    if let (Some(note), false) = (context, text.is_empty()) {
        full = format!("{text}\n\n{note}");
    }
    if !files.is_empty() {
        let base = if text.is_empty() { "Revisa los archivos adjuntos." } else { full.as_str() };
        let list: Vec<String> = files.iter().map(|f| format!("@{f}")).collect();
        full = format!("{base}\n\n(Archivos adjuntos: {})", list.join(" "));
    }
    if full.is_empty() && images {
        full = "Mira la imagen adjunta.".into();
    }
    full
}

/// El borrador con `text` al final: separado por un espacio si lo escrito no
/// termina en uno (como `insert` de la referencia).
fn append_draft(draft: &str, text: &str) -> String {
    if draft.is_empty() || draft.ends_with(char::is_whitespace) {
        format!("{draft}{text}")
    } else {
        format!("{draft} {text}")
    }
}

/// La línea de estado bajo la caja, como la de la referencia: modelo · contexto N% ·
/// $costo y, si ya hubo una respuesta, cuánto tardó.
fn status_line(model: &str, context: Option<(u64, u64)>, cost: f64, last_ms: Option<u64>) -> String {
    let pct = context.filter(|(_, max)| *max > 0).map_or(0, |(used, max)| (used as f64 / max as f64 * 100.).round() as u64);
    let mut line = format!("{model} · contexto {pct}% · ${cost:.2}");
    if let Some(ms) = last_ms {
        line.push_str(&format!(" · {:.1} s", ms as f64 / 1000.));
    }
    line
}

/// La ruta de un adjunto relativa a la carpeta del proyecto que la contiene
/// (con `/`, como las menciones de Claude Code); fuera de ellas, la completa.
fn relative_to(path: &std::path::Path, roots: &[PathBuf]) -> String {
    for root in roots {
        if let Ok(rest) = path.strip_prefix(root) {
            let rest = rest.to_string_lossy().replace('\\', "/");
            if !rest.is_empty() {
                return rest;
            }
        }
    }
    path.display().to_string()
}

/// La misma ruta aunque cambien las barras o las mayúsculas (Windows).
fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    let norm = |p: &std::path::Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// El «espacio» de los chats sueltos (sin proyecto): no está en la lista de
/// espacios. Corren en una carpeta propia para no mezclarse con ningún
/// proyecto, como `~/.referencia/chats` en la referencia.
pub const LOOSE: u64 = u64::MAX;
/// Lo que se le dice a Claude en un chat suelto (el de la referencia).
/// La sesión de sondeo: una de Claude Code abierta sin mensajes, solo para saber qué
/// modelos hay (con sus nombres completos), la cuenta y el uso antes de la primera
/// conversación. Es el precalentado de la referencia; no gasta nada hasta que se le escribe, y
/// nunca se le escribe.
const PROBE: &str = "__probe";
const LOOSE_CONTEXT: &str = "El usuario abrió un chat suelto en Atic Code, sin proyecto: es una conversación general. No asumas que hay un repositorio o código de por medio salvo que lo mencione.";

/// La carpeta de los chats sueltos, dentro de los datos de Atic.
fn loose_dir() -> Option<PathBuf> {
    crate::paths::file("code-chats")
}

#[derive(Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub title: String,
    /// La última actividad (segundos o milisegundos desde 1970).
    pub modified: Option<f64>,
    /// La rama de git en la que quedó, si Claude Code la anotó.
    pub branch: Option<String>,
}

/// Como mucho estas conversaciones con su proceso de Claude abierto
/// (`MAX_LIVE` de la referencia): las demás se retoman al escribirles.
const MAX_LIVE: usize = 4;

/// Las conversaciones abiertas que hay que cerrar para quedar en `max`: las
/// que llevan más tiempo sin usarse, nunca la visible. `live` trae, por cada
/// abierta, su clave, su último uso y si se puede cerrar (no trabaja, no
/// espera un permiso y tiene sesión para retomarla).
fn idle_to_close(live: &[(String, std::time::Instant, bool)], active: Option<&str>, max: usize) -> Vec<String> {
    if live.len() <= max {
        return Vec::new();
    }
    let mut idle: Vec<&(String, std::time::Instant, bool)> =
        live.iter().filter(|(key, _, closable)| *closable && Some(key.as_str()) != active).collect();
    idle.sort_by_key(|(_, used, _)| *used);
    idle.into_iter().take(live.len() - max).map(|(key, _, _)| key.clone()).collect()
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
    /// Los grupos del panel de cambios plegados (por raíz).
    collapsed_repos: HashSet<PathBuf>,
    /// Se está actualizando git a pedido (el spinner del panel).
    refreshing: bool,
    /// La búsqueda del panel Archivos, el índice (de qué carpetas y sus
    /// archivos) y si se está armando.
    file_search: Entity<gpui_m3::TextField>,
    file_index: Option<(Vec<PathBuf>, Arc<Vec<files::Indexed>>)>,
    indexing: bool,
    /// El panel de la derecha, si está abierto.
    side: Option<Side>,
    doc: Option<Doc>,
    /// Los archivos abiertos en el editor (pestañas).
    tabs: editor::Tabs,
    /// El último archivo abierto: queda resaltado en el árbol.
    active_file: Option<PathBuf>,
    composer: Entity<TextArea>,
    /// La lista de @-menciones abierta (y lo último que mostró, para su salida).
    mention: Option<mention::Mention>,
    mention_last: overlay::Last<mention::MentionShown>,
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
    /// La ventana está al frente (si no, los avisos van al sistema, `notify.rs`).
    window_active: bool,
    /// El menú abierto y dónde se pidió.
    menu: Option<(Menu, Point<Pixels>)>,
    /// Lo último que mostraron las capas flotantes, para que salgan animadas.
    menu_last: overlay::Last<(Menu, Point<Pixels>)>,
    session_menu_last: overlay::Last<(sidebar::SessionRef, Point<Pixels>)>,
    space_menu_last: overlay::Last<(u64, Point<Pixels>)>,
    pop_last: overlay::Last<usage::Pop>,
    new_space_last: overlay::Last<Vec<PathBuf>>,
    toast_last: overlay::Last<gpui::SharedString>,
    attachments: Vec<Attachment>,
    /// Los comandos de Claude Code (de `meta`): nombre y descripción.
    commands: Vec<(String, String)>,
    /// Con qué estilo, modo y acento se armó la caja de texto (cambian sus colores).
    composer_style: (style::Style, bool, Option<u32>),
    /// Un cambio de estilo o modo esperando al revelado circular: se aplica cuando el círculo
    /// cubre la ventana y gpui-m3 cambia su esquema (`theme_reveal`).
    pending_look: Option<(style::Style, style::Mode, gpui_m3::Scheme)>,
    /// El cambio de colores que viene ya se animó (el revelado): no mezclar otra vez.
    skip_blend: bool,
    /// En Apariencia: el selector de color propio abierto.
    accent_picker: bool,
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
    /// La pestaña «Atajos»: qué atajo espera tecla y el último aviso.
    shortcuts_ui: shortcuts::Ui,
    /// El buscador de la Configuración: filtra los ajustes de todas las pestañas.
    settings_search: Entity<gpui_m3::TextField>,
    claude_info: Option<settings_m3::ClaudeInfo>,
    updating_claude: bool,
    update_log: String,
    update_result: Option<(bool, String)>,
    account: Option<(Option<String>, Option<String>, Option<String>)>,
    /// «Cuenta y uso» o el mapa de agentes, y lo que muestra el primero.
    pop: Option<usage::Pop>,
    usage: Option<usage::UsageInfo>,
    /// El aviso del último evento `rate_limit` (chip bajo la caja).
    rate_alert: Option<usage::RateAlert>,
    /// El reloj del mapa de agentes (cada apertura empieza uno nuevo) y si
    /// se ven los terminados.
    agent_clock: u64,
    agents_done_open: bool,
    /// La barra lateral de Expressive: abierta o el riel, la página de
    /// historial, el menú contextual y el renombrar en el sitio.
    sidebar_open: bool,
    history_page: bool,
    /// Los chats sueltos de la barra: todos o los últimos cinco.
    loose_all: bool,
    session_menu: Option<(sidebar::SessionRef, Point<Pixels>)>,
    renaming: Option<sidebar::SessionRef>,
    rename_field: Entity<gpui_m3::TextField>,
    /// Se renombra desde el título de la barra superior (no desde la lista).
    rename_in_header: bool,
    /// El campo de renombrar tuvo el foco: al perderlo se guarda (como la referencia).
    rename_had_focus: bool,
    /// Un espacio: su menú contextual y el renombrar en el sitio.
    space_menu: Option<(u64, Point<Pixels>)>,
    renaming_space: Option<u64>,
    space_rename_field: Entity<gpui_m3::TextField>,
    space_rename_had_focus: bool,
    /// El diálogo «Nuevo espacio», con las carpetas elegidas, y su nombre.
    new_space: Option<Vec<PathBuf>>,
    space_name_field: Entity<gpui_m3::TextField>,
    /// El título que tiene la ventana («espacio — Atic Code»).
    window_title: String,
    history_search: Entity<gpui_m3::TextField>,
    /// La conversación del historial con el «Eliminar» armado.
    confirm_delete: Option<String>,
    user_name: Option<String>,
    /// El perfil (nombre y foto), su tarjeta abierta (dónde se pidió), el campo
    /// del nombre y el recorte de la foto elegida.
    profile: profile::Profile,
    profile_card: Option<Point<Pixels>>,
    profile_last: overlay::Last<Point<Pixels>>,
    /// El menú rápido de Apariencia (junto al botón de la barra).
    style_menu: Option<Point<Pixels>>,
    style_menu_last: overlay::Last<Point<Pixels>>,
    profile_field: Entity<gpui_m3::TextField>,
    cropper: Option<Entity<gpui_m3::ImageCropper>>,
    cropper_last: overlay::Last<Entity<gpui_m3::ImageCropper>>,
    /// Mensajes enviados: cada uno hace despegar el botón de enviar.
    sends: u64,
    /// La caja de texto tiene el foco (en Expressive cambia de fondo).
    composer_focused: bool,
    /// Dónde quedó la caja de texto en el último cuadro: en Expressive los
    /// menús se abren sobre ella, como en la referencia.
    composer_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Los mensajes marcados y las conversaciones destacadas.
    marks: marks::Marks,
    /// El último Esc (dos seguidos abren el Rewind).
    last_esc: Option<std::time::Instant>,
    /// Dónde quedó cada mensaje marcado en el último cuadro (fila → límites),
    /// para «Siguiente mensaje marcado»; y el que destella.
    flag_bounds: Rc<std::cell::RefCell<HashMap<usize, Bounds<Pixels>>>>,
    flash: Option<usize>,
    flash_gen: u64,
    /// El historial muestra solo las conversaciones con marcador.
    history_marked: bool,
    /// Un error del agente que no es de una conversación.
    error: Option<String>,
    /// La terminal integrada, bajo el chat.
    terminals: Entity<terminal::Terminals>,
    /// La sesión de sondeo (`PROBE`) está abierta o abriéndose.
    probe_live: bool,
    /// El cursor está sobre las formas del inicio (se vuelven formas M3).
    hero_hover: bool,
    /// Los espacios cuyo historial se está pidiendo (para no pedirlo dos veces).
    history_loading: HashSet<u64>,
    /// Cuántas veces se dibujó la ventana desde `frames_since`, y cuántos tramos
    /// seguidos fueron de más (ver `note_frame`).
    frames: u32,
    /// Partes de más que se dibujan en cada hilo (`first_shown` en `view.rs`).
    older: HashMap<String, usize>,
    frames_since: std::time::Instant,
    /// CPU del hilo principal (ms) al empezar el tramo.
    frames_cpu: f64,
    hot_spells: u32,
    /// La conversación que se ve al lado de la activa (`split.rs`), de qué lado va,
    /// su desplazamiento y los borradores de las que no tienen la caja.
    split: Option<String>,
    split_left: bool,
    split_thread: ScrollHandle,
    drafts: HashMap<String, String>,
    /// Se está arrastrando una conversación de la barra (se ven las mitades para soltarla).
    dragging_chat: bool,
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

fn appearance_light(appearance: gpui::WindowAppearance) -> bool {
    matches!(appearance, gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight)
}

/// Lleva a gpui-m3 el «reducir movimiento» de Windows: sus animaciones quedan quietas.
fn set_reduced_motion(cx: &mut App) {
    let reduced = style::system_reduced_motion();
    let mut motion = gpui_m3::MotionSettings::get(cx);
    if motion.reduced != reduced {
        motion.reduced = reduced;
        cx.set_global(motion);
    }
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
    // Enter con texto rechaza (o sigue planificando) con la indicación; Esc rechaza.
    cx.subscribe(&field, |view, _, event: &gpui_m3::TextFieldEvent, cx| {
        use permissions::{PermAt, PermKey};
        match event {
            gpui_m3::TextFieldEvent::Submitted(text) => {
                view.permission_keypress(PermKey::Enter, PermAt::Field { empty: text.trim().is_empty() }, None, cx);
            }
            gpui_m3::TextFieldEvent::Cancelled => {
                view.permission_keypress(PermKey::Escape, PermAt::Field { empty: true }, None, cx);
            }
            gpui_m3::TextFieldEvent::Changed(_) => {}
        }
        cx.notify();
    })
    .detach();
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

        // «Sistema» sigue a Windows, también si cambia con la ventana abierta.
        style::set_system_light(appearance_light(window.appearance()));
        cx.observe_window_appearance(window, |_, window, cx| {
            style::set_system_light(appearance_light(window.appearance()));
            cx.notify();
        })
        .detach();
        // Movimiento reducido si Windows lo pide (se vuelve a mirar al activar la ventana).
        set_reduced_motion(cx);
        cx.observe_window_activation(window, |view, window, cx| {
            view.window_active = window.is_window_active();
            if view.window_active {
                set_reduced_motion(cx);
            }
        })
        .detach();
        let configs = Configs::load();
        style::set_accent(configs.active_accent(Workspaces::load().active_id()));
        style::apply_m3(cx);
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
        let space_rename_field = cx.new(|cx| gpui_m3::TextField::new(cx).inline().select_all_on_focus(true).restore_on_cancel(true));
        cx.subscribe(&space_rename_field, |view: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
            gpui_m3::TextFieldEvent::Submitted(_) => view.commit_space_rename(cx),
            gpui_m3::TextFieldEvent::Cancelled => view.cancel_space_rename(cx),
            gpui_m3::TextFieldEvent::Changed(_) => {}
        })
        .detach();
        let terminals = cx.new(|cx| terminal::Terminals::new(configs.terminal_height, cx));
        cx.subscribe_in(&terminals, window, |view: &mut Self, _, event: &terminal::TerminalEvent, window, cx| match event {
            terminal::TerminalEvent::Height(height) => view.configs.set_terminal_height(*height),
            terminal::TerminalEvent::Hidden => view.focus_composer(window, cx),
            terminal::TerminalEvent::Error(error) => view.show_toast(error.clone(), cx),
        })
        .detach();
        let space_name_field = cx.new(|cx| gpui_m3::TextField::new(cx).placeholder("Nombre del espacio"));
        cx.subscribe(&space_name_field, |view: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
            gpui_m3::TextFieldEvent::Submitted(_) => view.create_space(cx),
            gpui_m3::TextFieldEvent::Cancelled => {
                view.new_space = None;
                cx.notify();
            }
            gpui_m3::TextFieldEvent::Changed(_) => {}
        })
        .detach();
        let file_search = cx.new(|cx| gpui_m3::TextField::new(cx).compact().placeholder("Buscar archivos").icon("search"));
        cx.subscribe(&file_search, |view: &mut Self, field, event: &gpui_m3::TextFieldEvent, cx| {
            match event {
                // Esc limpia la búsqueda, como en la referencia.
                gpui_m3::TextFieldEvent::Cancelled => field.update(cx, |field, cx| field.set_text("", cx)),
                gpui_m3::TextFieldEvent::Changed(text) if !text.trim().is_empty() => view.ensure_index(cx),
                _ => {}
            }
            cx.notify();
        })
        .detach();
        let history_search = cx.new(|cx| gpui_m3::TextField::new(cx).placeholder("Buscar conversaciones").icon("search"));
        cx.subscribe(&history_search, |_, _, _: &gpui_m3::TextFieldEvent, cx| cx.notify()).detach();

        let settings_search = cx.new(|cx| gpui_m3::TextField::new(cx).placeholder("Buscar en la configuración…").icon("search"));
        cx.subscribe(&settings_search, |_, field, event: &gpui_m3::TextFieldEvent, cx| {
            // Esc borra la búsqueda y vuelve a la pestaña.
            if matches!(event, gpui_m3::TextFieldEvent::Cancelled) {
                field.update(cx, |field, cx| field.set_text("", cx));
            }
            cx.notify();
        })
        .detach();

        let mut view = Self {
            focus,
            workspaces: Workspaces::load(),
            configs: Configs::load(),
            sidecar,
            chats: Vec::new(),
            active: None,
            history: HashMap::new(),
            explorer: Explorer::for_code(),
            repos: Vec::new(),
            collapsed_repos: HashSet::new(),
            refreshing: false,
            file_search,
            file_index: None,
            indexing: false,
            side: None,
            doc: None,
            tabs: Default::default(),
            active_file: None,
            composer,
            mention: None,
            mention_last: Default::default(),
            thread: ScrollHandle::new(),
            follow: true,
            expanded: HashSet::new(),
            models: config::MODELS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect(),
            settings_open: false,
            more_models: false,
            claude_path: sidecar::claude_path(),
            window_active: true,
            menu: None,
            menu_last: Default::default(),
            session_menu_last: Default::default(),
            space_menu_last: Default::default(),
            pop_last: Default::default(),
            new_space_last: Default::default(),
            toast_last: Default::default(),
            attachments: Vec::new(),
            commands: Vec::new(),
            composer_style: (t.style, t.light, style::accent()),
            pending_look: None,
            skip_blend: false,
            accent_picker: false,
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
            shortcuts_ui: Default::default(),
            settings_search,
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
            rate_alert: None,
            agent_clock: 0,
            agents_done_open: false,
            sidebar_open: true,
            history_page: false,
            loose_all: false,
            session_menu: None,
            renaming: None,
            rename_field,
            rename_in_header: false,
            rename_had_focus: false,
            space_menu: None,
            renaming_space: None,
            space_rename_field,
            space_rename_had_focus: false,
            new_space: None,
            space_name_field,
            window_title: String::new(),
            history_search,
            confirm_delete: None,
            user_name: sidebar::git_user_name(),
            profile: profile::Profile::load(),
            profile_card: None,
            profile_last: Default::default(),
            style_menu: None,
            style_menu_last: Default::default(),
            profile_field: profile::name_field(cx),
            cropper: None,
            cropper_last: Default::default(),
            ask_other: HashMap::new(),
            composer_focused: false,
            composer_bounds: Rc::new(Cell::new(None)),
            marks: marks::Marks::load(),
            last_esc: None,
            flag_bounds: Rc::default(),
            flash: None,
            flash_gen: 0,
            history_marked: false,
            error: None,
            next_key: 0,
            next_doc: 0,
            terminals,
            probe_live: false,
            hero_hover: false,
            history_loading: HashSet::new(),
            frames: 0,
            older: HashMap::new(),
            frames_since: std::time::Instant::now(),
            frames_cpu: main_thread_cpu_ms(),
            hot_spells: 0,
            split: None,
            split_left: false,
            split_thread: ScrollHandle::new(),
            drafts: HashMap::new(),
            dragging_chat: false,
        };
        if let Some(id) = view.active_workspace() {
            view.expand_first_open(id);
            view.warn_missing_folders(id, cx);
        }
        view.load_history(cx);
        view.load_history_for(LOOSE, cx);
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
                self.probe_live = false;
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
                    "rate_limit" => self.read_rate_limit(&data),
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
                // Un turno que el usuario detuvo no se avisa (el resultado limpia la marca).
                let interrupted = chat.interrupted;
                // Las marcas hechas antes del primer mensaje pasan a la sesión.
                if event == "session" && chat.session_id.is_none() {
                    if let Some(session) = data.get("sessionId").and_then(Value::as_str) {
                        self.marks.move_flags(&chat.key, session);
                    }
                }
                chat.apply(&event, &data);
                let workspace = chat.workspace;
                let edited = std::mem::take(&mut chat.edited);
                if let Some(doc) = self.doc.as_mut().filter(|doc| edited.iter().any(|path| same_path(path, &doc.path))) {
                    doc.reload();
                }
                if event == "assistant" && data.get("parent").is_none_or(Value::is_null) {
                    let model = data.get("model").and_then(Value::as_str).unwrap_or_default();
                    if let Some(want) = chat.check_model(&self.models, model) {
                        self.fire("setModel", json!({ "key": key, "model": want }), cx);
                    }
                }
                // Claude editó archivos abiertos en el editor: se recargan o se avisa.
                self.files_edited(&edited, cx);
                if event == "external" {
                    self.resync(&key, cx);
                }
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
                    // Al terminar cada respuesta, el panel de cambios se pone al día
                    // y el índice de archivos se rehace la próxima vez que se busque.
                    self.refresh_changes_now(cx);
                    self.file_index = None;
                }
                // Terminó, falló o pide un permiso con la ventana atrás: aviso del sistema.
                self.system_alert(&key, &event, &data, interrupted);
                if self.active.as_deref() == Some(key.as_str()) && self.follow {
                    self.thread.scroll_to_bottom();
                }
                // La de al lado también sigue el final mientras escribe.
                if self.split.as_deref() == Some(key.as_str()) {
                    self.split_thread.scroll_to_bottom();
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
            // Los correos van tapados a medias: la configuración se muestra en pantalla
            // (capturas, llamadas) y basta con reconocer la cuenta.
            let field = |name: &str| account.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()).map(config::mask_emails);
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

    /// Donde corren las conversaciones de un espacio: su primera carpeta o,
    /// para los chats sueltos, la carpeta propia.
    fn workspace_dir(&self, workspace: u64) -> Option<PathBuf> {
        if workspace == LOOSE {
            return loose_dir();
        }
        self.workspaces.get(workspace).and_then(|w| w.main().cloned())
    }

    /// La conversación visible es un chat suelto.
    fn in_loose_chat(&self) -> bool {
        self.active_chat().is_some_and(|c| c.workspace == LOOSE)
    }

    fn load_history_for(&mut self, workspace: u64, cx: &mut Context<Self>) {
        let Some(dir) = self.workspace_dir(workspace) else {
            // Un espacio sin carpetas no tiene conversaciones que listar.
            self.history.insert(workspace, Vec::new());
            return;
        };
        // Sin chats sueltos todavía, la carpeta no existe: no hay nada que listar.
        if workspace == LOOSE && !dir.is_dir() {
            self.history.insert(LOOSE, Vec::new());
            return;
        }
        let params = json!({ "dir": dir, "limit": 40 });
        self.history_loading.insert(workspace);
        self.request("listSessions", params, cx, move |view, reply, _| match {
            view.history_loading.remove(&workspace);
            reply
        } {
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
                            branch: text("gitBranch").map(str::to_string),
                        })
                    })
                    .collect();
                view.history.insert(workspace, sessions);
            }
            // Sin lista, la barra dice «Sin conversaciones» en vez de quedar cargando.
            Err(error) => {
                view.history.insert(workspace, Vec::new());
                view.error = Some(error);
            }
        });
    }

    /// Cuenta los cuadros. Si la ventana se dibuja más de 30 veces por segundo, o el hilo
    /// principal pasa más del 60 % ocupado, durante 4 s seguidos, deja en el registro el
    /// ritmo, cuánta CPU cuesta cada cuadro, cuántas partes tienen los hilos a la vista y
    /// quién pide los cuadros (las animaciones de gpui-m3, por archivo y línea).
    pub(super) fn note_frame(&mut self) {
        self.frames += 1;
        let elapsed = self.frames_since.elapsed().as_secs_f32();
        if elapsed < 2. {
            return;
        }
        let rate = self.frames as f32 / elapsed;
        let cpu = main_thread_cpu_ms();
        let busy = ((cpu - self.frames_cpu) / (elapsed as f64 * 1000.)) as f32;
        let per_frame = (cpu - self.frames_cpu) / self.frames as f64;
        let requests = gpui_m3::motion::take_frame_requests();
        self.frames = 0;
        self.frames_since = std::time::Instant::now();
        self.frames_cpu = cpu;
        if rate < 30. && busy < 0.6 {
            self.hot_spells = 0;
            return;
        }
        let shown = |key: Option<&String>| key.and_then(|k| self.chats.iter().find(|c| &c.key == k)).map_or(0, |c| c.items.len());
        self.hot_spells += 1;
        if self.hot_spells == 2 || self.hot_spells % 15 == 0 {
            let top: Vec<String> = requests.iter().take(8).map(|(at, n)| format!("{at} ×{n}")).collect();
            tracing::warn!(
                cuadros_por_s = format!("{rate:.0}"),
                hilo_ocupado = format!("{:.0} %", busy * 100.),
                ms_por_cuadro = format!("{per_frame:.0}"),
                partes = format!("{} + {}", shown(self.active.as_ref()), shown(self.split.as_ref())),
                mezcla = style::blending(),
                dividido = self.split.is_some(),
                trabajando = self.chats.iter().filter(|c| c.busy).count(),
                pedidos = %top.join(", "),
                "Atic Code se redibuja sin parar"
            );
        }
    }

    /// Los espacios desplegados en la barra sin su historial lo piden. Al abrir la
    /// app solo se pedía el del espacio activo, y uno que ya venía desplegado se
    /// quedaba en «Cargando…» hasta plegarlo y desplegarlo.
    pub(super) fn load_missing_history(&mut self, cx: &mut Context<Self>) {
        let missing: Vec<u64> = self
            .workspaces
            .list()
            .iter()
            .filter(|w| !w.collapsed && !self.history.contains_key(&w.id) && !self.history_loading.contains(&w.id))
            .map(|w| w.id)
            .collect();
        for workspace in missing {
            self.load_history_for(workspace, cx);
        }
    }

    /// Al abrir un espacio por primera vez se despliega; si después se pliega,
    /// se respeta (`expandOnOpen` de la referencia).
    fn expand_first_open(&mut self, id: u64) {
        if self.configs.first_open(id) {
            self.workspaces.set_collapsed(id, false);
        }
    }

    fn select_workspace(&mut self, id: u64, cx: &mut Context<Self>) {
        self.workspaces.select(id);
        self.expand_first_open(id);
        if self.active_chat().is_some_and(|c| c.workspace != id) {
            self.active = None;
        }
        self.doc = None;
        self.leave_workspace_tabs(cx);
        self.active_file = None;
        self.repos.clear();
        self.file_index = None;
        self.explorer.refresh();
        self.load_history_for(id, cx);
        self.refresh_changes(cx);
        self.warn_missing_folders(id, cx);
        cx.notify();
    }

    /// Avisa de las carpetas del espacio que ya no existen (`store.ts:102` de la referencia).
    fn warn_missing_folders(&mut self, id: u64, cx: &mut Context<Self>) {
        let note = self.workspaces.get(id).and_then(|w| vscode::missing_note(&w.folders));
        if let Some(note) = note {
            self.show_toast(note, cx);
        }
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

    /// Un chat suelto nuevo, sin proyecto (`newLooseChat` de la referencia). La sesión
    /// se abre al mandar el primer mensaje, como las demás.
    pub(super) fn new_loose_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_page = false;
        let Some(dir) = loose_dir() else {
            self.show_toast("No se encontró la carpeta de datos de Atic", cx);
            return;
        };
        // Los chats sueltos vacíos que quedaron sin usar se descartan.
        let current = self.active.clone();
        self.chats.retain(|c| c.workspace != LOOSE || !c.items.is_empty() || c.live || c.session_id.is_some() || Some(&c.key) == current.as_ref());
        if self.active_chat().is_some_and(|c| c.workspace == LOOSE && c.items.is_empty() && c.session_id.is_none()) {
            self.focus_composer(window, cx);
            return;
        }
        let key = self.new_key();
        self.chats.push(Chat::new(key.clone(), LOOSE, dir));
        self.active = Some(key);
        self.follow = true;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Abre la sesión de sondeo si no está: así el menú de modelos y «Cuenta y uso» tienen
    /// datos desde el inicio, sin esperar a la primera conversación.
    pub(super) fn ensure_probe(&mut self, cx: &mut Context<Self>) {
        if self.probe_live {
            return;
        }
        self.probe_live = true;
        let cwd = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        self.request("start", json!({ "key": PROBE, "cwd": cwd }), cx, |view, reply, cx| {
            if reply.is_err() {
                view.probe_live = false;
            }
            cx.notify();
        });
    }

    /// Una sesión viva para preguntar por la cuenta y el uso: la de la conversación a la
    /// vista o, si no hay, la de sondeo.
    pub(super) fn info_key(&self) -> Option<String> {
        self.active_chat().filter(|c| c.live).map(|c| c.key.clone()).or_else(|| self.probe_live.then(|| PROBE.to_string()))
    }

    /// Dónde abre la terminal: la carpeta de la conversación a la vista o, si
    /// no hay, la primera del proyecto abierto.
    fn terminal_dir(&self) -> Option<PathBuf> {
        match self.active_chat() {
            Some(chat) => self.workspace_dir(chat.workspace),
            None => self.workspaces.active().and_then(|w| w.main().cloned()),
        }
    }

    /// Ctrl+J, Ctrl+` y el botón Terminal: la muestra u oculta (`toggleTerminal`).
    fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.terminal_dir();
        let result = self.terminals.update(cx, |terminals, cx| {
            terminals.cwd = cwd;
            terminals.toggle(window, cx)
        });
        if let Err(error) = result {
            self.show_toast(error, cx);
        }
        cx.notify();
    }

    fn toggle_terminal_action(&mut self, _: &terminal::ToggleTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_terminal(window, cx);
    }

    /// Corre `command` en una pestaña nueva de la terminal integrada (`inTerminal` de la referencia).
    fn run_in_terminal(&mut self, command: String, name: &str, cwd: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = cwd.or_else(|| self.terminal_dir());
        let result = self.terminals.update(cx, |terminals, cx| {
            terminals.cwd = cwd;
            terminals.open(Some(command), Some(name.to_string()), window, cx)
        });
        if let Err(error) = result {
            self.show_toast(error, cx);
        }
        cx.notify();
    }

    fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.read(cx).focus_handle(cx).focus(window);
    }

    fn select_chat(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        // La de al lado: pasa a ser la activa sin dejar de verse la otra.
        if self.split.as_deref() == Some(key.as_str()) {
            self.focus_split(window, cx);
            return;
        }
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.unread = false;
            chat.used_at = std::time::Instant::now();
        }
        self.active = Some(key.clone());
        self.limit_live(cx);
        self.sync_chat_settings(&key, cx);
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
        let Some(cwd) = self.workspace_dir(workspace) else {
            return;
        };
        let key = self.new_key();
        let mut chat = Chat::new(key.clone(), workspace, cwd.clone());
        chat.session_id = Some(info.session_id.clone());
        chat.title = info.title.clone();
        self.chats.push(chat);
        self.select_chat(key.clone(), window, cx);
        let params = json!({ "sessionId": info.session_id, "dir": cwd });
        // Retoma el modelo y el esfuerzo con los que terminó (`adoptSessionSettings` de la referencia).
        let settings_key = key.clone();
        self.request("sessionSettings", params.clone(), cx, move |view, reply, cx| {
            let Ok(last) = reply else {
                return;
            };
            let Some(model) = last.get("model").and_then(Value::as_str).filter(|m| !m.is_empty()) else {
                return;
            };
            let Some(chat) = view.chats.iter_mut().find(|c| c.key == settings_key) else {
                return;
            };
            chat.want_model = Some(model.to_string());
            if let Some(effort) = last.get("effort").and_then(Value::as_str).filter(|e| !e.is_empty()) {
                chat.want_effort = Some(effort.to_string());
            }
            chat.sent_model = Some(model.to_string());
            view.sync_chat_settings(&settings_key, cx);
            cx.notify();
        });
        self.request("sessionMessages", params, cx, move |view, reply, _| {
            let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) else {
                return;
            };
            match reply {
                Ok(messages) => chat.load_history(&view.models, messages.as_array().map(Vec::as_slice).unwrap_or_default(), None),
                Err(error) => chat.notice(format!("No se pudo leer la conversación: {error}"), true),
            }
            if view.active.as_deref() == Some(key.as_str()) {
                view.thread.scroll_to_bottom();
            }
        });
    }

    /// Deja como mucho `MAX_LIVE` procesos de Claude abiertos: cierra los de
    /// las conversaciones inactivas, que siguen en la lista y se retoman al
    /// escribirles.
    fn limit_live(&mut self, cx: &mut Context<Self>) {
        let live: Vec<(String, std::time::Instant, bool)> = self
            .chats
            .iter()
            .filter(|c| c.live)
            .map(|c| (c.key.clone(), c.used_at, !c.busy && c.permissions.is_empty() && c.session_id.is_some()))
            .collect();
        for key in idle_to_close(&live, self.active.as_deref(), MAX_LIVE) {
            self.fire("close", json!({ "key": key }), cx);
            if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
                chat.live = false;
                chat.busy = false;
                chat.starting = false;
                chat.applied = None;
            }
        }
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
        if self.split.as_deref() == Some(key) {
            self.split = None;
        }
        cx.notify();
    }

    /// Una conversación sin mensajes ni sesión cuyo proceso sigue abierto: se puede
    /// reiniciar sin perder nada (`refreshIdleChat` de la referencia).
    fn is_idle_chat(chat: &Chat) -> bool {
        chat.live && chat.items.is_empty() && chat.session_id.is_none() && chat.fork.is_none() && !chat.busy
    }

    /// Cierra el proceso de la conversación visible si está vacía: la próxima vez que
    /// se escriba se abre otro con la configuración y la versión de Claude de ahora.
    /// Devuelve si lo reinició.
    pub(super) fn restart_idle_chat(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.active.clone().filter(|key| self.chats.iter().any(|c| &c.key == key && Self::is_idle_chat(c))) else {
            return false;
        };
        self.fire("close", json!({ "key": key }), cx);
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.live = false;
            chat.starting = false;
            chat.applied = None;
        }
        cx.notify();
        true
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
        if chat.workspace == LOOSE {
            // La carpeta propia se crea con la primera sesión.
            params["createCwd"] = json!(true);
            params["appendSystemPrompt"] = json!(LOOSE_CONTEXT);
        }
        if !extras.is_empty() {
            params["additionalDirectories"] = json!(extras);
        }
        if names.len() > 1 {
            params["appendSystemPrompt"] =
                json!(format!("Este proyecto tiene varias carpetas; puedes leer y editar en todas:\n{}", names.join("\n")));
        }
        if let Some(session) = &chat.session_id {
            params["resume"] = json!(session);
        } else if let Some((session, at)) = &chat.fork {
            // Una rama del Rewind: sigue desde ese mensaje en una sesión nueva.
            params["resume"] = json!(session);
            params["resumeSessionAt"] = json!(at);
            params["forkSession"] = json!(true);
        }
        self.configs.get(chat.workspace).start_params(&mut params);
        // Con el modelo y el esfuerzo de esta conversación, no los del espacio.
        for (field, want) in [("model", &chat.want_model), ("effort", &chat.want_effort)] {
            match want.as_deref() {
                Some("") => {
                    if let Some(params) = params.as_object_mut() {
                        params.remove(field);
                    }
                }
                Some(value) => params[field] = json!(value),
                None => {}
            }
        }
        let text = |field: &str| params.get(field).and_then(Value::as_str).unwrap_or_default().to_string();
        let started = Applied { model: text("model"), effort: text("effort") };
        let key = key.to_string();
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.live = true;
            chat.starting = true;
            chat.applied = None;
        }
        self.request("start", params, cx, move |view, reply, cx| {
            let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) else {
                return;
            };
            match reply {
                Ok(reply) => {
                    // Una sesión que seguía abierta no vuelve a mandar `meta`.
                    if reply.get("reused").and_then(Value::as_bool).unwrap_or(false) {
                        chat.starting = false;
                    }
                    chat.applied = Some(started);
                    // Si se eligió otro modelo mientras abría, se le aplica ahora.
                    view.sync_chat_settings(&key, cx);
                }
                Err(error) => {
                    chat.live = false;
                    chat.busy = false;
                    chat.starting = false;
                    chat.notice(format!("No se pudo abrir la sesión: {error}"), true);
                }
            }
        });
    }

    fn send(&mut self, _: &Send, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.composer.read(cx).text().trim().to_string();
        if typed.is_empty() && self.attachments.is_empty() {
            // Con la caja vacía, Enter responde la solicitud de permiso pendiente.
            self.permission_keypress(permissions::PermKey::Enter, permissions::PermAt::Composer, None, cx);
            return;
        }
        // En un chat suelto los adjuntos van con su ruta completa.
        let roots = match self.active_chat() {
            Some(chat) if chat.workspace == LOOSE => Vec::new(),
            chat => chat
                .and_then(|c| self.workspaces.get(c.workspace))
                .or_else(|| self.workspaces.active())
                .map(|w| w.folders.clone())
                .unwrap_or_default(),
        };
        let mut images = Vec::new();
        let mut thumbs = Vec::new();
        let mut files = Vec::new();
        for attachment in &self.attachments {
            match attachment {
                Attachment::Image { media_type, data, image, .. } => {
                    images.push(json!({ "mediaType": media_type, "data": data }));
                    thumbs.push(image.clone());
                }
                Attachment::File(path) => files.push(relative_to(path, &roots)),
            }
        }
        let note = self
            .editor_context(cx)
            .and_then(|ctx| editor::context_note(&typed, &relative_to(&ctx.path, &roots), ctx.lines));
        let text = compose_message(&typed, note.as_deref(), &files, !images.is_empty());
        // Con un turno en curso, el mensaje queda en la cola del sidecar y se
        // responde al terminar (como en la referencia).
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
        // Si se cambió de modelo desde el último mensaje, la marca queda en la conversación.
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            let model = chat.chosen_model(&self.configs.get(chat.workspace));
            chat.note_model_for_send(&self.models, &model);
        }
        self.ensure_live(&key, cx);
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) {
            chat.push_user(&text, thumbs);
            chat.used_at = std::time::Instant::now();
        }
        self.limit_live(cx);
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
        self.close_mention(cx);
        self.follow = true;
        self.thread.scroll_to_bottom();
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn interrupt(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active.clone() else {
            return;
        };
        if let Some(chat) = self.chats.iter_mut().find(|c| c.key == key && c.live && c.busy) {
            // El resultado se mostrará como «Detenido.», no como un error.
            chat.interrupted = true;
            self.fire("interrupt", json!({ "key": key }), cx);
        }
    }

    // --- Adjuntos y menús -----------------------------------------------------------

    fn attach_paths(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            let image = image_type(&path).and_then(|media_type| {
                let bytes = std::fs::read(&path).ok()?;
                Some(Attachment::Image {
                    name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    media_type,
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                    image: chat::image_from(media_type, bytes)?,
                })
            });
            let attachment = image.unwrap_or(Attachment::File(path));
            match &attachment {
                Attachment::File(path) => {
                    if self.attachments.iter().any(|a| matches!(a, Attachment::File(p) if p == path)) {
                        continue;
                    }
                }
                Attachment::Image { .. } => {
                    if self.image_count() >= MAX_IMAGES {
                        continue;
                    }
                }
            }
            self.attachments.push(attachment);
        }
    }

    /// Archivos pegados o soltados sobre la caja: las imágenes van como
    /// imagen (hasta `MAX_IMAGES`) y el resto como ruta (`takeFiles` de la referencia).
    fn take_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        let before = self.image_count();
        let images = files.iter().filter(|p| image_type(p).is_some()).count();
        self.attach_paths(files);
        if before + images > MAX_IMAGES {
            self.show_toast(format!("Como mucho {MAX_IMAGES} imágenes por mensaje"), cx);
        }
        cx.notify();
    }

    fn image_count(&self) -> usize {
        self.attachments.iter().filter(|a| matches!(a, Attachment::Image { .. })).count()
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
        // Archivos copiados en el Explorador: las imágenes van como imagen y el
        // resto como ruta (el `savePasted` de la referencia, que en Windows trae la ruta).
        let files = crate::clip_image::read_files();
        if !files.is_empty() {
            self.take_files(files, cx);
            return;
        }
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
                let n = self.image_count() + 1;
                if n > MAX_IMAGES {
                    self.show_toast(format!("Como mucho {MAX_IMAGES} imágenes por mensaje"), cx);
                    return;
                }
                let Some(thumb) = chat::image_from(media_type, image.bytes().to_vec()) else {
                    continue;
                };
                self.attachments.push(Attachment::Image {
                    name: format!("Imagen {n}"),
                    media_type,
                    data: base64::engine::general_purpose::STANDARD.encode(image.bytes()),
                    image: thumb,
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

    /// Esc: cierra lo que esté abierto; si no, rechaza el permiso pendiente
    /// (como en la referencia), detiene la respuesta o, dos seguidos, abre el Rewind.
    fn close_menu(&mut self, _: &CloseMenu, _: &mut Window, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        let double = self.last_esc.is_some_and(|last| now.duration_since(last) < DOUBLE_ESC);
        self.last_esc = Some(now);
        if self.menu.take().is_some() || self.pop.take().is_some() || self.profile_card.take().is_some() || self.style_menu.take().is_some() || self.settings_open {
            self.settings_open = false;
            self.last_esc = None;
            cx.notify();
            return;
        }
        if self.permission_keypress(permissions::PermKey::Escape, permissions::PermAt::Composer, None, cx) {
            self.last_esc = None;
            return;
        }
        let Some(chat) = self.active_chat() else {
            return;
        };
        if chat.live && chat.busy {
            self.last_esc = None;
            self.interrupt(cx);
        } else if double && !chat.items.is_empty() {
            self.last_esc = None;
            self.open_rewind(cx);
        }
    }

    fn submit_answers_action(&mut self, _: &SubmitAnswers, _: &mut Window, cx: &mut Context<Self>) {
        self.permission_keypress(permissions::PermKey::CtrlEnter, permissions::PermAt::Composer, None, cx);
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

    fn show_files_action(&mut self, _: &ShowFiles, _: &mut Window, cx: &mut Context<Self>) {
        if self.workspaces.active().is_some() {
            self.toggle_side(Side::Files, cx);
        }
    }

    fn show_changes_action(&mut self, _: &ShowChanges, _: &mut Window, cx: &mut Context<Self>) {
        if self.workspaces.active().is_some() {
            self.toggle_side(Side::Changes, cx);
        }
    }

    fn open_folder_action(&mut self, _: &OpenFolder, _: &mut Window, cx: &mut Context<Self>) {
        self.pick_folders(None, cx);
    }

    /// Ctrl+L: nueva conversación. No hay editor ni terminal dentro de Atic Code
    /// todavía; cuando lleguen, ahí no debe actuar (Decisiones del traspaso).
    fn clear_conversation_action(&mut self, _: &ClearConversation, window: &mut Window, cx: &mut Context<Self>) {
        self.new_conversation(window, cx);
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
        self.close_mention(cx);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// «/» como único texto abre el menú de acciones, como en la referencia.
    fn composer_changed(&mut self, area: Entity<TextArea>, _: &crate::text_input::Changed, cx: &mut Context<Self>) {
        if area.read(cx).text() == "/" {
            area.update(cx, |area, cx| area.set_text("", cx));
            self.toggle_menu(Menu::Actions, Point::default(), cx);
            return;
        }
        self.update_mention(cx);
    }

    /// Escribe `/comando ` al final de la caja, listo para completar o mandar.
    fn insert_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.insert(&format!("/{name} "), window, cx);
    }

    /// Agrega un texto al final del borrador, con un espacio si hace falta
    /// (`insert` de la referencia): subagentes, comandos y Claude Design.
    fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let draft = append_draft(self.composer.read(cx).text(), text);
        self.composer.update(cx, |area, cx| area.set_text(&draft, cx));
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn set_appearance(&mut self, style: style::Style, mode: style::Mode, window: &mut Window, cx: &mut Context<Self>) {
        let shown = style::settled();
        let target = style::resolved(style, mode);
        self.configs.style = style;
        self.configs.mode = mode;
        self.configs.save();
        // En Expressive, cambiar de tema o de modo se revela con un círculo desde el puntero
        // (`theme.ts:137`): Atic cambia sus colores cuando el círculo cubre la ventana. Si ya
        // hay un revelado en curso, el nuevo lo reemplaza para que no aplique uno viejo.
        let changes_look = (shown.style, shown.light) != (target.style, target.light);
        let reveal = !gpui_m3::MotionSettings::get(cx).reduced
            && ((target.style == style::Style::Expressive && changes_look) || self.pending_look.is_some());
        if reveal {
            let scheme = style::m3_scheme(&target);
            self.pending_look = Some((style, mode, scheme));
            gpui_m3::theme_reveal(scheme, !target.light, window, cx);
        } else {
            self.pending_look = None;
            self.skip_blend = false;
            style::set(style, mode);
            window.set_background_appearance(background(style));
        }
        cx.notify();
    }

    /// Cuando el revelado circular ya cambió el esquema de gpui-m3, Atic cambia sus colores.
    fn apply_pending_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((style, mode, scheme)) = self.pending_look else {
            return;
        };
        if gpui_m3::Theme::get(cx).scheme != scheme {
            return;
        }
        self.pending_look = None;
        self.skip_blend = true;
        style::set(style, mode);
        window.set_background_appearance(background(style));
    }

    /// Si cambió el estilo, el modo o el acento, la caja de texto se rehace con sus colores
    /// (conserva lo escrito y el foco). En Expressive, un cambio de acento o de modo sin
    /// revelado mezcla los colores del anterior al nuevo (`animate_scheme` en gpui-m3 y
    /// `style::begin_blend` en los colores propios de Atic).
    fn sync_style(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_pending_look(window, cx);
        let shown = style::t();
        style::set_accent(self.configs.active_accent(self.config_workspace()));
        let t = style::settled();
        if self.composer_style == (t.style, t.light, style::accent()) {
            if style::blending() {
                window.request_animation_frame();
            }
            return;
        }
        self.composer_style = (t.style, t.light, style::accent());
        let motion = gpui_m3::MotionSettings::get(cx);
        let animate = !self.skip_blend && !motion.reduced && self.pending_look.is_none() && t.style == style::Style::Expressive && shown.style == t.style;
        self.skip_blend = false;
        if animate {
            style::begin_blend(shown, gpui_m3::SCHEME_BLEND * motion.time_scale);
            gpui_m3::animate_scheme(style::m3_scheme(&t), !t.light, cx);
            window.request_animation_frame();
        } else {
            style::end_blend();
            style::apply_m3(cx);
        }
        self.refresh_editor_colors(cx);
        let text = self.composer.read(cx).text().to_string();
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        self.composer = new_composer(&text, cx);
        if focused {
            self.focus_composer(window, cx);
        }
    }

    /// Cambia el acento del espacio (o del chat suelto) en el estilo de ahora;
    /// `None` vuelve al del estilo.
    fn set_accent_color(&mut self, color: Option<u32>, cx: &mut Context<Self>) {
        let Some(workspace) = self.config_workspace() else {
            return;
        };
        if self.configs.accent(workspace, self.configs.style) != color {
            self.configs.set_accent(workspace, self.configs.style, color);
        }
        cx.notify();
    }

    // --- Configuración -------------------------------------------------------------

    fn config(&self) -> ClaudeConfig {
        self.config_workspace().map(|id| self.configs.get(id)).unwrap_or_default()
    }

    /// De quién es la configuración que se ve y se cambia: del chat suelto
    /// visible (los chats sueltos tienen la suya) o del espacio activo.
    fn config_workspace(&self) -> Option<u64> {
        if self.in_loose_chat() {
            return Some(LOOSE);
        }
        self.active_workspace()
    }

    /// El modelo de la conversación visible (cada una tiene el suyo); sin
    /// conversación, el predeterminado del espacio.
    fn chat_model(&self) -> String {
        match self.active_chat() {
            Some(chat) => chat.chosen_model(&self.configs.get(chat.workspace)),
            None => self.config().model,
        }
    }

    /// El esfuerzo de la conversación visible o, sin conversación, el del espacio.
    fn chat_effort(&self) -> String {
        match self.active_chat() {
            Some(chat) => chat.chosen_effort(&self.configs.get(chat.workspace)),
            None => self.config().effort,
        }
    }

    /// Aplica al proceso de una conversación abierta su modelo y su esfuerzo si
    /// tiene otros (`syncChatSettings` de la referencia). Se marca como aplicado antes
    /// de pedirlo y se revierte si falla.
    fn sync_chat_settings(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
            return;
        };
        let Some(before) = chat.applied.clone().filter(|_| chat.live) else {
            return;
        };
        let config = self.configs.get(chat.workspace);
        let want = Applied { model: chat.chosen_model(&config), effort: chat.chosen_effort(&config) };
        let model = !config::same_model(&self.models, &want.model, &before.model);
        let effort = !want.effort.is_empty() && want.effort != before.effort;
        if !model && !effort {
            return;
        }
        chat.applied = Some(Applied {
            model: if model { want.model.clone() } else { before.model.clone() },
            effort: if effort { want.effort.clone() } else { before.effort.clone() },
        });
        // Si una petición falla se revierte solo lo que pedía: la otra puede
        // haberse aplicado (`agent.ts:770`).
        let revert = |key: String, before: Applied, field: Setting| {
            move |view: &mut Self, reply: Reply, _: &mut Context<Self>| {
                if let Err(error) = reply {
                    if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
                        if let Some(applied) = chat.applied.as_mut() {
                            field.revert(applied, &before);
                        }
                    }
                    view.error = Some(format!("No se pudo aplicar el modelo o el esfuerzo: {error}"));
                }
            }
        };
        if model {
            let id = (!want.model.is_empty()).then(|| want.model.clone());
            self.request("setModel", json!({ "key": key, "model": id }), cx, revert(key.to_string(), before.clone(), Setting::Model));
        }
        if effort {
            let settings = json!({ "effortLevel": want.effort });
            self.request("applyFlags", json!({ "key": key, "settings": settings }), cx, revert(key.to_string(), before, Setting::Effort));
        }
    }

    /// Cambia la configuración desde el chat (menús y composer): el modelo y el
    /// esfuerzo son los de la conversación visible y quedan además como
    /// predeterminados del espacio; lo demás se guarda en el espacio y se aplica
    /// a sus sesiones abiertas.
    fn set_config(&mut self, change: impl FnOnce(&mut ClaudeConfig), cx: &mut Context<Self>) {
        self.update_config(change, true, cx);
    }

    /// Cambia los valores por defecto del espacio (la página de ajustes): el
    /// modelo y el esfuerzo valen para las conversaciones nuevas, no la visible.
    fn set_defaults(&mut self, change: impl FnOnce(&mut ClaudeConfig), cx: &mut Context<Self>) {
        self.update_config(change, false, cx);
    }

    fn update_config(&mut self, change: impl FnOnce(&mut ClaudeConfig), for_chat: bool, cx: &mut Context<Self>) {
        let Some(workspace) = self.config_workspace() else {
            return;
        };
        let before = self.configs.get(workspace);
        let chat = self.active.clone().filter(|_| for_chat).filter(|key| self.chats.iter().any(|c| &c.key == key && c.workspace == workspace));
        // Lo que se ve en los menús: el modelo y el esfuerzo de la conversación.
        let mut shown = before.clone();
        if let Some(chat) = chat.as_ref().and_then(|key| self.chats.iter().find(|c| &c.key == key)) {
            shown.model = chat.chosen_model(&before);
            shown.effort = chat.chosen_effort(&before);
        }
        let mut after = shown.clone();
        change(&mut after);
        let (model, effort) = (after.model != shown.model, after.effort != shown.effort);
        if !model {
            after.model = before.model.clone();
        }
        if !effort {
            after.effort = before.effort.clone();
        }
        if let Some(chat) = chat.as_ref().and_then(|key| self.chats.iter_mut().find(|c| &c.key == key)) {
            if model {
                chat.want_model = Some(after.model.clone());
            }
            if effort {
                chat.want_effort = Some(after.effort.clone());
            }
        }
        if let Some(key) = chat.filter(|_| model || effort) {
            self.sync_chat_settings(&key, cx);
            cx.notify();
        }
        if after == before {
            return;
        }
        self.configs.set(workspace, after.clone());
        let live: Vec<String> = self.chats.iter().filter(|c| c.live && c.workspace == workspace).map(|c| c.key.clone()).collect();
        for key in live {
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
        config::model_name(&self.models, id)
    }

    /// El modelo de la lista que corresponde a `current` (para el radio de los menús).
    fn current_model(&self, current: &str) -> Option<&str> {
        config::current_model(&self.models, current)
    }

    // --- Archivos y cambios -----------------------------------------------------------

    fn changed_files(&self) -> usize {
        self.repos.iter().map(|r| r.files.len()).sum()
    }

    fn toggle_side(&mut self, side: Side, cx: &mut Context<Self>) {
        let viewing = self.doc.is_some() || self.tabs.showing();
        self.side = if self.side == Some(side) && !viewing { None } else { Some(side) };
        self.doc = None;
        self.tabs.hide();
        cx.notify();
    }

    /// Abre un archivo: en el editor, o su diff en el visor si se pide y git lo tiene.
    fn open_doc(&mut self, path: PathBuf, show_diff: bool, cx: &mut Context<Self>) {
        if show_diff {
            self.next_doc += 1;
            let doc = Doc::load(self.next_doc, &path, true);
            if doc.show_diff {
                self.doc = Some(doc);
                self.active_file = Some(path);
                cx.notify();
                return;
            }
        }
        self.open_file(path, cx);
    }

    /// Las carpetas contra las que se leen las rutas que escribe Claude: las del espacio de
    /// la conversación visible (o la carpeta de los chats sueltos).
    fn ref_roots(&self) -> Vec<PathBuf> {
        let workspace = self.active_chat().map(|c| c.workspace).or_else(|| self.workspaces.active().map(|w| w.id));
        match workspace {
            Some(LOOSE) => loose_dir().into_iter().collect(),
            Some(id) => self.workspaces.get(id).map(|w| w.folders.clone()).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Abre en el visor el `código` en línea de una respuesta (`src/x.rs:42`).
    fn open_ref(&mut self, text: &str, cx: &mut Context<Self>) {
        match files::resolve_ref(text, &self.ref_roots()) {
            Some(path) => match gpui_m3::split_path_line(text.trim()).1 {
                Some((line, _)) => self.open_file_at(path, line, cx),
                None => self.open_file(path, cx),
            },
            None => self.show_toast(format!("No encontré {}", gpui_m3::split_path_line(text.trim()).0), cx),
        }
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

    /// Actualiza git ya, con el spinner del panel (el botón y el fin de cada respuesta).
    fn refresh_changes_now(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        self.refresh_changes(cx);
        cx.notify();
    }

    fn refresh_changes(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspaces.active() else {
            self.refreshing = false;
            return;
        };
        let folders = workspace.folders.clone();
        let id = workspace.id;
        let scan = cx.background_executor().spawn(async move { git::status(&folders) });
        cx.spawn(async move |this, cx| {
            let repos = scan.await;
            let _ = this.update(cx, |view, cx| {
                view.refreshing = false;
                if view.active_workspace() == Some(id) {
                    view.repos = repos;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Arma el índice de archivos del espacio si no está o es de otras carpetas.
    fn ensure_index(&mut self, cx: &mut Context<Self>) {
        let Some(roots) = self.workspaces.active().map(|w| w.folders.clone()) else {
            return;
        };
        if self.indexing || self.file_index.as_ref().is_some_and(|(of, _)| *of == roots) {
            return;
        }
        self.indexing = true;
        let scan_roots = roots.clone();
        let scan = cx.background_executor().spawn(async move { files::index(&scan_roots) });
        cx.spawn(async move |this, cx| {
            let found = scan.await;
            let _ = this.update(cx, |view, cx| {
                view.indexing = false;
                if view.workspaces.active().is_some_and(|w| w.folders == roots) {
                    view.file_index = Some((roots, Arc::new(found)));
                    // Una @ escrita mientras se armaba el índice ya puede mostrar archivos.
                    if view.mention.is_some() {
                        view.update_mention(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Qué parte de lo aplicado pide una petición de `sync_chat_settings`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Setting {
    Model,
    Effort,
}

impl Setting {
    /// Devuelve a `applied` solo el valor de esta parte.
    fn revert(self, applied: &mut Applied, before: &Applied) {
        match self {
            Setting::Model => applied.model = before.model.clone(),
            Setting::Effort => applied.effort = before.effort.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_se_reinicia_la_conversacion_vacia_y_abierta() {
        let mut chat = Chat::new("k".into(), 1, PathBuf::new());
        assert!(!CodeView::is_idle_chat(&chat), "sin proceso no hay nada que reiniciar");
        chat.live = true;
        assert!(CodeView::is_idle_chat(&chat));
        chat.session_id = Some("s".into());
        assert!(!CodeView::is_idle_chat(&chat), "una sesión guardada no se pierde");
        chat.session_id = None;
        chat.push_user("hola", Vec::new());
        assert!(!CodeView::is_idle_chat(&chat));
    }

    #[test]
    fn una_peticion_fallida_revierte_solo_su_parte() {
        let before = Applied { model: "opus".into(), effort: "low".into() };
        // El modelo se aplicó y el esfuerzo falló: el modelo nuevo se conserva.
        let mut applied = Applied { model: "sonnet".into(), effort: "high".into() };
        Setting::Effort.revert(&mut applied, &before);
        assert_eq!(applied, Applied { model: "sonnet".into(), effort: "low".into() });
        Setting::Model.revert(&mut applied, &before);
        assert_eq!(applied, before);
    }

    #[test]
    fn el_texto_que_se_manda_con_adjuntos() {
        assert_eq!(compose_message("hola", None, &[], false), "hola");
        // Solo imágenes: el texto por defecto de la referencia.
        assert_eq!(compose_message("", None, &[], true), "Mira la imagen adjunta.");
        assert_eq!(compose_message("", None, &["src/a.rs".into()], true), "Revisa los archivos adjuntos.\n\n(Archivos adjuntos: @src/a.rs)");
        assert_eq!(compose_message("mira", None, &["a.rs".into(), "b.rs".into()], false), "mira\n\n(Archivos adjuntos: @a.rs @b.rs)");
        // El contexto del editor va antes de los adjuntos, y solo con texto escrito.
        let note = Some("(Contexto: @src/a.rs, líneas 3-9)");
        assert_eq!(compose_message("explica", note, &[], false), "explica\n\n(Contexto: @src/a.rs, líneas 3-9)");
        assert_eq!(compose_message("explica", note, &["b.rs".into()], false), "explica\n\n(Contexto: @src/a.rs, líneas 3-9)\n\n(Archivos adjuntos: @b.rs)");
        assert_eq!(compose_message("", note, &[], true), "Mira la imagen adjunta.");
    }

    #[test]
    fn insertar_agrega_al_final_del_borrador() {
        assert_eq!(append_draft("", "/review "), "/review ");
        assert_eq!(append_draft("mira esto", "Usa el subagente x para "), "mira esto Usa el subagente x para ");
        assert_eq!(append_draft("hola ", "/design "), "hola /design ");
        assert_eq!(append_draft("dos\n", "/compact"), "dos\n/compact");
    }

    #[test]
    fn la_linea_de_estado_como_en_la_referencia() {
        assert_eq!(status_line("Opus 4.5", Some((50_000, 200_000)), 0.4567, Some(12_340)), "Opus 4.5 · contexto 25% · $0.46 · 12.3 s");
        assert_eq!(status_line("Predeterminado", None, 0., None), "Predeterminado · contexto 0% · $0.00");
    }

    #[test]
    fn como_mucho_cuatro_procesos_vivos() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let live = |list: &[(&str, u64, bool)]| list.iter().map(|(k, s, c)| (k.to_string(), at(*s), *c)).collect::<Vec<_>>();
        // Cuatro o menos: no se cierra nada.
        assert!(idle_to_close(&live(&[("a", 1, true), ("b", 2, true), ("c", 3, true), ("d", 4, true)]), Some("d"), MAX_LIVE).is_empty());
        // Una de más: la usada hace más tiempo.
        assert_eq!(idle_to_close(&live(&[("a", 5, true), ("b", 1, true), ("c", 3, true), ("d", 4, true), ("e", 6, true)]), Some("e"), MAX_LIVE), vec!["b"]);
        // Ni la visible ni las que trabajan o esperan permiso, aunque sean las más viejas.
        assert_eq!(
            idle_to_close(&live(&[("a", 1, false), ("b", 2, true), ("c", 3, true), ("d", 4, true), ("e", 5, true), ("f", 0, true)]), Some("f"), MAX_LIVE),
            vec!["b", "c"]
        );
        // Si todas trabajan, se quedan abiertas.
        assert!(idle_to_close(&live(&[("a", 1, false), ("b", 2, false), ("c", 3, false), ("d", 4, false), ("e", 5, false)]), None, MAX_LIVE).is_empty());
    }

    #[test]
    fn los_adjuntos_van_relativos_al_proyecto() {
        let roots = [PathBuf::from(r"C:\repo"), PathBuf::from(r"D:\otro")];
        assert_eq!(relative_to(std::path::Path::new(r"C:\repo\src\main.rs"), &roots), "src/main.rs");
        assert_eq!(relative_to(std::path::Path::new(r"D:\otro\x.md"), &roots), "x.md");
        // Fuera del proyecto, la ruta completa.
        assert_eq!(relative_to(std::path::Path::new(r"E:\fuera\y.txt"), &roots), r"E:\fuera\y.txt");
    }

    #[test]
    fn la_misma_ruta_con_otras_barras() {
        assert!(same_path(std::path::Path::new("C:/Repo/a.rs"), std::path::Path::new(r"c:\repo\a.rs")));
        assert!(!same_path(std::path::Path::new(r"C:\repo\a.rs"), std::path::Path::new(r"C:\repo\b.rs")));
    }
}
