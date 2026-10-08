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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, point, prelude::*, px, size, svg, AnyElement, Bounds, ClickEvent, Context,
    CursorStyle, Div, ElementId, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ScrollWheelEvent, SharedString, Stateful, Window,
};

use super::changes::{self, Kind};
use super::chrome;
use super::console::{self, hsla, GridSize};
use super::explorer::Explorer;
use super::picker::Purpose;
use super::viewer::{Doc, LineKind};
use super::workspaces::{self, short_name};
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
    /// El menú de «nuevo agente», abierto donde se hizo clic.
    menu: Option<Menu>,
    /// La pestaña del panel de la derecha.
    tab: Tab,
    explorer: Explorer,
    /// El último agente enfocado: el panel de la derecha lo sigue mostrando
    /// mientras se mira un archivo.
    agent: Option<u64>,
}

/// Lo que muestra el panel de la derecha.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Changes,
    Files,
    Review,
}

/// El menú para abrir un agente: dónde se dibuja y en qué espacio abre.
struct Menu {
    x: f32,
    y: f32,
    workspace: Option<u64>,
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
        let alive: Vec<u64> = self.cards.iter().map(|c| c.id).chain(self.docs.iter().map(|d| d.id)).collect();
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
        // `focused` es siempre una consola: un panel con un archivo no recibe
        // lo que se escribe.
        let content = panes.card_in(panes.focused);
        self.focused = content.filter(|id| self.cards.iter().any(|c| c.id == *id));
        if self.focused.is_some() {
            self.mando.agent = self.focused;
        }
        if self.mando.agent.is_some_and(|id| self.card(id).is_none()) {
            self.mando.agent = None;
        }
        // Un archivo vive mientras está en un panel.
        let shown = self.mando.panes.shown();
        self.docs.retain(|doc| shown.contains(&doc.id));
    }

    /// Abre un archivo en un panel: el que ya muestra un archivo, uno vacío o
    /// uno nuevo al lado del enfocado. El foco se queda donde estaba.
    fn open_doc(&mut self, path: &Path, show_diff: bool, cx: &mut Context<Self>) {
        if let Some(doc) = self.docs.iter_mut().find(|d| workspaces::same(&d.path, path)) {
            doc.reload();
            doc.show_diff = show_diff && doc.diff.is_some();
            cx.notify();
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let doc = Doc::load(id, path, show_diff);
        let is_doc = |content: Option<u64>| content.is_some_and(|id| self.docs.iter().any(|d| d.id == id));
        let slots = self.mando.panes.slots();
        let target = slots
            .iter()
            .find(|(_, content)| is_doc(*content))
            .or_else(|| slots.iter().find(|(_, content)| content.is_none()))
            .map(|(pane, _)| *pane);
        let panes = &mut self.mando.panes;
        let pane = match target {
            Some(pane) => pane,
            None => {
                let back = panes.focused;
                let pane = panes.split(back, Axis::Row);
                panes.focused = back;
                pane
            }
        };
        panes.put(pane, Some(id));
        self.docs.push(doc);
        self.sync_panes();
        cx.notify();
    }

    /// Las consolas en el orden de la barra lateral: por espacio, y al final
    /// las que no están en ninguno. Es el orden de Ctrl+1…9.
    fn side_groups(&self) -> (Vec<(u64, Vec<u64>)>, Vec<u64>) {
        let mut sorted: Vec<&super::Card> = self.cards.iter().collect();
        sorted.sort_by_key(|c| c.id);
        let known = |card: &super::Card| card.workspace.filter(|id| self.spaces.get(*id).is_some());
        let groups = self
            .spaces
            .list()
            .iter()
            .map(|space| (space.id, sorted.iter().filter(|c| known(c) == Some(space.id)).map(|c| c.id).collect()))
            .collect();
        let others = sorted.iter().filter(|c| known(c).is_none()).map(|c| c.id).collect();
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

    /// Abre un agente (o PowerShell, sin `cli`) en un espacio; sin espacio, en
    /// el activo.
    fn open_here(&mut self, cli: Option<&'static str>, workspace: Option<u64>, cx: &mut Context<Self>) {
        self.mando.menu = None;
        if let Some(id) = workspace {
            self.spaces.select(id);
        }
        self.opening_in = workspace.or_else(|| self.spaces.active_id());
        let open = match cli.and_then(|cli| crate::agents::AGENTS.iter().find(|a| a.cli == cli)) {
            Some(agent) => Open::agent(agent.cli, agent.name, agent.cli, None),
            None => Open::shell(None),
        };
        self.open(open, cx);
    }

    /// Ctrl+W: termina la consola del panel (o cierra el archivo) y, si hay
    /// otros paneles, quita este.
    fn kill_pane(&mut self, pane: usize, cx: &mut Context<Self>) {
        if let Some(id) = self.mando.panes.card_in(pane).filter(|id| self.card(*id).is_some()) {
            self.mando.closing = None;
            self.close(id, cx);
        }
        self.mando.panes.put(pane, None);
        self.mando.panes.close(pane);
        self.focused = self.mando.panes.card_in(self.mando.panes.focused);
        cx.notify();
    }

    /// Los agentes que se pueden abrir: los instalados y PowerShell.
    fn launchers(&self) -> Vec<(Option<&'static str>, &'static str)> {
        self.available
            .iter()
            .map(|&index| {
                let agent = &crate::agents::AGENTS[index];
                (Some(agent.cli), agent.name)
            })
            .chain(std::iter::once((None, "PowerShell")))
            .collect()
    }
}

/// Atajos del Mando y de la pizarra; lo demás sigue al `key_down` de siempre.
///
/// - `Alt+J`: la consola que más espera. (`Ctrl+J` es el salto de línea de
///   Claude Code, no se puede tocar.)
/// - `Ctrl+1…9`: la consola número N de la barra lateral.
/// - `Ctrl+Shift+M`: cambia entre Mando y pizarra.
/// - En el Mando: `Ctrl+W` termina la consola del panel (y quita el panel si
///   hay otros), `Alt+Shift+=` / `Ctrl+\` divide hacia el lado,
///   `Alt+Shift+-` hacia abajo y `Alt+flechas` cambia de panel.
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
    // El selector de carpetas se queda con el teclado mientras está abierto.
    if let Some(picker) = &mut view.picker {
        match picker.key(key, m.control) {
            super::picker::KeyResult::Close => view.picker = None,
            super::picker::KeyResult::Confirm => view.confirm_picker(cx),
            _ => {}
        }
        cx.notify();
        cx.stop_propagation();
        return;
    }
    // Esc cancela la pregunta de terminar o el menú. Solo mientras están ahí:
    // el resto del tiempo Esc es del agente.
    if (view.mando.closing.is_some() || view.mando.menu.is_some()) && key == "escape" && only(false, false, false) {
        view.mando.closing = None;
        view.mando.menu = None;
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
            "w" if only(true, false, false) => view.kill_pane(view.mando.panes.focused, cx),
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

/// Sobre qué está un botón: su color es el único borde que tiene.
#[derive(Clone, Copy)]
enum Tone {
    /// El botón principal: claro sobre lo oscuro.
    Light,

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
    doc: Option<DocView>,
}

/// Lo que se dibuja de un archivo abierto en un panel.
struct DocView {
    name: String,
    dir: String,
    lines: Arc<Vec<String>>,
    diff: Option<Arc<Vec<super::viewer::DiffLine>>>,
    show_diff: bool,
    note: Option<String>,
}

struct Detail {
    agent: Option<AgentData>,
    dirs: Vec<PathBuf>,
    files: Vec<changes::Change>,
    closing: bool,
    tab: Tab,
    tree: Vec<super::explorer::Row>,
    changed: HashSet<PathBuf>,
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
                doc: id.and_then(|id| view.docs.iter().find(|d| d.id == id)).map(|doc| {
                    let shown = doc.path.display().to_string();
                    let (dir, name) = shown.rsplit_once(['\\', '/']).unwrap_or(("", &shown));
                    DocView {
                        name: name.to_string(),
                        dir: dir.to_string(),
                        lines: doc.lines.clone(),
                        diff: doc.diff.clone(),
                        show_diff: doc.show_diff,
                        note: doc.note.clone(),
                    }
                }),
            }
        })
        .collect();
    for card in &view.cards {
        card.console.take_dirty();
    }
    let dividers = view.mando.panes.dividers(layout.panes, GAP);

    // El panel de la derecha sigue al último agente enfocado; sin agente, el
    // árbol muestra las carpetas del espacio activo.
    let agent_card = view.mando.agent.and_then(|id| view.card(id));
    let dirs: Vec<PathBuf> = agent_card
        .map(|c| c.dirs.clone())
        .filter(|d| !d.is_empty())
        .or_else(|| view.spaces.active().map(|s| s.folders.clone()))
        .unwrap_or_default();
    let files: Vec<changes::Change> =
        agent_card.map(|c| c.changes.list.iter().take(MAX_FILES).cloned().collect()).unwrap_or_default();
    let changed: HashSet<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let agent = agent_card.and_then(|c| agents.get(&c.id).cloned());
    let closing = agent.as_ref().is_some_and(|a| view.mando.closing == Some(a.id));
    let tab = view.mando.tab;
    let tree = if tab == Tab::Files && layout.detail.is_some() { view.mando.explorer.rows(&dirs) } else { Vec::new() };
    let detail = Detail { agent, dirs, files, closing, tab, tree, changed };

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
    let launchers = view.launchers();
    let active_space = view.spaces.active().map(|s| s.name.clone());

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
        .child(top_bar(maximized, active_space, cx))
        .child(side_bar(layout.side, view, &groups, &others, &agents, cx))
        .children(panes.into_iter().map(|p| pane_panel(p, cell, &hidden, &launchers, cx).into_any_element()))
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
    if let Some(menu) = &view.mando.menu {
        root = root.child(agent_menu(menu, &view.launchers(), (vw, vh), cx));
    }
    if let Some(picker) = super::picker::render(view, vw, vh, cx) {
        root = root.child(picker);
    }
    root.into_any_element()
}

