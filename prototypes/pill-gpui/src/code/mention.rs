//! Las @-menciones de la caja de texto, como las de la referencia (`Agent.tsx`):
//! escribir `@algo` junto al cursor abre una lista con los archivos del
//! proyecto que coinciden (el índice de `files.rs`), ↑↓ mueven, Enter o Tab
//! eligen y Esc la cierra. Elegir uno deja `@ruta ` en el lugar del token.

use std::sync::Arc;

use gpui::{anchored, deferred, point, prelude::*, px, AnyElement, Context, Corner, SharedString, Window};
use gpui_m3::{rank_mentions, Exit, Suggestion, SuggestionList, SuggestionNav};

use super::files::Indexed;
use super::CodeView;

/// Cuántas sugerencias muestra la lista (las de la referencia).
pub const MAX_SUGGESTIONS: usize = 8;

/// El token `@consulta` que termina en el cursor: el byte de la `@` y lo que
/// se lleva escrito después. Como `/(^|\s)@([^\s@]*)$/` sobre el texto previo.
pub fn mention_at(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let at = before.rfind('@')?;
    let query = &before[at + 1..];
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    // La `@` va al inicio o tras un espacio: «correo@sitio» no es una mención.
    if before[..at].chars().next_back().is_some_and(|c| !c.is_whitespace()) {
        return None;
    }
    Some((at, query))
}

/// El texto con el token que empieza en `start` y llega hasta `cursor`
/// cambiado por `@ruta ` y dónde queda el cursor (después del espacio).
pub fn apply_mention(text: &str, start: usize, cursor: usize, path: &str) -> Option<(String, usize)> {
    let head = text.get(..start)?;
    let tail = text.get(cursor.max(start)..)?;
    let next = format!("{head}@{path} {tail}");
    Some((next, start + path.len() + 2))
}

/// La lista abierta: dónde empieza el token, lo escrito, las sugerencias
/// y la fila resaltada.
#[derive(Clone)]
pub struct Mention {
    pub start: usize,
    pub query: String,
    pub nav: SuggestionNav,
    pub matches: Vec<Indexed>,
}

/// Los mejores archivos para `query` (hasta [`MAX_SUGGESTIONS`]): el nombre que
/// empieza con ella, luego el que la contiene, la ruta y la subsecuencia.
pub fn rank(files: &[Indexed], query: &str) -> Vec<Indexed> {
    rank_mentions(files.iter(), query, MAX_SUGGESTIONS, |f| f.rel.as_str()).into_iter().cloned().collect()
}

impl Indexed {
    /// Cómo se escribe la mención: con varias carpetas, `carpeta/ruta`.
    pub fn mention_path(&self, several: bool) -> String {
        if several {
            format!("{}/{}", self.root, self.rel)
        } else {
            self.rel.clone()
        }
    }
}

/// Lo que dibuja la lista mientras sale: las filas, lo escrito y la resaltada.
#[derive(Clone)]
pub struct MentionShown {
    items: Vec<(SharedString, SharedString)>,
    query: SharedString,
    selected: usize,
}

/// Las teclas de la lista abierta (↑ ↓ Enter Tab Esc), en el contenedor de la caja.
pub(super) fn with_keys(el: gpui::Div, cx: &mut Context<CodeView>) -> gpui::Div {
    use gpui_m3::{SuggestionAccept, SuggestionDismiss, SuggestionNext, SuggestionPrevious};
    el.on_action(cx.listener(|view, _: &SuggestionNext, _, cx| view.mention_move(true, cx)))
        .on_action(cx.listener(|view, _: &SuggestionPrevious, _, cx| view.mention_move(false, cx)))
        .on_action(cx.listener(|view, _: &SuggestionAccept, window, cx| view.pick_mention(None, window, cx)))
        .on_action(cx.listener(|view, _: &SuggestionDismiss, _, cx| view.close_mention(cx)))
}

