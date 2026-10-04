//! La vista «Mando» del espacio: la forma de mirar muchos agentes a la vez
//! sin recorrer un plano.
//!
//! Arriba, una carta por agente con su estado. Debajo, la consola enfocada
//! grande (la misma terminal de la pizarra, dibujada con el mismo `paint_grid`)
//! y, a la derecha, la **bandeja de revisión**: lo que terminó o se detuvo y
//! nadie ha mirado todavía. La pizarra sigue ahí como la otra vista del
//! espacio (`View::Pizarra`).
//!
//! Los estados salen de la salida de cada PTY, como en `consoleStatus.ts` de
//! Atic: trabajando si escribió hace menos de 1,5 s; un turno que se calla
//! después de 3 s o más y que nadie miraba pasa a la bandeja. «Bloqueado por
//! un permiso» todavía no existe: en Atic llega por los hooks.
//!
//! **Estilo**: colores planos y sin bordes. Entre una capa y la siguiente
//! solo cambia el color (ventana, terminal, panel, carta enfocada). GPUI no
//! recorta el contenido a las esquinas redondeadas, solo a un rectángulo: por
//! eso nada pinta contra la esquina de su contenedor. Todo va metido hacia
//! adentro y con su propio radio (el de afuera es el de adentro más el margen).
//!
//! Todo el estado propio vive en `State`; este módulo solo toca `SpaceView`
//! por sus campos y por los ganchos `tick_mando`, `render` y `key_down`.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, point, prelude::*, px, size, svg, AnyElement, Bounds, ClickEvent, Context, Div,
    ElementId, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, ScrollWheelEvent,
    SharedString, Stateful, Window,
};

use super::chrome;
use super::console::{self, hsla, GridSize};
use super::{
    paint_grid, Area, Cell, Open, SpaceView, FONT_FAMILY, FONT_SIZE, PAD, TOOLBAR_H, WORKING_FOR,
};

/// Cómo se mira el espacio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Cartas, la consola enfocada y la bandeja.
    Mando,
    /// El plano infinito con las consolas sueltas.
    Pizarra,
}

/// La barra de arriba es la de la ventana: mide lo mismo en las dos vistas.
const TOP_H: f32 = TOOLBAR_H;
const TILES_Y: f32 = TOP_H + 2.0;
const STATUS_H: f32 = 30.0;
const GUTTER: f32 = 16.0;
const GAP: f32 = 12.0;
const TILE_H: f32 = 84.0;
const TILE_GAP: f32 = 10.0;
const TRAY_W: f32 = 340.0;
/// Margen del panel de la consola y alto de su encabezado.
const PANEL_PAD: f32 = 10.0;
const HEAD_H: f32 = 44.0;
/// Aire sobre la primera fila de la terminal y bajo la última.
const TERM_TOP: f32 = 6.0;
const TERM_BOTTOM: f32 = 8.0;
/// Menos que esto no es un turno: un repintado o el eco de unas teclas.
const TURN: Duration = Duration::from_secs(3);

// Radios: cada capa de adentro tiene menos que la de afuera.
const R_PANEL: f32 = 20.0;
const R_TILE: f32 = 16.0;
const R_ITEM: f32 = 14.0;
const R_INSET: f32 = 12.0;

// Colores, de más oscuro a más claro. Sin bordes: lo único que separa dos
// cosas es que una es de otro color.
const WINDOW: u32 = 0x0f0f0e;
/// La terminal: el fondo con el que ya se dibujan sus celdas.
const TERMINAL: u32 = console::BACKGROUND;
const SURFACE: u32 = 0x1d1d1b;
const SURFACE_HOVER: u32 = 0x252523;
/// La carta enfocada: se distingue por ser más clara, no por un marco.
const SURFACE_ON: u32 = 0x2d2d2a;
/// Un elemento dentro de un panel (los de la bandeja).
const ITEM: u32 = 0x262624;
const TEXT: u32 = 0xf0f0ea;
const MUTED: u32 = 0x9a9a90;
const FAINT: u32 = 0x6a6a64;
const INK: u32 = 0x141413;
const WORKING_DOT: u32 = 0xe8b04b;
const READY_DOT: u32 = 0x6cc48a;

// --- Estados ------------------------------------------------------------------

/// En qué anda una consola.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Working,
    Ready,
    Ended,
}

pub fn phase(ended: bool, quiet: Duration) -> Phase {
    if ended {
        Phase::Ended
    } else if quiet < WORKING_FOR {
        Phase::Working
    } else {
        Phase::Ready
    }
}

/// El estado que se muestra. Un PTY al que se le acaba de cambiar el tamaño
/// repinta entero: eso es salida, pero no es trabajo. Mientras dura el
/// repintado, un agente que estaba listo sigue mostrándose listo.
pub fn effective_phase(shown: Phase, raw: Phase, settling: bool) -> Phase {
    if settling && shown == Phase::Ready && raw == Phase::Working {
        Phase::Ready
    } else {
        raw
    }
}

/// Lo que dura el repintado de una TUI después de cambiarle el tamaño.
const SETTLE: Duration = Duration::from_millis(1500);

/// ¿Este cambio de estado merece un lugar en la bandeja? Solo si nadie la
/// está mirando, y por algo que valga: un proceso que terminó, o un turno
/// de verdad que se calló.
pub fn needs_attention(prev: Phase, next: Phase, turn: Duration, watched: bool) -> bool {
    if watched || prev == next {
        return false;
    }
    match next {
        Phase::Ended => true,
        Phase::Ready => prev == Phase::Working && turn >= TURN,
        Phase::Working => false,
    }
}

