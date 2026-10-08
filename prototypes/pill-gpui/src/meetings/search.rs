//! Buscar en la lista y los encabezados de día que quedan pegados arriba.
//!
//! La búsqueda mira el título y también lo que se dijo y lo que se resumió:
//! al releer la lista se arma (fuera del hilo de UI) un índice con el texto de
//! cada reunión, normalizado sin tildes ni mayúsculas. Coinciden las reuniones
//! que tienen todas las palabras, en el título o en el contenido; las que
//! coinciden por contenido muestran el pedazo donde aparece.
//!
//! El índice guarda la fecha de los archivos: al releer solo se vuelve a leer
//! lo que cambió.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{Local, NaiveDate};
use gpui::{
    actions, div, point, prelude::*, px, svg, AnyElement, App, ClickEvent, Context, Entity,
    Focusable, FontWeight, HighlightStyle, KeyBinding, NoAction, ScrollHandle, SharedString,
    StyledText, Subscription, Window,
};

use super::data::Source;
use super::summary::{self, Block as SummaryBlock, Section};
use super::text::{self, Block};
use super::{hsla, MeetingsView, FAINT, KEY_CONTEXT, MUTED, SURFACE, TEXT};
use crate::hover;
use crate::text_input::{self, TextInput};

actions!(meetings_search, [FocusSearch, ClearSearch, LeaveSearch]);

/// El campo de búsqueda: Esc lo limpia, Enter vuelve a la lista. Las flechas
/// ↑/↓ siguen hacia la lista (recorren lo filtrado).
const SEARCH_CONTEXT: &str = "MeetingsSearch";

pub fn bind_keys(cx: &mut App) {
    let search = Some(SEARCH_CONTEXT);
    cx.bind_keys([
        // Ctrl+F es de la transcripción.
        KeyBinding::new("ctrl-k", FocusSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", ClearSearch, search),
        KeyBinding::new("enter", LeaveSearch, search),
        // Escribiendo, el espacio es un espacio (no ▶) y Ctrl+Z no resucita
        // una reunión borrada.
        KeyBinding::new("space", NoAction, search),
        KeyBinding::new("ctrl-z", NoAction, search),
    ]);
}

/// El pozo del buscador: más oscuro que el panel (como los campos de Ajustes).
const FIELD: u32 = crate::theme::SUNKEN;
const FIELD_H: f32 = 34.0;

// --- Texto -------------------------------------------------------------------------

/// Una letra en minúscula y sin tilde. Siempre una letra por letra: así una
/// posición en el texto normalizado es la misma en el original.
fn fold(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        c if c.is_whitespace() => ' ',
        c => c,
    }
}

pub(super) fn normalize(text: &str) -> String {
    text.chars().map(fold).collect()
}

/// Las palabras buscadas, normalizadas y sin signos en los bordes.
pub(super) fn terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in normalize(query).split(' ') {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        if !word.is_empty() && !out.iter().any(|w| w == word) {
            out.push(word.to_string());
        }
    }
    out
}

/// El texto de una reunión para buscar: el resumen y después lo que se dijo,
/// en una sola línea.
pub(super) fn doc_text(sections: &[Section], blocks: &[Block]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for section in sections {
        for block in &section.blocks {
            match block {
                SummaryBlock::Paragraph(text) => parts.push(text),
                SummaryBlock::List { items, .. } => parts.extend(items.iter().map(|i| i.text.as_str())),
            }
        }
    }
    parts.extend(blocks.iter().map(|b| b.text.as_str()));
    let joined = parts.join(" · ");
    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lo indexado de una reunión.
pub(super) struct Doc {
    stamp: Stamp,
    text: String,
    norm: String,
}

/// Las fechas de la transcripción y del resumen al leerlos.
type Stamp = (Option<SystemTime>, Option<SystemTime>);

impl Doc {
    pub(super) fn new(text: String) -> Self {
        Self { stamp: (None, None), norm: normalize(&text), text }
    }
}

/// El pedazo del contenido donde aparece lo buscado; `marks` son rangos (en
/// bytes de `text`) de las palabras encontradas.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Snippet {
    pub text: String,
    pub marks: Vec<Range<usize>>,
}

