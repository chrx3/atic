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
    Eraser,
}

pub(crate) const TOOLS: [(Tool, &str, &str); 13] = [
    (Tool::Pen, "icons/pencil.svg", "Lápiz · P"),
    (Tool::Highlight, "icons/highlighter.svg", "Resaltador · H"),
    (Tool::Line, "icons/line.svg", "Línea · L"),
    (Tool::Arrow, "icons/arrow-up-right.svg", "Flecha · A"),
    (Tool::Curve, "icons/arrow-curve.svg", "Flecha curva · C"),
    (Tool::Rect, "icons/square.svg", "Rectángulo · R"),
    (Tool::Ellipse, "icons/circle.svg", "Elipse · E"),
    (Tool::Step, "icons/step.svg", "Pasos 1, 2, 3 · N"),
    (Tool::Pixelate, "icons/pixelate.svg", "Pixelar · B"),
    (Tool::Focus, "icons/focus.svg", "Foco · O"),
    (Tool::Grab, "icons/scan-text.svg", "Copiar texto · G"),
    (Tool::Text, "icons/type.svg", "Texto · T"),
    (Tool::Eraser, "icons/eraser.svg", "Borrador · X"),
];

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
const TOOLBAR_W: f32 = 968.0;
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

/// Segoe UI seminegrita, la del editor; si falta, la normal o Arial. Se lee
/// una vez y vive lo que el proceso.
fn font() -> Option<&'static ttf_parser::Face<'static>> {
    static FACE: OnceLock<Option<ttf_parser::Face<'static>>> = OnceLock::new();
    FACE.get_or_init(|| {
        let fonts = std::env::var_os("WINDIR")
            .map(|dir| std::path::PathBuf::from(dir).join("Fonts"))?;
        ["seguisb.ttf", "segoeui.ttf", "arial.ttf"].iter().find_map(|name| {
            let data: &'static [u8] = Box::leak(std::fs::read(fonts.join(name)).ok()?.into_boxed_slice());
            ttf_parser::Face::parse(data, 0).ok()
        })
    })
    .as_ref()
}

