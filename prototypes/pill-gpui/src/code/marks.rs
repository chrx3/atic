//! Las marcas de Atic Code, como en la referencia: los mensajes marcados de cada
//! conversación (`flags.ts`) y las conversaciones destacadas (los marcadores
//! de la barra y del historial). Se guardan en esta máquina, en
//! `code-marks.json`, junto a `code-claude.json`.
//!
//! Un mensaje se identifica por su tipo y su texto: así la marca sobrevive a
//! reabrir la conversación (las filas cambian al recargar).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const FILE: &str = "code-marks.json";

/// El identificador estable de un mensaje: tipo + hash de su texto, el mismo
/// de `flagId` de la referencia (djb2 sobre UTF-16, en base 36).
pub fn flag_id(kind: &str, text: &str) -> String {
    let mut hash: i32 = 5381;
    let mut len = 0usize;
    for unit in text.encode_utf16() {
        hash = (hash << 5).wrapping_add(hash).wrapping_add(unit as i32);
        len += 1;
    }
    format!("{kind}:{}:{len}", base36(hash as u32))
}

fn base36(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(std::char::from_digit(n % 36, 36).unwrap_or('0'));
        n /= 36;
    }
    digits.iter().rev().collect()
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Marks {
    /// Sesión (o clave de la conversación antes del primer mensaje) → mensajes marcados.
    flags: HashMap<String, Vec<String>>,
    /// Sesiones destacadas.
    bookmarks: Vec<String>,
    /// No se guarda en disco (para las pruebas).
    #[serde(skip)]
    memory_only: bool,
}

impl Marks {
    pub fn load() -> Self {
        crate::paths::file(FILE)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        Self { memory_only: true, ..Default::default() }
    }

    fn save(&self) {
        if self.memory_only {
            return;
        }
        let Some(path) = crate::paths::file(FILE) else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "atic code: no se guardaron las marcas");
                }
            }
            Err(error) => tracing::warn!(%error, "atic code: marcas sin serializar"),
        }
    }

    pub fn is_flagged(&self, session: &str, id: &str) -> bool {
        self.flags.get(session).is_some_and(|list| list.iter().any(|f| f == id))
    }

    pub fn flag_count(&self, session: &str) -> usize {
        self.flags.get(session).map_or(0, Vec::len)
    }

    /// Marca o desmarca un mensaje; devuelve si quedó marcado.
    pub fn toggle_flag(&mut self, session: &str, id: &str) -> bool {
        let list = self.flags.entry(session.to_string()).or_default();
        let on = match list.iter().position(|f| f == id) {
            Some(at) => {
                list.remove(at);
                false
            }
            None => {
                list.push(id.to_string());
                true
            }
        };
        if list.is_empty() {
            self.flags.remove(session);
        }
        self.save();
        on
    }

    /// Las marcas hechas antes del primer mensaje (bajo la clave de la
    /// conversación) pasan a su sesión cuando Claude Code le da una.
    pub fn move_flags(&mut self, from: &str, to: &str) {
        let Some(moved) = self.flags.remove(from) else {
            return;
        };
        let list = self.flags.entry(to.to_string()).or_default();
        for id in moved {
            if !list.contains(&id) {
                list.push(id);
            }
        }
        self.save();
    }

    pub fn is_bookmarked(&self, session: &str) -> bool {
        self.bookmarks.iter().any(|b| b == session)
    }

    /// Agrega o quita un marcador; devuelve si quedó puesto.
    pub fn toggle_bookmark(&mut self, session: &str) -> bool {
        let on = match self.bookmarks.iter().position(|b| b == session) {
            Some(at) => {
                self.bookmarks.remove(at);
                false
            }
            None => {
                self.bookmarks.push(session.to_string());
                true
            }
        };
        self.save();
        on
    }

    /// Lo de una conversación eliminada.
    pub fn forget(&mut self, session: &str) {
        let had = self.flags.remove(session).is_some();
        let before = self.bookmarks.len();
        self.bookmarks.retain(|b| b != session);
        if had || before != self.bookmarks.len() {
            self.save();
        }
    }
}

/// Cuánto dura el destello del mensaje al que se salta.
const FLASH: std::time::Duration = std::time::Duration::from_millis(1100);

impl super::CodeView {
    /// El id con que se marca la fila `index` de la conversación visible.
    fn flag_target(&self, index: usize) -> Option<(String, String)> {
        let chat = self.active_chat()?;
        let (kind, text) = chat.items.get(index)?.flag_key()?;
        Some((chat.flag_session().to_string(), flag_id(kind, text)))
    }

