//! Las conversaciones de Atic Code vistas desde la bandeja del notch (`agents.rs`).
//!
//! El notch lee las sesiones de los JSONL de Claude (y los rollouts de Codex),
//! y las de Atic Code también salen ahí: el `claude` que lanza el sidecar escribe
//! en el mismo `~/.claude/projects`. Una sesión cuyo id es el de una conversación
//! de Atic Code no tiene terminal que traer al frente: «Ver» abre esa
//! conversación, «Aceptar» la da por leída y el mensaje rápido se le manda ahí.
//!
//! La ventana se registra al abrirse (`register`); cerrada, no hay conversaciones
//! que alcanzar y el notch hace lo de siempre.

use std::path::PathBuf;

use gpui::{App, Context, Global, Window, WindowHandle};

use crate::kit::workspaces::same;

use super::chat::Chat;
use super::CodeView;

/// La ventana de Atic Code mientras está abierta.
struct OpenCode(WindowHandle<CodeView>);

impl Global for OpenCode {}

pub(super) fn register(handle: WindowHandle<CodeView>, cx: &mut App) {
    cx.set_global(OpenCode(handle));
}

fn handle(cx: &App) -> Option<WindowHandle<CodeView>> {
    cx.try_global::<OpenCode>().map(|open| open.0)
}

/// Una sesión de agente que vive en una conversación de Atic Code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLink {
    /// El id de la sesión: el nombre del JSONL de Claude o la sesión del agente ACP.
    pub session: String,
    /// La conversación en Atic Code (su clave en el sidecar).
    pub key: String,
    /// Se está mirando: la ventana al frente con esa conversación activa.
    pub in_view: bool,
    /// Lo que el celular muestra de ella (`phone::set_agents`).
    pub title: String,
    pub cwd: PathBuf,
    /// Terminó un turno mientras no se miraba.
    pub unread: bool,
    pub busy: bool,
    pub model: Option<String>,
    /// El agente ACP (`opencode`, `gemini`…); `None` es Claude.
    pub acp: Option<&'static str>,
}

/// Las sesiones de las conversaciones, con su clave. Una conversación sin sesión
/// todavía (no ha respondido nada) no aparece en el notch y no se enlaza.
fn links_of(chats: &[Chat], active: Option<&str>, window_active: bool) -> Vec<ChatLink> {
    chats
        .iter()
        .flat_map(|chat| {
            let in_view = window_active && active == Some(chat.key.as_str());
            [chat.session_id.as_deref(), chat.acp_session.as_deref()]
                .into_iter()
                .flatten()
                .filter(|session| !session.is_empty())
                .map(move |session| ChatLink {
                    session: session.to_string(),
                    key: chat.key.clone(),
                    in_view,
                    title: chat.title.clone(),
                    cwd: chat.cwd.clone(),
                    unread: chat.unread,
                    busy: chat.busy,
                    model: chat.model.clone(),
                    acp: chat.acp,
                })
        })
        .collect()
}

/// La conversación de Atic Code de esa sesión, si la hay.
pub fn link_for<'a>(links: &'a [ChatLink], session: &str) -> Option<&'a ChatLink> {
    if session.is_empty() {
        return None;
    }
    links.iter().find(|link| link.session == session)
}

/// Las sesiones de Atic Code ahora mismo; vacío con la ventana cerrada.
pub fn chat_links(cx: &App) -> Vec<ChatLink> {
    let Some(handle) = handle(cx) else {
        return Vec::new();
    };
    handle
        .read_with(cx, |view, _| links_of(&view.chats, view.active.as_deref(), view.window_active))
        .unwrap_or_default()
}

/// «Aceptar» en la bandeja: la conversación queda leída.
pub fn mark_read(cx: &mut App, key: &str) {
    let Some(handle) = handle(cx) else {
        return;
    };
    let _ = handle.update(cx, |view, _, cx| {
        if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
            chat.unread = false;
            cx.notify();
        }
    });
}

/// El mensaje rápido del notch en una conversación: sale ya o, con un turno en
/// curso, queda en su cola como si se hubiera escrito ahí. Dice si llegó.
pub fn send_to_chat(cx: &mut App, key: &str, text: &str) -> bool {
    send_with_images(cx, key, text, Vec::new())
}

