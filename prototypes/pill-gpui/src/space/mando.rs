//! La vista «Mando» del espacio: la forma de mirar muchos agentes a la vez
//! sin recorrer un plano.
//!
//! A la izquierda, las carpetas del espacio con los agentes que trabajan en
//! cada una. Al centro, los paneles (`panes.rs`): uno o varios, partidos hacia
//! el lado o hacia abajo, cada uno con una consola. A la derecha, el detalle
//! del agente enfocado (sus carpetas y los archivos que cambió) y la
//! **bandeja de revisión**: lo que terminó o se detuvo y nadie ha mirado. La
//! pizarra sigue ahí como la otra vista del espacio (`View::Pizarra`).
//!
//! Cada consola tiene un nombre y un color propios (`identity.rs`): el color
//! pinta su logo en la barra lateral, en su panel y en la bandeja.
//!
//! Los estados salen de la salida de cada PTY, como en `consoleStatus.ts` de
//! Atic: trabajando si escribió hace menos de 1,5 s; un turno que se calla
//! después de 3 s o más y que nadie miraba pasa a la bandeja.
//!
//! **Estilo**: colores planos y sin bordes. Entre una capa y la siguiente
//! solo cambia el color (ventana, terminal, panel, panel enfocado). GPUI no
//! recorta el contenido a las esquinas redondeadas, solo a un rectángulo: por
//! eso nada pinta contra la esquina de su contenedor. Todo va metido hacia
//! adentro y con su propio radio (el de afuera es el de adentro más el margen).
//!
//! Todo el estado propio vive en `State`; este módulo solo toca `SpaceView`
//! por sus campos y por los ganchos `tick_mando`, `render` y `key_down`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, point, prelude::*, px, size, svg, AnyElement, Bounds, ClickEvent, Context,
    CursorStyle, Div, ElementId, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ScrollWheelEvent, SharedString, Stateful, Window,
};

use super::changes::{self, Kind};
use super::chrome;
use super::console::{self, hsla, GridSize};
use super::folders;
use super::panes::{Axis, Dir, Panes};
use super::{paint_grid, Area, Cell, Open, SpaceView, FONT_FAMILY, FONT_SIZE, PAD, TOOLBAR_H, WORKING_FOR};

/// Cómo se mira el espacio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Carpetas, paneles y detalle.
    Mando,
    /// El plano infinito con las consolas sueltas.
    Pizarra,
}

/// La barra de arriba es la de la ventana: mide lo mismo en las dos vistas.
const TOP_H: f32 = TOOLBAR_H;
const STATUS_H: f32 = 30.0;
const GUTTER: f32 = 12.0;
const GAP: f32 = 10.0;
const SIDE_W: f32 = 250.0;
const DETAIL_W: f32 = 310.0;
/// Más angosto que esto, el detalle no cabe: se esconde.
const DETAIL_FROM: f32 = 1120.0;
/// Margen del panel de la consola y alto de su encabezado.
const PANEL_PAD: f32 = 8.0;
const HEAD_H: f32 = 40.0;
/// Aire sobre la primera fila de la terminal y bajo la última.
const TERM_TOP: f32 = 6.0;
const TERM_BOTTOM: f32 = 8.0;
/// Menos que esto no es un turno: un repintado o el eco de unas teclas.
const TURN: Duration = Duration::from_secs(3);
/// Lo que se muestra de la lista de archivos: más no se alcanza a leer.
const MAX_FILES: usize = 200;

// Radios: cada capa de adentro tiene menos que la de afuera.
const R_PANEL: f32 = 18.0;
const R_ITEM: f32 = 12.0;
const R_INSET: f32 = 11.0;

// Colores, de más oscuro a más claro. Sin bordes: lo único que separa dos
// cosas es que una es de otro color.
const WINDOW: u32 = 0x0f0f0e;
/// La terminal: el fondo con el que ya se dibujan sus celdas.
const TERMINAL: u32 = console::BACKGROUND;
const SURFACE: u32 = 0x1d1d1b;
const SURFACE_HOVER: u32 = 0x252523;
/// El panel enfocado (con más de uno) y lo seleccionado: más claro, sin marco.
const SURFACE_ON: u32 = 0x2b2b28;
/// Un elemento dentro de un panel (los de la bandeja).
const ITEM: u32 = 0x262624;
const TEXT: u32 = 0xf0f0ea;
const MUTED: u32 = 0x9a9a90;
const FAINT: u32 = 0x6a6a64;
const INK: u32 = 0x141413;
const WORKING_DOT: u32 = 0xe8b04b;
const READY_DOT: u32 = 0x6cc48a;
const DELETED: u32 = 0xf07b6e;

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

/// Se está arrastrando la raya entre dos paneles.
struct SplitDrag {
    path: Vec<bool>,
    axis: Axis,
    span: Area,
}

/// Lo que el Mando recuerda de cada consola entre cuadros.
#[derive(Default)]
pub struct State {
    watch: HashMap<u64, Watch>,
    /// La ventana está maximizada: el botón de la barra pasa a «restaurar».
    maximized: bool,
    /// La consola a la que se le pidió terminar y espera la confirmación.
    closing: Option<u64>,
    panes: Panes,
    split_drag: Option<SplitDrag>,
}