#[derive(Debug, PartialEq)]
pub(super) enum Hit {
    Title,
    Content(Snippet),
}

/// ¿Coincide? Todas las palabras tienen que estar en el título o en el
/// contenido. Si alguna está solo en el contenido, va el pedazo donde aparece
/// primero.
pub(super) fn find(title: &str, doc: Option<&Doc>, terms: &[String]) -> Option<Hit> {
    let title = normalize(title);
    let mut first: Option<usize> = None;
    for term in terms {
        if title.contains(term.as_str()) {
            continue;
        }
        let at = doc?.norm.find(term.as_str())?;
        first = Some(first.map_or(at, |b| b.min(at)));
    }
    match (first, doc) {
        (Some(at), Some(doc)) => Some(Hit::Content(snippet(&doc.text, &doc.norm, at, terms))),
        _ => Some(Hit::Title),
    }
}

/// Letras de contexto antes de lo encontrado y largo del pedazo: una línea
/// de la fila (~280 px a 12 px son unas 48 letras), con margen para las
/// anchas y para no partir palabras.
const BEFORE: usize = 14;
const SNIPPET: usize = 44;

/// El pedazo de `text` alrededor de `at` (posición en bytes de `norm`), sin
/// cortar palabras, con «…» donde sigue y las palabras marcadas.
pub(super) fn snippet(text: &str, norm: &str, at: usize, terms: &[String]) -> Snippet {
    let at_char = norm[..at].chars().count();
    let mut start = at_char.saturating_sub(BEFORE);
    let window: Vec<char> = text.chars().skip(start).take(SNIPPET + BEFORE).collect();
    let mut from = 0;
    if start > 0 {
        // Desde la palabra siguiente, si hay una antes de lo encontrado.
        if let Some(space) = window[..at_char - start].iter().position(|c| *c == ' ') {
            from = space + 1;
        }
    }
    // Ni espacios ni el «·» que separa partes al comienzo.
    while start > 0 && window.get(from).is_some_and(|c| *c == ' ' || *c == '·') && from < at_char - start {
        from += 1;
    }
    start += from;
    let mut chars: Vec<char> = window[from..].to_vec();
    let cut = chars.len() > SNIPPET;
    let more = cut || start + chars.len() < text.chars().count();
    if cut {
        chars.truncate(SNIPPET);
        // Sin dejar media palabra, pero sin comerse lo encontrado.
        let found_end = (at_char - start + 1).min(chars.len());
        if let Some(space) = chars[found_end..].iter().rposition(|c| *c == ' ') {
            chars.truncate(found_end + space);
        }
    }
    while chars.last().is_some_and(|c| *c == ' ' || *c == '·') {
        chars.pop();
    }

    // Las marcas se buscan en el pedazo normalizado (misma cantidad de letras).
    let folded: Vec<char> = chars.iter().map(|c| fold(*c)).collect();
    let mut spans: Vec<Range<usize>> = Vec::new();
    for term in terms {
        let needle: Vec<char> = term.chars().collect();
        if needle.is_empty() || needle.len() > folded.len() {
            continue;
        }
        for i in 0..=folded.len() - needle.len() {
            if folded[i..i + needle.len()] == needle[..] {
                spans.push(i..i + needle.len());
            }
        }
    }
    spans.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }

    let lead = if start > 0 { "…" } else { "" };
    let mut out = String::from(lead);
    let mut offsets = Vec::with_capacity(chars.len() + 1);
    for c in &chars {
        offsets.push(out.len());
        out.push(*c);
    }
    offsets.push(out.len());
    // Tras un punto final, la frase ya cerró: sin «…» pegado.
    if more && !chars.last().is_some_and(|c| matches!(c, '.' | '!' | '?')) {
        out.push('…');
    }
    let marks = merged.into_iter().map(|r| offsets[r.start]..offsets[r.end]).collect();
    Snippet { text: out, marks }
}

