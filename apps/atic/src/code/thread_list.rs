//! El hilo de una conversación como lista virtual de GPUI (`gpui::list`): solo se
//! arman y miden las partes que se ven (más un margen), así que una conversación de
//! cientos de herramientas cuesta lo mismo que una corta. Antes se dibujaba entera en
//! cada cuadro y, con la animación de «Trabajando…», el hilo de la ventana (el mismo
//! del notch) quedaba ocupado casi todo el tiempo.
//!
//! La lista va anclada abajo (`ListAlignment::Bottom`), como un chat: mientras se está
//! al final, lo nuevo la empuja; al subir con la rueda se queda donde se dejó y, al
//! volver al final, sigue otra vez. Cada conversación tiene su lista.
//!
//! Sobre la lista van dos cosas: el minimapa (una marca por mensaje tuyo, en el margen
//! izquierdo: un clic salta a ese mensaje y arrastrándolo se recorren todos) y el botón
//! «Citar», que aparece al seleccionar texto y lo pone como cita en la caja.

use std::ops::Range;

use gpui::{
    div, list, prelude::*, px, AnyElement, Bounds, ClickEvent, Context, DragMoveEvent, ListAlignment, ListOffset, ListState, MouseButton, Pixels, Render, SharedString, Subscription,
    Window,
};

use super::chat::{Chat, Item};
use super::style::{t, Style};
use super::CodeView;
use crate::i18n::t as tr;

/// Cuánto se arma por encima y por debajo de lo visible.
const OVERDRAW: f32 = 800.;

/// Desde cuántos mensajes tuyos aparece el minimapa: con menos no hace falta.
const MINIMAP_MIN: usize = 3;
/// El margen mínimo a los lados para que el minimapa no pise el texto.
const MINIMAP_SIDE: f32 = 20.;
/// Lo que pide cada marca del minimapa; con muchas, se reparten el alto que haya.
const MARK_H: f32 = 10.;
/// El aire arriba y abajo de la franja del minimapa, y el de la píldora que envuelve las marcas.
const RAIL_PAD_Y: f32 = 24.;
const PILL_PAD_Y: f32 = 6.;
/// Desde cuánto por encima del final aparece «Ir al final».
const AWAY_FROM_END: f32 = 240.;

/// ¿La lista quedó lejos de su final (se subió a leer)?
fn far_from_end(state: &ListState) -> bool {
    let max = state.max_offset_for_scrollbar().height;
    let at = -state.scroll_px_offset_for_scrollbar().y;
    f32::from(max - at) > AWAY_FROM_END
}

/// Lo más largo que se cita de una vez.
const QUOTE_MAX_CHARS: usize = 4000;

/// Lo que va en cada fila de la lista.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    /// El aviso de un chat suelto todavía vacío.
    Loose,
    /// Una parte de la conversación (`Chat::items[i]`).
    Item(usize),
    /// Una tanda ya cerrada de herramientas seguidas (`Chat::items[a..=b]`), plegada en una
    /// sola tarjeta con su resumen (ver `super::tools::tool_group`).
    Tools(usize, usize),
    /// Lo que va siempre al final: el modelo del próximo mensaje, los permisos y «Trabajando…».
    Footer,
}

/// Cuántas herramientas seguidas hacen falta para plegarlas en una tarjeta.
const TOOLS_GROUP_MIN: usize = 3;