impl State {
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Pone al día los estados y levanta lo que pasó a la bandeja.
    fn refresh(&mut self, cards: &[super::Card], watched: &[u64], now: Instant) {
        self.watch.retain(|id, _| cards.iter().any(|c| c.id == *id));
        for card in cards {
            let raw = phase(card.console.exited(), card.console.quiet_for());
            let watched = watched.contains(&card.id);
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

// --- Medidas --------------------------------------------------------------------

struct Layout {
    side: Area,
    panes: Area,
    detail: Option<Area>,
}

impl Layout {
    fn new(vw: f32, vh: f32) -> Self {
        let y = TOP_H;
        let h = (vh - STATUS_H - y).max(160.0);
        let side = Area { x: GUTTER, y, w: SIDE_W, h };
        let detail = (vw >= DETAIL_FROM).then(|| Area { x: vw - GUTTER - DETAIL_W, y, w: DETAIL_W, h });
        let right = detail.map_or(vw - GUTTER, |d| d.x - GAP);
        let x = side.x + side.w + GAP;
        Self { side, panes: Area { x, y, w: (right - x).max(240.0), h }, detail }
    }
}

/// El rectángulo de la terminal dentro de un panel, en coordenadas del panel.
fn term_area(pane: Area) -> Area {
    Area {
        x: PANEL_PAD,
        y: PANEL_PAD + HEAD_H,
        w: pane.w - PANEL_PAD * 2.0,
        h: pane.h - HEAD_H - PANEL_PAD * 2.0,
    }
}

/// Cuántas columnas y filas le caben al PTY en un panel.
fn term_grid(pane: Area, cell: Cell) -> GridSize {
    let term = term_area(pane);
    GridSize {
        cols: (((term.w - PAD * 2.0) / cell.w).floor() as usize).max(20),
        rows: (((term.h - TERM_TOP - 2.0 - TERM_BOTTOM) / cell.h).floor() as usize).max(5),
    }
}

// --- Ganchos en SpaceView -------------------------------------------------------

impl SpaceView {
    /// Se llama en cada cuadro, con cualquier vista, para que la bandeja no
    /// se pierda lo que pasa mientras se mira la pizarra.
    pub(super) fn tick_mando(&mut self, window: &Window, now: Instant) {
        self.mando.maximized = window.is_maximized();
        let watched: Vec<u64> = if self.view == View::Mando {
            self.sync_panes();
            self.mando.panes.shown()
        } else {
            self.focused.into_iter().collect()
        };
        self.mando.refresh(&self.cards, &watched, now);
    }

    /// Los paneles y `focused` dicen lo mismo. Lo que se abre o se elige en
    /// otra parte (`open`, la pizarra) pone `focused`: aquí encuentra panel.
    fn sync_panes(&mut self) {
        let alive: Vec<u64> = self.cards.iter().map(|c| c.id).collect();
        let panes = &mut self.mando.panes;
        panes.forget(&alive);
        match self.focused.filter(|id| alive.contains(id)) {
            Some(id) => match panes.pane_of(id) {
                Some(pane) => panes.focused = pane,
                None => {
                    let pane = panes.place();
                    panes.put(pane, Some(id));
                    panes.focused = pane;
                }
            },
            None => {}
        }
        self.focused = panes.card_in(panes.focused);
    }

    /// Las consolas en el orden de la barra lateral: por carpeta, y al final
    /// las que no están en ninguna. Es el orden de Ctrl+1…9.
    fn side_groups(&self) -> (Vec<(usize, Vec<u64>)>, Vec<u64>) {
        let mut sorted: Vec<&super::Card> = self.cards.iter().collect();
        sorted.sort_by_key(|c| c.id);
        let list = self.folders.list();
        let folder_of = |card: &super::Card| {
            card.cwd.as_deref().and_then(|cwd| list.iter().position(|f| folders::same(f, cwd)))
        };
        let groups = (0..list.len())
            .map(|index| (index, sorted.iter().filter(|c| folder_of(c) == Some(index)).map(|c| c.id).collect()))
            .collect();
        let others = sorted.iter().filter(|c| folder_of(c).is_none()).map(|c| c.id).collect();
        (groups, others)
    }

    fn side_order(&self) -> Vec<u64> {
        let (groups, others) = self.side_groups();
        groups.into_iter().flat_map(|(_, ids)| ids).chain(others).collect()
    }

    /// Enfoca una consola: en el Mando la pone en un panel; en la pizarra la
    /// sube y la muestra.
    fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.card(id).is_none() {
            return;
        }
        self.mando.closing = None;
        match self.view {
            View::Mando => {
                self.focused = Some(id);
                self.sync_panes();
            }
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

    /// Pone una consola en un panel dado.
    fn show_in(&mut self, pane: usize, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.mando.panes.put(pane, Some(id));
        self.focus_pane(pane, window, cx);
    }

    fn focus_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.mando.panes.focused = pane;
        self.focused = self.mando.panes.card_in(pane);
        if let Some(watch) = self.focused.and_then(|id| self.mando.watch.get_mut(&id)) {
            watch.unseen = None;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// Ctrl+W: el panel se va y su consola sigue corriendo en la barra lateral.
    fn close_pane(&mut self, pane: usize, cx: &mut Context<Self>) {
        self.mando.closing = None;
        self.mando.panes.close(pane);
        self.focused = self.mando.panes.card_in(self.mando.panes.focused);
        cx.notify();
    }

    fn split(&mut self, axis: Axis, cx: &mut Context<Self>) {
        let pane = self.mando.panes.focused;
        self.mando.panes.split(pane, axis);
        self.focused = None;
        cx.notify();
    }

    fn move_focus(&mut self, dir: Dir, window: &mut Window, cx: &mut Context<Self>) {
        let area = Layout::new(self.viewport.0, self.viewport.1).panes;
        if let Some(pane) = self.mando.panes.neighbor(self.mando.panes.focused, dir, area) {
            self.focus_pane(pane, window, cx);
        }
    }

    /// Detiene al agente y su consola se va; su panel queda vacío.
    fn terminate(&mut self, id: u64, cx: &mut Context<Self>) {
        self.mando.closing = None;
        self.close(id, cx);
        self.focused = None;
        self.sync_panes();
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

    fn open_here(&mut self, cli: Option<&'static str>, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        let open = match cli {
            Some("claude") => Open::agent("claude", "Claude Code", "claude", cwd),
            Some("codex") => Open::agent("codex", "Codex", "codex", cwd),
            _ => Open::shell(cwd),
        };
        self.open(open, cx);
    }
}

/// Atajos del Mando y de la pizarra; lo demás sigue al `key_down` de siempre.
///
/// - `Alt+J`: la consola que más espera. (`Ctrl+J` es el salto de línea de
///   Claude Code, no se puede tocar.)
/// - `Ctrl+1…9`: la consola número N de la barra lateral.
/// - `Ctrl+Shift+M`: cambia entre Mando y pizarra.
/// - En el Mando: `Ctrl+W` quita el panel (la consola sigue corriendo),
///   `Ctrl+Shift+W` pregunta si terminarla, `Alt+Shift+=` / `Ctrl+\` divide
///   hacia el lado, `Alt+Shift+-` hacia abajo y `Alt+flechas` cambia de panel.
pub(super) fn key_down(
    view: &mut SpaceView,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<SpaceView>,
) {
    let ks = &event.keystroke;
    let m = &ks.modifiers;
    let key = ks.key.as_str();
    let only = |control: bool, alt: bool, shift: bool| m.control == control && m.alt == alt && m.shift == shift;
    let mut handled = true;
    // Esc cancela la pregunta de terminar. Solo mientras está ahí: el resto
    // del tiempo Esc es del agente.
    if view.mando.closing.is_some() && key == "escape" && only(false, false, false) {
        view.mando.closing = None;
        cx.notify();
    } else if only(false, true, false) && key == "j" {
        view.next_attention(window, cx);
    } else if only(true, false, true) && key == "m" {
        let other = match view.view {
            View::Mando => View::Pizarra,
            View::Pizarra => View::Mando,
        };
        view.set_view(other, cx);
    } else if let Some(n) = key.parse::<usize>().ok().filter(|n| only(true, false, false) && (1..=9).contains(n)) {
        let ids = match view.view {
            View::Mando => view.side_order(),
            View::Pizarra => {
                let mut ids: Vec<u64> = view.cards.iter().map(|c| c.id).collect();
                ids.sort();
                ids
            }
        };
        match ids.get(n - 1) {
            Some(&id) => view.select(id, window, cx),
            None => handled = false,
        }
    } else if view.view == View::Mando {
        match key {
            "w" if only(true, false, false) => view.close_pane(view.mando.panes.focused, cx),
            "w" if only(true, false, true) => {
                view.mando.closing = view.focused;
                cx.notify();
            }
            "=" | "+" if only(false, true, true) => view.split(Axis::Row, cx),
            "\\" if only(true, false, false) => view.split(Axis::Row, cx),
            "-" | "_" if only(false, true, true) => view.split(Axis::Column, cx),
            "left" if only(false, true, false) => view.move_focus(Dir::Left, window, cx),
            "right" if only(false, true, false) => view.move_focus(Dir::Right, window, cx),
            "up" if only(false, true, false) => view.move_focus(Dir::Up, window, cx),
            "down" if only(false, true, false) => view.move_focus(Dir::Down, window, cx),
            _ => handled = false,
        }
    } else {
        handled = false;
    }
    if handled {
        cx.stop_propagation();
        return;
    }
    view.key_down(event, window, cx);
}

// --- Piezas ---------------------------------------------------------------------

/// El logo de cada agente; sin agente, una terminal.
fn agent_icon(agent: Option<&str>) -> &'static str {
    match agent {
        Some("claude") => "icons/agents/claude.svg",
        Some("codex") => "icons/agents/openai.svg",
        Some("cursor-agent") => "icons/agents/cursor.svg",
        Some("opencode") => "icons/agents/opencode.svg",
        Some("agy") => "icons/agents/antigravity.svg",
        Some("grok") => "icons/agents/grok.svg",
        _ => "icons/square-terminal.svg",
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

/// El logo del agente sobre el color de la consola: así se reconoce cuál es.
fn logo_tile(agent: Option<&str>, color: u32, size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(size * 0.28))
        .bg(hsla(color))
        .flex()
        .items_center()
        .justify_center()
        .child(svg().path(agent_icon(agent)).size(px(size * 0.6)).flex_none().text_color(hsla(INK)))
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
        .gap(px(7.))
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(tone.ink()))
        .bg(hsla(fill))
        .hover(move |el| el.bg(hsla(hover)))
        .cursor_pointer()
        .child(label.into())
}

/// Un botón de solo icono, con su explicación al pasar el mouse.
fn icon_button(id: impl Into<ElementId>, icon: &'static str, tip: &'static str, hover: u32) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(26.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(move |el| el.bg(hsla(hover)))
        .tooltip(crate::hover::tip(tip))
        .child(svg().path(icon).size(px(14.)).text_color(hsla(MUTED)))
}

/// Un icono chico de la barra lateral: el logo del agente que abre, o la ×.
fn mini_button(id: impl Into<ElementId>, icon: &'static str, tip: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(22.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .cursor_pointer()
        .hover(|el| el.bg(hsla(SURFACE_ON)))
        .tooltip(crate::hover::tip(tip))
        .child(svg().path(icon).size(px(13.)).text_color(hsla(MUTED)))
}

fn section(text: &'static str) -> Div {
    div()
        .px(px(8.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(hsla(FAINT))
        .child(text)
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

#[derive(Clone)]
struct AgentData {
    id: u64,
    name: SharedString,
    label: SharedString,
    sub: SharedString,
    color: u32,
    agent: Option<&'static str>,
    phase: Phase,
    since: String,
    unseen: bool,
    /// Está en algún panel.
    shown: bool,
    focused: bool,
    changes: usize,
}

struct TrayItem {
    id: u64,
    name: SharedString,
    color: u32,
    agent: Option<&'static str>,
    ago: String,
    lines: Vec<String>,
    ended: bool,
}

struct PaneData {
    pane: usize,
    area: Area,
    focused: bool,
    card: Option<AgentData>,
    grid: Option<super::Grid>,
    closing: bool,
    cwd: Option<PathBuf>,
    dirs: Vec<PathBuf>,
}

struct Detail {
    data: AgentData,
    cwd: Option<PathBuf>,
    dirs: Vec<PathBuf>,
    files: Vec<changes::Change>,
    closing: bool,
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

pub(super) fn render(view: &mut SpaceView, _window: &mut Window, cx: &mut Context<SpaceView>) -> AnyElement {
    let (vw, vh) = view.viewport;
    let cell = view.cell();
    let layout = Layout::new(vw, vh);
    let now = Instant::now();
    let maximized = view.mando.maximized;
    view.sync_panes();

    // Cada consola a la vista ocupa justo la terminal de su panel. Mientras se
    // arrastra una raya no: cada cambio de tamaño hace repintar a la TUI.
    let rects = view.mando.panes.layout(layout.panes, GAP);
    if view.mando.split_drag.is_none() {
        for (pane, area) in &rects {
            let Some(id) = view.mando.panes.card_in(*pane) else {
                continue;
            };
            let grid = term_grid(*area, cell);
            let resized = view.card_mut(id).is_some_and(|card| {
                let before = card.console.size;
                card.console.resize(grid, (cell.w.round() as u16, cell.h as u16));
                card.console.size != before
            });
            if resized {
                view.mando.settle(id, now);
            }
        }
    }

    let shown = view.mando.panes.shown();
    let agents: HashMap<u64, AgentData> = view
        .cards
        .iter()
        .map(|card| {
            let watch = view.mando.watch.get(&card.id);
            let phase_now = watch
                .map(|w| w.phase)
                .unwrap_or_else(|| phase(card.console.exited(), card.console.quiet_for()));
            let since = watch.map(|w| elapsed_label(now.duration_since(w.since))).unwrap_or_default();
            let data = AgentData {
                id: card.id,
                name: card.name.into(),
                label: card.label.clone(),
                sub: card_sub(card).into(),
                color: card.color,
                agent: card.agent,
                phase: phase_now,
                since,
                unseen: watch.is_some_and(|w| w.unseen.is_some()),
                shown: shown.contains(&card.id),
                focused: view.focused == Some(card.id),
                changes: card.changes.list.len(),
            };
            (card.id, data)
        })
        .collect();

    let mut inbox: Vec<(Instant, TrayItem)> = view
        .cards
        .iter()
        .filter_map(|card| {
            let unseen = view.mando.watch.get(&card.id)?.unseen.as_ref()?;
            Some((
                unseen.at,
                TrayItem {
                    id: card.id,
                    name: card.name.into(),
                    color: card.color,
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

    let multi = view.mando.panes.count() > 1;
    let panes: Vec<PaneData> = rects
        .iter()
        .map(|(pane, area)| {
            let id = view.mando.panes.card_in(*pane);
            let card = id.and_then(|id| view.card(id));
            PaneData {
                pane: *pane,
                area: *area,
                focused: multi && view.mando.panes.focused == *pane,
                card: id.and_then(|id| agents.get(&id).cloned()),
                grid: card.map(|card| view.grid_rows(card)),
                closing: id.is_some() && view.mando.closing == id,
                cwd: card.and_then(|c| c.cwd.clone()),
                dirs: card.map(|c| c.dirs.clone()).unwrap_or_default(),
            }
        })
        .collect();
    for card in &view.cards {
        card.console.take_dirty();
    }
    let dividers = view.mando.panes.dividers(layout.panes, GAP);

    let detail = view.focused.and_then(|id| view.card(id)).and_then(|card| {
        Some(Detail {
            data: agents.get(&card.id)?.clone(),
            cwd: card.cwd.clone(),
            dirs: card.dirs.clone(),
            files: card.changes.list.iter().take(MAX_FILES).cloned().collect(),
            closing: view.mando.closing == Some(card.id),
        })
    });

    // Los estados cuentan a todas las consolas, también a las que no están en
    // un panel: siguen trabajando.
    let working = agents.values().filter(|t| t.phase == Phase::Working).count();
    let ready = agents.values().filter(|t| t.phase == Phase::Ready).count();
    let ended = agents.values().filter(|t| t.phase == Phase::Ended).count();
    let total = agents.len();
    let hidden: Vec<AgentData> = {
        let mut list: Vec<AgentData> = agents.values().filter(|a| !a.shown).cloned().collect();
        list.sort_by_key(|a| a.id);
        list
    };
    let (groups, others) = view.side_groups();
    let active_folder = view.folders.active().map(|p| folders::name(p));

    let mut root = div()
        .id("space")
        .key_context("Space")
        .track_focus(&view.focus)
        .on_key_down(cx.listener(key_down))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _: &MouseDownEvent, window, _| window.focus(&view.focus)),
        )
        .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
            let Some(drag) = &view.mando.split_drag else {
                return;
            };
            if event.pressed_button != Some(MouseButton::Left) {
                view.mando.split_drag = None;
                cx.notify();
                return;
            }
            let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
            let ratio = match drag.axis {
                Axis::Row => (x - drag.span.x) / drag.span.w,
                Axis::Column => (y - drag.span.y) / drag.span.h,
            };
            let path = drag.path.clone();
            view.mando.panes.set_ratio(&path, ratio);
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|view, _: &MouseUpEvent, _, cx| {
                if view.mando.split_drag.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .size_full()
        .relative()
        .bg(hsla(WINDOW))
        .font_family("Segoe UI")
        .child(super::input::layer(cx.weak_entity(), view.focus.clone()))
        .child(top_bar(maximized, active_folder, cx))
        .child(side_bar(layout.side, view, &groups, &others, &agents, cx))
        .children(panes.into_iter().map(|p| pane_panel(p, cell, &hidden, cx).into_any_element()))
        .children(dividers.into_iter().enumerate().map(|(index, d)| {
            let cursor = match d.axis {
                Axis::Row => CursorStyle::ResizeLeftRight,
                Axis::Column => CursorStyle::ResizeUpDown,
            };
            div()
                .id(("mando-divider", index))
                .absolute()
                .left(px(d.grip.x))
                .top(px(d.grip.y))
                .w(px(d.grip.w))
                .h(px(d.grip.h))
                .cursor(cursor)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        view.mando.split_drag = Some(SplitDrag { path: d.path.clone(), axis: d.axis, span: d.span });
                        cx.stop_propagation();
                    }),
                )
                .into_any_element()
        }))
        .child(status_bar(working, ready, ended, total));
    if let Some(area) = layout.detail {
        root = root.child(detail_panel(area, detail, &inbox, cx));
    }
    root.into_any_element()
}

fn top_bar(maximized: bool, folder: Option<String>, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let opens = |id: &'static str, label: &'static str, cli: Option<&'static str>, cx: &mut Context<SpaceView>| {
        pill(id, label, Tone::Window).on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.open_here(cli, None, cx)))
    };
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(TOP_H))
        .pl(px(GUTTER + 4.0))
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
                .when_some(folder, |el, folder| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .mr(px(4.))
                            .text_size(px(11.5))
                            .text_color(hsla(MUTED))
                            .child(svg().path("icons/folder.svg").size(px(12.)).text_color(hsla(MUTED)))
                            .child(format!("Nuevas en {folder}")),
                    )
                })
                .child(opens("mando-claude", "+ Claude", Some("claude"), cx))
                .child(opens("mando-codex", "+ Codex", Some("codex"), cx))
                .child(opens("mando-shell", "+ PowerShell", None, cx)),
        )
        .child(chrome::controls(maximized, TOP_H))
}

/// Las carpetas del espacio y, bajo cada una, los agentes que trabajan ahí.
fn side_bar(
    area: Area,
    view: &SpaceView,
    groups: &[(usize, Vec<u64>)],
    others: &[u64],
    agents: &HashMap<u64, AgentData>,
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
    let list = view.folders.list();
    let active = view.folders.active_index();
    let mut side = div()
        .id("mando-side")
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .flex()
        .flex_col()
        .gap(px(2.))
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .items_center()
                .pr(px(2.))
                .child(section("CARPETAS").flex_1())
                .child(
                    icon_button("side-add", "icons/folder-plus.svg", "Agregar carpetas", SURFACE_HOVER)
                        .mt(px(6.))
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.browse_folders(cx))),
                ),
        );
    if list.is_empty() {
        side = side.child(
            div()
                .px(px(8.))
                .text_size(px(12.))
                .text_color(hsla(MUTED))
                .child("Agrega las carpetas de tu proyecto. Cada agente nuevo trabaja en la carpeta elegida y puede leer las demás."),
        );
    }
    for (index, ids) in groups {
        let path = list[*index].clone();
        side = side.child(folder_row(*index, &path, *index == active, cx));
        for id in ids {
            if let Some(agent) = agents.get(id) {
                side = side.child(agent_row(agent, cx));
            }
        }
    }
    if !others.is_empty() {
        side = side.child(section(if list.is_empty() { "CONSOLAS" } else { "EN OTRAS CARPETAS" }));
        for id in others {
            if let Some(agent) = agents.get(id) {
                side = side.child(agent_row(agent, cx));
            }
        }
    }
    side
}

