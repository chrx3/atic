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

/// Una copia de lo que cambian fijar, hecha, posponer y el orden, para poder deshacerlo.
#[derive(Clone)]
pub struct MarksSnap {
    bookmarks: Vec<String>,
    settled: Vec<String>,
    snoozed: HashMap<String, u64>,
    order: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Marks {
    /// Sesión (o clave de la conversación antes del primer mensaje) → mensajes marcados.
    flags: HashMap<String, Vec<String>>,
    /// Sesiones fijadas arriba en la barra.
    bookmarks: Vec<String>,
    /// Sesiones marcadas como hechas: salen de las activas y quedan en la repisa «Hechas».
    settled: Vec<String>,
    /// El orden que dejó el usuario en las Activas (de arriba abajo). Las que no están aquí
    /// (nuevas) van arriba, por fecha.
    order: Vec<String>,
    /// Sesiones pospuestas → hasta cuándo (segundos desde 1970): no salen en la barra hasta
    /// entonces.
    snoozed: HashMap<String, u64>,
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
                // Fijar una conversación hecha la devuelve a las activas.
                self.settled.retain(|s| s != session);
                true
            }
        };
        self.save();
        on
    }

    /// Lo que cambian fijar, hecha, posponer y el orden, tal como está ahora.
    pub fn snapshot(&self) -> MarksSnap {
        MarksSnap { bookmarks: self.bookmarks.clone(), settled: self.settled.clone(), snoozed: self.snoozed.clone(), order: self.order.clone() }
    }

    /// Vuelve a como estaba en `snap`.
    pub fn restore(&mut self, snap: MarksSnap) {
        self.bookmarks = snap.bookmarks;
        self.settled = snap.settled;
        self.snoozed = snap.snoozed;
        self.order = snap.order;
        self.save();
    }

    /// Las fijadas, en el orden en que se muestran.
    pub fn pinned(&self) -> &[String] {
        &self.bookmarks
    }

    /// El orden manual de las Activas.
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// Deja las fijadas visibles en el orden `visible` (las demás conservan su lugar).
    pub fn set_pinned_order(&mut self, visible: &[String]) {
        reorder_subset(&mut self.bookmarks, visible);
        self.save();
    }

    /// Deja las activas visibles en el orden `visible`: desde ahora todas llevan lugar propio y
    /// las que lleguen después irán arriba.
    pub fn set_active_order(&mut self, visible: &[String]) {
        for id in visible {
            if !self.order.contains(id) {
                self.order.push(id.clone());
            }
        }
        reorder_subset(&mut self.order, visible);
        self.save();
    }

    /// Hasta cuándo está pospuesta la sesión (segundos desde 1970), si lo está.
    pub fn snoozed_until(&self, session: &str) -> Option<u64> {
        self.snoozed.get(session).copied()
    }

    pub fn is_settled(&self, session: &str) -> bool {
        self.settled.iter().any(|s| s == session)
    }

    /// Marca la sesión como hecha. Una hecha no queda fijada.
    pub fn settle(&mut self, session: &str) {
        self.bookmarks.retain(|b| b != session);
        if !self.is_settled(session) {
            self.settled.push(session.to_string());
        }
        self.save();
    }

    /// Devuelve la sesión a las activas; devuelve si estaba hecha.
    pub fn unsettle(&mut self, session: &str) -> bool {
        let before = self.settled.len();
        self.settled.retain(|s| s != session);
        let had = before != self.settled.len();
        if had {
            self.save();
        }
        had
    }

    /// ¿Sigue pospuesta la sesión en el instante `now` (segundos desde 1970)?
    pub fn is_snoozed(&self, session: &str, now: u64) -> bool {
        self.snoozed.get(session).is_some_and(|until| *until > now)
    }

    /// Pospone la sesión hasta `until`, y de paso olvida las que ya vencieron.
    pub fn snooze(&mut self, session: &str, until: u64, now: u64) {
        self.snoozed.retain(|_, at| *at > now);
        self.snoozed.insert(session.to_string(), until);
        self.save();
    }

    /// Deja de posponer la sesión; devuelve si lo estaba.
    pub fn unsnooze(&mut self, session: &str) -> bool {
        let had = self.snoozed.remove(session).is_some();
        if had {
            self.save();
        }
        had
    }

    /// Lo de una conversación eliminada.
    pub fn forget(&mut self, session: &str) {
        let had = self.flags.remove(session).is_some();
        let snoozed = self.snoozed.remove(session).is_some();
        let before = (self.bookmarks.len(), self.settled.len(), self.order.len());
        self.bookmarks.retain(|b| b != session);
        self.settled.retain(|s| s != session);
        self.order.retain(|o| o != session);
        if had || snoozed || before != (self.bookmarks.len(), self.settled.len(), self.order.len()) {
            self.save();
        }
    }
}

