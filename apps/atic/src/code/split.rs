//! Varias conversaciones a la vez, en filas y columnas. Se arrastra una de la barra
//! lateral a un panel: soltarla en su mitad izquierda lo parte y la deja a la
//! izquierda; en la derecha, a la derecha. Con la opción «Reemplazar el panel
//! enfocado» (`SplitPick::Replace`) vuelven los bordes de arriba y abajo y el centro,
//! que reemplaza la conversación de ese panel.
//!
//! Elegir otra conversación con la vista dividida la abre sola (de fábrica): los
//! paneles quedan guardados como un grupo (`SavedSplit`) que la barra muestra junto,
//! y un clic en él los devuelve tal como estaban.
//!
//! El árbol de paneles es `kit::panes`, con la clave de la
//! conversación en cada hoja. Solo un panel es el activo (`panes.focused`): suyos
//! son la caja de texto, los menús y el panel derecho, y `CodeView::active` es la
//! conversación que tiene. Los demás se ven enteros, con sus permisos, y un clic en
//! cualquier parte de ellos los vuelve el activo. El borrador de cada conversación
//! se guarda al cambiar de panel.

use crate::i18n::tf;
use std::cell::Cell;
use std::rc::Rc;

use gpui::{div, prelude::*, px, Bounds, Context, Pixels, Render, SharedString, Window};

use super::config::SplitPick;
use super::{CodeView, SessionInfo};
use crate::kit::panes::{Axis, Divider, Panes};
use crate::kit::Area;

/// Lo mínimo que mide un panel: si no cabe, no se divide ni se encoge más.
pub const MIN_PANE: (f32, f32) = (320.0, 240.0);

/// Lo que se arrastra desde la barra: una conversación abierta (`key`) o una
/// guardada (`session`) de un espacio.
#[derive(Clone)]
pub struct ChatDrag {
    pub key: Option<String>,
    pub session: Option<SessionInfo>,
    pub workspace: u64,
    pub title: SharedString,
}

/// Lo que sigue al cursor mientras se arrastra: el título en una cápsula.
pub struct DragChip {
    title: SharedString,
    /// La ventana donde empezó el arrastre: GPUI pinta lo arrastrado en todas las ventanas que
    /// se redibujan (la pill también), y solo aquí debe verse.
    origin: gpui::AnyWindowHandle,
}

impl DragChip {
    pub fn from_window(title: SharedString, window: &Window) -> Self {
        Self { title, origin: window.window_handle() }
    }
}

impl Render for DragChip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = super::style::t();
        if self.origin != window.window_handle() {
            return div();
        }
        div()
            .max_w(px(260.))
            .px(px(14.))
            .py(px(8.))
            .rounded(px(18.))
            .bg(t.accent_soft)
            .text_color(t.on_accent_soft)
            .text_size(px(13.))
            .font_family(t.font)
            .shadow(super::view::float_shadow())
            .truncate()
            .child(self.title.clone())
    }
}

// --- Dónde cae un soltar -----------------------------------------------------------

/// La parte de un panel donde se suelta: sus cuatro bordes o el centro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Up,
    Down,
    Center,
}

impl Zone {
    /// Cómo parte el panel: el eje y si el panel nuevo queda antes.
    fn split(self) -> Option<(Axis, bool)> {
        match self {
            Zone::Left => Some((Axis::Row, true)),
            Zone::Right => Some((Axis::Row, false)),
            Zone::Up => Some((Axis::Column, true)),
            Zone::Down => Some((Axis::Column, false)),
            Zone::Center => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Zone::Left => crate::i18n::t("pill.code.split.openLeft"),
            Zone::Right => crate::i18n::t("pill.code.split.openRight"),
            Zone::Up => crate::i18n::t("pill.code.split.openUp"),
            Zone::Down => crate::i18n::t("pill.code.split.openDown"),
            Zone::Center => crate::i18n::t("pill.code.split.replace"),
        }
    }
}

/// Qué tan adentro del panel (de 0 a 1) empieza el centro.
const EDGE: f32 = 0.25;

/// La zona bajo el punto `(x, y)`, relativo a la esquina de un panel de `w` × `h`. De
/// fábrica solo cuenta la mitad: la izquierda o la derecha. Con `replace` (la opción
/// «Reemplazar el panel enfocado») están los cuatro bordes y el centro.
pub fn zone_at(w: f32, h: f32, x: f32, y: f32, replace: bool) -> Zone {
    if !replace {
        return if x < w / 2. { Zone::Left } else { Zone::Right };
    }
    if w <= 0. || h <= 0. {
        return Zone::Center;
    }
    let (nx, ny) = ((x / w).clamp(0., 1.), (y / h).clamp(0., 1.));
    let inside = |v: f32| (EDGE..=1. - EDGE).contains(&v);
    if inside(nx) && inside(ny) {
        return Zone::Center;
    }
    [(nx, Zone::Left), (1. - nx, Zone::Right), (ny, Zone::Up), (1. - ny, Zone::Down)]
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(Zone::Center, |(_, zone)| zone)
}