fn folder_row(index: usize, path: &Path, active: bool, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let group: SharedString = format!("folder-{index}").into();
    let open = |suffix: &'static str, cli: Option<&'static str>, tip: &'static str, cx: &mut Context<SpaceView>| {
        let path = path.to_path_buf();
        mini_button(SharedString::from(format!("folder-{index}-{suffix}")), agent_icon(cli), tip).on_click(cx.listener(
            move |v, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                v.folders.select(index);
                v.open_here(cli, Some(path.clone()), cx);
            },
        ))
    };
    div()
        .id(("folder-row", index))
        .group(group.clone())
        .mt(px(4.))
        .h(px(32.))
        .pl(px(8.))
        .pr(px(4.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .rounded(px(R_ITEM))
        .when(active, |el| el.bg(hsla(SURFACE)))
        .hover(|el| el.bg(hsla(SURFACE_HOVER)))
        .cursor_pointer()
        .tooltip(crate::hover::tip_text(format!("{}\nLas consolas nuevas se abren aquí", path.display()).into()))
        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
            v.folders.select(index);
            cx.notify();
        }))
        .child(svg().path("icons/folder.svg").size(px(14.)).flex_none().text_color(hsla(if active { TEXT } else { MUTED })))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(12.5))
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(hsla(if active { TEXT } else { MUTED }))
                .child(folders::name(path)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .child(open("claude", Some("claude"), "Claude aquí", cx))
                .child(open("codex", Some("codex"), "Codex aquí", cx))
                .child(open("shell", None, "PowerShell aquí", cx))
                .child(
                    mini_button(("folder-remove", index), "icons/x.svg", "Quitar del espacio")
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            v.folders.remove(index);
                            cx.notify();
                        })),
                ),
        )
}

