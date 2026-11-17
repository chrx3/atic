//! La rueda en M3 Expressive: un menú FAB radial.
//!
//! En el estilo Atic la rueda es líquida (`Pill::wheel_frame`); en Expressive
//! el núcleo es un FAB de 56 en `primary_container` que sale de la pill como
//! cuadrado redondeado con «+» y, al abrirse, se vuelve círculo y el «+» gira
//! a «×» (la forma del menú FAB de M3 abierto: un clic ahí la cierra). Los
//! gajos son botones circulares de 48 en `secondary_container`, en anillo y
//! sin cuellos: salen del núcleo escalonados y se recogen en orden inverso,
//! más rápido.
//!
//! Junto a una orilla o en una esquina, el anillo completo taparía la cara de
//! la pill o se saldría de la pantalla: los gajos se abren en arco hacia
//! adentro, en una o más filas (`fan`). El núcleo se queda sobre la gota.
//!
//! Un solo reloj (`WheelMotion::since`, desde el último cambio) reparte el
//! turno de cada gajo; cada uno sigue su objetivo con un resorte de gpui-m3,
//! así que abrir a mitad del cierre (o al revés) sigue sin saltos, como
//! `gpui_m3::FabMenu`. `wheel_time` sigue siendo el sobre de la rueda: queda
//! encendido mientras los gajos se recogen (`advance_wheel_m3`).
//!
//! Lo dibujado y el hit-test salen de la misma geometría (`geometry`).
//!
//! Afinables en `tweaks.json` (`numbers`):
//! `pill.m3.wheel.core` (56), `.item` (48), `.ring` (80), `.gap` (8),
//! `.icon` (24), `.staggerIn` (35 ms), `.staggerOut` (20 ms), `.lead`
//! (70 ms) y `.hoverGrow` (0.1).

use std::f32::consts::{FRAC_PI_4, PI, TAU};
use std::time::Instant;

use gpui::{point, px, size, BoxShadow, Bounds, Corners, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, Styled, Window};
use gpui_m3::{ShapeName, Spring};

use crate::anim::lerp;
use crate::geometry::Rect;
use crate::pill_look::{PillTokens, Track};
use crate::pill_tools::StripId;
use crate::tweak::num;

const CORE: f32 = 56.0;
const ITEM: f32 = 48.0;
/// Del centro del núcleo al centro de cada gajo (crece si no caben).
const RING: f32 = 80.0;
/// Lo mínimo entre dos gajos vecinos.
const GAP: f32 = 8.0;
/// El lado mínimo de un gajo: el área de clic de M3.
const MIN_ITEM: f32 = 40.0;
/// Cuánto achicar los gajos de una vez cuando no caben.
const SHRINK_STEP: f32 = 4.0;
/// Cuánto puede crecer una fila sola antes de sumar otra, y de a cuánto.
const GROW: f32 = 64.0;
const GROW_STEP: f32 = 4.0;
const MAX_ROWS: usize = 4;
/// Entre un gajo y la orilla del área de trabajo.
const EDGE_MARGIN: f32 = 8.0;
/// La última fila a medias junta sus gajos: a lo más esto por el paso justo.
const TIGHT: f32 = 1.25;
const ICON: f32 = 24.0;
const STAGGER_IN_MS: f32 = 35.0;
const STAGGER_OUT_MS: f32 = 20.0;
/// El primer gajo sale cuando el núcleo ya va llegando.
const LEAD_MS: f32 = 70.0;
/// Cuánto crece un gajo con el cursor encima.
const HOVER_GROW: f32 = 0.1;
/// El hit-test es lo dibujado más este margen.
const HIT_SLACK: f32 = 4.0;
/// Fuera del anillo, cuánto más sigue contando como «cerca» (no se cierra).
const NEAR_SLACK: f32 = 16.0;
/// La «×» del núcleo: largo y grosor de cada trazo, por lado del ícono.
const CROSS_LONG: f32 = 0.58;
const CROSS_THICK: f32 = 2.0;

/// La pill está en M3 Expressive: la rueda es este menú FAB.
pub(crate) fn on() -> bool {
    crate::appearance::current().pill == crate::appearance::PillStyle::Expressive
}

/// Los tamaños de la rueda, con lo que diga `tweaks.json`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sizes {
    pub core: f32,
    pub item: f32,
    pub ring: f32,
    pub gap: f32,
}

impl Sizes {
    fn current() -> Self {
        Self {
            core: num("pill.m3.wheel.core", CORE),
            item: num("pill.m3.wheel.item", ITEM),
            ring: num("pill.m3.wheel.ring", RING),
            gap: num("pill.m3.wheel.gap", GAP),
        }
    }
}

/// Un gajo con la rueda abierta: su ángulo y su distancia al núcleo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spot {
    pub angle: f32,
    pub ring: f32,
}

