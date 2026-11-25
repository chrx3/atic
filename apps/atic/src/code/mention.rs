//! Las @-menciones de la caja de texto, como las de la referencia (`Agent.tsx`):
//! escribir `@algo` junto al cursor abre una lista con los archivos del
//! proyecto que coinciden (el índice de `files.rs`), ↑↓ mueven, Enter o Tab
//! eligen y Esc la cierra. Elegir uno deja `@ruta ` en el lugar del token.

use crate::i18n::{t, tf};
use std::path::{Path, PathBuf};

use gpui::{anchored, deferred, div, point, prelude::*, px, AnyElement, Context, Corner, Div, Hsla, SharedString, Window};
use gpui_m3::{match_ranges, rank_mentions, Chip, Icon, Kbd, Menu, MenuItem, Scheme, SuggestionNav};

use super::files::{self, Indexed};
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
/// (abiertos y cambiados primero) y la fila resaltada.
#[derive(Clone)]
pub struct Mention {
    pub start: usize,
    pub query: String,
    pub nav: SuggestionNav,
    pub matches: Vec<Indexed>,
    groups: Vec<MentionGroup>,
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

/// En qué parte de la lista va un archivo: abierto en una pestaña, cambiado
/// según git, o el resto del proyecto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum MentionGroup {
    Open,
    Changed,
    Rest,
}

/// El grupo de `path` según las pestañas abiertas y los cambios de git.
fn group_of(path: &Path, open: &[PathBuf], changed: &[PathBuf]) -> MentionGroup {
    if open.iter().any(|p| super::same_path(p, path)) {
        MentionGroup::Open
    } else if changed.iter().any(|p| super::same_path(p, path)) {
        MentionGroup::Changed
    } else {
        MentionGroup::Rest
    }
}

/// Los archivos con abiertos y cambiados primero, sin cambiar el ranking que
/// ya traen dentro de cada grupo.
fn group_mentions(files: Vec<Indexed>, open: &[PathBuf], changed: &[PathBuf]) -> Vec<Indexed> {
    let mut groups: [Vec<Indexed>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for f in files {
        groups[group_of(&f.path, open, changed) as usize].push(f);
    }
    groups.into_iter().flatten().collect()
}

/// El título de cada sección de la lista.
fn group_title(group: MentionGroup) -> &'static str {
    match group {
        MentionGroup::Open => t("pill.code.mention.openSection"),
        MentionGroup::Changed => t("pill.code.mention.changedSection"),
        MentionGroup::Rest => t("pill.code.mention.projectSection"),
    }
}

/// El color del ícono según el tipo de archivo, con roles del tema para que se
/// lea en claro y oscuro (el código lleva el primario, como el árbol).
fn kind_tint(kind: files::Kind, scheme: &Scheme) -> Hsla {
    match kind {
        files::Kind::Code => scheme.primary,
        files::Kind::Style => scheme.tertiary,
        files::Kind::Doc => scheme.secondary,
        files::Kind::Config => scheme.warning,
        files::Kind::Image => scheme.success,
        files::Kind::Other => scheme.on_surface_variant,
    }
}

/// El pie de la lista: los atajos con su tecla.
fn hint_footer(scheme: &Scheme, hints: &[(&'static str, &'static str)]) -> Div {
    let mut row =
        div().flex().flex_wrap().items_center().justify_center().gap(px(10.)).px(px(8.)).pt(px(4.));
    for &(key, label) in hints {
        row = row.child(
            div().flex().items_center().gap(px(4.))
                .child(Kbd::new(key).small(true).muted(true))
                .child(div().text_size(px(11.)).text_color(scheme.on_surface_variant).child(label)),
        );
    }
    row
}

/// Una fila de la lista: el nombre, la carpeta, la ruta para mencionar y su grupo.
#[derive(Clone)]
struct MentionRow {
    name: SharedString,
    dir: SharedString,
    kind: files::Kind,
    group: MentionGroup,
}