impl CodeView {
    /// Las @-menciones valen con un proyecto abierto (no en un chat suelto).
    pub(super) fn mentions_enabled(&self) -> bool {
        self.workspaces.active().is_some() && !self.in_loose_chat()
    }

    /// Mira el token junto al cursor y arma (o cierra) la lista.
    pub(super) fn update_mention(&mut self, cx: &mut Context<Self>) {
        let (text, cursor) = {
            let area = self.composer.read(cx);
            (area.text().to_string(), area.cursor())
        };
        let token = if self.mentions_enabled() { mention_at(&text, cursor) } else { None };
        let Some((start, query)) = token else {
            self.close_mention(cx);
            return;
        };
        let Some(index) = self.file_index.as_ref().map(|(_, files)| Arc::clone(files)) else {
            // Sin índice todavía: se arma y la lista aparece al terminar.
            self.ensure_index(cx);
            self.mention = Some(Mention { start, query: query.to_string(), nav: SuggestionNav::new(0), matches: Vec::new() });
            self.composer.update(cx, |area, cx| area.set_suggesting(false, cx));
            return;
        };
        let matches = rank(&index, query);
        let mut nav = SuggestionNav::new(matches.len());
        // Si solo cambió el cursor dentro del mismo token, la fila se mantiene.
        if let Some(old) = self.mention.as_ref().filter(|m| m.start == start && m.query == query) {
            nav = old.nav;
            nav.set_len(matches.len());
        }
        let open = !matches.is_empty();
        self.mention = Some(Mention { start, query: query.to_string(), nav, matches });
        self.composer.update(cx, |area, cx| area.set_suggesting(open, cx));
        cx.notify();
    }

    pub(super) fn close_mention(&mut self, cx: &mut Context<Self>) {
        if self.mention.take().is_some() {
            self.composer.update(cx, |area, cx| area.set_suggesting(false, cx));
            cx.notify();
        }
    }

    /// Las carpetas del espacio son varias: las menciones llevan la carpeta.
    fn several_roots(&self) -> bool {
        self.workspaces.active().is_some_and(|w| w.folders.len() > 1)
    }

    pub(super) fn mention_move(&mut self, next: bool, cx: &mut Context<Self>) {
        if let Some(mention) = self.mention.as_mut() {
            if next {
                mention.nav.next();
            } else {
                mention.nav.previous();
            }
            cx.notify();
        }
    }

