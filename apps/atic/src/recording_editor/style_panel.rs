//! La barra lateral del editor: toda la columna derecha. Arriba, lo elegido en la
//! línea de tiempo (zoom, texto, corte, velocidad o zona tapada); debajo, una fila de
//! pestañas (Fondo, Cursor, Zoom, Movimiento, Texto, Subtítulos y Proyecto) y el panel
//! de la pestaña activa, con todas sus opciones.
//!
//! - **Controles deslizantes** (`Slider`): una tabla con los límites, el paso, el valor
//!   por defecto y la clave de deshacer de cada número continuo. Arrastrar solo muta el
//!   proyecto (la vista previa lo lee en cada cuadro); al soltar se registra UN paso
//!   (`changed_keyed`). El arrastre sigue aunque el puntero salga de la pista: la raíz del
//!   editor llama a `style_drag` y `style_release`.
//! - Los campos de color en hex (el del texto elegido, el del clic y los dos del fondo)
//!   viven en `StyleUi`. Cada uno aplica lo que se escribe en cuanto es un `#RRGGBB`
//!   válido y se vuelve a llenar solo cuando el color cambia por otro lado (una muestra,
//!   deshacer, elegir otro texto): `shown` recuerda lo último que el campo mostró o
//!   aceptó, así nunca se pisa lo que se está escribiendo.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px, svg, AnyElement, App, Bounds, ClickEvent, Context, Div, ElementId, Entity, FocusHandle, Focusable, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, SharedString, Window,
};

use super::fluid;
use super::wallpapers;
use super::palette::{over, Ed, RING_W, T_BODY, T_CONTROL, T_SMALL};
use super::timeline::Lane;
use super::{backdrop_fill, hex, Item, Preview, RecordingEditor, Tone, BUTTON, SIDEBAR_W};
use crate::edit_project::{
    Backdrop, CameraOpts, CameraStyle, ClickKind, CursorOpts, CursorStyle, Ease, Focus, Follow, Look, MotionOpts, Project, TextAt, ZoomSpeed, BACKDROPS,
};
use crate::hover::{mix, HoverExt};
use crate::i18n::t;
use crate::text_input::{Changed, TextInput};

/// Acercamientos de un zoom.
const ZOOM_SCALES: [f64; 6] = [1.25, 1.5, 2.0, 2.5, 3.0, 4.0];
/// Velocidades de un tramo acelerado.
const SPEED_RATES: [f64; 6] = [1.5, 2.0, 3.0, 4.0, 8.0, 16.0];
/// Tamaños del texto (`Text.size`).
const TEXT_SIZES: [(f64, &str); 4] = [(0.8, "S"), (1.0, "M"), (1.4, "L"), (2.0, "XL")];
/// Colores a un clic del texto y del clic: blanco, negro, amarillo, rojo, verde y celeste.
const TEXT_COLORS: [u32; 6] = [0xffffff, 0x000000, 0xffd60a, 0xff453a, 0x30d158, 0x64d2ff];
/// Dos números son el mismo valor si se llevan menos que esto.
const SAME: f64 = 0.004;
/// Un color cuenta como una muestra si está a menos de esto (suma de cuadrados
/// por canal: unos 4 niveles de diferencia en cada uno).
const SWATCH_NEAR: u32 = 48;
/// Lo que espera «¿Seguro?» la segunda pulsación de restablecer.
const RESET_CONFIRM: Duration = Duration::from_secs(4);
/// Los colores propios del fondo antes de que se escriba alguno.
const DEFAULT_CUSTOM: (u32, u32) = (0x8e2de2, 0x4a00e0);
/// Las imágenes que se aceptan de fondo.
const IMAGE_EXTS: [&str; 9] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "heic"];
/// Opciones por fila cuando son cortas (números, S/M/L) y cuando son nombres.
const ROW_SHORT: usize = 4;
const ROW_NAMES: usize = 3;

/// Cuántas pestañas y cuántos controles deslizantes hay (el largo de sus arreglos).
const TAB_COUNT: usize = 8;
const SLIDER_COUNT: usize = 10;
/// Un paso grande (con Shift) son tantos pasos normales.
const BIG_STEPS: f64 = 5.0;
/// Lo que espera el teclado, sin otra flecha, antes de registrar el paso de deshacer.
const KEY_SETTLE: Duration = Duration::from_millis(350);
/// Medidas de un control deslizante: alto de la zona que responde (24: un objetivo cómodo), grosor de la
/// pista y manilla (en reposo y mientras se arrastra).
const SLIDER_H: f32 = 24.0;
/// El relleno de una acción apagada (alfa de `control`).
const DISABLED_FILL: f32 = 0.3;
/// La muesca que marca el valor por defecto en la pista: ancho, alto y opacidad.
const DEFAULT_TICK_W: f32 = 2.0;
const DEFAULT_TICK_H: f32 = 8.0;
const DEFAULT_TICK_ALPHA: f32 = 0.6;
const BAR_H: f32 = 4.0;
const THUMB: f32 = 14.0;
const THUMB_DRAG: f32 = 16.0;
/// El punto central de una opción elegida en una lista (px).
const DOT: f32 = 6.0;
/// Alto de una pestaña y alto de una muestra de estilo de cursor.
const TAB_H: f32 = 32.0;
/// La fila de pestañas: el aire alrededor y entre ellas (la píldora sale de aquí: `fluid::pill_rect`).
const TABS_PAD: f32 = 3.0;
const TABS_GAP: f32 = 2.0;
/// El relleno de la barra lateral y su borde (para estimar el ancho de la fila antes de medirla).
const SIDEBAR_PAD: f32 = 14.0;
const STYLE_TILE_H: f32 = 58.0;
/// Las losas del selector de imágenes (16:10) y el hueco entre ellas: cuatro por fila en el ancho del panel.
const TILE_W: f32 = 61.0;
const TILE_H: f32 = 40.0;
const TILE_GAP: f32 = 6.0;
/// La tarjeta de lo elegido: relleno y radio extra, banda de color de su pista y X para soltar la selección.
const CARD_PAD: f32 = 12.0;
const CARD_FILL: f32 = 0.55;
const CARD_BAND_W: f32 = 3.0;
const CARD_CLOSE: f32 = 20.0;
/// Entre la fila de pestañas y lo que se desplaza.
const TABS_BELOW: f32 = 12.0;
/// El filete de los bordes de la barra (con vidrio): su opacidad.
const SCROLL_RULE_ALPHA: f32 = 0.2;
/// El degradado de los bordes de la barra cuando hay más contenido arriba o abajo (alto en px).
const SCROLL_FADE_H: f32 = 22.0;
/// Se considera que queda contenido más allá del borde si falta más que esto (px).
const SCROLL_SLACK: f32 = 1.0;

/// Las claves de `changed_keyed` de esta barra que NO mueven la cámara ni las velocidades
/// (no entran en `Motion::new`): `changed_keyed` no las recalcula. Las demás (suavizado del
/// cursor, efecto de clic, seguimiento, velocidad y curva del zoom…) sí.
const KEEPS_CAMERA: [&str; 9] = [
    "look-padding",
    "look-radius",
    "look-shadow",
    "cursor-size",
    "click-size",
    "click-color",
    "cursor-style",
    "camera-depth",
    "motion-blur",
];

/// ¿Esta clave de `changed_keyed` deja la cámara como está? (la lista de arriba). La usa
/// `changed_keyed` en mod.rs junto con su propia lista.
pub(super) fn keeps_camera(key: &str) -> bool {
    KEEPS_CAMERA.contains(&key)
}

// ---- Lógica pura ------------------------------------------------------------------

/// `#RRGGBB` (con o sin `#`) a `0xRRGGBB`. Solo seis dígitos hex: con tres, a
/// medio escribir `#ff0000` ya valdría `#ffff00`.
fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

/// `0xRRGGBB` a `#RRGGBB` en mayúsculas.
fn format_hex(color: u32) -> String {
    format!("#{:06X}", color & 0xffffff)
}

/// El cuadrado de la distancia entre dos colores, canal por canal.
fn distance2(a: u32, b: u32) -> u32 {
    [16, 8, 0]
        .into_iter()
        .map(|shift| {
            let d = ((a >> shift) & 0xff) as i32 - ((b >> shift) & 0xff) as i32;
            (d * d) as u32
        })
        .sum()
}

/// La muestra de `colors` más cercana a `color`, si está a menos de `near`
/// (para marcarla aunque el hex difiera en un nivel); si no, ninguna.
fn nearest_swatch(colors: &[u32], color: u32, near: u32) -> Option<usize> {
    colors.iter().enumerate().map(|(i, &c)| (i, distance2(c, color))).filter(|&(_, d)| d <= near).min_by_key(|&(_, d)| d).map(|(i, _)| i)
}

/// Los dos colores propios que muestran los campos: los del fondo si es un
/// degradado o un color; si no, los últimos que hubo.
fn custom_pair(backdrop: Backdrop, last: (u32, u32)) -> (u32, u32) {
    match backdrop {
        Backdrop::Gradient(a, b) => (a, b),
        Backdrop::Solid(c) => (c, last.1),
        Backdrop::None | Backdrop::Image => last,
    }
}

/// El fondo con los colores `(a, b)`. Sin marco o con imagen, escribir un color
/// arma un degradado con los dos.
fn with_colors(backdrop: Backdrop, (a, b): (u32, u32)) -> Backdrop {
    match backdrop {
        Backdrop::Solid(_) => Backdrop::Solid(a),
        Backdrop::Gradient(..) | Backdrop::None | Backdrop::Image => Backdrop::Gradient(a, b),
    }
}

/// El fondo al pasar a color sólido (`solid`) o a degradado.
fn to_mode(backdrop: Backdrop, solid: bool, last: (u32, u32)) -> Backdrop {
    match (backdrop, solid) {
        (Backdrop::Gradient(a, _), true) => Backdrop::Solid(a),
        (Backdrop::Solid(c), false) => Backdrop::Gradient(c, last.1),
        (Backdrop::None | Backdrop::Image, true) => Backdrop::Solid(last.0),
        (Backdrop::None | Backdrop::Image, false) => Backdrop::Gradient(last.0, last.1),
        (same, _) => same,
    }
}

/// Quita los zooms que hizo el zoom automático; cuántos eran.
fn remove_auto_zooms(project: &mut Project) -> usize {
    let before = project.zooms.len();
    project.zooms.retain(|z| !z.auto);
    before - project.zooms.len()
}

/// `1.5`, `2`, `1,25`: hasta dos decimales y sin ceros de sobra.
pub(super) fn fmt_num(v: f64, comma: bool) -> String {
    let mut s = format!("{v:.2}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if comma {
        s = s.replace('.', ",");
    }
    s
}

/// El texto con coma decimal si el idioma la usa (los `*_text()` del modelo traen punto).
fn localized(text: String, comma: bool) -> String {
    if comma {
        text.replace('.', ",")
    } else {
        text
    }
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < SAME
}

fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Cuántos huecos faltan para completar la última fila de `per_row`.
fn spacers(len: usize, per_row: usize) -> usize {
    if per_row == 0 {
        return 0;
    }
    (per_row - len % per_row) % per_row
}

/// ¿Sigue vigente la pregunta «¿Seguro?»?
fn armed(until: Option<Instant>, now: Instant) -> bool {
    until.is_some_and(|until| now < until)
}

/// La pista de cada muestra de color del texto.
fn color_tip(n: usize) -> &'static str {
    match n {
        0 => t("pill.editor.side.colorWhite"),
        1 => t("pill.editor.side.colorBlack"),
        2 => t("pill.editor.side.colorYellow"),
        3 => t("pill.editor.side.colorRed"),
        4 => t("pill.editor.side.colorGreen"),
        _ => t("pill.editor.side.colorSky"),
    }
}

/// La pista de la línea de tiempo a la que pertenece lo elegido (de ahí sale el color de la banda de su tarjeta).
fn lane_of(item: Item) -> Lane {
    match item {
        Item::Zoom(_) => Lane::Zoom,
        Item::Text(_) => Lane::Text,
        Item::Cut(_) => Lane::Video,
        Item::Speed(_) => Lane::Speed,
        Item::Blur(_) => Lane::Blur,
    }
}

/// El color de la banda de la tarjeta: el mismo del bloque en la línea de tiempo.
fn lane_color(pal: &Ed, lane: Lane) -> Hsla {
    match lane {
        Lane::Zoom => pal.zoom,
        Lane::Text => pal.text_lane,
        Lane::Speed => pal.speed,
        Lane::Blur => pal.blur,
        Lane::Video | Lane::Audio => pal.cut,
    }
}

/// La ayuda de la pestaña Texto: con un texto elegido, sus opciones ya están en la tarjeta de arriba y no se repite.
fn text_tab_help(text_selected: bool) -> Option<&'static str> {
    (!text_selected).then(|| t("pill.editor.side.textHow"))
}

/// ¿Queda contenido más allá del borde de arriba y del de abajo? `offset_y` es el desplazamiento (0 arriba, negativo
/// al bajar) y `max_y` lo más que se puede desplazar.
fn scroll_hints(offset_y: f32, max_y: f32) -> (bool, bool) {
    (offset_y < -SCROLL_SLACK, max_y + offset_y > SCROLL_SLACK)
}

// ---- Números continuos: de la posición del mouse al valor --------------------------

/// `value` dentro de `range` y en la grilla de `step` (que parte del mínimo). Un valor
/// roto (NaN, infinito) vale el mínimo. Se limpia el ruido de la suma (2,0000000000000004).
fn snap(value: f64, (min, max): (f64, f64), step: f64) -> f64 {
    if !value.is_finite() {
        return min;
    }
    let v = value.clamp(min, max);
    if step <= 0.0 {
        return v;
    }
    let snapped = (min + ((v - min) / step).round() * step).clamp(min, max);
    (snapped * 1e6).round() / 1e6
}

