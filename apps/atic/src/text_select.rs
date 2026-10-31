//! Seleccionar texto sobre una imagen congelada, como «Circle to Search» de
//! Google: la pantalla se oscurece un poco, cada palabra que encontró el OCR
//! se ilumina y el texto se elige arrastrando, como en un documento (doble
//! clic: una palabra; triple: la línea; Ctrl+A: todo). Lo usan la mira de
//! Capturas (G o T) y la herramienta «Copiar texto» de la Pizarra.
//!
//! El OCR (`ocr::recognize_lines`) corre una sola vez por foto, en otro hilo.
//! Lo demás es lógica pura en píxeles de la imagen (orden de lectura, a qué
//! palabra apunta el cursor, el rango elegido y su texto), probada abajo.
//! Quien la usa solo convierte coordenadas con `Mapping`: la foto se dibuja
//! a `píxeles / scale` lógicos, corrida `offset` en la ventana.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use atic_capture::Frame;
use gpui::{
    div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size, svg, Animation,
    AnimationExt, AnyElement, Background, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle,
    Hsla, MouseButton, Pixels, Point, SharedString, Window,
};

use crate::anim;
use crate::hover::HoverExt;

// --- Orden de lectura ------------------------------------------------------------

/// Un hueco vertical entre columnas tiene que medir al menos esto (en alturas
/// de línea) para separarlas.
const GUTTER: f32 = 1.5;
/// Un hueco horizontal así de alto (en alturas de línea) separa bloques.
const BAND_GAP: f32 = 0.9;
/// Dentro de una fila, un hueco más ancho que esto (en alturas) va con
/// tabulación: son celdas, no palabras.
const CELL_GAP: f32 = 2.0;
/// Una línea del OCR se parte en un hueco así de ancho (en alturas): son dos
/// columnas que el OCR juntó.
const SPLIT_GAP: f32 = 3.0;

/// Una palabra leída y su caja, en píxeles de la imagen.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Word {
    pub fn new(text: impl Into<String>, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { text: text.into(), x, y, w, h }
    }

    fn right(&self) -> f32 {
        self.x + self.w
    }

    fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Distancia del punto a la caja (0 adentro); lo vertical pesa `vertical`.
    fn distance(&self, (px, py): (f32, f32), vertical: f32) -> f32 {
        let dx = (self.x - px).max(px - self.right()).max(0.0);
        let dy = (self.y - py).max(py - self.bottom()).max(0.0);
        dx.hypot(dy * vertical)
    }
}

/// Un trozo de línea: palabras de izquierda a derecha y su caja.
#[derive(Clone, Debug)]
struct Seg {
    words: Vec<Word>,
    x: f32,
    y: f32,
    right: f32,
    bottom: f32,
    h: f32,
}

impl Seg {
    fn new(words: Vec<Word>) -> Self {
        let x = words.iter().map(|w| w.x).fold(f32::INFINITY, f32::min);
        let y = words.iter().map(|w| w.y).fold(f32::INFINITY, f32::min);
        let right = words.iter().map(Word::right).fold(f32::NEG_INFINITY, f32::max);
        let bottom = words.iter().map(Word::bottom).fold(f32::NEG_INFINITY, f32::max);
        let h = median(words.iter().map(|w| w.h).collect());
        Self { words, x, y, right, bottom, h }
    }

    fn mid_y(&self) -> f32 {
        (self.y + self.bottom) / 2.0
    }
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(|a, b| a.total_cmp(b));
    values.get(values.len() / 2).copied().unwrap_or(1.0).max(1.0)
}

/// Las líneas del OCR, limpias y partidas donde juntaron dos columnas.
fn segments(lines: Vec<Vec<Word>>) -> Vec<Seg> {
    let mut out = Vec::new();
    for line in lines {
        let mut words: Vec<Word> = line
            .into_iter()
            .filter_map(|mut word| {
                word.text = word.text.trim().to_string();
                let valid = !word.text.is_empty()
                    && word.x.is_finite()
                    && word.y.is_finite()
                    && word.w >= 0.0
                    && word.h > 0.0;
                valid.then_some(word)
            })
            .collect();
        if words.is_empty() {
            continue;
        }
        words.sort_by(|a, b| a.x.total_cmp(&b.x));
        let h = median(words.iter().map(|w| w.h).collect());
        let mut current: Vec<Word> = Vec::new();
        let mut reach = f32::NEG_INFINITY;
        for word in words {
            if !current.is_empty() && word.x - reach >= h * SPLIT_GAP {
                out.push(Seg::new(std::mem::take(&mut current)));
            }
            reach = if current.is_empty() { word.right() } else { reach.max(word.right()) };
            current.push(word);
        }
        if !current.is_empty() {
            out.push(Seg::new(current));
        }
    }
    out
}

/// Corta en columnas y bloques (un XY-cut prudente, como el de
/// `platform/macos/ocr/layout.rs`) y deja los grupos en orden de lectura.
fn cut(segs: Vec<Seg>, unit: f32, out: &mut Vec<Vec<Seg>>) {
    if segs.len() < 2 {
        if !segs.is_empty() {
            out.push(segs);
        }
        return;
    }
    if let Some(at) = column_cut(&segs, unit) {
        let (left, right): (Vec<Seg>, Vec<Seg>) = segs.into_iter().partition(|s| s.right <= at);
        cut(left, unit, out);
        cut(right, unit, out);
        return;
    }
    let bands = bands(segs, unit);
    if bands.len() == 1 {
        out.extend(bands);
        return;
    }
    for band in bands {
        cut(band, unit, out);
    }
}