/// La rueda abierta: dónde se planta el núcleo y dónde queda cada gajo, en
/// el orden en que salen.
#[derive(Clone, Debug)]
pub(crate) struct Fan {
    pub core: (f32, f32),
    pub side: f32,
    pub spots: Vec<Spot>,
}

impl Fan {
    pub(crate) fn center(&self, spot: Spot) -> (f32, f32) {
        (self.core.0 + spot.angle.cos() * spot.ring, self.core.1 + spot.angle.sin() * spot.ring)
    }

    /// Del centro del núcleo al borde del gajo más lejano.
    pub(crate) fn reach(&self) -> f32 {
        self.spots.iter().map(|spot| spot.ring + self.side / 2.0).fold(0.0, f32::max)
    }

    /// La caja de la rueda abierta, núcleo incluido: izquierda, arriba,
    /// derecha y abajo.
    pub(crate) fn bounds(&self, core_side: f32) -> (f32, f32, f32, f32) {
        let mut bounds = (
            self.core.0 - core_side / 2.0,
            self.core.1 - core_side / 2.0,
            self.core.0 + core_side / 2.0,
            self.core.1 + core_side / 2.0,
        );
        for &spot in &self.spots {
            let (x, y) = self.center(spot);
            let half = self.side / 2.0;
            bounds = (bounds.0.min(x - half), bounds.1.min(y - half), bounds.2.max(x + half), bounds.3.max(y + half));
        }
        bounds
    }
}

