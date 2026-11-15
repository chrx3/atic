//! El panel derecho de Atic Code como un solo panel con una sola tira de pestañas:
//! Archivos y Cambios (fijas, sin ✕), las consolas, la vista previa y Dispositivos (con
//! la terminal acoplada a la derecha), el diff del visor y los archivos abiertos.
//!
//! Cada parte sigue guardando lo suyo (`Terminals`, `editor::Tabs`, `doc`) y dibujando su
//! contenido; aquí se decide cuál está a la vista (`CodeView::side`), qué queda al cerrar
//! una pestaña, qué hacen los botones de la barra superior y cómo se arma la tira.

use std::path::PathBuf;

use gpui::{div, prelude::*, px, AnyElement, App, Context, SharedString, Window};
use gpui_m3::{Tab, TabStrip};

use super::terminal::Dock;
use super::{CodeView, Side};

/// Con el panel más angosto que esto, Archivos y Cambios van sin nombre si no están a la vista.
const NARROW_W: f32 = 440.0;

/// Una pestaña de la tira (su clave en `TabStrip` es `encode`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Key {
    Files,
    Changes,
    /// Una consola, la vista previa o Dispositivos (su id en `Terminals`).
    Term(u64),
    Diff,
    File(PathBuf),
}

impl Key {
    /// Archivos y Cambios siempre están: no llevan ✕.
    pub fn closable(&self) -> bool {
        !matches!(self, Key::Files | Key::Changes)
    }

    fn encode(&self) -> String {
        match self {
            Key::Files => "files".into(),
            Key::Changes => "changes".into(),
            Key::Diff => "diff".into(),
            Key::Term(id) => format!("term:{id}"),
            Key::File(path) => format!("file:{}", path.to_string_lossy()),
        }
    }

    fn decode(key: &str) -> Option<Key> {
        match key {
            "files" => Some(Key::Files),
            "changes" => Some(Key::Changes),
            "diff" => Some(Key::Diff),
            _ => match key.split_once(':')? {
                ("term", id) => id.parse().ok().map(Key::Term),
                ("file", path) => Some(Key::File(PathBuf::from(path))),
                _ => None,
            },
        }
    }
}

/// La pestaña que pasa a la vista al cerrar `closed` (que lo estaba): la de su derecha
/// o, si era la última, la de su izquierda, como `editor::after_close`. Archivos y
/// Cambios van primero y no se cierran: siempre queda una.
pub(super) fn neighbor(keys: &[Key], closed: &Key) -> Option<Key> {
    let index = keys.iter().position(|key| key == closed)?;
    keys.get(index + 1).or_else(|| index.checked_sub(1).and_then(|i| keys.get(i))).cloned()
}

/// Un botón de la barra superior (o su atajo) con el panel en `current`: salta a su
/// pestaña y abre el panel; si ya está a la vista, lo cierra.
pub(super) fn toggled(current: Option<Side>, want: Side) -> Option<Side> {
    if current == Some(want) {
        None
    } else {
        Some(want)
    }
}

/// La pestaña a la vista después de que algo cambió por fuera: la terminal a la derecha
/// que se mostró sola (una consola nueva, la vista previa que se abre, ⌘J) pasa a la
/// vista; la que ya no existe (la terminal se ocultó o se movió abajo, se cerró el diff o
/// el último archivo) cierra el panel.
pub(super) fn synced(side: Option<Side>, term_shown_here: bool, has_doc: bool, has_file: bool) -> Option<Side> {
    match side {
        _ if term_shown_here => Some(Side::Term),
        Some(Side::Term) => None,
        Some(Side::Diff) if !has_doc => None,
        Some(Side::File) if !has_file => None,
        side => side,
    }
}

/// Si el panel queda «abierto por una pestaña del momento» al pasar de `before` a
/// `after`: abrirlo con un archivo, una consola o el diff lo marca; elegir Archivos o
/// Cambios lo desmarca; lo demás no lo cambia.
pub(super) fn opened_by_item(before: Option<Side>, after: Option<Side>, was: bool) -> bool {
    match after {
        Some(Side::Files | Side::Changes) => false,
        Some(_) if before.is_none() => true,
        _ => was,
    }
}

impl CodeView {
    /// Con la terminal a la derecha, sus pestañas van en este panel.
    pub(super) fn terms_in_panel(&self, cx: &App) -> bool {
        self.terminals.read(cx).dock() == Dock::Right
    }

