//! Dos conversaciones lado a lado: se arrastra una de la barra lateral a la mitad
//! izquierda o derecha del chat y se abre ahí, junto a la que estaba.
//!
//! Solo una de las dos es la activa (la caja de texto, los menús y el panel
//! derecho son de ella); la otra se ve entera, con sus permisos, y un clic en su
//! caja la vuelve la activa. El borrador de cada una se guarda al cambiar.

use gpui::{div, prelude::*, px, Context, Render, SharedString, Window};

use super::{CodeView, SessionInfo};

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

/// Dónde queda la otra conversación al soltar `dropped` en una mitad: a la
/// izquierda si se soltó en la derecha, y al revés.
pub(super) fn other_on_left(dropped_on_left: bool) -> bool {
    !dropped_on_left
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

    /// Se soltó una conversación en una mitad del chat: se abre ahí y la que
    /// estaba pasa al otro lado. Sin una conversación a la vista, solo se abre.
    pub(super) fn open_split(&mut self, drag: &ChatDrag, on_left: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.dragging_chat = false;
        // La de al lado se soltó en la otra mitad: solo cambian de lugar.
        if drag.key.is_some() && drag.key == self.split {
            self.focus_split(window, cx);
            self.split_left = other_on_left(on_left);
            return;
        }
        let previous = self.active.clone().filter(|_| !self.history_page);
        self.history_page = false;
        self.save_draft(cx);
        match (&drag.key, &drag.session) {
            (Some(key), _) => self.select_chat(key.clone(), window, cx),
            (None, Some(info)) => self.open_session(drag.workspace, info.clone(), window, cx),
            _ => return,
        }
        self.restore_draft(window, cx);
        match previous.filter(|key| self.active.as_ref() != Some(key)) {
            Some(previous) => {
                self.split = Some(previous);
                self.split_left = other_on_left(on_left);
                self.split_thread.scroll_to_bottom();
            }
            // Era la misma (o no había ninguna): queda una sola.
            None => self.split = None,
        }
        cx.notify();
    }

    /// La otra conversación pasa a ser la activa (y la activa, la otra).
    pub(super) fn focus_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(other) = self.split.take() else {
            return;
        };
        self.save_draft(cx);
        self.split = self.active.clone();
        self.split_left = !self.split_left;
        std::mem::swap(&mut self.thread, &mut self.split_thread);
        self.select_chat(other, window, cx);
        self.restore_draft(window, cx);
    }

    pub(super) fn close_split(&mut self, cx: &mut Context<Self>) {
        self.split = None;
        cx.notify();
    }

    /// Guarda lo escrito en la caja como borrador de la conversación activa.
    fn save_draft(&mut self, cx: &mut Context<Self>) {
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
    fn restore_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.active.as_ref().and_then(|key| self.drafts.remove(key)).unwrap_or_default();
        self.composer.update(cx, |area, cx| area.set_text(&text, cx));
        self.focus_composer(window, cx);
    }

    /// La conversación del otro lado, si sigue abierta.
    pub(super) fn split_chat(&self) -> Option<&super::Chat> {
        let key = self.split.as_ref()?;
        self.chats.iter().find(|c| &c.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_otra_queda_del_lado_contrario() {
        assert!(other_on_left(false));
        assert!(!other_on_left(true));
    }
}
