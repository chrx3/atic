//! Lo que se hizo en el editor de grabaciones (`recording_editor.rs`), sin tocar
//! el video: recorte, cortes, zooms, textos y el cursor. Vive en
//! `.atic/<nombre>.edit.json` junto a la grabación.
//!
//! Todo lo que se ve en un momento sale del proyecto y del registro del mouse
//! (`screen_cursor.rs`), nunca del cuadro anterior (`transiciones-fluidas`,
//! regla 3): la vista previa, saltar a cualquier punto y la exportación dan lo
//! mismo. La cámara y el cursor suavizados se integran una sola vez a 60 Hz
//! (`Motion`) y después solo se leen.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::screen_cursor::Track;

/// Lo que dura la onda de un clic.
pub const RIPPLE: f64 = 0.55;
/// Lo que dura la entrada (y la salida) de un zoom con la velocidad Normal. La que
/// cuenta en el cálculo sale de `CameraOpts::ramp_secs`.
pub const ZOOM_RAMP: f64 = 0.8;
/// La entrada y la salida de un texto.
const TEXT_FADE: f64 = 0.25;
/// Clics a menos de esto forman un mismo zoom automático.
const CLICK_GAP: f64 = 1.5;
/// Teclas a menos de esto son la misma racha de escritura…
const TYPING_GAP: f64 = 1.2;
/// …y una racha de verdad tiene al menos estas.
const TYPING_MIN_KEYS: usize = 5;
/// Mientras se escribió hace menos de esto, la cámara sigue al texto y no al mouse.
const TYPING_HOLD: f64 = 1.0;
const RATE: f64 = 60.0;
/// Un zoom más corto que esto no vale la pena (ni a mano ni de pedazo sobrante).
const MIN_ZOOM: f64 = 0.3;
/// Un tramo de velocidad más corto que esto no se agrega.
const MIN_SPEED: f64 = 0.1;
/// Los zooms automáticos más largos que esto se parten donde no pasa nada…
const LONG_ZOOM: f64 = 12.0;
/// …si el mouse (y el teclado) estuvo quieto más de esto.
const IDLE_SPLIT: f64 = 2.5;
/// El mouse se movió si se alejó esto del cuadro (parte del ancho y el alto)
/// de donde estaba.
const IDLE_MOVE: f64 = 0.005;
/// Con menos nivel que esto, el audio está en silencio.
const QUIET_LEVEL: f32 = 0.04;
/// Lo que se recorta de cada borde de un tramo inactivo, para no comerse el
/// comienzo de la acción.
const IDLE_EDGE: f64 = 0.3;
/// Para esconder el cursor quieto, cualquier movimiento cuenta (más fino que `IDLE_MOVE`).
const CURSOR_MOVE: f64 = 0.001;
/// Cuánto se encoge el cursor al apretar (parte de su tamaño).
const PRESS_SHRINK: f64 = 0.2;
/// …en esto (el cursor quieto más de `CursorOpts::hide_delay` se esconde)…
const CURSOR_HIDE: f64 = 0.4;
/// …y vuelve en esto al moverlo.
const CURSOR_SHOW: f64 = 0.15;
/// El fundido de entrada y salida de una zona difuminada.
const BLUR_FADE: f64 = 0.25;
/// Cuánto zoom nace por defecto, y entre qué valores se puede elegir.
const DEPTH_DEFAULT: f64 = 2.0;
const DEPTH_MIN: f64 = 1.2;
const DEPTH_MAX: f64 = 4.0;
/// El suavizado de la cámara (multiplica el ancho de la campana) y el desenfoque de movimiento.
const SMOOTHING_MIN: f64 = 0.5;
const SMOOTHING_MAX: f64 = 2.0;
const BLUR_MAX: f64 = 2.0;
/// Tamaño del cursor, de la onda del clic y cuánto tarda en esconderse el cursor quieto.
const CURSOR_SIZE_MIN: f64 = 0.5;
const CURSOR_SIZE_MAX: f64 = 4.0;
const CLICK_SIZE_MIN: f64 = 0.5;
const CLICK_SIZE_MAX: f64 = 2.5;
const HIDE_DELAY_MIN: f64 = 1.0;
const HIDE_DELAY_MAX: f64 = 6.0;
const HIDE_DELAY_DEFAULT: f64 = 2.0;
/// El resorte del cursor: con el nivel 0 casi no suaviza (`OMEGA_MAX`), con el 1 suaviza
/// mucho (`OMEGA_MIN`); el nivel por defecto (0,5) da 18, lo de siempre.
const OMEGA_MAX: f64 = 54.0;
const OMEGA_MIN: f64 = 6.0;
const SMOOTH_LEVEL_DEFAULT: f64 = 0.5;
/// El marco: margen (parte del lado corto), radio de las esquinas (parte del alto) y sombra.
const PADDING_MAX: f64 = 0.3;
const RADIUS_MAX: f64 = 0.1;
const SHADOW_DEFAULT: f64 = 0.5;
/// Cuántos pasos de deshacer se guardan.
const HISTORY_MAX: usize = 100;
/// Los cambios con la misma clave dentro de esto son un solo paso (arrastres).
const HISTORY_GROUP: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Focus {
    /// Sigue al cursor.
    Auto,
    /// Un punto fijo del video (0–1).
    At(f64, f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zoom {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub focus: Focus,
    /// Lo propuso el autofocus (se rehace al pedirlo de nuevo).
    #[serde(default)]
    pub auto: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextAt {
    Top,
    Center,
    Bottom,
}

fn one() -> f64 {
    1.0
}

fn white() -> u32 {
    0xffffff
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub at: TextAt,
    /// Múltiplo del tamaño base.
    #[serde(default = "one")]
    pub size: f64,
    /// 0xRRGGBB.
    #[serde(default = "white")]
    pub color: u32,
    /// Con píldora detrás.
    #[serde(default = "yes")]
    pub boxed: bool,
    #[serde(default = "yes")]
    pub bold: bool,
    /// Subtítulo: sobre el borde de abajo, con ancho más acotado.
    #[serde(default)]
    pub caption: bool,
}

impl Text {
    /// Un texto con el estilo de siempre (tamaño 1, blanco, con píldora y en negrita).
    pub fn new(start: f64, end: f64, text: impl Into<String>, at: TextAt) -> Self {
        Self { start, end, text: text.into(), at, size: 1.0, color: 0xffffff, boxed: true, bold: true, caption: false }
    }
}

/// Una zona del cuadro que se tapa un rato (difuminada o pixelada).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Blur {
    pub start: f64,
    pub end: f64,
    /// x, y, ancho y alto (0–1 del video).
    pub rect: [f64; 4],
    /// Pixelada en vez de difuminada.
    #[serde(default)]
    pub pixelate: bool,
}

/// Un tramo del original que va más rápido.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Speed {
    pub start: f64,
    pub end: f64,
    /// 1,25 a 16 veces.
    pub rate: f64,
}

/// Un número fuera de rango (o roto: NaN, infinito) vuelve al rango o al valor por defecto.
fn bound(value: f64, min: f64, max: f64, default: f64) -> f64 {
    if value.is_finite() { value.clamp(min, max) } else { default }
}

/// Hasta dónde se deja alejar al cursor del centro de lo que se ve (ver `Motion::new`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Follow {
    /// El cursor casi siempre al centro: la cámara va pegada a él.
    Centered,
    /// El cursor puede moverse cerca del centro sin que la cámara se mueva, y nunca
    /// llega al borde.
    #[default]
    Balanced,
    /// Mucha holgura: la cámara se mueve lo menos posible, pero sin perder al cursor.
    Free,
}

impl Follow {
    pub const ALL: [Follow; 3] = [Follow::Centered, Follow::Balanced, Follow::Free];

    /// La zona muerta: cuánto puede alejarse el cursor del centro (parte de media
    /// vista) sin que la cámara se mueva.
    pub fn dead_zone(self) -> f64 {
        match self {
            Self::Centered => 0.04,
            Self::Balanced => 0.25,
            Self::Free => 0.45,
        }
    }

    /// La banda de guarda: el cursor nunca pasa de esto (parte de media vista).
    pub fn guard(self) -> f64 {
        match self {
            Self::Centered => 0.38,
            Self::Balanced => 0.70,
            Self::Free => 0.85,
        }
    }

    /// Cuánto de la campana de suavizado se usa (la cámara de `Centered` debe poder
    /// seguir tirones del cursor).
    fn sigma_scale(self) -> f64 {
        match self {
            Self::Centered => 0.45,
            Self::Balanced => 1.0,
            Self::Free => 1.0,
        }
    }
}

/// Qué tan rápido entra y sale un zoom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoomSpeed {
    Fast,
    #[default]
    Normal,
    Slow,
}

impl ZoomSpeed {
    pub const ALL: [ZoomSpeed; 3] = [ZoomSpeed::Fast, ZoomSpeed::Normal, ZoomSpeed::Slow];

    /// Lo que dura la entrada (y la salida), en segundos.
    pub fn secs(self) -> f64 {
        match self {
            Self::Fast => 0.5,
            Self::Normal => ZOOM_RAMP,
            Self::Slow => 1.2,
        }
    }
}

/// La curva de entrada y salida del zoom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ease {
    /// Arranca y llega sin tirón (derivadas primera y segunda en cero en los extremos).
    #[default]
    Smooth,
    /// Sale rápido y asienta: la misma suavidad en los extremos, con el cambio al comienzo.
    Snappy,
    /// Parejo, sin aceleración.
    Linear,
}

impl Ease {
    pub const ALL: [Ease; 3] = [Ease::Smooth, Ease::Snappy, Ease::Linear];

    /// La curva: de 0 a 1 en `x` de 0 a 1.
    pub fn apply(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Smooth => smootherstep(x),
            // Un smootherstep al que se le adelanta el tiempo: llega a la mitad en el 30 %.
            Self::Snappy => smootherstep(1.0 - (1.0 - x) * (1.0 - x)),
            Self::Linear => x,
        }
    }
}

/// Los números del zoom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraOpts {
    /// El zoom con que nace uno nuevo (1,2–4×).
    pub depth: f64,
    pub speed: ZoomSpeed,
    pub ease: Ease,
}

impl Default for CameraOpts {
    fn default() -> Self {
        Self { depth: DEPTH_DEFAULT, speed: ZoomSpeed::Normal, ease: Ease::Smooth }
    }
}

impl CameraOpts {
    /// Los mismos números dentro de sus límites.
    pub fn clamped(self) -> Self {
        Self { depth: bound(self.depth, DEPTH_MIN, DEPTH_MAX, DEPTH_DEFAULT), ..self }
    }

    /// Lo que dura la entrada (y la salida) del zoom, en segundos.
    pub fn ramp_secs(&self) -> f64 {
        self.speed.secs()
    }

    /// Los límites de `depth`, para el control de la interfaz.
    pub const DEPTH_RANGE: (f64, f64) = (DEPTH_MIN, DEPTH_MAX);

    /// «2,0×» con punto decimal (la interfaz lo cambia a coma si hace falta).
    pub fn depth_text(&self) -> String {
        format!("{:.1}×", self.depth)
    }

    /// «0.8 s».
    pub fn ramp_text(&self) -> String {
        format!("{:.1} s", self.ramp_secs())
    }
}

/// Cómo se mueve la cámara y el desenfoque que deja.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionOpts {
    /// El desenfoque de movimiento: 0 apagado, 1 normal, hasta 2.
    pub blur: f64,
    pub follow: Follow,
    /// Multiplica el suavizado de la cámara (0,5–2): más es más pausada.
    pub smoothing: f64,
}

impl Default for MotionOpts {
    fn default() -> Self {
        Self { blur: 1.0, follow: Follow::Balanced, smoothing: 1.0 }
    }
}

impl MotionOpts {
    pub fn clamped(self) -> Self {
        Self { blur: bound(self.blur, 0.0, BLUR_MAX, 1.0), smoothing: bound(self.smoothing, SMOOTHING_MIN, SMOOTHING_MAX, 1.0), ..self }
    }

    /// Los límites de `smoothing` y de `blur`, para los controles.
    pub const SMOOTHING_RANGE: (f64, f64) = (SMOOTHING_MIN, SMOOTHING_MAX);
    pub const BLUR_RANGE: (f64, f64) = (0.0, BLUR_MAX);

    /// «100 %».
    pub fn blur_text(&self) -> String {
        format!("{:.0} %", self.blur * 100.0)
    }

    /// «1.0×».
    pub fn smoothing_text(&self) -> String {
        format!("{:.1}×", self.smoothing)
    }
}

/// Qué tan movida es la cámara del zoom: un preajuste que escribe los números de
/// `CameraOpts` y `MotionOpts` (`Project::apply_style`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraStyle {
    /// Acerca poco, entra lento y sigue al cursor con holgura.
    Calm,
    #[default]
    Normal,
    /// Acerca más, entra rápido y sigue al cursor de cerca.
    Dynamic,
}

impl CameraStyle {
    pub const ALL: [CameraStyle; 3] = [CameraStyle::Calm, CameraStyle::Normal, CameraStyle::Dynamic];

    /// El zoom con que nace uno nuevo.
    pub fn scale(self) -> f64 {
        match self {
            Self::Calm => 1.6,
            Self::Normal => DEPTH_DEFAULT,
            Self::Dynamic => 2.4,
        }
    }

    /// La velocidad del zoom.
    pub fn speed(self) -> ZoomSpeed {
        match self {
            Self::Calm => ZoomSpeed::Slow,
            Self::Normal => ZoomSpeed::Normal,
            Self::Dynamic => ZoomSpeed::Fast,
        }
    }

    /// Lo que dura la entrada (y la salida) del zoom.
    pub fn ramp(self) -> f64 {
        self.speed().secs()
    }

    /// Hasta dónde se deja alejar al cursor.
    pub fn follow(self) -> Follow {
        match self {
            Self::Calm => Follow::Free,
            Self::Normal => Follow::Balanced,
            Self::Dynamic => Follow::Centered,
        }
    }

    /// El suavizado de la cámara (el ancho de la campana respecto al de Normal).
    pub fn smoothing(self) -> f64 {
        match self {
            Self::Calm => 1.4,
            Self::Normal => 1.0,
            Self::Dynamic => 0.7,
        }
    }

    /// Los números de `CameraOpts` de este preajuste (con la curva de siempre).
    fn camera_opts(self) -> CameraOpts {
        CameraOpts { depth: self.scale(), speed: self.speed(), ease: Ease::Smooth }
    }

    /// Los números de `MotionOpts` de este preajuste (con el desenfoque normal).
    fn motion_opts(self) -> MotionOpts {
        MotionOpts { blur: 1.0, follow: self.follow(), smoothing: self.smoothing() }
    }
}

/// Cómo se dibuja el cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorStyle {
    /// El del sistema (flecha, mano, I-beam…).
    #[default]
    System,
    Dot,
    Ring,
    Dark,
    /// Siempre la flecha, aunque la grabación muestre mano o I-beam.
    Arrow,
}

impl CursorStyle {
    pub const ALL: [CursorStyle; 5] = [CursorStyle::System, CursorStyle::Dot, CursorStyle::Ring, CursorStyle::Dark, CursorStyle::Arrow];
}

/// El efecto de un clic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClickKind {
    None,
    #[default]
    Ripple,
    Ring,
    Pulse,
}

impl ClickKind {
    pub const ALL: [ClickKind; 4] = [ClickKind::None, ClickKind::Ripple, ClickKind::Ring, ClickKind::Pulse];
}

/// La constante del resorte que suaviza el cursor para un nivel de 0 a 1: `None` con
/// el 0 (el cursor va crudo); si no, de 54 (casi crudo) a 6 (muy suave), con 18 en el
/// 0,5, lo de siempre. Un nivel roto (NaN) vale como el por defecto.
pub fn smooth_omega(level: f64) -> Option<f64> {
    let level = bound(level, 0.0, 1.0, SMOOTH_LEVEL_DEFAULT);
    (level > 0.0).then(|| OMEGA_MAX * (OMEGA_MIN / OMEGA_MAX).powf(level))
}

/// El cursor. Los proyectos viejos traían `smooth` y `ripple` (sí o no): se leen como
/// el nivel 0 (o el por defecto) y el clic `None` (o `Ripple`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "CursorOptsRaw")]
pub struct CursorOpts {
    /// Cuánto más grande que el del sistema (0,5–4×).
    pub size: f64,
    /// 0 sin suavizar … 1 muy suave (ver `smooth_omega`).
    pub smooth_level: f64,
    pub style: CursorStyle,
    /// El efecto donde se hace clic.
    pub click: ClickKind,
    /// Tamaño del efecto del clic (0,5–2,5×).
    pub click_size: f64,
    /// 0xRRGGBB.
    pub click_color: u32,
    /// El cursor se encoge un poco al apretar.
    pub press_shrink: bool,
    /// Esconder el cursor cuando lleva rato quieto…
    pub hide_idle: bool,
    /// …este tiempo, en segundos (1–6).
    pub hide_delay: f64,
}

impl Default for CursorOpts {
    fn default() -> Self {
        Self {
            size: 1.5,
            smooth_level: SMOOTH_LEVEL_DEFAULT,
            style: CursorStyle::System,
            click: ClickKind::Ripple,
            click_size: 1.0,
            click_color: 0xffffff,
            press_shrink: true,
            hide_idle: true,
            hide_delay: HIDE_DELAY_DEFAULT,
        }
    }
}

impl CursorOpts {
    pub fn clamped(self) -> Self {
        Self {
            size: bound(self.size, CURSOR_SIZE_MIN, CURSOR_SIZE_MAX, 1.5),
            smooth_level: bound(self.smooth_level, 0.0, 1.0, SMOOTH_LEVEL_DEFAULT),
            click_size: bound(self.click_size, CLICK_SIZE_MIN, CLICK_SIZE_MAX, 1.0),
            click_color: self.click_color & 0xffffff,
            hide_delay: bound(self.hide_delay, HIDE_DELAY_MIN, HIDE_DELAY_MAX, HIDE_DELAY_DEFAULT),
            ..self
        }
    }

    /// Los límites, para los controles.
    pub const SIZE_RANGE: (f64, f64) = (CURSOR_SIZE_MIN, CURSOR_SIZE_MAX);
    pub const CLICK_SIZE_RANGE: (f64, f64) = (CLICK_SIZE_MIN, CLICK_SIZE_MAX);
    pub const HIDE_DELAY_RANGE: (f64, f64) = (HIDE_DELAY_MIN, HIDE_DELAY_MAX);

    /// «1.5×».
    pub fn size_text(&self) -> String {
        format!("{:.1}×", self.size)
    }

    /// «50 %».
    pub fn smooth_text(&self) -> String {
        format!("{:.0} %", self.smooth_level * 100.0)
    }

    /// «2.0 s».
    pub fn hide_delay_text(&self) -> String {
        format!("{:.1} s", self.hide_delay)
    }
}

/// Lo que se lee de `CursorOpts` en el JSON: lo de antes y lo de ahora.
#[derive(Deserialize)]
#[serde(default)]
struct CursorOptsRaw {
    size: f64,
    smooth: Option<bool>,
    smooth_level: Option<f64>,
    style: CursorStyle,
    ripple: Option<bool>,
    click: Option<ClickKind>,
    click_size: f64,
    click_color: u32,
    press_shrink: bool,
    hide_idle: bool,
    hide_delay: f64,
}

impl Default for CursorOptsRaw {
    fn default() -> Self {
        let d = CursorOpts::default();
        Self {
            size: d.size,
            smooth: None,
            smooth_level: None,
            style: d.style,
            ripple: None,
            click: None,
            click_size: d.click_size,
            click_color: d.click_color,
            press_shrink: d.press_shrink,
            hide_idle: d.hide_idle,
            hide_delay: d.hide_delay,
        }
    }
}

