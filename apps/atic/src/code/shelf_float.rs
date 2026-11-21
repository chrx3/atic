//! La cabecera pegajosa de las repisas de la barra lateral: con «Hechas» o «Pospuestas»
//! abierta y muchas filas, al bajar con la rueda su cabecera («Ocultar hechas») se queda
//! pegada arriba de la lista mientras sus filas sigan a la vista, y al terminar la repisa
//! la empuja hacia arriba, como un encabezado pegajoso. Un clic la pliega y deja la cabecera
//! de verdad en el mismo lugar, sin tener que volver con la rueda hasta ella.
//!
//! GPUI no tiene encabezados pegajosos: se arma como la cabecera flotante del hilo
//! (`fold_bar.rs`). Cada repisa abierta anota su caja en el cuadro en que se dibuja (un
//! `canvas`, en el prepaint), relativa al comienzo del contenido. Al dibujar la barra se
//! elige con las cajas del cuadro anterior y la posición de ahora de la lista: al rodar la
//! rueda la cabecera sale bien en el mismo cuadro. Si las cajas cambiaron, una comprobación
//! tras la lista pide un cuadro más, solo cuando la elección cambia: nada continuo.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::rc::Rc;

use gpui::{canvas, div, point, prelude::*, px, AnyElement, App, ClickEvent, ScrollHandle, SharedString, Window};
use gpui_m3::NavItem;

use super::style::t;

/// El alto de la cabecera de una repisa (un `NavItem` denso).
const HEAD: f32 = 30.;

/// Una repisa abierta, con su caja (cabecera y filas) relativa al comienzo del contenido de
/// la lista y al borde izquierdo de la lista.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Placed {
    pub key: u64,
    pub snoozed: bool,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub width: f32,
    /// Lo que dice su cabecera abierta: «Ocultar hechas».
    pub label: SharedString,
}

/// La repisa que pega su cabecera arriba con la lista rodada `edge` píxeles; `inset` es
/// dónde empieza lo visible dentro de la lista (su borde de arriba). Es la que tiene la
/// cabecera saliendo por arriba y todavía algo de sus filas a la vista.
fn pick(shelves: &[Placed], edge: f32, inset: f32) -> Option<Placed> {
    let line = edge + inset;
    shelves
        .iter()
        .filter(|shelf| shelf.top < line - 0.5 && shelf.bottom > line + 1.)
        .max_by(|a, b| a.top.total_cmp(&b.top))
        .cloned()
}

/// Dónde va la cabecera pegada, desde donde empieza lo visible: arriba de todo o, cuando
/// la repisa termina, subiendo con su última fila.
fn bar_top(shelf: &Placed, edge: f32, inset: f32) -> f32 {
    (shelf.bottom - edge - inset - HEAD).min(0.)
}

/// Adónde se rueda la lista al plegar desde la cabecera pegada: la cabecera de la repisa
/// queda donde estaba la pegada. Lo que hay antes de la repisa no cambia al plegarla.
fn fold_edge(shelf: &Placed, inset: f32) -> f32 {
    (shelf.top - inset).max(0.)
}

#[derive(Default)]
struct Frame {
    boxes: HashMap<(u64, bool), Placed>,
    /// La repisa con cabecera pegada en el último dibujo.
    shown: Option<Placed>,
}

/// La cabecera pegajosa de una lista con scroll de la barra (la de los proyectos o el pie
/// con las repisas de la barra sin carpetas). Se clona barato: los clones comparten estado.
#[derive(Clone)]
pub(super) struct ShelfFloat {
    scroll: ScrollHandle,
    frame: Rc<RefCell<Frame>>,
    inset: f32,
}

impl ShelfFloat {
    pub(super) fn new(inset: f32) -> Self {
        Self { scroll: ScrollHandle::new(), frame: Rc::default(), inset }
    }

