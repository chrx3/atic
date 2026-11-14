//! La cabecera flotante del hilo: una tarjeta abierta (una herramienta, una tanda, un
//! subagente, el razonamiento, un mensaje largo…) cuya cabecera quedó arriba, fuera de la
//! vista, mientras su cuerpo sigue a la vista, deja su título pegado arriba de la lista con
//! «Plegar». Un clic la pliega y deja su cabecera arriba, a la vista: no hay que volver con
//! la rueda hasta ella.
//!
//! `gpui::list` no tiene encabezados pegajosos, así que se arma con lo que la lista ya mide:
//! cada tarjeta abierta anota su caja en el cuadro en que se dibuja (un `canvas`, en el
//! prepaint) y cada fila anota dónde empieza. Al dibujar el hilo se elige la tarjeta con las
//! cajas del cuadro anterior, relativas a su fila, y la posición de ahora de la lista: al rodar
//! la rueda, la cabecera ya sale bien en el mismo cuadro. Si las cajas cambiaron (algo se abrió
//! o llegó texto), una comprobación tras la lista pide un cuadro más, solo cuando la elección
//! cambia: nada continuo.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use std::rc::Rc;

use gpui::{canvas, div, prelude::*, px, AnyElement, App, ClickEvent, ElementId, SharedString, Window};
use gpui_m3::interaction::{interactive, keyboard_focus, state_layer, use_interaction};
use gpui_m3::motion::{entrance, key};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::{Elevation, Exit, Icon, Presence, Spring, Theme};

/// El alto de la cabecera de una tarjeta (`ExpandableCard`) y el de una sin fondo (el
/// razonamiento, la nota de una tarea).
pub(super) const HEAD: f32 = 30.;
pub(super) const HEAD_PLAIN: f32 = 20.;
/// Lo que tapa la cabecera flotante (su alto y su margen): bajo ella todavía tiene que verse
/// algo del cuerpo, si no la tarjeta ya se fue.
const BAR_ROOM: f32 = 44.;
const BAR_H: f32 = 34.;

/// Cómo se pliega una tarjeta: casi todas están abiertas mientras su id está en
/// `CodeView::expanded`; las que abren solas (`inverted`), mientras no está.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Fold {
    pub id: String,
    pub inverted: bool,
    /// Lo que dice la cabecera flotante: el título de la tarjeta.
    pub label: SharedString,
}

impl Fold {
    pub(super) fn new(id: impl Into<String>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), inverted: false, label: label.into() }
    }

    pub(super) fn inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    /// Pliega la tarjeta. Repetirlo no la vuelve a abrir.
    pub(super) fn apply(&self, expanded: &mut HashSet<String>) {
        if self.inverted {
            expanded.insert(self.id.clone());
        } else {
            expanded.remove(&self.id);
        }
    }
}

/// Una tarjeta abierta, con su caja relativa al comienzo de su fila.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Placed {
    pub row: usize,
    pub top: f32,
    pub bottom: f32,
    pub head: f32,
    pub fold: Fold,
}

/// La tarjeta que tiene la cabecera flotante: su fila y dónde empieza dentro de ella.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub row: usize,
    pub top: f32,
    pub fold: Fold,
}

/// La tarjeta que corresponde a la cabecera flotante, con la lista rodada hasta `edge`
/// píxeles dentro de la fila `top_row` (el borde de arriba de lo visible). Es una tarjeta de
/// esa fila con la cabecera entera por encima del borde y el cuerpo todavía visible bajo la
/// cabecera flotante; si hay varias (una herramienta abierta dentro de una tanda abierta),
/// la más interna: la que empieza más abajo.
pub(super) fn pick(cards: &[Placed], top_row: usize, edge: f32) -> Option<Target> {
    cards
        .iter()
        .filter(|card| card.row == top_row && card.top + card.head <= edge && card.bottom > edge + BAR_ROOM)
        .max_by(|a, b| a.top.total_cmp(&b.top).then(b.bottom.total_cmp(&a.bottom)))
        .map(|card| Target { row: card.row, top: card.top, fold: card.fold.clone() })
}

/// Adónde va la lista al plegar desde la cabecera flotante: la cabecera de la tarjeta queda
/// arriba, donde empieza lo visible (bajo el relleno de la lista). Lo que hay antes en la
/// fila no cambia al plegar, así que su lugar dentro de la fila sigue valiendo.
pub(super) fn fold_scroll(target: &Target) -> gpui::ListOffset {
    gpui::ListOffset { item_ix: target.row, offset_in_item: px(target.top.max(0.)) }
}