/// Dónde cortar en dos columnas: el hueco vertical más ancho que cruza todo
/// el grupo. No corta una tabla (celdas cortas alineadas por fila a los dos
/// lados): esa se lee fila por fila.
fn column_cut(segs: &[Seg], unit: f32) -> Option<f32> {
    let mut spans: Vec<(f32, f32)> = segs.iter().map(|s| (s.x, s.right)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<(f32, f32)> = None;
    let mut reach = spans[0].1;
    for &(start, end) in &spans[1..] {
        let gap = start - reach;
        if gap >= unit * GUTTER && best.is_none_or(|(width, _)| gap > width) {
            best = Some((gap, reach + gap / 2.0));
        }
        reach = reach.max(end);
    }
    let (_, at) = best?;
    let (left, right): (Vec<&Seg>, Vec<&Seg>) = segs.iter().partition(|s| s.right <= at);
    let (fewer, other) = if left.len() <= right.len() { (&left, &right) } else { (&right, &left) };
    let aligned = fewer
        .iter()
        .filter(|s| other.iter().any(|o| (s.mid_y() - o.mid_y()).abs() < unit * 0.35))
        .count();
    let short = median(fewer.iter().map(|s| s.right - s.x).collect()) < unit * 8.0;
    let table = aligned * 10 >= fewer.len() * 6 && short;
    (!table).then_some(at)
}

/// Parte en bloques donde hay un hueco horizontal claro que cruza todo.
fn bands(mut segs: Vec<Seg>, unit: f32) -> Vec<Vec<Seg>> {
    segs.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut out: Vec<Vec<Seg>> = Vec::new();
    let mut reach = f32::NEG_INFINITY;
    for seg in segs {
        let starts_band = seg.y - reach >= unit * BAND_GAP;
        reach = if starts_band { seg.bottom } else { reach.max(seg.bottom) };
        match out.last_mut() {
            Some(band) if !starts_band => band.push(seg),
            _ => out.push(vec![seg]),
        }
    }
    out
}

/// Las filas de un bloque, de arriba abajo; cada una con sus palabras de
/// izquierda a derecha.
fn rows_of(mut segs: Vec<Seg>) -> Vec<Vec<Word>> {
    segs.sort_by(|a, b| a.mid_y().total_cmp(&b.mid_y()));
    let mut groups: Vec<(f32, f32, Vec<Seg>)> = Vec::new();
    for seg in segs {
        let same_row = groups.last().is_some_and(|(top, bottom, _)| {
            let mid = (top + bottom) / 2.0;
            (*top..=*bottom).contains(&seg.mid_y()) && (seg.y..=seg.bottom).contains(&mid)
        });
        match groups.last_mut() {
            Some(group) if same_row => {
                group.0 = group.0.min(seg.y);
                group.1 = group.1.max(seg.bottom);
                group.2.push(seg);
            }
            _ => groups.push((seg.y, seg.bottom, vec![seg])),
        }
    }
    groups
        .into_iter()
        .map(|(_, _, segs)| {
            let mut words: Vec<Word> = segs.into_iter().flat_map(|s| s.words).collect();
            words.sort_by(|a, b| a.x.total_cmp(&b.x));
            words
        })
        .collect()
}

/// ¿La línea termina con una palabra cortada con guion que sigue abajo?
fn hyphenated(end: &str, next: &str) -> bool {
    let mut back = end.chars().rev();
    matches!(back.next(), Some('-' | '\u{2010}' | '\u{00AD}'))
        && back.next().is_some_and(char::is_alphabetic)
        && next.chars().next().is_some_and(char::is_lowercase)
}

/// Las palabras de la imagen en orden de lectura, agrupadas en filas.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub words: Vec<Word>,
    /// Cada fila: el rango de `words` que ocupa.
    pub rows: Vec<Range<usize>>,
    /// La fila de cada palabra.
    row_of: Vec<usize>,
    /// El bloque (columna o párrafo separado) de cada fila.
    block_of: Vec<usize>,
    /// La altura típica de una línea.
    unit: f32,
}

