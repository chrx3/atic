//! Pizarra: dibujar sobre la pantalla congelada.
//!
//! Lo que mejora respecto de la de Atic (un canvas de WebView):
//! - Cada movimiento no redibuja el pantallazo: la foto es una textura y solo
//!   se arman los trazos, a la frecuencia de la pantalla.
//! - Trazos suavizados (Chaikin) con puntas y uniones redondas.
//! - Borrador por trazo y «limpiar todo», que Atic no tiene.
//! - Shift: líneas a 45°, cuadrados y círculos.
//! - La exportación la compone Rust (`tiny-skia`) con las mismas primitivas
//!   que se ven en pantalla, a resolución física: sin base64 ni canvas.
//! - Texto (T): se escribe con el editor de Flip (`TextArea`) y queda como
//!   una forma más; las letras salen de los contornos de Segoe UI
//!   (`ttf-parser`), así que se ven igual en pantalla y en el PNG.

use std::time::{Duration, Instant};

use std::sync::{Arc, OnceLock};

use atic_capture::Frame;
use gpui::{
    actions, canvas, div, img, point, prelude::*, px, rgb, svg, App, ClickEvent, ClipboardItem, Context,
    CursorStyle, Entity, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PathStyle, Pixels, Point,
    RenderImage, SharedString, Subscription, Window,
};
use lyon::tessellation::{LineCap, LineJoin, StrokeOptions};

use crate::capture::{self, Frozen, Saved};
use crate::hover::{tip, HoverExt};
use crate::text_area::TextArea;

/// «Pizarra» en la tira y la rueda.
pub const TOOL: usize = 6;

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Tool {
    /// Tomar una figura: moverla, borrarla o duplicarla.
    Select,
    /// Recortar: lo de fuera del recuadro no sale al exportar.
    Crop,
    Pen,
    Highlight,
    Line,
    Arrow,
    /// Flecha curva: se dobla hacia donde pasa el arrastre.
    Curve,
    Rect,
    Ellipse,
    /// Pasos numerados: cada clic pone el siguiente número.
    Step,
    /// Tapa lo que hay en el recuadro con bloques de su color promedio.
    Pixelate,
    /// Oscurece todo menos los recuadros marcados.
    Focus,
    /// Copia el texto que hay en el recuadro (OCR); no deja nada dibujado.
    Grab,
    Text,
    /// Globo de diálogo: se arrastra desde lo que señala hasta donde va el
    /// texto, que queda en una etiqueta con colita.
    Bubble,
    /// Lupa: un círculo que muestra al doble lo que tiene debajo.
    Magnify,
    /// Medir: una línea con su largo en píxeles de la foto.
    Ruler,
    /// Cuentagotas: toma el color de la foto (no dibuja).
    Picker,
    /// Sellos de emoji a color: cada clic pone el elegido.
    Emoji,
    Eraser,
}

pub(crate) const TOOLS: [(Tool, &str, &str); 20] = [
    (Tool::Select, "icons/mouse-pointer-2.svg", "Mover · V · Supr borra, Ctrl+D duplica"),
    (Tool::Crop, "icons/crop.svg", "Recortar · K · clic sin arrastrar lo quita"),
    (Tool::Pen, "icons/pencil.svg", "Lápiz · P"),
    (Tool::Highlight, "icons/highlighter.svg", "Resaltador · H"),
    (Tool::Line, "icons/line.svg", "Línea · L"),
    (Tool::Arrow, "icons/arrow-up-right.svg", "Flecha · A"),
    (Tool::Curve, "icons/arrow-curve.svg", "Flecha curva · C"),
    (Tool::Rect, "icons/square.svg", "Rectángulo · R"),
    (Tool::Ellipse, "icons/circle.svg", "Elipse · E"),
    (Tool::Step, "icons/step.svg", "Pasos 1, 2, 3 · N"),
    (Tool::Text, "icons/type.svg", "Texto · T"),
    (Tool::Bubble, "icons/bubble.svg", "Globo · U"),
    (Tool::Emoji, "icons/smile.svg", "Emoji · J"),
    (Tool::Pixelate, "icons/pixelate.svg", "Pixelar · B"),
    (Tool::Focus, "icons/focus.svg", "Foco · O"),
    (Tool::Magnify, "icons/magnify.svg", "Lupa · Z"),
    (Tool::Ruler, "icons/ruler.svg", "Medir · M"),
    (Tool::Grab, "icons/scan-text.svg", "Copiar texto · G"),
    (Tool::Picker, "icons/pipette.svg", "Cuentagotas · I"),
    (Tool::Eraser, "icons/eraser.svg", "Borrador · X"),
];

/// La letra de los textos.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Font {
    #[default]
    Sans,
    Mono,
    Hand,
}

impl Font {
    const ALL: [Font; 3] = [Font::Sans, Font::Mono, Font::Hand];

    /// La familia para el editor (GPUI) y los archivos para dibujarla.
    fn family(self) -> &'static str {
        match self {
            Font::Sans => "Segoe UI",
            Font::Mono => "Consolas",
            Font::Hand => "Segoe Print",
        }
    }

    fn files(self) -> &'static [&'static str] {
        match self {
            Font::Sans => &["seguisb.ttf", "segoeui.ttf", "arial.ttf"],
            Font::Mono => &["consolab.ttf", "consola.ttf", "cour.ttf"],
            Font::Hand => &["segoepr.ttf", "segoesc.ttf", "comic.ttf"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Font::Sans => "Normal",
            Font::Mono => "Mono",
            Font::Hand => "A mano",
        }
    }
}

/// Lo que cambia el aspecto de una figura además del color y el grosor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Style {
    /// Trazo punteado (líneas, flechas, figuras).
    #[serde(default)]
    pub dashed: bool,
    /// Una sombra suave debajo: se despega de la foto.
    #[serde(default)]
    pub shadow: bool,
    #[serde(default)]
    pub font: Font,
}

/// Cómo van rectángulos, elipses y textos: solo el borde (o la letra), llenos
/// del color, o con un relleno suave y el borde encima. En un texto, lleno es
/// una etiqueta: el fondo del color y la letra en blanco o negro.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Fill {
    #[default]
    Outline,
    Solid,
    Soft,
}

impl Fill {
    fn next(self) -> Self {
        match self {
            Fill::Outline => Fill::Solid,
            Fill::Solid => Fill::Soft,
            Fill::Soft => Fill::Outline,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Fill::Outline => "Relleno: solo borde · F",
            Fill::Solid => "Relleno: lleno · F",
            Fill::Soft => "Relleno: suave · F",
        }
    }
}

/// La transparencia del relleno suave.
const SOFT_ALPHA: f32 = 0.25;
/// El fondo suave de un texto: oscuro y translúcido, para que la letra de
/// color se lea sobre cualquier imagen (un tinte del mismo color no se leía).
const SOFT_TEXT_BG: (u32, f32) = (0x111111, 0.6);
/// El radio de las esquinas de los rectángulos.
const RECT_RADIUS: f32 = 6.0;
/// El radio de los pasos numerados según el grosor.
const STEP_RADII: [f32; 3] = [11.0, 14.0, 19.0];
/// Lo que se separa el fondo de un texto de sus letras.
const LABEL_PAD: (f32, f32) = (8.0, 3.0);
/// El lado de cada bloque al pixelar, en píxeles lógicos.
const PIXEL_BLOCK: f32 = 12.0;
/// Cuánto se oscurece lo que queda fuera del foco.
const FOCUS_DIM: f32 = 0.55;
/// Lo que dura un aviso («Texto copiado») en lugar de la ayuda.
const NOTICE_FOR: Duration = Duration::from_millis(2600);
/// Cuánto agranda la lupa.
const MAGNIFY_ZOOM: f32 = 2.0;
/// La sombra: cuánto se corre y qué tan oscura.
const SHADOW_OFFSET: (f32, f32) = (2.0, 3.0);
const SHADOW_ALPHA: f32 = 0.35;

/// El recuadro `(x0, y0, x1, y1)` de la pizarra recortado de la foto, en
/// BGRA, para el OCR. Si es bajo se agranda al doble: el OCR de Windows lee
/// mal las letras chicas. `None` si casi no tiene tamaño.
fn crop(photo: &Frame, (x0, y0, x1, y1): (f32, f32, f32, f32), scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let clamp = |v: f32, max: u32| (v * scale).round().clamp(0.0, max as f32) as u32;
    let (x0, y0) = (clamp(x0, photo.width()), clamp(y0, photo.height()));
    let (x1, y1) = (clamp(x1, photo.width()), clamp(y1, photo.height()));
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w < 8 || h < 8 {
        return None;
    }
    let zoom = if h < 120 { 2 } else { 1 };
    let stride = photo.width() as usize * 4;
    let mut out = Vec::with_capacity((w * h * 4 * zoom * zoom) as usize);
    for y in 0..h * zoom {
        let row = (y0 + y / zoom) as usize * stride;
        for x in 0..w * zoom {
            let i = row + (x0 + x / zoom) as usize * 4;
            out.extend_from_slice(&photo.bgra[i..i + 4]);
        }
    }
    Some((w * zoom, h * zoom, out))
}

/// Rojo por defecto, como Atic.
pub(crate) const COLORS: [u32; 7] = [
    0xE5484D, 0xF5A524, 0xFFD60A, 0x30A46C, 0x3E8BFF, 0xFFFFFF, 0x111111,
];
/// Grosor lógico; el resaltador va 4 veces más ancho.
const WIDTHS: [f32; 3] = [2.5, 4.5, 8.0];
/// El tamaño de la letra según el grosor elegido.
const TEXT_SIZES: [f32; 3] = [18.0, 26.0, 40.0];
/// El alto de cada línea de texto, en veces el tamaño de la letra.
const TEXT_LINE: f32 = 1.3;
/// Hasta dónde llega una línea antes de saltar, si la imagen deja.
const TEXT_WRAP_MAX: f32 = 720.0;
const TEXT_WRAP_MIN: f32 = 60.0;
const HIGHLIGHT_WIDTH: f32 = 4.0;
const HIGHLIGHT_ALPHA: f32 = 0.35;
/// Distancia mínima entre puntos del lápiz: un arrastre lento no mete cientos.
const MIN_STEP: f32 = 2.5;
/// El segundo Esc dentro de este rato descarta lo dibujado.
const CLOSE_ARM: Duration = Duration::from_millis(1600);

const BAR_H: f32 = 44.0;
/// Ancho aproximado de la barra, para centrarla bajo una captura.
const TOOLBAR_W: f32 = 1400.0;
const BUTTON: f32 = 30.0;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Shape {
    pub tool: Tool,
    pub color: u32,
    /// El grosor del trazo; en un texto, el tamaño de la letra.
    pub width: f32,
    /// En un texto: la esquina de arriba a la izquierda y, en el segundo, el
    /// borde derecho donde salta de línea.
    pub points: Vec<(f32, f32)>,
    /// Lo escrito, en un texto; el número, en un paso; vacío en lo demás.
    pub text: String,
    /// Rectángulo, elipse y texto: borde, lleno o suave.
    pub fill: Fill,
    /// Punteado, sombra y letra. Lo guardado antes de que existiera vale lo
    /// de siempre.
    #[serde(default)]
    pub style: Style,
}

/// Lo que se dibuja, igual en pantalla y en el PNG.
#[derive(Debug, PartialEq)]
pub enum Prim {
    Stroke {
        points: Vec<(f32, f32)>,
        width: f32,
        closed: bool,
    },
    /// Un relleno, con su propia transparencia (la del relleno suave).
    Fill(Vec<(f32, f32)>, f32),
    Dot((f32, f32), f32),
}

/// Blanco o negro, lo que se lee sobre `color`.
fn contrast(color: u32) -> u32 {
    let [_, r, g, b] = color.to_be_bytes();
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 160.0 {
        0x111111
    } else {
        0xFFFFFF
    }
}

/// Un rectángulo con las esquinas redondeadas, como polígono (cada esquina
/// en arcos cortos).
fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> Vec<(f32, f32)> {
    let r = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    let corners = [
        ((x1 - r, y0 + r), -std::f32::consts::FRAC_PI_2),
        ((x1 - r, y1 - r), 0.0),
        ((x0 + r, y1 - r), std::f32::consts::FRAC_PI_2),
        ((x0 + r, y0 + r), std::f32::consts::PI),
    ];
    let mut points = Vec::with_capacity(4 * 7);
    for (center, start) in corners {
        for i in 0..=6 {
            let t = start + i as f32 / 6.0 * std::f32::consts::FRAC_PI_2;
            points.push((center.0 + r * t.cos(), center.1 + r * t.sin()));
        }
    }
    points
}

/// El contorno de una figura, según su relleno: lleno es solo el relleno;
/// suave, el relleno transparente con el borde encima; borde, solo el borde.
fn filled(points: Vec<(f32, f32)>, width: f32, fill: Fill) -> Vec<Prim> {
    match fill {
        Fill::Solid => vec![Prim::Fill(points, 1.0)],
        Fill::Soft => vec![
            Prim::Fill(points.clone(), SOFT_ALPHA),
            Prim::Stroke { points, width, closed: true },
        ],
        Fill::Outline => vec![Prim::Stroke { points, width, closed: true }],
    }
}

/// El punto por donde pasa la flecha curva: el del arrastre más lejos de la
/// recta entre los extremos. Si el arrastre fue recto, se dobla igual un poco
/// (hacia la izquierda de la dirección), para que se note que es curva.
fn curve_through(points: &[(f32, f32)]) -> (f32, f32) {
    let (a, b) = (points[0], points[points.len() - 1]);
    let far = points
        .iter()
        .copied()
        .max_by(|p, q| segment_distance(*p, a, b).total_cmp(&segment_distance(*q, a, b)))
        .unwrap_or(a);
    if segment_distance(far, a, b) >= 6.0 {
        return far;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    (mid.0 + dy * 0.2, mid.1 - dx * 0.2)
}

/// La curva cuadrática que pasa por `a`, `m` (a la mitad) y `b`, en puntos.
fn quad_points(a: (f32, f32), m: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    // El punto de control que hace pasar la curva por `m` en t = 0,5.
    let c = (2.0 * m.0 - (a.0 + b.0) / 2.0, 2.0 * m.1 - (a.1 + b.1) / 2.0);
    (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0;
            let u = 1.0 - t;
            (
                u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
                u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
            )
        })
        .collect()
}

/// La punta de una flecha que llega a `tip` desde la dirección `angle`, y
/// dónde termina el asta (dentro de la punta, para no asomar por ella).
fn arrow_head(tip: (f32, f32), angle: f32, head: f32) -> (Prim, (f32, f32)) {
    let spread = 0.45;
    let wing = |side: f32| {
        (
            tip.0 - head * (angle + side * spread).cos(),
            tip.1 - head * (angle + side * spread).sin(),
        )
    };
    let shaft_end = (tip.0 - head * 0.6 * angle.cos(), tip.1 - head * 0.6 * angle.sin());
    (Prim::Fill(vec![tip, wing(1.0), wing(-1.0)], 1.0), shaft_end)
}