/// Las cajas de un cuadro, en coordenadas de la ventana, y lo que se eligió al dibujar.
#[derive(Default)]
pub(super) struct Folds {
    rows: HashMap<usize, f32>,
    cards: HashMap<String, (usize, f32, f32, f32, Fold)>,
    /// La tarjeta de la cabecera flotante en el último dibujo.
    pub shown: Option<Target>,
    /// La última que tuvo cabecera: la que se ve mientras sale.
    pub last: Option<Target>,
}

impl Folds {
    /// Las tarjetas anotadas, relativas a su fila.
    fn placed(&self) -> Vec<Placed> {
        self.cards
            .values()
            .filter_map(|(row, top, bottom, head, fold)| {
                let start = *self.rows.get(row)?;
                Some(Placed { row: *row, top: top - start, bottom: bottom - start, head: *head, fold: fold.clone() })
            })
            .collect()
    }

    pub(super) fn decide(&self, top_row: usize, edge: f32) -> Option<Target> {
        pick(&self.placed(), top_row, edge)
    }

    /// Antes de un dibujo nuevo: las cajas se vuelven a anotar.
    pub(super) fn clear_frame(&mut self) {
        self.rows.clear();
        self.cards.clear();
    }
}

thread_local! {
    /// La fila del hilo que se está armando (`thread_row`), para que sus tarjetas se anoten.
    static ROW: RefCell<Option<(Rc<RefCell<Folds>>, usize)>> = const { RefCell::new(None) };
}

/// Arma la fila `row` con `f`; sus tarjetas abiertas se anotan en `folds`. Sin `folds` (la
/// conversación que sale al cambiar), no se anota nada.
pub(super) fn in_row<R>(folds: Option<Rc<RefCell<Folds>>>, row: usize, f: impl FnOnce() -> R) -> R {
    let before = ROW.with(|cell| cell.replace(folds.map(|folds| (folds, row))));
    let result = f();
    ROW.with(|cell| *cell.borrow_mut() = before);
    result
}

/// Una sonda en la fila: anota dónde empieza en el cuadro.
pub(super) fn row_probe(folds: Rc<RefCell<Folds>>, row: usize) -> impl IntoElement {
    canvas(move |bounds, _, _| {
        folds.borrow_mut().rows.insert(row, f32::from(bounds.top()));
    }, |_, _, _, _| {})
    .absolute()
    .size_full()
}

/// Envuelve una tarjeta del hilo. Si está abierta (`fold`), anota su caja en el cuadro;
/// fuera de una fila del hilo la deja como está.
pub(super) fn track(card: AnyElement, fold: Option<Fold>, head: f32) -> AnyElement {
    let Some((folds, row)) = ROW.with(|cell| cell.borrow().clone()) else {
        return card;
    };
    // El envoltorio va siempre (abierta o no): así el árbol no cambia al abrirla.
    let probe = fold.map(|fold| {
        canvas(move |bounds, _, _| {
            let id = fold.id.clone();
            folds.borrow_mut().cards.insert(id, (row, f32::from(bounds.top()), f32::from(bounds.bottom()), head, fold));
        }, |_, _, _, _| {})
        .absolute()
        .size_full()
    });
    div().relative().flex().flex_col().min_w(px(0.)).child(card).children(probe).into_any_element()
}