/// En qué parte de `range` (0–1) está `value`.
fn fraction(value: f64, (min, max): (f64, f64)) -> f64 {
    if !value.is_finite() || max <= min {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// El valor que queda bajo la fracción `f` (0–1) de la pista: con el paso y los límites.
fn value_at(f: f64, range: (f64, f64), step: f64) -> f64 {
    let f = if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 };
    snap(range.0 + f * (range.1 - range.0), range, step)
}

/// La fracción (0–1) de la pista que cae en la posición `x`; la pista empieza en `left`
/// y mide `width`. Fuera de la pista se queda en el extremo.
fn fraction_at(x: f32, left: f32, width: f32) -> f64 {
    if !x.is_finite() || !width.is_finite() || width <= 0.0 {
        return 0.0;
    }
    (((x - left) / width) as f64).clamp(0.0, 1.0)
}

/// Un paso del teclado: `dir` es -1 o 1; con `big`, `BIG_STEPS` pasos.
fn nudge(value: f64, range: (f64, f64), step: f64, dir: f64, big: bool) -> f64 {
    let steps = if big { BIG_STEPS } else { 1.0 };
    snap(value + dir * step * steps, range, step)
}

/// Dos valores son el mismo si no los separa ni el ruido del redondeo.
fn same_value(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// El tramo (0 a 4) del texto del suavizado del cursor: «Sin suavizar» … «Muy suave».
fn smooth_band(level: f64) -> usize {
    if level < 0.02 {
        0
    } else if level < 0.3 {
        1
    } else if level < 0.65 {
        2
    } else if level < 0.95 {
        3
    } else {
        4
    }
}

/// El tramo (0 a 3) del texto del desenfoque: «Apagado», «Bajo», «Normal», «Alto».
fn blur_band(amount: f64) -> usize {
    if amount < 0.02 {
        0
    } else if amount < 0.75 {
        1
    } else if amount <= 1.25 {
        2
    } else {
        3
    }
}

fn smooth_label(level: f64) -> &'static str {
    match smooth_band(level) {
        0 => t("pill.editor.side.smoothNone"),
        1 => t("pill.editor.side.smoothLow"),
        2 => t("pill.editor.side.smoothMid"),
        3 => t("pill.editor.side.smoothHigh"),
        _ => t("pill.editor.side.smoothMax"),
    }
}

fn blur_label(amount: f64) -> &'static str {
    match blur_band(amount) {
        0 => t("pill.editor.side.blurOff"),
        1 => t("pill.editor.side.blurLow"),
        2 => t("pill.editor.side.blurMid"),
        _ => t("pill.editor.side.blurHigh"),
    }
}

/// Los límites, el paso y el valor por defecto de un control continuo.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spec {
    range: (f64, f64),
    step: f64,
    default: f64,
}

/// Los controles deslizantes del panel: cada uno es un número del proyecto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slider {
    Padding,
    Radius,
    Shadow,
    CursorSize,
    CursorSmooth,
    ClickSize,
    HideDelay,
    Depth,
    Smoothing,
    Blur,
}

impl Slider {
    #[cfg(test)]
    const ALL: [Slider; SLIDER_COUNT] = [
        Slider::Padding,
        Slider::Radius,
        Slider::Shadow,
        Slider::CursorSize,
        Slider::CursorSmooth,
        Slider::ClickSize,
        Slider::HideDelay,
        Slider::Depth,
        Slider::Smoothing,
        Slider::Blur,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn spec(self) -> Spec {
        let (look, cursor, camera, motion) = (Look::default(), CursorOpts::default(), CameraOpts::default(), MotionOpts::default());
        match self {
            Self::Padding => Spec { range: Look::PADDING_RANGE, step: 0.01, default: look.padding },
            Self::Radius => Spec { range: Look::RADIUS_RANGE, step: 0.005, default: look.radius },
            Self::Shadow => Spec { range: (0.0, 1.0), step: 0.05, default: look.shadow_amount },
            Self::CursorSize => Spec { range: CursorOpts::SIZE_RANGE, step: 0.1, default: cursor.size },
            Self::CursorSmooth => Spec { range: (0.0, 1.0), step: 0.05, default: cursor.smooth_level },
            Self::ClickSize => Spec { range: CursorOpts::CLICK_SIZE_RANGE, step: 0.1, default: cursor.click_size },
            Self::HideDelay => Spec { range: CursorOpts::HIDE_DELAY_RANGE, step: 0.5, default: cursor.hide_delay },
            Self::Depth => Spec { range: CameraOpts::DEPTH_RANGE, step: 0.1, default: camera.depth },
            Self::Smoothing => Spec { range: MotionOpts::SMOOTHING_RANGE, step: 0.1, default: motion.smoothing },
            Self::Blur => Spec { range: MotionOpts::BLUR_RANGE, step: 0.1, default: motion.blur },
        }
    }

    fn get(self, p: &Project) -> f64 {
        match self {
            Self::Padding => p.look.padding,
            Self::Radius => p.look.radius,
            Self::Shadow => p.look.shadow_amount,
            Self::CursorSize => p.cursor.size,
            Self::CursorSmooth => p.cursor.smooth_level,
            Self::ClickSize => p.cursor.click_size,
            Self::HideDelay => p.cursor.hide_delay,
            Self::Depth => p.camera_opts.depth,
            Self::Smoothing => p.motion.smoothing,
            Self::Blur => p.motion.blur,
        }
    }

    fn set(self, p: &mut Project, v: f64) {
        match self {
            Self::Padding => p.look.padding = v,
            Self::Radius => p.look.radius = v,
            Self::Shadow => p.look.shadow_amount = v,
            Self::CursorSize => p.cursor.size = v,
            Self::CursorSmooth => p.cursor.smooth_level = v,
            Self::ClickSize => p.cursor.click_size = v,
            Self::HideDelay => p.cursor.hide_delay = v,
            Self::Depth => p.camera_opts.depth = v,
            Self::Smoothing => p.motion.smoothing = v,
            Self::Blur => p.motion.blur = v,
        }
    }

    /// La clave de deshacer: una racha con la misma clave es un solo paso.
    fn key(self) -> &'static str {
        match self {
            Self::Padding => "look-padding",
            Self::Radius => "look-radius",
            Self::Shadow => "look-shadow",
            Self::CursorSize => "cursor-size",
            Self::CursorSmooth => "cursor-smooth",
            Self::ClickSize => "click-size",
            Self::HideDelay => "hide-delay",
            Self::Depth => "camera-depth",
            Self::Smoothing => "smoothing",
            Self::Blur => "motion-blur",
        }
    }

    /// Si cambiarlo rehace la cámara (`Motion::new` lo lee). El suavizado del cursor, cuándo
    /// se esconde y el de la cámara, sí; el resto lo lee la vista previa directo del proyecto.
    #[cfg(test)]
    fn recalcs_camera(self) -> bool {
        matches!(self, Self::CursorSmooth | Self::HideDelay | Self::Smoothing)
    }

    /// El valor a la derecha de la etiqueta.
    fn text(self, p: &Project, comma: bool) -> String {
        let plain = match self {
            Self::Padding => format!("{} %", fmt_num(p.look.padding * 100.0, comma)),
            Self::Radius => format!("{} %", fmt_num(p.look.radius * 100.0, comma)),
            Self::Shadow if p.look.has_shadow() => format!("{:.0} %", p.look.shadow_amount * 100.0),
            Self::Shadow => t("pill.editor.side.noShadow").to_owned(),
            Self::CursorSize => p.cursor.size_text(),
            Self::CursorSmooth => smooth_label(p.cursor.smooth_level).to_owned(),
            Self::ClickSize => format!("{:.1}×", p.cursor.click_size),
            Self::HideDelay => p.cursor.hide_delay_text(),
            Self::Depth => p.camera_opts.depth_text(),
            Self::Smoothing => p.motion.smoothing_text(),
            Self::Blur => blur_label(p.motion.blur).to_owned(),
        };
        localized(plain, comma)
    }
}

/// El preajuste de cámara que coincide con los números de ahora.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Preset {
    Style(CameraStyle),
    /// Los números se movieron a mano: ningún preajuste.
    Custom,
}

fn preset_of(project: &Project) -> Preset {
    project.active_style().map_or(Preset::Custom, Preset::Style)
}

/// Qué partes del panel Cursor se muestran. Si el cursor ya viene en el video no hay
/// qué dibujar: solo el efecto de clic y una nota.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CursorParts {
    /// «El cursor viene en el video…».
    note: bool,
    /// Forma, tamaño y suavizado.
    look: bool,
    /// Tamaño y color del efecto (con efecto «Ninguno» no hay qué ajustar).
    click_detail: bool,
    /// Encoger al apretar.
    shrink: bool,
    /// Esconder cuando está quieto…
    idle: bool,
    /// …y cuánto tarda (solo si se esconde).
    idle_delay: bool,
}

fn cursor_parts(drawn: bool, click: ClickKind, hide_idle: bool) -> CursorParts {
    CursorParts { note: !drawn, look: drawn, click_detail: click != ClickKind::None, shrink: drawn, idle: drawn, idle_delay: drawn && hide_idle }
}

/// Las pestañas de la barra.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Frame,
    Cursor,
    Zoom,
    Motion,
    Text,
    Captions,
    /// El chat con un agente de IA (`assistant.rs`).
    Assistant,
    Session,
}

impl Tab {
    const ALL: [Tab; TAB_COUNT] = [Tab::Frame, Tab::Cursor, Tab::Zoom, Tab::Motion, Tab::Text, Tab::Captions, Tab::Assistant, Tab::Session];

    fn index(self) -> usize {
        self as usize
    }

    /// La pestaña `dir` lugares después (o antes, con negativo), dando la vuelta.
    fn step(self, dir: isize) -> Tab {
        Self::ALL[(self.index() as isize + dir).rem_euclid(TAB_COUNT as isize) as usize]
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Frame => "icons/layers.svg",
            Self::Cursor => "icons/mouse-pointer-2.svg",
            Self::Zoom => "icons/magnify.svg",
            Self::Motion => "icons/activity.svg",
            Self::Text => "icons/type.svg",
            Self::Captions => "icons/captions.svg",
            Self::Assistant => "icons/sparkles.svg",
            Self::Session => "icons/settings-2.svg",
        }
    }

    /// El nombre: lo que dice el globito de la pestaña y el título del primer grupo de su panel.
    fn label(self) -> &'static str {
        match self {
            Self::Frame => t("pill.editor.background"),
            Self::Cursor => t("pill.editor.cursor"),
            Self::Zoom => t("pill.editor.zoom"),
            Self::Motion => t("pill.editor.side.tabMotion"),
            Self::Text => t("pill.editor.text"),
            Self::Captions => t("pill.editor.captions"),
            Self::Assistant => t("pill.editor.ai.tab"),
            Self::Session => t("pill.editor.side.project"),
        }
    }
}

// ---- El estado de la barra ----------------------------------------------------------

/// Lo que la barra lateral guarda aparte del proyecto: la pestaña activa, los campos de
/// color en hex, el arrastre de un control deslizante y la confirmación de restablecer.
pub(super) struct StyleUi {
    /// La pestaña del panel que se ve.
    tab: Tab,
    /// Cuándo cambió por última vez y hacia qué lado (1 derecha, −1 izquierda): el panel nuevo entra de ahí.
    tab_at: (Instant, f32),
    /// El ancho de la fila de pestañas (lo deja un `canvas`; 0 hasta medirla) y el scroll de la barra.
    tabs_w: Rc<Cell<f32>>,
    scroll: ScrollHandle,
    /// El foco de cada pestaña (con una enfocada, ←/→ cambian de pestaña).
    tab_focus: [FocusHandle; TAB_COUNT],
    /// El control deslizante que se arrastra con el mouse apretado y si el mouse ya se movió desde
    /// que se apretó (un clic sin mover desliza la manilla al valor; arrastrando, la sigue 1:1).
    drag: Option<Slider>,
    drag_moved: bool,
    /// El foco de cada control deslizante (con uno enfocado, ←/→ ajustan un paso).
    slider_focus: [FocusHandle; SLIDER_COUNT],
    /// Dónde quedó la pista de cada control (la deja un `canvas` en cada cuadro).
    slider_bounds: [Rc<Cell<Option<Bounds<Pixels>>>>; SLIDER_COUNT],
    /// El control que se movió con el teclado y aún no se registró; y la cuenta de pasos
    /// para que solo el último temporizador lo registre.
    pending: Option<Slider>,
    pending_stamp: u64,
    /// El color del texto elegido.
    text_hex: Entity<TextInput>,
    /// El color del efecto del clic.
    click_hex: Entity<TextInput>,
    /// Los dos colores propios del fondo.
    back_hex: [Entity<TextInput>; 2],
    /// Lo último que mostró (o aceptó) `text_hex`: para qué texto y qué color.
    text_shown: Cell<Option<(Item, u32)>>,
    click_shown: Cell<Option<u32>>,
    back_shown: [Cell<Option<u32>>; 2],
    /// Los colores propios de ahora (los del fondo, o los últimos si no es de colores).
    custom: Cell<(u32, u32)>,
    /// Cuándo se eligió lo que muestra la tarjeta de arriba (entra con un fundido) y lo más que se pudo desplazar
    /// la última vez (si cambia, se vuelve a dibujar para mostrar u ocultar los degradados de los bordes).
    card_at: Instant,
    last_max: Cell<f32>,
    /// Hasta cuándo vale el «¿Seguro?» de restablecer.
    reset_until: Option<Instant>,
    /// El archivo que se eligió de fondo no era una imagen.
    image_error: bool,
}