/// La zona de un panel que ocupa `bounds` en la ventana bajo `position`, también de la
/// ventana. Se mide contra el mismo rectángulo que recibe el arrastre (el del panel,
/// con la barra lateral ya a la izquierda), así la guía calza con la mitad del mouse.
pub fn zone_in(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>, replace: bool) -> Zone {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    zone_at(w, h, f32::from(position.x - bounds.origin.x), f32::from(position.y - bounds.origin.y), replace)
}

/// El rectángulo que ocuparía lo soltado en esa zona, relativo al panel.
pub fn zone_preview(w: f32, h: f32, zone: Zone) -> Area {
    match zone {
        Zone::Left => Area { x: 0., y: 0., w: w / 2., h },
        Zone::Right => Area { x: w / 2., y: 0., w: w / 2., h },
        Zone::Up => Area { x: 0., y: 0., w, h: h / 2. },
        Zone::Down => Area { x: 0., y: h / 2., w, h: h / 2. },
        Zone::Center => Area { x: 0., y: 0., w, h },
    }
}

/// Lo que pasa al soltar una conversación en una zona de un panel.
pub enum Dropped {
    /// Ya estaba ahí: solo se enfoca ese panel.
    Focus(usize),
    /// Los paneles como quedan y el que recibe la conversación (vacío si es nuevo).
    Place { panes: Panes<String>, dest: usize },
    /// Con un panel más, alguno quedaría bajo el mínimo.
    NoRoom,
}

/// Decide qué hacer al soltar `key` (si es una conversación abierta) en `zone` de
/// `target`. Si ya está a la vista en otro panel, se mueve: en el centro se
/// intercambian, y en un borde su panel anterior se quita.
pub fn plan_drop(panes: &Panes<String>, target: usize, zone: Zone, key: Option<&str>, area: Area, gap: f32) -> Dropped {
    let from = key.and_then(|k| panes.pane_of(k.to_string()));
    if from == Some(target) {
        return Dropped::Focus(target);
    }
    let mut next = panes.clone();
    let Some((axis, before)) = zone.split() else {
        if let Some(from) = from {
            next.swap(from, target);
        }
        return Dropped::Place { panes: next, dest: target };
    };
    if let Some(from) = from {
        next.close(from);
    }
    let dest = next.split_at(target, axis, before);
    if !next.fits(area, gap, MIN_PANE) {
        return Dropped::NoRoom;
    }
    Dropped::Place { panes: next, dest }
}

/// Quién queda como activo al cerrar `closing`: el panel más cercano.
pub fn after_close(panes: &Panes<String>, closing: usize, area: Area, gap: f32) -> Option<usize> {
    let rects = panes.layout(area, gap);
    let (_, from) = rects.iter().find(|(p, _)| *p == closing)?;
    let (cx, cy) = (from.x + from.w / 2., from.y + from.h / 2.);
    rects
        .iter()
        .filter(|(p, _)| *p != closing)
        .min_by(|(_, a), (_, b)| {
            let d = |r: &Area| (r.x + r.w / 2. - cx).abs() + (r.y + r.h / 2. - cy).abs();
            d(a).total_cmp(&d(b))
        })
        .map(|(p, _)| *p)
}

/// El panel que lleva los botones de la ventana: el de arriba a la derecha.
pub fn controls_pane(rects: &[(usize, Area)]) -> Option<usize> {
    let right = rects.iter().map(|(_, r)| r.x + r.w).fold(f32::MIN, f32::max);
    rects
        .iter()
        .filter(|(_, r)| (r.x + r.w - right).abs() < 1.)
        .min_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
        .map(|(p, _)| *p)
}

// --- Grupos guardados ------------------------------------------------------------

/// Una vista dividida guardada: los paneles como quedaron al elegir otra conversación
/// (`SplitPick::Alone`). La barra la muestra como un grupo y un clic la devuelve.
#[derive(Clone)]
pub struct SavedSplit {
    pub id: u64,
    pub panes: Panes<String>,
}

/// Qué hacer cuando el usuario elige una conversación (la barra, el historial, la paleta o
/// el atajo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Está en el grupo guardado de ese índice: vuelve ese grupo.
    Restore(usize),
    /// Ya está a la vista: solo se enfoca su panel.
    Focus,
    /// Con la vista dividida: se guarda como grupo y la elegida se abre sola.
    Alone,
    /// En el panel enfocado (sin dividir, o con `SplitPick::Replace`).
    Here,
}

