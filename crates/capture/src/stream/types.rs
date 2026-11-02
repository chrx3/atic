//! Lo que se pinta y cómo sale un video editado: los tipos y las cuentas que no dependen
//! del sistema. Los comparten el exportador y el compositor de cada plataforma (macOS:
//! Core Image y AVFoundation, `export_macos.rs`; Windows: Direct2D y Media Foundation,
//! `export_win.rs`), y la interfaz del editor (`recording_editor/`) los arma por igual.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Lo que se ve en un cuadro.
#[derive(Clone, Debug)]
pub struct Decor {
    /// La parte del video que llena el cuadro: x, y, ancho y alto (0–1, desde arriba a
    /// la izquierda). `[0, 0, 1, 1]`: todo, sin zoom.
    pub crop: [f64; 4],
    /// Dónde dibujar el cursor (0–1 del video) y su tamaño (1: normal), si se dibuja.
    pub cursor: Option<(f64, f64, f64)>,
    /// La forma del cursor en este cuadro (flecha, mano, I-beam, cruz).
    pub cursor_shape: CursorShape,
    /// Opacidad del cursor (0–1): baja a 0 cuando el mouse lleva rato quieto.
    pub cursor_alpha: f64,
    /// Los textos visibles (índice en [`Export::texts`]) y su opacidad.
    pub texts: Vec<(usize, f64)>,
    /// Ondas de clic: dónde (0–1 del video) y cuánto van (0–1).
    pub ripples: Vec<(f64, f64, f64)>,
    /// Desenfoque de movimiento: cuánto se corrió la vista en este cuadro (partes de
    /// su ancho y alto) y cuánto cambió el zoom (ancho de antes sobre el de ahora).
    pub blur: Option<(f64, f64, f64)>,
    /// Zonas tapadas (datos sensibles) visibles en este cuadro.
    pub redactions: Vec<Redaction>,
}

impl Default for Decor {
    fn default() -> Self {
        Self {
            crop: [0.0, 0.0, 1.0, 1.0],
            cursor: None,
            cursor_shape: CursorShape::Arrow,
            cursor_alpha: 1.0,
            texts: Vec::new(),
            ripples: Vec::new(),
            blur: None,
            redactions: Vec::new(),
        }
    }
}

/// La forma del cursor del sistema, la que se dibuja.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Arrow,
    /// La mano de los vínculos.
    Hand,
    /// El cursor de texto.
    IBeam,
    Cross,
}

/// Una zona del video que se tapa (difuminada o pixelada), en el original.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Redaction {
    /// x, y, ancho y alto (0–1 del video original, desde arriba a la izquierda).
    pub rect: [f64; 4],
    /// Pixelada en vez de difuminada.
    pub pixelate: bool,
    /// Cuánto se nota (0–1): entra y sale con un fundido.
    pub alpha: f64,
}

/// Un color RGB (0–1).
pub type Rgb = [f64; 3];

/// El fondo del marco.
#[derive(Clone, Debug, PartialEq)]
pub enum Backdrop {
    Solid(Rgb),
    /// De arriba a la izquierda a abajo a la derecha.
    Gradient(Rgb, Rgb),
    /// Una imagen que llena el cuadro (recortada a su proporción, como `object-fit: cover`).
    Image(std::path::PathBuf),
}

/// El marco: el video más chico sobre un fondo, con esquinas redondeadas y sombra.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSpec {
    pub backdrop: Backdrop,
    /// El margen (parte del lado corto del cuadro).
    pub padding: f64,
    /// El radio de las esquinas (parte del alto del cuadro).
    pub radius: f64,
    /// La sombra del video: 0 sin sombra, 0,5 la de siempre, hasta 1 (más oscura y algo
    /// más difuminada).
    pub shadow: f64,
}

/// Dónde va un texto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextAt {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    pub text: String,
    pub at: TextAt,
    /// Tamaño relativo al de siempre (1.0: `alto del cuadro × 0.045`).
    pub size: f64,
    pub color: Rgb,
    /// La píldora oscura detrás del texto.
    pub boxed: bool,
    pub bold: bool,
    /// Subtítulo: se parte en líneas (hasta 80 % del ancho) y va sobre el borde de abajo.
    pub caption: bool,
}