fn top_bar(maximized: bool, space: Option<String>, cx: &mut Context<SpaceView>) -> impl IntoElement {
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
                .when_some(space, |el, space| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .mr(px(4.))
                            .text_size(px(11.5))
                            .text_color(hsla(MUTED))
                            .child(svg().path("icons/layers.svg").size(px(12.)).text_color(hsla(MUTED)))
                            .child(format!("Nuevos en {space}")),
                    )
                })
                .child(
                    pill("mando-new", "Nuevo agente", Tone::Light)
                        .child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(hsla(INK)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|v, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                                v.mando.menu = Some(Menu { x: x - 120.0, y: y + 16.0, workspace: None });
                                cx.notify();
                            }),
                        ),
                ),
        )
        .child(chrome::controls(maximized, TOP_H))
}

/// El menú con los agentes instalados, para abrir uno en un espacio.
fn agent_menu(
    menu: &Menu,
    launchers: &[(Option<&'static str>, &'static str)],
    (vw, vh): (f32, f32),
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
    const W: f32 = 220.0;
    let h = launchers.len() as f32 * 34.0 + 12.0;
    let x = menu.x.clamp(8.0, vw - W - 8.0);
    let y = menu.y.min(vh - h - 8.0).max(8.0);
    let workspace = menu.workspace;
    div()
        .id("menu-veil")
        .absolute()
        .inset_0()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|v, _: &MouseDownEvent, _, cx| {
                v.mando.menu = None;
                cx.notify();
            }),
        )
        .child(
            div()
                .id("agent-menu")
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(W))
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .rounded(px(14.))
                .bg(hsla(0x252523))
                .shadow_lg()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(launchers.iter().enumerate().map(|(index, &(cli, name))| {
                    div()
                        .id(("menu-item", index))
                        .h(px(32.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .rounded(px(9.))
                        .text_size(px(12.5))
                        .text_color(hsla(TEXT))
                        .hover(|el| el.bg(hsla(0x323230)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.open_here(cli, workspace, cx)))
                        .child(svg().path(agent_icon(cli)).size(px(15.)).flex_none().text_color(hsla(MUTED)))
                        .child(name)
                        .into_any_element()
                })),
        )
}