/// El arco libre para gajos de lado `side` a `ring` del núcleo: ni cruzan
/// la orilla de `area` (con `EDGE_MARGIN`) ni tocan la cara de la pill.
/// `(inicio, barrido)` en radianes; barrido `TAU` es el anillo completo y
/// `None`, que no queda nada.
fn free_arc(core: (f32, f32), ring: f32, side: f32, gap: f32, area: &Rect, face: ((f32, f32), f32)) -> Option<(f32, f32)> {
    // Cada estorbo tapa los ángulos θ con cos(θ − dirección) > límite: un
    // tramo de medio ancho acos(límite). Al acercarse el núcleo, el límite
    // baja de a poco y el tramo crece sin saltos.
    let mut cuts: Vec<(f32, f32)> = Vec::with_capacity(5);
    let inset = side / 2.0 + EDGE_MARGIN;
    let ((fx, fy), fr) = face;
    let (dx, dy) = (fx - core.0, fy - core.1);
    let distance = dx.hypot(dy);
    let clear = fr + side / 2.0 + gap;
    // La cara, por la ley del coseno; justo bajo el núcleo, todo o nada.
    let face_limit = if distance > 1e-3 {
        (ring * ring + distance * distance - clear * clear) / (2.0 * ring * distance)
    } else if ring >= clear {
        1.0
    } else {
        -1.0
    };
    for (limit, direction) in [
        ((area.right() - core.0 - inset) / ring, 0.0),
        ((area.bottom() - core.1 - inset) / ring, PI / 2.0),
        ((core.0 - area.x - inset) / ring, PI),
        ((core.1 - area.y - inset) / ring, -PI / 2.0),
        (face_limit, dy.atan2(dx)),
    ] {
        if limit <= -1.0 {
            return None;
        }
        if limit < 1.0 {
            let half = limit.acos();
            let start = (direction - half).rem_euclid(TAU);
            cuts.push((start, start + 2.0 * half));
        }
    }
    if cuts.is_empty() {
        return Some((-PI / 2.0, TAU));
    }
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::with_capacity(cuts.len());
    for (start, end) in cuts {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    // Lo que pasa de TAU da la vuelta y se junta con los primeros.
    while merged.len() > 1 && merged[merged.len() - 1].1 - TAU >= merged[0].0 {
        let first = merged.remove(0);
        if let Some(last) = merged.last_mut() {
            last.1 = last.1.max(first.1 + TAU);
        }
    }
    let mut best: Option<(f32, f32)> = None;
    for (index, &(_, end)) in merged.iter().enumerate() {
        let next = merged.get(index + 1).map_or(merged[0].0 + TAU, |cut| cut.0);
        let sweep = next - end;
        if sweep >= 0.0 && best.map_or(true, |(_, widest)| sweep > widest) {
            best = Some((end, sweep));
        }
    }
    best
}

/// Una fila de gajos a `ring` del núcleo.
struct Row {
    ring: f32,
    /// `(inicio, barrido)`; barrido 0 sin lugar.
    arc: (f32, f32),
    /// El ángulo entre dos vecinos que se tocan (con `gap` entre ellos).
    step: f32,
    capacity: usize,
}

impl Row {
    fn new(core: (f32, f32), ring: f32, side: f32, sizes: Sizes, area: &Rect, face: ((f32, f32), f32)) -> Self {
        let step = 2.0 * ((side + sizes.gap) / (2.0 * ring)).min(1.0).asin();
        let arc = free_arc(core, ring, side, sizes.gap, area, face);
        // Ni en un arco casi completo los extremos se tocan por detrás.
        let whole = (TAU / step + 1e-4).floor() as usize;
        let capacity = match arc {
            None => 0,
            Some((_, sweep)) if sweep >= TAU => whole,
            Some((_, sweep)) => ((sweep / step + 1e-4).floor() as usize + 1).min(whole),
        };
        Self { ring, arc: arc.unwrap_or((0.0, 0.0)), step, capacity }
    }

    /// `count` gajos en la fila. En el anillo completo, como siempre: el
    /// primero arriba y en el sentido del reloj. En un arco, repartidos de
    /// un extremo al otro (o juntos al medio con `tight`), empezando por el
    /// extremo de más arriba, o el de la izquierda: se lee como un texto.
    fn spots(&self, count: usize, tight: bool) -> Vec<Spot> {
        let ring = self.ring;
        let (start, sweep) = self.arc;
        if sweep >= TAU {
            return (0..count)
                .map(|index| Spot { angle: -PI / 2.0 + index as f32 / count as f32 * TAU, ring })
                .collect();
        }
        let gaps = count.saturating_sub(1) as f32;
        let mut spread = if count > 1 { (sweep / gaps).min(TAU / count as f32) } else { 0.0 };
        if tight {
            spread = spread.min(self.step * TIGHT);
        }
        let middle = start + sweep / 2.0;
        let half = spread * gaps / 2.0;
        let rank = |angle: f32| 2.0 * angle.sin() + angle.cos();
        let turn = if rank(middle + half) < rank(middle - half) - 1e-4 { -1.0 } else { 1.0 };
        (0..count)
            .map(|index| Spot { angle: middle + turn * (index as f32 * spread - half), ring })
            .collect()
    }
}

/// Llena las filas de adentro hacia afuera; la última, si queda a medias,
/// junta sus gajos al medio del arco. Si no caben (una pantalla diminuta),
/// la última fila con lugar se aprieta.
fn fill(rows: &[Row], count: usize) -> Vec<Spot> {
    let last = rows.iter().rposition(|row| row.capacity > 0).unwrap_or(rows.len().saturating_sub(1));
    let mut spots = Vec::with_capacity(count);
    for (index, row) in rows.iter().enumerate() {
        let left = count - spots.len();
        let take = if index == last { left } else { row.capacity.min(left) };
        if take == 0 {
            continue;
        }
        spots.extend(row.spots(take, index > 0 && take < row.capacity));
    }
    spots
}

/// Dónde queda todo con la rueda abierta. `rest` es donde querría estar el
/// núcleo (sobre la gota, o lejos del tab); entra al área lo justo para
/// verse entero. Los gajos ocupan lo que dejan libre las orillas y la cara
/// de la pill (`face`): lejos de todo, el anillo completo; junto a una
/// orilla, medio círculo hacia adentro; en una esquina, un cuarto largo
/// hacia la diagonal. Si no caben, primero crece el radio, después se suman
/// filas y recién al final se achican, hasta `MIN_ITEM`.
pub(crate) fn fan(rest: (f32, f32), face: ((f32, f32), f32), area: &Rect, count: usize, sizes: Sizes) -> Fan {
    let inset = sizes.core / 2.0 + EDGE_MARGIN;
    let keep = |value: f32, low: f32, high: f32| value.min(high - inset).max(low + inset);
    let core = (keep(rest.0, area.x, area.right()), keep(rest.1, area.y, area.bottom()));
    if count == 0 {
        return Fan { core, side: sizes.item, spots: Vec::new() };
    }
    let row = |ring: f32, side: f32| Row::new(core, ring, side, sizes, area, face);
    let mut side = sizes.item;
    let (side, rows) = loop {
        let base = sizes.ring.max(sizes.core / 2.0 + side / 2.0 + sizes.gap);
        // Lejos de las orillas, el anillo de siempre, que crece lo justo.
        let whole = base.max((side + sizes.gap) / (2.0 * (PI / count.max(2) as f32).sin()));
        if whole <= base + GROW {
            let full = row(whole, side);
            if full.arc.1 >= TAU && full.capacity >= count {
                return Fan { core, side, spots: full.spots(count, false) };
            }
        }
        let mut ring = base;
        while ring <= base + GROW {
            let single = row(ring, side);
            if single.capacity >= count {
                return Fan { core, side, spots: single.spots(count, false) };
            }
            ring += GROW_STEP;
        }
        let pitch = side + sizes.gap;
        let rows: Vec<Row> = (0..MAX_ROWS).map(|index| row(base + index as f32 * pitch, side)).collect();
        for used in 2..=MAX_ROWS {
            if rows[..used].iter().map(|row| row.capacity).sum::<usize>() >= count {
                return Fan { core, side, spots: fill(&rows[..used], count) };
            }
        }
        if side - SHRINK_STEP < MIN_ITEM - 1e-3 {
            break (side, rows);
        }
        side -= SHRINK_STEP;
    };
    Fan { core, side, spots: fill(&rows, count) }
}

fn seconds(key: &str, default_ms: f32) -> f32 {
    num(key, default_ms).max(0.0) / 1000.0
}

/// Un gajo que se va al cambiar de nivel: vuelve al núcleo en su lugar.
struct Leaving {
    slot: StripId,
    index: usize,
    count: usize,
    progress: Track,
}

/// Lo que se anima de la rueda en Expressive.
pub(crate) struct WheelMotion {
    open: bool,
    /// Segundos desde que se abrió, se cerró o cambió de nivel.
    since: f32,
    /// 0 en la pill, 1 en el centro de la rueda.
    core: Track,
    /// 0 «+» cuadrado, 1 «×» redondo.
    turn: Track,
    items: Vec<Track>,
    hover: Vec<Track>,
    leaving: Vec<Leaving>,
}

impl Default for WheelMotion {
    fn default() -> Self {
        Self {
            open: false,
            since: f32::MAX,
            core: Track::at(0.0),
            turn: Track::at(0.0),
            items: Vec::new(),
            hover: Vec::new(),
            leaving: Vec::new(),
        }
    }
}

impl WheelMotion {
    /// Avanza `dt` segundos. Devuelve si hay que pedir otro cuadro.
    fn advance(&mut self, open: bool, count: usize, hover: &[f32], reduced: bool, dt: f32) -> bool {
        if open != self.open {
            self.open = open;
            self.since = 0.0;
        }
        self.since = (self.since + dt).min(f32::MAX / 2.0);
        self.items.resize(count, Track::at(0.0));
        self.hover.resize(count, Track::at(0.0));
        let stagger_in = seconds("pill.m3.wheel.staggerIn", STAGGER_IN_MS);
        let stagger_out = seconds("pill.m3.wheel.staggerOut", STAGGER_OUT_MS);
        let lead = seconds("pill.m3.wheel.lead", LEAD_MS);
        let last = count.saturating_sub(1);
        // Abriendo, resorte espacial; cerrando, el rápido.
        let spring = if open { Spring::SPATIAL } else { Spring::SPATIAL_FAST };
        let target = if open { 1.0 } else { 0.0 };
        let mut pending = false;

        // El núcleo sale primero y vuelve a la pill a mitad del cierre.
        let core_due = open || reduced || self.since >= last as f32 * stagger_out * 0.5;
        pending |= !core_due;
        let move_to = |track: &mut Track, due: bool, goal: f32, spring: Spring| {
            // Sin su turno, sigue hacia donde iba.
            if due && reduced {
                track.snap(goal);
            } else if due {
                track.set(goal);
            }
            track.advance(spring, dt);
        };
        move_to(&mut self.core, core_due, target, spring);
        move_to(&mut self.turn, true, target, Spring::SPATIAL);
        for (index, item) in self.items.iter_mut().enumerate() {
            let delay = if open { lead + index as f32 * stagger_in } else { (last - index) as f32 * stagger_out };
            let due = reduced || self.since >= delay;
            pending |= !due;
            move_to(item, due, target, spring);
        }
        for (track, &goal) in self.hover.iter_mut().zip(hover) {
            move_to(track, true, goal, Spring::SPATIAL_FAST);
        }
        for leaving in &mut self.leaving {
            let delay = (leaving.count.saturating_sub(1) - leaving.index) as f32 * stagger_out;
            let due = reduced || self.since >= delay;
            pending |= !due;
            move_to(&mut leaving.progress, due, 0.0, Spring::SPATIAL_FAST);
        }
        self.leaving.retain(|leaving| leaving.progress.moving() || leaving.progress.value > 1e-3);

        pending
            || self.core.moving()
            || self.turn.moving()
            || self.items.iter().chain(&self.hover).any(Track::moving)
            || !self.leaving.is_empty()
    }

    /// Algo de la rueda sigue a la vista.
    fn visible(&self) -> bool {
        self.open
            || self.core.moving()
            || self.core.value.abs() > 1e-3
            || self.items.iter().any(|item| item.moving() || item.value.abs() > 1e-3)
            || !self.leaving.is_empty()
    }

    /// Abierta y con los primeros gajos afuera: ya se puede elegir.
    pub(crate) fn ready(&self) -> bool {
        self.open && self.since >= seconds("pill.m3.wheel.lead", LEAD_MS) + 0.1
    }

    /// Cambio de nivel: los gajos de `old` vuelven al núcleo y los nuevos
    /// salen como al abrir, sin que el núcleo se recoja.
    pub(crate) fn switch_page(&mut self, old: &[StripId]) {
        let count = old.len();
        for (index, (&slot, progress)) in old.iter().zip(self.items.drain(..)).enumerate() {
            self.leaving.push(Leaving { slot, index, count, progress });
        }
        for hover in &mut self.hover {
            hover.snap(0.0);
        }
        self.since = 0.0;
    }
}

/// Un botón de la rueda en este cuadro.
pub(crate) struct ItemPlace {
    pub slot: StripId,
    /// El índice en el nivel a la vista; `None` si es de uno que se va.
    pub index: Option<usize>,
    pub center: (f32, f32),
    pub side: f32,
    /// Cuánto salió (0..1, sin rebote).
    pub shown: f32,
    pub alpha: f32,
}

/// Dónde está cada cosa de la rueda en este cuadro: lo mismo para dibujar y
/// para el hit-test.
pub(crate) struct Geometry {
    pub core: (f32, f32),
    pub core_side: f32,
    pub core_alpha: f32,
    /// La rueda abierta del nivel a la vista.
    pub fan: Fan,
    pub items: Vec<ItemPlace>,
}

/// Lo que pinta `Frame::paint_base`: botones y núcleo, con sus sombras. Los
/// íconos de los gajos van con los demás (`wheel_icons`, en `paint_top`).
pub(crate) struct WheelPaint {
    items: Vec<ItemPaint>,
    core: CorePaint,
}

struct ItemPaint {
    center: (f32, f32),
    side: f32,
    color: Hsla,
    layer: Hsla,
    shadow: Vec<BoxShadow>,
}

struct CorePaint {
    center: (f32, f32),
    side: f32,
    points: Vec<(f32, f32)>,
    /// El radio de la sombra, que sigue a la forma.
    radius: f32,
    color: Hsla,
    ink: Hsla,
    turn: f32,
    shadow: Vec<BoxShadow>,
}

fn square(center: (f32, f32), side: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(center.0 - side / 2.0), px(center.1 - side / 2.0)), size(px(side), px(side)))
}