impl TextSpec {
    /// El texto de siempre: blanco, en negrita, sobre su píldora.
    pub fn plain(text: impl Into<String>, at: TextAt) -> Self {
        Self { text: text.into(), at, size: 1.0, color: [1.0; 3], boxed: true, bold: true, caption: false }
    }
}

/// El dibujo del cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorStyle {
    /// La flecha del sistema; la mano, el I-beam y la cruz dibujados aquí (blancos con borde negro).
    #[default]
    System,
    /// Un punto blanco con borde oscuro, para toda forma salvo el I-beam (que conserva el suyo).
    Dot,
    /// Un anillo blanco de centro vacío, para toda forma salvo el I-beam (que conserva el suyo).
    Ring,
    /// Flecha, mano, I-beam y cruz negros con borde blanco.
    Dark,
    /// Siempre la flecha, sea cual sea la forma grabada (mano, I-beam, cruz): el cursor
    /// se ve igual en toda la toma.
    Arrow,
}

/// El efecto de un clic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClickKind {
    /// Sin efecto.
    None,
    /// Un disco suave que crece y se desvanece.
    #[default]
    Ripple,
    /// Un anillo fino que crece y se desvanece.
    Ring,
    /// Un resplandor suave que crece más y se desvanece.
    Pulse,
}

/// Cómo se dibuja el clic (`Decor::ripples` dice dónde y cuánto va).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickSpec {
    pub kind: ClickKind,
    /// Multiplica el radio (0,5–2,5; 1 es el de siempre).
    pub size: f64,
    pub color: Rgb,
}

impl Default for ClickSpec {
    fn default() -> Self {
        Self { kind: ClickKind::Ripple, size: 1.0, color: [1.0; 3] }
    }
}

/// El aspecto de lo que se dibuja encima del video. `Default`: el de siempre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookSpec {
    pub cursor_style: CursorStyle,
    pub click: ClickSpec,
    /// Multiplica el desenfoque de movimiento: 0 lo apaga, 1 es el normal, hasta 2.
    pub blur_amount: f64,
}

impl Default for LookSpec {
    fn default() -> Self {
        Self { cursor_style: CursorStyle::System, click: ClickSpec::default(), blur_amount: 1.0 }
    }
}

/// Lo fijo de lo que se pinta: lo mismo para exportar y para la vista previa. Cambiarlo
/// obliga al [`Composer`] a rearmar lo caro (marco, fondo y textos).
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// Las medidas del video original, en píxeles.
    pub src: (f64, f64),
    /// Las medidas del cuadro de salida, en píxeles (pares).
    pub out: (u32, u32),
    /// El marco y el fondo, si hay.
    pub frame: Option<FrameSpec>,
    pub texts: Vec<TextSpec>,
    /// El alto de la flecha en píxeles del original (sin zoom).
    pub cursor_px: f64,
    pub look: LookSpec,
}

/// Qué se ve en el segundo `src` del original, que en el exportado es `out`.
pub type DecorFn = Box<dyn Fn(f64, f64) -> Decor + Send>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    Low,
    #[default]
    Standard,
    High,
    Max,
}

/// Cómo sale el archivo.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub codec: Codec,
    pub quality: Quality,
    /// Tope del alto del cuadro de salida en píxeles; `None`: el del original. Nunca
    /// agranda: si el cuadro ya es más chico que el tope, queda como está.
    pub max_height: Option<u32>,
    /// Cuadros por segundo de referencia (bitrate y ritmo de salida).
    pub fps: u32,
    /// La proporción del cuadro de salida (ancho, alto); `None`: la del original. Si es
    /// otra, el video entra entero dentro del cuadro (con el margen del marco) sobre el fondo.
    pub aspect: Option<(u32, u32)>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { codec: Codec::H264, quality: Quality::Standard, max_height: None, fps: 60, aspect: None }
    }
}

/// Un tramo del original que va a otra velocidad (segundos del original).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedSpan {
    pub start: f64,
    pub end: f64,
    /// 2.0: al doble. Su audio no se exporta (queda en silencio).
    pub rate: f64,
}

