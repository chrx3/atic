//! Captura desplazable: une los cuadros de una región que se va desplazando
//! en una sola imagen larga (como «Scrolling Capture» de CleanShot X).
//!
//! Puro y sin UI: la mira (`capture.rs`) toma los cuadros en segundo plano y
//! se los pasa a [`Stitcher::push`]. Cada cuadro se resume en celdas: la
//! luminancia sumada de cada fila en `STRIPS` franjas verticales. Entre un
//! cuadro y el anterior se busca el desplazamiento vertical por mayoría de
//! bloques (franja × grupo de filas), no fila entera contra fila entera: así
//! lo que no se desplaza con la página no impide calzar.
//!
//! - Lo fijo (encabezados, columnas laterales) queda igual sin desplazar y no
//!   vota.
//! - Lo que cambia solo (un video, un GIF, el cursor de texto, el resalte bajo
//!   el mouse) no calza con ningún desplazamiento: se cuenta en contra, pero
//!   basta con que calce una parte de lo que se movió.
//! - Se busca en todo el alto, hacia abajo y hacia arriba: volver atrás no
//!   pierde el hilo, y de cada cuadro se agrega solo lo que falta.
//! - La imagen crece por los dos extremos: lo que aparece más abajo de lo
//!   unido va al final y lo que aparece más arriba de donde se empezó, al
//!   principio. El encabezado y el pie fijos quedan una vez cada uno.

/// Franjas verticales por fila.
const STRIPS: usize = 16;

/// Diferencia media de luminancia por píxel con que dos celdas son la misma
/// (el ruido del suavizado).
const CELL_TOLERANCE: u32 = 1;

/// Una celda tiene dibujo si cambia respecto de la fila vecina en más de
/// esto por píxel. Solo esas cuentan: una zona lisa calza con cualquier
/// desplazamiento.
const CELL_TEXTURE: u32 = 3;

/// Filas de prueba con que se proponen desplazamientos, repartidas en el alto.
const PROBES: usize = 32;

/// Cuántos desplazamientos propuestos se revisan bloque por bloque.
const CANDIDATES: usize = 5;

/// Celdas con dibujo que necesita un bloque para opinar.
const BLOCK_MIN_CELLS: usize = 3;

/// Bloques que tienen que calzar para creerle a un desplazamiento, y las
/// celdas con dibujo que suman entre todos.
const MIN_BLOCKS: usize = 3;
const MIN_EVIDENCE: usize = 24;

/// De los bloques que cambiaron, la parte que tiene que calzar con el
/// desplazamiento. El resto puede ser un video. Con otra parte de una página
/// de estructura repetida (los íconos y nombres de cada publicación) calzan
/// sueltos hasta un cuarto: aceptar eso pegaría mal sin avisar.
const MIN_AGREE: f32 = 0.35;

/// Sin desplazamiento que calce: si cambió en casi todo el alto con dibujo,
/// la página se fue (un salto). Si solo en una parte, algo se mueve solo.
const GONE: f32 = 0.8;

/// Con la página ida, la parte de lo que cambió que tiene que estar quieta
/// respecto del cuadro recién pasado para creerlo: si todo sigue cambiando
/// de un cuadro al otro sin desplazarse, es un video que llena la zona.
const SETTLED: f32 = 0.3;

/// La huella de una fila.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    sums: [u32; STRIPS],
    /// Un bit por franja: la celda tiene dibujo.
    textured: u16,
}

/// Las huellas de las filas de un cuadro BGRA de `width × height`. Se ignoran
/// las `skip_right` columnas de la derecha: ahí aparece la barra de
/// desplazamiento, que se mueve con la página y cambia esas filas.
pub fn rows(width: u32, height: u32, bgra: &[u8], skip_right: u32) -> Vec<Row> {
    let (w, h) = (width as usize, height as usize);
    let used = w.saturating_sub(skip_right as usize).max(1).min(w);
    // Dónde empieza cada franja (la última termina en `used`).
    let mut edges = [0usize; STRIPS + 1];
    for (s, edge) in edges.iter_mut().enumerate() {
        *edge = s * used / STRIPS;
    }
    let mut out: Vec<Row> = Vec::with_capacity(h);
    for y in 0..h {
        let line = &bgra[y * w * 4..(y * w + used) * 4];
        let mut sums = [0u32; STRIPS];
        for s in 0..STRIPS {
            let mut sum = 0u32;
            for px in line[edges[s] * 4..edges[s + 1] * 4].chunks_exact(4) {
                sum += (px[0] as u32 + 2 * px[1] as u32 + px[2] as u32) / 4;
            }
            sums[s] = sum;
        }
        out.push(Row { sums, textured: 0 });
    }
    let texture = cell_width(width, skip_right) * CELL_TEXTURE;
    for y in 0..h {
        let mut bits = 0u16;
        for s in 0..STRIPS {
            let here = out[y].sums[s];
            let above = y > 0 && here.abs_diff(out[y - 1].sums[s]) > texture;
            let below = y + 1 < h && here.abs_diff(out[y + 1].sums[s]) > texture;
            if above || below {
                bits |= 1 << s;
            }
        }
        out[y].textured = bits;
    }
    out
}

/// El ancho de una franja, en píxeles.
fn cell_width(width: u32, skip_right: u32) -> u32 {
    (width.saturating_sub(skip_right).max(1) / STRIPS as u32).max(1)
}

/// La tolerancia de [`best_offset`] para cuadros de este ancho.
pub fn tolerance(width: u32, skip_right: u32) -> u32 {
    cell_width(width, skip_right) * CELL_TOLERANCE
}