impl From<CursorOptsRaw> for CursorOpts {
    fn from(raw: CursorOptsRaw) -> Self {
        let smooth_level = raw.smooth_level.unwrap_or(if raw.smooth == Some(false) { 0.0 } else { SMOOTH_LEVEL_DEFAULT });
        let click = raw.click.unwrap_or(if raw.ripple == Some(false) { ClickKind::None } else { ClickKind::Ripple });
        Self {
            size: raw.size,
            smooth_level,
            style: raw.style,
            click,
            click_size: raw.click_size,
            click_color: raw.click_color,
            press_shrink: raw.press_shrink,
            hide_idle: raw.hide_idle,
            hide_delay: raw.hide_delay,
        }
        .clamped()
    }
}

/// Lo que va detrás del video cuando se le pone marco.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Backdrop {
    /// Sin marco: el video llena el cuadro.
    None,
    Solid(u32),
    /// De arriba a la izquierda a abajo a la derecha.
    Gradient(u32, u32),
    /// Una imagen; la ruta va en `Project::backdrop_image` (así `Look` sigue siendo `Copy`).
    Image,
}

/// Los fondos que trae el editor, como los de Screen Studio.
pub const BACKDROPS: [Backdrop; 8] = [
    Backdrop::None,
    Backdrop::Gradient(0xff7e5f, 0xfeb47b),
    Backdrop::Gradient(0x8e2de2, 0x4a00e0),
    Backdrop::Gradient(0x2193b0, 0x6dd5ed),
    Backdrop::Gradient(0x11998e, 0x38ef7d),
    Backdrop::Gradient(0xee9ca7, 0xffdde1),
    Backdrop::Gradient(0x232526, 0x414345),
    Backdrop::Solid(0xf5f5f2),
];

/// El marco: el video más chico sobre un fondo, con esquinas redondeadas y sombra.
/// Los proyectos viejos traían `shadow` (sí o no): se lee como la intensidad 0,5 o 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "LookRaw")]
pub struct Look {
    pub backdrop: Backdrop,
    /// El margen alrededor del video (parte del lado corto, 0–0,3).
    pub padding: f64,
    /// El radio de las esquinas (parte del alto, 0–0,1).
    pub radius: f64,
    /// La intensidad de la sombra: 0 sin sombra, 0,5 la de siempre, hasta 1.
    pub shadow_amount: f64,
}

impl Default for Look {
    fn default() -> Self {
        Self { backdrop: Backdrop::None, padding: 0.08, radius: 0.02, shadow_amount: SHADOW_DEFAULT }
    }
}

impl Look {
    /// Si hay marco (con `Backdrop::None`, el video llena todo).
    pub fn framed(&self) -> bool {
        self.backdrop != Backdrop::None
    }

    /// Los mismos números dentro de sus límites.
    pub fn clamped(self) -> Self {
        Self {
            padding: bound(self.padding, 0.0, PADDING_MAX, 0.08),
            radius: bound(self.radius, 0.0, RADIUS_MAX, 0.02),
            shadow_amount: bound(self.shadow_amount, 0.0, 1.0, SHADOW_DEFAULT),
            ..self
        }
    }

    /// Los límites, para los controles.
    pub const PADDING_RANGE: (f64, f64) = (0.0, PADDING_MAX);
    pub const RADIUS_RANGE: (f64, f64) = (0.0, RADIUS_MAX);

    /// Si tiene sombra.
    pub fn has_shadow(&self) -> bool {
        self.shadow_amount > 0.0
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct LookRaw {
    backdrop: Backdrop,
    padding: f64,
    radius: f64,
    shadow: Option<bool>,
    shadow_amount: Option<f64>,
}

impl Default for LookRaw {
    fn default() -> Self {
        let d = Look::default();
        Self { backdrop: d.backdrop, padding: d.padding, radius: d.radius, shadow: None, shadow_amount: None }
    }
}

impl From<LookRaw> for Look {
    fn from(raw: LookRaw) -> Self {
        let shadow_amount = raw.shadow_amount.unwrap_or(if raw.shadow == Some(false) { 0.0 } else { SHADOW_DEFAULT });
        Self { backdrop: raw.backdrop, padding: raw.padding, radius: raw.radius, shadow_amount }.clamped()
    }
}

/// La proporción del cuadro exportado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    /// La del original.
    #[default]
    Source,
    /// 16:9.
    Wide,
    /// 9:16.
    Vertical,
    /// 1:1.
    Square,
    /// 4:5.
    Portrait,
}

impl Aspect {
    #[allow(dead_code)] // el diálogo de exportar arma su propia lista
    pub const ALL: [Aspect; 5] = [Aspect::Source, Aspect::Wide, Aspect::Vertical, Aspect::Square, Aspect::Portrait];

    /// Ancho y alto relativos; con `Source`, los del original (16:9 si no se sabe).
    pub fn ratio(self, source: (u32, u32)) -> (u32, u32) {
        match self {
            Self::Source if source.0 > 0 && source.1 > 0 => source,
            Self::Source | Self::Wide => (16, 9),
            Self::Vertical => (9, 16),
            Self::Square => (1, 1),
            Self::Portrait => (4, 5),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Mp4,
    Gif,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Low,
    #[default]
    Standard,
    High,
    Max,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

/// Cómo se exporta.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: Format,
    pub aspect: Aspect,
    /// Alto de salida; `None`: el del original.
    pub height: Option<u32>,
    pub quality: Quality,
    pub codec: Codec,
    pub fps: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        // 1080 de tope: un máster de 2880×1800 exportado a su propio alto pide ~18 Mbps
        // y no gana nada sobre el original (7,4). Nunca agranda una fuente menor, y
        // quien quiera el alto original lo elige en «Resolución».
        Self { format: Format::Mp4, aspect: Aspect::Source, height: Some(1080), quality: Quality::Standard, codec: Codec::H264, fps: 60 }
    }
}

/// Lo que se hizo en la grabación. Los proyectos viejos se leen completos: `camera`
/// (el estilo) pasa a los números de `camera_opts` y `motion`, y `sharp` a `motion.blur` 0
/// (ver `ProjectRaw`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "ProjectRaw")]
pub struct Project {
    /// Lo que queda del inicio al final; `None`: todo.
    pub trim: Option<(f64, f64)>,
    /// Tramos quitados del medio.
    pub cuts: Vec<(f64, f64)>,
    pub zooms: Vec<Zoom>,
    pub texts: Vec<Text>,
    pub cursor: CursorOpts,
    pub look: Look,
    /// El último preajuste de cámara que se aplicó (`apply_style`). Para saber cuál
    /// está activo ahora, `active_style`: los números pueden haberse movido a mano.
    pub camera: CameraStyle,
    /// Los números del zoom.
    pub camera_opts: CameraOpts,
    /// Cómo se mueve la cámara y el desenfoque de movimiento.
    pub motion: MotionOpts,
    /// Tramos del original que van más rápido (en orden, sin pisarse).
    pub speeds: Vec<Speed>,
    /// Zonas tapadas.
    pub blurs: Vec<Blur>,
    /// La imagen de fondo con `Backdrop::Image`: una ruta, o `wallpaper:NN` si es uno de los
    /// fondos que trae el editor.
    pub backdrop_image: Option<PathBuf>,
    /// La última imagen que el usuario trajo de su disco: sigue a mano en el selector aunque
    /// después elija uno de los fondos del editor.
    pub backdrop_own: Option<PathBuf>,
    pub export: ExportSettings,
}

/// Lo que se lee del JSON de un proyecto: lo de ahora y lo de antes (`sharp`, y el
/// estilo de cámara sin números).
#[derive(Deserialize)]
#[serde(default)]
struct ProjectRaw {
    trim: Option<(f64, f64)>,
    cuts: Vec<(f64, f64)>,
    zooms: Vec<Zoom>,
    texts: Vec<Text>,
    cursor: CursorOpts,
    look: Look,
    sharp: bool,
    camera: CameraStyle,
    camera_opts: Option<CameraOpts>,
    motion: Option<MotionOpts>,
    speeds: Vec<Speed>,
    blurs: Vec<Blur>,
    backdrop_image: Option<PathBuf>,
    backdrop_own: Option<PathBuf>,
    export: ExportSettings,
}

impl Default for ProjectRaw {
    fn default() -> Self {
        Self {
            trim: None,
            cuts: Vec::new(),
            zooms: Vec::new(),
            texts: Vec::new(),
            cursor: CursorOpts::default(),
            look: Look::default(),
            sharp: false,
            camera: CameraStyle::default(),
            camera_opts: None,
            motion: None,
            speeds: Vec::new(),
            blurs: Vec::new(),
            backdrop_image: None,
            backdrop_own: None,
            export: ExportSettings::default(),
        }
    }
}

impl From<ProjectRaw> for Project {
    fn from(raw: ProjectRaw) -> Self {
        let camera_opts = raw.camera_opts.unwrap_or_else(|| raw.camera.camera_opts()).clamped();
        let mut motion = raw.motion.unwrap_or_else(|| raw.camera.motion_opts());
        if raw.sharp {
            motion.blur = 0.0;
        }
        Self {
            trim: raw.trim,
            cuts: raw.cuts,
            zooms: raw.zooms,
            texts: raw.texts,
            cursor: raw.cursor.clamped(),
            look: raw.look.clamped(),
            camera: raw.camera,
            camera_opts,
            motion: motion.clamped(),
            speeds: raw.speeds,
            blurs: raw.blurs,
            backdrop_image: raw.backdrop_image,
            backdrop_own: raw.backdrop_own,
            export: raw.export,
        }
    }
}

/// `.atic/<nombre>.edit.json`.
pub fn project_path(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    Some(video.parent()?.join(".atic").join(format!("{stem}.edit.json")))
}

/// `<ruta>.<ext>`: el mismo nombre con una terminación más (`x.edit.json.bak`).
fn suffixed(path: &Path, ext: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

/// Deja una copia del archivo dañado junto a él (`.corrupt`, y `.corrupt.1`,
/// `.corrupt.2`… si ya había una: nunca se pisa una anterior).
fn keep_corrupt(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    for n in 0..1000 {
        let copy = suffixed(path, &if n == 0 { "corrupt".to_string() } else { format!("corrupt.{n}") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&copy) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                return Ok(copy);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "ya hay demasiadas copias de proyectos dañados"))
}

impl Project {
    /// El proyecto de este video, o uno nuevo si no hay o está dañado.
    #[allow(dead_code)] // la usa el editor vía load_checked; se queda por compatibilidad y para los tests
    pub fn load(video: &Path) -> Self {
        Self::load_checked(video).0
    }

    /// Como `load`, pero dice qué pasó si el archivo estaba dañado. Sin archivo:
    /// proyecto nuevo y sin error. Con el JSON dañado (o ilegible) el archivo malo
    /// no se toca: se copia a `<nombre>.edit.json.corrupt` (sin pisar una copia
    /// anterior) y vuelve el proyecto nuevo con el texto del error. Así el editor
    /// puede avisar antes de guardar encima.
    pub fn load_checked(video: &Path) -> (Self, Option<String>) {
        let Some(path) = project_path(video) else { return (Self::default(), None) };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(e) => return (Self::default(), Some(format!("no se pudo leer {}: {e}", path.display()))),
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(project) => (project, None),
            Err(e) => {
                let mut message = format!("el proyecto está dañado ({e})");
                match keep_corrupt(&path, &bytes) {
                    Ok(copy) => message.push_str(&format!("; se guardó una copia en {}", copy.display())),
                    Err(err) => message.push_str(&format!("; no se pudo guardar una copia: {err}")),
                }
                (Self::default(), Some(message))
            }
        }
    }

    /// Guarda el proyecto sin riesgo de dejarlo a medias: escribe a `.tmp`, deja el
    /// archivo anterior como `.bak` y recién ahí lo cambia (`rename` es atómico).
    pub fn save(&self, video: &Path) -> Result<(), String> {
        let path = project_path(video).ok_or_else(|| format!("la ruta {} no sirve para guardar el proyecto", video.display()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| format!("no se pudo armar el proyecto: {e}"))?;
        let tmp = suffixed(&path, "tmp");
        let written = std::fs::File::create(&tmp).and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            // Que los datos estén en el disco antes del `rename`.
            file.sync_all()
        });
        // Si algo falla, el `.tmp` sobra: se limpia (el error que importa va en el `Err`).
        let cleanup = |tmp: &Path| std::fs::remove_file(tmp).ok();
        if let Err(e) = written {
            cleanup(&tmp);
            return Err(format!("no se pudo escribir {}: {e}", tmp.display()));
        }
        if path.exists() {
            let bak = suffixed(&path, "bak");
            if let Err(e) = std::fs::copy(&path, &bak) {
                cleanup(&tmp);
                return Err(format!("no se pudo copiar a {}: {e}", bak.display()));
            }
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            cleanup(&tmp);
            return Err(format!("no se pudo reemplazar {}: {e}", path.display()));
        }
        Ok(())
    }

    /// Aplica un preajuste de cámara: escribe el zoom, la velocidad, el seguimiento y
    /// el suavizado (la curva y el desenfoque quedan como están). Quien llama rehace
    /// los zooms (`tidy`), porque cambia la rampa.
    pub fn apply_style(&mut self, style: CameraStyle) {
        self.camera = style;
        self.camera_opts = CameraOpts { ease: self.camera_opts.ease, ..style.camera_opts() };
        self.motion = MotionOpts { blur: self.motion.blur, ..style.motion_opts() };
    }

    /// El preajuste que coincide con los números de ahora, o `None` si están
    /// personalizados.
    pub fn active_style(&self) -> Option<CameraStyle> {
        let (camera, motion) = (self.camera_opts, self.motion);
        // Primero el último que se aplicó: si dos coincidieran, ese manda.
        std::iter::once(self.camera).chain(CameraStyle::ALL).find(|style| {
            (camera.depth - style.scale()).abs() < 1e-6
                && camera.speed == style.speed()
                && motion.follow == style.follow()
                && (motion.smoothing - style.smoothing()).abs() < 1e-6
        })
    }

    /// Deja todos los números del proyecto dentro de sus límites (se hace al leerlo;
    /// sirve si se editaron a mano).
    pub fn clamp_options(&mut self) {
        self.cursor = self.cursor.clamped();
        self.look = self.look.clamped();
        self.camera_opts = self.camera_opts.clamped();
        self.motion = self.motion.clamped();
    }

    pub fn trim(&self, duration: f64) -> (f64, f64) {
        let (a, b) = self.trim.unwrap_or((0.0, duration));
        (a.clamp(0.0, duration), b.clamp(0.0, duration).max(a))
    }

    /// Los tramos que quedan, en orden: el recorte menos los cortes.
    pub fn segments(&self, duration: f64) -> Vec<(f64, f64)> {
        let (from, to) = self.trim(duration);
        let mut cuts: Vec<(f64, f64)> = self.cuts.iter().map(|(a, b)| (a.min(*b), a.max(*b))).collect();
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = Vec::new();
        let mut at = from;
        for (a, b) in cuts {
            if b <= at || a >= to {
                continue;
            }
            if a > at {
                out.push((at, a));
            }
            at = at.max(b);
        }
        if to > at {
            out.push((at, to));
        }
        out.retain(|(a, b)| b - a > 0.01);
        out
    }

    /// Lo que dura el exportado.
    #[allow(dead_code)] // la duración sin velocidades; el editor usa out_length
    pub fn length(&self, duration: f64) -> f64 {
        self.segments(duration).iter().map(|(a, b)| b - a).sum()
    }

    /// Si `t` cae fuera de lo que queda, el próximo momento que sí (o `None` al final).
    pub fn next_kept(&self, t: f64, duration: f64) -> Option<f64> {
        self.segments(duration).into_iter().find(|(_, b)| t < *b).map(|(a, _)| t.max(a))
    }