pub struct Export {
    /// Los tramos del original que quedan, en orden (segundos).
    pub segments: Vec<(f64, f64)>,
    /// Tramos que van a otra velocidad (segundos del original, dentro de `segments`).
    pub speeds: Vec<SpeedSpan>,
    pub texts: Vec<TextSpec>,
    /// El alto del cursor en píxeles del original (sin zoom).
    pub cursor_px: f64,
    /// El marco, si hay.
    pub frame: Option<FrameSpec>,
    /// El aspecto del cursor, los clics y el desenfoque (`Default`: el de siempre).
    pub look: LookSpec,
    pub options: ExportOptions,
    pub decor: DecorFn,
    /// Avance de 0 a 1.
    pub progress: Box<dyn Fn(f64) + Send>,
    pub cancel: Arc<AtomicBool>,
}

/// El cuadro de salida `(ancho, alto)`, pares: el alto es el del original sin pasar de
/// `max_height` (nunca agranda) y el ancho sale de `aspect` (o de la proporción del original).
/// Es el `Scene::out` de la exportación: la vista previa lo usa para ver lo mismo.
pub fn output_size((src_w, src_h): (u32, u32), max_height: Option<u32>, aspect: Option<(u32, u32)>) -> (u32, u32) {
    let src_h = src_h.max(2);
    let out_h = (src_h.min(max_height.unwrap_or(u32::MAX).max(2))) & !1;
    let out_w = match aspect {
        Some((aw, ah)) if aw > 0 && ah > 0 => (out_h as u64 * aw as u64 / ah as u64) as u32,
        // El del original: si el alto no cambió, su ancho tal cual.
        _ if out_h == src_h & !1 => src_w,
        _ => (out_h as u64 * src_w as u64 / src_h as u64) as u32,
    };
    ((out_w & !1).max(2), out_h.max(2))
}

/// El margen del marco en píxeles (`padding` es parte del lado corto del cuadro).
pub(crate) fn frame_margin(padding: f64, out_w: f64, out_h: f64) -> f64 {
    padding.clamp(0.0, 0.3) * out_w.min(out_h)
}

/// Dónde va el video dentro del cuadro de salida: `[x, y, ancho, alto]` en píxeles, con
/// el origen abajo a la izquierda (como Core Image). Tiene la proporción del original,
/// entra entero y queda centrado, con `margin` por los lados que lo limitan.
pub(crate) fn content_rect((out_w, out_h): (f64, f64), (src_w, src_h): (f64, f64), margin: f64) -> [f64; 4] {
    let (src_w, src_h) = (src_w.max(1.0), src_h.max(1.0));
    let margin = margin.max(0.0);
    // Una proporción casi igual a la del cuadro: el margen es el mismo de siempre.
    let same = ((src_w * out_h) / (out_w * src_h) - 1.0).abs() < 0.005;
    let (w, h) = if same {
        let w = out_w - margin * 2.0;
        (w, w * out_h / out_w)
    } else if src_w * out_h > out_w * src_h {
        let w = out_w - margin * 2.0;
        (w, w * src_h / src_w)
    } else {
        let h = out_h - margin * 2.0;
        (h * src_w / src_h, h)
    };
    let w = w.round().clamp(2.0, out_w);
    let h = h.round().clamp(2.0, out_h);
    [((out_w - w) / 2.0).floor(), ((out_h - h) / 2.0).floor(), w, h]
}

/// Un número de aspecto dentro de `[lo, hi]`; si no es finito, `default`.
pub(crate) fn normalized(value: f64, lo: f64, hi: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.clamp(lo, hi)
    } else {
        default
    }
}

/// Los filtros del desenfoque de movimiento de un cuadro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlurPlan {
    /// Radio y ángulo (radianes, contra el reloj desde la derecha) del `CIMotionBlur`.
    pub(crate) pan: Option<(f64, f64)>,
    /// La cantidad del `CIZoomBlur`.
    pub(crate) zoom: Option<f64>,
}

