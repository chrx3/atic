//! Buscar dentro de la conversación (Ctrl+F): una barra bajo la cabecera con el campo,
//! el contador «3 de 12» y los botones anterior y siguiente. Las coincidencias son partes
//! enteras de la conversación (`Chat::items`): al moverse, la lista se desplaza hasta la
//! parte y esta se tiñe con el acento (la actual más fuerte). No se resalta la palabra
//! dentro del Markdown.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, Hsla, KeyDownEvent, Window};

use super::chat::Item;
use super::style::t;
use super::view::icon_button;
use super::{CodeView, FindInChat};
use crate::i18n::{t as tr, tf};

/// El estado de la búsqueda abierta: sobre qué conversación, qué se busca y dónde.
pub(super) struct Find {
    /// La conversación en que se abrió (`Chat::key`): si la activa cambia, se cierra.
    pub key: String,
    pub query: String,
    /// Los índices de `Chat::items` que coinciden, de menor a mayor.
    pub matches: Vec<usize>,
    /// La coincidencia a la vista: un lugar de `matches`.
    pub current: usize,
    /// Cuántas partes tenía la conversación al buscar (para volver a buscar si crece).
    items: usize,
}

impl Find {
    fn new(key: String) -> Self {
        Self { key, query: String::new(), matches: Vec::new(), current: 0, items: 0 }
    }
}

/// El texto de una parte de la conversación donde se busca; `None` si no se busca en ella.
fn searchable(item: &Item) -> Option<String> {
    match item {
        Item::User { text, .. } => Some(text.clone()),
        Item::Text(text) | Item::Thinking(text) => Some(text.clone()),
        Item::Tool(call) => {
            let mut text = call.name.clone();
            if let Some(input) = &call.input {
                text.push(' ');
                text.push_str(&input.to_string());
            }
            Some(text)
        }
        Item::Task(note) => {
            let mut text = note.summary.clone();
            if let Some(result) = &note.result {
                text.push(' ');
                text.push_str(result);
            }
            Some(text)
        }
        _ => None,
    }
}

/// Los índices de las partes que contienen `query` (sin distinguir mayúsculas), saltando
/// las que `hidden` esconde. Una búsqueda vacía no encuentra nada.
pub(super) fn matches_in(items: &[Item], hidden: impl Fn(usize) -> bool, query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    items
        .iter()
        .enumerate()
        .filter(|(index, _)| !hidden(*index))
        .filter(|(_, item)| searchable(item).is_some_and(|text| text.to_lowercase().contains(&query)))
        .map(|(index, _)| index)
        .collect()
}

/// La coincidencia que sigue (o la anterior) desde `current`, dando la vuelta en los extremos.
fn step_from(current: usize, total: usize, forward: bool) -> usize {
    if total == 0 {
        0
    } else if forward {
        (current + 1) % total
    } else {
        (current + total - 1) % total
    }
}

impl CodeView {
    /// Ctrl+F: abre la barra sobre la conversación activa y deja el cursor en su campo.
    pub(super) fn find_action(&mut self, _: &FindInChat, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.active_chat().map(|chat| chat.key.clone()) else {
            return;
        };
        if self.find.as_ref().is_none_or(|find| find.key != key) {
            self.find = Some(Find::new(key));
            self.find_field.update(cx, |field, cx| field.set_text("", cx));
        }
        self.find_field.update(cx, |field, _| field.focus(window));
        cx.notify();
    }

    /// Cierra la barra, borra la búsqueda y devuelve el foco a la caja.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.find_field.update(cx, |field, cx| field.set_text("", cx));
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Cambió lo escrito en el campo: se busca de nuevo y se va a la primera coincidencia.
    pub(super) fn find_changed(&mut self, text: String, _: &mut Window, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.query == text {
            return;
        }
        find.query = text;
        find.current = 0;
        self.refresh_matches();
        self.reveal_match();
        cx.notify();
    }

    /// Enter (siguiente) y Mayús+Enter (anterior).
    pub(super) fn find_step(&mut self, forward: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh_matches();
        if let Some(find) = self.find.as_mut() {
            find.current = step_from(find.current, find.matches.len(), forward);
        }
        self.reveal_match();
        cx.notify();
    }

    /// Vuelve a buscar en la conversación (por si le llegaron partes nuevas).
    fn refresh_matches(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let Some(chat) = self.chats.iter().find(|chat| chat.key == find.key) else {
            return;
        };
        find.matches = matches_in(&chat.items, |index| chat.hidden(index), &find.query);
        find.items = chat.items.len();
        if find.current >= find.matches.len() {
            find.current = 0;
        }
    }