/// El título que una app le pone a su ventana, si dice algo. Un shell pone la
/// ruta de su ejecutable o la carpeta donde está (`PS C:\…>`), y eso no ayuda
/// a reconocer nada.
pub fn useful_title(title: &str) -> Option<&str> {
    let title = title.trim();
    let path = title.contains(":\\") || title.contains(":/");
    let exe = title.to_ascii_lowercase().ends_with(".exe");
    (!title.is_empty() && !path && !exe).then_some(title)
}

pub fn elapsed_label(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} min", secs / 60)
    } else {
        format!("{} h", secs / 3600)
    }
}

/// «1 listo», «3 listos».
pub fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// A quién pasa el foco cuando `gone` deja de verse: la consola visible que
/// sigue en el orden en que se abrieron o, si no hay, la anterior. `order` va
/// ordenado por id y dice cuáles están en segundo plano.
pub fn pick_visible(order: &[(u64, bool)], gone: u64) -> Option<u64> {
    let visible: Vec<u64> = order
        .iter()
        .filter(|(id, background)| *id != gone && !background)
        .map(|(id, _)| *id)
        .collect();
    visible.iter().copied().find(|id| *id > gone).or_else(|| visible.last().copied())
}

/// Lo que dejó una consola al terminar, esperando a que alguien lo mire.
struct Unseen {
    at: Instant,
    /// Las últimas líneas de la pantalla, para decidir sin abrirla.
    summary: Vec<String>,
    ended: bool,
}

struct Watch {
    phase: Phase,
    /// Desde cuándo está en ese estado, según lo que vio la interfaz.
    since: Instant,
    unseen: Option<Unseen>,
    /// Hasta cuándo la salida de la consola es un repintado por cambio de
    /// tamaño y no trabajo (ver `effective_phase`).
    settle_until: Instant,
}

/// Lo que el Mando recuerda de cada consola entre cuadros.
#[derive(Default)]
pub struct State {
    watch: HashMap<u64, Watch>,
    /// La ventana está maximizada: el botón de la barra pasa a «restaurar».
    maximized: bool,
    /// La consola a la que se le pulsó «Cerrar» y espera la respuesta: dejarla
    /// en segundo plano o terminarla.
    closing: Option<u64>,
}

impl State {
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Pone al día los estados y levanta lo que pasó a la bandeja.
    fn refresh(&mut self, cards: &[super::Card], focused: Option<u64>, now: Instant) {
        self.watch.retain(|id, _| cards.iter().any(|c| c.id == *id));
        for card in cards {
            let raw = phase(card.console.exited(), card.console.quiet_for());
            let watched = focused == Some(card.id);
            let watch = self.watch.entry(card.id).or_insert(Watch {
                phase: raw,
                since: now,
                unseen: None,
                settle_until: now,
            });
            let next = effective_phase(watch.phase, raw, now < watch.settle_until);
            if watch.phase != next {
                let turn = now.duration_since(watch.since);
                if needs_attention(watch.phase, next, turn, watched) {
                    watch.unseen = Some(Unseen {
                        at: now,
                        summary: card.console.last_lines(3),
                        ended: next == Phase::Ended,
                    });
                }
                watch.phase = next;
                watch.since = now;
            }
            if watched {
                watch.unseen = None;
            }
        }
    }

    /// Se le acaba de cambiar el tamaño a esta consola: su próxima salida es
    /// un repintado.
    fn settle(&mut self, id: u64, now: Instant) {
        if let Some(watch) = self.watch.get_mut(&id) {
            watch.settle_until = now + SETTLE;
        }
    }
}

// --- Ganchos en SpaceView -------------------------------------------------------

impl SpaceView {
    /// Se llama en cada cuadro, con cualquier vista, para que la bandeja no
    /// se pierda lo que pasa mientras se mira la pizarra.
    pub(super) fn tick_mando(&mut self, window: &Window, now: Instant) {
        self.mando.maximized = window.is_maximized();
        let focused = self.focused;
        self.mando.refresh(&self.cards, focused, now);
        // La consola que se mira en el Mando ya no está «en segundo plano».
        if self.view == View::Mando {
            if let Some(card) = focused.and_then(|id| self.card_mut(id)) {
                card.background = false;
            }
        }
    }

    /// Los ids de las consolas, en el orden en que se abrieron, con si están
    /// en segundo plano.
    fn order(&self) -> Vec<(u64, bool)> {
        let mut order: Vec<(u64, bool)> = self.cards.iter().map(|c| (c.id, c.background)).collect();
        order.sort();
        order
    }

    /// Las que se numeran con Ctrl+1…9: las del Mando sin las que están en
    /// segundo plano; la pizarra las muestra todas.
    fn visible_ids(&self) -> Vec<u64> {
        let pizarra = self.view == View::Pizarra;
        self.order().into_iter().filter(|(_, bg)| pizarra || !bg).map(|(id, _)| id).collect()
    }

    /// «Cerrar» → «En segundo plano»: el agente sigue trabajando, sin carta.
    /// Vuelve desde la bandeja («Abrir»); si termina o se detiene, avisa en la
    /// bandeja de revisión como cualquier otro, y «Ver» lo trae de vuelta.
    fn detach(&mut self, id: u64, cx: &mut Context<Self>) {
        self.mando.closing = None;
        let next = pick_visible(&self.order(), id);
        if let Some(card) = self.card_mut(id) {
            card.background = true;
        }
        if self.focused == Some(id) {
            self.focused = next;
        }
        cx.notify();
    }