/// Qué desenfoque lleva un cuadro del video de `content_w × content_h` píxeles, si lleva: `blur`
/// es lo que se corrió la vista (partes de su ancho y alto) y cuánto cambió el zoom. `amount`
/// (0–2) multiplica el radio y la cantidad y los topes; con 0 no hay desenfoque.
pub(crate) fn blur_plan(amount: f64, (content_w, content_h): (f64, f64), (dx, dy, ratio): (f64, f64, f64)) -> Option<BlurPlan> {
    let amount = normalized(amount, 0.0, 2.0, 1.0);
    if amount <= 0.0 {
        return None;
    }
    let (mx, my) = (dx * content_w, dy * content_h);
    // Medido como en 1080p: en 4K el mismo movimiento lleva el doble de píxeles.
    let k = (content_h / 1080.0).max(0.5);
    // Una fracción del recorrido de un cuadro (un obturador corto) y con tope:
    // más se ve como un video borroso, no como movimiento.
    let pan = ((mx * mx + my * my).sqrt() * 0.2 * amount).min(7.0 * k * amount);
    let zoom = ((1.0 - ratio).abs() * content_w * 0.08 * amount).min(3.0 * k * amount);
    let pan = (pan >= 1.5 * k).then(|| (pan, (-my).atan2(mx)));
    let zoom = (zoom >= 1.0 * k).then_some(zoom);
    (pan.is_some() || zoom.is_some()).then_some(BlurPlan { pan, zoom })
}

/// Cómo se dibuja un clic en un cuadro, en píxeles del cuadro de salida.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ClickDraw {
    /// Un disco: opaco hasta `solid` y desvanecido hasta `radius`.
    Disk { radius: f64, solid: f64, alpha: f64 },
    /// Un anillo de `radius` (borde de afuera) y `thickness`.
    Ring { radius: f64, thickness: f64, alpha: f64 },
}

/// El clic con avance `progress` (0–1). `unit` es lo que mide el ancho del video entero en el
/// cuadro (con el zoom). `None` si no se dibuja nada.
pub(crate) fn click_draw(kind: ClickKind, size: f64, progress: f64, unit: f64) -> Option<ClickDraw> {
    let size = normalized(size, 0.5, 2.5, 1.0);
    let p = normalized(progress, 0.0, 1.0, 1.0);
    let fall = (1.0 - p) * (1.0 - p);
    let draw = match kind {
        ClickKind::None => return None,
        ClickKind::Ripple => {
            let radius = (0.012 + 0.03 * p) * unit * size;
            ClickDraw::Disk { radius, solid: radius * 0.55, alpha: 0.32 * fall }
        }
        ClickKind::Ring => {
            let radius = (0.010 + 0.034 * p) * unit * size;
            ClickDraw::Ring { radius, thickness: (0.0028 * unit * size).max(1.5), alpha: 0.9 * fall }
        }
        ClickKind::Pulse => {
            let radius = (0.016 + 0.06 * p) * unit * size;
            ClickDraw::Disk { radius, solid: radius * 0.12, alpha: 0.5 * fall }
        }
    };
    let alpha = match draw {
        ClickDraw::Disk { alpha, .. } | ClickDraw::Ring { alpha, .. } => alpha,
    };
    (alpha > 0.0005).then_some(draw)
}

/// La sombra del video: opacidad, radio del difuminado y cuánto baja, en píxeles del cuadro de
/// alto `out_h`. `shadow` 0,5 es la de siempre; 0 no lleva.
pub(crate) fn shadow_params(shadow: f64, out_h: f64) -> Option<(f64, f64, f64)> {
    let shadow = normalized(shadow, 0.0, 1.0, 0.0);
    if shadow <= 0.003 {
        return None;
    }
    // Un poco más difuminada cuanto más fuerte.
    let spread = 0.75 + 0.5 * shadow;
    Some((shadow, out_h * 0.022 * spread, out_h * 0.012))
}

// --- Los tramos, el ritmo y el bitrate del exportado ---------------------------------

/// El factor del bitrate según la calidad (`Standard` es el de siempre).
pub(crate) fn quality_factor(quality: Quality) -> f64 {
    match quality {
        Quality::Low => 0.07,
        Quality::Standard => 0.12,
        Quality::High => 0.20,
        Quality::Max => 0.35,
    }
}