impl Doc {
    /// Desde las líneas del OCR, con cajas en píxeles de la imagen.
    pub fn new(lines: Vec<Vec<Word>>) -> Self {
        let segs = segments(lines);
        if segs.is_empty() {
            return Self::default();
        }
        let unit = median(segs.iter().map(|s| s.h).collect());
        let mut blocks = Vec::new();
        cut(segs, unit, &mut blocks);
        let mut doc = Doc { unit, ..Default::default() };
        for (block, segs) in blocks.into_iter().enumerate() {
            for row in rows_of(segs) {
                let start = doc.words.len();
                let index = doc.rows.len();
                for word in row {
                    doc.words.push(word);
                    doc.row_of.push(index);
                }
                doc.rows.push(start..doc.words.len());
                doc.block_of.push(block);
            }
        }
        doc
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Las palabras de la fila de la palabra `i`.
    pub fn row_range(&self, i: usize) -> Range<usize> {
        self.row_of.get(i).and_then(|row| self.rows.get(*row)).cloned().unwrap_or(i..i + 1)
    }

    /// (x0, y0, x1, y1) de una fila.
    fn row_bounds(&self, row: usize) -> (f32, f32, f32, f32) {
        let words = &self.words[self.rows[row].clone()];
        (
            words.iter().map(|w| w.x).fold(f32::INFINITY, f32::min),
            words.iter().map(|w| w.y).fold(f32::INFINITY, f32::min),
            words.iter().map(Word::right).fold(f32::NEG_INFINITY, f32::max),
            words.iter().map(Word::bottom).fold(f32::NEG_INFINITY, f32::max),
        )
    }

    /// La palabra bajo `p`: dentro de su caja (o casi, a `slop / 2`), o la
    /// más cercana de la fila cuando `p` cae en un hueco de ella o a menos de
    /// `slop` de sus bordes. `None` en lo vacío.
    pub fn hit(&self, p: (f32, f32), slop: f32) -> Option<usize> {
        let inside = self
            .words
            .iter()
            .enumerate()
            .map(|(i, word)| (i, word.distance(p, 1.0)))
            .filter(|(_, d)| *d <= slop * 0.5)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = inside {
            return Some(i);
        }
        let mut best: Option<(usize, f32)> = None;
        for (row, range) in self.rows.iter().enumerate() {
            let (x0, y0, x1, y1) = self.row_bounds(row);
            if p.1 < y0 - slop || p.1 > y1 + slop || p.0 < x0 - slop || p.0 > x1 + slop {
                continue;
            }
            for i in range.clone() {
                let d = self.words[i].distance(p, 2.0);
                if best.is_none_or(|(_, nearest)| d < nearest) {
                    best = Some((i, d));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// La palabra hasta donde llega un arrastre en `p`: en la fila a esa
    /// altura (la más cercana en horizontal, si hay columnas), o si no la
    /// palabra más cercana. Siempre hay una si la imagen tiene texto.
    pub fn nearest(&self, p: (f32, f32)) -> Option<usize> {
        let mut row_pick: Option<(usize, f32)> = None;
        for row in 0..self.rows.len() {
            let (x0, y0, x1, y1) = self.row_bounds(row);
            if p.1 < y0 || p.1 > y1 {
                continue;
            }
            let dx = (x0 - p.0).max(p.0 - x1).max(0.0);
            if row_pick.is_none_or(|(_, best)| dx < best) {
                row_pick = Some((row, dx));
            }
        }
        if let Some((row, _)) = row_pick {
            return self.rows[row]
                .clone()
                .map(|i| (i, self.words[i].distance(p, 1.0)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
        }
        self.words
            .iter()
            .enumerate()
            .map(|(i, word)| (i, word.distance(p, 1.0)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// El texto de las palabras `lo..=hi`: las de una fila con espacios (o
    /// tabulación entre celdas), cada fila en su línea, y las palabras
    /// cortadas con guion al final de una línea, unidas.
    pub fn text(&self, lo: usize, hi: usize) -> String {
        if self.words.is_empty() || lo >= self.words.len() {
            return String::new();
        }
        let hi = hi.min(self.words.len() - 1);
        let mut out = String::new();
        for i in lo..=hi {
            let word = &self.words[i];
            if i > lo {
                let prev = &self.words[i - 1];
                let (row, prev_row) = (self.row_of[i], self.row_of[i - 1]);
                if row == prev_row {
                    out.push(if word.x - prev.right() >= self.unit * CELL_GAP { '\t' } else { ' ' });
                } else if self.block_of[row] == self.block_of[prev_row] && hyphenated(&prev.text, &word.text) {
                    // Sin el guion: la palabra sigue.
                    out.pop();
                } else {
                    out.push('\n');
                }
            }
            out.push_str(&word.text);
        }
        out
    }

    /// Lo que se pinta de `lo..=hi`: una franja por fila (x0, y0, x1, y1),
    /// partida solo entre celdas lejanas.
    pub fn bands(&self, lo: usize, hi: usize) -> Vec<(f32, f32, f32, f32)> {
        let mut out = Vec::new();
        if self.words.is_empty() || lo >= self.words.len() {
            return out;
        }
        let hi = hi.min(self.words.len() - 1);
        let gap = self.unit * CELL_GAP;
        let mut current: Option<(usize, (f32, f32, f32, f32))> = None;
        for i in lo..=hi {
            let word = &self.words[i];
            let row = self.row_of[i];
            let extend = matches!(current, Some((r, rect)) if r == row && word.x - rect.2 < gap);
            if extend {
                if let Some((_, rect)) = current.as_mut() {
                    rect.1 = rect.1.min(word.y);
                    rect.2 = rect.2.max(word.right());
                    rect.3 = rect.3.max(word.bottom());
                }
            } else {
                out.extend(current.take().map(|(_, rect)| rect));
                current = Some((row, (word.x, word.y, word.right(), word.bottom())));
            }
        }
        out.extend(current.map(|(_, rect)| rect));
        out
    }
}

// --- Selección ---------------------------------------------------------------

/// Cómo crece la selección al arrastrar: de a palabra o, tras un triple
/// clic, de a fila.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Word,
    Row,
}

/// Desde dónde (ancla) hasta dónde (foco), en índices de `Doc::words`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
    pub unit: Unit,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    at: (f32, f32),
    dragged: bool,
}

/// Elegir texto como en un documento, en píxeles de la imagen.
#[derive(Clone, Debug, Default)]
pub struct Selector {
    pub doc: Doc,
    pub selection: Option<Selection>,
    press: Option<Press>,
}

impl Selector {
    pub fn new(doc: Doc) -> Self {
        Self { doc, selection: None, press: None }
    }

    /// Lo elegido, `lo..=hi`, ya extendido a filas enteras si toca.
    pub fn range(&self) -> Option<(usize, usize)> {
        let selection = self.selection?;
        let last = self.doc.words.len().checked_sub(1)?;
        let lo = selection.anchor.min(selection.focus).min(last);
        let hi = selection.anchor.max(selection.focus).min(last);
        Some(match selection.unit {
            Unit::Word => (lo, hi),
            Unit::Row => (self.doc.row_range(lo).start, self.doc.row_range(hi).end.saturating_sub(1)),
        })
    }

    pub fn pressing(&self) -> bool {
        self.press.is_some()
    }

    /// Botón apretado en `p`. Sobre una palabra la elige (como tocarla en
    /// «Circle to Search»); con `clicks` ≥ 3, su fila. En lo vacío borra la
    /// selección (arrastrar desde ahí elige igual). Shift extiende la que hay.
    pub fn press(&mut self, p: (f32, f32), clicks: usize, shift: bool, slop: f32) {
        let hit = self.doc.hit(p, slop);
        if shift && self.selection.is_some() {
            if let Some(i) = hit.or_else(|| self.doc.nearest(p)) {
                if let Some(selection) = self.selection.as_mut() {
                    selection.focus = i;
                }
                self.press = Some(Press { at: p, dragged: true });
                return;
            }
        }
        self.selection = hit.map(|i| Selection {
            anchor: i,
            focus: i,
            unit: if clicks >= 3 { Unit::Row } else { Unit::Word },
        });
        self.press = Some(Press { at: p, dragged: false });
    }

    /// El arrastre llega a `p`: el foco va a la palabra de ahí. `true` si
    /// cambió algo.
    pub fn drag(&mut self, p: (f32, f32), slop: f32) -> bool {
        let Some(press) = self.press.as_mut() else {
            return false;
        };
        if !press.dragged {
            let (dx, dy) = (p.0 - press.at.0, p.1 - press.at.1);
            if dx.hypot(dy) < slop * 0.5 {
                return false;
            }
            press.dragged = true;
        }
        let start = press.at;
        let Some(focus) = self.doc.nearest(p) else {
            return false;
        };
        match self.selection.as_mut() {
            Some(selection) => {
                if selection.focus == focus {
                    return false;
                }
                selection.focus = focus;
            }
            None => {
                let Some(anchor) = self.doc.nearest(start) else {
                    return false;
                };
                self.selection = Some(Selection { anchor, focus, unit: Unit::Word });
            }
        }
        true
    }

    pub fn release(&mut self) {
        self.press = None;
    }

    pub fn select_all(&mut self) {
        self.press = None;
        self.selection = self.doc.words.len().checked_sub(1).map(|last| Selection {
            anchor: 0,
            focus: last,
            unit: Unit::Word,
        });
    }

    /// Esc: borra la selección. `false` si no había nada que borrar.
    pub fn clear(&mut self) -> bool {
        self.press = None;
        self.selection.take().is_some()
    }

    pub fn selected_text(&self) -> Option<String> {
        self.range().map(|(lo, hi)| self.doc.text(lo, hi))
    }

    pub fn all_text(&self) -> String {
        match self.doc.words.len() {
            0 => String::new(),
            n => self.doc.text(0, n - 1),
        }
    }
}

// --- Coordenadas ---------------------------------------------------------------

/// Dónde está la imagen en la ventana: se dibuja a `píxeles / scale` lógicos,
/// con su esquina en `offset`. `size`: la imagen, en píxeles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    pub scale: f32,
    pub offset: (f32, f32),
    pub size: (f32, f32),
}

impl Mapping {
    pub fn new(scale: f32, offset: (f32, f32), size: (f32, f32)) -> Self {
        let scale = if scale > 0.0 && scale.is_finite() { scale } else { 1.0 };
        Self { scale, offset, size }
    }

    /// Un punto lógico de la ventana, en píxeles de la imagen.
    pub fn to_image(&self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.offset.0) * self.scale, (y - self.offset.1) * self.scale)
    }

    pub fn point(&self, p: Point<Pixels>) -> (f32, f32) {
        self.to_image((f32::from(p.x), f32::from(p.y)))
    }

    /// Un recuadro (x0, y0, x1, y1) de la imagen, en lógico de la ventana.
    pub fn to_window(&self, (x0, y0, x1, y1): (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
        let (ox, oy, k) = (self.offset.0, self.offset.1, self.scale);
        (x0 / k + ox, y0 / k + oy, x1 / k + ox, y1 / k + oy)
    }

    /// Una distancia lógica, en píxeles de la imagen.
    pub fn slop(&self, logical: f32) -> f32 {
        logical * self.scale
    }

    /// La imagen en la ventana: (x, y, ancho, alto) lógicos.
    pub fn area(&self) -> (f32, f32, f32, f32) {
        (self.offset.0, self.offset.1, self.size.0 / self.scale, self.size.1 / self.scale)
    }
}

// --- OCR ---------------------------------------------------------------------

/// Lee la imagen entera. Corre en un hilo de fondo.
pub fn read(width: u32, height: u32, bgra: &[u8]) -> Result<Doc, String> {
    if width == 0 || height == 0 || bgra.len() < width as usize * height as usize * 4 {
        return Err("la imagen está vacía".into());
    }
    let zoom = ocr_zoom(width, height);
    let lines = if zoom > 1 {
        let (w, h, big) = upscale(width, height, bgra, zoom);
        crate::ocr::recognize_lines(w, h, &big)?
    } else {
        crate::ocr::recognize_lines(width, height, bgra)?
    };
    let k = zoom as f32;
    Ok(Doc::new(
        lines
            .iter()
            .map(|line| line.iter().map(|w| Word::new(w.text.clone(), w.x / k, w.y / k, w.w / k, w.h / k)).collect())
            .collect(),
    ))
}

/// El OCR de Windows lee mal las letras chicas: una pantalla normal va al
/// doble (Vision no lo necesita).
fn ocr_zoom(width: u32, height: u32) -> u32 {
    if cfg!(target_os = "windows") && width.max(height) <= 2560 {
        2
    } else {
        1
    }
}

/// La imagen agrandada `zoom` veces, píxel por píxel.
fn upscale(width: u32, height: u32, bgra: &[u8], zoom: u32) -> (u32, u32, Vec<u8>) {
    let (w, h) = (width * zoom, height * zoom);
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let start = (y / zoom as usize) * stride;
        for px in bgra[start..start + stride].chunks_exact(4) {
            for _ in 0..zoom {
                out.extend_from_slice(px);
            }
        }
    }
    (w, h, out)
}

// --- La capa -------------------------------------------------------------------

/// Cuánto se oscurece la foto y en cuánto.
const DIM: f32 = 0.3;
const DIM_IN: Duration = Duration::from_millis(200);
/// Las palabras se encienden una tras otra en este tiempo, cada una en `FADE`.
const SPREAD: Duration = Duration::from_millis(420);
const FADE: Duration = Duration::from_millis(300);
/// El brillo de una palabra sin elegir.
const WORD_ALPHA: f32 = 0.2;
/// La franja de lo elegido, sobre el acento.
const BAND_ALPHA: f32 = 0.45;
/// El barrido de «Leyendo texto…», en segundos por pasada.
const SCAN_PERIOD: f32 = 1.4;
/// «Copiado» se ve este tiempo.
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// Tolerancia del cursor, en lógico.
const SLOP: f32 = 8.0;
/// El acento con el estilo «Atic»: el azul de la paleta de la Pizarra.
const BLUE: u32 = 0x3E8BFF;
/// Lo que mide a lo más la barra de «Copiar», para centrarla.
const BAR_W: f32 = 300.0;
const BAR_H: f32 = 38.0;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

enum Reading {
    Busy,
    Ready(Selector),
    Failed(String),
}

/// El modo texto de una foto: el OCR (una vez), la selección y su animación.
pub struct TextLayer {
    /// Para descartar un OCR que llega tarde, de otra foto.
    id: u64,
    reading: Reading,
    /// Desde cuándo se ve (la foto se oscurece).
    since: Instant,
    /// Desde cuándo están las palabras (se encienden en cascada).
    ready_at: Instant,
    copied: Option<Instant>,
    /// Ctrl+A antes de que terminara el OCR.
    select_all_pending: bool,
    /// El cursor está sobre una palabra: va la I de texto.
    over_word: bool,
}

/// La vista que tiene la capa: la barra y el OCR la buscan al volver.
pub trait TextHost: Sized + 'static {
    fn text_layer(&mut self) -> Option<&mut TextLayer>;
}

/// Empieza a leer `frame` en otro hilo; la capa queda leyendo hasta que
/// llegue. Guárdala en la vista: `TextHost::text_layer` la encuentra.
pub fn start<V: TextHost>(frame: &Frame, cx: &mut Context<V>) -> TextLayer {
    let layer = TextLayer::new();
    let id = layer.id;
    let (w, h, bgra) = (frame.width(), frame.height(), frame.bgra.clone());
    cx.spawn(async move |this, cx| {
        let started = Instant::now();
        let read = cx.background_spawn(async move { read(w, h, &bgra) }).await;
        match &read {
            Ok(doc) => println!("texto: {} palabras en {} ms", doc.words.len(), started.elapsed().as_millis()),
            Err(error) => eprintln!("texto: no se pudo leer: {error}"),
        }
        let _ = this.update(cx, |view, cx| {
            if let Some(layer) = view.text_layer().filter(|layer| layer.id == id) {
                layer.set_result(read);
                cx.notify();
            }
        });
    })
    .detach();
    layer
}

/// Copia lo elegido (o todo) y muestra «Copiado» un momento.
pub fn copy<V: TextHost>(view: &mut V, all: bool, cx: &mut Context<V>) -> bool {
    let Some(layer) = view.text_layer() else {
        return false;
    };
    let Some(text) = layer.text_to_copy(all) else {
        return false;
    };
    println!("texto: {} caracteres copiados", text.chars().count());
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    layer.copied = Some(Instant::now());
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(COPIED_FOR + Duration::from_millis(50)).await;
        let _ = this.update(cx, |_, cx| cx.notify());
    })
    .detach();
    cx.notify();
    true
}

impl TextLayer {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            reading: Reading::Busy,
            since: now,
            ready_at: now,
            copied: None,
            select_all_pending: false,
            over_word: false,
        }
    }