    /// «Cerrar» → «Terminar»: el agente se detiene y su consola se va.
    fn terminate(&mut self, id: u64, cx: &mut Context<Self>) {
        self.mando.closing = None;
        let next = pick_visible(&self.order(), id);
        let was_focused = self.focused == Some(id);
        self.close(id, cx);
        if was_focused {
            self.focused = next;
        }
        cx.notify();
    }

    /// Enfoca una consola: en el Mando es la que se ve grande; en la pizarra,
    /// la que sube y se muestra. Si estaba en segundo plano, vuelve.
    fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.card(id).is_none() {
            return;
        }
        self.mando.closing = None;
        if let Some(card) = self.card_mut(id) {
            card.background = false;
        }
        match self.view {
            View::Mando => self.focused = Some(id),
            View::Pizarra => {
                self.raise(id);
                self.reveal(id);
            }
        }
        if let Some(watch) = self.mando.watch.get_mut(&id) {
            watch.unseen = None;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// Salta a la consola que lleva más tiempo esperando que la miren.
    fn next_attention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let oldest = self
            .mando
            .watch
            .iter()
            .filter_map(|(id, w)| w.unseen.as_ref().map(|u| (u.at, *id)))
            .min();
        if let Some((_, id)) = oldest {
            self.select(id, window, cx);
        }
    }

    pub(super) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        self.view = view;
        if view == View::Pizarra {
            // En el Mando el PTY tomó el tamaño del panel: la pizarra lo
            // devuelve al de su tarjeta.
            let cell = self.cell();
            let now = Instant::now();
            let sizes: Vec<GridSize> = self.cards.iter().map(|c| self.grid_for(&c.area)).collect();
            for (card, grid) in self.cards.iter_mut().zip(sizes) {
                let before = card.console.size;
                card.console.resize(grid, (cell.w.round() as u16, cell.h as u16));
                if card.console.size != before {
                    self.mando.settle(card.id, now);
                }
            }
        }
        cx.notify();
    }
}

/// Atajos del Mando y de la pizarra; lo demás sigue al `key_down` de siempre.
///
/// - `Alt+J`: la consola que más espera. (`Ctrl+J` es el salto de línea de
///   Claude Code, no se puede tocar.)
/// - `Ctrl+1…9`: la consola número N.
/// - `Ctrl+Shift+M`: cambia entre Mando y pizarra.
pub(super) fn key_down(
    view: &mut SpaceView,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<SpaceView>,
) {
    let ks = &event.keystroke;
    let m = &ks.modifiers;
    // Esc cancela la pregunta de «Cerrar». Solo mientras está ahí: el resto
    // del tiempo Esc es del agente.
    if view.mando.closing.is_some() && ks.key == "escape" && !m.control && !m.alt && !m.shift {
        view.mando.closing = None;
        cx.notify();
        cx.stop_propagation();
        return;
    }
    if m.alt && !m.control && !m.shift && ks.key == "j" {
        view.next_attention(window, cx);
        cx.stop_propagation();
        return;
    }
    if m.control && m.shift && !m.alt && ks.key == "m" {
        let other = match view.view {
            View::Mando => View::Pizarra,
            View::Pizarra => View::Mando,
        };
        view.set_view(other, cx);
        cx.stop_propagation();
        return;
    }
    if m.control && !m.shift && !m.alt {
        if let Some(n) = ks.key.parse::<usize>().ok().filter(|n| (1..=9).contains(n)) {
            let ids = view.visible_ids();
            if let Some(&id) = ids.get(n - 1) {
                view.select(id, window, cx);
                cx.stop_propagation();
                return;
            }
        }
    }
    view.key_down(event, window, cx);
}

// --- Piezas ---------------------------------------------------------------------

/// El color de cada agente y su logo; sin agente, una terminal cualquiera.
fn agent_look(agent: Option<&str>) -> (&'static str, u32) {
    match agent {
        Some("claude") => ("icons/agents/claude.svg", 0xe08a6a),
        Some("codex") => ("icons/agents/openai.svg", 0x6fdc9a),
        Some("cursor-agent") => ("icons/agents/cursor.svg", 0x86b6ff),
        Some("opencode") => ("icons/agents/opencode.svg", 0xe8b4f0),
        Some("agy") => ("icons/agents/antigravity.svg", 0xf2c36b),
        Some("grok") => ("icons/agents/grok.svg", 0xc9c9c2),
        _ => ("icons/square-terminal.svg", 0x3a3a37),
    }
}

fn phase_dot(phase: Phase) -> u32 {
    match phase {
        Phase::Working => WORKING_DOT,
        Phase::Ready => READY_DOT,
        Phase::Ended => FAINT,
    }
}

fn phase_text(phase: Phase) -> &'static str {
    match phase {
        Phase::Working => "trabajando",
        Phase::Ready => "listo",
        Phase::Ended => "terminó",
    }
}

fn logo_tile(agent: Option<&str>, size: f32) -> impl IntoElement {
    let (path, color) = agent_look(agent);
    let glyph = if agent.is_some() { INK } else { TEXT };
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(size * 0.28))
        .bg(hsla(color))
        .flex()
        .items_center()
        .justify_center()
        .child(svg().path(path).size(px(size * 0.6)).flex_none().text_color(hsla(glyph)))
}

fn dot(color: u32) -> impl IntoElement {
    div().size(px(7.)).flex_none().rounded(px(4.)).bg(hsla(color))
}

