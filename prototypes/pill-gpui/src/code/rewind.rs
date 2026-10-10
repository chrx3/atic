//! Volver atrás y seguir al día, como en la referencia:
//!
//! - El Rewind (Agent.tsx:389-409): el código vuelve a como estaba antes de
//!   un mensaje (`rewindFiles`) y la conversación sigue en una rama nueva
//!   desde el mensaje anterior (`start` con `resumeSessionAt` y
//!   `forkSession`, `forkAt` de store.ts), con el modelo de la conversación.
//! - El resync (agent.ts:522-550): si otro cliente siguió la conversación, se
//!   vuelve a leer de su archivo y su proceso se reabre en el punto más nuevo.

use gpui::{Context, Point};
use serde_json::{json, Value};

use super::agent_menu::Sub;
use super::chat::Chat;
use super::{CodeView, Menu};

/// Qué se rebobina.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rewind {
    Code,
    Conversation,
    Both,
}

impl CodeView {
    /// Esc Esc o «Rewind» del menú: el submenú con los mensajes.
    pub(super) fn open_rewind(&mut self, cx: &mut Context<Self>) {
        self.pop = None;
        self.menu = Some((Menu::Actions, Point::default()));
        self.menu_sub = Some(Sub::Rewind);
        cx.notify();
    }

    pub(super) fn rewind(&mut self, uuid: String, text: String, what: Rewind, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.close_menus(cx);
        let Some(chat) = self.active_chat() else {
            return;
        };
        let key = chat.key.clone();
        let session = chat.session_id.clone();
        let before = chat.uuid_before(&uuid);
        if what != Rewind::Conversation {
            // Los puntos de restauración son del proceso: si estaba cerrado, se reabre.
            self.ensure_live(&key, cx);
            self.request("rewindFiles", json!({ "key": key, "messageId": uuid }), cx, |view, reply, cx| match reply {
                Ok(result) => {
                    if result.get("canRewind").and_then(Value::as_bool).unwrap_or(false) {
                        let files = result.get("filesChanged").and_then(Value::as_array).map_or(0, Vec::len);
                        let detail = if files > 0 { format!(" ({files} archivos)") } else { String::new() };
                        view.show_toast(format!("Código restaurado{detail}"), cx);
                    } else {
                        let error = result.get("error").and_then(Value::as_str).unwrap_or("No hay cambios de código que restaurar en ese punto");
                        view.show_toast(error.to_string(), cx);
                    }
                }
                Err(error) => view.show_toast(error, cx),
            });
        }
        if what == Rewind::Code {
            return;
        }
        match (session, before) {
            (Some(session), Some(before)) => self.fork_at(&key, session, before, window, cx),
            // El primer mensaje: no hay nada antes, es una conversación nueva.
            _ => self.new_conversation(window, cx),
        }
        self.prefill(&text, window, cx);
    }

    /// Una rama de la conversación `source` que sigue desde el mensaje `at`.
    fn fork_at(&mut self, source: &str, session: String, at: String, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if !self.chats.iter().any(|c| c.key == source) {
            return;
        }
        let key = self.new_key();
        let Some(from) = self.chats.iter().find(|c| c.key == source) else {
            return;
        };
        let mut chat = Chat::new(key.clone(), from.workspace, from.cwd.clone());
        chat.title = from.title.clone();
        // Hereda el modelo y el esfuerzo de la conversación de la que sale.
        chat.want_model = from.want_model.clone();
        chat.want_effort = from.want_effort.clone();
        chat.sent_model = from.sent_model.clone();
        chat.fork = Some((session.clone(), at.clone()));
        let dir = chat.cwd.clone();
        self.chats.push(chat);
        self.history_page = false;
        self.select_chat(key.clone(), window, cx);
        self.ensure_live(&key, cx);
        self.request("sessionMessages", json!({ "sessionId": session, "dir": dir }), cx, move |view, reply, _| {
            let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) else {
                return;
            };
            match reply {
                Ok(messages) => chat.load_history(&view.models, messages.as_array().map(Vec::as_slice).unwrap_or_default(), Some(&at)),
                Err(error) => chat.notice(format!("No se pudo leer la conversación: {error}"), true),
            }
            if view.panes.pane_of(key.clone()).is_some() {
                view.scroll_to_end(&key);
            }
        });
    }

    /// Otro cliente (VS Code, la terminal, el móvil) siguió la conversación:
    /// se cierra su proceso y se recarga desde el archivo. Si es la que se ve,
    /// se reabre ya; si no, queda como no leída. Así nunca sigue desde un
    /// punto viejo (lo que partiría la conversación en dos ramas).
    pub(super) fn resync(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
            return;
        };
        let Some(session) = chat.session_id.clone() else {
            return;
        };
        if chat.busy || !chat.permissions.is_empty() {
            return;
        }
        let was_live = chat.live;
        chat.live = false;
        chat.starting = false;
        chat.applied = None;
        chat.reset_items();
        let (dir, workspace) = (chat.cwd.clone(), chat.workspace);
        if was_live {
            self.fire("close", json!({ "key": key }), cx);
        }
        let seen = self.active.as_deref() == Some(key);
        let key = key.to_string();
        self.request("sessionMessages", json!({ "sessionId": session, "dir": dir }), cx, move |view, reply, cx| {
            let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) else {
                return;
            };
            match reply {
                Ok(messages) => chat.load_history(&view.models, messages.as_array().map(Vec::as_slice).unwrap_or_default(), None),
                Err(error) => chat.notice(format!("No se pudo leer la conversación: {error}"), true),
            }
            if seen {
                view.ensure_live(&key, cx);
                view.scroll_to_end(&key);
            } else {
                chat.unread = true;
            }
        });
        self.load_history_for(workspace, cx);
    }
}