/// Cómo calza un cuadro nuevo con el anterior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Offset {
    /// No se desplazó ni cambió (o apenas: el cursor de texto).
    Same,
    /// Se desplazó `dy` filas: hacia abajo si es positivo, de vuelta hacia
    /// arriba si es negativo. `header` y `footer`: las filas de arriba y de
    /// abajo que no cambiaron (encabezado y pie fijos).
    Moved { dy: isize, header: usize, footer: usize },
    /// Cambió y ningún desplazamiento lo explica: un salto demasiado grande,
    /// otra página o algo que se mueve solo ([`Match::partial`]).
    Lost,
}

/// Cómo se llegó a un [`Offset`], para el log.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Match {
    /// Celdas con dibujo y cuántas cambiaron sin desplazar.
    pub cells: usize,
    pub changed: usize,
    /// El mejor desplazamiento propuesto, calce o no, y sus bloques a favor
    /// y en contra.
    pub dy: isize,
    pub agree: usize,
    pub against: usize,
    /// Sin calzar: cambió solo una parte del alto (hay grupos de filas
    /// enteros en su lugar), como con un video; no la página entera.
    pub partial: bool,
}

impl Match {
    /// Quieto: nada cambió, o apenas (el cursor de texto).
    fn is_still(&self) -> bool {
        self.changed <= (self.cells / 100).max(4)
    }
}

/// Por fila, un bit por franja: la celda tiene dibujo (en cualquiera de los
/// dos cuadros) y cambió sin desplazar.
fn changed_cells(prev: &[Row], next: &[Row], tolerance: u32) -> Vec<u16> {
    prev.iter()
        .zip(next)
        .map(|(a, b)| {
            let textured = a.textured | b.textured;
            let mut bits = 0u16;
            for s in 0..STRIPS {
                if textured & (1 << s) != 0 && a.sums[s].abs_diff(b.sums[s]) > tolerance {
                    bits |= 1 << s;
                }
            }
            bits
        })
        .collect()
}

/// Los bloques de un desplazamiento propuesto.
#[derive(Clone, Copy, Default)]
struct Tally {
    /// Calzan con el desplazamiento y no sin desplazar.
    agree: usize,
    /// Cambiaron y no calzan.
    against: usize,
    /// Celdas con dibujo de los que calzan.
    evidence: usize,
}

/// Dos cuadros comparados.
struct Comparison {
    offset: Offset,
    detail: Match,
    /// Con [`Offset::Moved`]: por fila del cuadro nuevo, las celdas de los
    /// bloques que no calzaron (lo que cambia solo mientras se desplaza).
    against: Vec<u16>,
}

/// Las filas `i` del cuadro nuevo que tienen pareja `i + dy` en el anterior,
/// dentro de lo que se mueve (`top..bottom`).
fn overlap(top: usize, bottom: usize, dy: isize) -> std::ops::Range<usize> {
    let from = top.max((top as isize - dy).max(0) as usize);
    let to = bottom.min((bottom as isize - dy).max(0) as usize);
    from..to.max(from)
}

/// El desplazamiento que mejor calza `next` con `prev` (las huellas de dos
/// cuadros del mismo tamaño). `ignore`: por fila, las celdas que se sabe que
/// cambian solas y no opinan. `tolerance`: lo de [`tolerance`].
fn compare(prev: &[Row], next: &[Row], ignore: &[u16], tolerance: u32) -> Comparison {
    let n = prev.len();
    let mut detail = Match::default();
    let done = |offset, detail| Comparison { offset, detail, against: Vec::new() };
    if n == 0 || next.len() != n || ignore.len() != n {
        return done(Offset::Lost, detail);
    }
    let same = |a: u32, b: u32| a.abs_diff(b) <= tolerance;
    // Con dibujo en el cuadro nuevo y que opina.
    let voter: Vec<u16> = (0..n).map(|i| next[i].textured & !ignore[i]).collect();
    let mut changed = changed_cells(prev, next, tolerance);
    for i in 0..n {
        changed[i] &= !ignore[i];
        detail.cells += ((prev[i].textured | next[i].textured) & !ignore[i]).count_ones() as usize;
        detail.changed += changed[i].count_ones() as usize;
    }
    if detail.is_still() {
        return done(Offset::Same, detail);
    }
    // Lo fijo arriba y abajo, hasta un tercio cada uno.
    let cap = n / 3;
    let header = (0..cap).take_while(|&i| changed[i] == 0).count();
    let footer = (0..cap).take_while(|&i| changed[n - 1 - i] == 0).count();
    let (top, bottom) = (header, n - footer);

    // Propuestas: unas pocas filas del cuadro nuevo, las que más cambiaron,
    // contra todas las del anterior. Cada celda que calza vota por su
    // desplazamiento.
    let mut votes = vec![0u32; 2 * n + 1];
    let bands = PROBES.min((bottom - top) / 2).max(1);
    for band in 0..bands {
        let from = top + (bottom - top) * band / bands;
        let to = top + (bottom - top) * (band + 1) / bands;
        let Some(i) = (from..to).max_by_key(|&i| (changed[i] & voter[i]).count_ones()) else {
            continue;
        };
        let mask = changed[i] & voter[i];
        if mask == 0 {
            continue;
        }
        for j in top..bottom {
            if j == i {
                continue;
            }
            let mut hits = 0u32;
            for s in 0..STRIPS {
                if mask & (1 << s) != 0 && same(next[i].sums[s], prev[j].sums[s]) {
                    hits += 1;
                }
            }
            votes[j + n - i] += hits;
        }
    }
    let mut proposals: Vec<(u32, isize)> = votes
        .iter()
        .enumerate()
        .filter(|(_, &v)| v > 0)
        .map(|(at, &v)| (v, at as isize - n as isize))
        .collect();
    // Más votos primero; con los mismos, el desplazamiento más corto.
    proposals.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.abs().cmp(&b.1.abs())));
    proposals.truncate(CANDIDATES);

    let block = (n / 40).clamp(8, 32);
    let mut best: Option<(Tally, isize)> = None;
    for &(_, dy) in &proposals {
        let tally = tally_blocks(prev, next, &voter, &changed, overlap(top, bottom, dy), dy, block, tolerance, None);
        let better = match best {
            None => true,
            Some((b, _)) => tally.agree > b.agree || (tally.agree == b.agree && tally.evidence > b.evidence),
        };
        if better {
            best = Some((tally, dy));
        }
    }
    if let Some((tally, dy)) = best {
        detail.dy = dy;
        detail.agree = tally.agree;
        detail.against = tally.against;
        let enough = tally.agree >= MIN_BLOCKS && tally.evidence >= MIN_EVIDENCE;
        if enough && tally.agree as f32 >= (tally.agree + tally.against) as f32 * MIN_AGREE {
            let mut against = vec![0u16; n];
            tally_blocks(prev, next, &voter, &changed, overlap(top, bottom, dy), dy, block, tolerance, Some(&mut against));
            return Comparison { offset: Offset::Moved { dy, header, footer }, detail, against };
        }
    }

    // Nada calza. Con grupos de filas enteros en su lugar cambió solo una
    // parte (un video entre texto, o un salto bajo un encabezado fijo); si
    // no, cambió la página entera.
    let (mut alive, mut moved) = (0usize, 0usize);
    let mut from = 0;
    while from < n {
        let to = (from + block).min(n);
        let (mut band_alive, mut band_moved) = (false, false);
        for s in 0..STRIPS {
            let (mut cells, mut still) = (0usize, 0usize);
            for i in from..to {
                if voter[i] & (1 << s) != 0 {
                    cells += 1;
                    if changed[i] & (1 << s) == 0 {
                        still += 1;
                    }
                }
            }
            if cells >= BLOCK_MIN_CELLS {
                band_alive = true;
                band_moved |= still * 10 < cells * 9;
            }
        }
        alive += band_alive as usize;
        moved += band_moved as usize;
        from = to;
    }
    detail.partial = (moved as f32) < alive as f32 * GONE;
    done(Offset::Lost, detail)
}

