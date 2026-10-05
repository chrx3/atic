//! Buscar en la transcripción (Ctrl+F): una barrita plana sobre el contenido,
//! las coincidencias en ámbar tenue dentro del texto, «2 de 7» y saltos con
//! ↑/↓ o Enter/Shift+Enter que llevan el bloque a la vista.
//!
//! Se busca sin tildes ni mayúsculas («reunion» encuentra «Reunión»), pero la
//! ñ se respeta: «ano» no es «año». Ctrl+F desde el Resumen cambia a la
//! Transcripción: ahí es donde se busca qué dijo quién; el resumen se lee de
//! una mirada.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    actions, div, point, prelude::*, px, svg, App, Bounds, ClickEvent, Context, Entity, Focusable, FontWeight,
    HighlightStyle, KeyBinding, NoAction, Pixels, ScrollHandle, SharedString, StyledText,
    Subscription, Window,
};

use super::text::Block;
use super::{hsla, MeetingsView, Tab, AMBER, FAINT, ITEM, KEY_CONTEXT, MUTED, TEXT};
use crate::hover;
use crate::text_input::{self, TextInput};

actions!(meetings_find, [OpenFind, FindNext, FindPrev, CloseFind, FindSwallow]);

/// El campo de búsqueda: Enter y las flechas saltan, Esc cierra, y ni
/// Espacio ni ↑/↓ ni Tab llegan al reproductor o a la lista.
const FIND_CONTEXT: &str = "MeetingsFind";

pub fn bind_keys(cx: &mut App) {
    let find = Some(FIND_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("ctrl-f", OpenFind, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", FindNext, find),
        KeyBinding::new("down", FindNext, find),
        KeyBinding::new("shift-enter", FindPrev, find),
        KeyBinding::new("up", FindPrev, find),
        KeyBinding::new("escape", CloseFind, find),
        KeyBinding::new("tab", FindSwallow, find),
        KeyBinding::new("ctrl-z", FindSwallow, find),
        // Sin acción: la tecla sigue de largo hasta el campo (el texto).
        KeyBinding::new("space", NoAction, find),
    ]);
}

// --- Lógica pura ---------------------------------------------------------------------

/// Una letra como se compara: minúscula y sin tilde (salvo la ñ).
fn fold_char(c: char, out: &mut Vec<char>) {
    for c in c.to_lowercase() {
        out.push(match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_whitespace() => ' ',
            c => c,
        });
    }
}

/// El texto plegado y, por cada letra plegada, el tramo del original del que
/// sale (para resaltar en el texto que se ve). Los espacios seguidos cuentan
/// como uno.
pub fn fold(text: &str) -> (Vec<char>, Vec<Range<usize>>) {
    let mut chars = Vec::with_capacity(text.len());
    let mut spans: Vec<Range<usize>> = Vec::with_capacity(text.len());
    let mut buf = Vec::with_capacity(2);
    for (at, c) in text.char_indices() {
        let end = at + c.len_utf8();
        if c.is_whitespace() && chars.last() == Some(&' ') {
            if let Some(span) = spans.last_mut() {
                span.end = end;
            }
            continue;
        }
        buf.clear();
        fold_char(c, &mut buf);
        for &f in &buf {
            chars.push(f);
            spans.push(at..end);
        }
    }
    (chars, spans)
}

/// Lo que se busca, plegado igual y sin espacios en los bordes.
pub fn fold_query(query: &str) -> Vec<char> {
    let (chars, _) = fold(query.trim());
    chars
}