// --- Índice ------------------------------------------------------------------------

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Lee lo que cambió desde `old` (en un hilo aparte).
fn build_index(source: Source, ids: Vec<String>, old: Arc<HashMap<String, Arc<Doc>>>) -> HashMap<String, Arc<Doc>> {
    let mut docs = HashMap::with_capacity(ids.len());
    for id in ids {
        let stamp: Stamp = match source.paths() {
            Some(paths) => (modified(&paths.transcript_path(&id)), modified(&paths.summary_path(&id))),
            None => (None, None),
        };
        if let Some(doc) = old.get(&id).filter(|d| d.stamp == stamp) {
            docs.insert(id, doc.clone());
            continue;
        }
        let sections = source
            .summary(&id)
            .ok()
            .flatten()
            .map(|s| summary::parse(&s.body, "Resumen"))
            .unwrap_or_default();
        let blocks = source
            .transcript(&id)
            .ok()
            .flatten()
            .map(|t| text::blocks(&t.segments))
            .unwrap_or_default();
        let mut doc = Doc::new(doc_text(&sections, &blocks));
        doc.stamp = stamp;
        docs.insert(id, Arc::new(doc));
    }
    docs
}

// --- Estado --------------------------------------------------------------------------

/// Lo que va en la lista, en orden: encabezados de día y filas (índice en
/// `items`). Son los hijos directos del scroll.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Slot {
    Day(NaiveDate),
    Row(usize),
}

#[derive(Default)]
pub(super) struct Search {
    input: Option<Entity<TextInput>>,
    _changed: Option<Subscription>,
    terms: Vec<String>,
    docs: Arc<HashMap<String, Arc<Doc>>>,
    /// Sube con cada relectura: un índice que llega tarde se descarta.
    epoch: u64,
    /// Las filas que se ven, en orden (índices en `items`); todas sin búsqueda.
    visible: Vec<usize>,
    snippets: HashMap<usize, Snippet>,
    pub(super) scroll: ScrollHandle,
}

