//! Color: el cuentagotas, en vivo como el de Atic (`color_picker.rs`,
//! `ColorLoupeSurface.svelte`).
//!
//! Una lupa de 13×13 píxeles sigue al cursor; clic o Enter copia y cierra.
//! R abre la rosa: muestra grande, formato, saturación/brillo, matiz, HEX,
//! matices fijos y recientes. Mejoras respecto de Atic:
//! - Se lee el color también bajo la lupa: se muestrea sin `CAPTUREBLT`, que
//!   ignora las ventanas layered como el overlay, así que la lupa no tapa lo
//!   que mide (y sigue saliendo en grabaciones).
//! - Flechas mueven el cursor 1 px (Shift: 10 px) para clavar el píxel.
//! - Tab cambia el formato sin abrir la rosa.
//! - El color elegido cuelga del notch un momento.

use std::path::PathBuf;
use std::time::Instant;

use atic_capture::{monitors, Frame, Rect as PhysRect};
use gpui::{
    actions, canvas, div, fill, linear_color_stop, linear_gradient, outline, point, prelude::*,
    px, rgb, size, App, BorderStyle, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle,
    Entity, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, SharedString, Subscription, Window,
};

use crate::text_input::{self, TextInput};

/// «Color» en la tira y la rueda.
pub const TOOL: usize = 7;

const LOUPE_PIXELS: i32 = 13;
const LOUPE_CELL: f32 = 10.0;
const LOUPE_OFFSET: f32 = 28.0;
const RECENTS_MAX: usize = 8;

const ROSE_W: f32 = 296.0;
const ROSE_PAD: f32 = 12.0;
const PLANE_H: f32 = 150.0;
const HUE_H: f32 = 14.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Format {
    Hex,
    Rgb,
    Hsl,
}

impl Format {
    fn next(self) -> Self {
        match self {
            Format::Hex => Format::Rgb,
            Format::Rgb => Format::Hsl,
            Format::Hsl => Format::Hex,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Format::Hex => "hex",
            Format::Rgb => "rgb()",
            Format::Hsl => "hsl()",
        }
    }
}

// --- Matemática de color ----------------------------------------------------

fn channels(color: u32) -> (u8, u8, u8) {
    let [_, r, g, b] = color.to_be_bytes();
    (r, g, b)
}

pub fn format(color: u32, format: Format) -> String {
    let (r, g, b) = channels(color);
    match format {
        Format::Hex => format!("#{r:02X}{g:02X}{b:02X}"),
        Format::Rgb => format!("rgb({r}, {g}, {b})"),
        Format::Hsl => {
            let (h, s, l) = rgb_to_hsl(color);
            format!(
                "hsl({}, {}%, {}%)",
                h.round() as i32,
                (s * 100.0).round() as i32,
                (l * 100.0).round() as i32
            )
        }
    }
}

pub fn rgb_to_hsl(color: u32) -> (f32, f32, f32) {
    let (r, g, b) = channels(color);
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    (hue_of(r, g, b, max, d), s, l)
}