/// Las coincidencias de `query` (ya plegada) en `text`, como tramos de bytes
/// del original, sin solaparse.
pub fn find_in(text: &str, query: &[char]) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let (chars, spans) = fold(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i + query.len() <= chars.len() {
        if chars[i..i + query.len()] == *query {
            out.push(spans[i].start..spans[i + query.len() - 1].end);
            i += query.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Una coincidencia: el bloque y el tramo dentro de su texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: usize,
    pub range: Range<usize>,
}

pub fn search(blocks: &[Block], query: &str) -> Vec<Hit> {
    let query = fold_query(query);
    blocks
        .iter()
        .enumerate()
        .flat_map(|(block, b)| find_in(&b.text, &query).into_iter().map(move |range| Hit { block, range }))
        .collect()
}

/// El índice siguiente (o anterior) dando la vuelta.
pub fn wrap_step(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(len as isize) as usize
}

/// «2 de 7», «Sin resultados» o nada si no se escribió nada.
pub fn count_label(current: usize, len: usize, has_query: bool) -> String {
    match (len, has_query) {
        (_, false) => String::new(),
        (0, true) => "Sin resultados".into(),
        (n, true) => format!("{} de {n}", current + 1),
    }
}

// --- Estado ----------------------------------------------------------------------------

#[derive(Default)]
pub struct Find {
    input: Option<(Entity<TextInput>, Subscription)>,
    hits: Vec<Hit>,
    current: usize,
    /// Para qué se calcularon `hits`: la reunión, sus bloques y la consulta.
    key: Option<(String, usize, usize, String)>,
    /// Los bloques medidos en el último cuadro (para llevar uno a la vista).
    bounds: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    /// Hay que llevar la coincidencia actual a la vista al pintar.
    reveal: Rc<RefCell<bool>>,
}

impl MeetingsView {
    fn find_query(&self, cx: &App) -> Option<String> {
        self.find.input.as_ref().map(|(input, _)| input.read(cx).text().to_string())
    }

    /// Antes de dibujar: si cambió la consulta o la transcripción, se busca de
    /// nuevo (una vez, no en cada cuadro).
    pub(super) fn sync_find(&mut self, cx: &App) {
        let (Some(query), Some(detail)) = (self.find_query(cx), self.detail.as_ref()) else {
            self.find.hits.clear();
            self.find.key = None;
            return;
        };
        let size = detail.blocks.iter().map(|b| b.text.len()).sum();
        let key = (detail.id.clone(), detail.blocks.len(), size, query.clone());
        if self.find.key.as_ref() == Some(&key) {
            return;
        }
        let same = self.find.key.as_ref().is_some_and(|k| k.0 == key.0 && k.3 == query);
        self.find.hits = search(&detail.blocks, &query);
        self.find.key = Some(key);
        // Otra consulta u otra reunión: a la primera. La misma (se releyó):
        // donde estaba.
        if !same {
            self.find.current = 0;
            *self.find.reveal.borrow_mut() = !self.find.hits.is_empty();
        }
        self.find.current = self.find.current.min(self.find.hits.len().saturating_sub(1));
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.detail.as_ref().is_none_or(|d| d.blocks.is_empty()) {
            cx.propagate();
            return;
        }
        self.set_tab(Tab::Transcript, cx);
        if let Some((input, _)) = &self.find.input {
            window.focus(&input.focus_handle(cx));
            return;
        }
        let input = cx.new(|cx| TextInput::new("Buscar en la transcripción", hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        let changed = cx.subscribe(&input, |v, _, _: &text_input::Changed, cx| {
            v.sync_find(cx);
            cx.notify();
        });
        window.focus(&input.focus_handle(cx));
        self.find.input = Some((input, changed));
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.input.take().is_some() {
            self.find.hits.clear();
            self.find.key = None;
            window.focus(&self.focus);
            cx.notify();
        }
    }

    fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.find.hits.is_empty() {
            return;
        }
        self.find.current = wrap_step(self.find.current, delta, self.find.hits.len());
        *self.find.reveal.borrow_mut() = true;
        cx.notify();
    }

    /// Para la raíz de la ventana: Ctrl+F.
    pub(super) fn find_actions(div: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        div.on_action(cx.listener(|v, _: &OpenFind, window, cx| v.open_find(window, cx)))
    }

    /// Los resaltados de un bloque, para `StyledText::with_highlights`.
    pub(super) fn find_highlights(&self, block: usize) -> Vec<(Range<usize>, HighlightStyle)> {
        if self.find.input.is_none() {
            return Vec::new();
        }
        let current = self.find.hits.get(self.find.current);
        self.find
            .hits
            .iter()
            .filter(|h| h.block == block)
            .map(|h| {
                let on = Some(h) == current;
                let style = HighlightStyle {
                    background_color: Some(hsla(AMBER).opacity(if on { 0.6 } else { 0.16 })),
                    color: on.then(|| hsla(TEXT)),
                    ..Default::default()
                };
                (h.range.clone(), style)
            })
            .collect()
    }

    /// El texto de un bloque, con sus coincidencias resaltadas.
    pub(super) fn block_text(&self, ix: usize, block: &Block) -> gpui::AnyElement {
        let text = SharedString::from(block.text.clone());
        let highlights = self.find_highlights(ix);
        if highlights.is_empty() {
            return text.into_any_element();
        }
        StyledText::new(text).with_highlights(highlights).into_any_element()
    }

    /// Para la columna de bloques: se miden al pintar y, si hace falta, se
    /// lleva la coincidencia actual a un tercio desde arriba.
    pub(super) fn find_measure(&self, column: gpui::Div) -> gpui::Div {
        if self.find.input.is_none() {
            return column;
        }
        let cells = self.find.bounds.clone();
        let reveal = self.find.reveal.clone();
        let scroll: ScrollHandle = self.detail_scroll.clone();
        // Dónde cae la coincidencia dentro de su bloque, a ojo por la
        // proporción del texto: un bloque largo no deja la palabra fuera.
        let target = self.find.hits.get(self.find.current).and_then(|hit| {
            let block = self.detail.as_ref()?.blocks.get(hit.block)?;
            Some((hit.block, hit.range.start as f32 / block.text.len().max(1) as f32))
        });
        column.on_children_prepainted(move |bounds, window, _| {
            *cells.borrow_mut() = bounds;
            if !std::mem::take(&mut *reveal.borrow_mut()) {
                return;
            }
            let Some((ix, frac)) = target else {
                return;
            };
            let Some(block) = cells.borrow().get(ix).copied() else {
                return;
            };
            let view = scroll.bounds();
            let offset = scroll.offset();
            let max = scroll.max_offset();
            let y = block.top() + block.size.height * frac;
            let margin = px(40.);
            if y >= view.top() + margin && y <= view.bottom() - margin * 2. {
                return;
            }
            let target = (offset.y - (y - view.top() - view.size.height / 3.0)).clamp(-max.height, px(0.));
            scroll.set_offset(point(offset.x, target));
            window.refresh();
        })
    }

    /// La barrita, arriba del contenido de la pestaña.
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.tab != Tab::Transcript {
            return None;
        }
        let (input, _) = self.find.input.as_ref()?;
        let has_query = !fold_query(input.read(cx).text()).is_empty();
        let count = count_label(self.find.current, self.find.hits.len(), has_query);
        let none = has_query && self.find.hits.is_empty();
        let step = |id: &'static str, icon: &'static str, tip: &'static str, delta: isize, cx: &mut Context<Self>| {
            hover::round_button(id, icon, tip, false, hsla(TEXT), hsla(MUTED), cx.listener(
                move |v, _: &ClickEvent, _, cx| v.find_step(delta, cx),
            ))
        };
        let bar = div()
            .key_context(FIND_CONTEXT)
            .on_action(cx.listener(|v, _: &FindNext, _, cx| v.find_step(1, cx)))
            .on_action(cx.listener(|v, _: &FindPrev, _, cx| v.find_step(-1, cx)))
            .on_action(cx.listener(|v, _: &CloseFind, window, cx| v.close_find(window, cx)))
            .on_action(cx.listener(|_, _: &FindSwallow, _, _| {}))
            .h(px(40.))
            .pl(px(14.))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(20.))
            .bg(hsla(ITEM))
            .child(svg().path("icons/search.svg").size(px(14.)).flex_none().text_color(hsla(MUTED)))
            .child(div().flex_1().min_w_0().text_size(px(13.)).child(input.clone()))
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(hsla(if none { FAINT } else { MUTED }))
                    .child(count),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .child(step("find-prev", "icons/chevron-up.svg", "Anterior (↑)", -1, cx))
                    .child(step("find-next", "icons/chevron-down.svg", "Siguiente (↓ o Enter)", 1, cx))
                    .child(hover::round_button(
                        "find-close",
                        "icons/x.svg",
                        "Cerrar (Esc)",
                        false,
                        hsla(TEXT),
                        hsla(MUTED),
                        cx.listener(|v, _: &ClickEvent, window, cx| v.close_find(window, cx)),
                    )),
            );
        Some(div().flex_none().px(px(24.)).pb(px(12.)).child(bar).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str) -> Block {
        Block { me: false, label: "Los demás".into(), start_ms: 0, text: text.into() }
    }

    #[test]
    fn sin_tildes_ni_mayusculas() {
        let text = "La Reunión de hoy: REUNION corta, reunión larga.";
        let hits = find_in(text, &fold_query("reunion"));
        assert_eq!(hits.len(), 3);
        // Los tramos son del original, con la tilde incluida.
        assert_eq!(&text[hits[0].clone()], "Reunión");
        assert_eq!(&text[hits[1].clone()], "REUNION");
        assert_eq!(&text[hits[2].clone()], "reunión");
        // Y al revés: la tilde en la consulta tampoco importa.
        assert_eq!(find_in("la reunion", &fold_query("Reunión")).len(), 1);
    }

    #[test]
    fn la_enie_se_respeta() {
        assert!(find_in("el año pasado", &fold_query("ano")).is_empty());
        assert_eq!(find_in("el AÑO pasado", &fold_query("año")).len(), 1);
    }

    #[test]
    fn espacios_y_bordes() {
        let text = "hola   mundo\nnuevo";
        let hits = find_in(text, &fold_query("  hola mundo "));
        assert_eq!(hits, vec![0..12]);
        assert!(find_in(text, &fold_query("   ")).is_empty());
        assert!(find_in(text, &[]).is_empty());
    }

    #[test]
    fn no_se_solapan() {
        assert_eq!(find_in("aaaa", &fold_query("aa")), vec![0..2, 2..4]);
    }

    #[test]
    fn busca_en_todos_los_bloques_en_orden() {
        let blocks = [block("el presupuesto"), block("nada"), block("Presupuesto y presupuesto")];
        let hits = search(&blocks, "presupuesto");
        let where_: Vec<usize> = hits.iter().map(|h| h.block).collect();
        assert_eq!(where_, vec![0, 2, 2]);
        assert_eq!(hits[2].range, 14..25);
        assert!(search(&blocks, "").is_empty());
    }

    #[test]
    fn saltar_da_la_vuelta_y_cuenta() {
        assert_eq!(wrap_step(0, -1, 7), 6);
        assert_eq!(wrap_step(6, 1, 7), 0);
        assert_eq!(wrap_step(2, 1, 7), 3);
        assert_eq!(wrap_step(0, 1, 0), 0);
        assert_eq!(count_label(1, 7, true), "2 de 7");
        assert_eq!(count_label(0, 0, true), "Sin resultados");
        assert_eq!(count_label(0, 0, false), "");
    }
}