/// La etiqueta chica de color de «para revisar» y del conteo de la bandeja.
fn badge(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(7.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .bg(hsla(READY_DOT))
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(hsla(INK))
        .child(text.into())
}

/// Una tecla, para decir el atajo dentro de un botón.
fn keycap(text: &'static str, fill: u32) -> Div {
    div()
        .h(px(18.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(hsla(fill))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(MUTED))
        .child(text)
}

/// Sobre qué está un botón: su color es el único borde que tiene.
#[derive(Clone, Copy)]
enum Tone {
    /// El botón principal: claro sobre lo oscuro.
    Light,
    /// Sobre el fondo de la ventana.
    Window,
    /// Sobre un panel.
    Surface,
    /// Sobre un elemento de la bandeja.
    Item,
    /// Lo que detiene a un agente: un rojo apagado, plano, sin borde.
    Danger,
}

impl Tone {
    /// Color en reposo y con el cursor encima.
    fn fill(self) -> (u32, u32) {
        match self {
            Tone::Light => (0xe9e9e2, 0xffffff),
            Tone::Window => (0x1f1f1d, 0x2a2a28),
            Tone::Surface => (0x2a2a28, 0x353532),
            Tone::Item => (0x33332f, 0x3e3e39),
            Tone::Danger => (0x3b2321, 0x4a2b28),
        }
    }

    fn ink(self) -> u32 {
        match self {
            Tone::Light => INK,
            Tone::Danger => 0xffa89b,
            _ => TEXT,
        }
    }
}

fn pill(id: impl Into<ElementId>, label: impl Into<SharedString>, tone: Tone) -> Stateful<Div> {
    let (fill, hover) = tone.fill();
    div()
        .id(id)
        .h(px(28.))
        .px(px(13.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(tone.ink()))
        .bg(hsla(fill))
        .hover(move |el| el.bg(hsla(hover)))
        .cursor_pointer()
        .child(label.into())
}

/// Las pestañas de las vistas; las comparten el Mando y la barra de la pizarra.
pub(super) fn tabs(current: View, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let tab = |id: &'static str, label: &'static str, view: View, cx: &mut Context<SpaceView>| {
        let on = current == view;
        div()
            .id(id)
            .h(px(24.))
            .px(px(14.))
            .flex()
            .items_center()
            .rounded(px(12.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(hsla(if on { INK } else { MUTED }))
            .when(on, |el| el.bg(hsla(0xe9e9e2)))
            .when(!on, |el| el.cursor_pointer().hover(|el| el.text_color(hsla(TEXT))))
            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.set_view(view, cx)))
            .child(label)
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(3.))
        .rounded(px(15.))
        .bg(hsla(0x1f1f1d))
        .child(tab("tab-mando", "Mando", View::Mando, cx))
        .child(tab("tab-pizarra", "Pizarra", View::Pizarra, cx))
}

// --- Datos del cuadro -------------------------------------------------------------

struct TileData {
    id: u64,
    label: SharedString,
    sub: SharedString,
    agent: Option<&'static str>,
    phase: Phase,
    since: String,
    unseen: bool,
    focused: bool,
    /// Se dejó en segundo plano: no tiene carta, pero sigue en la bandeja.
    background: bool,
}

struct TrayItem {
    id: u64,
    label: SharedString,
    agent: Option<&'static str>,
    ago: String,
    lines: Vec<String>,
    ended: bool,
}

/// Lo que va en el encabezado del panel de la consola enfocada.
struct Head {
    id: u64,
    label: SharedString,
    sub: SharedString,
    agent: Option<&'static str>,
    phase: Phase,
    /// Se pulsó «Cerrar»: el encabezado pregunta qué hacer con el agente.
    closing: bool,
}

struct Layout {
    panel: Area,
    tray: Area,
}

impl Layout {
    fn new(vw: f32, vh: f32) -> Self {
        let y = TILES_Y + TILE_H + GAP;
        let h = (vh - STATUS_H - 4.0 - y).max(160.0);
        let tray_w = TRAY_W.min(vw * 0.32).max(240.0);
        let panel_w = (vw - GUTTER * 2.0 - GAP - tray_w).max(320.0);
        Self {
            panel: Area { x: GUTTER, y, w: panel_w, h },
            tray: Area { x: GUTTER + panel_w + GAP, y, w: tray_w, h },
        }
    }

    /// El rectángulo de la terminal, en coordenadas del panel: metido hacia
    /// adentro, para que nada suyo toque la esquina redonda del panel.
    fn term(&self) -> Area {
        Area {
            x: PANEL_PAD,
            y: PANEL_PAD + HEAD_H,
            w: self.panel.w - PANEL_PAD * 2.0,
            h: self.panel.h - HEAD_H - PANEL_PAD * 2.0,
        }
    }

    /// Cuántas columnas y filas le caben al PTY dentro de la terminal.
    fn term_grid(&self, cell: Cell) -> GridSize {
        let term = self.term();
        GridSize {
            cols: (((term.w - PAD * 2.0) / cell.w).floor() as usize).max(20),
            rows: (((term.h - TERM_TOP - 2.0 - TERM_BOTTOM) / cell.h).floor() as usize).max(5),
        }
    }
}

/// La línea de abajo del nombre: lo que el agente dice de sí (su título, que
/// suele ser la tarea), si no la carpeta donde trabaja, y si no la última
/// línea de su salida.
pub(super) fn card_sub(card: &super::Card) -> String {
    let title = card.console.title();
    if let Some(title) = title.as_deref().and_then(useful_title) {
        // «✳ Claude Code» repite el nombre de la carta: no dice nada.
        if !title.to_lowercase().contains(&card.label.to_lowercase()) {
            return title.to_string();
        }
    }
    if let Some(folder) = card.cwd.as_deref().and_then(Path::file_name) {
        return folder.to_string_lossy().into_owned();
    }
    card.console.last_lines(1).pop().map(|line| line.trim().to_string()).unwrap_or_default()
}

// --- Dibujo ----------------------------------------------------------------------

pub(super) fn render(
    view: &mut SpaceView,
    _window: &mut Window,
    cx: &mut Context<SpaceView>,
) -> AnyElement {
    let (vw, vh) = view.viewport;
    let cell = view.cell();
    let layout = Layout::new(vw, vh);
    let now = Instant::now();
    let maximized = view.mando.maximized;

    // La consola enfocada ocupa justo la terminal: ni sobra ni falta.
    if let Some(id) = view.focused {
        let grid = layout.term_grid(cell);
        let resized = view.card_mut(id).is_some_and(|card| {
            let before = card.console.size;
            card.console.resize(grid, (cell.w.round() as u16, cell.h as u16));
            card.console.size != before
        });
        if resized {
            view.mando.settle(id, now);
        }
    }

    let mut rows: Vec<TileData> = view
        .cards
        .iter()
        .map(|card| {
            let watch = view.mando.watch.get(&card.id);
            let phase_now = watch
                .map(|w| w.phase)
                .unwrap_or_else(|| phase(card.console.exited(), card.console.quiet_for()));
            let since = watch.map(|w| elapsed_label(now.duration_since(w.since))).unwrap_or_default();
            TileData {
                id: card.id,
                label: card.label.clone(),
                sub: card_sub(card).into(),
                agent: card.agent,
                phase: phase_now,
                since,
                unseen: watch.is_some_and(|w| w.unseen.is_some()),
                focused: view.focused == Some(card.id),
                background: card.background,
            }
        })
        .collect();
    rows.sort_by_key(|t| t.id);

    let mut inbox: Vec<(Instant, TrayItem)> = view
        .cards
        .iter()
        .filter_map(|card| {
            let watch = view.mando.watch.get(&card.id)?;
            let unseen = watch.unseen.as_ref()?;
            Some((
                unseen.at,
                TrayItem {
                    id: card.id,
                    label: card.label.clone(),
                    agent: card.agent,
                    ago: elapsed_label(now.duration_since(unseen.at)),
                    lines: unseen.summary.clone(),
                    ended: unseen.ended,
                },
            ))
        })
        .collect();
    inbox.sort_by_key(|(at, _)| *at);
    let inbox: Vec<TrayItem> = inbox.into_iter().map(|(_, item)| item).collect();

    let head = view
        .focused
        .and_then(|id| rows.iter().find(|t| t.id == id))
        .map(|t| Head {
            id: t.id,
            label: t.label.clone(),
            sub: t.sub.clone(),
            agent: t.agent,
            phase: t.phase,
            closing: view.mando.closing == Some(t.id),
        });
    let grid = view.focused.and_then(|id| view.card(id)).map(|card| view.grid_rows(card));
    for card in &view.cards {
        card.console.take_dirty();
    }

    // Los estados cuentan a todas las consolas, también a las que están en
    // segundo plano: siguen trabajando aunque no tengan carta.
    let working = rows.iter().filter(|t| t.phase == Phase::Working).count();
    let ready = rows.iter().filter(|t| t.phase == Phase::Ready).count();
    let ended = rows.iter().filter(|t| t.phase == Phase::Ended).count();
    let (parked, tiles): (Vec<TileData>, Vec<TileData>) = rows.into_iter().partition(|t| t.background);
    let total = tiles.len();

    let tile_w = if total == 0 {
        0.0
    } else {
        ((vw - GUTTER * 2.0 - TILE_GAP * (total as f32 - 1.0)) / total as f32).clamp(180.0, 320.0)
    };
    let panel = layout.panel;
    let tray = layout.tray;
    let term = layout.term();

    div()
        .id("space")
        .key_context("Space")
        .track_focus(&view.focus)
        .on_key_down(cx.listener(key_down))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _: &MouseDownEvent, window, _| window.focus(&view.focus)),
        )
        .size_full()
        .relative()
        .bg(hsla(WINDOW))
        .font_family("Segoe UI")
        .child(super::input::layer(cx.weak_entity(), view.focus.clone()))
        .child(top_bar(maximized, cx))
        .child(
            div()
                .absolute()
                .top(px(TILES_Y))
                .left(px(GUTTER))
                .right(px(GUTTER))
                .h(px(TILE_H))
                .flex()
                .gap(px(TILE_GAP))
                .overflow_hidden()
                .children(tiles.iter().map(|t| tile(t, tile_w, cx).into_any_element())),
        )
        .child(focused_panel(panel, term, cell, head, grid, total == 0, cx))
        .child(tray_panel(tray, &inbox, &tiles, &parked, cx))
        .child(status_bar(working, ready, ended, total, parked.len()))
        .into_any_element()
}

fn top_bar(maximized: bool, cx: &mut Context<SpaceView>) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(TOP_H))
        .pl(px(GUTTER))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(chrome::logo(28.0))
        .child(
            div()
                .mr(px(4.))
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(TEXT))
                .child("Agentes"),
        )
        .child(tabs(View::Mando, cx))
        .child(chrome::drag(TOP_H))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mr(px(10.))
                .child(
                    pill("mando-next", "Siguiente sin mirar", Tone::Window)
                        .pr(px(8.))
                        .child(keycap("Alt+J", 0x2c2c2a))
                        .on_click(cx.listener(|v, _: &ClickEvent, window, cx| {
                            v.next_attention(window, cx)
                        })),
                )
                .child(pill("mando-claude", "+ Claude", Tone::Window).on_click(cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.open(Open::agent("claude", "Claude Code", "claude", None), cx)
                    },
                )))
                .child(pill("mando-codex", "+ Codex", Tone::Window).on_click(cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.open(Open::agent("codex", "Codex", "codex", None), cx)
                    },
                )))
                .child(
                    pill("mando-shell", "+ PowerShell", Tone::Window)
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.open(Open::shell(None), cx))),
                ),
        )
        .child(chrome::controls(maximized, TOP_H))
}