    /// Al volver al modo con la foto ya leída: la entrada se anima otra vez.
    pub fn replay(&mut self) {
        let now = Instant::now();
        self.since = now;
        self.ready_at = now;
        self.copied = None;
    }

    fn set_result(&mut self, read: Result<Doc, String>) {
        self.ready_at = Instant::now();
        self.reading = match read {
            Ok(doc) => {
                let mut selector = Selector::new(doc);
                if std::mem::take(&mut self.select_all_pending) {
                    selector.select_all();
                }
                Reading::Ready(selector)
            }
            Err(error) => Reading::Failed(error),
        };
    }

    fn selector(&self) -> Option<&Selector> {
        match &self.reading {
            Reading::Ready(selector) => Some(selector),
            _ => None,
        }
    }

    fn selector_mut(&mut self) -> Option<&mut Selector> {
        match &mut self.reading {
            Reading::Ready(selector) => Some(selector),
            _ => None,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selector().is_some_and(|s| s.range().is_some())
    }

    /// Lo que se dice en vez de la ayuda: leyendo, sin texto o el error.
    pub fn status(&self) -> Option<String> {
        match &self.reading {
            Reading::Busy => Some(crate::i18n::t("pill.clipboard.selectReading").into()),
            Reading::Ready(selector) if selector.doc.is_empty() => Some(crate::i18n::t("pill.shelf.noText").into()),
            Reading::Ready(_) => None,
            Reading::Failed(error) => Some(crate::i18n::tf("pill.clipboard.selectFailed", &[("error", error)])),
        }
    }

    pub fn select_all(&mut self) {
        match &mut self.reading {
            Reading::Ready(selector) => selector.select_all(),
            Reading::Busy => self.select_all_pending = true,
            Reading::Failed(_) => {}
        }
    }

    /// Esc: borra la selección. `false` si no había: quien la usa decide si
    /// sale del modo.
    pub fn escape(&mut self) -> bool {
        self.select_all_pending = false;
        self.selector_mut().is_some_and(Selector::clear)
    }

    pub fn press(&mut self, map: Mapping, p: Point<Pixels>, clicks: usize, shift: bool) {
        let slop = map.slop(SLOP);
        let at = map.point(p);
        if let Some(selector) = self.selector_mut() {
            selector.press(at, clicks, shift, slop);
        }
    }

    /// El cursor se movió: arrastra si hay botón apretado. `true` si hay que
    /// volver a dibujar.
    pub fn moved(&mut self, map: Mapping, p: Point<Pixels>) -> bool {
        let slop = map.slop(SLOP);
        let at = map.point(p);
        let Some(selector) = self.selector_mut() else {
            return false;
        };
        let over = selector.doc.hit(at, slop).is_some();
        let dragged = selector.drag(at, slop);
        let changed = dragged || over != self.over_word;
        self.over_word = over;
        changed
    }

    pub fn release(&mut self) {
        if let Some(selector) = self.selector_mut() {
            selector.release();
        }
    }

    pub fn cursor_style(&self) -> CursorStyle {
        match self.selector() {
            Some(selector) if self.over_word || selector.pressing() => CursorStyle::IBeam,
            _ => CursorStyle::Arrow,
        }
    }

    fn text_to_copy(&self, all: bool) -> Option<String> {
        let selector = self.selector()?;
        let text = if all { selector.all_text() } else { selector.selected_text()? };
        (!text.trim().is_empty()).then_some(text)
    }

    /// Mientras algo se anima hay que pedir cuadros; quieta no cuesta nada.
    pub fn animating(&self, now: Instant) -> bool {
        let shown = now.saturating_duration_since(self.since);
        match &self.reading {
            Reading::Busy => true,
            Reading::Ready(_) => shown < DIM_IN || now.saturating_duration_since(self.ready_at) < SPREAD + FADE,
            Reading::Failed(_) => shown < DIM_IN,
        }
    }

    /// Lo que pinta el canvas, ya en coordenadas de la ventana.
    pub fn paint(&self, map: Mapping, now: Instant) -> TextPaint {
        let mut quads: Vec<(Bounds<Pixels>, f32, Background)> = Vec::new();
        let (ax, ay, aw, ah) = map.area();
        let shown = now.saturating_duration_since(self.since).as_secs_f32();
        let dim = anim::ease_smooth_out(shown / DIM_IN.as_secs_f32());
        quads.push((bounds(ax, ay, ax + aw, ay + ah), 0.0, gpui::black().opacity(DIM * dim).into()));
        match &self.reading {
            Reading::Busy => {
                // Un brillo que barre la foto de arriba abajo mientras lee.
                let t = (shown / SCAN_PERIOD).fract();
                let band = (ah * 0.18).clamp(60.0, 220.0);
                let top = ay - band + t * (ah + band);
                let half = band / 2.0;
                let glow = gpui::white().opacity(0.1);
                let clear = gpui::white().opacity(0.0);
                let parts = [(top, top + half, clear, glow), (top + half, top + band, glow, clear)];
                for (y0, y1, from, to) in parts {
                    let (y0, y1) = (y0.max(ay), y1.min(ay + ah));
                    if y1 > y0 {
                        let gradient = linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.));
                        quads.push((bounds(ax, y0, ax + aw, y1), 0.0, gradient));
                    }
                }
            }
            Reading::Ready(selector) => {
                let doc = &selector.doc;
                let range = selector.range();
                let n = doc.words.len().max(1) as f32;
                let lit = now.saturating_duration_since(self.ready_at).as_secs_f32();
                for (i, word) in doc.words.iter().enumerate() {
                    if range.is_some_and(|(lo, hi)| (lo..=hi).contains(&i)) {
                        continue;
                    }
                    let delay = SPREAD.as_secs_f32() * i as f32 / n;
                    let t = anim::segment(lit, delay, FADE.as_secs_f32());
                    if t <= 0.0 {
                        continue;
                    }
                    // Al encenderse cada palabra destella un poco y se asienta.
                    let glint = (t * std::f32::consts::PI).sin() * 0.22;
                    let alpha = WORD_ALPHA * anim::ease_smooth_out(t) + glint;
                    let pad = word.h * 0.12;
                    let rect = map.to_window((word.x - pad, word.y - pad, word.right() + pad, word.bottom() + pad));
                    let radius = ((rect.3 - rect.1) * 0.28).clamp(2.0, 6.0);
                    quads.push((bounds(rect.0, rect.1, rect.2, rect.3), radius, gpui::white().opacity(alpha).into()));
                }
                if let Some((lo, hi)) = range {
                    let accent = accent();
                    for band in doc.bands(lo, hi) {
                        let pad = (band.3 - band.1) * 0.14;
                        let rect = map.to_window((band.0 - pad, band.1 - pad, band.2 + pad, band.3 + pad));
                        let radius = ((rect.3 - rect.1) * 0.3).clamp(3.0, 8.0);
                        quads.push((bounds(rect.0, rect.1, rect.2, rect.3), radius, accent.opacity(BAND_ALPHA).into()));
                    }
                }
            }
            Reading::Failed(_) => {}
        }
        TextPaint { quads }
    }

    /// La barra flotante junto a lo elegido: «Copiar» y «Copiar todo». Sin
    /// selección, «Copiar todo» abajo al centro. Escondida mientras se arrastra.
    pub fn toolbar<V: TextHost>(&self, map: Mapping, cx: &mut Context<V>) -> Option<AnyElement> {
        let selector = self.selector()?;
        if selector.doc.is_empty() || selector.pressing() {
            return None;
        }
        let look = look();
        let (ax, ay, aw, ah) = map.area();
        let copied = self.copied.is_some_and(|at| at.elapsed() < COPIED_FOR);
        let range = selector.range();
        let (center, top) = match range {
            Some((lo, hi)) => {
                let bands = selector.doc.bands(lo, hi);
                let (x0, y0, x1, y1) = bands.iter().fold(
                    (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
                    |(x0, y0, x1, y1), b| (x0.min(b.0), y0.min(b.1), x1.max(b.2), y1.max(b.3)),
                );
                let (x0, y0, x1, y1) = map.to_window((x0, y0, x1, y1));
                let below = y1 + 10.0;
                let top = if below + BAR_H + 8.0 <= ay + ah {
                    below
                } else {
                    (y0 - 10.0 - BAR_H).max(ay + 8.0)
                };
                ((x0 + x1) / 2.0, top)
            }
            None => (ax + aw / 2.0, ay + ah - BAR_H - 28.0),
        };
        let left = (center - BAR_W / 2.0).clamp(ax + 8.0, (ax + aw - BAR_W - 8.0).max(ax + 8.0));
        let mut buttons: Vec<AnyElement> = Vec::new();
        if range.is_some() {
            let (icon, label) = if copied { ("icons/check.svg", crate::i18n::t("pill.peek.copied")) } else { ("icons/copy.svg", crate::i18n::t("page.common.copy")) };
            buttons.push(button("text-select-copy", icon, label, true, false, &look, cx));
            buttons.push(button("text-select-all", "icons/text-align-start.svg", crate::i18n::t("pill.clipboard.copyAll"), false, true, &look, cx));
        } else {
            let (icon, label) = if copied { ("icons/check.svg", crate::i18n::t("pill.peek.copied")) } else { ("icons/copy.svg", crate::i18n::t("pill.clipboard.copyAll")) };
            buttons.push(button("text-select-all", icon, label, false, true, &look, cx));
        }
        let bar = div()
            .id("text-select-bar")
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(4.))
            .rounded(px(look.radius))
            .bg(look.bg)
            .border_1()
            .border_color(look.border)
            .shadow_lg()
            .font_family(look.font)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(buttons)
            .with_animation(
                "text-select-bar-in",
                Animation::new(Duration::from_millis(160)).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(t).mt(px(4.0 * (1.0 - t))),
            );
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(BAR_W))
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }
}