    /// Cuánto se nota cada texto en `t` (0–1).
    pub fn text_alpha(&self, t: f64) -> Vec<(usize, f64)> {
        self.texts
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                let a = fade(t, text.start, text.end, TEXT_FADE);
                (a > 0.0).then_some((i, a))
            })
            .collect()
    }

    /// Cuánto se nota cada zona tapada en `t` (0–1, con fundido de 0,25 s).
    pub fn blurs_at(&self, t: f64) -> Vec<(usize, f64)> {
        self.blurs
            .iter()
            .enumerate()
            .filter_map(|(i, blur)| {
                let a = fade(t, blur.start, blur.end, BLUR_FADE);
                (a > 0.0).then_some((i, a))
            })
            .collect()
    }

    // ---- Zooms ----

    /// Ordena los zooms y los deja limpios, para llamarla tras cualquier cambio:
    ///
    /// - los recorta a `0..duration` y quita los que quedan sin largo;
    /// - **junta** los automáticos separados por menos de 2 rampas (si no, la cámara
    ///   saldría a 1× y volvería a entrar: «respira»); se queda acercada y sigue al
    ///   cursor. Se queda con la escala mayor;
    /// - quita los manuales idénticos (mismo tramo ±0,05 s, misma escala y foco);
    /// - recorta los automáticos que pisan a uno manual (el manual manda); los
    ///   pedazos que sobran y duran menos de 2 rampas se descartan. Un automático que
    ///   queda pegado a un manual deja un instante a 1× entre los dos, porque cada uno
    ///   entra y sale con su propia rampa.
    ///
    /// Es idempotente.
    pub fn tidy(&mut self, duration: f64) {
        let ramp = self.camera_opts.ramp_secs();
        if duration > 0.0 {
            for z in &mut self.zooms {
                z.start = z.start.max(0.0);
                z.end = z.end.min(duration);
            }
        }
        // Sin largo (o con números rotos) no hay zoom.
        self.zooms.retain(|z| z.end - z.start > 0.05);
        self.zooms.sort_by(|a, b| a.start.total_cmp(&b.start));

        let mut manual: Vec<Zoom> = Vec::new();
        let mut autos: Vec<Zoom> = Vec::new();
        for z in std::mem::take(&mut self.zooms) {
            if z.auto {
                autos.push(z);
            } else if !manual.iter().any(|k| {
                (k.start - z.start).abs() <= 0.05 && (k.end - z.end).abs() <= 0.05 && (k.scale - z.scale).abs() < 1e-6 && k.focus == z.focus
            }) {
                manual.push(z);
            }
        }

        let mut merged: Vec<Zoom> = Vec::new();
        for z in autos {
            match merged.last_mut() {
                Some(prev) if z.start - prev.end < ramp * 2.0 => {
                    prev.end = prev.end.max(z.end);
                    prev.scale = prev.scale.max(z.scale);
                }
                _ => merged.push(z),
            }
        }

        let mut out = manual.clone();
        for auto in merged {
            let mut pieces = vec![(auto.start, auto.end)];
            for m in &manual {
                pieces = pieces
                    .into_iter()
                    .flat_map(|(a, b)| {
                        if m.end <= a || m.start >= b {
                            return vec![(a, b)];
                        }
                        let mut left = Vec::new();
                        if m.start > a {
                            left.push((a, m.start));
                        }
                        if m.end < b {
                            left.push((m.end, b));
                        }
                        left
                    })
                    .collect();
            }
            let changed = pieces.len() != 1 || pieces[0] != (auto.start, auto.end);
            let min = if changed { ramp * 2.0 } else { MIN_ZOOM };
            for (a, b) in pieces {
                if b - a >= min {
                    out.push(Zoom { start: a, end: b, scale: auto.scale, focus: auto.focus, auto: true });
                }
            }
        }
        // A igual inicio, el manual primero.
        out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.auto.cmp(&b.auto)));
        self.zooms = out;
    }

    /// Agrega un zoom a mano de `len` segundos desde `at`, con la profundidad
    /// de la cámara (`camera_opts.depth`) y siguiendo al cursor. Nunca duplica ni apila: si `at` cae dentro de
    /// un zoom, el nuevo empieza donde termina ese; y no pasa del siguiente ni de
    /// `duration`. `None` si lo que cabe dura menos de 0,3 s. Devuelve su índice con
    /// los zooms ya ordenados por inicio.
    pub fn add_zoom(&mut self, at: f64, len: f64, duration: f64) -> Option<usize> {
        self.zooms.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut start = at.max(0.0);
        // En orden de inicio, así un empujón no deja al nuevo dentro de uno que ya pasamos.
        for z in &self.zooms {
            if z.start <= start && start < z.end {
                start = z.end;
            }
        }
        let mut end = (start + len).min(duration);
        if let Some(next) = self.zooms.iter().map(|z| z.start).filter(|s| *s > start).min_by(f64::total_cmp) {
            end = end.min(next);
        }
        // `!(>=)` y no `<`: un número roto (NaN) tampoco cabe.
        if !(end - start >= MIN_ZOOM) {
            return None;
        }
        let scale = self.camera_opts.clamped().depth;
        self.zooms.push(Zoom { start, end, scale, focus: Focus::Auto, auto: false });
        self.zooms.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.zooms.iter().position(|z| !z.auto && z.start == start && z.end == end && z.scale == scale)
    }

    /// Los zooms del autofocus: uno por cada grupo de clics y cada racha de
    /// escritura, siguiendo al mouse (o al texto, mientras se escribe). Reemplaza a
    /// los automáticos de antes y respeta los que se hicieron a mano. Los que quedan
    /// cerca se juntan (`tidy`).
    #[allow(dead_code)] // la versión sin registro; el editor usa autofocus_track
    pub fn autofocus(&mut self, clicks_ms: &[u32], keys_ms: &[u32], duration: f64) {
        self.autofocus_with(clicks_ms, keys_ms, duration, None);
    }

    /// Lo mismo que `autofocus`, leyendo los clics y las teclas del registro, y
    /// además **parte** los zooms automáticos largos: uno de más de 12 s se corta
    /// donde el mouse estuvo quieto, sin clics ni teclas, más de 2,5 s (elige el hueco
    /// que deja los pedazos más parejos). El corte deja un hueco centrado de 2 rampas
    /// más 0,01 s: así `tidy` no vuelve a juntar los pedazos (junta solo si la
    /// separación es menor a 2 rampas). Cada pedazo dura al menos 2 rampas.
    pub fn autofocus_track(&mut self, track: &Track, duration: f64) {
        self.autofocus_with(&track.clicks(), &track.keys, duration, Some(track));
    }

    fn autofocus_with(&mut self, clicks_ms: &[u32], keys_ms: &[u32], duration: f64, track: Option<&Track>) {
        self.zooms.retain(|z| !z.auto);
        let ramp = self.camera_opts.ramp_secs();
        let scale = self.camera_opts.clamped().depth;
        let group = |times: &[u32], gap: f64| -> Vec<(f64, f64, usize)> {
            let mut groups: Vec<(f64, f64, usize)> = Vec::new();
            for ms in times {
                let t = *ms as f64 / 1000.0;
                match groups.last_mut() {
                    Some((_, last, n)) if t - *last <= gap => {
                        *last = t;
                        *n += 1;
                    }
                    _ => groups.push((t, t, 1)),
                }
            }
            groups
        };
        let mut moments: Vec<(f64, f64)> = group(clicks_ms, CLICK_GAP).into_iter().map(|(a, b, _)| (a - 0.4, b + 1.2)).collect();
        // Escribir: una racha de verdad (no un atajo suelto).
        moments.extend(
            group(keys_ms, TYPING_GAP)
                .into_iter()
                .filter(|(a, b, n)| *n >= TYPING_MIN_KEYS && b - a >= 0.6)
                .map(|(a, b, _)| (a - 0.3, b + 1.0)),
        );
        moments.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (first, last) in moments {
            let start = (first - ramp).max(0.0);
            let end = (last + ramp).min(duration);
            if end - start < ramp * 2.0 {
                continue;
            }
            self.zooms.push(Zoom { start, end, scale, focus: Focus::Auto, auto: true });
        }
        // Se juntan los que se pisan o quedan cerca y se respetan los manuales.
        self.tidy(duration);
        if let Some(track) = track {
            self.split_long_autos(track);
        }
    }

    /// Parte los automáticos largos donde no pasa nada (ver `autofocus_track`).
    fn split_long_autos(&mut self, track: &Track) {
        let ramp = self.camera_opts.ramp_secs();
        let events = activity(track);
        let mut out = Vec::new();
        for z in std::mem::take(&mut self.zooms) {
            if !z.auto {
                out.push(z);
                continue;
            }
            for (start, end) in split_at_idle(z.start, z.end, &events, ramp) {
                out.push(Zoom { start, end, scale: z.scale, focus: z.focus, auto: true });
            }
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.auto.cmp(&b.auto)));
        self.zooms = out;
    }

    // ---- Velocidades ----

    /// Agrega un tramo del original a `rate`× (acotado a 1,25..=16). No se pisan con
    /// otro tramo: si `start` cae dentro de uno, el nuevo empieza donde termina; y no
    /// pasa del siguiente. `None` si lo que cabe dura menos de 0,1 s (o los números
    /// no sirven). Devuelve su índice con los tramos ya ordenados por inicio.
    pub fn add_speed(&mut self, start: f64, end: f64, rate: f64) -> Option<usize> {
        if !(start.is_finite() && end.is_finite() && rate.is_finite()) {
            return None;
        }
        let rate = rate.clamp(1.25, 16.0);
        let (mut start, mut end) = (start.min(end).max(0.0), start.max(end));
        self.speeds.sort_by(|a, b| a.start.total_cmp(&b.start));
        for s in &self.speeds {
            if s.start <= start && start < s.end {
                start = s.end;
            }
        }
        if let Some(next) = self.speeds.iter().map(|s| s.start).filter(|s| *s > start).min_by(f64::total_cmp) {
            end = end.min(next);
        }
        if end - start < MIN_SPEED {
            return None;
        }
        self.speeds.push(Speed { start, end, rate });
        self.speeds.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.speeds.iter().position(|s| s.start == start && s.end == end && s.rate == rate)
    }

    /// Los tramos con velocidad `(inicio, fin, rate)` recortados a lo que queda tras
    /// el recorte y los cortes, en orden. Tiempos del original.
    pub fn speed_spans(&self, duration: f64) -> Vec<(f64, f64, f64)> {
        let segments = self.segments(duration);
        let mut speeds: Vec<&Speed> = self.speeds.iter().collect();
        speeds.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut out = Vec::new();
        // Por si un JSON a mano los pisó: cada uno empieza donde terminó el anterior.
        let mut floor = f64::NEG_INFINITY;
        for s in speeds {
            let (from, to) = (s.start.max(floor), s.end);
            floor = floor.max(to);
            for (a, b) in &segments {
                let (a, b) = (from.max(*a), to.min(*b));
                if b - a > 0.01 {
                    out.push((a, b, s.rate));
                }
            }
        }
        out
    }

    /// Cuánto dura en el exportado el tramo `a..b` del original (con velocidades).
    fn out_span(spans: &[(f64, f64, f64)], a: f64, b: f64) -> f64 {
        let saved: f64 = spans
            .iter()
            .map(|(sa, sb, rate)| (sb.min(b) - sa.max(a)).max(0.0) * (1.0 - 1.0 / rate))
            .sum();
        (b - a) - saved
    }

    /// El momento del exportado que corresponde a `src` (del original): lo que
    /// duran los tramos anteriores, con sus velocidades. `None` si `src` queda cortado
    /// (fuera del recorte o dentro de un corte).
    #[allow(dead_code)] // el tiempo de salida de un instante; lo usan los tests y el exportador tiene el suyo
    pub fn out_time(&self, src: f64, duration: f64) -> Option<f64> {
        let spans = self.speed_spans(duration);
        let mut before = 0.0;
        for (a, b) in self.segments(duration) {
            if src < a {
                return None;
            }
            if src <= b {
                return Some(before + Self::out_span(&spans, a, src));
            }
            before += Self::out_span(&spans, a, b);
        }
        None
    }

    /// Lo que dura de verdad el exportado (`length`, pero con las velocidades).
    pub fn out_length(&self, duration: f64) -> f64 {
        let spans = self.speed_spans(duration);
        self.segments(duration).iter().map(|(a, b)| Self::out_span(&spans, *a, *b)).sum()
    }

    // ---- Tramos muertos ----

    /// Tramos del original (inicio, fin) que se pueden cortar: sin sonido y sin
    /// actividad. Una ventana cuenta si el audio está bajo 0,04 (con `levels` vacío
    /// no se mira el audio), no hubo clics ni teclas y el mouse casi no se movió
    /// (menos de 0,5 % del cuadro). Une las ventanas seguidas, descarta los tramos de
    /// menos de `min_len` s y le quita 0,3 s a cada borde, para no comerse el comienzo
    /// de la acción. Sin audio ni registro no hay de dónde juzgar: devuelve nada.
    /// `levels` trae `per_second` niveles por segundo.
    pub fn suggest_idle(&self, levels: &[f32], per_second: u32, track: Option<&Track>, duration: f64, min_len: f64) -> Vec<(f64, f64)> {
        let has_audio = !levels.is_empty() && per_second > 0;
        if duration <= 0.0 || (!has_audio && track.is_none()) {
            return Vec::new();
        }
        // Sin audio, ventanas de una décima.
        let rate = if has_audio { per_second as f64 } else { 10.0 };
        let windows = (duration * rate).ceil() as usize;
        let events = track.map(activity).unwrap_or_default();
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for i in 0..windows {
            let (a, b) = (i as f64 / rate, ((i + 1) as f64 / rate).min(duration));
            let quiet = !has_audio || levels.get(i).map_or(true, |level| *level < QUIET_LEVEL);
            let still = events.partition_point(|e| *e < a) == events.partition_point(|e| *e < b);
            if !(quiet && still) {
                continue;
            }
            match spans.last_mut() {
                Some(last) if (a - last.1).abs() < 1e-9 => last.1 = b,
                _ => spans.push((a, b)),
            }
        }
        spans
            .into_iter()
            .filter(|(a, b)| b - a >= min_len)
            .map(|(a, b)| (a + IDLE_EDGE, b - IDLE_EDGE))
            .filter(|(a, b)| b - a > 0.05)
            .collect()
    }
}

/// Cuánto se nota algo que dura de `start` a `end` en `t` (0–1): entra y sale en `ramp`.
fn fade(t: f64, start: f64, end: f64, ramp: f64) -> f64 {
    ((t - start) / ramp).min((end - t) / ramp).clamp(0.0, 1.0)
}

/// Cuándo (segundos) se movió el mouse: cada vez que se aleja `eps` (parte del
/// cuadro) de donde quedó la vez anterior. Mide contra ese punto y no entre muestras
/// seguidas, así un arrastre lento también cuenta.
fn move_events(track: &Track, eps: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut anchor: Option<(f64, f64)> = None;
    for s in &track.samples {
        let at = (s.1 as f64, s.2 as f64);
        match anchor {
            None => anchor = Some(at),
            Some(a) if (at.0 - a.0).hypot(at.1 - a.1) >= eps => {
                out.push(s.0 as f64 / 1000.0);
                anchor = Some(at);
            }
            _ => {}
        }
    }
    out
}

/// Cuándo (segundos, en orden) pasó algo: el mouse se movió, un clic o una tecla.
fn activity(track: &Track) -> Vec<f64> {
    let mut out = move_events(track, IDLE_MOVE);
    out.extend(track.clicks().into_iter().map(|ms| ms as f64 / 1000.0));
    out.extend(track.keys.iter().map(|ms| *ms as f64 / 1000.0));
    out.sort_by(f64::total_cmp);
    out
}

/// Parte `a..b` (un zoom automático) en el hueco de actividad que lo deja más parejo,
/// mientras dure más de `LONG_ZOOM`. `events` va en orden. Ver `autofocus_track`.
fn split_at_idle(a: f64, b: f64, events: &[f64], ramp: f64) -> Vec<(f64, f64)> {
    if b - a <= LONG_ZOOM {
        return vec![(a, b)];
    }
    let hole = ramp * 2.0 + 0.01;
    let first = events.partition_point(|e| *e <= a);
    let last = events.partition_point(|e| *e < b);
    let mut edges = vec![a];
    edges.extend_from_slice(&events[first..last]);
    edges.push(b);
    let middle = (a + b) / 2.0;
    let mut best: Option<f64> = None;
    for w in edges.windows(2) {
        let mid = (w[0] + w[1]) / 2.0;
        // Un hueco de verdad, y que los dos pedazos duren al menos 2 rampas.
        if w[1] - w[0] > IDLE_SPLIT && mid - hole / 2.0 - a >= ramp * 2.0 && b - (mid + hole / 2.0) >= ramp * 2.0 && best.map_or(true, |m| (mid - middle).abs() < (m - middle).abs()) {
            best = Some(mid);
        }
    }
    let Some(mid) = best else { return vec![(a, b)] };
    let mut out = split_at_idle(a, mid - hole / 2.0, events, ramp);
    out.extend(split_at_idle(mid + hole / 2.0, b, events, ramp));
    out
}

/// La curva de entrada y salida del zoom: arranca y llega sin tirón (derivadas
/// primera y segunda en cero en los dos extremos).
fn smootherstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

/// El zoom en `t`: la escala de ahora, la del tramo completo (hacia la que va) y
/// hacia dónde apunta. La rampa y la curva salen de `camera`.
fn zoom_at(zooms: &[Zoom], t: f64, camera: &CameraOpts) -> (f64, f64, Option<Focus>) {
    let ramp = camera.ramp_secs();
    let mut best = (1.0, 1.0, None);
    for z in zooms {
        let a = camera.ease.apply((t - z.start) / ramp) * camera.ease.apply((z.end - t) / ramp);
        if a <= 0.0 {
            continue;
        }
        let full = z.scale.max(1.0);
        let scale = 1.0 + (full - 1.0) * a;
        if scale > best.0 {
            best = (scale, full, Some(z.focus));
        }
    }
    best
}

/// El ancho (s) de la campana que suaviza la cámara con el suavizado en 1,0: hacia los
/// dos lados, así no llega tarde y no tiembla.
const CAMERA_SIGMA: f64 = 0.22;
/// La corrección de la banda de guarda se aparta del cero con una curvatura que cuesta
/// `(GUARD_LEN · RATE)⁴`: el largo (s) con que se ensancha un empujón.
const GUARD_LEN: f64 = 0.12;
/// Lo que se agrega (s) a cada lado de una violación de la banda al armar el tramo que se corrige.
const GUARD_PAD: f64 = 0.7;
/// ADMM: vueltas como máximo y cuándo se da por resuelto (el peso del acople es el
/// medio geométrico de lo que va entre 1 y la curvatura, `4·(GUARD_LEN · RATE)²`).
const ADMM_ITERS: usize = 400;
const ADMM_TOL: f64 = 1e-5;
/// Mientras se escribe: zona muerta y banda de guarda (parte de media vista). El texto
/// queda cerca del centro: ahí se está mirando.
const TYPING_DEAD: f64 = 0.12;
const TYPING_GUARD: f64 = 0.45;
/// El paso del mouse al texto (y de vuelta) se reparte con una campana de este ancho (s),
/// así lo que sigue la cámara no salta.
const FOLLOW_BLEND: f64 = 0.15;
/// Menos que esto de pasarse de la banda no se corrige.
const GUARD_EPS: f64 = 1e-4;

/// Qué hace la cámara en un cuadro, en un eje.
#[derive(Clone, Copy, PartialEq)]
enum Aim {
    /// Mira a un punto fijo.
    Fixed(f64),
    /// Sigue al cursor (o al texto) con zona muerta y banda de guarda.
    Follow,
    /// Sin zoom: se ve todo; queda sobre el cursor, lista para el próximo.
    Free,
}

/// Suaviza `values` con una campana de `sigma` cuadros, sin correrla en el tiempo.
fn gaussian(values: &[f64], sigma: f64) -> Vec<f64> {
    if values.is_empty() || sigma < 0.5 {
        return values.to_vec();
    }
    let radius = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f64> = (-radius..=radius).map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let n = values.len() as isize;
    (0..n)
        .map(|i| {
            let (mut sum, mut weight) = (0.0, 0.0);
            for (k, w) in kernel.iter().enumerate() {
                let j = (i + k as isize - radius).clamp(0, n - 1);
                sum += values[j as usize] * w;
                weight += w;
            }
            sum / weight
        })
        .collect()
}

/// Un resorte críticamente amortiguado (sin rebote) un paso `dt` hacia `target`.
fn spring(pos: &mut f64, vel: &mut f64, target: f64, omega: f64, dt: f64) {
    let x = *pos - target;
    let e = (-omega * dt).exp();
    let temp = (*vel + omega * x) * dt;
    *vel = (*vel - omega * temp) * e;
    *pos = target + (x + temp) * e;
}

/// La cámara y el cursor, ya integrados a 60 Hz.
pub struct Motion {
    /// Por cuadro: centro (x, y) y escala de la cámara.
    camera: Vec<(f64, f64, f64)>,
    /// Por cuadro: el cursor (suavizado si se pidió).
    cursor: Vec<(f64, f64)>,
    /// Por cuadro: lo que sigue la cámara (el cursor o el texto que se escribe: x, y y
    /// cuánto es texto, 0–1).
    followed: Vec<(f64, f64, f64)>,
    /// Por cuadro: cuánto está apretado el botón (0–1, suave): el cursor se encoge.
    press: Vec<f64>,
    /// Por cuadro: cuánto se ve el cursor (0–1; se esconde si lleva rato quieto).
    alpha: Vec<f64>,
    /// Por cuadro: la forma del cursor del sistema (`Track::shape_at`).
    shapes: Vec<u8>,
    /// Los clics: cuándo y dónde (0–1 del video), para la onda.
    clicks: Vec<(f64, f64, f64)>,
    /// El efecto de los clics (con `None` no hay ondas).
    click: ClickKind,
    press_shrink: bool,
    has_cursor: bool,
}

/// La posición cruda del registro en `t`, interpolada.
fn raw_cursor(track: &Track, t: f64) -> Option<(f64, f64)> {
    let ms = (t * 1000.0) as u32;
    let samples = &track.samples;
    let i = samples.partition_point(|s| s.0 <= ms);
    match (i.checked_sub(1).and_then(|j| samples.get(j)), samples.get(i)) {
        (Some(a), Some(b)) if b.0 > a.0 => {
            let k = (ms - a.0) as f64 / (b.0 - a.0) as f64;
            Some((a.1 as f64 + (b.1 - a.1) as f64 * k, a.2 as f64 + (b.2 - a.2) as f64 * k))
        }
        (Some(a), _) => Some((a.1 as f64, a.2 as f64)),
        (None, Some(b)) => Some((b.1 as f64, b.2 as f64)),
        (None, None) => None,
    }
}

/// Si el botón estaba apretado en `t`.
fn pressed_at(track: &Track, t: f64) -> bool {
    let ms = (t * 1000.0) as u32;
    let i = track.samples.partition_point(|s| s.0 <= ms);
    i.checked_sub(1).and_then(|j| track.samples.get(j)).is_some_and(|s| s.3)
}

