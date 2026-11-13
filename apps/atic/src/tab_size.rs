//! El notch cerrado (el tab en reposo) a gusto: el modo «no molestar», que lo
//! deja en los ojos de la marca; su ancho y su alto, que se eligen arrastrando
//! sus esquinas (como una imagen en Flip); y lo que muestra, en orden: el
//! reloj, la música, la cara, los avisos y la onda, cada uno se puede mover o
//! sacar a la fila «Fuera». Todo lo de adentro se escala con el alto.
//!
//! Al pasar el cursor, o con algo abierto, el tab vuelve a su tamaño de
//! siempre. Se guarda en `<datos de Atic>\pill\tab.json`.
//!
//! El orden se respeta en cualquier borde (en un costado, de arriba abajo),
//! salvo fundido con el notch de la cámara: ahí manda él.

use std::path::PathBuf;
use std::time::Instant;

use gpui::{div, prelude::*, px, rgb, svg, AnyElement, Context, Hsla};
use serde::{Deserialize, Serialize};

use crate::anim::lerp;
use crate::geometry::{Edge, Rect};
use crate::i18n::t;
use crate::{distance, Home, Pill, PillShape};

/// Lo más angosto y lo más ancho, lo más bajo y lo más alto (respecto del
/// tab de siempre) que se puede dejar a mano.
pub(crate) const MIN_W: f32 = 0.4;
pub(crate) const MAX_W: f32 = 2.2;
pub(crate) const MIN_H: f32 = 0.5;
pub(crate) const MAX_H: f32 = 1.4;
/// El tab sin molestar, antes de la escala: apenas los ojos.
pub(crate) const QUIET_LENGTH: f32 = 46.0;
pub(crate) const QUIET_THICK: f32 = 20.0;
/// La cara sin molestar se dibuja más grande que la de siempre (solo se ven
/// los ojos, que en la cara de 32 miden unos 7 px de ancho).
pub(crate) const QUIET_FACE: f32 = 44.0;
/// El lado de un tirador.
pub(crate) const HANDLE: f32 = 10.0;
/// Desde las puntas del tab hasta lo primero, y lo mínimo entre dos cosas.
const PAD: f32 = 10.0;
const GAP: f32 = 8.0;
/// El punto de privacidad, siempre al final.
const PRIVACY: f32 = 8.0;
// La tarjeta de edición: junto al tab, del lado del escritorio, sin taparlo.
// Arriba «Fuera» con lo sacado y «Listo»; abajo, la ayuda. Escala de 4 px;
// los radios son concéntricos (el de la tarjeta = el de adentro + el margen).
const CARD_W: f32 = 300.0;
const CARD_PAD: f32 = 12.0;
const CARD_ROW: f32 = 28.0;
const CARD_ROW_GAP: f32 = 8.0;
/// Dos líneas de ayuda de 11 px.
const CARD_HINT: f32 = 32.0;
const CARD_H: f32 = CARD_PAD * 2.0 + CARD_ROW + CARD_ROW_GAP + CARD_HINT;
const INNER_RADIUS: f32 = 8.0;
const CARD_RADIUS: f32 = INNER_RADIUS + CARD_PAD;
/// Del tab a la tarjeta (más allá de los tiradores) y de la tarjeta a la
/// orilla del área de trabajo.
const CARD_GAP: f32 = 12.0;
const CARD_MARGIN: f32 = 8.0;
const OUT_LABEL_W: f32 = 44.0;
const OUT_TILE: f32 = 28.0;
const OUT_TILE_GAP: f32 = 6.0;
const DONE_W: f32 = 64.0;
/// El largo de cada ficha en el tab mientras se edita.
const EDIT_TILE: f32 = 26.0;
/// Se separa unos px antes de que el movimiento cuente como arrastre.
const DRAG_THRESHOLD: f32 = 4.0;

/// Lo que puede ir en el notch cerrado.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestItem {
    /// El reloj de grabar, del Pomodoro o el aviso de Computadores.
    Clock,
    /// La carátula de lo que suena.
    Music,
    Face,
    /// Los contadores de la bandeja de Agentes.
    Alerts,
    /// La onda de lo que suena.
    Wave,
}

impl RestItem {
    pub(crate) const ALL: [RestItem; 5] = [RestItem::Clock, RestItem::Music, RestItem::Face, RestItem::Alerts, RestItem::Wave];

    /// La cara es por donde se abre todo y el reloj avisa que se está
    /// grabando: esos no se sacan.
    pub(crate) fn removable(self) -> bool {
        matches!(self, RestItem::Music | RestItem::Alerts | RestItem::Wave)
    }