impl Shape {
    /// Lo que ocupa la figura, para el recuadro de la selección:
    /// (x0, y0, x1, y1).
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let first = self.points.first().copied().unwrap_or_default();
        match self.tool {
            Tool::Text => {
                let (x, y, w, h) = self.text_box();
                let (px, py) = if self.fill == Fill::Outline { (0.0, 0.0) } else { LABEL_PAD };
                (x - px, y - py, x + w + px, y + h + py)
            }
            Tool::Step => (first.0 - self.width, first.1 - self.width, first.0 + self.width, first.1 + self.width),
            Tool::Emoji => {
                let half = self.width / 2.0;
                (first.0 - half, first.1 - half, first.0 + half, first.1 + half)
            }
            Tool::Magnify => {
                let r = distance(first, self.points.last().copied().unwrap_or(first));
                (first.0 - r, first.1 - r, first.0 + r, first.1 + r)
            }
            Tool::Pixelate | Tool::Focus => self.corners(),
            _ => {
                let pad = self.stroke_width() / 2.0;
                let mut points = self.prims().into_iter().flat_map(|prim| match prim {
                    Prim::Stroke { points, .. } | Prim::Fill(points, _) => points,
                    Prim::Dot(c, r) => vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)],
                });
                let Some(p) = points.next() else {
                    return (first.0, first.1, first.0, first.1);
                };
                let (x0, y0, x1, y1) = points.fold((p.0, p.1, p.0, p.1), |(x0, y0, x1, y1), q| {
                    (x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1))
                });
                (x0 - pad, y0 - pad, x1 + pad, y1 + pad)
            }
        }
    }

    /// La misma figura corrida `(dx, dy)`.
    fn moved(&self, dx: f32, dy: f32) -> Shape {
        Shape { points: self.points.iter().map(|&(x, y)| (x + dx, y + dy)).collect(), ..self.clone() }
    }

    /// El recuadro entre el primer y el último punto: (x0, y0, x1, y1).
    fn corners(&self) -> (f32, f32, f32, f32) {
        let first = self.points.first().copied().unwrap_or_default();
        let last = self.points.last().copied().unwrap_or(first);
        (first.0.min(last.0), first.1.min(last.1), first.0.max(last.0), first.1.max(last.1))
    }

    fn alpha(&self) -> f32 {
        if self.tool == Tool::Highlight {
            HIGHLIGHT_ALPHA
        } else {
            1.0
        }
    }

    fn stroke_width(&self) -> f32 {
        if self.tool == Tool::Highlight {
            self.width * HIGHLIGHT_WIDTH
        } else {
            self.width
        }
    }

    pub fn prims(&self) -> Vec<Prim> {
        let width = self.stroke_width();
        let (Some(&first), Some(&last)) = (self.points.first(), self.points.last()) else {
            return Vec::new();
        };
        match self.tool {
            Tool::Pen | Tool::Highlight => {
                if self.points.len() == 1 || distance(first, last) < 0.5 && self.points.len() < 3 {
                    return vec![Prim::Dot(first, width / 2.0)];
                }
                vec![Prim::Stroke {
                    points: smooth(&self.points),
                    width,
                    closed: false,
                }]
            }
            Tool::Line => vec![Prim::Stroke {
                points: vec![first, last],
                width,
                closed: false,
            }],
            Tool::Arrow => {
                let angle = (last.1 - first.1).atan2(last.0 - first.0);
                let head = (width * 4.0).max(12.0).min(distance(first, last) * 0.6);
                let (tip, shaft_end) = arrow_head(last, angle, head);
                vec![
                    Prim::Stroke {
                        points: vec![first, shaft_end],
                        width,
                        closed: false,
                    },
                    tip,
                ]
            }
            Tool::Curve => {
                let curve = quad_points(first, curve_through(&self.points), last);
                let head = (width * 4.0).max(12.0).min(distance(first, last) * 0.6);
                // La punta sigue la curva: la dirección del último tramo que
                // queda fuera de ella.
                let from = curve
                    .iter()
                    .rev()
                    .find(|p| distance(**p, last) >= head)
                    .copied()
                    .unwrap_or(first);
                let angle = (last.1 - from.1).atan2(last.0 - from.0);
                let (tip, shaft_end) = arrow_head(last, angle, head);
                let mut shaft: Vec<_> = curve.into_iter().filter(|p| distance(*p, last) >= head * 0.6).collect();
                shaft.push(shaft_end);
                vec![
                    Prim::Stroke {
                        points: shaft,
                        width,
                        closed: false,
                    },
                    tip,
                ]
            }
            Tool::Rect => {
                let (x0, y0, x1, y1) = (
                    first.0.min(last.0),
                    first.1.min(last.1),
                    first.0.max(last.0),
                    first.1.max(last.1),
                );
                filled(rounded_rect(x0, y0, x1, y1, RECT_RADIUS), width, self.fill)
            }
            Tool::Ellipse => {
                let center = ((first.0 + last.0) / 2.0, (first.1 + last.1) / 2.0);
                let (rx, ry) = ((last.0 - first.0).abs() / 2.0, (last.1 - first.1).abs() / 2.0);
                let points = (0..72)
                    .map(|i| {
                        let t = i as f32 / 72.0 * std::f32::consts::TAU;
                        (center.0 + rx * t.cos(), center.1 + ry * t.sin())
                    })
                    .collect();
                filled(points, width, self.fill)
            }
            // El círculo del paso; el número lo pone `draw_step`.
            Tool::Step => vec![Prim::Dot(first, self.width)],
            // Mientras se arrastra se ve el recuadro; el efecto lo hacen
            // `draw_pixelate` y `draw_focus` sobre la foto.
            // Mientras se arrastra: el círculo de la lupa (el aumento lo pone
            // `draw_magnify`) y la línea del globo hacia lo que señala.
            Tool::Magnify => {
                let r = distance(first, last);
                let ring = (0..48)
                    .map(|i| {
                        let t = i as f32 / 48.0 * std::f32::consts::TAU;
                        (first.0 + r * t.cos(), first.1 + r * t.sin())
                    })
                    .collect();
                vec![Prim::Stroke { points: ring, width: width.max(2.0), closed: true }]
            }
            Tool::Bubble => vec![Prim::Stroke { points: vec![first, last], width: 2.0, closed: false }],
            // Medir: la línea con una rayita en cada punta; el largo lo pone
            // `draw_ruler_label`.
            Tool::Ruler => {
                let (dx, dy) = (last.0 - first.0, last.1 - first.1);
                let len = dx.hypot(dy).max(1.0);
                let tick = (width * 2.5).max(7.0);
                let (nx, ny) = (-dy / len * tick, dx / len * tick);
                let end = |p: (f32, f32)| Prim::Stroke {
                    points: vec![(p.0 + nx, p.1 + ny), (p.0 - nx, p.1 - ny)],
                    width,
                    closed: false,
                };
                vec![Prim::Stroke { points: vec![first, last], width, closed: false }, end(first), end(last)]
            }
            // El cuentagotas y mover no dibujan; el emoji lo pinta `draw_emoji`.
            Tool::Picker | Tool::Emoji | Tool::Select => Vec::new(),
            // Mientras se arrastra el recorte se ve su recuadro.
            Tool::Crop => {
                let (x0, y0, x1, y1) = self.corners();
                vec![Prim::Stroke { points: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)], width: 1.5, closed: true }]
            }
            Tool::Pixelate | Tool::Focus | Tool::Grab => {
                let (x0, y0, x1, y1) = self.corners();
                vec![Prim::Stroke {
                    points: rounded_rect(x0, y0, x1, y1, RECT_RADIUS),
                    width: 1.5,
                    closed: true,
                }]
            }
            // El texto no son trazos: lo dibuja `draw_text`.
            Tool::Text | Tool::Eraser => Vec::new(),
        }
    }

    /// El trazo pasa a menos de `slack` del punto; en un texto, el punto cae
    /// en su caja; en algo relleno, dentro de él.
    fn hit(&self, p: (f32, f32), slack: f32) -> bool {
        if self.tool == Tool::Text {
            let (x, y, w, h) = self.text_box();
            return p.0 >= x - slack && p.0 <= x + w + slack && p.1 >= y - slack && p.1 <= y + h + slack;
        }
        if matches!(self.tool, Tool::Pixelate | Tool::Focus) {
            let (x0, y0, x1, y1) = self.corners();
            return p.0 >= x0 - slack && p.0 <= x1 + slack && p.1 >= y0 - slack && p.1 <= y1 + slack;
        }
        if self.tool == Tool::Magnify {
            let (Some(&center), Some(&edge)) = (self.points.first(), self.points.last()) else {
                return false;
            };
            return distance(p, center) <= distance(center, edge) + slack;
        }
        if self.tool == Tool::Emoji {
            let Some(&center) = self.points.first() else {
                return false;
            };
            let half = self.width / 2.0 + slack;
            return (p.0 - center.0).abs() <= half && (p.1 - center.1).abs() <= half;
        }
        self.prims().iter().any(|prim| match prim {
            Prim::Dot(center, r) => distance(*center, p) <= r + slack,
            Prim::Fill(points, _) => inside(p, points) || points.iter().any(|q| distance(*q, p) <= slack * 2.0),
            Prim::Stroke {
                points,
                width,
                closed,
            } => {
                let reach = width / 2.0 + slack;
                let mut segments: Vec<((f32, f32), (f32, f32))> =
                    points.windows(2).map(|w| (w[0], w[1])).collect();
                if *closed {
                    if let (Some(a), Some(b)) = (points.last(), points.first()) {
                        segments.push((*a, *b));
                    }
                }
                segments.iter().any(|(a, b)| segment_distance(p, *a, *b) <= reach)
            }
        })
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    if len == 0.0 {
        return distance(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len).clamp(0.0, 1.0);
    distance(p, (a.0 + t * dx, a.1 + t * dy))
}

/// `p` cae dentro del polígono (regla par-impar).
fn inside(p: (f32, f32), polygon: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// --- Texto ------------------------------------------------------------------

/// La letra `kind` (Segoe UI seminegrita, Consolas o Segoe Print, o la que
/// haya parecida), la misma del editor. Se lee una vez y vive lo que el proceso.
fn font_of(kind: Font) -> Option<&'static ttf_parser::Face<'static>> {
    static FACES: [OnceLock<Option<ttf_parser::Face<'static>>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let index = Font::ALL.iter().position(|f| *f == kind).unwrap_or(0);
    FACES[index]
        .get_or_init(|| {
            let fonts = std::env::var_os("WINDIR").map(|dir| std::path::PathBuf::from(dir).join("Fonts"))?;
            kind.files().iter().find_map(|name| {
                let data: &'static [u8] = Box::leak(std::fs::read(fonts.join(name)).ok()?.into_boxed_slice());
                ttf_parser::Face::parse(data, 0).ok()
            })
        })
        .as_ref()
        // Si falta, la normal.
        .or_else(|| (kind != Font::Sans).then(|| font_of(Font::Sans)).flatten())
}

fn font() -> Option<&'static ttf_parser::Face<'static>> {
    font_of(Font::Sans)
}

/// El ancho de `text` a `size` en la letra normal.
fn text_width(text: &str, size: f32) -> f32 {
    text_width_in(text, size, Font::Sans)
}

/// El ancho de `text` a `size` en la letra `kind`. Sin la fuente, una
/// aproximación.
fn text_width_in(text: &str, size: f32, kind: Font) -> f32 {
    let Some(face) = font_of(kind) else {
        return text.chars().count() as f32 * size * 0.55;
    };
    let em = size / face.units_per_em() as f32;
    text.chars()
        .map(|ch| {
            face.glyph_index(ch)
                .and_then(|id| face.glyph_hor_advance(id))
                .map_or(size * 0.5, |advance| advance as f32 * em)
        })
        .sum()
}

/// Las líneas como se ven: cada salto escrito y, si una no cabe en `wrap`,
/// salta en el último espacio (o corta la palabra si es más larga).
fn wrap_lines(body: &str, wrap: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in body.split('\n') {
        let mut line = String::new();
        for word in raw.split_inclusive(' ') {
            let candidate = format!("{line}{word}");
            if !line.is_empty() && measure(candidate.trim_end()) > wrap {
                lines.push(line.trim_end().to_string());
                line = word.to_string();
            } else {
                line = candidate;
            }
            // Una palabra sola más larga que la línea: se corta a la fuerza.
            while measure(line.trim_end()) > wrap && line.trim_end().chars().count() > 1 {
                let mut head = String::new();
                for ch in line.chars() {
                    if !head.is_empty() && measure(&format!("{head}{ch}")) > wrap {
                        break;
                    }
                    head.push(ch);
                }
                let rest = line[head.len()..].to_string();
                lines.push(head);
                line = rest;
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

impl Shape {
    fn text_lines(&self) -> Vec<String> {
        let wrap = self.points.get(1).map_or(TEXT_WRAP_MAX, |end| end.0 - self.points[0].0);
        wrap_lines(&self.text, wrap, |s| text_width_in(s, self.width, self.style.font))
    }

    /// La caja que ocupa un texto: (x, y, ancho, alto).
    fn text_box(&self) -> (f32, f32, f32, f32) {
        let (x, y) = self.points.first().copied().unwrap_or_default();
        let lines = self.text_lines();
        let w = lines.iter().map(|line| text_width_in(line, self.width, self.style.font)).fold(0.0, f32::max);
        (x, y, w, lines.len() as f32 * self.width * TEXT_LINE)
    }
}

/// Lleva los contornos de una letra a un camino de `tiny-skia`: de las
/// unidades de la fuente (y hacia arriba) a la imagen (y hacia abajo).
struct Outline<'a> {
    path: &'a mut tiny_skia::PathBuilder,
    x: f32,
    baseline: f32,
    em: f32,
}

impl Outline<'_> {
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x * self.em, self.baseline - y * self.em)
    }
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let ((x1, y1), (x, y)) = (self.at(x1, y1), self.at(x, y));
        self.path.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let ((x1, y1), (x2, y2), (x, y)) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

/// Las letras de un texto, rellenas. Cada línea centra la letra en su alto,
/// como el editor de GPUI, para que al terminar no se corra.
fn draw_text(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, paint: &tiny_skia::Paint, transform: tiny_skia::Transform) {
    let Some(face) = font_of(shape.style.font) else {
        return;
    };
    let size = shape.width;
    let (ascent, descent) = metrics(face, size);
    let line_h = size * TEXT_LINE;
    let (x0, y0) = shape.points.first().copied().unwrap_or_default();
    let mut path = tiny_skia::PathBuilder::new();
    for (row, line) in shape.text_lines().iter().enumerate() {
        let top = y0 + row as f32 * line_h;
        let baseline = top + (line_h - (ascent + descent)) / 2.0 + ascent;
        glyph_line(&mut path, face, line, x0, baseline, size);
    }
    if let Some(path) = path.finish() {
        pixmap.fill_path(&path, paint, tiny_skia::FillRule::Winding, transform, None);
    }
}

/// Lo que sube y baja la letra de la línea base, a `size`.
fn metrics(face: &ttf_parser::Face, size: f32) -> (f32, f32) {
    let em = size / face.units_per_em() as f32;
    (face.ascender() as f32 * em, -face.descender() as f32 * em)
}

/// Los contornos de una línea, desde `x` sobre `baseline`.
fn glyph_line(path: &mut tiny_skia::PathBuilder, face: &ttf_parser::Face, line: &str, mut x: f32, baseline: f32, size: f32) {
    let em = size / face.units_per_em() as f32;
    for ch in line.chars() {
        let Some(id) = face.glyph_index(ch) else {
            x += size * 0.5;
            continue;
        };
        face.outline_glyph(id, &mut Outline { path, x, baseline, em });
        x += face.glyph_hor_advance(id).unwrap_or(0) as f32 * em;
    }
}

/// El fondo de un texto lleno o suave: una etiqueta redondeada alrededor.
fn label_box(shape: &Shape) -> Vec<(f32, f32)> {
    let (x, y, w, h) = shape.text_box();
    let (px, py) = LABEL_PAD;
    rounded_rect(x - px, y - py, x + w + px, y + h + py, (shape.width * 0.45).max(6.0))
}

/// El número de un paso, centrado en su círculo, en blanco o negro.
fn draw_step(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, transform: tiny_skia::Transform) {
    let Some(face) = font() else {
        return;
    };
    let Some(&center) = shape.points.first() else {
        return;
    };
    let size = shape.width * 1.15;
    let (ascent, descent) = metrics(face, size);
    // Las cifras no bajan de la línea base: se centra lo que sube.
    let digits = face.capital_height().map_or(ascent - descent, |h| h as f32 * size / face.units_per_em() as f32);
    let x = center.0 - text_width(&shape.text, size) / 2.0;
    let baseline = center.1 + digits / 2.0;
    let mut path = tiny_skia::PathBuilder::new();
    glyph_line(&mut path, face, &shape.text, x, baseline, size);
    let [_, r, g, b] = contrast(shape.color).to_be_bytes();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    if let Some(path) = path.finish() {
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
    }
}

/// Chaikin, dos pasadas: redondea las esquinas que deja el muestreo del mouse
/// sin correr los extremos.
pub fn smooth(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut current = points.to_vec();
    for _ in 0..2 {
        if current.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(current.len() * 2);
        next.push(current[0]);
        for pair in current.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            next.push((a.0 * 0.75 + b.0 * 0.25, a.1 * 0.75 + b.1 * 0.25));
            next.push((a.0 * 0.25 + b.0 * 0.75, a.1 * 0.25 + b.1 * 0.75));
        }
        next.push(*current.last().unwrap());
        current = next;
    }
    current
}

/// Shift: la flecha a múltiplos de 45°, rectángulo y elipse parejos, el lápiz
/// recto.
fn constrain(tool: Tool, from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    match tool {
        Tool::Rect | Tool::Ellipse | Tool::Pixelate | Tool::Focus | Tool::Grab | Tool::Crop => {
            let side = dx.abs().max(dy.abs());
            (from.0 + side * dx.signum(), from.1 + side * dy.signum())
        }
        _ => {
            let step = std::f32::consts::FRAC_PI_4;
            let angle = (dy.atan2(dx) / step).round() * step;
            let len = dx.hypot(dy);
            (from.0 + len * angle.cos(), from.1 + len * angle.sin())
        }
    }
}

fn hsla(color: u32, alpha: f32) -> Hsla {
    let mut c: Hsla = rgb(color).into();
    c.a = alpha;
    c
}

fn gpoint((x, y): (f32, f32), offset: (f32, f32)) -> Point<Pixels> {
    point(px(x + offset.0), px(y + offset.1))
}

/// Un recuadro fino con borde oscuro (se ve sobre cualquier foto).
fn outline(window: &mut Window, (x0, y0, x1, y1): (f32, f32, f32, f32), offset: (f32, f32), color: Hsla) {
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|p| gpoint(p, offset));
    for (width, color) in [(3.0, gpui::black().opacity(0.5)), (1.5, color)] {
        let mut builder = PathBuilder::stroke(px(width));
        builder.add_polygon(&corners, true);
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

/// Pinta una forma con los trazos de GPUI.
fn paint_shape(shape: &Shape, offset: (f32, f32), window: &mut Window) {
    let color = hsla(shape.color, shape.alpha());
    for prim in shape.prims() {
        let path = match prim {
            Prim::Stroke {
                points,
                width,
                closed,
            } => {
                let options = StrokeOptions::default()
                    .with_line_width(width)
                    .with_line_cap(LineCap::Round)
                    .with_line_join(LineJoin::Round);
                let mut builder =
                    PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(options));
                let gpoints: Vec<_> = points.into_iter().map(|p| gpoint(p, offset)).collect();
                builder.add_polygon(&gpoints, closed);
                builder.build()
            }
            Prim::Fill(points, alpha) => {
                let mut builder = PathBuilder::fill();
                let gpoints: Vec<_> = points.into_iter().map(|p| gpoint(p, offset)).collect();
                builder.add_polygon(&gpoints, true);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color.opacity(alpha));
                }
                continue;
            }
            Prim::Dot(center, r) => {
                let mut builder = PathBuilder::fill();
                let ring: Vec<_> = (0..24)
                    .map(|i| {
                        let t = i as f32 / 24.0 * std::f32::consts::TAU;
                        gpoint((center.0 + r * t.cos(), center.1 + r * t.sin()), offset)
                    })
                    .collect();
                builder.add_polygon(&ring, true);
                builder.build()
            }
        };
        if let Ok(path) = path {
            window.paint_path(path, color);
        }
    }
}

/// Tapa el recuadro con bloques del color promedio de la foto debajo. Usa la
/// foto, no lo pintado: así lo dibujado encima del pixelado sigue nítido.
fn draw_pixelate(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, scale: f32, photo: &Frame) {
    let (w, h) = (photo.width().min(pixmap.width()), photo.height().min(pixmap.height()));
    let (x0, y0, x1, y1) = shape.corners();
    let clamp = |v: f32, max: u32| (v * scale).round().clamp(0.0, max as f32) as u32;
    let (x0, y0, x1, y1) = (clamp(x0, w), clamp(y0, h), clamp(x1, w), clamp(y1, h));
    let block = (PIXEL_BLOCK * scale).round().max(4.0) as u32;
    let stride = photo.width() as usize * 4;
    let mut by = y0;
    while by < y1 {
        let bh = block.min(y1 - by);
        let mut bx = x0;
        while bx < x1 {
            let bw = block.min(x1 - bx);
            let mut sum = [0u64; 3];
            for y in by..by + bh {
                let row = y as usize * stride;
                for x in bx..bx + bw {
                    let i = row + x as usize * 4;
                    sum[0] += photo.bgra[i + 2] as u64;
                    sum[1] += photo.bgra[i + 1] as u64;
                    sum[2] += photo.bgra[i] as u64;
                }
            }
            let n = (bw * bh) as u64;
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 255);
            if let Some(rect) = tiny_skia::Rect::from_xywh(bx as f32, by as f32, bw as f32, bh as f32) {
                pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
            }
            bx += bw;
        }
        by += bh;
    }
}

/// Oscurece toda la imagen menos los recuadros de foco (todos a la vez: uno
/// solo no apaga a los otros).
fn draw_focus(pixmap: &mut tiny_skia::Pixmap, shapes: &[Shape], scale: f32) {
    let spots: Vec<_> = shapes.iter().filter(|shape| shape.tool == Tool::Focus).collect();
    if spots.is_empty() {
        return;
    }
    let (w, h) = (pixmap.width() as f32 / scale, pixmap.height() as f32 / scale);
    let mut path = tiny_skia::PathBuilder::new();
    if let Some(all) = tiny_skia::Rect::from_xywh(0.0, 0.0, w, h) {
        path.push_rect(all);
    }
    for spot in spots {
        let (x0, y0, x1, y1) = spot.corners();
        let ring = rounded_rect(x0, y0, x1, y1, RECT_RADIUS * 2.0);
        path.move_to(ring[0].0, ring[0].1);
        for p in &ring[1..] {
            path.line_to(p.0, p.1);
        }
        path.close();
    }
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0, 0, 0, (FOCUS_DIM * 255.0).round() as u8);
    paint.anti_alias = true;
    if let Some(path) = path.finish() {
        // Par-impar: lo de dentro de cada recuadro queda sin oscurecer.
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::EvenOdd, tiny_skia::Transform::from_scale(scale, scale), None);
    }
}

/// Muestra al doble lo que hay bajo el círculo de la lupa (de la foto, sin
/// lo dibujado) y le pone un aro del color.
fn draw_magnify(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, scale: f32, photo: &Frame, transform: tiny_skia::Transform) {
    let (Some(&center), Some(&edge)) = (shape.points.first(), shape.points.last()) else {
        return;
    };
    let r = distance(center, edge);
    if r < 4.0 {
        return;
    }
    let (cx, cy, rr) = (center.0 * scale, center.1 * scale, r * scale);
    let (pw, ph) = (pixmap.width().min(photo.width()) as i64, pixmap.height().min(photo.height()) as i64);
    let stride = photo.width() as usize * 4;
    let width = pixmap.width() as usize;
    let pixels = pixmap.pixels_mut();
    let (y0, y1) = (((cy - rr).floor() as i64).max(0), ((cy + rr).ceil() as i64).min(ph - 1));
    let (x0, x1) = (((cx - rr).floor() as i64).max(0), ((cx + rr).ceil() as i64).min(pw - 1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if dx * dx + dy * dy > rr * rr {
                continue;
            }
            let sx = ((cx + dx / MAGNIFY_ZOOM).round() as i64).clamp(0, pw - 1) as usize;
            let sy = ((cy + dy / MAGNIFY_ZOOM).round() as i64).clamp(0, ph - 1) as usize;
            let i = sy * stride + sx * 4;
            let (b, g, r) = (photo.bgra[i], photo.bgra[i + 1], photo.bgra[i + 2]);
            if let Some(color) = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, 255) {
                pixels[y as usize * width + x as usize] = color;
            }
        }
    }
    // El aro, del color y del grosor elegidos.
    let mut paint = tiny_skia::Paint::default();
    let [_, r8, g8, b8] = shape.color.to_be_bytes();
    paint.set_color_rgba8(r8, g8, b8, 255);
    paint.anti_alias = true;
    if let Some(path) = tiny_skia::PathBuilder::from_circle(center.0, center.1, r) {
        let stroke = tiny_skia::Stroke { width: shape.width.max(2.0), ..tiny_skia::Stroke::default() };
        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

/// El largo de una medida, en píxeles de la foto: «245 px».
fn ruler_label(from: (f32, f32), to: (f32, f32), scale: f32) -> String {
    format!("{} px", (distance(from, to) * scale).round() as i64)
}

/// La etiqueta de una medida: oscura, con el largo en blanco, a un costado
/// de la mitad de la línea.
fn draw_ruler_label(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, transform: tiny_skia::Transform) {
    let (Some(face), Some(&a), Some(&b)) = (font(), shape.points.first(), shape.points.last()) else {
        return;
    };
    if shape.text.is_empty() {
        return;
    }
    let size = 13.0;
    let (ascent, descent) = metrics(face, size);
    let w = text_width(&shape.text, size);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).max(1.0);
    // Hacia arriba de la línea (o a la izquierda si es vertical).
    let (nx, ny) = if dy.abs() > dx.abs() { (-1.0, 0.0) } else { (dy / len * dx.signum(), -dx.abs() / len) };
    let mid = ((a.0 + b.0) / 2.0 + nx * 16.0, (a.1 + b.1) / 2.0 + ny * 16.0);
    let (bw, bh) = (w + 12.0, ascent + descent + 6.0);
    let (x0, y0) = (mid.0 - bw / 2.0, mid.1 - bh / 2.0);
    let mut bg = tiny_skia::Paint::default();
    bg.set_color_rgba8(17, 17, 17, 220);
    bg.anti_alias = true;
    let ring = rounded_rect(x0, y0, x0 + bw, y0 + bh, bh / 2.0);
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(ring[0].0, ring[0].1);
    for p in &ring[1..] {
        path.line_to(p.0, p.1);
    }
    path.close();
    if let Some(path) = path.finish() {
        pixmap.fill_path(&path, &bg, tiny_skia::FillRule::Winding, transform, None);
    }
    let mut glyphs = tiny_skia::PathBuilder::new();
    glyph_line(&mut glyphs, face, &shape.text, x0 + 6.0, y0 + 3.0 + ascent, size);
    let mut ink = tiny_skia::Paint::default();
    ink.set_color_rgba8(255, 255, 255, 255);
    ink.anti_alias = true;
    if let Some(path) = glyphs.finish() {
        pixmap.fill_path(&path, &ink, tiny_skia::FillRule::Winding, transform, None);
    }
}

