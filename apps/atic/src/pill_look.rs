//! El aspecto de la pill según su estilo (`appearance::PillStyle`).
//!
//! La pill se pinta a mano (`Frame::paint_base` y `paint_top` en `main.rs`):
//! los componentes de gpui-m3 no entran en ese lienzo. Lo que cambia entre un
//! estilo y otro vive en `PillTokens`, que `frame` resuelve una vez por cuadro:
//!
//! - **Atic**: las constantes de siempre, tal cual (`PillTokens::atic`).
//! - **M3 Expressive** («toolbar flotante»): los roles de un `Scheme` oscuro
//!   sembrado con la piel elegida (la de Atic usa `baseline_dark`). La tira
//!   es un grupo de botones conectado (`group_layout`, `group_radii`): cada
//!   ficha con su fondo tonal, la del cursor más ancha (sus vecinas ceden, el
//!   largo no cambia), más cuadrada al presionar y la elegida rellena y
//!   redonda. La cara lleva un contenedor cuya forma dice el estado
//!   (`mark_shape`).
//!
//! `LookState` guarda lo que se anima solo en Expressive (el ancho y la
//! elegida de cada ficha, el radio al presionar y la forma de la cara), con
//! resortes de gpui-m3; `Pill::advance` lo avanza. En Atic no hace nada.

use std::sync::Mutex;
use std::time::Instant;

use gpui::{point, px, rgb, BoxShadow, Hsla, PathBuilder, Window};
use gpui_m3::shapes::LOADING_SEQUENCE;
use gpui_m3::{Elevation, Morph, Scheme, SchemeVariant, ShapeName, Spring};

use crate::anim::{SpringKind, SpringTrack};
use crate::appearance::{PillStyle, Skin};
use crate::pill_tools::STRIP_MAX;
use crate::tweak::num;

/// Opacidad de la capa de estado al pasar el cursor y al presionar (M3).
const HOVER_LAYER: f32 = 0.08;
const PRESS_LAYER: f32 = 0.10;
/// La tira como grupo de botones conectado (M3 Expressive): cada ficha es un
/// segmento de este grueso, con esquinas chicas entre fichas y redondas en los
/// extremos del grupo.
const GROUP_THICK: f32 = 32.0;
const GROUP_INNER: f32 = 8.0;
/// El radio de todas sus esquinas mientras se presiona.
const GROUP_PRESSED: f32 = 4.0;
/// Lo que se ensancha la ficha con el cursor encima; lo ceden sus vecinas.
const GROUP_GROW: f32 = 12.0;
/// Íconos de la tira en Expressive (en Atic, `TOOL_ICON`).
const M3_ICON: f32 = 20.0;
/// El contenedor de la cara.
pub(crate) const FACE_BOX: f32 = 32.0;
/// Lo que dura el «hecho» (Cookie4) antes de volver a reposo.
const DONE_MS: u64 = 420;
/// La cara «procesando» recorre `LOADING_SEQUENCE` a estos cuadros por
/// segundo (como `LoadingIndicator`): se lee continuo y no redibuja de más.
const LOADING_FPS: f32 = 15.0;

/// Los colores, tamaños y sombras de la pill en un estilo.
#[derive(Clone)]
pub(crate) struct PillTokens {
    /// La piel para los contornos: lisa o en degradado.
    pub fill: Skin,
    /// La piel en un solo color (lo que no cabe en un degradado).
    pub skin: Hsla,
    /// La cara, sin contenedor.
    pub face: Hsla,
    /// Íconos de la tira: apagado y con el cursor encima.
    pub off: Hsla,
    pub on: Hsla,
    /// Cuánto crece el ícono con el cursor encima (0 en Expressive: ahí lo
    /// dice la capa de estado).
    pub hover_grow: f32,
    pub icon: f32,
    /// Color de la capa de estado (sin opacidad); `None` en Atic.
    pub layer: Option<Hsla>,
    /// El fondo de cada ficha del grupo de la tira; `None` en Atic.
    pub segment: Option<Hsla>,
    /// La ficha elegida: contenedor y contenido.
    pub chosen: Option<(Hsla, Hsla)>,
    /// El reloj de la grabación en un contenedor: contenedor y contenido.
    pub recording: Option<(Hsla, Hsla)>,
    /// Otro reloj del tab (Pomodoro, Computadores) en un contenedor.
    pub clock: Option<(Hsla, Hsla)>,
    /// El rótulo de la tira como tooltip de M3: fondo y texto.
    pub tooltip: Option<(Hsla, Hsla)>,
    pub scheme: Option<Scheme>,
    shadow: Shadow,
}

#[derive(Clone)]
enum Shadow {
    /// `pill_shadow` de siempre.
    Atic,
    Elevation(Vec<BoxShadow>),
}

impl PillTokens {
    /// Los del estilo elegido en Apariencia.
    pub fn current() -> Self {
        let look = crate::appearance::current();
        match look.pill {
            PillStyle::Atic => Self::atic(look.skin),
            PillStyle::Expressive => Self::expressive(look.skin),
        }
    }

    /// El estilo de siempre: exactamente las constantes de antes de que
    /// hubiera estilos (`Palette::dark`, `pill_shadow`, `TOOL_ICON`).
    pub fn atic(skin: Skin) -> Self {
        let text: Hsla = rgb(0xf0f0ea).into();
        Self {
            fill: skin,
            skin: skin.mid(),
            face: text,
            off: rgb(0xa8a89e).into(),
            on: text,
            hover_grow: 0.14,
            icon: crate::TOOL_ICON,
            layer: None,
            segment: None,
            chosen: None,
            recording: None,
            clock: None,
            tooltip: None,
            scheme: None,
            shadow: Shadow::Atic,
        }
    }

    /// Material 3 Expressive, desde un esquema oscuro sembrado con la piel.
    pub fn expressive(skin: Skin) -> Self {
        let s = scheme_for(skin);
        Self {
            fill: Skin::Solid { color: to_hex(s.surface_container) },
            skin: s.surface_container,
            face: s.on_surface,
            off: s.on_surface_variant,
            on: s.on_surface,
            hover_grow: 0.0,
            icon: num("pill.m3.icon", M3_ICON),
            layer: Some(s.on_surface),
            segment: Some(s.surface_container_high),
            chosen: Some((s.secondary_container, s.on_secondary_container)),
            recording: Some((s.error_container, s.on_error_container)),
            clock: Some((s.surface_container_highest, s.on_surface)),
            tooltip: Some((s.inverse_surface, s.inverse_on_surface)),
            scheme: Some(s),
            shadow: Shadow::Elevation(Elevation::Level3.shadow(&s)),
        }
    }

