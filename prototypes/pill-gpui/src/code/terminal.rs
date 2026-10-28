//! La terminal integrada (`Terminal.tsx` y `terminals.ts` de la referencia): un panel
//! bajo el chat o a su derecha (`Dock`) con varias pestañas, cada una con su consola.
//! Se le puede hacer zoom (Ctrl+rueda, Ctrl+= / Ctrl+- / Ctrl+0) y soltarle archivos,
//! imágenes o texto, que llegan como rutas entre comillas o como texto pegado.
//!
//! Además del shell, abre los CLI de agentes instalados (Claude Code, OpenCode, Codex,
//! Cursor, Antigravity, Grok, Gemini) en la carpeta del proyecto, con las demás carpetas
//! del espacio (`--add-dir`) y los hooks de Atic. Con la
//! pestaña de un agente a la vista, un chip muestra cuánto le queda de cuota (`quota`).
//!
//! Las consolas son `kit::console` (alacritty + ConPTY) y se dibujan con
//! `kit::grid_of` y `kit::paint_grid`. Aquí va lo propio de la terminal: las pestañas (nueva, cerrar, renombrar con doble
//! clic), el alto que se ajusta y se recuerda, la paleta ANSI de la referencia según
//! el estilo y el modo, y el teclado.
//!
//! El texto (letras, tildes, AltGr) llega por el manejador de texto de la
//! ventana (`kit::input::layer`); las teclas especiales y
//! los atajos con Ctrl, por `console::key_bytes`. Dentro de la terminal los
//! atajos de Atic Code con Ctrl+letra no actúan (`bind_keys`): son del shell.

use crate::i18n::tf;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use alacritty_terminal::term::TermMode;
use futures::StreamExt;
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, App, Bounds, ClipboardEntry, Context, DispatchPhase, Entity, EntityInputHandler,
    EventEmitter, ExternalPaths, FocusHandle, Focusable, Font, HitboxBehavior, KeyBinding, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, SharedString, UTF16Selection, WeakEntity,
    Window,
};
use serde::{Deserialize, Serialize};

use crate::kit::console::{self, Console, GridSize, Launch, Palette};
use crate::kit::{self, Cell, GridLook};

use super::devices_view::{DevicesEvent, DevicesView};
use super::preview::{self, Preview};
use super::style::{self, t, Style};

/// El contexto de teclas del panel.
const CONTEXT: &str = "CodeTerminal";
/// El tamaño de la letra (la referencia usa 12,5 con interlineado 1,25) y los topes del
/// zoom, que avanza de a un punto.
pub const DEFAULT_FONT: f32 = 12.5;
const FONT_MIN: f32 = 8.0;
const FONT_MAX: f32 = 28.0;
const LINE_HEIGHT: f32 = 1.3;
/// El alto por defecto y el mínimo (`Terminal.tsx`); el máximo deja 220 px al chat.
pub const DEFAULT_HEIGHT: f32 = 260.0;
const MIN_HEIGHT: f32 = 120.0;
const CHAT_MIN: f32 = 220.0;
/// A la derecha: el ancho por defecto, el mínimo y lo que se le deja al resto de la
/// ventana (la barra y el chat).
pub const DEFAULT_WIDTH: f32 = 520.0;
const MIN_WIDTH: f32 = 280.0;
const REST_MIN_W: f32 = 520.0;
/// Cada cuánto se pone al día la cuota mientras haya un agente abierto (la de `quota`
/// guarda un minuto en caché).
const QUOTA_EVERY: Duration = Duration::from_secs(60);

/// Dónde va el panel: bajo el chat o a su derecha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dock {
    #[default]
    Bottom,
    Right,
}
/// El largo máximo del nombre de una pestaña.
const NAME_MAX: usize = 40;
/// El margen de la grilla dentro del panel: arriba, derecha, abajo, izquierda
/// (`.term-view { inset: 2px 6px 6px 14px }`).
const INSET: (f32, f32, f32, f32) = (2., 6., 6., 14.);

// Los atajos de Atic Code que dentro de la terminal son del shell: Ctrl+L
// limpia, Ctrl+K y Ctrl+U borran, Ctrl+P y Ctrl+N recorren el historial…
// `NoAction` en el contexto de la terminal deja pasar la tecla a `key_down`. Las teclas
// salen de `shortcuts::terminal_keys` (siguen a los atajos que el usuario cambie).

gpui::actions!(atic_code_terminal, [ToggleTerminal]);

/// Ctrl+` (en el teclado latinoamericano, la tecla de `|`) abre y oculta la terminal desde
/// cualquier lado y Ctrl+J solo fuera de ella: ambas están en `shortcuts::TABLE`.
pub fn bind_keys(cx: &mut App, keys: &[String]) {
    let context = Some(CONTEXT);
    cx.bind_keys(keys.iter().map(|key| KeyBinding::new(key, gpui::NoAction, context)));
}

/// Lo que el panel le avisa a la ventana.
pub enum TerminalEvent {
    /// Se soltó el divisor: el alto nuevo, para guardarlo.
    Height(f32),
    /// Lo mismo a la derecha: el ancho.
    Width(f32),
    /// Se movió el panel abajo o a la derecha.
    Dock(Dock),
    /// Cambió el zoom: el tamaño de la letra.
    FontSize(f32),
    /// Se ocultó o se cerró la última pestaña: el foco vuelve a la caja.
    Hidden,
    /// No se pudo abrir una terminal (va en un aviso).
    Error(String),
}

/// Un CLI de agente que se puede abrir en una pestaña.
#[derive(Clone, Copy)]
pub struct Launcher {
    pub cli: &'static str,
    pub name: &'static str,
    pub logo: &'static str,
}

/// Los agentes que ofrece la terminal: el catálogo de la pill y Gemini, que todavía no
/// está en él (sin MCP ni sesiones que leer, solo su consola).
fn launchers() -> Vec<Launcher> {
    crate::agents::AGENTS
        .iter()
        .map(|agent| Launcher { cli: agent.cli, name: agent.name, logo: agent.logo })
        .chain(std::iter::once(Launcher { cli: "gemini", name: "Gemini", logo: "icons/agents/gemini.svg" }))
        .collect()
}

struct TermTab {
    id: u64,
    console: Console,
    /// El nombre del shell (`pwsh`, `powershell`) o del agente.
    shell: String,
    /// La marca con que los hooks reconocen esta consola (`agent_prompts`).
    token: Option<String>,
    /// El CLI del agente que corre (`claude`, `codex`…), si es uno.
    agent: Option<&'static str>,
    /// El que puso el usuario: tiene prioridad sobre el del shell.
    name: Option<String>,
    /// Ya se escribió «[proceso terminado]».
    noted_exit: bool,
}

impl TermTab {
    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.shell.clone())
    }
}

pub struct Terminals {
    focus: FocusHandle,
    tabs: Vec<TermTab>,
    active: Option<u64>,
    /// El panel está a la vista. Ocultarlo no mata las consolas.
    shown: bool,
    height: f32,
    width: f32,
    dock: Dock,
    /// El tamaño de la letra: el zoom de todas las pestañas.
    font_size: f32,
    next_id: u64,
    wake: futures::channel::mpsc::UnboundedSender<()>,
    /// El texto a medio componer de una tecla muerta (ver `kit::input`).
    marked: Option<String>,
    font: Font,
    fonts: [Font; 4],
    cell: Option<Cell>,
    /// Dónde quedó la grilla en el último cuadro: de ahí salen filas y columnas.
    body: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// Lo que la rueda movió sin llegar a una línea: el trackpad manda pocos píxeles por
    /// evento y, redondeado cada uno, no avanzaba nunca.
    wheel_rest: f32,
    /// Arrastrando la barra de desplazamiento: dónde se tomó el pulgar, desde su borde de arriba.
    bar_drag: Rc<std::cell::Cell<Option<f32>>>,
    /// La pestaña que se está renombrando y su campo.
    editing: Option<u64>,
    rename_field: Entity<gpui_m3::TextField>,
    rename_had_focus: bool,
    /// Dónde abren las pestañas nuevas: la primera carpeta del proyecto.
    pub cwd: Option<PathBuf>,
    /// Las otras carpetas del espacio, para los agentes que las aceptan (`--add-dir`).
    pub extras: Vec<PathBuf>,
    /// Los agentes instalados en esta máquina (se buscan al abrir, en segundo plano).
    installed: Vec<Launcher>,
    /// Los que el usuario ocultó en la configuración (por su CLI).
    pub hidden: Vec<String>,
    /// Lo más que puede medir a la derecha sin quitarle su mínimo al chat ni a los
    /// paneles (lo calcula la vista en cada cuadro, `view::Widths`).
    pub max_width: Option<f32>,
    /// La última cuota de cada agente, por su CLI.
    quotas: HashMap<String, crate::quota::AgentQuota>,
    quota_loading: bool,
    /// El menú de los agentes está abierto, donde se hizo clic.
    agent_menu: Option<Point<Pixels>>,
    /// La vista previa web: una pestaña más (`preview::TAB_ID`), si se abrió.
    preview: Option<Preview>,
    /// Dónde va la página en el último cuadro.
    preview_bounds: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// La barra de dirección de la vista previa.
    address: Entity<gpui_m3::TextField>,
    /// Algo de GPUI encima del panel (un menú, un diálogo): la página se cambia por su foto.
    covered: bool,
    /// Las direcciones que ya se abrieron solas: un servidor que reimprime su dirección
    /// no vuelve a abrir la vista previa si el usuario la cerró.
    auto_opened: std::collections::HashSet<String>,
    release_pending: bool,
    /// Cmd+L en la página: la barra toma el foco en el próximo cuadro (hace falta la ventana).
    focus_address: bool,
    /// Un clic en la página: la barra suelta el foco (el teclado ya es de la página).
    blur_address: bool,
    /// La pestaña Dispositivos (emuladores), si se abrió.
    devices: Option<Entity<DevicesView>>,
    /// Un comando que pidió Dispositivos (instalar lo que falta): se abre en una pestaña
    /// en el próximo cuadro (hace falta la ventana).
    pending_run: Option<(String, String)>,
}

