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

/// Dentro de una fila, un hueco más ancho que esto (en alturas) va con
/// tabulación: son celdas, no palabras.
const CELL_GAP: f32 = 2.0;
/// Una línea del OCR se parte en un hueco así de ancho (en alturas): son dos
/// columnas (o dos celdas) que el OCR juntó. Entre palabras hay ~0,3 (y en
/// código, con espacios de más, hasta ~2).
const SPLIT_GAP: f32 = 2.5;

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

/// Un hueco de una fila más ancho que esto (en alturas) puede ser una palabra
/// que el OCR se saltó (entre palabras hay ~0,3); más ancho que `HOLE_MAX`,
/// es espacio de la interfaz.
const HOLE_MIN: f32 = 0.7;
const HOLE_MAX: f32 = 14.0;

/// Un hueco de una fila donde pudo quedar una palabra sin leer.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// La palabra vecina: lo que se lea en el hueco va en su línea.
    pub anchor: Word,
    /// La vecina del otro lado, si la hay: su línea se junta con la del ancla.
    pub partner: Option<Word>,
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
    /// La columna de la interfaz donde cae (ver `Doc::with_dividers`).
    pane: usize,
}

impl Seg {
    fn new(words: Vec<Word>) -> Self {
        let x = words.iter().map(|w| w.x).fold(f32::INFINITY, f32::min);
        let y = words.iter().map(|w| w.y).fold(f32::INFINITY, f32::min);
        let right = words.iter().map(Word::right).fold(f32::NEG_INFINITY, f32::max);
        let bottom = words.iter().map(Word::bottom).fold(f32::NEG_INFINITY, f32::max);
        let h = median(words.iter().map(|w| w.h).collect());
        Self { words, x, y, right, bottom, h, pane: 0 }
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

/// Dos líneas seguidas son del mismo bloque con un hueco de hasta esto (en
/// alturas de la más alta).
const LINK_GAP: f32 = 1.2;
/// Alineadas a la izquierda (un título y sus párrafos, las celdas de una
/// columna), el hueco puede ser mayor: el espacio entre párrafos o filas.
const ALIGNED_GAP: f32 = 2.4;
/// Una línea más corta que esto (en alturas) puede ser una celda de tabla.
const CELL_WIDTH: f32 = 10.0;
/// Un bloque de prosa: sus líneas suelen medir al menos esto (en alturas). En
/// la prosa, una línea llena sigue en la de abajo (se unen al copiar); en
/// líneas más cortas (listas, menús, código) cada una va en la suya.
const PROSE_WIDTH: f32 = 16.0;

/// Un bloque: un párrafo, una columna, el contenido de una caja o una tabla
/// entera. La selección no se escapa de él al bajar de línea.
#[derive(Clone, Debug)]
struct Block {
    segs: Vec<Seg>,
    x: f32,
    y: f32,
    right: f32,
    bottom: f32,
    /// Las columnas de la tabla (x de inicio y fin), si es una.
    columns: Option<Vec<(f32, f32)>>,
    /// La columna de la interfaz del bloque (los divisores de `Doc`).
    pane: usize,
}

impl Block {
    fn new(segs: Vec<Seg>) -> Self {
        let x = segs.iter().map(|s| s.x).fold(f32::INFINITY, f32::min);
        let y = segs.iter().map(|s| s.y).fold(f32::INFINITY, f32::min);
        let right = segs.iter().map(|s| s.right).fold(f32::NEG_INFINITY, f32::max);
        let bottom = segs.iter().map(|s| s.bottom).fold(f32::NEG_INFINITY, f32::max);
        let pane = segs.first().map_or(0, |s| s.pane);
        Self { segs, x, y, right, bottom, columns: None, pane }
    }

    /// Una columna de celdas: varias líneas cortas.
    fn cellish(&self, unit: f32) -> bool {
        self.segs.len() >= 2 && median(self.segs.iter().map(|s| s.right - s.x).collect()) < unit * CELL_WIDTH
    }
}

/// ¿`b` sigue a `a` en el mismo bloque? Una debajo de la otra, cerca, de
/// letra parecida y una sobre la otra en horizontal: con anchos parecidos, o
/// alineadas a la izquierda (la última línea corta de un párrafo). Un título
/// a todo el ancho no se lleva las columnas de abajo: solo la que empieza
/// donde empieza él. Una regla de la interfaz (`rules`: y, x0, x1) entre las
/// dos tampoco las deja juntarse.
fn linked(a: &Seg, b: &Seg, unit: f32, rules: &[Rule]) -> bool {
    // Un divisor de la interfaz entre las dos: nunca son del mismo bloque.
    if a.pane != b.pane {
        return false;
    }
    let (up, down) = if a.mid_y() <= b.mid_y() { (a, b) } else { (b, a) };
    let shared = up.bottom.min(down.bottom) - up.y.max(down.y);
    // En la misma fila: se partieron por un hueco ancho, son otras columnas
    // (juntarlas mezclaba columnas con poco espacio entre ellas).
    if shared > 0.5 * up.h.min(down.h) {
        return false;
    }
    let tall = up.h.max(down.h);
    let gap = down.y - up.bottom;
    if gap > ALIGNED_GAP * tall || tall / up.h.min(down.h) > 2.2 {
        return false;
    }
    let overlap = up.right.min(down.right) - up.x.max(down.x);
    if overlap <= 0.0 {
        return false;
    }
    // Una regla entre las dos, que cruza lo que comparten en horizontal.
    let crossed = rules.iter().any(|&(y, x0, x1)| {
        y > up.bottom && y < down.y && x0 < up.right.min(down.right) && x1 > up.x.max(down.x)
    });
    if crossed {
        return false;
    }
    let (wa, wb) = (up.right - up.x, down.right - down.x);
    let (narrow, wide) = (wa.min(wb), wa.max(wb));
    let similar = narrow >= 0.5 * wide && overlap >= 0.6 * narrow && gap <= LINK_GAP * tall;
    // Alineadas a la izquierda: con letra parecida (la última línea corta de
    // un párrafo, las celdas de una columna) o si la de arriba es la angosta
    // (un título sobre su texto). Un título grande y ancho no se lleva la
    // columna de abajo.
    // (Una palabra sola con tilde y «g» tiene la caja más alta que una línea.)
    let same_size = tall / up.h.min(down.h) <= 1.45;
    let aligned = (up.x - down.x).abs() <= 1.5 * unit && overlap >= 0.8 * narrow && (same_size || wa <= wb);
    similar || aligned
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    let mut at = i;
    while parent[at] != root {
        let next = parent[at];
        parent[at] = root;
        at = next;
    }
    root
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[rb] = ra;
    }
}

/// Las líneas, juntas en bloques por cercanía (no por un hueco que cruce
/// todo: con un título encima o cajas con borde, las columnas no tienen uno).
fn blocks(segs: Vec<Seg>, unit: f32, rules: &[Rule]) -> Vec<Block> {
    let n = segs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            if linked(&segs[i], &segs[j], unit, rules) {
                union(&mut parent, i, j);
            }
        }
    }
    let mut groups: Vec<Vec<Seg>> = Vec::new();
    let mut index: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, seg) in segs.into_iter().enumerate() {
        let root = find(&mut parent, i);
        let slot = *index.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(seg);
    }
    tables(groups.into_iter().map(Block::new).collect(), unit)
}

/// Columnas de celdas lado a lado con las filas alineadas: una tabla. Se
/// juntan en un solo bloque que sabe dónde va cada columna.
fn tables(blocks: Vec<Block>, unit: f32) -> Vec<Block> {
    let n = blocks.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&blocks[i], &blocks[j]);
            if a.pane != b.pane || !a.cellish(unit) || !b.cellish(unit) {
                continue;
            }
            let side_by_side = a.right <= b.x || b.right <= a.x;
            let shared = a.bottom.min(b.bottom) - a.y.max(b.y);
            let shorter = (a.bottom - a.y).min(b.bottom - b.y);
            if !side_by_side || shared < 0.5 * shorter {
                continue;
            }
            let (fewer, other) = if a.segs.len() <= b.segs.len() { (a, b) } else { (b, a) };
            let aligned = fewer
                .segs
                .iter()
                .filter(|s| other.segs.iter().any(|o| (s.mid_y() - o.mid_y()).abs() < unit * 0.45))
                .count();
            if aligned * 10 >= fewer.segs.len() * 6 {
                union(&mut parent, i, j);
            }
        }
    }
    let mut groups: Vec<Vec<Block>> = Vec::new();
    let mut index: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, block) in blocks.into_iter().enumerate() {
        let root = find(&mut parent, i);
        let slot = *index.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(block);
    }
    groups
        .into_iter()
        .map(|mut group| {
            if group.len() == 1 {
                return group.pop().unwrap_or_else(|| Block::new(Vec::new()));
            }
            let mut table = Block::new(group.into_iter().flat_map(|b| b.segs).collect());
            table.columns = Some(columns_of(&table.segs));
            table
        })
        .collect::<Vec<_>>()
        .pipe(|blocks| absorb_cells(blocks, unit))
}

/// Una columna de la que el OCR leyó una sola celda (el resto eran textos
/// cortos que se saltó) no alcanza a ser columna: si sus celdas caen en filas
/// de una tabla y están al lado, son otra columna de ella.
fn absorb_cells(mut blocks: Vec<Block>, unit: f32) -> Vec<Block> {
    loop {
        let mut moved = false;
        for t in 0..blocks.len() {
            if blocks[t].columns.is_none() {
                continue;
            }
            let pick = (0..blocks.len()).find(|&i| {
                let (table, cell) = (&blocks[t], &blocks[i]);
                i != t
                    && cell.pane == table.pane
                    && cell.columns.is_none()
                    && cell.segs.iter().all(|s| s.right - s.x < unit * CELL_WIDTH)
                    && cell.y >= table.y - unit
                    && cell.bottom <= table.bottom + unit
                    && (cell.x - table.right).max(table.x - cell.right) < unit * CELL_WIDTH
                    && cell.segs.iter().all(|s| table.segs.iter().any(|o| (s.mid_y() - o.mid_y()).abs() < unit * 0.45))
            });
            let Some(i) = pick else {
                continue;
            };
            let cell = blocks.remove(i);
            let t = if i < t { t - 1 } else { t };
            let table = &mut blocks[t];
            let mut segs = std::mem::take(&mut table.segs);
            segs.extend(cell.segs);
            *table = Block::new(segs);
            table.columns = Some(columns_of(&table.segs));
            moved = true;
            break;
        }
        if !moved {
            return blocks;
        }
    }
}