/// La colita de un globo: un triángulo del color del fondo que sale del
/// centro de la etiqueta hacia lo que señala.
fn bubble_tail(shape: &Shape) -> Option<Vec<(f32, f32)>> {
    let &tip = shape.points.get(2)?;
    let (x, y, w, h) = shape.text_box();
    let center = (x + w / 2.0, y + h / 2.0);
    let (dx, dy) = (tip.0 - center.0, tip.1 - center.1);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return None;
    }
    let half = (shape.width * 0.4).clamp(6.0, 14.0);
    let (px, py) = (-dy / len * half, dx / len * half);
    Some(vec![(center.0 + px, center.1 + py), tip, (center.0 - px, center.1 - py)])
}

fn polyline(points: &[(f32, f32)], close: bool) -> Option<tiny_skia::Path> {
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(points.first()?.0, points[0].1);
    for p in &points[1..] {
        builder.line_to(p.0, p.1);
    }
    if close {
        builder.close();
    }
    builder.finish()
}

/// Una figura. Con `tint`, toda de ese color y transparencia (la sombra).
fn draw_one(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, transform: tiny_skia::Transform, tint: Option<(u32, f32)>) {
    use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Stroke, StrokeDash};
    let paint_of = |color: u32, alpha: f32| {
        let (color, alpha) = tint.map_or((color, alpha), |(c, a)| (c, a * alpha));
        let [_, r, g, b] = color.to_be_bytes();
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, (alpha * 255.0).round() as u8);
        paint.anti_alias = true;
        paint
    };
    let paint = paint_of(shape.color, shape.alpha());
    if shape.tool == Tool::Text {
        // Con fondo: la etiqueta (y la colita, si es globo) primero; llena, la
        // letra en blanco o negro.
        if shape.fill != Fill::Outline {
            let (color, alpha) = if shape.fill == Fill::Solid { (shape.color, 1.0) } else { SOFT_TEXT_BG };
            let bg = paint_of(color, alpha);
            if let Some(path) = polyline(&label_box(shape), true) {
                pixmap.fill_path(&path, &bg, FillRule::Winding, transform, None);
            }
            if let Some(path) = bubble_tail(shape).and_then(|tail| polyline(&tail, true)) {
                pixmap.fill_path(&path, &bg, FillRule::Winding, transform, None);
            }
        }
        let ink = if shape.fill == Fill::Solid { paint_of(contrast(shape.color), 1.0) } else { paint.clone() };
        draw_text(pixmap, shape, &ink, transform);
        return;
    }
    for prim in shape.prims() {
        match prim {
            Prim::Stroke { points, width, closed } => {
                if let Some(path) = polyline(&points, closed) {
                    let dash = shape.style.dashed.then(|| StrokeDash::new(vec![width * 2.6, width * 2.0], 0.0)).flatten();
                    let stroke = Stroke {
                        width,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                        dash,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
            Prim::Fill(points, alpha) => {
                if let Some(path) = polyline(&points, true) {
                    let fill = paint_of(shape.color, shape.alpha() * alpha);
                    pixmap.fill_path(&path, &fill, FillRule::Winding, transform, None);
                }
            }
            Prim::Dot(center, r) => {
                if let Some(path) = PathBuilder::from_circle(center.0, center.1, r) {
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                }
            }
        }
    }
    if tint.is_none() {
        match shape.tool {
            Tool::Step => draw_step(pixmap, shape, transform),
            Tool::Ruler => draw_ruler_label(pixmap, shape, transform),
            _ => {}
        }
    }
}

/// Las formas con `tiny-skia`, a resolución física. Primero lo que cambia la
/// foto (pixelar, foco, lupa) y encima las anotaciones, que así siempre se
/// leen.
fn draw_shapes(pixmap: &mut tiny_skia::Pixmap, shapes: &[Shape], scale: f32, photo: &Frame) {
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    for shape in shapes.iter().filter(|shape| shape.tool == Tool::Pixelate) {
        draw_pixelate(pixmap, shape, scale, photo);
    }
    draw_focus(pixmap, shapes, scale);
    for shape in shapes.iter().filter(|shape| shape.tool == Tool::Magnify) {
        draw_magnify(pixmap, shape, scale, photo, transform);
    }
    for shape in shapes {
        if matches!(shape.tool, Tool::Pixelate | Tool::Focus | Tool::Grab | Tool::Magnify) {
            continue;
        }
        if shape.tool == Tool::Emoji {
            draw_emoji(pixmap, shape, scale);
            continue;
        }
        // La sombra: la misma figura en negro translúcido, un poco más abajo.
        if shape.style.shadow {
            let (dx, dy) = SHADOW_OFFSET;
            draw_one(pixmap, shape, transform.pre_translate(dx, dy), Some((0x000000, SHADOW_ALPHA)));
        }
        draw_one(pixmap, shape, transform, None);
    }
}

/// La foto con los trazos encima, a resolución física.
pub fn compose(frame: &Frame, shapes: &[Shape], scale: f32) -> Result<Frame, String> {
    let (w, h) = (frame.width(), frame.height());
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("tamaño inválido")?;
    for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(frame.bgra.chunks_exact(4)) {
        dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
    }
    draw_shapes(&mut pixmap, shapes, scale, frame);
    // La foto es opaca: premultiplicado y directo coinciden.
    let mut bgra = pixmap.take();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Ok(Frame::new(frame.bounds, bgra))
}

/// Los trazos terminados en una capa transparente, para pintarla como una
/// imagen más. Redibujarlos todos en cada cuadro como trazos de GPUI hacía
/// que la pizarra se arrastrara a medida que crecía el dibujo.
fn rasterize(shapes: &[Shape], photo: &Frame, scale: f32) -> Option<Arc<RenderImage>> {
    if shapes.is_empty() {
        return None;
    }
    let (width, height) = (photo.width(), photo.height());
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    draw_shapes(&mut pixmap, shapes, scale, photo);
    // GPUI espera BGRA con alfa directo; tiny-skia lo da premultiplicado.
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        bgra.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

/// Una imagen ya tomada lista para la pizarra (sin ventanas ni tiempos).
pub fn frozen_for(frame: Frame, scale: f32, offset: (f32, f32)) -> Option<Frozen> {
    let buffer = image::RgbaImage::from_raw(frame.width(), frame.height(), frame.bgra.clone())?;
    Some(Frozen {
        image: Arc::new(RenderImage::new([image::Frame::new(buffer)])),
        frame,
        scale,
        windows: Vec::new(),
        took: Duration::ZERO,
        offset,
    })
}

// --- La vista ---------------------------------------------------------------

actions!(
    board,
    [
        Close, Done, Undo, Redo, Clear, PickPen, PickHighlight, PickLine, PickArrow, PickCurve,
        PickRect, PickEllipse, PickStep, PickPixelate, PickFocus, PickGrab, PickText, PickEraser,
        PickBubble, PickMagnify, PickRuler, PickPicker, PickEmoji, ToggleDashed, ToggleShadow,
        PickSelect, PickCrop, Duplicate, NudgeLeft, NudgeRight, NudgeUp, NudgeDown, ZoomIn, ZoomOut,
        ZoomReset,
        EndText, CycleFill, HideData
    ]
);

const KEY_CONTEXT: &str = "Board";
/// Escribiendo un texto: las letras son del editor, no atajos de la barra.
const TYPING_CONTEXT: &str = "BoardText";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Close, context),
        KeyBinding::new("enter", Done, context),
        KeyBinding::new("ctrl-c", Done, context),
        KeyBinding::new("ctrl-s", Done, context),
        KeyBinding::new("ctrl-z", Undo, context),
        KeyBinding::new("ctrl-y", Redo, context),
        KeyBinding::new("ctrl-shift-z", Redo, context),
        KeyBinding::new("ctrl-backspace", Clear, context),
        KeyBinding::new("delete", Clear, context),
        // En el orden de la barra: letra y número.
        KeyBinding::new("p", PickPen, context),
        KeyBinding::new("1", PickPen, context),
        KeyBinding::new("h", PickHighlight, context),
        KeyBinding::new("2", PickHighlight, context),
        KeyBinding::new("l", PickLine, context),
        KeyBinding::new("3", PickLine, context),
        KeyBinding::new("a", PickArrow, context),
        KeyBinding::new("4", PickArrow, context),
        KeyBinding::new("c", PickCurve, context),
        KeyBinding::new("5", PickCurve, context),
        KeyBinding::new("r", PickRect, context),
        KeyBinding::new("6", PickRect, context),
        KeyBinding::new("e", PickEllipse, context),
        KeyBinding::new("7", PickEllipse, context),
        KeyBinding::new("n", PickStep, context),
        KeyBinding::new("8", PickStep, context),
        // Sin número: ya no quedan.
        KeyBinding::new("b", PickPixelate, context),
        KeyBinding::new("o", PickFocus, context),
        KeyBinding::new("g", PickGrab, context),
        KeyBinding::new("u", PickBubble, context),
        KeyBinding::new("z", PickMagnify, context),
        KeyBinding::new("m", PickRuler, context),
        KeyBinding::new("i", PickPicker, context),
        KeyBinding::new("j", PickEmoji, context),
        KeyBinding::new("v", PickSelect, context),
        KeyBinding::new("k", PickCrop, context),
        KeyBinding::new("ctrl-d", Duplicate, context),
        KeyBinding::new("ctrl-=", ZoomIn, context),
        KeyBinding::new("ctrl-+", ZoomIn, context),
        KeyBinding::new("ctrl--", ZoomOut, context),
        KeyBinding::new("ctrl-0", ZoomReset, context),
        KeyBinding::new("left", NudgeLeft, context),
        KeyBinding::new("right", NudgeRight, context),
        KeyBinding::new("up", NudgeUp, context),
        KeyBinding::new("down", NudgeDown, context),
        KeyBinding::new("shift-left", NudgeLeft, context),
        KeyBinding::new("shift-right", NudgeRight, context),
        KeyBinding::new("shift-up", NudgeUp, context),
        KeyBinding::new("shift-down", NudgeDown, context),
        KeyBinding::new("d", ToggleDashed, context),
        KeyBinding::new("s", ToggleShadow, context),
        KeyBinding::new("ctrl-h", HideData, context),
        KeyBinding::new("t", PickText, context),
        KeyBinding::new("9", PickText, context),
        KeyBinding::new("x", PickEraser, context),
        KeyBinding::new("0", PickEraser, context),
        KeyBinding::new("f", CycleFill, context),
        // Enter es salto de línea (`TextArea`); Esc o Ctrl+Enter terminan.
        KeyBinding::new("escape", EndText, Some(TYPING_CONTEXT)),
        KeyBinding::new("ctrl-enter", EndText, Some(TYPING_CONTEXT)),
    ]);
}

/// Dónde va la imagen tras un zoom de `k` (cuánto crece lo dibujado) para
/// que `focus` (un punto de la imagen) quede en el mismo lugar de la pantalla.
/// Nunca (0, 0): ese origen es el de la pantalla entera.
fn zoom_origin(origin: (f32, f32), focus: (f32, f32), k: f32) -> (f32, f32) {
    let next = (origin.0 + focus.0 - focus.0 * k, origin.1 + focus.1 - focus.1 * k);
    if next == (0.0, 0.0) {
        (0.5, 0.5)
    } else {
        next
    }
}

/// Una figura que se está moviendo con Mover.
struct Dragging {
    start: (f32, f32),
    delta: (f32, f32),
    before: Vec<Shape>,
}

/// Un texto que se está escribiendo: dónde, cómo, y lo que había antes de
/// empezar (deshacer lo devuelve de una vez).
struct Typing {
    at: (f32, f32),
    wrap: f32,
    color: u32,
    size: f32,
    fill: Fill,
    style: Style,
    /// Un globo: adónde apunta su colita.
    tail: Option<(f32, f32)>,
    editor: Entity<TextArea>,
    /// Cada letra redibuja la pizarra: la caja crece con el texto.
    _changed: Subscription,
    before: Vec<Shape>,
}

/// El editor del texto en curso, con la letra como va a quedar: sobre una
/// etiqueta llena, en blanco o negro.
fn text_editor(body: &str, color: u32, fill: Fill, cx: &mut Context<BoardView>) -> (Entity<TextArea>, Subscription) {
    let ink = hsla(if fill == Fill::Solid { contrast(color) } else { color }, 1.0);
    let editor = cx.new(|cx| {
        let mut editor = TextArea::new("Escribe…", ink, ink.opacity(0.45), ink, cx);
        editor.set_text(body, cx);
        editor
    });
    let changed = cx.subscribe(&editor, |_, _, _: &crate::text_input::Changed, cx| cx.notify());
    (editor, changed)
}

/// La herramienta, el color, el grosor, el relleno y el estilo de la última
/// pizarra: la próxima empieza igual. El vistazo de la tira también los elige.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Prefs {
    pub tool: Tool,
    pub color: u32,
    pub width: usize,
    pub fill: Fill,
    pub style: Style,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { tool: Tool::Pen, color: COLORS[0], width: 1, fill: Fill::Outline, style: Style::default() }
    }
}

/// Cada herramienta con su nombre en `pizarra.txt`.
const TOOL_NAMES: [(Tool, &str); 20] = [
    (Tool::Select, "select"),
    (Tool::Crop, "crop"),
    (Tool::Pen, "pen"),
    (Tool::Highlight, "highlight"),
    (Tool::Line, "line"),
    (Tool::Arrow, "arrow"),
    (Tool::Curve, "curve"),
    (Tool::Rect, "rect"),
    (Tool::Ellipse, "ellipse"),
    (Tool::Step, "step"),
    (Tool::Text, "text"),
    (Tool::Bubble, "bubble"),
    (Tool::Emoji, "emoji"),
    (Tool::Pixelate, "pixelate"),
    (Tool::Focus, "focus"),
    (Tool::Magnify, "magnify"),
    (Tool::Ruler, "ruler"),
    (Tool::Grab, "grab"),
    (Tool::Picker, "picker"),
    (Tool::Eraser, "eraser"),
];

impl Prefs {
    fn file() -> Option<std::path::PathBuf> {
        crate::paths::file("pizarra.txt")
    }

    /// `pen e5484d 1 solid dashed shadow mono`, como lo guarda `save`. Lo que
    /// viene después del grosor puede faltar: así lo guardaban las versiones
    /// anteriores.
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split_whitespace();
        let name = parts.next()?;
        let tool = TOOL_NAMES
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(tool, _)| *tool)
            // El borrador y el cuentagotas no se recuerdan: abrir con ellos no sirve.
            .filter(|tool| !matches!(tool, Tool::Eraser | Tool::Picker))
            .unwrap_or(Tool::Pen);
        let color = u32::from_str_radix(parts.next()?, 16).ok()?;
        let width = parts.next()?.parse::<usize>().ok()?.min(WIDTHS.len() - 1);
        let mut fill = Fill::Outline;
        let mut style = Style::default();
        for word in parts {
            match word {
                "solid" => fill = Fill::Solid,
                "soft" => fill = Fill::Soft,
                "dashed" => style.dashed = true,
                "shadow" => style.shadow = true,
                "mono" => style.font = Font::Mono,
                "hand" => style.font = Font::Hand,
                _ => {}
            }
        }
        Some(Self { tool, color, width, fill, style })
    }

    fn text(&self) -> String {
        let tool = TOOL_NAMES.iter().find(|(t, _)| *t == self.tool).map_or("pen", |(_, n)| n);
        let fill = match self.fill {
            Fill::Outline => "outline",
            Fill::Solid => "solid",
            Fill::Soft => "soft",
        };
        let mut text = format!("{tool} {:06x} {} {fill}", self.color, self.width);
        if self.style.dashed {
            text.push_str(" dashed");
        }
        if self.style.shadow {
            text.push_str(" shadow");
        }
        match self.style.font {
            Font::Sans => {}
            Font::Mono => text.push_str(" mono"),
            Font::Hand => text.push_str(" hand"),
        }
        text
    }

    pub fn load() -> Self {
        Self::file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::file() {
            let _ = std::fs::write(path, self.text());
        }
    }
}

/// Adónde va además lo dibujado al terminar.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SendTo {
    /// Una página nueva del tablero de Flip.
    Flip,
    /// Al agente del espacio con el foco (o el más reciente), con una pregunta.
    Agent,
}

pub enum BoardEvent {
    Done {
        frame: Frame,
        shapes: Vec<Shape>,
        scale: f32,
        send: Option<SendTo>,
        /// Lo que sale: el recorte, en coordenadas de la pizarra.
        crop: Option<(f32, f32, f32, f32)>,
    },
    Closed,
}

/// Lo que se le escribe al agente: la ruta de la captura (Claude Code la
/// abre como imagen) y qué mirar.
fn agent_prompt(path: &std::path::Path) -> String {
    let path = path.display().to_string();
    let path = if path.contains(' ') { format!("\"{path}\"") } else { path };
    format!("{path} Mira esta captura: lo que marqué es lo importante.")
}

pub struct BoardView {
    frozen: Frozen,
    /// Dónde va la imagen dentro del monitor, en lógico: (0, 0) para la
    /// pantalla entera, o donde se tomó una captura para dibujarle encima.
    origin: (f32, f32),
    pub previous: Option<crate::paste::Target>,
    shapes: Vec<Shape>,
    /// Estados anteriores y deshechos: deshacer revierte por igual un trazo,
    /// un borrado o un «limpiar».
    past: Vec<Vec<Shape>>,
    future: Vec<Vec<Shape>>,
    current: Option<Shape>,
    /// Arrastrando el borrador; el estado previo se guardó al primer toque.
    erasing: bool,
    erase_saved: bool,
    /// Los trazos terminados, ya rasterizados.
    layer: Option<Arc<RenderImage>>,
    tool: Tool,
    color: u32,
    width: usize,
    fill: Fill,
    style: Style,
    /// La herramienta de antes del cuentagotas: se vuelve a ella al tomar el color.
    prev_tool: Tool,
    /// El panel de estilo (relleno, punteado, sombra, letra) abierto.
    style_open: bool,
    /// Cómo sale al terminar, y si su panel está abierto.
    export: Export,
    export_open: bool,
    /// El emoji que pone la herramienta Emoji, y los usados hace poco.
    emoji: String,
    recent_emojis: Vec<String>,
    /// La figura tomada con Mover.
    selected: Option<usize>,
    /// Arrastrándola: desde dónde, cuánto va y cómo estaba todo antes.
    dragging: Option<Dragging>,
    /// La figura que se arrastra, pintada aparte: se corre la imagen, no se
    /// vuelve a pintar todo en cada movimiento.
    drag_layer: Option<Arc<RenderImage>>,
    /// El recorte (x0, y0, x1, y1): al exportar sale solo eso.
    crop: Option<(f32, f32, f32, f32)>,
    /// La escala con que se abrió (Ctrl+0 vuelve a ella) y el tamaño de la
    /// ventana del último cuadro, para el zoom.
    base_scale: f32,
    viewport: (f32, f32),
    /// Los últimos colores de la herramienta Color, junto a la paleta.
    recent: Vec<u32>,
    cursor: (f32, f32),
    /// Primer Esc con algo dibujado: el segundo, dentro de `CLOSE_ARM`, cierra.
    close_armed: Option<Instant>,
    /// El texto que se está escribiendo.
    typing: Option<Typing>,
    /// El foco cambia en el próximo cuadro (hace falta la ventana): al
    /// editor al empezar un texto, a la pizarra al terminarlo.
    focus_editor: bool,
    refocus: bool,
    /// Un aviso corto en vez de la ayuda («Texto copiado», «Oculté 3 datos»).
    notice: Option<(String, Instant)>,
    /// El recuadro de «Copiar texto» mientras el OCR lo lee.
    grabbing: Option<Shape>,
    /// «Ocultar datos» buscando: el botón queda encendido.
    hiding: bool,
    focus: FocusHandle,
    meter: crate::meter::Meter,
}

