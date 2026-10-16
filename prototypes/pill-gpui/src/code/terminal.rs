//! La terminal integrada (`Terminal.tsx` y `terminals.ts` de la referencia): un panel
//! bajo el chat o a su derecha (`Dock`) con varias pestañas, cada una con su consola.
//! Se le puede hacer zoom (Ctrl+rueda, Ctrl+= / Ctrl+- / Ctrl+0) y soltarle archivos,
//! imágenes o texto, que llegan como rutas entre comillas o como texto pegado.
//!
//! Además del shell, abre los CLI de agentes instalados (Claude Code, OpenCode, Codex,
//! Cursor, Antigravity, Grok, Gemini) en la carpeta del proyecto, con las demás carpetas
//! del espacio (`--add-dir`) y los hooks de Atic, como las consolas del espacio. Con la
//! pestaña de un agente a la vista, un chip muestra cuánto le queda de cuota (`quota`).
//!
//! Las consolas son las del Mando (`space::console`: alacritty + ConPTY) y se
//! dibujan con su misma grilla (`space::grid_of`, `space::paint_grid`). Aquí
//! va lo propio de Atic Code: las pestañas (nueva, cerrar, renombrar con doble
//! clic), el alto que se ajusta y se recuerda, la paleta ANSI de la referencia según
//! el estilo y el modo, y el teclado.
//!
//! El texto (letras, tildes, AltGr) llega por el manejador de texto de la
//! ventana (`space::input::layer`), como en el Mando; las teclas especiales y
//! los atajos con Ctrl, por `console::key_bytes`. Dentro de la terminal los
//! atajos de Atic Code con Ctrl+letra no actúan (`bind_keys`): son del shell.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use alacritty_terminal::term::TermMode;
use futures::StreamExt;
use gpui::{
    canvas, div, prelude::*, px, App, Bounds, ClipboardEntry, Context, Entity, EntityInputHandler, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, Font, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, UTF16Selection, Window,
};
use serde::{Deserialize, Serialize};