/// Un trazo de puntas redondas de `long` × `thick`, girado `angle`.
fn stroke(center: (f32, f32), long: f32, thick: f32, angle: f32) -> Vec<gpui::Point<Pixels>> {
    const CAP: usize = 6;
    let (sin, cos) = angle.sin_cos();
    let half = (long - thick).max(0.0) / 2.0;
    let r = thick / 2.0;
    let mut points = Vec::with_capacity(2 * (CAP + 1));
    for (end, base) in [(half, -PI / 2.0), (-half, PI / 2.0)] {
        for step in 0..=CAP {
            let a = base + PI * step as f32 / CAP as f32;
            let (x, y) = (end + r * a.cos(), r * a.sin());
            points.push(point(px(center.0 + x * cos - y * sin), px(center.1 + x * sin + y * cos)));
        }
    }
    points
}

impl WheelPaint {
    pub(crate) fn paint(&self, window: &mut Window) {
        for item in &self.items {
            let bounds = square(item.center, item.side);
            let corners = Corners::all(px(item.side / 2.0));
            window.paint_shadows(bounds, corners.clone(), &item.shadow);
            window.paint_quad(gpui::fill(bounds, item.color).corner_radii(corners.clone()));
            if item.layer.a > 0.003 {
                window.paint_quad(gpui::fill(bounds, item.layer).corner_radii(corners));
            }
        }
        let core = &self.core;
        if core.color.a <= 0.01 {
            return;
        }
        window.paint_shadows(square(core.center, core.side), Corners::all(px(core.radius)), &core.shadow);
        let points: Vec<_> = core
            .points
            .iter()
            .map(|&(x, y)| point(px(core.center.0 + (x - 0.5) * core.side), px(core.center.1 + (y - 0.5) * core.side)))
            .collect();
        let mut builder = PathBuilder::fill();
        builder.add_polygon(&points, true);
        if let Ok(path) = builder.build() {
            window.paint_path(path, core.color);
        }
        // «+» que gira a «×».
        let icon = num("pill.m3.wheel.icon", ICON) * core.side / num("pill.m3.wheel.core", CORE);
        let angle = core.turn * FRAC_PI_4;
        for angle in [angle, angle + PI / 2.0] {
            let mut builder = PathBuilder::fill();
            builder.add_polygon(&stroke(core.center, icon * CROSS_LONG, CROSS_THICK, angle), true);
            if let Ok(path) = builder.build() {
                window.paint_path(path, core.ink);
            }
        }
    }
}