impl MeetingsView {
    pub(super) fn install_search(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Buscar", hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        let changed = cx.subscribe(&input, |v, input, _: &text_input::Changed, cx| {
            let terms = terms(input.read(cx).text());
            if terms != v.search.terms {
                v.search.terms = terms;
                v.search.scroll.set_offset(point(px(0.), px(0.)));
                v.apply_search(true, cx);
            }
        });
        self.search.input = Some(input);
        self.search._changed = Some(changed);
    }

    /// Tras releer la lista: se filtra al tiro con lo que hay y el índice se
    /// pone al día por detrás.
    pub(super) fn refresh_search(&mut self, cx: &mut Context<Self>) {
        self.apply_search(false, cx);
        let source = match self.source.as_ref() {
            Some(source) => match source.paths() {
                Some(paths) => Source::Atic(paths.clone()),
                None => Source::Demo,
            },
            None => return,
        };
        self.search.epoch += 1;
        let epoch = self.search.epoch;
        let ids: Vec<String> = self.items.iter().map(|r| r.id.clone()).collect();
        let old = self.search.docs.clone();
        cx.spawn(async move |this, cx| {
            let docs = cx.background_spawn(async move { build_index(source, ids, old) }).await;
            let _ = this.update(cx, |v, cx| {
                if v.search.epoch != epoch {
                    return;
                }
                v.search.docs = Arc::new(docs);
                if !v.search.terms.is_empty() {
                    v.apply_search(false, cx);
                }
            });
        })
        .detach();
    }

    /// Recalcula lo que se ve. `follow`: si la elegida quedó fuera, se elige
    /// la primera que coincide (al escribir; al releer no se mueve nada).
    fn apply_search(&mut self, follow: bool, cx: &mut Context<Self>) {
        let search = &mut self.search;
        search.snippets.clear();
        search.visible.clear();
        if search.terms.is_empty() {
            search.visible.extend(0..self.items.len());
        } else {
            for (ix, rec) in self.items.iter().enumerate() {
                match find(&rec.title, search.docs.get(&rec.id).map(|d| &**d), &search.terms) {
                    Some(Hit::Title) => search.visible.push(ix),
                    Some(Hit::Content(snippet)) => {
                        search.visible.push(ix);
                        search.snippets.insert(ix, snippet);
                    }
                    None => {}
                }
            }
        }
        let first = search.visible.first().copied();
        let outside = !self.selected.is_some_and(|ix| self.search.visible.contains(&ix));
        if follow && outside && first.is_some() {
            self.load_detail(first, true);
        }
        cx.notify();
    }

    /// Las filas que se ven, en orden: lo que recorren ↑ y ↓.
    pub(super) fn visible_rows(&self) -> &[usize] {
        &self.search.visible
    }

    pub(super) fn searching(&self) -> bool {
        !self.search.terms.is_empty()
    }

    /// Los hijos del scroll de la lista: un encabezado al cambiar de día.
    pub(super) fn list_slots(&self) -> Vec<Slot> {
        let mut slots = Vec::with_capacity(self.search.visible.len() + 8);
        let mut current: Option<NaiveDate> = None;
        for &ix in &self.search.visible {
            let Some(rec) = self.items.get(ix) else { continue };
            let day = rec.started_at.with_timezone(&Local).date_naive();
            if current != Some(day) {
                current = Some(day);
                slots.push(Slot::Day(day));
            }
            slots.push(Slot::Row(ix));
        }
        slots
    }

    /// Deja la fila a la vista. Subiendo se lleva al hijo anterior (su
    /// encabezado o la fila de arriba): así no queda bajo el encabezado fijo.
    pub(super) fn reveal_row(&self, ix: usize, up: bool) {
        let Some(child) = self.list_slots().iter().position(|s| *s == Slot::Row(ix)) else {
            return;
        };
        let target = if up { child.saturating_sub(1) } else { child };
        self.search.scroll.scroll_to_item(target);
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.search.input {
            window.focus(&input.focus_handle(cx));
        }
    }

    /// Si se estaba escribiendo en el buscador, el foco vuelve a la lista
    /// (sin borrar lo buscado): así Espacio vuelve a ser ▶.
    pub(super) fn leave_search(&self, window: &mut Window, cx: &App) {
        if self.search.input.as_ref().is_some_and(|i| i.focus_handle(cx).is_focused(window)) {
            window.focus(&self.focus);
        }
    }

    /// Vacía el campo (al importar, para que lo nuevo se vea).
    pub(super) fn clear_search_text(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.search.input.clone() {
            if !input.read(cx).text().is_empty() {
                input.update(cx, |input, cx| input.clear(cx));
            }
        }
    }

    pub(super) fn search_actions(el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        el.on_action(cx.listener(|v, _: &FocusSearch, window, cx| v.focus_search(window, cx)))
    }

    // --- Dibujo ------------------------------------------------------------------

    /// El campo arriba de la lista, con el contador y el botón de importar.
    pub(super) fn search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = self.search.input.as_ref()?;
        let active = self.searching();
        let well = div()
            .key_context(SEARCH_CONTEXT)
            .on_action(cx.listener(|v, _: &ClearSearch, window, cx| {
                v.clear_search_text(cx);
                window.focus(&v.focus);
            }))
            .on_action(cx.listener(|v, _: &LeaveSearch, window, _| window.focus(&v.focus)))
            .flex_1()
            .min_w_0()
            .h(px(FIELD_H))
            .pl(px(12.))
            .pr(px(if active { 4. } else { 12. }))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(12.))
            .bg(hsla(FIELD))
            .child(
                svg()
                    .path("icons/search.svg")
                    .size(px(14.))
                    .flex_none()
                    .text_color(hsla(if active { MUTED } else { FAINT })),
            )
            .child(div().flex_1().min_w_0().text_size(px(13.)).child(input.clone()))
            .when(!active, |el| {
                el.child(div().flex_none().text_size(px(11.)).text_color(hsla(FAINT)).child("Ctrl+K"))
            })
            .when(active, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(hsla(MUTED))
                        .child(format!("{} de {}", self.search.visible.len(), self.items.len())),
                )
                .child(hover::round_button(
                    "meetings-search-clear",
                    "icons/x.svg",
                    "Limpiar · Esc",
                    false,
                    hsla(TEXT),
                    hsla(FAINT),
                    cx.listener(|v, _: &ClickEvent, window, cx| {
                        v.clear_search_text(cx);
                        window.focus(&v.focus);
                    }),
                ))
            });
        Some(
            div()
                .flex_none()
                .px(px(8.))
                .pt(px(8.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(well)
                .children(self.import_button(cx))
                .into_any_element(),
        )
    }

    /// La línea con el pedazo donde aparece lo buscado. Una sola: con texto
    /// que se parte en dos, GPUI deja la línea de abajo (hora y estado) del
    /// ancho del pedazo y el estado se corre a la izquierda.
    pub(super) fn snippet_line(&self, ix: usize) -> Option<AnyElement> {
        let snippet = self.search.snippets.get(&ix)?;
        let bright = HighlightStyle { color: Some(hsla(TEXT)), ..Default::default() };
        Some(
            div()
                .truncate()
                .text_size(px(12.))
                .line_height(px(17.))
                .text_color(hsla(MUTED))
                .child(
                    StyledText::new(SharedString::from(snippet.text.clone()))
                        .with_highlights(snippet.marks.iter().map(|r| (r.clone(), bright))),
                )
                .into_any_element(),
        )
    }

    /// Nada coincide.
    pub(super) fn no_results(&self, cx: &App) -> AnyElement {
        let query = self
            .search
            .input
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default();
        div()
            .flex_1()
            .px(px(24.))
            .pb(px(40.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("Nada con «{query}»")),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .text_center()
                    .child("Prueba con otras palabras; Esc muestra todas."),
            )
            .into_any_element()
    }
}