/// Las filas de una conversación: sus partes visibles y el pie. Las tandas de al menos
/// `TOOLS_GROUP_MIN` herramientas terminadas y seguidas se pliegan en una fila; mientras
/// la tanda sigue abierta (nada la cierra todavía: es lo último de la conversación), cada
/// herramienta va por su lado para no ver cómo se pliegan a mitad de turno.
pub(super) fn rows_of(chat: &Chat, loose_empty: bool) -> Vec<Row> {
    let mut rows = Vec::with_capacity(chat.items.len() + 2);
    if loose_empty {
        rows.push(Row::Loose);
    }
    let shown: Vec<usize> = (0..chat.items.len()).filter(|&i| !chat.hidden(i)).collect();
    let groupable = |item: usize| matches!(chat.items.get(item), Some(Item::Tool(tool)) if super::tools::is_groupable(tool));
    let mut at = 0;
    while at < shown.len() {
        if groupable(shown[at]) {
            let mut end = at;
            while end + 1 < shown.len() && groupable(shown[end + 1]) {
                end += 1;
            }
            let closed = end + 1 < shown.len();
            if closed && end - at + 1 >= TOOLS_GROUP_MIN {
                rows.push(Row::Tools(shown[at], shown[end]));
                at = end + 1;
                continue;
            }
        }
        rows.push(Row::Item(shown[at]));
        at += 1;
    }
    rows.push(Row::Footer);
    rows
}

/// Qué tramo de la lista cambió de `old` a `new`, como lo pide `ListState::splice`: desde
/// la primera fila distinta hasta el final de `old`, reemplazado por las filas nuevas.
/// `None` si son iguales. Las filas que siguen iguales conservan su alto medido.
pub(super) fn splice_of(old: &[Row], new: &[Row]) -> Option<(Range<usize>, usize)> {
    let same = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    (same < old.len() || same < new.len()).then(|| (same..old.len(), new.len() - same))
}

/// La lista de una conversación y las filas con que se armó la última vez.
pub(super) struct ThreadList {
    pub state: ListState,
    rows: Vec<Row>,
    /// El texto elegido en la conversación: va de un mensaje al otro (`gpui_m3::SelectionScope`).
    selection: gpui::Entity<gpui_m3::TextSelection>,
    /// Lo seleccionado la última vez que cambió la selección. «Citar» lo lee de aquí: al
    /// pulsarlo, la selección ya se limpió (un clic fuera del texto la quita).
    quote: String,
    /// La marca del minimapa que se dibujó resaltada.
    mark: usize,
    /// La marca a la que se llegó arrastrando el minimapa, mientras dura el arrastre.
    scrub: Option<usize>,
    /// Se subió lejos del final: se ve «Ir al final».
    away: bool,
    /// Avisa a la vista cuando cambia la selección.
    _watch: Subscription,
}

impl ThreadList {
    fn new(key: &str, cx: &mut Context<CodeView>) -> Self {
        let selection = gpui_m3::TextSelection::shared(cx);
        let owner = key.to_string();
        // Solo se redibuja la vista cuando aparece o desaparece la selección: no a cada
        // movimiento del arrastre.
        let watch = cx.observe(&selection, move |view, selection, cx| {
            let text = selection.read(cx).selected_text();
            let appeared = {
                let mut lists = view.lists.borrow_mut();
                let Some(list) = lists.get_mut(&owner) else {
                    return;
                };
                let appeared = list.quote.is_empty() != text.is_empty();
                list.quote = text;
                appeared
            };
            if appeared {
                cx.notify();
            }
        });
        let state = ListState::new(0, ListAlignment::Bottom, px(OVERDRAW));
        // Al rodar la lista, la marca resaltada del minimapa sigue a lo que se lee. Se
        // difiere: el aviso puede llegar mientras la vista está en uso.
        let view = cx.weak_entity();
        let owner = key.to_string();
        state.set_scroll_handler(move |_, _, cx| {
            let (view, owner) = (view.clone(), owner.clone());
            cx.defer(move |cx| {
                let _ = view.update(cx, |view, cx| {
                    if view.mark_moved(&owner) || view.away_changed(&owner) {
                        cx.notify();
                    }
                });
            });
        });
        Self { state, rows: Vec::new(), selection, quote: String::new(), mark: 0, scrub: None, away: false, _watch: watch }
    }

    /// Pone las filas de ahora; solo se vuelven a medir las que cambiaron.
    fn update(&mut self, rows: Vec<Row>) {
        if let Some((range, count)) = splice_of(&self.rows, &rows) {
            self.state.splice(range, count);
        }
        self.rows = rows;
    }
}