pub fn plan_pick(current: &Panes<String>, groups: &[SavedSplit], key: Option<&str>, mode: SplitPick) -> Pick {
    if let Some(key) = key {
        let in_current = current.pane_of(key.to_string()).is_some();
        if in_current && current.count() > 1 {
            return Pick::Focus;
        }
        // Va antes que un panel solo: en la barra se ve dentro de su grupo.
        if let Some(at) = groups.iter().position(|g| g.panes.pane_of(key.to_string()).is_some()) {
            return Pick::Restore(at);
        }
        if in_current {
            return Pick::Focus;
        }
    }
    if current.count() > 1 && mode == SplitPick::Alone {
        Pick::Alone
    } else {
        Pick::Here
    }
}

/// Las conversaciones de unos paneles en el orden en que se ven (de izquierda a derecha y
/// de arriba abajo).
pub fn ordered(panes: &Panes<String>) -> Vec<String> {
    panes.layout(Area { x: 0., y: 0., w: 1000., h: 1000. }, 0.).into_iter().filter_map(|(pane, _)| panes.card_in(pane)).collect()
}

/// Ordena los grupos guardados: las conversaciones que se cerraron o que ya están en la
/// vista dividida de ahora salen de ellos, y un grupo con menos de dos deja de serlo.
pub fn prune_groups(groups: &mut Vec<SavedSplit>, current: &Panes<String>, alive: &[String]) {
    let taken = if current.count() > 1 { current.shown() } else { Vec::new() };
    for group in groups.iter_mut() {
        let gone: Vec<usize> = group
            .panes
            .slots()
            .iter()
            .filter(|(_, card)| card.as_ref().is_none_or(|key| !alive.contains(key) || taken.contains(key)))
            .map(|(pane, _)| *pane)
            .collect();
        for pane in gone {
            group.panes.close(pane);
        }
    }
    groups.retain(|group| group.panes.shown().len() >= 2);
}

/// Un grupo de la barra: el de la vista de ahora o uno guardado.
pub struct GroupRow {
    pub id: u64,
    pub current: bool,
    pub keys: Vec<String>,
}

/// Qué tanto cabe en el encabezado y la caja de un panel, por su ancho.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
    /// Casi sin lugar: solo lo esencial.
    Tight,
    /// Los botones pasan a solo ícono.
    Narrow,
    /// Todo con su texto.
    Wide,
}

pub fn fit(width: f32) -> Fit {
    if width < 460. {
        Fit::Tight
    } else if width < 760. {
        Fit::Narrow
    } else {
        Fit::Wide
    }
}

// --- El estado de los paneles ------------------------------------------------------

/// Una raya que se está arrastrando.
pub struct ResizeDrag {
    path: Vec<bool>,
    axis: Axis,
    span: Area,
}

/// Lo que `CodeView` guarda de los paneles que no es el árbol.
pub struct PaneState {
    /// El rectángulo del área de paneles en la ventana (lo anota un `canvas`).
    pub bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Sobre qué zona de qué panel está lo que se arrastra.
    pub hint: Option<(usize, Zone)>,
    pub resizing: Option<ResizeDrag>,
    /// Las vistas divididas guardadas, en el orden en que se crearon.
    pub groups: Vec<SavedSplit>,
    /// El id de grupo de la vista dividida de ahora (con un solo panel, ninguno): al
    /// guardarla lo conserva, así su tarjeta no cambia de lugar en la barra.
    pub live_group: Option<u64>,
    next_group: u64,
    /// Sube cada vez que los paneles se cambian enteros (volver a un grupo, abrir una sola):
    /// los paneles de la vista nueva entran con su resorte.
    pub layout_gen: u64,
    /// Los paneles ya vistos en esta vista (de `layout_gen`) y si entraron animados.
    pub(super) born: (u64, std::collections::HashMap<usize, bool>),
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            bounds: Rc::new(Cell::new(None)),
            hint: None,
            resizing: None,
            groups: Vec::new(),
            live_group: None,
            next_group: 1,
            layout_gen: 0,
            born: (0, Default::default()),
        }
    }
}

impl PaneState {
    fn new_group_id(&mut self) -> u64 {
        let id = self.next_group;
        self.next_group += 1;
        id
    }

    /// Si el panel `pane` entra con resorte: solo uno que aparece con la vista ya dividida
    /// (uno nuevo al soltar, o todos al volver a un grupo). El de una vista de un panel y
    /// los que ya estaban no.
    pub(super) fn enters(&mut self, pane: usize, multi: bool) -> bool {
        if self.born.0 != self.layout_gen {
            self.born = (self.layout_gen, Default::default());
        }
        *self.born.1.entry(pane).or_insert(multi)
    }
}

/// Lo que separa los paneles (y el ancho de su raya).
pub fn gap() -> f32 {
    super::style::t().gap.max(6.)
}

impl CodeView {
    /// Hace que `el` (una fila de la barra) se pueda arrastrar al chat.
    pub(super) fn chat_drag<E: StatefulInteractiveElement>(&self, el: E, drag: ChatDrag, cx: &mut Context<Self>) -> E {
        let view = cx.weak_entity();
        el.on_drag(drag, move |drag, _, window, cx| {
            let _ = view.update(cx, |view, cx| {
                view.dragging_chat = true;
                cx.notify();
            });
            let chip = DragChip::from_window(drag.title.clone(), window);
            cx.new(|_| chip)
        })
    }