    fn icon(self) -> &'static str {
        match self {
            RestItem::Clock => "icons/timer.svg",
            RestItem::Music => "icons/headphones.svg",
            RestItem::Face => "icons/smile.svg",
            RestItem::Alerts => "icons/circle-check.svg",
            RestItem::Wave => "icons/audio-lines.svg",
        }
    }

    /// El largo que ocupa a escala 1 cuando no se sabe lo que trae (o en un
    /// costado, donde no depende de eso). En un costado (`vertical`) se mide
    /// de arriba abajo: el reloj es solo su punto y los avisos van uno sobre
    /// otro.
    fn sample_w(self, vertical: bool) -> f32 {
        match self {
            RestItem::Clock if vertical => 12.0,
            RestItem::Clock => 40.0,
            RestItem::Music => crate::LIVE_ART,
            RestItem::Face => crate::MARK_SIZE,
            RestItem::Alerts if vertical => 24.0,
            RestItem::Alerts => 34.0,
            RestItem::Wave if vertical => 18.0,
            RestItem::Wave => crate::BARS_W,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TabSize {
    #[serde(default)]
    pub(crate) quiet: bool,
    #[serde(default = "one")]
    pub(crate) width: f32,
    #[serde(default = "one")]
    pub(crate) height: f32,
    #[serde(default = "default_order")]
    pub(crate) order: Vec<RestItem>,
    #[serde(default)]
    pub(crate) hidden: Vec<RestItem>,
}

fn one() -> f32 {
    1.0
}

fn default_order() -> Vec<RestItem> {
    RestItem::ALL.to_vec()
}

impl Default for TabSize {
    fn default() -> Self {
        Self { quiet: false, width: 1.0, height: 1.0, order: default_order(), hidden: Vec::new() }
    }
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() { value.clamp(min, max) } else { 1.0 }
}

impl TabSize {
    /// Dentro de lo permitido y con cada cosa una vez, en el notch o fuera
    /// (un archivo editado a mano, o de una versión con menos cosas, puede
    /// traer cualquier cosa). La cara y el reloj no quedan fuera.
    pub(crate) fn normalized(mut self) -> Self {
        self.width = clamp(self.width, MIN_W, MAX_W);
        self.height = clamp(self.height, MIN_H, MAX_H);
        let mut seen: Vec<RestItem> = Vec::new();
        self.order.retain(|item| {
            let fresh = !seen.contains(item);
            seen.push(*item);
            fresh
        });
        self.hidden.retain(|item| {
            let fresh = item.removable() && !seen.contains(item);
            seen.push(*item);
            fresh
        });
        for item in RestItem::ALL {
            if !seen.contains(&item) {
                self.order.push(item);
            }
        }
        self
    }

    pub(crate) fn shows(&self, item: RestItem) -> bool {
        !self.hidden.contains(&item)
    }

    /// Pone `item` en el notch en la posición `index` (al final si falta),
    /// sacándolo de donde estaba.
    pub(crate) fn place(&mut self, item: RestItem, index: Option<usize>) {
        self.order.retain(|other| *other != item);
        self.hidden.retain(|other| *other != item);
        let at = index.unwrap_or(self.order.len()).min(self.order.len());
        self.order.insert(at, item);
    }

    /// Lo saca a «Fuera»; la cara y el reloj no salen.
    pub(crate) fn remove(&mut self, item: RestItem) -> bool {
        if !item.removable() {
            return false;
        }
        self.order.retain(|other| *other != item);
        if !self.hidden.contains(&item) {
            self.hidden.push(item);
        }
        true
    }
}

/// Lo que mide a lo largo lo que va en el tab: `items` con su ancho a escala
/// 1, `k` la escala.
pub(crate) fn content_length(items: &[(RestItem, f32)], k: f32, privacy: bool) -> f32 {
    let n = items.len();
    let sum: f32 = items.iter().map(|(_, w)| w * k).sum();
    let gaps = GAP * k * n.saturating_sub(1) as f32;
    let privacy = if privacy { (GAP + PRIVACY) * k } else { 0.0 };
    PAD * k * 2.0 + sum + gaps + privacy
}

/// Dónde va cada cosa a lo largo de un tab de `length` desde `start`, en
/// orden y con el aire repartido parejo (una sola va al medio). El punto de
/// privacidad va siempre al final. Devuelve cada una con su centro y su ancho
/// (ya a escala), y el centro del punto.
pub(crate) fn arrange(
    items: &[(RestItem, f32)],
    start: f32,
    length: f32,
    k: f32,
    privacy: bool,
) -> (Vec<(RestItem, f32, f32)>, Option<f32>) {
    let pad = PAD * k;
    let dot = privacy.then(|| start + length - pad - PRIVACY * k / 2.0);
    let end = start + length - if privacy { pad + (PRIVACY + GAP) * k } else { pad };
    let lo = start + pad;
    let n = items.len();
    let sum: f32 = items.iter().map(|(_, w)| w * k).sum();
    let gap = GAP * k;
    let spare = ((end - lo) - sum - gap * n.saturating_sub(1) as f32).max(0.0) / (n + 1) as f32;
    let mut x = lo + spare;
    let placed = items
        .iter()
        .map(|&(item, w)| {
            let w = w * k;
            let center = x + w / 2.0;
            x += w + gap + spare;
            (item, center, w)
        })
        .collect();
    (placed, dot)
}

/// Lo que va en el notch cerrado, ya ubicado (`Pill::rest_places`). Las
/// posiciones van a lo largo del tab (`x` arriba o abajo, `y` en un costado).
#[derive(Clone, Debug)]
pub(crate) struct RestPlaces {
    pub(crate) k: f32,
    pub(crate) vertical: bool,
    /// El centro de la franja, de través (la `y` arriba, la `x` en un costado).
    pub(crate) cross: f32,
    /// El grosor de la franja.
    pub(crate) band: f32,
    pub(crate) items: Vec<(RestItem, f32, f32)>,
    pub(crate) privacy: Option<f32>,
}

impl RestPlaces {
    /// El punto en pantalla de algo a `along` del tab, en el medio de la franja.
    pub(crate) fn point(&self, along: f32) -> (f32, f32) {
        if self.vertical { (self.cross, along) } else { (along, self.cross) }
    }

    /// Cuánto avanzó `p` a lo largo del tab.
    pub(crate) fn along(&self, p: (f32, f32)) -> f32 {
        if self.vertical { p.1 } else { p.0 }
    }

    pub(crate) fn center_of(&self, item: RestItem) -> Option<f32> {
        self.items.iter().find(|(other, ..)| *other == item).map(|(_, center, _)| *center)
    }

    pub(crate) fn at(&self, item: RestItem) -> Option<(f32, f32)> {
        self.center_of(item).map(|along| self.point(along))
    }

    pub(crate) fn start_of(&self, item: RestItem) -> Option<f32> {
        self.items.iter().find(|(other, ..)| *other == item).map(|(_, center, w)| center - w / 2.0)
    }

    /// Si `p` cae a lo largo sobre `item` (con `margin` de holgura).
    pub(crate) fn over(&self, item: RestItem, p: (f32, f32), margin: f32) -> bool {
        let along = self.along(p);
        self.items.iter().any(|&(other, center, w)| other == item && (along - center).abs() <= w / 2.0 + margin)
    }

    /// La privacidad, en pantalla.
    pub(crate) fn privacy_at(&self) -> Option<(f32, f32)> {
        self.privacy.map(|along| self.point(along))
    }
}

/// Las dos esquinas del tab que miran al escritorio: las otras dos van
/// pegadas a la orilla de la pantalla.
pub(crate) fn outer_corners(edge: Edge, rect: &Rect) -> [(f32, f32); 2] {
    match edge {
        Edge::Top => [(rect.x, rect.bottom()), (rect.right(), rect.bottom())],
        Edge::Bottom => [(rect.x, rect.y), (rect.right(), rect.y)],
        Edge::Left => [(rect.right(), rect.y), (rect.right(), rect.bottom())],
        Edge::Right => [(rect.x, rect.y), (rect.x, rect.bottom())],
    }
}

/// Si `p` cae sobre un tirador (con algo de holgura para agarrarlo).
pub(crate) fn over_handle(edge: Edge, rect: &Rect, p: (f32, f32)) -> bool {
    outer_corners(edge, rect)
        .iter()
        .any(|&(x, y)| (p.0 - x).abs() <= HANDLE && (p.1 - y).abs() <= HANDLE)
}

/// Dónde va la tarjeta de edición: del lado del escritorio del tab `tab`
/// (acoplado en `edge`, con su centro a lo largo en `along`), sin pisarlo, y
/// corrida a lo largo lo justo para quedar dentro de `work`.
pub(crate) fn card_rect(edge: Edge, tab: &Rect, along: f32, work: &Rect) -> Rect {
    let fit = |start: f32, size: f32, lo: f32, hi: f32| start.min(hi - CARD_MARGIN - size).max(lo + CARD_MARGIN);
    match edge {
        Edge::Top | Edge::Bottom => {
            let x = fit(along - CARD_W / 2.0, CARD_W, work.x, work.right());
            let y = if edge == Edge::Top { tab.bottom() + CARD_GAP } else { tab.y - CARD_GAP - CARD_H };
            Rect::new(x, y, CARD_W, CARD_H)
        }
        Edge::Left | Edge::Right => {
            let y = fit(along - CARD_H / 2.0, CARD_H, work.y, work.bottom());
            let x = if edge == Edge::Left { tab.right() + CARD_GAP } else { tab.x - CARD_GAP - CARD_W };
            Rect::new(x, y, CARD_W, CARD_H)
        }
    }
}

/// La tarjeta de edición ya armada: dónde va, sus fichas de «Fuera» y el
/// botón «Listo».
struct EditCard {
    rect: Rect,
    tiles: Vec<(RestItem, Rect)>,
    done: Rect,
}

/// Lo que se tiene agarrado mientras se edita.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Grab {
    /// Un tirador: el ancho y el alto siguen al cursor.
    Corner,
    /// Una ficha, del tab o de «Fuera»; `moved` cuando ya es un arrastre.
    Item { item: RestItem, from_out: bool, start: (f32, f32), moved: bool },
}

/// Editando el notch cerrado.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Edit {
    pub(crate) grab: Option<Grab>,
}