/// Bits por segundo del video: `píxeles × 30 × factor` (HEVC × 0,6), entre 2 y 60 Mbps.
pub(crate) fn bitrate(w: u32, h: u32, quality: Quality, codec: Codec) -> i64 {
    let codec_factor = if codec == Codec::Hevc { 0.6 } else { 1.0 };
    let bits = w as f64 * h as f64 * 30.0 * quality_factor(quality) * codec_factor;
    bits.round().clamp(2_000_000.0, 60_000_000.0) as i64
}

/// Un trozo del original que se exporta a una sola velocidad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Piece {
    /// Inicio y fin en segundos del original.
    pub(crate) a: f64,
    pub(crate) b: f64,
    pub(crate) rate: f64,
    /// Dónde empieza en el exportado.
    pub(crate) out0: f64,
}

/// Los trozos del exportado: cada tramo que queda, partido donde cambia la velocidad
/// (lo que no está en `speeds` va a 1×). Los trozos contiguos de igual velocidad se juntan.
pub(crate) fn pieces(segments: &[(f64, f64)], speeds: &[SpeedSpan]) -> Vec<Piece> {
    let rate_at = |t: f64| {
        speeds
            .iter()
            .find(|s| t >= s.start && t < s.end)
            .map(|s| s.rate)
            .filter(|r| r.is_finite() && *r > 0.0)
            .map_or(1.0, |r| r.clamp(0.05, 64.0))
    };
    let mut out: Vec<Piece> = Vec::new();
    for &(a, b) in segments.iter().filter(|(a, b)| b > a) {
        let mut cuts = vec![a, b];
        for s in speeds {
            for t in [s.start, s.end] {
                if t > a && t < b {
                    cuts.push(t);
                }
            }
        }
        cuts.sort_by(|x, y| x.total_cmp(y));
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            if q - p <= 1e-9 {
                continue;
            }
            let rate = rate_at((p + q) / 2.0);
            if let Some(last) = out.last_mut() {
                if last.b == p && last.rate == rate {
                    last.b = q;
                    continue;
                }
            }
            let out0 = out.last().map_or(0.0, |l| l.out0 + (l.b - l.a) / l.rate);
            out.push(Piece { a: p, b: q, rate, out0 });
        }
    }
    out
}

/// Lo que dura el exportado.
pub(crate) fn out_length(pieces: &[Piece]) -> f64 {
    pieces.last().map_or(0.0, |l| l.out0 + (l.b - l.a) / l.rate)
}

/// El tiempo en el exportado de `src` (segundos del original), si ese momento queda:
/// lo que duran los trozos anteriores más `(src - inicio del trozo) / velocidad`.
pub(crate) fn out_time(pieces: &[Piece], src: f64) -> Option<f64> {
    pieces.iter().find(|p| src >= p.a && src < p.b).map(|p| p.out0 + (src - p.a) / p.rate)
}

/// Si un cuadro en `at` se escribe: no puede quedar a menos de `0,9 / fps` del anterior
/// (con velocidades altas llegan más cuadros de los que caben).
pub(crate) fn keep_frame(last_out: f64, at: f64, fps: f64) -> bool {
    at >= last_out + 0.9 / fps.max(1.0)
}

/// La velocidad del trozo al que pertenece `src` (1 si no cae en ninguno).
pub(crate) fn rate_at(pieces: &[Piece], src: f64) -> f64 {
    pieces.iter().find(|p| src >= p.a && src < p.b).map_or(1.0, |p| p.rate)
}

/// Si el cuadro de `src`, que cae en `at` del exportado, se escribe.
///
/// Se descarta por dos razones: la salida pide menos cuadros por segundo que el
/// original (`fps` < `source_fps`, p. ej. una toma de 60 exportada a 30), o el tramo va
/// acelerado y llegan más cuadros de los que caben en su tiempo. A la misma cadencia
/// que la fuente no se descarta ninguno: la grabación es de cuadros variables y un
/// jitter de unos milisegundos no debe comerse ninguno.
pub(crate) fn keep_at(pieces: &[Piece], src: f64, last_out: f64, at: f64, fps: f64, source_fps: f64) -> bool {
    if fps + 0.5 < source_fps || rate_at(pieces, src) > 1.0 {
        keep_frame(last_out, at, fps)
    } else {
        at > last_out
    }
}