/// La clave de la pestaña Dispositivos entre las de la terminal.
pub const DEVICES_ID: u64 = u64::MAX - 1;

/// El chip de la cuota de un agente: la ventana más gastada (la que primero se acaba),
/// con lo que le queda; y el detalle de todas para el globito. `None` sin nada que decir.
struct QuotaChip {
    label: String,
    /// Lo que queda de la ventana más gastada, 0..=100.
    left: f64,
    tip: String,
}

fn quota_chip(quota: &crate::quota::AgentQuota, now_ms: i64) -> Option<QuotaChip> {
    // Las ventanas que ya se reiniciaron no cuentan: su número es viejo.
    let live: Vec<&crate::quota::QuotaWindow> =
        quota.windows.iter().filter(|w| w.resets_at.is_none_or(|at| at > now_ms)).collect();
    let mut lines: Vec<String> = quota.plan.iter().map(|plan| format!("Plan {plan}")).collect();
    for window in &live {
        let label = crate::usage::window_label(&window.kind, window.minutes);
        let reset = window.resets_at.map(|at| tf("pill.code.terminal.resetsIn", &[("span", &crate::usage::span(at - now_ms))])).unwrap_or_default();
        lines.push(tf("pill.code.terminal.used", &[("label", &label), ("pct", &format!("{:.0}", window.used_percent)), ("reset", &reset)]));
    }
    let worst = live.iter().max_by(|a, b| a.used_percent.total_cmp(&b.used_percent));
    match worst {
        Some(window) => {
            let left = (100.0 - window.used_percent).clamp(0.0, 100.0);
            let label = format!("{} · {:.0} %", crate::usage::window_label(&window.kind, window.minutes), left);
            Some(QuotaChip { label, left, tip: lines.join("\n") })
        }
        None => {
            if let Some(spend) = &quota.spend {
                lines.push(tf("pill.code.terminal.spent", &[("amount", &format!("{:.2}", spend.cents / 100.0))]));
                return Some(QuotaChip { label: format!("US$ {:.2}", spend.cents / 100.0), left: 100.0, tip: lines.join("\n") });
            }
            let error = quota.error.as_ref()?;
            lines.push(error.clone());
            Some(QuotaChip { label: crate::i18n::t("pill.code.terminal.quotaUnknown").into(), left: 100.0, tip: lines.join("\n") })
        }
    }
}

impl EventEmitter<TerminalEvent> for Terminals {}

impl Focusable for Terminals {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// La paleta ANSI de la referencia (`PALETTES` de `Terminal.tsx`): la normal y la
/// brillante, por estilo y modo.
fn ansi(style: Style, light: bool) -> [u32; 16] {
    let (base, bright): ([u32; 8], [u32; 8]) = match (style, light) {
        (Style::Formal, false) => (
            [0x1a1c20, 0xf06a62, 0x3fb27f, 0xd9ae3a, 0x4c8dff, 0xc7a6ff, 0x4fc3b5, 0xc9ccd3],
            [0x5c6069, 0xff8a82, 0x5fd49c, 0xf0c75a, 0x7aaaff, 0xdcc3ff, 0x6fdccf, 0xffffff],
        ),
        (Style::Formal, true) => (
            [0x17181b, 0xc93a32, 0x1f8a5b, 0xa77a00, 0x2563eb, 0x7a3fc2, 0x0e7a70, 0x8c9099],
            [0x5f636b, 0xe5534b, 0x2fa36c, 0xc39300, 0x4c7ff0, 0x9a5fe0, 0x17958a, 0xc9ccd3],
        ),
        (Style::Expressive, false) => (
            [0x2b2930, 0xf2b8b5, 0xa8dab5, 0xf6c779, 0xa8c7fa, 0xd0bcff, 0x9ee0e0, 0xe6e0e9],
            [0x49454f, 0xffdad6, 0xc4f0cf, 0xffe2b8, 0xd3e3fd, 0xeaddff, 0xbdf3f3, 0xffffff],
        ),
        (Style::Expressive, true) => (
            [0x1d1b20, 0xb3261e, 0x146c2e, 0x7d5700, 0x0b57d0, 0x6750a4, 0x006a6a, 0x79747e],
            [0x49454f, 0xdc362e, 0x1e8e3e, 0xa36f00, 0x3d75e0, 0x8b6fd6, 0x008b8b, 0xcac4d0],
        ),
        (Style::Glass, false) => (
            [0x1c1c1e, 0xff453a, 0x30d158, 0xffd60a, 0x0a84ff, 0xbf5af2, 0x64d2ff, 0xe5e5ea],
            [0x636366, 0xff6961, 0x4be06d, 0xffe066, 0x409cff, 0xda8fff, 0x8ad9ff, 0xffffff],
        ),
        (Style::Glass, true) => (
            [0x1d1d1f, 0xd70015, 0x248a3d, 0xb25000, 0x0040dd, 0x8944ab, 0x0071a4, 0x8e8e93],
            [0x48484a, 0xff3b30, 0x34c759, 0xc93400, 0x007aff, 0xaf52de, 0x32ade6, 0xc7c7cc],
        ),
    };
    let mut out = [0; 16];
    out[..8].copy_from_slice(&base);
    out[8..].copy_from_slice(&bright);
    out
}

/// El fondo del panel según el estilo (`.term-panel`): un contenedor tonal
/// en Expressive y el del editor en los demás.
fn panel_bg() -> gpui::Hsla {
    let t = t();
    if t.style == Style::Expressive {
        t.raised
    } else {
        t.editor
    }
}

/// Los colores de la consola: la paleta de la referencia, el texto del estilo y, de
/// fondo, el del panel (así las celdas sin color propio no se pintan).
fn palette() -> Palette {
    let t = t();
    Palette {
        ansi: ansi(t.style, t.light),
        fg: style::rgb_of(t.text),
        bg: style::rgb_of(panel_bg()),
        dim_fg: style::rgb_of(t.muted),
    }
}

/// Cuántas columnas y filas caben en `bounds` con celdas de `cell`.
fn grid_size(bounds: Bounds<Pixels>, cell: Cell) -> GridSize {
    let w = f32::from(bounds.size.width) - kit::GRID_PAD * 2.;
    let h = f32::from(bounds.size.height) - 2.;
    GridSize {
        cols: (w / cell.w).floor().max(2.) as usize,
        rows: (h / cell.h).floor().max(2.) as usize,
    }
}

/// El nombre del shell para la pestaña, sin carpeta ni `.exe`.
fn shell_name(program: &str) -> String {
    std::path::Path::new(program)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string())
}

/// Un nombre puesto por el usuario: sin espacios a los lados y con 40 letras
/// como mucho. Vacío vuelve al del shell (`renameTerminal` de la referencia).
fn clean_name(name: &str) -> Option<String> {
    let clean: String = name.trim().chars().take(NAME_MAX).collect();
    (!clean.is_empty()).then_some(clean)
}