    /// Las pestañas en el orden de la tira.
    pub(super) fn panel_keys(&self, cx: &App) -> Vec<Key> {
        let mut keys = vec![Key::Files, Key::Changes];
        if self.terms_in_panel(cx) {
            keys.extend(self.terminals.read(cx).tab_ids().into_iter().map(Key::Term));
        }
        if self.doc.is_some() {
            keys.push(Key::Diff);
        }
        keys.extend(self.tabs.files.iter().map(|open| Key::File(open.path.clone())));
        keys
    }

    /// La pestaña a la vista, si el panel está abierto.
    pub(super) fn shown_panel_key(&self, cx: &App) -> Option<Key> {
        Some(match self.side? {
            Side::Files => Key::Files,
            Side::Changes => Key::Changes,
            Side::Diff => Key::Diff,
            Side::File => Key::File(self.tabs.current()?.path.clone()),
            Side::Term => Key::Term(self.terminals.read(cx).active_id()?),
        })
    }

    /// Pone `side` a la vista o, con `None`, cierra el panel. La terminal a la derecha deja
    /// de verse si pasa otra pestaña: no se cierra ni se mata ninguna consola.
    pub(super) fn set_side(&mut self, side: Option<Side>, cx: &mut Context<Self>) {
        self.note_opened(side);
        if side != Some(Side::Term) && self.terms_in_panel(cx) {
            self.terminals.update(cx, |terminals, cx| terminals.conceal(cx));
        }
        self.side = side;
        cx.notify();
    }

    /// Recuerda quién abrió el panel (`panel_by_item`). Elegir Archivos o Cambios cuenta
    /// como abrirlo por ellos: desde ahí, cerrar pestañas vuelve a ellos.
    fn note_opened(&mut self, side: Option<Side>) {
        self.panel_by_item = opened_by_item(self.side, side, self.panel_by_item);
    }

    /// Después de cerrar una pestaña a la vista: su vecina, o cerrar el panel si lo abrió
    /// un archivo o una consola y solo quedarían Archivos o Cambios.
    pub(super) fn after_panel_close(&mut self, next: Option<Key>, window: Option<&mut Window>, cx: &mut Context<Self>) {
        match next {
            Some(Key::Files | Key::Changes) if self.panel_by_item => self.set_side(None, cx),
            Some(next) => self.reveal_panel_key(next, window, cx),
            None => {}
        }
    }

    /// Una vez por cuadro, antes de dibujar (ver `synced`).
    pub(super) fn sync_panel(&mut self, cx: &mut Context<Self>) {
        let term_shown_here = self.terms_in_panel(cx) && self.terminals.read(cx).shown();
        let side = synced(self.side, term_shown_here, self.doc.is_some(), self.tabs.showing());
        if side != self.side {
            self.note_opened(side);
            self.side = side;
            cx.notify();
        }
    }

    /// Pone a la vista la pestaña `key`: un clic en la tira o la vecina de la que se
    /// cerró. Con la ventana, la consola toma el foco (el editor lo toma solo).
    pub(super) fn reveal_panel_key(&mut self, key: Key, window: Option<&mut Window>, cx: &mut Context<Self>) {
        match key {
            Key::Files => self.set_side(Some(Side::Files), cx),
            Key::Changes => self.set_side(Some(Side::Changes), cx),
            Key::Diff if self.doc.is_some() => self.set_side(Some(Side::Diff), cx),
            Key::Diff => {}
            Key::File(path) => {
                if let Some(index) = self.tabs.index_of(&path) {
                    self.tabs.select(index);
                    self.active_file = Some(path);
                    self.set_side(Some(Side::File), cx);
                }
            }
            Key::Term(id) => {
                self.terminals.update(cx, |terminals, cx| match window {
                    Some(window) => terminals.select(id, window, cx),
                    None => terminals.reveal(id, cx),
                });
                self.note_opened(Some(Side::Term));
                self.side = Some(Side::Term);
                cx.notify();
            }
        }
    }

    /// Si `key` está a la vista, la que la reemplaza al cerrarla.
    pub(super) fn next_after_close(&self, key: &Key, cx: &App) -> Option<Key> {
        if self.shown_panel_key(cx).as_ref() != Some(key) {
            return None;
        }
        neighbor(&self.panel_keys(cx), key)
    }