fn file() -> Option<PathBuf> {
    crate::paths::file("tab.json")
}

/// Lo elegido la última vez; sin archivo, el tab de siempre.
pub(crate) fn load() -> TabSize {
    file()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<TabSize>(&bytes).ok())
        .unwrap_or_default()
        .normalized()
}

pub(crate) fn save(size: &TabSize) {
    let Some(path) = file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(size) {
        let _ = std::fs::write(path, bytes);
    }
}

/// El rojo de los avisos sin ver, sin molestar.
pub(crate) const PENDING_RED: u32 = 0xff453a;

impl Pill {
    pub(crate) fn quiet_on(&self) -> bool {
        self.tab_size.quiet
    }

    /// Sin molestar, con avisos sin ver: cuánto se ve ahora el punto que
    /// parpadea junto a los ojos (lento, sin apagarse del todo). Con algo
    /// abierto no va: la tira trae sus propios puntos.
    pub(crate) fn pending_pulse(&self, now: Instant) -> f32 {
        let pending = self.tray.pending.0 || self.tray.pending.1;
        if !self.tab_size.quiet || !pending || self.resizing.is_some() || !self.ordered_rest() {
            return 0.0;
        }
        let t = now.duration_since(self.born).as_secs_f32();
        let wave = 0.5 + 0.5 * (t * std::f32::consts::TAU / 1.4).sin();
        (0.3 + 0.7 * wave) * self.rest_calm(now)
    }