/// Los espacios y, bajo cada uno, sus carpetas y los agentes que trabajan ahí.
fn side_bar(
    area: Area,
    view: &SpaceView,
    groups: &[(u64, Vec<u64>)],
    others: &[u64],
    agents: &HashMap<u64, AgentData>,
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
    let active = view.spaces.active_id();
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
                .child(section("ESPACIOS").flex_1())
                .child(
                    icon_button("side-add", "icons/plus.svg", "Nuevo espacio", SURFACE_HOVER)
                        .mt(px(6.))
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.open_picker(Purpose::NewWorkspace, cx))),
                ),
        );
    if view.spaces.list().is_empty() {
        side = side.child(
            div()
                .px(px(8.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .text_size(px(12.))
                .text_color(hsla(MUTED))
                .child("Un espacio junta las carpetas de un proyecto (por ejemplo el frontend y el backend). Cada agente que abras ahí trabaja en todas.")
                .child(
                    pill("side-first", "Crear un espacio", Tone::Surface)
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.open_picker(Purpose::NewWorkspace, cx))),
                ),
        );
    }
    for (id, ids) in groups {
        let Some(space) = view.spaces.get(*id) else {
            continue;
        };
        side = side.child(space_row(space, active == Some(*id), ids.len(), cx));
        if !space.collapsed {
            for (index, folder) in space.folders.iter().enumerate() {
                side = side.child(folder_line(*id, index, folder, cx));
            }
        }
        for agent in ids.iter().filter_map(|id| agents.get(id)) {
            side = side.child(agent_row(agent, cx));
        }
    }
    if !others.is_empty() {
        side = side.child(section("SIN ESPACIO"));
        for agent in others.iter().filter_map(|id| agents.get(id)) {
            side = side.child(agent_row(agent, cx));
        }
    }
    side
}