fn agent_row(agent: &AgentData, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let id = agent.id;
    let state = match agent.phase {
        Phase::Ended => "terminó".to_string(),
        phase => format!("{} · {}", phase_text(phase), agent.since),
    };
    div()
        .id(("agent-row", id as usize))
        .ml(px(12.))
        .h(px(44.))
        .pl(px(8.))
        .pr(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .rounded(px(R_ITEM))
        .when(agent.focused, |el| el.bg(hsla(SURFACE_ON)))
        .when(!agent.focused && agent.shown, |el| el.bg(hsla(SURFACE)))
        .when(!agent.focused, |el| el.hover(|el| el.bg(hsla(SURFACE_HOVER))))
        .cursor_pointer()
        .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.select(id, window, cx)))
        .child(logo_tile(agent.agent, agent.color, 24.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.5))
                        .child(
                            div()
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hsla(if agent.shown { TEXT } else { MUTED }))
                                .child(agent.name.clone()),
                        )
                        .child(div().truncate().text_size(px(11.)).text_color(hsla(FAINT)).child(agent.label.clone())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(11.))
                        .text_color(hsla(MUTED))
                        .child(dot(phase_dot(agent.phase)))
                        .child(div().truncate().child(state))
                        .when(agent.changes > 0, |el| {
                            el.child(div().flex_none().text_color(hsla(FAINT)).child(format!("· {}", plural(agent.changes, "archivo", "archivos"))))
                        }),
                ),
        )
        .when(agent.unseen, |el| el.child(dot(READY_DOT)))
}