fn hue_of(r: f32, g: f32, b: f32, max: f32, d: f32) -> f32 {
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

pub fn rgb_to_hsv(color: u32) -> (f32, f32, f32) {
    let (r, g, b) = channels(color);
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0.0 { 0.0 } else { hue_of(r, g, b, max, d) };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |value: f32| ((value + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

fn parse_hex(text: &str) -> Option<u32> {
    let hex = text.trim().trim_start_matches('#');
    let full = match hex.len() {
        3 => hex.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok()
}

/// Texto legible sobre el color: negro sobre claros, blanco sobre oscuros.
fn ink_on(color: u32) -> Hsla {
    let (r, g, b) = channels(color);
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 150.0 {
        gpui::black()
    } else {
        gpui::white()
    }
}

// --- Recientes ----------------------------------------------------------------

fn recents_file() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("atic-gpui").join("colors.json"))
}

pub fn load_recents() -> Vec<u32> {
    recents_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .map(|list| list.iter().filter_map(|hex| parse_hex(hex)).collect())
        .unwrap_or_default()
}

fn save_recents(recents: &[u32]) {
    let Some(path) = recents_file() else {
        return;
    };
    let list: Vec<String> = recents.iter().map(|c| format(*c, Format::Hex)).collect();
    if let Ok(raw) = serde_json::to_string(&list) {
        let _ = std::fs::write(path, raw);
    }
}

// --- La vista -------------------------------------------------------------------

actions!(
    color_picker,
    [
        Cancel, Pick, ToggleRose, CycleFormat, NudgeLeft, NudgeRight, NudgeUp, NudgeDown,
        JumpLeft, JumpRight, JumpUp, JumpDown
    ]
);

const KEY_CONTEXT: &str = "ColorPicker";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, context),
        KeyBinding::new("enter", Pick, context),
        KeyBinding::new("r", ToggleRose, context),
        KeyBinding::new("tab", CycleFormat, context),
        KeyBinding::new("left", NudgeLeft, context),
        KeyBinding::new("right", NudgeRight, context),
        KeyBinding::new("up", NudgeUp, context),
        KeyBinding::new("down", NudgeDown, context),
        KeyBinding::new("shift-left", JumpLeft, context),
        KeyBinding::new("shift-right", JumpRight, context),
        KeyBinding::new("shift-up", JumpUp, context),
        KeyBinding::new("shift-down", JumpDown, context),
    ]);
}

pub enum ColorEvent {
    Picked { color: u32, text: String },
    Cancelled,
}

/// La rosa abierta: dónde está y el color que se edita (HSV).
struct Rose {
    at: (f32, f32),
    h: f32,
    s: f32,
    v: f32,
}

impl Rose {
    fn color(&self) -> u32 {
        hsv_to_rgb(self.h, self.s, self.v)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Plane,
    Hue,
}

pub struct ColorView {
    focus: FocusHandle,
    /// Píxel físico de la esquina (0, 0) lógica de la ventana.
    origin: (i32, i32),
    scale: f32,
    cursor: (f32, f32),
    loupe: Option<Frame>,
    format: Format,
    recents: Vec<u32>,
    rose: Option<Rose>,
    drag: Option<Drag>,
    hex: Entity<TextInput>,
    _hex_changed: Subscription,
    meter: crate::meter::Meter,
}

impl EventEmitter<ColorEvent> for ColorView {}

impl Focusable for ColorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ColorView {
    pub fn new(origin: (i32, i32), scale: f32, cursor: (f32, f32), cx: &mut Context<Self>) -> Self {
        let text: Hsla = rgb(0xf0f0ea).into();
        let hex = cx.new(|cx| TextInput::new("#RRGGBB", text, text.opacity(0.5), text, cx));
        let hex_changed = cx.subscribe(&hex, |view, input, _: &text_input::Changed, cx| {
            // Escribir un HEX válido mueve la rosa a ese color.
            if let (Some(color), Some(rose)) = (parse_hex(input.read(cx).text()), view.rose.as_mut()) {
                let (h, s, v) = rgb_to_hsv(color);
                rose.h = h;
                rose.s = s;
                rose.v = v;
                cx.notify();
            }
        });
        let mut view = Self {
            focus: cx.focus_handle(),
            origin,
            scale,
            cursor,
            loupe: None,
            format: Format::Hex,
            recents: load_recents(),
            rose: None,
            drag: None,
            hex,
            _hex_changed: hex_changed,
            meter: crate::meter::Meter::new("color"),
        };
        view.sample();
        view
    }

    fn physical(&self, (x, y): (f32, f32)) -> (i32, i32) {
        (
            self.origin.0 + (x * self.scale).floor() as i32,
            self.origin.1 + (y * self.scale).floor() as i32,
        )
    }