/// El alto, entre el mínimo y lo que deja `CHAT_MIN` al chat.
fn clamp_height(height: f32, window_h: f32) -> f32 {
    height.min(window_h - CHAT_MIN).max(MIN_HEIGHT)
}

/// El ancho a la derecha, entre el mínimo y lo que deja `REST_MIN_W` al resto.
fn clamp_width(width: f32, window_w: f32) -> f32 {
    width.min(window_w - REST_MIN_W).max(MIN_WIDTH)
}

/// El tamaño de la letra tras un paso de zoom (`step` en puntos; `None` vuelve al de
/// fábrica), dentro de los topes.
fn zoomed(size: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) => (size + step).clamp(FONT_MIN, FONT_MAX),
        None => DEFAULT_FONT,
    }
}

/// Rutas para escribir en la consola: cada una entre comillas si lleva espacios (así las
/// leen PowerShell, cmd y los CLI de agentes, que toman las imágenes por su ruta),
/// separadas por un espacio y con uno al final para seguir escribiendo.
fn quoted_paths(paths: &[PathBuf]) -> String {
    let mut out = String::new();
    for path in paths {
        let text = path.to_string_lossy();
        if text.contains(' ') {
            out.push('"');
            out.push_str(&text);
            out.push('"');
        } else {
            out.push_str(&text);
        }
        out.push(' ');
    }
    out
}

/// Guarda una imagen del portapapeles para pasarla por su ruta (los CLI no reciben
/// imágenes pegadas): en la carpeta temporal, con un nombre que no se repite.
fn save_pasted_image(bytes: &[u8], extension: &str) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("atic-terminal");
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f");
    let path = dir.join(format!("imagen-{stamp}.{extension}"));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

impl Terminals {
    pub fn new(height: Option<f32>, width: Option<f32>, dock: Dock, font_size: Option<f32>, cx: &mut Context<Self>) -> Self {
        let (wake, mut woken) = futures::channel::mpsc::unbounded::<()>();
        // Los avisos del PTY se juntan: un render por tanda, no por aviso.
        cx.spawn(async move |this, cx| {
            while woken.next().await.is_some() {
                while woken.try_recv().is_ok() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        let rename_field = cx.new(|cx| gpui_m3::TextField::new(cx).inline().select_all_on_focus(true).restore_on_cancel(true));
        cx.subscribe(&rename_field, |this: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
            gpui_m3::TextFieldEvent::Submitted(_) => this.commit_rename(cx),
            gpui_m3::TextFieldEvent::Cancelled => {
                this.editing = None;
                cx.notify();
            }
            _ => {}
        })
        .detach();
        let address = cx.new(|cx| {
            gpui_m3::TextField::new(cx).compact().select_all_on_focus(true).placeholder(crate::i18n::t("pill.code.preview.address"))
        });
        cx.subscribe(&address, |this: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| {
            if let gpui_m3::TextFieldEvent::Submitted(text) = event {
                let text = text.to_string();
                if let Some(preview) = this.preview.as_mut() {
                    preview.navigate(&text);
                    this.wait_for_server(cx);
                    cx.notify();
                }
            }
        })
        .detach();
        Self::watch_quota(cx);
        // Buscar los CLI en el PATH toca el disco: fuera del hilo de la interfaz.
        cx.spawn(async move |this, cx| {
            let installed = cx
                .background_spawn(async move {
                    launchers().into_iter().filter(|launcher| crate::agents::on_path(launcher.cli)).collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.installed = installed;
                cx.notify();
            });
        })
        .detach();
        let mut font = gpui::font(t().mono);
        font.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![crate::fonts::MONO.into(), crate::fonts::MONO_ALT.into()]));
        Self {
            focus: cx.focus_handle(),
            tabs: Vec::new(),
            active: None,
            shown: false,
            height: height.unwrap_or(DEFAULT_HEIGHT),
            width: width.unwrap_or(DEFAULT_WIDTH),
            dock,
            font_size: font_size.map(|size| size.clamp(FONT_MIN, FONT_MAX)).unwrap_or(DEFAULT_FONT),
            next_id: 1,
            wake,
            marked: None,
            fonts: kit::fonts_of(font.clone()),
            font,
            cell: None,
            body: Rc::new(std::cell::Cell::new(None)),
            wheel_rest: 0.,
            bar_drag: Rc::new(std::cell::Cell::new(None)),
            editing: None,
            rename_field,
            rename_had_focus: false,
            cwd: None,
            extras: Vec::new(),
            installed: Vec::new(),
            hidden: Vec::new(),
            max_width: None,
            quotas: HashMap::new(),
            quota_loading: false,
            agent_menu: None,
            preview: None,
            preview_bounds: Rc::new(std::cell::Cell::new(None)),
            address,
            covered: false,
            auto_opened: Default::default(),
            release_pending: false,
            focus_address: false,
            blur_address: false,
            devices: None,
            pending_run: None,
        }
    }

    /// Abre `url` en la vista previa (una dirección, un servidor local o un HTML del
    /// disco) y la deja a la vista.
    pub fn open_preview(&mut self, url: String, cx: &mut Context<Self>) {
        match self.preview.as_mut() {
            Some(preview) => preview.navigate(&url),
            None => {
                let (events, mut messages) = futures::channel::mpsc::unbounded();
                cx.spawn(async move |this, cx| {
                    while let Some(msg) = messages.next().await {
                        let alive = this.update(cx, |this, cx| this.on_preview_msg(msg, cx));
                        if alive.is_err() {
                            break;
                        }
                    }
                })
                .detach();
                self.preview = Some(Preview::new(url, events));
                // Las herramientas de desarrollo, en su ventana apenas se abren
                // (`keep_inspector_detached`), y la página, viva (`health_check`).
                cx.spawn(async move |this, cx| loop {
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                    let alive = this.update(cx, |this, cx| match this.preview.as_mut() {
                        Some(preview) => {
                            preview.keep_inspector_detached();
                            if preview.health_check() {
                                cx.notify();
                            }
                            true
                        }
                        None => false,
                    });
                    if !alive.unwrap_or(false) {
                        break;
                    }
                })
                .detach();
            }
        }
        let shown = self.preview.as_ref().map(|p| p.url.clone()).unwrap_or_default();
        self.address.update(cx, |field, cx| field.set_text(shown, cx));
        self.active = Some(preview::TAB_ID);
        self.shown = true;
        self.wait_for_server(cx);
        cx.notify();
    }