fn tile(data: &TileData, width: f32, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let id = data.id;
    let focused = data.focused;
    let state = match data.phase {
        Phase::Working => format!("trabajando · {}", data.since),
        Phase::Ready => format!("listo · {}", data.since),
        Phase::Ended => "terminó".to_string(),
    };
    div()
        .id(("mando-tile", id as usize))
        .w(px(width))
        .h(px(TILE_H))
        .flex_none()
        .px(px(14.))
        .py(px(12.))
        .flex()
        .flex_col()
        .justify_between()
        .rounded(px(R_TILE))
        .bg(hsla(if focused { SURFACE_ON } else { SURFACE }))
        .when(!focused, |el| el.cursor_pointer().hover(|el| el.bg(hsla(SURFACE_HOVER))))
        .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.select(id, window, cx)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(logo_tile(data.agent, 28.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hsla(TEXT))
                                .child(data.label.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(hsla(MUTED))
                                .child(data.sub.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(11.5))
                .text_color(hsla(MUTED))
                .child(dot(phase_dot(data.phase)))
                .child(state)
                .when(data.unseen, |el| el.child(div().flex_1()).child(badge("Para revisar"))),
        )
}

fn focused_panel(
    area: Area,
    term: Area,
    cell: Cell,
    head: Option<Head>,
    grid: Option<super::Grid>,
    empty: bool,
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
    let mut panel = div()
        .id("mando-panel")
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .rounded(px(R_PANEL))
        .bg(hsla(SURFACE))
        .on_scroll_wheel(cx.listener(|v, event: &ScrollWheelEvent, _, cx| {
            let lines = event.delta.pixel_delta(px(v.cell().h)).y;
            let lines = (f32::from(lines) / v.cell().h).round() as i32;
            if let Some(card) = v.focused.and_then(|id| v.card(id)) {
                if lines != 0 {
                    card.console.scroll(lines);
                }
            }
            cx.notify();
        }));
    if let Some(Head { id, label, sub, agent, phase, closing }) = head {
        panel = panel
            .child(
                // Sin fondo propio: el encabezado es del panel y no tiene
                // esquinas que sobresalgan de las redondas del panel.
                div()
                    .absolute()
                    .left(px(PANEL_PAD))
                    .top(px(PANEL_PAD))
                    .w(px(area.w - PANEL_PAD * 2.0))
                    .h(px(HEAD_H))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(logo_tile(agent, 26.0))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hsla(TEXT))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(12.))
                            .text_color(hsla(MUTED))
                            .child(sub),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .text_color(hsla(MUTED))
                            .child(dot(phase_dot(phase)))
                            .child(phase_text(phase)),
                    )
                    .when(!closing, |el| {
                        el.child(pill(("mando-close", id as usize), "Cerrar", Tone::Surface).on_click(
                            cx.listener(move |v, _: &ClickEvent, _, cx| {
                                v.mando.closing = Some(id);
                                cx.notify();
                            }),
                        ))
                    })
                    // Cerrar no mata al agente de entrada: pregunta. Dejarlo en
                    // segundo plano lo mantiene trabajando, sin carta.
                    .when(closing, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(hsla(MUTED))
                                .child("¿Qué hago con él?"),
                        )
                        .child(pill(("mando-bg", id as usize), "En segundo plano", Tone::Surface).on_click(
                            cx.listener(move |v, _: &ClickEvent, _, cx| v.detach(id, cx)),
                        ))
                        .child(pill(("mando-end", id as usize), "Terminar", Tone::Danger).on_click(
                            cx.listener(move |v, _: &ClickEvent, _, cx| v.terminate(id, cx)),
                        ))
                        .child(pill(("mando-keep", id as usize), "Cancelar", Tone::Surface).on_click(
                            cx.listener(move |v, _: &ClickEvent, _, cx| {
                                v.mando.closing = None;
                                cx.notify();
                            }),
                        ))
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(term.x))
                    .top(px(term.y))
                    .w(px(term.w))
                    .h(px(term.h))
                    .rounded(px(R_INSET))
                    .bg(hsla(TERMINAL))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, cx| {
                                if let Some(grid) = &grid {
                                    // La primera fila baja un poco: así tiene el
                                    // mismo aire arriba que a la izquierda.
                                    let content = Bounds::new(
                                        point(bounds.origin.x, bounds.origin.y + px(TERM_TOP)),
                                        size(bounds.size.width, bounds.size.height - px(TERM_TOP)),
                                    );
                                    paint_grid(grid, content, cell, 1.0, FONT_SIZE, true, window, cx);
                                }
                            },
                        )
                        .size_full(),
                    ),
            );
    }
    if empty {
        panel = panel.child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(hsla(MUTED))
                .child("Abre un agente con los botones de arriba, o desde Agentes en el notch."),
        );
    }
    panel
}

