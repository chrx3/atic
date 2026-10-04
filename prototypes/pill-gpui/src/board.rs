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

use std::time::{Duration, Instant};

use std::sync::Arc;

use atic_capture::Frame;
use gpui::{
    actions, canvas, div, img, point, prelude::*, px, rgb, svg, App, ClickEvent, Context,
    CursorStyle, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PathStyle, Pixels, Point,
    RenderImage, SharedString, Window,
};
use lyon::tessellation::{LineCap, LineJoin, StrokeOptions};

use crate::capture::{self, Frozen, Saved};

/// «Pizarra» en la tira y la rueda.
pub const TOOL: usize = 6;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Pen,
    Highlight,
    Arrow,
    Rect,
    Ellipse,
    Eraser,
}

const TOOLS: [(Tool, &str, &str); 6] = [
    (Tool::Pen, "icons/pencil.svg", "Lápiz · P"),
    (Tool::Highlight, "icons/highlighter.svg", "Resaltador · H"),
    (Tool::Arrow, "icons/arrow-up-right.svg", "Flecha · A"),
    (Tool::Rect, "icons/square.svg", "Rectángulo · R"),
    (Tool::Ellipse, "icons/circle.svg", "Elipse · E"),
    (Tool::Eraser, "icons/eraser.svg", "Borrador · X"),
];

/// Rojo por defecto, como Atic.
const COLORS: [u32; 7] = [
    0xE5484D, 0xF5A524, 0xFFD60A, 0x30A46C, 0x3E8BFF, 0xFFFFFF, 0x111111,
];
/// Grosor lógico; el resaltador va 4 veces más ancho.
const WIDTHS: [f32; 3] = [2.5, 4.5, 8.0];
const HIGHLIGHT_WIDTH: f32 = 4.0;
const HIGHLIGHT_ALPHA: f32 = 0.35;
/// Distancia mínima entre puntos del lápiz: un arrastre lento no mete cientos.
const MIN_STEP: f32 = 2.5;
/// El segundo Esc dentro de este rato descarta lo dibujado.
const CLOSE_ARM: Duration = Duration::from_millis(1600);

const BAR_H: f32 = 44.0;
/// Ancho aproximado de la barra, para centrarla bajo una captura.
const TOOLBAR_W: f32 = 680.0;
const BUTTON: f32 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub tool: Tool,
    pub color: u32,
    pub width: f32,
    pub points: Vec<(f32, f32)>,
}

/// Lo que se dibuja, igual en pantalla y en el PNG.
#[derive(Debug, PartialEq)]
pub enum Prim {
    Stroke {
        points: Vec<(f32, f32)>,
        width: f32,
        closed: bool,
    },
    Fill(Vec<(f32, f32)>),
    Dot((f32, f32), f32),
}