/// Cuenta los bloques (franja × `block` filas) del cuadro nuevo que calzan
/// con el anterior desplazado `dy`, sobre las filas `rows`. En `mark` quedan
/// las celdas de los que no calzan.
#[allow(clippy::too_many_arguments)]
fn tally_blocks(
    prev: &[Row],
    next: &[Row],
    voter: &[u16],
    changed: &[u16],
    rows: std::ops::Range<usize>,
    dy: isize,
    block: usize,
    tolerance: u32,
    mut mark: Option<&mut [u16]>,
) -> Tally {
    let mut tally = Tally::default();
    let mut from = rows.start;
    while from < rows.end {
        let to = (from + block).min(rows.end);
        for s in 0..STRIPS {
            let (mut cells, mut still, mut moved) = (0usize, 0usize, 0usize);
            for i in from..to {
                if voter[i] & (1 << s) == 0 {
                    continue;
                }
                cells += 1;
                if changed[i] & (1 << s) == 0 {
                    still += 1;
                }
                let j = (i as isize + dy) as usize;
                if next[i].sums[s].abs_diff(prev[j].sums[s]) <= tolerance {
                    moved += 1;
                }
            }
            if cells < BLOCK_MIN_CELLS {
                continue;
            }
            let is_still = still * 10 >= cells * 9;
            let agrees = moved * 10 >= cells * 9;
            if agrees && !is_still {
                tally.agree += 1;
                tally.evidence += cells;
            } else if !agrees && !is_still {
                tally.against += 1;
                if let Some(mark) = mark.as_deref_mut() {
                    for row in &mut mark[from..to] {
                        *row |= 1 << s;
                    }
                }
            }
        }
        from = to;
    }
    tally
}

/// Sin calzar con `last`: si la página de verdad se fue, mirando también el
/// cuadro recién pasado. Se fue si desde ese se desplazó, o si buena parte de
/// lo que cambió está quieta desde entonces. Si todo lo que cambió sigue
/// cambiando sin desplazarse, es algo que se mueve solo.
fn settled(last: &[Row], recent: &[Row], next: &[Row], ignore: &[u16], tolerance: u32) -> bool {
    if recent.len() != next.len() || last.len() != next.len() {
        return true;
    }
    if matches!(compare(recent, next, ignore, tolerance).offset, Offset::Moved { .. }) {
        return true;
    }
    let gone = changed_cells(last, next, tolerance);
    let flicker = changed_cells(recent, next, tolerance);
    let (mut total, mut quiet) = (0usize, 0usize);
    for i in 0..next.len() {
        total += (gone[i] & !ignore[i]).count_ones() as usize;
        quiet += (gone[i] & !flicker[i] & !ignore[i]).count_ones() as usize;
    }
    quiet as f32 >= total as f32 * SETTLED
}

/// Qué pasó con un cuadro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// El primero: la imagen empieza con él.
    First,
    /// Se agregaron filas nuevas abajo.
    Added(u32),
    /// Se agregaron filas nuevas arriba: se subió más allá de donde empezó.
    AddedAbove(u32),
    /// Nada nuevo: la página está quieta (aunque algo adentro se mueva solo).
    Same,
    /// Se desplazó por algo que ya estaba unido, hacia arriba o hacia abajo.
    Seen,
    /// No calzó y se ignoró: hay que volver a donde sí.
    Lost,
    /// Ya se llegó al alto máximo.
    Full,
}