impl StyleUi {
    pub(super) fn new(pal: &Ed, cx: &mut Context<RecordingEditor>) -> Self {
        let field = |cx: &mut Context<RecordingEditor>| cx.new(|cx| TextInput::new("#RRGGBB", pal.text, pal.sub, pal.accent, cx));
        let text_hex = field(cx);
        let click_hex = field(cx);
        let back_hex = [field(cx), field(cx)];
        cx.subscribe(&text_hex, |view: &mut RecordingEditor, field, _: &Changed, cx| {
            let value = field.read(cx).text().to_owned();
            view.side_text_hex_typed(&value, cx);
        })
        .detach();
        cx.subscribe(&click_hex, |view: &mut RecordingEditor, field, _: &Changed, cx| {
            let value = field.read(cx).text().to_owned();
            view.side_click_hex_typed(&value, cx);
        })
        .detach();
        for (n, input) in back_hex.iter().enumerate() {
            cx.subscribe(input, move |view: &mut RecordingEditor, field, _: &Changed, cx| {
                let value = field.read(cx).text().to_owned();
                view.side_back_hex_typed(n, &value, cx);
            })
            .detach();
        }
        Self {
            tab: Tab::default(),
            // Hace rato: la primera vez no se reproduce una entrada que no corresponde.
            tab_at: (Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now), 0.0),
            tabs_w: Rc::new(Cell::new(0.0)),
            scroll: ScrollHandle::new(),
            tab_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            drag: None,
            drag_moved: false,
            slider_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            slider_bounds: std::array::from_fn(|_| Rc::new(Cell::new(None))),
            pending: None,
            pending_stamp: 0,
            text_hex,
            click_hex,
            back_hex,
            text_shown: Cell::new(None),
            click_shown: Cell::new(None),
            back_shown: [Cell::new(None), Cell::new(None)],
            custom: Cell::new(DEFAULT_CUSTOM),
            card_at: Instant::now(),
            last_max: Cell::new(0.0),
            reset_until: None,
            image_error: false,
        }
    }

    /// Pasa a la pestaña `tab`: anota cuándo y hacia qué lado para que el panel nuevo entre de ese
    /// lado, y lo lleva arriba (la altura de cada panel es otra: así no queda a medio scroll).
    fn select_tab(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        self.tab_at = (Instant::now(), fluid::slide_dir(self.tab.index(), tab.index()));
        self.tab = tab;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
    }

    /// Se eligió algo nuevo en la línea de tiempo: la tarjeta de arriba entra con un fundido y la barra
    /// vuelve arriba (lo elegido es lo primero que se ve).
    pub(super) fn reveal_selection(&mut self) {
        self.card_at = Instant::now();
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
    }

    /// ¿Se está escribiendo en uno de los campos? Con uno enfocado, las teclas
    /// (¡la A es un dígito hex!) son del campo y no atajos del editor.
    pub(super) fn editing(&self, window: &Window, cx: &App) -> bool {
        [&self.text_hex, &self.click_hex, &self.back_hex[0], &self.back_hex[1]].into_iter().any(|field| field.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Llena `field` con `color` si lo que mostró no es ya ese color.
    fn sync(shown: &Cell<Option<u32>>, field: &Entity<TextInput>, color: u32, cx: &mut Context<RecordingEditor>) {
        if shown.get() != Some(color) {
            field.update(cx, |field, cx| field.set_text(format_hex(color), cx));
            shown.set(Some(color));
        }
    }
}

// ---- Piezas sueltas -----------------------------------------------------------------

/// Una línea de ayuda.
fn note(pal: &Ed, text: impl Into<SharedString>) -> AnyElement {
    div().text_size(px(T_SMALL)).text_color(pal.sub).child(text.into()).into_any_element()
}

/// Opciones en filas de `per_row`, con su nombre arriba y el fondo de los grupos.
fn options(pal: &Ed, label: impl Into<SharedString>, per_row: usize, mut chips: Vec<AnyElement>) -> AnyElement {
    chips.extend((0..spacers(chips.len(), per_row)).map(|_| div().flex_1().into_any_element()));
    let mut rows = Vec::new();
    let mut rest = chips.into_iter().peekable();
    while rest.peek().is_some() {
        rows.push(div().flex().gap(px(4.)).children(rest.by_ref().take(per_row.max(1))).into_any_element());
    }
    div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(label.into()))
        .child(div().flex().flex_col().gap(px(2.)).p(px(3.)).rounded(px(pal.r_chip.min(14.0) + 3.0)).bg(pal.control.opacity(0.55)).children(rows))
        .into_any_element()
}

fn labeled<T: Copy>(items: &[(T, &str)]) -> Vec<(T, SharedString)> {
    items.iter().map(|&(v, label)| (v, SharedString::from(label.to_owned()))).collect()
}

fn numbers(values: &[f64], comma: bool, suffix: &str) -> Vec<(f64, SharedString)> {
    values.iter().map(|&v| (v, SharedString::from(format!("{}{suffix}", fmt_num(v, comma))))).collect()
}

/// Los grupos de un panel en columna; el último sin la raya de abajo.
fn panel(mut groups: Vec<Div>) -> Div {
    let last = groups.pop();
    div().flex().flex_col().children(groups).children(last.map(|group| group.border_b_0().mb_0().pb_0()))
}

/// Un campo de color en hex: la muestra del color y el campo; borde rojo si lo
/// escrito no es un color.
fn hex_field(pal: &Ed, color: u32, input: &Entity<TextInput>, cx: &App) -> AnyElement {
    let text = input.read(cx).text();
    let bad = !text.trim().is_empty() && parse_hex(text).is_none();
    div()
        .h(px(30.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(7.))
        .rounded(px(pal.r_ctl))
        .bg(pal.control.opacity(0.7))
        .border_1()
        .border_color(if bad { pal.cut } else { pal.border.opacity(0.6) })
        .text_size(px(T_CONTROL))
        .child(div().size(px(14.)).flex_none().rounded(px(4.)).bg(hex(color)).border_1().border_color(pal.border))
        .child(div().flex_1().min_w_0().flex().overflow_hidden().child(input.clone()))
        .into_any_element()
}

/// El nombre del archivo de una imagen.
/// El nombre del archivo sin su extensión y con «…» si es largo (el `truncate` de GPUI no lo dibuja aquí).
fn short_name(path: &Path) -> SharedString {
    const MAX: usize = 28;
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if stem.chars().count() <= MAX {
        return stem.into();
    }
    format!("{}…", stem.chars().take(MAX - 1).collect::<String>().trim_end()).into()
}

fn file_label(path: &Path) -> SharedString {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into()
}

fn cursor_style_name(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::System => t("pill.editor.side.cursorSystem"),
        CursorStyle::Dot => t("pill.editor.side.cursorDot"),
        CursorStyle::Ring => t("pill.editor.side.cursorRing"),
        CursorStyle::Dark => t("pill.editor.side.cursorDark"),
        CursorStyle::Arrow => t("pill.editor.side.cursorArrow"),
    }
}

fn click_kind_name(kind: ClickKind) -> &'static str {
    match kind {
        ClickKind::None => t("pill.editor.side.clickNone"),
        ClickKind::Ripple => t("pill.editor.side.clickRipple"),
        ClickKind::Ring => t("pill.editor.side.clickRing"),
        ClickKind::Pulse => t("pill.editor.side.clickPulse"),
    }
}

/// La flecha del sistema dibujada con los dos SVG de cursor (relleno y borde) en `fill` y `edge`.
fn arrow(fill: gpui::Hsla, edge: gpui::Hsla) -> Div {
    div()
        .relative()
        .w(px(10.7))
        .h(px(16.))
        .child(svg().path("icons/cursor-fill.svg").size_full().text_color(fill))
        .child(svg().path("icons/cursor-edge.svg").absolute().top_0().left_0().size_full().text_color(edge))
}

/// La vista previa pequeña de un estilo de cursor.
fn cursor_swatch(pal: &Ed, style: CursorStyle) -> AnyElement {
    let (white, black) = (gpui::white(), gpui::black());
    let shape = match style {
        CursorStyle::System | CursorStyle::Arrow => arrow(white, black).into_any_element(),
        CursorStyle::Dark => arrow(black, white).into_any_element(),
        CursorStyle::Dot => div().size(px(11.)).rounded_full().bg(white).border_1().border_color(black.opacity(0.4)).into_any_element(),
        CursorStyle::Ring => div()
            .size(px(17.))
            .rounded_full()
            .border_1()
            .border_color(black.opacity(0.35))
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(13.)).rounded_full().border_2().border_color(white))
            .into_any_element(),
    };
    div().size(px(32.)).flex_none().rounded(px(8.)).bg(pal.control).flex().items_center().justify_center().child(shape).into_any_element()
}

impl RecordingEditor {
    // --- Piezas con estado --------------------------------------------------------