/// Un espacio: su nombre y, al pasar el mouse, abrir un agente, agregar
/// carpetas o quitarlo.
fn space_row(space: &workspaces::Workspace, active: bool, agents: usize, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let id = space.id;
    let group: SharedString = format!("space-{id}").into();
    let chevron = if space.collapsed { "icons/chevron-down.svg" } else { "icons/chevron-up.svg" };
    div()
        .id(("space-row", id as usize))
        .group(group.clone())
        .mt(px(6.))
        .h(px(34.))
        .pl(px(4.))
        .pr(px(4.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(R_ITEM))
        .when(active, |el| el.bg(hsla(SURFACE)))
        .hover(|el| el.bg(hsla(SURFACE_HOVER)))
        .cursor_pointer()
        .tooltip(crate::hover::tip_text(
            if active { "Los agentes nuevos se abren en este espacio" } else { "Clic para abrir aquí los agentes nuevos" }.into(),
        ))
        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
            v.spaces.select(id);
            cx.notify();
        }))
        .child(
            mini_button(("space-toggle", id as usize), chevron, "Mostrar u ocultar las carpetas").on_click(cx.listener(
                move |v, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    v.spaces.toggle(id);
                    cx.notify();
                },
            )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(if active { TEXT } else { MUTED }))
                .child(space.name.clone()),
        )
        .when(agents > 0, |el| el.child(div().flex_none().text_size(px(11.)).text_color(hsla(FAINT)).child(agents.to_string())))
        .child(
            div()
                .flex()
                .items_center()
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .child(
                    mini_button(("space-new", id as usize), "icons/plus.svg", "Abrir un agente aquí").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |v, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                            v.mando.menu = Some(Menu { x: x - 10.0, y: y + 14.0, workspace: Some(id) });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    mini_button(("space-add", id as usize), "icons/folder-plus.svg", "Agregar carpetas").on_click(
                        cx.listener(move |v, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            v.open_picker(Purpose::AddTo(id), cx);
                        }),
                    ),
                )
                .child(
                    mini_button(("space-remove", id as usize), "icons/x.svg", "Quitar el espacio (no borra nada)")
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            v.spaces.remove(id);
                            cx.notify();
                        })),
                ),
        )
}