fn tray_panel(
    area: Area,
    inbox: &[TrayItem],
    tiles: &[TileData],
    parked: &[TileData],
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
    let running: Vec<&TileData> = tiles.iter().filter(|t| t.phase == Phase::Working).collect();
    div()
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .rounded(px(R_PANEL))
        .bg(hsla(SURFACE))
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hsla(TEXT))
                        .child("Bandeja de revisión"),
                )
                .when(!inbox.is_empty(), |el| el.child(badge(inbox.len().to_string()))),
        )
        .when(inbox.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(12.))
                    .text_color(hsla(MUTED))
                    .child("Nada que revisar. Cuando un agente termine un turno o se detenga y no lo estés mirando, aparece aquí."),
            )
        })
        .children(inbox.iter().map(|item| inbox_item(item, cx).into_any_element()))
        .when(!running.is_empty(), |el| {
            el.child(
                div()
                    .mt(px(6.))
                    .text_size(px(11.))
                    .text_color(hsla(MUTED))
                    .child("EN CURSO"),
            )
            .children(running.iter().map(|t| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(hsla(TEXT))
                    .child(dot(WORKING_DOT))
                    .child(div().flex_1().truncate().child(t.label.clone()))
                    .child(div().text_color(hsla(MUTED)).child(t.since.clone()))
                    .into_any_element()
            }))
        })
        .when(!parked.is_empty(), |el| {
            el.child(
                div()
                    .mt(px(6.))
                    .text_size(px(11.))
                    .text_color(hsla(MUTED))
                    .child("EN SEGUNDO PLANO"),
            )
            .children(parked.iter().map(|t| parked_row(t, cx).into_any_element()))
        })
}