    /// Sin molestar, un punto rojo sobre las herramientas de la tira con
    /// avisos sin ver: Agentes y Celular (o la puerta de «Más», si van ahí).
    pub(crate) fn quiet_dots(&self, edge: Edge, mark: (f32, f32), strip: f32, now: Instant) -> Vec<AnyElement> {
        if strip <= 0.6 {
            return Vec::new();
        }
        let slots = self.strip_slots();
        let slot_of = |tool: usize| {
            slots.iter().position(|&slot| slot == crate::pill_tools::StripId::Tool(tool)).or_else(|| {
                self.layout
                    .more
                    .contains(&tool)
                    .then(|| slots.iter().position(|&slot| slot == crate::pill_tools::StripId::More))
                    .flatten()
            })
        };
        let (agents, phone) = self.tray.pending;
        let mut at: Vec<usize> = [(agents, crate::AGENTES_TOOL), (phone, crate::CELULAR_TOOL)]
            .into_iter()
            .filter(|(pending, _)| *pending)
            .filter_map(|(_, tool)| slot_of(tool))
            .collect();
        at.dedup();
        let alpha = crate::anim::segment(strip, 0.6, 0.4);
        at.into_iter()
            .map(|slot| {
                let icon = edge.point(&self.work, self.strip_tool_along(slot, now), edge.depth_of(&self.work, mark));
                let (x, y) = (icon.0 + 8.0, icon.1 - 8.0);
                div()
                    .absolute()
                    .left(px(x - 3.5))
                    .top(px(y - 3.5))
                    .size(px(7.0))
                    .rounded_full()
                    .bg(rgb(PENDING_RED))
                    .opacity(alpha)
                    .into_any_element()
            })
            .collect()
    }

    /// Enciende o apaga sin molestar; el tab se encoge o vuelve animado.
    pub(crate) fn set_quiet(&mut self, on: bool, cx: &mut Context<Self>) {
        self.tab_size.quiet = on;
        // Editando se ve el modo normal (es lo que se edita): sin molestar
        // vuelve a encogerlo al terminar.
        if self.resizing.is_none() {
            self.quiet.set(if on { 1.0 } else { 0.0 }, Instant::now());
        }
        save(&self.tab_size);
        cx.notify();
    }

    /// El ancho y el alto elegidos (1 es el de siempre).
    pub(crate) fn tab_scale(&self) -> (f32, f32) {
        (self.tab_size.width, self.tab_size.height)
    }

    /// Si el notch cerrado no está como viene (tamaño u orden).
    pub(crate) fn tab_customized(&self) -> bool {
        let plain = TabSize { quiet: self.tab_size.quiet, ..TabSize::default() };
        self.tab_size != plain
    }

    pub(crate) fn resizing_tab(&self) -> bool {
        self.resizing.is_some()
    }

    /// Si se tiene algo agarrado editando (la ventana recibe el cursor).
    pub(crate) fn edit_grabbing(&self) -> bool {
        self.resizing.is_some_and(|edit| edit.grab.is_some())
    }

    /// Si `item` se muestra en el notch cerrado: no si se sacó, ni mientras
    /// se edita (ahí va su ficha).
    pub(crate) fn rest_shows(&self, item: RestItem) -> bool {
        self.resizing.is_none() && self.tab_size.shows(item)
    }