/// Lo mismo con imágenes (`mime` y bytes), como si se hubieran pegado en la
/// conversación: van al agente como imágenes y se ven en la burbuja.
pub fn send_with_images(cx: &mut App, key: &str, text: &str, images: Vec<(String, Vec<u8>)>) -> bool {
    let Some(handle) = handle(cx) else {
        return false;
    };
    use base64::Engine;
    let mut blocks = Vec::new();
    let mut thumbs = Vec::new();
    for (mime, data) in images {
        // La burbuja muestra una miniatura con la proporción de la imagen, no la
        // imagen entera (`image_from`).
        let Some(thumb) = super::chat::image_from(&mime, data.clone()) else {
            continue;
        };
        blocks.push(serde_json::json!({ "mediaType": mime, "data": base64::engine::general_purpose::STANDARD.encode(&data) }));
        thumbs.push(thumb);
    }
    // Ninguna imagen se pudo leer y no hay texto: no sale un mensaje vacío.
    if blocks.is_empty() && text.trim().is_empty() {
        return false;
    }
    handle
        .update(cx, |view, _, cx| view.send_from_outside(key, text.to_string(), blocks, thumbs, cx))
        .unwrap_or(false)
}

/// «Detener» desde el celular: corta el turno en curso de esa conversación.
pub fn interrupt(cx: &mut App, key: &str) -> bool {
    let Some(handle) = handle(cx) else {
        return false;
    };
    handle.update(cx, |view, _, cx| view.interrupt_chat(key, cx)).unwrap_or(false)
}

impl CodeView {
    /// La ↗ de una sesión de Claude en el notch: se retoma aquí y no en una consola.
    /// Va al espacio que arranca en esa carpeta (la sesión se busca por la carpeta
    /// donde corrió) o crea uno con ella.
    pub(crate) fn resume_session(&mut self, session: String, cwd: PathBuf, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let found = self
            .workspaces
            .list()
            .iter()
            .find(|w| w.main().is_some_and(|main| same(main, &cwd)))
            .map(|w| w.id);
        let workspace = found.unwrap_or_else(|| self.workspaces.create(vec![cwd]));
        self.select_workspace(workspace, cx);
        let info = super::SessionInfo { session_id: session, title, modified: None, branch: None };
        self.open_session(workspace, info, window, cx);
    }

    /// Una sesión nueva lista para escribir (desde el lanzador): en ese
    /// espacio, en el activo o, sin ninguno, un chat suelto.
    pub(crate) fn start_session(&mut self, workspace: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = workspace
            .filter(|id| self.workspaces.get(*id).is_some())
            .or_else(|| self.workspaces.active().map(|w| w.id));
        match workspace {
            Some(id) => {
                self.history_page = false;
                self.new_chat(id, window, cx);
            }
            None => self.new_loose_chat(window, cx),
        }
    }

    fn send_from_outside(
        &mut self,
        key: &str,
        text: String,
        images: Vec<serde_json::Value>,
        thumbs: Vec<std::sync::Arc<gpui::Image>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
            return false;
        };
        chat.unread = false;
        if chat.busy {
            chat.enqueue(text, images, thumbs);
        } else {
            self.dispatch_message(key.to_string(), text, images, thumbs, cx);
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chat(key: &str, session: Option<&str>, acp: Option<&str>) -> Chat {
        let mut chat = Chat::new(key.into(), 1, PathBuf::from("."));
        chat.session_id = session.map(str::to_string);
        chat.acp_session = acp.map(str::to_string);
        chat
    }

    #[test]
    fn la_sesion_lleva_a_su_conversacion() {
        let chats = [
            chat("c1", Some("aaa"), None),
            chat("c2", None, None),
            chat("c3", Some(""), Some("thread-9")),
        ];
        let links = links_of(&chats, Some("c1"), true);
        assert_eq!(links.len(), 2);
        let found = link_for(&links, "aaa").unwrap();
        assert_eq!((found.key.as_str(), found.in_view), ("c1", true));
        // La sesión del agente ACP también enlaza; la vacía, no.
        assert_eq!(link_for(&links, "thread-9").map(|l| l.key.as_str()), Some("c3"));
        assert!(link_for(&links, "").is_none());
        // Una sesión de una terminal cualquiera no es de Atic Code.
        assert!(link_for(&links, "bbb").is_none());
    }

    #[test]
    fn a_la_vista_solo_con_la_ventana_al_frente() {
        let chats = [chat("c1", Some("aaa"), None), chat("c2", Some("bbb"), None)];
        let behind = links_of(&chats, Some("c1"), false);
        assert!(behind.iter().all(|l| !l.in_view));
        let front = links_of(&chats, Some("c1"), true);
        assert!(link_for(&front, "aaa").unwrap().in_view);
        assert!(!link_for(&front, "bbb").unwrap().in_view);
    }
}