    /// Los 13×13 píxeles alrededor del cursor, recién leídos.
    fn sample(&mut self) {
        let (x, y) = self.physical(self.cursor);
        let half = LOUPE_PIXELS / 2;
        let rect = PhysRect::new(x - half, y - half, LOUPE_PIXELS as u32, LOUPE_PIXELS as u32);
        self.loupe = atic_capture::engine::capture_rect_without_layered(rect).ok();
    }

    fn under_cursor(&self) -> Option<u32> {
        let half = LOUPE_PIXELS / 2;
        let [r, g, b, _] = self.loupe.as_ref()?.pixel_rgba(half, half)?;
        Some(u32::from_be_bytes([0, r, g, b]))
    }

    fn current(&self) -> Option<u32> {
        match &self.rose {
            Some(rose) => Some(rose.color()),
            None => self.under_cursor(),
        }
    }

    fn pick(&mut self, color: u32, cx: &mut Context<Self>) {
        let text = format(color, self.format);
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.recents.retain(|c| *c != color);
        self.recents.insert(0, color);
        self.recents.truncate(RECENTS_MAX);
        save_recents(&self.recents);
        cx.emit(ColorEvent::Picked { color, text });
    }

    fn on_pick(&mut self, _: &Pick, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(color) = self.current() {
            self.pick(color, cx);
        }
    }

    fn on_cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        // Con la rosa abierta, Esc vuelve a la lupa.
        if self.rose.take().is_some() {
            cx.notify();
        } else {
            cx.emit(ColorEvent::Cancelled);
        }
    }

    fn toggle_rose(&mut self, _: &ToggleRose, window: &mut Window, cx: &mut Context<Self>) {
        if self.rose.take().is_some() {
            window.focus(&self.focus);
        } else {
            self.open_rose(self.under_cursor().unwrap_or(0xE5484D), window, cx);
        }
        cx.notify();
    }

    fn open_rose(&mut self, color: u32, window: &mut Window, cx: &mut Context<Self>) {
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let rose_h = rose_height();
        let x = (self.cursor.0 + LOUPE_OFFSET).min(vw - ROSE_W - 8.0).max(8.0);
        let y = (self.cursor.1 - rose_h / 2.0).clamp(8.0, (vh - rose_h - 8.0).max(8.0));
        let (h, s, v) = rgb_to_hsv(color);
        self.rose = Some(Rose { at: (x, y), h, s, v });
        self.hex
            .update(cx, |input, cx| input.set_text(format(color, Format::Hex), cx));
    }

    fn set_rose_color(&mut self, color: u32, cx: &mut Context<Self>) {
        if let Some(rose) = self.rose.as_mut() {
            let (h, s, v) = rgb_to_hsv(color);
            rose.h = h;
            rose.s = s;
            rose.v = v;
        }
        self.hex
            .update(cx, |input, cx| input.set_text(format(color, Format::Hex), cx));
        cx.notify();
    }

    /// Mueve el cursor del sistema: las flechas clavan el píxel.
    fn nudge(&mut self, dx: i32, dy: i32, cx: &mut Context<Self>) {
        if self.rose.is_some() {
            return;
        }
        let (x, y) = self.physical(self.cursor);
        set_cursor(x + dx, y + dy);
        self.cursor = (
            self.cursor.0 + dx as f32 / self.scale,
            self.cursor.1 + dy as f32 / self.scale,
        );
        self.sample();
        cx.notify();
    }

    /// Arrastrar en el cuadro o la barra de matiz.
    fn drag_to(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let (Some(drag), Some(rose)) = (self.drag, self.rose.as_mut()) else {
            return;
        };
        let (plane, hue) = rose_parts(rose.at);
        match drag {
            Drag::Plane => {
                rose.s = ((p.0 - plane.x) / plane.w).clamp(0.0, 1.0);
                rose.v = 1.0 - ((p.1 - plane.y) / plane.h).clamp(0.0, 1.0);
            }
            Drag::Hue => {
                rose.h = ((p.0 - hue.x) / hue.w).clamp(0.0, 1.0) * 359.0;
            }
        }
        let color = rose.color();
        self.hex
            .update(cx, |input, cx| input.set_text(format(color, Format::Hex), cx));
        cx.notify();
    }
}