    /// Un servidor local que todavía no contesta (un agente imprimió su dirección antes
    /// de que arrancara): se pregunta cada medio segundo, hasta dos minutos, y se carga
    /// apenas contesta. Al cabo, se carga igual (la página dirá qué pasa).
    fn wait_for_server(&mut self, cx: &mut Context<Self>) {
        let Some((generation, host, port)) = self.preview.as_ref().and_then(Preview::waiting_target) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            for _ in 0..240 {
                let (h, p) = (host.clone(), port);
                if cx.background_spawn(async move { preview::server_up(&h, p) }).await {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let current = this.read_with(cx, |this, _| this.preview.as_ref().map(|p| p.nav_gen));
                if current.ok().flatten() != Some(generation) {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(preview) = this.preview.as_mut() {
                    preview.ready(generation);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Un agente editó archivos: un HTML del disco a la vista se recarga si son de su carpeta.
    pub fn preview_files_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if let (Some(preview), false) = (self.preview.as_mut(), paths.is_empty()) {
            preview.files_changed(paths);
            cx.notify();
        }
    }

    /// Una herramienta dejó algo para ver (un servidor local, un HTML): se abre solo la
    /// primera vez.
    pub fn offer_preview(&mut self, url: String, cx: &mut Context<Self>) {
        if self.auto_opened.insert(url.clone()) {
            self.open_preview(url, cx);
        }
    }

    /// Algo de GPUI tapa el panel (lo dice la vista de Atic Code en cada cuadro).
    pub fn set_covered(&mut self, covered: bool, cx: &mut Context<Self>) {
        if self.covered != covered {
            self.covered = covered;
            if self.preview.is_some() {
                cx.notify();
            }
        }
    }

    fn on_preview_msg(&mut self, msg: preview::Msg, cx: &mut Context<Self>) {
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        if let preview::Msg::External(url) = &msg {
            cx.open_url(url);
            return;
        }
        if matches!(&msg, preview::Msg::Key(key) if key == "address") {
            self.focus_address = true;
            cx.notify();
            return;
        }
        if matches!(&msg, preview::Msg::Key(key) if key == "focused") {
            self.blur_address = true;
            cx.notify();
            return;
        }
        let loaded = matches!(msg, preview::Msg::Loaded(_));
        let paste = matches!(&msg, preview::Msg::Key(key) if key == "paste");
        let clipboard = paste.then(|| cx.read_from_clipboard().and_then(|item| item.text())).flatten();
        preview.on_msg(msg, clipboard);
        // La barra sigue a la página (enlaces, redirecciones), salvo mientras se escribe en ella.
        if loaded {
            let url = preview.url.clone();
            self.address.update(cx, |field, cx| field.set_text(url, cx));
        }
        cx.notify();
    }

    /// La vista previa en cada cuadro: a la vista, tapada o escondida; oculta un buen
    /// rato, se suelta.
    fn sync_preview(&mut self, on_screen: bool, window: &mut Window, cx: &mut Context<Self>) {
        let covered = self.covered || self.agent_menu.is_some();
        let bounds = self.preview_bounds.get();
        if self.preview.is_none() {
            return;
        }
        if std::mem::take(&mut self.blur_address) && self.address.read(cx).is_focused(window) {
            // Solo el foco de GPUI: el teclado ya lo tiene la página.
            self.focus.focus(window);
        }
        if std::mem::take(&mut self.focus_address) {
            // La página suelta el teclado y la barra lo toma, con todo seleccionado.
            self.address.read(cx).focus(window);
        }
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        preview.sync(on_screen, covered, bounds, panel_bg(), window);
        if preview.hidden_since.is_some() && !self.release_pending {
            self.release_pending = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(preview::RELEASE_AFTER + Duration::from_secs(1)).await;
                let _ = this.update(cx, |this, _| {
                    this.release_pending = false;
                    if let Some(preview) = this.preview.as_mut() {
                        preview.release_if_idle();
                    }
                });
            })
            .detach();
        }
    }

    /// Cierra la vista previa (su vista web se suelta).
    fn close_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = None;
        self.preview_bounds.set(None);
        if self.active == Some(preview::TAB_ID) {
            self.active = self.tabs.last().map(|tab| tab.id).or(self.devices.as_ref().map(|_| DEVICES_ID));
        }
        if self.tabs.is_empty() && self.devices.is_none() {
            self.hide(cx);
        } else {
            self.focus.focus(window);
        }
        cx.notify();
    }

    /// Se ve una consola (el botón Terminal está encendido).
    pub fn consoles_shown(&self) -> bool {
        self.shown() && self.active_tab().is_some()
    }

    /// Se ve la pestaña Dispositivos.
    pub fn devices_shown(&self) -> bool {
        self.shown() && self.devices_active()
    }

    fn devices_active(&self) -> bool {
        self.devices.is_some() && self.active == Some(DEVICES_ID)
    }

    /// El botón Dispositivos: la oculta si se ve; si no, la abre (la primera vez revisa
    /// qué hay instalado).
    pub fn toggle_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.devices_shown() {
            self.hide(cx);
            return;
        }
        let view = match &self.devices {
            Some(view) => view.clone(),
            None => {
                let view = cx.new(DevicesView::new);
                cx.subscribe(&view, |this: &mut Self, _, event: &DevicesEvent, cx| match event {
                    DevicesEvent::Run { command, title } => {
                        this.pending_run = Some((command.clone(), title.clone()));
                        cx.notify();
                    }
                })
                .detach();
                cx.observe(&view, |_, _, cx| cx.notify()).detach();
                self.devices = Some(view.clone());
                view
            }
        };
        self.active = Some(DEVICES_ID);
        self.shown = true;
        view.read(cx).focus_handle(cx).focus(window);
        cx.notify();
    }