    /// Un grupo de opciones: marcada la que cumple `on`; al elegir una, `apply` la
    /// pone en el proyecto y se registra el cambio: con `key`, como paso de esa clave
    /// (`changed_keyed`: no recalcula la cámara si la clave está en `keeps_camera`);
    /// sin ella, con `changed`. El nombre de cada opción sale de `items`.
    #[allow(clippy::too_many_arguments)]
    fn side_pick<T: Copy + 'static>(
        &self,
        pal: &Ed,
        id: &'static str,
        label: impl Into<SharedString>,
        per_row: usize,
        items: &[(T, SharedString)],
        on: impl Fn(T) -> bool,
        apply: impl Fn(&mut Self, T) + Copy + 'static,
        key: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chips = items
            .iter()
            .enumerate()
            .map(|(n, (value, name))| {
                let value = *value;
                self.chip(pal, format!("{id}-{n}").into(), name.clone(), on(value), move |view, _, cx| {
                    apply(view, value);
                    match key {
                        Some(key) => view.changed_keyed(key, cx),
                        None => view.changed(cx),
                    }
                }, cx)
            })
            .collect();
        options(pal, label, per_row, chips)
    }

    /// Opciones en filas, cada una con su nombre y una línea que la explica: la elegida lleva
    /// el punto relleno. Al elegir una se registra el paso `key`.
    #[allow(clippy::too_many_arguments)]
    fn side_rows<T: Copy + 'static>(
        &self,
        pal: &Ed,
        id: &'static str,
        label: &'static str,
        items: &[(T, &'static str, &'static str)],
        on: impl Fn(T) -> bool,
        apply: impl Fn(&mut Self, T) + Copy + 'static,
        key: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (accent, faint, text, muted, sel, hover) = (pal.accent, pal.faint, pal.text, pal.sub, pal.sel, pal.hover);
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(n, &(value, name, hint))| {
                let active = on(value);
                let row_id = SharedString::from(format!("{id}-{n}"));
                // El punto y los colores se funden al elegirla; sin movimiento, el punto aparece entero.
                let on_t = self.fluid.toggle(row_id.clone(), active, fluid::CHIP_FADE, fluid::CHIP_FADE);
                let dot_size = if self.fluid.reduced() { if active { DOT } else { 0.0 } } else { DOT * on_t };
                let dot = div()
                    .size(px(14.))
                    .flex_none()
                    .mt(px(1.))
                    .rounded_full()
                    .border_2()
                    .border_color(mix(faint, accent, on_t))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(dot_size > 0.1, |dot| dot.child(div().size(px(dot_size)).rounded_full().bg(accent)));
                let row = div()
                    .id(row_id.clone())
                    .flex()
                    .gap(px(9.))
                    .px(px(7.))
                    .py(px(5.))
                    .rounded(px(pal.r_ctl.min(12.0)))
                    .cursor_pointer()
                    .child(dot)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .text_size(px(T_CONTROL))
                                    .text_color(mix(muted, text, on_t))
                                    .when(active, |el| el.font_weight(gpui::FontWeight::MEDIUM))
                                    .child(name),
                            )
                            .child(div().text_size(px(T_SMALL)).text_color(muted).child(hint)),
                    );
                self.interactive(accent, row, move |view, _, cx| {
                    apply(view, value);
                    view.changed_keyed(key, cx);
                }, cx)
                .fx(SharedString::from(format!("{row_id}-fx")), move |el, h| el.bg(fluid::select_bg(h, on_t, hover, sel)))
                .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(label))
            .child(div().flex().flex_col().gap(px(2.)).p(px(3.)).rounded(px(pal.r_ctl.min(12.0) + 3.0)).bg(pal.control.opacity(0.55)).children(rows))
            .into_any_element()
    }

    /// Una acción de ícono y nombre que, apagada, no hace nada (deshacer sin pasos).
    #[allow(clippy::too_many_arguments)]
    fn side_ghost(
        &self,
        pal: &Ed,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        tip: &'static str,
        enabled: bool,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if enabled {
            return self.action(pal, id, icon, label, tip, Tone::Normal, act, cx);
        }
        // Apagada: la misma forma, sin relleno de acción y con el texto tenue (se sigue leyendo).
        div()
            .h(px(BUTTON))
            .w_full()
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(pal.r_ctl))
            .bg(pal.control.opacity(DISABLED_FILL))
            .text_size(px(T_CONTROL))
            .text_color(pal.faint)
            .child(svg().path(icon).flex_none().size(px(15.)).text_color(pal.faint))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .into_any_element()
    }

    // --- Controles deslizantes ----------------------------------------------------

    /// Un control deslizante: su nombre y su valor arriba; abajo la pista con el relleno del
    /// acento y la manilla. Arrastrar mueve el valor (sigue aunque el puntero salga de la
    /// pista), un clic salta ahí, el doble clic restablece, y con la pista enfocada ←/→
    /// ajustan un paso (Shift, uno grande).
    ///
    /// Movimiento: la manilla crece de `THUMB` a `THUMB_DRAG` con el mouse encima o al arrastrar;
    /// el relleno sigue a la manilla sin retraso mientras se arrastra, y un clic (o una flecha)
    /// la desliza al valor; el número se resalta mientras se arrastra. Con movimiento reducido,
    /// ni crece ni se desliza.
    fn side_slider(&self, pal: &Ed, slider: Slider, label: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let spec = slider.spec();
        let comma = !crate::i18n::english();
        let target = fraction(slider.get(&self.project), spec.range) as f32;
        let dragging = self.style_ui.drag == Some(slider);
        let idx = slider.index();
        let following = dragging && self.style_ui.drag_moved;
        let frac = self.fluid.glide(format!("slider-{idx}"), target, fluid::SLIDER_GLIDE, following);
        let grab = self.fluid.toggle(format!("slider-{idx}-grab"), dragging, fluid::THUMB_GROW, fluid::THUMB_GROW);
        let hot = self.fluid.toggle(format!("slider-{idx}-hot"), dragging, fluid::THUMB_GROW, fluid::THUMB_GROW);
        let reduced = self.fluid.reduced();
        let (accent, hover) = (pal.accent, pal.hover);
        let (raised, faint) = (pal.raised, pal.faint);
        let slot = self.style_ui.slider_bounds[idx].clone();
        let handle = self.style_ui.slider_focus[idx].clone();
        // El alto útil de la pista: sin el borde del anillo de foco (que solo se ve con el teclado).
        let inner_h = SLIDER_H - 2.0 * RING_W;
        let bar_top = (inner_h - BAR_H) / 2.0;
        let default_at = fraction(spec.default, spec.range) as f32;
        let track = div()
            .id(("editor-slider", idx))
            .relative()
            .w_full()
            .h(px(SLIDER_H))
            .rounded(px(6.))
            .cursor_pointer()
            .track_focus(&handle)
            .tooltip(super::keys::tip(t("pill.editor.side.sliderTipShort")));
        let track = self
            .ringed(accent, track)
            .on_mouse_down(MouseButton::Left, cx.listener(move |view, event: &MouseDownEvent, window, cx| view.slider_press(slider, event, window, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, cx| {
                view.style_release(cx);
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| view.slider_key(slider, event, window, cx)))
            .fx(SharedString::from(format!("editor-slider-{idx}-fx")), move |el, h| {
                // Crece con el mouse encima o al arrastrar (con movimiento reducido, no).
                let size = crate::anim::lerp(THUMB, THUMB_DRAG, if reduced { 0.0 } else { h.t.max(grab) });
                let thumb = div().absolute().right(px(-size / 2.0)).top(px((inner_h - size) / 2.0)).size(px(size)).rounded_full().bg(accent).border_2().border_color(raised);
                let fill = div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(gpui::relative(frac))
                    .child(div().absolute().left_0().right_0().top(px(bar_top)).h(px(BAR_H)).rounded_full().bg(accent))
                    .child(thumb);
                // La zona útil de la pista deja media manilla a cada lado, para que no se salga.
                let area = div()
                    .absolute()
                    .left(px(THUMB_DRAG / 2.0))
                    .right(px(THUMB_DRAG / 2.0))
                    .top_0()
                    .bottom_0()
                    .child(canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full())
                    .child(div().absolute().left_0().right_0().top(px(bar_top)).h(px(BAR_H)).rounded_full().bg(faint.opacity(0.35)))
                    // La muesca del valor por defecto (a lo que vuelve el doble clic), debajo del relleno y de la manilla.
                    .child(div().absolute().left(gpui::relative(default_at)).ml(px(-DEFAULT_TICK_W / 2.0)).top(px((inner_h - DEFAULT_TICK_H) / 2.0)).w(px(DEFAULT_TICK_W)).h(px(DEFAULT_TICK_H)).rounded(px(1.)).bg(faint.opacity(DEFAULT_TICK_ALPHA)))
                    .child(fill);
                el.bg(mix(hover.opacity(0.0), hover.opacity(0.5), h.t)).child(area)
            });
        let head = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(label))
            .child(div().text_size(px(T_BODY)).text_color(mix(pal.sub, pal.text, hot)).child(slider.text(&self.project, comma)));
        div().flex().flex_col().gap(px(2.)).child(head).child(track).into_any_element()
    }

    /// Se apretó el mouse en la pista: enfoca, salta ahí y empieza el arrastre; el doble clic
    /// restablece el valor por defecto.
    fn slider_press(&mut self, slider: Slider, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.style_ui.slider_focus[slider.index()]);
        if event.click_count >= 2 {
            self.style_ui.drag = None;
            if self.slider_set(slider, slider.spec().default, cx) {
                self.changed_keyed(slider.key(), cx);
            }
            return;
        }
        self.style_ui.drag = Some(slider);
        self.style_ui.drag_moved = false;
        self.slider_to(slider, f32::from(event.position.x), cx);
    }

    /// Pone en `slider` el valor que cae bajo la posición `x` (de la ventana).
    fn slider_to(&mut self, slider: Slider, x: f32, cx: &mut Context<Self>) {
        let Some(bounds) = self.style_ui.slider_bounds[slider.index()].get() else { return };
        let spec = slider.spec();
        let f = fraction_at(x, f32::from(bounds.origin.x), f32::from(bounds.size.width));
        self.slider_set(slider, value_at(f, spec.range, spec.step), cx);
    }

    /// Pone `value` (con paso y límites) en el proyecto y repinta; solo muta el proyecto, sin
    /// registrar nada. `true` si cambió.
    fn slider_set(&mut self, slider: Slider, value: f64, cx: &mut Context<Self>) -> bool {
        let spec = slider.spec();
        let value = snap(value, spec.range, spec.step);
        if same_value(slider.get(&self.project), value) {
            return false;
        }
        slider.set(&mut self.project, value);
        cx.notify();
        true
    }

    /// Una flecha con la pista enfocada: un paso (Shift, grande); Inicio y Fin, los extremos.
    /// Las flechas seguidas son un solo paso de deshacer: se registra al soltar la tecla un rato.
    fn slider_key(&mut self, slider: Slider, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let spec = slider.spec();
        let now = slider.get(&self.project);
        let next = match event.keystroke.key.as_str() {
            "left" | "down" => nudge(now, spec.range, spec.step, -1.0, modifiers.shift),
            "right" | "up" => nudge(now, spec.range, spec.step, 1.0, modifiers.shift),
            "home" => spec.range.0,
            "end" => spec.range.1,
            // Esc devuelve el teclado al editor; la tecla sigue su camino.
            "escape" => {
                window.focus(&self.focus);
                return;
            }
            _ => return,
        };
        cx.stop_propagation();
        if self.slider_set(slider, next, cx) {
            self.style_ui.pending = Some(slider);
            self.style_ui.pending_stamp += 1;
            let stamp = self.style_ui.pending_stamp;
            cx.spawn(async move |view, cx| {
                cx.background_executor().timer(KEY_SETTLE).await;
                let _ = view.update(cx, |view, cx| {
                    if view.style_ui.pending_stamp == stamp {
                        if let Some(slider) = view.style_ui.pending.take() {
                            view.changed_keyed(slider.key(), cx);
                        }
                    }
                });
            })
            .detach();
        }
    }

    /// Lo llama la raíz del editor en cada movimiento del mouse: mueve el control que se
    /// arrastra según la posición. `true` si lo manejó.
    pub(super) fn style_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        let Some(slider) = self.style_ui.drag else { return false };
        // El botón se soltó fuera de la ventana y nadie lo avisó: el arrastre termina aquí.
        if event.pressed_button != Some(MouseButton::Left) {
            return self.style_release(cx);
        }
        self.style_ui.drag_moved = true;
        self.slider_to(slider, f32::from(event.position.x), cx);
        true
    }

    /// Lo llama la raíz del editor al soltar el botón: termina el arrastre y registra UN paso
    /// de deshacer. `true` si había un arrastre.
    pub(super) fn style_release(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(slider) = self.style_ui.drag.take() else { return false };
        self.changed_keyed(slider.key(), cx);
        cx.notify();
        true
    }

    // --- Pestañas -----------------------------------------------------------------

    /// La fila de pestañas: un botón de ícono por panel (el nombre va en su globito y en el
    /// título del primer grupo del panel; con siete no cabrían los nombres en 296 px). Una sola
    /// píldora marca la activa y se desliza de una a otra (elemento compartido: su posición es el
    /// valor animado y no se duplica); con movimiento reducido, salta.
    fn side_tabs(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let active = self.style_ui.tab;
        let pos = self.fluid.glide("side-tab-pill", active.index() as f32, fluid::TAB_PILL, false);
        // El ancho sale de la fila medida; el primer cuadro, de lo que da la barra.
        let measured = self.style_ui.tabs_w.get();
        let width = if measured > 0.0 { measured } else { SIDEBAR_W - SIDEBAR_PAD * 2.0 - 2.0 };
        let (left, w) = fluid::pill_rect(pos, TAB_COUNT, width, TABS_PAD, TABS_GAP);
        let slot = self.style_ui.tabs_w.clone();
        let radius = pal.r_chip.min(TAB_H / 2.0);
        let tabs: Vec<AnyElement> = Tab::ALL.iter().map(|&tab| self.side_tab(pal, tab, fluid::tab_weight(pos, tab.index()), cx)).collect();
        div()
            .relative()
            // Concéntrico con la píldora: su radio más el margen.
            .rounded(px(radius + TABS_PAD))
            .bg(pal.control.opacity(0.55))
            .child(canvas(move |bounds, _, _| slot.set(f32::from(bounds.size.width)), |_, _, _, _| {}).absolute().size_full())
            .child(div().absolute().left(px(left)).top(px(TABS_PAD)).w(px(w)).h(px(TAB_H)).rounded(px(radius)).bg(pal.sel))
            .child(div().flex().gap(px(TABS_GAP)).p(px(TABS_PAD)).children(tabs))
            .into_any_element()
    }

    /// Una pestaña. `weight` (0–1) dice cuánto la cubre la píldora: el ícono se enciende mientras
    /// llega y el hover se apaga (la píldora ya la marca).
    fn side_tab(&self, pal: &Ed, tab: Tab, weight: f32, cx: &mut Context<Self>) -> AnyElement {
        let (text, muted, hover, accent) = (pal.text, pal.sub, pal.hover, pal.accent);
        let id = SharedString::from(format!("editor-tab-{}", tab.index()));
        let icon = tab.icon();
        let el = div()
            .id(id.clone())
            .h(px(TAB_H))
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(pal.r_chip.min(TAB_H / 2.0)))
            .cursor_pointer()
            .track_focus(&self.style_ui.tab_focus[tab.index()])
            // El anillo solo con el teclado: la pestaña recién clicada no se ve como «seleccionada con aro».
            .tooltip(super::keys::tip(tab.label()));
        self.ringed(accent, el)
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.style_ui.select_tab(tab);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| view.tab_key(tab, event, window, cx)))
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                let fg = mix(h.mix(muted, text), text, weight);
                el.bg(mix(hover.opacity(0.0), hover, h.t * (1.0 - weight)))
                    .child(svg().path(icon).flex_none().size(px(16. - 1.5 * h.press)).text_color(fg))
            })
            .into_any_element()
    }

    /// Con una pestaña enfocada: ←/→ pasan a la de al lado, Inicio y Fin a las puntas.
    fn tab_key(&mut self, tab: Tab, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.shift {
            return;
        }
        let next = match event.keystroke.key.as_str() {
            "left" => tab.step(-1),
            "right" => tab.step(1),
            "home" => Tab::ALL[0],
            "end" => Tab::ALL[TAB_COUNT - 1],
            "escape" => {
                window.focus(&self.focus);
                return;
            }
            _ => return,
        };
        cx.stop_propagation();
        // Moverse entre pestañas con las flechas es navegar con el teclado: el anillo se muestra.
        self.kbd_nav = true;
        self.style_ui.select_tab(next);
        window.focus(&self.style_ui.tab_focus[next.index()]);
        cx.notify();
    }

    // --- Lo elegido ---------------------------------------------------------------

    /// La tarjeta de lo elegido en la línea de tiempo: va al comienzo del contenido que se desplaza (las pestañas
    /// quedan fijas arriba) y tiene identidad propia: fondo de tarjeta, una banda del color de su pista (el mismo del
    /// bloque), título y una X que suelta la selección. Entra con un fundido (sin animar el alto).
    fn selection_card(&self, pal: &Ed, item: Item, title: &'static str, body: Vec<AnyElement>, cx: &mut Context<Self>) -> Div {
        let appear = self.fluid.since(self.style_ui.card_at, fluid::CARD_FADE);
        let (sub, text, hover) = (pal.sub, pal.text, pal.hover);
        let close = div()
            .id("editor-selection-close")
            .size(px(CARD_CLOSE))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .tooltip(super::keys::tip(t("pill.editor.side.deselectTip")));
        let close = self
            .interactive(pal.accent, close, |view, window, cx| view.select(None, window, cx), cx)
            .fx("editor-selection-close-fx", move |el, h| {
                el.bg(h.mix(hover.opacity(0.0), hover)).child(svg().path("icons/x.svg").size(px(12.)).text_color(h.mix(sub, text)))
            });
        div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.))
            .mb(px(14.))
            .p(px(CARD_PAD))
            .pl(px(CARD_PAD + CARD_BAND_W))
            .rounded(px(pal.r_ctl + 2.0))
            .overflow_hidden()
            .bg(pal.control.opacity(CARD_FILL))
            .opacity(appear)
            // La banda de color es un hijo (no un borde): sigue las esquinas redondeadas gracias al recorte de la tarjeta.
            .child(div().absolute().left_0().top_0().bottom_0().w(px(CARD_BAND_W)).bg(lane_color(pal, lane_of(item))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(T_CONTROL)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(pal.text).child(title))
                    .child(close),
            )
            .children(body)
    }

    /// Lo elegido en la línea de tiempo: la tarjeta que va al comienzo del contenido de la barra.
    fn side_selection(&self, pal: &Ed, cx: &mut Context<Self>) -> Option<Div> {
        let item = self.selected?;
        let comma = !crate::i18n::english();
        let delete = self.action(pal, "editor-delete", "icons/trash.svg", t("pill.editor.delete"), t("pill.editor.deleteTip"), Tone::Danger, |view, window, cx| view.delete_selected(window, cx), cx);
        match item {
            Item::Zoom(i) => {
                let (scale, auto) = self.project.zooms.get(i).map(|z| (z.scale, matches!(z.focus, Focus::Auto)))?;
                let scales = self.side_pick(pal, "editor-scale", t("pill.editor.depth"), ROW_NAMES, &numbers(&ZOOM_SCALES, comma, "×"), |v| same(scale, v), move |view, v| {
                    if let Some(z) = view.project.zooms.get_mut(i) {
                        (z.scale, z.auto) = (v, false);
                    }
                }, None, cx);
                let follow = [(true, SharedString::from(t("pill.editor.followMouse"))), (false, SharedString::from(t("pill.editor.fixedPoint")))];
                let focus = self.side_pick(pal, "editor-focus", t("pill.editor.aim"), 2, &follow, |v| v == auto, move |view, follow| {
                    if let Some(z) = view.project.zooms.get_mut(i) {
                        if follow {
                            z.focus = Focus::Auto;
                        } else {
                            if matches!(z.focus, Focus::Auto) {
                                z.focus = Focus::At(0.5, 0.5);
                            }
                            z.auto = false;
                        }
                    }
                }, None, cx);
                Some(self.selection_card(pal, item, t("pill.editor.zoomSelected"), vec![scales, focus, note(pal, t("pill.editor.aimHint")), delete], cx))
            }
            Item::Text(i) => {
                let text = self.project.texts.get(i)?;
                let (at, size, color, boxed, bold, caption) = (text.at, text.size, text.color, text.boxed, text.bold, text.caption);
                let field = div()
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(pal.r_ctl))
                    .bg(pal.control.opacity(0.7))
                    .border_1()
                    .border_color(pal.border.opacity(0.6))
                    .text_size(px(T_CONTROL))
                    .child(self.text_field.clone())
                    .into_any_element();
                let mut body = vec![field];
                if caption {
                    body.push(note(pal, t("pill.editor.side.isCaption")));
                } else {
                    let places = [
                        (TextAt::Top, SharedString::from(t("pill.editor.top"))),
                        (TextAt::Center, SharedString::from(t("pill.editor.center"))),
                        (TextAt::Bottom, SharedString::from(t("pill.editor.bottom"))),
                    ];
                    body.push(self.side_pick(pal, "editor-at", t("pill.editor.position"), ROW_NAMES, &places, |v| v == at, move |view, v| {
                        if let Some(x) = view.project.texts.get_mut(i) {
                            x.at = v;
                        }
                    }, None, cx));
                }
                body.push(self.side_pick(pal, "editor-text-size", t("pill.editor.side.size"), ROW_SHORT, &labeled(&TEXT_SIZES), |v| same(size, v), move |view, v| {
                    if let Some(x) = view.project.texts.get_mut(i) {
                        x.size = v;
                    }
                }, None, cx));
                body.push(self.side_text_color(pal, i, color, cx));
                body.push(self.switch_row(pal, "editor-text-boxed", t("pill.editor.side.boxed"), boxed, move |view, _, cx| {
                    if let Some(x) = view.project.texts.get_mut(i) {
                        x.boxed = !x.boxed;
                    }
                    view.changed(cx);
                }, cx));
                body.push(self.switch_row(pal, "editor-text-bold", t("pill.editor.side.bold"), bold, move |view, _, cx| {
                    if let Some(x) = view.project.texts.get_mut(i) {
                        x.bold = !x.bold;
                    }
                    view.changed(cx);
                }, cx));
                body.push(delete);
                Some(self.selection_card(pal, item, t("pill.editor.textSelected"), body, cx))
            }
            Item::Cut(_) => Some(self.selection_card(
                pal,
                item,
                t("pill.editor.cutSelected"),
                vec![
                    div().text_size(px(T_BODY)).text_color(pal.sub).child(t("pill.editor.cutLabel")).into_any_element(),
                    self.action(pal, "editor-uncut", "icons/undo.svg", t("pill.editor.uncut"), t("pill.editor.uncutTip"), Tone::Normal, |view, window, cx| view.delete_selected(window, cx), cx),
                ],
                cx,
            )),
            Item::Speed(i) => {
                let rate = self.project.speeds.get(i)?.rate;
                let rates = self.side_pick(pal, "editor-rate", t("pill.editor.side.speed"), ROW_NAMES, &numbers(&SPEED_RATES, comma, "×"), |v| same(rate, v), move |view, v| {
                    if let Some(s) = view.project.speeds.get_mut(i) {
                        s.rate = v;
                    }
                }, None, cx);
                Some(self.selection_card(pal, item, t("pill.editor.side.speedSelected"), vec![rates, note(pal, t("pill.editor.side.noAudio")), delete], cx))
            }
            Item::Blur(i) => {
                let pixelate = self.project.blurs.get(i)?.pixelate;
                let kinds = [(false, SharedString::from(t("pill.editor.side.blurSoft"))), (true, SharedString::from(t("pill.editor.side.pixelate")))];
                let kind = self.side_pick(pal, "editor-blur-kind", t("pill.editor.side.effect"), 2, &kinds, |v| v == pixelate, move |view, v| {
                    if let Some(b) = view.project.blurs.get_mut(i) {
                        b.pixelate = v;
                    }
                }, None, cx);
                Some(self.selection_card(pal, item, t("pill.editor.side.blurSelected"), vec![kind, note(pal, t("pill.editor.side.blurHint")), delete], cx))
            }
        }
    }

    /// Las muestras de color (las de `TEXT_COLORS`) y, debajo, el campo en hex. `apply` pone el
    /// color elegido en el proyecto y registra el paso.
    #[allow(clippy::too_many_arguments)]
    fn side_colors(
        &self,
        pal: &Ed,
        id: &'static str,
        label: &'static str,
        color: u32,
        input: &Entity<TextInput>,
        apply: impl Fn(&mut Self, u32, &mut Context<Self>) + Copy + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let near = nearest_swatch(&TEXT_COLORS, color, SWATCH_NEAR);
        let swatches: Vec<_> = TEXT_COLORS
            .iter()
            .enumerate()
            .map(|(n, &c)| {
                div()
                    .id((id, n))
                    .size(px(26.))
                    .flex_none()
                    .rounded_full()
                    .p(px(2.))
                    .border_2()
                    .border_color(if near == Some(n) { pal.accent } else { pal.border.opacity(0.0) })
                    .cursor_pointer()
                    .tooltip(super::keys::tip(color_tip(n)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| apply(view, c, cx)))
                    .child(div().size_full().rounded_full().bg(hex(c)).border_1().border_color(pal.border))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(label))
            .child(div().flex().items_center().justify_between().children(swatches))
            .child(hex_field(pal, color, input, cx))
            .into_any_element()
    }

    /// El color del texto `i`: las muestras y el campo en hex.
    fn side_text_color(&self, pal: &Ed, i: usize, color: u32, cx: &mut Context<Self>) -> AnyElement {
        // El campo muestra el color de este texto; si es otro texto u otro color, se llena.
        if self.style_ui.text_shown.get() != Some((Item::Text(i), color)) {
            self.style_ui.text_hex.update(cx, |field, cx| field.set_text(format_hex(color), cx));
            self.style_ui.text_shown.set(Some((Item::Text(i), color)));
        }
        self.side_colors(pal, "editor-text-color", t("pill.editor.side.color"), color, &self.style_ui.text_hex, move |view, c, cx| {
            if let Some(x) = view.project.texts.get_mut(i) {
                x.color = c;
            }
            view.changed(cx);
        }, cx)
    }

    /// Se escribió en el campo del color del texto elegido.
    fn side_text_hex_typed(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(Item::Text(i)) = self.selected else { return };
        let Some(color) = parse_hex(value) else { return };
        let Some(text) = self.project.texts.get_mut(i) else { return };
        let was = std::mem::replace(&mut text.color, color);
        self.style_ui.text_shown.set(Some((Item::Text(i), color)));
        if was != color {
            self.changed_keyed("text-color", cx);
        }
    }

    /// Se escribió en el campo del color del clic.
    fn side_click_hex_typed(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(color) = parse_hex(value) else { return };
        let was = std::mem::replace(&mut self.project.cursor.click_color, color);
        self.style_ui.click_shown.set(Some(color));
        if was != color {
            self.changed_keyed("click-color", cx);
        }
    }

    // --- Fondo --------------------------------------------------------------------

    /// El panel Fondo: las muestras, la imagen y los colores propios; y el marco (margen,
    /// esquinas y sombra), que solo vale con un fondo puesto.
    fn side_frame(&self, pal: &Ed, cx: &mut Context<Self>) -> Div {
        let look = self.project.look;
        // Las muestras: los fondos que trae el editor.
        let swatches: Vec<_> = BACKDROPS
            .iter()
            .enumerate()
            .map(|(i, &backdrop)| {
                let on = look.backdrop == backdrop;
                let swatch = div()
                    .id(("editor-backdrop", i))
                    .size(px(30.))
                    .rounded(px(9.))
                    .p(px(2.))
                    .border_2()
                    .border_color(if on { pal.accent } else { pal.border.opacity(0.0) })
                    .cursor_pointer()
                    .tooltip(super::keys::tip(if backdrop == Backdrop::None { t("pill.editor.noFrame") } else { t("pill.editor.backdropTip") }))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.style_ui.image_error = false;
                        view.project.look.backdrop = backdrop;
                        view.changed_keyed("backdrop-color", cx);
                    }));
                // Concéntrica con el marco: 9 menos el margen y el borde (2 + 2).
                let inner = div().size_full().rounded(px(5.));
                let inner = match backdrop {
                    // Sin marco: el video llena el cuadro (un cuadro con una cruz).
                    Backdrop::None => inner.border_1().border_color(pal.border).flex().items_center().justify_center().child(svg().path("icons/x.svg").size(px(14.)).text_color(pal.muted)),
                    other => inner.bg(backdrop_fill(other, 1.0, 1.0)),
                };
                swatch.child(inner).into_any_element()
            })
            .collect();
        let back = vec![div().flex().flex_wrap().gap(px(3.)).children(swatches).into_any_element(), self.side_image(pal, cx), self.side_custom_colors(pal, cx)];
        let frame = if look.framed() {
            vec![
                self.side_slider(pal, Slider::Padding, t("pill.editor.padding"), cx),
                self.side_slider(pal, Slider::Radius, t("pill.editor.corners"), cx),
                self.side_slider(pal, Slider::Shadow, t("pill.editor.shadow"), cx),
            ]
        } else {
            vec![note(pal, t("pill.editor.side.frameOff"))]
        };
        panel(vec![Self::page(pal, t("pill.editor.background"), back), Self::section(pal, t("pill.editor.side.frame"), frame)])
    }

    /// Las imágenes de fondo: una grilla con «Subir imagen», la imagen propia (si hay) y los fondos que
    /// trae el editor. La elegida lleva el aro, igual que las muestras de color.
    fn side_image(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let active = if self.project.look.backdrop == Backdrop::Image { self.project.backdrop_image.clone() } else { None };
        let own = self.own_image();
        let (faint, control) = (pal.faint, pal.control);
        // Mientras carga o si falla, un cuadro con el ícono de imagen.
        let placeholder = move || div().size_full().flex().items_center().justify_center().bg(control).child(svg().path("icons/image.svg").size(px(12.)).text_color(faint)).into_any_element();
        let face = move |source: gpui::ImageSource| {
            gpui::img(source).w(px(TILE_W - 8.0)).h(px(TILE_H - 8.0)).flex_none().object_fit(gpui::ObjectFit::Cover).with_loading(placeholder.clone()).with_fallback(placeholder.clone()).into_any_element()
        };
        // La losa: el aro de elegida (con el mismo hueco que las muestras) y la cara recortada dentro.
        let tile = |id: ElementId, on: bool, tip: SharedString, face: AnyElement| {
            div()
                .id(id)
                .w(px(TILE_W))
                .h(px(TILE_H))
                .flex_none()
                .rounded(px(9.))
                .p(px(2.))
                .border_2()
                .border_color(if on { pal.accent } else { pal.border.opacity(0.0) })
                .cursor_pointer()
                .tooltip(crate::hover::tip_text(tip))
                .child(div().size_full().rounded(px(5.)).overflow_hidden().child(face))
        };
        let mut tiles = Vec::new();
        let add = div().size_full().border_1().border_color(pal.border).rounded(px(5.)).flex().items_center().justify_center().child(svg().path("icons/plus.svg").size(px(14.)).text_color(pal.sub));
        tiles.push(tile("editor-image-pick".into(), false, t("pill.editor.side.imageUpload").into(), add.into_any_element()).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.side_pick_image(cx))).into_any_element());
        if let Some(path) = own.clone() {
            let on = active.as_deref() == Some(path.as_path());
            let tip = file_label(&path);
            let source = gpui::ImageSource::from(path.clone());
            tiles.push(tile("editor-image-own".into(), on, tip, face(source)).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.side_use_image(path.clone(), cx))).into_any_element());
        }
        for (n, id) in wallpapers::ids().enumerate() {
            let reference = wallpapers::reference(id);
            let on = active.as_deref() == Some(reference.as_path());
            let tip = crate::i18n::tf("pill.editor.side.wallpaperTip", &[("n", &(n + 1))]);
            tiles.push(tile(("editor-wallpaper", n).into(), on, tip.into(), face(gpui::ImageSource::from(wallpapers::thumb_asset(id)))).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.side_use_image(reference.clone(), cx))).into_any_element());
        }
        let mut body = vec![
            div().text_size(px(T_BODY)).text_color(pal.sub).child(t("pill.editor.side.pictures")).into_any_element(),
            div().flex().flex_wrap().gap(px(TILE_GAP)).children(tiles).into_any_element(),
        ];
        // La imagen propia: su nombre y quitarla (quitarla no destruye nada: el fondo vuelve al anterior).
        if let Some(path) = &own {
            let on = active.as_deref() == Some(path.as_path());
            body.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(svg().path("icons/image.svg").size(px(14.)).flex_none().text_color(if on { pal.text } else { pal.faint }))
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(T_BODY)).text_color(if on { pal.text } else { pal.sub }).child(short_name(path)))
                    .child(self.button(pal, "editor-image-remove", "icons/x.svg", None, t("pill.editor.side.imageRemove"), |view, _, cx| view.side_remove_image(cx), cx))
                    .into_any_element(),
            );
        }
        if self.style_ui.image_error {
            body.push(div().text_size(px(T_SMALL)).text_color(pal.cut).child(t("pill.editor.side.imageBad")).into_any_element());
        }
        div().flex().flex_col().gap(px(6.)).children(body).into_any_element()
    }

    /// La imagen que el usuario trajo de su disco, si hay (los proyectos de antes la guardaban en
    /// `backdrop_image`, sin `backdrop_own`).
    fn own_image(&self) -> Option<PathBuf> {
        self.project.backdrop_own.clone().or_else(|| self.project.backdrop_image.clone().filter(|p| wallpapers::id_of(p).is_none()))
    }

    /// Poner `image` de fondo: una imagen propia o `wallpaper:NN`. La propia se queda a mano en el selector.
    fn side_use_image(&mut self, image: PathBuf, cx: &mut Context<Self>) {
        self.style_ui.image_error = false;
        self.project.backdrop_own = if wallpapers::id_of(&image).is_some() { self.own_image() } else { Some(image.clone()) };
        self.project.backdrop_image = Some(image);
        self.project.look.backdrop = Backdrop::Image;
        self.changed_keyed("backdrop-color", cx);
    }

    /// El selector del sistema, solo para imágenes (el diálogo de GPUI no filtra por
    /// tipo: lo que no lo sea se rechaza al volver).
    fn side_pick_image(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(t("pill.editor.side.imagePrompt").into()) });
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = view.update(cx, |view, cx| {
                if !is_image(&path) {
                    view.style_ui.image_error = true;
                    cx.notify();
                    return;
                }
                view.side_use_image(path, cx);
            });
        })
        .detach();
    }

    /// Quita la imagen propia; si era el fondo, vuelve al primer degradado (el marco sigue).
    fn side_remove_image(&mut self, cx: &mut Context<Self>) {
        let own = self.own_image();
        self.project.backdrop_own = None;
        if self.project.backdrop_image == own {
            self.project.backdrop_image = None;
            if self.project.look.backdrop == Backdrop::Image {
                self.project.look.backdrop = BACKDROPS[1];
            }
        }
        self.style_ui.image_error = false;
        self.changed(cx);
    }

    /// Colores propios: degradado o color sólido, con campos en hex.
    fn side_custom_colors(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let backdrop = self.project.look.backdrop;
        let (a, b) = custom_pair(backdrop, self.style_ui.custom.get());
        self.style_ui.custom.set((a, b));
        let ui = &self.style_ui;
        StyleUi::sync(&ui.back_shown[0], &ui.back_hex[0], a, cx);
        StyleUi::sync(&ui.back_shown[1], &ui.back_hex[1], b, cx);
        let solid = matches!(backdrop, Backdrop::Solid(_));
        let modes = [(false, SharedString::from(t("pill.editor.side.gradient"))), (true, SharedString::from(t("pill.editor.side.solid")))];
        let mode = self.side_pick(pal, "editor-back-mode", t("pill.editor.side.custom"), 2, &modes, |v| matches!(backdrop, Backdrop::Gradient(..) | Backdrop::Solid(_)) && v == solid, |view, solid| {
            view.project.look.backdrop = to_mode(view.project.look.backdrop, solid, view.style_ui.custom.get());
        }, Some("backdrop-color"), cx);
        let column = |label: &'static str, color: u32, n: usize| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(label))
                .child(hex_field(pal, color, &ui.back_hex[n], cx))
        };
        let mut fields = div().flex().gap(px(8.)).child(column(if solid { t("pill.editor.side.color") } else { t("pill.editor.side.colorA") }, a, 0));
        if !solid {
            fields = fields.child(column(t("pill.editor.side.colorB"), b, 1));
        }
        div().flex().flex_col().gap(px(8.)).child(mode).child(fields).into_any_element()
    }

    /// Se escribió en el campo `n` de los colores propios del fondo.
    fn side_back_hex_typed(&mut self, n: usize, value: &str, cx: &mut Context<Self>) {
        let Some(color) = parse_hex(value) else { return };
        let (a, b) = self.style_ui.custom.get();
        let pair = if n == 0 { (color, b) } else { (a, color) };
        self.style_ui.custom.set(pair);
        self.style_ui.back_shown[n.min(1)].set(Some(color));
        let next = with_colors(self.project.look.backdrop, pair);
        if next != self.project.look.backdrop {
            self.project.look.backdrop = next;
            self.changed_keyed("backdrop-color", cx);
        }
    }

    // --- Cursor -------------------------------------------------------------------

    /// El panel Cursor: forma, tamaño y suavizado; el efecto del clic (tipo, tamaño y color);
    /// y esconderlo cuando está quieto. Si el cursor viene en el video, solo el efecto del clic.
    fn side_cursor(&self, pal: &Ed, cx: &mut Context<Self>) -> Div {
        let cursor = self.project.cursor;
        let parts = cursor_parts(self.cursor_drawn, cursor.click, cursor.hide_idle);
        let mut groups = Vec::new();
        if parts.note {
            groups.push(Self::page(pal, t("pill.editor.cursor"), vec![note(pal, t("pill.editor.cursorBaked"))]));
        }
        if parts.look {
            let body = vec![
                self.side_cursor_styles(pal, cx),
                self.side_slider(pal, Slider::CursorSize, t("pill.editor.cursorSize"), cx),
                self.side_slider(pal, Slider::CursorSmooth, t("pill.editor.smooth"), cx),
            ];
            groups.push(Self::page(pal, t("pill.editor.cursor"), body));
        }
        let kinds: Vec<_> = ClickKind::ALL.iter().map(|&kind| (kind, SharedString::from(click_kind_name(kind)))).collect();
        let mut click = vec![self.side_pick(pal, "editor-click-kind", t("pill.editor.side.effect"), 2, &kinds, |v| v == cursor.click, |view, v| view.project.cursor.click = v, Some("click-kind"), cx)];
        if parts.click_detail {
            click.push(self.side_slider(pal, Slider::ClickSize, t("pill.editor.side.clickSize"), cx));
            StyleUi::sync(&self.style_ui.click_shown, &self.style_ui.click_hex, cursor.click_color, cx);
            click.push(self.side_colors(pal, "editor-click-color", t("pill.editor.side.clickColor"), cursor.click_color, &self.style_ui.click_hex, |view, c, cx| {
                view.project.cursor.click_color = c;
                view.changed_keyed("click-color", cx);
            }, cx));
        }
        if parts.shrink {
            click.push(self.switch_row(pal, "editor-press-shrink", t("pill.editor.side.pressShrink"), cursor.press_shrink, |view, _, cx| {
                view.project.cursor.press_shrink = !view.project.cursor.press_shrink;
                view.changed_keyed("press-shrink", cx);
            }, cx));
        }
        groups.push(Self::section(pal, t("pill.editor.side.clickGroup"), click));
        if parts.idle {
            let mut idle = vec![self.switch_row(pal, "editor-hide-idle", t("pill.editor.side.hideIdle"), cursor.hide_idle, |view, _, cx| {
                view.project.cursor.hide_idle = !view.project.cursor.hide_idle;
                view.changed_keyed("hide-idle", cx);
            }, cx)];
            if parts.idle_delay {
                idle.push(self.side_slider(pal, Slider::HideDelay, t("pill.editor.side.hideDelay"), cx));
            }
            groups.push(Self::section(pal, t("pill.editor.side.idleGroup"), idle));
        }
        panel(groups)
    }

    /// Las cuatro formas del cursor (Sistema, Punto, Anillo, Oscuro), cada una con su vista previa.
    fn side_cursor_styles(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let current = self.project.cursor.style;
        let (accent, sel, hover) = (pal.accent, pal.sel, pal.hover);
        let tiles: Vec<AnyElement> = CursorStyle::ALL
            .iter()
            .enumerate()
            .map(|(n, &style)| {
                let on = style == current;
                let id = SharedString::from(format!("editor-cursor-style-{n}"));
                let on_t = self.fluid.toggle(id.clone(), on, fluid::CHIP_FADE, fluid::CHIP_FADE);
                let ring = pal.border.opacity(0.0);
                div()
                    .id(id.clone())
                    .flex_1()
                    .h(px(STYLE_TILE_H))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .rounded(px(pal.r_ctl))
                    .border_1()
                    .border_color(mix(ring, accent, on_t))
                    .cursor_pointer()
                    .tooltip(super::keys::tip(cursor_style_name(style)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.project.cursor.style = style;
                        view.changed_keyed("cursor-style", cx);
                    }))
                    .child(cursor_swatch(pal, style))
                    .child(div().text_size(px(T_SMALL)).text_color(mix(pal.sub, pal.text, on_t)).child(cursor_style_name(style)))
                    .fx(SharedString::from(format!("{id}-fx")), move |el, h| el.bg(fluid::select_bg(h, on_t, hover, sel)))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(t("pill.editor.side.cursorStyle")))
            .child(div().flex().gap(px(4.)).children(tiles))
            .into_any_element()
    }

    // --- Zoom ---------------------------------------------------------------------

    /// El panel Zoom: el preajuste (Calmo, Normal, Dinámico), la profundidad, la velocidad y la
    /// curva; cómo sigue la cámara al mouse; y los botones de los zooms.
    fn side_zoom(&self, pal: &Ed, cx: &mut Context<Self>) -> Div {
        let comma = !crate::i18n::english();
        let camera = self.project.camera_opts;
        let active = preset_of(&self.project);
        let presets = [
            (CameraStyle::Calm, SharedString::from(t("pill.editor.side.camCalm"))),
            (CameraStyle::Normal, SharedString::from(t("pill.editor.side.camNormal"))),
            (CameraStyle::Dynamic, SharedString::from(t("pill.editor.side.camDynamic"))),
        ];
        let hint = match active {
            Preset::Style(CameraStyle::Calm) => t("pill.editor.side.camCalmHint"),
            Preset::Style(CameraStyle::Normal) => t("pill.editor.side.camNormalHint"),
            Preset::Style(CameraStyle::Dynamic) => t("pill.editor.side.camDynamicHint"),
            Preset::Custom => t("pill.editor.side.camCustomHint"),
        };
        let speeds = [
            (ZoomSpeed::Fast, SharedString::from(t("pill.editor.side.speedFast"))),
            (ZoomSpeed::Normal, SharedString::from(t("pill.editor.side.camNormal"))),
            (ZoomSpeed::Slow, SharedString::from(t("pill.editor.side.speedSlow"))),
        ];
        let eases = [
            (Ease::Smooth, SharedString::from(t("pill.editor.side.easeSmooth"))),
            (Ease::Snappy, SharedString::from(t("pill.editor.side.easeSnappy"))),
            (Ease::Linear, SharedString::from(t("pill.editor.side.easeLinear"))),
        ];
        let speed_label = format!("{} · {}", t("pill.editor.side.speed"), localized(camera.ramp_text(), comma));
        // Cambiar el preajuste o la velocidad cambia la rampa, y con ella qué zooms automáticos
        // se juntan (`tidy`): se ordenan de nuevo antes de registrar el paso.
        let zoom = vec![
            self.side_pick(pal, "editor-camera", t("pill.editor.side.preset"), 3, &presets, |v| active == Preset::Style(v), |view, v| {
                view.project.apply_style(v);
                let duration = view.duration;
                view.project.tidy(duration);
            }, Some("camera-style"), cx),
            note(pal, hint),
            self.side_slider(pal, Slider::Depth, t("pill.editor.side.zoomDepth"), cx),
            note(pal, t("pill.editor.side.zoomDepthHint")),
            self.side_pick(pal, "editor-zoom-speed", speed_label, 3, &speeds, |v| v == camera.speed, |view, v| {
                view.project.camera_opts.speed = v;
                let duration = view.duration;
                view.project.tidy(duration);
            }, Some("camera-speed"), cx),
            self.side_pick(pal, "editor-zoom-ease", t("pill.editor.side.zoomCurve"), 3, &eases, |v| v == camera.ease, |view, v| view.project.camera_opts.ease = v, Some("camera-ease"), cx),
        ];
        let follows = [
            (Follow::Centered, t("pill.editor.side.followCentered"), t("pill.editor.side.followCenteredHint")),
            (Follow::Balanced, t("pill.editor.side.followBalanced"), t("pill.editor.side.followBalancedHint")),
            (Follow::Free, t("pill.editor.side.followFree"), t("pill.editor.side.followFreeHint")),
        ];
        let follow_now = self.project.motion.follow;
        let follow = vec![
            self.side_rows(pal, "editor-follow", t("pill.editor.side.follow"), &follows, |v| v == follow_now, |view, v| view.project.motion.follow = v, "follow", cx),
            self.side_slider(pal, Slider::Smoothing, t("pill.editor.side.cameraSmoothing"), cx),
        ];
        // El zoom automático, como el «Auto zoom» de Screen Studio.
        let mut zooms = Vec::new();
        let autos = self.project.zooms.iter().filter(|z| z.auto).count();
        let typed = self.track.as_ref().map_or(0, |t| t.keys.len());
        if self.clicks.is_empty() && typed == 0 {
            zooms.push(note(pal, t("pill.editor.noTrack")));
        } else {
            zooms.push(self.primary(pal, "editor-auto", "icons/sparkles.svg", t("pill.editor.side.autoZoom").into(), t("pill.editor.autofocusTip"), |view, window, cx| view.autofocus(window, cx), cx));
            zooms.push(note(pal, crate::i18n::tf("pill.editor.autofocusCount", &[("clicks", &self.clicks.len()), ("keys", &typed), ("zooms", &autos)])));
        }
        if autos > 0 {
            zooms.push(self.action(pal, "editor-auto-clear", "icons/eraser.svg", t("pill.editor.side.removeAuto"), t("pill.editor.side.removeAutoTip"), Tone::Danger, |view, window, cx| {
                let removed = remove_auto_zooms(&mut view.project);
                if removed > 0 {
                    view.changed(cx);
                    view.select(None, window, cx);
                    view.say_with(crate::i18n::tf("pill.editor.autoRemoved", &[("count", &removed)]), false, Some(super::NoticeAction::Undo), cx);
                }
            }, cx));
        }
        zooms.push(self.action(pal, "editor-zoom-add", "icons/magnify.svg", t("pill.editor.zoomAdd"), t("pill.editor.zoomTip"), Tone::Normal, |view, window, cx| view.add_zoom(window, cx), cx));
        panel(vec![
            Self::page(pal, t("pill.editor.zoom"), zoom),
            Self::section(pal, t("pill.editor.side.followGroup"), follow),
            Self::section(pal, t("pill.editor.side.zoomsGroup"), zooms),
        ])
    }

    // --- Movimiento ---------------------------------------------------------------

    /// El panel Movimiento: el desenfoque de movimiento y «Ver cómo queda»; y la velocidad y
    /// el recorte.
    fn side_motion(&self, pal: &Ed, cx: &mut Context<Self>) -> Div {
        let (icon, label, tip) = match self.preview {
            None => ("icons/play.svg", t("pill.editor.preview"), t("pill.editor.previewTip")),
            Some(Preview::Rendering { .. }) => ("icons/x.svg", t("pill.editor.previewCancel"), t("pill.editor.previewCancelTip")),
            Some(Preview::Ready(_)) => ("icons/x.svg", t("pill.editor.previewClose"), t("pill.editor.previewCloseTip")),
        };
        let blur = vec![
            self.side_slider(pal, Slider::Blur, t("pill.editor.motionBlur"), cx),
            note(pal, t("pill.editor.side.motionBlurHint")),
            self.action(pal, "editor-preview", icon, label, tip, Tone::Normal, |view, _, cx| view.toggle_preview(cx), cx),
        ];
        // Los pares (acelerar / cortar silencios, agregar velocidad / difuminado) van en una fila de dos columnas.
        let pair = |a: AnyElement, b: AnyElement| div().flex().gap(px(6.)).child(div().flex_1().min_w_0().child(a)).child(div().flex_1().min_w_0().child(b)).into_any_element();
        let timing = vec![
            pair(
                self.action(pal, "editor-idle-speed", "icons/timer.svg", t("pill.editor.side.speedUpIdle"), t("pill.editor.side.speedUpIdleTip"), Tone::Normal, |view, window, cx| view.speed_up_idle(window, cx), cx),
                self.action(pal, "editor-idle-cut", "icons/scissors.svg", t("pill.editor.side.cutIdle"), t("pill.editor.side.cutIdleTip"), Tone::Danger, |view, window, cx| view.cut_idle(window, cx), cx),
            ),
            note(pal, t("pill.editor.side.idleHint")),
            pair(
                self.action(pal, "editor-speed-add", "icons/gauge.svg", t("pill.editor.side.speedAdd"), t("pill.editor.side.speedAddTip"), Tone::Normal, |view, window, cx| view.add_speed(window, cx), cx),
                self.action(pal, "editor-blur-add", "icons/pixelate.svg", t("pill.editor.side.blurAdd"), t("pill.editor.side.blurAddTip"), Tone::Normal, |view, window, cx| view.add_blur(window, cx), cx),
            ),
        ];
        panel(vec![Self::page(pal, t("pill.editor.side.tabMotion"), blur), Self::section(pal, t("pill.editor.side.timing"), timing)])
    }

    // --- Texto --------------------------------------------------------------------

    /// El panel Texto: agregar uno y cómo se edita (las opciones del elegido van arriba, en «Lo elegido»).
    fn side_text_tab(&self, pal: &Ed, cx: &mut Context<Self>) -> Div {
        // Con un texto elegido, sus opciones ya están en la tarjeta de arriba: no se repite la ayuda.
        let help = text_tab_help(matches!(self.selected, Some(Item::Text(_))));
        let mut body = vec![self.action(pal, "editor-text-add", "icons/type.svg", t("pill.editor.side.textAdd"), t("pill.editor.textTip"), Tone::Normal, |view, window, cx| view.add_text(window, cx), cx)];
        body.extend(help.map(|help| note(pal, help)));
        panel(vec![Self::page(pal, t("pill.editor.text"), body)])
    }

    // --- Proyecto -----------------------------------------------------------------

    /// Deshacer, rehacer, restablecer (con una segunda pulsación para confirmar) y los atajos.
    fn side_project(&self, pal: &Ed, cx: &mut Context<Self>) -> Div {
        let (can_undo, can_redo) = (self.history.can_undo(), self.history.can_redo());
        let half = |el: AnyElement| div().flex_1().min_w_0().child(el);
        let history = div()
            .flex()
            .gap(px(6.))
            .child(half(self.side_ghost(pal, "editor-undo", "icons/undo.svg", t("pill.editor.side.undo"), t("pill.editor.side.undoTip"), can_undo, |view, window, cx| view.undo(window, cx), cx)))
            .child(half(self.side_ghost(pal, "editor-redo", "icons/redo.svg", t("pill.editor.side.redo"), t("pill.editor.side.redoTip"), can_redo, |view, window, cx| view.redo(window, cx), cx)))
            .into_any_element();
        let reset = if armed(self.style_ui.reset_until, Instant::now()) {
            self.primary(pal, "editor-reset", "icons/rotate-ccw.svg", t("pill.editor.side.resetSure").into(), t("pill.editor.side.resetSureTip"), |view, window, cx| {
                view.style_ui.reset_until = None;
                view.reset_project(window, cx);
            }, cx)
        } else {
            self.action(pal, "editor-reset", "icons/rotate-ccw.svg", t("pill.editor.side.reset"), t("pill.editor.side.resetTip"), Tone::Danger, |view, _, cx| {
                let until = Instant::now() + RESET_CONFIRM;
                view.style_ui.reset_until = Some(until);
                // Pasado un rato sin confirmar, la pregunta se retira sola.
                cx.spawn(async move |view, cx| {
                    cx.background_executor().timer(RESET_CONFIRM).await;
                    let _ = view.update(cx, |view, cx| {
                        if view.style_ui.reset_until == Some(until) {
                            view.style_ui.reset_until = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
                cx.notify();
            }, cx)
        };
        // Los atajos ya no viven aquí: están en el pie de la barra («Atajos», o la tecla «?»), en toda pestaña.
        panel(vec![Self::page(pal, t("pill.editor.side.project"), vec![history, reset])])
    }

    // --- La barra entera ----------------------------------------------------------

    /// El panel de la pestaña activa.
    fn side_panel(&self, tab: Tab, pal: &Ed, cx: &mut Context<Self>) -> Div {
        match tab {
            Tab::Frame => self.side_frame(pal, cx),
            Tab::Cursor => self.side_cursor(pal, cx),
            Tab::Zoom => self.side_zoom(pal, cx),
            Tab::Motion => self.side_motion(pal, cx),
            Tab::Text => self.side_text_tab(pal, cx),
            Tab::Captions => panel(vec![div().child(self.captions_section(pal, cx))]),
            // El chat ocupa toda la columna (`sidebar_panel`): aquí no se llega.
            Tab::Assistant => div(),
            Tab::Session => self.side_project(pal, cx),
        }
    }

    /// TODA la columna derecha. Arriba, la fila de pestañas, **fija** (Fondo, Cursor, Zoom, Movimiento, Texto, Subtítulos y
    /// Proyecto): elegir un bloque en la línea de tiempo ya no la empuja fuera de la vista. Debajo, lo que se desplaza: la
    /// tarjeta de lo elegido (si hay algo) y el panel de la pestaña activa. Abajo, el pie con el enlace a los atajos. Un
    /// degradado en el borde de arriba o de abajo avisa que queda contenido por ver (con vidrio, un filete).
    pub(super) fn sidebar_panel(&self, pal: &Ed, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.side_selection(pal, cx);
        let tabs = self.side_tabs(pal, cx);
        // El panel de la pestaña nueva entra con un fundido y corrido unos px hacia el lado al que
        // se fue; el anterior se va de golpe (una sola copia a la vez: cambiar rápido no deja restos).
        // El progreso sale de cuándo cambió la pestaña, no de cuántas veces se dibujó.
        let (changed_at, dir) = self.style_ui.tab_at;
        let enter = self.fluid.since(changed_at, fluid::TAB_BODY);
        let shift = dir * self.fluid.lift(enter, fluid::TAB_SHIFT);
        let body = div().relative().left(px(shift)).opacity(enter).child(self.side_panel(self.style_ui.tab, pal, cx));
        // ¿Queda contenido arriba o abajo? Lo dice el desplazamiento del cuadro anterior; si lo que se puede
        // desplazar cambió (otra pestaña, otra tarjeta), se vuelve a dibujar una vez para ponerlo al día.
        let scroll = &self.style_ui.scroll;
        let (offset, max) = (f32::from(scroll.offset().y), f32::from(scroll.max_offset().height));
        if (max - self.style_ui.last_max.get()).abs() > SCROLL_SLACK {
            self.style_ui.last_max.set(max);
            cx.notify();
        }
        let (more_above, more_below) = scroll_hints(offset, max);
        let edge = |top: bool, on: bool| -> Option<Div> {
            on.then(|| {
                let el = div().absolute().left_0().right_0();
                let el = if top { el.top_0() } else { el.bottom_0() };
                if pal.glass {
                    // Sobre un vidrio no hay color plano al que desvanecerse: un filete tenue.
                    el.h(px(1.)).bg(pal.sub.opacity(SCROLL_RULE_ALPHA))
                } else {
                    let solid = over(pal.panel, pal.bg);
                    let (from, to) = if top { (solid, solid.opacity(0.0)) } else { (solid.opacity(0.0), solid) };
                    el.h(px(SCROLL_FADE_H)).bg(linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.)))
                }
            })
        };
        // El panel (con su fondo o vidrio) no se mueve y recorta; adentro, la lista
        // se desplaza. Con el fondo en el mismo elemento que se desplaza, el vidrio
        // quedaba del alto de la ventana y lo de abajo se salía.
        pal.panel(div(), pal.r_panel)
            .w(px(SIDEBAR_W))
            .flex_none()
            .h_full()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(div().flex_none().px(px(SIDEBAR_PAD)).pt(px(SIDEBAR_PAD)).pb(px(TABS_BELOW)).child(tabs))
            .child(
                if self.style_ui.tab == Tab::Assistant {
                    // El chat no se desplaza con la barra: su conversación sí, y el campo queda abajo.
                    div().relative().flex_1().min_h_0().px(px(SIDEBAR_PAD)).pb(px(SIDEBAR_PAD)).child(self.assistant.clone())
                } else {
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(div().id("editor-sidebar").size_full().overflow_y_scroll().track_scroll(scroll).px(px(SIDEBAR_PAD)).pb(px(SIDEBAR_PAD)).flex().flex_col().children(selection).child(body))
                        .children(edge(true, more_above))
                        .children(edge(false, more_below))
                },
            )
            .child(div().flex_none().px(px(SIDEBAR_PAD - 6.0)).pt(px(4.)).pb(px(8.)).border_t_1().border_color(pal.border.opacity(0.35)).child(self.help_link(pal, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_project::Zoom;

    #[test]
    fn el_nombre_de_la_imagen_se_acorta_sin_extension() {
        assert_eq!(short_name(Path::new("/x/foto.png")).as_ref(), "foto");
        let largo = short_name(Path::new("/x/Whimsical Miniature Robot Office Deluxe Edition.png"));
        assert!(largo.ends_with('…') && largo.chars().count() <= 28, "{largo}");
        assert_eq!(short_name(Path::new("/x/wallpaper:11")).as_ref(), "wallpaper:11");
    }

    #[test]
    fn parse_hex_acepta_seis_digitos_con_o_sin_almohadilla() {
        assert_eq!(parse_hex("#FF7E5F"), Some(0xff7e5f));
        assert_eq!(parse_hex("ff7e5f"), Some(0xff7e5f));
        assert_eq!(parse_hex("  #00ff00 "), Some(0x00ff00));
        assert_eq!(parse_hex("#000000"), Some(0));
    }

    #[test]
    fn parse_hex_rechaza_lo_que_no_es_un_color() {
        for bad in ["", "#", "#fff", "#12345", "#ff7e5f0", "#gggggg", "+12345", "-12345", "#ff 7e5f", "#ÿÿÿÿÿÿ"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn format_hex_va_en_mayusculas_con_seis_digitos() {
        assert_eq!(format_hex(0xff7e5f), "#FF7E5F");
        assert_eq!(format_hex(0), "#000000");
        assert_eq!(format_hex(0x00abcd), "#00ABCD");
        assert_eq!(parse_hex(&format_hex(0x123456)), Some(0x123456));
    }

    #[test]
    fn la_muestra_mas_cercana_solo_cuenta_si_esta_cerca() {
        assert_eq!(nearest_swatch(&TEXT_COLORS, 0xffffff, SWATCH_NEAR), Some(0));
        assert_eq!(nearest_swatch(&TEXT_COLORS, 0x000000, SWATCH_NEAR), Some(1));
        // Un nivel de diferencia por canal: distancia 3.
        assert_eq!(nearest_swatch(&TEXT_COLORS, 0xfefefe, SWATCH_NEAR), Some(0));
        assert_eq!(nearest_swatch(&TEXT_COLORS, 0xff453a, SWATCH_NEAR), Some(3));
        // Un gris del medio no es ninguna.
        assert_eq!(nearest_swatch(&TEXT_COLORS, 0x808080, SWATCH_NEAR), None);
        assert_eq!(nearest_swatch(&[], 0xffffff, SWATCH_NEAR), None);
        // Con dos candidatas, gana la más cerca.
        assert_eq!(nearest_swatch(&[0x101010, 0x0c0c0c], 0x0a0a0a, 200), Some(1));
    }

    #[test]
    fn los_colores_propios_siguen_al_fondo() {
        let last = (0x111111, 0x222222);
        assert_eq!(custom_pair(Backdrop::Gradient(1, 2), last), (1, 2));
        assert_eq!(custom_pair(Backdrop::Solid(7), last), (7, 0x222222));
        assert_eq!(custom_pair(Backdrop::None, last), last);
        assert_eq!(custom_pair(Backdrop::Image, last), last);
    }

    #[test]
    fn escribir_un_color_arma_el_fondo_que_corresponde() {
        assert_eq!(with_colors(Backdrop::Gradient(1, 2), (3, 4)), Backdrop::Gradient(3, 4));
        assert_eq!(with_colors(Backdrop::Solid(1), (3, 4)), Backdrop::Solid(3));
        assert_eq!(with_colors(Backdrop::None, (3, 4)), Backdrop::Gradient(3, 4));
        assert_eq!(with_colors(Backdrop::Image, (3, 4)), Backdrop::Gradient(3, 4));
    }

    #[test]
    fn pasar_de_degradado_a_solido_y_de_vuelta() {
        let last = (0xaaaaaa, 0xbbbbbb);
        assert_eq!(to_mode(Backdrop::Gradient(1, 2), true, last), Backdrop::Solid(1));
        assert_eq!(to_mode(Backdrop::Solid(5), false, last), Backdrop::Gradient(5, 0xbbbbbb));
        assert_eq!(to_mode(Backdrop::Gradient(1, 2), false, last), Backdrop::Gradient(1, 2));
        assert_eq!(to_mode(Backdrop::Solid(5), true, last), Backdrop::Solid(5));
        assert_eq!(to_mode(Backdrop::None, true, last), Backdrop::Solid(0xaaaaaa));
        assert_eq!(to_mode(Backdrop::Image, false, last), Backdrop::Gradient(0xaaaaaa, 0xbbbbbb));
    }

    #[test]
    fn quitar_los_zooms_automaticos_respeta_los_manuales() {
        let zoom = |start: f64, auto: bool| Zoom { start, end: start + 2.0, scale: 2.0, focus: Focus::Auto, auto };
        let mut project = Project::default();
        project.zooms = vec![zoom(0.0, true), zoom(5.0, false), zoom(10.0, true)];
        assert_eq!(remove_auto_zooms(&mut project), 2);
        assert_eq!(project.zooms.len(), 1);
        assert_eq!(project.zooms[0].start, 5.0);
        assert_eq!(remove_auto_zooms(&mut project), 0);
    }

    #[test]
    fn los_numeros_van_sin_ceros_de_sobra() {
        assert_eq!(fmt_num(2.0, false), "2");
        assert_eq!(fmt_num(1.5, false), "1.5");
        assert_eq!(fmt_num(1.25, false), "1.25");
        assert_eq!(fmt_num(1.25, true), "1,25");
        assert_eq!(fmt_num(16.0, true), "16");
        assert_eq!(fmt_num(10.0, false), "10");
        assert_eq!(fmt_num(0.0, false), "0");
    }

    #[test]
    fn los_textos_del_modelo_pasan_a_coma_decimal() {
        assert_eq!(localized("1.5×".into(), true), "1,5×");
        assert_eq!(localized("1.5×".into(), false), "1.5×");
        assert_eq!(localized("50 %".into(), true), "50 %");
    }

    #[test]
    fn dos_numeros_cercanos_son_el_mismo_valor() {
        assert!(same(1.5, 1.501));
        assert!(!same(0.02, 0.04));
        assert!(!same(0.02, 0.045));
        // Las escalas de zoom y de velocidad nunca se confunden.
        for pair in ZOOM_SCALES.windows(2) {
            assert!(!same(pair[0], pair[1]));
        }
        for pair in SPEED_RATES.windows(2) {
            assert!(!same(pair[0], pair[1]));
        }
    }

    #[test]
    fn solo_se_aceptan_imagenes() {
        assert!(is_image(Path::new("/tmp/fondo.PNG")));
        assert!(is_image(Path::new("/tmp/fondo.jpeg")));
        assert!(is_image(Path::new("fondo.heic")));
        assert!(!is_image(Path::new("/tmp/video.mp4")));
        assert!(!is_image(Path::new("/tmp/sin_extension")));
        assert!(!is_image(Path::new("/tmp/png")));
    }

    #[test]
    fn las_filas_incompletas_se_rellenan_con_huecos() {
        assert_eq!(spacers(6, 3), 0);
        assert_eq!(spacers(5, 5), 0);
        assert_eq!(spacers(2, 3), 1);
        assert_eq!(spacers(1, 2), 1);
        assert_eq!(spacers(5, 4), 3);
        assert_eq!(spacers(0, 3), 0);
        assert_eq!(spacers(3, 0), 0);
    }

    #[test]
    fn la_pregunta_de_restablecer_vence() {
        let now = Instant::now();
        assert!(armed(Some(now + Duration::from_secs(1)), now));
        assert!(!armed(Some(now), now));
        assert!(!armed(None, now));
    }

    // ---- Controles deslizantes ----

    #[test]
    fn la_posicion_del_mouse_cae_en_la_fraccion_de_la_pista() {
        assert_eq!(fraction_at(100.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction_at(300.0, 100.0, 200.0), 1.0);
        // Fuera de la pista se queda en el extremo (el arrastre no se pierde).
        assert_eq!(fraction_at(-50.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(900.0, 100.0, 200.0), 1.0);
        // Una pista sin ancho o una posición rota no inventan valores.
        assert_eq!(fraction_at(150.0, 100.0, 0.0), 0.0);
        assert_eq!(fraction_at(f32::NAN, 100.0, 200.0), 0.0);
    }

    #[test]
    fn la_fraccion_da_un_valor_con_paso_y_limites() {
        assert_eq!(value_at(0.0, (0.5, 4.0), 0.1), 0.5);
        assert_eq!(value_at(1.0, (0.5, 4.0), 0.1), 4.0);
        assert_eq!(value_at(0.5, (0.0, 1.0), 0.05), 0.5);
        // Entre dos pasos, gana el más cercano.
        assert_eq!(value_at(0.52, (0.0, 1.0), 0.05), 0.5);
        assert_eq!(value_at(0.53, (0.0, 1.0), 0.05), 0.55);
        // Fuera de 0–1 o roto: los extremos.
        assert_eq!(value_at(-1.0, (0.0, 1.0), 0.05), 0.0);
        assert_eq!(value_at(2.0, (0.0, 1.0), 0.05), 1.0);
        assert_eq!(value_at(f64::NAN, (0.0, 1.0), 0.05), 0.0);
        // Sin paso, el valor sigue continuo.
        assert_eq!(value_at(0.25, (0.0, 2.0), 0.0), 0.5);
    }

    #[test]
    fn el_valor_vuelve_a_la_fraccion_de_la_pista() {
        assert_eq!(fraction(0.5, (0.0, 1.0)), 0.5);
        assert_eq!(fraction(2.25, (0.5, 4.0)), 0.5);
        assert_eq!(fraction(-3.0, (0.0, 1.0)), 0.0);
        assert_eq!(fraction(9.0, (0.0, 1.0)), 1.0);
        assert_eq!(fraction(f64::NAN, (0.0, 1.0)), 0.0);
        assert_eq!(fraction(1.0, (2.0, 2.0)), 0.0);
    }

    #[test]
    fn las_flechas_dan_un_paso_y_shift_uno_grande() {
        let range = (0.0, 1.0);
        assert_eq!(nudge(0.5, range, 0.05, 1.0, false), 0.55);
        assert_eq!(nudge(0.5, range, 0.05, -1.0, false), 0.45);
        assert_eq!(nudge(0.5, range, 0.05, 1.0, true), 0.75);
        assert_eq!(nudge(0.5, range, 0.05, -1.0, true), 0.25);
        // En los extremos se queda.
        assert_eq!(nudge(1.0, range, 0.05, 1.0, false), 1.0);
        assert_eq!(nudge(0.0, range, 0.05, -1.0, true), 0.0);
        // Un paso que no cae justo en el límite se pega a él.
        assert_eq!(nudge(0.9, range, 0.05, 1.0, true), 1.0);
    }

    #[test]
    fn snap_limpia_el_ruido_de_la_suma() {
        // 1,2 + 8 × 0,1 da 2,0000000000000004 sin limpiar.
        assert_eq!(snap(2.0, (1.2, 4.0), 0.1), 2.0);
        assert_eq!(snap(1.4, (1.2, 4.0), 0.1), 1.4);
        assert_eq!(snap(f64::INFINITY, (1.2, 4.0), 0.1), 1.2);
        assert_eq!(snap(9.0, (1.2, 4.0), 0.1), 4.0);
    }

    #[test]
    fn los_controles_usan_los_limites_del_modelo() {
        assert_eq!(Slider::Padding.spec().range, Look::PADDING_RANGE);
        assert_eq!(Slider::Radius.spec().range, Look::RADIUS_RANGE);
        assert_eq!(Slider::CursorSize.spec().range, CursorOpts::SIZE_RANGE);
        assert_eq!(Slider::ClickSize.spec().range, CursorOpts::CLICK_SIZE_RANGE);
        assert_eq!(Slider::HideDelay.spec().range, CursorOpts::HIDE_DELAY_RANGE);
        assert_eq!(Slider::Depth.spec().range, CameraOpts::DEPTH_RANGE);
        assert_eq!(Slider::Smoothing.spec().range, MotionOpts::SMOOTHING_RANGE);
        assert_eq!(Slider::Blur.spec().range, MotionOpts::BLUR_RANGE);
        // Shadow y suavizado del cursor van de 0 a 1.
        assert_eq!(Slider::Shadow.spec().range, (0.0, 1.0));
        assert_eq!(Slider::CursorSmooth.spec().range, (0.0, 1.0));
    }

    #[test]
    fn cada_control_tiene_su_defecto_en_la_grilla_y_su_rango_en_pasos_enteros() {
        for slider in Slider::ALL {
            let spec = slider.spec();
            assert!(spec.range.0 < spec.range.1, "{slider:?}");
            assert!(spec.range.0 <= spec.default && spec.default <= spec.range.1, "{slider:?}");
            assert_eq!(snap(spec.default, spec.range, spec.step), spec.default, "{slider:?}");
            let steps = (spec.range.1 - spec.range.0) / spec.step;
            assert!((steps - steps.round()).abs() < 1e-6, "{slider:?}: {steps} pasos");
        }
    }

    #[test]
    fn el_valor_por_defecto_de_cada_control_es_el_de_un_proyecto_nuevo() {
        let fresh = Project::default();
        for slider in Slider::ALL {
            assert_eq!(slider.get(&fresh), slider.spec().default, "{slider:?}");
        }
    }

    #[test]
    fn cada_control_lee_y_escribe_su_numero() {
        for slider in Slider::ALL {
            let spec = slider.spec();
            let mut project = Project::default();
            for v in [spec.range.0, spec.range.1] {
                slider.set(&mut project, v);
                assert_eq!(slider.get(&project), v, "{slider:?}");
            }
        }
        // Y cada uno toca solo lo suyo.
        let mut project = Project::default();
        Slider::Padding.set(&mut project, 0.2);
        assert_eq!(project.look.padding, 0.2);
        assert_eq!(project.look.radius, Look::default().radius);
        assert_eq!(project.cursor, CursorOpts::default());
    }

    #[test]
    fn las_claves_de_deshacer_son_distintas() {
        let mut keys: Vec<_> = Slider::ALL.iter().map(|s| s.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SLIDER_COUNT);
        for (n, slider) in Slider::ALL.iter().enumerate() {
            assert_eq!(slider.index(), n);
        }
    }

    #[test]
    fn las_claves_que_no_mueven_la_camara_son_las_de_la_lista() {
        for slider in Slider::ALL {
            assert_eq!(keeps_camera(slider.key()), !slider.recalcs_camera(), "{slider:?}");
        }
        // Las de los grupos de opciones: estilo, color y tamaño no tocan `Motion`; el efecto, el
        // seguimiento y el zoom, sí.
        for key in ["cursor-style", "click-color", "click-size"] {
            assert!(keeps_camera(key), "{key}");
        }
        for key in ["click-kind", "press-shrink", "hide-idle", "camera-style", "camera-speed", "camera-ease", "follow"] {
            assert!(!keeps_camera(key), "{key}");
        }
    }

    #[test]
    fn el_texto_del_suavizado_va_de_sin_suavizar_a_muy_suave() {
        assert_eq!(smooth_band(0.0), 0);
        assert_eq!(smooth_band(0.05), 1);
        // El valor por defecto es «Normal».
        assert_eq!(smooth_band(CursorOpts::default().smooth_level), 2);
        assert_eq!(smooth_band(0.8), 3);
        assert_eq!(smooth_band(1.0), 4);
        let bands: Vec<_> = (0..=20).map(|n| smooth_band(n as f64 * 0.05)).collect();
        assert!(bands.windows(2).all(|w| w[0] <= w[1]), "{bands:?}");
    }

    #[test]
    fn el_texto_del_desenfoque_va_de_apagado_a_alto() {
        assert_eq!(blur_band(0.0), 0);
        assert_eq!(blur_band(0.5), 1);
        assert_eq!(blur_band(MotionOpts::default().blur), 2);
        assert_eq!(blur_band(1.25), 2);
        assert_eq!(blur_band(1.5), 3);
        assert_eq!(blur_band(2.0), 3);
    }

    #[test]
    fn el_preajuste_activo_sale_de_los_numeros() {
        let mut project = Project::default();
        assert_eq!(preset_of(&project), Preset::Style(CameraStyle::Normal));
        for style in CameraStyle::ALL {
            project.apply_style(style);
            assert_eq!(preset_of(&project), Preset::Style(style));
        }
        // Mover un número a mano deja «Personalizado»…
        project.camera_opts.depth = 3.3;
        assert_eq!(preset_of(&project), Preset::Custom);
        // …y también el seguimiento o el suavizado.
        project.apply_style(CameraStyle::Calm);
        project.motion.follow = Follow::Centered;
        assert_eq!(preset_of(&project), Preset::Custom);
        project.apply_style(CameraStyle::Calm);
        project.motion.smoothing = 0.6;
        assert_eq!(preset_of(&project), Preset::Custom);
        // La curva y el desenfoque no cuentan para el preajuste.
        project.apply_style(CameraStyle::Dynamic);
        project.camera_opts.ease = Ease::Linear;
        project.motion.blur = 0.0;
        assert_eq!(preset_of(&project), Preset::Style(CameraStyle::Dynamic));
    }

    #[test]
    fn los_preajustes_caen_en_la_grilla_de_sus_controles() {
        for style in CameraStyle::ALL {
            let mut project = Project::default();
            project.apply_style(style);
            for slider in [Slider::Depth, Slider::Smoothing] {
                let spec = slider.spec();
                let value = slider.get(&project);
                assert_eq!(snap(value, spec.range, spec.step), value, "{style:?} {slider:?}");
            }
        }
    }

    #[test]
    fn el_panel_del_cursor_muestra_lo_que_corresponde() {
        // Dibujado por el editor y con todo prendido: todo.
        let all = cursor_parts(true, ClickKind::Ripple, true);
        assert_eq!(all, CursorParts { note: false, look: true, click_detail: true, shrink: true, idle: true, idle_delay: true });
        // Sin efecto de clic no hay tamaño ni color del clic.
        assert!(!cursor_parts(true, ClickKind::None, true).click_detail);
        // Sin esconderlo, no hay cuánto tarda.
        let kept = cursor_parts(true, ClickKind::Ring, false);
        assert!(kept.idle && !kept.idle_delay);
        // El cursor viene en el video: una nota y solo lo del clic.
        let baked = cursor_parts(false, ClickKind::Pulse, true);
        assert_eq!(baked, CursorParts { note: true, look: false, click_detail: true, shrink: false, idle: false, idle_delay: false });
        assert!(!cursor_parts(false, ClickKind::None, true).click_detail);
    }

    #[test]
    fn las_pestanas_dan_la_vuelta_y_tienen_su_indice() {
        for (n, tab) in Tab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), n);
        }
        assert_eq!(Tab::default(), Tab::Frame);
        assert_eq!(Tab::Frame.step(1), Tab::Cursor);
        assert_eq!(Tab::Frame.step(-1), Tab::Session);
        assert_eq!(Tab::Session.step(1), Tab::Frame);
        assert_eq!(Tab::Zoom.step(-2), Tab::Frame);
        let mut icons: Vec<_> = Tab::ALL.iter().map(|t| t.icon()).collect();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), TAB_COUNT, "cada pestaña con su ícono");
    }
}