    /// El tamaño del área de paneles: el último medido, o una estimación el
    /// primer cuadro.
    pub(super) fn pane_area(&self, window: &Window) -> Area {
        match self.pane_state.bounds.get() {
            Some(b) if f32::from(b.size.width) > 0. => Area { x: 0., y: 0., w: f32::from(b.size.width), h: f32::from(b.size.height) },
            _ => {
                let size = window.viewport_size();
                Area { x: 0., y: 0., w: (f32::from(size.width) - 300.).max(400.), h: (f32::from(size.height) - 20.).max(300.) }
            }
        }
    }

    /// El origen del área de paneles en la ventana.
    fn pane_origin(&self) -> (f32, f32) {
        self.pane_state.bounds.get().map_or((0., 0.), |b| (f32::from(b.origin.x), f32::from(b.origin.y)))
    }

    /// Los paneles y `active` dicen lo mismo: lo que se abre o se elige en otra
    /// parte pone `active`, y aquí el panel activo lo alcanza. Las conversaciones
    /// que ya no existen dejan su panel vacío.
    pub(super) fn sync_panes(&mut self) {
        let alive: Vec<String> = self.chats.iter().map(|c| c.key.clone()).collect();
        self.panes.forget(&alive);
        prune_groups(&mut self.pane_state.groups, &self.panes, &alive);
        if self.panes.count() > 1 {
            if self.pane_state.live_group.is_none() {
                self.pane_state.live_group = Some(self.pane_state.new_group_id());
            }
        } else {
            self.pane_state.live_group = None;
        }
        match self.active.clone() {
            Some(key) => match self.panes.pane_of(key.clone()) {
                Some(pane) => self.panes.focused = pane,
                None => {
                    let focused = self.panes.focused;
                    self.panes.put(focused, Some(key));
                }
            },
            None => {
                let focused = self.panes.focused;
                self.panes.put(focused, None);
            }
        }
    }

    /// Se soltó una conversación sobre una zona de un panel.
    pub(super) fn drop_chat(&mut self, drag: &ChatDrag, pane: usize, zone: Zone, window: &mut Window, cx: &mut Context<Self>) {
        self.dragging_chat = false;
        self.pane_state.hint = None;
        self.sync_panes();
        let area = self.pane_area(window);
        match plan_drop(&self.panes, pane, zone, drag.key.as_deref(), area, gap()) {
            Dropped::NoRoom => {
                self.show_toast(tf("pill.code.split.noRoom", &[("w", &MIN_PANE.0), ("h", &MIN_PANE.1)]), cx);
            }
            Dropped::Focus(pane) => self.focus_pane(pane, window, cx),
            Dropped::Place { panes, dest } => {
                self.save_draft(cx);
                self.history_page = false;
                self.panes = panes;
                self.panes.focused = dest;
                match (&drag.key, &drag.session) {
                    (Some(key), _) => self.select_chat(key.clone(), window, cx),
                    (None, Some(info)) => self.open_session(drag.workspace, info.clone(), window, cx),
                    _ => {}
                }
                self.restore_draft(window, cx);
            }
        }
        cx.notify();
    }

    /// Si soltar puede reemplazar (la opción «Reemplazar el panel enfocado»): con ella
    /// vuelven el centro y los bordes de arriba y abajo.
    pub(super) fn drop_replaces(&self) -> bool {
        self.configs.split_pick == SplitPick::Replace
    }

    /// El usuario eligió una conversación (la barra, el historial, la paleta o el atajo).
    /// Con la vista dividida, según la opción: la abre sola y deja los paneles guardados
    /// como grupo en la barra, o la pone en el panel enfocado. Si está en un grupo guardado,
    /// vuelve ese grupo con ella enfocada.
    pub(super) fn pick_chat(&mut self, key: Option<String>, session: Option<SessionInfo>, workspace: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.history_page = false;
        let key = key.or_else(|| {
            let id = session.as_ref()?.session_id.clone();
            self.chats.iter().find(|c| c.listed_session() == Some(id.as_str())).map(|c| c.key.clone())
        });
        match plan_pick(&self.panes, &self.pane_state.groups, key.as_deref(), self.configs.split_pick) {
            Pick::Restore(at) => {
                let id = self.pane_state.groups[at].id;
                self.restore_group(id, key, window, cx);
                return;
            }
            Pick::Alone => self.stash_split(cx),
            Pick::Focus | Pick::Here => {}
        }
        match (key, session) {
            (Some(key), _) => self.select_chat(key, window, cx),
            (None, Some(info)) => self.open_session(workspace, info, window, cx),
            _ => {}
        }
    }