/// La imagen larga que se va armando.
pub struct Stitcher {
    width: u32,
    height: u32,
    /// El alto de los cuadros (todos iguales).
    frame_height: u32,
    bgra: Vec<u8>,
    /// Las huellas del último cuadro que calzó.
    last: Vec<Row>,
    /// Sin calzar: las del cuadro recién pasado ([`settled`]).
    recent: Option<Vec<Row>>,
    /// Por fila de `last`, las celdas que cambian solas (un video): no
    /// opinan. Se desplazan con la página.
    volatile: Vec<u16>,
    /// En qué fila de la imagen cae la primera de `last`. Al quedar
    /// negativo (se subió más arriba de lo unido) se antepone lo nuevo y
    /// vuelve a cero.
    position: isize,
    max_height: u32,
    skip_right: u32,
    full: bool,
    /// Cómo calzó el último cuadro, para el log.
    detail: Match,
}

impl Stitcher {
    pub fn new(max_height: u32, skip_right: u32) -> Self {
        Self {
            width: 0,
            height: 0,
            frame_height: 0,
            bgra: Vec::new(),
            last: Vec::new(),
            recent: None,
            volatile: Vec::new(),
            position: 0,
            max_height: max_height.max(1),
            skip_right,
            full: false,
            detail: Match::default(),
        }
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Cómo calzó el último cuadro que se comparó.
    pub fn detail(&self) -> Match {
        self.detail
    }

    /// Suma un cuadro BGRA de `width × height`.
    pub fn push(&mut self, width: u32, height: u32, bgra: &[u8]) -> Step {
        if width == 0 || height == 0 || bgra.len() < width as usize * height as usize * 4 {
            return Step::Lost;
        }
        let skip = self.skip_right.min(width / 4);
        if self.height == 0 {
            self.width = width;
            self.frame_height = height;
            let rows_kept = height.min(self.max_height);
            self.bgra = bgra[..rows_kept as usize * width as usize * 4].to_vec();
            self.height = rows_kept;
            self.full = rows_kept >= self.max_height;
            self.last = rows(width, height, bgra, skip);
            self.volatile = vec![0; height as usize];
            self.position = 0;
            return Step::First;
        }
        // Otro tamaño (cambió la pantalla): no se puede pegar.
        if width != self.width || height != self.frame_height {
            return Step::Lost;
        }
        if self.full {
            return Step::Full;
        }
        let next = rows(width, height, bgra, skip);
        let tolerance = tolerance(width, skip);
        let Comparison { offset, detail, against } = compare(&self.last, &next, &self.volatile, tolerance);
        self.detail = detail;
        match offset {
            Offset::Same => {
                // El siguiente se compara con este.
                self.last = next;
                self.recent = None;
                Step::Same
            }
            Offset::Lost => {
                let Some(recent) = self.recent.take() else {
                    // Todavía no se sabe si se fue o es algo que se mueve
                    // solo: lo dice el cuadro que sigue.
                    self.recent = Some(next);
                    return Step::Same;
                };
                if !detail.partial || settled(&self.last, &recent, &next, &self.volatile, tolerance) {
                    self.recent = Some(next);
                    return Step::Lost;
                }
                // Cambia solo, sin desplazarse y en una parte: un video. Esas
                // celdas dejan de opinar.
                let now = changed_cells(&self.last, &next, tolerance);
                let before = changed_cells(&recent, &next, tolerance);
                for (i, bits) in self.volatile.iter_mut().enumerate() {
                    *bits |= now[i] | before[i];
                }
                self.last = next;
                Step::Same
            }
            Offset::Moved { dy, header, footer } => {
                // Lo que cambia solo se va con la página, y se suma lo que
                // no calzó esta vez.
                let frame_rows = height as usize;
                let mut volatile = against;
                for (i, bits) in volatile.iter_mut().enumerate() {
                    let from = i as isize + dy;
                    if (0..frame_rows as isize).contains(&from) {
                        *bits |= self.volatile[from as usize];
                    }
                }
                self.volatile = volatile;
                self.position += dy;
                self.last = next;
                self.recent = None;
                let stride = width as usize * 4;
                if self.position < 0 {
                    // El encabezado de antes sale y entra el del cuadro, con
                    // lo nuevo debajo. Si no había encabezado fijo y `header`
                    // son filas lisas, igual calza: esas filas del cuadro son
                    // las primeras de lo unido. Pasado el alto máximo se deja
                    // de agregar arriba: lo ya unido (y el pie) no se pierde y
                    // de lo nuevo entra solo lo que cabe, junto a lo de antes.
                    let keep = (self.height as usize).min(header);
                    let from = ((keep as isize - self.position) as usize).min(frame_rows);
                    let mut bgra_above = Vec::with_capacity((from + self.height as usize - keep) * stride);
                    bgra_above.extend_from_slice(&bgra[..from * stride]);
                    bgra_above.extend_from_slice(&self.bgra[keep * stride..]);
                    self.bgra = bgra_above;
                    let mut total = from + self.height as usize - keep;
                    if total >= self.max_height as usize {
                        let excess = total - self.max_height as usize;
                        self.bgra.drain(keep * stride..(keep + excess) * stride);
                        total = self.max_height as usize;
                        self.full = true;
                    }
                    let added = (total as u32).saturating_sub(self.height);
                    self.height = total as u32;
                    self.position = 0;
                    return Step::AddedAbove(added);
                }
                let bottom = self.position + frame_rows as isize;
                if bottom <= self.height as isize {
                    return Step::Seen;
                }
                // El pie de antes sale y vuelve después de lo nuevo.
                let keep = (self.height as usize).saturating_sub(footer);
                let from = (keep as isize - self.position).clamp(0, frame_rows as isize) as usize;
                self.bgra.truncate(keep * stride);
                self.bgra.extend_from_slice(&bgra[from * stride..frame_rows * stride]);
                let mut total = keep + frame_rows - from;
                if total >= self.max_height as usize {
                    total = self.max_height as usize;
                    self.bgra.truncate(total * stride);
                    self.full = true;
                }
                let added = (total as u32).saturating_sub(self.height);
                self.height = total as u32;
                Step::Added(added)
            }
        }
    }

    /// Una miniatura que cabe en `max_w × max_h`, sin deformar.
    pub fn thumbnail(&self, max_w: u32, max_h: u32) -> Option<(u32, u32, Vec<u8>)> {
        if self.height == 0 || max_w == 0 || max_h == 0 {
            return None;
        }
        let fit = (max_w as f32 / self.width as f32).min(max_h as f32 / self.height as f32).min(1.0);
        let w = ((self.width as f32 * fit).round() as u32).max(1);
        let h = ((self.height as f32 * fit).round() as u32).max(1);
        let mut out = vec![0u8; w as usize * h as usize * 4];
        for y in 0..h {
            let sy = (y as u64 * self.height as u64 / h as u64) as usize;
            for x in 0..w {
                let sx = (x as u64 * self.width as u64 / w as u64) as usize;
                let from = (sy * self.width as usize + sx) * 4;
                let to = (y as usize * w as usize + x as usize) * 4;
                out[to..to + 4].copy_from_slice(&self.bgra[from..from + 4]);
            }
        }
        Some((w, h, out))
    }

    /// La imagen entera: ancho, alto y BGRA.
    pub fn into_image(self) -> Option<(u32, u32, Vec<u8>)> {
        (self.height > 0).then_some((self.width, self.height, self.bgra))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 48;

    /// Una página alta con dibujo distinto en cada fila (pseudoaleatorio).
    fn page(height: u32) -> Vec<u8> {
        let mut seed = 0x2545_f491_u32;
        let mut out = Vec::with_capacity((W * height * 4) as usize);
        for _ in 0..W * height {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let v = (seed & 0xff) as u8;
            out.extend_from_slice(&[v, v.wrapping_mul(3), v ^ 0x5a, 255]);
        }
        out
    }

    fn window(page: &[u8], top: u32, height: u32) -> Vec<u8> {
        let stride = (W * 4) as usize;
        page[top as usize * stride..(top + height) as usize * stride].to_vec()
    }

    /// Pinta `rows` filas de un color fijo a partir de `at` (un encabezado).
    fn paint(frame: &mut [u8], at: u32, rows: u32, shade: u8) {
        let stride = (W * 4) as usize;
        for y in at..at + rows {
            for x in 0..W as usize {
                // Con dibujo: rayas, para que cuente como fila con evidencia.
                let v = if x % 6 < 3 { shade } else { 255 - shade };
                let at = y as usize * stride + x * 4;
                frame[at..at + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
    }

    #[test]
    fn ventanas_solapadas_rearman_la_pagina() {
        let tall = page(600);
        let h = 120;
        let mut stitcher = Stitcher::new(20_000, 0);
        let mut steps = Vec::new();
        for top in [0, 37, 80, 150, 151, 151, 230, 300, 333, 410, 480] {
            steps.push(stitcher.push(W, h, &window(&tall, top, h)));
        }
        assert_eq!(steps[0], Step::First);
        assert_eq!(steps[1], Step::Added(37));
        assert_eq!(steps[5], Step::Same);
        assert_eq!(stitcher.height(), 600);
        let (w, height, bgra) = stitcher.into_image().unwrap();
        assert_eq!((w, height), (W, 600));
        assert!(bgra == tall, "la imagen unida no es la página");
    }

    #[test]
    fn el_encabezado_y_el_pie_fijos_salen_una_vez() {
        let tall = page(500);
        let (h, head, foot) = (140, 18, 12);
        let mut stitcher = Stitcher::new(20_000, 0);
        let offsets = [0, 40, 95, 160, 230, 300, 360];
        for top in offsets {
            let mut frame = window(&tall, top, h);
            paint(&mut frame, 0, head, 30);
            paint(&mut frame, h - foot, foot, 90);
            let step = stitcher.push(W, h, &frame);
            assert_ne!(step, Step::Lost, "en {top}");
        }
        let last = *offsets.last().unwrap();
        // Encabezado, la página que pasó entre medio y el pie.
        let mut expected = window(&tall, 0, head);
        paint(&mut expected, 0, head, 30);
        expected.extend_from_slice(&window(&tall, head, last + h - foot - head));
        let mut footer = window(&tall, 0, foot);
        paint(&mut footer, 0, foot, 90);
        expected.extend_from_slice(&footer);
        assert_eq!(stitcher.height(), last + h);
        assert!(stitcher.into_image().unwrap().2 == expected);
    }

    #[test]
    fn un_poco_de_ruido_igual_calza() {
        let tall = page(300);
        let h = 100;
        let mut stitcher = Stitcher::new(20_000, 0);
        stitcher.push(W, h, &window(&tall, 0, h));
        let mut noisy = window(&tall, 30, h);
        // Un nivel de diferencia en algunos píxeles, como el suavizado.
        for (i, byte) in noisy.iter_mut().enumerate() {
            if i % 37 == 0 {
                *byte = byte.saturating_add(1);
            }
        }
        assert_eq!(stitcher.push(W, h, &noisy), Step::Added(30));
    }

    #[test]
    fn otra_pagina_o_un_salto_grande_no_calza_y_volver_atras_si() {
        let tall = page(400);
        let other = page(800);
        let h = 100;
        let mut stitcher = Stitcher::new(20_000, 0);
        stitcher.push(W, h, &window(&tall, 0, h));
        // Sin nada en común con lo anterior.
        // El primer cuadro sin calzar queda en duda: lo confirma el siguiente.
        assert_eq!(stitcher.push(W, h, &window(&tall, 200, h)), Step::Same);
        // Perdido si se queda ahí, si sigue bajando o si es otra página.
        assert_eq!(stitcher.push(W, h, &window(&tall, 200, h)), Step::Lost);
        assert_eq!(stitcher.push(W, h, &window(&tall, 230, h)), Step::Lost);
        assert_eq!(stitcher.push(W, h, &window(&other, 650, h)), Step::Lost);
        assert_eq!(stitcher.push(W, h, &window(&other, 650, h)), Step::Lost);
        // De vuelta hacia arriba calza, sin agregar nada.
        assert_eq!(stitcher.push(W, h, &window(&tall, 60, h)), Step::Added(60));
        assert_eq!(stitcher.push(W, h, &window(&tall, 20, h)), Step::Seen);
        // Y al bajar de nuevo solo entra lo que faltaba.
        assert_eq!(stitcher.push(W, h, &window(&tall, 50, h)), Step::Seen);
        assert_eq!(stitcher.push(W, h, &window(&tall, 90, h)), Step::Added(30));
        // Otro tamaño.
        assert_eq!(stitcher.push(W, h - 1, &window(&tall, 70, h - 1)), Step::Lost);
        assert_eq!(stitcher.height(), 190);
        assert!(stitcher.into_image().unwrap().2 == window(&tall, 0, 190));
    }

    /// Lo que debería decir un cuadro en `top` (de alto `h`) después de los
    /// de `tops`: agrega arriba o abajo lo que no se había visto.
    fn expected_step(tops: &[u32], i: usize, h: u32) -> Step {
        if i == 0 {
            return Step::First;
        }
        let (top, before) = (tops[i], &tops[..i]);
        let (first, last) = (*before.iter().min().unwrap(), before.iter().max().unwrap() + h);
        if top == tops[i - 1] {
            Step::Same
        } else if top < first {
            Step::AddedAbove(first - top)
        } else if top + h > last {
            Step::Added(top + h - last)
        } else {
            Step::Seen
        }
    }

    #[test]
    fn desde_el_medio_sube_hasta_el_principio_y_baja_hasta_el_final() {
        let tall = page(700);
        let h = 120;
        let mut stitcher = Stitcher::new(20_000, 0);
        let tops = [300, 300, 270, 230, 231, 180, 120, 60, 20, 0, 0, 40, 100, 180, 260, 300, 330, 400, 470, 520, 580];
        for (i, &top) in tops.iter().enumerate() {
            let step = stitcher.push(W, h, &window(&tall, top, h));
            assert_eq!(step, expected_step(&tops, i, h), "cuadro {i} en {top}: {:?}", stitcher.detail());
        }
        assert_eq!(stitcher.height(), 700);
        let (w, height, bgra) = stitcher.into_image().unwrap();
        assert_eq!((w, height), (W, 700));
        assert!(bgra == tall, "la imagen unida no es la página");
    }

    #[test]
    fn subir_y_bajar_varias_veces_no_repite_nada() {
        let tall = page(600);
        let h = 100;
        let mut stitcher = Stitcher::new(20_000, 0);
        // Sube un poco, baja más allá del inicio, vuelve a subir hasta el
        // principio y baja hasta el final.
        let tops = [250, 220, 280, 330, 290, 200, 140, 90, 30, 0, 70, 160, 250, 340, 420, 500];
        for (i, &top) in tops.iter().enumerate() {
            let step = stitcher.push(W, h, &window(&tall, top, h));
            assert_eq!(step, expected_step(&tops, i, h), "cuadro {i} en {top}: {:?}", stitcher.detail());
        }
        assert_eq!(stitcher.height(), 600);
        assert!(stitcher.into_image().unwrap().2 == tall);
    }

    #[test]
    fn hacia_arriba_el_encabezado_y_el_pie_fijos_salen_una_vez() {
        let tall = page(600);
        let (h, head, foot) = (140, 18, 12);
        let mut stitcher = Stitcher::new(20_000, 0);
        let tops = [240, 200, 150, 90, 40, 0, 60, 130, 200, 270, 340, 400, 460];
        for (i, &top) in tops.iter().enumerate() {
            let mut frame = window(&tall, top, h);
            paint(&mut frame, 0, head, 30);
            paint(&mut frame, h - foot, foot, 90);
            let step = stitcher.push(W, h, &frame);
            assert_eq!(step, expected_step(&tops, i, h), "cuadro {i} en {top}: {:?}", stitcher.detail());
        }
        let last = *tops.last().unwrap();
        let mut expected = window(&tall, 0, head);
        paint(&mut expected, 0, head, 30);
        expected.extend_from_slice(&window(&tall, head, last + h - foot - head));
        let mut footer = window(&tall, 0, foot);
        paint(&mut footer, 0, foot, 90);
        expected.extend_from_slice(&footer);
        assert_eq!(stitcher.height(), last + h);
        assert!(stitcher.into_image().unwrap().2 == expected);
    }

    #[test]
    fn hacia_arriba_no_pasa_del_alto_maximo() {
        let tall = page(600);
        let h = 100;
        let mut stitcher = Stitcher::new(250, 0);
        stitcher.push(W, h, &window(&tall, 300, h));
        assert_eq!(stitcher.push(W, h, &window(&tall, 220, h)), Step::AddedAbove(80));
        // Arriba entra solo lo que cabe: lo de abajo no se pierde.
        assert_eq!(stitcher.push(W, h, &window(&tall, 140, h)), Step::AddedAbove(70));
        assert!(stitcher.is_full());
        assert_eq!(stitcher.push(W, h, &window(&tall, 60, h)), Step::Full);
        let (_, height, bgra) = stitcher.into_image().unwrap();
        assert_eq!(height, 250);
        assert!(bgra == window(&tall, 150, 250));
    }

    /// Ruido nuevo en cada cuadro sobre las filas `at..at + rows` de la
    /// página (las que se vean con el cuadro en `top`), entre las columnas
    /// `x0..x1`: un video que se desplaza con la página.
    fn video(frame: &mut [u8], top: u32, at: u32, rows: u32, (x0, x1): (u32, u32), seed: &mut u32) {
        let stride = (W * 4) as usize;
        let height = (frame.len() / stride) as u32;
        for y in 0..height {
            if !(at..at + rows).contains(&(top + y)) {
                continue;
            }
            for x in x0..x1 {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 17;
                *seed ^= *seed << 5;
                let v = (*seed & 0xff) as u8;
                let at = y as usize * stride + x as usize * 4;
                frame[at..at + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
    }

    #[test]
    fn un_video_en_la_zona_no_impide_calzar() {
        let tall = page(400);
        let h = 120;
        let (at, rows, columns) = (70, 90, (6, 42));
        let mut seed = 0x9e37_79b9_u32;
        let mut stitcher = Stitcher::new(20_000, 0);
        let tops = [0, 0, 0, 0, 20, 50, 50, 90, 130, 170, 170, 200, 240, 280];
        for (i, top) in tops.into_iter().enumerate() {
            let mut frame = window(&tall, top, h);
            video(&mut frame, top, at, rows, columns, &mut seed);
            let step = stitcher.push(W, h, &frame);
            // Quieta con el video andando no es «perdido», y al desplazar calza.
            let expected = match i {
                0 => Step::First,
                _ if top == tops[i - 1] => Step::Same,
                _ => Step::Added(top - tops[i - 1]),
            };
            assert_eq!(step, expected, "cuadro {i} en {top}: {:?}", stitcher.detail());
        }
        assert_eq!(stitcher.height(), 280 + h);
        // Fuera del video, la página tal cual.
        let (_, _, bgra) = stitcher.into_image().unwrap();
        let stride = (W * 4) as usize;
        for y in 0..(280 + h) as usize {
            let inside = (at..at + rows).contains(&(y as u32));
            let end = if inside { columns.0 as usize * 4 } else { stride };
            assert!(bgra[y * stride..y * stride + end] == tall[y * stride..y * stride + end], "fila {y}");
        }
    }

    #[test]
    fn un_video_tambien_deja_subir_desde_el_medio() {
        let tall = page(500);
        let h = 120;
        let (at, rows, columns) = (250, 60, (6, 42));
        let mut seed = 0x85eb_ca6b_u32;
        let mut stitcher = Stitcher::new(20_000, 0);
        let tops = [180, 180, 180, 180, 150, 120, 120, 80, 40, 10, 0, 0, 30, 70, 110, 150, 190, 230, 280, 330, 380];
        for (i, &top) in tops.iter().enumerate() {
            let mut frame = window(&tall, top, h);
            video(&mut frame, top, at, rows, columns, &mut seed);
            let step = stitcher.push(W, h, &frame);
            assert_eq!(step, expected_step(&tops, i, h), "cuadro {i} en {top}: {:?}", stitcher.detail());
        }
        assert_eq!(stitcher.height(), 500);
        let (_, _, bgra) = stitcher.into_image().unwrap();
        let stride = (W * 4) as usize;
        for y in 0..500usize {
            let inside = (at..at + rows).contains(&(y as u32));
            let end = if inside { columns.0 as usize * 4 } else { stride };
            assert!(bgra[y * stride..y * stride + end] == tall[y * stride..y * stride + end], "fila {y}");
        }
    }

    #[test]
    fn las_columnas_fijas_no_impiden_calzar() {
        let tall = page(500);
        let side = page(700);
        let h = 140;
        let stride = (W * 4) as usize;
        let mut stitcher = Stitcher::new(20_000, 0);
        let tops = [0, 30, 75, 76, 140, 230, 300];
        for (i, top) in tops.into_iter().enumerate() {
            let mut frame = window(&tall, top, h);
            // Una columna a cada lado que no se desplaza (los menús de una web).
            for y in 0..h as usize {
                let fixed = &side[(560 + y) * stride..(561 + y) * stride];
                frame[y * stride..y * stride + 40].copy_from_slice(&fixed[..40]);
                frame[(y + 1) * stride - 48..(y + 1) * stride].copy_from_slice(&fixed[stride - 48..]);
            }
            let step = stitcher.push(W, h, &frame);
            let expected = if i == 0 { Step::First } else { Step::Added(top - tops[i - 1]) };
            assert_eq!(step, expected, "cuadro {i} en {top}: {:?}", stitcher.detail());
        }
        assert_eq!(stitcher.height(), 300 + h);
    }

    #[test]
    fn una_zona_lisa_no_inventa_desplazamientos() {
        let flat = vec![200u8; (W * 300 * 4) as usize];
        let h = 100;
        let mut stitcher = Stitcher::new(20_000, 0);
        stitcher.push(W, h, &window(&flat, 0, h));
        assert_eq!(stitcher.push(W, h, &window(&flat, 50, h)), Step::Same);
        assert_eq!(stitcher.height(), h);
    }

    #[test]
    fn no_pasa_del_alto_maximo() {
        let tall = page(600);
        let h = 100;
        let mut stitcher = Stitcher::new(250, 0);
        stitcher.push(W, h, &window(&tall, 0, h));
        stitcher.push(W, h, &window(&tall, 80, h));
        assert_eq!(stitcher.push(W, h, &window(&tall, 160, h)), Step::Added(70));
        assert!(stitcher.is_full());
        assert_eq!(stitcher.push(W, h, &window(&tall, 240, h)), Step::Full);
        let (_, height, bgra) = stitcher.into_image().unwrap();
        assert_eq!(height, 250);
        assert!(bgra == window(&tall, 0, 250));
    }

    #[test]
    fn la_columna_de_la_derecha_no_cuenta() {
        let tall = page(300);
        let h = 100;
        let mut stitcher = Stitcher::new(20_000, 6);
        stitcher.push(W, h, &window(&tall, 0, h));
        // La barra de desplazamiento: las últimas columnas cambian en todo el alto.
        let mut moved = window(&tall, 40, h);
        for y in 0..h as usize {
            for x in W as usize - 6..W as usize {
                let at = (y * W as usize + x) * 4;
                moved[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        assert_eq!(stitcher.push(W, h, &moved), Step::Added(40));
    }

    #[test]
    fn la_miniatura_cabe_sin_deformar() {
        let tall = page(400);
        let mut stitcher = Stitcher::new(20_000, 0);
        stitcher.push(W, 400, &tall);
        let (w, h, bgra) = stitcher.thumbnail(24, 100).unwrap();
        assert_eq!((w, h), (12, 100));
        assert_eq!(bgra.len(), 12 * 100 * 4);
    }

    /// Repite una captura guardada con `PILL_SCROLL_DEBUG` (los PNG de los
    /// cuadros, en orden) y deja `unida.png` al lado:
    /// `PILL_SCROLL_REPLAY=<carpeta> cargo test ... scroll_capture -- --ignored --nocapture cuadros_guardados`.
    #[test]
    #[ignore = "necesita una carpeta de cuadros en PILL_SCROLL_REPLAY"]
    fn cuadros_guardados_se_unen() {
        let dir = std::path::PathBuf::from(std::env::var("PILL_SCROLL_REPLAY").expect("falta PILL_SCROLL_REPLAY"));
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("no se pudo leer la carpeta")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "png") && path.file_stem().is_some_and(|name| name != "unida"))
            .collect();
        files.sort();
        let skip = std::env::var("PILL_SCROLL_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut stitcher = Stitcher::new(20_000, skip);
        let mut lost = 0;
        for path in &files {
            let (w, h, bgra) = atic_capture::encoding::png_to_bgra(&std::fs::read(path).unwrap()).expect("PNG inválido");
            let step = stitcher.push(w, h, &bgra);
            lost += (step == Step::Lost) as u32;
            eprintln!("{} {w}×{h} → {step:?}, alto {} {:?}", path.file_name().unwrap().to_string_lossy(), stitcher.height(), stitcher.detail());
        }
        eprintln!("{} cuadros, {lost} sin calzar", files.len());
        let (w, h, bgra) = stitcher.into_image().expect("sin cuadros");
        std::fs::write(dir.join("unida.png"), atic_capture::encoding::bgra_to_png(w, h, &bgra).unwrap()).unwrap();
    }

    /// Con la pantalla de verdad: el motor de captura entrega cuadros de una
    /// región y dos ventanas corridas de uno de ellos se unen. Se corre a mano
    /// (necesita el permiso de grabación de pantalla de la terminal):
    /// `cargo test ... scroll_capture -- --ignored --nocapture`.
    #[test]
    #[ignore = "fotografía la pantalla: necesita el permiso de grabación de pantalla"]
    fn cuadros_reales_de_la_pantalla_se_unen() {
        let monitor = atic_capture::monitors::enumerate().into_iter().next().expect("sin pantallas");
        let b = monitor.bounds;
        let region = atic_capture::Rect::new(b.x + b.width as i32 / 4, b.y + b.height as i32 / 4, b.width / 2, b.height / 2);
        let started = std::time::Instant::now();
        let frame = atic_capture::engine::capture_rect(region, false).expect("no se pudo tomar el cuadro");
        let took = started.elapsed();
        let (w, h) = (frame.width(), frame.height());
        eprintln!("región {region:?} → cuadro {w}×{h} en {took:?}");
        assert!(w > 0 && h > 120);
        let second = atic_capture::engine::capture_rect(region, false).expect("no se pudo tomar el segundo cuadro");
        assert_eq!((second.width(), second.height()), (w, h), "dos cuadros de la misma región miden distinto");

        // Dos ventanas del mismo cuadro, la segunda 60 filas más abajo.
        let stride = w as usize * 4;
        let shown = h - 60;
        let top = frame.bgra[..shown as usize * stride].to_vec();
        let lower = frame.bgra[60 * stride..h as usize * stride].to_vec();
        let mut stitcher = Stitcher::new(20_000, 0);
        assert_eq!(stitcher.push(w, shown, &top), Step::First);
        let step = stitcher.push(w, shown, &lower);
        eprintln!("ventana corrida 60 filas → {step:?}, alto {}", stitcher.height());
        // Lisa (un fondo sin dibujo) no hay cómo calzarla: queda igual.
        assert!(matches!(step, Step::Added(60) | Step::Same), "no calzó: {step:?}");
    }
}