impl crate::Pill {
    /// Avanza la rueda de Expressive (desde `advance`). Devuelve si hay que
    /// pedir otro cuadro. En Atic no hace nada.
    pub(crate) fn advance_wheel_m3(&mut self, dt: f32) -> bool {
        if !on() {
            if self.wheel_m3.visible() {
                self.wheel_m3 = WheelMotion::default();
            }
            return false;
        }
        let count = self.wheel_slots().len();
        let hover: Vec<f32> = self.wheel_hover.iter().take(count).map(|tween| tween.target()).collect();
        let moving = self.wheel_m3.advance(self.wheel_target_open, count, &hover, self.reduced_motion, dt);
        if !self.wheel_target_open {
            // `wheel_time` dice si la rueda se ve (`wheel_visible`): queda
            // encendido mientras los gajos se recogen y se apaga con ellos.
            self.wheel_time = if self.wheel_m3.visible() { self.wheel_time.max(1.0) } else { 0.0 };
        }
        moving
    }

    pub(crate) fn wheel_m3_geometry(&self, place: &crate::WheelPlace) -> Geometry {
        let motion = &self.wheel_m3;
        let sizes = Sizes::current();
        let fan_of = |count: usize| fan(place.core, place.anchor, &place.room, count, sizes);
        let slots = self.wheel_slots();
        let current = fan_of(slots.len());
        let core_full = sizes.core;
        let k = motion.core.value;
        let core = (lerp(place.source.0, current.core.0, k), lerp(place.source.1, current.core.1, k));
        let core_side = lerp((place.source_r * 2.0).min(core_full), core_full, k.max(0.0));
        let core_alpha = (k * 3.0).clamp(0.0, 1.0);
        let grow = num("pill.m3.wheel.hoverGrow", HOVER_GROW);

        // Cada gajo sale del núcleo (donde vaya) hacia su lugar en la rueda.
        let place_item = |fan: &Fan, slot: StripId, index: usize, progress: f32, hover: f32| {
            let spot = fan.spots.get(index).copied().unwrap_or(Spot { angle: -PI / 2.0, ring: 0.0 });
            let reach = spot.ring * progress.max(0.0);
            let shown = progress.clamp(0.0, 1.0);
            ItemPlace {
                slot,
                index: None,
                center: (core.0 + spot.angle.cos() * reach, core.1 + spot.angle.sin() * reach),
                side: fan.side * lerp(0.4, 1.0, shown) * (1.0 + grow * hover.max(0.0)),
                shown,
                alpha: (progress * 1.6).clamp(0.0, 1.0),
            }
        };
        // Los que se van vuelven por donde salieron, con la rueda de su nivel.
        let mut old: Vec<(usize, Fan)> = Vec::new();
        for leaving in &motion.leaving {
            if !old.iter().any(|(count, _)| *count == leaving.count) {
                old.push((leaving.count, fan_of(leaving.count)));
            }
        }
        let mut items: Vec<ItemPlace> = motion
            .leaving
            .iter()
            .filter_map(|leaving| {
                let (_, fan) = old.iter().find(|(count, _)| *count == leaving.count)?;
                Some(place_item(fan, leaving.slot, leaving.index, leaving.progress.value, 0.0))
            })
            .collect();
        for (index, (&slot, track)) in slots.iter().zip(&motion.items).enumerate() {
            let hover = motion.hover.get(index).map_or(0.0, |track| track.value);
            let mut item = place_item(&current, slot, index, track.value, hover);
            item.index = Some(index);
            items.push(item);
        }
        Geometry {
            core,
            core_side,
            core_alpha,
            fan: current,
            items: items.into_iter().filter(|item| item.alpha > 0.01).collect(),
        }
    }