    /// Si la fila `index` de `chat` está marcada.
    pub(super) fn is_flagged(&self, chat: &super::Chat, index: usize) -> bool {
        chat.items.get(index).and_then(|item| item.flag_key()).is_some_and(|(kind, text)| self.marks.is_flagged(chat.flag_session(), &flag_id(kind, text)))
    }

    pub(super) fn flag_count(&self) -> usize {
        self.active_chat().map_or(0, |chat| self.marks.flag_count(chat.flag_session()))
    }

    /// El botón «Marcar» bajo un mensaje.
    pub(super) fn toggle_flag(&mut self, index: usize, cx: &mut gpui::Context<Self>) {
        if let Some((session, id)) = self.flag_target(index) {
            self.marks.toggle_flag(&session, &id);
            cx.notify();
        }
    }

    /// «Siguiente mensaje marcado»: lleva la vista al siguiente bajo el borde
    /// de arriba (al primero al llegar al final), lo centra y lo hace destellar.
    pub(super) fn next_flagged(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let Some(chat) = self.active_chat() else {
            return false;
        };
        let flagged: Vec<usize> = (0..chat.items.len()).filter(|&i| self.is_flagged(chat, i)).collect();
        if flagged.is_empty() {
            return false;
        }
        let view = self.thread.bounds();
        let bounds = self.flag_bounds.borrow().clone();
        let below = flagged.iter().copied().find(|i| bounds.get(i).is_some_and(|b| b.top() > view.top() + gpui::px(24.)));
        let target = below.unwrap_or(flagged[0]);
        if let Some(item) = bounds.get(&target) {
            let offset = self.thread.offset();
            let max = self.thread.max_offset().height;
            // Centrado en la vista: el desplazamiento es negativo hacia abajo.
            let delta = (item.top() - view.top()) - (view.size.height - item.size.height.min(view.size.height)) / 2.;
            let y = (offset.y - delta).min(gpui::px(0.)).max(-max);
            self.thread.set_offset(gpui::point(offset.x, y));
            self.follow = false;
        }
        self.flash_gen += 1;
        let generation = self.flash_gen;
        self.flash = Some(target);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH).await;
            let _ = this.update(cx, |view, cx| {
                if view.flash_gen == generation {
                    view.flash = None;
                    cx.notify();
                }
            });
        })
        .detach();
        true
    }

    pub(super) fn is_bookmarked(&self, session: &str) -> bool {
        self.marks.is_bookmarked(session)
    }

    /// Agrega o quita el marcador de una conversación guardada.
    pub(super) fn toggle_bookmark(&mut self, session: &str, cx: &mut gpui::Context<Self>) {
        let on = self.marks.toggle_bookmark(session);
        self.show_toast(if on { "Marcador agregado" } else { "Marcador quitado" }, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_id_de_un_mensaje_es_el_de_la_referencia() {
        // Los valores salen de `flagId` de la referencia (flags.ts).
        assert_eq!(flag_id("user", ""), "user:45h:0");
        assert_eq!(flag_id("user", "hola"), "user:ykj59l:4");
        // Cuenta en UTF-16, como `text.length` de JavaScript.
        assert_eq!(flag_id("text", "¿qué?"), "text:61875e:5");
        assert_ne!(flag_id("user", "hola"), flag_id("text", "hola"));
    }

    #[test]
    fn marcar_desmarcar_y_contar() {
        let mut marks = Marks::in_memory();
        let id = flag_id("user", "hola");
        assert!(marks.toggle_flag("s1", &id));
        assert!(marks.is_flagged("s1", &id));
        assert!(!marks.is_flagged("s2", &id));
        assert!(marks.toggle_flag("s1", &flag_id("text", "respuesta")));
        assert_eq!(marks.flag_count("s1"), 2);
        assert!(!marks.toggle_flag("s1", &id));
        assert_eq!(marks.flag_count("s1"), 1);
    }

    #[test]
    fn las_marcas_previas_pasan_a_la_sesion() {
        let mut marks = Marks::in_memory();
        marks.toggle_flag("c1", "user:a:1");
        marks.toggle_flag("sesion", "user:b:1");
        marks.move_flags("c1", "sesion");
        assert_eq!(marks.flag_count("c1"), 0);
        assert_eq!(marks.flag_count("sesion"), 2);
    }

    #[test]
    fn los_marcadores_se_ponen_y_se_quitan() {
        let mut marks = Marks::in_memory();
        assert!(marks.toggle_bookmark("s1"));
        assert!(marks.is_bookmarked("s1"));
        marks.toggle_flag("s1", "x");
        marks.forget("s1");
        assert!(!marks.is_bookmarked("s1"));
        assert_eq!(marks.flag_count("s1"), 0);
        assert!(marks.toggle_bookmark("s2"));
        assert!(!marks.toggle_bookmark("s2"));
    }
}