impl EventEmitter<BoardEvent> for BoardView {}

impl Focusable for BoardView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BoardView {
    pub fn new(
        frozen: Frozen,
        origin: (f32, f32),
        cursor: (f32, f32),
        previous: Option<crate::paste::Target>,
        prefs: Prefs,
        cx: &mut Context<Self>,
    ) -> Self {
        let frozen_scale = frozen.scale;
        Self {
            frozen,
            origin,
            previous,
            shapes: Vec::new(),
            past: Vec::new(),
            future: Vec::new(),
            current: None,
            erasing: false,
            erase_saved: false,
            layer: None,
            tool: prefs.tool,
            color: prefs.color,
            width: prefs.width.min(WIDTHS.len() - 1),
            fill: prefs.fill,
            style: prefs.style,
            prev_tool: Tool::Pen,
            style_open: false,
            export: Export::load(),
            export_open: false,
            emoji: recent_emojis().into_iter().next().unwrap_or_else(|| EMOJIS[0].to_string()),
            recent_emojis: recent_emojis(),
            selected: None,
            dragging: None,
            drag_layer: None,
            crop: None,
            base_scale: frozen_scale,
            viewport: (0.0, 0.0),
            recent: crate::color::load_recents(),
            cursor,
            close_armed: None,
            typing: None,
            focus_editor: false,
            refocus: false,
            notice: None,
            grabbing: None,
            hiding: false,
            focus: cx.focus_handle(),
            meter: crate::meter::Meter::new("pizarra"),
        }
    }

    /// Un punto de la ventana, en coordenadas de la imagen.
    fn local(&self, p: Point<Pixels>) -> (f32, f32) {
        let (x, y) = self.frozen.local(p);
        (x - self.origin.0, y - self.origin.1)
    }

    fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.end_text(cx);
        if tool != Tool::Select {
            self.selected = None;
        }
        if tool == Tool::Picker && self.tool != Tool::Picker {
            self.prev_tool = self.tool;
        }
        self.tool = tool;
        cx.notify();
    }

    /// Cambia el estilo y lo aplica al texto en curso.
    fn set_style(&mut self, change: impl FnOnce(&mut Style), cx: &mut Context<Self>) {
        change(&mut self.style);
        self.restyle_text(cx);
        cx.notify();
    }

    /// El cuentagotas: el color de la foto en `p` pasa a ser el de dibujar, y
    /// se vuelve a la herramienta de antes.
    fn pick_color(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let frame = &self.frozen.frame;
        let (x, y) = ((p.0 * self.frozen.scale) as i64, (p.1 * self.frozen.scale) as i64);
        if x < 0 || y < 0 || x >= frame.width() as i64 || y >= frame.height() as i64 {
            return;
        }
        let i = (y as usize * frame.width() as usize + x as usize) * 4;
        let (b, g, r) = (frame.bgra[i], frame.bgra[i + 1], frame.bgra[i + 2]);
        self.color = u32::from_be_bytes([0, r, g, b]);
        self.tool = self.prev_tool;
        self.restyle_text(cx);
        self.notify_user(format!("Color #{:06X} · listo para dibujar", self.color), cx);
    }

    fn prefs(&self) -> Prefs {
        Prefs { tool: self.tool, color: self.color, width: self.width, fill: self.fill, style: self.style }
    }

    // --- Texto --------------------------------------------------------------

    /// Empieza un texto en `p`, o vuelve a editar el que está ahí. `tail`: un
    /// globo, con la colita apuntando ahí.
    fn start_text(&mut self, p: (f32, f32), tail: Option<(f32, f32)>, cx: &mut Context<Self>) {
        let before = self.shapes.clone();
        let existing = if tail.is_some() {
            None
        } else {
            self.shapes.iter().rposition(|shape| shape.tool == Tool::Text && shape.hit(p, 4.0))
        };
        let (at, wrap, color, size, fill, style, tail, body) = match existing {
            Some(index) => {
                // Sale de la capa mientras se edita: el editor va en su lugar.
                let shape = self.shapes.remove(index);
                self.rebuild_layer();
                let (x, y) = shape.points[0];
                let wrap = shape.points.get(1).map_or(TEXT_WRAP_MAX, |end| end.0 - x);
                let tail = shape.points.get(2).copied();
                ((x, y), wrap, shape.color, shape.width, shape.fill, shape.style, tail, shape.text)
            }
            None => {
                let size = TEXT_SIZES[self.width];
                // El clic queda a media altura de la primera línea.
                let at = (p.0, p.1 - size * TEXT_LINE / 2.0);
                let image_w = self.frozen.frame.width() as f32 / self.frozen.scale;
                let wrap = (image_w - at.0).clamp(TEXT_WRAP_MIN, TEXT_WRAP_MAX);
                // Un globo sin fondo no se ve como globo: va lleno.
                let fill = if tail.is_some() && self.fill == Fill::Outline { Fill::Solid } else { self.fill };
                (at, wrap, self.color, size, fill, self.style, tail, String::new())
            }
        };
        let (editor, _changed) = text_editor(&body, color, fill, cx);
        self.typing = Some(Typing { at, wrap, color, size, fill, style, tail, editor, _changed, before });
        self.focus_editor = true;
        cx.notify();
    }

    /// Cambiar el color, el grosor, el relleno o el estilo mientras se escribe
    /// cambia ese texto: el editor se rehace con lo escrito (su color no se
    /// cambia por fuera).
    fn restyle_text(&mut self, cx: &mut Context<Self>) {
        let (color, size, fill, style) = (self.color, TEXT_SIZES[self.width], self.fill, self.style);
        let Some((body, tail)) = self.typing.as_ref().map(|t| (t.editor.read(cx).text().to_string(), t.tail)) else {
            return;
        };
        let fill = if tail.is_some() && fill == Fill::Outline { Fill::Solid } else { fill };
        let (editor, changed) = text_editor(&body, color, fill, cx);
        if let Some(typing) = self.typing.as_mut() {
            typing.color = color;
            typing.size = size;
            typing.fill = fill;
            typing.style = style;
            typing.editor = editor;
            typing._changed = changed;
        }
        self.focus_editor = true;
    }

    /// Termina el texto: lo escrito queda como una forma (si no está vacío).
    fn end_text(&mut self, cx: &mut Context<Self>) {
        let Some(typing) = self.typing.take() else {
            return;
        };
        let body = typing.editor.read(cx).text().trim_end().to_string();
        if !body.trim().is_empty() {
            // Un globo guarda, de tercer punto, adónde apunta su colita.
            let mut points = vec![typing.at, (typing.at.0 + typing.wrap, typing.at.1)];
            points.extend(typing.tail);
            self.shapes.push(Shape {
                tool: Tool::Text,
                color: typing.color,
                width: typing.size,
                points,
                text: body,
                fill: typing.fill,
                style: typing.style,
            });
        }
        // Un solo paso para deshacer, sea texto nuevo, editado o borrado.
        if self.shapes != typing.before {
            self.past.push(typing.before);
            self.future.clear();
        }
        self.rebuild_layer();
        self.refocus = true;
        cx.notify();
    }

    fn rebuild_layer(&mut self) {
        let started = Instant::now();
        let frame = &self.frozen.frame;
        self.layer = rasterize(&self.shapes, frame, self.frozen.scale);
        if std::env::var_os("PILL_FPS").is_some() {
            eprintln!(
                "pizarra: capa de {} trazos en {} ms",
                self.shapes.len(),
                started.elapsed().as_millis()
            );
        }
    }

    /// Guarda el estado actual para deshacer y descarta lo deshecho.
    fn remember(&mut self) {
        self.past.push(self.shapes.clone());
        self.future.clear();
    }

    fn erase_at(&mut self, p: (f32, f32)) {
        let slack = 4.0;
        if let Some(index) = self.shapes.iter().rposition(|shape| shape.hit(p, slack)) {
            if !self.erase_saved {
                self.remember();
                self.erase_saved = true;
            }
            self.shapes.remove(index);
            self.rebuild_layer();
        }
    }

    fn undo_step(&mut self) {
        self.selected = None;
        if let Some(previous) = self.past.pop() {
            self.future.push(std::mem::replace(&mut self.shapes, previous));
            self.rebuild_layer();
        }
    }

    fn redo_step(&mut self) {
        self.selected = None;
        if let Some(next) = self.future.pop() {
            self.past.push(std::mem::replace(&mut self.shapes, next));
            self.rebuild_layer();
        }
    }

    fn clear_all(&mut self) {
        if !self.shapes.is_empty() {
            self.remember();
            self.shapes.clear();
            self.rebuild_layer();
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.undo_step();
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.redo_step();
        cx.notify();
    }

    fn clear(&mut self, _: &Clear, _: &mut Window, cx: &mut Context<Self>) {
        if !self.delete_selected() {
            self.clear_all();
        }
        cx.notify();
    }

    fn done(&mut self, _: &Done, _: &mut Window, cx: &mut Context<Self>) {
        self.finish(cx);
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        self.end_text(cx);
        self.finish_to(None, cx);
    }

    /// Termina y, además de guardar y copiar, lo manda a Flip o a un agente.
    fn finish_to(&mut self, send: Option<SendTo>, cx: &mut Context<Self>) {
        self.end_text(cx);
        if send == Some(SendTo::Agent) && crate::space::agent_consoles(cx) == 0 {
            self.notify_user("No hay agentes abiertos en el Espacio", cx);
            return;
        }
        cx.emit(BoardEvent::Done {
            frame: self.frozen.frame.clone(),
            shapes: self.shapes.clone(),
            scale: self.frozen.scale,
            send,
            crop: self.crop,
        });
    }

    fn close(&mut self, _: &Close, _: &mut Window, cx: &mut Context<Self>) {
        let armed = self
            .close_armed
            .is_some_and(|at| at.elapsed() < CLOSE_ARM);
        if self.shapes.is_empty() || armed {
            cx.emit(BoardEvent::Closed);
        } else {
            self.close_armed = Some(Instant::now());
            cx.notify();
        }
    }

    fn down(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        self.cursor = p;
        self.close_armed = None;
        // Un clic fuera del texto lo termina; el siguiente empieza otro.
        if self.typing.is_some() {
            self.end_text(cx);
            return;
        }
        if self.tool == Tool::Text {
            self.start_text(p, None, cx);
        } else if self.tool == Tool::Picker {
            self.pick_color(p, cx);
        } else if self.tool == Tool::Emoji {
            self.place_emoji(p);
        } else if self.tool == Tool::Select {
            self.grab_shape(p);
        } else if self.tool == Tool::Step {
            self.place_step(p);
        } else if self.tool == Tool::Eraser {
            self.erasing = true;
            self.erase_saved = false;
            self.erase_at(p);
        } else {
            self.current = Some(Shape {
                tool: self.tool,
                color: self.color,
                width: WIDTHS[self.width],
                points: vec![p],
                text: String::new(),
                fill: self.fill,
                style: self.style,
            });
        }
        cx.notify();
    }

    fn moved(&mut self, p: (f32, f32), shift: bool, cx: &mut Context<Self>) {
        self.cursor = p;
        if self.erasing {
            self.erase_at(p);
        }
        if let Some(drag) = self.dragging.as_mut() {
            let (dx, dy) = (p.0 - drag.start.0, p.1 - drag.start.1);
            // Shift: solo en horizontal o en vertical.
            drag.delta = if shift { if dx.abs() >= dy.abs() { (dx, 0.0) } else { (0.0, dy) } } else { (dx, dy) };
            cx.notify();
            return;
        }
        if let Some(shape) = self.current.as_mut() {
            let first = shape.points[0];
            match shape.tool {
                // La curva guarda el arrastre entero: se dobla por donde pasó.
                Tool::Pen | Tool::Highlight | Tool::Curve if !shift => {
                    let last = *shape.points.last().unwrap();
                    if distance(last, p) >= MIN_STEP {
                        shape.points.push(p);
                    }
                }
                tool => {
                    let to = if shift { constrain(tool, first, p) } else { p };
                    shape.points = vec![first, to];
                }
            }
            // Medir: el largo se ve mientras se arrastra.
            if shape.tool == Tool::Ruler {
                let (from, to) = (shape.points[0], *shape.points.last().unwrap());
                shape.text = ruler_label(from, to, self.frozen.scale);
            }
        }
        cx.notify();
    }

    // --- Mover --------------------------------------------------------------

    /// Toma la figura de más arriba bajo `p` y empieza a arrastrarla.
    fn grab_shape(&mut self, p: (f32, f32)) {
        self.selected = self.shapes.iter().rposition(|shape| shape.hit(p, 6.0));
        let Some(index) = self.selected else {
            return;
        };
        self.dragging = Some(Dragging { start: p, delta: (0.0, 0.0), before: self.shapes.clone() });
        // Sin ella en la capa de siempre, y ella sola en la suya.
        let frame = &self.frozen.frame;
        let others: Vec<Shape> = self.shapes.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, s)| s.clone()).collect();
        self.layer = rasterize(&others, frame, self.frozen.scale);
        self.drag_layer = rasterize(&self.shapes[index..=index], frame, self.frozen.scale);
    }

    /// Suelta la figura donde quedó (un paso para deshacer).
    fn drop_shape(&mut self, drag: Dragging) {
        self.drag_layer = None;
        if let Some(index) = self.selected.filter(|i| *i < self.shapes.len()) {
            if drag.delta != (0.0, 0.0) {
                self.shapes[index] = self.shapes[index].moved(drag.delta.0, drag.delta.1);
                self.past.push(drag.before);
                self.future.clear();
            }
        }
        self.rebuild_layer();
    }

    /// Supr: la figura tomada; sin ninguna, todo (como antes).
    fn delete_selected(&mut self) -> bool {
        let Some(index) = self.selected.take().filter(|i| *i < self.shapes.len()) else {
            return false;
        };
        self.remember();
        self.shapes.remove(index);
        self.rebuild_layer();
        true
    }

    /// Ctrl+D: una copia un poco corrida, que queda tomada.
    fn duplicate_selected(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.selected.filter(|i| *i < self.shapes.len()) else {
            return;
        };
        self.remember();
        let copy = self.shapes[index].moved(14.0, 14.0);
        self.shapes.push(copy);
        self.selected = Some(self.shapes.len() - 1);
        self.rebuild_layer();
        cx.notify();
    }

    /// Las flechas corren la figura tomada 1 px (10 con Shift).
    fn nudge(&mut self, dx: f32, dy: f32, window: &Window, cx: &mut Context<Self>) {
        let Some(index) = self.selected.filter(|i| *i < self.shapes.len()) else {
            return;
        };
        let step = if window.modifiers().shift { 10.0 } else { 1.0 };
        self.remember();
        self.shapes[index] = self.shapes[index].moved(dx * step, dy * step);
        self.rebuild_layer();
        cx.notify();
    }

    // --- Zoom ----------------------------------------------------------------

    /// Acerca (`factor` > 1) o aleja la imagen dejando quieto lo que está bajo
    /// el cursor. Solo sobre una captura (no la pantalla entera congelada):
    /// cambia la escala de la vista, y lo dibujado se lleva a la nueva.
    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let target = self.frozen.scale / factor;
        self.zoom_to(target, cx);
    }

    fn zoom_to(&mut self, target: f32, cx: &mut Context<Self>) {
        if self.origin == (0.0, 0.0) {
            self.notify_user("El zoom es para capturas abiertas en la pizarra", cx);
            return;
        }
        // Entre la cuarta parte y cuatro veces lo de al abrir.
        let new = target.clamp(self.base_scale / 4.0, self.base_scale * 4.0);
        let old = self.frozen.scale;
        if (new - old).abs() < f32::EPSILON {
            return;
        }
        self.end_text(cx);
        self.dragging = None;
        self.drag_layer = None;
        let k = old / new;
        self.shapes = scale_shapes(&self.shapes, k);
        self.past = self.past.iter().map(|state| scale_shapes(state, k)).collect();
        self.future = self.future.iter().map(|state| scale_shapes(state, k)).collect();
        self.current = None;
        self.crop = self.crop.map(|(x0, y0, x1, y1)| (x0 * k, y0 * k, x1 * k, y1 * k));
        let focus = self.cursor;
        self.origin = zoom_origin(self.origin, focus, k);
        self.cursor = (focus.0 * k, focus.1 * k);
        self.frozen.scale = new;
        self.rebuild_layer();
        let percent = (self.base_scale / new * 100.0).round();
        self.notify_user(format!("Zoom {percent} % · Ctrl+0 vuelve · la rueda mueve la imagen"), cx);
    }

    /// La rueda mueve la imagen (si quedó más grande que la ventana); con
    /// Ctrl, acerca o aleja.
    fn wheel(&mut self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(40.0));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control {
            if dy != 0.0 {
                self.zoom_by(if dy > 0.0 { 1.12 } else { 1.0 / 1.12 }, cx);
            }
            return;
        }
        if self.origin == (0.0, 0.0) {
            return;
        }
        let (w, h) = (self.frozen.frame.width() as f32 / self.frozen.scale, self.frozen.frame.height() as f32 / self.frozen.scale);
        let (vw, vh) = self.viewport;
        // Solo si no cabe; y sin dejar la imagen fuera de la vista.
        let pan = |origin: f32, d: f32, size: f32, view: f32| {
            if size <= view { origin } else { (origin + d).clamp(view - size - 40.0, 40.0) }
        };
        let next = (pan(self.origin.0, dx, w, vw), pan(self.origin.1, dy, h, vh));
        if next != self.origin {
            self.origin = if next == (0.0, 0.0) { (0.5, 0.5) } else { next };
            cx.notify();
        }
    }

    /// Un sello del emoji elegido, centrado donde se hizo clic.
    fn place_emoji(&mut self, p: (f32, f32)) {
        self.remember();
        self.shapes.push(Shape {
            tool: Tool::Emoji,
            color: 0,
            width: EMOJI_SIZES[self.width],
            points: vec![p],
            text: self.emoji.clone(),
            fill: Fill::Outline,
            style: Style::default(),
        });
        self.rebuild_layer();
        remember_emoji(&self.emoji);
        self.recent_emojis.retain(|e| *e != self.emoji);
        self.recent_emojis.insert(0, self.emoji.clone());
        self.recent_emojis.truncate(8);
    }

    /// Los emojis para elegir, bajo la barra mientras la herramienta es Emoji.
    fn emoji_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let mut list: Vec<String> = self.recent_emojis.clone();
        for emoji in EMOJIS {
            if !list.iter().any(|e| e == emoji) {
                list.push(emoji.to_string());
            }
        }
        let cells = list.into_iter().enumerate().map(|(i, emoji)| {
            let on = emoji == self.emoji;
            let pick = emoji.clone();
            div()
                .id(("board-emoji-pick", i))
                .size(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(px(18.))
                .font_family("Segoe UI Emoji")
                .when(on, |el| el.border_1().border_color(text.opacity(0.7)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.emoji = pick.clone();
                    cx.notify();
                }))
                .child(emoji)
                .hover_bg(("board-emoji-fx", i), text.opacity(if on { 0.14 } else { 0.0 }), text.opacity(0.12))
        });
        div()
            .id("board-emoji-panel")
            .max_w(px(560.))
            .px(px(8.))
            .py(px(6.))
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .rounded(px(18.))
            .bg(rgb(0x1a1a18))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(cells)
    }

    /// Un paso numerado donde se hizo clic: el número que sigue al más alto.
    fn place_step(&mut self, p: (f32, f32)) {
        let next = self
            .shapes
            .iter()
            .filter(|shape| shape.tool == Tool::Step)
            .filter_map(|shape| shape.text.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        self.remember();
        self.shapes.push(Shape {
            tool: Tool::Step,
            color: self.color,
            width: STEP_RADII[self.width],
            points: vec![p],
            text: next.to_string(),
            fill: Fill::Solid,
            style: self.style,
        });
        self.rebuild_layer();
    }

    // --- OCR: copiar texto y ocultar datos -------------------------------------

    /// Muestra `text` un momento en lugar de la ayuda.
    fn notify_user(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some((text.into(), Instant::now()));
        cx.notify();
        // Pasado el aviso, vuelve la ayuda.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_FOR + Duration::from_millis(50)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Lee el texto del recuadro y lo copia.
    fn grab_text(&mut self, shape: Shape, cx: &mut Context<Self>) {
        let Some((w, h, bgra)) = crop(&self.frozen.frame, shape.corners(), self.frozen.scale) else {
            self.notify_user("El recuadro es muy chico para leer texto", cx);
            return;
        };
        self.grabbing = Some(shape);
        self.notify_user("Leyendo el texto…", cx);
        cx.spawn(async move |this, cx| {
            let read = cx.background_spawn(async move { crate::ocr::recognize(w, h, &bgra) }).await;
            let _ = this.update(cx, |view, cx| {
                view.grabbing = None;
                match read {
                    Ok(text) if !text.trim().is_empty() => {
                        let count = text.chars().count();
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        view.notify_user(format!("Texto copiado · {count} caracteres"), cx);
                    }
                    Ok(_) => view.notify_user("No encontré texto ahí", cx),
                    Err(error) => view.notify_user(format!("No se pudo leer: {error}"), cx),
                }
            });
        })
        .detach();
    }

    /// Busca correos, teléfonos, RUT, claves… en toda la imagen y los pixela.
    fn hide_data(&mut self, cx: &mut Context<Self>) {
        if self.hiding {
            return;
        }
        self.end_text(cx);
        self.hiding = true;
        self.notify_user("Buscando datos para ocultar…", cx);
        let frame = &self.frozen.frame;
        let (w, h, bgra, scale) = (frame.width(), frame.height(), frame.bgra.clone(), self.frozen.scale);
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    crate::ocr::recognize_lines(w, h, &bgra).map(|lines| crate::redact::sensitive_areas(&lines))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.hiding = false;
                match found {
                    Ok(areas) if areas.is_empty() => view.notify_user("No encontré datos para ocultar", cx),
                    Ok(areas) => {
                        view.remember();
                        for area in &areas {
                            // Del OCR (píxeles de la foto) a la pizarra.
                            let (x0, y0) = (area.x / scale, area.y / scale);
                            let (x1, y1) = ((area.x + area.w) / scale, (area.y + area.h) / scale);
                            view.shapes.push(Shape {
                                tool: Tool::Pixelate,
                                color: 0,
                                width: WIDTHS[1],
                                points: vec![(x0, y0), (x1, y1)],
                                text: String::new(),
                                fill: Fill::Outline,
                                style: Style::default(),
                            });
                        }
                        view.rebuild_layer();
                        let n = areas.len();
                        let noun = if n == 1 { "dato" } else { "datos" };
                        view.notify_user(format!("Oculté {n} {noun} · Ctrl+Z para deshacer"), cx);
                    }
                    Err(error) => view.notify_user(format!("No se pudo leer: {error}"), cx),
                }
            });
        })
        .detach();
    }

    fn cycle_fill(&mut self, cx: &mut Context<Self>) {
        self.fill = self.fill.next();
        self.restyle_text(cx);
        cx.notify();
    }

    fn up(&mut self, cx: &mut Context<Self>) {
        self.erasing = false;
        if let Some(drag) = self.dragging.take() {
            self.drop_shape(drag);
            cx.notify();
            return;
        }
        if let Some(shape) = self.current.take() {
            // El recorte no es una figura: un arrastre lo pone, un clic lo quita.
            if shape.tool == Tool::Crop {
                let (x0, y0, x1, y1) = shape.corners();
                if x1 - x0 >= 8.0 && y1 - y0 >= 8.0 {
                    self.crop = Some((x0, y0, x1, y1));
                    self.notify_user("Recorte listo: al exportar sale solo eso · clic sin arrastrar lo quita", cx);
                } else if self.crop.take().is_some() {
                    self.notify_user("Recorte quitado", cx);
                }
                cx.notify();
                return;
            }
            let span = shape
                .points
                .last()
                .map(|last| distance(shape.points[0], *last))
                .unwrap_or_default();
            // El globo: se arrastró desde lo que señala hasta donde va el
            // texto; un clic solo es un texto como cualquiera.
            if shape.tool == Tool::Bubble {
                let end = *shape.points.last().unwrap_or(&shape.points[0]);
                let tail = (span >= 12.0).then_some(shape.points[0]);
                self.start_text(end, tail, cx);
                cx.notify();
                return;
            }
            // «Copiar texto» no deja nada: lee el recuadro y lo copia.
            if shape.tool == Tool::Grab {
                if span >= 3.0 {
                    self.grab_text(shape, cx);
                }
                cx.notify();
                return;
            }
            // Un clic con la flecha o el rectángulo no deja una forma vacía.
            let keep = matches!(shape.tool, Tool::Pen | Tool::Highlight) || span >= 3.0;
            if keep {
                self.remember();
                self.shapes.push(shape);
                self.rebuild_layer();
            }
        }
        cx.notify();
    }

    fn button(
        &self,
        id: &'static str,
        icon: &'static str,
        active: bool,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        div()
            .id(id)
            .size(px(BUTTON))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(BUTTON / 2.0))
            .cursor_pointer()
            .when_some(button_tip(id), |el, label| el.tooltip(tip(label)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                // Deshacer, borrar todo o listo: primero queda el texto en curso.
                view.end_text(cx);
                on_click(view, cx)
            }))
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                let (rest, over) = if active { (0.16, 0.18) } else { (0.0, 0.09) };
                let fg = if active { text } else { h.mix(text.opacity(0.62), text) };
                el.bg(h.mix(text.opacity(rest), text.opacity(over + 0.05 * h.press)))
                    .child(svg().path(icon).size(px(15. - 1.5 * h.press)).text_color(fg))
            })
    }

    fn separator() -> impl IntoElement {
        div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(gpui::white().opacity(0.12))
    }

    /// La barra cuelga del borde de arriba como el notch; sobre una captura
    /// va pegada a ella, como en Flameshot. Si no cabe, salta a otra fila.
    fn toolbar(&self, framed: bool, max_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let tools = TOOLS.iter().map(|&(tool, icon, _)| {
            self.button(tool_id(tool), icon, self.tool == tool, move |view, cx| view.set_tool(tool, cx), cx)
                .into_any_element()
        })
        .collect::<Vec<_>>();
        // La paleta, el color propio (del cuentagotas) y los recientes de la
        // herramienta Color.
        let custom = (!COLORS.contains(&self.color)).then_some(self.color);
        let swatches: Vec<u32> = COLORS
            .iter()
            .copied()
            .chain(custom)
            .chain(self.recent.iter().copied().filter(|c| !COLORS.contains(c) && Some(*c) != custom).take(3))
            .collect();
        let colors = swatches.into_iter().enumerate().map(|(i, color)| {
            let active = self.color == color;
            div()
                .id(("board-color", i))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(11.))
                .cursor_pointer()
                .when(i >= COLORS.len(), |el| el.tooltip(crate::hover::tip_text(format!("#{color:06X}").into())))
                .when(active, |el| el.border_2().border_color(gpui::white().opacity(0.85)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.color = color;
                    if matches!(view.tool, Tool::Eraser | Tool::Picker) {
                        view.tool = if view.tool == Tool::Picker { view.prev_tool } else { Tool::Pen };
                    }
                    view.restyle_text(cx);
                    cx.notify();
                }))
                .fx(("board-color-fx", i), move |el, h| {
                    // El color crece con el cursor y se achica al apretar.
                    let size = 14. + 3. * h.t - 2. * h.press;
                    el.child(
                        div()
                            .size(px(size))
                            .rounded(px(size / 2.))
                            .bg(rgb(color))
                            .border_1()
                            .border_color(gpui::white().opacity(0.25 + 0.2 * h.t)),
                    )
                })
                .into_any_element()
        })
        .collect::<Vec<_>>();
        let widths = WIDTHS.iter().enumerate().map(|(i, &width)| {
            let active = self.width == i;
            div()
                .id(("board-width", i))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.width = i;
                    view.restyle_text(cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(width + 2.0))
                        .rounded(px(width))
                        .bg(gpui::white().opacity(0.85)),
                )
                .hover_bg(
                    ("board-width-fx", i),
                    gpui::white().opacity(if active { 0.16 } else { 0.0 }),
                    gpui::white().opacity(if active { 0.18 } else { 0.09 }),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
        // El relleno: un cuadrito que muestra cómo va a quedar (borde, lleno,
        // suave). Vale para rectángulo, elipse y texto.
        let fill = {
            let state = self.fill;
            let ink = hsla(self.color, 1.0);
            div()
                .id("board-fill")
                .size(px(BUTTON))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(BUTTON / 2.0))
                .cursor_pointer()
                .tooltip(tip(state.label()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cycle_fill(cx)))
                .child(
                    div()
                        .size(px(14.))
                        .rounded(px(4.))
                        .border_2()
                        .border_color(ink)
                        .when(state == Fill::Solid, |el| el.bg(ink))
                        .when(state == Fill::Soft, |el| el.bg(ink.opacity(0.35))),
                )
                .hover_bg("board-fill-fx", gpui::white().opacity(0.0), gpui::white().opacity(0.09))
        };
        div()
            .id("board-bar")
            .min_h(px(BAR_H))
            .max_w(px(max_w))
            .py(px(7.))
            .px(px(10.))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .when(framed, |el| el.rounded(px(20.)))
            .when(!framed, |el| el.rounded_b(px(20.)))
            .bg(rgb(0x1a1a18))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(tools)
            .child(Self::separator())
            .children(colors)
            .child(Self::separator())
            .children(widths)
            .child(fill)
            .child(self.button("board-style", "icons/sliders.svg", self.style_open, |view, cx| {
                view.style_open = !view.style_open;
                cx.notify();
            }, cx))
            .child(Self::separator())
            .child(self.button("board-hide", "icons/eye-off.svg", self.hiding, |view, cx| view.hide_data(cx), cx))
            .child(self.button("board-undo", "icons/undo.svg", false, |view, cx| {
                view.undo_step();
                cx.notify();
            }, cx))
            .child(self.button("board-redo", "icons/redo.svg", false, |view, cx| {
                view.redo_step();
                cx.notify();
            }, cx))
            .child(self.button("board-clear", "icons/trash.svg", false, |view, cx| {
                view.clear_all();
                cx.notify();
            }, cx))
            .child(Self::separator())
            .child(self.button("board-export", "icons/download.svg", self.export_open, |view, cx| {
                view.export_open = !view.export_open;
                cx.notify();
            }, cx))
            .child(self.button("board-flip", "icons/flip.svg", false, |view, cx| view.finish_to(Some(SendTo::Flip), cx), cx))
            .child(self.button("board-agent", "icons/sparkles.svg", false, |view, cx| view.finish_to(Some(SendTo::Agent), cx), cx))
            .child(self.button("board-done", "icons/check.svg", false, |view, cx| view.finish(cx), cx))
            .child(self.button("board-close", "icons/x.svg", false, |_, cx| cx.emit(BoardEvent::Closed), cx))
    }

    /// El panel de estilo, bajo la barra: relleno, trazo, sombra y letra.
    fn style_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let option = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>, act: fn(&mut Self, &mut Context<Self>)| {
            div()
                .id(id)
                .px(px(10.))
                .h(px(26.))
                .flex()
                .items_center()
                .rounded(px(13.))
                .text_size(px(12.))
                .cursor_pointer()
                .text_color(if on { text } else { text.opacity(0.62) })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| act(view, cx)))
                .child(label)
                .hover_bg(SharedString::from(format!("{id}-fx")), text.opacity(if on { 0.16 } else { 0.0 }), text.opacity(if on { 0.18 } else { 0.09 }))
                .into_any_element()
        };
        let group = |name: &'static str, items: Vec<gpui::AnyElement>| {
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(div().text_size(px(11.)).text_color(text.opacity(0.45)).mr(px(4.)).child(name))
                .children(items)
        };
        let (fill, style) = (self.fill, self.style);
        div()
            .id("board-style-panel")
            .px(px(12.))
            .py(px(6.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(14.))
            .rounded(px(18.))
            .bg(rgb(0x1a1a18))
            .shadow_lg()
            .font_family("Segoe UI")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(group("Relleno", vec![
                option("st-outline", "Borde", fill == Fill::Outline, cx, |v, cx| v.set_fill(Fill::Outline, cx)),
                option("st-solid", "Lleno", fill == Fill::Solid, cx, |v, cx| v.set_fill(Fill::Solid, cx)),
                option("st-soft", "Suave", fill == Fill::Soft, cx, |v, cx| v.set_fill(Fill::Soft, cx)),
            ]))
            .child(group("Trazo · D", vec![
                option("st-solid-line", "Continuo", !style.dashed, cx, |v, cx| v.set_style(|s| s.dashed = false, cx)),
                option("st-dashed", "Punteado", style.dashed, cx, |v, cx| v.set_style(|s| s.dashed = true, cx)),
            ]))
            .child(group("Sombra · S", vec![
                option("st-flat", "No", !style.shadow, cx, |v, cx| v.set_style(|s| s.shadow = false, cx)),
                option("st-shadow", "Sí", style.shadow, cx, |v, cx| v.set_style(|s| s.shadow = true, cx)),
            ]))
            .child(group("Letra", vec![
                option("st-sans", Font::Sans.label(), style.font == Font::Sans, cx, |v, cx| v.set_style(|s| s.font = Font::Sans, cx)),
                option("st-mono", Font::Mono.label(), style.font == Font::Mono, cx, |v, cx| v.set_style(|s| s.font = Font::Mono, cx)),
                option("st-hand", Font::Hand.label(), style.font == Font::Hand, cx, |v, cx| v.set_style(|s| s.font = Font::Hand, cx)),
            ]))
    }

    /// El panel de exportar, bajo la barra: vale para Listo, Flip y el agente.
    fn export_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let option = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>, act: fn(&mut Export)| {
            div()
                .id(id)
                .px(px(10.))
                .h(px(26.))
                .flex()
                .items_center()
                .rounded(px(13.))
                .text_size(px(12.))
                .cursor_pointer()
                .text_color(if on { text } else { text.opacity(0.62) })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    act(&mut view.export);
                    view.export.save();
                    cx.notify();
                }))
                .child(label)
                .hover_bg(SharedString::from(format!("{id}-fx")), text.opacity(if on { 0.16 } else { 0.0 }), text.opacity(if on { 0.18 } else { 0.09 }))
                .into_any_element()
        };
        let group = |name: &'static str, items: Vec<gpui::AnyElement>| {
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(div().text_size(px(11.)).text_color(text.opacity(0.45)).mr(px(4.)).child(name))
                .children(items)
        };
        let e = self.export;
        div()
            .id("board-export-panel")
            .px(px(12.))
            .py(px(6.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(14.))
            .rounded(px(18.))
            .bg(rgb(0x1a1a18))
            .shadow_lg()
            .font_family("Segoe UI")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(group("Tamaño", vec![
                option("ex-50", EXPORT_SIZES[0].1, e.size == 0, cx, |x| x.size = 0),
                option("ex-100", EXPORT_SIZES[1].1, e.size == 1, cx, |x| x.size = 1),
                option("ex-200", EXPORT_SIZES[2].1, e.size == 2, cx, |x| x.size = 2),
            ]))
            .child(group("Archivo", vec![
                option("ex-png", "PNG", !e.jpeg, cx, |x| x.jpeg = false),
                option("ex-jpg", "JPG liviano", e.jpeg, cx, |x| x.jpeg = true),
            ]))
            .child(group("Fondo", vec![
                option("ex-photo", "Con la foto", !e.transparent, cx, |x| x.transparent = false),
                option("ex-clear", "Solo lo dibujado", e.transparent, cx, |x| x.transparent = true),
            ]))
            .child(group("Marca de agua", vec![
                option("ex-nomark", "No", !e.mark, cx, |x| x.mark = false),
                option("ex-mark", "Sí", e.mark, cx, |x| x.mark = true),
            ]))
            .child(group("Marco", vec![
                option("ex-bg0", "Ninguno", e.backdrop == 0, cx, |x| x.backdrop = 0),
                option("ex-bg1", BACKDROPS[0].0, e.backdrop == 1, cx, |x| x.backdrop = 1),
                option("ex-bg2", BACKDROPS[1].0, e.backdrop == 2, cx, |x| x.backdrop = 2),
                option("ex-bg3", BACKDROPS[2].0, e.backdrop == 3, cx, |x| x.backdrop = 3),
                option("ex-bg4", BACKDROPS[3].0, e.backdrop == 4, cx, |x| x.backdrop = 4),
            ]))
    }

    fn set_fill(&mut self, fill: Fill, cx: &mut Context<Self>) {
        self.fill = fill;
        self.restyle_text(cx);
        cx.notify();
    }
}