    /// Lo que pinta la rueda en este cuadro y los íconos de sus gajos; `None`
    /// en Atic o con la rueda guardada.
    pub(crate) fn wheel_m3_frame(
        &self,
        place: &crate::WheelPlace,
        tokens: &PillTokens,
        now: Instant,
    ) -> Option<(WheelPaint, Vec<crate::IconDraw>)> {
        let scheme = tokens.scheme.as_ref().filter(|_| on() && self.wheel_visible())?;
        let geometry = self.wheel_m3_geometry(place);
        let icon = num("pill.m3.wheel.icon", ICON);
        let item_full = num("pill.m3.wheel.item", ITEM);
        let mut items = Vec::new();
        let mut icons = Vec::new();
        // La herramienta abierta, con el ícono más marcado (como en la tira).
        let chosen = self.chosen_tool();
        for item in &geometry.items {
            let hover = item.index.and_then(|index| self.wheel_hover.get(index)).map_or(0.0, |tween| tween.value(now));
            items.push(ItemPaint {
                center: item.center,
                side: item.side,
                color: scheme.secondary_container.opacity(item.alpha),
                layer: scheme
                    .on_secondary_container
                    .opacity(PillTokens::layer_alpha(hover, 0.0) * item.alpha),
                // Quince gajos con la sombra del FAB pesan: el escalón tonal
                // los separa y la elevación 2 solo refuerza.
                shadow: gpui_m3::Elevation::Level2
                    .shadow(scheme)
                    .into_iter()
                    .map(|shadow| BoxShadow { color: shadow.color.opacity(item.alpha * item.alpha), ..shadow })
                    .collect(),
            });
            icons.push(crate::IconDraw {
                path: item.slot.icon(),
                center: item.center,
                size: icon * item.side / item_full,
                color: scheme.on_secondary_container.opacity(item.alpha),
                art: (item.slot.tool() == Some(crate::MEDIA_TOOL))
                    .then(|| self.media.track().and_then(|t| t.art))
                    .flatten(),
                bold: chosen.is_some() && item.slot.tool() == chosen,
            });
        }
        let turn = self.wheel_m3.turn.value;
        let from = ShapeName::Square.unit_points();
        let to = ShapeName::Circle.unit_points();
        let t = turn.clamp(0.0, 1.0);
        let points = from
            .iter()
            .zip(to)
            .map(|(&(ax, ay), &(bx, by))| (lerp(ax, bx, t), lerp(ay, by, t)))
            .collect();
        let core = CorePaint {
            center: geometry.core,
            side: geometry.core_side,
            points,
            radius: geometry.core_side * lerp(0.3, 0.5, t),
            color: scheme.primary_container.opacity(geometry.core_alpha),
            ink: scheme.on_primary_container.opacity(geometry.core_alpha),
            turn,
            shadow: tokens.shadow(geometry.core_alpha),
        };
        Some((WheelPaint { items, core }, icons))
    }