fn set_cursor(x: i32, y: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

/// Alto total de la rosa: muestra, formatos, cuadro, matiz, HEX, matices y
/// recientes.
fn rose_height() -> f32 {
    ROSE_PAD + 56.0 + 10.0 + 26.0 + 10.0 + PLANE_H + 10.0 + HUE_H + 12.0 + 28.0 + 10.0 + 18.0 + 22.0
        + 18.0
        + ROSE_PAD
}

/// El cuadro de saturación/brillo y la barra de matiz, en la ventana.
fn rose_parts(at: (f32, f32)) -> (crate::geometry::Rect, crate::geometry::Rect) {
    let inner = ROSE_W - ROSE_PAD * 2.0;
    let top = at.1 + ROSE_PAD + 56.0 + 10.0 + 26.0 + 10.0;
    let plane = crate::geometry::Rect::new(at.0 + ROSE_PAD, top, inner, PLANE_H);
    let hue = crate::geometry::Rect::new(at.0 + ROSE_PAD, top + PLANE_H + 10.0, inner, HUE_H);
    (plane, hue)
}

fn chip(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(8.))
        .bg(gpui::black().opacity(0.8))
        .text_color(gpui::white())
        .text_size(px(11.))
        .font_family("Segoe UI")
        .child(text.into())
}