// --- Encabezados de día ----------------------------------------------------------

/// Medidas de un encabezado: el texto a 12 px en una línea de 16, con 6 de
/// aire arriba (14 entre días) y 4 abajo. El scroll tiene 8 de margen arriba.
pub(super) const DAY_LINE: f32 = 16.0;
const LIST_PAD: f32 = 8.0;
const FIRST_PT: f32 = 6.0;
const DAY_PT: f32 = 14.0;
/// Dónde queda el texto del encabezado fijo (como el primero sin scroll).
const STUCK_TEXT: f32 = LIST_PAD + FIRST_PT;
const STUCK_H: f32 = STUCK_TEXT + DAY_LINE + 4.0;

pub(super) fn day_header(label: String, first: bool) -> impl IntoElement {
    div()
        .px(px(12.))
        .pt(px(if first { FIRST_PT } else { DAY_PT }))
        .pb(px(4.))
        .text_size(px(12.))
        .line_height(px(DAY_LINE))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(MUTED))
        .child(label)
}

/// Dónde va el encabezado fijo: cuál día y cuánto lo empuja el siguiente.
/// `tops`: el borde de arriba de cada encabezado dentro del contenido, con su
/// aire (`pt`); `scroll`: cuánto se bajó.
pub(super) fn stuck(tops: &[(f32, f32)], scroll: f32) -> Option<(usize, f32)> {
    if scroll <= 0.5 {
        return None;
    }
    let text_top = |(top, pt): (f32, f32)| top + pt;
    let current = tops.iter().rposition(|t| text_top(*t) <= scroll + STUCK_TEXT)?;
    let shift = match tops.get(current + 1) {
        // El siguiente empuja cuando su texto llega al pie del fijo; cuando
        // llega a su lugar, el fijo ya salió del todo.
        Some(next) => (text_top(*next) - scroll - STUCK_TEXT - DAY_LINE - 14.0).min(0.0),
        None => 0.0,
    };
    Some((current, shift))
}