/// El identificador del botón de cada herramienta.
fn tool_id(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "board-select",
        Tool::Crop => "board-crop",
        Tool::Pen => "board-pen",
        Tool::Highlight => "board-highlight",
        Tool::Line => "board-line",
        Tool::Arrow => "board-arrow",
        Tool::Curve => "board-curve",
        Tool::Rect => "board-rect",
        Tool::Ellipse => "board-ellipse",
        Tool::Step => "board-step",
        Tool::Text => "board-text",
        Tool::Bubble => "board-bubble",
        Tool::Emoji => "board-emoji",
        Tool::Pixelate => "board-pixelate",
        Tool::Focus => "board-focus",
        Tool::Magnify => "board-magnify",
        Tool::Ruler => "board-ruler",
        Tool::Grab => "board-grab",
        Tool::Picker => "board-picker",
        Tool::Eraser => "board-eraser",
    }
}

/// El nombre de cada botón de la barra, con su atajo.
fn button_tip(id: &str) -> Option<&'static str> {
    if let Some((_, _, label)) = TOOLS.iter().find(|(tool, ..)| tool_id(*tool) == id) {
        return Some(label);
    }
    match id {
        "board-hide" => Some("Ocultar datos: correos, teléfonos, RUT, claves… · Ctrl+H"),
        "board-style" => Some("Estilo: relleno, punteado, sombra y letra"),
        "board-undo" => Some("Deshacer · Ctrl+Z"),
        "board-redo" => Some("Rehacer · Ctrl+Y"),
        "board-clear" => Some("Borrar todo"),
        "board-export" => Some("Exportar: tamaño, formato, fondo y marca de agua"),
        "board-flip" => Some("Enviar a Flip: una página nueva del tablero"),
        "board-agent" => Some("Preguntarle a un agente del Espacio: le paso la captura"),
        "board-done" => Some("Listo · copia y guarda"),
        "board-close" => Some("Cerrar · Esc"),
        _ => None,
    }
}