use crate::space::console::{self, Console, GridSize, Launch, Palette};
use crate::space::{self, Cell, GridLook};

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
    /// El texto a medio componer de una tecla muerta (ver `space::input`).
    marked: Option<String>,
    font: Font,
    fonts: [Font; 4],
    cell: Option<Cell>,
    /// Dónde quedó la grilla en el último cuadro: de ahí salen filas y columnas.
    body: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
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
}

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
        let reset = window.resets_at.map(|at| format!(" · se reinicia en {}", crate::usage::span(at - now_ms))).unwrap_or_default();
        lines.push(format!("{label}: {:.0} % usado{reset}", window.used_percent));
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
                lines.push(format!("Gastado: US$ {:.2}", spend.cents / 100.0));
                return Some(QuotaChip { label: format!("US$ {:.2}", spend.cents / 100.0), left: 100.0, tip: lines.join("\n") });
            }
            let error = quota.error.as_ref()?;
            lines.push(error.clone());
            Some(QuotaChip { label: "Cuota ?".into(), left: 100.0, tip: lines.join("\n") })
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
    let w = f32::from(bounds.size.width) - space::GRID_PAD * 2.;
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
            fonts: space::fonts_of(font.clone()),
            font,
            cell: None,
            body: Rc::new(std::cell::Cell::new(None)),
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
        }
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
        self.shown && !self.tabs.is_empty()
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
        let (program, args) = space::shell();
        let launch = Launch {
            program: program.clone(),
            args,
            cwd: self.open_dir(),
            env: Vec::new(),
        };
        self.spawn_tab(launch, shell_name(&program), name, None, None, command, window, cx)
    }

    /// Abre un CLI de agente en una pestaña nueva, en `cmd /k` (deja el prompt al
    /// terminar), con los hooks de Atic y las demás carpetas del espacio, como las
    /// consolas del espacio (`space::open_at`).
    pub fn open_agent(&mut self, launcher: Launcher, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let mut line = launcher.cli.to_string();
        let mut env = Vec::new();
        let token = crate::agent_prompts::prepare(launcher.cli, &line).map(|(hooked, token)| {
            line = hooked;
            env.push((crate::agent_prompts::token_var().to_string(), token.clone()));
            token
        });
        // Después de los hooks: con comillas en la línea, `prepare` no los pondría.
        let line = space::workspaces::with_add_dirs(launcher.cli, &line, &self.extras);
        let (program, args) = space::agent_shell(&line);
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
            .map_err(|error| format!("No se pudo abrir la terminal: {error}"))?;
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

    /// Ctrl+J: la oculta si se ve; si no, la muestra (y abre una si no hay).
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        if self.shown() {
            self.hide(cx);
            return Ok(());
        }
        if self.tabs.is_empty() {
            return self.open(None, None, window, cx);
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
        }
        if self.tabs.is_empty() {
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
                    None => cx.emit(TerminalEvent::Error("No se pudo guardar la imagen para pegarla.".into())),
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
        let lines = (dy / cell.h).round() as i32;
        if let (Some(tab), true) = (self.active_tab(), lines != 0) {
            tab.console.scroll(lines);
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
                tab.console.note("\r\n\x1b[2m[proceso terminado]\x1b[0m\r\n");
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
                .tooltip(if tab.console.exited() { "Terminó · doble clic para renombrar" } else { "Doble clic para renombrar" })
        });
        // «+» abre el shell; al lado, el logo de cada agente instalado abre su CLI.
        let mut new = div().flex().items_center().gap(px(2.)).child(
            gpui_m3::IconButton::new("term-new", "plus")
                .size(px(28.))
                .tooltip("Nueva terminal")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                    if let Err(error) = this.open(None, None, window, cx) {
                        cx.emit(TerminalEvent::Error(error));
                    }
                })),
        );
        let t = t();
        for (index, launcher) in self.installed.iter().copied().filter(|l| !self.hidden.iter().any(|h| h == l.cli)).enumerate() {
            new = new.child(
                div()
                    .id(("term-agent", index))
                    .size(px(28.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(14.))
                    .cursor_pointer()
                    .hover(|el| el.bg(t.hover))
                    .tooltip(crate::hover::tip_text(format!("Abrir {}", launcher.name).into()))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        if let Err(error) = this.open_agent(launcher, window, cx) {
                            cx.emit(TerminalEvent::Error(error));
                        }
                    }))
                    .child(gpui::svg().path(launcher.logo).size(px(15.)).text_color(t.muted)),
            );
        }
        let mut strip = gpui_m3::TabStrip::new("term-tabs").tabs(tabs.collect::<Vec<_>>()).trailing(new)
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
                    Dock::Bottom => ("panel-right", "Mover a la derecha", Dock::Right),
                    Dock::Right => ("panel-bottom", "Mover abajo", Dock::Bottom),
                };
                gpui_m3::IconButton::new("term-dock", icon)
                    .size(px(28.))
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| this.set_dock(other, window, cx)))
            })
            .child(
                gpui_m3::IconButton::new("term-hide", if self.dock == Dock::Right { "chevron-right" } else { "chevron-down" })
                    .size(px(28.))
                    .tooltip(crate::keymap::hint("Ocultar (Ctrl+J)"))
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.hide(cx))),
            )
    }
}

impl Terminals {
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
        let tip = format!("{}\nClic para actualizar", chip.tip);
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
                .border_1()
                .border_color(t.border)
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
        if !self.shown() {
            return div().id("code-terminal");
        }
        let cell = self.cell(window);
        self.sync(cell);
        let t = t();
        let palette = palette();
        let focused = self.focus.is_focused(window);
        let grid = self.active_tab().map(|tab| space::grid_of(&tab.console, &self.fonts, &palette));
        let look = GridLook {
            pad: space::GRID_PAD,
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
            .child(
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
                            .left(px(INSET.3 - space::GRID_PAD))
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
                                            space::paint_grid(grid, bounds, cell, 1.0, font_size, focused, look, window, cx);
                                        }
                                    },
                                )
                                .size_full(),
                            ),
                    )
                    .when(focused, |el| el.child(space::input::layer(this.clone(), self.focus.clone()))),
            )
    }
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