fn pane_panel(p: PaneData, cell: Cell, hidden: &[AgentData], cx: &mut Context<SpaceView>) -> impl IntoElement {
    let pane = p.pane;
    let area = p.area;
    let mut panel = div()
        .id(("mando-pane", pane))
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .rounded(px(R_PANEL))
        .bg(hsla(if p.focused { SURFACE_ON } else { SURFACE }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |v, _: &MouseDownEvent, window, cx| {
                if v.mando.panes.focused != pane {
                    v.focus_pane(pane, window, cx);
                }
            }),
        )
        .on_scroll_wheel(cx.listener(move |v, event: &ScrollWheelEvent, _, cx| {
            let lines = event.delta.pixel_delta(px(v.cell().h)).y;
            let lines = (f32::from(lines) / v.cell().h).round() as i32;
            if let Some(card) = v.mando.panes.card_in(pane).and_then(|id| v.card(id)) {
                if lines != 0 {
                    card.console.scroll(lines);
                }
            }
            cx.notify();
        }));
    let Some(agent) = p.card else {
        return panel.child(empty_pane(pane, hidden, cx));
    };
    let id = agent.id;
    let term = term_area(area);
    let wide = area.w >= 600.0;
    let grid = p.grid;
    let dirs = p.dirs;
    let cwd = p.cwd;
    let head = div()
        .absolute()
        .left(px(PANEL_PAD))
        .top(px(PANEL_PAD))
        .w(px(area.w - PANEL_PAD * 2.0))
        .h(px(HEAD_H))
        .pl(px(6.))
        .flex()
        .items_center()
        .gap(px(9.))
        .child(logo_tile(agent.agent, agent.color, 24.0))
        .child(
            div()
                .flex_none()
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(agent.color))
                .child(agent.name.clone()),
        )
        .when(wide, |el| el.child(div().flex_none().text_size(px(12.)).text_color(hsla(MUTED)).child(agent.label.clone())))
        .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(12.)).text_color(hsla(FAINT)).child(agent.sub.clone()));
    let head = if p.closing {
        head.child(div().flex_none().text_size(px(12.)).text_color(hsla(MUTED)).child(format!("¿Terminar a {}?", agent.name)))
            .child(
                pill(("pane-end", id as usize), "Terminar", Tone::Danger)
                    .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.terminate(id, cx))),
            )
            .child(pill(("pane-keep", id as usize), "Cancelar", Tone::Surface).on_click(cx.listener(
                |v, _: &ClickEvent, _, cx| {
                    v.mando.closing = None;
                    cx.notify();
                },
            )))
    } else {
        head.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .mr(px(4.))
                .text_size(px(12.))
                .text_color(hsla(MUTED))
                .child(dot(phase_dot(agent.phase)))
                .when(wide, |el| el.child(phase_text(agent.phase))),
        )
        .child(
            icon_button(("pane-code", pane), "icons/code.svg", "Abrir sus carpetas en VS Code", SURFACE_HOVER)
                .on_click(move |_, _, _| changes::open_in_code(&dirs, false)),
        )
        .when_some(cwd, |el, cwd| {
            el.child(
                icon_button(("pane-folder", pane), "icons/folder-open.svg", "Abrir la carpeta", SURFACE_HOVER)
                    .on_click(move |_, _, _| changes::open_folder(&cwd)),
            )
        })
        .child(
            icon_button(("pane-split-r", pane), "icons/columns-2.svg", "Dividir hacia el lado (Alt+Shift+=)", SURFACE_HOVER)
                .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| {
                    v.focus_pane(pane, window, cx);
                    v.split(Axis::Row, cx);
                })),
        )
        .child(
            icon_button(("pane-split-d", pane), "icons/rows-2.svg", "Dividir hacia abajo (Alt+Shift+-)", SURFACE_HOVER)
                .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| {
                    v.focus_pane(pane, window, cx);
                    v.split(Axis::Column, cx);
                })),
        )
        .child(
            icon_button(("pane-close", pane), "icons/x.svg", "Quitar del panel; sigue corriendo (Ctrl+W)", SURFACE_HOVER)
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.close_pane(pane, cx))),
        )
    };
    panel = panel.child(head).child(
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
                            // La primera fila baja un poco: así tiene el mismo
                            // aire arriba que a la izquierda.
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
    panel
}