    /// Elige la fila `row` (el clic, o Enter y Tab con la resaltada).
    pub(super) fn pick_mention(&mut self, row: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mention) = self.mention.clone() else {
            return;
        };
        let Some(file) = row.or(mention.nav.current()).and_then(|row| mention.matches.get(row)) else {
            return;
        };
        let path = file.mention_path(self.several_roots());
        let (text, cursor) = {
            let area = self.composer.read(cx);
            (area.text().to_string(), area.cursor())
        };
        if let Some((next, at)) = apply_mention(&text, mention.start, cursor, &path) {
            self.composer.update(cx, |area, cx| area.set_text_at(&next, at, cx));
        }
        self.close_mention(cx);
        self.focus_composer(window, cx);
    }

    /// «Mencionar archivo… @» del menú: escribe la `@` y abre la lista.
    pub(super) fn start_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mentions_enabled() {
            self.show_toast("Abre un proyecto para mencionar sus archivos", cx);
            return;
        }
        self.insert("@", window, cx);
        self.update_mention(cx);
    }

    /// La lista sobre la caja de texto; se sigue dibujando mientras sale.
    pub(super) fn mention_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let several = self.several_roots();
        let now = self.mention.as_ref().filter(|m| !m.matches.is_empty()).map(|m| MentionShown {
            items: m
                .matches
                .iter()
                .map(|f| {
                    let hint = f.mention_path(several);
                    (SharedString::from(f.name().to_string()), SharedString::from(hint))
                })
                .collect(),
            query: SharedString::from(m.query.clone()),
            selected: m.nav.selected(),
        });
        let shown = self.mention_last.show("mention-presence", now, window, cx)?;
        let bounds = self.composer_bounds.get()?;
        let items: Vec<Suggestion> = shown.value.items.iter().map(|(name, path)| Suggestion::new("file", name.clone()).hint(path.clone())).collect();
        let view = cx.entity();
        let hover_view = view.clone();
        let list = SuggestionList::new("mention-list", items)
            .query(shown.value.query.clone())
            .selected(shown.value.selected)
            .width(((bounds.size.width / px(1.)).min(460.) - 16.).max(280.))
            .on_hover(move |row, _, cx| {
                hover_view.update(cx, |view, cx| {
                    if view.mention.as_mut().is_some_and(|m| m.nav.hover(row)) {
                        cx.notify();
                    }
                });
            })
            .on_pick(move |row, window, cx| {
                view.update(cx, |view, cx| view.pick_mention(Some(row), window, cx));
            });
        Some(
            deferred(
                anchored()
                    .position(point(bounds.left() + px(8.), bounds.top() - px(8.)))
                    .anchor(Corner::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(shown.wrap(Exit::Rise, list)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn el_token_termina_en_el_cursor() {
        assert_eq!(mention_at("@", 1), Some((0, "")));
        assert_eq!(mention_at("mira @src/ma", 12), Some((5, "src/ma")));
        assert_eq!(mention_at("mira @src/ma y más", 12), Some((5, "src/ma")));
        // Tras un espacio ya no es el token.
        assert_eq!(mention_at("mira @src ", 10), None);
        // Una `@` pegada a una palabra es un correo, no una mención.
        assert_eq!(mention_at("a@b.cl", 6), None);
        // Tras un salto de línea sí vale.
        assert_eq!(mention_at("hola\n@ab", 8), Some((5, "ab")));
        // Otra `@` en el medio corta el token.
        assert_eq!(mention_at("@a@b", 4), None);
        assert_eq!(mention_at("sin nada", 8), None);
    }

    #[test]
    fn el_token_respeta_los_limites_de_caracter() {
        let text = "ver @añ";
        assert_eq!(mention_at(text, text.len()), Some((4, "añ")));
        // Un cursor en medio de una letra de dos bytes no es una posición válida.
        assert_eq!(mention_at(text, text.len() - 1), None);
    }

    #[test]
    fn elegir_reemplaza_el_token_y_deja_el_cursor_despues() {
        let (next, cursor) = apply_mention("mira @sr y más", 5, 8, "src/main.rs").unwrap();
        assert_eq!(next, "mira @src/main.rs  y más");
        assert_eq!(cursor, 5 + "src/main.rs".len() + 2);
        let (next, cursor) = apply_mention("@", 0, 1, "a.rs").unwrap();
        assert_eq!(next, "@a.rs ");
        assert_eq!(cursor, next.len());
        // Con tildes: los offsets son bytes.
        let (next, _) = apply_mention("año @ma", 5, 8, "mañana.md").unwrap();
        assert_eq!(next, "año @mañana.md ");
    }

    #[test]
    fn el_ranking_prefiere_el_nombre() {
        let file = |rel: &str| Indexed { path: PathBuf::from(rel), rel: rel.to_string(), root: "r".into() };
        let files = vec![file("docs/view/notes.md"), file("src/code/view.rs"), file("src/view.rs"), file("src/main.rs")];
        let found: Vec<String> = rank(&files, "view").into_iter().map(|f| f.rel).collect();
        assert_eq!(found[0], "src/view.rs");
        assert_eq!(found.len(), 3);
        // Sin consulta: los primeros del índice.
        assert_eq!(rank(&files, "").len(), 4);
        assert!(rank(&files, "zzz").is_empty());
        assert_eq!(files[1].mention_path(true), "r/src/code/view.rs");
        assert_eq!(files[1].mention_path(false), "src/code/view.rs");
    }
}