impl CodeView {
    /// El hilo de la conversación `chat` como lista virtual, con `side` de margen a los
    /// lados. El ancho de lectura lo pone el ajuste «Ancho del chat» (`style::chat_width`).
    pub(super) fn thread_list(&self, chat: &Chat, loose_empty: bool, side: f32, cx: &mut Context<Self>) -> AnyElement {
        let rows = rows_of(chat, loose_empty);
        let dragging = cx.has_active_drag();
        let (state, selection, marks, active, quoting, scrub, away) = {
            let mut lists = self.lists.borrow_mut();
            let list = lists.entry(chat.key.clone()).or_insert_with(|| ThreadList::new(&chat.key, cx));
            list.update(rows);
            let marks = user_marks(chat, &list.rows);
            let active = active_mark(&marks, list.state.logical_scroll_top().item_ix);
            list.mark = active.unwrap_or(0);
            // Si ya no se arrastra nada, el arrastre del minimapa terminó (soltar fuera de la franja).
            if !dragging {
                list.scrub = None;
            }
            list.away = far_from_end(&list.state);
            (list.state.clone(), list.selection.clone(), marks, active, !list.quote.is_empty(), list.scrub, list.away)
        };
        let key = chat.key.clone();
        let render = cx.processor(move |view: &mut CodeView, ix: usize, _: &mut Window, cx: &mut Context<CodeView>| {
            view.thread_row(&key, ix, side, cx)
        });
        let thread = list(state, render).size_full().pt(px(if super::view::expressive() { 20. } else { 12. })).pb(px(24.));
        let thread = gpui_m3::selection_root(&selection, div().size_full(), cx).child(thread);
        let minimap = if side >= MINIMAP_SIDE { self.minimap(chat, &marks, active, scrub, away, cx) } else { None };
        div()
            .relative()
            .size_full()
            .child(thread)
            .when_some(minimap, |el, minimap| el.child(minimap))
            .when(quoting, |el| el.child(self.quote_button(&chat.key, cx)))
            // «Citar» gana el lugar mientras haya texto elegido. «Ir al final» está siempre en el
            // árbol para entrar y salir con `Presence` en vez de aparecer de golpe.
            .child(self.jump_button(&chat.key, away && !quoting, cx))
            .into_any_element()
    }