    /// La sombra de la pill, de los paneles y de la rueda, a `alpha`.
    pub fn shadow(&self, alpha: f32) -> Vec<BoxShadow> {
        match &self.shadow {
            Shadow::Atic => vec![crate::pill_shadow(alpha)],
            Shadow::Elevation(shadows) => shadows
                .iter()
                .map(|shadow| BoxShadow { color: shadow.color.opacity(alpha), ..shadow.clone() })
                .collect(),
        }
    }

    /// La opacidad de la capa de estado con `hover` y `press` (0..1).
    pub fn layer_alpha(hover: f32, press: f32) -> f32 {
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        num("pill.m3.hoverLayer", HOVER_LAYER) * hover * (1.0 - press) + num("pill.m3.pressLayer", PRESS_LAYER) * press
    }

    /// El color de una ficha del grupo: su fondo tonal, el de la elegida
    /// según `chosen` y encima la capa de estado. `None` en Atic.
    pub fn segment_color(&self, hover: f32, press: f32, chosen: f32) -> Option<Hsla> {
        let base = self.segment?;
        let fill = match self.chosen {
            Some((container, _)) => blend(base, container, chosen),
            None => base,
        };
        Some(match self.layer {
            Some(layer) => blend(fill, layer, Self::layer_alpha(hover, press)),
            None => fill,
        })
    }
}

// --- La tira como grupo conectado (Expressive) ----------------------------

/// El grueso de cada ficha del grupo.
pub(crate) fn group_thick() -> f32 {
    num("pill.m3.group.thick", GROUP_THICK)
}

/// Lo que se ensancha la ficha con el cursor encima.
pub(crate) fn group_grow() -> f32 {
    num("pill.m3.group.grow", GROUP_GROW)
}

/// Dónde empieza cada ficha y cuánto mide a lo largo, desde el principio de
/// la tira abierta del todo. `widen` (0..1 por ficha; el resorte puede pasarse
/// un poco) dice cuánto se ensancha cada una: crece `grow * widen` y sus
/// vecinas ceden lo mismo, mitad y mitad (todo la única vecina en un extremo).
/// El largo del grupo no cambia nunca: el tab no se mueve con el hover.
pub(crate) fn group_layout(count: usize, base: f32, gap: f32, grow: f32, widen: &[f32]) -> Vec<(f32, f32)> {
    let mut widths = vec![base; count];
    // Que una vecina que cede por los dos lados no quede más angosta que el ícono.
    let grow = grow.clamp(0.0, base * 0.3);
    if count > 1 && grow > 0.0 {
        for index in 0..count {
            let extra = grow * widen.get(index).copied().unwrap_or(0.0).clamp(-0.25, 1.25);
            if extra == 0.0 {
                continue;
            }
            let left = index.checked_sub(1);
            let right = (index + 1 < count).then_some(index + 1);
            let share = extra / (left.is_some() as u8 + right.is_some() as u8) as f32;
            widths[index] += extra;
            for neighbour in left.into_iter().chain(right) {
                widths[neighbour] -= share;
            }
        }
    }
    let mut start = 0.0;
    widths
        .into_iter()
        .map(|width| {
            let segment = (start, width);
            start += width + gap;
            segment
        })
        .collect()
}

/// La ficha bajo `along` (medido desde el principio de la tira) con la tira
/// abierta en `open`. Cada una se lleva la mitad del espacio hasta sus
/// vecinas: entre dos no queda hueco muerto. Con todas iguales es la cuenta
/// de siempre (centro ± (ancho + espacio) / 2).
pub(crate) fn group_slot_at(layout: &[(f32, f32)], gap: f32, open: f32, along: f32) -> Option<usize> {
    layout
        .iter()
        .position(|&(start, width)| (along - (start + width / 2.0) * open).abs() <= (width + gap) / 2.0)
}

/// Los radios de una ficha del grupo, al principio y al final a lo largo:
/// redondas en los extremos del grupo y chicas entre fichas; la elegida,
/// redonda entera (`chosen`); al presionar, todas más cuadradas (`press`).
pub(crate) fn group_radii(index: usize, count: usize, thick: f32, chosen: f32, press: f32) -> (f32, f32) {
    let full = thick / 2.0;
    let inner = num("pill.m3.group.inner", GROUP_INNER).min(full);
    let pressed = num("pill.m3.group.pressed", GROUP_PRESSED).min(full);
    let side = |outer: bool| {
        let rest = if outer { full } else { inner };
        let rest = rest + (full - rest) * chosen.clamp(0.0, 1.0);
        rest + (pressed - rest) * press.clamp(0.0, 1.0)
    };
    (side(index == 0), side(index + 1 == count))
}

/// Una ficha del grupo de la tira, lista para pintar. `corners`: arriba a la
/// izquierda, arriba a la derecha, abajo a la derecha, abajo a la izquierda.
pub(crate) struct StripSegment {
    pub rect: crate::geometry::Rect,
    pub corners: [f32; 4],
    pub color: Hsla,
}

/// `from_seed` hace cuentas de color (HCT): se calcula una vez por piel.
static SCHEME: Mutex<Option<(Skin, Scheme)>> = Mutex::new(None);

fn scheme_for(skin: Skin) -> Scheme {
    if let Ok(cached) = SCHEME.lock() {
        if let Some((seen, scheme)) = *cached {
            if seen == skin {
                return scheme;
            }
        }
    }
    let seed = skin.mid();
    let scheme = match skin {
        Skin::Atic => Scheme::baseline_dark(),
        // Negro, gris o blanco no tienen matiz: con TonalSpot salían marrones
        // o morados sacados de la nada. Sin color, el esquema va en grises.
        _ if seed.s < 0.12 || seed.l < 0.06 || seed.l > 0.94 => {
            Scheme::from_seed_variant(seed, true, SchemeVariant::Monochrome, 0.)
        }
        _ => Scheme::from_seed(seed, true),
    };
    if let Ok(mut cached) = SCHEME.lock() {
        *cached = Some((skin, scheme));
    }
    scheme
}