/// Un agente que se dejó en segundo plano: sigue trabajando, sin carta.
fn parked_row(t: &TileData, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let id = t.id;
    let state = match t.phase {
        Phase::Working => format!("trabajando · {}", t.since),
        Phase::Ready => format!("listo · {}", t.since),
        Phase::Ended => "terminó".to_string(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(logo_tile(t.agent, 24.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(hsla(TEXT))
                        .child(t.label.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(11.))
                        .text_color(hsla(MUTED))
                        .child(dot(phase_dot(t.phase)))
                        .child(state),
                ),
        )
        .child(
            pill(("park-open", id as usize), "Abrir", Tone::Surface)
                .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.select(id, window, cx))),
        )
        .child(
            pill(("park-end", id as usize), "Terminar", Tone::Danger)
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.terminate(id, cx))),
        )
}

fn inbox_item(item: &TrayItem, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let id = item.id;
    let title = if item.ended { "terminó el proceso" } else { "terminó un turno" };
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(R_ITEM))
        .bg(hsla(ITEM))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(logo_tile(item.agent, 22.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(hsla(TEXT))
                        .child(format!("{} {title}", item.label)),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(hsla(MUTED))
                        .child(format!("hace {}", item.ago)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .font_family(FONT_FAMILY)
                .text_size(px(11.))
                .text_color(hsla(MUTED))
                .children(item.lines.iter().map(|line| {
                    div().truncate().child(line.trim().to_string()).into_any_element()
                })),
        )
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    pill(("inbox-view", id as usize), "Ver", Tone::Light)
                        .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.select(id, window, cx))),
                )
                .child(
                    pill(("inbox-ok", id as usize), "Aceptar", Tone::Item).on_click(cx.listener(
                        move |v, _: &ClickEvent, _, cx| {
                            if let Some(watch) = v.mando.watch.get_mut(&id) {
                                watch.unseen = None;
                            }
                            cx.notify();
                        },
                    )),
                ),
        )
}

fn status_bar(
    working: usize,
    ready: usize,
    ended: usize,
    total: usize,
    parked: usize,
) -> impl IntoElement {
    let count = |color: u32, text: String| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(dot(color))
            .child(text)
    };
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .h(px(STATUS_H))
        .px(px(GUTTER + 6.0))
        .flex()
        .items_center()
        .gap(px(18.))
        .text_size(px(11.5))
        .text_color(hsla(MUTED))
        .child(count(WORKING_DOT, format!("{working} trabajando")))
        .child(count(READY_DOT, plural(ready, "listo", "listos")))
        .when(ended > 0, |el| el.child(count(FAINT, plural(ended, "terminó", "terminaron"))))
        .child(div().flex_1())
        .when(parked > 0, |el| el.child(format!("{parked} en segundo plano")))
        .child(plural(total, "consola", "consolas"))
        .child("Alt+J siguiente")
        .child("Ctrl+1…9 saltar")
        .child("Ctrl+Shift+M pizarra")
}

// --- Demostración ------------------------------------------------------------------