/// Un panel sin consola: elegir una de las que corren sin panel o abrir otra.
fn empty_pane(pane: usize, hidden: &[AgentData], cx: &mut Context<SpaceView>) -> impl IntoElement {
    let opens = |suffix: &'static str, label: &'static str, cli: Option<&'static str>, cx: &mut Context<SpaceView>| {
        pill(SharedString::from(format!("empty-{pane}-{suffix}")), label, Tone::Surface).on_click(cx.listener(
            move |v, _: &ClickEvent, window, cx| {
                v.focus_pane(pane, window, cx);
                v.open_here(cli, None, cx);
            },
        ))
    };
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .p(px(20.))
        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(TEXT)).child("Panel vacío"))
        .when(!hidden.is_empty(), |el| {
            el.child(div().text_size(px(12.)).text_color(hsla(MUTED)).child("Muestra aquí una consola que sigue corriendo:"))
                .child(
                    div().w(px(280.)).flex().flex_col().gap(px(4.)).children(hidden.iter().take(8).map(|agent| {
                        let id = agent.id;
                        div()
                            .id(SharedString::from(format!("empty-{pane}-pick-{id}")))
                            .h(px(36.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .rounded(px(R_ITEM))
                            .bg(hsla(ITEM))
                            .hover(|el| el.bg(hsla(SURFACE_HOVER)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.show_in(pane, id, window, cx)))
                            .child(logo_tile(agent.agent, agent.color, 22.0))
                            .child(div().flex_none().text_size(px(12.5)).text_color(hsla(TEXT)).child(agent.name.clone()))
                            .child(div().flex_1().truncate().text_size(px(11.)).text_color(hsla(FAINT)).child(agent.label.clone()))
                            .child(dot(phase_dot(agent.phase)))
                            .into_any_element()
                    })),
                )
        })
        .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child("O abre una nueva:"))
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(opens("claude", "+ Claude", Some("claude"), cx))
                .child(opens("codex", "+ Codex", Some("codex"), cx))
                .child(opens("shell", "+ PowerShell", None, cx)),
        )
}