fn to_hex(color: Hsla) -> u32 {
    let c = gpui::Rgba::from(color);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(c.r) << 16) | (byte(c.g) << 8) | byte(c.b)
}

/// Mezcla en sRGB: `t` 0 es `from`, 1 es `to`.
fn blend(from: Hsla, to: Hsla, t: f32) -> Hsla {
    gpui_m3::theme::mix(to, from, t.clamp(0.0, 1.0))
}

// --- La cara ----------------------------------------------------------------

/// Lo que dice la cara con su contenedor, de lo más a lo menos importante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkState {
    Rest,
    Hover,
    /// El notch abierto.
    Open,
    Dictating,
    Recording,
    /// Algo trabaja sin que haya que mirarlo: se transcribe, se resume o un
    /// agente está en su turno. La forma recorre `LOADING_SEQUENCE`.
    Processing,
    /// Hay una actualización.
    Attention,
    /// Algo terminó (el dictado se pegó, la grabación se detuvo, terminó una
    /// transcripción, se guardó una captura).
    Done,
}

/// El rol de color del contenedor de la cara.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Sin contenedor.
    Hidden,
    Highest,
    Secondary,
    Primary,
    Error,
    Tertiary,
}

impl Role {
    /// Contenedor y contenido (la cara encima).
    fn colors(self, s: &Scheme) -> (Hsla, Hsla) {
        match self {
            Role::Hidden => (s.surface_container_highest.opacity(0.0), s.on_surface),
            Role::Highest => (s.surface_container_highest, s.on_surface),
            Role::Secondary => (s.secondary_container, s.on_secondary_container),
            Role::Primary => (s.primary_container, s.on_primary_container),
            Role::Error => (s.error_container, s.on_error_container),
            Role::Tertiary => (s.tertiary_container, s.on_tertiary_container),
        }
    }
}

/// El estado de la cara con lo que pasa ahora, en orden de importancia.
pub(crate) fn mark_state(
    recording: bool,
    dictating: bool,
    processing: bool,
    done: bool,
    attention: bool,
    open: bool,
    hover: bool,
) -> MarkState {
    if recording {
        MarkState::Recording
    } else if dictating {
        MarkState::Dictating
    } else if processing {
        MarkState::Processing
    } else if done {
        MarkState::Done
    } else if attention {
        MarkState::Attention
    } else if open {
        MarkState::Open
    } else if hover {
        MarkState::Hover
    } else {
        MarkState::Rest
    }
}

/// La forma y el color del contenedor de la cara en cada estado.
pub(crate) fn mark_shape(state: MarkState) -> (ShapeName, Role) {
    match state {
        MarkState::Rest => (ShapeName::Circle, Role::Hidden),
        MarkState::Hover => (ShapeName::Cookie9, Role::Highest),
        MarkState::Open => (ShapeName::Square, Role::Secondary),
        MarkState::Dictating => (ShapeName::Sunny, Role::Primary),
        MarkState::Recording => (ShapeName::SoftBurst, Role::Error),
        // La primera de `LOADING_SEQUENCE`: desde ahí sigue la secuencia
        // (`LookState::mark_points`). Con movimiento reducido, quieta en Circle.
        MarkState::Processing => (LOADING_SEQUENCE[0], Role::Secondary),
        MarkState::Attention => (ShapeName::Clover4, Role::Tertiary),
        MarkState::Done => (ShapeName::Cookie4, Role::Primary),
    }
}

/// El contenedor de la cara en este cuadro: sus puntos en la caja unidad
/// (0..1) y su color. La cara encima va en `ink`.
#[derive(Clone)]
pub(crate) struct MarkPaint {
    pub points: Vec<(f32, f32)>,
    pub color: Hsla,
    pub ink: Hsla,
}