/// El ancho de `text` a `size`. Sin la fuente, una aproximación.
fn text_width(text: &str, size: f32) -> f32 {
    let Some(face) = font() else {
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
        wrap_lines(&self.text, wrap, |s| text_width(s, self.width))
    }

    /// La caja que ocupa un texto: (x, y, ancho, alto).
    fn text_box(&self) -> (f32, f32, f32, f32) {
        let (x, y) = self.points.first().copied().unwrap_or_default();
        let lines = self.text_lines();
        let w = lines.iter().map(|line| text_width(line, self.width)).fold(0.0, f32::max);
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
    let Some(face) = font() else {
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
        Tool::Rect | Tool::Ellipse | Tool::Pixelate | Tool::Focus | Tool::Grab => {
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

/// Las formas con `tiny-skia`, a resolución física. Primero lo que cambia la
/// foto (pixelar, foco) y encima las anotaciones, que así siempre se leen.
fn draw_shapes(pixmap: &mut tiny_skia::Pixmap, shapes: &[Shape], scale: f32, photo: &Frame) {
    use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Stroke, Transform};
    let transform = Transform::from_scale(scale, scale);
    for shape in shapes.iter().filter(|shape| shape.tool == Tool::Pixelate) {
        draw_pixelate(pixmap, shape, scale, photo);
    }
    draw_focus(pixmap, shapes, scale);
    let polyline = |points: &[(f32, f32)], close: bool| {
        let mut builder = PathBuilder::new();
        builder.move_to(points[0].0, points[0].1);
        for p in &points[1..] {
            builder.line_to(p.0, p.1);
        }
        if close {
            builder.close();
        }
        builder.finish()
    };
    for shape in shapes {
        if matches!(shape.tool, Tool::Pixelate | Tool::Focus | Tool::Grab) {
            continue;
        }
        let [_, r, g, b] = shape.color.to_be_bytes();
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, (shape.alpha() * 255.0).round() as u8);
        paint.anti_alias = true;
        if shape.tool == Tool::Text {
            // Con fondo: la etiqueta primero; llena, la letra en blanco o negro.
            if shape.fill != Fill::Outline {
                let (color, alpha) = if shape.fill == Fill::Solid { (shape.color, 1.0) } else { SOFT_TEXT_BG };
                let [_, r, g, b] = color.to_be_bytes();
                let mut bg = paint.clone();
                bg.set_color_rgba8(r, g, b, (alpha * 255.0).round() as u8);
                if let Some(path) = polyline(&label_box(shape), true) {
                    pixmap.fill_path(&path, &bg, FillRule::Winding, transform, None);
                }
            }
            let mut ink = paint.clone();
            if shape.fill == Fill::Solid {
                let [_, r, g, b] = contrast(shape.color).to_be_bytes();
                ink.set_color_rgba8(r, g, b, 255);
            }
            draw_text(pixmap, shape, &ink, transform);
            continue;
        }
        for prim in shape.prims() {
            match prim {
                Prim::Stroke {
                    points,
                    width,
                    closed,
                } => {
                    if let Some(path) = polyline(&points, closed) {
                        let stroke = Stroke {
                            width,
                            line_cap: LineCap::Round,
                            line_join: LineJoin::Round,
                            ..Stroke::default()
                        };
                        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                    }
                }
                Prim::Fill(points, alpha) => {
                    if let Some(path) = polyline(&points, true) {
                        let mut fill = paint.clone();
                        fill.set_color_rgba8(r, g, b, (shape.alpha() * alpha * 255.0).round() as u8);
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
        if shape.tool == Tool::Step {
            draw_step(pixmap, shape, transform);
        }
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

/// Un texto que se está escribiendo: dónde, cómo, y lo que había antes de
/// empezar (deshacer lo devuelve de una vez).
struct Typing {
    at: (f32, f32),
    wrap: f32,
    color: u32,
    size: f32,
    fill: Fill,
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

/// La herramienta, el color, el grosor y el relleno de la última pizarra: la
/// próxima empieza igual. El vistazo de la tira también los elige.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Prefs {
    pub tool: Tool,
    pub color: u32,
    pub width: usize,
    pub fill: Fill,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { tool: Tool::Pen, color: COLORS[0], width: 1, fill: Fill::Outline }
    }
}

/// Cada herramienta con su nombre en `pizarra.txt`.
const TOOL_NAMES: [(Tool, &str); 13] = [
    (Tool::Pen, "pen"),
    (Tool::Highlight, "highlight"),
    (Tool::Line, "line"),
    (Tool::Arrow, "arrow"),
    (Tool::Curve, "curve"),
    (Tool::Rect, "rect"),
    (Tool::Ellipse, "ellipse"),
    (Tool::Step, "step"),
    (Tool::Pixelate, "pixelate"),
    (Tool::Focus, "focus"),
    (Tool::Grab, "grab"),
    (Tool::Text, "text"),
    (Tool::Eraser, "eraser"),
];

impl Prefs {
    fn file() -> Option<std::path::PathBuf> {
        crate::paths::file("pizarra.txt")
    }

    /// `pen e5484d 1 solid`, como lo guarda `save` (el relleno puede faltar:
    /// así lo guardaba la versión anterior).
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split_whitespace();
        let name = parts.next()?;
        let tool = TOOL_NAMES
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(tool, _)| *tool)
            // El borrador no se recuerda: abrir borrando no sirve.
            .filter(|tool| *tool != Tool::Eraser)
            .unwrap_or(Tool::Pen);
        let color = u32::from_str_radix(parts.next()?, 16).ok()?;
        let width = parts.next()?.parse::<usize>().ok()?.min(WIDTHS.len() - 1);
        let fill = match parts.next() {
            Some("solid") => Fill::Solid,
            Some("soft") => Fill::Soft,
            _ => Fill::Outline,
        };
        Some(Self { tool, color, width, fill })
    }

    fn text(&self) -> String {
        let tool = TOOL_NAMES.iter().find(|(t, _)| *t == self.tool).map_or("pen", |(_, n)| n);
        let fill = match self.fill {
            Fill::Outline => "outline",
            Fill::Solid => "solid",
            Fill::Soft => "soft",
        };
        format!("{tool} {:06x} {} {fill}", self.color, self.width)
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

pub enum BoardEvent {
    Done {
        frame: Frame,
        shapes: Vec<Shape>,
        scale: f32,
    },
    Closed,
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
        self.tool = tool;
        cx.notify();
    }

    fn prefs(&self) -> Prefs {
        Prefs { tool: self.tool, color: self.color, width: self.width, fill: self.fill }
    }

    // --- Texto --------------------------------------------------------------

    /// Empieza un texto en `p`, o vuelve a editar el que está ahí.
    fn start_text(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let before = self.shapes.clone();
        let existing = self
            .shapes
            .iter()
            .rposition(|shape| shape.tool == Tool::Text && shape.hit(p, 4.0));
        let (at, wrap, color, size, fill, body) = match existing {
            Some(index) => {
                // Sale de la capa mientras se edita: el editor va en su lugar.
                let shape = self.shapes.remove(index);
                self.rebuild_layer();
                let (x, y) = shape.points[0];
                let wrap = shape.points.get(1).map_or(TEXT_WRAP_MAX, |end| end.0 - x);
                ((x, y), wrap, shape.color, shape.width, shape.fill, shape.text)
            }
            None => {
                let size = TEXT_SIZES[self.width];
                // El clic queda a media altura de la primera línea.
                let at = (p.0, p.1 - size * TEXT_LINE / 2.0);
                let image_w = self.frozen.frame.width() as f32 / self.frozen.scale;
                let wrap = (image_w - at.0).clamp(TEXT_WRAP_MIN, TEXT_WRAP_MAX);
                (at, wrap, self.color, size, self.fill, String::new())
            }
        };
        let (editor, _changed) = text_editor(&body, color, fill, cx);
        self.typing = Some(Typing { at, wrap, color, size, fill, editor, _changed, before });
        self.focus_editor = true;
        cx.notify();
    }

    /// Cambiar el color, el grosor o el relleno mientras se escribe cambia ese
    /// texto: el editor se rehace con lo escrito (su color no se cambia por fuera).
    fn restyle_text(&mut self, cx: &mut Context<Self>) {
        let (color, size, fill) = (self.color, TEXT_SIZES[self.width], self.fill);
        let Some(body) = self.typing.as_ref().map(|t| t.editor.read(cx).text().to_string()) else {
            return;
        };
        let (editor, changed) = text_editor(&body, color, fill, cx);
        if let Some(typing) = self.typing.as_mut() {
            typing.color = color;
            typing.size = size;
            typing.fill = fill;
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
            self.shapes.push(Shape {
                tool: Tool::Text,
                color: typing.color,
                width: typing.size,
                points: vec![typing.at, (typing.at.0 + typing.wrap, typing.at.1)],
                text: body,
                fill: typing.fill,
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
        if let Some(previous) = self.past.pop() {
            self.future.push(std::mem::replace(&mut self.shapes, previous));
            self.rebuild_layer();
        }
    }

    fn redo_step(&mut self) {
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
        self.clear_all();
        cx.notify();
    }

    fn done(&mut self, _: &Done, _: &mut Window, cx: &mut Context<Self>) {
        self.finish(cx);
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        self.end_text(cx);
        cx.emit(BoardEvent::Done {
            frame: self.frozen.frame.clone(),
            shapes: self.shapes.clone(),
            scale: self.frozen.scale,
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
            self.start_text(p, cx);
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
            });
        }
        cx.notify();
    }

    fn moved(&mut self, p: (f32, f32), shift: bool, cx: &mut Context<Self>) {
        self.cursor = p;
        if self.erasing {
            self.erase_at(p);
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
        }
        cx.notify();
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
        if let Some(shape) = self.current.take() {
            let span = shape
                .points
                .last()
                .map(|last| distance(shape.points[0], *last))
                .unwrap_or_default();
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
    /// va pegada a ella, como en Flameshot.
    fn toolbar(&self, framed: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let tools = TOOLS.iter().map(|&(tool, icon, _)| {
            self.button(
                match tool {
                    Tool::Pen => "board-pen",
                    Tool::Highlight => "board-highlight",
                    Tool::Line => "board-line",
                    Tool::Arrow => "board-arrow",
                    Tool::Curve => "board-curve",
                    Tool::Rect => "board-rect",
                    Tool::Ellipse => "board-ellipse",
                    Tool::Step => "board-step",
                    Tool::Pixelate => "board-pixelate",
                    Tool::Focus => "board-focus",
                    Tool::Grab => "board-grab",
                    Tool::Text => "board-text",
                    Tool::Eraser => "board-eraser",
                },
                icon,
                self.tool == tool,
                move |view, cx| view.set_tool(tool, cx),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
        let colors = COLORS.iter().enumerate().map(|(i, &color)| {
            let active = self.color == color;
            div()
                .id(("board-color", i))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(11.))
                .cursor_pointer()
                .when(active, |el| el.border_2().border_color(gpui::white().opacity(0.85)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.color = color;
                    if view.tool == Tool::Eraser {
                        view.tool = Tool::Pen;
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
            .h(px(BAR_H))
            .px(px(10.))
            .flex()
            .items_center()
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
            .child(self.button("board-done", "icons/check.svg", false, |view, cx| view.finish(cx), cx))
            .child(self.button("board-close", "icons/x.svg", false, |_, cx| cx.emit(BoardEvent::Closed), cx))
    }
}

/// El nombre de cada botón de la barra, con su atajo.
fn button_tip(id: &str) -> Option<&'static str> {
    let tool = |tool: Tool| TOOLS.iter().find(|t| t.0 == tool).map(|t| t.2);
    match id {
        "board-pen" => tool(Tool::Pen),
        "board-highlight" => tool(Tool::Highlight),
        "board-line" => tool(Tool::Line),
        "board-arrow" => tool(Tool::Arrow),
        "board-curve" => tool(Tool::Curve),
        "board-rect" => tool(Tool::Rect),
        "board-ellipse" => tool(Tool::Ellipse),
        "board-step" => tool(Tool::Step),
        "board-pixelate" => tool(Tool::Pixelate),
        "board-focus" => tool(Tool::Focus),
        "board-grab" => tool(Tool::Grab),
        "board-hide" => Some("Ocultar datos: correos, teléfonos, RUT, claves… · Ctrl+H"),
        "board-text" => tool(Tool::Text),
        "board-eraser" => tool(Tool::Eraser),
        "board-undo" => Some("Deshacer · Ctrl+Z"),
        "board-redo" => Some("Rehacer · Ctrl+Y"),
        "board-clear" => Some("Borrar todo"),
        "board-done" => Some("Listo"),
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
        let (screen_w, screen_h) = (f32::from(viewport.width), f32::from(viewport.height));
        let scale = self.frozen.scale;
        let (w, h) = (
            self.frozen.frame.width() as f32 / scale,
            self.frozen.frame.height() as f32 / scale,
        );
        // Lo que se está arrastrando y el recuadro que el OCR está leyendo.
        let shapes: Vec<Shape> = self.current.clone().into_iter().chain(self.grabbing.clone()).collect();
        let layer = self.layer.clone();
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
            let rows = wrap_lines(t.editor.read(cx).text(), t.wrap, |s| text_width(s, t.size)).len().max(1);
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
                        .font_family("Segoe UI")
                        .font_weight(gpui::FontWeight::SEMIBOLD)
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
                if framed {
                    let left = (offset.0 + w / 2.0 - TOOLBAR_W / 2.0)
                        .clamp(8.0, (screen_w - TOOLBAR_W - 8.0).max(8.0));
                    let below = offset.1 + h + 10.0;
                    let top = if below + BAR_H + 40.0 < screen_h {
                        below
                    } else {
                        (offset.1 - BAR_H - 40.0).max(8.0)
                    };
                    bar.left(px(left))
                        .top(px(top))
                        .w(px(TOOLBAR_W))
                        .child(self.toolbar(true, cx))
                        .children(hint.map(chip))
                } else {
                    bar.top(px(0.))
                        .w_full()
                        .child(self.toolbar(false, cx))
                        .children(hint.map(chip))
                }
            })
    }
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
        self.board_events = Some(cx.subscribe(&view, |pill, _, event: &BoardEvent, cx| {
            pill.end_board(event, cx)
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

    fn end_board(&mut self, event: &BoardEvent, cx: &mut Context<Self>) {
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
        } = event
        {
            // El resultado vuela al estante desde donde estaba la imagen.
            let board = view.read(cx);
            let from = crate::geometry::Rect::new(
                board.frozen.offset.0 + board.origin.0,
                board.frozen.offset.1 + board.origin.1,
                frame.width() as f32 / scale,
                frame.height() as f32 / scale,
            );
            let (frame, shapes, scale) = (frame.clone(), shapes.clone(), *scale);
            cx.spawn(async move |this, cx| {
                let started = Instant::now();
                let saved: Result<Saved, String> = cx
                    .background_spawn(async move {
                        let composed = compose(&frame, &shapes, scale)?;
                        let saved = capture::save(&composed, composed.bounds)?;
                        // Para seguir editando después.
                        if let Err(error) = save_edit(&saved.path, &frame, &shapes, scale) {
                            eprintln!("pizarra: no se guardó la edición: {error}");
                        }
                        Ok(saved)
                    })
                    .await;
                let _ =
                    this.update(cx, |pill, cx| pill.capture_saved(saved, started, from, cx));
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
        for (tool, _) in TOOL_NAMES.iter().filter(|(tool, _)| *tool != Tool::Eraser) {
            let prefs = Prefs { tool: *tool, color: 0x3E8BFF, width: 2, fill: Fill::Soft };
            assert_eq!(Prefs::parse(&prefs.text()), Some(prefs));
        }
        // El borrador no se recuerda: se vuelve al lápiz.
        let eraser = Prefs { tool: Tool::Eraser, color: 0x3E8BFF, width: 2, fill: Fill::Solid };
        assert_eq!(Prefs::parse(&eraser.text()).map(|p| p.tool), Some(Tool::Pen));
        // Lo que guardaba la versión anterior, sin relleno, sigue sirviendo.
        assert_eq!(Prefs::parse("text e5484d 1").map(|p| p.fill), Some(Fill::Outline));
        assert_eq!(Prefs::parse("basura"), None);
    }

    fn shape(tool: Tool, points: Vec<(f32, f32)>, fill: Fill) -> Shape {
        Shape { tool, color: 0xE5484D, width: 4.0, points, text: String::new(), fill }
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
            Shape { tool: Tool::Text, color: 0x3E8BFF, width: 26.0, points: vec![(5.0, 40.0), (200.0, 40.0)], text: "Hola".into(), fill: Fill::Solid },
            Shape { tool: Tool::Step, color: 0xFFD60A, width: 14.0, points: vec![(80.0, 80.0)], text: "3".into(), fill: Fill::Solid },
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