/// Cuánto se ve el cursor en cada uno de `frames` cuadros: 1, salvo que lleve más de
/// `idle` s sin moverse ni hacer clic; ahí baja a 0 en 0,4 s, y al moverse vuelve a 1 en
/// 0,15 s (desde donde iba, sin saltos).
fn cursor_alpha_curve(track: &Track, frames: usize, idle: f64) -> Vec<f64> {
    let mut events = move_events(track, CURSOR_MOVE);
    events.extend(track.clicks().into_iter().map(|ms| ms as f64 / 1000.0));
    events.sort_by(f64::total_cmp);
    let dt = 1.0 / RATE;
    let (mut level, mut last, mut next) = (1.0f64, 0.0f64, 0usize);
    (0..frames)
        .map(|i| {
            let t = i as f64 * dt;
            while next < events.len() && events[next] <= t {
                last = events[next];
                next += 1;
            }
            level = if t - last > idle { (level - dt / CURSOR_HIDE).max(0.0) } else { (level + dt / CURSOR_SHOW).min(1.0) };
            smootherstep(level)
        })
        .collect()
}

/// Une los huecos de `values` (`None`) con el valor anterior y, al comienzo, con el
/// siguiente. Sin ningún valor: `fallback`.
fn fill_gaps(values: &[Option<(f64, f64)>], fallback: (f64, f64)) -> Vec<(f64, f64)> {
    let mut out = vec![fallback; values.len()];
    let mut last = values.iter().flatten().next().copied().unwrap_or(fallback);
    for (i, v) in values.iter().enumerate() {
        if let Some(v) = v {
            last = *v;
        }
        out[i] = last;
    }
    out
}

/// Resuelve `(a·I + μ·D₂ᵀD₂) x = rhs` (D₂: segunda diferencia, con ceros fuera) por
/// Cholesky de banda 2. Devuelve una función que resuelve para cada lado derecho.
struct Banded {
    l0: Vec<f64>,
    l1: Vec<f64>,
    l2: Vec<f64>,
}

impl Banded {
    fn new(m: usize, a: f64, mu: f64) -> Self {
        let (mut l0, mut l1, mut l2) = (vec![0.0; m], vec![0.0; m], vec![0.0; m]);
        for i in 0..m {
            if i >= 2 {
                l2[i] = mu / l0[i - 2];
            }
            if i >= 1 {
                l1[i] = (-4.0 * mu - l2[i] * if i >= 2 { l1[i - 1] } else { 0.0 }) / l0[i - 1];
            }
            l0[i] = (a + 6.0 * mu - l1[i] * l1[i] - l2[i] * l2[i]).sqrt();
        }
        Self { l0, l1, l2 }
    }

    fn solve(&self, rhs: &[f64]) -> Vec<f64> {
        let m = rhs.len();
        let mut y = vec![0.0; m];
        for i in 0..m {
            let (p1, p2) = (if i >= 1 { y[i - 1] } else { 0.0 }, if i >= 2 { y[i - 2] } else { 0.0 });
            y[i] = (rhs[i] - self.l1[i] * p1 - self.l2[i] * p2) / self.l0[i];
        }
        let mut x = vec![0.0; m];
        for i in (0..m).rev() {
            let (n1, n2) = (if i + 1 < m { x[i + 1] } else { 0.0 }, if i + 2 < m { x[i + 2] } else { 0.0 });
            let (c1, c2) = (if i + 1 < m { self.l1[i + 1] } else { 0.0 }, if i + 2 < m { self.l2[i + 2] } else { 0.0 });
            x[i] = (y[i] - c1 * n1 - c2 * n2) / self.l0[i];
        }
        x
    }
}

/// Una corrección `c` para que `base + c` quede dentro de `lo..=hi` (por cuadro; sin
/// límite con ±∞): la de menor tamaño y curvatura, `min ½|c|² + μ/2·|D₂c|²` sujeto a
/// los límites, resuelta con ADMM solo alrededor de donde `base` se sale. Fuera de esos
/// tramos es cero; adentro es una curva suave que sube antes y baja después de la
/// violación. Lo que queda por resolver es del orden de `ADMM_TOL`, y se recorta.
fn guard_correction(base: &[f64], lo: &[f64], hi: &[f64]) -> Vec<f64> {
    let n = base.len();
    let mut out = vec![0.0; n];
    let outside = |i: usize| base[i] < lo[i] - GUARD_EPS || base[i] > hi[i] + GUARD_EPS;
    let pad = (GUARD_PAD * RATE).ceil() as usize;
    let mu = (GUARD_LEN * RATE).powi(4);
    let rho = 4.0 * mu.sqrt();
    let mut i = 0;
    while i < n {
        if !outside(i) {
            i += 1;
            continue;
        }
        // El tramo: desde `pad` antes de la primera violación hasta `pad` después de la
        // última de las que quedan a menos de `2·pad` una de otra.
        let mut last = i;
        let mut j = i + 1;
        while j < n && j <= last + 2 * pad {
            if outside(j) {
                last = j;
            }
            j += 1;
        }
        let (a, b) = (i.saturating_sub(pad), (last + pad).min(n - 1));
        let m = b - a + 1;
        let (low, high): (Vec<f64>, Vec<f64>) = (a..=b).map(|k| (lo[k] - base[k], hi[k] - base[k])).unzip();
        let chol = Banded::new(m, 1.0 + rho, mu);
        let (mut z, mut u, mut c) = (vec![0.0; m], vec![0.0; m], vec![0.0; m]);
        for _ in 0..ADMM_ITERS {
            let rhs: Vec<f64> = (0..m).map(|k| rho * (z[k] - u[k])).collect();
            c = chol.solve(&rhs);
            let mut delta: f64 = 0.0;
            for k in 0..m {
                z[k] = (c[k] + u[k]).clamp(low[k], high[k]);
                u[k] += c[k] - z[k];
                delta = delta.max((c[k] - z[k]).abs());
            }
            if delta < ADMM_TOL {
                break;
            }
        }
        for k in 0..m {
            out[a + k] = c[k].clamp(low[k], high[k]);
        }
        i = b + 1;
    }
    out
}

/// La cámara en un eje (ver `Motion::new`): el centro por cuadro.
///
/// - `point`: lo que se sigue (cursor o texto), por cuadro.
/// - `half`: media vista del zoom completo del tramo (parte del video).
/// - `seen`: media vista de este cuadro (mayor que `half` mientras el zoom entra o sale):
///   el centro posible sale de ella, así el camino pegado al borde sigue la rampa y no
///   la cruza con un quiebre.
/// - `room` y `guard`: la zona muerta y la banda de guarda (partes de `half`).
/// - `aim`: qué hace la cámara en cada cuadro.
/// - `sigma`: el ancho de la campana, en cuadros.
fn camera_axis(point: &[f64], half: &[f64], seen: &[f64], room: &[f64], guard: &[f64], aim: &[Aim], sigma: f64) -> Vec<f64> {
    let n = point.len();
    // a. El camino perezoso: quieta mientras el cursor esté en la zona muerta; al
    //    salir, lo acompaña justo lo que haga falta.
    let mut d = point.first().copied().unwrap_or(0.5);
    let lazy: Vec<f64> = (0..n)
        .map(|i| {
            d = match aim[i] {
                Aim::Fixed(v) => v,
                Aim::Follow => d.clamp(point[i] - room[i] * half[i], point[i] + room[i] * half[i]),
                Aim::Free => point[i],
            };
            // Lo que se ve no se sale del video: el centro posible depende de la escala.
            d = d.clamp(seen[i], 1.0 - seen[i]);
            d
        })
        .collect();
    // b. Suave y a tiempo: la campana hacia los dos lados (se adelanta sola al cursor).
    let mut cam = gaussian(&lazy, sigma);
    // c. La banda de guarda: el centro tiene que quedar a menos de `guard` del cursor. La
    //    campana corta las esquinas y el cursor se asoma en los giros bruscos; lo que
    //    falta se suma como la corrección más chica y suave que lo deje adentro
    //    (`guard_correction`), que además empieza antes de que el cursor llegue al límite.
    let (mut lo, mut hi) = (vec![f64::NEG_INFINITY; n], vec![f64::INFINITY; n]);
    for i in (0..n).filter(|i| aim[*i] == Aim::Follow) {
        let reach = guard[i] * half[i];
        // Sin sitio para el cursor dentro del video, la cámara se pega al borde. Y el
        // centro tampoco sale del video: si el camino lo cruzara con velocidad, el recorte
        // de la vista lo frenaría de golpe.
        lo[i] = (point[i] - reach).min(1.0 - seen[i]).max(seen[i]);
        hi[i] = (point[i] + reach).max(seen[i]).min(1.0 - seen[i]);
    }
    let fix = guard_correction(&cam, &lo, &hi);
    for i in 0..n {
        cam[i] += fix[i];
    }
    cam
}

impl Motion {
    /// La cámara y el cursor de todo el video, cuadro a cuadro.
    ///
    /// **El cursor** es un resorte rápido (`smooth_omega`) que solo quita los tirones.
    ///
    /// **La cámara** sigue al cursor (o al cursor de texto, mientras se escribe) con
    /// `MotionOpts::follow`. Se conoce todo el registro, así que se adelanta en vez de
    /// llegar tarde. Por eje (`camera_axis`), con las medidas en partes de media vista:
    /// 1. *Camino perezoso.* El cursor puede moverse dentro de una zona muerta
    ///    (`Follow::dead_zone`) sin que la cámara se mueva: no marea con
    ///    micromovimientos. Al salir de la zona la cámara lo acompaña justo lo necesario.
    ///    La zona se mide con el zoom completo del tramo y no con el de este cuadro: así,
    ///    mientras el zoom entra, la cámara ya va hacia donde va a quedar. El centro no sale
    ///    del video con la escala de este cuadro, así el camino pegado al borde sigue la
    ///    rampa del zoom sin quiebres.
    /// 2. *Campana.* Ese camino se suaviza con una gaussiana hacia los dos lados
    ///    (`CAMERA_SIGMA` × `smoothing` × el factor del modo). No tiene esquinas y, como
    ///    mira también el futuro, la cámara empieza a moverse antes de que el cursor salga
    ///    de la zona.
    /// 3. *Banda de guarda.* La campana corta las esquinas y el cursor se asoma en los
    ///    giros bruscos, así que el centro queda obligado a estar a menos de
    ///    `Follow::guard` del cursor (y dentro del video). Es una restricción dura, sin
    ///    quiebres: se suma la corrección más chica y suave que la cumpla
    ///    (`guard_correction`, un problema cuadrático con cotas que se resuelve solo
    ///    donde el camino se sale). Como mira el futuro, la corrección sube antes de que
    ///    el cursor llegue al límite. Donde el cursor está pegado al borde del video y la
    ///    cámara no puede ir más lejos, la cámara se queda en el borde.
    /// 4. *Texto.* Mientras se escribe, lo que se sigue es el cursor de texto con una
    ///    zona muerta y una guarda propias (`TYPING_DEAD`, `TYPING_GUARD`); el paso del
    ///    mouse al texto (y de vuelta) se reparte en `FOLLOW_BLEND` para no dar saltos.
    pub fn new(project: &Project, track: Option<&Track>, duration: f64) -> Self {
        let frames = (duration * RATE).ceil() as usize + 2;
        let dt = 1.0 / RATE;
        let (camera_opts, motion, cursor_opts) = (project.camera_opts.clamped(), project.motion.clamped(), project.cursor.clamped());
        let mut cursor = Vec::with_capacity(frames);
        let mut press = Vec::with_capacity(frames);
        let (mut down, mut vdown) = (0.0, 0.0);
        let (mut px, mut py, mut pvx, mut pvy) = (0.5, 0.5, 0.0, 0.0);
        if let Some((x, y)) = track.and_then(|track| raw_cursor(track, 0.0)) {
            (px, py) = (x, y);
        }
        // 1. El cursor.
        let omega = smooth_omega(cursor_opts.smooth_level);
        for i in 0..frames {
            let t = i as f64 * dt;
            let raw = track.and_then(|track| raw_cursor(track, t)).unwrap_or((px, py));
            if let Some(omega) = omega {
                spring(&mut px, &mut pvx, raw.0, omega, dt);
                spring(&mut py, &mut pvy, raw.1, omega, dt);
            } else {
                (px, py) = raw;
            }
            cursor.push((px, py));
            let pressed = track.is_some_and(|track| pressed_at(track, t));
            spring(&mut down, &mut vdown, if pressed { 1.0 } else { 0.0 }, 30.0, dt);
            press.push(down.clamp(0.0, 1.0));
        }
        // 2. Qué sigue la cámara: el cursor, o el texto mientras se escribe.
        let keys: &[u32] = track.map_or(&[], |t| &t.keys);
        let carets: &[(u32, f32, f32)] = track.map_or(&[], |t| &t.carets);
        let caret_at: Vec<Option<(f64, f64)>> = (0..frames)
            .map(|i| {
                let ms = (i as f64 * dt * 1000.0) as u32;
                let typing = keys.partition_point(|k| *k <= ms).checked_sub(1).is_some_and(|k| ((ms - keys[k]) as f64) < TYPING_HOLD * 1000.0);
                typing
                    .then(|| carets.partition_point(|c| c.0 <= ms).checked_sub(1).map(|c| carets[c]))
                    .flatten()
                    .filter(|c| ((ms - c.0) as f64) < TYPING_HOLD * 1000.0 * 2.0)
                    .map(|(_, cx, cy)| (cx as f64, cy as f64))
            })
            .collect();
        let active: Vec<f64> = caret_at.iter().map(|c| if c.is_some() { 1.0 } else { 0.0 }).collect();
        let blend = if active.iter().any(|a| *a > 0.0) { gaussian(&active, FOLLOW_BLEND * RATE) } else { active };
        let caret = fill_gaps(&caret_at, (0.5, 0.5));
        let followed: Vec<(f64, f64, f64)> = (0..frames)
            .map(|i| {
                let w = blend[i];
                (cursor[i].0 + (caret[i].0 - cursor[i].0) * w, cursor[i].1 + (caret[i].1 - cursor[i].1) * w, w)
            })
            .collect();
        // 3. Hacia dónde mira la cámara en cada cuadro.
        let mut scales = Vec::with_capacity(frames);
        let (mut half, mut seen, mut room, mut guard) = (Vec::with_capacity(frames), Vec::with_capacity(frames), Vec::with_capacity(frames), Vec::with_capacity(frames));
        let (mut aim_x, mut aim_y) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
        for (i, &(_, _, w)) in followed.iter().enumerate() {
            let (scale, full, focus) = zoom_at(&project.zooms, i as f64 * dt, &camera_opts);
            let (ax, ay) = match focus {
                Some(Focus::At(fx, fy)) => (Aim::Fixed(fx), Aim::Fixed(fy)),
                Some(Focus::Auto) => (Aim::Follow, Aim::Follow),
                None => (Aim::Free, Aim::Free),
            };
            aim_x.push(ax);
            aim_y.push(ay);
            half.push(0.5 / full);
            seen.push(0.5 / scale);
            room.push(motion.follow.dead_zone() + (TYPING_DEAD - motion.follow.dead_zone()) * w);
            guard.push(motion.follow.guard() + (TYPING_GUARD - motion.follow.guard()) * w);
            scales.push(scale);
        }
        let sigma = CAMERA_SIGMA * motion.smoothing * motion.follow.sigma_scale() * RATE;
        let (point_x, point_y): (Vec<f64>, Vec<f64>) = followed.iter().map(|&(x, y, _)| (x, y)).unzip();
        let cam_x = camera_axis(&point_x, &half, &seen, &room, &guard, &aim_x, sigma);
        let cam_y = camera_axis(&point_y, &half, &seen, &room, &guard, &aim_y, sigma);
        let camera = (0..frames).map(|i| (cam_x[i], cam_y[i], scales[i])).collect();
        let clicks = track
            .map(|track| {
                track.clicks().into_iter().filter_map(|ms| {
                    let t = ms as f64 / 1000.0;
                    raw_cursor(track, t).map(|(x, y)| (t, x, y))
                }).collect()
            })
            .unwrap_or_default();
        let alpha = match track {
            Some(track) if cursor_opts.hide_idle => cursor_alpha_curve(track, frames, cursor_opts.hide_delay),
            _ => vec![1.0; frames],
        };
        let shapes: Vec<u8> = track.map_or_else(Vec::new, |track| (0..frames).map(|i| track.shape_at((i as f64 * dt * 1000.0) as u32)).collect());
        Self {
            camera,
            cursor,
            followed,
            press,
            alpha,
            shapes,
            clicks,
            click: cursor_opts.click,
            press_shrink: cursor_opts.press_shrink,
            has_cursor: track.is_some_and(|t| !t.samples.is_empty()),
        }
    }

    /// Las ondas visibles en `t`: dónde (0–1 del video) y cuánto van (0–1).
    pub fn ripples(&self, t: f64) -> Vec<(f64, f64, f64)> {
        if self.click == ClickKind::None {
            return Vec::new();
        }
        self.clicks
            .iter()
            .filter(|(at, _, _)| t >= *at && t < at + RIPPLE)
            .map(|(at, x, y)| (*x, *y, (t - at) / RIPPLE))
            .collect()
    }

    /// Cuánto se movió la cámara entre el cuadro anterior y `t`: el corrimiento de lo
    /// que se ve (en partes del ancho y el alto de la vista) y cuánto cambió el zoom
    /// (el ancho visto antes sobre el de ahora). Para el desenfoque de movimiento.
    pub fn camera_motion(&self, t: f64) -> (f64, f64, f64) {
        let [x1, y1, w1, h1] = self.crop(t);
        let [x0, y0, w0, h0] = self.crop(t - 1.0 / RATE);
        let (cx1, cy1) = (x1 + w1 / 2.0, y1 + h1 / 2.0);
        let (cx0, cy0) = (x0 + w0 / 2.0, y0 + h0 / 2.0);
        ((cx1 - cx0) / w1, (cy1 - cy0) / h1, w0 / w1)
    }

    /// El tamaño del cursor en `t` (1: normal; un poco menos al hacer clic si el
    /// cursor se encoge al apretar).
    pub fn cursor_scale(&self, t: f64) -> f64 {
        if !self.press_shrink {
            return 1.0;
        }
        1.0 - PRESS_SHRINK * self.press.get(self.index(t)).copied().unwrap_or(0.0)
    }

    /// Cuánto se ve el cursor en `t` (0–1): se esconde si lleva más de `hide_delay` s quieto.
    pub fn cursor_alpha(&self, t: f64) -> f64 {
        self.alpha.get(self.index(t)).copied().unwrap_or(1.0)
    }

    /// La forma del cursor del sistema en `t`: 0 flecha, 1 mano, 2 I-beam, 3 cruz.
    pub fn cursor_shape(&self, t: f64) -> u8 {
        self.shapes.get(self.index(t)).copied().unwrap_or(0)
    }

    fn index(&self, t: f64) -> usize {
        ((t.max(0.0) * RATE).round() as usize).min(self.camera.len().saturating_sub(1))
    }

    /// Lo que se ve en `t`: x, y, ancho y alto (0–1), dentro del video.
    pub fn crop(&self, t: f64) -> [f64; 4] {
        let Some(&(cx, cy, scale)) = self.camera.get(self.index(t)) else {
            return [0.0, 0.0, 1.0, 1.0];
        };
        let size = 1.0 / scale.max(1.0);
        let half = size / 2.0;
        let x = cx.clamp(half, 1.0 - half) - half;
        let y = cy.clamp(half, 1.0 - half) - half;
        [x, y, size, size]
    }

    /// Lo que sigue la cámara en `t`: x, y (0–1 del video) y cuánto es texto (0–1; con 0
    /// es el cursor).
    #[allow(dead_code)] // para medir la cámara
    pub fn followed(&self, t: f64) -> Option<(f64, f64, f64)> {
        self.has_cursor.then(|| self.followed.get(self.index(t)).copied()).flatten()
    }

    pub fn cursor(&self, t: f64) -> Option<(f64, f64)> {
        self.has_cursor.then(|| self.cursor.get(self.index(t)).copied()).flatten()
    }
}