impl MarkPaint {
    /// Pinta el contenedor centrado en `center`, de lado `side`.
    pub fn paint(&self, window: &mut Window, center: (f32, f32), side: f32, alpha: f32) {
        let color = self.color.opacity(alpha);
        if color.a <= 0.01 {
            return;
        }
        let points: Vec<_> = self
            .points
            .iter()
            .map(|&(x, y)| point(px(center.0 + (x - 0.5) * side), px(center.1 + (y - 0.5) * side)))
            .collect();
        let mut builder = PathBuilder::fill();
        builder.add_polygon(&points, true);
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

/// El lado del contenedor para una cara de `mark_size` (la cara se achica en
/// el notch cerrado según `tab_size`).
pub(crate) fn face_box(mark_size: f32) -> f32 {
    num("pill.m3.faceBox", FACE_BOX) * mark_size / crate::MARK_SIZE
}

// --- Lo que se anima --------------------------------------------------------

/// Un valor que sigue a su objetivo con un resorte de gpui-m3: se puede
/// redirigir a mitad de camino sin perder la velocidad.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Track {
    pub value: f32,
    velocity: f32,
    target: f32,
}

impl Track {
    pub const fn at(value: f32) -> Self {
        Self { value, velocity: 0.0, target: value }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap(&mut self, value: f32) {
        *self = Self::at(value);
    }

    pub fn moving(&self) -> bool {
        (self.value - self.target).abs() > 1e-3 || self.velocity.abs() > 1e-2
    }

    fn step(&mut self, spring: Spring, dt: f32) {
        if !self.moving() {
            self.snap(self.target);
            return;
        }
        let (x, v) = spring.solve(self.value - self.target, self.velocity, dt);
        self.value = self.target + x;
        self.velocity = v;
        if !self.moving() {
            self.snap(self.target);
        }
    }
}

/// Lo que mira `LookState::update` en cada cuadro.
#[derive(Clone, Copy)]
pub(crate) struct LookInputs {
    /// La ficha de la herramienta cuyo vistazo o panel está abierto.
    pub chosen: Option<usize>,
    /// La ficha apretada.
    pub pressed: Option<usize>,
    /// Hacia dónde va el ancho de cada ficha del grupo (1: con el cursor
    /// encima o con su vistazo abierto). Es el objetivo, el mismo que usa el
    /// hit-test (`Pill::strip_layout`).
    pub widen: [f32; STRIP_MAX],
    pub recording: bool,
    pub dictating: bool,
    /// Se transcribe, se resume o un agente está en su turno.
    pub processing: bool,
    /// Lo que estaba en marcha terminó mal: sin «hecho».
    pub failed: bool,
    pub attention: bool,
    pub open: bool,
    pub hover: bool,
    pub reduced: bool,
}

impl LookInputs {
    /// Nada pasa.
    pub fn idle() -> Self {
        Self {
            chosen: None,
            pressed: None,
            widen: [0.0; STRIP_MAX],
            recording: false,
            dictating: false,
            processing: false,
            failed: false,
            attention: false,
            open: false,
            hover: false,
            reduced: false,
        }
    }
}

pub(crate) struct LookState {
    /// El último cuadro en que algo se movía (`None`: todo quieto).
    last: Option<Instant>,
    /// Por ficha del grupo: cuánto se ensanchó (`Spatial`, con su leve
    /// sobrepaso) y cuánto es la elegida (`Effects`).
    widen: [SpringTrack; STRIP_MAX],
    chosen: [SpringTrack; STRIP_MAX],
    /// Cuánto está apretada `pressed_slot`.
    pub press: Track,
    pub pressed_slot: Option<usize>,
    /// Lo que trabaja fuera de la pill (Reuniones, agentes); lo pone
    /// `Pill::tick`: si está en marcha y si acaba de terminar mal (se borra
    /// al usarlo).
    pub work: (bool, bool),
    reduced: bool,
    mark: MarkMorph,
}

struct MarkMorph {
    state: MarkState,
    /// Los puntos de donde parte (la forma que se veía al cambiar) y a cuál va.
    from: Vec<(f32, f32)>,
    to: ShapeName,
    shape_t: Track,
    /// Colores de donde parte y rol al que va.
    from_color: Hsla,
    from_ink: Hsla,
    role: Role,
    color_t: Track,
    /// Cuándo se acaba el «hecho».
    done_until: Option<Instant>,
    was_busy: bool,
    /// Desde cuándo procesa y el último cuadro de la secuencia que se pintó.
    loading_since: Option<Instant>,
    loading_seen: Option<u32>,
}

impl Default for LookState {
    fn default() -> Self {
        let rest = ShapeName::Circle;
        Self {
            last: None,
            widen: std::array::from_fn(|_| SpringTrack::new(0.0, SpringKind::Spatial)),
            chosen: std::array::from_fn(|_| SpringTrack::new(0.0, SpringKind::Effects)),
            press: Track::at(0.0),
            pressed_slot: None,
            work: (false, false),
            reduced: false,
            mark: MarkMorph {
                state: MarkState::Rest,
                from: rest.unit_points().to_vec(),
                to: rest,
                shape_t: Track::at(1.0),
                from_color: gpui::transparent_black(),
                from_ink: rgb(0xf0f0ea).into(),
                role: Role::Hidden,
                color_t: Track::at(1.0),
                done_until: None,
                was_busy: false,
                loading_since: None,
                loading_seen: None,
            },
        }
    }
}

/// El cuadro de la secuencia de carga (a `LOADING_FPS`) a `now`.
fn loading_frame(since: Instant, now: Instant) -> u32 {
    (now.saturating_duration_since(since).as_secs_f32() * num("pill.m3.loading.fps", LOADING_FPS).max(1.0)) as u32
}

/// Los puntos de la forma de carga en el cuadro `frame`, en la caja unidad.
/// Como `LoadingIndicator` de gpui-m3: una forma nueva cada 650 ms con un
/// resorte con rebote, un cuarto de vuelta en cada una y encima una vuelta
/// entera cada 4,666 s.
fn loading_points(frame: u32) -> Vec<(f32, f32)> {
    const SEGMENT: f32 = 0.65;
    const SPIN_PERIOD: f32 = 4.666;
    let t = frame as f32 / num("pill.m3.loading.fps", LOADING_FPS).max(1.0);
    let n = LOADING_SEQUENCE.len();
    let index = (t / SEGMENT) as usize;
    let local = t - index as f32 * SEGMENT;
    let progress = Spring { stiffness: 200.0, damping_ratio: 0.6 }.step(local);
    let morph = Morph {
        from: LOADING_SEQUENCE[index % n],
        to: LOADING_SEQUENCE[(index + 1) % n],
        t: progress,
        rotation: ((index % 4) as f32 + progress) * std::f32::consts::FRAC_PI_2
            + (t / SPIN_PERIOD).fract() * std::f32::consts::TAU,
        toward: None,
    };
    let unit = gpui::Bounds { origin: point(px(0.0), px(0.0)), size: gpui::size(px(1.0), px(1.0)) };
    morph.points(unit).into_iter().map(|p| (f32::from(p.x), f32::from(p.y))).collect()
}

impl LookState {
    /// Avanza un cuadro. Devuelve si algo sigue moviéndose (hay que pedir
    /// otro cuadro). Sin esquema (estilo Atic) no hace nada.
    ///
    /// La cara procesando no cuenta como movimiento: cambia a `LOADING_FPS`
    /// con lo que pide `Pill::tick` (`loading_due`), no a 60.
    pub fn update(&mut self, inputs: LookInputs, scheme: Option<&Scheme>, now: Instant) -> bool {
        let Some(scheme) = scheme else {
            if self.last.is_some() || self.mark.done_until.is_some() || self.mark.state != MarkState::Rest {
                let work = self.work;
                *self = Self::default();
                self.work = work;
            }
            return false;
        };
        // Recién despierta, el primer paso no avanza: sin esto el resorte
        // saltaba de una lo que pasó desde el último movimiento.
        let dt = self.last.map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.05));
        self.reduced = inputs.reduced;

        // El grupo: la ficha con el cursor se ensancha y la elegida se
        // rellena, cada una en su lugar (no se desliza de una a otra).
        for index in 0..STRIP_MAX {
            let chosen = if inputs.chosen == Some(index) { 1.0 } else { 0.0 };
            if inputs.reduced {
                self.widen[index].snap(inputs.widen[index]);
                self.chosen[index].snap(chosen);
            } else {
                self.widen[index].set(inputs.widen[index], now);
                self.chosen[index].set(chosen, now);
            }
        }