    /// El gajo o el núcleo bajo `p`, igual que `wheel_target_at`: solo lo
    /// dibujado (más `HIT_SLACK`) y solo gajos que ya salieron.
    pub(crate) fn wheel_m3_target_at(&self, p: (f32, f32), now: Instant) -> Option<Option<usize>> {
        let geometry = self.wheel_m3_geometry(&self.wheel_place(now));
        let distance = |c: (f32, f32)| (p.0 - c.0).hypot(p.1 - c.1);
        let item = geometry.items.iter().find(|item| {
            item.index.is_some() && item.shown >= 0.5 && distance(item.center) <= item.side / 2.0 + HIT_SLACK
        });
        if let Some(item) = item {
            return Some(item.index);
        }
        (distance(geometry.core) <= geometry.core_side / 2.0 + HIT_SLACK).then_some(None)
    }

    /// El cursor anda por la rueda (sobre algo o entre los gajos): no se
    /// cierra sola ni deja pasar el clic.
    pub(crate) fn wheel_m3_near(&self, p: (f32, f32), now: Instant) -> bool {
        let geometry = self.wheel_m3_geometry(&self.wheel_place(now));
        let distance = |c: (f32, f32)| (p.0 - c.0).hypot(p.1 - c.1);
        distance(geometry.fan.core) <= geometry.fan.reach() + NEAR_SLACK
            || distance(geometry.core) <= geometry.core_side / 2.0 + NEAR_SLACK
    }