/// La cabecera flotante: entra con un fundido y una subida corta, y sale con `Presence`.
#[derive(IntoElement)]
pub(super) struct FoldBar {
    id: SharedString,
    target: Option<Target>,
    shown: bool,
    side: f32,
    on_fold: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

impl FoldBar {
    pub(super) fn new(id: SharedString, target: Option<Target>, shown: bool, side: f32, on_fold: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self { id, target, shown, side, on_fold: Rc::new(on_fold) }
    }
}

impl RenderOnce for FoldBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let holder = div().absolute().top(px(6.)).left(px(self.side)).right(px(self.side));
        let Some(target) = self.target else {
            return holder;
        };
        let s = *Theme::of(cx);
        let bar_id = ElementId::from(SharedString::from(format!("{}-bar", self.id)));
        let state = use_interaction(&bar_id, window, cx);
        let radii = gpui::Corners::all(px(BAR_H / 2.));
        // Entra subiendo 6 px: la opacidad con el resorte de efectos y el desplazamiento con
        // el espacial rápido. Su estado vive mientras se dibuja: cada aparición entra de nuevo.
        let fade = entrance(key(&bar_id, "fade"), Spring::EFFECTS, 0., window, cx);
        let rise = entrance(key(&bar_id, "rise"), Spring::SPATIAL_FAST, 0., window, cx);
        let layer = state_layer(&bar_id, &state, radii, s.on_surface, window, cx);
        let on_fold = self.on_fold.clone();
        let bar = interactive(div().id(bar_id.clone()), &state, true, true)
            .relative()
            .top(px((1. - rise) * 6.))
            .opacity(fade.clamp(0., 1.))
            .h(px(BAR_H))
            .pl(px(14.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(BAR_H / 2.))
            .bg(s.surface_container_highest)
            .shadow(Elevation::Level2.shadow(&s))
            .on_click(move |event, window, cx| on_fold(event, window, cx))
            .child(layer)
            .child(div().flex_1().min_w(px(0.)).truncate().type_style(type_scale::LABEL_LARGE).text_color(s.on_surface).child(target.fold.label.clone()))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .type_style(type_scale::LABEL_MEDIUM)
                    .text_color(s.on_surface_variant)
                    .child(crate::i18n::t("pill.code.view.collapse"))
                    .child(Icon::new("chevron-down").size(px(14.)).color(s.on_surface_variant).rotate(PI)),
            );
        // Tab llega a ella y Enter o Espacio la pliegan, con el anillo de foco de M3.
        let bar = keyboard_focus(bar, &bar_id, radii, false, window, cx);
        holder.child(Presence::new(self.id, self.shown).exit(Exit::Rise).child(bar))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, row: usize, top: f32, bottom: f32) -> Placed {
        Placed { row, top, bottom, head: HEAD, fold: Fold::new(id, id.to_string()) }
    }

    #[test]
    fn la_cabecera_flotante_es_de_la_tarjeta_que_cruza_el_borde() {
        let cards = [card("a", 3, 0., 900.), card("b", 4, 0., 400.)];
        // Arriba de todo, la cabecera se ve: no hace falta la flotante.
        assert_eq!(pick(&cards, 3, 0.), None);
        // A medio leer: la cabecera ya se fue y el cuerpo sigue.
        assert_eq!(pick(&cards, 3, 200.).map(|t| t.fold.id), Some("a".into()));
        // Lo que queda bajo la cabecera flotante ya es el final de la tarjeta: se va.
        assert_eq!(pick(&cards, 3, 870.), None);
        // Otra fila arriba: la de esa fila.
        assert_eq!(pick(&cards, 4, 100.).map(|t| t.fold.id), Some("b".into()));
        // Con la cabecera a medio tapar, todavía no.
        assert_eq!(pick(&cards, 3, 20.), None);
    }

    #[test]
    fn entre_tarjetas_anidadas_gana_la_mas_interna() {
        // Una tanda abierta (fila 2) con una herramienta abierta adentro, más abajo.
        let cards = [card("tanda", 2, 0., 1200.), card("herramienta", 2, 300., 800.)];
        // Antes de la herramienta, la tanda.
        assert_eq!(pick(&cards, 2, 150.).map(|t| t.fold.id), Some("tanda".into()));
        // Dentro de la herramienta, ella.
        assert_eq!(pick(&cards, 2, 500.).map(|t| t.fold.id), Some("herramienta".into()));
        // Pasada la herramienta, vuelve la tanda.
        assert_eq!(pick(&cards, 2, 900.).map(|t| t.fold.id), Some("tanda".into()));
    }

    #[test]
    fn las_cajas_se_miden_desde_su_fila() {
        let mut folds = Folds::default();
        folds.rows.insert(5, 100.);
        folds.cards.insert("x".into(), (5, 140., 940., HEAD, Fold::new("x", "x")));
        // Una tarjeta de una fila sin sonda no cuenta.
        folds.cards.insert("y".into(), (6, 0., 2000., HEAD, Fold::new("y", "y")));
        let target = folds.decide(5, 300.).unwrap();
        assert_eq!((target.row, target.top, target.fold.id.as_str()), (5, 40., "x"));
        assert_eq!(folds.decide(6, 300.), None);
        folds.clear_frame();
        assert_eq!(folds.decide(5, 300.), None);
    }

    #[test]
    fn al_plegar_la_cabecera_queda_arriba() {
        let target = Target { row: 7, top: 260., fold: Fold::new("t", "t") };
        let offset = fold_scroll(&target);
        assert_eq!((offset.item_ix, f32::from(offset.offset_in_item)), (7, 260.));
        // Una tarjeta al comienzo de su fila.
        let offset = fold_scroll(&Target { top: 0., ..target });
        assert_eq!(f32::from(offset.offset_in_item), 0.);
    }

    #[test]
    fn plegar_dos_veces_no_la_abre() {
        let mut expanded: HashSet<String> = ["a".to_string()].into();
        let fold = Fold::new("a", "a");
        fold.apply(&mut expanded);
        fold.apply(&mut expanded);
        assert!(!expanded.contains("a"));
        // Las que abren solas se pliegan anotándolas.
        let auto = Fold::new("b", "b").inverted(true);
        auto.apply(&mut expanded);
        auto.apply(&mut expanded);
        assert!(expanded.contains("b"));
    }
}