/// Lo que dibuja la lista mientras sale: las filas, lo escrito, la resaltada y
/// si el índice todavía se está armando.
#[derive(Clone)]
pub struct MentionShown {
    rows: Vec<MentionRow>,
    query: SharedString,
    selected: usize,
    loading: bool,
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
        // Sin índice todavía: se arma en segundo plano y la lista muestra la espera.
        let loading = self.file_index.is_none();
        if loading {
            self.ensure_index(cx);
        }
        let matches = self.file_index.as_ref().map(|(_, files)| rank(files, query)).unwrap_or_default();
        // Abiertos y cambiados primero, sin cambiar el ranking dentro de cada grupo.
        let open_paths: Vec<PathBuf> = self.tabs.files.iter().map(|f| f.path.clone()).collect();
        let changed_paths: Vec<PathBuf> =
            self.repos.iter().flat_map(|r| r.files.iter().map(|f| f.path.clone())).collect();
        let matches = group_mentions(matches, &open_paths, &changed_paths);
        let groups = matches.iter().map(|f| group_of(&f.path, &open_paths, &changed_paths)).collect();
        let mut nav = SuggestionNav::new(matches.len());
        // Si solo cambió el cursor dentro del mismo token, la fila se mantiene.
        if let Some(old) = self.mention.as_ref().filter(|m| m.start == start && m.query == query) {
            nav = old.nav;
            nav.set_len(matches.len());
        }
        self.mention = Some(Mention { start, query: query.to_string(), nav, matches, groups });
        // La lista se ve también cargando o sin resultados: Esc la cierra.
        self.composer.update(cx, |area, cx| area.set_suggesting(true, cx));
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