fn chip(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .px(px(10.))
        .py(px(4.))
        .rounded(px(10.))
        .bg(gpui::black().opacity(0.78))
        .text_color(gpui::white())
        .text_size(px(11.))
        .font_family("Segoe UI")
        .child(text.into())
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.meter.frame(Instant::now());
        if std::mem::take(&mut self.focus_editor) {
            if let Some(typing) = &self.typing {
                window.focus(&typing.editor.focus_handle(cx));
            }
        }
        if std::mem::take(&mut self.refocus) && self.typing.is_none() {
            window.focus(&self.focus);
        }
        let viewport = window.viewport_size();
        self.viewport = (f32::from(viewport.width), f32::from(viewport.height));
        let (screen_w, screen_h) = (f32::from(viewport.width), f32::from(viewport.height));
        let scale = self.frozen.scale;
        let (w, h) = (
            self.frozen.frame.width() as f32 / scale,
            self.frozen.frame.height() as f32 / scale,
        );
        // Lo que se está arrastrando y el recuadro que el OCR está leyendo.
        let shapes: Vec<Shape> = self.current.clone().into_iter().chain(self.grabbing.clone()).collect();
        let layer = self.layer.clone();
        // Mover: la figura arrastrada (aparte, corrida) y su recuadro.
        let delta = self.dragging.as_ref().map_or((0.0, 0.0), |drag| drag.delta);
        let drag_layer = self.drag_layer.clone();
        let selection = self.selected.and_then(|i| self.shapes.get(i)).map(|shape| {
            let (x0, y0, x1, y1) = shape.bounds();
            (x0 + delta.0 - 4.0, y0 + delta.1 - 4.0, x1 + delta.0 + 4.0, y1 + delta.1 + 4.0)
        });
        let crop = self.crop;
        // El pincel bajo el cursor: el tamaño real de lo que va a pintar.
        let brush = match self.tool {
            Tool::Pen => Some(WIDTHS[self.width]),
            Tool::Highlight => Some(WIDTHS[self.width] * HIGHLIGHT_WIDTH),
            Tool::Eraser => Some(14.0),
            _ => None,
        };
        let cursor = self.cursor;
        let offset = (
            self.frozen.offset.0 + self.origin.0,
            self.frozen.offset.1 + self.origin.1,
        );
        // Sobre una captura, lo de alrededor se oscurece: se dibuja en ella.
        let framed = self.origin != (0.0, 0.0);
        let brush_color = if self.tool == Tool::Eraser {
            gpui::white()
        } else {
            hsla(self.color, 1.0)
        };
        let hint = if self
            .close_armed
            .is_some_and(|at| at.elapsed() < CLOSE_ARM)
        {
            Some("Esc otra vez para descartar lo dibujado".to_string())
        } else if let Some((text, _)) = self.notice.as_ref().filter(|(_, at)| at.elapsed() < NOTICE_FOR) {
            Some(text.clone())
        } else if self.typing.is_some() {
            Some("Enter salto de línea · Esc o clic fuera para terminar".to_string())
        } else if let Some(measure) = self.current.as_ref().filter(|s| s.tool == Tool::Ruler && !s.text.is_empty()) {
            Some(format!("Medida: {} · Shift recta", measure.text))
        } else if self.tool == Tool::Ruler {
            Some("Medir · M · arrastra entre dos puntos: queda el largo en píxeles · Esc salir".to_string())
        } else if self.tool == Tool::Bubble {
            Some("Globo · U · arrastra desde lo que señalas hasta donde va el texto · Esc salir".to_string())
        } else if self.tool == Tool::Magnify {
            Some("Lupa · Z · arrastra desde el centro: lo de adentro se ve al doble · Esc salir".to_string())
        } else if self.tool == Tool::Select {
            Some("Mover · V · arrastra una figura · Supr la borra · Ctrl+D duplica · flechas la corren".to_string())
        } else if self.tool == Tool::Crop {
            Some("Recortar · K · marca lo que sale al exportar · clic sin arrastrar lo quita".to_string())
        } else if self.tool == Tool::Emoji {
            Some(format!("Emoji · J · clic para poner {} · el grosor cambia el tamaño · Esc salir", self.emoji))
        } else if self.tool == Tool::Picker {
            Some("Cuentagotas · I · clic en la foto para tomar su color · Esc salir".to_string())
        } else if self.tool == Tool::Grab {
            Some("Copiar texto · G · marca un recuadro y su texto queda copiado · Esc salir".to_string())
        } else if self.tool == Tool::Text {
            Some("Texto · T · clic para escribir, o sobre un texto para cambiarlo · F fondo · Esc salir".to_string())
        } else if self.tool == Tool::Step {
            Some("Pasos · N · cada clic pone el número siguiente · Ctrl+Z deshacer · Esc salir".to_string())
        } else if self.tool == Tool::Pixelate {
            Some("Pixelar · B · arrastra sobre lo que quieres tapar · Ctrl+Z deshacer · Esc salir".to_string())
        } else if self.tool == Tool::Focus {
            Some("Foco · O · marca lo importante: lo demás se oscurece · Shift parejo · Esc salir".to_string())
        } else if self.tool == Tool::Curve {
            Some("Flecha curva · C · arrastra por donde quieres que pase · Shift recta · Esc salir".to_string())
        } else if matches!(self.tool, Tool::Rect | Tool::Ellipse) {
            TOOLS.iter().find(|(tool, ..)| *tool == self.tool).map(|(_, _, label)| {
                format!("{label} · Shift parejo · F relleno · Ctrl+Z deshacer · Enter copiar · Esc salir")
            })
        } else {
            TOOLS
                .iter()
                .find(|(tool, ..)| *tool == self.tool)
                .map(|(_, _, label)| {
                    format!("{label} · Shift recto · Ctrl+Z deshacer · Enter copiar · Esc salir")
                })
        };

        // El editor del texto en curso, justo donde va a quedar: misma letra,
        // tamaño y alto de línea que `draw_text`, para que al terminar no salte.
        let editor = self.typing.as_ref().map(|t| {
            let line_h = t.size * TEXT_LINE;
            let rows = wrap_lines(t.editor.read(cx).text(), t.wrap, |s| text_width_in(s, t.size, t.style.font)).len().max(1);
            let height = (rows as f32 * line_h).max(f32::from(t.editor.read(cx).content_height(1)));
            // El fondo como va a quedar: etiqueta llena, suave o nada.
            let bg = match t.fill {
                Fill::Solid => hsla(t.color, 1.0),
                Fill::Soft => hsla(SOFT_TEXT_BG.0, SOFT_TEXT_BG.1),
                Fill::Outline => gpui::black().opacity(0.12),
            };
            div()
                .absolute()
                .left(px(offset.0 + t.at.0 - 6.0))
                .top(px(offset.1 + t.at.1 - 3.0))
                .w(px(t.wrap + 12.0))
                .h(px(height + 6.0))
                .rounded(px(6.))
                .border_1()
                .border_color(gpui::white().opacity(0.45))
                .bg(bg)
                // Clics y arrastres dentro son del editor, no de la pizarra.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .top(px(3.))
                        .w(px(t.wrap))
                        .h(px(height))
                        // La misma letra que `draw_text`; la normal en seminegrita.
                        .font_family(t.style.font.family())
                        .when(t.style.font == Font::Sans, |el| el.font_weight(gpui::FontWeight::SEMIBOLD))
                        .when(t.style.font == Font::Mono, |el| el.font_weight(gpui::FontWeight::BOLD))
                        .text_size(px(t.size))
                        .line_height(px(line_h))
                        .child(t.editor.clone()),
                )
        });

        div()
            .id("board")
            .track_focus(&self.focus)
            .key_context(if self.typing.is_some() { TYPING_CONTEXT } else { KEY_CONTEXT })
            .on_action(cx.listener(|v, _: &EndText, _, cx| v.end_text(cx)))
            .on_action(cx.listener(|v, _: &PickText, _, cx| v.set_tool(Tool::Text, cx)))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::done))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(|v, _: &PickPen, _, cx| v.set_tool(Tool::Pen, cx)))
            .on_action(cx.listener(|v, _: &PickHighlight, _, cx| v.set_tool(Tool::Highlight, cx)))
            .on_action(cx.listener(|v, _: &PickLine, _, cx| v.set_tool(Tool::Line, cx)))
            .on_action(cx.listener(|v, _: &PickArrow, _, cx| v.set_tool(Tool::Arrow, cx)))
            .on_action(cx.listener(|v, _: &PickCurve, _, cx| v.set_tool(Tool::Curve, cx)))
            .on_action(cx.listener(|v, _: &PickRect, _, cx| v.set_tool(Tool::Rect, cx)))
            .on_action(cx.listener(|v, _: &PickEllipse, _, cx| v.set_tool(Tool::Ellipse, cx)))
            .on_action(cx.listener(|v, _: &PickStep, _, cx| v.set_tool(Tool::Step, cx)))
            .on_action(cx.listener(|v, _: &PickPixelate, _, cx| v.set_tool(Tool::Pixelate, cx)))
            .on_action(cx.listener(|v, _: &PickFocus, _, cx| v.set_tool(Tool::Focus, cx)))
            .on_action(cx.listener(|v, _: &PickGrab, _, cx| v.set_tool(Tool::Grab, cx)))
            .on_action(cx.listener(|v, _: &PickBubble, _, cx| v.set_tool(Tool::Bubble, cx)))
            .on_action(cx.listener(|v, _: &PickMagnify, _, cx| v.set_tool(Tool::Magnify, cx)))
            .on_action(cx.listener(|v, _: &PickRuler, _, cx| v.set_tool(Tool::Ruler, cx)))
            .on_action(cx.listener(|v, _: &PickPicker, _, cx| v.set_tool(Tool::Picker, cx)))
            .on_action(cx.listener(|v, _: &PickEmoji, _, cx| v.set_tool(Tool::Emoji, cx)))
            .on_action(cx.listener(|v, _: &PickSelect, _, cx| v.set_tool(Tool::Select, cx)))
            .on_action(cx.listener(|v, _: &PickCrop, _, cx| v.set_tool(Tool::Crop, cx)))
            .on_action(cx.listener(|v, _: &Duplicate, _, cx| v.duplicate_selected(cx)))
            .on_action(cx.listener(|v, _: &ZoomIn, _, cx| v.zoom_by(1.25, cx)))
            .on_action(cx.listener(|v, _: &ZoomOut, _, cx| v.zoom_by(1.0 / 1.25, cx)))
            .on_action(cx.listener(|v, _: &ZoomReset, _, cx| { let base = v.base_scale; v.zoom_to(base, cx) }))
            .on_scroll_wheel(cx.listener(|v, event: &gpui::ScrollWheelEvent, _, cx| v.wheel(event, cx)))
            .on_action(cx.listener(|v, _: &NudgeLeft, w, cx| v.nudge(-1.0, 0.0, w, cx)))
            .on_action(cx.listener(|v, _: &NudgeRight, w, cx| v.nudge(1.0, 0.0, w, cx)))
            .on_action(cx.listener(|v, _: &NudgeUp, w, cx| v.nudge(0.0, -1.0, w, cx)))
            .on_action(cx.listener(|v, _: &NudgeDown, w, cx| v.nudge(0.0, 1.0, w, cx)))
            .on_action(cx.listener(|v, _: &ToggleDashed, _, cx| v.set_style(|s| s.dashed = !s.dashed, cx)))
            .on_action(cx.listener(|v, _: &ToggleShadow, _, cx| v.set_style(|s| s.shadow = !s.shadow, cx)))
            .on_action(cx.listener(|v, _: &HideData, _, cx| v.hide_data(cx)))
            .on_action(cx.listener(|v, _: &CycleFill, _, cx| v.cycle_fill(cx)))
            .on_action(cx.listener(|v, _: &PickEraser, _, cx| v.set_tool(Tool::Eraser, cx)))
            .size_full()
            .relative()
            .cursor(if self.tool == Tool::Text { CursorStyle::IBeam } else { CursorStyle::Crosshair })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                    let p = view.local(event.position);
                    view.down(p, cx)
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                let p = view.local(event.position);
                view.moved(p, event.modifiers.shift, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| view.up(cx)),
            )
            .when(framed, |el| {
                el.child(div().absolute().size_full().bg(gpui::black().opacity(0.7)))
            })
            .child(
                img(self.frozen.image.clone())
                    .absolute()
                    .left(px(offset.0))
                    .top(px(offset.1))
                    .w(px(w))
                    .h(px(h)),
            )
            .when_some(layer, |el, layer| {
                el.child(
                    img(layer)
                        .absolute()
                        .left(px(offset.0))
                        .top(px(offset.1))
                        .w(px(w))
                        .h(px(h)),
                )
            })
            .when_some(drag_layer, |el, layer| {
                el.child(
                    img(layer)
                        .absolute()
                        .left(px(offset.0 + delta.0))
                        .top(px(offset.1 + delta.1))
                        .w(px(w))
                        .h(px(h)),
                )
            })
            // El recorte oscurece lo que no sale; la selección, su recuadro.
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        if let Some((x0, y0, x1, y1)) = crop {
                            let dim = gpui::black().opacity(0.55);
                            for (a, b, c, d) in [(0.0, 0.0, w, y0), (0.0, y1, w, h), (0.0, y0, x0, y1), (x1, y0, w, y1)] {
                                if c > a && d > b {
                                    let mut builder = PathBuilder::fill();
                                    let corners = [(a, b), (c, b), (c, d), (a, d)].map(|p| gpoint(p, offset));
                                    builder.add_polygon(&corners, true);
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, dim);
                                    }
                                }
                            }
                            outline(window, (x0, y0, x1, y1), offset, gpui::white().opacity(0.9));
                        }
                        if let Some(rect) = selection {
                            outline(window, rect, offset, gpui::white());
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let started = Instant::now();
                        let points: usize = shapes.iter().map(|shape| shape.points.len()).sum();
                        for shape in &shapes {
                            paint_shape(shape, offset, window);
                        }
                        if std::env::var_os("PILL_FPS").is_some() && started.elapsed().as_millis() > 8 {
                            eprintln!(
                                "pizarra: pintar {} trazos ({points} puntos) tomó {} ms",
                                shapes.len(),
                                started.elapsed().as_millis()
                            );
                        }
                        if let Some(size) = brush {
                            let ring = Shape {
                                tool: Tool::Ellipse,
                                color: 0,
                                width: 1.0,
                                points: vec![
                                    (cursor.0 - size / 2.0, cursor.1 - size / 2.0),
                                    (cursor.0 + size / 2.0, cursor.1 + size / 2.0),
                                ],
                                text: String::new(),
                                fill: Fill::Outline,
                                style: Style::default(),
                            };
                            for prim in ring.prims() {
                                if let Prim::Stroke { points, .. } = prim {
                                    let mut builder = PathBuilder::stroke(px(1.0));
                                    let gpoints: Vec<_> =
                                        points.into_iter().map(|p| gpoint(p, offset)).collect();
                                    builder.add_polygon(&gpoints, true);
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, brush_color.opacity(0.9));
                                    }
                                }
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(editor)
            .child({
                // Sobre una captura, la barra va debajo de ella (o arriba si no
                // cabe), centrada y dentro de la pantalla.
                let bar = div()
                    .absolute()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.));
                // En una pantalla angosta la barra salta a dos filas.
                let bar_w = TOOLBAR_W.min(screen_w - 16.0);
                let style = self.style_open.then(|| self.style_panel(cx));
                let export = self.export_open.then(|| self.export_panel(cx));
                let emojis = (self.tool == Tool::Emoji).then(|| self.emoji_panel(cx));
                if framed {
                    let left = (offset.0 + w / 2.0 - bar_w / 2.0)
                        .clamp(8.0, (screen_w - bar_w - 8.0).max(8.0));
                    let below = offset.1 + h + 10.0;
                    let top = if below + BAR_H + 40.0 < screen_h {
                        below
                    } else {
                        (offset.1 - BAR_H - 40.0).max(8.0)
                    };
                    bar.left(px(left))
                        .top(px(top))
                        .w(px(bar_w))
                        .child(self.toolbar(true, bar_w, cx))
                        .children(style)
                        .children(export)
                        .children(emojis)
                        .children(hint.map(chip))
                } else {
                    bar.top(px(0.))
                        .w_full()
                        .child(self.toolbar(false, bar_w, cx))
                        .children(style)
                        .children(export)
                        .children(emojis)
                        .children(hint.map(chip))
                }
            })
    }
}

// --- Emojis -----------------------------------------------------------------------

/// Los emojis a mano en el panel de sellos, después de los recientes.
pub(crate) const EMOJIS: [&str; 40] = [
    "✅", "❌", "⚠️", "❗", "❓", "💡", "⭐", "🔥", "👉", "👈", "👆", "👇", "👍", "👎", "👀", "🎯",
    "🚀", "🎉", "❤️", "😀", "😂", "😅", "🤔", "😍", "😎", "🙌", "👏", "🐛", "🔒", "🔑", "📌", "📎",
    "✏️", "🗑️", "⏰", "📅", "💬", "🔗", "1️⃣", "2️⃣",
];
/// El tamaño de un sello según el grosor elegido.
const EMOJI_SIZES: [f32; 3] = [28.0, 44.0, 72.0];

/// Las fuentes para los emojis: Segoe UI Emoji (a color) y Segoe UI de
/// respaldo. Se cargan una vez.
fn emoji_fonts() -> Option<std::sync::Arc<usvg::fontdb::Database>> {
    static DB: OnceLock<Option<std::sync::Arc<usvg::fontdb::Database>>> = OnceLock::new();
    DB.get_or_init(|| {
        let fonts = std::path::PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
        let mut db = usvg::fontdb::Database::new();
        for name in ["seguiemj.ttf", "segoeui.ttf"] {
            let _ = db.load_font_file(fonts.join(name));
        }
        (db.len() > 0).then(|| std::sync::Arc::new(db))
    })
    .clone()
}

/// El emoji pintado a color en un cuadrado de `side` píxeles.
fn emoji_pixmap(emoji: &str, side: u32) -> Option<tiny_skia::Pixmap> {
    let side = side.clamp(8, 1024);
    // Lo que se dibujó con ese emoji y tamaño ya está pintado.
    static CACHE: std::sync::Mutex<Vec<(String, u32, tiny_skia::Pixmap)>> = std::sync::Mutex::new(Vec::new());
    if let Ok(cache) = CACHE.lock() {
        if let Some((_, _, pixmap)) = cache.iter().find(|(e, s, _)| e == emoji && *s == side) {
            return Some(pixmap.clone());
        }
    }
    let size = side as f32 * 0.82;
    let escaped = emoji.replace('&', "&amp;").replace('<', "&lt;");
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}"><text x="{x}" y="{y}" font-family="Segoe UI Emoji" font-size="{size}" text-anchor="middle">{escaped}</text></svg>"#,
        x = side as f32 / 2.0,
        y = side as f32 * 0.82,
    );
    let options = usvg::Options { fontdb: emoji_fonts()?, ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(&svg, &options).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() > 64 {
            cache.remove(0);
        }
        cache.push((emoji.to_string(), side, pixmap.clone()));
    }
    Some(pixmap)
}

/// Un sello de emoji centrado en su punto, a resolución física.
fn draw_emoji(pixmap: &mut tiny_skia::Pixmap, shape: &Shape, scale: f32) {
    let Some(&center) = shape.points.first() else {
        return;
    };
    let side = (shape.width * scale).round() as u32;
    let Some(stamp) = emoji_pixmap(&shape.text, side) else {
        return;
    };
    let (x, y) = (center.0 * scale - side as f32 / 2.0, center.1 * scale - side as f32 / 2.0);
    pixmap.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        stamp.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
}

/// Los emojis usados hace poco, primero el último.
fn recent_emojis() -> Vec<String> {
    crate::paths::file("pizarra-emojis.txt")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn remember_emoji(emoji: &str) {
    let mut recent = recent_emojis();
    recent.retain(|e| e != emoji);
    recent.insert(0, emoji.to_string());
    recent.truncate(8);
    if let Some(path) = crate::paths::file("pizarra-emojis.txt") {
        let _ = std::fs::write(path, recent.join("\n"));
    }
}

// --- Exportar -------------------------------------------------------------------

/// Cómo sale lo dibujado al terminar: tamaño, formato del archivo, con la
/// foto o solo lo dibujado, y con marca de agua. El portapapeles siempre
/// recibe la imagen (PNG); el formato es el del archivo guardado.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Export {
    /// 0: 50 %, 1: 100 %, 2: 200 %.
    pub size: usize,
    pub jpeg: bool,
    /// Solo lo dibujado, sobre fondo transparente.
    pub transparent: bool,
    pub mark: bool,
    /// El marco: 0 ninguno; si no, el degradado de `BACKDROPS` (uno menos).
    pub backdrop: usize,
}

impl Default for Export {
    fn default() -> Self {
        Self { size: 1, jpeg: false, transparent: false, mark: false, backdrop: 0 }
    }
}

const EXPORT_SIZES: [(f32, &str); 3] = [(0.5, "50 %"), (1.0, "100 %"), (2.0, "200 %")];
/// Los marcos: un degradado en diagonal detrás de la captura.
const BACKDROPS: [(&str, u32, u32); 4] = [
    ("Atardecer", 0xFF7E5F, 0xFEB47B),
    ("Océano", 0x2193B0, 0x6DD5ED),
    ("Menta", 0x43CEA2, 0x185A9D),
    ("Grafito", 0x232526, 0x5A5F63),
];

/// La captura sobre un marco: degradado alrededor, esquinas redondeadas y
/// una sombra suave debajo.
fn frame_backdrop(image: &tiny_skia::Pixmap, backdrop: usize) -> Option<tiny_skia::Pixmap> {
    use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Mask, Paint, PixmapPaint, Point, SpreadMode, Transform};
    let (_, from, to) = *BACKDROPS.get(backdrop.checked_sub(1)?)?;
    let (w, h) = (image.width() as f32, image.height() as f32);
    let pad = (w.max(h) * 0.06).max(24.0).round();
    let radius = (w.min(h) * 0.02 + 8.0).min(28.0);
    let (bw, bh) = ((w + pad * 2.0) as u32, (h + pad * 2.0) as u32);
    let mut out = tiny_skia::Pixmap::new(bw, bh)?;
    let color = |c: u32| {
        let [_, r, g, b] = c.to_be_bytes();
        Color::from_rgba8(r, g, b, 255)
    };
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(bw as f32, bh as f32),
        vec![GradientStop::new(0.0, color(from)), GradientStop::new(1.0, color(to))],
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, bw as f32, bh as f32)?;
    out.fill_rect(full, &Paint { shader, anti_alias: true, ..Paint::default() }, Transform::identity(), None);
    let rounded = |x: f32, y: f32, w: f32, h: f32, r: f32| polyline(&rounded_rect(x, y, x + w, y + h, r), true);
    // La sombra: unas capas cada vez más anchas y más tenues.
    for i in (1..=6).rev() {
        let grow = i as f32 * pad * 0.05;
        let mut shade = Paint::default();
        shade.set_color_rgba8(0, 0, 0, 14);
        shade.anti_alias = true;
        if let Some(path) = rounded(pad - grow, pad - grow + pad * 0.12, w + grow * 2.0, h + grow * 2.0, radius + grow) {
            out.fill_path(&path, &shade, FillRule::Winding, Transform::identity(), None);
        }
    }
    // La captura, recortada con las esquinas redondeadas.
    let mut mask = Mask::new(bw, bh)?;
    if let Some(path) = rounded(pad, pad, w, h, radius) {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    }
    out.draw_pixmap(pad as i32, pad as i32, image.as_ref(), &PixmapPaint::default(), Transform::identity(), Some(&mask));
    Some(out)
}
const JPEG_QUALITY: u8 = 88;

impl Export {
    fn file() -> Option<std::path::PathBuf> {
        crate::paths::file("pizarra-exportar.txt")
    }