/// Deshacer y rehacer: instantáneas del `Project` (hasta `HISTORY_MAX` pasos). El
/// editor llama a `record` con el proyecto ya cambiado; `undo` y `redo` devuelven
/// el proyecto al que volver.
#[derive(Clone, Debug)]
pub struct History {
    /// Los estados anotados, del más viejo al más nuevo; `steps[pos]` es el de ahora.
    steps: Vec<Project>,
    pos: usize,
    /// La clave y la hora del último `record` con clave (para agrupar arrastres).
    last: Option<(&'static str, Instant)>,
}

impl History {
    pub fn new(initial: &Project) -> Self {
        Self { steps: vec![initial.clone()], pos: 0, last: None }
    }

    /// Anota el proyecto tras un cambio. Si no cambió nada, no hace nada. Con la misma
    /// `key` que el paso anterior y dentro de 600 ms, lo reemplaza en vez de sumar otro
    /// (un arrastre es un solo paso). Anotar tras deshacer descarta lo que se podía rehacer.
    pub fn record(&mut self, project: &Project, key: Option<&'static str>) {
        self.record_at(project, key, Instant::now());
    }

    /// `record` con la hora dada (para probar sin dormir).
    pub fn record_at(&mut self, project: &Project, key: Option<&'static str>, now: Instant) {
        if *project == self.steps[self.pos] {
            return;
        }
        self.steps.truncate(self.pos + 1);
        let same = match (key, self.last) {
            (Some(key), Some((last, at))) => key == last && now.saturating_duration_since(at) <= HISTORY_GROUP,
            _ => false,
        };
        self.last = key.map(|key| (key, now));
        if same && self.pos > 0 {
            self.steps[self.pos] = project.clone();
            // El arrastre volvió a donde empezó: ese paso no cambia nada.
            if self.steps[self.pos] == self.steps[self.pos - 1] {
                self.steps.pop();
                self.pos -= 1;
                self.last = None;
            }
            return;
        }
        self.steps.push(project.clone());
        self.pos += 1;
        if self.steps.len() > HISTORY_MAX + 1 {
            self.steps.remove(0);
            self.pos -= 1;
        }
    }

    /// Si `project` es el último paso anotado: no hay nada que registrar ni recalcular
    /// (tocar una opción que ya estaba elegida).
    pub fn unchanged(&self, project: &Project) -> bool {
        *project == self.steps[self.pos]
    }

    /// El proyecto anterior, o `None` si no hay. Si `current` tiene cambios sin anotar,
    /// vuelve al último anotado y esos cambios quedan para rehacer.
    pub fn undo(&mut self, current: &Project) -> Option<Project> {
        if *current != self.steps[self.pos] {
            self.steps.truncate(self.pos + 1);
            self.steps.push(current.clone());
            self.pos += 1;
        }
        if self.pos == 0 {
            return None;
        }
        self.pos -= 1;
        self.last = None;
        Some(self.steps[self.pos].clone())
    }

    /// El proyecto siguiente, o `None` si no hay (o si `current` tiene cambios sin
    /// anotar: rehacer los pisaría).
    pub fn redo(&mut self, current: &Project) -> Option<Project> {
        if *current != self.steps[self.pos] || self.pos + 1 >= self.steps.len() {
            return None;
        }
        self.pos += 1;
        self.last = None;
        Some(self.steps[self.pos].clone())
    }

    pub fn can_undo(&self) -> bool {
        self.pos > 0
    }

    pub fn can_redo(&self) -> bool {
        self.pos + 1 < self.steps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_cursor::Sample;

    #[test]
    fn los_cortes_y_el_recorte_dejan_los_tramos() {
        let project = Project { trim: Some((1.0, 9.0)), cuts: vec![(3.0, 4.0), (0.0, 2.0), (8.5, 12.0)], ..Default::default() };
        assert_eq!(project.segments(10.0), vec![(2.0, 3.0), (4.0, 8.5)]);
        assert!((project.length(10.0) - 5.5).abs() < 1e-9);
        assert_eq!(project.next_kept(3.2, 10.0), Some(4.0));
        assert_eq!(project.next_kept(9.0, 10.0), None);
    }

    #[test]
    fn los_clics_juntos_hacen_un_solo_zoom() {
        let mut project = Project::default();
        project.autofocus(&[2000, 2800, 3500, 9000], &[], 20.0);
        assert_eq!(project.zooms.len(), 2);
        assert!(project.zooms[0].start < 2.0 && project.zooms[0].end > 3.5);
        // Rehacerlo no duplica.
        project.autofocus(&[2000, 2800, 3500, 9000], &[], 20.0);
        assert_eq!(project.zooms.len(), 2);
    }

    #[test]
    fn escribir_un_rato_hace_un_zoom_y_un_atajo_no() {
        let mut project = Project::default();
        let typing: Vec<u32> = (0..12).map(|i| 5000 + i * 150).collect();
        project.autofocus(&[], &typing, 20.0);
        assert_eq!(project.zooms.len(), 1);
        let mut project = Project::default();
        project.autofocus(&[], &[9000, 9100], 20.0);
        assert!(project.zooms.is_empty());
    }

    #[test]
    fn escribiendo_la_camara_va_al_texto() {
        let samples = vec![Sample(0, 0.1, 0.1, false), Sample(6000, 0.1, 0.1, false)];
        let keys: Vec<u32> = (0..20).map(|i| 2000 + i * 150).collect();
        let carets = vec![(2000, 0.8, 0.8), (3000, 0.8, 0.8), (4500, 0.8, 0.8)];
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: keys.clone(), carets, shapes: Vec::new() };
        let mut project = Project::default();
        project.autofocus(&[], &keys, 6.0);
        let motion = Motion::new(&project, Some(&track), 6.0);
        let [x, y, w, h] = motion.crop(3.5);
        assert!(x + w > 0.8 && y + h > 0.8, "la vista {x:.2},{y:.2} {w:.2} no llega al texto");
    }

    #[test]
    fn el_autofocus_respeta_los_zooms_a_mano() {
        let mut project = Project::default();
        project.zooms.push(Zoom { start: 1.0, end: 5.0, scale: 2.0, focus: Focus::At(0.2, 0.2), auto: false });
        project.autofocus(&[3000], &[], 20.0);
        assert_eq!(project.zooms.len(), 1);
        assert!(!project.zooms[0].auto);
    }

    #[test]
    fn la_camara_no_se_sale_del_video_y_vuelve_sin_zoom() {
        let track = Track { version: 1, region: [0, 0, 100, 100], samples: vec![Sample(0, 0.98, 0.02, false), Sample(5000, 0.98, 0.02, false)], cursor_hidden: true, keys: Vec::new(), carets: Vec::new(), shapes: Vec::new() };
        let project = Project { zooms: vec![Zoom { start: 1.0, end: 3.0, scale: 2.0, focus: Focus::Auto, auto: false }], ..Default::default() };
        let motion = Motion::new(&project, Some(&track), 5.0);
        let [x, y, w, h] = motion.crop(2.0);
        assert!((w - 0.5).abs() < 1e-6 && (h - 0.5).abs() < 1e-6);
        assert!(x >= 0.0 && x + w <= 1.0 + 1e-9 && y >= 0.0 && y + h <= 1.0 + 1e-9);
        // Arriba a la derecha, donde está el cursor.
        assert!(x > 0.4 && y < 0.1);
        assert_eq!(motion.crop(4.5), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn el_zoom_entra_suave() {
        let zooms = vec![Zoom { start: 1.0, end: 4.0, scale: 2.0, focus: Focus::Auto, auto: false }];
        let (early, _, _) = zoom_at(&zooms, 1.1, &CameraOpts::default());
        let (mid, _, _) = zoom_at(&zooms, 1.4, &CameraOpts::default());
        let (full, _, _) = zoom_at(&zooms, 2.0, &CameraOpts::default());
        assert!(early > 1.0 && early < mid && mid < full && (full - 2.0).abs() < 1e-9);
    }

    /// El cursor cruza la pantalla durante un zoom: la cámara va con él, sin
    /// quedarse atrás ni dar saltos entre cuadros.
    #[test]
    fn la_camara_sigue_al_cursor_sin_atrasarse_ni_saltar() {
        let samples: Vec<Sample> = (0..=600).map(|i| {
            let t = i * 10;
            let x = 0.15 + 0.7 * (t as f32 / 6000.0);
            Sample(t, x, 0.5, false)
        }).collect();
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes: Vec::new() };
        let project = Project { zooms: vec![Zoom { start: 0.0, end: 6.0, scale: 2.0, focus: Focus::Auto, auto: false }], ..Default::default() };
        let motion = Motion::new(&project, Some(&track), 6.0);
        let mut last = None;
        for i in 60..300 {
            let t = i as f64 / 60.0;
            let [x, _, w, _] = motion.crop(t);
            let (cx, _) = motion.cursor(t).unwrap();
            // El cursor siempre a la vista, lejos de los bordes.
            assert!(cx > x + w * 0.1 && cx < x + w * 0.9, "en {t:.2} el cursor {cx:.3} quedó fuera de {x:.3}..{:.3}", x + w);
            if let Some(prev) = last {
                assert!((x - prev as f64).abs() < 0.01, "salto en {t:.2}");
            }
            last = Some(x as f32);
        }
    }

    /// `ATIC_TEST_CAMERA=<video>`: exporta el video con un zoom 2× que sigue al
    /// mouse todo el rato, para mirar la cámara de verdad (macOS).
    #[cfg(target_os = "macos")]
    #[test]
    fn exporta_la_camara_de_verdad() {
        let Ok(video) = std::env::var("ATIC_TEST_CAMERA") else { return };
        let video = std::path::PathBuf::from(video);
        let track = crate::screen_cursor::load(&video).expect("registro del mouse");
        let (duration, _) = atic_capture::stream::video_info(&video).unwrap();
        let project = Project { zooms: vec![Zoom { start: 0.0, end: duration, scale: 2.0, focus: Focus::Auto, auto: false }], ..Default::default() };
        let motion = Motion::new(&project, Some(&track), duration);
        let out = std::env::temp_dir().join("atic-camara.mp4");
        atic_capture::stream::export(&video, &out, atic_capture::stream::Export {
            segments: vec![(0.0, duration)],
            speeds: Vec::new(),
            texts: Vec::new(),
            cursor_px: 0.0,
            frame: None,
            options: Default::default(),
            look: Default::default(),
            decor: Box::new(move |src, _| atic_capture::stream::Decor { crop: motion.crop(src), blur: Some(motion.camera_motion(src)), ..Default::default() }),
            progress: Box::new(|_| {}),
            cancel: Default::default(),
        }).unwrap();
        eprintln!("{}", out.display());
    }

    #[test]
    fn los_textos_aparecen_y_se_van() {
        let project = Project { texts: vec![Text::new(1.0, 3.0, "Hola", TextAt::Top)], ..Default::default() };
        assert!(project.text_alpha(0.9).is_empty());
        assert_eq!(project.text_alpha(2.0), vec![(0, 1.0)]);
        assert!(project.text_alpha(1.1)[0].1 < 1.0);
    }

    // ---- Lo nuevo ----

    /// Un registro con el mouse quieto en el centro y clics en `clicks` (ms).
    fn still_track(clicks: &[u32], keys: &[u32], end: u32) -> Track {
        let mut samples = vec![Sample(0, 0.5, 0.5, false)];
        for c in clicks {
            samples.push(Sample(*c, 0.5, 0.5, true));
            samples.push(Sample(c + 100, 0.5, 0.5, false));
        }
        samples.push(Sample(end, 0.5, 0.5, false));
        Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: keys.to_vec(), carets: Vec::new(), shapes: Vec::new() }
    }

    fn auto(start: f64, end: f64) -> Zoom {
        Zoom { start, end, scale: 2.0, focus: Focus::Auto, auto: true }
    }

    fn manual(start: f64, end: f64) -> Zoom {
        Zoom { start, end, scale: 2.0, focus: Focus::At(0.3, 0.3), auto: false }
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn un_proyecto_viejo_sigue_leyendose() {
        // Como lo dejaba la versión anterior: sin cámara, velocidades, desenfoques,
        // exportación ni los campos nuevos de los textos y del cursor.
        let old = r#"{
            "trim": [1.0, 9.0],
            "cuts": [[3.0, 4.0]],
            "zooms": [{ "start": 1.0, "end": 3.0, "scale": 2.0, "focus": "Auto", "auto": true },
                      { "start": 5.0, "end": 8.0, "scale": 1.6, "focus": { "At": [0.2, 0.4] } }],
            "texts": [{ "start": 0.0, "end": 3.0, "text": "Hola", "at": "Top" }],
            "cursor": { "size": 1.5, "smooth": true, "ripple": true },
            "look": { "backdrop": { "Gradient": [16744031, 16692347] }, "padding": 0.08, "radius": 0.02, "shadow": true },
            "sharp": false
        }"#;
        let project: Project = serde_json::from_str(old).expect("el JSON viejo se lee");
        assert_eq!(project.trim, Some((1.0, 9.0)));
        assert_eq!(project.zooms.len(), 2);
        assert!(!project.zooms[1].auto);
        assert_eq!(project.camera, CameraStyle::Normal);
        assert!(project.speeds.is_empty() && project.blurs.is_empty() && project.backdrop_image.is_none());
        assert_eq!(project.export, ExportSettings::default());
        let text = &project.texts[0];
        assert!(near(text.size, 1.0) && text.color == 0xffffff && text.boxed && text.bold && !text.caption);
        assert!(project.cursor.hide_idle);
        assert_eq!(project.look.backdrop, Backdrop::Gradient(0xff7e5f, 0xfeb47b));
        // Y uno vacío también.
        assert_eq!(serde_json::from_str::<Project>("{}").unwrap(), Project::default());
    }

    #[test]
    fn los_valores_por_defecto_del_contrato() {
        let export = ExportSettings::default();
        // 1080 de tope: un máster más grande no se exporta a su propio alto (pedía el
        // triple de bits sin ganar nada). Nunca agranda una fuente menor.
        assert_eq!((export.format, export.aspect, export.height), (Format::Mp4, Aspect::Source, Some(1080)));
        assert_eq!((export.quality, export.codec, export.fps), (Quality::Standard, Codec::H264, 60));
        let text = Text::new(0.0, 1.0, "x", TextAt::Center);
        assert!(near(text.size, 1.0) && text.color == 0xffffff && text.boxed && text.bold && !text.caption);
        assert!(CursorOpts::default().hide_idle);
        assert_eq!(Project::default().camera, CameraStyle::Normal);
    }

    #[test]
    fn el_aspecto_da_su_proporcion() {
        assert_eq!(Aspect::ALL.len(), 5);
        assert_eq!(Aspect::Source.ratio((2940, 1912)), (2940, 1912));
        assert_eq!(Aspect::Source.ratio((0, 0)), (16, 9));
        assert_eq!(Aspect::Wide.ratio((100, 100)), (16, 9));
        assert_eq!(Aspect::Vertical.ratio((100, 100)), (9, 16));
        assert_eq!(Aspect::Square.ratio((100, 50)), (1, 1));
        assert_eq!(Aspect::Portrait.ratio((100, 50)), (4, 5));
    }

    #[test]
    fn cada_estilo_de_camara_trae_sus_numeros() {
        let all = CameraStyle::ALL.map(|s| (s.scale(), s.ramp(), s.follow(), s.smoothing()));
        assert_eq!(all[0], (1.6, 1.2, Follow::Free, 1.4));
        assert_eq!(all[1], (2.0, 0.8, Follow::Balanced, 1.0));
        assert_eq!(all[2], (2.4, 0.5, Follow::Centered, 0.7));
        assert!(near(CameraStyle::Normal.ramp(), ZOOM_RAMP));
        // El proyecto nuevo es el estilo Normal.
        assert_eq!(Project::default().active_style(), Some(CameraStyle::Normal));
        // Aplicar un estilo escribe los números, y un zoom nuevo y el autofocus nacen con su profundidad.
        let mut calm = Project::default();
        calm.apply_style(CameraStyle::Calm);
        assert_eq!(calm.camera, CameraStyle::Calm);
        assert_eq!(calm.camera_opts, CameraOpts { depth: 1.6, speed: ZoomSpeed::Slow, ease: Ease::Smooth });
        assert_eq!(calm.motion, MotionOpts { blur: 1.0, follow: Follow::Free, smoothing: 1.4 });
        let i = calm.add_zoom(2.0, 4.0, 20.0).unwrap();
        assert!(near(calm.zooms[i].scale, 1.6));
        let mut dynamic = Project::default();
        dynamic.apply_style(CameraStyle::Dynamic);
        dynamic.autofocus(&[5000], &[], 20.0);
        assert_eq!(dynamic.zooms.len(), 1);
        assert!(near(dynamic.zooms[0].scale, 2.4));
    }

    #[test]
    fn aplicar_un_estilo_conserva_la_curva_y_el_desenfoque() {
        let mut project = Project::default();
        project.camera_opts.ease = Ease::Snappy;
        project.motion.blur = 1.7;
        project.apply_style(CameraStyle::Dynamic);
        assert_eq!(project.camera_opts.ease, Ease::Snappy);
        assert!(near(project.motion.blur, 1.7));
        assert_eq!(project.active_style(), Some(CameraStyle::Dynamic));
    }

    #[test]
    fn el_estilo_activo_es_el_que_coincide_con_los_numeros() {
        let mut project = Project::default();
        project.apply_style(CameraStyle::Calm);
        assert_eq!(project.active_style(), Some(CameraStyle::Calm));
        // Mover un número a mano: personalizado.
        project.camera_opts.depth = 1.7;
        assert_eq!(project.active_style(), None);
        // Volver a dejar los números de otro preajuste lo activa, aunque el guardado sea otro.
        project.camera_opts.depth = 2.4;
        project.camera_opts.speed = ZoomSpeed::Fast;
        project.motion.follow = Follow::Centered;
        project.motion.smoothing = 0.7;
        assert_eq!(project.camera, CameraStyle::Calm);
        assert_eq!(project.active_style(), Some(CameraStyle::Dynamic));
        // La curva y el desenfoque no cuentan para el preajuste.
        project.camera_opts.ease = Ease::Linear;
        project.motion.blur = 0.0;
        assert_eq!(project.active_style(), Some(CameraStyle::Dynamic));
    }

    #[test]
    fn la_rampa_y_la_curva_salen_de_los_numeros() {
        let zooms = vec![Zoom { start: 1.0, end: 6.0, scale: 2.0, focus: Focus::Auto, auto: false }];
        let with = |speed, ease| zoom_at(&zooms, 1.3, &CameraOpts { speed, ease, ..Default::default() }).0;
        // 0,3 s después de empezar: la rampa corta ya va más acercada que la larga.
        let (slow, normal, fast) = (with(ZoomSpeed::Slow, Ease::Smooth), with(ZoomSpeed::Normal, Ease::Smooth), with(ZoomSpeed::Fast, Ease::Smooth));
        assert!(1.0 < slow && slow < normal && normal < fast && fast < 2.0);
        assert!(near(CameraOpts { speed: ZoomSpeed::Fast, ..Default::default() }.ramp_secs(), 0.5));
        assert!(near(CameraOpts { speed: ZoomSpeed::Slow, ..Default::default() }.ramp_secs(), 1.2));
        // A 0,3 s de una rampa de 0,8: Snappy va más adelante que Smooth, y Smooth más que Linear.
        let (smooth, snappy, linear) = (with(ZoomSpeed::Normal, Ease::Smooth), with(ZoomSpeed::Normal, Ease::Snappy), with(ZoomSpeed::Normal, Ease::Linear));
        assert!(near(linear, 1.0 + 0.375), "{linear}");
        assert!(snappy > linear && linear > smooth, "{smooth} {linear} {snappy}");
        // Todos llegan al zoom completo en el medio y salen a 1× fuera.
        for speed in ZoomSpeed::ALL {
            for ease in Ease::ALL {
                let camera = CameraOpts { speed, ease, ..Default::default() };
                assert!(near(zoom_at(&zooms, 3.5, &camera).0, 2.0));
                assert!(near(zoom_at(&zooms, 0.5, &camera).0, 1.0) && near(zoom_at(&zooms, 6.5, &camera).0, 1.0));
            }
        }
    }