/// Las columnas de una tabla: los tramos en x de sus celdas, juntando los que
/// se traslapan (el encabezado y los valores de una columna caen uno sobre
/// otro aunque no tengan el mismo ancho).
fn columns_of(segs: &[Seg]) -> Vec<(f32, f32)> {
    let mut spans: Vec<(f32, f32)> = segs.iter().map(|s| (s.x, s.right)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::new();
    for (x0, x1) in spans {
        match out.last_mut() {
            Some(last) if x0 < last.1 => last.1 = last.1.max(x1),
            _ => out.push((x0, x1)),
        }
    }
    out
}

/// `valor.pipe(f)` = `f(valor)`: para seguir una cadena.
trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

/// Los bloques en orden de lectura: columnas de izquierda a derecha y, si no
/// hay columnas, franjas de arriba abajo (un XY-cut sobre los bloques).
fn order(mut blocks: Vec<Block>, unit: f32, out: &mut Vec<Block>) {
    if blocks.len() <= 1 {
        out.extend(blocks);
        return;
    }
    // Franjas: huecos horizontales que ningún bloque cruza. Van primero: una
    // columna entera es un solo bloque, así que entre columnas casi nunca hay
    // un hueco horizontal común (y si lo hay, es porque son franjas de verdad).
    blocks.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut bands: Vec<Vec<Block>> = Vec::new();
    let mut reach = f32::NEG_INFINITY;
    for block in blocks {
        let starts = block.y >= reach;
        reach = if starts { block.bottom } else { reach.max(block.bottom) };
        match bands.last_mut() {
            Some(band) if !starts => band.push(block),
            _ => bands.push(vec![block]),
        }
    }
    if bands.len() > 1 {
        for band in bands {
            order(band, unit, out);
        }
        return;
    }
    // Columnas: el hueco vertical más ancho que ningún bloque cruza.
    let mut blocks = bands.pop().unwrap_or_default();
    blocks.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut reach = blocks[0].right;
    let mut best: Option<(f32, usize)> = None;
    for (i, block) in blocks.iter().enumerate().skip(1) {
        let gap = block.x - reach;
        if gap > 0.0 && best.is_none_or(|(widest, _)| gap > widest) {
            best = Some((gap, i));
        }
        reach = reach.max(block.right);
    }
    if let Some((_, at)) = best {
        let right = blocks.split_off(at);
        order(blocks, unit, out);
        order(right, unit, out);
        return;
    }
    // Ni franjas ni columnas (bloques que se traslapan): de arriba abajo y,
    // a la misma altura, de izquierda a derecha.
    let mut rest = blocks;
    rest.sort_by(|a, b| {
        if (a.y - b.y).abs() < unit {
            a.x.total_cmp(&b.x)
        } else {
            a.y.total_cmp(&b.y)
        }
    });
    out.extend(rest);
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

/// Qué va entre una fila y la anterior del mismo bloque al copiar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// La línea anterior estaba llena: es el mismo párrafo (un espacio).
    Join,
    /// Otra línea (una lista, una fila de tabla, el fin de un párrafo).
    Line,
    /// Un párrafo nuevo (hay más espacio arriba): una línea en blanco.
    Para,
}

/// ¿La línea empieza como un elemento de lista?
fn bullet(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some('•' | '·' | '◦' | '▪' | '–' | '—' | '-' | '*' | '→' | '►') => true,
        Some(c) if c.is_ascii_digit() || c.is_alphabetic() => {
            let rest: String = text.chars().skip_while(|c| c.is_ascii_digit()).collect();
            let numbered = text.chars().next().is_some_and(|c| c.is_ascii_digit()) && matches!(rest.chars().next(), Some('.' | ')'));
            let lettered = text.chars().count() == 2 && text.ends_with(')');
            numbered || lettered
        }
        _ => false,
    }
}

/// La columna de la tabla donde cae una palabra: la que más la cubre (o la
/// más cercana).
fn column_at(columns: &[(f32, f32)], word: &Word) -> usize {
    let center = word.x + word.w / 2.0;
    columns
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (a.0 - center).max(center - a.1).max(0.0);
            let db = (b.0 - center).max(center - b.1).max(0.0);
            da.total_cmp(&db)
        })
        .map_or(0, |(i, _)| i)
}

/// En qué columna cae `x`: cuántos divisores quedan a su izquierda.
fn pane_at(dividers: &[f32], x: f32) -> usize {
    dividers.iter().filter(|d| **d < x).count()
}

/// Una regla horizontal de la interfaz: su y y el tramo (x0, x1) que cubre.
pub type Rule = (f32, f32, f32);

/// ¿Son celdas de tabla alineadas unas con otras? `few` y `many` son los dos
/// lados de un divisor; `key` es la coordenada que se compara.
fn aligned_cells(few: &[&Seg], many: &[&Seg], unit: f32, key: impl Fn(&Seg) -> f32) -> bool {
    let cells = |side: &[&Seg]| side.len() >= 3 && median(side.iter().map(|s| s.right - s.x).collect()) < unit * CELL_WIDTH;
    if !cells(few) || !cells(many) {
        return false;
    }
    let aligned = few.iter().filter(|s| many.iter().any(|o| (key(s) - key(o)).abs() < unit * 0.45)).count();
    aligned * 10 >= few.len() * 6
}

/// De las reglas que se ven en la imagen, las que de verdad separan textos:
/// con texto arriba y abajo de su tramo, sin palabras que las crucen (el borde
/// de una caja con texto encima no cuenta) y sin ser la raya bajo el
/// encabezado de una tabla, que se lee entera.
fn valid_rules(segs: &[Seg], candidates: &[Rule], unit: f32) -> Vec<Rule> {
    let mut out = Vec::new();
    for &(y, x0, x1) in candidates {
        let within = |s: &Seg| s.right > x0 && s.x < x1;
        let crossing = segs
            .iter()
            .flat_map(|s| &s.words)
            .filter(|w| w.right() > x0 && w.x < x1 && w.y + w.h * 0.2 < y && w.bottom() - w.h * 0.2 > y)
            .count();
        if crossing >= 2 {
            continue;
        }
        let (above, below): (Vec<&Seg>, Vec<&Seg>) = segs.iter().filter(|s| within(s)).partition(|s| s.mid_y() < y);
        if above.is_empty() || below.is_empty() {
            continue;
        }
        let (few, many) = if above.len() <= below.len() { (&above, &below) } else { (&below, &above) };
        if aligned_cells(few, many, unit, |s| s.x) || aligned_cells(few, many, unit, |s| s.right) {
            continue;
        }
        out.push((y, x0, x1));
    }
    out
}

/// De los divisores que se ven en la imagen, los que de verdad separan
/// columnas de texto: con texto a los dos lados, sin palabras que los crucen
/// (la raya de una caja o de una tabla no es un divisor) y sin ser el borde
/// entre las celdas de una tabla, que se lee por filas.
fn valid_dividers(segs: &[Seg], candidates: &[f32], unit: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &x in candidates {
        let crossing = segs
            .iter()
            .flat_map(|s| &s.words)
            .filter(|w| w.x + w.w * 0.2 < x && w.right() - w.w * 0.2 > x)
            .count();
        if crossing >= 2 {
            continue;
        }
        let (left, right): (Vec<&Seg>, Vec<&Seg>) = segs.iter().partition(|s| (s.x + s.right) / 2.0 < x);
        if left.is_empty() || right.is_empty() {
            continue;
        }
        let cells = |side: &[&Seg]| {
            side.len() >= 3 && median(side.iter().map(|s| s.right - s.x).collect()) < unit * CELL_WIDTH
        };
        if cells(&left) && cells(&right) {
            let (few, many) = if left.len() <= right.len() { (&left, &right) } else { (&right, &left) };
            let aligned = few
                .iter()
                .filter(|s| many.iter().any(|o| (s.mid_y() - o.mid_y()).abs() < unit * 0.45))
                .count();
            if aligned * 10 >= few.len() * 6 {
                continue;
            }
        }
        out.push(x);
    }
    out
}

/// Las palabras de la imagen en orden de lectura, agrupadas en filas.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub words: Vec<Word>,
    /// Cada fila: el rango de `words` que ocupa.
    pub rows: Vec<Range<usize>>,
    /// La fila de cada palabra.
    row_of: Vec<usize>,
    /// El bloque (párrafo, columna, caja o tabla) de cada fila.
    pub block_of: Vec<usize>,
    /// Qué va entre cada fila y la anterior (`Line` en la primera de un bloque).
    breaks: Vec<Break>,
    /// La columna de tabla de cada palabra (0 fuera de una tabla).
    col_of: Vec<usize>,
    /// Las columnas de cada bloque que es tabla.
    tables: Vec<Option<Vec<(f32, f32)>>>,
    /// La columna de la interfaz de cada bloque.
    block_pane: Vec<usize>,
    /// Los divisores verticales de la interfaz (x en píxeles de la imagen) que
    /// separan sus columnas: una barra lateral, el centro, un panel.
    seps: Vec<f32>,
    /// Las reglas horizontales de la interfaz (una barra de herramientas, el
    /// borde de una tarjeta): lo de arriba y lo de abajo son bloques distintos.
    rules: Vec<Rule>,
    /// La altura típica de una línea.
    unit: f32,
}

impl Doc {
    /// Desde las líneas del OCR, con cajas en píxeles de la imagen.
    pub fn new(lines: Vec<Vec<Word>>) -> Self {
        Self::with_dividers(lines, &[])
    }

    /// Como `new`, con los divisores verticales que se ven en la imagen
    /// (`find_dividers`). Cada columna que dejan es un mundo aparte: sus
    /// líneas no se juntan con las de al lado aunque queden a la misma altura
    /// y cerca, se leen una columna entera tras otra y un arrastre no se sale
    /// de la suya mientras el cursor no pase a la de al lado.
    pub fn with_dividers(lines: Vec<Vec<Word>>, dividers: &[f32]) -> Self {
        Self::with_guides(lines, dividers, &[])
    }

    /// Como `with_dividers`, y con las reglas horizontales que se ven en la
    /// imagen (`find_rules`): dos líneas con una regla entre medio no son del
    /// mismo bloque, aunque estén cerca y alineadas.
    pub fn with_guides(lines: Vec<Vec<Word>>, dividers: &[f32], rules: &[Rule]) -> Self {
        let mut segs = segments(lines);
        if segs.is_empty() {
            return Self::default();
        }
        let unit = median(segs.iter().map(|s| s.h).collect());
        let seps = valid_dividers(&segs, dividers, unit);
        let rules = valid_rules(&segs, rules, unit);
        for seg in &mut segs {
            seg.pane = pane_at(&seps, (seg.x + seg.right) / 2.0);
        }
        let mut by_pane: Vec<Vec<Block>> = vec![Vec::new(); seps.len() + 1];
        for block in blocks(segs, unit, &rules) {
            let pane = block.pane.min(seps.len());
            by_pane[pane].push(block);
        }
        let mut ordered = Vec::new();
        for pane in by_pane {
            order(pane, unit, &mut ordered);
        }
        let mut doc = Doc { unit, seps, rules, ..Default::default() };
        for (index, block) in ordered.into_iter().enumerate() {
            let columns = block.columns.clone();
            doc.block_pane.push(block.pane);
            let first = doc.rows.len();
            for row in rows_of(block.segs) {
                let start = doc.words.len();
                let row_index = doc.rows.len();
                for word in row {
                    let col = columns.as_ref().map_or(0, |cols| column_at(cols, &word));
                    doc.words.push(word);
                    doc.row_of.push(row_index);
                    doc.col_of.push(col);
                }
                doc.rows.push(start..doc.words.len());
                doc.block_of.push(index);
            }
            let table = columns.is_some();
            doc.tables.push(columns);
            let breaks = doc.block_breaks(first, table);
            doc.breaks.extend(breaks);
        }
        doc
    }