    /// `100 png photo nomark`, como lo guarda `save`.
    fn parse(text: &str) -> Self {
        let mut export = Self::default();
        for word in text.split_whitespace() {
            match word {
                "50" => export.size = 0,
                "100" => export.size = 1,
                "200" => export.size = 2,
                "jpg" => export.jpeg = true,
                "png" => export.jpeg = false,
                "transparent" => export.transparent = true,
                "photo" => export.transparent = false,
                "mark" => export.mark = true,
                "nomark" => export.mark = false,
                bg if bg.starts_with("bg") => {
                    export.backdrop = bg[2..].parse::<usize>().unwrap_or(0).min(BACKDROPS.len());
                }
                _ => {}
            }
        }
        export
    }

    fn text(&self) -> String {
        format!(
            "{} {} {} {} bg{}",
            ["50", "100", "200"][self.size.min(2)],
            if self.jpeg { "jpg" } else { "png" },
            if self.transparent { "transparent" } else { "photo" },
            if self.mark { "mark" } else { "nomark" },
            self.backdrop,
        )
    }

    pub fn load() -> Self {
        Self::file().and_then(|path| std::fs::read_to_string(path).ok()).map(|text| Self::parse(&text)).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::file() {
            let _ = std::fs::write(path, self.text());
        }
    }
}

/// El texto de la marca de agua: `pizarra-marca.txt` si existe; si no, el
/// usuario de Windows.
fn mark_text() -> String {
    crate::paths::file("pizarra-marca.txt")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "Atic".to_string())
}