    #[test]
    fn las_curvas_son_continuas_y_acotadas() {
        for ease in Ease::ALL {
            assert!(near(ease.apply(0.0), 0.0) && near(ease.apply(1.0), 1.0), "{ease:?}");
            assert!(near(ease.apply(-1.0), 0.0) && near(ease.apply(2.0), 1.0));
            // Crecen sin pasarse de 0..1 (sin rebote).
            let mut prev = 0.0;
            for i in 0..=100 {
                let y = ease.apply(i as f64 / 100.0);
                assert!((0.0..=1.0 + 1e-12).contains(&y) && y >= prev - 1e-12, "{ease:?} en {i}: {y}");
                prev = y;
            }
        }
        // Smooth y Snappy arrancan y llegan sin tirón (pendiente casi cero en los extremos).
        for ease in [Ease::Smooth, Ease::Snappy] {
            assert!(ease.apply(0.01) < 1e-3 && ease.apply(0.99) > 1.0 - 1e-3, "{ease:?}");
        }
        // Snappy sale rápido: ya va por la mitad antes del 30 %.
        assert!(Ease::Snappy.apply(0.3) > 0.5 && Ease::Smooth.apply(0.3) < 0.5);
    }

    #[test]
    fn tidy_junta_los_automaticos_cercanos_y_deja_los_lejanos() {
        // Separados por 1 s (< 2 rampas = 1,6 s): la cámara se queda acercada.
        let mut project = Project { zooms: vec![auto(5.0, 9.0), auto(1.0, 4.0)], ..Default::default() };
        project.tidy(20.0);
        assert_eq!(project.zooms, vec![auto(1.0, 9.0)]);
        // Separados por 2 s: salen y vuelven a entrar.
        let mut project = Project { zooms: vec![auto(1.0, 4.0), auto(6.0, 9.0)], ..Default::default() };
        project.tidy(20.0);
        assert_eq!(project.zooms, vec![auto(1.0, 4.0), auto(6.0, 9.0)]);
        // Con la velocidad lenta (rampa 1,2 s) esos 2 s ya son «cerca» (< 2,4 s).
        let mut project = Project { zooms: vec![auto(1.0, 4.0), auto(6.0, 9.0)], camera_opts: CameraOpts { speed: ZoomSpeed::Slow, ..Default::default() }, ..Default::default() };
        project.tidy(20.0);
        assert_eq!(project.zooms, vec![auto(1.0, 9.0)]);
    }

    #[test]
    fn tidy_es_idempotente_y_recorta_a_la_duracion() {
        let mut project = Project { zooms: vec![auto(18.0, 30.0), manual(2.0, 5.0), manual(25.0, 30.0)], ..Default::default() };
        project.tidy(20.0);
        // El manual que quedó fuera no existe; el automático llega hasta el final.
        assert_eq!(project.zooms, vec![manual(2.0, 5.0), auto(18.0, 20.0)]);
        let once = project.zooms.clone();
        project.tidy(20.0);
        assert_eq!(project.zooms, once);
    }

    #[test]
    fn tidy_quita_los_manuales_identicos() {
        let mut project = Project {
            zooms: vec![
                manual(2.0, 5.0),
                // Mismo tramo con 0,04 y 0,03 s de diferencia, misma escala y foco: sobra.
                manual(2.04, 5.03),
                // Misma escala y tramo pero otro foco: se queda.
                Zoom { focus: Focus::At(0.8, 0.8), ..manual(2.0, 5.0) },
                // Otro tramo (0,2 s de diferencia): se queda.
                manual(2.2, 5.0),
            ],
            ..Default::default()
        };
        project.tidy(20.0);
        assert_eq!(project.zooms.len(), 3);
        assert!(project.zooms.iter().all(|z| !z.auto));
        assert!(project.zooms.iter().any(|z| z.focus == Focus::At(0.8, 0.8)));
        assert!(project.zooms.iter().any(|z| near(z.start, 2.2)));
    }

    #[test]
    fn tidy_recorta_los_automaticos_que_pisan_un_manual() {
        // Los automáticos se juntan en [1, 15] y el manual [5, 9] los parte en dos.
        let mut project = Project { zooms: vec![auto(1.0, 6.0), auto(5.5, 7.0), auto(8.0, 15.0), manual(5.0, 9.0)], ..Default::default() };
        project.tidy(20.0);
        assert_eq!(project.zooms, vec![auto(1.0, 5.0), manual(5.0, 9.0), auto(9.0, 15.0)]);
        // Los pedazos que sobran y duran menos de 2 rampas (1,6 s) se descartan.
        let mut project = Project { zooms: vec![auto(1.0, 10.0), manual(2.0, 9.5)], ..Default::default() };
        project.tidy(20.0);
        assert_eq!(project.zooms, vec![manual(2.0, 9.5)]);
    }

    #[test]
    fn el_autofocus_ya_no_hace_respirar_la_camara() {
        // Clics a 4 s: los zooms (0,8–4,0 y 4,8–8,0) quedan a 0,8 s: uno solo.
        let mut project = Project::default();
        project.autofocus(&[2000, 6000], &[], 20.0);
        assert_eq!(project.zooms.len(), 1);
        assert!(near(project.zooms[0].start, 0.8) && near(project.zooms[0].end, 8.0));
        // A 5 s quedan a 1,8 s (>= 1,6): son dos.
        let mut project = Project::default();
        project.autofocus(&[2000, 7000], &[], 20.0);
        assert_eq!(project.zooms.len(), 2);
    }

    #[test]
    fn add_zoom_nunca_duplica_ni_apila() {
        let mut project = Project::default();
        assert_eq!(project.add_zoom(2.0, 3.0, 20.0), Some(0));
        assert!(near(project.zooms[0].start, 2.0) && near(project.zooms[0].end, 5.0));
        assert!(near(project.zooms[0].scale, 2.0) && project.zooms[0].focus == Focus::Auto && !project.zooms[0].auto);
        // Otra vez en el mismo lugar: empieza donde termina el anterior.
        assert_eq!(project.add_zoom(2.0, 3.0, 20.0), Some(1));
        assert!(near(project.zooms[1].start, 5.0) && near(project.zooms[1].end, 8.0));
        // Cae dentro del segundo: empuja hasta su final.
        assert_eq!(project.add_zoom(4.0, 3.0, 20.0), Some(2));
        assert!(near(project.zooms[2].start, 8.0) && near(project.zooms[2].end, 11.0));
        // Desde antes: se recorta al siguiente y queda primero (índice 0).
        assert_eq!(project.add_zoom(0.0, 5.0, 20.0), Some(0));
        assert!(near(project.zooms[0].start, 0.0) && near(project.zooms[0].end, 2.0));
        assert_eq!(project.zooms.len(), 4);
        // Ninguno pisa a otro.
        for pair in project.zooms.windows(2) {
            assert!(pair[0].end <= pair[1].start + 1e-9);
        }
        // Al final no cabe (0,2 s < 0,3 s).
        assert_eq!(project.add_zoom(19.8, 3.0, 20.0), None);
        assert_eq!(project.zooms.len(), 4);
        // Con hueco hasta el final, se recorta a la duración.
        let i = project.add_zoom(15.0, 10.0, 20.0).unwrap();
        assert!(near(project.zooms[i].end, 20.0));
    }

    #[test]
    fn un_zoom_automatico_largo_se_parte_donde_el_mouse_esta_quieto() {
        // Clics cada 4 s con el mouse quieto: un solo zoom de 0,8 a 20 s (19,2 s).
        let track = still_track(&[2000, 6000, 10000, 14000, 18000], &[], 25000);
        let mut project = Project::default();
        project.autofocus_track(&track, 25.0);
        // El hueco que más parejo lo deja es el de 10–14 s: se corta en 12 s, con
        // 1,61 s libres (2 rampas + 0,01) centrados.
        assert_eq!(project.zooms.len(), 2, "{:?}", project.zooms);
        assert!(project.zooms.iter().all(|z| z.auto));
        assert!(near(project.zooms[0].start, 0.8) && near(project.zooms[0].end, 11.195));
        assert!(near(project.zooms[1].start, 12.805) && near(project.zooms[1].end, 20.0));
        // `tidy` no vuelve a juntarlos.
        project.tidy(25.0);
        assert_eq!(project.zooms.len(), 2);
        // Rehacerlo da lo mismo.
        project.autofocus_track(&track, 25.0);
        assert_eq!(project.zooms.len(), 2);
    }

    #[test]
    fn un_zoom_automatico_no_se_parte_si_es_corto_o_hay_actividad() {
        // Corto (7,2 s): aunque haya 4 s quietos, no se parte.
        let track = still_track(&[2000, 6000], &[], 25000);
        let mut project = Project::default();
        project.autofocus_track(&track, 25.0);
        assert_eq!(project.zooms.len(), 1);
        // Largo, pero se escribe cada segundo: nunca hay 2,5 s quietos.
        let keys: Vec<u32> = (0..17).map(|i| 2000 + i * 1000).collect();
        let track = still_track(&[2000, 6000, 10000, 14000, 18000], &keys, 25000);
        let mut project = Project::default();
        project.autofocus_track(&track, 25.0);
        assert_eq!(project.zooms.len(), 1);
        assert!(project.zooms[0].end - project.zooms[0].start > LONG_ZOOM);
    }

    #[test]
    fn el_autofocus_con_registro_y_el_simple_dan_lo_mismo_si_no_hay_nada_que_partir() {
        let track = still_track(&[2000, 2800, 3500, 9000], &[], 20000);
        let mut with_track = Project::default();
        with_track.autofocus_track(&track, 20.0);
        let mut simple = Project::default();
        simple.autofocus(&track.clicks(), &track.keys, 20.0);
        assert_eq!(with_track.zooms, simple.zooms);
    }

    #[test]
    fn las_velocidades_cambian_el_tiempo_de_salida() {
        let mut project = Project { cuts: vec![(8.0, 10.0)], ..Default::default() };
        assert_eq!(project.add_speed(4.0, 12.0, 2.0), Some(0));
        // Recortado a lo que queda: (0–8) y (10–20).
        assert_eq!(project.speed_spans(20.0), vec![(4.0, 8.0, 2.0), (10.0, 12.0, 2.0)]);
        // length no sabe de velocidades: 8 + 10.
        assert!(near(project.length(20.0), 18.0));
        // 8 s con 4 al doble = 6 s; 10 s con 2 al doble = 9 s.
        assert!(near(project.out_length(20.0), 15.0));
        assert!(near(project.out_time(0.0, 20.0).unwrap(), 0.0));
        assert!(near(project.out_time(4.0, 20.0).unwrap(), 4.0));
        assert!(near(project.out_time(6.0, 20.0).unwrap(), 5.0));
        assert!(near(project.out_time(8.0, 20.0).unwrap(), 6.0));
        // Dentro del corte no hay momento de salida.
        assert_eq!(project.out_time(9.0, 20.0), None);
        assert!(near(project.out_time(11.0, 20.0).unwrap(), 6.5));
        assert!(near(project.out_time(20.0, 20.0).unwrap(), 15.0));
        // Fuera del recorte tampoco.
        project.trim = Some((1.0, 19.0));
        assert_eq!(project.out_time(0.5, 20.0), None);
        assert_eq!(project.out_time(19.5, 20.0), None);
        // Sin velocidades, out_length es length.
        let plain = Project { cuts: vec![(8.0, 10.0)], ..Default::default() };
        assert!(near(plain.out_length(20.0), plain.length(20.0)));
    }

    #[test]
    fn add_speed_acota_y_no_pisa() {
        let mut project = Project::default();
        // Se acota a 1,25..=16.
        assert_eq!(project.add_speed(15.0, 18.0, 100.0), Some(0));
        assert!(near(project.speeds[0].rate, 16.0));
        assert_eq!(project.add_speed(4.0, 12.0, 2.0), Some(0));
        assert_eq!(project.add_speed(0.0, 1.0, 1.0), Some(0));
        assert!(near(project.speeds[0].rate, 1.25));
        // Empieza dentro de [4, 12]: parte de 12 y no pasa de 15 (queda entre [4, 12] y [15, 18]).
        assert_eq!(project.add_speed(11.0, 20.0, 4.0), Some(2));
        assert!(near(project.speeds[2].start, 12.0) && near(project.speeds[2].end, 15.0));
        // Muy corto, al revés o con números rotos.
        assert_eq!(project.add_speed(3.0, 3.05, 2.0), None);
        assert_eq!(project.add_speed(f64::NAN, 3.0, 2.0), None);
        assert_eq!(project.speeds.len(), 4);
        // Ninguno pisa a otro.
        for pair in project.speeds.windows(2) {
            assert!(pair[0].end <= pair[1].start + 1e-9);
        }
        // Start y end al revés se ordenan.
        let mut other = Project::default();
        assert_eq!(other.add_speed(9.0, 5.0, 2.0), Some(0));
        assert!(near(other.speeds[0].start, 5.0) && near(other.speeds[0].end, 9.0));
    }

    fn quiet_audio() -> Vec<f32> {
        // 10 niveles por segundo, 20 s: sonido, silencio de 5 a 15 s, sonido.
        (0..200).map(|i| if (50..150).contains(&i) { 0.01 } else { 0.5 }).collect()
    }

    #[test]
    fn el_silencio_largo_se_sugiere_como_tramo_muerto() {
        let project = Project::default();
        let idle = project.suggest_idle(&quiet_audio(), 10, None, 20.0, 3.0);
        // 5–15 s sin sonido, menos 0,3 s de cada borde.
        assert_eq!(idle.len(), 1);
        assert!(near(idle[0].0, 5.3) && near(idle[0].1, 14.7), "{idle:?}");
        // Más corto que min_len: nada.
        assert!(project.suggest_idle(&quiet_audio(), 10, None, 20.0, 11.0).is_empty());
    }

    #[test]
    fn un_clic_o_una_tecla_parten_el_tramo_muerto() {
        let project = Project::default();
        let track = still_track(&[10000], &[], 20000);
        let idle = project.suggest_idle(&quiet_audio(), 10, Some(&track), 20.0, 3.0);
        // 5,0–10,0 y 10,1–15,0, cada uno con 0,3 s menos por borde.
        assert_eq!(idle.len(), 2, "{idle:?}");
        assert!(near(idle[0].0, 5.3) && near(idle[0].1, 9.7));
        assert!(near(idle[1].0, 10.4) && near(idle[1].1, 14.7));
        // Con min_len 4,95 se va el segundo (4,9 s).
        assert_eq!(project.suggest_idle(&quiet_audio(), 10, Some(&track), 20.0, 4.95).len(), 1);
        // Una tecla hace lo mismo.
        let typed = still_track(&[], &[10000], 20000);
        assert_eq!(project.suggest_idle(&quiet_audio(), 10, Some(&typed), 20.0, 3.0).len(), 2);
    }

    #[test]
    fn mover_el_mouse_cuenta_como_actividad() {
        // Quieto en (0,5, 0,5) hasta los 10 s, se mueve un 10 % y queda quieto.
        let samples = vec![Sample(0, 0.5, 0.5, false), Sample(9990, 0.5, 0.5, false), Sample(10000, 0.6, 0.5, false), Sample(20000, 0.6, 0.5, false)];
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes: Vec::new() };
        let project = Project::default();
        let idle = project.suggest_idle(&quiet_audio(), 10, Some(&track), 20.0, 3.0);
        assert_eq!(idle.len(), 2, "{idle:?}");
        // Un temblor de 0,1 % del cuadro no es actividad.
        let mut shaky = Track { samples: vec![Sample(0, 0.5, 0.5, false), Sample(10000, 0.501, 0.5, false), Sample(20000, 0.5, 0.5, false)], ..track };
        shaky.keys.clear();
        assert_eq!(project.suggest_idle(&quiet_audio(), 10, Some(&shaky), 20.0, 3.0).len(), 1);
    }

    #[test]
    fn sin_audio_se_juzga_solo_por_el_registro_y_sin_nada_no_se_sugiere() {
        let project = Project::default();
        // Sin audio ni registro no hay de dónde juzgar: no se propone cortar todo.
        assert!(project.suggest_idle(&[], 0, None, 20.0, 3.0).is_empty());
        assert!(project.suggest_idle(&[], 10, None, 20.0, 3.0).is_empty());
        // Sin audio, con un clic a los 10 s: 0–10 y 10,1–20.
        let track = still_track(&[10000], &[], 20000);
        let idle = project.suggest_idle(&[], 0, Some(&track), 20.0, 3.0);
        assert_eq!(idle.len(), 2, "{idle:?}");
        assert!(near(idle[0].0, 0.3) && near(idle[0].1, 9.7));
        assert!(near(idle[1].0, 10.4) && near(idle[1].1, 19.7));
        // Sin registro, solo audio.
        let idle = project.suggest_idle(&quiet_audio(), 10, None, 20.0, 3.0);
        assert_eq!(idle.len(), 1);
        // Un solo cuadro.
        assert!(project.suggest_idle(&[], 0, Some(&track), 0.0, 3.0).is_empty());
    }

    #[test]
    fn las_zonas_tapadas_se_funden() {
        let project = Project { blurs: vec![Blur { start: 2.0, end: 4.0, rect: [0.1, 0.1, 0.3, 0.2], pixelate: false }], ..Default::default() };
        assert!(project.blurs_at(1.9).is_empty() && project.blurs_at(4.1).is_empty());
        assert_eq!(project.blurs_at(3.0), vec![(0, 1.0)]);
        let entering = project.blurs_at(2.125);
        assert_eq!(entering.len(), 1);
        assert!(near(entering[0].1, 0.5));
        let leaving = project.blurs_at(3.9);
        assert!(near(leaving[0].1, 0.4));
    }