impl Shape {
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
            Tool::Arrow => {
                let angle = (last.1 - first.1).atan2(last.0 - first.0);
                let head = (width * 4.0).max(12.0).min(distance(first, last) * 0.6);
                let spread = 0.45;
                let wing = |side: f32| {
                    (
                        last.0 - head * (angle + side * spread).cos(),
                        last.1 - head * (angle + side * spread).sin(),
                    )
                };
                // El asta termina dentro de la punta para no asomar por ella.
                let shaft_end = (
                    last.0 - head * 0.6 * angle.cos(),
                    last.1 - head * 0.6 * angle.sin(),
                );
                vec![
                    Prim::Stroke {
                        points: vec![first, shaft_end],
                        width,
                        closed: false,
                    },
                    Prim::Fill(vec![last, wing(1.0), wing(-1.0)]),
                ]
            }
            Tool::Rect => {
                let (x0, y0, x1, y1) = (
                    first.0.min(last.0),
                    first.1.min(last.1),
                    first.0.max(last.0),
                    first.1.max(last.1),
                );
                vec![Prim::Stroke {
                    points: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
                    width,
                    closed: true,
                }]
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
                vec![Prim::Stroke {
                    points,
                    width,
                    closed: true,
                }]
            }
            Tool::Eraser => Vec::new(),
        }
    }

    /// El trazo pasa a menos de `slack` del punto.
    fn hit(&self, p: (f32, f32), slack: f32) -> bool {
        self.prims().iter().any(|prim| match prim {
            Prim::Dot(center, r) => distance(*center, p) <= r + slack,
            Prim::Fill(points) => points.iter().any(|q| distance(*q, p) <= slack * 2.0),
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
        Tool::Rect | Tool::Ellipse => {
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
            Prim::Fill(points) => {
                let mut builder = PathBuilder::fill();
                let gpoints: Vec<_> = points.into_iter().map(|p| gpoint(p, offset)).collect();
                builder.add_polygon(&gpoints, true);
                builder.build()
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

/// Las formas con `tiny-skia`, a resolución física.
fn draw_shapes(pixmap: &mut tiny_skia::Pixmap, shapes: &[Shape], scale: f32) {
    use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Stroke, Transform};
    let transform = Transform::from_scale(scale, scale);
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
        let [_, r, g, b] = shape.color.to_be_bytes();
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, (shape.alpha() * 255.0).round() as u8);
        paint.anti_alias = true;
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
                Prim::Fill(points) => {
                    if let Some(path) = polyline(&points, true) {
                        pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    }
                }
                Prim::Dot(center, r) => {
                    if let Some(path) = PathBuilder::from_circle(center.0, center.1, r) {
                        pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    }
                }
            }
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
    draw_shapes(&mut pixmap, shapes, scale);
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
fn rasterize(shapes: &[Shape], width: u32, height: u32, scale: f32) -> Option<Arc<RenderImage>> {
    if shapes.is_empty() {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    draw_shapes(&mut pixmap, shapes, scale);
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
        Close, Done, Undo, Redo, Clear, PickPen, PickHighlight, PickArrow, PickRect, PickEllipse,
        PickEraser
    ]
);

const KEY_CONTEXT: &str = "Board";

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
        KeyBinding::new("p", PickPen, context),
        KeyBinding::new("1", PickPen, context),
        KeyBinding::new("h", PickHighlight, context),
        KeyBinding::new("2", PickHighlight, context),
        KeyBinding::new("a", PickArrow, context),
        KeyBinding::new("3", PickArrow, context),
        KeyBinding::new("r", PickRect, context),
        KeyBinding::new("4", PickRect, context),
        KeyBinding::new("e", PickEllipse, context),
        KeyBinding::new("5", PickEllipse, context),
        KeyBinding::new("x", PickEraser, context),
        KeyBinding::new("6", PickEraser, context),
    ]);
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
    cursor: (f32, f32),
    /// Primer Esc con algo dibujado: el segundo, dentro de `CLOSE_ARM`, cierra.
    close_armed: Option<Instant>,
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
            tool: Tool::Pen,
            color: COLORS[0],
            width: 1,
            cursor,
            close_armed: None,
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
        self.tool = tool;
        cx.notify();
    }

    fn rebuild_layer(&mut self) {
        let started = Instant::now();
        let frame = &self.frozen.frame;
        self.layer = rasterize(&self.shapes, frame.width(), frame.height(), self.frozen.scale);
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
        if self.tool == Tool::Eraser {
            self.erasing = true;
            self.erase_saved = false;
            self.erase_at(p);
        } else {
            self.current = Some(Shape {
                tool: self.tool,
                color: self.color,
                width: WIDTHS[self.width],
                points: vec![p],
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
                Tool::Pen | Tool::Highlight if !shift => {
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

    fn up(&mut self, cx: &mut Context<Self>) {
        self.erasing = false;
        if let Some(shape) = self.current.take() {
            let span = shape
                .points
                .last()
                .map(|last| distance(shape.points[0], *last))
                .unwrap_or_default();
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
            .when(active, |el| el.bg(text.opacity(0.16)))
            .hover(|el| el.bg(text.opacity(0.09)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| on_click(view, cx)))
            .child(
                svg()
                    .path(icon)
                    .size(px(15.))
                    .text_color(if active { text } else { text.opacity(0.62) }),
            )
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
                    Tool::Arrow => "board-arrow",
                    Tool::Rect => "board-rect",
                    Tool::Ellipse => "board-ellipse",
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
                .when(active, |el| el.border_2().border_color(gpui::white().opacity(0.85)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.color = color;
                    if view.tool == Tool::Eraser {
                        view.tool = Tool::Pen;
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(14.))
                        .rounded(px(7.))
                        .bg(rgb(color))
                        .border_1()
                        .border_color(gpui::white().opacity(0.25)),
                )
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
                .when(active, |el| el.bg(gpui::white().opacity(0.16)))
                .hover(|el| el.bg(gpui::white().opacity(0.09)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.width = i;
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(width + 2.0))
                        .rounded(px(width))
                        .bg(gpui::white().opacity(0.85)),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
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
            .child(Self::separator())
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
        let viewport = window.viewport_size();
        let (screen_w, screen_h) = (f32::from(viewport.width), f32::from(viewport.height));
        let scale = self.frozen.scale;
        let (w, h) = (
            self.frozen.frame.width() as f32 / scale,
            self.frozen.frame.height() as f32 / scale,
        );
        let shapes: Vec<Shape> = self.current.clone().into_iter().collect();
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
        } else {
            TOOLS
                .iter()
                .find(|(tool, ..)| *tool == self.tool)
                .map(|(_, _, label)| {
                    format!("{label} · Shift recto · Ctrl+Z deshacer · Enter copiar · Esc salir")
                })
        };

        div()
            .id("board")
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::done))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(|v, _: &PickPen, _, cx| v.set_tool(Tool::Pen, cx)))
            .on_action(cx.listener(|v, _: &PickHighlight, _, cx| v.set_tool(Tool::Highlight, cx)))
            .on_action(cx.listener(|v, _: &PickArrow, _, cx| v.set_tool(Tool::Arrow, cx)))
            .on_action(cx.listener(|v, _: &PickRect, _, cx| v.set_tool(Tool::Rect, cx)))
            .on_action(cx.listener(|v, _: &PickEllipse, _, cx| v.set_tool(Tool::Ellipse, cx)))
            .on_action(cx.listener(|v, _: &PickEraser, _, cx| v.set_tool(Tool::Eraser, cx)))
            .size_full()
            .relative()
            .cursor(CursorStyle::Crosshair)
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
                el.child(div().absolute().size_full().bg(gpui::black().opacity(0.55)))
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
        cx.spawn_in(window, async move |this, cx| {
            let frozen = cx.background_spawn(async move { capture::freeze(scale) }).await;
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
        let view = cx.new(|cx| BoardView::new(frozen, origin, cursor, previous, cx));
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

    fn end_board(&mut self, event: &BoardEvent, cx: &mut Context<Self>) {
        let Some(view) = self.board.take() else {
            return;
        };
        self.board_events = None;
        let previous = view.read(cx).previous;
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
                        capture::save(&composed, composed.bounds)
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
        }];
        // Al 200 %: el borde del rectángulo cae en (4, 4) físico.
        let out = compose(&frame, &shapes, 2.0).unwrap();
        let px = out.pixel_rgba(4, 10).unwrap();
        assert!(px[0] > 200 && px[1] < 80 && px[2] < 80, "rojo: {px:?}");
        // Lo que no se tocó sigue blanco.
        assert_eq!(out.pixel_rgba(10, 10).unwrap(), [255, 255, 255, 255]);
    }
}