    /// Guarda la vista dividida de ahora como grupo (si tiene al menos dos conversaciones)
    /// y deja un solo panel, vacío, para lo que se abra.
    fn stash_split(&mut self, cx: &mut Context<Self>) {
        self.save_draft(cx);
        let panes = std::mem::take(&mut self.panes);
        let id = self.pane_state.live_group.take();
        if panes.shown().len() >= 2 {
            let id = id.unwrap_or_else(|| self.pane_state.new_group_id());
            self.pane_state.groups.push(SavedSplit { id, panes });
        }
        self.pane_state.layout_gen += 1;
        self.pane_state.hint = None;
    }

    /// Vuelve el grupo guardado `id` tal como estaba, con `focus` (o el que estaba activo)
    /// enfocado. Si lo de ahora también es una vista dividida, queda guardada en su lugar.
    pub(super) fn restore_group(&mut self, id: u64, focus: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.pane_state.groups.iter().position(|g| g.id == id) else {
            return;
        };
        self.save_draft(cx);
        self.history_page = false;
        let group = self.pane_state.groups.remove(at);
        let current = std::mem::replace(&mut self.panes, group.panes);
        let current_id = self.pane_state.live_group.replace(group.id);
        if current.shown().len() >= 2 {
            let id = current_id.unwrap_or_else(|| self.pane_state.new_group_id());
            self.pane_state.groups.insert(at, SavedSplit { id, panes: current });
        }
        self.pane_state.layout_gen += 1;
        let pane = focus.and_then(|key| self.panes.pane_of(key)).unwrap_or(self.panes.focused);
        self.enter_pane(pane, window, cx);
    }

    /// Deshace un grupo guardado: sus conversaciones vuelven sueltas a la barra.
    pub(super) fn dissolve_group(&mut self, id: u64, cx: &mut Context<Self>) {
        self.pane_state.groups.retain(|g| g.id != id);
        cx.notify();
    }

    /// Los grupos que muestra la barra, en un orden fijo (el de creación): la vista
    /// dividida de ahora y las guardadas.
    pub(super) fn split_groups(&self) -> Vec<GroupRow> {
        let mut rows: Vec<GroupRow> = self.pane_state.groups.iter().map(|g| GroupRow { id: g.id, current: false, keys: ordered(&g.panes) }).collect();
        if let Some(id) = self.pane_state.live_group.filter(|_| self.panes.count() > 1) {
            let keys = ordered(&self.panes);
            if !keys.is_empty() {
                rows.push(GroupRow { id, current: true, keys });
            }
        }
        rows.sort_by_key(|row| row.id);
        rows
    }