/// Una carpeta de un espacio. La primera es donde arrancan los agentes.
fn folder_line(space: u64, index: usize, path: &Path, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let group: SharedString = format!("folder-{space}-{index}").into();
    div()
        .id(SharedString::from(format!("folder-{space}-{index}")))
        .group(group.clone())
        .ml(px(24.))
        .h(px(26.))
        .pl(px(6.))
        .pr(px(4.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .rounded(px(8.))
        .hover(|el| el.bg(hsla(SURFACE_HOVER)))
        .tooltip(crate::hover::tip_text(
            if index == 0 {
                format!("{}\nLos agentes arrancan aquí y ven las demás", path.display())
            } else {
                path.display().to_string()
            }
            .into(),
        ))
        .child(svg().path("icons/folder.svg").size(px(12.)).flex_none().text_color(hsla(FAINT)))
        .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(12.)).text_color(hsla(MUTED)).child(short_name(path)))
        .child(
            div().opacity(0.).group_hover(group, |s| s.opacity(1.)).child(
                mini_button(SharedString::from(format!("folder-x-{space}-{index}")), "icons/x.svg", "Quitar del espacio")
                    .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                        v.spaces.remove_folder(space, index);
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

fn pane_panel(
    p: PaneData,
    cell: Cell,
    hidden: &[AgentData],
    launchers: &[(Option<&'static str>, &'static str)],
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
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
    if let Some(doc) = p.doc {
        return panel.child(doc_pane(pane, area, doc, cx));
    }
    let Some(agent) = p.card else {
        return panel.child(empty_pane(pane, hidden, launchers, cx));
    };
    let id = agent.id;
    let term = term_area(area);
    let wide = area.w >= 600.0;
    let grid = p.grid;
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
            icon_button(("pane-files", pane), "icons/folder-open.svg", "Ver sus archivos", SURFACE_HOVER).on_click(
                cx.listener(move |v, _: &ClickEvent, _, cx| {
                    v.mando.agent = Some(id);
                    v.mando.tab = Tab::Files;
                    cx.notify();
                }),
            ),
        )
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
            icon_button(("pane-close", pane), "icons/x.svg", "Terminar (Ctrl+W)", SURFACE_HOVER)
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.kill_pane(pane, cx))),
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
fn empty_pane(
    pane: usize,
    hidden: &[AgentData],
    launchers: &[(Option<&'static str>, &'static str)],
    cx: &mut Context<SpaceView>,
) -> impl IntoElement {
    let opens = launchers
        .iter()
        .enumerate()
        .map(|(index, &(cli, name))| {
            pill(SharedString::from(format!("empty-{pane}-open-{index}")), name, Tone::Surface)
                .pl(px(10.))
                .child(svg().path(agent_icon(cli)).size(px(13.)).text_color(hsla(MUTED)))
                .flex_row_reverse()
                .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| {
                    v.focus_pane(pane, window, cx);
                    v.open_here(cli, None, cx);
                }))
                .into_any_element()
        })
        .collect::<Vec<_>>();
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
        .child(div().max_w(px(560.)).flex().flex_wrap().justify_center().gap(px(8.)).children(opens))
}

/// El agente (sus carpetas, lo que cambió y su árbol de archivos) y la
/// bandeja de revisión, en pestañas.
fn detail_panel(area: Area, detail: Detail, inbox: &[TrayItem], cx: &mut Context<SpaceView>) -> impl IntoElement {
    let Detail { agent, dirs, files, closing, tab, tree, changed } = detail;
    let mut panel = div()
        .id("mando-detail")
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .rounded(px(R_PANEL))
        .bg(hsla(SURFACE))
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(8.));
    if let Some(data) = &agent {
        let id = data.id;
        panel = panel.child(
            div()
                .flex()
                .flex_none()
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
                )
                .when(!closing, |el| {
                    el.child(pill("detail-end", "Terminar", Tone::Danger).on_click(cx.listener(
                        move |v, _: &ClickEvent, _, cx| {
                            v.mando.closing = Some(id);
                            cx.notify();
                        },
                    )))
                })
                .when(closing, |el| {
                    el.child(
                        pill("detail-end-yes", "¿Seguro?", Tone::Danger)
                            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.terminate(id, cx))),
                    )
                }),
        );
    }
    let tab_button = |id: &'static str, label: String, this: Tab, cx: &mut Context<SpaceView>| {
        let on = tab == this;
        div()
            .id(id)
            .h(px(26.))
            .px(px(11.))
            .flex()
            .items_center()
            .rounded(px(13.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(hsla(if on { INK } else { MUTED }))
            .when(on, |el| el.bg(hsla(0xe9e9e2)))
            .when(!on, |el| el.cursor_pointer().hover(|el| el.text_color(hsla(TEXT))))
            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                v.mando.tab = this;
                cx.notify();
            }))
            .child(label)
    };
    let count = |label: &str, n: usize| if n > 0 { format!("{label} {n}") } else { label.to_string() };
    panel = panel.child(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(16.))
            .bg(hsla(0x161615))
            .child(tab_button("tab-changes", count("Cambios", files.len()), Tab::Changes, cx))
            .child(tab_button("tab-files", "Archivos".into(), Tab::Files, cx))
            .child(tab_button("tab-review", count("Revisar", inbox.len()), Tab::Review, cx))
            .child(div().flex_1())
            .when(tab == Tab::Files, |el| {
                el.child(
                    icon_button("tree-refresh", "icons/rotate-cw.svg", "Volver a leer las carpetas", SURFACE_HOVER)
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| {
                            v.mando.explorer.refresh();
                            cx.notify();
                        })),
                )
            }),
    );
    let note = |text: &'static str| div().px(px(4.)).text_size(px(12.)).text_color(hsla(MUTED)).child(text);
    let body = div().id("detail-body").flex_1().min_h(px(0.)).flex().flex_col().gap(px(1.)).overflow_y_scroll();
    let body = match tab {
        Tab::Changes => {
            if agent.is_none() {
                body.child(note("Elige un agente para ver lo que cambió en sus carpetas."))
            } else if files.is_empty() {
                body.child(note(if dirs.is_empty() {
                    "Este agente no tiene carpeta."
                } else {
                    "Nada todavía. Aquí aparece lo que cambie en sus carpetas desde que empezó."
                }))
            } else {
                body.children(files.into_iter().enumerate().map(|(index, file)| file_row(index, file, cx).into_any_element()))
            }
        }
        Tab::Files => {
            if tree.is_empty() {
                body.child(note("Crea un espacio con carpetas, o elige un agente, para ver sus archivos."))
            } else {
                let full = tree.len() >= super::explorer::MAX_ROWS;
                body.children(tree.into_iter().enumerate().map(|(index, row)| {
                    let edited = changed.contains(&row.path);
                    tree_row(index, row, edited, cx).into_any_element()
                }))
                .when(full, |el| el.child(note("Hay más; cierra alguna carpeta para verlos.")))
            }
        }
        Tab::Review => {
            if inbox.is_empty() {
                body.child(note("Nada que revisar. Cuando un agente termine un turno o se detenga y no lo estés mirando, aparece aquí."))
            } else {
                body.gap(px(8.)).children(inbox.iter().map(|item| inbox_item(item, cx).into_any_element()))
            }
        }
    };
    panel.child(body)
}