/// El agente enfocado (sus carpetas y lo que cambió) y la bandeja de revisión.
fn detail_panel(area: Area, detail: Option<Detail>, inbox: &[TrayItem], cx: &mut Context<SpaceView>) -> impl IntoElement {
    let mut panel = div()
        .id("mando-detail")
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .rounded(px(R_PANEL))
        .bg(hsla(SURFACE))
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .overflow_y_scroll();
    if let Some(Detail { data, cwd, dirs, files, closing }) = detail {
        let id = data.id;
        let code_dirs = dirs.clone();
        panel = panel
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(logo_tile(data.agent, data.color, 30.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hsla(data.color))
                                    .child(data.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(11.5))
                                    .text_color(hsla(MUTED))
                                    .child(data.label.clone())
                                    .child(dot(phase_dot(data.phase)))
                                    .child(phase_text(data.phase)),
                            ),
                    ),
            )
            .children(dirs.iter().enumerate().map(|(index, dir)| {
                div()
                    .id(("detail-dir", index))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(12.))
                    .text_color(hsla(if index == 0 { TEXT } else { MUTED }))
                    .tooltip(crate::hover::tip_text(dir.display().to_string().into()))
                    .child(svg().path("icons/folder.svg").size(px(13.)).flex_none().text_color(hsla(MUTED)))
                    .child(div().truncate().child(folders::name(dir)))
                    .when(index > 0, |el| el.child(div().flex_none().text_size(px(11.)).text_color(hsla(FAINT)).child("también")))
                    .into_any_element()
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(2.))
                    .when(!code_dirs.is_empty(), |el| {
                        el.child(
                            pill("detail-code", "Ver código", Tone::Surface)
                                .child(svg().path("icons/code.svg").size(px(13.)).text_color(hsla(MUTED)))
                                .on_click(move |_, _, _| changes::open_in_code(&code_dirs, false)),
                        )
                    })
                    .when_some(cwd, |el, cwd| {
                        el.child(
                            pill("detail-folder", "Carpeta", Tone::Surface)
                                .child(svg().path("icons/folder-open.svg").size(px(13.)).text_color(hsla(MUTED)))
                                .on_click(move |_, _, _| changes::open_folder(&cwd)),
                        )
                    })
                    .when(!closing, |el| {
                        el.child(pill("detail-end", "Terminar", Tone::Danger).on_click(cx.listener(
                            move |v, _: &ClickEvent, _, cx| {
                                v.mando.closing = Some(id);
                                cx.notify();
                            },
                        )))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(section("ARCHIVOS MODIFICADOS").px(px(0.)).flex_1())
                    .when(!files.is_empty(), |el| el.child(div().mt(px(6.)).child(badge(files.len().to_string())))),
            )
            .when(files.is_empty(), |el| {
                el.child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(if dirs.is_empty() {
                    "Sin carpeta."
                } else {
                    "Nada todavía. Aquí aparece lo que cambie en sus carpetas desde que empezó."
                }))
            })
            .children(files.into_iter().enumerate().map(|(index, file)| file_row(index, file).into_any_element()));
    }
    panel = panel.child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(section("PARA REVISAR").px(px(0.)).flex_1())
            .when(!inbox.is_empty(), |el| el.child(div().mt(px(6.)).child(badge(inbox.len().to_string())))),
    );
    if inbox.is_empty() {
        panel = panel.child(
            div()
                .text_size(px(12.))
                .text_color(hsla(MUTED))
                .child("Nada que revisar. Cuando un agente termine un turno o se detenga y no lo estés mirando, aparece aquí."),
        );
    }
    panel.children(inbox.iter().map(|item| inbox_item(item, cx).into_any_element()))
}