    /// El minimapa: una marca por mensaje tuyo, en el margen izquierdo. La del mensaje que se
    /// está leyendo va resaltada; un clic lleva la lista hasta ese mensaje y, arrastrando por
    /// la franja, se recorren todos con un globo que dice cuál es. Las marcas viven sobre una
    /// píldora que solo se ve al acercar el cursor o arrastrar: así no distrae al leer.
    ///
    /// La píldora y el globo flotan sobre el texto, así que su base es `raised` (opaca en
    /// Formal y en Expressive, 92 % en Liquid Glass) y no `control`; el texto del globo es
    /// `text`, que contrasta con `raised` en los tres estilos y en claro y oscuro.
    fn minimap(&self, chat: &Chat, marks: &[Mark], active: Option<usize>, scrub: Option<usize>, away: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        if marks.len() < MINIMAP_MIN {
            return None;
        }
        let tokens = t();
        // Las marcas sin resaltar: `faint` se pierde sobre el fondo claro de Formal y de
        // Liquid Glass (2,5:1 y menos), así que ahí van en `muted`. En oscuro y en
        // Expressive `faint` ya contrasta, y deja al acento destacarse.
        let idle = if tokens.light && tokens.style != Style::Expressive { tokens.muted } else { tokens.faint };
        let scrubbing = scrub.is_some();
        let lift = || vec![gpui::BoxShadow { color: tokens.shadow, offset: gpui::point(px(0.), px(4.)), blur_radius: px(14.), spread_radius: px(0.) }];
        let mut column = div().h(px(marks.len() as f32 * MARK_H)).min_h(px(0.)).flex_shrink().flex().flex_col();
        for (n, mark) in marks.iter().enumerate() {
            let on = active == Some(n);
            let snippet: SharedString = match chat.items.get(mark.item) {
                Some(Item::User { text, .. }) => mark_snippet(text),
                _ => String::new(),
            }
            .into();
            let (key, row) = (chat.key.clone(), mark.row);
            let bubble = (scrub == Some(n)).then(|| {
                div()
                    .absolute()
                    .left(px(22.))
                    .top(px(-10.))
                    .h(px(24.))
                    .max_w(px(280.))
                    .px(px(11.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(tokens.raised)
                    .border_1()
                    .border_color(tokens.border)
                    .shadow(lift())
                    .text_size(px(12.))
                    .text_color(tokens.text)
                    .whitespace_nowrap()
                    .truncate()
                    .child(snippet.clone())
            });
            column = column.child(
                div()
                    .id(("minimap-mark", n))
                    .relative()
                    .flex_1()
                    .min_h(px(3.))
                    .flex()
                    .items_center()
                    .justify_start()
                    .cursor_pointer()
                    .when(!scrubbing, |el| el.tooltip(crate::hover::tip_text(snippet.clone())))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.reveal_row(&key, row);
                        cx.notify();
                    }))
                    .child(div().h(px(if on { 3. } else { 2. })).w(px(if on { 14. } else { 8. })).rounded_full().bg(if on { tokens.accent } else { idle }))
                    .children(bubble),
            );
        }
        let pill = div()
            .flex()
            .flex_col()
            .flex_shrink()
            .max_h_full()
            .py(px(PILL_PAD_Y))
            .px(px(3.))
            .rounded_full()
            .border_1()
            .border_color(gpui::transparent_black())
            .group_hover("minimap", move |style| style.bg(tokens.raised).border_color(tokens.border).shadow(lift()))
            .when(scrubbing, |el| el.bg(tokens.raised).border_color(tokens.border).shadow(lift()))
            .child(column)
            // Al pie de las marcas, lejos del final: una flecha que baja hasta lo último.
            .when(away, |el| {
                let key = chat.key.clone();
                el.child(
                    div()
                        .id("minimap-end")
                        .mt(px(4.))
                        .h(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|el| el.bg(tokens.hover))
                        .tooltip(crate::hover::tip(tr("pill.code.view.jumpToEnd")))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.jump_to_end(&key, cx)))
                        .child(gpui::svg().path("icons/arrow-down.svg").size(px(12.)).text_color(tokens.accent)),
                )
            });
        let (scrub_key, up_key, out_key) = (chat.key.clone(), chat.key.clone(), chat.key.clone());
        Some(
            div()
                .id("minimap")
                .group("minimap")
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(2.))
                .w(px(18.))
                .py(px(RAIL_PAD_Y))
                .flex()
                .flex_col()
                .justify_center()
                .on_drag(MinimapDrag, |_, _, _, cx| cx.new(|_| MinimapGhost))
                .on_drag_move::<MinimapDrag>(cx.listener(move |view, event: &DragMoveEvent<MinimapDrag>, _, cx| {
                    view.scrub_to(&scrub_key, event.bounds, event.event.position.y, cx);
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(move |view, _, _, cx| view.end_scrub(&up_key, cx)))
                .on_mouse_up_out(MouseButton::Left, cx.listener(move |view, _, _, cx| view.end_scrub(&out_key, cx)))
                .child(pill)
                .into_any_element(),
        )
    }

    /// Arrastrando por la franja del minimapa (`bounds`), el cursor en `y` apunta a una marca:
    /// la lista salta a su mensaje y el globo dice cuál es.
    fn scrub_to(&mut self, key: &str, bounds: Bounds<Pixels>, y: Pixels, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter().find(|c| c.key == key) else {
            return;
        };
        let marks = match self.lists.borrow().get(key) {
            Some(list) => user_marks(chat, &list.rows),
            None => return,
        };
        let count = marks.len();
        if count == 0 {
            return;
        }
        // Las marcas ocupan, centradas, lo que piden o lo que cabe entre los aires de la franja.
        let height = f32::from(bounds.size.height);
        let column = (count as f32 * MARK_H).min((height - 2. * (RAIL_PAD_Y + PILL_PAD_Y)).max(MARK_H));
        let top = f32::from(bounds.origin.y) + (height - column) / 2.;
        let at = scrub_index(f32::from(y) - top, column, count);
        let changed = match self.lists.borrow_mut().get_mut(key) {
            Some(list) => {
                let changed = list.scrub != Some(at);
                list.scrub = Some(at);
                changed
            }
            None => false,
        };
        if changed {
            self.reveal_row(key, marks[at].row);
            cx.notify();
        }
    }

    /// Terminó el arrastre del minimapa: el globo se va.
    fn end_scrub(&mut self, key: &str, cx: &mut Context<Self>) {
        let ended = self.lists.borrow_mut().get_mut(key).is_some_and(|list| list.scrub.take().is_some());
        if ended {
            cx.notify();
        }
    }

    /// El botón «Citar», abajo al centro de la lista mientras haya texto seleccionado. Actúa
    /// al presionar y no al soltar: al presionar, el clic fuera del texto limpia la selección
    /// y el botón desaparece antes de poder soltarse.
    ///
    /// Flota sobre el texto del chat, así que su base es `raised` (opaca en Formal y en
    /// Expressive, 92 % en Liquid Glass) y no `control` (10 % de blanco en vidrio: dejaría
    /// ver el texto de abajo). Encima va el par tonal `accent_soft` / `on_accent_soft`, que
    /// cada estilo y modo ya define para contrastar.
    fn quote_button(&self, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let key = key.to_string();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(10.))
            .flex()
            .justify_center()
            .child(
                div()
                    .rounded_full()
                    .bg(tokens.raised)
                    .border_1()
                    .border_color(tokens.border)
                    .shadow(vec![gpui::BoxShadow { color: tokens.shadow, offset: gpui::point(px(0.), px(4.)), blur_radius: px(14.), spread_radius: px(0.) }])
                    .child(
                        div()
                            .id("quote-selection")
                            .h(px(30.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(tokens.accent_soft)
                            .hover(|el| el.bg(tokens.sel))
                            .text_size(px(12.5))
                            .text_color(tokens.on_accent_soft)
                            .tooltip(crate::hover::tip(tr("pill.code.view.quoteTip")))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |view, _, window, cx| {
                                    view.quote_selection(&key, window, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(tr("pill.code.view.quote")),
                    ),
            )
            .into_any_element()
    }

    /// Pone lo seleccionado en `key` como cita al final de la caja y la enfoca.
    fn quote_selection(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.lists.borrow().get(key).map(|list| list.quote.clone()).unwrap_or_default();
        let Some(quote) = quote_of(&text) else {
            return;
        };
        let current = self.composer.read(cx).text().trim_end().to_string();
        let joined = if current.is_empty() { format!("{quote}\n\n") } else { format!("{current}\n\n{quote}\n\n") };
        self.composer.update(cx, |area, cx| area.set_text_at(&joined, joined.len(), cx));
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// ¿Cambió la marca resaltada del minimapa de `key` desde la última vez que se dibujó?
    /// Se cruzó el umbral de «Ir al final» (hacia arriba o de vuelta al final).
    fn away_changed(&self, key: &str) -> bool {
        let mut lists = self.lists.borrow_mut();
        let Some(list) = lists.get_mut(key) else {
            return false;
        };
        let away = far_from_end(&list.state);
        std::mem::replace(&mut list.away, away) != away
    }

    /// El botón flotante «Ir al final», sobre la caja de texto: baja la lista hasta lo último
    /// y vuelve a seguir lo nuevo.
    fn jump_button(&self, key: &str, shown: bool, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let presence = SharedString::from(format!("jump-to-end-{key}"));
        let key = key.to_string();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(10.))
            .flex()
            .justify_center()
            .child(gpui_m3::Presence::new(presence, shown).exit(gpui_m3::Exit::Sink).child(
                div()
                    .id("jump-to-end")
                    .h(px(32.))
                    .pl(px(12.))
                    .pr(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(tokens.raised)
                    .border_1()
                    .border_color(tokens.border)
                    .shadow(vec![gpui::BoxShadow { color: tokens.shadow, offset: gpui::point(px(0.), px(4.)), blur_radius: px(14.), spread_radius: px(0.) }])
                    .hover(|el| el.bg(tokens.hover))
                    .text_size(px(12.5))
                    .text_color(tokens.text)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.jump_to_end(&key, cx)))
                    .child(gpui::svg().path("icons/arrow-down.svg").size(px(14.)).text_color(tokens.accent))
                    .child(tr("pill.code.view.jumpToEnd")),
            ))
            .into_any_element()
    }

    fn jump_to_end(&mut self, key: &str, cx: &mut Context<Self>) {
        self.scroll_to_end(key);
        if let Some(list) = self.lists.borrow_mut().get_mut(key) {
            list.away = false;
        }
        cx.notify();
    }

    fn mark_moved(&self, key: &str) -> bool {
        let Some(chat) = self.chats.iter().find(|c| c.key == key) else {
            return false;
        };
        let lists = self.lists.borrow();
        let Some(list) = lists.get(key) else {
            return false;
        };
        let marks = user_marks(chat, &list.rows);
        marks.len() >= MINIMAP_MIN && active_mark(&marks, list.state.logical_scroll_top().item_ix).unwrap_or(0) != list.mark
    }

    /// Una fila de la lista de `key`.
    fn thread_row(&mut self, key: &str, ix: usize, side: f32, cx: &mut Context<Self>) -> AnyElement {
        let (row, selection) = match self.lists.borrow().get(key) {
            Some(list) => (list.rows.get(ix).copied(), Some(list.selection.clone())),
            None => (None, None),
        };
        let Some(chat) = self.chats.iter().find(|c| c.key == key) else {
            return div().into_any_element();
        };
        let content = match row {
            Some(Row::Item(index)) => match chat.items.get(index) {
                Some(item) => {
                    let flagged = self.is_flagged(chat, index);
                    let fresh = index + super::view::ENTER_MAX >= chat.items.len();
                    self.item(chat, index, item, flagged, fresh, cx)
                }
                None => div().into_any_element(),
            },
            Some(Row::Tools(first, last)) => self.tool_group(chat, first, last, cx),
            Some(Row::Footer) => self.thread_footer(chat, cx),
            Some(Row::Loose) => super::view::loose_notice(),
            None => div().into_any_element(),
        };
        // Cada fila es un tramo de la selección, en el orden de lectura.
        let owner = match row {
            Some(Row::Item(index)) => index as u64,
            Some(Row::Tools(first, _)) => first as u64,
            Some(Row::Footer) => u64::MAX - 1,
            _ => u64::MAX,
        };
        let content = match selection {
            Some(selection) => gpui_m3::SelectionScope::new(selection, owner, content).into_any_element(),
            None => content,
        };
        // La parte que coincide con la búsqueda (Ctrl+F) se tiñe con el acento. El relleno y
        // el margen negativo se anulan: el texto no se mueve al teñirse.
        let mark = match row {
            Some(Row::Item(index)) => self.find_highlight(key, index),
            Some(Row::Tools(first, last)) => (first..=last).find_map(|index| self.find_highlight(key, index)),
            _ => None,
        };
        let content = match mark {
            Some(color) => div().p(px(6.)).m(px(-6.)).rounded(px(10.)).bg(color).child(content).into_any_element(),
            None => content,
        };
        let gap = if super::view::expressive() { 12. } else { 16. };
        div().w_full().px(px(side)).pb(px(gap)).child(content).into_any_element()
    }

    /// Lleva la lista de `key` al final y la deja siguiéndolo.
    pub(super) fn scroll_to_end(&self, key: &str) {
        if let Some(list) = self.lists.borrow().get(key) {
            // Al llegar al tope de abajo, la lista vuelve a anclarse al final sola.
            list.state.scroll_by(px(1.0e7));
        }
    }

    /// La fila de la lista de `key` que muestra la parte `index`.
    pub(super) fn row_of_item(&self, key: &str, index: usize) -> Option<usize> {
        self.lists.borrow().get(key)?.rows.iter().position(|row| match *row {
            Row::Item(item) => item == index,
            Row::Tools(first, last) => (first..=last).contains(&index),
            _ => false,
        })
    }

    /// La primera fila que se ve arriba en la lista de `key`.
    pub(super) fn top_row(&self, key: &str) -> usize {
        self.lists.borrow().get(key).map_or(0, |list| list.state.logical_scroll_top().item_ix)
    }

    /// Lleva la lista de `key` hasta que se vea la fila `row`.
    pub(super) fn reveal_row(&self, key: &str, row: usize) {
        if let Some(list) = self.lists.borrow().get(key) {
            list.state.scroll_to(ListOffset { item_ix: row, offset_in_item: px(0.) });
        }
    }

    /// Antes de abrir o cerrar la tarjeta de la parte `index`: que crezca o se encoja hacia
    /// abajo. Pegada al final, la lista sigue al último mensaje y lo que crece empuja todo
    /// hacia arriba (la tarjeta parecía abrirse al revés); se deja quieta donde está. Al
    /// cerrarla desde abajo con su encabezado fuera de la vista, se vuelve a él.
    pub(super) fn hold_scroll(&self, key: &str, index: usize, closing: bool) {
        let Some(row) = self.row_of_item(key, index) else { return };
        let lists = self.lists.borrow();
        let Some(list) = lists.get(key) else { return };
        let state = &list.state;
        let following = state.logical_scroll_top().item_ix >= state.item_count();
        if following {
            // Pasa de «sigue al final» a una posición fija, la misma que se ve ahora.
            let height = state.viewport_bounds().size.height;
            if f32::from(state.max_offset_for_scrollbar().height) <= 0. {
                return;
            }
            state.scroll_by(-height);
        }
        if closing {
            let top = state.viewport_bounds().top();
            if state.bounds_for_item(row).is_none_or(|bounds| bounds.top() < top) {
                state.scroll_to(ListOffset { item_ix: row, offset_in_item: px(0.) });
            }
        }
    }

    /// Se olvida de la lista de una conversación cerrada.
    pub(super) fn drop_list(&self, key: &str) {
        self.lists.borrow_mut().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_se_vuelve_a_medir_lo_que_cambio() {
        use Row::*;
        let old = [Item(0), Item(1), Footer];
        // Llega una parte nueva: se reemplaza el pie por la parte y el pie.
        assert_eq!(splice_of(&old, &[Item(0), Item(1), Item(2), Footer]), Some((2..3, 2)));
        // Nada cambió.
        assert_eq!(splice_of(&old, &old), None);
        // Una parte de antes se ocultó (razonamiento vacío): desde ahí.
        assert_eq!(splice_of(&old, &[Item(1), Footer]), Some((0..3, 2)));
        // Lista nueva.
        assert_eq!(splice_of(&[], &[Footer]), Some((0..0, 1)));
    }
}

/// El gesto de arrastrar por el minimapa (no lleva datos: la lista y la marca salen de la franja).
struct MinimapDrag;

/// Lo que sigue al cursor en ese arrastre: nada. El globo de la marca ya dice dónde se está.
struct MinimapGhost;

impl Render for MinimapGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// La marca `0..count` que cae a `offset` px del borde de arriba de una columna de `height` px.
/// Fuera de la columna, la primera o la última.
fn scrub_index(offset: f32, height: f32, count: usize) -> usize {
    if count == 0 || height <= 0. || offset.is_nan() {
        return 0;
    }
    (((offset / height) * count as f32).floor().max(0.) as usize).min(count - 1)
}

/// Una marca del minimapa: un mensaje tuyo, en su fila de la lista.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mark {
    pub row: usize,
    pub item: usize,
}

/// Los mensajes del usuario entre las filas de la lista, en orden.
fn user_marks(chat: &Chat, rows: &[Row]) -> Vec<Mark> {
    rows.iter()
        .enumerate()
        .filter_map(|(row, kind)| match kind {
            Row::Item(item) if matches!(chat.items.get(*item), Some(Item::User { .. })) => Some(Mark { row, item: *item }),
            _ => None,
        })
        .collect()
}

/// La marca del mensaje que se está leyendo: la última cuya fila no pasa de la primera fila
/// visible (o la primera, si arriba solo hay avisos).
fn active_mark(marks: &[Mark], top_row: usize) -> Option<usize> {
    if marks.is_empty() {
        return None;
    }
    Some(marks.iter().rposition(|mark| mark.row <= top_row).unwrap_or(0))
}

/// El mensaje en una línea, para el globo de su marca.
fn mark_snippet(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return tr("pill.code.view.minimapEmpty").to_string();
    }
    let mut short: String = line.chars().take(90).collect();
    if line.chars().count() > 90 {
        short.push('…');
    }
    short
}