    fn close_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Soltar la vista apaga el teléfono que encendió Atic (`DevicesView::drop`).
        self.devices = None;
        if self.active == Some(DEVICES_ID) {
            self.active = self.tabs.last().map(|tab| tab.id).or(self.preview.as_ref().map(|_| preview::TAB_ID));
        }
        if self.tabs.is_empty() && self.preview.is_none() {
            self.hide(cx);
        } else {
            self.focus.focus(window);
        }
        cx.notify();
    }

    /// Se ve la vista previa (su botón está encendido).
    pub fn preview_shown(&self) -> bool {
        self.shown() && self.preview_active()
    }

    /// El botón Vista previa: la oculta si se ve; si no, la muestra (vacía, con el foco
    /// en la barra de dirección, si todavía no se abrió nada).
    pub fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview_shown() {
            self.hide(cx);
            return;
        }
        let url = self.preview.as_ref().map(|p| p.url.clone()).unwrap_or_default();
        let empty = url.is_empty();
        self.open_preview(url, cx);
        if empty {
            self.address.read(cx).focus(window);
        }
    }

    /// La pestaña a la vista es la vista previa.
    fn preview_active(&self) -> bool {
        self.preview.is_some() && self.active == Some(preview::TAB_ID)
    }

    /// Pide la cuota de los agentes en segundo plano (`force` salta la caché de un minuto:
    /// es el clic en el chip).
    fn refresh_quota(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.quota_loading {
            return;
        }
        self.quota_loading = true;
        cx.spawn(async move |this, cx| {
            let overview = cx.background_spawn(async move { crate::quota::fetch_overview(force) }).await;
            let _ = this.update(cx, |this, cx| {
                this.quota_loading = false;
                this.quotas = overview.agents.into_iter().map(|quota| (quota.agent.clone(), quota)).collect();
                cx.notify();
            });
        })
        .detach();
    }

    /// Mientras haya un agente abierto, su cuota se pone al día sola cada minuto.
    fn watch_quota(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(QUOTA_EVERY).await;
            let alive = this.update(cx, |this, cx| {
                if this.tabs.iter().any(|tab| tab.agent.is_some()) {
                    this.refresh_quota(false, cx);
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    pub fn shown(&self) -> bool {
        self.shown && (!self.tabs.is_empty() || self.preview.is_some() || self.devices.is_some())
    }

    pub fn dock(&self) -> Dock {
        self.dock
    }

    /// Lo que quiere medir a la derecha, si está a la vista ahí.
    pub fn right_width(&self) -> Option<f32> {
        (self.shown() && self.dock == Dock::Right).then_some(self.width.max(MIN_WIDTH))
    }

    /// Mueve el panel abajo o a la derecha.
    fn set_dock(&mut self, dock: Dock, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock == dock {
            return;
        }
        self.dock = dock;
        // La grilla se vuelve a medir en su lugar nuevo.
        self.body.set(None);
        cx.emit(TerminalEvent::Dock(dock));
        self.focus.focus(window);
        cx.notify();
    }

    /// Un paso de zoom (`None` vuelve al tamaño de fábrica): todas las pestañas cambian
    /// de letra y se reajustan a su panel.
    fn zoom(&mut self, step: Option<f32>, cx: &mut Context<Self>) {
        let size = zoomed(self.font_size, step);
        if size == self.font_size {
            return;
        }
        self.font_size = size;
        self.cell = None;
        cx.emit(TerminalEvent::FontSize(size));
        cx.notify();
    }

    /// Pega en la pestaña a la vista, como lo haría el portapapeles.
    fn paste_text(&mut self, text: &str) {
        if let Some(tab) = self.active_tab() {
            tab.console.scroll(i32::MIN / 2);
            tab.console.paste(text);
        }
    }

    /// Texto soltado en `point` (coordenadas de la ventana): si cae en la consola a la
    /// vista, se pega ahí y ella toma el foco.
    pub fn drop_text_at(&mut self, point: Point<Pixels>, text: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let inside = self.shown() && self.body.get().is_some_and(|bounds| bounds.contains(&point));
        if inside {
            self.paste_text(text);
            self.focus.focus(window);
            cx.notify();
        }
        inside
    }

    /// Lo que se suelta en la consola (archivos del Explorador o del portapapeles de
    /// Atic): sus rutas, entre comillas.
    fn drop_paths(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = paths.paths().to_vec();
        if paths.is_empty() {
            return;
        }
        self.paste_text(&quoted_paths(&paths));
        self.focus.focus(window);
        cx.notify();
    }

    /// Abre una terminal nueva en `cwd` y, si se pide, escribe un comando
    /// (`claude --resume …`) como si se tecleara (`newTerminal` de la referencia).
    pub fn open(
        &mut self,
        command: Option<String>,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (program, args) = kit::shell();
        let launch = Launch {
            program: program.clone(),
            args,
            cwd: self.open_dir(),
            env: Vec::new(),
        };
        self.spawn_tab(launch, shell_name(&program), name, None, None, command, window, cx)
    }

    /// Abre un CLI de agente en una pestaña nueva, en `cmd /k` (deja el prompt al
    /// terminar), con los hooks de Atic y las demás carpetas del espacio.
    pub fn open_agent(&mut self, launcher: Launcher, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let mut line = launcher.cli.to_string();
        let mut env = Vec::new();
        let token = crate::agent_prompts::prepare(launcher.cli, &line).map(|(hooked, token)| {
            line = hooked;
            env.push((crate::agent_prompts::token_var().to_string(), token.clone()));
            token
        });
        // Después de los hooks: con comillas en la línea, `prepare` no los pondría.
        let line = kit::workspaces::with_add_dirs(launcher.cli, &line, &self.extras);
        let (program, args) = kit::agent_shell(&line);
        let launch = Launch { program, args, cwd: self.open_dir(), env };
        self.spawn_tab(launch, launcher.name.to_string(), None, Some(launcher.cli), token, None, window, cx)?;
        self.refresh_quota(false, cx);
        Ok(())
    }

    /// La carpeta donde abre una pestaña: la del proyecto o, sin una, la del usuario.
    fn open_dir(&self) -> Option<PathBuf> {
        self.cwd.clone().filter(|dir| dir.is_dir()).or_else(|| crate::paths::home().map(std::ffi::OsString::from).map(PathBuf::from))
    }

    /// Arranca la consola de una pestaña nueva y la deja a la vista.
    #[allow(clippy::too_many_arguments)]
    fn spawn_tab(
        &mut self,
        launch: Launch,
        shell: String,
        name: Option<String>,
        agent: Option<&'static str>,
        token: Option<String>,
        command: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let cell = self.cell(window);
        // Hasta el primer cuadro no se sabe cuánto mide: el tamaño del último
        // panel, o uno razonable que se corrige al dibujarse.
        let size = self
            .body
            .get()
            .map(|bounds| grid_size(bounds, cell))
            .unwrap_or(GridSize { cols: 100, rows: 12 });
        let console = Console::spawn(launch, size, (cell.w.round() as u16, cell.h as u16), self.wake.clone())
            .map_err(|error| tf("pill.code.terminal.openFailed", &[("error", &error)]))?;
        if let Some(command) = command.filter(|c| !c.is_empty()) {
            console.write(format!("{command}\r").into_bytes());
        }
        if let (Some(token), Some(cli)) = (&token, agent) {
            crate::agent_prompts::register(token, cli, console.writer());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TermTab {
            id,
            console,
            shell,
            token,
            agent,
            name: name.and_then(|n| clean_name(&n)),
            noted_exit: false,
        });
        self.active = Some(id);
        self.shown = true;
        self.focus.focus(window);
        cx.notify();
        Ok(())
    }

    /// Ctrl+J: la oculta si se ve una consola; si no, la muestra (y abre una si no hay).
    /// Con la vista previa a la vista, pasa a una consola.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        if self.consoles_shown() {
            self.hide(cx);
            return Ok(());
        }
        if self.tabs.is_empty() {
            return self.open(None, None, window, cx);
        }
        if self.active_tab().is_none() {
            self.active = self.tabs.last().map(|tab| tab.id);
        }
        self.shown = true;
        self.focus.focus(window);
        cx.notify();
        Ok(())
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        self.shown = false;
        cx.emit(TerminalEvent::Hidden);
        cx.notify();
    }

    /// Cierra la pestaña y mata su proceso; con la última se oculta el panel.
    fn close(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if id == preview::TAB_ID {
            self.close_preview(window, cx);
            return;
        }
        if id == DEVICES_ID {
            self.close_devices(window, cx);
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        // Soltar la consola manda `Shutdown` al PTY (`Console::drop`).
        let tab = self.tabs.remove(index);
        if let Some(token) = &tab.token {
            crate::agent_prompts::unregister(token);
        }
        if self.editing == Some(id) {
            self.editing = None;
        }
        if self.active == Some(id) {
            self.active = self.tabs.get(index.min(self.tabs.len().saturating_sub(1))).map(|tab| tab.id);
            if self.active.is_none() && self.preview.is_some() {
                self.active = Some(preview::TAB_ID);
            }
            if self.active.is_none() && self.devices.is_some() {
                self.active = Some(DEVICES_ID);
            }
        }
        if self.tabs.is_empty() && self.preview.is_none() && self.devices.is_none() {
            self.hide(cx);
        } else {
            self.focus.focus(window);
        }
        cx.notify();
    }

    fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.active = Some(id);
        self.focus.focus(window);
        cx.notify();
    }

    fn start_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let label = tab.label();
        self.editing = Some(id);
        self.rename_had_focus = false;
        self.rename_field.update(cx, |field, cx| field.set_text(label, cx));
        self.rename_field.read(cx).focus(window);
        cx.notify();
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editing.take() else {
            return;
        };
        let name = clean_name(self.rename_field.read(cx).text());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.name = name;
        }
        cx.notify();
    }

    fn active_tab(&self) -> Option<&TermTab> {
        self.active.and_then(|id| self.tabs.iter().find(|tab| tab.id == id))
    }

    /// El ancho de una celda y el alto de una línea, medidos con la fuente.
    fn cell(&mut self, window: &mut Window) -> Cell {
        if let Some(cell) = self.cell {
            return cell;
        }
        let text = window.text_system();
        let id = text.resolve_font(&self.font);
        let size = self.font_size;
        let w = text
            .advance(id, px(size), 'm')
            .map(|s| f32::from(s.width))
            .unwrap_or(size * 0.6);
        let cell = Cell { w, h: (size * LINE_HEIGHT).round() };
        self.cell = Some(cell);
        cell
    }

    /// Escribe texto en la terminal a la vista.
    fn type_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(tab) = self.active_tab() {
            // Escribir vuelve al final si se estaba mirando el historial.
            tab.console.scroll(i32::MIN / 2);
            tab.console.write(text.as_bytes().to_vec());
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        if self.active_tab().is_none() {
            return;
        }
        // Zoom, como en Windows Terminal: Ctrl+= o Ctrl++ agranda, Ctrl+- achica y
        // Ctrl+0 vuelve al tamaño de fábrica.
        if crate::keymap::primary_held(m) && !m.alt {
            let step = match ks.key.as_str() {
                "=" | "+" => Some(Some(1.)),
                "-" => Some(Some(-1.)),
                "0" => Some(None),
                _ => None,
            };
            if let Some(step) = step {
                self.zoom(step, cx);
                cx.stop_propagation();
                return;
            }
        }
        // Ctrl+V pega (Ctrl+Mayús+V también, como en Windows Terminal).
        if crate::keymap::primary_held(m) && !m.alt && ks.key == "v" {
            self.paste_clipboard(cx);
            cx.stop_propagation();
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let app_cursor = tab.console.term.lock().mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = console::key_bytes(ks, app_cursor) {
            tab.console.scroll(i32::MIN / 2);
            tab.console.write(bytes);
            cx.stop_propagation();
        }
    }

    /// Ctrl+V: archivos copiados en el Explorador van por su ruta; una imagen se guarda
    /// y va su ruta (los CLI la leen así); si no, el texto.
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let files = crate::clip_image::read_files();
        if !files.is_empty() {
            self.paste_text(&quoted_paths(&files));
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for entry in item.entries() {
            if let ClipboardEntry::Image(image) = entry {
                let extension = match image.format() {
                    gpui::ImageFormat::Png => "png",
                    gpui::ImageFormat::Jpeg => "jpg",
                    gpui::ImageFormat::Gif => "gif",
                    gpui::ImageFormat::Webp => "webp",
                    _ => continue,
                };
                match save_pasted_image(image.bytes(), extension) {
                    Some(path) => self.paste_text(&quoted_paths(&[path])),
                    None => cx.emit(TerminalEvent::Error(crate::i18n::t("pill.code.terminal.imageFailed").into())),
                }
                return;
            }
        }
        if let Some(text) = item.text() {
            self.paste_text(&text);
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let cell = self.cell(window);
        let dy = f32::from(event.delta.pixel_delta(px(cell.h)).y);
        // Ctrl+rueda: zoom.
        if crate::keymap::primary_held(&event.modifiers) {
            if dy != 0. {
                self.zoom(Some(if dy > 0. { 1. } else { -1. }), cx);
            }
            return;
        }
        let rest = self.wheel_rest + dy;
        let lines = (rest / cell.h).trunc() as i32;
        self.wheel_rest = rest - lines as f32 * cell.h;
        // La celda bajo el puntero, para un programa que pidió el mouse.
        let (col, row) = self.body.get().map_or((0, 0), |body| {
            let at = event.position - body.origin;
            ((f32::from(at.x) / cell.w).max(0.) as usize, (f32::from(at.y) / cell.h).max(0.) as usize)
        });
        if let (Some(tab), true) = (self.active_tab(), lines != 0) {
            tab.console.wheel(lines, col, row);
            cx.notify();
        }
    }

    /// Ajusta cada consola al tamaño del panel y anota las que terminaron.
    fn sync(&mut self, cell: Cell) {
        let size = self.body.get().map(|bounds| grid_size(bounds, cell));
        for tab in &mut self.tabs {
            if let Some(size) = size {
                tab.console.resize(size, (cell.w.round() as u16, cell.h as u16));
            }
            if tab.console.exited() && !tab.noted_exit {
                tab.noted_exit = true;
                tab.console.note(crate::i18n::t("pill.code.terminal.processEnded"));
            }
        }
    }

    /// La barra de arriba: las pestañas, «+» y ocultar.
    fn head(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expressive = t().style == Style::Expressive;
        let several = self.tabs.len() > 1;
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            // Con varias y sin nombre propio, el número las distingue (`.term-num`).
            let label = match (&tab.name, several) {
                (None, true) => format!("{} {}", tab.label(), index + 1),
                _ => tab.label(),
            };
            gpui_m3::Tab::new(tab.id.to_string(), label)
                .icon("terminal")
                .tooltip(if tab.console.exited() { crate::i18n::t("pill.code.terminal.tabExitedTip") } else { crate::i18n::t("pill.code.terminal.tabTip") })
        });
        // «+» abre el shell; al lado, un solo botón con los agentes instalados en un menú
        // (antes, un logo por agente: le quitaban el lugar a las pestañas).
        let new = div().flex().items_center().gap(px(2.)).child(
            gpui_m3::IconButton::new("term-new", "plus")
                .size(px(28.))
                .tooltip(crate::i18n::t("pill.code.terminal.new"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                    if let Err(error) = this.open(None, None, window, cx) {
                        cx.emit(TerminalEvent::Error(error));
                    }
                })),
        );
        let new = new.map(|el| {
            el.child(
                gpui_m3::IconButton::new("term-agents", "spark")
                    .size(px(28.))
                    .selected(self.agent_menu.is_some())
                    .tooltip(crate::i18n::t("pill.code.terminal.agents"))
                    .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                        this.agent_menu = if this.agent_menu.is_some() { None } else { Some(event.position()) };
                        cx.notify();
                    })),
            )
        });
        let mut tabs = tabs.collect::<Vec<_>>();
        if let Some(preview) = &self.preview {
            tabs.push(gpui_m3::Tab::new(preview::TAB_ID.to_string(), preview.label()).icon("monitor").tooltip(preview.url.clone()));
        }
        if let Some(devices) = &self.devices {
            tabs.push(gpui_m3::Tab::new(DEVICES_ID.to_string(), devices.read(cx).label()).icon("cpu"));
        }
        let mut strip = gpui_m3::TabStrip::new("term-tabs").tabs(tabs).trailing(new)
            .on_select(on_tab(cx, |this, id, window, cx| this.select(id, window, cx)))
            .on_close(on_tab(cx, |this, id, window, cx| this.close(id, window, cx)))
            .on_rename(on_tab(cx, |this, id, window, cx| this.start_rename(id, window, cx)));
        if let Some(id) = self.active {
            strip = strip.selected(id.to_string());
        }
        if let Some(id) = self.editing {
            strip = strip.editing(id.to_string(), self.rename_field.clone());
        }
        div()
            .h(px(if expressive { 44. } else { 36. }))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .pl(px(if expressive { 12. } else { 8. }))
            .pr(px(if expressive { 10. } else { 6. }))
            .child(div().flex_1().min_w(px(0.)).child(strip))
            .children(self.quota_button(cx))
            .child({
                let (icon, tip, other) = match self.dock {
                    Dock::Bottom => ("panel-right", crate::i18n::t("pill.code.terminal.dockRight"), Dock::Right),
                    Dock::Right => ("panel-bottom", crate::i18n::t("pill.code.terminal.dockBottom"), Dock::Bottom),
                };
                gpui_m3::IconButton::new("term-dock", icon)
                    .size(px(28.))
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| this.set_dock(other, window, cx)))
            })
            .child(
                gpui_m3::IconButton::new("term-hide", if self.dock == Dock::Right { "chevron-right" } else { "chevron-down" })
                    .size(px(28.))
                    .tooltip(crate::keymap::hint(crate::i18n::t("pill.code.terminal.hide")))
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.hide(cx))),
            )
    }
}