    /// El «Borrar filtro» del estado sin resultados: deja solo la `@` y vuelve
    /// a listar todos los archivos.
    pub(super) fn clear_mention_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mention) = self.mention.clone() else {
            return;
        };
        let (text, cursor) = {
            let area = self.composer.read(cx);
            (area.text().to_string(), area.cursor())
        };
        let (Some(head), Some(tail)) =
            (text.get(..mention.start), text.get(cursor.max(mention.start)..))
        else {
            return;
        };
        let next = format!("{head}@{tail}");
        let at = mention.start + 1;
        self.composer.update(cx, |area, cx| area.set_text_at(&next, at, cx));
        self.update_mention(cx);
        self.focus_composer(window, cx);
    }

    /// «Mencionar archivo… @» del menú: escribe la `@` y abre la lista.
    pub(super) fn start_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mentions_enabled() {
            self.show_toast(t("pill.code.mention.openProject"), cx);
            return;
        }
        self.insert("@", window, cx);
        self.update_mention(cx);
    }

    /// La lista sobre la caja de texto; se sigue dibujando mientras sale.
    pub(super) fn mention_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let scheme = *gpui_m3::Theme::of(cx);
        let several = self.several_roots();
        let loading = self.file_index.is_none();
        // Las filas en el orden en que se muestran (el de `update_mention`).
        let now = self.mention.as_ref().map(|m| MentionShown {
            rows: m
                .matches
                .iter()
                .zip(m.groups.iter())
                .map(|(f, group)| {
                    let dir = f.dir(several);
                    MentionRow {
                        name: SharedString::from(f.name().to_string()),
                        dir: SharedString::from(if dir.is_empty() {
                            t("pill.code.mention.noDir").to_string()
                        } else {
                            dir
                        }),
                        kind: files::kind_of(f.name()),
                        group: *group,
                    }
                })
                .collect(),
            query: SharedString::from(m.query.clone()),
            selected: m.nav.selected(),
            loading,
        });
        let shown = self.mention_last.show("mention-presence", now, window, cx)?;
        let bounds = self.composer_bounds.get()?;
        // Abre hacia arriba desde la caja: el pie con las pistas queda del lado del
        // ancla y no se mueve al filtrar (solo la lista cambia de alto).
        let mut menu = Menu::new("mention-list")
            .width(((bounds.size.width / px(1.)).min(460.) - 16.).max(280.))
            .max_h(px(320.))
            .origin(Corner::BottomLeft)
            .pin_to_anchor(true);
        if shown.value.loading {
            menu = menu.section(t("pill.code.mention.loadingTitle"));
            menu = menu.item(
                MenuItem::new("mention-loading", t("pill.code.mention.loadingRow"))
                    .icon("file")
                    .static_row(true),
            );
            menu = menu.footer(hint_footer(&scheme, &[("Esc", t("pill.code.mention.hintClose"))]));
        } else if shown.value.rows.is_empty() {
            let view = cx.entity();
            menu = menu.item(
                MenuItem::new("mention-empty", tf("pill.code.mention.noResults", &[("query", &shown.value.query)]))
                    .icon("search")
                    .static_row(true)
                    .action(Chip::new("mention-clear", t("pill.code.mention.clearFilter")).on_click(
                        move |_, window, cx| {
                            view.update(cx, |view, cx| view.clear_mention_query(window, cx));
                        },
                    )),
            );
            menu = menu.footer(hint_footer(&scheme, &[("Esc", t("pill.code.mention.hintClose"))]));
        } else {
            // Secciones solo cuando aportan: con un solo grupo la lista va plana.
            let multi = shown.value.rows.iter().any(|r| r.group != shown.value.rows[0].group);
            let mut last: Option<MentionGroup> = None;
            let view = cx.entity();
            let hover_view = view.clone();
            for (row, item) in shown.value.rows.iter().enumerate() {
                if multi && Some(item.group) != last {
                    menu = menu.section(group_title(item.group));
                    last = Some(item.group);
                }
                let pick_view = view.clone();
                let entry = MenuItem::new(("mention-pick", row), item.name.clone())
                    .leading(Icon::new("file").size(px(16.)).color(kind_tint(item.kind, &scheme)))
                    .sublabel(item.dir.clone())
                    .label_highlights(match_ranges(&item.name, &shown.value.query))
                    .selected(row == shown.value.selected)
                    .hover_fill(false)
                    .on_click(move |_, window, cx| {
                        pick_view.update(cx, |view, cx| view.pick_mention(Some(row), window, cx));
                    });
                let hover = hover_view.clone();
                menu = menu.item(div().id(("mention-row", row)).on_mouse_move(move |_, _, cx| {
                    hover.update(cx, |view, cx| {
                        if view.mention.as_mut().is_some_and(|m| m.nav.hover(row)) {
                            cx.notify();
                        }
                    });
                }).child(entry));
            }
            menu = menu.footer(hint_footer(
                &scheme,
                &[
                    ("↑↓", t("pill.code.mention.hintMove")),
                    ("Enter", t("pill.code.mention.hintPick")),
                    ("Esc", t("pill.code.mention.hintClose")),
                ],
            ));
        }
        Some(
            deferred(
                anchored()
                    .position(point(bounds.left() + px(8.), bounds.top() - px(8.)))
                    .anchor(Corner::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(shown.shrink(gpui_m3::PopDirection::Up, menu)),
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

    #[test]
    fn los_abiertos_y_cambiados_van_primero_sin_romper_el_ranking() {
        let file = |rel: &str| Indexed { path: PathBuf::from(rel), rel: rel.to_string(), root: "r".into() };
        let files = vec![file("src/view.rs"), file("src/code/view.rs"), file("docs/view/notes.md")];
        let open = vec![PathBuf::from("docs/view/notes.md")];
        let changed = vec![PathBuf::from("src/code/view.rs")];
        let grouped: Vec<String> =
            group_mentions(files, &open, &changed).into_iter().map(|f| f.rel).collect();
        assert_eq!(grouped, vec!["docs/view/notes.md", "src/code/view.rs", "src/view.rs"]);
        // Sin abiertos ni cambiados, el orden no cambia.
        let files = vec![file("b.rs"), file("a.rs")];
        let grouped: Vec<String> =
            group_mentions(files, &[], &[]).into_iter().map(|f| f.rel).collect();
        assert_eq!(grouped, vec!["b.rs", "a.rs"]);
        // Abierto y cambiado a la vez va en abiertos, una sola vez.
        assert_eq!(
            group_of(Path::new("x.rs"), &[PathBuf::from("x.rs")], &[PathBuf::from("x.rs")]),
            MentionGroup::Open
        );
        assert_eq!(group_of(Path::new("y.rs"), &[], &[PathBuf::from("Y.RS")]), MentionGroup::Changed);
    }
}