/// La marca de agua abajo a la derecha: blanca, con un borde oscuro que la
/// deja leer sobre cualquier foto.
fn draw_watermark(pixmap: &mut tiny_skia::Pixmap, text: &str) {
    let Some(face) = font() else {
        return;
    };
    let size = (pixmap.height() as f32 * 0.028).clamp(12.0, 44.0);
    let (ascent, descent) = metrics(face, size);
    let pad = size * 0.7;
    let x = pixmap.width() as f32 - pad - text_width(text, size);
    let baseline = pixmap.height() as f32 - pad - descent;
    let _ = ascent;
    let mut path = tiny_skia::PathBuilder::new();
    glyph_line(&mut path, face, text, x, baseline, size);
    let Some(path) = path.finish() else {
        return;
    };
    let mut edge = tiny_skia::Paint::default();
    edge.set_color_rgba8(0, 0, 0, 150);
    edge.anti_alias = true;
    let stroke = tiny_skia::Stroke { width: (size * 0.12).max(1.5), line_join: tiny_skia::LineJoin::Round, ..tiny_skia::Stroke::default() };
    pixmap.stroke_path(&path, &edge, &stroke, tiny_skia::Transform::identity(), None);
    let mut ink = tiny_skia::Paint::default();
    ink.set_color_rgba8(255, 255, 255, 220);
    ink.anti_alias = true;
    pixmap.fill_path(&path, &ink, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
}

/// La imagen final según `opts`, en BGRA con alfa directo.
pub fn export_frame(
    photo: &Frame,
    shapes: &[Shape],
    scale: f32,
    opts: Export,
    crop: Option<(f32, f32, f32, f32)>,
) -> Result<Frame, String> {
    let (w, h) = (photo.width(), photo.height());
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("tamaño inválido")?;
    if !opts.transparent {
        for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(photo.bgra.chunks_exact(4)) {
            dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
        }
    }
    draw_shapes(&mut pixmap, shapes, scale, photo);
    // El recorte, antes de la marca y del marco (que van alrededor de él).
    if let Some((x0, y0, x1, y1)) = crop {
        let clamp = |v: f32, max: u32| (v * scale).round().clamp(0.0, max as f32) as i32;
        let (x0, y0, x1, y1) = (clamp(x0, w), clamp(y0, h), clamp(x1, w), clamp(y1, h));
        if x1 - x0 >= 2 && y1 - y0 >= 2 {
            let mut cut = tiny_skia::Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32).ok_or("recorte inválido")?;
            cut.draw_pixmap(-x0, -y0, pixmap.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
            pixmap = cut;
        }
    }
    if opts.mark {
        draw_watermark(&mut pixmap, &mark_text());
    }
    // El marco va alrededor, así que agranda la imagen.
    if let Some(framed) = frame_backdrop(&pixmap, opts.backdrop) {
        pixmap = framed;
    }
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let factor = EXPORT_SIZES[opts.size.min(2)].0;
    let (rgba, w, h) = if (factor - 1.0).abs() > f32::EPSILON {
        let image = image::RgbaImage::from_raw(w, h, rgba).ok_or("imagen inválida")?;
        let (nw, nh) = (((w as f32 * factor).round() as u32).max(1), ((h as f32 * factor).round() as u32).max(1));
        let resized = image::imageops::resize(&image, nw, nh, image::imageops::FilterType::Lanczos3);
        (resized.into_raw(), nw, nh)
    } else {
        (rgba, w, h)
    };
    let mut bgra = rgba;
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let bounds = atic_capture::Rect { x: photo.bounds.x, y: photo.bounds.y, width: w, height: h };
    Ok(Frame::new(bounds, bgra))
}

/// Guarda la imagen exportada (PNG o JPG) en las capturas y la deja en el
/// portapapeles. Corre en un hilo de fondo.
pub fn save_export(frame: &Frame, opts: Export) -> Result<Saved, String> {
    let png = frame.to_png().map_err(|e| e.to_string())?;
    let dir = crate::paths::captures_dir().ok_or("sin carpeta de capturas")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut name = atic_capture::naming::unique_capture_filename(&dir);
    let bytes = if opts.jpeg {
        name = std::path::Path::new(&name).with_extension("jpg").to_string_lossy().into_owned();
        frame.to_jpeg(JPEG_QUALITY).map_err(|e| e.to_string())?
    } else {
        png.clone()
    };
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    crate::clip_image::write(frame.width(), frame.height(), &frame.bgra, &png)?;
    Ok(Saved { path, png, width: frame.width(), height: frame.height(), frame: frame.clone() })
}

// --- Seguir editando después ----------------------------------------------------
//
// Al terminar, junto a la captura quedan la foto sin marcas y las figuras
// (`pizarra-ediciones/<captura>/`). Si esa captura se vuelve a abrir en la
// pizarra, vuelve limpia y con las figuras editables: se pueden borrar,
// cambiar o agregar, y al terminar sale una captura nueva.

/// Todo lo que cambia de tamaño con el zoom de la pizarra, por `factor`:
/// puntos y grosor. Las figuras se guardan en píxeles de la foto.
fn scale_shapes(shapes: &[Shape], factor: f32) -> Vec<Shape> {
    shapes
        .iter()
        .map(|shape| Shape {
            points: shape.points.iter().map(|&(x, y)| (x * factor, y * factor)).collect(),
            width: shape.width * factor,
            ..shape.clone()
        })
        .collect()
}

fn edits_dir() -> Option<std::path::PathBuf> {
    crate::paths::file("pizarra-ediciones")
}

fn edit_dir(capture: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(edits_dir()?.join(capture.file_name()?))
}

/// Guarda la foto sin marcas y las figuras de la captura `capture`. De paso
/// borra las ediciones de capturas que ya no están.
pub fn save_edit(capture: &std::path::Path, photo: &Frame, shapes: &[Shape], scale: f32) -> Result<(), String> {
    if shapes.is_empty() {
        return Ok(());
    }
    let dir = edit_dir(capture).ok_or("sin carpeta de datos")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let png = photo.to_png().map_err(|e| e.to_string())?;
    std::fs::write(dir.join("foto.png"), png).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(&scale_shapes(shapes, scale)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("figuras.json"), json).map_err(|e| e.to_string())?;
    if let (Some(edits), Some(captures)) = (edits_dir(), capture.parent()) {
        for entry in std::fs::read_dir(edits).into_iter().flatten().flatten() {
            if !captures.join(entry.file_name()).exists() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(())
}

/// La foto sin marcas y las figuras (en píxeles de la foto) de `capture`, si
/// se dibujó en la pizarra.
pub fn load_edit(capture: &std::path::Path) -> Option<(Frame, Vec<Shape>)> {
    let dir = edit_dir(capture)?;
    let shapes: Vec<Shape> = serde_json::from_str(&std::fs::read_to_string(dir.join("figuras.json")).ok()?).ok()?;
    let image = image::open(dir.join("foto.png")).ok()?;
    let mut bgra = image.to_rgba8().into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let bounds = atic_capture::Rect { x: 0, y: 0, width: image.width(), height: image.height() };
    Some((Frame::new(bounds, bgra), shapes))
}

/// Las figuras para la próxima pizarra que se abra (en píxeles de la foto):
/// `open_board` las toma y las lleva a su zoom.
static PENDING: std::sync::Mutex<Option<Vec<Shape>>> = std::sync::Mutex::new(None);

impl BoardView {
    /// Carga figuras guardadas (en píxeles de la foto) a esta pizarra.
    fn restore(&mut self, shapes: Vec<Shape>, cx: &mut Context<Self>) {
        self.shapes = scale_shapes(&shapes, 1.0 / self.frozen.scale);
        self.rebuild_layer();
        self.notify_user("Sigues donde quedaste: las figuras se pueden borrar o cambiar", cx);
    }
}

// --- En la pill ------------------------------------------------------------

impl crate::Pill {
    pub(crate) fn start_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.is_some() || self.capture.is_some() || self.capture_pending {
            return;
        }
        self.capture_pending = true;
        let now = Instant::now();
        self.close_wheel();
        self.close_panel(false, cx);
        self.peek.set(0.0, now);
        self.strip.set(0.0, now);
        let previous = crate::paste::foreground_target();
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(true);
        }
        let scale = self.scale_factor;
        let Some(area) = self.pill_area() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let frozen = cx.background_spawn(async move { capture::freeze(scale, area) }).await;
            let _ = this.update_in(cx, |pill, window, cx| {
                pill.capture_pending = false;
                if let Some(overlay) = pill.overlay.as_ref() {
                    overlay.exclude_from_capture(false);
                }
                let mut frozen = match frozen {
                    Ok(frozen) => frozen,
                    Err(error) => {
                        eprintln!("pizarra: no se pudo congelar: {error}");
                        return;
                    }
                };
                frozen.offset = (pill.monitor.x, pill.monitor.y);
                println!("pizarra: congelada en {} ms", frozen.took.as_millis());
                pill.open_board(frozen, (0.0, 0.0), previous, window, cx);
            });
        })
        .detach();
    }

    /// Abre la pizarra sobre una imagen ya tomada, puesta en `origin`
    /// (lógico, dentro del monitor): «Dibujar» del estante.
    pub(crate) fn open_board(
        &mut self,
        frozen: Frozen,
        origin: (f32, f32),
        previous: Option<crate::paste::Target>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self
            .cursor
            .map(|(x, y)| (x - frozen.offset.0 - origin.0, y - frozen.offset.1 - origin.1))
            .unwrap_or_default();
        let prefs = Prefs::load();
        let pending = PENDING.lock().ok().and_then(|mut slot| slot.take());
        let view = cx.new(|cx| {
            let mut view = BoardView::new(frozen, origin, cursor, previous, prefs, cx);
            if let Some(shapes) = pending {
                view.restore(shapes, cx);
            }
            view
        });
        self.board_events = Some(cx.subscribe_in(&view, window, |pill, _, event: &BoardEvent, window, cx| {
            pill.end_board(event, window, cx)
        }));
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_passthrough(false);
            overlay.set_focusable(true);
        }
        window.focus(&view.focus_handle(cx));
        self.board = Some(view);
        cx.notify();
    }

    /// Una imagen del historial, en la pizarra: centrada en el monitor y
    /// achicada si no cabe. Al terminar se copia y se guarda como una captura.
    pub(crate) fn draw_entry(&mut self, entry: &crate::clipboard::Entry, window: &mut Window, cx: &mut Context<Self>) {
        let crate::clipboard::Content::Image(picture) = &entry.content else {
            return;
        };
        if self.board.is_some() || self.capture.is_some() {
            return;
        }
        let decoded = picture
            .load()
            .map_err(|error| error.to_string())
            .and_then(|image| image::load_from_memory(&image.bytes).map_err(|error| error.to_string()));
        self.draw_image(decoded, window, cx);
    }

    /// Una imagen guardada (la última captura, desde el vistazo de la Pizarra).
    pub(crate) fn draw_file(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.is_some() || self.capture.is_some() {
            return;
        }
        if self.reopen_edit(path, window, cx) {
            return;
        }
        self.draw_image(image::open(path).map_err(|error| error.to_string()), window, cx);
    }

    /// Si `capture` salió de la pizarra, la abre limpia y con sus figuras
    /// editables. `false` si no tiene edición guardada.
    pub(crate) fn reopen_edit(&mut self, capture: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((photo, shapes)) = load_edit(capture) else {
            return false;
        };
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(shapes);
        }
        let previous = crate::paste::foreground_target();
        self.close_panel(false, cx);
        self.open_board_centered(photo, previous, window, cx);
        // Si no llegó a abrirse, que no queden para la próxima.
        if let Ok(mut slot) = PENDING.lock() {
            slot.take();
        }
        true
    }

    fn draw_image(
        &mut self,
        decoded: Result<image::DynamicImage, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let image = match decoded {
            Ok(image) => image,
            Err(error) => {
                eprintln!("pizarra: no se pudo abrir la imagen: {error}");
                return;
            }
        };
        let mut bgra = image.to_rgba8().into_raw();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let bounds = atic_capture::Rect {
            x: 0,
            y: 0,
            width: image.width(),
            height: image.height(),
        };
        let frame = atic_capture::Frame::new(bounds, bgra);
        let previous = crate::paste::foreground_target();
        self.close_panel(false, cx);
        self.open_board_centered(frame, previous, window, cx);
    }

    /// Dibujar sobre una imagen suelta (una captura, una del estante o del
    /// portapapeles): centrada en el monitor de la pill sobre fondo oscuro, y
    /// agrandada o achicada para que se dibuje cómodo. El zoom es solo de la
    /// vista: lo dibujado se compone a la resolución de la imagen.
    pub(crate) fn open_board_centered(
        &mut self,
        frame: atic_capture::Frame,
        previous: Option<crate::paste::Target>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        /// Lo que puede ocupar del monitor, y cuánto se agranda una chica.
        const FILL: f32 = 0.8;
        const MAX_ZOOM: f32 = 3.0;
        let scale = self.scale_factor;
        // El monitor de la pill en la ventana de ahora: tras la mira puede
        // seguir estirada a todas las pantallas.
        let monitor = match (self.overlay.as_ref(), self.screen.as_ref()) {
            (Some(overlay), Some((screen, _, _))) => overlay.to_logical(&screen.monitor, scale).unwrap_or(self.monitor),
            _ => self.monitor,
        };
        let (w, h) = (frame.width() as f32 / scale, frame.height() as f32 / scale);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let zoom = (monitor.w * FILL / w).min(monitor.h * FILL / h).min(MAX_ZOOM);
        // La pizarra muestra la imagen a `frame / view_scale` px lógicos.
        let view_scale = scale / zoom;
        let (shown_w, shown_h) = (w * zoom, h * zoom);
        // Nunca (0, 0): ese origen es el de la pantalla entera, sin fondo oscuro.
        let origin = (
            ((monitor.w - shown_w) / 2.0).max(1.0),
            ((monitor.h - shown_h) / 2.0).max(1.0),
        );
        let Some(frozen) = frozen_for(frame, view_scale, (monitor.x, monitor.y)) else {
            return;
        };
        self.open_board(frozen, origin, previous, window, cx);
        cx.notify();
    }

    fn end_board(&mut self, event: &BoardEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.board.take() else {
            return;
        };
        self.board_events = None;
        // Si venía de la mira con todas las pantallas, la ventana vuelve.
        self.end_span();
        let previous = view.read(cx).previous;
        // La próxima pizarra empieza con la herramienta y el color de esta.
        view.read(cx).prefs().save();
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(false);
        }
        if let Some(target) = previous {
            crate::paste::force_foreground(target);
        }
        if let BoardEvent::Done {
            frame,
            shapes,
            scale,
            send,
            crop,
        } = event
        {
            let send = *send;
            let crop = *crop;
            let export = view.read(cx).export;
            export.save();
            // El resultado vuela al estante desde donde estaba la imagen.
            let board = view.read(cx);
            let from = crate::geometry::Rect::new(
                board.frozen.offset.0 + board.origin.0,
                board.frozen.offset.1 + board.origin.1,
                frame.width() as f32 / scale,
                frame.height() as f32 / scale,
            );
            let (frame, shapes, scale) = (frame.clone(), shapes.clone(), *scale);
            cx.spawn_in(window, async move |this, cx| {
                let started = Instant::now();
                let saved: Result<Saved, String> = cx
                    .background_spawn(async move {
                        let exported = export_frame(&frame, &shapes, scale, export, crop)?;
                        let saved = save_export(&exported, export)?;
                        // Para seguir editando después.
                        if let Err(error) = save_edit(&saved.path, &frame, &shapes, scale) {
                            eprintln!("pizarra: no se guardó la edición: {error}");
                        }
                        Ok(saved)
                    })
                    .await;
                let sent = saved.as_ref().ok().map(|s| (s.png.clone(), s.path.clone()));
                let _ = this.update_in(cx, |pill, window, cx| {
                    pill.capture_saved(saved, started, from, cx);
                    let Some((png, path)) = sent else {
                        return;
                    };
                    match send {
                        // A Flip solo con Flip cerrado: abierto, lo suyo en memoria
                        // pisaría la página nueva al guardar.
                        Some(SendTo::Flip) if pill.flip.is_none() => match crate::flip_board::append_image_page(&png) {
                            Some(page) => {
                                crate::flip_board::OPEN_PAGE.store(page, std::sync::atomic::Ordering::Relaxed);
                                pill.start_flip(window, cx);
                            }
                            None => eprintln!("pizarra: no se pudo poner en Flip"),
                        },
                        Some(SendTo::Flip) => eprintln!("pizarra: Flip está abierto; la captura quedó copiada"),
                        Some(SendTo::Agent) => match crate::space::send_to_latest_agent(cx, &agent_prompt(&path)) {
                            Some(name) => println!("pizarra: enviada al agente {name}"),
                            None => eprintln!("pizarra: no quedaban agentes; la captura quedó copiada"),
                        },
                        None => {}
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suavizar_conserva_los_extremos() {
        let points = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let smoothed = smooth(&points);
        assert_eq!(smoothed.first(), Some(&(0.0, 0.0)));
        assert_eq!(smoothed.last(), Some(&(10.0, 10.0)));
        assert!(smoothed.len() > points.len());
        // La esquina queda redondeada: ningún punto cae justo en ella.
        assert!(!smoothed.contains(&(10.0, 0.0)));
    }

    #[test]
    fn shift_endereza() {
        let (x, y) = constrain(Tool::Arrow, (0.0, 0.0), (10.0, 9.0));
        assert!((x - y).abs() < 0.01, "45°: {x}, {y}");
        assert_eq!(constrain(Tool::Rect, (0.0, 0.0), (10.0, -4.0)), (10.0, -10.0));
    }

    #[test]
    fn el_borrador_toca_el_trazo() {
        let line = Shape {
            tool: Tool::Pen,
            color: 0,
            width: 4.0,
            points: vec![(0.0, 0.0), (100.0, 0.0)],
            text: String::new(),
            fill: Fill::Outline,
            style: Style::default(),
        };
        assert!(line.hit((50.0, 3.0), 2.0));
        assert!(!line.hit((50.0, 20.0), 2.0));
    }

    #[test]
    fn exporta_los_trazos_sobre_la_foto() {
        let frame = Frame::new(atic_capture::Rect::new(0, 0, 20, 20), vec![255; 20 * 20 * 4]);
        let shapes = vec![Shape {
            tool: Tool::Rect,
            color: 0xFF0000,
            width: 2.0,
            points: vec![(2.0, 2.0), (8.0, 8.0)],
            text: String::new(),
            fill: Fill::Outline,
            style: Style::default(),
        }];
        // Al 200 %: el borde del rectángulo cae en (4, 4) físico.
        let out = compose(&frame, &shapes, 2.0).unwrap();
        let px = out.pixel_rgba(4, 10).unwrap();
        assert!(px[0] > 200 && px[1] < 80 && px[2] < 80, "rojo: {px:?}");
        // Lo que no se tocó sigue blanco.
        assert_eq!(out.pixel_rgba(10, 10).unwrap(), [255, 255, 255, 255]);
    }

    /// Cada letra mide 10: así el ajuste no depende de la fuente instalada.
    fn tens(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn el_texto_salta_de_linea_en_los_espacios() {
        assert_eq!(wrap_lines("hola mundo", 100.0, tens), vec!["hola mundo"]);
        assert_eq!(wrap_lines("hola mundo", 60.0, tens), vec!["hola", "mundo"]);
        // Los saltos escritos se respetan, también los vacíos.
        assert_eq!(wrap_lines("a\n\nb", 100.0, tens), vec!["a", "", "b"]);
    }

    #[test]
    fn una_palabra_larga_se_corta() {
        assert_eq!(wrap_lines("abcdefgh", 30.0, tens), vec!["abc", "def", "gh"]);
        // También si viene después de otra.
        assert_eq!(wrap_lines("ab abcdefgh", 30.0, tens), vec!["ab", "abc", "def", "gh"]);
    }

    #[test]
    fn la_pizarra_recuerda_herramienta_color_y_grosor() {
        for (tool, _) in TOOL_NAMES.iter().filter(|(tool, _)| !matches!(tool, Tool::Eraser | Tool::Picker)) {
            let prefs = Prefs { tool: *tool, color: 0x3E8BFF, width: 2, fill: Fill::Soft, style: Style::default() };
            assert_eq!(Prefs::parse(&prefs.text()), Some(prefs));
        }
        // El estilo también se recuerda.
        let styled = Prefs {
            tool: Tool::Text,
            color: 0xE5484D,
            width: 0,
            fill: Fill::Solid,
            style: Style { dashed: true, shadow: true, font: Font::Hand },
        };
        assert_eq!(Prefs::parse(&styled.text()), Some(styled));
        // El borrador no se recuerda: se vuelve al lápiz.
        let eraser = Prefs { tool: Tool::Eraser, color: 0x3E8BFF, width: 2, fill: Fill::Solid, style: Style::default() };
        assert_eq!(Prefs::parse(&eraser.text()).map(|p| p.tool), Some(Tool::Pen));
        // Lo que guardaba la versión anterior, sin relleno, sigue sirviendo.
        assert_eq!(Prefs::parse("text e5484d 1").map(|p| p.fill), Some(Fill::Outline));
        assert_eq!(Prefs::parse("basura"), None);
    }

    fn shape(tool: Tool, points: Vec<(f32, f32)>, fill: Fill) -> Shape {
        Shape { tool, color: 0xE5484D, width: 4.0, points, text: String::new(), fill, style: Style::default() }
    }

    #[test]
    fn la_flecha_curva_pasa_por_donde_se_arrastro() {
        let drag = vec![(0.0, 0.0), (50.0, -40.0), (100.0, 0.0)];
        let curve = shape(Tool::Curve, drag, Fill::Outline);
        let Prim::Stroke { points, .. } = &curve.prims()[0] else {
            panic!("el asta es un trazo");
        };
        // A mitad de camino pasa por el punto más alejado del arrastre.
        assert!(points.iter().any(|p| distance(*p, (50.0, -40.0)) < 2.0));
        // Arrastre recto: se dobla igual, para que se vea curva.
        let straight = shape(Tool::Curve, vec![(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)], Fill::Outline);
        assert!(segment_distance(curve_through(&straight.points), (0.0, 0.0), (100.0, 0.0)) > 10.0);
    }

    #[test]
    fn rellenos_de_las_figuras() {
        let corners = vec![(0.0, 0.0), (40.0, 30.0)];
        assert!(matches!(shape(Tool::Rect, corners.clone(), Fill::Outline).prims()[..], [Prim::Stroke { .. }]));
        assert!(matches!(shape(Tool::Rect, corners.clone(), Fill::Solid).prims()[..], [Prim::Fill(_, a)] if a == 1.0));
        assert!(matches!(
            shape(Tool::Ellipse, corners.clone(), Fill::Soft).prims()[..],
            [Prim::Fill(_, a), Prim::Stroke { .. }] if a == SOFT_ALPHA
        ));
        // Relleno, el borrador lo toca por dentro; solo borde, no.
        assert!(shape(Tool::Rect, corners.clone(), Fill::Solid).hit((20.0, 15.0), 2.0));
        assert!(!shape(Tool::Rect, corners, Fill::Outline).hit((20.0, 15.0), 2.0));
    }

    /// Una foto de 40×40: mitad izquierda negra, mitad derecha blanca.
    fn half_photo() -> Frame {
        let mut bgra = Vec::with_capacity(40 * 40 * 4);
        for _ in 0..40 {
            for x in 0..40 {
                let v = if x < 20 { 0 } else { 255 };
                bgra.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Frame::new(atic_capture::Rect::new(0, 0, 40, 40), bgra)
    }

    #[test]
    fn las_figuras_se_guardan_y_vuelven_con_otro_zoom() {
        let shapes = vec![
            shape(Tool::Curve, vec![(10.0, 10.0), (30.0, 5.0), (50.0, 10.0)], Fill::Outline),
            Shape { tool: Tool::Text, color: 0x3E8BFF, width: 26.0, points: vec![(5.0, 40.0), (200.0, 40.0)], text: "Hola".into(), fill: Fill::Solid, style: Style::default() },
            Shape { tool: Tool::Step, color: 0xFFD60A, width: 14.0, points: vec![(80.0, 80.0)], text: "3".into(), fill: Fill::Solid, style: Style::default() },
        ];
        // Se dibujó al 150 %: se guarda en píxeles de la foto.
        let json = serde_json::to_string(&scale_shapes(&shapes, 1.5)).unwrap();
        let saved: Vec<Shape> = serde_json::from_str(&json).unwrap();
        assert_eq!(saved[1].points[0], (7.5, 60.0));
        assert_eq!(saved[1].width, 39.0);
        // Se reabre al 150 % y queda igual que antes.
        assert_eq!(scale_shapes(&saved, 1.0 / 1.5), shapes);
    }

    #[test]
    fn la_lupa_agranda_lo_de_abajo() {
        // Foto: una franja negra vertical de 2 px en x = 20..22, el resto blanco.
        let mut bgra = Vec::with_capacity(40 * 40 * 4);
        for _ in 0..40 {
            for x in 0..40 {
                let v = if (20..22).contains(&x) { 0 } else { 255 };
                bgra.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let photo = Frame::new(atic_capture::Rect::new(0, 0, 40, 40), bgra);
        let lens = shape(Tool::Magnify, vec![(21.0, 20.0), (35.0, 20.0)], Fill::Outline);
        let out = compose(&photo, &[lens.clone()], 1.0).unwrap();
        // Al doble, la franja (20 y 21) se ensancha: del 19 al 21 queda negro, y
        // el 24, que sin lupa no tocaría, sigue blanco.
        let black = (17..=25).filter(|&x| out.pixel_rgba(x, 20).unwrap()[0] < 60).count();
        assert!(black >= 3, "franja agrandada: {black} px");
        assert!(out.pixel_rgba(19, 20).unwrap()[0] < 60);
        assert!(out.pixel_rgba(24, 20).unwrap()[0] > 200);
        // Fuera de la lupa sigue blanco.
        assert_eq!(out.pixel_rgba(19, 39).unwrap(), [255, 255, 255, 255]);
        assert!(lens.hit((25.0, 20.0), 0.0));
    }

    #[test]
    fn medir_da_el_largo_en_pixeles_de_la_foto() {
        assert_eq!(ruler_label((0.0, 0.0), (30.0, 40.0), 1.0), "50 px");
        // Al 150 %, lo que se ve como 50 son 75 de la foto.
        assert_eq!(ruler_label((0.0, 0.0), (30.0, 40.0), 1.5), "75 px");
    }

    #[test]
    fn el_globo_tiene_colita_hacia_lo_que_senala() {
        let bubble = Shape {
            tool: Tool::Text,
            color: 0x3E8BFF,
            width: 20.0,
            points: vec![(100.0, 100.0), (300.0, 100.0), (20.0, 20.0)],
            text: "Aquí".into(),
            fill: Fill::Solid,
            style: Style::default(),
        };
        let tail = bubble_tail(&bubble).expect("colita");
        assert_eq!(tail[1], (20.0, 20.0));
        // Sin tercer punto no hay colita.
        let plain = Shape { points: vec![(100.0, 100.0), (300.0, 100.0)], ..bubble };
        assert!(bubble_tail(&plain).is_none());
    }

    #[test]
    fn punteado_deja_huecos_y_la_sombra_se_ve() {
        let photo = Frame::new(atic_capture::Rect::new(0, 0, 100, 20), vec![255; 100 * 20 * 4]);
        let mut line = shape(Tool::Line, vec![(5.0, 10.0), (95.0, 10.0)], Fill::Outline);
        line.style.dashed = true;
        let out = compose(&photo, &[line.clone()], 1.0).unwrap();
        let white = (5..95).filter(|&x| out.pixel_rgba(x, 10).unwrap()[1] > 200).count();
        assert!(white > 10, "huecos del punteado: {white}");
        // Con sombra: debajo de la línea (corrida 3 px) queda gris.
        line.style = Style { shadow: true, ..Style::default() };
        let out = compose(&photo, &[line], 1.0).unwrap();
        let under = out.pixel_rgba(50, 14).unwrap();
        assert!(under[1] < 230, "sombra: {under:?}");
    }

    #[test]
    fn las_letras_tienen_su_fuente_o_la_normal() {
        if font().is_none() {
            return;
        }
        for kind in Font::ALL {
            assert!(font_of(kind).is_some(), "{kind:?}");
        }
        // Mono: todas las letras miden lo mismo.
        assert!((text_width_in("iii", 20.0, Font::Mono) - text_width_in("mmm", 20.0, Font::Mono)).abs() < 0.5);
    }

    #[test]
    fn el_emoji_sale_a_color() {
        if emoji_fonts().is_none() {
            return;
        }
        let stamp = emoji_pixmap("🔥", 64).expect("emoji pintado");
        // Tiene píxeles visibles y de color (no solo gris).
        let colored = stamp
            .pixels()
            .iter()
            .filter(|p| p.alpha() > 200 && (p.red() as i32 - p.blue() as i32).abs() > 60)
            .count();
        assert!(colored > 100, "píxeles de color: {colored}");
    }

    #[test]
    fn exportar_cambia_tamano_fondo_y_marca() {
        let photo = Frame::new(atic_capture::Rect::new(0, 0, 200, 100), vec![200; 200 * 100 * 4]);
        let line = vec![shape(Tool::Line, vec![(10.0, 50.0), (190.0, 50.0)], Fill::Outline)];
        // Al 50 %: la mitad de cada lado.
        let half = export_frame(&photo, &line, 1.0, Export { size: 0, ..Export::default() }, None).unwrap();
        assert_eq!((half.width(), half.height()), (100, 50));
        // Solo lo dibujado: la foto queda transparente y la línea no.
        let clear = export_frame(&photo, &line, 1.0, Export { transparent: true, ..Export::default() }, None).unwrap();
        assert_eq!(clear.pixel_rgba(100, 10).unwrap()[3], 0);
        assert_eq!(clear.pixel_rgba(100, 50).unwrap()[3], 255);
        // La marca de agua deja algo abajo a la derecha.
        if font().is_some() {
            let plain = export_frame(&photo, &[], 1.0, Export::default(), None).unwrap();
            let marked = export_frame(&photo, &[], 1.0, Export { mark: true, ..Export::default() }, None).unwrap();
            let changed = (100..200)
                .flat_map(|x| (70..100).map(move |y| (x, y)))
                .filter(|&(x, y)| plain.pixel_rgba(x, y) != marked.pixel_rgba(x, y))
                .count();
            assert!(changed > 20, "marca: {changed} píxeles");
        }
    }

    #[test]
    fn mover_corre_la_figura_y_su_recuadro() {
        let rect = shape(Tool::Rect, vec![(10.0, 10.0), (50.0, 40.0)], Fill::Outline);
        let (x0, y0, x1, y1) = rect.bounds();
        // El recuadro abarca el rectángulo y la mitad del trazo.
        assert!(x0 <= 8.0 && y0 <= 8.0 && x1 >= 52.0 && y1 >= 42.0);
        let moved = rect.moved(5.0, -3.0);
        assert_eq!(moved.points, vec![(15.0, 7.0), (55.0, 37.0)]);
        let (mx0, my0, ..) = moved.bounds();
        assert!((mx0 - (x0 + 5.0)).abs() < 0.01 && (my0 - (y0 - 3.0)).abs() < 0.01);
        // Un emoji: su cuadro, centrado.
        let mut stamp = shape(Tool::Emoji, vec![(100.0, 100.0)], Fill::Outline);
        stamp.width = 40.0;
        assert_eq!(stamp.bounds(), (80.0, 80.0, 120.0, 120.0));
    }

    #[test]
    fn el_zoom_deja_quieto_lo_del_cursor() {
        let (origin, focus) = ((100.0, 50.0), (40.0, 30.0));
        // El punto se ve en origen + punto, antes y después del zoom al doble.
        let before = (origin.0 + focus.0, origin.1 + focus.1);
        let next = zoom_origin(origin, focus, 2.0);
        let after = (next.0 + focus.0 * 2.0, next.1 + focus.1 * 2.0);
        assert_eq!(before, after);
        // Nunca cae en (0, 0), que es la pantalla entera.
        assert_ne!(zoom_origin((10.0, 10.0), (10.0, 10.0), 2.0), (0.0, 0.0));
    }

    #[test]
    fn el_recorte_deja_solo_esa_parte() {
        let photo = half_photo();
        // Del negro de la izquierda al blanco de la derecha: 10..30 × 5..15.
        let cut = export_frame(&photo, &[], 1.0, Export::default(), Some((10.0, 5.0, 30.0, 15.0))).unwrap();
        assert_eq!((cut.width(), cut.height()), (20, 10));
        assert_eq!(cut.pixel_rgba(2, 2).unwrap(), [0, 0, 0, 255]);
        assert_eq!(cut.pixel_rgba(18, 2).unwrap(), [255, 255, 255, 255]);
        // Al 150 %: el recorte se mide en la pizarra y se corta en la foto.
        let cut = export_frame(&photo, &[], 1.5, Export::default(), Some((0.0, 0.0, 10.0, 10.0))).unwrap();
        assert_eq!((cut.width(), cut.height()), (15, 15));
    }

    #[test]
    fn el_marco_agranda_y_redondea() {
        let photo = Frame::new(atic_capture::Rect::new(0, 0, 400, 200), vec![255; 400 * 200 * 4]);
        let framed = export_frame(&photo, &[], 1.0, Export { backdrop: 2, ..Export::default() }, None).unwrap();
        // 6 % del lado mayor a cada lado: 24 px.
        assert_eq!((framed.width(), framed.height()), (448, 248));
        // La esquina es degradado (Océano, azul), no la foto blanca.
        let corner = framed.pixel_rgba(1, 1).unwrap();
        assert!(corner[2] > corner[0] + 60, "degradado: {corner:?}");
        // El centro sigue siendo la foto.
        assert_eq!(framed.pixel_rgba(224, 124).unwrap(), [255, 255, 255, 255]);
        // La esquina de la foto queda redondeada: ahí se ve el marco.
        let rounded = framed.pixel_rgba(24, 24).unwrap();
        assert!(rounded != [255, 255, 255, 255], "esquina redondeada: {rounded:?}");
    }

    #[test]
    fn exportar_recuerda_sus_opciones() {
        for export in [
            Export::default(),
            Export { size: 0, jpeg: true, transparent: true, mark: true, backdrop: 0 },
            Export { size: 2, jpeg: false, transparent: false, mark: true, backdrop: 3 },
        ] {
            assert_eq!(Export::parse(&export.text()), export);
        }
        assert_eq!(Export::parse(""), Export::default());
    }

    #[test]
    fn al_agente_va_la_ruta_y_la_pregunta() {
        let plain = agent_prompt(std::path::Path::new(r"C:\datos\Atic_1.png"));
        assert!(plain.starts_with(r"C:\datos\Atic_1.png "));
        // Con espacios, entre comillas para que el agente la lea entera.
        let spaced = agent_prompt(std::path::Path::new(r"C:\Mis datos\Atic_1.png"));
        assert!(spaced.starts_with(r#""C:\Mis datos\Atic_1.png" "#));
    }

    #[test]
    fn pixelar_promedia_por_bloques() {
        let photo = half_photo();
        // Un bloque de 12 que cruza el borde negro/blanco queda gris.
        let pixelate = shape(Tool::Pixelate, vec![(14.0, 0.0), (26.0, 12.0)], Fill::Outline);
        let out = compose(&photo, &[pixelate], 1.0).unwrap();
        let px = out.pixel_rgba(20, 5).unwrap();
        assert!(px[0] > 100 && px[0] < 160, "gris: {px:?}");
        // Fuera del recuadro la foto no se toca.
        assert_eq!(out.pixel_rgba(5, 30).unwrap(), [0, 0, 0, 255]);
        assert_eq!(out.pixel_rgba(35, 30).unwrap(), [255, 255, 255, 255]);
    }

    #[test]
    fn el_foco_oscurece_lo_de_afuera() {
        let photo = half_photo();
        let focus = shape(Tool::Focus, vec![(22.0, 2.0), (38.0, 38.0)], Fill::Outline);
        let out = compose(&photo, &[focus.clone()], 1.0).unwrap();
        // Dentro del foco, blanco como estaba; fuera, blanco oscurecido.
        assert_eq!(out.pixel_rgba(30, 20).unwrap(), [255, 255, 255, 255]);
        let dim = out.pixel_rgba(30, 0).unwrap();
        assert!(dim[0] < 140, "oscurecido: {dim:?}");
        // El borrador lo toma por dentro.
        assert!(focus.hit((30.0, 20.0), 0.0));
    }

    #[test]
    fn el_relleno_da_la_vuelta() {
        assert_eq!(Fill::Outline.next(), Fill::Solid);
        assert_eq!(Fill::Solid.next(), Fill::Soft);
        assert_eq!(Fill::Soft.next(), Fill::Outline);
    }

    #[test]
    fn sobre_colores_claros_la_letra_va_negra() {
        assert_eq!(contrast(0xFFD60A), 0x111111);
        assert_eq!(contrast(0xFFFFFF), 0x111111);
        assert_eq!(contrast(0xE5484D), 0xFFFFFF);
        assert_eq!(contrast(0x111111), 0xFFFFFF);
    }

    #[test]
    fn el_paso_lleva_su_numero_en_contraste() {
        if font().is_none() {
            return;
        }
        let frame = Frame::new(atic_capture::Rect::new(0, 0, 60, 60), vec![0; 60 * 60 * 4]);
        let step = Shape {
            tool: Tool::Step,
            color: 0xE5484D,
            width: 20.0,
            points: vec![(30.0, 30.0)],
            text: "8".into(),
            fill: Fill::Solid,
            style: Style::default(),
        };
        let out = compose(&frame, &[step], 1.0).unwrap();
        // El círculo rojo, y adentro el 8 en blanco.
        let red = out.pixel_rgba(30, 14).unwrap();
        assert!(red[0] > 200 && red[1] < 100, "círculo: {red:?}");
        let white = (20..40)
            .flat_map(|y| (24..36).map(move |x| (x, y)))
            .filter(|&(x, y)| out.pixel_rgba(x, y).unwrap()[1] > 200)
            .count();
        assert!(white > 15, "el número se ve: {white} píxeles blancos");
    }

    #[test]
    fn el_texto_lleno_lleva_etiqueta() {
        if font().is_none() {
            return;
        }
        let frame = Frame::new(atic_capture::Rect::new(0, 0, 120, 60), vec![255; 120 * 60 * 4]);
        let label = Shape {
            tool: Tool::Text,
            color: 0x3E8BFF,
            width: 24.0,
            points: vec![(16.0, 14.0), (110.0, 14.0)],
            text: "Hola".into(),
            fill: Fill::Solid,
            style: Style::default(),
        };
        let out = compose(&frame, &[label], 1.0).unwrap();
        // Justo a la izquierda de la primera letra, dentro del margen: azul.
        let pad = out.pixel_rgba(12, 28).unwrap();
        assert!(pad[2] > 200 && pad[0] < 120, "etiqueta: {pad:?}");
    }

    #[test]
    fn el_texto_se_pinta_en_la_exportacion() {
        if font().is_none() {
            return; // Sin Segoe UI ni Arial (fuera de Windows) no hay letras.
        }
        let frame = Frame::new(atic_capture::Rect::new(0, 0, 120, 60), vec![255; 120 * 60 * 4]);
        let text = Shape {
            tool: Tool::Text,
            color: 0x000000,
            width: 30.0,
            points: vec![(4.0, 4.0), (116.0, 4.0)],
            text: "HH".into(),
            fill: Fill::Outline,
            style: Style::default(),
        };
        let out = compose(&frame, &[text.clone()], 1.0).unwrap();
        let dark = (0..60)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .filter(|&(x, y)| out.pixel_rgba(x, y).unwrap()[0] < 100)
            .count();
        assert!(dark > 100, "las letras dejan tinta: {dark} píxeles");
        // Y el borrador lo encuentra en su caja.
        assert!(text.hit((10.0, 20.0), 0.0));
        assert!(!text.hit((110.0, 55.0), 0.0));
    }
}