impl ColorView {
    fn render_loupe(&self, viewport: (f32, f32)) -> impl IntoElement {
        let side = LOUPE_PIXELS as f32 * LOUPE_CELL;
        let (cx_, cy_) = self.cursor;
        let lx = if cx_ + LOUPE_OFFSET + side + 8.0 > viewport.0 {
            cx_ - LOUPE_OFFSET - side
        } else {
            cx_ + LOUPE_OFFSET
        };
        let ly = if cy_ + LOUPE_OFFSET + side + 40.0 > viewport.1 {
            cy_ - LOUPE_OFFSET - side - 30.0
        } else {
            cy_ + LOUPE_OFFSET
        };
        let cells: Vec<Hsla> = match &self.loupe {
            Some(frame) => (0..LOUPE_PIXELS * LOUPE_PIXELS)
                .map(|i| match frame.pixel_rgba(i % LOUPE_PIXELS, i / LOUPE_PIXELS) {
                    Some([r, g, b, _]) => rgb(u32::from_be_bytes([0, r, g, b])).into(),
                    None => gpui::black(),
                })
                .collect(),
            None => Vec::new(),
        };
        let color = self.under_cursor();
        let label = color.map(|c| format(c, self.format)).unwrap_or_default();
        let swatch = color.unwrap_or(0);
        div()
            .absolute()
            .left(px(lx))
            .top(px(ly))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .size(px(side))
                    .rounded(px(14.))
                    .overflow_hidden()
                    .border_2()
                    .border_color(gpui::white().opacity(0.85))
                    .shadow_lg()
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds_, _, window, _| {
                                let origin = bounds_.origin;
                                for (i, color) in cells.iter().enumerate() {
                                    let col = (i as i32 % LOUPE_PIXELS) as f32;
                                    let row = (i as i32 / LOUPE_PIXELS) as f32;
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(
                                                origin.x + px(col * LOUPE_CELL),
                                                origin.y + px(row * LOUPE_CELL),
                                            ),
                                            size(px(LOUPE_CELL), px(LOUPE_CELL)),
                                        ),
                                        *color,
                                    ));
                                }
                                let center = (LOUPE_PIXELS / 2) as f32 * LOUPE_CELL;
                                window.paint_quad(outline(
                                    Bounds::new(
                                        point(origin.x + px(center), origin.y + px(center)),
                                        size(px(LOUPE_CELL), px(LOUPE_CELL)),
                                    ),
                                    gpui::white(),
                                    BorderStyle::Solid,
                                ));
                            },
                        )
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .h(px(26.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .bg(rgb(swatch))
                    .text_color(ink_on(swatch))
                    .text_size(px(12.))
                    .font_family("Segoe UI")
                    .child(div().flex_1().child(label))
                    .child(div().text_size(px(10.)).opacity(0.7).child("R editar")),
            )
    }

    fn render_rose(&self, rose: &Rose, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let color = rose.color();
        let (plane, hue) = rose_parts(rose.at);
        let pure = hsv_to_rgb(rose.h, 1.0, 1.0);
        let format_button = |id: &'static str, fmt: Format, active: bool| {
            div()
                .id(id)
                .flex_1()
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .text_size(px(11.))
                .when(active, |el| el.bg(text.opacity(0.16)))
                .hover(|el| el.bg(text.opacity(0.09)))
                .child(fmt.label())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.format = fmt;
                    cx.notify();
                }))
        };
        let swatch = |id: (&'static str, usize), c: u32| {
            div()
                .id(id)
                .size(px(18.))
                .rounded(px(9.))
                .bg(rgb(c))
                .border_1()
                .border_color(gpui::white().opacity(0.25))
                .cursor_pointer()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_rose_color(c, cx)))
        };
        let hues: Vec<_> = (0..12)
            .map(|i| swatch(("color-hue", i), hsv_to_rgb(i as f32 * 30.0, 0.85, 0.95)).into_any_element())
            .collect();
        let recents: Vec<_> = self
            .recents
            .iter()
            .enumerate()
            .map(|(i, &c)| swatch(("color-recent", i), c).into_any_element())
            .collect();
        let s_marker = (plane.x + rose.s * plane.w, plane.y + (1.0 - rose.v) * plane.h);
        let h_marker = hue.x + rose.h / 359.0 * hue.w;

        div()
            .id("color-rose")
            .absolute()
            .left(px(rose.at.0))
            .top(px(rose.at.1))
            .w(px(ROSE_W))
            .p(px(ROSE_PAD))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(18.))
            .bg(rgb(0x1a1a18))
            .shadow_lg()
            .font_family("Segoe UI")
            .text_color(text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Muestra grande: un clic copia.
            .child(
                div()
                    .id("color-big")
                    .h(px(56.))
                    .rounded(px(12.))
                    .bg(rgb(color))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(ink_on(color))
                    .cursor_pointer()
                    .child(div().text_size(px(14.)).child(format(color, self.format)))
                    .child(div().text_size(px(11.)).opacity(0.75).child("Enter copia"))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.pick(color, cx))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(format_button("color-hex", Format::Hex, self.format == Format::Hex))
                    .child(format_button("color-rgb", Format::Rgb, self.format == Format::Rgb))
                    .child(format_button("color-hsl", Format::Hsl, self.format == Format::Hsl)),
            )
            // Cuadro: blanco → matiz a lo ancho, transparente → negro a lo alto.
            .child(
                div()
                    .id("color-plane")
                    .relative()
                    .h(px(PLANE_H))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(gpui::white(), 0.),
                        linear_color_stop(rgb(pure), 1.),
                    ))
                    .cursor(CursorStyle::Crosshair)
                    .child(div().absolute().size_full().bg(linear_gradient(
                        180.,
                        linear_color_stop(gpui::black().opacity(0.), 0.),
                        linear_color_stop(gpui::black(), 1.),
                    )))
                    .child(
                        div()
                            .absolute()
                            .left(px(s_marker.0 - plane.x - 7.0))
                            .top(px(s_marker.1 - plane.y - 7.0))
                            .size(px(14.))
                            .rounded(px(7.))
                            .border_2()
                            .border_color(gpui::white())
                            .shadow_md(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &MouseDownEvent, _, cx| {
                            view.drag = Some(Drag::Plane);
                            view.drag_to((f32::from(event.position.x), f32::from(event.position.y)), cx);
                        }),
                    ),
            )
            // Matiz: 60 franjas, GPUI solo hace degradados de dos paradas.
            .child(
                div()
                    .id("color-hue")
                    .relative()
                    .h(px(HUE_H))
                    .rounded(px(7.))
                    .overflow_hidden()
                    .flex()
                    .children((0..60).map(|i| {
                        div()
                            .flex_1()
                            .h_full()
                            .bg(rgb(hsv_to_rgb(i as f32 * 6.0, 1.0, 1.0)))
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(h_marker - hue.x - 3.0))
                            .top(px(0.))
                            .w(px(6.))
                            .h_full()
                            .rounded(px(3.))
                            .border_2()
                            .border_color(gpui::white()),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &MouseDownEvent, _, cx| {
                            view.drag = Some(Drag::Hue);
                            view.drag_to((f32::from(event.position.x), f32::from(event.position.y)), cx);
                        }),
                    ),
            )
            .child(
                div()
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .bg(text.opacity(0.07))
                    .text_size(px(12.))
                    .child(div().text_size(px(10.)).opacity(0.6).child("HEX"))
                    .child(div().flex_1().child(self.hex.clone())),
            )
            .child(div().flex().justify_between().children(hues))
            .child(div().text_size(px(10.)).opacity(0.6).child("Recientes"))
            .child(div().flex().gap(px(6.)).h(px(18.)).children(recents))
    }
}

