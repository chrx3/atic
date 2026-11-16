//! El aspecto de la pill según su estilo (`appearance::PillStyle`).
//!
//! La pill se pinta a mano (`Frame::paint_base` y `paint_top` en `main.rs`):
//! los componentes de gpui-m3 no entran en ese lienzo. Lo que cambia entre un
//! estilo y otro vive en `PillTokens`, que `frame` resuelve una vez por cuadro:
//!
//! - **Atic**: las constantes de siempre, tal cual (`PillTokens::atic`).
//! - **M3 Expressive** («toolbar flotante»): los roles de un `Scheme` oscuro
//!   sembrado con la piel elegida (la de Atic usa `baseline_dark`). La tira
//!   lleva capa de estado al pasar el cursor, una píldora para la herramienta
//!   elegida y la cara un contenedor cuya forma dice el estado
//!   (`mark_shape`).
//!
//! `LookState` guarda lo que se anima solo en Expressive (la píldora que se
//! desliza entre fichas, el radio al presionar y la forma de la cara), con
//! resortes de gpui-m3; `Pill::advance` lo avanza. En Atic no hace nada.

use std::sync::Mutex;
use std::time::Instant;

use gpui::{point, px, rgb, BoxShadow, Hsla, PathBuilder, Window};
use gpui_m3::{Elevation, Scheme, ShapeName, Spring};

use crate::appearance::{PillStyle, Skin};
use crate::tweak::num;

/// Opacidad de la capa de estado al pasar el cursor y al presionar (M3).
const HOVER_LAYER: f32 = 0.08;
const PRESS_LAYER: f32 = 0.10;
/// La capa de estado de una ficha: un círculo de 32 que al presionar baja su
/// radio (de 16 a 10).
pub(crate) const LAYER_SIDE: f32 = 32.0;
const LAYER_PRESSED_R: f32 = 10.0;
/// La píldora de la herramienta elegida: 40 a lo largo de la tira, 32 de
/// grueso (como el indicador de una barra de navegación de M3).
pub(crate) const CHOSEN_LONG: f32 = 40.0;
pub(crate) const CHOSEN_THICK: f32 = 32.0;
/// Íconos de la tira en Expressive (en Atic, `TOOL_ICON`).
const M3_ICON: f32 = 20.0;
/// El contenedor de la cara.
pub(crate) const FACE_BOX: f32 = 32.0;
/// Lo que dura el «hecho» (Cookie4) antes de volver a reposo.
const DONE_MS: u64 = 420;

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
    /// La píldora de la elegida: contenedor y contenido.
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

    /// El radio de la capa de estado: redonda, y más cuadrada al presionar.
    pub fn layer_radius(press: f32) -> f32 {
        let round = LAYER_SIDE / 2.0;
        round + (num("pill.m3.pressRadius", LAYER_PRESSED_R) - round) * press.clamp(0.0, 1.0)
    }
}

/// La capa de estado de una ficha de la tira, lista para pintar.
pub(crate) struct ToolLayer {
    pub center: (f32, f32),
    pub radius: f32,
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
    let scheme = match skin {
        Skin::Atic => Scheme::baseline_dark(),
        _ => Scheme::from_seed(skin.mid(), true),
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
    /// Hay una actualización.
    Attention,
    /// Algo terminó (el dictado se pegó, la grabación se detuvo).
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
pub(crate) fn mark_state(recording: bool, dictating: bool, done: bool, attention: bool, open: bool, hover: bool) -> MarkState {
    if recording {
        MarkState::Recording
    } else if dictating {
        MarkState::Dictating
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
    pub recording: bool,
    pub dictating: bool,
    /// El dictado terminó mal: sin «hecho».
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
            recording: false,
            dictating: false,
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
    /// La píldora de la elegida: en qué ficha (con decimales mientras se
    /// desliza) y cuánto se ve.
    pub chosen_at: Track,
    pub chosen_shown: Track,
    chosen_slot: Option<usize>,
    /// Cuánto está apretada `pressed_slot`.
    pub press: Track,
    pub pressed_slot: Option<usize>,
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
}

impl Default for LookState {
    fn default() -> Self {
        let rest = ShapeName::Circle;
        Self {
            last: None,
            chosen_at: Track::at(0.0),
            chosen_shown: Track::at(0.0),
            chosen_slot: None,
            press: Track::at(0.0),
            pressed_slot: None,
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
            },
        }
    }
}

impl LookState {
    /// Avanza un cuadro. Devuelve si algo sigue moviéndose (hay que pedir
    /// otro cuadro). Sin esquema (estilo Atic) no hace nada.
    pub fn update(&mut self, inputs: LookInputs, scheme: Option<&Scheme>, now: Instant) -> bool {
        let Some(scheme) = scheme else {
            if self.last.is_some() || self.chosen_slot.is_some() {
                *self = Self::default();
            }
            return false;
        };
        // Recién despierta, el primer paso no avanza: sin esto el resorte
        // saltaba de una lo que pasó desde el último movimiento.
        let dt = self.last.map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.05));

        // La píldora: aparece en su ficha (sin venir de otra), se desliza
        // entre fichas y se apaga en el lugar.
        match inputs.chosen {
            Some(slot) => {
                if self.chosen_slot.is_none() && self.chosen_shown.value < 0.05 {
                    self.chosen_at.snap(slot as f32);
                } else {
                    self.chosen_at.set(slot as f32);
                }
                self.chosen_shown.set(1.0);
            }
            None => self.chosen_shown.set(0.0),
        }
        self.chosen_slot = inputs.chosen;

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
        let busy = inputs.recording || inputs.dictating;
        if self.mark.was_busy && !busy && !inputs.failed {
            self.mark.done_until = Some(now + std::time::Duration::from_millis(DONE_MS));
        }
        self.mark.was_busy = busy;
        if self.mark.done_until.is_some_and(|until| now >= until) {
            self.mark.done_until = None;
        }
        let state = mark_state(
            inputs.recording,
            inputs.dictating,
            self.mark.done_until.is_some(),
            inputs.attention,
            inputs.open,
            inputs.hover,
        );
        if state != self.mark.state {
            let (shape, role) = mark_shape(state);
            let (color, ink) = self.mark_colors(scheme);
            self.mark.from = self.mark_points();
            self.mark.from_color = color;
            self.mark.from_ink = ink;
            self.mark.state = state;
            self.mark.to = shape;
            self.mark.role = role;
            self.mark.shape_t.snap(0.0);
            self.mark.shape_t.set(1.0);
            self.mark.color_t.snap(0.0);
            self.mark.color_t.set(1.0);
        }

        if inputs.reduced {
            // Sin movimiento: cada cosa salta a su final.
            for track in [&mut self.chosen_at, &mut self.chosen_shown, &mut self.press, &mut self.mark.shape_t, &mut self.mark.color_t] {
                track.snap(track.target);
            }
        } else {
            self.chosen_at.step(Spring::SPATIAL, dt);
            self.chosen_shown.step(Spring::EFFECTS, dt);
            self.press.step(Spring::SPATIAL_FAST, dt);
            self.mark.shape_t.step(Spring::SPATIAL_FAST, dt);
            self.mark.color_t.step(Spring::EFFECTS, dt);
        }

        let moving = self.chosen_at.moving()
            || self.chosen_shown.moving()
            || self.press.moving()
            || self.mark.shape_t.moving()
            || self.mark.color_t.moving()
            || self.mark.done_until.is_some();
        self.last = moving.then_some(now);
        moving
    }

