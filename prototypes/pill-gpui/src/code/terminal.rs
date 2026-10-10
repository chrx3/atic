//! La terminal integrada (`Terminal.tsx` y `terminals.ts` de la referencia): un panel
//! bajo el chat con varias pestañas, cada una con su consola.
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

use std::path::PathBuf;
use std::rc::Rc;

use alacritty_terminal::term::TermMode;
use futures::StreamExt;
use gpui::{
    canvas, div, prelude::*, px, App, Bounds, Context, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Font, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, ScrollWheelEvent,
    SharedString, UTF16Selection, Window,
};

use crate::space::console::{self, Console, GridSize, Launch, Palette};
use crate::space::{self, Cell, GridLook};

use super::style::{self, t, Style};

/// El contexto de teclas del panel.
const CONTEXT: &str = "CodeTerminal";
/// El tamaño de la letra (la referencia usa 12,5 con interlineado 1,25).
const FONT_SIZE: f32 = 12.5;
const LINE_HEIGHT: f32 = 1.3;
/// El alto por defecto y el mínimo (`Terminal.tsx`); el máximo deja 220 px al chat.
pub const DEFAULT_HEIGHT: f32 = 260.0;
const MIN_HEIGHT: f32 = 120.0;
const CHAT_MIN: f32 = 220.0;
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
    /// Se ocultó o se cerró la última pestaña: el foco vuelve a la caja.
    Hidden,
    /// No se pudo abrir una terminal (va en un aviso).
    Error(String),
}

struct TermTab {
    id: u64,
    console: Console,
    /// El nombre del shell (`pwsh`, `powershell`).
    shell: String,
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

impl Terminals {
    pub fn new(height: Option<f32>, cx: &mut Context<Self>) -> Self {
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
        let mut font = gpui::font(t().mono);
        font.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec!["Cascadia Mono".into(), "Consolas".into()]));
        Self {
            focus: cx.focus_handle(),
            tabs: Vec::new(),
            active: None,
            shown: false,
            height: height.unwrap_or(DEFAULT_HEIGHT),
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
        }
    }

    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    pub fn shown(&self) -> bool {
        self.shown && !self.tabs.is_empty()
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
        let cell = self.cell(window);
        let program = space::powershell();
        let launch = Launch {
            program: program.clone(),
            args: vec!["-NoLogo".into()],
            cwd: self.cwd.clone().filter(|dir| dir.is_dir()).or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from)),
            env: Vec::new(),
        };
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
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TermTab {
            id,
            console,
            shell: shell_name(&program),
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
        self.tabs.remove(index);
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
        let w = text
            .advance(id, px(FONT_SIZE), 'm')
            .map(|s| f32::from(s.width))
            .unwrap_or(FONT_SIZE * 0.6);
        let cell = Cell { w, h: (FONT_SIZE * LINE_HEIGHT).round() };
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
        let Some(tab) = self.active_tab() else {
            return;
        };
        // Ctrl+V pega (Ctrl+Mayús+V también, como en Windows Terminal).
        if m.control && !m.alt && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                tab.console.paste(&text);
            }
            cx.stop_propagation();
            return;
        }
        let app_cursor = tab.console.term.lock().mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = console::key_bytes(ks, app_cursor) {
            tab.console.scroll(i32::MIN / 2);
            tab.console.write(bytes);
            cx.stop_propagation();
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let cell = self.cell(window);
        let dy = f32::from(event.delta.pixel_delta(px(cell.h)).y);
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
        let mut strip = gpui_m3::TabStrip::new("term-tabs")
            .tabs(tabs.collect::<Vec<_>>())
            .trailing(
                gpui_m3::IconButton::new("term-new", "plus")
                    .size(px(28.))
                    .tooltip("Nueva terminal")
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                        if let Err(error) = this.open(None, None, window, cx) {
                            cx.emit(TerminalEvent::Error(error));
                        }
                    })),
            )
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
            .child(
                gpui_m3::IconButton::new("term-hide", "chevron-down")
                    .size(px(28.))
                    .tooltip("Ocultar (Ctrl+J)")
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.hide(cx))),
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
        let window_h = f32::from(window.viewport_size().height);
        let height = clamp_height(self.height, window_h);
        let body = self.body.clone();
        let this = cx.weak_entity();
        let resize = gpui_m3::ResizeHandle::height("term-resize", px(height))
            .min(px(MIN_HEIGHT))
            .max(px((window_h - CHAT_MIN).max(MIN_HEIGHT)))
            .invert(true)
            .on_resize({
                let this = this.clone();
                move |h, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.height = f32::from(h);
                        cx.notify();
                    });
                }
            })
            .on_resize_end({
                let this = this.clone();
                move |h, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.height = f32::from(h);
                        cx.emit(TerminalEvent::Height(this.height));
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
            .h(px(height))
            .flex()
            .flex_col()
            .bg(panel_bg())
            .map(|el| match t.style {
                // Un contenedor tonal dentro del chat.
                Style::Expressive => el.mx(px(8.)).mb(px(8.)).rounded(px(20.)),
                // Una tarjeta flotante.
                Style::Glass => el.mt(px(8.)).rounded(px(18.)).border_1().border_color(t.highlight.opacity(0.35)),
                Style::Formal => el.border_t_1().border_color(t.border),
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(if expressive || glass { -8. } else { -4. }))
                    .h(px(8.))
                    .child(resize),
            )
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
                                            space::paint_grid(grid, bounds, cell, 1.0, FONT_SIZE, focused, look, window, cx);
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
        assert_eq!(shell_name(r"C:\Program Files\PowerShell\7\pwsh.exe"), "pwsh");
        assert_eq!(shell_name("powershell.exe"), "powershell");
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