impl Terminals {
    /// Los agentes instalados que el usuario no ocultó.
    fn launchers(&self) -> impl Iterator<Item = Launcher> + '_ {
        self.installed.iter().copied().filter(|l| !self.hidden.iter().any(|h| h == l.cli))
    }

    /// El menú del botón de agentes: el logo y el nombre de cada uno; elegir uno abre su CLI
    /// en una pestaña nueva. Con la terminal abajo, se abre hacia arriba.
    fn agent_menu_layer(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let at = self.agent_menu?;
        let t = t();
        let mut menu = gpui_m3::Menu::new("term-agent-menu").width(220.).stagger(false).item(
            gpui_m3::MenuItem::new("term-preview", crate::i18n::t("pill.code.preview.open")).icon("monitor").on_click(cx.listener(
                |this, _: &gpui::ClickEvent, window, cx| {
                    this.agent_menu = None;
                    let url = this.preview.as_ref().map(|p| p.url.clone()).unwrap_or_default();
                    this.open_preview(url, cx);
                    this.address.read(cx).focus(window);
                },
            )),
        );
        if super::devices_view::enabled() {
            menu = menu.item(gpui_m3::MenuItem::new("term-devices", crate::i18n::t("pill.code.devices.title")).icon("cpu").on_click(cx.listener(
                |this, _: &gpui::ClickEvent, window, cx| {
                    this.agent_menu = None;
                    if !this.devices_shown() {
                        this.toggle_devices(window, cx);
                    }
                },
            )));
        }
        if self.launchers().next().is_some() {
            menu = menu.separator();
        }
        for (index, launcher) in self.launchers().enumerate() {
            menu = menu.item(
                gpui_m3::MenuItem::new(("term-agent", index), tf("pill.code.terminal.openLauncher", &[("name", &launcher.name)]))
                    .leading(gpui::svg().path(launcher.logo).size(px(16.)).text_color(t.muted))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.agent_menu = None;
                        if let Err(error) = this.open_agent(launcher, window, cx) {
                            cx.emit(TerminalEvent::Error(error));
                        }
                        cx.notify();
                    })),
            );
        }
        let corner = if self.dock == Dock::Bottom { gpui::Corner::BottomLeft } else { gpui::Corner::TopLeft };
        Some(
            gpui::deferred(
                gpui::anchored().position(at).anchor(corner).snap_to_window_with_margin(px(8.)).child(
                    div()
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.agent_menu = None;
                            cx.notify();
                        }))
                        .child(menu),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// La cuota del agente de la pestaña a la vista: lo que le queda de la ventana más
    /// gastada; el globito trae todas y un clic la vuelve a pedir.
    fn quota_button(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let agent = self.active_tab()?.agent?;
        let quota = self.quotas.get(agent)?;
        let chip = quota_chip(quota, chrono::Utc::now().timestamp_millis())?;
        let t = t();
        let color = if chip.left < 15.0 {
            t.bad
        } else if chip.left < 40.0 {
            t.warn
        } else {
            t.muted
        };
        let tip = tf("pill.code.terminal.clickRefresh", &[("tip", &chip.tip)]);
        Some(
            div()
                .id("term-quota")
                .h(px(24.))
                .px(px(9.))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .rounded(px(12.))
                .text_size(px(11.5))
                .text_color(color)
                .cursor_pointer()
                .hover(|el| el.bg(t.hover))
                .tooltip(crate::hover::tip_text(tip.into()))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.refresh_quota(true, cx)))
                .child(gpui::svg().path("icons/gauge.svg").size(px(12.)).text_color(color))
                .child(chip.label),
        )
    }
}