    /// Cierra la pestaña `key` (su ✕, el clic de la rueda o ⌘W). Un archivo con cambios
    /// sin guardar pregunta antes (`request_close`; el cierre sigue en `close_tab`).
    pub(super) fn close_panel_tab(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        match &key {
            Key::Files | Key::Changes => {}
            Key::File(path) => {
                if let Some(index) = self.tabs.index_of(path) {
                    self.request_close(index, cx);
                }
                // Si quedó una consola a la vista, toma el foco (un archivo lo toma solo).
                let term = self.terminals.read(cx).active_id().filter(|_| self.side == Some(Side::Term));
                if let Some(id) = term {
                    self.terminals.update(cx, |terminals, cx| terminals.select(id, window, cx));
                }
            }
            Key::Diff => {
                let next = self.next_after_close(&key, cx);
                self.doc = None;
                self.after_panel_close(next, None, cx);
            }
            Key::Term(id) => {
                let next = self.next_after_close(&key, cx);
                let id = *id;
                self.terminals.update(cx, |terminals, cx| terminals.close(id, window, cx));
                self.after_panel_close(next, Some(window), cx);
            }
        }
        cx.notify();
    }

    /// La tira del panel: Archivos, Cambios, las pestañas de la terminal (si está a la
    /// derecha, con «+» y los agentes al final), el diff y los archivos abiertos. También
    /// el menú de los agentes, si está abierto.
    pub(super) fn panel_strip(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // En un panel angosto, Archivos y Cambios dicen su nombre solo a la vista (y en el
        // globito): así caben las pestañas del momento.
        let narrow = self.widths.get().right < NARROW_W;
        let shown = self.side;
        let fixed = |key: Key, side: Side, name: &'static str, icon: &'static str| {
            let name = crate::i18n::t(name);
            let label = if narrow && shown != Some(side) { SharedString::default() } else { name.into() };
            Tab::new(key.encode(), label).icon(icon).tooltip(name).closable(false)
        };
        let mut tabs = vec![
            fixed(Key::Files, Side::Files, "pill.code.view.files", "folder"),
            fixed(Key::Changes, Side::Changes, "pill.code.view.changes", "branch"),
        ];
        let terms = self.terms_in_panel(cx).then(|| {
            self.terminals.update(cx, |terminals, cx| {
                (terminals.strip_tabs("term:", cx), terminals.editing(), terminals.new_buttons(cx), terminals.agent_menu_layer(cx))
            })
        });
        let (term_tabs, editing, new_buttons, agent_menu) = match terms {
            Some((tabs, editing, buttons, menu)) => (tabs, editing, Some(buttons), menu),
            None => (Vec::new(), None, None, None),
        };
        tabs.extend(term_tabs);
        if let Some(doc) = &self.doc {
            tabs.push(
                Tab::new(Key::Diff.encode(), super::view::file_name(&doc.path))
                    .icon("diff")
                    .tooltip(crate::i18n::tf("pill.code.panel.diffTip", &[("path", &doc.path.display())])),
            );
        }
        for (index, open) in self.tabs.files.iter().enumerate() {
            tabs.push(
                Tab::new(Key::File(open.path.clone()).encode(), super::view::file_name(&open.path))
                    .icon("file")
                    .tooltip(open.path.display().to_string())
                    .dirty(self.tab_dirty(index, cx)),
            );
        }
        let on_key = |cx: &mut Context<Self>, f: fn(&mut CodeView, Key, &mut Window, &mut Context<CodeView>)| {
            let this = cx.weak_entity();
            move |key: SharedString, window: &mut Window, cx: &mut App| {
                if let Some(key) = Key::decode(&key) {
                    let _ = this.update(cx, |view, cx| f(view, key, window, cx));
                }
            }
        };
        let mut strip = TabStrip::new("panel-tabs")
            .tabs(tabs)
            // Un clic en una pestaña: desde ahí el panel es de quien lo está usando.
            .on_select(on_key(cx, |view, key, window, cx| {
                view.panel_by_item = false;
                view.reveal_panel_key(key, Some(window), cx)
            }))
            .on_close(on_key(cx, |view, key, window, cx| view.close_panel_tab(key, window, cx)))
            // Doble clic: renombrar una consola.
            .on_rename(on_key(cx, |view, key, window, cx| {
                if let Key::Term(id) = key {
                    view.terminals.update(cx, |terminals, cx| terminals.start_rename(id, window, cx));
                }
            }))
            // Clic derecho en un archivo o en el diff: el menú del archivo.
            .on_context({
                let this = cx.weak_entity();
                move |key: SharedString, at, window: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |view, cx| {
                        let path = match Key::decode(&key) {
                            Some(Key::File(path)) => Some(path),
                            Some(Key::Diff) => view.doc.as_ref().map(|doc| doc.path.clone()),
                            _ => None,
                        };
                        if let Some(path) = path {
                            view.open_file_menu(path, false, at, window, cx);
                        }
                    });
                }
            });
        if let Some(key) = self.shown_panel_key(cx) {
            strip = strip.selected(key.encode());
        }
        if let Some((id, field)) = editing {
            strip = strip.editing(Key::Term(id).encode(), field);
        }
        if let Some(buttons) = new_buttons {
            strip = strip.trailing(buttons);
        }
        div().flex_1().min_w(px(0.)).child(strip).children(agent_menu).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<Key> {
        vec![Key::Files, Key::Changes, Key::Term(1), Key::Term(2), Key::Diff, Key::File("a.rs".into())]
    }

    #[test]
    fn al_cerrar_queda_la_de_la_derecha_o_la_de_la_izquierda() {
        let keys = keys();
        assert_eq!(neighbor(&keys, &Key::Term(1)), Some(Key::Term(2)));
        assert_eq!(neighbor(&keys, &Key::Diff), Some(Key::File("a.rs".into())));
        // La última: la de su izquierda.
        assert_eq!(neighbor(&keys, &Key::File("a.rs".into())), Some(Key::Diff));
        // La única consola, sin nada más a la derecha: vuelve a Cambios.
        assert_eq!(neighbor(&[Key::Files, Key::Changes, Key::Term(7)], &Key::Term(7)), Some(Key::Changes));
        assert_eq!(neighbor(&keys, &Key::Term(9)), None);
    }

    #[test]
    fn archivos_y_cambios_no_se_cierran() {
        assert!(!Key::Files.closable());
        assert!(!Key::Changes.closable());
        assert!(keys().iter().skip(2).all(Key::closable));
    }

    #[test]
    fn la_clave_de_la_tira_va_y_vuelve() {
        for key in keys().into_iter().chain([Key::File("/tmp/con: dos puntos.txt".into())]) {
            assert_eq!(Key::decode(&key.encode()), Some(key));
        }
        assert_eq!(Key::decode("otra"), None);
        assert_eq!(Key::decode("term:x"), None);
    }

    #[test]
    fn un_boton_salta_a_su_pestana_o_cierra_el_panel() {
        // Cerrado: lo abre en su pestaña.
        assert_eq!(toggled(None, Side::Files), Some(Side::Files));
        // Con otra a la vista (un archivo, una consola): salta a la suya.
        assert_eq!(toggled(Some(Side::File), Side::Files), Some(Side::Files));
        assert_eq!(toggled(Some(Side::Term), Side::Changes), Some(Side::Changes));
        // Ya a la vista: cierra el panel.
        assert_eq!(toggled(Some(Side::Changes), Side::Changes), None);
    }

    #[test]
    fn lo_que_abrio_el_panel_lo_cierra_al_irse() {
        // Un archivo abre el panel cerrado: al cerrarlo, se cierra el panel.
        assert!(opened_by_item(None, Some(Side::File), false));
        // Pasar a otra pestaña del momento no lo cambia.
        assert!(opened_by_item(Some(Side::File), Some(Side::Term), true));
        // Elegir Cambios o Archivos: desde ahí el panel se queda en ellos.
        assert!(!opened_by_item(Some(Side::File), Some(Side::Changes), true));
        assert!(!opened_by_item(None, Some(Side::Files), false));
        // Con el panel abierto en Cambios, abrir un archivo no lo marca.
        assert!(!opened_by_item(Some(Side::Changes), Some(Side::File), false));
    }

    #[test]
    fn la_terminal_que_se_muestra_sola_pasa_a_la_vista() {
        // Una consola nueva (Ctrl+T, ⌘J, la vista previa) con Archivos a la vista o el panel cerrado.
        assert_eq!(synced(Some(Side::Files), true, false, false), Some(Side::Term));
        assert_eq!(synced(None, true, false, false), Some(Side::Term));
        // Se ocultó (⌘J otra vez) o se movió abajo: el panel se cierra.
        assert_eq!(synced(Some(Side::Term), false, true, true), None);
        // Lo que ya no existe también lo cierra; lo demás sigue igual.
        assert_eq!(synced(Some(Side::Diff), false, false, true), None);
        assert_eq!(synced(Some(Side::File), false, true, false), None);
        assert_eq!(synced(Some(Side::Changes), false, false, false), Some(Side::Changes));
        assert_eq!(synced(None, false, true, true), None);
    }
}