/// `ids` con `id` movida `delta` lugares (negativo, hacia arriba); `None` si no hay a dónde ir.
pub(super) fn moved_ids(ids: &[String], id: &str, delta: i32) -> Option<Vec<String>> {
    let at = ids.iter().position(|s| s == id)?;
    let to = at as i64 + i64::from(delta);
    if to < 0 || to as usize >= ids.len() {
        return None;
    }
    let mut out = ids.to_vec();
    let item = out.remove(at);
    out.insert(to as usize, item);
    Some(out)
}

/// Pone los elementos de `visible` en `full` en ese orden, usando los mismos lugares que ya
/// ocupaban; lo que no está en `visible` no se mueve.
fn reorder_subset(full: &mut [String], visible: &[String]) {
    let slots: Vec<usize> = full.iter().enumerate().filter(|(_, s)| visible.contains(s)).map(|(at, _)| at).collect();
    if slots.len() != visible.len() {
        return;
    }
    for (slot, id) in slots.into_iter().zip(visible) {
        full[slot] = id.clone();
    }
}

/// Cuánto falta, en pocas palabras: «5 min», «3 h», «2 d».
pub(super) fn duration_label(secs: u64) -> String {
    if secs < 3600 {
        format!("{} min", secs.div_ceil(60).max(1))
    } else if secs < 86_400 {
        format!("{} h", secs.div_ceil(3600))
    } else {
        format!("{} d", secs.div_ceil(86_400))
    }
}

/// En qué grupo de la barra cae una conversación guardada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bucket {
    /// Fijadas arriba.
    Pinned,
    /// El agente está respondiendo.
    Working,
    /// Las demás.
    Active,
    /// Marcadas como hechas: van en una repisa plegada.
    Settled,
}

/// Fijada gana a todo (hacerla «hecha» la suelta); si no, una que está respondiendo va a
/// «Trabajando» aunque esté hecha, y una hecha va a la repisa.
pub(super) fn bucket_of(pinned: bool, working: bool, settled: bool) -> Bucket {
    if pinned {
        Bucket::Pinned
    } else if working {
        Bucket::Working
    } else if settled {
        Bucket::Settled
    } else {
        Bucket::Active
    }
}

/// Cuánto dura a la vista el aviso con «Deshacer», en milisegundos.
const UNDO_MS: u64 = 5000;

/// Cuánto se pospone una conversación con cada opción del menú, en segundos.
pub(super) const SNOOZE_HOUR: u64 = 3600;
pub(super) const SNOOZE_DAY: u64 = 24 * 3600;
pub(super) const SNOOZE_WEEK: u64 = 7 * 24 * 3600;