fn file_row(index: usize, file: changes::Change) -> impl IntoElement {
    let (letter, color) = match file.kind {
        Kind::Added => ("A", READY_DOT),
        Kind::Deleted => ("D", DELETED),
        Kind::Modified => ("M", WORKING_DOT),
    };
    let (dir, name) = match file.shown.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), file.shown.clone()),
    };
    let path = file.path.clone();
    let deleted = file.kind == Kind::Deleted;
    div()
        .id(("detail-file", index))
        .h(px(26.))
        .px(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .text_size(px(12.))
        .tooltip(crate::hover::tip_text(file.path.display().to_string().into()))
        .when(!deleted, |el| {
            el.cursor_pointer()
                .hover(|el| el.bg(hsla(SURFACE_HOVER)))
                .on_click(move |_, _, _| changes::open_in_code(std::slice::from_ref(&path), true))
        })
        .child(
            div()
                .w(px(12.))
                .flex_none()
                .font_family(FONT_FAMILY)
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(hsla(color))
                .child(letter),
        )
        .child(div().flex_none().text_color(hsla(if deleted { MUTED } else { TEXT })).child(name))
        .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(11.)).text_color(hsla(FAINT)).child(dir))
}

fn inbox_item(item: &TrayItem, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let id = item.id;
    let title = if item.ended { "terminó el proceso" } else { "terminó un turno" };
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(R_ITEM))
        .bg(hsla(ITEM))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(logo_tile(item.agent, item.color, 22.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(hsla(TEXT))
                        .child(format!("{} {title}", item.name)),
                )
                .child(div().text_size(px(11.)).text_color(hsla(MUTED)).child(format!("hace {}", item.ago))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .font_family(FONT_FAMILY)
                .text_size(px(11.))
                .text_color(hsla(MUTED))
                .children(item.lines.iter().map(|line| div().truncate().child(line.trim().to_string()).into_any_element())),
        )
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    pill(("inbox-view", id as usize), "Ver", Tone::Light)
                        .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.select(id, window, cx))),
                )
                .child(pill(("inbox-ok", id as usize), "Aceptar", Tone::Item).on_click(cx.listener(
                    move |v, _: &ClickEvent, _, cx| {
                        if let Some(watch) = v.mando.watch.get_mut(&id) {
                            watch.unseen = None;
                        }
                        cx.notify();
                    },
                ))),
        )
}

fn status_bar(working: usize, ready: usize, ended: usize, total: usize) -> impl IntoElement {
    let count = |color: u32, text: String| div().flex().items_center().gap(px(6.)).child(dot(color)).child(text);
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .h(px(STATUS_H))
        .px(px(GUTTER + 8.0))
        .flex()
        .items_center()
        .gap(px(18.))
        .text_size(px(11.5))
        .text_color(hsla(MUTED))
        .child(count(WORKING_DOT, format!("{working} trabajando")))
        .child(count(READY_DOT, plural(ready, "listo", "listos")))
        .when(ended > 0, |el| el.child(count(FAINT, plural(ended, "terminó", "terminaron"))))
        .child(div().flex_1())
        .child(plural(total, "consola", "consolas"))
        .child("Ctrl+W quitar")
        .child("Alt+Shift+= / - dividir")
        .child("Alt+flechas mover")
        .child("Alt+J siguiente")
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