        match inputs.pressed {
            Some(slot) => {
                if self.pressed_slot != Some(slot) {
                    self.press.snap(0.0);
                }
                self.pressed_slot = Some(slot);
                self.press.set(1.0);
            }
            None => self.press.set(0.0),
        }
        if !self.press.moving() && self.press.value == 0.0 {
            self.pressed_slot = None;
        }

        // La cara.
        let busy = inputs.recording || inputs.dictating || inputs.processing;
        if self.mark.was_busy && !busy && !inputs.failed {
            self.pop_done(now);
        }
        self.mark.was_busy = busy;
        // Que el trabajo de afuera terminó mal vale solo para este final.
        self.work.1 = false;
        if self.mark.done_until.is_some_and(|until| now >= until) {
            self.mark.done_until = None;
        }
        let state = mark_state(
            inputs.recording,
            inputs.dictating,
            inputs.processing,
            self.mark.done_until.is_some(),
            inputs.attention,
            inputs.open,
            inputs.hover,
        );
        if state != self.mark.state {
            let (shape, role) = mark_shape(state);
            let (color, ink) = self.mark_colors(scheme);
            self.mark.from = self.mark_points(now);
            self.mark.from_color = color;
            self.mark.from_ink = ink;
            self.mark.state = state;
            self.mark.to = shape;
            self.mark.role = role;
            self.mark.shape_t.snap(0.0);
            self.mark.shape_t.set(1.0);
            self.mark.color_t.snap(0.0);
            self.mark.color_t.set(1.0);
            self.mark.loading_since = (state == MarkState::Processing).then_some(now);
        }
        self.mark.loading_seen = self.mark.loading_since.map(|since| loading_frame(since, now));

        if inputs.reduced {
            // Sin movimiento: cada cosa salta a su final.
            for track in [&mut self.press, &mut self.mark.shape_t, &mut self.mark.color_t] {
                track.snap(track.target);
            }
        } else {
            self.press.step(Spring::SPATIAL_FAST, dt);
            self.mark.shape_t.step(Spring::SPATIAL_FAST, dt);
            self.mark.color_t.step(Spring::EFFECTS, dt);
        }

        let moving = self.widen.iter().chain(&self.chosen).any(|track| track.is_running(now))
            || self.press.moving()
            || self.mark.shape_t.moving()
            || self.mark.color_t.moving()
            || self.mark.done_until.is_some();
        self.last = moving.then_some(now);
        moving
    }

    /// El «hecho» (Cookie4) por un momento: lo que terminó salió bien.
    pub fn pop_done(&mut self, now: Instant) {
        self.mark.done_until = Some(now + std::time::Duration::from_millis(DONE_MS));
    }

    /// Procesando, si ya toca el cuadro siguiente de la secuencia de carga:
    /// `Pill::tick` redibuja entonces (a `LOADING_FPS`, no a 60). Quieta con
    /// movimiento reducido.
    pub fn loading_due(&self, now: Instant) -> bool {
        !self.reduced
            && self.mark.state == MarkState::Processing
            && self.mark.loading_since.is_some_and(|since| Some(loading_frame(since, now)) != self.mark.loading_seen)
    }

    /// Cuánto se ensanchó la ficha `index` a `now` (lo que se ve).
    pub fn widen_of(&self, index: usize, now: Instant) -> f32 {
        self.widen.get(index).map_or(0.0, |track| track.value(now))
    }

    /// Cuánto es la elegida la ficha `index` a `now` (0..1).
    pub fn chosen_of(&self, index: usize, now: Instant) -> f32 {
        self.chosen.get(index).map_or(0.0, |track| track.value(now).clamp(0.0, 1.0))
    }

    /// Los puntos del contenedor de la cara en este momento. Procesando va
    /// hacia la secuencia de carga (o a Circle, quieta, con movimiento
    /// reducido) en vez de a una forma fija.
    fn mark_points(&self, now: Instant) -> Vec<(f32, f32)> {
        let to: Vec<(f32, f32)> = match (self.mark.state, self.mark.loading_since) {
            (MarkState::Processing, _) if self.reduced => ShapeName::Circle.unit_points().to_vec(),
            (MarkState::Processing, Some(since)) => loading_points(loading_frame(since, now)),
            _ => self.mark.to.unit_points().to_vec(),
        };
        let t = self.mark.shape_t.value;
        self.mark
            .from
            .iter()
            .zip(to)
            .map(|(&(ax, ay), (bx, by))| (ax + (bx - ax) * t, ay + (by - ay) * t))
            .collect()
    }

    fn mark_colors(&self, scheme: &Scheme) -> (Hsla, Hsla) {
        let (color, ink) = self.mark.role.colors(scheme);
        let t = self.mark.color_t.value;
        (blend(self.mark.from_color, color, t), blend(self.mark.from_ink, ink, t))
    }

    /// El contenedor de la cara para pintar; `None` en reposo del todo.
    pub fn mark_paint(&self, scheme: &Scheme, now: Instant) -> Option<MarkPaint> {
        let (color, ink) = self.mark_colors(scheme);
        if color.a <= 0.01 && self.mark.role == Role::Hidden {
            return None;
        }
        Some(MarkPaint { points: self.mark_points(now), color, ink })
    }
}

// --- Desde la pill ---------------------------------------------------------

impl crate::Pill {
    /// La herramienta cuyo vistazo o panel está abierto, si tiene ficha en la
    /// tira: la ficha elegida del grupo.
    fn chosen_tool(&self) -> Option<usize> {
        if let Some(tool) = self.tool_peek_anchor() {
            return Some(tool);
        }
        if self.peek.target() == 1.0 {
            return Some(crate::CLIPBOARD_TOOL);
        }
        if !self.panel_open {
            return None;
        }
        use crate::NotchTool;
        match self.notch_tool {
            NotchTool::Clipboard => Some(crate::CLIPBOARD_TOOL),
            NotchTool::Textos => Some(crate::TEXTOS_TOOL),
            NotchTool::Agentes => Some(crate::AGENTES_TOOL),
            NotchTool::Sistema => Some(crate::SISTEMA_TOOL),
            NotchTool::Media => Some(crate::MEDIA_TOOL),
            NotchTool::Celular => Some(crate::CELULAR_TOOL),
            NotchTool::Reuniones => Some(crate::REUNIONES_TOOL),
            NotchTool::Pomodoro => Some(crate::POMODORO_TOOL),
            NotchTool::Apps | NotchTool::Personalizar => None,
        }
    }