/// Un callback de `TabStrip` (que da la clave de la pestaña) sobre la vista.
fn on_tab(
    cx: &mut Context<Terminals>,
    f: impl Fn(&mut Terminals, u64, &mut Window, &mut Context<Terminals>) + 'static,
) -> impl Fn(SharedString, &mut Window, &mut App) + 'static {
    let this = cx.weak_entity();
    move |key, window, cx| {
        if let Ok(id) = key.parse() {
            let _ = this.update(cx, |this, cx| f(this, id, window, cx));
        }
    }
}

impl Terminals {
    /// Lo que hay que hacer en cada cuadro antes de dibujar: guardar el
    /// renombre al perder el foco (como la referencia, `onBlur`).
    fn commit_rename_on_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            return;
        }
        if self.rename_field.read(cx).is_focused(window) {
            self.rename_had_focus = true;
        } else if self.rename_had_focus {
            self.commit_rename(cx);
        }
    }
}

impl Render for Terminals {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.commit_rename_on_blur(window, cx);
        let preview_on = self.shown() && self.preview_active();
        self.sync_preview(preview_on, window, cx);
        // Lo que pidió Dispositivos (instalar algo): en una pestaña, a la vista.
        if let Some((command, title)) = self.pending_run.take() {
            if let Err(error) = self.open(Some(command), Some(title), window, cx) {
                cx.emit(TerminalEvent::Error(error));
            }
        }
        if !self.shown() {
            return div().id("code-terminal");
        }
        let cell = self.cell(window);
        self.sync(cell);
        let t = t();
        let palette = palette();
        let focused = self.focus.is_focused(window);
        let grid = self.active_tab().map(|tab| kit::grid_of(&tab.console, &self.fonts, &palette));
        let bar = self.active_tab().map(|tab| tab.console.scroll_state()).filter(|&(_, history, _)| history > 0);
        let look = GridLook {
            pad: kit::GRID_PAD,
            cursor: style::rgb_of(t.accent),
            idle: style::rgb_of(t.faint),
        };
        let viewport = window.viewport_size();
        let (window_w, window_h) = (f32::from(viewport.width), f32::from(viewport.height));
        let right = self.dock == Dock::Right;
        let height = clamp_height(self.height, window_h);
        let max_width = self.max_width.unwrap_or(window_w - REST_MIN_W).max(MIN_WIDTH);
        let width = clamp_width(self.width, window_w).min(max_width);
        let font_size = self.font_size;
        let body = self.body.clone();
        let this = cx.weak_entity();
        // Abajo, el divisor de arriba cambia el alto; a la derecha, el de la izquierda
        // cambia el ancho. En los dos, arrastrar hacia el chat agranda el panel.
        let resize = if right {
            gpui_m3::ResizeHandle::width("term-resize", px(width))
                .min(px(MIN_WIDTH))
                .max(px(max_width))
        } else {
            gpui_m3::ResizeHandle::height("term-resize", px(height))
                .min(px(MIN_HEIGHT))
                .max(px((window_h - CHAT_MIN).max(MIN_HEIGHT)))
        }
        .invert(true)
        .on_resize({
            let this = this.clone();
            move |size, _, cx| {
                let _ = this.update(cx, |this, cx| {
                    match this.dock {
                        Dock::Right => this.width = f32::from(size),
                        Dock::Bottom => this.height = f32::from(size),
                    }
                    cx.notify();
                });
            }
        })
        .on_resize_end({
            let this = this.clone();
            move |size, _, cx| {
                let _ = this.update(cx, |this, cx| match this.dock {
                    Dock::Right => {
                        this.width = f32::from(size);
                        cx.emit(TerminalEvent::Width(this.width));
                    }
                    Dock::Bottom => {
                        this.height = f32::from(size);
                        cx.emit(TerminalEvent::Height(this.height));
                    }
                });
            }
        });
        let expressive = t.style == Style::Expressive;
        let glass = t.style == Style::Glass;
        // En Expressive el panel sube con resorte al abrirse (`m3-rise`, `terminal.css:176`).
        let enter = super::enter::Enter::new("terminal-enter").from(0., 18.);
        let root = if expressive { enter.apply(div(), window, cx) } else { div() };
        root
            .id("code-terminal")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .relative()
            .flex_none()
            .map(|el| if right { el.w(px(width)).h_full() } else { el.h(px(height)) })
            .flex()
            .flex_col()
            .bg(panel_bg())
            .map(|el| match (t.style, right) {
                // Un contenedor tonal dentro del chat.
                (Style::Expressive, false) => el.mx(px(8.)).mb(px(8.)).rounded(px(20.)),
                (Style::Expressive, true) => el.ml(px(8.)).my(px(8.)).mr(px(8.)).rounded(px(20.)),
                // Una tarjeta flotante.
                (Style::Glass, false) => el.mt(px(8.)).rounded(px(18.)).border_1().border_color(t.highlight.opacity(0.35)),
                (Style::Glass, true) => el.ml(px(8.)).rounded(px(18.)).border_1().border_color(t.highlight.opacity(0.35)),
                (Style::Formal, false) => el.border_t_1().border_color(t.border),
                (Style::Formal, true) => el.border_l_1().border_color(t.border),
            })
            .child(if right {
                div().absolute().top_0().bottom_0().left(px(if expressive || glass { -8. } else { -4. })).w(px(8.)).child(resize)
            } else {
                div().absolute().left_0().right_0().top(px(if expressive || glass { -8. } else { -4. })).h(px(8.)).child(resize)
            })
            .child(self.head(cx))
            .children(self.agent_menu_layer(cx))
            .child(if self.preview_active() {
                self.preview_body(cx)
            } else if let Some(devices) = self.devices.clone().filter(|_| self.devices_active()) {
                div().flex_1().min_h(px(0.)).child(devices).into_any_element()
            } else {
                div()
                    .id("term-body")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.focus.focus(window);
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(Self::scroll))
                    // Archivos e imágenes soltados (del Explorador o del portapapeles de Atic).
                    .drag_over::<ExternalPaths>(|el, _, _, _| el.bg(style::t().hover))
                    .on_drop(cx.listener(Self::drop_paths))
                    .child(
                        div()
                            .absolute()
                            .top(px(INSET.0))
                            .right(px(INSET.1))
                            .bottom(px(INSET.2))
                            .left(px(INSET.3 - kit::GRID_PAD))
                            .overflow_hidden()
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        // Con otro tamaño, otro cuadro: `sync` ajusta las consolas.
                                        if body.get() != Some(bounds) {
                                            body.set(Some(bounds));
                                            window.refresh();
                                        }
                                    },
                                    move |bounds, _, window, cx| {
                                        if let Some(grid) = &grid {
                                            kit::paint_grid(grid, bounds, cell, 1.0, font_size, focused, look, window, cx);
                                        }
                                    },
                                )
                                .size_full(),
                            ),
                    )
                    .when_some(bar, |el, (offset, history, screen)| el.child(scroll_bar(this.clone(), self.bar_drag.clone(), offset, history, screen)))
                    .when(focused, |el| el.child(kit::input::layer(this.clone(), self.focus.clone())))
                    .into_any_element()
            })
    }
}