    /// Los puntos del contenedor de la cara en este momento.
    fn mark_points(&self) -> Vec<(f32, f32)> {
        let to = self.mark.to.unit_points();
        let t = self.mark.shape_t.value;
        self.mark
            .from
            .iter()
            .zip(to)
            .map(|(&(ax, ay), &(bx, by))| (ax + (bx - ax) * t, ay + (by - ay) * t))
            .collect()
    }

    fn mark_colors(&self, scheme: &Scheme) -> (Hsla, Hsla) {
        let (color, ink) = self.mark.role.colors(scheme);
        let t = self.mark.color_t.value;
        (blend(self.mark.from_color, color, t), blend(self.mark.from_ink, ink, t))
    }

    /// El contenedor de la cara para pintar; `None` en reposo del todo.
    pub fn mark_paint(&self, scheme: &Scheme) -> Option<MarkPaint> {
        let (color, ink) = self.mark_colors(scheme);
        if color.a <= 0.01 && self.mark.role == Role::Hidden {
            return None;
        }
        Some(MarkPaint { points: self.mark_points(), color, ink })
    }
}

// --- Desde la pill ---------------------------------------------------------

impl crate::Pill {
    /// La herramienta cuyo vistazo o panel está abierto, si tiene ficha en la
    /// tira: la que lleva la píldora de elegida.
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
        // Cambiando de paso, las fichas no son las mismas: la píldora se apaga.
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
        let (phase, _) = self.dictation.phase();
        let inputs = LookInputs {
            chosen,
            pressed,
            recording: self.rec_clock.is_some(),
            dictating: matches!(
                phase,
                crate::dictation::Phase::Starting | crate::dictation::Phase::Listening | crate::dictation::Phase::Transcribing
            ),
            failed: matches!(phase, crate::dictation::Phase::Error(_)),
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
        assert!(t.layer.is_none() && t.chosen.is_none() && t.recording.is_none() && t.tooltip.is_none());
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
        assert_eq!(t.hover_grow, 0.0);
        assert_eq!(t.shadow(1.0).len(), 2);
    }

    #[test]
    fn la_cara_dice_lo_mas_importante() {
        assert_eq!(mark_state(true, true, true, true, true, true), MarkState::Recording);
        assert_eq!(mark_state(false, true, false, true, true, true), MarkState::Dictating);
        assert_eq!(mark_state(false, false, true, true, true, true), MarkState::Done);
        assert_eq!(mark_state(false, false, false, true, true, true), MarkState::Attention);
        assert_eq!(mark_state(false, false, false, false, true, true), MarkState::Open);
        assert_eq!(mark_state(false, false, false, false, false, true), MarkState::Hover);
        assert_eq!(mark_state(false, false, false, false, false, false), MarkState::Rest);
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
    fn la_pildora_aparece_en_su_ficha_y_se_desliza_a_otra() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let mut now = Instant::now();
        assert!(look.update(LookInputs { chosen: Some(3), ..inputs() }, Some(&s), now));
        assert_eq!(look.chosen_at.value, 3.0);
        for _ in 0..120 {
            now += std::time::Duration::from_millis(16);
            look.update(LookInputs { chosen: Some(1), ..inputs() }, Some(&s), now);
        }
        assert!((look.chosen_at.value - 1.0).abs() < 1e-3);
        assert!((look.chosen_shown.value - 1.0).abs() < 1e-3);
    }

    #[test]
    fn con_movimiento_reducido_salta_al_final() {
        let s = Scheme::baseline_dark();
        let mut look = LookState::default();
        let now = Instant::now();
        let moving = look.update(LookInputs { chosen: Some(2), hover: true, reduced: true, ..inputs() }, Some(&s), now);
        assert!(!moving);
        assert_eq!(look.chosen_shown.value, 1.0);
        let paint = look.mark_paint(&s).unwrap();
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
}