    /// Un panel pasa a ser el activo: se guarda el borrador del que lo era y se
    /// pone el del nuevo.
    pub(super) fn focus_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        if pane == self.panes.focused || !self.panes.slots().iter().any(|(p, _)| *p == pane) {
            return;
        }
        self.save_draft(cx);
        self.enter_pane(pane, window, cx);
    }

    /// Hace activo a `pane` con lo que tiene (o un inicio vacío).
    fn enter_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.history_page = false;
        self.panes.focused = pane;
        match self.panes.card_in(pane) {
            Some(key) => self.select_chat(key, window, cx),
            None => self.active = None,
        }
        self.restore_draft(window, cx);
        cx.notify();
    }

    /// Cierra un panel (la conversación sigue abierta en la barra). Si era el
    /// activo, lo pasa a su vecino más cercano. Con uno solo no hay nada que cerrar.
    pub(super) fn close_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.count() <= 1 {
            return;
        }
        let was_active = pane == self.panes.focused;
        let next = after_close(&self.panes, pane, self.pane_area(window), gap());
        if was_active {
            self.save_draft(cx);
        }
        self.panes.close(pane);
        if was_active {
            match next {
                Some(next) => self.enter_pane(next, window, cx),
                None => self.sync_panes(),
            }
        }
        cx.notify();
    }

    /// Una conversación se cerró del todo: su panel, si no es el activo, se quita.
    pub(super) fn forget_in_panes(&mut self, key: &str) {
        if let Some(pane) = self.panes.pane_of(key.to_string()) {
            if pane != self.panes.focused && self.panes.count() > 1 {
                self.panes.close(pane);
            } else {
                self.panes.put(pane, None);
            }
        }
    }

    /// Guarda lo escrito en la caja como borrador de la conversación activa (`drafts.rs`).
    pub(super) fn save_draft(&mut self, cx: &mut Context<Self>) {
        self.sync_draft(cx);
    }

    /// Pone en la caja el borrador de la conversación activa (o la deja vacía).
    pub(super) fn restore_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_draft(cx);
        self.sync_preview_owner(cx);
        self.focus_composer(window, cx);
    }

    // --- Rayas ---------------------------------------------------------------------

    pub(super) fn start_resize(&mut self, divider: &Divider) {
        self.pane_state.resizing = Some(ResizeDrag { path: divider.path.clone(), axis: divider.axis, span: divider.span });
    }

    /// El mouse se movió con una raya agarrada: la proporción sigue al cursor.
    /// Devuelve si hay algo que redibujar.
    pub(super) fn drag_resize(&mut self, position: gpui::Point<Pixels>, pressed: bool, window: &Window) -> bool {
        let Some(drag) = &self.pane_state.resizing else {
            return false;
        };
        if !pressed {
            self.pane_state.resizing = None;
            return true;
        }
        let (ox, oy) = self.pane_origin();
        let (x, y) = (f32::from(position.x) - ox, f32::from(position.y) - oy);
        let gap = gap();
        let ratio = match drag.axis {
            Axis::Row => (x - drag.span.x - gap / 2.) / (drag.span.w - gap).max(1.),
            Axis::Column => (y - drag.span.y - gap / 2.) / (drag.span.h - gap).max(1.),
        };
        let path = drag.path.clone();
        let area = self.pane_area(window);
        self.panes.set_ratio_within(&path, ratio, area, gap, MIN_PANE);
        true
    }

    /// La zona del panel `pane` (con rectángulo `rect` en el área de paneles) bajo
    /// un punto de la ventana. Solo para cuando no hay guía: la guía se mide contra el
    /// rectángulo del panel en la ventana (`zone_in`).
    pub(super) fn zone_under(&self, rect: Area, position: gpui::Point<Pixels>) -> Zone {
        let (ox, oy) = self.pane_origin();
        zone_at(rect.w, rect.h, f32::from(position.x) - ox - rect.x, f32::from(position.y) - oy - rect.y, self.drop_replaces())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::panes::Axis;
    use gpui::px;

    const AREA: Area = Area { x: 0.0, y: 0.0, w: 1400.0, h: 800.0 };
    const GAP: f32 = 8.0;

    fn two() -> (Panes<String>, usize) {
        let mut panes = Panes::default();
        panes.put(0, Some("a".to_string()));
        let right = panes.split(0, Axis::Row);
        panes.put(right, Some("b".to_string()));
        (panes, right)
    }

    #[test]
    fn la_zona_sale_de_la_posicion() {
        assert_eq!(zone_at(400., 300., 200., 150., true), Zone::Center);
        assert_eq!(zone_at(400., 300., 20., 150., true), Zone::Left);
        assert_eq!(zone_at(400., 300., 390., 150., true), Zone::Right);
        assert_eq!(zone_at(400., 300., 200., 10., true), Zone::Up);
        assert_eq!(zone_at(400., 300., 200., 295., true), Zone::Down);
        // En una esquina gana el borde más cercano (en proporción).
        assert_eq!(zone_at(400., 300., 30., 10., true), Zone::Up);
        assert_eq!(zone_at(400., 300., 10., 40., true), Zone::Left);
        assert_eq!(zone_at(0., 0., 5., 5., true), Zone::Center);
    }

    #[test]
    fn de_fabrica_solo_cuenta_la_mitad() {
        // El centro ya no reemplaza: a la izquierda de la mitad, izquierda.
        assert_eq!(zone_at(400., 300., 199., 150., false), Zone::Left);
        assert_eq!(zone_at(400., 300., 200., 150., false), Zone::Right);
        // Arriba y abajo tampoco: solo el lado.
        assert_eq!(zone_at(400., 300., 20., 2., false), Zone::Left);
        assert_eq!(zone_at(400., 300., 390., 298., false), Zone::Right);
    }

    #[test]
    fn la_zona_se_mide_contra_el_panel_en_la_ventana() {
        use gpui::{point, size};
        // El chat empieza tras la barra lateral (244 px): el panel va de 244 a 844.
        let pane = Bounds::new(point(px(244.), px(0.)), size(px(600.), px(500.)));
        assert_eq!(zone_in(pane, point(px(244. + 100.), px(250.)), false), Zone::Left);
        assert_eq!(zone_in(pane, point(px(244. + 299.), px(250.)), false), Zone::Left);
        assert_eq!(zone_in(pane, point(px(244. + 301.), px(250.)), false), Zone::Right);
        // Sin restar la barra, 344 caía en la mitad derecha (344 > 300): ya no.
        assert_eq!(zone_at(600., 500., 344., 250., false), Zone::Right);
        // Con tres paneles, cuenta el que está bajo el mouse: el de la derecha va de 844 a 1244.
        let right = Bounds::new(point(px(850.), px(0.)), size(px(400.), px(500.)));
        assert_eq!(zone_in(right, point(px(900.), px(10.)), false), Zone::Left);
        assert_eq!(zone_in(right, point(px(1100.), px(10.)), false), Zone::Right);
        // Con la opción de reemplazar, el centro vuelve.
        assert_eq!(zone_in(pane, point(px(244. + 300.), px(250.)), true), Zone::Center);
    }

    fn group(id: u64, keys: &[&str]) -> SavedSplit {
        let mut panes = Panes::default();
        panes.put(0, Some(keys[0].to_string()));
        let mut last = 0;
        for key in &keys[1..] {
            last = panes.split(last, Axis::Row);
            panes.put(last, Some(key.to_string()));
        }
        SavedSplit { id, panes }
    }

    #[test]
    fn elegir_con_la_vista_dividida_segun_la_opcion() {
        let (panes, _) = two();
        // Otra conversación: de fábrica se abre sola; con la opción, en el panel enfocado.
        assert_eq!(plan_pick(&panes, &[], Some("c"), SplitPick::Alone), Pick::Alone);
        assert_eq!(plan_pick(&panes, &[], Some("c"), SplitPick::Replace), Pick::Here);
        assert_eq!(plan_pick(&panes, &[], None, SplitPick::Alone), Pick::Alone);
        // Una que ya está a la vista: se enfoca su panel.
        assert_eq!(plan_pick(&panes, &[], Some("a"), SplitPick::Alone), Pick::Focus);
        // Sin dividir, siempre en su panel.
        let mut single = Panes::default();
        single.put(0, Some("a".to_string()));
        assert_eq!(plan_pick(&single, &[], Some("c"), SplitPick::Alone), Pick::Here);
        // Una de un grupo guardado vuelve ese grupo, aunque se vea sola.
        let groups = [group(7, &["x", "y"]), group(8, &["a", "z"])];
        assert_eq!(plan_pick(&single, &groups, Some("y"), SplitPick::Alone), Pick::Restore(0));
        assert_eq!(plan_pick(&single, &groups, Some("a"), SplitPick::Replace), Pick::Restore(1));
        // La de la vista dividida de ahora gana a la del grupo guardado.
        assert_eq!(plan_pick(&panes, &groups, Some("a"), SplitPick::Alone), Pick::Focus);
    }

    #[test]
    fn los_grupos_guardados_se_ordenan_solos() {
        let alive: Vec<String> = ["a", "b", "c", "x", "y", "z"].iter().map(|k| k.to_string()).collect();
        let (current, _) = two();
        let mut groups = vec![group(1, &["x", "y", "z"]), group(2, &["a", "c"]), group(3, &["x", "q"])];
        prune_groups(&mut groups, &current, &alive);
        // «a» ya está en la vista de ahora: el grupo 2 queda con una y deja de serlo; «q» se cerró.
        assert_eq!(groups.iter().map(|g| g.id).collect::<Vec<_>>(), vec![1]);
        assert_eq!(ordered(&groups[0].panes), vec!["x", "y", "z"]);
        // Con un solo panel, lo que se ve no le quita conversaciones a los grupos.
        let mut single = Panes::default();
        single.put(0, Some("x".to_string()));
        prune_groups(&mut groups, &single, &alive);
        assert_eq!(groups[0].panes.shown().len(), 3);
        // Al cerrarse una, el grupo sigue con las otras dos en su orden.
        let fewer: Vec<String> = alive.iter().filter(|k| *k != "y").cloned().collect();
        prune_groups(&mut groups, &single, &fewer);
        assert_eq!(ordered(&groups[0].panes), vec!["x", "z"]);
    }

    #[test]
    fn la_barra_muestra_los_grupos_en_el_orden_en_que_se_ven() {
        let (mut panes, right) = two();
        let below = panes.split_at(0, Axis::Column, false);
        panes.put(below, Some("c".to_string()));
        assert_eq!(panes.card_in(right), Some("b".to_string()));
        assert_eq!(ordered(&panes), vec!["a", "c", "b"]);
    }

    #[test]
    fn un_panel_entra_con_resorte_solo_si_aparece_dividido() {
        let mut state = PaneState::default();
        // El de una vista de un panel no entra; al dividir, solo el nuevo.
        assert!(!state.enters(0, false));
        assert!(!state.enters(0, true));
        assert!(state.enters(1, true));
        assert!(state.enters(1, true), "sigue entrando hasta terminar");
        // Al volver a un grupo, todos entran.
        state.layout_gen += 1;
        assert!(state.enters(0, true));
    }

    #[test]
    fn la_vista_previa_ocupa_la_mitad_que_se_abre() {
        assert_eq!(zone_preview(400., 300., Zone::Left), Area { x: 0., y: 0., w: 200., h: 300. });
        assert_eq!(zone_preview(400., 300., Zone::Down), Area { x: 0., y: 150., w: 400., h: 150. });
        assert_eq!(zone_preview(400., 300., Zone::Center), Area { x: 0., y: 0., w: 400., h: 300. });
    }

    #[test]
    fn soltar_en_un_borde_parte_el_panel_y_deja_la_nueva_ahi() {
        let (panes, right) = two();
        let Dropped::Place { panes: next, dest } = plan_drop(&panes, right, Zone::Down, Some("c"), AREA, GAP) else {
            panic!("debía caber");
        };
        assert_eq!(next.count(), 3);
        assert_eq!(next.card_in(dest), None);
        assert_eq!(next.card_in(right), Some("b".to_string()));
        let rects = next.layout(AREA, GAP);
        let below = rects.iter().find(|(p, _)| *p == dest).unwrap().1;
        let above = rects.iter().find(|(p, _)| *p == right).unwrap().1;
        assert!(below.y > above.y);
        // Antes: a la izquierda queda el nuevo.
        let Dropped::Place { panes: next, dest } = plan_drop(&panes, right, Zone::Left, None, AREA, GAP) else {
            panic!("debía caber");
        };
        let rects = next.layout(AREA, GAP);
        let new = rects.iter().find(|(p, _)| *p == dest).unwrap().1;
        let old = rects.iter().find(|(p, _)| *p == right).unwrap().1;
        assert!(new.x < old.x);
    }

    #[test]
    fn soltar_en_el_centro_reemplaza_o_intercambia() {
        let (panes, right) = two();
        // Una que no estaba a la vista: se reemplaza al activarla (el panel queda igual).
        let Dropped::Place { panes: next, dest } = plan_drop(&panes, right, Zone::Center, Some("c"), AREA, GAP) else {
            panic!();
        };
        assert_eq!((dest, next.count()), (right, 2));
        // Una que ya estaba en el otro panel: se intercambian.
        let Dropped::Place { panes: next, dest } = plan_drop(&panes, right, Zone::Center, Some("a"), AREA, GAP) else {
            panic!();
        };
        assert_eq!(dest, right);
        assert_eq!(next.card_in(right), Some("a".to_string()));
        assert_eq!(next.card_in(0), Some("b".to_string()));
    }

    #[test]
    fn mover_una_a_otro_borde_no_la_deja_en_dos_paneles() {
        let (panes, right) = two();
        let Dropped::Place { panes: next, dest } = plan_drop(&panes, right, Zone::Down, Some("a"), AREA, GAP) else {
            panic!();
        };
        // Su panel anterior se quitó y el nuevo queda en el borde.
        assert_eq!(next.count(), 2);
        assert_eq!(next.pane_of("a".to_string()), None);
        assert_eq!(next.card_in(dest), None);
        assert_eq!(next.card_in(right), Some("b".to_string()));
    }

    #[test]
    fn soltar_la_misma_en_su_panel_solo_enfoca() {
        let (panes, right) = two();
        assert!(matches!(plan_drop(&panes, right, Zone::Left, Some("b"), AREA, GAP), Dropped::Focus(p) if p == right));
    }

    #[test]
    fn no_se_divide_si_un_panel_queda_bajo_el_minimo() {
        let (panes, right) = two();
        // 1400 de ancho: cada mitad mide ~696, partir una a lo ancho deja 344 y 348: cabe.
        assert!(matches!(plan_drop(&panes, right, Zone::Right, None, AREA, GAP), Dropped::Place { .. }));
        // En una ventana de 900 de ancho cada mitad mide 446: partirla deja 219.
        let narrow = Area { w: 900., ..AREA };
        assert!(matches!(plan_drop(&panes, right, Zone::Right, None, narrow, GAP), Dropped::NoRoom));
        // Y a lo alto: 800 alto -> 396 cada uno; 400 de alto no alcanza para dos de 240.
        let short = Area { h: 450., ..AREA };
        assert!(matches!(plan_drop(&panes, right, Zone::Down, None, short, GAP), Dropped::NoRoom));
    }

    #[test]
    fn al_cerrar_el_activo_pasa_el_vecino_mas_cercano() {
        let mut panes = Panes::default();
        let right = panes.split(0, Axis::Row);
        let below = panes.split(right, Axis::Column);
        assert_eq!(after_close(&panes, right, AREA, GAP), Some(below));
        assert_eq!(after_close(&panes, below, AREA, GAP), Some(right));
        assert!(after_close(&panes, 0, AREA, GAP).is_some());
        let single = Panes::<String>::default();
        assert_eq!(after_close(&single, 0, AREA, GAP), None);
    }

    #[test]
    fn los_botones_de_la_ventana_van_arriba_a_la_derecha() {
        let mut panes = Panes::<String>::default();
        let right = panes.split(0, Axis::Row);
        let below = panes.split(right, Axis::Column);
        assert_eq!(controls_pane(&panes.layout(AREA, GAP)), Some(right));
        assert_ne!(controls_pane(&panes.layout(AREA, GAP)), Some(below));
        assert_eq!(controls_pane(&Panes::<String>::default().layout(AREA, GAP)), Some(0));
    }

    #[test]
    fn el_ancho_decide_cuanto_se_muestra() {
        assert_eq!(fit(320.), Fit::Tight);
        assert_eq!(fit(459.), Fit::Tight);
        assert_eq!(fit(600.), Fit::Narrow);
        assert_eq!(fit(900.), Fit::Wide);
    }
}