/// `SPACE_MANDO_DEMO=1`: cuatro consolas que fingen ser agentes, una por cada
/// estado, para probar la vista sin gastar sesiones reales.
pub(super) fn demo(view: &mut SpaceView, cx: &mut Context<SpaceView>) {
    let fake = |label: &str, agent: &'static str, script: &str| Open {
        label: label.into(),
        program: super::powershell(),
        args: vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into(), script.into()],
        cwd: None,
        agent: Some(agent),
    };
    // Sigue trabajando.
    view.open(
        fake(
            "Claude Code",
            "claude",
            "while ($true) { 'Editando src/auth/session.rs ...'; Start-Sleep -Milliseconds 400 }",
        ),
        cx,
    );
    // Trabaja 9 s y se calla: llega a la bandeja.
    view.open(
        fake(
            "Codex",
            "codex",
            "1..22 | ForEach-Object { \"corriendo prueba $_ de 22\"; Start-Sleep -Milliseconds 400 }; 'Listo: las 22 pruebas pasan'; Start-Sleep 3600",
        ),
        cx,
    );
    // Su proceso termina a los 7 s.
    view.open(
        fake(
            "Cursor",
            "cursor-agent",
            "1..12 | ForEach-Object { \"escribiendo el handler $_\"; Start-Sleep -Milliseconds 550 }; 'fin del proceso'",
        ),
        cx,
    );
    // Quieto desde el principio.
    view.open(fake("OpenCode", "opencode", "'Esperando instrucciones...'; Start-Sleep 3600"), cx);
    // El primero queda enfocado: las otras tres corren sin que nadie las mire.
    view.focused = view.cards.first().map(|c| c.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Duration = Duration::from_secs(5);
    const BUSY: Duration = Duration::from_millis(200);

    #[test]
    fn el_estado_sale_del_silencio_de_la_salida() {
        assert_eq!(phase(false, BUSY), Phase::Working);
        assert_eq!(phase(false, QUIET), Phase::Ready);
        assert_eq!(phase(true, BUSY), Phase::Ended);
    }

    #[test]
    fn el_repintado_por_cambio_de_tamano_no_es_trabajo() {
        // Estaba listo, se le cambia el tamaño y repinta: sigue listo.
        assert_eq!(effective_phase(Phase::Ready, Phase::Working, true), Phase::Ready);
        // Pasado el repintado, si sigue escribiendo, sí trabaja.
        assert_eq!(effective_phase(Phase::Ready, Phase::Working, false), Phase::Working);
        // Lo que ya trabajaba o terminó no se toca.
        assert_eq!(effective_phase(Phase::Working, Phase::Working, true), Phase::Working);
        assert_eq!(effective_phase(Phase::Ready, Phase::Ended, true), Phase::Ended);
        assert_eq!(effective_phase(Phase::Working, Phase::Ready, true), Phase::Ready);
    }

    #[test]
    fn un_turno_largo_que_se_calla_sin_que_lo_miren_avisa() {
        let turn = Duration::from_secs(8);
        assert!(needs_attention(Phase::Working, Phase::Ready, turn, false));
    }

    #[test]
    fn un_repintado_corto_no_es_un_turno() {
        let turn = Duration::from_millis(900);
        assert!(!needs_attention(Phase::Working, Phase::Ready, turn, false));
    }

    #[test]
    fn lo_que_se_esta_mirando_no_avisa() {
        let turn = Duration::from_secs(20);
        assert!(!needs_attention(Phase::Working, Phase::Ready, turn, true));
        assert!(!needs_attention(Phase::Working, Phase::Ended, turn, true));
    }

    #[test]
    fn un_proceso_que_termina_siempre_avisa_si_nadie_lo_mira() {
        assert!(needs_attention(Phase::Working, Phase::Ended, Duration::ZERO, false));
        assert!(needs_attention(Phase::Ready, Phase::Ended, Duration::ZERO, false));
    }

    #[test]
    fn empezar_a_trabajar_o_no_cambiar_no_avisa() {
        let turn = Duration::from_secs(20);
        assert!(!needs_attention(Phase::Ready, Phase::Working, turn, false));
        assert!(!needs_attention(Phase::Ready, Phase::Ready, turn, false));
    }

    #[test]
    fn las_rutas_del_shell_no_sirven_de_titulo() {
        assert_eq!(useful_title(r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe"), None);
        assert_eq!(useful_title(r"C:\WINDOWS\system32\cmd.exe - claude"), None);
        assert_eq!(useful_title(r"PS C:\Users\Lenovo\Documents\atic>"), None);
        assert_eq!(useful_title("pwsh.exe"), None);
        assert_eq!(useful_title("   "), None);
        assert_eq!(useful_title("✳ Migrar el login"), Some("✳ Migrar el login"));
        assert_eq!(useful_title(" codex "), Some("codex"));
    }

    #[test]
    fn al_dejar_de_verse_una_consola_el_foco_pasa_a_la_siguiente_visible() {
        // 1 y 2 a la vista, 3 en segundo plano, 4 a la vista.
        let order = [(1, false), (2, false), (3, true), (4, false)];
        // Sigue la visible que viene después: la 3 está en segundo plano, se salta.
        assert_eq!(pick_visible(&order, 2), Some(4));
        assert_eq!(pick_visible(&order, 1), Some(2));
        // Si era la última, la anterior.
        assert_eq!(pick_visible(&order, 4), Some(2));
        // Una sola a la vista: al irse no queda a quién dar el foco.
        assert_eq!(pick_visible(&[(1, false), (2, true)], 1), None);
        assert_eq!(pick_visible(&[], 1), None);
    }

    #[test]
    fn el_plural_concuerda_con_la_cantidad() {
        assert_eq!(plural(0, "listo", "listos"), "0 listos");
        assert_eq!(plural(1, "listo", "listos"), "1 listo");
        assert_eq!(plural(1, "consola", "consolas"), "1 consola");
        assert_eq!(plural(4, "terminó", "terminaron"), "4 terminaron");
    }

    #[test]
    fn el_tiempo_se_dice_en_la_unidad_que_corresponde() {
        assert_eq!(elapsed_label(Duration::from_secs(40)), "40 s");
        assert_eq!(elapsed_label(Duration::from_secs(60)), "1 min");
        assert_eq!(elapsed_label(Duration::from_secs(185)), "3 min");
        assert_eq!(elapsed_label(Duration::from_secs(7300)), "2 h");
    }
}