/// Lo que pinta la capa: rectángulos redondeados, ya en la ventana.
pub struct TextPaint {
    quads: Vec<(Bounds<Pixels>, f32, Background)>,
}

impl TextPaint {
    pub fn paint(self, window: &mut Window) {
        for (rect, radius, background) in self.quads {
            window.paint_quad(fill(rect, background).corner_radii(px(radius)));
        }
    }
}

fn bounds(x0: f32, y0: f32, x1: f32, y1: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x0), px(y0)), size(px((x1 - x0).max(0.0)), px((y1 - y0).max(0.0))))
}

/// El acento del estilo de las ventanas (con «Atic», azul).
fn accent() -> Hsla {
    crate::theme::styled().map_or_else(|| rgb(BLUE).into(), |t| t.accent)
}

/// Los colores de la barra según el tema y el estilo de las ventanas.
struct Look {
    bg: Hsla,
    border: Hsla,
    text: Hsla,
    accent: Hsla,
    on_accent: Hsla,
    radius: f32,
    font: &'static str,
}

fn look() -> Look {
    match crate::theme::styled() {
        Some(t) => Look {
            bg: t.raised,
            border: t.border,
            text: t.text,
            accent: t.accent,
            on_accent: t.on_accent,
            radius: (t.r_pop + 4.0).max(12.0),
            font: t.font,
        },
        None if crate::theme::is_light() => Look {
            bg: rgb(0xfbfbf8).into(),
            border: gpui::black().opacity(0.08),
            text: rgb(0x141413).into(),
            accent: rgb(BLUE).into(),
            on_accent: gpui::white(),
            radius: 18.0,
            font: crate::fonts::UI,
        },
        None => Look {
            bg: rgb(0x1a1a18).into(),
            border: gpui::white().opacity(0.08),
            text: rgb(0xf0f0ea).into(),
            accent: rgb(BLUE).into(),
            on_accent: gpui::white(),
            radius: 18.0,
            font: crate::fonts::UI,
        },
    }
}