    /// Desplaza la lista hasta la parte de la coincidencia de ahora.
    fn reveal_match(&self) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let Some(&index) = find.matches.get(find.current) else {
            return;
        };
        if let Some(row) = self.row_of_item(&find.key, index) {
            self.reveal_row(&find.key, row);
        }
    }

    /// Se llama en cada cuadro: si la conversación activa ya no es la de la búsqueda, la
    /// cierra y la limpia; si le llegaron partes, vuelve a buscar.
    pub(super) fn sync_find(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let (key, items) = (find.key.clone(), find.items);
        match self.active_chat().map(|chat| (chat.key.clone(), chat.items.len())) {
            Some((active, len)) if active == key => {
                if len != items {
                    self.refresh_matches();
                }
            }
            _ => {
                self.find = None;
                self.find_field.update(cx, |field, cx| field.set_text("", cx));
            }
        }
    }

    /// El color con que se tiñe la parte `index` de la conversación `key`, si coincide con
    /// la búsqueda: más fuerte la que está a la vista.
    pub(super) fn find_highlight(&self, key: &str, index: usize) -> Option<Hsla> {
        let find = self.find.as_ref().filter(|find| find.key == key)?;
        let at = find.matches.binary_search(&index).ok()?;
        let accent = t().accent;
        Some(accent.opacity(if at == find.current { 0.22 } else { 0.08 }))
    }

    /// La barra de búsqueda, si está abierta.
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        let t = t();
        let count = if find.query.trim().is_empty() {
            String::new()
        } else if find.matches.is_empty() {
            tr("pill.code.find.none").to_string()
        } else {
            let (n, total) = ((find.current + 1).to_string(), find.matches.len().to_string());
            tf("pill.code.find.count", &[("n", &n), ("total", &total)])
        };
        Some(
            div()
                .flex_none()
                .w_full()
                .px(px(16.))
                .py(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .border_b_1()
                .border_color(t.border)
                // Mayús+Enter va a la anterior (Enter solo lo resuelve el campo).
                .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" && event.keystroke.modifiers.shift {
                        view.find_step(false, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(div().flex_1().min_w(px(0.)).max_w(px(420.)).child(self.find_field.clone()))
                .child(div().flex_none().min_w(px(64.)).text_size(px(12.)).text_color(t.muted).child(count))
                .child(
                    icon_button("find-prev", "icons/chevron-up.svg", tr("pill.code.find.prev"))
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.find_step(false, window, cx))),
                )
                .child(
                    icon_button("find-next", "icons/chevron-down.svg", tr("pill.code.find.next"))
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.find_step(true, window, cx))),
                )
                .child(
                    icon_button("find-close", "icons/x.svg", tr("pill.code.find.close"))
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Item {
        Item::User { text: text.into(), uuid: None, images: Vec::new() }
    }

    #[test]
    fn busca_sin_distinguir_mayusculas() {
        let items = vec![user("Hola Mundo"), Item::Text("nada que ver".into()), Item::Thinking("pensando en el MUNDO".into())];
        assert_eq!(matches_in(&items, |_| false, "mundo"), vec![0, 2]);
        assert_eq!(matches_in(&items, |_| false, "MUNDO"), vec![0, 2]);
    }

    #[test]
    fn una_busqueda_vacia_no_encuentra_nada() {
        let items = vec![user("hola"), Item::Text("chao".into())];
        assert!(matches_in(&items, |_| false, "").is_empty());
        assert!(matches_in(&items, |_| false, "   ").is_empty());
    }

    #[test]
    fn salta_las_partes_escondidas_y_las_que_no_se_leen() {
        let items = vec![Item::Thinking("secreto".into()), Item::Turn("secreto 3 s".into()), Item::Text("secreto".into())];
        assert_eq!(matches_in(&items, |index| index == 0, "secreto"), vec![2]);
    }

    #[test]
    fn busca_en_el_nombre_y_la_entrada_de_una_herramienta() {
        let call = super::super::chat::ToolCall::new("t1".into(), "Bash".into(), Some(serde_json::json!({ "command": "cargo build" })));
        let items = vec![Item::Tool(call)];
        assert_eq!(matches_in(&items, |_| false, "cargo"), vec![0]);
        assert_eq!(matches_in(&items, |_| false, "bash"), vec![0]);
        assert!(matches_in(&items, |_| false, "npm").is_empty());
    }

    #[test]
    fn el_paso_da_la_vuelta_en_los_extremos() {
        assert_eq!(step_from(0, 3, true), 1);
        assert_eq!(step_from(2, 3, true), 0);
        assert_eq!(step_from(0, 3, false), 2);
        assert_eq!(step_from(1, 3, false), 0);
        assert_eq!(step_from(0, 0, true), 0);
    }
}