    /// El nombre del gajo bajo el cursor, como tooltip simple de M3
    /// (`inverse_surface`), justo debajo de la rueda; si no cabe (la rueda
    /// junto a la orilla de abajo), justo encima.
    pub(crate) fn wheel_m3_label(&self, now: Instant) -> Option<gpui::AnyElement> {
        use gpui::div;
        use gpui_m3::theme::type_scale::{self, TypeScaleExt as _};
        const W: f32 = 300.0;
        const H: f32 = 24.0;
        if !self.wheel_target_open || !self.wheel_visible() {
            return None;
        }
        let (bg, ink) = PillTokens::current().tooltip?;
        let slots = self.wheel_slots();
        // El que más se encendió: al pasar de un gajo al vecino, el nuevo.
        let (slot, amount) = slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| Some((*slot, self.wheel_hover.get(index)?.value(now).clamp(0.0, 1.0))))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, amount)| *amount > 0.05)?;
        let geometry = self.wheel_m3_geometry(&self.wheel_place(now));
        let (left, top, right, bottom) = geometry.fan.bounds(num("pill.m3.wheel.core", CORE));
        let work = self.work;
        let y = if bottom + 8.0 + H <= work.bottom() - 4.0 { bottom + 8.0 } else { (top - 8.0 - H).max(work.y + 4.0) };
        let x = ((left + right) / 2.0 - W / 2.0).min(work.right() - W).max(work.x);
        Some(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(W))
                .h(px(H))
                .flex()
                .justify_center()
                .opacity(amount)
                .font_family(crate::fonts::UI)
                .child(
                    div()
                        .h(px(H))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(gpui_m3::corner::XS)
                        .bg(bg)
                        .text_color(ink)
                        .type_style(type_scale::BODY_SMALL)
                        .child(slot.hint()),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: Sizes = Sizes { core: CORE, item: ITEM, ring: RING, gap: GAP };
    const FACE_R: f32 = 26.0;

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 1440.0, 900.0)
    }

    /// La gota en `at`, con la rueda de `count` gajos.
    fn open(at: (f32, f32), count: usize) -> Fan {
        fan(at, (at, FACE_R), &area(), count, SIZES)
    }

    /// Ningún gajo sale del área, tapa la cara o el núcleo, ni pisa a otro.
    fn assert_clean(fan: &Fan, face: (f32, f32), count: usize) {
        let area = area();
        let half = fan.side / 2.0;
        let slack = 0.5;
        assert_eq!(fan.spots.len(), count);
        assert!(fan.side >= MIN_ITEM, "gajos de {}", fan.side);
        let centers: Vec<_> = fan.spots.iter().map(|&spot| fan.center(spot)).collect();
        for (index, &(x, y)) in centers.iter().enumerate() {
            assert!(x - half >= area.x - slack && x + half <= area.right() + slack, "gajo {index} fuera en x: {x}");
            assert!(y - half >= area.y - slack && y + half <= area.bottom() + slack, "gajo {index} fuera en y: {y}");
            let to_face = (x - face.0).hypot(y - face.1);
            assert!(to_face + slack >= FACE_R + half, "gajo {index} tapa la cara ({to_face})");
            let to_core = (x - fan.core.0).hypot(y - fan.core.1);
            assert!(to_core + slack >= CORE / 2.0 + half, "gajo {index} tapa el núcleo ({to_core})");
            for (other, &(ox, oy)) in centers.iter().enumerate().skip(index + 1) {
                let apart = (x - ox).hypot(y - oy);
                assert!(apart + slack >= fan.side, "gajos {index} y {other} se pisan ({apart})");
            }
        }
        let (core_x, core_y) = fan.core;
        assert!(core_x - CORE / 2.0 >= area.x && core_x + CORE / 2.0 <= area.right());
        assert!(core_y - CORE / 2.0 >= area.y && core_y + CORE / 2.0 <= area.bottom());
    }

    #[test]
    fn lejos_de_las_orillas_es_el_anillo_de_siempre() {
        for count in 1..=8 {
            let fan = open((720.0, 450.0), count);
            assert_eq!(fan.side, ITEM);
            assert_eq!(fan.core, (720.0, 450.0));
            for (index, spot) in fan.spots.iter().enumerate() {
                let angle = -PI / 2.0 + index as f32 / count as f32 * TAU;
                assert!((spot.angle - angle).abs() < 1e-4 && (spot.ring - RING).abs() < 1e-4, "{count}: {spot:?}");
            }
            assert_clean(&fan, (720.0, 450.0), count);
        }
        // Con muchos, el anillo crece en vez de achicar los gajos.
        let fan = open((720.0, 450.0), 15);
        assert_eq!(fan.side, ITEM);
        assert!(fan.spots.iter().all(|spot| spot.ring > RING && spot.ring == fan.spots[0].ring));
        assert_clean(&fan, (720.0, 450.0), 15);
    }

    #[test]
    fn junto_a_una_orilla_se_abre_en_medio_circulo_hacia_adentro() {
        let edges = [(720.0, 20.0), (720.0, 880.0), (20.0, 450.0), (1420.0, 450.0)];
        for at in edges {
            for count in [3, 6, 8, 12, 15] {
                let fan = open(at, count);
                assert_clean(&fan, at, count);
            }
            // El arco apunta hacia adentro: el promedio de los gajos queda del
            // lado del centro de la pantalla.
            let fan = open(at, 6);
            let (sx, sy) = fan.spots.iter().fold((0.0, 0.0), |(x, y), &spot| (x + spot.angle.cos(), y + spot.angle.sin()));
            let inward = (720.0 - fan.core.0, 450.0 - fan.core.1);
            assert!(sx * inward.0 + sy * inward.1 > 0.0, "{at:?}");
        }
    }

    #[test]
    fn en_una_esquina_todos_quedan_adentro() {
        let corners = [(15.0, 15.0), (1425.0, 15.0), (15.0, 885.0), (1425.0, 885.0), (60.0, 60.0)];
        for at in corners {
            for count in 1..=15 {
                let fan = open(at, count);
                assert_clean(&fan, at, count);
            }
        }
    }

    #[test]
    fn quince_en_una_esquina_no_bajan_de_cuarenta() {
        let fan = open((10.0, 890.0), 15);
        assert!(fan.side >= 40.0);
        assert_clean(&fan, (10.0, 890.0), 15);
    }

    #[test]
    fn el_arco_cambia_de_a_poco_al_acercarse_a_la_orilla() {
        // Lo libre del primer anillo nunca salta al acercar la gota a la
        // orilla de abajo: de completo a medio círculo, de a poco.
        let mut previous: Option<f32> = None;
        for step in 0..=480 {
            let y = 890.0 - step as f32 * 0.25;
            let core = (720.0, y.min(900.0 - CORE / 2.0 - EDGE_MARGIN));
            let sweep = free_arc(core, RING, ITEM, GAP, &area(), (core, FACE_R)).map_or(0.0, |arc| arc.1);
            if let Some(previous) = previous {
                assert!(sweep >= previous - 1e-4, "{y}: {sweep} < {previous}");
                assert!(sweep - previous < 0.2, "salto en {y}: {previous} → {sweep}");
            }
            previous = Some(sweep);
        }
        assert!(previous.is_some_and(|sweep| sweep >= TAU));
    }
}