    /// El scroll de la lista: va en su `track_scroll`.
    pub(super) fn scroll(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// Cuánto se rodó la lista hacia abajo.
    fn edge(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    /// Antes de armar la lista: elige la repisa con las cajas del cuadro anterior y la
    /// posición de ahora, y las borra (este dibujo las vuelve a anotar).
    pub(super) fn begin(&self) {
        let mut frame = self.frame.borrow_mut();
        let boxes: Vec<Placed> = frame.boxes.values().cloned().collect();
        frame.shown = pick(&boxes, self.edge(), self.inset);
        frame.boxes.clear();
    }

    /// Una sonda en la caja de una repisa abierta: anota dónde quedó en el cuadro.
    pub(super) fn probe(&self, key: u64, snoozed: bool, label: SharedString) -> impl IntoElement {
        let (scroll, frame) = (self.scroll.clone(), self.frame.clone());
        canvas(
            move |bounds, _, _| {
                // El scroll ya puso su caja y su posición antes de dibujar a sus hijos.
                let list = scroll.bounds();
                let origin = f32::from(list.top() + scroll.offset().y);
                let placed = Placed {
                    key,
                    snoozed,
                    top: f32::from(bounds.top()) - origin,
                    bottom: f32::from(bounds.bottom()) - origin,
                    left: f32::from(bounds.left() - list.left()),
                    width: f32::from(bounds.size.width),
                    label: label.clone(),
                };
                frame.borrow_mut().boxes.insert((key, snoozed), placed);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Tras la lista, con las cajas de este cuadro: si la repisa elegida ya no es la que
    /// corresponde (se abrió o cerró algo, cambió el ancho), un cuadro más la corrige.
    pub(super) fn check(&self) -> impl IntoElement {
        let this = self.clone();
        canvas(
            move |_, window, _| {
                let frame = this.frame.borrow();
                let boxes: Vec<Placed> = frame.boxes.values().cloned().collect();
                if pick(&boxes, this.edge(), this.inset) != frame.shown {
                    gpui_m3::motion::request_animation_frame(window);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    /// Pliega desde la cabecera pegada: deja la lista con la cabecera de la repisa donde
    /// estaba la pegada y devuelve cuál plegar (su espacio y si es «Pospuestas»).
    pub(super) fn fold(&self) -> Option<(u64, bool)> {
        let shelf = self.frame.borrow().shown.clone()?;
        let offset = self.scroll.offset();
        self.scroll.set_offset(point(offset.x, px(-fold_edge(&shelf, self.inset))));
        Some((shelf.key, shelf.snoozed))
    }

    /// La cabecera pegada, encima de la lista (va en un contenedor `relative` con ella).
    /// Mismo aspecto que la de la repisa abierta, sobre el fondo de la barra, y recortada a
    /// lo visible de la lista cuando la empuja el final de la repisa.
    pub(super) fn bar(&self, on_fold: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Option<AnyElement> {
        let shelf = self.frame.borrow().shown.clone()?;
        let ink = gpui_m3::Theme::of(cx).on_surface_variant;
        let chevron = gpui_m3::Icon::new("chevron-down").size(px(14.)).color(ink).rotate(PI);
        let id = SharedString::from(format!("shelf-float-{}-{}", u8::from(shelf.snoozed), shelf.key));
        let item = NavItem::new(id, shelf.label.clone()).dense(true).muted(true).trailing(chevron).on_click(on_fold);
        let bar = div().absolute().top(px(bar_top(&shelf, self.edge(), self.inset))).left(px(shelf.left)).w(px(shelf.width)).h(px(HEAD)).bg(t().bg).child(item);
        Some(div().absolute().top(px(self.inset)).left_0().right_0().bottom_0().overflow_hidden().child(bar).into_any_element())
    }
}

/// Las dos listas con scroll de la barra de Expressive que pueden tener repisas.
pub(super) struct SideFloats {
    /// La de los proyectos y los chats sueltos (`side-scroll`).
    pub list: ShelfFloat,
    /// El pie con las repisas de la barra sin carpetas (`side-shelves`), bajo su borde.
    pub dock: ShelfFloat,
}

impl Default for SideFloats {
    fn default() -> Self {
        Self { list: ShelfFloat::new(0.), dock: ShelfFloat::new(1.) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf(key: u64, top: f32, bottom: f32) -> Placed {
        Placed { key, snoozed: false, top, bottom, left: 26., width: 200., label: "Ocultar hechas".into() }
    }

    #[test]
    fn la_cabecera_se_pega_mientras_sus_filas_se_ven() {
        let shelves = [shelf(1, 100., 900.)];
        // Arriba de todo, la cabecera se ve en su lugar.
        assert_eq!(pick(&shelves, 0., 0.), None);
        assert_eq!(pick(&shelves, 100., 0.), None);
        // Empieza a salir: se pega.
        assert_eq!(pick(&shelves, 120., 0.).map(|s| s.key), Some(1));
        assert_eq!(pick(&shelves, 600., 0.).map(|s| s.key), Some(1));
        // Al final, empujada, todavía.
        assert_eq!(pick(&shelves, 880., 0.).map(|s| s.key), Some(1));
        // La repisa ya se fue entera.
        assert_eq!(pick(&shelves, 900., 0.), None);
    }

    #[test]
    fn de_varias_repisas_la_que_cruza_el_borde() {
        // Las Hechas de un proyecto y, más abajo, sus Pospuestas.
        let shelves = [shelf(1, 100., 400.), Placed { snoozed: true, ..shelf(1, 401., 900.) }];
        assert_eq!(pick(&shelves, 200., 0.).map(|s| s.snoozed), Some(false));
        assert_eq!(pick(&shelves, 500., 0.).map(|s| s.snoozed), Some(true));
    }

    #[test]
    fn al_terminar_la_repisa_la_empuja_hacia_arriba() {
        let s = shelf(1, 100., 900.);
        assert_eq!(bar_top(&s, 500., 0.), 0.);
        // Quedan 20 px de la repisa: la cabecera sube 10.
        assert_eq!(bar_top(&s, 880., 0.), -10.);
        // En el pie, bajo su borde de 1 px.
        assert_eq!(bar_top(&s, 500., 1.), 0.);
        assert_eq!(bar_top(&s, 880., 1.), -11.);
    }

    #[test]
    fn al_plegar_la_cabecera_queda_donde_estaba_la_pegada() {
        assert_eq!(fold_edge(&shelf(1, 260., 900.), 0.), 260.);
        assert_eq!(fold_edge(&shelf(1, 260., 900.), 1.), 259.);
        assert_eq!(fold_edge(&shelf(1, 0., 900.), 1.), 0.);
    }
}