impl Render for ColorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.meter.frame(Instant::now());
        // En vivo: una lectura por cuadro mientras no se edita en la rosa.
        if self.rose.is_none() {
            self.sample();
        }
        let viewport = window.viewport_size();
        let viewport = (f32::from(viewport.width), f32::from(viewport.height));
        let hint = if self.rose.is_some() {
            "Arrastra en el cuadro y el matiz · Enter copia · Esc vuelve a la lupa"
        } else {
            "Clic o Enter copia · Flechas: 1 px (Shift 10) · Tab: formato · R: editar · Esc"
        };
        div()
            .id("color")
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_pick))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(Self::toggle_rose))
            .on_action(cx.listener(|view, _: &CycleFormat, _, cx| {
                view.format = view.format.next();
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &NudgeLeft, _, cx| v.nudge(-1, 0, cx)))
            .on_action(cx.listener(|v, _: &NudgeRight, _, cx| v.nudge(1, 0, cx)))
            .on_action(cx.listener(|v, _: &NudgeUp, _, cx| v.nudge(0, -1, cx)))
            .on_action(cx.listener(|v, _: &NudgeDown, _, cx| v.nudge(0, 1, cx)))
            .on_action(cx.listener(|v, _: &JumpLeft, _, cx| v.nudge(-10, 0, cx)))
            .on_action(cx.listener(|v, _: &JumpRight, _, cx| v.nudge(10, 0, cx)))
            .on_action(cx.listener(|v, _: &JumpUp, _, cx| v.nudge(0, -10, cx)))
            .on_action(cx.listener(|v, _: &JumpDown, _, cx| v.nudge(0, 10, cx)))
            .size_full()
            .relative()
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                let p = (f32::from(event.position.x), f32::from(event.position.y));
                if view.drag.is_some() {
                    view.drag_to(p, cx);
                } else if view.rose.is_none() {
                    view.cursor = p;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| {
                    view.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    if view.rose.is_some() {
                        // Fuera de la rosa: tomar el color de ahí y seguir
                        // editando.
                        view.cursor = (f32::from(event.position.x), f32::from(event.position.y));
                        view.sample();
                        if let Some(color) = view.under_cursor() {
                            view.set_rose_color(color, cx);
                        }
                        window.focus(&view.focus);
                    } else if let Some(color) = view.under_cursor() {
                        view.pick(color, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _: &MouseDownEvent, _, cx| cx.emit(ColorEvent::Cancelled)),
            )
            .when(self.rose.is_none(), |el| el.child(self.render_loupe(viewport)))
            .when_some(self.rose.as_ref(), |el, rose| el.child(self.render_rose(rose, cx)))
            .child(
                div()
                    .absolute()
                    .top(px(10.))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(chip(hint)),
            )
    }
}

// --- En la pill -----------------------------------------------------------------

impl crate::Pill {
    pub(crate) fn start_color(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.color.is_some() || self.capture.is_some() || self.board.is_some() {
            return;
        }
        let now = Instant::now();
        self.close_wheel();
        self.close_panel(false, cx);
        self.peek.set(0.0, now);
        self.strip.set(0.0, now);
        let previous = crate::paste::foreground_target();
        // Píxel físico de la esquina lógica (0, 0) de la ventana.
        let scale = self.scale_factor;
        let all = monitors::enumerate();
        let monitor = all
            .iter()
            .find(|monitor| monitor.is_primary)
            .or(all.first())
            .map(|monitor| monitor.bounds)
            .unwrap_or(PhysRect::new(0, 0, 0, 0));
        let origin = (
            monitor.x - (self.monitor.x * scale).round() as i32,
            monitor.y - (self.monitor.y * scale).round() as i32,
        );
        let cursor = self.cursor.unwrap_or_default();
        let view = cx.new(|cx| ColorView::new(origin, scale, cursor, cx));
        self.color_events = Some(cx.subscribe(&view, move |pill, _, event: &ColorEvent, cx| {
            pill.end_color(event, previous, cx)
        }));
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_passthrough(false);
            overlay.set_focusable(true);
        }
        window.focus(&view.focus_handle(cx));
        self.color = Some(view);
        cx.notify();
    }

    fn end_color(
        &mut self,
        event: &ColorEvent,
        previous: Option<crate::paste::Target>,
        cx: &mut Context<Self>,
    ) {
        if self.color.take().is_none() {
            return;
        }
        self.color_events = None;
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(false);
        }
        if let Some(target) = previous {
            crate::paste::force_foreground(target);
        }
        if let ColorEvent::Picked { color, text } = event {
            println!("color: {text} copiado");
            // Cuelga del notch con la burbuja, como vista previa del color.
            let (r, g, b) = channels(*color);
            let entry = crate::clipboard::Entry::new(
                0,
                "color".into(),
                crate::clipboard::Content::Color(text.clone().into(), rgb(u32::from_be_bytes([0, r, g, b])).into()),
                &format!("{text} · copiado"),
            );
            self.toast = Some((entry, Instant::now() + std::time::Duration::from_millis(1800)));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatos() {
        assert_eq!(format(0xE5484D, Format::Hex), "#E5484D");
        assert_eq!(format(0xE5484D, Format::Rgb), "rgb(229, 72, 77)");
        assert_eq!(format(0xFFFFFF, Format::Hsl), "hsl(0, 0%, 100%)");
        assert_eq!(format(0x00FF00, Format::Hsl), "hsl(120, 100%, 50%)");
    }

    #[test]
    fn hsv_ida_y_vuelta() {
        for color in [0xE5484D, 0x30A46C, 0x3E8BFF, 0x111111, 0xFFD60A] {
            let (h, s, v) = rgb_to_hsv(color);
            assert_eq!(hsv_to_rgb(h, s, v), color, "{color:06X}");
        }
    }

    #[test]
    fn hex_corto_y_largo() {
        assert_eq!(parse_hex("#abc"), Some(0xAABBCC));
        assert_eq!(parse_hex("E5484D"), Some(0xE5484D));
        assert_eq!(parse_hex("#12"), None);
    }
}