impl Terminals {
    /// La vista previa: atrás, recargar, la dirección y «abrir en el navegador»; abajo,
    /// la página. Su rectángulo lo toma la vista web en cada cuadro (`sync_preview`), con
    /// margen para que no asome por las esquinas redondeadas del panel.
    fn preview_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = t();
        let Some(preview) = &self.preview else {
            return div().into_any_element();
        };
        let bounds = self.preview_bounds.clone();
        let url = preview.url.clone();
        let button = |id: &'static str, icon: &'static str, tip: &'static str| gpui_m3::IconButton::new(id, icon).size(px(28.)).tooltip(crate::i18n::t(tip));
        div()
            .id("term-body")
            .relative()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(px(36.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(button("preview-back", "arrow-left", "pill.code.preview.back").on_click(cx.listener(|this, _: &gpui::ClickEvent, _, _| {
                        if let Some(preview) = &this.preview {
                            preview.back();
                        }
                    })))
                    .child(button("preview-reload", "refresh", "pill.code.preview.reload").on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        if let Some(preview) = this.preview.as_mut() {
                            preview.reload();
                            cx.notify();
                        }
                    })))
                    .child(div().flex_1().min_w(px(0.)).px(px(4.)).child(self.address.clone()))
                    .child(button("preview-inspect", "layers", "pill.code.preview.inspect").on_click(cx.listener(|this, _: &gpui::ClickEvent, _, _| {
                        if let Some(preview) = this.preview.as_mut() {
                            preview.inspect();
                        }
                    })))
                    .child(button("preview-open", "export", "pill.code.preview.openBrowser").on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| cx.open_url(&url)))),
            )
            // Mientras carga, una línea del acento bajo la barra.
            .child(div().flex_none().h(px(2.)).mx(px(12.)).rounded(px(1.)).bg(t.accent.opacity(if preview.loading { 0.6 } else { 0. })))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    .mx(px(8.))
                    .mt(px(4.))
                    .mb(px(8.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(t.hover)
                    .child(
                        canvas(
                            move |rect, window, _| {
                                if bounds.get() != Some(rect) {
                                    bounds.set(Some(rect));
                                    window.refresh();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    )
                    // Esperando a que el servidor local conteste.
                    .when_some(preview.waiting.clone(), |el, host| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(10.))
                                .px(px(24.))
                                .text_center()
                                .child(gpui_m3::LoadingIndicator::new().size(px(28.)))
                                .child(div().text_size(px(13.)).text_color(t.text).child(tf("pill.code.preview.waiting", &[("host", &host)])))
                                .child(div().text_size(px(12.)).text_color(t.muted).child(crate::i18n::t("pill.code.preview.waitingHint"))),
                        )
                    })
                    // Sin dirección todavía: qué se puede abrir aquí.
                    .when(preview.url.is_empty(), |el| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(8.))
                                .px(px(24.))
                                .text_center()
                                .child(gpui::svg().path("icons/monitor.svg").size(px(28.)).text_color(t.faint))
                                .child(div().text_size(px(13.)).text_color(t.muted).child(crate::i18n::t("pill.code.preview.empty"))),
                        )
                    })
                    // Con algo de GPUI encima, la foto fija de la página ocupa su lugar.
                    .when_some(preview.snapshot.clone(), |el, image| {
                        el.child(gpui::img(image).absolute().top_0().left_0().size_full().object_fit(gpui::ObjectFit::Fill))
                    }),
            )
            .into_any_element()
    }
}

/// La barra de desplazamiento de la consola: un pulgar delgado a la derecha que muestra
/// dónde está la vista en la historia. Se arrastra, y un clic en el riel salta ahí.
fn scroll_bar(this: WeakEntity<Terminals>, drag: Rc<std::cell::Cell<Option<f32>>>, offset: usize, history: usize, screen: usize) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            let track = f32::from(bounds.size.height);
            let thumb_h = (track * screen as f32 / (history + screen) as f32).max(24.).min(track);
            let span = track - thumb_h;
            if span <= 0. {
                return;
            }
            let top = span * (history - offset.min(history)) as f32 / history as f32;
            let active = drag.get().is_some() || hitbox.is_hovered(window);
            let thumb = Bounds::new(point(bounds.right() - px(8.), bounds.top() + px(top)), size(px(5.), px(thumb_h)));
            window.paint_quad(fill(thumb, style::t().faint.opacity(if active { 0.9 } else { 0.45 })).corner_radii(px(2.5)));
            // Del puntero (y donde se tomó el pulgar) a cuántas líneas sube la vista.
            let offset_at = move |y: Pixels, grab: f32| {
                let frac = ((f32::from(y - bounds.top()) - grab) / span).clamp(0., 1.);
                history - (frac * history as f32).round() as usize
            };
            let go = {
                let this = this.clone();
                move |offset: usize, cx: &mut App| {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(tab) = this.active_tab() {
                            tab.console.scroll_to(offset);
                        }
                        cx.notify();
                    });
                }
            };
            let (down_drag, down_go) = (drag.clone(), go.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                    return;
                }
                let y = f32::from(event.position.y - bounds.top());
                // Sobre el pulgar se toma donde estaba; en el riel, el pulgar salta con su centro ahí.
                let grab = if (top..top + thumb_h).contains(&y) { y - top } else { thumb_h / 2. };
                down_drag.set(Some(grab));
                down_go(offset_at(event.position.y, grab), cx);
                cx.stop_propagation();
            });
            let (move_drag, move_go) = (drag.clone(), go.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                let Some(grab) = move_drag.get() else {
                    return;
                };
                if phase != DispatchPhase::Capture {
                    return;
                }
                if event.pressed_button != Some(MouseButton::Left) {
                    move_drag.set(None);
                    return;
                }
                move_go(offset_at(event.position.y, grab), cx);
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left && drag.take().is_some() {
                    let _ = this.update(cx, |_, cx| cx.notify());
                }
            });
        },
    )
    .absolute()
    .top(px(INSET.0))
    .bottom(px(INSET.2))
    .right_0()
    .w(px(14.))
}

impl EntityInputHandler for Terminals {
    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        self.marked.as_ref().map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        self.type_text(text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(Bounds::new(
            gpui::point(bounds.origin.x + px(8.), bounds.origin.y + px(8.)),
            gpui::size(px(8.), px(18.)),
        ))
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_nombre_de_la_pestana_como_en_la_referencia() {
        assert_eq!(clean_name("  claude  "), Some("claude".into()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(&"x".repeat(60)).map(|n| n.chars().count()), Some(NAME_MAX));
        #[cfg(windows)]
        assert_eq!(shell_name(r"C:\Program Files\PowerShell\7\pwsh.exe"), "pwsh");
        #[cfg(not(windows))]
        assert_eq!(shell_name("/bin/zsh"), "zsh");
        assert_eq!(shell_name("powershell.exe"), "powershell");
    }

    #[test]
    fn la_cuota_muestra_la_ventana_mas_gastada() {
        use crate::quota::{AgentQuota, QuotaWindow};
        let window = |kind: &str, used: f64, resets_at: Option<i64>| QuotaWindow { kind: kind.into(), minutes: None, used_percent: used, resets_at };
        let quota = AgentQuota {
            agent: "claude".into(),
            plan: Some("Max".into()),
            windows: vec![window("5h", 30.0, Some(10_000)), window("weekly", 70.0, Some(20_000)), window("7dOpus", 99.0, Some(500))],
            spend: None,
            fetched_at: None,
            error: None,
        };
        // La de Opus ya se reinició: manda la semana, con 30 % libre.
        let chip = quota_chip(&quota, 1_000).unwrap();
        assert_eq!(chip.label, "Semana · 30 %");
        assert_eq!(chip.left, 30.0);
        assert!(chip.tip.starts_with("Plan Max"));
        // Sin ventanas ni gasto, el error va en el globito.
        let failed = AgentQuota { windows: Vec::new(), error: Some("Sin sesión".into()), plan: None, ..quota };
        assert_eq!(quota_chip(&failed, 1_000).map(|c| c.tip), Some("Sin sesión".into()));
    }

    #[test]
    fn el_zoom_avanza_de_a_un_punto_con_topes() {
        assert_eq!(zoomed(12.5, Some(1.)), 13.5);
        assert_eq!(zoomed(FONT_MAX, Some(1.)), FONT_MAX);
        assert_eq!(zoomed(FONT_MIN, Some(-1.)), FONT_MIN);
        assert_eq!(zoomed(20., None), DEFAULT_FONT);
    }

    #[test]
    fn las_rutas_van_entre_comillas_si_tienen_espacios() {
        let paths = [PathBuf::from(r"C:\fotos\a.png"), PathBuf::from(r"C:\Mis fotos\b.png")];
        assert_eq!(quoted_paths(&paths), "C:\\fotos\\a.png \"C:\\Mis fotos\\b.png\" ");
        assert_eq!(quoted_paths(&[]), "");
    }

    #[test]
    fn a_la_derecha_deja_lugar_al_resto() {
        assert_eq!(clamp_width(520., 1600.), 520.);
        assert_eq!(clamp_width(1400., 1600.), 1600. - REST_MIN_W);
        assert_eq!(clamp_width(100., 1600.), MIN_WIDTH);
    }

    #[test]
    fn el_alto_deja_lugar_al_chat() {
        assert_eq!(clamp_height(260., 860.), 260.);
        assert_eq!(clamp_height(800., 860.), 860. - CHAT_MIN);
        assert_eq!(clamp_height(40., 860.), MIN_HEIGHT);
    }

    #[test]
    fn la_paleta_cambia_con_el_estilo_y_el_modo() {
        assert_ne!(ansi(Style::Expressive, false), ansi(Style::Expressive, true));
        assert_ne!(ansi(Style::Formal, false), ansi(Style::Glass, false));
        // Rojo de M3 oscuro y blanco brillante, como en `Terminal.tsx`.
        assert_eq!(ansi(Style::Expressive, false)[1], 0xf2b8b5);
        assert_eq!(ansi(Style::Expressive, false)[15], 0xffffff);
    }
}