    /// Avanza lo que anima el estilo Expressive (`LookState`). Devuelve si
    /// hay que pedir otro cuadro. Con el estilo Atic no hace nada.
    pub(crate) fn advance_look(&mut self, now: Instant) -> bool {
        let tokens = PillTokens::current();
        let Some(scheme) = tokens.scheme else {
            return self.look.update(LookInputs::idle(), None, now);
        };
        let slots = self.strip_slots();
        // Cambiando de paso, las fichas no son las mismas: la elegida se apaga.
        let chosen = self
            .chosen_tool()
            .filter(|_| self.page_switch.is_none())
            .and_then(|tool| slots.iter().position(|slot| slot.tool() == Some(tool)));
        let pressed = match &self.press {
            Some(press) if !press.dragging => match press.target {
                crate::PressTarget::Tool(id) => slots.iter().position(|slot| *slot == id),
                _ => None,
            },
            _ => None,
        };
        let mut widen = [0.0; STRIP_MAX];
        for (index, amount) in widen.iter_mut().enumerate().take(slots.len()) {
            *amount = self.strip_widen_target(index);
        }
        let (phase, _) = self.dictation.phase();
        let (work_busy, work_failed) = self.look.work;
        let inputs = LookInputs {
            chosen,
            pressed,
            widen,
            recording: self.rec_clock.is_some(),
            dictating: matches!(phase, crate::dictation::Phase::Starting | crate::dictation::Phase::Listening),
            processing: phase == crate::dictation::Phase::Transcribing || work_busy,
            failed: matches!(phase, crate::dictation::Phase::Error(_)) || work_failed,
            attention: self.update_mark.target() > 0.5,
            open: self.panel_open,
            hover: self.cursor.is_some_and(|c| self.over_face(c, now)),
            reduced: self.reduced_motion,
        };
        self.look.update(inputs, Some(&scheme), now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atic_son_las_constantes_de_siempre() {
        let t = PillTokens::atic(Skin::Atic);
        assert_eq!(t.skin, Skin::Atic.mid());
        assert_eq!(t.fill, Skin::Atic);
        assert_eq!(t.face, Hsla::from(rgb(0xf0f0ea)));
        assert_eq!(t.on, Hsla::from(rgb(0xf0f0ea)));
        assert_eq!(t.off, Hsla::from(rgb(0xa8a89e)));
        assert_eq!(t.hover_grow, 0.14);
        assert_eq!(t.icon, crate::TOOL_ICON);
        assert!(t.layer.is_none() && t.segment.is_none() && t.chosen.is_none() && t.recording.is_none() && t.tooltip.is_none());
        assert_eq!(t.segment_color(1.0, 1.0, 1.0), None);
        assert_eq!(t.shadow(1.0), vec![crate::pill_shadow(1.0)]);
        assert_eq!(t.shadow(0.4), vec![crate::pill_shadow(0.4)]);
        // Con otra piel solo cambia la piel.
        let wine = Skin::Solid { color: 0x3b1220 };
        assert_eq!(PillTokens::atic(wine).skin, wine.mid());
        assert_eq!(PillTokens::atic(wine).fill, wine);
    }

    #[test]
    fn expressive_sale_del_esquema_oscuro() {
        let t = PillTokens::expressive(Skin::Atic);
        let s = Scheme::baseline_dark();
        assert_eq!(t.skin, s.surface_container);
        assert_eq!(t.fill, Skin::Solid { color: to_hex(s.surface_container) });
        assert_eq!(t.chosen, Some((s.secondary_container, s.on_secondary_container)));
        assert_eq!(t.segment, Some(s.surface_container_high));
        assert_eq!(t.segment_color(0.0, 0.0, 0.0).map(to_hex), Some(to_hex(s.surface_container_high)));
        assert_eq!(t.segment_color(0.0, 0.0, 1.0).map(to_hex), Some(to_hex(s.secondary_container)));
        assert_eq!(t.hover_grow, 0.0);
        assert_eq!(t.shadow(1.0).len(), 2);
    }

    #[test]
    fn la_cara_dice_lo_mas_importante() {
        assert_eq!(mark_state(true, true, true, true, true, true, true), MarkState::Recording);
        assert_eq!(mark_state(false, true, true, false, true, true, true), MarkState::Dictating);
        assert_eq!(mark_state(false, false, true, true, true, true, true), MarkState::Processing);
        assert_eq!(mark_state(false, false, false, true, true, true, true), MarkState::Done);
        assert_eq!(mark_state(false, false, false, false, true, true, true), MarkState::Attention);
        assert_eq!(mark_state(false, false, false, false, false, true, true), MarkState::Open);
        assert_eq!(mark_state(false, false, false, false, false, false, true), MarkState::Hover);
        assert_eq!(mark_state(false, false, false, false, false, false, false), MarkState::Rest);
    }

    #[test]
    fn cada_estado_tiene_su_forma_y_su_rol() {
        assert_eq!(mark_shape(MarkState::Rest), (ShapeName::Circle, Role::Hidden));
        assert_eq!(mark_shape(MarkState::Hover), (ShapeName::Cookie9, Role::Highest));
        assert_eq!(mark_shape(MarkState::Open), (ShapeName::Square, Role::Secondary));
        assert_eq!(mark_shape(MarkState::Dictating), (ShapeName::Sunny, Role::Primary));
        assert_eq!(mark_shape(MarkState::Recording), (ShapeName::SoftBurst, Role::Error));
        assert_eq!(mark_shape(MarkState::Attention), (ShapeName::Clover4, Role::Tertiary));
        assert_eq!(mark_shape(MarkState::Done), (ShapeName::Cookie4, Role::Primary));
        assert_eq!(mark_shape(MarkState::Processing), (LOADING_SEQUENCE[0], Role::Secondary));
    }

    fn inputs() -> LookInputs {
        LookInputs::idle()
    }

    #[test]
    fn en_reposo_no_pide_cuadros() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        assert!(!look.update(inputs(), Some(&s), Instant::now()));
        // En Atic nunca.
        assert!(!look.update(LookInputs { hover: true, ..inputs() }, None, Instant::now()));
    }

    #[test]
    fn la_elegida_se_rellena_en_su_lugar_y_pasa_a_otra() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        assert!(look.update(LookInputs { chosen: Some(3), ..inputs() }, Some(&s), now));
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(LookInputs { chosen: Some(1), ..inputs() }, Some(&s), now);
        }
        assert!((look.chosen_of(1, now) - 1.0).abs() < 1e-3);
        assert!(look.chosen_of(3, now) < 1e-3);
        // Las de en medio nunca se encendieron: no hay píldora que pase por ellas.
        assert_eq!(look.chosen_of(2, now), 0.0);
    }

    #[test]
    fn la_ficha_con_el_cursor_se_ensancha_con_resorte_y_vuelve() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        let mut widen = [0.0; STRIP_MAX];
        widen[2] = 1.0;
        let mut peak: f32 = 0.0;
        assert!(look.update(LookInputs { widen, ..inputs() }, Some(&s), now));
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(LookInputs { widen, ..inputs() }, Some(&s), now);
            peak = peak.max(look.widen_of(2, now));
        }
        assert_eq!(look.widen_of(2, now), 1.0);
        // `Spatial` se pasa un poco antes de asentarse: se lee como resorte.
        assert!(peak > 1.0);
        assert_eq!(look.widen_of(1, now), 0.0);
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(inputs(), Some(&s), now);
        }
        assert_eq!(look.widen_of(2, now), 0.0);
        assert!(!look.update(inputs(), Some(&s), now));
    }

    #[test]
    fn con_movimiento_reducido_salta_al_final() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        let moving = look.update(LookInputs { chosen: Some(2), hover: true, reduced: true, ..inputs() }, Some(&s), now);
        assert!(!moving);
        assert_eq!(look.chosen_of(2, now), 1.0);
        let paint = look.mark_paint(&s, now).unwrap();
        assert_eq!(to_hex(paint.color), to_hex(s.surface_container_highest));
        for (a, b) in paint.points.iter().zip(ShapeName::Cookie9.unit_points()) {
            assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4);
        }
    }

    #[test]
    fn al_terminar_de_grabar_hace_pop_y_vuelve() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        look.update(LookInputs { recording: true, reduced: true, ..inputs() }, Some(&s), now);
        now += std::time::Duration::from_millis(16);
        assert!(look.update(LookInputs { reduced: true, ..inputs() }, Some(&s), now));
        assert_eq!(look.mark.state, MarkState::Done);
        now += std::time::Duration::from_millis(DONE_MS + 10);
        assert!(!look.update(LookInputs { reduced: true, ..inputs() }, Some(&s), now));
        assert_eq!(look.mark.state, MarkState::Rest);
    }

    fn close(a: &[(f32, f32)], b: &[(f32, f32)]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4)
    }

    #[test]
    fn procesando_recorre_la_secuencia_solo_a_15_fps_y_sin_pedir_cuadros() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let start = Instant::now();
        let mut now = start;
        // Al empezar se mueve (la forma llega); después, quieta entre cuadros.
        for _ in 0..60 {
            look.update(LookInputs { processing: true, ..inputs() }, Some(&s), now);
            now += std::time::Duration::from_millis(16);
        }
        assert_eq!(look.mark.state, MarkState::Processing);
        assert!(!look.update(LookInputs { processing: true, ..inputs() }, Some(&s), now));
        // Nada nuevo dentro del mismo cuadro de 1/15 s; el siguiente sí.
        assert!(!look.loading_due(now));
        let frame = loading_frame(start, now);
        let next = start + std::time::Duration::from_secs_f32((frame + 1) as f32 / LOADING_FPS + 1e-3);
        assert!(look.loading_due(next));
        let a = look.mark_paint(&s, now).unwrap().points;
        let b = look.mark_paint(&s, next + std::time::Duration::from_millis(300)).unwrap().points;
        assert!(!close(&a, &b), "la forma cambia mientras procesa");
        // Al terminar bien, «hecho»; después nada pide cuadros.
        assert!(look.update(inputs(), Some(&s), next));
        assert_eq!(look.mark.state, MarkState::Done);
        assert!(!look.loading_due(next + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn procesando_con_movimiento_reducido_queda_en_circulo() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        assert!(!look.update(LookInputs { processing: true, reduced: true, ..inputs() }, Some(&s), now));
        let later = now + std::time::Duration::from_secs(2);
        assert!(!look.loading_due(later));
        let paint = look.mark_paint(&s, later).unwrap();
        assert!(close(&paint.points, ShapeName::Circle.unit_points()));
    }

    #[test]
    fn el_trabajo_de_afuera_que_falla_no_hace_pop() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        look.update(LookInputs { processing: true, reduced: true, ..inputs() }, Some(&s), now);
        now += std::time::Duration::from_millis(16);
        look.update(LookInputs { failed: true, reduced: true, ..inputs() }, Some(&s), now);
        assert_eq!(look.mark.state, MarkState::Rest);
    }

    #[test]
    fn guardar_una_captura_hace_pop() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        look.pop_done(now);
        assert!(look.update(LookInputs { reduced: true, ..inputs() }, Some(&s), now));
        assert_eq!(look.mark.state, MarkState::Done);
        // En Atic se olvida sin pedir cuadros.
        let mut atic = LookState::default();
        atic.pop_done(now);
        assert!(!atic.update(inputs(), None, now));
        assert!(atic.mark.done_until.is_none());
    }

    // --- El grupo de la tira ---

    const W: f32 = 44.0;
    const GAP: f32 = 2.0;

    fn total(layout: &[(f32, f32)]) -> f32 {
        layout.last().map_or(0.0, |&(start, width)| start + width)
    }

    #[test]
    fn el_grupo_no_cambia_de_largo_con_el_hover() {
        for count in 1..=STRIP_MAX {
            let rest = group_layout(count, W, GAP, 12.0, &[]);
            assert!((total(&rest) - (count as f32 * (W + GAP) - GAP)).abs() < 1e-3);
            for hovered in 0..count {
                for amount in [0.3, 1.0, 1.2] {
                    let mut widen = vec![0.0; count];
                    widen[hovered] = amount;
                    let layout = group_layout(count, W, GAP, 12.0, &widen);
                    assert!((total(&layout) - total(&rest)).abs() < 1e-3, "count {count} hovered {hovered}");
                }
                // Dos a la vez (cursor en una, vistazo de otra): tampoco.
                let mut widen = vec![0.0; count];
                widen[hovered] = 1.0;
                widen[(hovered + 1) % count] = 0.5;
                assert!((total(&group_layout(count, W, GAP, 12.0, &widen)) - total(&rest)).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn la_ficha_con_hover_es_mas_ancha_y_sus_vecinas_ceden() {
        let mut widen = vec![0.0; 5];
        widen[2] = 1.0;
        let layout = group_layout(5, W, GAP, 12.0, &widen);
        assert_eq!(layout[2].1, W + 12.0);
        assert_eq!(layout[1].1, W - 6.0);
        assert_eq!(layout[3].1, W - 6.0);
        assert_eq!(layout[0].1, W);
        // Al medio, su centro no se mueve.
        let rest = group_layout(5, W, GAP, 12.0, &[]);
        assert!((layout[2].0 + layout[2].1 / 2.0 - (rest[2].0 + rest[2].1 / 2.0)).abs() < 1e-3);
        // En un extremo cede todo la única vecina.
        widen = vec![0.0; 5];
        widen[0] = 1.0;
        let layout = group_layout(5, W, GAP, 12.0, &widen);
        assert_eq!((layout[0].1, layout[1].1), (W + 12.0, W - 12.0));
        // Sola no tiene a quién quitarle: no crece.
        assert_eq!(group_layout(1, W, GAP, 12.0, &[1.0]), vec![(0.0, W)]);
        // En Atic (sin crecer) es la tira de siempre.
        let atic = group_layout(4, W, GAP, 0.0, &[1.0, 0.0, 1.0, 0.0]);
        for (index, &(start, width)) in atic.iter().enumerate() {
            assert_eq!((start, width), (index as f32 * (W + GAP), W));
        }
    }

    #[test]
    fn el_hit_test_no_deja_huecos_ni_alterna_en_el_borde() {
        let count = 6;
        let rest = group_layout(count, W, GAP, 12.0, &[]);
        // Sin hover, la cuenta de siempre: centro ± (ancho + espacio) / 2.
        for along in [0.0, 21.9, 45.0, 46.0, 90.0, total(&rest)] {
            let old = (0..count).find(|&i| (along - (i as f32 * (W + GAP) + W / 2.0)).abs() <= (W + GAP) / 2.0);
            assert_eq!(group_slot_at(&rest, GAP, 1.0, along), old, "en {along}");
        }
        // Barrer todo el grupo: siempre hay ficha (sin huecos muertos)…
        let mut along = -GAP / 2.0;
        while along <= total(&rest) + GAP / 2.0 {
            assert!(group_slot_at(&rest, GAP, 1.0, along).is_some(), "hueco en {along}");
            along += 0.25;
        }
        // …y el hover es estable: la ficha bajo el cursor, ya ensanchada (la
        // geometría objetivo), sigue bajo el cursor. Si no, el borde se
        // correría y el hover saltaría entre dos fichas en cada cuadro.
        let mut along = 0.0;
        while along <= total(&rest) {
            let first = group_slot_at(&rest, GAP, 1.0, along).unwrap();
            let mut widen = vec![0.0; count];
            widen[first] = 1.0;
            let grown = group_layout(count, W, GAP, 12.0, &widen);
            assert_eq!(group_slot_at(&grown, GAP, 1.0, along), Some(first), "en {along}");
            along += 0.25;
        }
    }

    #[test]
    fn las_esquinas_del_grupo() {
        let thick = 32.0;
        // Extremos redondos, interiores chicas.
        assert_eq!(group_radii(0, 4, thick, 0.0, 0.0), (16.0, GROUP_INNER));
        assert_eq!(group_radii(1, 4, thick, 0.0, 0.0), (GROUP_INNER, GROUP_INNER));
        assert_eq!(group_radii(3, 4, thick, 0.0, 0.0), (GROUP_INNER, 16.0));
        assert_eq!(group_radii(0, 1, thick, 0.0, 0.0), (16.0, 16.0));
        // La elegida, redonda entera; apretada, más cuadrada.
        assert_eq!(group_radii(1, 4, thick, 1.0, 0.0), (16.0, 16.0));
        assert_eq!(group_radii(1, 4, thick, 0.0, 1.0), (GROUP_PRESSED, GROUP_PRESSED));
        assert_eq!(group_radii(0, 4, thick, 1.0, 1.0), (GROUP_PRESSED, GROUP_PRESSED));
    }
}

/// El esquema del contenido de los vistazos y de los paneles del notch: el de
/// la pill en Expressive (oscuro, sembrado con la piel); `None` en Atic, donde
/// cada panel sigue con sus colores de siempre.
pub(crate) fn content_scheme() -> Option<Scheme> {
    let look = crate::appearance::current();
    (look.pill == PillStyle::Expressive).then(|| scheme_for(look.skin))
}

/// En Expressive, mientras se dibuja la ventana de la pill, los componentes de
/// gpui-m3 (vistazos, paneles del notch) usan el esquema de la pill y no el de
/// Atic Code. Devuelve la capa que, pintada al final, deja el de antes, como
/// `theme::scope_m3`. En Atic no hace nada.
pub(crate) fn scope_m3(cx: &mut gpui::App) -> Option<gpui::AnyElement> {
    content_scheme().map(|scheme| gpui_m3::Theme::scoped(scheme, true, cx))
}

impl Track {
    /// Avanza el resorte `dt` segundos, para lo que se anima fuera de
    /// `LookState` (la rueda de `wheel_m3.rs`).
    pub(crate) fn advance(&mut self, spring: Spring, dt: f32) {
        self.step(spring, dt);
    }
}