    /// Pone los tiradores y las fichas. Flotando no hay tab.
    pub(crate) fn start_resize(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.home, Home::Docked { .. }) {
            return;
        }
        let now = Instant::now();
        self.resizing = Some(Edit::default());
        self.strip.set(0.0, now);
        // Sin molestar solo hay ojos, nada que editar: se edita el notch
        // normal, que se ve mientras tanto.
        self.quiet.set(0.0, now);
        cx.notify();
    }

    /// Termina de editar: queda guardado como se dejó y, si estaba sin
    /// molestar, vuelve a los ojos.
    pub(crate) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        self.resizing = None;
        if self.tab_size.quiet {
            self.quiet.set(1.0, Instant::now());
        }
        save(&self.tab_size);
        cx.notify();
    }

    /// Vuelve al notch cerrado de siempre (sin tocar «no molestar»).
    pub(crate) fn reset_tab_size(&mut self, cx: &mut Context<Self>) {
        self.tab_size = TabSize { quiet: self.tab_size.quiet, ..TabSize::default() };
        let now = Instant::now();
        self.rest_w.set(1.0, now);
        self.rest_h.set(1.0, now);
        save(&self.tab_size);
        cx.notify();
    }

    /// La escala de lo que va adentro: la del alto elegido en reposo, la de
    /// siempre con algo abierto.
    pub(crate) fn rest_k(&self, now: Instant) -> f32 {
        if self.fused().is_some() {
            return 1.0;
        }
        // Sin molestar no se escala: lo poco que muestra va a su tamaño.
        let height = lerp(self.rest_h.value(now), 1.0, self.quiet.value(now).clamp(0.0, 1.0));
        lerp(1.0, height, self.rest_calm(now))
    }

    /// El notch cerrado sigue el orden elegido: acoplado y sin fundirse con
    /// la cámara.
    pub(crate) fn ordered_rest(&self) -> bool {
        matches!(self.home, Home::Docked { .. }) && self.fused().is_none() && self.flight.is_none()
    }

    fn rest_vertical(&self) -> bool {
        matches!(self.home, Home::Docked { edge, .. } if edge.is_vertical())
    }

    /// El punto de privacidad del final, si hay algo usando el micrófono o
    /// la cámara (editando no: no hay nada que avisar en una maqueta).
    fn rest_privacy(&self) -> bool {
        self.resizing.is_none() && !self.privacy.uses().is_empty()
    }

    /// Lo que va en el notch cerrado, en orden y con su ancho a escala 1.
    /// `target`: con lo que está entrando ya adentro (para el largo al que
    /// va el tab); si no, con lo que se ve de cada uno ahora.
    fn rest_items(&self, now: Instant, target: bool) -> Vec<(RestItem, f32)> {
        let editing = self.resizing.is_some();
        let vertical = self.rest_vertical();
        let quiet = self.tab_size.quiet;
        let live = if target { self.live.target() } else { self.live.value(now).clamp(0.0, 1.0) };
        self.tab_size
            .order
            .iter()
            .filter_map(|&item| {
                // Editando, todas las fichas del mismo largo (la cara, el suyo):
                // se lee como una fila de piezas iguales, como en Personalizar.
                if editing {
                    return Some((item, if item == RestItem::Face { crate::MARK_SIZE } else { EDIT_TILE }));
                }
                let w = match item {
                    // Sin molestar, los ojos caben en el tab de los ojos.
                    RestItem::Face if quiet => QUIET_LENGTH - PAD * 2.0,
                    RestItem::Face => crate::MARK_SIZE,
                    RestItem::Clock if vertical => {
                        self.left_clock()?;
                        item.sample_w(true)
                    }
                    RestItem::Clock => 15.0 + self.left_clock()?.chars().count() as f32 * 7.0,
                    RestItem::Music if live > 0.01 => crate::LIVE_ART * live,
                    RestItem::Alerts if !quiet && self.tray.chips.any() => {
                        if vertical {
                            crate::tray::chips_height(self.tray.chips)
                        } else {
                            crate::tray::chips_width(self.tray.chips)
                        }
                    }
                    RestItem::Wave if self.bars_shown() => item.sample_w(vertical),
                    _ => return None,
                };
                Some((item, w))
            })
            .collect()
    }

    /// El largo que necesita lo que va en el notch cerrado, al alto elegido.
    pub(crate) fn rest_content_target(&self) -> f32 {
        let quiet = self.tab_size.quiet && self.resizing.is_none();
        let height = if quiet { 1.0 } else { self.tab_size.height };
        content_length(&self.rest_items(Instant::now(), true), height, self.rest_privacy())
    }

    /// Dónde va cada cosa del notch cerrado, si sigue el orden elegido. Se
    /// mide con el tab en reposo (sin la tira ni lo que lo ensancha): al
    /// abrirse algo, nada se corre bajo el cursor.
    pub(crate) fn rest_places(&self, now: Instant) -> Option<RestPlaces> {
        if !self.ordered_rest() {
            return None;
        }
        let Home::Docked { edge, along } = self.home else {
            return None;
        };
        let PillShape::Tab { thick, .. } = self.shape(now) else {
            return None;
        };
        let k = self.rest_k(now);
        let length = self.rest_length(now);
        let area = if self.notch_morph() > 0.0 { self.notch_area(edge) } else { self.work };
        let center = self.place_along(edge, &area, along, length);
        let (items, privacy) = arrange(&self.rest_items(now, false), center - length / 2.0, length, k, self.rest_privacy());
        let band = thick.min(self.band_reach(now));
        let middle = edge.point(&self.work, center, band / 2.0);
        let vertical = edge.is_vertical();
        let cross = if vertical { middle.0 } else { middle.1 };
        Some(RestPlaces { k, vertical, cross, band, items, privacy })
    }

    /// El tab en reposo con sus tiradores, si se está editando.
    fn resize_target(&self, now: Instant) -> Option<(Edge, Rect, f32)> {
        self.resizing?;
        if self.panel_visible() {
            return None;
        }
        match self.shape(now) {
            PillShape::Tab { edge, rect, along, .. } => Some((edge, self.skin_rect(rect), along)),
            PillShape::Disc { .. } => None,
        }
    }

    /// La ficha de algo en el tab, mientras se edita.
    fn tile_rect(&self, places: &RestPlaces, center: f32, w: f32) -> Rect {
        // Dentro de la franja, con aire parejo a los dos lados.
        let across = (places.band - 6.0).max(10.0);
        let long = w + 6.0 * places.k;
        if places.vertical {
            Rect::centered(places.point(center), across, long)
        } else {
            Rect::centered(places.point(center), long, across)
        }
    }

    fn tile_at(&self, p: (f32, f32), now: Instant) -> Option<RestItem> {
        let places = self.rest_places(now)?;
        places
            .items
            .iter()
            .find(|&&(_, center, w)| self.tile_rect(&places, center, w).contains(p, 4.0))
            .map(|(item, ..)| *item)
    }

    /// La tarjeta de edición, si hay orden que cambiar.
    fn edit_card(&self, now: Instant) -> Option<EditCard> {
        let (edge, rect, along) = self.resize_target(now)?;
        if !self.ordered_rest() {
            return None;
        }
        let card = card_rect(edge, &rect, along, &self.work);
        let row_y = card.y + CARD_PAD;
        let first = card.x + CARD_PAD + OUT_LABEL_W + CARD_ROW_GAP;
        let tiles = self
            .tab_size
            .hidden
            .iter()
            .enumerate()
            .map(|(index, &item)| {
                let x = first + index as f32 * (OUT_TILE + OUT_TILE_GAP);
                (item, Rect::new(x, row_y + (CARD_ROW - OUT_TILE) / 2.0, OUT_TILE, OUT_TILE))
            })
            .collect();
        let done = Rect::new(card.right() - CARD_PAD - DONE_W, row_y, DONE_W, CARD_ROW);
        Some(EditCard { rect: card, tiles, done })
    }

    /// Lo que recibe el cursor mientras se edita, además del tab: la tarjeta.
    pub(crate) fn over_edit(&self, p: (f32, f32), now: Instant) -> bool {
        self.edit_card(now).is_some_and(|card| card.rect.contains(p, 4.0))
    }

    /// Un clic editando: en un tirador lo agarra; en una ficha (del tab o de
    /// «Fuera») la agarra; en el resto del tab, termina. Devuelve si el clic
    /// era de esto.
    pub(crate) fn resize_mouse_down(&mut self, p: (f32, f32), now: Instant, cx: &mut Context<Self>) -> bool {
        if self.resizing.is_none() {
            return false;
        }
        let Some((edge, rect, _)) = self.resize_target(now) else {
            // Sin tab (se fue flotando o se abrió el notch): no queda qué editar.
            self.finish_resize(cx);
            return false;
        };
        let card = self.edit_card(now);
        if card.as_ref().is_some_and(|card| card.done.contains(p, 4.0)) {
            self.finish_resize(cx);
            return true;
        }
        let grab = if over_handle(edge, &rect, p) {
            Some(Grab::Corner)
        } else if let Some(item) = self.tile_at(p, now) {
            Some(Grab::Item { item, from_out: false, start: p, moved: false })
        } else {
            match card.as_ref().and_then(|card| card.tiles.iter().find(|(_, tile)| tile.contains(p, 2.0))) {
                Some(&(item, _)) => Some(Grab::Item { item, from_out: true, start: p, moved: false }),
                // En la tarjeta, fuera de una ficha: nada.
                None if card.as_ref().is_some_and(|card| card.rect.contains(p, 0.0)) => return true,
                None => None,
            }
        };
        match grab {
            Some(grab) => self.resizing = Some(Edit { grab: Some(grab) }),
            None if self.over_pill(p, now, 0.0) => self.finish_resize(cx),
            None => return false,
        }
        cx.notify();
        true
    }

    /// Lo agarrado sigue al cursor: el tirador cambia el ancho y el alto; la
    /// ficha, pasado el umbral, se arrastra.
    pub(crate) fn follow_resize(&mut self, cursor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        let (Some(Edit { grab: Some(grab) }), Some(cursor)) = (self.resizing, cursor) else {
            return;
        };
        match grab {
            Grab::Corner => {
                let now = Instant::now();
                let Some((edge, _, along)) = self.resize_target(now) else {
                    return;
                };
                // El tirador va donde está el cursor: el tab crece parejo hacia
                // los dos lados, así que el ancho es el doble de lo que se
                // alejó del centro, y el alto, lo que se alejó del borde.
                // Se edita el notch normal, aunque esté sin molestar.
                let (base_length, base_thick) = (crate::TAB_LENGTH, crate::TAB_THICK);
                let width = clamp(2.0 * (edge.along_of(cursor) - along).abs() / base_length, MIN_W, MAX_W);
                let height = clamp(edge.depth_of(&self.work, cursor) / base_thick, MIN_H, MAX_H);
                if (width - self.tab_size.width).abs() > 0.001 || (height - self.tab_size.height).abs() > 0.001 {
                    self.tab_size.width = width;
                    self.tab_size.height = height;
                    self.rest_w.snap(width);
                    self.rest_h.snap(height);
                    cx.notify();
                }
            }
            Grab::Item { item, from_out, start, moved } => {
                let moved = moved || distance(cursor, start) >= DRAG_THRESHOLD;
                self.resizing = Some(Edit { grab: Some(Grab::Item { item, from_out, start, moved }) });
                if moved {
                    cx.notify();
                }
            }
        }
    }

    /// Se soltó lo agarrado. Un clic en una ficha la saca o la devuelve; un
    /// arrastre la deja donde cayó: en el tab, entre las otras; en «Fuera»,
    /// fuera. Queda guardado y se sigue editando.
    pub(crate) fn resize_release(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Edit { grab: Some(grab) }) = self.resizing else {
            return false;
        };
        self.resizing = Some(Edit::default());
        if let Grab::Item { item, from_out, moved, .. } = grab {
            let now = Instant::now();
            let cursor = self.cursor;
            if !moved {
                if from_out {
                    self.tab_size.place(item, None);
                } else {
                    self.tab_size.remove(item);
                }
            } else if let Some(cursor) = cursor {
                let into_out = self.edit_card(now).is_some_and(|card| card.rect.contains(cursor, 8.0));
                let into_tab = self.over_pill(cursor, now, 16.0);
                if into_out {
                    self.tab_size.remove(item);
                } else if into_tab {
                    let index = self.rest_places(now).map(|places| {
                        let along = places.along(cursor);
                        places.items.iter().filter(|(other, center, _)| *other != item && *center < along).count()
                    });
                    self.tab_size.place(item, index);
                }
            }
        }
        save(&self.tab_size);
        cx.notify();
        true
    }

    /// El marco con sus tiradores, las fichas del tab y la tarjeta de
    /// edición mientras se edita.
    pub(crate) fn resize_element(&self, now: Instant) -> Option<AnyElement> {
        let (edge, rect, _) = self.resize_target(now)?;
        let ink: Hsla = rgb(0xf0f0ea).into();
        let on_ink: Hsla = rgb(0x1a1a18).into();
        let skin = crate::appearance::current().skin.mid();
        let lift = || {
            vec![
                gpui::BoxShadow {
                    color: gpui::black().opacity(0.28),
                    offset: gpui::point(px(0.), px(6.)),
                    blur_radius: px(20.),
                    spread_radius: px(0.),
                },
                gpui::BoxShadow {
                    color: gpui::black().opacity(0.2),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                },
            ]
        };
        let dragged = match self.resizing.and_then(|edit| edit.grab) {
            Some(Grab::Item { item, moved: true, .. }) => Some(item),
            _ => None,
        };
        let cursor = self.cursor;
        // Los tiradores: redondos, con un aro de la piel para que se lean
        // sobre cualquier fondo, y algo más grandes bajo el cursor.
        let handles = outer_corners(edge, &rect).map(|(x, y)| {
            let hot = cursor.is_some_and(|c| (c.0 - x).abs() <= HANDLE && (c.1 - y).abs() <= HANDLE);
            let size = if hot { HANDLE + 2.0 } else { HANDLE };
            div()
                .absolute()
                .left(px(x - rect.x - size / 2.0))
                .top(px(y - rect.y - size / 2.0))
                .size(px(size))
                .rounded_full()
                .bg(ink)
                .border_2()
                .border_color(skin)
                .shadow(lift())
        });
        let [tl, tr, br, bl] = crate::tab_corners(edge, crate::TAB_RADIUS.min(rect.w.min(rect.h) / 2.0));
        let frame = div()
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .rounded_tl(px(tl))
            .rounded_tr(px(tr))
            .rounded_br(px(br))
            .rounded_bl(px(bl))
            .border_1()
            .border_color(ink.opacity(0.6))
            .children(handles);
        let tile = |rect: Rect, item: RestItem, alpha: f32| {
            let icon = (rect.h * 0.58).min(rect.w * 0.7).min(16.0);
            div()
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(rect.w))
                .h(px(rect.h))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(INNER_RADIUS.min(rect.w.min(rect.h) / 2.0)))
                .bg(ink.opacity(0.1))
                .border_1()
                .border_color(ink.opacity(0.22))
                .opacity(alpha)
                .when(item != RestItem::Face, |el| {
                    el.child(svg().path(item.icon()).size(px(icon)).text_color(ink))
                })
        };
        // Las fichas del tab: la cara la dibuja la marca; encima va su marco.
        let mut tiles: Vec<AnyElement> = Vec::new();
        if let Some(places) = self.rest_places(now) {
            for &(item, center, w) in &places.items {
                let alpha = if dragged == Some(item) { 0.3 } else { 1.0 };
                let mut el = tile(self.tile_rect(&places, center, w), item, alpha);
                if item == RestItem::Face {
                    el = el.bg(ink.opacity(0.0));
                }
                tiles.push(el.into_any_element());
            }
        }
        let card = self.edit_card(now).map(|card| {
            let local = |r: Rect| Rect::new(r.x - card.rect.x, r.y - card.rect.y, r.w, r.h);
            let empty = card.tiles.is_empty();
            let done = local(card.done);
            let done_hot = cursor.is_some_and(|c| card.done.contains(c, 4.0));
            let first = CARD_PAD + OUT_LABEL_W + CARD_ROW_GAP;
            div()
                .absolute()
                .left(px(card.rect.x))
                .top(px(card.rect.y))
                .w(px(card.rect.w))
                .h(px(card.rect.h))
                .rounded(px(CARD_RADIUS))
                .bg(skin)
                .shadow(lift())
                .font_family(crate::fonts::UI)
                .text_color(ink)
                // «Fuera», lo sacado (o dónde soltarlo) y «Listo», en una fila.
                .child(
                    div()
                        .absolute()
                        .left(px(CARD_PAD))
                        .top(px(CARD_PAD))
                        .w(px(OUT_LABEL_W))
                        .h(px(CARD_ROW))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(ink.opacity(0.72))
                        .child(t("pill.closed.out")),
                )
                .when(empty, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left(px(first))
                            .top(px(CARD_PAD))
                            .w(px(done.x - first - CARD_ROW_GAP))
                            .h(px(CARD_ROW))
                            .flex()
                            .items_center()
                            .px(px(8.))
                            .rounded(px(INNER_RADIUS))
                            .border_1()
                            .border_color(ink.opacity(0.16))
                            .text_size(px(11.))
                            .text_color(ink.opacity(0.5))
                            .truncate()
                            .child(t("pill.closed.dropHere")),
                    )
                })
                .children(card.tiles.iter().map(|&(item, r)| tile(local(r), item, if dragged == Some(item) { 0.3 } else { 1.0 })))
                .child(
                    div()
                        .absolute()
                        .left(px(done.x))
                        .top(px(done.y))
                        .w(px(done.w))
                        .h(px(done.h))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(INNER_RADIUS))
                        .bg(ink.opacity(if done_hot { 1.0 } else { 0.9 }))
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(on_ink)
                        .child(t("pill.settings.tabSizeDone")),
                )
                // La ayuda, debajo y dentro de la tarjeta: nunca encima del tab.
                .child(
                    div()
                        .absolute()
                        .left(px(CARD_PAD))
                        .top(px(CARD_PAD + CARD_ROW + CARD_ROW_GAP))
                        .w(px(CARD_W - CARD_PAD * 2.0))
                        .h(px(CARD_HINT))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(ink.opacity(0.6))
                        .child(t("pill.tabSize.hint")),
                )
        });
        // La ficha arrastrada, bajo el cursor.
        let ghost = dragged.zip(cursor).map(|(item, (x, y))| {
            tile(Rect::centered((x, y), OUT_TILE, OUT_TILE), item, 0.95).bg(skin).shadow(lift())
        });
        Some(
            div()
                .absolute()
                .inset_0()
                .child(frame)
                .children(tiles)
                .children(card)
                .children(ghost)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_archivo_viejo_o_raro_da_el_notch_de_siempre() {
        let size: TabSize = serde_json::from_str("{\"scale\": 0.7}").unwrap();
        assert_eq!(size.normalized(), TabSize::default());
        let odd: TabSize =
            serde_json::from_str("{\"width\": 9, \"height\": 0.1, \"order\": [\"face\", \"face\"], \"hidden\": [\"face\", \"music\"]}")
                .unwrap();
        let odd = odd.normalized();
        assert_eq!((odd.width, odd.height), (MAX_W, MIN_H));
        // Cada cosa una vez; la cara no queda fuera.
        assert_eq!(odd.hidden, vec![RestItem::Music]);
        assert_eq!(odd.order.iter().filter(|item| **item == RestItem::Face).count(), 1);
        assert_eq!(odd.order.len() + odd.hidden.len(), RestItem::ALL.len());
    }

    #[test]
    fn mover_y_sacar_cosas() {
        let mut size = TabSize::default();
        size.place(RestItem::Wave, Some(0));
        assert_eq!(size.order[0], RestItem::Wave);
        assert!(size.remove(RestItem::Music));
        assert!(!size.shows(RestItem::Music));
        assert!(!size.remove(RestItem::Face));
        size.place(RestItem::Music, None);
        assert!(size.shows(RestItem::Music));
        assert_eq!(*size.order.last().unwrap(), RestItem::Music);
    }

    #[test]
    fn el_aire_se_reparte_parejo_y_una_sola_va_al_medio() {
        let (one, _) = arrange(&[(RestItem::Face, 32.0)], 100.0, 124.0, 1.0, false);
        assert_eq!(one[0].1, 162.0);
        let items = [(RestItem::Music, 24.0), (RestItem::Face, 32.0), (RestItem::Wave, 22.0)];
        let (three, dot) = arrange(&items, 0.0, 212.0, 1.0, true);
        assert!(three[0].1 < three[1].1 && three[1].1 < three[2].1);
        let dot = dot.unwrap();
        assert!(three[2].1 + 11.0 < dot - 4.0, "la onda no pisa el punto");
        assert!(dot + 4.0 <= 212.0);
        // Lo justo: cabe todo sin aire de sobra.
        let fit = content_length(&items, 0.5, true);
        let (tight, _) = arrange(&items, 0.0, fit, 0.5, true);
        assert!((tight[0].1 - tight[0].2 / 2.0 - PAD * 0.5).abs() < 0.01);
    }

    #[test]
    fn la_tarjeta_no_pisa_el_tab_ni_se_sale_de_la_pantalla() {
        let work = Rect::new(0.0, 0.0, 1920.0, 1040.0);
        // En el costado derecho, casi abajo: al lado del tab y dentro.
        let tab = Rect::new(1880.0, 940.0, 40.0, 90.0);
        let card = card_rect(Edge::Right, &tab, 985.0, &work);
        assert!(card.right() + CARD_GAP <= tab.x + 0.01, "no pisa el tab");
        assert!(card.bottom() <= work.bottom() - CARD_MARGIN + 0.01 && card.y >= work.y);
        // Arriba, pegado a la esquina izquierda: bajo el tab y sin salirse.
        let top = Rect::new(0.0, 0.0, 124.0, 40.0);
        let card = card_rect(Edge::Top, &top, 62.0, &work);
        assert!(card.y >= top.bottom() + CARD_GAP - 0.01);
        assert!(card.x >= work.x + CARD_MARGIN - 0.01);
        // A la izquierda y abajo, del otro lado.
        let left = card_rect(Edge::Left, &Rect::new(0.0, 400.0, 40.0, 124.0), 462.0, &work);
        assert!(left.x >= 40.0 + CARD_GAP - 0.01);
        let bottom = card_rect(Edge::Bottom, &Rect::new(900.0, 1000.0, 124.0, 40.0), 962.0, &work);
        assert!(bottom.bottom() <= 1000.0 - CARD_GAP + 0.01);
    }

    #[test]
    fn los_tiradores_miran_al_escritorio() {
        let rect = Rect::new(100.0, 0.0, 124.0, 40.0);
        assert_eq!(outer_corners(Edge::Top, &rect), [(100.0, 40.0), (224.0, 40.0)]);
        assert!(over_handle(Edge::Top, &rect, (222.0, 43.0)));
        assert!(!over_handle(Edge::Top, &rect, (160.0, 40.0)));
        let side = Rect::new(0.0, 300.0, 40.0, 124.0);
        assert_eq!(outer_corners(Edge::Left, &side), [(40.0, 300.0), (40.0, 424.0)]);
    }
}