/// Ahora, en segundos desde 1970.
pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
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
        // El siguiente bajo la primera fila que se ve (o el primero, al llegar al final).
        let key = chat.key.clone();
        let top = self.top_row(&key);
        let rows: Vec<(usize, usize)> = flagged.iter().filter_map(|&i| self.row_of_item(&key, i).map(|row| (i, row))).collect();
        let Some(&(target, row)) = rows.iter().find(|(_, row)| *row > top).or(rows.first()) else {
            return false;
        };
        self.reveal_row(&key, row);
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

    /// ¿Está pospuesta esta conversación guardada? Los chats abiertos sin guardar no se posponen.
    pub(super) fn is_snoozed(&self, session: &str) -> bool {
        self.marks.is_snoozed(session, now_secs())
    }

    /// ¿Está marcada como hecha esta conversación guardada?
    pub(super) fn is_settled(&self, session: &str) -> bool {
        self.marks.is_settled(session)
    }

    /// Manda la conversación a la repisa «Hechas» de la barra.
    pub(super) fn settle_session(&mut self, session: &str, cx: &mut gpui::Context<Self>) {
        let before = self.marks.snapshot();
        self.marks.settle(session);
        self.toast_undo(crate::i18n::t("pill.code.sidebar.settledToast"), before, cx);
        cx.notify();
    }

    /// Un aviso con «Deshacer»: durante unos segundos, el botón devuelve las marcas a como
    /// estaban en `before`.
    pub(super) fn toast_undo(&mut self, text: &'static str, before: MarksSnap, cx: &mut gpui::Context<Self>) {
        self.show_toast_for(text, UNDO_MS, cx);
        self.undo = Some((self.toast_gen, before));
    }

    /// Deshace el último cambio de marcas, si su aviso sigue a la vista.
    pub(super) fn undo_marks(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some((generation, before)) = self.undo.take() {
            if generation == self.toast_gen {
                self.marks.restore(before);
                self.show_toast(crate::i18n::t("pill.code.sidebar.undone"), cx);
            }
        }
        cx.notify();
    }

    /// Devuelve la conversación a las activas.
    pub(super) fn unsettle_session(&mut self, session: &str, cx: &mut gpui::Context<Self>) {
        let before = self.marks.snapshot();
        if self.marks.unsettle(session) {
            self.toast_undo(crate::i18n::t("pill.code.sidebar.unsettledToast"), before, cx);
        }
        cx.notify();
    }

    /// Esconde la conversación de la barra durante `secs` segundos. Al vencer, la barra se
    /// redibuja sola y vuelve a aparecer.
    pub(super) fn snooze_session(&mut self, session: &str, secs: u64, cx: &mut gpui::Context<Self>) {
        let now = now_secs();
        let before = self.marks.snapshot();
        self.marks.snooze(session, now + secs, now);
        self.toast_undo(crate::i18n::t("pill.code.sidebar.snoozedToast"), before, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(secs)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
    }

    /// Vuelve a mostrar una conversación pospuesta.
    pub(super) fn unsnooze_session(&mut self, session: &str, cx: &mut gpui::Context<Self>) {
        let before = self.marks.snapshot();
        if self.marks.unsnooze(session) {
            self.toast_undo(crate::i18n::t("pill.code.sidebar.unsnoozedToast"), before, cx);
        }
        cx.notify();
    }

    /// Agrega o quita el marcador de una conversación guardada.
    pub(super) fn toggle_bookmark(&mut self, session: &str, cx: &mut gpui::Context<Self>) {
        let before = self.marks.snapshot();
        let on = self.marks.toggle_bookmark(session);
        self.toast_undo(if on { crate::i18n::t("pill.code.sidebar.pinnedToast") } else { crate::i18n::t("pill.code.sidebar.unpinnedToast") }, before, cx);
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
    fn mover_conversaciones_de_lugar() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(moved_ids(&ids, "b", -1), Some(vec!["b".into(), "a".into(), "c".into()]));
        assert_eq!(moved_ids(&ids, "b", 1), Some(vec!["a".into(), "c".into(), "b".into()]));
        // En los extremos no hay a dónde ir; una desconocida tampoco.
        assert_eq!(moved_ids(&ids, "a", -1), None);
        assert_eq!(moved_ids(&ids, "c", 1), None);
        assert_eq!(moved_ids(&ids, "x", 1), None);

        // Reordenar lo visible deja las demás en su lugar.
        let mut full: Vec<String> = ["a", "x", "b", "y", "c"].iter().map(|s| s.to_string()).collect();
        reorder_subset(&mut full, &["c".to_string(), "a".to_string(), "b".to_string()]);
        assert_eq!(full, ["c", "x", "a", "y", "b"]);

        // El orden manual congela lo visible; lo nuevo no está en la lista.
        let mut marks = Marks::in_memory();
        marks.set_active_order(&["a".to_string(), "b".to_string()]);
        marks.set_active_order(&["b".to_string(), "a".to_string()]);
        assert_eq!(marks.order(), ["b", "a"]);
        marks.forget("b");
        assert_eq!(marks.order(), ["a"]);
    }

    #[test]
    fn cuanto_falta_para_volver() {
        assert_eq!(duration_label(30), "1 min");
        assert_eq!(duration_label(300), "5 min");
        assert_eq!(duration_label(3600), "1 h");
        assert_eq!(duration_label(3 * 3600 - 5), "3 h");
        assert_eq!(duration_label(86_400), "1 d");
        assert_eq!(duration_label(6 * 86_400 + 1), "7 d");
    }

    #[test]
    fn hechas_fijadas_y_trabajando() {
        // Una conversación cualquiera es activa; fijada gana a todo.
        assert_eq!(bucket_of(false, false, false), Bucket::Active);
        assert_eq!(bucket_of(true, true, false), Bucket::Pinned);
        // Respondiendo gana a «hecha»: la hecha vuelve a verse mientras trabaja.
        assert_eq!(bucket_of(false, true, true), Bucket::Working);
        assert_eq!(bucket_of(false, false, true), Bucket::Settled);

        let mut marks = Marks::in_memory();
        marks.toggle_bookmark("s1");
        // Hacerla la suelta de las fijadas.
        marks.settle("s1");
        assert!(marks.is_settled("s1"));
        assert!(!marks.is_bookmarked("s1"));
        // Fijarla de nuevo la devuelve a las activas.
        marks.toggle_bookmark("s1");
        assert!(!marks.is_settled("s1"));
        assert!(marks.is_bookmarked("s1"));
        // Volver a activas.
        marks.settle("s2");
        assert!(marks.unsettle("s2"));
        assert!(!marks.unsettle("s2"));
        // Borrarla borra también su estado.
        marks.settle("s3");
        marks.forget("s3");
        assert!(!marks.is_settled("s3"));
    }

    #[test]
    fn posponer_y_volver() {
        let mut marks = Marks::in_memory();
        assert!(!marks.is_snoozed("s1", 100));
        marks.snooze("s1", 200, 100);
        assert!(marks.is_snoozed("s1", 100));
        assert!(marks.is_snoozed("s1", 199));
        // Al vencer vuelve sola.
        assert!(!marks.is_snoozed("s1", 200));
        // Posponer otra olvida las vencidas.
        marks.snooze("s2", 500, 300);
        assert!(!marks.snoozed.contains_key("s1"));
        assert!(marks.unsnooze("s2"));
        assert!(!marks.unsnooze("s2"));
        marks.snooze("s3", 900, 300);
        marks.forget("s3");
        assert!(!marks.is_snoozed("s3", 300));
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