fn button<V: TextHost>(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    primary: bool,
    all: bool,
    look: &Look,
    cx: &mut Context<V>,
) -> AnyElement {
    let (rest, over, fg) = if primary {
        (look.accent, crate::hover::mix(look.accent, look.on_accent, 0.14), look.on_accent)
    } else {
        (look.text.opacity(0.0), look.text.opacity(0.09), look.text)
    };
    div()
        .id(id)
        .h(px(30.))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(15.))
        .cursor_pointer()
        .text_size(px(12.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(fg)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| {
            copy(view, all, cx);
        }))
        .child(svg().path(icon).size(px(14.)).text_color(fg))
        .child(label)
        .hover_bg(SharedString::from(format!("{id}-fx")), rest, over)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una línea de palabras de 10 px de alto, en `y`, con cada palabra de
    /// 8 px por letra y 6 px de espacio, desde `x`.
    fn line(x: f32, y: f32, text: &str) -> Vec<Word> {
        let mut at = x;
        text.split(' ')
            .map(|word| {
                let w = word.chars().count() as f32 * 8.0;
                let out = Word::new(word, at, y, w, 10.0);
                at += w + 6.0;
                out
            })
            .collect()
    }

    fn texts(doc: &Doc) -> Vec<&str> {
        doc.words.iter().map(|w| w.text.as_str()).collect()
    }

    #[test]
    fn ordena_de_arriba_abajo_y_de_izquierda_a_derecha() {
        // El OCR las entrega desordenadas y con palabras sueltas.
        let mut second = line(0.0, 14.0, "dos tres");
        second.reverse();
        let doc = Doc::new(vec![second, line(0.0, 0.0, "uno")]);
        assert_eq!(texts(&doc), ["uno", "dos", "tres"]);
        assert_eq!(doc.rows, vec![0..1, 1..3]);
    }

    #[test]
    fn dos_columnas_se_leen_una_despues_de_la_otra() {
        // Líneas largas (no celdas de una tabla) a cada lado de un hueco.
        let doc = Doc::new(vec![
            line(0.0, 0.0, "a1 aaaaaaaa aaaaaaaa"),
            line(300.0, 0.0, "b1 bbbbbbbb bbbbbbbb"),
            line(0.0, 14.0, "a2 aaaaaaaa aaaaaaaa"),
            line(300.0, 14.0, "b2 bbbbbbbb bbbbbbbb"),
            line(0.0, 28.0, "a3 aaaaaaaa aaaaaaaa"),
            line(300.0, 28.0, "b3 bbbbbbbb bbbbbbbb"),
        ]);
        let left: Vec<bool> = doc.words.iter().map(|w| w.text.starts_with('a')).collect();
        assert_eq!(left, [vec![true; 9], vec![false; 9]].concat());
        let labels: Vec<&str> = texts(&doc).into_iter().filter(|w| w.len() == 2).collect();
        assert_eq!(labels, ["a1", "a2", "a3", "b1", "b2", "b3"]);
    }

    #[test]
    fn una_linea_que_junta_dos_columnas_se_parte() {
        // Windows a veces entrega una sola línea que cruza el hueco.
        let mut joined = line(0.0, 0.0, "a1 a2 a3 a4 a5 a6");
        joined.extend(line(300.0, 0.0, "b1 b2 b3 b4 b5 b6"));
        let mut second = line(0.0, 14.0, "a7 a8 a9 a10 a11 a12");
        second.extend(line(300.0, 14.0, "b7 b8 b9 b10 b11 b12"));
        let doc = Doc::new(vec![joined, second]);
        let words = texts(&doc);
        let a7 = words.iter().position(|w| *w == "a7").unwrap();
        let b1 = words.iter().position(|w| *w == "b1").unwrap();
        assert!(a7 < b1, "la columna izquierda entera va primero: {words:?}");
    }

    #[test]
    fn una_tabla_se_lee_fila_por_fila() {
        let doc = Doc::new(vec![
            line(0.0, 0.0, "Nombre"),
            line(120.0, 0.0, "Edad"),
            line(0.0, 14.0, "Ana"),
            line(120.0, 14.0, "31"),
        ]);
        assert_eq!(texts(&doc), ["Nombre", "Edad", "Ana", "31"]);
        assert_eq!(doc.text(0, 3), "Nombre\tEdad\nAna\t31");
    }

    #[test]
    fn el_cursor_da_con_la_palabra() {
        let doc = Doc::new(vec![line(0.0, 0.0, "hola mundo"), line(0.0, 14.0, "chao")]);
        // Dentro de «mundo» (x 38..78).
        assert_eq!(doc.hit((60.0, 5.0), 8.0), Some(1));
        // En el espacio entre «hola» (0..32) y «mundo»: la más cercana.
        assert_eq!(doc.hit((34.0, 5.0), 8.0), Some(0));
        assert_eq!(doc.hit((37.0, 5.0), 8.0), Some(1));
        // Un poco a la derecha de la fila, todavía cerca.
        assert_eq!(doc.hit((84.0, 5.0), 8.0), Some(1));
        // Lejos de todo: nada.
        assert_eq!(doc.hit((300.0, 5.0), 8.0), None);
        assert_eq!(doc.hit((10.0, 80.0), 8.0), None);
    }

    #[test]
    fn arrastrar_a_la_derecha_de_una_linea_llega_a_su_final() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos tres"), line(0.0, 14.0, "cuatro")]);
        // Muy a la derecha de la primera fila, a su altura: «tres», no la de abajo.
        assert_eq!(doc.nearest((400.0, 5.0)), Some(2));
        // Debajo de todo: la palabra más cercana.
        assert_eq!(doc.nearest((5.0, 200.0)), Some(3));
        assert_eq!(Doc::default().nearest((0.0, 0.0)), None);
    }

    #[test]
    fn arrastrar_elige_en_orden_de_lectura() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos tres"), line(0.0, 14.0, "cuatro cinco")]);
        let mut s = Selector::new(doc);
        // Desde «dos» hasta «cuatro».
        s.press((40.0, 5.0), 1, false, 8.0);
        assert_eq!(s.range(), Some((1, 1)));
        s.drag((10.0, 19.0), 8.0);
        assert_eq!(s.range(), Some((1, 3)));
        assert_eq!(s.selected_text().as_deref(), Some("dos tres\ncuatro"));
        // Hacia atrás también: el ancla queda en «dos».
        s.drag((5.0, 5.0), 8.0);
        assert_eq!(s.range(), Some((0, 1)));
        s.release();
        assert!(!s.pressing());
    }

    #[test]
    fn un_temblor_no_es_arrastre() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos")]);
        let mut s = Selector::new(doc);
        s.press((5.0, 5.0), 1, false, 8.0);
        assert!(!s.drag((7.0, 6.0), 8.0));
        assert_eq!(s.range(), Some((0, 0)));
    }

    #[test]
    fn doble_y_triple_clic() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos tres"), line(0.0, 14.0, "cuatro cinco")]);
        let mut s = Selector::new(doc);
        s.press((40.0, 5.0), 2, false, 8.0);
        s.release();
        assert_eq!(s.selected_text().as_deref(), Some("dos"));
        s.press((40.0, 5.0), 3, false, 8.0);
        assert_eq!(s.selected_text().as_deref(), Some("uno dos tres"));
        // Tras el triple clic, arrastrar crece de a filas.
        s.drag((10.0, 19.0), 8.0);
        assert_eq!(s.selected_text().as_deref(), Some("uno dos tres\ncuatro cinco"));
    }

    #[test]
    fn clic_en_lo_vacio_borra_y_arrastrar_desde_ahi_elige() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos tres")]);
        let mut s = Selector::new(doc);
        s.select_all();
        assert_eq!(s.range(), Some((0, 2)));
        s.press((20.0, 60.0), 1, false, 8.0);
        assert_eq!(s.range(), None);
        // Barrer desde el margen hasta «dos».
        s.drag((40.0, 5.0), 8.0);
        assert_eq!(s.range(), Some((0, 1)));
    }

    #[test]
    fn shift_extiende_y_esc_borra() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos tres")]);
        let mut s = Selector::new(doc);
        s.press((5.0, 5.0), 1, false, 8.0);
        s.release();
        s.press((70.0, 5.0), 1, true, 8.0);
        s.release();
        assert_eq!(s.range(), Some((0, 2)));
        assert!(s.clear());
        assert!(!s.clear(), "el segundo Esc ya no tiene qué borrar");
    }

    #[test]
    fn el_texto_une_las_palabras_cortadas_con_guion() {
        let doc = Doc::new(vec![line(0.0, 0.0, "una pala-"), line(0.0, 14.0, "bra larga y Fin-"), line(0.0, 28.0, "Cosa")]);
        assert_eq!(doc.text(0, doc.words.len() - 1), "una palabra larga y Fin-\nCosa");
        let s = Selector::new(Doc::default());
        assert_eq!(s.all_text(), "");
        assert_eq!(s.selected_text(), None);
    }

    #[test]
    fn la_franja_de_una_fila_es_una_sola() {
        let doc = Doc::new(vec![line(0.0, 0.0, "uno dos tres"), line(0.0, 14.0, "cuatro")]);
        let bands = doc.bands(1, 3);
        assert_eq!(bands, vec![(30.0, 0.0, 92.0, 10.0), (0.0, 14.0, 48.0, 24.0)]);
    }

    #[test]
    fn de_la_ventana_a_la_imagen_y_de_vuelta() {
        // Al 200 %, con la foto corrida (10, 20) en la ventana.
        let map = Mapping::new(2.0, (10.0, 20.0), (400.0, 200.0));
        assert_eq!(map.to_image((15.0, 25.0)), (10.0, 10.0));
        assert_eq!(map.to_window((10.0, 10.0, 30.0, 50.0)), (15.0, 25.0, 25.0, 45.0));
        assert_eq!(map.slop(8.0), 16.0);
        assert_eq!(map.area(), (10.0, 20.0, 200.0, 100.0));
        // Una escala inválida no rompe nada.
        assert_eq!(Mapping::new(0.0, (0.0, 0.0), (1.0, 1.0)).scale, 1.0);
    }

    #[test]
    fn agrandar_repite_cada_pixel() {
        let bgra = [1, 2, 3, 4, 5, 6, 7, 8];
        let (w, h, big) = upscale(2, 1, &bgra, 2);
        assert_eq!((w, h), (4, 2));
        assert_eq!(&big[..16], &[1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]);
        assert_eq!(&big[16..], &big[..16]);
    }
}
