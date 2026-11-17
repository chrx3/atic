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

use std::f32::consts::{FRAC_PI_4, PI};
use std::time::Instant;

use gpui::{point, px, size, BoxShadow, Bounds, Corners, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, Styled, Window};
use gpui_m3::{ShapeName, Spring};

use crate::anim::lerp;
use crate::pill_look::{PillTokens, Track};
use crate::pill_tools::StripId;
use crate::tweak::num;

const CORE: f32 = 56.0;
const ITEM: f32 = 48.0;
/// Del centro del núcleo al centro de cada gajo (crece si no caben).
const RING: f32 = 80.0;
/// Lo mínimo entre dos gajos vecinos.
const GAP: f32 = 8.0;
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

/// Lado de los gajos y radio del anillo para `count` gajos. Si no caben en la
/// caja de la rueda (`WHEEL_BOX`) con su separación, se achican.
pub(crate) fn layout(count: usize) -> (f32, f32) {
    let item = num("pill.m3.wheel.item", ITEM);
    let gap = num("pill.m3.wheel.gap", GAP);
    let ring = num("pill.m3.wheel.ring", RING);
    let sin = (PI / count.max(2) as f32).sin();
    let room = crate::WHEEL_BOX / 2.0 - 4.0;
    let r = ring.max((item + gap) / (2.0 * sin));
    if r + item / 2.0 <= room {
        return (item, r);
    }
    // El lado con el que la cuerda entre vecinos deja `gap` y el gajo cabe:
    // lado (1 + sin) = 2 sin room - gap.
    let side = ((2.0 * sin * room - gap) / (1.0 + sin)).clamp(28.0, item);
    (side, room - side / 2.0)
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
    /// El anillo a la vista: radio y lado de los gajos.
    pub ring: f32,
    pub item_side: f32,
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
        let core_full = num("pill.m3.wheel.core", CORE);
        let k = motion.core.value;
        let core = (lerp(place.source.0, place.center.0, k), lerp(place.source.1, place.center.1, k));
        let core_side = lerp((place.source_r * 2.0).min(core_full), core_full, k.max(0.0));
        let core_alpha = (k * 3.0).clamp(0.0, 1.0);
        let grow = num("pill.m3.wheel.hoverGrow", HOVER_GROW);

        let place_item = |slot: StripId, index: usize, count: usize, progress: f32, hover: f32| {
            let (side, ring) = layout(count);
            let angle = crate::wheel_angle(index, count);
            let reach = ring * progress.max(0.0);
            let shown = progress.clamp(0.0, 1.0);
            ItemPlace {
                slot,
                index: None,
                center: (core.0 + angle.cos() * reach, core.1 + angle.sin() * reach),
                side: side * lerp(0.4, 1.0, shown) * (1.0 + grow * hover.max(0.0)),
                shown,
                alpha: (progress * 1.6).clamp(0.0, 1.0),
            }
        };
        let mut items: Vec<ItemPlace> = motion
            .leaving
            .iter()
            .map(|leaving| place_item(leaving.slot, leaving.index, leaving.count, leaving.progress.value, 0.0))
            .collect();
        let slots = self.wheel_slots();
        for (index, (&slot, track)) in slots.iter().zip(&motion.items).enumerate() {
            let hover = motion.hover.get(index).map_or(0.0, |track| track.value);
            let mut item = place_item(slot, index, slots.len(), track.value, hover);
            item.index = Some(index);
            items.push(item);
        }
        let (item_side, ring) = layout(slots.len());
        Geometry {
            core,
            core_side,
            core_alpha,
            ring,
            item_side,
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
        let place = self.wheel_place(now);
        let geometry = self.wheel_m3_geometry(&place);
        let distance = |c: (f32, f32)| (p.0 - c.0).hypot(p.1 - c.1);
        distance(place.center) <= geometry.ring + geometry.item_side / 2.0 + NEAR_SLACK
            || distance(geometry.core) <= geometry.core_side / 2.0 + NEAR_SLACK
    }

    /// El nombre del gajo bajo el cursor, como tooltip simple de M3
    /// (`inverse_surface`), justo debajo del anillo.
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
        let place = self.wheel_place(now);
        let geometry = self.wheel_m3_geometry(&place);
        let top = place.center.1 + geometry.ring + geometry.item_side / 2.0 + 8.0;
        Some(
            div()
                .absolute()
                .left(px(place.center.0 - W / 2.0))
                .top(px(top))
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