    #[test]
    fn el_cursor_se_esconde_quieto_y_vuelve_al_moverlo() {
        // Se mueve hasta 1 s, queda quieto y vuelve a moverse a los 5 s.
        let samples = vec![
            Sample(0, 0.1, 0.1, false),
            Sample(1000, 0.5, 0.5, false),
            Sample(5000, 0.5, 0.5, false),
            Sample(5100, 0.6, 0.6, false),
            Sample(7000, 0.6, 0.6, false),
        ];
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes: Vec::new() };
        let motion = Motion::new(&Project::default(), Some(&track), 7.0);
        assert!(near(motion.cursor_alpha(0.5), 1.0));
        // Lleva 1,5 s quieto: todavía se ve.
        assert!(near(motion.cursor_alpha(2.5), 1.0));
        // A los 2 s quieto empieza a bajar; 0,2 s después va por la mitad.
        let mid = motion.cursor_alpha(3.2);
        assert!(mid > 0.1 && mid < 0.9, "{mid}");
        // Desde 0,4 s después, escondido del todo.
        assert!(motion.cursor_alpha(3.5) < 0.01 && motion.cursor_alpha(5.0) < 0.01);
        // Al moverlo vuelve a 1 en 0,15 s.
        assert!(near(motion.cursor_alpha(5.3), 1.0));
        // Sin la opción, siempre se ve.
        let always = Project { cursor: CursorOpts { hide_idle: false, ..Default::default() }, ..Default::default() };
        assert!(near(Motion::new(&always, Some(&track), 7.0).cursor_alpha(4.0), 1.0));
        // Sin registro del mouse tampoco se esconde.
        assert!(near(Motion::new(&Project::default(), None, 7.0).cursor_alpha(4.0), 1.0));
    }

    #[test]
    fn la_forma_del_cursor_sale_del_registro() {
        let samples = vec![Sample(0, 0.5, 0.5, false), Sample(3000, 0.5, 0.5, false)];
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes: vec![(0, 0), (1000, 2), (2000, 1)] };
        let motion = Motion::new(&Project::default(), Some(&track), 3.0);
        assert_eq!(motion.cursor_shape(0.5), 0);
        assert_eq!(motion.cursor_shape(1.5), 2);
        assert_eq!(motion.cursor_shape(2.5), 1);
        // Sin registro: flecha.
        assert_eq!(Motion::new(&Project::default(), None, 3.0).cursor_shape(1.5), 0);
    }

    /// Una carpeta nueva en la temporal con un «video» (el archivo no existe).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-edit-project-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        dir.join("clip.mp4")
    }

    fn cleanup(video: &Path) {
        if let Some(dir) = video.parent() {
            std::fs::remove_dir_all(dir).ok();
        }
    }

    #[test]
    fn sin_archivo_el_proyecto_es_nuevo_y_sin_error() {
        let video = scratch("ausente");
        let (project, error) = Project::load_checked(&video);
        assert_eq!(project, Project::default());
        assert_eq!(error, None);
        cleanup(&video);
    }

    #[test]
    fn guardar_es_atomico_y_deja_la_copia_anterior() {
        let video = scratch("guardar");
        let path = project_path(&video).unwrap();
        let first = Project { motion: MotionOpts { blur: 0.0, ..Default::default() }, ..Default::default() };
        first.save(&video).expect("guarda");
        assert_eq!(Project::load_checked(&video), (first.clone(), None));
        // No hay `.bak` hasta que hay algo anterior.
        assert!(!suffixed(&path, "bak").exists());
        let second = Project { trim: Some((1.0, 2.0)), ..Default::default() };
        second.save(&video).expect("guarda otra vez");
        assert_eq!(Project::load(&video), second);
        let bak: Project = serde_json::from_str(&std::fs::read_to_string(suffixed(&path, "bak")).unwrap()).unwrap();
        assert_eq!(bak, first);
        // Ni rastro del temporal.
        assert!(!suffixed(&path, "tmp").exists());
        cleanup(&video);
    }

    #[test]
    fn un_json_danado_se_copia_y_vuelve_el_proyecto_nuevo() {
        let video = scratch("danado");
        let path = project_path(&video).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ \"trim\": [1.0, ").unwrap();
        let (project, error) = Project::load_checked(&video);
        assert_eq!(project, Project::default());
        let error = error.expect("avisa del daño");
        assert!(error.contains("dañado"), "{error}");
        // El malo queda donde estaba y con una copia al lado.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ \"trim\": [1.0, ");
        assert_eq!(std::fs::read_to_string(suffixed(&path, "corrupt")).unwrap(), "{ \"trim\": [1.0, ");
        // Una segunda vez no pisa la copia anterior.
        std::fs::write(&path, "otro roto").unwrap();
        assert!(Project::load_checked(&video).1.is_some());
        assert_eq!(std::fs::read_to_string(suffixed(&path, "corrupt")).unwrap(), "{ \"trim\": [1.0, ");
        assert_eq!(std::fs::read_to_string(suffixed(&path, "corrupt.1")).unwrap(), "otro roto");
        // `load` da el proyecto nuevo sin más.
        assert_eq!(Project::load(&video), Project::default());
        cleanup(&video);
    }

    #[test]
    fn guardar_dice_por_que_falla() {
        // Una ruta sin carpeta madre ni nombre no sirve.
        assert!(Project::default().save(Path::new("")).is_err());
        // Si `.atic` es un archivo, no se puede crear la carpeta.
        let video = scratch("falla");
        std::fs::write(video.parent().unwrap().join(".atic"), "no soy una carpeta").unwrap();
        let error = Project::default().save(&video).unwrap_err();
        assert!(error.contains("no se pudo"), "{error}");
        cleanup(&video);
    }

    fn sized(size: f64) -> Project {
        Project { cursor: CursorOpts { size, ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn deshacer_y_rehacer_van_por_los_pasos() {
        let p0 = Project::default();
        let p1 = Project { motion: MotionOpts { blur: 0.0, ..Default::default() }, ..Default::default() };
        let p2 = Project { motion: MotionOpts { blur: 0.0, ..Default::default() }, trim: Some((1.0, 2.0)), ..Default::default() };
        let mut history = History::new(&p0);
        assert!(!history.can_undo() && !history.can_redo());
        history.record(&p1, None);
        history.record(&p2, None);
        // Anotar lo mismo no suma un paso.
        history.record(&p2, None);
        assert!(history.can_undo() && !history.can_redo());
        assert_eq!(history.undo(&p2), Some(p1.clone()));
        assert_eq!(history.undo(&p1), Some(p0.clone()));
        assert_eq!(history.undo(&p0), None);
        assert!(!history.can_undo() && history.can_redo());
        assert_eq!(history.redo(&p0), Some(p1.clone()));
        assert_eq!(history.redo(&p1), Some(p2.clone()));
        assert_eq!(history.redo(&p2), None);
    }

    #[test]
    fn anotar_tras_deshacer_descarta_lo_que_se_podia_rehacer() {
        let (p0, p1) = (Project::default(), Project { motion: MotionOpts { blur: 0.0, ..Default::default() }, ..Default::default() });
        let (p2, p3) = (sized(2.0), sized(3.0));
        let mut history = History::new(&p0);
        history.record(&p1, None);
        history.record(&p2, None);
        assert_eq!(history.undo(&p2), Some(p1.clone()));
        assert!(history.can_redo());
        history.record(&p3, None);
        assert!(!history.can_redo());
        assert_eq!(history.undo(&p3), Some(p1));
    }

    #[test]
    fn deshacer_con_cambios_sin_anotar_vuelve_al_ultimo_anotado() {
        let (p0, p1, p2) = (Project::default(), sized(2.0), sized(3.0));
        let mut history = History::new(&p0);
        history.record(&p1, None);
        // `p2` no se anotó: deshacer vuelve a `p1` y `p2` queda para rehacer.
        assert_eq!(history.undo(&p2), Some(p1.clone()));
        assert_eq!(history.redo(&p1), Some(p2.clone()));
        // Rehacer con cambios sin anotar los pisaría: no hace nada.
        assert_eq!(history.undo(&p2), Some(p1.clone()));
        assert_eq!(history.redo(&sized(9.0)), None);
        // Sin nada anotado ni cambios, no hay a dónde volver.
        assert_eq!(History::new(&p0).undo(&p0), None);
    }

    #[test]
    fn un_arrastre_con_la_misma_clave_es_un_solo_paso() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut history = History::new(&Project::default());
        history.record_at(&sized(2.0), Some("size"), ms(0));
        history.record_at(&sized(2.5), Some("size"), ms(300));
        // 250 ms después del último (no del primero): sigue siendo el mismo paso.
        history.record_at(&sized(2.8), Some("size"), ms(550));
        // 950 ms después: paso nuevo.
        history.record_at(&sized(3.0), Some("size"), ms(1500));
        // Otra clave, aunque sea enseguida: paso nuevo.
        history.record_at(&sized(3.5), Some("otra"), ms(1550));
        // Sin clave tampoco agrupa.
        history.record_at(&sized(3.6), None, ms(1560));
        assert_eq!(history.undo(&sized(3.6)), Some(sized(3.5)));
        assert_eq!(history.undo(&sized(3.5)), Some(sized(3.0)));
        assert_eq!(history.undo(&sized(3.0)), Some(sized(2.8)));
        assert_eq!(history.undo(&sized(2.8)), Some(Project::default()));
        assert_eq!(history.undo(&Project::default()), None);
    }

    #[test]
    fn un_arrastre_que_vuelve_al_inicio_no_deja_paso() {
        let t0 = Instant::now();
        let mut history = History::new(&Project::default());
        history.record_at(&sized(2.0), Some("size"), t0);
        history.record_at(&Project::default(), Some("size"), t0 + Duration::from_millis(100));
        assert!(!history.can_undo());
        // Y la agrupación no se arrastra al paso de antes.
        history.record_at(&sized(4.0), Some("size"), t0 + Duration::from_millis(200));
        assert!(history.can_undo());
        assert_eq!(history.undo(&sized(4.0)), Some(Project::default()));
    }

    #[test]
    fn el_historial_guarda_hasta_cien_pasos() {
        let mut history = History::new(&Project::default());
        let mut current = Project::default();
        for i in 1..=150 {
            current = Project { trim: Some((0.0, i as f64)), ..Default::default() };
            history.record(&current, None);
        }
        let mut steps = 0;
        while let Some(previous) = history.undo(&current) {
            current = previous;
            steps += 1;
        }
        assert_eq!(steps, 100);
        // Y rehacer vuelve hasta el último.
        let mut redone = 0;
        while let Some(next) = history.redo(&current) {
            current = next;
            redone += 1;
        }
        assert_eq!(redone, 100);
        assert_eq!(current.trim, Some((0.0, 150.0)));
    }

    // ---- Opciones nuevas ----

    #[test]
    fn los_valores_por_defecto_de_las_opciones_nuevas() {
        let project = Project::default();
        assert_eq!(project.motion, MotionOpts { blur: 1.0, follow: Follow::Balanced, smoothing: 1.0 });
        assert_eq!(project.camera_opts, CameraOpts { depth: 2.0, speed: ZoomSpeed::Normal, ease: Ease::Smooth });
        let cursor = project.cursor;
        assert!(near(cursor.size, 1.5) && near(cursor.smooth_level, 0.5) && near(cursor.click_size, 1.0) && near(cursor.hide_delay, 2.0));
        assert_eq!((cursor.style, cursor.click, cursor.click_color), (CursorStyle::System, ClickKind::Ripple, 0xffffff));
        assert!(cursor.press_shrink && cursor.hide_idle);
        assert!(near(project.look.shadow_amount, 0.5) && project.look.has_shadow());
        // Lo de siempre: el nivel por defecto del cursor da el resorte de 18.
        assert!(near(smooth_omega(cursor.smooth_level).unwrap(), 18.0));
        // Y vacío y por defecto son lo mismo.
        assert_eq!(serde_json::from_str::<Project>("{}").unwrap(), project);
    }

    #[test]
    fn un_proyecto_viejo_se_migra_a_las_opciones_nuevas() {
        // Como lo dejaba la versión anterior, con todo lo que cambió de forma.
        let old = r#"{
            "cursor": { "size": 2.0, "smooth": false, "ripple": false, "hide_idle": false },
            "look": { "backdrop": { "Solid": 1 }, "padding": 0.1, "radius": 0.03, "shadow": false },
            "sharp": true,
            "camera": "Calm"
        }"#;
        let project: Project = serde_json::from_str(old).expect("el JSON viejo se lee");
        // El estilo pasa a sus números (y sigue siendo el preajuste activo).
        assert_eq!(project.camera, CameraStyle::Calm);
        assert_eq!(project.camera_opts, CameraOpts { depth: 1.6, speed: ZoomSpeed::Slow, ease: Ease::Smooth });
        assert_eq!((project.motion.follow, project.motion.smoothing), (Follow::Free, 1.4));
        assert_eq!(project.active_style(), Some(CameraStyle::Calm));
        // `sharp` es desenfoque 0.
        assert!(near(project.motion.blur, 0.0));
        // Sin suavizado y sin onda; lo demás conserva lo suyo y toma lo por defecto.
        assert!(near(project.cursor.smooth_level, 0.0) && project.cursor.click == ClickKind::None && smooth_omega(0.0).is_none());
        assert!(near(project.cursor.size, 2.0) && !project.cursor.hide_idle && project.cursor.press_shrink && near(project.cursor.hide_delay, 2.0));
        // Sin sombra; el resto del marco igual.
        assert!(near(project.look.shadow_amount, 0.0) && !project.look.has_shadow());
        assert!(near(project.look.padding, 0.1) && near(project.look.radius, 0.03) && project.look.backdrop == Backdrop::Solid(1));
        // Los que decían que sí a todo pasan a lo por defecto, no a «apagado».
        let yes: Project = serde_json::from_str(r#"{ "cursor": { "smooth": true, "ripple": true }, "look": { "shadow": true }, "sharp": false }"#).unwrap();
        assert!(near(yes.cursor.smooth_level, 0.5) && yes.cursor.click == ClickKind::Ripple && near(yes.look.shadow_amount, 0.5) && near(yes.motion.blur, 1.0));
        // Y si ya trae lo nuevo, manda lo nuevo sobre lo viejo.
        let both: Project = serde_json::from_str(
            r#"{ "camera": "Calm", "camera_opts": { "depth": 3.0 }, "motion": { "follow": "Centered" },
                 "cursor": { "smooth": false, "smooth_level": 0.8, "ripple": false, "click": "Pulse" }, "look": { "shadow": false, "shadow_amount": 0.9 } }"#,
        )
        .unwrap();
        assert_eq!(both.camera_opts, CameraOpts { depth: 3.0, ..Default::default() });
        assert_eq!(both.motion, MotionOpts { follow: Follow::Centered, ..Default::default() });
        assert!(near(both.cursor.smooth_level, 0.8) && both.cursor.click == ClickKind::Pulse && near(both.look.shadow_amount, 0.9));
        // Con un estilo conocido, `active_style` solo lo reconoce si los números coinciden.
        assert_eq!(both.active_style(), None);
    }

    #[test]
    fn lo_que_se_guarda_ya_no_trae_los_campos_viejos_y_se_lee_igual() {
        let mut project = Project::default();
        project.apply_style(CameraStyle::Dynamic);
        project.cursor = CursorOpts { size: 3.0, smooth_level: 0.0, style: CursorStyle::Ring, click: ClickKind::Pulse, click_size: 2.0, click_color: 0xff8800, press_shrink: false, hide_idle: true, hide_delay: 4.5 };
        project.look = Look { backdrop: Backdrop::Gradient(1, 2), padding: 0.2, radius: 0.05, shadow_amount: 0.75 };
        project.motion.blur = 1.5;
        project.camera_opts.ease = Ease::Snappy;
        let json = serde_json::to_string_pretty(&project).unwrap();
        for old in ["\"sharp\"", "\"ripple\"", "\"smooth\"", "\"shadow\""] {
            assert!(!json.contains(old), "se escribió {old}: {json}");
        }
        assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), project);
    }

    #[test]
    fn los_numeros_se_acotan_y_los_rotos_vuelven_a_lo_por_defecto() {
        let wild: Project = serde_json::from_str(
            r#"{ "camera_opts": { "depth": 99.0 }, "motion": { "blur": -3.0, "smoothing": 50.0 },
                 "cursor": { "size": 0.0, "smooth_level": 7.0, "click_size": 9.0, "hide_delay": 0.0, "click_color": 4294967295 },
                 "look": { "padding": 5.0, "radius": -1.0, "shadow_amount": 3.0 } }"#,
        )
        .unwrap();
        assert!(near(wild.camera_opts.depth, 4.0));
        assert!(near(wild.motion.blur, 0.0) && near(wild.motion.smoothing, 2.0));
        let cursor = wild.cursor;
        assert!(near(cursor.size, 0.5) && near(cursor.smooth_level, 1.0) && near(cursor.click_size, 2.5) && near(cursor.hide_delay, 1.0) && cursor.click_color == 0xffffff);
        assert!(near(wild.look.padding, 0.3) && near(wild.look.radius, 0.0) && near(wild.look.shadow_amount, 1.0));
        // El mínimo de cada uno.
        assert!(near(CameraOpts { depth: 0.1, ..Default::default() }.clamped().depth, 1.2));
        assert!(near(MotionOpts { smoothing: 0.0, ..Default::default() }.clamped().smoothing, 0.5));
        // Un número roto vuelve al por defecto (y no se cuela como NaN en la cámara).
        assert!(near(CameraOpts { depth: f64::NAN, ..Default::default() }.clamped().depth, 2.0));
        assert!(near(MotionOpts { blur: f64::INFINITY, smoothing: f64::NAN, ..Default::default() }.clamped().smoothing, 1.0));
        let broken = CursorOpts { size: f64::NAN, smooth_level: f64::NAN, hide_delay: f64::NAN, ..Default::default() }.clamped();
        assert!(near(broken.size, 1.5) && near(broken.smooth_level, 0.5) && near(broken.hide_delay, 2.0));
        assert!(near(Look { padding: f64::NAN, shadow_amount: f64::NAN, ..Default::default() }.clamped().padding, 0.08));
        // `clamp_options` lo hace sobre todo el proyecto.
        let mut project = Project { motion: MotionOpts { blur: 9.0, ..Default::default() }, ..Default::default() };
        project.clamp_options();
        assert!(near(project.motion.blur, 2.0));
        // Un zoom nuevo no nace con una profundidad fuera de rango.
        let mut deep = Project::default();
        deep.camera_opts.depth = 10.0;
        let i = deep.add_zoom(1.0, 3.0, 20.0).unwrap();
        assert!(near(deep.zooms[i].scale, 4.0));
        deep.autofocus(&[8000], &[], 20.0);
        assert!(deep.zooms.iter().filter(|z| z.auto).all(|z| near(z.scale, 4.0)));
    }

    #[test]
    fn el_suavizado_del_cursor_va_de_crudo_a_muy_suave() {
        assert_eq!(smooth_omega(0.0), None);
        assert!(near(smooth_omega(0.5).unwrap(), 18.0));
        assert!(near(smooth_omega(1.0).unwrap(), 6.0));
        // Más nivel, más suave (resorte más flojo), de forma continua.
        let mut prev = f64::INFINITY;
        for i in 1..=20 {
            let omega = smooth_omega(i as f64 / 20.0).unwrap();
            assert!(omega < prev && omega >= 6.0 - 1e-9 && omega <= 54.0, "{i}: {omega}");
            prev = omega;
        }
        // Fuera de rango o roto: se acota.
        assert!(near(smooth_omega(9.0).unwrap(), 6.0));
        assert_eq!(smooth_omega(-1.0), None);
        assert!(near(smooth_omega(f64::NAN).unwrap(), 18.0));
    }

    #[test]
    fn el_cursor_sin_suavizar_va_crudo_y_suavizado_se_atrasa() {
        // El mouse salta de un lado a otro en un cuadro.
        let samples = vec![Sample(0, 0.2, 0.5, false), Sample(1000, 0.2, 0.5, false), Sample(1017, 0.8, 0.5, false), Sample(3000, 0.8, 0.5, false)];
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes: Vec::new() };
        let with = |level: f64| Motion::new(&Project { cursor: CursorOpts { smooth_level: level, ..Default::default() }, ..Default::default() }, Some(&track), 3.0);
        let raw = with(0.0);
        assert!(near(raw.cursor(1.5).unwrap().0, 0.8) && near(raw.cursor(0.5).unwrap().0, 0.2));
        // Un cuadro después del salto, el crudo ya llegó y el suavizado va a medio camino.
        let (rough, soft) = (with(0.5).cursor(1.034).unwrap().0, with(1.0).cursor(1.034).unwrap().0);
        assert!(raw.cursor(1.034).unwrap().0 > 0.79);
        assert!(rough > 0.2 && rough < 0.79, "{rough}");
        assert!(soft < rough, "{soft} {rough}");
        // Y todos llegan.
        assert!(near_to(with(1.0).cursor(2.9).unwrap().0, 0.8, 1e-3));
    }

    fn near_to(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn las_opciones_del_cursor_llegan_al_movimiento() {
        // Un clic a 1 s con el mouse quieto.
        let track = still_track(&[1000], &[], 9000);
        let with = |cursor: CursorOpts| Motion::new(&Project { cursor, ..Default::default() }, Some(&track), 9.0);
        // Con el efecto por defecto hay una onda justo después del clic; sin efecto, ninguna.
        assert_eq!(with(CursorOpts::default()).ripples(1.1).len(), 1);
        for click in [ClickKind::Ripple, ClickKind::Ring, ClickKind::Pulse] {
            assert_eq!(with(CursorOpts { click, ..Default::default() }).ripples(1.1).len(), 1, "{click:?}");
        }
        assert!(with(CursorOpts { click: ClickKind::None, ..Default::default() }).ripples(1.1).is_empty());
        // El cursor se encoge al apretar, salvo que se pida que no.
        assert!(with(CursorOpts::default()).cursor_scale(1.05) < 0.95);
        assert!(near(with(CursorOpts { press_shrink: false, ..Default::default() }).cursor_scale(1.05), 1.0));
        assert!(near(with(CursorOpts::default()).cursor_scale(4.0), 1.0));
        // El tiempo de esconderlo quieto se elige (desde el clic, a 1 s): con 1 s ya se está yendo a los 2,3 s;
        // con 5 s se ve hasta los 6 s y recién ahí se va.
        let idle = |hide_delay: f64| with(CursorOpts { hide_delay, ..Default::default() });
        assert!(near(idle(5.0).cursor_alpha(3.0), 1.0) && near(idle(5.0).cursor_alpha(5.9), 1.0));
        assert!(idle(1.0).cursor_alpha(2.3) < 0.9 && idle(1.0).cursor_alpha(3.0) < 0.01);
        assert!(idle(5.0).cursor_alpha(6.2) < 0.9 && idle(5.0).cursor_alpha(7.0) < 0.01);
    }

    // ---- La cámara ----

    /// Un registro con el mouse yendo por `waypoints` (ms, x, y) en línea recta de uno al siguiente.
    fn path_track(waypoints: &[(u32, f32, f32)]) -> (Track, f64) {
        let end = waypoints.last().unwrap().0;
        let mut samples = Vec::new();
        for ms in (0..=end).step_by(10) {
            let i = waypoints.partition_point(|w| w.0 <= ms).saturating_sub(1).min(waypoints.len() - 2);
            let (a, b) = (waypoints[i], waypoints[i + 1]);
            let k = ((ms as f32 - a.0 as f32) / (b.0 - a.0) as f32).clamp(0.0, 1.0);
            samples.push(Sample(ms, a.1 + (b.1 - a.1) * k, a.2 + (b.2 - a.2) * k, false));
        }
        (Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes: Vec::new() }, end as f64 / 1000.0)
    }

    fn zoomed(follow: Follow, duration: f64) -> Project {
        Project {
            zooms: vec![Zoom { start: 0.0, end: duration, scale: 2.0, focus: Focus::Auto, auto: false }],
            motion: MotionOpts { follow, ..Default::default() },
            ..Default::default()
        }
    }

    /// La peor distancia de lo que sigue la cámara al centro de la vista (0 centro, 1 borde),
    /// entre `from` y `to`, sin contar los cuadros en que la cámara está pegada al borde del
    /// video y lo que sigue queda hacia ese borde (ahí no se puede más).
    fn worst_ratio(motion: &Motion, from: f64, to: f64) -> f64 {
        let mut worst: f64 = 0.0;
        let mut t = from;
        while t < to {
            let [x, y, w, h] = motion.crop(t);
            let (fx, fy, _) = motion.followed(t).unwrap();
            let axis = |at: f64, lo: f64, size: f64| {
                let off = at - (lo + size / 2.0);
                let pinned = (off < 0.0 && lo <= 0.005) || (off > 0.0 && lo + size >= 0.995);
                if pinned { 0.0 } else { off.abs() / (size / 2.0) }
            };
            worst = worst.max(axis(fx, x, w)).max(axis(fy, y, h));
            t += 1.0 / RATE;
        }
        worst
    }

    /// La segunda diferencia máxima por cuadro de x, y y tamaño de la vista entre `from` y `to`.
    fn worst_second_difference(motion: &Motion, from: f64, to: f64) -> [f64; 3] {
        let mut worst = [0.0f64; 3];
        let frame = |i: usize| {
            let [x, y, w, _] = motion.crop(i as f64 / RATE);
            [x, y, w]
        };
        for i in (from * RATE) as usize + 2..(to * RATE) as usize {
            let (a, b, c) = (frame(i - 2), frame(i - 1), frame(i));
            for k in 0..3 {
                worst[k] = worst[k].max((c[k] - 2.0 * b[k] + a[k]).abs());
            }
        }
        worst
    }

    /// Un mouse que da tirones de un lado a otro de la pantalla, con pausas.
    fn flicks() -> (Track, f64) {
        path_track(&[
            (0, 0.5, 0.5),
            (1000, 0.5, 0.5),
            (1250, 0.2, 0.25),
            (2200, 0.2, 0.25),
            (2450, 0.8, 0.7),
            (3400, 0.8, 0.7),
            (3550, 0.3, 0.4),
            (4500, 0.3, 0.4),
            (4650, 0.75, 0.3),
            (5600, 0.75, 0.3),
            (5900, 0.25, 0.8),
            (6900, 0.25, 0.8),
            (7100, 0.55, 0.45),
            (9000, 0.55, 0.45),
        ])
    }

    #[test]
    fn la_banda_de_guarda_se_cumple_en_los_tres_modos_y_la_camara_sigue_suave() {
        let (track, duration) = flicks();
        let mut previous_median = 0.0;
        for follow in [Follow::Centered, Follow::Balanced, Follow::Free] {
            let motion = Motion::new(&zoomed(follow, duration), Some(&track), duration);
            // Con el zoom completo (después de la rampa y antes de la salida).
            let worst = worst_ratio(&motion, ZOOM_RAMP + 0.1, duration - ZOOM_RAMP - 0.1);
            assert!(worst <= follow.guard() + 0.002, "{follow:?}: el cursor llegó a {worst:.3} (guarda {:.2})", follow.guard());
            // Más suelto el modo, más lejos del centro se queda el cursor en promedio.
            let mut ratios: Vec<f64> = (60..500).map(|i| worst_ratio(&motion, i as f64 / RATE, i as f64 / RATE + 0.001)).collect();
            ratios.sort_by(|a, b| a.total_cmp(b));
            let median = ratios[ratios.len() / 2];
            assert!(median >= previous_median - 1e-9, "{follow:?}: la mediana {median:.3} bajó de {previous_median:.3}");
            previous_median = median;
            // Sin tirones: nunca peor que lo de antes de esta vuelta (0,0090 en x/y; 0,0020 en el tamaño).
            let d2 = worst_second_difference(&motion, 0.0, duration);
            assert!(d2[0] < 0.009 && d2[1] < 0.009 && d2[2] < 0.0021, "{follow:?}: segunda diferencia {d2:?}");
        }
    }

    #[test]
    fn la_guarda_se_cumple_con_otros_numeros_de_camara() {
        let (track, duration) = flicks();
        for (depth, smoothing) in [(1.2, 0.5), (3.0, 2.0), (4.0, 1.0)] {
            for follow in Follow::ALL {
                let mut project = zoomed(follow, duration);
                project.camera_opts.depth = depth;
                project.motion.smoothing = smoothing;
                project.zooms[0].scale = depth;
                let motion = Motion::new(&project, Some(&track), duration);
                let worst = worst_ratio(&motion, ZOOM_RAMP + 0.1, duration - ZOOM_RAMP - 0.1);
                assert!(worst <= follow.guard() + 0.002, "{follow:?} depth {depth} suavizado {smoothing}: {worst:.3}");
                // La vista siempre queda dentro del video.
                for i in 0..(duration * RATE) as usize {
                    let [x, y, w, h] = motion.crop(i as f64 / RATE);
                    assert!(x >= -1e-9 && y >= -1e-9 && x + w <= 1.0 + 1e-9 && y + h <= 1.0 + 1e-9);
                }
            }
        }
    }

    #[test]
    fn los_micromovimientos_no_mueven_la_camara_dentro_de_la_zona_muerta() {
        // El mouse tiembla ±1,5 % del cuadro alrededor del centro.
        let mut waypoints = vec![(0u32, 0.5f32, 0.5f32)];
        for i in 1..=60u32 {
            let j = if i % 2 == 0 { 0.015 } else { -0.015 };
            waypoints.push((i * 100, 0.5 + j, 0.5 - j));
        }
        let (track, duration) = path_track(&waypoints);
        for follow in [Follow::Balanced, Follow::Free] {
            let motion = Motion::new(&zoomed(follow, duration), Some(&track), duration);
            let center = |t: f64| {
                let [x, y, w, h] = motion.crop(t);
                (x + w / 2.0, y + h / 2.0)
            };
            let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
            for i in 90..(duration * RATE) as usize - 90 {
                let c = center(i as f64 / RATE);
                lo = (lo.0.min(c.0), lo.1.min(c.1));
                hi = (hi.0.max(c.0), hi.1.max(c.1));
            }
            assert!(hi.0 - lo.0 < 0.002 && hi.1 - lo.1 < 0.002, "{follow:?}: la cámara se movió {:?}", (hi.0 - lo.0, hi.1 - lo.1));
        }
        // El modo centrado, en cambio, sí acompaña el temblor un poco (zona muerta casi nula).
        let motion = Motion::new(&zoomed(Follow::Centered, duration), Some(&track), duration);
        assert!(worst_ratio(&motion, 1.5, duration - 1.5) < Follow::Centered.guard());
    }

    #[test]
    fn la_camara_se_adelanta_al_cursor() {
        // Quieto en 0,3 hasta los 2 s y de ahí a 0,7 en medio segundo.
        let (track, duration) = path_track(&[(0, 0.3, 0.5), (2000, 0.3, 0.5), (2500, 0.7, 0.5), (6000, 0.7, 0.5)]);
        let motion = Motion::new(&zoomed(Follow::Balanced, duration), Some(&track), duration);
        let center = |t: f64| {
            let [x, _, w, _] = motion.crop(t);
            x + w / 2.0
        };
        // Antes de que el cursor salga de la zona muerta (a ~2,08 s), la cámara ya empezó a ir hacia donde va.
        let before = center(2.04);
        assert!(before > center(1.5) + 0.002, "la cámara no se adelantó: {before:.4} contra {:.4}", center(1.5));
        // Sin quedarse atrás: cuando el cursor llega, la cámara va cerca, y al final queda a lo más a la zona muerta (media vista = 0,25).
        assert!(worst_ratio(&motion, 2.0, 3.5) <= Follow::Balanced.guard() + 0.002);
        let (cx, _) = motion.cursor(4.5).unwrap();
        assert!((center(4.5) - cx).abs() <= Follow::Balanced.dead_zone() * 0.25 + 0.003, "{:.4} contra {cx:.4}", center(4.5));
    }

    #[test]
    fn la_camara_no_se_mueve_si_el_foco_es_fijo() {
        let (track, duration) = flicks();
        let mut project = zoomed(Follow::Centered, duration);
        project.zooms[0].focus = Focus::At(0.5, 0.5);
        let motion = Motion::new(&project, Some(&track), duration);
        for t in [2.0, 4.0, 6.0, 8.0] {
            let [x, y, w, h] = motion.crop(t);
            assert!(near_to(x + w / 2.0, 0.5, 1e-3) && near_to(y + h / 2.0, 0.5, 1e-3), "en {t} s la vista se movió a {x:.3},{y:.3}");
        }
    }

    #[test]
    fn el_zoom_entra_y_sale_con_su_rampa_y_su_curva() {
        let (track, duration) = path_track(&[(0, 0.5, 0.5), (8000, 0.5, 0.5)]);
        for speed in ZoomSpeed::ALL {
            for ease in Ease::ALL {
                let mut project = zoomed(Follow::Balanced, duration);
                project.zooms[0] = Zoom { start: 2.0, end: 6.0, scale: 2.0, focus: Focus::Auto, auto: false };
                project.camera_opts = CameraOpts { depth: 2.0, speed, ease };
                let motion = Motion::new(&project, Some(&track), duration);
                let ramp = speed.secs();
                // Sin zoom antes, con el zoom completo a mitad y de nuevo sin zoom al final.
                assert!(near(motion.crop(1.9)[2], 1.0), "{speed:?} {ease:?}");
                assert!(near(motion.crop(4.0)[2], 0.5), "{speed:?} {ease:?}");
                assert!(near(motion.crop(6.1 + ramp)[2], 1.0), "{speed:?} {ease:?}");
                // A mitad de la rampa de entrada ya hay un zoom intermedio (la curva decide cuánto).
                let mid = motion.crop(2.0 + ramp / 2.0)[2];
                assert!(mid < 1.0 && mid > 0.5, "{speed:?} {ease:?}: {mid}");
            }
        }
    }

    #[test]
    fn escribiendo_la_guarda_cubre_el_cursor_de_texto() {
        // El texto avanza de izquierda a derecha mientras se escribe sin parar; el mouse queda lejos.
        let samples = vec![Sample(0, 0.1, 0.9, false), Sample(11000, 0.1, 0.9, false)];
        let keys: Vec<u32> = (0..70).map(|i| 1500 + i * 100).collect();
        let carets: Vec<(u32, f32, f32)> = (0..30).map(|i| (1500 + i * 250, 0.2 + 0.02 * i as f32, 0.45 + 0.1 * ((i % 3) as f32 - 1.0))).collect();
        let track = Track { version: 1, region: [0, 0, 100, 100], samples, cursor_hidden: false, keys, carets, shapes: Vec::new() };
        for follow in Follow::ALL {
            let motion = Motion::new(&zoomed(follow, 11.0), Some(&track), 11.0);
            // Entre el primer y el último golpe de tecla (con el reparto del paso del mouse al texto aparte).
            let worst = worst_ratio(&motion, 2.2, 7.8);
            assert!(worst <= TYPING_GUARD + 0.002, "{follow:?}: el texto llegó a {worst:.3} del centro");
            let (_, _, text) = motion.followed(5.0).unwrap();
            assert!(near(text, 1.0), "{follow:?}: lo que sigue la cámara no es el texto ({text})");
            // Con el mouse lejos y sin escribir, lo que se sigue es el mouse.
            assert!(motion.followed(0.5).unwrap().2 < 0.01 && motion.followed(10.6).unwrap().2 < 0.01);
            // Sin saltos al pasar del mouse al texto y de vuelta.
            let d2 = worst_second_difference(&motion, 0.0, 11.0);
            assert!(d2[0] < 0.009 && d2[1] < 0.009, "{follow:?}: segunda diferencia {d2:?}");
        }
    }

    #[test]
    fn la_correccion_de_la_guarda_deja_el_camino_dentro_y_sin_tocar_lo_que_ya_cumple() {
        let n = 400;
        let (lo, hi) = (vec![0.3; n], vec![0.7; n]);
        // Un camino que cumple no cambia.
        let ok: Vec<f64> = (0..n).map(|i| 0.4 + 0.1 * (i as f64 / 50.0).sin()).collect();
        assert!(guard_correction(&ok, &lo, &hi).iter().all(|c| *c == 0.0));
        // Uno que se sale por los dos lados, rápido, queda adentro y la corrección es suave.
        let wild: Vec<f64> = (0..n).map(|i| 0.5 + 0.5 * (i as f64 / 9.0).sin()).collect();
        let fix = guard_correction(&wild, &lo, &hi);
        for i in 0..n {
            let x = wild[i] + fix[i];
            assert!(x >= 0.3 - 1e-9 && x <= 0.7 + 1e-9, "en {i}: {x}");
        }
        assert!(fix.iter().any(|c| c.abs() > 0.05));
        // Un salto brusco (el cursor «teletransportado») sube antes de que llegue: la corrección ya empezó 10 cuadros antes.
        let mut step = vec![0.5; n];
        let (mut lo2, hi2) = (vec![0.0; n], vec![1.0; n]);
        for i in 200..n {
            lo2[i] = 0.8;
        }
        step.truncate(n);
        let fix = guard_correction(&step, &lo2, &hi2);
        assert!(fix[190] > 1e-4 && fix[199] > 0.03, "{:?}", (fix[190], fix[199]));
        assert!(fix[200] >= 0.3 - 1e-3, "{}", fix[200]);
        // Y sin violaciones no hay trabajo: las cotas infinitas no estorban.
        let free = guard_correction(&ok, &vec![f64::NEG_INFINITY; n], &vec![f64::INFINITY; n]);
        assert!(free.iter().all(|c| *c == 0.0));
    }

    #[test]
    fn el_solver_de_banda_resuelve_el_sistema() {
        let (m, a, mu) = (40, 3.5, 12.0);
        let rhs: Vec<f64> = (0..m).map(|i| ((i * 7) % 5) as f64 - 2.0).collect();
        let x = Banded::new(m, a, mu).solve(&rhs);
        // A·x con A = a·I + μ·D₂ᵀD₂ (el patrón 1 −4 6 −4 1).
        let at = |k: isize| if k >= 0 && (k as usize) < m { x[k as usize] } else { 0.0 };
        for i in 0..m {
            let k = i as isize;
            let ax = a * x[i] + mu * (6.0 * at(k) - 4.0 * (at(k - 1) + at(k + 1)) + at(k - 2) + at(k + 2));
            assert!((ax - rhs[i]).abs() < 1e-9, "fila {i}: {ax} contra {}", rhs[i]);
        }
    }

    #[test]
    fn los_modos_de_seguimiento_van_de_pegado_a_suelto() {
        let [c, b, f] = Follow::ALL;
        assert!(c.dead_zone() < b.dead_zone() && b.dead_zone() < f.dead_zone());
        assert!(c.guard() < b.guard() && b.guard() < f.guard());
        for follow in Follow::ALL {
            assert!(follow.dead_zone() < follow.guard() && follow.guard() < 1.0);
        }
        assert_eq!(Follow::default(), Follow::Balanced);
    }

    #[test]
    fn los_textos_de_las_opciones() {
        assert_eq!(CameraOpts::default().depth_text(), "2.0×");
        assert_eq!(CameraOpts { speed: ZoomSpeed::Slow, ..Default::default() }.ramp_text(), "1.2 s");
        assert_eq!(MotionOpts::default().blur_text(), "100 %");
        assert_eq!(MotionOpts { smoothing: 1.5, ..Default::default() }.smoothing_text(), "1.5×");
        assert_eq!(CursorOpts::default().size_text(), "1.5×");
        assert_eq!(CursorOpts::default().smooth_text(), "50 %");
        assert_eq!(CursorOpts { hide_delay: 3.0, ..Default::default() }.hide_delay_text(), "3.0 s");
        assert_eq!(CameraOpts::DEPTH_RANGE, (1.2, 4.0));
        assert_eq!(CursorOpts::SIZE_RANGE, (0.5, 4.0));
    }

    #[test]
    fn la_imagen_propia_sobrevive_y_los_proyectos_viejos_cargan() {
        // Un proyecto de antes no tiene `backdrop_own`: carga igual, con su imagen de fondo.
        let viejo: Project = serde_json::from_str(r#"{"backdrop_image":"/x/foto.png"}"#).unwrap();
        assert_eq!(viejo.backdrop_image.as_deref(), Some(Path::new("/x/foto.png")));
        assert!(viejo.backdrop_own.is_none());
        // Con un fondo del editor puesto, la imagen propia se guarda aparte.
        let proyecto = Project { backdrop_own: Some("/x/foto.png".into()), backdrop_image: Some("wallpaper:11".into()), ..Project::default() };
        let vuelta: Project = serde_json::from_str(&serde_json::to_string(&proyecto).unwrap()).unwrap();
        assert_eq!(vuelta, proyecto);
    }
}
