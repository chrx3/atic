//! Varias conversaciones a la vez, en filas y columnas. Se arrastra una de la barra
//! lateral a un panel: soltarla en un borde lo parte en esa dirección y la deja en
//! la mitad nueva; en el centro reemplaza la conversación de ese panel.
//!
//! El árbol de paneles es el del Mando (`space::panes`), con la clave de la
//! conversación en cada hoja. Solo un panel es el activo (`panes.focused`): suyos
//! son la caja de texto, los menús y el panel derecho, y `CodeView::active` es la
//! conversación que tiene. Los demás se ven enteros, con sus permisos, y un clic en
//! cualquier parte de ellos los vuelve el activo. El borrador de cada conversación
//! se guarda al cambiar de panel.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{div, prelude::*, px, Bounds, Context, Pixels, Render, SharedString, Window};

use super::{CodeView, SessionInfo};
use crate::space::panes::{Axis, Divider, Panes};
use crate::space::Area;

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
}

impl DragChip {
    pub fn new(title: SharedString) -> Self {
        Self { title }
    }
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = super::style::t();
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
            Zone::Left => "Abrir a la izquierda",
            Zone::Right => "Abrir a la derecha",
            Zone::Up => "Abrir arriba",
            Zone::Down => "Abrir abajo",
            Zone::Center => "Reemplazar esta conversación",
        }
    }
}

/// Qué tan adentro del panel (de 0 a 1) empieza el centro.
const EDGE: f32 = 0.25;

/// La zona bajo el punto `(x, y)`, relativo a la esquina de un panel de `w` × `h`.
pub fn zone_at(w: f32, h: f32, x: f32, y: f32) -> Zone {
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
}

impl Default for PaneState {
    fn default() -> Self {
        Self { bounds: Rc::new(Cell::new(None)), hint: None, resizing: None }
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
        el.on_drag(drag, move |drag, _, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.dragging_chat = true;
                cx.notify();
            });
            cx.new(|_| DragChip::new(drag.title.clone()))
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
                self.show_toast(format!("No cabe otro panel ahí: cada uno necesita al menos {} × {} px", MIN_PANE.0, MIN_PANE.1), cx);
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

    /// Guarda lo escrito en la caja como borrador de la conversación activa.
    pub(super) fn save_draft(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.active.clone() {
            let text = self.composer.read(cx).text().to_string();
            if text.is_empty() {
                self.drafts.remove(&key);
            } else {
                self.drafts.insert(key, text);
            }
        }
    }

    /// Pone en la caja el borrador de la conversación activa (o la deja vacía).
    pub(super) fn restore_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.active.as_ref().and_then(|key| self.drafts.remove(key)).unwrap_or_default();
        self.composer.update(cx, |area, cx| area.set_text(&text, cx));
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
    /// un punto de la ventana.
    pub(super) fn zone_under(&self, rect: Area, position: gpui::Point<Pixels>) -> Zone {
        let (ox, oy) = self.pane_origin();
        zone_at(rect.w, rect.h, f32::from(position.x) - ox - rect.x, f32::from(position.y) - oy - rect.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::panes::Axis;

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
        assert_eq!(zone_at(400., 300., 200., 150.), Zone::Center);
        assert_eq!(zone_at(400., 300., 20., 150.), Zone::Left);
        assert_eq!(zone_at(400., 300., 390., 150.), Zone::Right);
        assert_eq!(zone_at(400., 300., 200., 10.), Zone::Up);
        assert_eq!(zone_at(400., 300., 200., 295.), Zone::Down);
        // En una esquina gana el borde más cercano (en proporción).
        assert_eq!(zone_at(400., 300., 30., 10.), Zone::Up);
        assert_eq!(zone_at(400., 300., 10., 40.), Zone::Left);
        assert_eq!(zone_at(0., 0., 5., 5.), Zone::Center);
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