/// El texto seleccionado como cita de Markdown (`> línea`); `None` si no hay nada que citar.
/// Se corta en `QUOTE_MAX_CHARS`.
fn quote_of(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut body: String = text.chars().take(QUOTE_MAX_CHARS).collect();
    if text.chars().count() > QUOTE_MAX_CHARS {
        body.push('…');
    }
    let lines: Vec<String> = body.lines().map(|line| if line.trim().is_empty() { ">".to_string() } else { format!("> {}", line.trim_end()) }).collect();
    Some(lines.join("\n"))
}

#[cfg(test)]
mod minimap_tests {
    use super::*;

    fn marks(rows: &[usize]) -> Vec<Mark> {
        rows.iter().enumerate().map(|(item, row)| Mark { row: *row, item }).collect()
    }

    #[test]
    fn arrastrar_el_minimapa_elige_la_marca_bajo_el_cursor() {
        // Cuatro marcas en 40 px: cada una pide 10.
        assert_eq!(scrub_index(0., 40., 4), 0);
        assert_eq!(scrub_index(9.9, 40., 4), 0);
        assert_eq!(scrub_index(10., 40., 4), 1);
        assert_eq!(scrub_index(39.9, 40., 4), 3);
        // Arriba o abajo de la columna se queda en la primera o la última.
        assert_eq!(scrub_index(-30., 40., 4), 0);
        assert_eq!(scrub_index(500., 40., 4), 3);
        // Casos raros: sin marcas o sin alto.
        assert_eq!(scrub_index(5., 40., 0), 0);
        assert_eq!(scrub_index(5., 0., 4), 0);
        assert_eq!(scrub_index(f32::NAN, 40., 4), 0);
    }

    #[test]
    fn la_marca_activa_es_la_ultima_que_no_pasa_de_la_fila_de_arriba() {
        let marks = marks(&[1, 5, 9]);
        assert_eq!(active_mark(&marks, 0), Some(0));
        assert_eq!(active_mark(&marks, 1), Some(0));
        assert_eq!(active_mark(&marks, 4), Some(0));
        assert_eq!(active_mark(&marks, 5), Some(1));
        assert_eq!(active_mark(&marks, 30), Some(2));
        assert_eq!(active_mark(&[], 3), None);
    }

    #[test]
    fn la_cita_antepone_el_signo_a_cada_linea() {
        assert_eq!(quote_of("uno\n\ndos  \n"), Some("> uno\n>\n> dos".to_string()));
        assert_eq!(quote_of("   \n "), None);
    }

    #[test]
    fn la_cita_larga_se_corta() {
        let quote = quote_of(&"a".repeat(QUOTE_MAX_CHARS + 50)).unwrap();
        assert_eq!(quote.chars().count(), 2 + QUOTE_MAX_CHARS + 1);
        assert!(quote.ends_with('…'));
    }
}