fn file_row(index: usize, file: changes::Change, cx: &mut Context<SpaceView>) -> impl IntoElement {
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
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.open_doc(&path, true, cx)))
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

/// Una fila del árbol: una carpeta se abre o cierra, un archivo se abre en un
/// panel. Lo que el agente cambió va en amarillo.
fn tree_row(index: usize, row: super::explorer::Row, edited: bool, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let path = row.path.clone();
    let (dir, root) = (row.dir, row.depth == 0);
    let icon = if !row.dir {
        "icons/text-align-start.svg"
    } else if row.open {
        "icons/folder-open.svg"
    } else {
        "icons/folder.svg"
    };
    div()
        .id(("tree-row", index))
        .h(px(24.))
        .pl(px(4.0 + row.depth as f32 * 14.0))
        .pr(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .rounded(px(7.))
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|el| el.bg(hsla(SURFACE_HOVER)))
        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
            if dir {
                v.mando.explorer.toggle(&path, root);
                cx.notify();
            } else {
                v.open_doc(&path, false, cx);
            }
        }))
        .child(svg().path(icon).size(px(13.)).flex_none().text_color(hsla(if row.dir { MUTED } else { FAINT })))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .when(root, |el| el.font_weight(FontWeight::SEMIBOLD))
                .text_color(hsla(if edited {
                    WORKING_DOT
                } else if row.dir {
                    TEXT
                } else {
                    MUTED
                }))
                .child(row.name),
        )
}

