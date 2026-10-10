//! El hilo de una conversación como lista virtual de GPUI (`gpui::list`): solo se
//! arman y miden las partes que se ven (más un margen), así que una conversación de
//! cientos de herramientas cuesta lo mismo que una corta. Antes se dibujaba entera en
//! cada cuadro y, con la animación de «Trabajando…», el hilo de la ventana (el mismo
//! del notch) quedaba ocupado casi todo el tiempo.
//!
//! La lista va anclada abajo (`ListAlignment::Bottom`), como un chat: mientras se está
//! al final, lo nuevo la empuja; al subir con la rueda se queda donde se dejó y, al
//! volver al final, sigue otra vez. Cada conversación tiene su lista.

use std::ops::Range;

use gpui::{div, list, prelude::*, px, AnyElement, Context, ListAlignment, ListOffset, ListState, Window};

use super::chat::Chat;
use super::CodeView;

/// Cuánto se arma por encima y por debajo de lo visible.
const OVERDRAW: f32 = 800.;

/// Lo que va en cada fila de la lista.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    /// El aviso de un chat suelto todavía vacío.
    Loose,
    /// Una parte de la conversación (`Chat::items[i]`).
    Item(usize),
    /// Lo que va siempre al final: el modelo del próximo mensaje, los permisos y «Trabajando…».
    Footer,
}

/// Las filas de una conversación: sus partes visibles y el pie.
pub(super) fn rows_of(chat: &Chat, loose_empty: bool) -> Vec<Row> {
    let mut rows = Vec::with_capacity(chat.items.len() + 2);
    if loose_empty {
        rows.push(Row::Loose);
    }
    rows.extend((0..chat.items.len()).filter(|&i| !chat.hidden(i)).map(Row::Item));
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
}

impl ThreadList {
    fn new() -> Self {
        Self { state: ListState::new(0, ListAlignment::Bottom, px(OVERDRAW)), rows: Vec::new() }
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
    /// lados. `narrow` lo limita al ancho de lectura (Formal y Glass).
    pub(super) fn thread_list(&self, chat: &Chat, loose_empty: bool, side: f32, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let rows = rows_of(chat, loose_empty);
        let state = {
            let mut lists = self.lists.borrow_mut();
            let list = lists.entry(chat.key.clone()).or_insert_with(ThreadList::new);
            list.update(rows);
            list.state.clone()
        };
        let key = chat.key.clone();
        let render = cx.processor(move |view: &mut CodeView, ix: usize, _: &mut Window, cx: &mut Context<CodeView>| {
            view.thread_row(&key, ix, side, narrow, cx)
        });
        list(state, render).size_full().pt(px(if super::view::expressive() { 20. } else { 12. })).pb(px(24.)).into_any_element()
    }

    /// Una fila de la lista de `key`.
    fn thread_row(&mut self, key: &str, ix: usize, side: f32, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let row = self.lists.borrow().get(key).and_then(|list| list.rows.get(ix).copied());
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
            Some(Row::Footer) => self.thread_footer(chat, cx),
            Some(Row::Loose) => super::view::loose_notice(),
            None => div().into_any_element(),
        };
        let gap = if super::view::expressive() { 12. } else { 16. };
        div()
            .w_full()
            .px(px(side))
            .pb(px(gap))
            .when(narrow, |el| el.max_w(px(super::view::THREAD_W)).mx_auto())
            .child(content)
            .into_any_element()
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
        self.lists.borrow().get(key)?.rows.iter().position(|row| *row == Row::Item(index))
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