    /// Qué va antes de cada fila de un bloque (desde la fila `first`).
    fn block_breaks(&self, first: usize, table: bool) -> Vec<Break> {
        let rows: Vec<(f32, f32, f32, f32)> = (first..self.rows.len()).map(|row| self.row_bounds(row)).collect();
        let mut out = vec![Break::Line; rows.len()];
        if table || rows.len() < 2 {
            return out;
        }
        let prose = median(rows.iter().map(|r| r.2 - r.0).collect()) >= self.unit * PROSE_WIDTH;
        let mid = |r: &(f32, f32, f32, f32)| (r.1 + r.3) / 2.0;
        let pitches: Vec<f32> = rows.windows(2).map(|w| mid(&w[1]) - mid(&w[0])).collect();
        let line = median(pitches.clone()).min(self.unit * 1.8);
        let width_of = |r: &(f32, f32, f32, f32)| r.2 - r.0;
        for k in 1..rows.len() {
            let (prev, cur) = (rows[k - 1], rows[k]);
            let first_word = &self.words[self.rows[first + k].start].text;
            // Si la primera palabra de abajo cabía al final de la de arriba, el
            // salto no lo hizo el ancho: es otra línea (una lista, un verso).
            // El borde es el de las líneas vecinas, no el de todo el bloque (un
            // título ancho arriba haría parecer cortas todas las líneas).
            let next_right = rows.get(k + 1).map_or(f32::NEG_INFINITY, |r| r.2);
            let edge = prev.2.max(cur.2).max(next_right);
            let lead = &self.words[self.rows[first + k].start];
            let fits = prev.2 + self.unit * 0.6 + lead.w <= edge;
            let full = !fits && width_of(&prev) >= (edge - prev.0) * 0.75;
            let indented = cur.0 > prev.0 + self.unit * 1.5;
            out[k] = if pitches[k - 1] > line * 1.3 && pitches[k - 1] > self.unit * 1.6 {
                Break::Para
            } else if !prose || bullet(first_word) || !full || indented {
                Break::Line
            } else {
                Break::Join
            };
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
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

    /// El bloque de la palabra `i`.
    pub fn block_of_word(&self, i: usize) -> Option<usize> {
        self.row_of.get(i).map(|row| self.block_of[*row])
    }

    /// (x0, y0, x1, y1) de un bloque.
    fn block_bounds(&self, block: usize) -> Option<(f32, f32, f32, f32)> {
        let rows: Vec<usize> = (0..self.rows.len()).filter(|r| self.block_of[*r] == block).collect();
        let bounds: Vec<_> = rows.iter().map(|r| self.row_bounds(*r)).collect();
        (!bounds.is_empty()).then(|| {
            bounds.iter().fold((f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY), |a, b| {
                (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
            })
        })
    }

    /// Los divisores de columnas de la interfaz (x de la imagen).
    pub fn dividers(&self) -> &[f32] {
        &self.seps
    }

    /// Las reglas horizontales de la interfaz (y, x0, x1 de la imagen).
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Huecos dentro de las filas donde el OCR pudo saltarse una palabra: el
    /// espacio entre dos palabras vecinas más ancho que un espacio normal, y el
    /// margen izquierdo de una fila que empieza más adentro que su bloque.
    /// Cada uno: (x, y, ancho, alto, la palabra a cuya línea pertenece lo que
    /// se lea ahí, y la vecina del otro lado, si la hay).
    pub fn holes(&self) -> Vec<Hole> {
        let mut out = Vec::new();
        // Entre vecinas de la misma línea de base, estén o no en el mismo
        // bloque: una palabra saltada parte la línea del OCR en dos.
        for a in &self.words {
            let next = self
                .words
                .iter()
                .filter(|b| {
                    let shared = a.bottom().min(b.bottom()) - a.y.max(b.y);
                    b.x >= a.right() - a.h * 0.1 && shared >= 0.5 * a.h.min(b.h)
                })
                .min_by(|p, q| p.x.total_cmp(&q.x));
            let Some(b) = next else {
                continue;
            };
            let gap = b.x - a.right();
            let tall = a.h.max(b.h);
            if gap >= HOLE_MIN * tall && gap <= HOLE_MAX * tall {
                let (top, bottom) = (a.y.min(b.y), a.bottom().max(b.bottom()));
                out.push(Hole { x: a.right(), y: top, w: gap, h: bottom - top, anchor: a.clone(), partner: Some(b.clone()) });
            }
        }
        for (row, range) in self.rows.iter().enumerate() {
            let words = &self.words[range.clone()];
            let (x0, y0, _, y1) = self.row_bounds(row);
            let h = (y1 - y0).max(1.0);
            // La fila empieza más adentro que su bloque (de dos o más filas):
            // puede faltarle el principio.
            let Some(first) = words.first() else {
                continue;
            };
            let block = self.block_of[row];
            let rows_in_block = self.block_of.iter().filter(|b| **b == block).count();
            if rows_in_block < 2 || self.tables[block].is_some() {
                continue;
            }
            if let Some((bx, _, _, _)) = self.block_bounds(block) {
                let gap = x0 - bx;
                if gap >= HOLE_MIN * h && gap <= HOLE_MAX * h {
                    out.push(Hole { x: bx, y: y0, w: gap, h, anchor: first.clone(), partner: None });
                }
            }
        }
        out
    }

    /// ¿La fila cae en la columna `pane` (o en cualquiera, si es `None`)?
    fn in_pane(&self, row: usize, pane: Option<usize>) -> bool {
        pane.is_none_or(|k| self.block_pane.get(self.block_of[row]) == Some(&k))
    }

    /// Hasta dónde llega un arrastre que empezó en el bloque `anchor`: mientras
    /// el cursor no esté sobre otro bloque, se queda en el suyo (soltar un poco
    /// a la derecha del final de un párrafo no se lleva la columna de al lado).
    /// Con columnas de la interfaz, el arrastre vive en la columna donde está el
    /// cursor: solo entra a la de al lado cuando el cursor pasa por encima.
    pub fn nearest_from(&self, p: (f32, f32), anchor: Option<usize>) -> Option<usize> {
        let Some(anchor) = anchor else {
            return self.nearest(p);
        };
        let columns = !self.seps.is_empty();
        let anchor_pane = self.block_pane.get(anchor).copied();
        if columns {
            let here = pane_at(&self.seps, p.0);
            if anchor_pane.is_some_and(|k| k != here) {
                return self.nearest_in(p, Some(here)).or_else(|| self.nearest(p));
            }
        }
        let scope = anchor_pane.filter(|_| columns);
        let slack = self.unit * 0.5;
        let over_other = (0..self.tables.len()).filter(|b| *b != anchor).any(|b| {
            self.block_bounds(b).is_some_and(|(x0, y0, x1, y1)| {
                p.0 >= x0 - slack && p.0 <= x1 + slack && p.1 >= y0 - slack && p.1 <= y1 + slack
            })
        });
        let (_, ay0, _, ay1) = self.block_bounds(anchor)?;
        if over_other || p.1 < ay0 - self.unit * 2.0 || p.1 > ay1 + self.unit * 2.0 {
            return self.nearest_in(p, scope).or_else(|| self.nearest(p));
        }
        // En el bloque propio: la fila a esa altura (o la más cercana) y, en
        // ella, la palabra más cercana.
        let row = (0..self.rows.len()).filter(|r| self.block_of[*r] == anchor).min_by(|a, b| {
            let d = |r: usize| {
                let (_, y0, _, y1) = self.row_bounds(r);
                (y0 - p.1).max(p.1 - y1).max(0.0)
            };
            d(*a).total_cmp(&d(*b))
        })?;
        self.rows[row]
            .clone()
            .map(|i| (i, self.words[i].distance(p, 1.0)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// La palabra hasta donde llega un arrastre en `p`: en la fila a esa
    /// altura (la más cercana en horizontal, si hay columnas), o si no la
    /// palabra más cercana. Siempre hay una si la imagen tiene texto.
    pub fn nearest(&self, p: (f32, f32)) -> Option<usize> {
        self.nearest_in(p, None)
    }

    /// Como `nearest`, solo entre las filas de la columna `pane`.
    fn nearest_in(&self, p: (f32, f32), pane: Option<usize>) -> Option<usize> {
        let mut row_pick: Option<(usize, f32)> = None;
        for row in (0..self.rows.len()).filter(|r| self.in_pane(*r, pane)) {
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
            .filter(|(i, _)| self.in_pane(self.row_of[*i], pane))
            .map(|(i, word)| (i, word.distance(p, 1.0)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// ¿La fila es de una tabla?
    fn in_table(&self, row: usize) -> bool {
        self.tables.get(self.block_of[row]).is_some_and(Option::is_some)
    }

    /// El texto de las palabras `lo..=hi`, como se lee:
    /// - En una fila, espacios; en una tabla, una tabulación por columna (las
    ///   celdas vacías también), así se pega como tabla en una planilla.
    /// - Las líneas de un mismo párrafo, unidas (y la palabra cortada con
    ///   guion, entera); otra línea, `\n`; un párrafo o un bloque nuevo, una
    ///   línea en blanco.
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
                    if self.in_table(row) {
                        let tabs = self.col_of[i].saturating_sub(self.col_of[i - 1]);
                        if tabs == 0 {
                            out.push(' ');
                        }
                        for _ in 0..tabs {
                            out.push('\t');
                        }
                    } else {
                        out.push(if word.x - prev.right() >= self.unit * CELL_GAP { '\t' } else { ' ' });
                    }
                } else if self.block_of[row] != self.block_of[prev_row] {
                    // Un párrafo que sigue en la columna siguiente: sin punto
                    // al final, en minúscula al empezar y más arriba (es otra
                    // columna, no lo que viene debajo).
                    let open = !prev.text.ends_with(['.', '!', '?', ':', ';', '"', '»', ')']);
                    let next_column = word.y < prev.y && word.x > prev.x;
                    if open && next_column && word.text.chars().next().is_some_and(char::is_lowercase) && !self.in_table(row) {
                        out.push(' ');
                    } else {
                        out.push_str("\n\n");
                    }
                } else {
                    match self.breaks[row] {
                        // Sin el guion: la palabra sigue (aunque la línea sea corta).
                        Break::Join | Break::Line if hyphenated(&prev.text, &word.text) => {
                            out.pop();
                        }
                        Break::Join => out.push(' '),
                        Break::Line => out.push('\n'),
                        Break::Para => out.push_str("\n\n"),
                    }
                }
                // Una fila de tabla que empieza en una columna corrida.
                if row != prev_row && self.in_table(row) {
                    for _ in 0..self.col_of[i] {
                        out.push('\t');
                    }
                }
            }
            out.push_str(&word.text);
        }
        out
    }

    /// Lo elegido como tabla HTML, si está todo dentro de una tabla: así se
    /// pega como tabla también en un documento o un correo.
    pub fn table_html(&self, lo: usize, hi: usize) -> Option<String> {
        if self.words.is_empty() || lo >= self.words.len() {
            return None;
        }
        let hi = hi.min(self.words.len() - 1);
        let (first, last) = (self.row_of[lo], self.row_of[hi]);
        let block = self.block_of[first];
        if self.block_of[last] != block || !self.in_table(first) {
            return None;
        }
        let columns = self.tables[block].as_ref()?.len();
        let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let mut html = String::from("<table>");
        for row in first..=last {
            let mut cells = vec![String::new(); columns];
            for i in self.rows[row].clone().filter(|i| (lo..=hi).contains(i)) {
                let cell = &mut cells[self.col_of[i].min(columns - 1)];
                if !cell.is_empty() {
                    cell.push(' ');
                }
                cell.push_str(&escape(&self.words[i].text));
            }
            html.push_str("<tr>");
            for cell in cells {
                html.push_str(&format!("<td>{cell}</td>"));
            }
            html.push_str("</tr>");
        }
        html.push_str("</table>");
        Some(html)
    }

    /// Las casillas de las tablas donde no se leyó nada (x, y, ancho, alto y
    /// dónde empieza la columna anterior):
    /// el OCR se salta los textos cortos sueltos («7», «3»), y leídas solas sí
    /// salen (`read_parts`).
    pub fn empty_cells(&self) -> Vec<(f32, f32, f32, f32, Option<f32>)> {
        let mut out = Vec::new();
        for (block, columns) in self.tables.iter().enumerate() {
            let Some(columns) = columns else {
                continue;
            };
            for row in (0..self.rows.len()).filter(|r| self.block_of[*r] == block) {
                let (_, y0, _, y1) = self.row_bounds(row);
                let h = y1 - y0;
                for (c, (x0, x1)) in columns.iter().enumerate() {
                    if !self.rows[row].clone().any(|i| self.col_of[i] == c) {
                        // Un poco de aire a los lados (con el texto pegado al
                        // borde del recorte el OCR no lo ve), sin llegar al
                        // medio del hueco con la columna vecina: ahí va el
                        // borde de la celda, y una raya también lo confunde.
                        // Hacia afuera de la tabla, poco: no se sabe dónde está su borde.
                        let left_room = c.checked_sub(1).map_or(h * 0.2, |p| (x0 - columns[p].1) * 0.35);
                        let right_room = columns.get(c + 1).map_or(h * 0.2, |n| (n.0 - x1) * 0.35);
                        let (pad_l, pad_r) = (left_room.clamp(0.0, h * 0.6), right_room.clamp(0.0, h * 0.6));
                        // Desde dónde leerla con contexto: la columna anterior.
                        let before = c.checked_sub(1).map(|p| columns[p].0);
                        out.push((x0 - pad_l, y0, x1 - x0 + pad_l + pad_r, h, before));
                    }
                }
            }
        }
        out
    }

    /// Las palabras del párrafo de la palabra `i` (las filas unidas por
    /// `Join`): lo que elige el triple clic.
    pub fn paragraph_range(&self, i: usize) -> Range<usize> {
        let Some(&row) = self.row_of.get(i) else {
            return i..i + 1;
        };
        let block = self.block_of[row];
        let mut first = row;
        while first > 0 && self.block_of[first - 1] == block && self.breaks[first] == Break::Join {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < self.rows.len() && self.block_of[last + 1] == block && self.breaks[last + 1] == Break::Join {
            last += 1;
        }
        self.rows[first].start..self.rows[last].end
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
/// clic, de a párrafo (en una tabla, de a fila).
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
            Unit::Row => (self.doc.paragraph_range(lo).start, self.doc.paragraph_range(hi).end.saturating_sub(1)),
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
        let anchor_block = self.selection.map(|s| s.anchor).or_else(|| self.doc.nearest(start)).and_then(|i| self.doc.block_of_word(i));
        let Some(focus) = self.doc.nearest_from(p, anchor_block) else {
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

/// Un trozo de la imagen que se lee aparte: (x, y, ancho, alto) en píxeles.
pub type Part = (u32, u32, u32, u32);

/// Lee la imagen. Corre en un hilo de fondo. El OCR lee cada uno de `parts`
/// por separado (cada monitor de una foto del escritorio entero): Vision
/// achica una imagen muy ancha antes de leerla y pierde la letra normal; con
/// la MacBook y el externo juntos leía un tercio de las palabras que lee un
/// monitor solo. Sin trozos, la imagen entera.
pub fn read_parts(width: u32, height: u32, bgra: &[u8], parts: &[Part]) -> Result<Doc, String> {
    if width == 0 || height == 0 || bgra.len() < width as usize * height as usize * 4 {
        return Err("la imagen está vacía".into());
    }
    let mut lines = recognize_parts(width, height, bgra, parts, crate::ocr::recognize_lines)?;
    let dividers = find_dividers(width, height, bgra);
    let rules = find_rules(width, height, bgra);
    let mut doc = Doc::with_guides(lines.clone(), &dividers, &rules);
    // Las casillas vacías de las tablas, leídas solas: el OCR se salta los
    // textos cortos sueltos de una imagen grande y en un recorte los ve.
    let cells = doc.empty_cells();
    let mut found = 0;
    for (x, y, w, h, before) in cells.into_iter().take(MAX_CELL_READS) {
        // Primero la casilla sola; si no sale nada, con la columna de al lado
        // de contexto (un dígito suelto, Vision a veces lo lee solo así), y de
        // eso, lo que cae en la casilla.
        // El OCR es caprichoso con lo suelto: se prueba de varias formas y vale
        // la primera que lea algo (son recortes chicos).
        let mut attempts = vec![(x, true), (x, false)];
        if let Some(from) = before {
            attempts.extend([(from, false), (from, true)]);
        }
        for (from, canvas) in attempts {
            let words = read_box(width, height, bgra, from, y, x + w - from, h, canvas, 2, false, false)
                .into_iter()
                .filter(|word| {
                    let center = word.x + word.w / 2.0;
                    (x - h * 0.3..=x + w + h * 0.3).contains(&center)
                })
                .collect::<Vec<_>>();
            if !words.is_empty() {
                found += words.len();
                lines.push(words);
                break;
            }
        }
    }
    if found > 0 {
        doc = Doc::with_guides(lines.clone(), &dividers, &rules);
    }
    // Los huecos con tinta dentro de una fila: una palabra que el OCR se
    // saltó (texto chico o gris, una cifra en negrita). Se relee el hueco
    // solo y, si no sale nada, con la palabra de al lado de contexto.
    let mut filled = 0;
    let debug = std::env::var_os("PILL_OCR_DEBUG").is_some();
    for hole in doc.holes().into_iter().filter(|h| has_ink(width, height, bgra, h)).take(MAX_HOLE_READS) {
        let (x, y, w, h) = (hole.x, hole.y, hole.w, hole.h);
        if debug {
            eprintln!("hueco tras «{}» ({x:.0},{y:.0} {w:.0}×{h:.0}) hasta {:?}", hole.anchor.text, hole.partner.as_ref().map(|p| &p.text));
        }
        // Primero el hueco solo; después con las vecinas de contexto (una
        // frase entera se lee mejor que un trozo), y al final más grande.
        let mut attempts = vec![(x, x + w, true, 2, false, false), (x, x + w, false, 2, false, false)];
        // Con contexto, un poco de aire a los lados: pegado al borde del
        // recorte, el OCR tampoco lo lee.
        let air = h * 0.5;
        if let Some(partner) = &hole.partner {
            attempts.push((hole.anchor.x, x + w, false, 2, false, false));
            attempts.push((hole.anchor.x - air, partner.right() + air, false, 3, true, false));
            attempts.push((hole.anchor.x - air, partner.right() + air, false, 3, true, true));
        } else {
            attempts.push((x - air, hole.anchor.right() + air, false, 3, true, false));
            attempts.push((x - air, hole.anchor.right() + air, false, 3, true, true));
        }
        for (from, to, canvas, zoom, smooth, enhance) in attempts {
            let raw = read_box(width, height, bgra, from, y, to - from, h, canvas, zoom, smooth, enhance);
            if debug {
                eprintln!(
                    "  intento {from:.0}..{to:.0} lienzo={canvas} x{zoom} suave={smooth} contraste={enhance}: {:?}",
                    raw.iter().map(|w| format!("{}@{:.0}", w.text, w.x)).collect::<Vec<_>>()
                );
            }
            let words: Vec<Word> = raw
                .into_iter()
                .filter(|word| {
                    let center = word.x + word.w / 2.0;
                    (x - h * 0.3..=x + w + h * 0.3).contains(&center) && word.text.chars().any(char::is_alphanumeric)
                })
                .collect();
            if !words.is_empty() {
                if debug {
                    eprintln!("  leído: {:?}", words.iter().map(|w| &w.text).collect::<Vec<_>>());
                }
                filled += words.len();
                attach(&mut lines, &hole, words);
                break;
            }
        }
    }
    if filled == 0 {
        return Ok(doc);
    }
    Ok(Doc::with_guides(lines, &dividers, &rules))
}

/// Huecos que se releen, a lo más (cada uno son hasta tres pasadas chicas del OCR).
const MAX_HOLE_READS: usize = 40;
/// Cuánto se aparta del fondo (0–255) un píxel de letra.
const INK_STEP: f32 = 48.0;

/// ¿Hay algo dibujado en el hueco? Trazos sobre el fondo en bastantes columnas
/// (una palabra), sin ser un relleno parejo (un botón, una raya).
fn has_ink(width: u32, height: u32, bgra: &[u8], hole: &Hole) -> bool {
    let (x0, y0) = (hole.x.max(0.0) as usize, hole.y.max(0.0) as usize);
    let (x1, y1) = (((hole.x + hole.w) as usize).min(width as usize), ((hole.y + hole.h) as usize).min(height as usize));
    if x1 <= x0 + 2 || y1 <= y0 + 2 {
        return false;
    }
    let luma = |x: usize, y: usize| {
        let i = (y * width as usize + x) * 4;
        0.114 * f32::from(bgra[i]) + 0.587 * f32::from(bgra[i + 1]) + 0.299 * f32::from(bgra[i + 2])
    };
    // El fondo: el tono más repetido.
    let mut bins = [0usize; 16];
    for y in y0..y1 {
        for x in x0..x1 {
            bins[(luma(x, y) / 16.0) as usize] += 1;
        }
    }
    let bin = bins.iter().enumerate().max_by_key(|(_, n)| **n).map_or(0, |(i, _)| i);
    let background = bin as f32 * 16.0 + 8.0;
    let total = (x1 - x0) * (y1 - y0);
    let mut inked = 0;
    let mut columns = 0;
    for x in x0..x1 {
        let hits = (y0..y1).filter(|&y| (luma(x, y) - background).abs() > INK_STEP).count();
        inked += hits;
        if hits > 0 {
            columns += 1;
        }
    }
    let min_columns = ((hole.h * 0.4) as usize).max(3);
    columns >= min_columns && inked * 100 < total * 60
}

/// Pone lo leído en un hueco en la línea de su ancla y, si la vecina del
/// otro lado quedó en otra línea del OCR (la palabra saltada la partió),
/// junta las dos.
fn attach(lines: &mut Vec<Vec<Word>>, hole: &Hole, words: Vec<Word>) {
    let line_of = |lines: &[Vec<Word>], word: &Word| lines.iter().position(|line| line.contains(word));
    let Some(at) = line_of(lines, &hole.anchor) else {
        lines.push(words);
        return;
    };
    lines[at].extend(words);
    if let Some(other) = hole.partner.as_ref().and_then(|p| line_of(lines, p)).filter(|o| *o != at) {
        let moved = std::mem::take(&mut lines[other]);
        lines[at].extend(moved);
    }
}

/// Las reglas horizontales de una captura de interfaz (una barra de
/// herramientas, el borde de una tarjeta, el pie de un panel): como
/// `find_dividers`, pero a lo ancho. Cada una con su y y el tramo que cubre.
fn find_rules(width: u32, height: u32, bgra: &[u8]) -> Vec<Rule> {
    let (w, h) = (width as usize, height as usize);
    if w < 120 || h < 240 || bgra.len() < w * h * 4 {
        return Vec::new();
    }
    let samples = w.min(DIVIDER_ROWS);
    let luma = |a: usize, s: usize| {
        let x = (s * w / samples + w / (2 * samples)).min(w - 1);
        let i = (a * w + x) * 4;
        0.114 * f32::from(bgra[i]) + 0.587 * f32::from(bgra[i + 1]) + 0.299 * f32::from(bgra[i + 2])
    };
    divider_runs(h, samples, luma, RULE_SPAN, MAX_RULES)
        .into_iter()
        .map(|(y, s0, s1)| (y, (s0 * w / samples) as f32, ((s1 + 1) * w / samples) as f32))
        .collect()
}

// --- Divisores de la interfaz ---------------------------------------------------

/// Filas de la imagen que se miran para buscar divisores.
const DIVIDER_ROWS: usize = 300;
/// Cuánto cambia el fondo (0–255) entre un lado y otro de un divisor sin raya.
const DIVIDER_STEP: f32 = 5.0;
/// Cuánto se aparta una raya fina del fondo que la rodea.
const DIVIDER_LINE: f32 = 8.0;
/// Lo que debe cubrir un divisor de la altura de la imagen.
const DIVIDER_SPAN: f32 = 0.4;
/// Cuánto puede variar el tono (0–255) a lo largo de un borde, entre el
/// cuarto más claro y el más oscuro de sus muestras.
const DIVIDER_EVEN: f32 = 24.0;
/// Más divisores que esto son una cuadrícula, no columnas.
const MAX_DIVIDERS: usize = 6;
/// Lo que debe cubrir una regla del ancho de la imagen: menos que un
/// divisor, que una regla suele vivir dentro de una columna.
const RULE_SPAN: f32 = 0.3;
/// Más reglas que esto son las filas de una lista, no la interfaz.
const MAX_RULES: usize = 12;

/// Los divisores verticales de una captura de interfaz (la barra lateral, el
/// centro, un panel): una raya fina o un cambio de color de fondo que sigue casi
/// sin cortes a lo alto de la imagen. Devuelve su x, en píxeles; vacío si no hay
/// (o si hay tantos que son una cuadrícula, no columnas).
///
/// El texto solo no los delata: «Search» de la barra lateral y «Macbook» del
/// panel derecho quedan a la misma altura y el OCR los junta en una línea.
fn find_dividers(width: u32, height: u32, bgra: &[u8]) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    if w < 240 || h < 120 || bgra.len() < w * h * 4 {
        return Vec::new();
    }
    let samples = h.min(DIVIDER_ROWS);
    let luma = |x: usize, s: usize| {
        let y = (s * h / samples + h / (2 * samples)).min(h - 1);
        let i = (y * w + x) * 4;
        0.114 * f32::from(bgra[i]) + 0.587 * f32::from(bgra[i + 1]) + 0.299 * f32::from(bgra[i + 2])
    };
    divider_runs(w, samples, luma, DIVIDER_SPAN, MAX_DIVIDERS).into_iter().map(|(x, _, _)| x).collect()
}

/// Los bordes que recorren la imagen a lo largo de un eje: `luma(a, s)` es el
/// tono en la posición `a` del eje y la muestra `s` (de `samples`) del otro.
/// Un borde debe cubrir `span` de las muestras casi sin cortes. Devuelve la
/// posición de cada uno y las muestras donde empieza y termina; vacío si hay
/// más de `max` (una cuadrícula).
fn divider_runs(len: usize, samples: usize, luma: impl Fn(usize, usize) -> f32, span: f32, max: usize) -> Vec<(f32, usize, usize)> {
    let margin = ((len as f32 * 0.03) as usize).max(6);
    if len <= margin * 2 || samples == 0 {
        return Vec::new();
    }
    let min_run = (samples as f32 * span) as usize;
    let mut hits: Vec<(usize, usize, usize)> = Vec::new();
    for a in margin..len - margin {
        let sides: Vec<(f32, f32, f32)> = (0..samples)
            .map(|s| {
                let at = |da: isize| luma((a as isize + da) as usize, s);
                let left = (at(-4) + at(-3) + at(-2)) / 3.0;
                let right = (at(2) + at(3) + at(4)) / 3.0;
                (left, at(0), right)
            })
            .collect();
        let edge: Vec<bool> = sides
            .iter()
            .map(|&(left, center, right)| {
                let step = (right - left).abs();
                let line = (center - (left + right) / 2.0).abs();
                step >= DIVIDER_STEP || (line >= DIVIDER_LINE && step < line)
            })
            .collect();
        let Some((first, last)) = long_run(&edge, min_run) else {
            continue;
        };
        // Un borde de verdad es parejo a lo largo: el mismo tono a cada lado
        // en casi todo el tramo. Una línea de texto también cambia el fondo,
        // pero a saltos, letra por letra.
        let run = &sides[first..=last];
        let even = |pick: fn(&(f32, f32, f32)) -> f32| {
            let mut values: Vec<f32> = run.iter().map(pick).collect();
            values.sort_by(|a, b| a.total_cmp(b));
            values[values.len() * 3 / 4] - values[values.len() / 4] <= DIVIDER_EVEN
        };
        if even(|v| v.0) && even(|v| v.1) && even(|v| v.2) {
            hits.push((a, first, last));
        }
    }
    // Una raya gruesa o un cambio de fondo dan varias posiciones seguidas: una
    // sola, al medio, con el tramo que cubren entre todas.
    let mut out: Vec<(f32, usize, usize)> = Vec::new();
    let mut at = 0;
    while at < hits.len() {
        let mut end = at;
        let (mut first, mut last) = (hits[at].1, hits[at].2);
        while end + 1 < hits.len() && hits[end + 1].0 - hits[end].0 <= 6 {
            end += 1;
            first = first.min(hits[end].1);
            last = last.max(hits[end].2);
        }
        let pos = (hits[at].0 + hits[end].0) as f32 / 2.0 + 0.5;
        // Una franja de menos del 6 % del largo no es una columna ni una fila.
        if out.last().is_none_or(|(prev, _, _)| pos - prev >= len as f32 * 0.06) {
            out.push((pos, first, last));
        }
        at = end + 1;
    }
    if out.len() > max {
        out.clear();
    }
    out
}

/// La tira larga de `true` (con huecos de a lo más dos) y casi sin cortes, si
/// la hay: dónde empieza y termina. Un borde de verdad cubre su tramo; las
/// letras apiladas del mismo x, no.
fn long_run(edge: &[bool], min_len: usize) -> Option<(usize, usize)> {
    let long = |first: usize, last: usize, count: usize| last - first + 1 >= min_len && count * 100 >= (last - first + 1) * 85;
    let mut run: Option<(usize, usize, usize)> = None;
    for (i, _) in edge.iter().enumerate().filter(|(_, e)| **e) {
        match run.as_mut() {
            Some((_, last, count)) if i - *last <= 3 => {
                *last = i;
                *count += 1;
            }
            _ => {
                if let Some((first, last, _)) = run.filter(|&(first, last, count)| long(first, last, count)) {
                    return Some((first, last));
                }
                run = Some((i, i, 1));
            }
        }
    }
    run.filter(|&(first, last, count)| long(first, last, count)).map(|(first, last, _)| (first, last))
}

/// Casillas vacías que se vuelven a leer, a lo más (cada una es otra pasada
/// del OCR, chica).
const MAX_CELL_READS: usize = 40;

/// Lee un recuadro de la imagen (con un margen y a `zoom` veces, píxel por
/// píxel o suavizado), con las palabras en píxeles de la imagen entera.
#[allow(clippy::too_many_arguments)]
fn read_box(width: u32, height: u32, bgra: &[u8], x: f32, y: f32, w: f32, h: f32, canvas: bool, zoom: u32, smooth: bool, enhance: bool) -> Vec<Word> {
    // Margen solo arriba y abajo: a los lados están los bordes de las celdas,
    // y una raya vertical en el recorte confunde al OCR.
    let pad = h * 0.3;
    let (x0, y0) = (x.max(0.0) as u32, (y - pad).max(0.0) as u32);
    let (x1, y1) = (((x + w) as u32).min(width), ((y + h + pad) as u32).min(height));
    if x1 <= x0 + 2 || y1 <= y0 + 2 {
        return Vec::new();
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut crop = crop_bgra(width, bgra, x0, y0, cw, ch);
    if enhance {
        enhance_contrast(&mut crop);
    }
    // Un carácter solo, Vision lo lee con bastante aire alrededor (y no con
    // menos): el recorte va al centro de un lienzo del color de su fondo.
    let margin = if canvas { ch } else { 0 };
    let (pw, ph, padded) = pad_bgra(&crop, cw, ch, margin);
    let (zw, zh, big) = if smooth { upscale_smooth(pw, ph, &padded, zoom) } else { upscale(pw, ph, &padded, zoom) };
    let Ok(read) = crate::ocr::recognize_lines(zw, zh, &big) else {
        return Vec::new();
    };
    let (ox, oy) = (x0 as f32 - margin as f32, y0 as f32 - margin as f32);
    let k = zoom as f32;
    read.into_iter()
        .flatten()
        .map(|w| Word::new(w.text, ox + w.x / k, oy + w.y / k, w.w / k, w.h / k))
        .collect()
}

/// Texto oscuro sobre fondo claro, a todo el rango de tonos: gris, con el
/// contraste estirado entre el 2 % más oscuro y el 2 % más claro, e invertido si
/// el fondo (el tono más repetido) es oscuro. El OCR de Windows lee mucho mejor
/// letras grises o chicas así que sobre un fondo negro.
fn enhance_contrast(bgra: &mut [u8]) {
    let luma = |px: &[u8]| 0.114 * f32::from(px[0]) + 0.587 * f32::from(px[1]) + 0.299 * f32::from(px[2]);
    let mut hist = [0usize; 256];
    for px in bgra.chunks_exact(4) {
        hist[luma(px) as usize] += 1;
    }
    let total: usize = hist.iter().sum();
    if total == 0 {
        return;
    }
    let percentile = |share: usize| {
        let mut seen = 0;
        hist.iter().position(|n| {
            seen += n;
            seen * 100 >= total * share
        })
        .unwrap_or(0) as f32
    };
    let (low, high) = (percentile(2), percentile(98));
    if high - low < 24.0 {
        return;
    }
    let background = hist.iter().enumerate().max_by_key(|(_, n)| **n).map_or(255, |(i, _)| i) as f32;
    let dark_background = background < (low + high) / 2.0;
    for px in bgra.chunks_exact_mut(4) {
        let t = ((luma(px) - low) / (high - low)).clamp(0.0, 1.0);
        let v = (if dark_background { 1.0 - t } else { t } * 255.0) as u8;
        px[0] = v;
        px[1] = v;
        px[2] = v;
        px[3] = 255;
    }
}

/// La imagen al centro de un lienzo `margin` más grande por lado, pintado con
/// el color de su esquina (el fondo).
fn pad_bgra(bgra: &[u8], w: u32, h: u32, margin: u32) -> (u32, u32, Vec<u8>) {
    let (pw, ph) = (w + margin * 2, h + margin * 2);
    let fill: [u8; 4] = bgra.get(0..4).and_then(|p| p.try_into().ok()).unwrap_or([255, 255, 255, 255]);
    let mut out = Vec::with_capacity(pw as usize * ph as usize * 4);
    for _ in 0..pw as usize * ph as usize {
        out.extend_from_slice(&fill);
    }
    let stride = pw as usize * 4;
    for row in 0..h as usize {
        let src = &bgra[row * w as usize * 4..(row + 1) * w as usize * 4];
        let at = (row + margin as usize) * stride + margin as usize * 4;
        out[at..at + src.len()].copy_from_slice(src);
    }
    (pw, ph, out)
}

/// Un rectángulo de una imagen BGRA.
fn crop_bgra(width: u32, bgra: &[u8], x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y..y + h {
        let start = row as usize * stride + x as usize * 4;
        out.extend_from_slice(&bgra[start..start + w as usize * 4]);
    }
    out
}

/// Las líneas de cada trozo (o de la imagen entera si no hay), cada uno con su
/// propio aumento (`ocr_zoom`) y con las palabras en píxeles de la imagen.
fn recognize_parts(
    width: u32,
    height: u32,
    bgra: &[u8],
    parts: &[Part],
    recognize: impl Fn(u32, u32, &[u8]) -> Result<Vec<Vec<crate::ocr::Word>>, String>,
) -> Result<Vec<Vec<Word>>, String> {
    let whole = [(0, 0, width, height)];
    let parts = if parts.is_empty() { &whole[..] } else { parts };
    let mut lines = Vec::new();
    let mut failed = None;
    for &(x, y, w, h) in parts {
        // Lo que se salga de la imagen se recorta.
        let (x1, y1) = ((x + w).min(width), (y + h).min(height));
        if x1 <= x || y1 <= y {
            continue;
        }
        let (w, h) = (x1 - x, y1 - y);
        let crop;
        let pixels = if (w, h) == (width, height) {
            bgra
        } else {
            crop = crop_bgra(width, bgra, x, y, w, h);
            &crop[..]
        };
        let zoom = ocr_zoom(w, h);
        let read = if zoom > 1 {
            let (zw, zh, big) = upscale(w, h, pixels, zoom);
            recognize(zw, zh, &big)
        } else {
            recognize(w, h, pixels)
        };
        // Un monitor que no se pudo leer no bota lo de los otros.
        let read = match read {
            Ok(read) => read,
            Err(error) => {
                failed = Some(error);
                continue;
            }
        };
        let (k, ox, oy) = (zoom as f32, x as f32, y as f32);
        lines.extend(
            read.iter()
                .map(|line| line.iter().map(|word| Word::new(word.text.clone(), ox + word.x / k, oy + word.y / k, word.w / k, word.h / k)).collect::<Vec<_>>()),
        );
    }
    match failed {
        Some(error) if lines.is_empty() => Err(error),
        _ => Ok(lines),
    }
}

/// El OCR de Windows lee mal las letras chicas: una pantalla normal va al
/// doble (Vision no lo necesita).
fn ocr_zoom(width: u32, height: u32) -> u32 {
    // Texto chico se lee mucho mejor al doble: Vision pierde las letras de 10–12 px
    // de una captura de ventana o de zona a 1×. Una captura Retina ya trae ese
    // detalle. En Windows el umbral es más alto porque las pantallas suelen ser 1×.
    let max = width.max(height);
    let cap = if cfg!(target_os = "windows") { 2560 } else { 1600 };
    if max <= cap {
        2
    } else {
        1
    }
}

/// La imagen agrandada `zoom` veces, interpolando entre píxeles vecinos: las
/// letras chicas quedan con bordes suaves, que el OCR de Windows lee mejor
/// que los escalones de `upscale`.
fn upscale_smooth(width: u32, height: u32, bgra: &[u8], zoom: u32) -> (u32, u32, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (zw, zh) = (w * zoom as usize, h * zoom as usize);
    let mut out = Vec::with_capacity(zw * zh * 4);
    let k = zoom as f32;
    for y in 0..zh {
        let sy = ((y as f32 + 0.5) / k - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, fy) = (sy as usize, sy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..zw {
            let sx = ((x as f32 + 0.5) / k - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, fx) = (sx as usize, sx.fract());
            let x1 = (x0 + 1).min(w - 1);
            let at = |px: usize, py: usize, c: usize| f32::from(bgra[(py * w + px) * 4 + c]);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
                let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
                out.push((top * (1.0 - fy) + bottom * fy).round() as u8);
            }
        }
    }
    (zw as u32, zh as u32, out)
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
/// La raya que marca el divisor entre dos columnas de la interfaz.
const DIVIDER_ALPHA: f32 = 0.3;
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
    start_parts(frame, Vec::new(), cx)
}

/// Como `start`, pero el OCR lee cada uno de `parts` aparte (`read_parts`).
pub fn start_parts<V: TextHost>(frame: &Frame, parts: Vec<Part>, cx: &mut Context<V>) -> TextLayer {
    let layer = TextLayer::new();
    let id = layer.id;
    let (w, h, bgra) = (frame.width(), frame.height(), frame.bgra.clone());
    cx.spawn(async move |this, cx| {
        let started = Instant::now();
        let read = cx.background_spawn(async move { read_parts(w, h, &bgra, &parts) }).await;
        match &read {
            Ok(doc) => println!("texto: {} palabras en {} ms", doc.words.len(), started.elapsed().as_millis()),
            Err(error) => eprintln!("texto: no se pudo leer ({w}×{h}): {error}"),
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
    // Una tabla va también como HTML (en macOS): una planilla la toma de las
    // tabulaciones y un documento o un correo, de la tabla.
    #[cfg(target_os = "macos")]
    let written = layer.table_to_copy(all).is_some_and(|html| crate::clip_image::write_text_and_html(&text, &html).is_ok());
    #[cfg(not(target_os = "macos"))]
    let written = false;
    if !written {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
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

    /// Lo elegido es (parte de) una tabla: su HTML, para pegarla como tabla.
    fn table_to_copy(&self, all: bool) -> Option<String> {
        let selector = self.selector()?;
        if all {
            return None;
        }
        let (lo, hi) = selector.range()?;
        selector.doc.table_html(lo, hi)
    }

    /// Lo elegido es una tabla (el botón lo dice).
    pub fn selection_is_table(&self) -> bool {
        self.table_to_copy(false).is_some()
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
                // Los divisores entre columnas: una raya tenue de punta a punta,
                // para ver dónde termina una y empieza la otra.
                let line_t = anim::ease_smooth_out(anim::segment(lit, 0.0, FADE.as_secs_f32()));
                for &x in doc.dividers() {
                    let rect = map.to_window((x, 0.0, x, map.size.1));
                    let alpha = DIVIDER_ALPHA * line_t;
                    quads.push((bounds(rect.0 - 0.75, rect.1, rect.0 + 0.75, rect.3), 0.0, gpui::white().opacity(alpha).into()));
                }
                // Las reglas horizontales, solo en el tramo que cubren.
                for &(y, x0, x1) in doc.rules() {
                    let rect = map.to_window((x0, y, x1, y));
                    let alpha = DIVIDER_ALPHA * line_t;
                    quads.push((bounds(rect.0, rect.1 - 0.75, rect.2, rect.1 + 0.75), 0.0, gpui::white().opacity(alpha).into()));
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
            let (icon, label) = if copied {
                ("icons/check.svg", crate::i18n::t("pill.peek.copied"))
            } else if self.selection_is_table() {
                // Una tabla se copia como tabla: el botón lo dice.
                ("icons/layout-grid.svg", crate::i18n::t("pill.textSelect.copyTable"))
            } else {
                ("icons/copy.svg", crate::i18n::t("page.common.copy"))
            };
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

    /// Cada monitor se lee aparte, con su propio aumento, y las palabras
    /// vuelven a su lugar en la foto entera.
    #[test]
    fn cada_trozo_se_lee_aparte_y_vuelve_a_su_lugar() {
        let (width, height) = (3600, 600);
        let bgra = vec![255u8; width as usize * height as usize * 4];
        let seen = std::cell::RefCell::new(Vec::new());
        let fake = |w: u32, h: u32, _: &[u8]| {
            seen.borrow_mut().push((w, h));
            if w == 14 {
                return Err("no".to_string());
            }
            Ok(vec![vec![crate::ocr::Word { text: format!("{w}x{h}"), x: 20.0, y: 40.0, w: 60.0, h: 16.0 }]])
        };
        // Uno chico (al doble), uno grande (tal cual), uno fuera de la foto
        // y uno que falla.
        let parts = [(0, 0, 1000, 500), (1000, 100, 2600, 500), (4000, 0, 100, 100), (0, 590, 7, 10)];
        let lines = recognize_parts(width, height, &bgra, &parts, fake).expect("líneas");
        assert_eq!(*seen.borrow(), vec![(2000, 1000), (2600, 500), (14, 20)]);
        let words: Vec<(String, f32, f32, f32, f32)> = lines.into_iter().flatten().map(|w| (w.text, w.x, w.y, w.w, w.h)).collect();
        assert_eq!(
            words,
            vec![("2000x1000".to_string(), 10.0, 20.0, 30.0, 8.0), ("2600x500".to_string(), 1020.0, 140.0, 60.0, 16.0)]
        );
        // Sin trozos, la imagen entera.
        seen.borrow_mut().clear();
        recognize_parts(width, height, &bgra, &[], fake).expect("líneas");
        assert_eq!(*seen.borrow(), vec![(3600, 600)]);
    }

    /// Lee un PNG de verdad con el OCR del sistema e imprime cómo quedó:
    /// `PILL_OCR_PNG=/ruta.png cargo test ... lee_una_captura -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lee_una_captura() {
        let path = std::env::var("PILL_OCR_PNG").expect("PILL_OCR_PNG");
        let image = image::open(&path).expect("png").to_rgba8();
        let (w, h) = image.dimensions();
        let mut bgra = image.into_raw();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        if let Ok(crop) = std::env::var("PILL_OCR_CROP") {
            let v: Vec<u32> = crop.split(',').map(|n| n.parse().unwrap()).collect();
            let img = image::open(&path).unwrap().to_rgba8();
            let sub = image::imageops::crop_imm(&img, v[0], v[1], v[2], v[3]).to_image();
            let zoom = v.get(4).copied().unwrap_or(1);
            let sub = image::imageops::resize(&sub, v[2] * zoom, v[3] * zoom, image::imageops::FilterType::CatmullRom);
            let (cw, ch) = sub.dimensions();
            let mut raw = sub.into_raw();
            for px in raw.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            for line in crate::ocr::recognize_lines(cw, ch, &raw).expect("ocr") {
                let t: Vec<String> = line.iter().map(|w| format!("{}@{:.0}", w.text, w.x / zoom as f32 + v[0] as f32)).collect();
                println!("crop: {}", t.join(" | "));
            }
            return;
        }
        if std::env::var_os("PILL_OCR_RAW").is_some() {
            for line in crate::ocr::recognize_lines(w, h, &bgra).expect("ocr") {
                let t: Vec<String> = line.iter().map(|w| format!("{}@{:.0},{:.0}", w.text, w.x, w.y)).collect();
                println!("raw: {}", t.join(" | "));
            }
        }
        let doc = read_parts(w, h, &bgra, &[]).expect("ocr");
        println!("divisores: {:?} reglas: {:?} (imagen {w}×{h})", doc.dividers(), doc.rules());
        let mut block = usize::MAX;
        for (row, range) in doc.rows.iter().enumerate() {
            if doc.block_of[row] != block {
                block = doc.block_of[row];
                println!("--- bloque {block}");
            }
            let text: Vec<&str> = doc.words[range.clone()].iter().map(|w| w.text.as_str()).collect();
            println!("  {}", text.join(" "));
        }
        println!("=== copiar todo ===\n{}", Selector::new(doc).all_text());
    }

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

    /// Una columna de prosa: líneas de ~200 px con 14 px de interlineado.
    fn column(x: f32, y: f32, lines: &[&str]) -> Vec<Vec<Word>> {
        lines.iter().enumerate().map(|(i, text)| line(x, y + i as f32 * 14.0, text)).collect()
    }

    #[test]
    fn arrastrar_en_la_columna_del_medio_no_se_sale_de_ella() {
        // Un título a todo el ancho y tres columnas angostas debajo (sin un
        // hueco que cruce todo: el título lo tapa). Antes, al bajar de línea,
        // la selección se llevaba la columna de la izquierda.
        let mut lines = vec![line(0.0, 0.0, "Titulo de la pagina que cruza las tres columnas de punta a punta")];
        lines.extend(column(0.0, 30.0, &["izquierda uno lleno de texto", "izquierda dos lleno de texto", "izquierda tres"]));
        lines.extend(column(300.0, 30.0, &["medio uno con mucho texto aqui", "medio dos con mucho texto aqui", "medio tres final"]));
        lines.extend(column(600.0, 30.0, &["derecha uno lleno de texto", "derecha dos lleno de texto", "derecha tres"]));
        let mut s = Selector::new(Doc::new(lines));
        // Desde «medio» (arriba a la izquierda de la columna) hasta «final».
        s.press((302.0, 35.0), 1, false, 4.0);
        assert!(s.drag((300.0 + 15.0 * 8.0, 63.0), 4.0));
        let text = s.selected_text().unwrap();
        assert!(text.starts_with("medio uno"), "{text}");
        assert!(text.ends_with("final"), "{text}");
        assert!(!text.contains("izquierda") && !text.contains("derecha"), "{text}");
    }

    #[test]
    fn soltar_a_la_derecha_del_parrafo_no_se_lleva_la_otra_columna() {
        // Dos columnas con un hueco: soltar el arrastre en el hueco, a la
        // altura de la última línea, termina en esa línea de la propia columna.
        let mut lines = column(0.0, 0.0, &["izquierda uno con texto", "izquierda dos con texto", "izquierda fin"]);
        lines.extend(column(400.0, 0.0, &["derecha uno con texto", "derecha dos con texto", "derecha tres"]));
        let mut s = Selector::new(Doc::new(lines));
        s.press((2.0, 5.0), 1, false, 4.0);
        // En el hueco (x 300), más cerca de la columna derecha que del final de
        // «izquierda fin» (x ~104).
        assert!(s.drag((300.0, 33.0), 4.0));
        let text = s.selected_text().unwrap();
        assert!(text.ends_with("izquierda fin"), "{text}");
        assert!(!text.contains("derecha"), "{text}");
        // Sobre la otra columna sí entra en ella (en orden de lectura).
        assert!(s.drag((410.0, 5.0), 4.0));
        assert!(s.selected_text().unwrap().contains("derecha"));
    }

    /// Una captura sintética en BGRA: tres columnas de fondo distinto (la
    /// segunda y la tercera separadas por una raya fina).
    fn three_panes(w: usize, h: usize) -> Vec<u8> {
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = if x < 200 { 24 } else if x < 600 { 34 } else { 40 };
                let v = if (599..601).contains(&x) { 90 } else { v };
                let i = (y * w + x) * 4;
                bgra[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        bgra
    }

    #[test]
    fn encuentra_los_divisores_de_una_interfaz() {
        let found = find_dividers(800, 400, &three_panes(800, 400));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!((found[0] - 200.0).abs() < 4.0, "{found:?}");
        assert!((found[1] - 600.0).abs() < 4.0, "{found:?}");
    }

    #[test]
    fn el_texto_o_un_fondo_liso_no_son_divisores() {
        // Fondo liso.
        assert!(find_dividers(800, 400, &vec![30u8; 800 * 400 * 4]).is_empty());
        // Letras: trazos cortos y sueltos, ninguno cubre la altura.
        let (w, h) = (800usize, 400usize);
        let mut bgra = vec![255u8; w * h * 4];
        for line in 0..20 {
            for stem in 0..40 {
                for y in line * 20 + 4..line * 20 + 14 {
                    let i = (y * w + 20 + stem * 7) * 4;
                    bgra[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        assert!(find_dividers(w as u32, h as u32, &bgra).is_empty());
    }

    /// La interfaz del ejemplo: una barra lateral, el centro y un panel, con
    /// textos a la misma altura y cerca unos de otros.
    fn interface() -> Vec<Vec<Word>> {
        vec![
            line(20.0, 30.0, "Search"),
            line(20.0, 80.0, "atic"),
            line(20.0, 100.0, "Marque en rojo el espacio"),
            line(230.0, 30.0, "atic New thread"),
            line(230.0, 100.0, "What should we build in atic"),
            line(230.0, 124.0, "or start without a project"),
            line(620.0, 80.0, "Macbook Air de Chris"),
            line(620.0, 100.0, "Open in Cursor"),
            line(620.0, 124.0, "dev"),
        ]
    }

    #[test]
    fn las_columnas_de_la_interfaz_se_leen_una_tras_otra() {
        let doc = Doc::with_dividers(interface(), &[200.0, 600.0]);
        let text = doc.text(0, doc.words.len() - 1);
        let at = |t: &str| text.find(t).unwrap_or_else(|| panic!("falta {t}: {text}"));
        assert!(at("Marque") < at("atic New"), "{text}");
        assert!(at("or start") < at("Macbook"), "{text}");
        // Cada línea queda entera en su columna: «dev» no se pega a «project».
        assert!(!text.contains("project dev") && !text.contains("project\tdev"), "{text}");
        assert!(!text.contains("Search\tMacbook"), "{text}");
    }

    #[test]
    fn el_arrastre_no_pasa_a_otra_columna_hasta_que_el_cursor_la_pisa() {
        let doc = Doc::with_dividers(interface(), &[200.0, 600.0]);
        let mut s = Selector::new(doc);
        // Desde «What» del centro, hacia abajo y soltando lejos, hacia la derecha
        // pero todavía dentro del centro (x 580 < divisor 600).
        s.press((232.0, 103.0), 1, false, 4.0);
        assert!(s.drag((580.0, 130.0), 4.0));
        let text = s.selected_text().unwrap();
        assert!(text.starts_with("What") && text.ends_with("project"), "{text}");
        assert!(!text.contains("Macbook") && !text.contains("dev") && !text.contains("Search"), "{text}");
        // Al pasar sobre el panel derecho sí entra en él.
        assert!(s.drag((640.0, 104.0), 4.0));
        assert!(s.selected_text().unwrap().contains("Open"));
    }

    #[test]
    fn un_divisor_con_palabras_encima_no_cuenta() {
        // Un título cruza el supuesto divisor: no son columnas.
        let mut lines = interface();
        lines.push(line(150.0, 10.0, "Un titulo ancho que cruza"));
        lines.push(line(160.0, 50.0, "otra frase que lo cruza"));
        let doc = Doc::with_dividers(lines, &[200.0]);
        assert!(doc.dividers().is_empty());
    }

    /// Una captura sintética: una raya fina a lo ancho (del 10 % al 90 %) y,
    /// más abajo, un cambio de fondo de punta a punta.
    fn two_bands(w: usize, h: usize) -> Vec<u8> {
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = if y < 300 { 24 } else { 40 };
                let v = if (199..201).contains(&y) && (w / 10..w * 9 / 10).contains(&x) { 90 } else { v };
                let i = (y * w + x) * 4;
                bgra[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        bgra
    }

    #[test]
    fn encuentra_las_reglas_de_una_interfaz() {
        let found = find_rules(800, 400, &two_bands(800, 400));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!((found[0].0 - 200.0).abs() < 4.0, "{found:?}");
        assert!(found[0].1 <= 90.0 && found[0].2 >= 710.0 && found[0].2 <= 740.0, "{found:?}");
        assert!((found[1].0 - 300.0).abs() < 4.0, "{found:?}");
        assert!(found[1].1 <= 10.0 && found[1].2 >= 790.0, "{found:?}");
        // Las columnas no se confunden con las reglas.
        assert!(find_dividers(800, 400, &two_bands(800, 400)).is_empty());
        assert!(find_rules(800, 400, &three_panes(800, 400)).is_empty());
    }

    #[test]
    fn una_regla_separa_lo_de_arriba_de_lo_de_abajo() {
        let lines = || vec![line(20.0, 30.0, "Primera linea del texto"), line(20.0, 46.0, "Segunda linea del texto")];
        let doc = Doc::new(lines());
        assert_eq!(doc.block_of[0], doc.block_of[1]);
        let doc = Doc::with_guides(lines(), &[], &[(43.0, 0.0, 400.0)]);
        assert_eq!(doc.rules().len(), 1);
        assert_ne!(doc.block_of[0], doc.block_of[1]);
        // Una regla que no llega hasta el texto no cuenta.
        let doc = Doc::with_guides(lines(), &[], &[(43.0, 300.0, 400.0)]);
        assert!(doc.rules().is_empty());
        assert_eq!(doc.block_of[0], doc.block_of[1]);
    }

    #[test]
    fn una_regla_con_palabras_encima_no_cuenta() {
        let lines = vec![line(20.0, 30.0, "Primera linea del texto"), line(20.0, 60.0, "Segunda linea del texto")];
        let doc = Doc::with_guides(lines, &[], &[(35.0, 0.0, 400.0)]);
        assert!(doc.rules().is_empty());
    }

    #[test]
    fn la_raya_bajo_el_encabezado_de_una_tabla_no_es_regla() {
        let mut lines = Vec::new();
        for (y, row) in [(0.0, ["Region", "Ventas", "Total"]), (30.0, ["Norte", "1240", "18"]), (60.0, ["Sur", "860", "4"])] {
            for (c, text) in row.iter().enumerate() {
                lines.push(line(c as f32 * 120.0, y, text));
            }
        }
        let doc = Doc::with_guides(lines, &[], &[(22.0, 0.0, 400.0)]);
        assert!(doc.rules().is_empty());
        assert!(doc.tables.iter().any(Option::is_some));
    }

    #[test]
    fn los_huecos_de_una_fila_se_detectan() {
        // «De [~2M] a [~97M] de descargas»: el OCR se saltó dos palabras y
        // devolvió tres líneas sueltas.
        let lines = vec![line(0.0, 0.0, "De"), line(48.0, 0.0, "a"), line(96.0, 0.0, "de descargas")];
        let doc = Doc::new(lines);
        let holes = doc.holes();
        assert_eq!(holes.len(), 2, "{holes:?}");
        let first = holes.iter().find(|h| h.anchor.text == "De").expect("hueco tras De");
        assert_eq!((first.x, first.w), (16.0, 32.0));
        assert_eq!(first.partner.as_ref().map(|w| w.text.as_str()), Some("a"));
        // Un espacio normal entre palabras no es un hueco.
        assert!(Doc::new(vec![line(0.0, 0.0, "De una sola linea")]).holes().is_empty());
    }

    #[test]
    fn una_fila_que_empieza_mas_adentro_que_su_bloque_tiene_hueco_al_principio() {
        let lines = vec![line(0.0, 0.0, "Primera linea larga de texto"), line(30.0, 14.0, "segunda linea de texto")];
        let doc = Doc::new(lines);
        assert_eq!(doc.block_of[0], doc.block_of[1]);
        let holes = doc.holes();
        let margin = holes.iter().find(|h| h.partner.is_none()).expect("hueco al principio");
        assert_eq!((margin.x, margin.w), (0.0, 30.0));
        assert_eq!(margin.anchor.text, "segunda");
    }

    #[test]
    fn la_tinta_de_un_hueco_se_distingue_del_fondo_y_de_un_relleno() {
        let (w, h) = (100usize, 40usize);
        let hole = Hole { x: 10.0, y: 10.0, w: 40.0, h: 20.0, anchor: Word::new("a", 0.0, 10.0, 8.0, 20.0), partner: None };
        let mut bgra = vec![255u8; w * h * 4];
        assert!(!has_ink(w as u32, h as u32, &bgra, &hole));
        // Unos trazos (letras) en el hueco.
        for stem in 0..6 {
            for y in 14..26 {
                for dx in 0..3 {
                    let i = (y * w + 14 + stem * 5 + dx) * 4;
                    bgra[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        assert!(has_ink(w as u32, h as u32, &bgra, &hole));
        // Un relleno parejo (un botón) no es tinta.
        let mut filled = vec![255u8; w * h * 4];
        for y in 10..30 {
            for x in 10..50 {
                let i = (y * w + x) * 4;
                filled[i..i + 4].copy_from_slice(&[40, 40, 40, 255]);
            }
        }
        assert!(!has_ink(w as u32, h as u32, &filled, &hole));
    }

    #[test]
    fn el_contraste_deja_texto_oscuro_sobre_fondo_claro() {
        let px = |v: u8| [v, v, v, 255];
        // Letras grises (90) sobre fondo negro (20): quedan oscuras sobre claro.
        let mut dark: Vec<u8> = Vec::new();
        for i in 0..100 {
            dark.extend(if i % 10 == 0 { px(90) } else { px(20) });
        }
        enhance_contrast(&mut dark);
        assert!(dark[0] < 40 && dark[4] > 215, "{} {}", dark[0], dark[4]);
        // Texto oscuro sobre fondo claro: se conserva el sentido.
        let mut light: Vec<u8> = Vec::new();
        for i in 0..100 {
            light.extend(if i % 10 == 0 { px(100) } else { px(230) });
        }
        enhance_contrast(&mut light);
        assert!(light[0] < 40 && light[4] > 215, "{} {}", light[0], light[4]);
        // Sin contraste (un fondo liso con ruido) no se toca.
        let mut flat: Vec<u8> = (0..100).flat_map(|i| px(100 + (i % 3) as u8)).collect();
        let before = flat.clone();
        enhance_contrast(&mut flat);
        assert_eq!(flat, before);
    }

    #[test]
    fn lo_leido_en_un_hueco_junta_las_dos_lineas_que_partio() {
        let mut lines = vec![line(0.0, 0.0, "De"), line(48.0, 0.0, "a"), line(0.0, 30.0, "otra")];
        let hole = Hole { x: 16.0, y: 0.0, w: 32.0, h: 10.0, anchor: lines[0][0].clone(), partner: Some(lines[1][0].clone()) };
        attach(&mut lines, &hole, vec![Word::new("~2M", 22.0, 0.0, 20.0, 10.0)]);
        let texts: Vec<Vec<&str>> = lines.iter().map(|l| l.iter().map(|w| w.text.as_str()).collect()).collect();
        assert_eq!(texts, vec![vec!["De", "~2M", "a"], vec![], vec!["otra"]]);
        let doc = Doc::new(lines);
        assert_eq!(doc.rows.len(), 2);
        assert_eq!(doc.text(0, 2), "De ~2M a");
    }

    #[test]
    fn una_tabla_se_copia_con_tabulaciones_y_como_html() {
        // Cuatro columnas cortas con las filas alineadas; a la última fila le
        // falta una celda (el OCR se saltó un «3»): la columna se mantiene.
        let cell = |x: f32, y: f32, t: &str| line(x, y, t);
        let mut lines = Vec::new();
        for (y, row) in [(0.0, ["Region", "Ventas", "Total", "Tiendas"]), (30.0, ["Norte", "1240", "18", "7"]), (60.0, ["Sur", "860", "4", ""])] {
            for (c, text) in row.iter().enumerate() {
                if !text.is_empty() {
                    lines.push(cell(c as f32 * 120.0, y, text));
                }
            }
        }
        let doc = Doc::new(lines);
        let all = doc.words.len() - 1;
        assert_eq!(doc.text(0, all), "Region\tVentas\tTotal\tTiendas\nNorte\t1240\t18\t7\nSur\t860\t4");
        assert_eq!(doc.empty_cells().len(), 1, "la casilla del 3 se vuelve a leer");
        let html = doc.table_html(0, all).unwrap();
        assert!(html.starts_with("<table><tr><td>Region</td>"), "{html}");
        assert!(html.ends_with("<tr><td>Sur</td><td>860</td><td>4</td><td></td></tr></table>"), "{html}");
    }

    #[test]
    fn los_parrafos_se_unen_y_las_listas_no() {
        // Prosa: las líneas llenas siguen abajo; un hueco mayor es otro párrafo.
        let prose = [
            "uno dos tres cuatro cinco seis siete ocho nueve diez once",
            "doce trece catorce quince dieciseis diecisiete dieciocho",
            "fin.",
        ];
        let mut lines = column(0.0, 0.0, &prose);
        lines.push(line(0.0, 3.0 * 14.0 + 12.0, "Otro parrafo empieza aqui y sigue bastante largo"));
        let doc = Doc::new(lines);
        assert_eq!(
            doc.text(0, doc.words.len() - 1),
            "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince dieciseis diecisiete dieciocho fin.\n\nOtro parrafo empieza aqui y sigue bastante largo"
        );
        // Una lista corta: cada ítem en su línea.
        let list = Doc::new(column(0.0, 0.0, &["• Pan", "• Leche", "• Parafina"]));
        assert_eq!(list.text(0, list.words.len() - 1), "• Pan\n• Leche\n• Parafina");
    }

    #[test]
    fn triple_clic_elige_el_parrafo() {
        let prose = [
            "uno dos tres cuatro cinco seis siete ocho nueve diez once",
            "doce trece catorce quince dieciseis diecisiete dieciocho",
            "fin.",
        ];
        let mut s = Selector::new(Doc::new(column(0.0, 0.0, &prose)));
        s.press((20.0, 18.0), 3, false, 4.0);
        assert_eq!(s.range(), Some((0, s.doc.words.len() - 1)));
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