/// Un archivo abierto en un panel: su contenido o lo que cambió.
fn doc_pane(pane: usize, area: Area, doc: DocView, cx: &mut Context<SpaceView>) -> impl IntoElement {
    const LINE_H: f32 = 19.0;
    let has_diff = doc.diff.is_some();
    let show_diff = doc.show_diff && has_diff;
    let toggle = |id: &'static str, label: &'static str, diff: bool, cx: &mut Context<SpaceView>| {
        let on = show_diff == diff;
        div()
            .id((id, pane))
            .h(px(24.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(12.))
            .text_size(px(11.5))
            .text_color(hsla(if on { INK } else { MUTED }))
            .when(on, |el| el.bg(hsla(0xe9e9e2)))
            .when(!on, |el| el.cursor_pointer().hover(|el| el.text_color(hsla(TEXT))))
            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                if let Some(doc) = v.mando.panes.card_in(pane).and_then(|id| v.docs.iter_mut().find(|d| d.id == id)) {
                    doc.show_diff = diff;
                    cx.notify();
                }
            }))
            .child(label)
    };
    let head = div()
        .absolute()
        .left(px(PANEL_PAD))
        .top(px(PANEL_PAD))
        .w(px(area.w - PANEL_PAD * 2.0))
        .h(px(HEAD_H))
        .pl(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(svg().path("icons/text-align-start.svg").size(px(15.)).flex_none().text_color(hsla(MUTED)))
        .child(div().flex_none().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(TEXT)).child(doc.name.clone()))
        .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(11.5)).text_color(hsla(FAINT)).child(doc.dir.clone()))
        .when(has_diff, |el| {
            el.child(
                div()
                    .flex()
                    .flex_none()
                    .p(px(2.))
                    .rounded(px(14.))
                    .bg(hsla(0x161615))
                    .child(toggle("doc-file", "Archivo", false, cx))
                    .child(toggle("doc-diff", "Cambios", true, cx)),
            )
        })
        .child(
            icon_button(("doc-reload", pane), "icons/rotate-cw.svg", "Volver a leer", SURFACE_HOVER).on_click(cx.listener(
                move |v, _: &ClickEvent, _, cx| {
                    if let Some(doc) = v.mando.panes.card_in(pane).and_then(|id| v.docs.iter_mut().find(|d| d.id == id)) {
                        doc.reload();
                        cx.notify();
                    }
                },
            )),
        )
        .child(
            icon_button(("doc-close", pane), "icons/x.svg", "Cerrar (Ctrl+W)", SURFACE_HOVER)
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.kill_pane(pane, cx))),
        );
    let term = term_area(area);
    let body = div()
        .absolute()
        .left(px(term.x))
        .top(px(term.y))
        .w(px(term.w))
        .h(px(term.h))
        .rounded(px(R_INSET))
        .bg(hsla(TERMINAL))
        .pt(px(6.))
        .font_family(FONT_FAMILY)
        .text_size(px(12.5));
    let body = if let Some(note) = doc.note.clone() {
        body.p(px(16.)).text_color(hsla(MUTED)).child(note).into_any_element()
    } else if show_diff {
        let lines = doc.diff.clone().unwrap_or_default();
        let digits = lines.iter().filter_map(|l| l.number).max().unwrap_or(1).to_string().len();
        let gutter = digits as f32 * 8.0 + 22.0;
        body.child(
            gpui::uniform_list(("doc-lines", pane), lines.len(), move |range, _, _| {
                range
                    .map(|index| {
                        let line = &lines[index];
                        let (bg, fg, mark) = match line.kind {
                            LineKind::Added => (Some(0x1d3324), 0xc8f0d2, "+"),
                            LineKind::Removed => (Some(0x3a2020), 0xf2c4bd, "-"),
                            LineKind::Hunk => (Some(0x1f2430), 0x8fa6d6, ""),
                            LineKind::Context => (None, console::FOREGROUND, ""),
                        };
                        div()
                            .h(px(LINE_H))
                            .flex()
                            .items_center()
                            .when_some(bg, |el, bg| el.bg(hsla(bg)))
                            .child(
                                div()
                                    .w(px(gutter))
                                    .flex_none()
                                    .pr(px(10.))
                                    .flex()
                                    .justify_end()
                                    .text_color(hsla(FAINT))
                                    .child(line.number.map(|n| n.to_string()).unwrap_or_default()),
                            )
                            .child(div().w(px(14.)).flex_none().text_color(hsla(fg)).child(mark))
                            .child(div().flex_1().min_w(px(0.)).truncate().text_color(hsla(fg)).child(line.text.clone()))
                            .into_any_element()
                    })
                    .collect()
            })
            .size_full(),
        )
        .into_any_element()
    } else {
        let lines = doc.lines.clone();
        let gutter = lines.len().max(1).to_string().len() as f32 * 8.0 + 22.0;
        body.child(
            gpui::uniform_list(("doc-lines", pane), lines.len(), move |range, _, _| {
                range
                    .map(|index| {
                        div()
                            .h(px(LINE_H))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .w(px(gutter))
                                    .flex_none()
                                    .pr(px(12.))
                                    .flex()
                                    .justify_end()
                                    .text_color(hsla(FAINT))
                                    .child((index + 1).to_string()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_color(hsla(console::FOREGROUND))
                                    .child(lines[index].clone()),
                            )
                            .into_any_element()
                    })
                    .collect()
            })
            .size_full(),
        )
        .into_any_element()
    };
    div().absolute().inset_0().child(head).child(body)
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