impl MeetingsView {
    /// El encabezado del día de arriba, pegado al borde mientras se baja. GPUI
    /// no tiene `sticky`: se calcula con las medidas del cuadro anterior y el
    /// scroll de ahora (que sí está al día), así no se atrasa ni parpadea.
    pub(super) fn sticky_day(&self, days: &[(usize, String)]) -> Option<AnyElement> {
        let handle = &self.search.scroll;
        let origin = handle.bounds().top();
        let mut tops = Vec::with_capacity(days.len());
        for (n, (child, _)) in days.iter().enumerate() {
            let bounds = handle.bounds_for_item(*child)?;
            let pt = if n == 0 && *child == 0 { FIRST_PT } else { DAY_PT };
            tops.push((f32::from(bounds.top() - origin), pt));
        }
        let scroll = -f32::from(handle.offset().y);
        let (current, shift) = stuck(&tops, scroll)?;
        let label = days.get(current)?.1.clone();
        Some(
            div()
                .absolute()
                .top(px(shift))
                .left_0()
                .right_0()
                .h(px(STUCK_H))
                .bg(hsla(SURFACE))
                // Tapa las filas de abajo también para el cursor, pero deja
                // pasar la rueda al scroll.
                .block_mouse_except_scroll()
                .px(px(LIST_PAD))
                .pt(px(STUCK_TEXT))
                .child(
                    div()
                        .px(px(12.))
                        .text_size(px(12.))
                        .line_height(px(DAY_LINE))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(hsla(MUTED))
                        .child(label),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn normaliza_tildes_y_mayusculas_letra_por_letra() {
        assert_eq!(normalize("Reunión Ñandú ÁÉÍÓÚ"), "reunion nandu aeiou");
        let text = "Planificación\tdel AÑO";
        assert_eq!(normalize(text).chars().count(), text.chars().count());
        assert_eq!(normalize(text), "planificacion del ano");
    }

    #[test]
    fn las_palabras_buscadas_sin_signos_ni_repetidas() {
        assert_eq!(terms("  Licencias,  ¿PRECIO? licencias "), t(&["licencias", "precio"]));
        assert_eq!(terms("q4"), t(&["q4"]));
        assert!(terms(" , ").is_empty());
    }

    #[test]
    fn coincide_solo_con_todas_las_palabras() {
        let doc = Doc::new("El precio sube un ocho por ciento en enero.".into());
        assert_eq!(find("Llamada con proveedor", Some(&doc), &t(&["proveedor"])), Some(Hit::Title));
        assert!(matches!(find("Llamada con proveedor", Some(&doc), &t(&["proveedor", "enero"])), Some(Hit::Content(_))));
        assert_eq!(find("Llamada con proveedor", Some(&doc), &t(&["proveedor", "febrero"])), None);
        // Sin índice todavía: solo el título.
        assert_eq!(find("Llamada", None, &t(&["enero"])), None);
        assert_eq!(find("Llamada", None, &t(&["llam"])), Some(Hit::Title));
    }

    #[test]
    fn busca_sin_tildes_en_ambos_lados() {
        let doc = Doc::new("Revisión del diseño con Camila.".into());
        let Some(Hit::Content(snippet)) = find("Notch", Some(&doc), &terms("revision DISENO")) else {
            panic!("debía coincidir por contenido");
        };
        let marked: Vec<&str> = snippet.marks.iter().map(|r| &snippet.text[r.clone()]).collect();
        assert_eq!(marked, ["Revisión", "diseño"]);
    }

    #[test]
    fn el_pedazo_rodea_lo_encontrado_sin_cortar_palabras() {
        let text = "Partamos. La idea hoy es cerrar qué entra en el cuarto trimestre. Tenemos tres frentes \
                    abiertos: la app de escritorio, el móvil y el sitio. Desde producto, lo que más piden \
                    los clientes son los resúmenes automáticos de reuniones.";
        let doc = Doc::new(text.into());
        let Some(Hit::Content(snippet)) = find("Q4", Some(&doc), &t(&["movil"])) else {
            panic!("debía coincidir");
        };
        assert!(snippet.text.starts_with('…'), "{}", snippet.text);
        assert!(snippet.text.ends_with('…'), "{}", snippet.text);
        assert!(snippet.text.chars().count() <= SNIPPET + 2);
        let body = snippet.text.trim_matches('…');
        // Empieza y termina en palabras enteras del original.
        assert!(text.contains(body), "{body}");
        let at = text.find(body).unwrap();
        assert!(text[..at].ends_with(' '));
        assert!(text[at + body.len()..].starts_with(' '));
        assert_eq!(&snippet.text[snippet.marks[0].clone()], "móvil");
    }

    #[test]
    fn el_pedazo_no_empieza_con_el_separador() {
        for n in 1..40 {
            let text = format!("{} · Camila pidió las esquinas redondas", "x".repeat(n));
            let doc = Doc::new(text);
            let Some(Hit::Content(snippet)) = find("Notch", Some(&doc), &t(&["esquinas"])) else {
                panic!("debía coincidir");
            };
            assert!(!snippet.text.starts_with("…·") && !snippet.text.starts_with("… "), "{n}: {}", snippet.text);
            assert_eq!(&snippet.text[snippet.marks[0].clone()], "esquinas");
        }
    }

    #[test]
    fn el_pedazo_del_comienzo_no_lleva_puntos_adelante() {
        let doc = Doc::new("Hola, ¿me escuchas bien?".into());
        let Some(Hit::Content(snippet)) = find("Llamada", Some(&doc), &t(&["escuchas"])) else {
            panic!("debía coincidir");
        };
        assert_eq!(snippet.text, "Hola, ¿me escuchas bien?");
        assert_eq!(&snippet.text[snippet.marks[0].clone()], "escuchas");
    }

    #[test]
    fn marca_cada_aparicion_y_une_las_que_se_tocan() {
        let doc = Doc::new("licencia y licencias".into());
        let Some(Hit::Content(snippet)) = find("x", Some(&doc), &t(&["licencia", "licencias"])) else {
            panic!("debía coincidir");
        };
        let marked: Vec<&str> = snippet.marks.iter().map(|r| &snippet.text[r.clone()]).collect();
        assert_eq!(marked, ["licencia", "licencias"]);
    }

    #[test]
    fn el_texto_de_busqueda_junta_resumen_y_transcripcion() {
        let sections = summary::parse("## Resumen\nSe habló del **precio**.\n\n## Tareas\n- [ ] Llamar a Ana", "Resumen");
        let blocks = vec![Block { me: true, label: "Yo".into(), start_ms: 0, text: "Hola\n  a todos".into() }];
        assert_eq!(doc_text(&sections, &blocks), "Se habló del precio. · Llamar a Ana · Hola a todos");
    }

    #[test]
    fn el_encabezado_fijo_es_el_del_dia_de_arriba_y_el_siguiente_lo_empuja() {
        // Tres días: el primero arriba del todo, los otros más abajo.
        let tops = [(8.0, FIRST_PT), (200.0, DAY_PT), (500.0, DAY_PT)];
        // Sin bajar no hay fijo: el de verdad está en su lugar.
        assert_eq!(stuck(&tops, 0.0), None);
        // Bajando un poco, queda el primero, sin empujar.
        assert_eq!(stuck(&tops, 40.0), Some((0, 0.0)));
        // El segundo se acerca: el fijo sube lo mismo que él.
        let (day, shift) = stuck(&tops, 190.0).unwrap();
        assert_eq!(day, 0);
        assert!(shift < 0.0);
        // El texto del segundo llega a su lugar: el fijo pasa a ser el segundo.
        assert_eq!(stuck(&tops, 200.0 + DAY_PT - STUCK_TEXT), Some((1, 0.0)));
        assert_eq!(stuck(&tops, 600.0), Some((2, 0.0)));
    }

    #[test]
    fn el_empuje_saca_al_fijo_entero_antes_del_cambio() {
        let tops = [(8.0, FIRST_PT), (200.0, DAY_PT)];
        // Justo antes de que el segundo tome el lugar, el fijo ya no se ve.
        let (day, shift) = stuck(&tops, 200.0 + DAY_PT - STUCK_TEXT - 0.01).unwrap();
        assert_eq!(day, 0);
        assert!(STUCK_TEXT + DAY_LINE + shift <= 0.1, "{shift}");
    }
}
