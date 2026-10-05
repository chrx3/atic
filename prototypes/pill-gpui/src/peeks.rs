//! Vistazos de las herramientas de la tira.
//!
//! Como `toolPeek` de la pill web: quedarse sobre una herramienta del notch
//! baja una franja con lo último de esa herramienta, usable ahí mismo:
//! - **Color**: los últimos colores; pasar por uno muestra su código y un
//!   clic lo copia.
//! - **Capturas**: las tres últimas; un clic copia la imagen y arrastrarla
//!   la suelta como archivo en otra app (un chat, una carpeta, un correo).
//! - **Textos**: los tres guardados más recientes; un clic los pega donde
//!   estabas.
//! - **Flip**: las páginas del tablero en miniatura; un clic abre esa.
//! - **Sistema**: CPU, memoria y lo que más consume.
//!
//! Aparece a los 450 ms (120 ms si ya había otro abierto: pasar de una
//! herramienta a la vecina), se queda mientras el cursor esté en la
//! herramienta o en el vistazo y se va 400 ms después de salir. Clipboard y
//! Agentes tienen los suyos (`peek` en `main.rs` y `usage.rs`); entre los
//! tres hay relevo: con uno abierto, el de la herramienta vecina baja a los
//! 120 ms y el anterior se recoge recién entonces, así el notch cambia de
//! forma en vez de cerrarse y volver a abrirse.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    div, img, prelude::*, px, rgb, svg, AnyElement, ClipboardItem, Context, Hsla, MouseButton,
    MouseDownEvent, ObjectFit, Window,
};

use crate::anim::{ease_island, ease_smooth_out, segment, Tween};
use crate::geometry::Rect;
use crate::hover::HoverExt;
use crate::usage::Drawer;
use crate::Pill;

const SHOW_DELAY: Duration = Duration::from_millis(450);
/// Con otro vistazo abierto: pasar a la herramienta vecina.
pub(crate) const SWITCH_DELAY: Duration = Duration::from_millis(120);
const GRACE: Duration = Duration::from_millis(400);
/// Tras copiar, el «Copiado» se ve un momento y el vistazo se va.
const COPIED_FOR: Duration = Duration::from_millis(650);
/// Lo que hay que mover el cursor con el botón apretado para que sea
/// arrastre y no clic (como en la web).
const DRAG_START: f32 = 4.0;

const SIDE: f32 = 14.0;
const TITLE_H: f32 = 30.0;
const FOOTER_H: f32 = 32.0;
const BOTTOM: f32 = 6.0;
const EMPTY_H: f32 = 30.0;
const SWATCH: f32 = 30.0;
const TEXT_ROW_H: f32 = 40.0;
const METER_H: f32 = 24.0;
const APP_ROW_H: f32 = 24.0;
const SECTION_H: f32 = 22.0;
const SHOT_GAP: f32 = 8.0;
const SHOT_LABEL_H: f32 = 18.0;
const PAGE_GAP: f32 = 8.0;
const PAGE_LABEL_H: f32 = 30.0;
const MAX_PAGES: usize = 4;
// En el bloque angosto de un costado lo que arriba va en fila va en columna:
// una fila por color, por captura o por página, con su miniatura al lado.
const COLOR_ROW_H: f32 = 28.0;
const SIDE_COLORS: usize = 6;
const SHOT_ROW_H: f32 = 56.0;
const SHOT_THUMB: (f32, f32) = (80.0, 50.0);
const PAGE_ROW_H: f32 = 56.0;
const PAGE_THUMB: (f32, f32) = (64.0, 48.0);

/// Los tres vistazos de la tira.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeekKind {
    Clipboard,
    Usage,
    Tool,
}

/// La herramienta tiene algún vistazo (aquí, Clipboard o Agentes).
pub(crate) fn any_peek(tool: usize) -> bool {
    tool == crate::CLIPBOARD_TOOL || tool == crate::AGENTES_TOOL || has_peek(tool)
}

/// Las herramientas con vistazo aquí.
fn has_peek(tool: usize) -> bool {
    [crate::TEXTOS_TOOL, crate::SISTEMA_TOOL, crate::capture::TOOL, crate::color::TOOL, crate::flip::TOOL]
        .contains(&tool)
}

#[derive(Clone)]
struct Shot {
    path: PathBuf,
    /// «18:10», o «ayer 18:10».
    label: String,
}

#[derive(Default)]
struct Data {
    colors: Vec<u32>,
    shots: Vec<Shot>,
    texts: Vec<(String, String)>,
    pages: Vec<crate::flip_board::PageGlance>,
}

pub(crate) struct ToolPeek {
    /// Cuánto bajó (0 a 1).
    amount: Tween,
    /// El alto del contenido: cambia con transición al pasar de una
    /// herramienta a otra.
    height: Tween,
    /// Lo que se muestra.
    tool: Option<usize>,
    /// Cuándo cambió lo que se muestra: el contenido nuevo entra fundiéndose.
    shown_at: Instant,
    /// La herramienta bajo el cursor y desde cuándo.
    hover: Option<(usize, Instant)>,
    leave_at: Option<Instant>,
    data: Data,
    /// Lo que se acaba de copiar (clave del elemento) y cuándo.
    copied: Option<(String, Instant)>,
    /// El color bajo el cursor: la fila de abajo muestra su código.
    hovered_color: Option<u32>,
    /// Una captura apretada: dónde empezó. Soltar sin moverse la copia;
    /// moverse la arrastra.
    press: Option<(PathBuf, (f32, f32))>,
    /// CPU y memoria de Sistema (0 a 1): entre lecturas se deslizan al
    /// valor nuevo en vez de saltar.
    cpu: Tween,
    ram: Tween,
}

/// El reloj de la entrada del vistazo: cada cosa aparece un poco después
/// de la anterior y los números cuentan desde cero, como en el de Agentes.
#[derive(Clone, Copy)]
struct Motion {
    /// Segundos desde que se mostró este contenido.
    t: f32,
}

impl Motion {
    /// El elemento `i` de una lista: cae desde un poco más arriba y se
    /// funde, en cascada.
    fn enter(&self, i: usize) -> f32 {
        ease_smooth_out(segment(self.t, 0.10 + i as f32 * 0.055, 0.42))
    }

    /// Los números y las barras: de cero a su valor.
    fn count(&self) -> f32 {
        ease_smooth_out(segment(self.t, 0.12, 0.75))
    }
}

/// El título letra por letra: cada una cae a su sitio tras la anterior.
pub(crate) fn falling(text: &'static str, t: f32) -> impl IntoElement {
    div().flex().children(text.chars().enumerate().map(move |(i, ch)| {
        let e = ease_smooth_out(segment(t, 0.06 + i as f32 * 0.016, 0.32));
        // Un espacio solo se colapsaría: el duro mantiene el ancho.
        let ch = if ch == ' ' { '\u{00a0}' } else { ch };
        div()
            .relative()
            .top(px(-6.0 * (1.0 - e)))
            .opacity(e)
            .child(ch.to_string())
    }))
}

impl ToolPeek {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            amount: Tween::new(0.0, Duration::from_millis(300), ease_island),
            height: Tween::new(0.0, Duration::from_millis(240), ease_island),
            tool: None,
            shown_at: now,
            hover: None,
            leave_at: None,
            data: Data::default(),
            copied: None,
            hovered_color: None,
            press: None,
            cpu: Tween::new(0.0, Duration::from_millis(600), ease_smooth_out),
            ram: Tween::new(0.0, Duration::from_millis(600), ease_smooth_out),
        }
    }

    fn load(&mut self, tool: usize) {
        match tool {
            t if t == crate::color::TOOL => self.data.colors = recent_colors(),
            t if t == crate::capture::TOOL => {
                self.data.shots = crate::flip_board::recent_captures()
                    .into_iter()
                    .take(3)
                    .map(|path| Shot {
                        label: shot_label(&path),
                        path,
                    })
                    .collect()
            }
            t if t == crate::TEXTOS_TOOL => {
                self.data.texts = crate::snippets::texts().into_iter().take(3).collect()
            }
            t if t == crate::flip::TOOL => self.data.pages = crate::flip_board::board_glance(),
            _ => {}
        }
    }

    /// El alto del contenido de cada vistazo, con su título y su pie. `narrow`:
    /// el bloque de un costado, con las filas en columna.
    fn content_height(&self, tool: usize, width: f32, narrow: bool) -> f32 {
        let body = match tool {
            t if t == crate::color::TOOL => {
                if self.data.colors.is_empty() {
                    EMPTY_H
                } else if narrow {
                    self.data.colors.len().min(SIDE_COLORS) as f32 * COLOR_ROW_H
                } else {
                    SWATCH + 10.0 + 22.0
                }
            }
            t if t == crate::capture::TOOL => {
                if self.data.shots.is_empty() {
                    EMPTY_H
                } else if narrow {
                    self.data.shots.len() as f32 * SHOT_ROW_H
                } else {
                    shot_height(width) + SHOT_LABEL_H
                }
            }
            t if t == crate::TEXTOS_TOOL => {
                if self.data.texts.is_empty() {
                    EMPTY_H
                } else {
                    self.data.texts.len() as f32 * TEXT_ROW_H
                }
            }
            t if t == crate::flip::TOOL && narrow => self.data.pages.len().clamp(1, MAX_PAGES) as f32 * PAGE_ROW_H,
            t if t == crate::flip::TOOL => page_size(width).1 + PAGE_LABEL_H,
            t if t == crate::SISTEMA_TOOL => METER_H * 2.0 + 6.0 + SECTION_H + APP_ROW_H * 3.0,
            _ => 0.0,
        };
        TITLE_H + body + FOOTER_H + BOTTOM
    }
}

/// El ancho del vistazo: el de la tira abierta.
fn peek_width() -> f32 {
    crate::strip_open_length()
}

fn shot_height(width: f32) -> f32 {
    let w = (width - SIDE * 2.0 - SHOT_GAP * 2.0) / 3.0;
    w * 10.0 / 16.0
}

/// El tamaño de la miniatura de una página (1200×900).
fn page_size(width: f32) -> (f32, f32) {
    let w = (width - SIDE * 2.0 - PAGE_GAP * (MAX_PAGES as f32 - 1.0)) / MAX_PAGES as f32;
    (w, w * 0.75)
}

/// «18:10» si es de hoy, «ayer 18:10», o «4 oct 18:10».
fn shot_label(path: &PathBuf) -> String {
    let Some(modified) = std::fs::metadata(path).and_then(|m| m.modified()).ok() else {
        return String::new();
    };
    let when: chrono::DateTime<chrono::Local> = modified.into();
    let today = chrono::Local::now().date_naive();
    let day = when.date_naive();
    if day == today {
        when.format("%H:%M").to_string()
    } else if today.pred_opt() == Some(day) {
        format!("ayer {}", when.format("%H:%M"))
    } else {
        const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
        use chrono::Datelike;
        format!("{} {} {}", day.day(), MONTHS[day.month0() as usize], when.format("%H:%M"))
    }
}

/// Los colores tomados con Color y, después, los que pasaron por el
/// portapapeles (un `#hex` o un `rgb()` copiado): sin repetir, hasta 10.
fn recent_colors() -> Vec<u32> {
    let mut colors = crate::color::load_recents();
    let copied = crate::history::dir()
        .and_then(|dir| crate::history::load_if_changed(&dir, None))
        .map(|(_, entries)| entries)
        .unwrap_or_default();
    for entry in copied.iter().take(60) {
        if let crate::clipboard::Content::Color(_, color) = &entry.content {
            let c = color.to_rgb();
            let value = ((c.r * 255.0).round() as u32) << 16 | ((c.g * 255.0).round() as u32) << 8 | (c.b * 255.0).round() as u32;
            if !colors.contains(&value) {
                colors.push(value);
            }
        }
    }
    colors.truncate(10);
    colors
}

fn hex(color: u32) -> String {
    crate::color::format(color, crate::color::Format::Hex).to_uppercase()
}

impl Pill {
    /// Hay otro vistazo abierto (o bajando) que no es `kind`.
    pub(crate) fn other_peek_open(&self, kind: PeekKind) -> bool {
        (kind != PeekKind::Clipboard && self.peek.target() == 1.0)
            || (kind != PeekKind::Usage && self.usage_peek.target() == 1.0)
            || (kind != PeekKind::Tool && self.tool_peek.amount.target() == 1.0)
    }

    /// Baja el vistazo `kind`: los otros se recogen a la vez (relevo).
    pub(crate) fn hand_off_peeks(&mut self, kind: PeekKind, now: Instant) {
        if kind != PeekKind::Clipboard && self.peek.target() == 1.0 {
            self.peek.set(0.0, now);
            self.peek_leave_at = None;
        }
        if kind != PeekKind::Usage && self.usage_peek.target() == 1.0 {
            self.close_usage_peek(now);
        }
        if kind != PeekKind::Tool && self.tool_peek.amount.target() == 1.0 {
            self.tool_peek.amount.set(0.0, now);
            self.tool_peek.leave_at = None;
            self.tool_peek.copied = None;
        }
    }

    /// Cada sondeo: baja, cambia o recoge el vistazo según dónde esté el
    /// cursor.
    pub(crate) fn update_tool_peek(
        &mut self,
        now: Instant,
        cursor: Option<(f32, f32)>,
        strip_hovered: Option<usize>,
        docked: bool,
        cx: &mut Context<Self>,
    ) {
        let usable = docked && self.docked_still() && !self.panel_visible() && self.strip.target() == 1.0;
        let on_tool = strip_hovered.filter(|t| has_peek(*t) && usable);
        let open = self.tool_peek.amount.target() == 1.0;
        let relay = open || self.other_peek_open(PeekKind::Tool);

        // La herramienta bajo el cursor: los datos se piden apenas llega,
        // para que al bajar ya estén.
        match (on_tool, self.tool_peek.hover) {
            (Some(tool), Some((was, _))) if tool == was => {}
            (Some(tool), _) => {
                self.tool_peek.hover = Some((tool, now));
                self.tool_peek.load(tool);
                if tool == crate::SISTEMA_TOOL {
                    self.system.update(cx, |panel, cx| panel.set_peek(true, false, cx));
                }
            }
            (None, _) => self.tool_peek.hover = None,
        }

        if let Some((tool, since)) = self.tool_peek.hover {
            let wait = if relay { SWITCH_DELAY } else { SHOW_DELAY };
            if self.tool_peek.tool != Some(tool) || !open {
                if now.duration_since(since) >= wait {
                    if self.tool_peek.tool != Some(tool) {
                        self.tool_peek.shown_at = now;
                        self.tool_peek.hovered_color = None;
                        self.tool_peek.copied = None;
                    }
                    self.tool_peek.tool = Some(tool);
                    self.tool_peek.amount.set(1.0, now);
                    self.hand_off_peeks(PeekKind::Tool, now);
                }
            }
        }

        // Una captura apretada: arrastre si el cursor se movió, clic (copia)
        // si se soltó sin moverse.
        if let Some((path, origin)) = self.tool_peek.press.clone() {
            if cursor.is_some_and(|c| (c.0 - origin.0).hypot(c.1 - origin.1) >= DRAG_START) {
                self.tool_peek.press = None;
                self.close_tool_peek(now, cx);
                self.drag_shot(path, cx);
            } else if !crate::win::left_button_down() {
                self.tool_peek.press = None;
                let key = path.to_string_lossy().to_string();
                self.copy_shot(path, key, cx);
            }
        }

        // Quedarse o irse.
        if self.tool_peek.amount.target() == 1.0 {
            let on_peek = cursor.is_some_and(|c| self.tool_peek_over(c, now));
            // Sobre Clipboard o Agentes se espera el relevo: este se
            // recoge cuando aquel baja (`hand_off_peeks`).
            let relay_to = strip_hovered.is_some_and(|t| !has_peek(t) && any_peek(t));
            let copied_done = self
                .tool_peek
                .copied
                .as_ref()
                .is_some_and(|(_, at)| now.duration_since(*at) >= COPIED_FOR);
            if !usable || copied_done {
                self.close_tool_peek(now, cx);
            } else if on_tool.is_some() || on_peek || relay_to || self.tool_peek.press.is_some() {
                self.tool_peek.leave_at = None;
            } else {
                let since = *self.tool_peek.leave_at.get_or_insert(now);
                if now.duration_since(since) >= GRACE {
                    self.close_tool_peek(now, cx);
                }
            }
        }

        if self.tool_peek.tool == Some(crate::SISTEMA_TOOL) {
            if let Some(procs) = self.system.read(cx).snapshot().procs.as_ref() {
                let ram = if procs.ram_total > 0 {
                    procs.ram_used as f32 / procs.ram_total as f32
                } else {
                    0.0
                };
                self.tool_peek.cpu.set((procs.cpu / 100.0).clamp(0.0, 1.0), now);
                self.tool_peek.ram.set(ram.clamp(0.0, 1.0), now);
            }
        }

        let narrow = self.side_drawers();
        let width = if narrow { crate::SIDE_W } else { peek_width() };
        if let Some(tool) = self.tool_peek.tool {
            let h = self.tool_peek.content_height(tool, width, narrow);
            if self.tool_peek.height.target() == 0.0 {
                self.tool_peek.height.snap(h);
            } else {
                self.tool_peek.height.set(h, now);
            }
        }
        // El vistazo se fue del todo: Sistema deja de medir procesos.
        if self.tool_peek.amount.value(now) <= 0.001 && self.tool_peek.amount.target() == 0.0 {
            if self.tool_peek.tool.take() == Some(crate::SISTEMA_TOOL) || self.tool_peek.hover.is_none() {
                let keep = self.panel_visible() && self.notch_tool == crate::NotchTool::Sistema;
                self.system.update(cx, |panel, cx| panel.set_peek(false, keep, cx));
            }
        }
    }

    /// La herramienta del vistazo abierto (o bajando), si hay uno: el bloque
    /// de un costado sale a su altura.
    pub(crate) fn tool_peek_owner(&self) -> Option<usize> {
        (self.tool_peek.amount.target() == 1.0).then_some(self.tool_peek.tool).flatten()
    }

    fn close_tool_peek(&mut self, now: Instant, cx: &mut Context<Self>) {
        self.tool_peek.amount.set(0.0, now);
        self.tool_peek.leave_at = None;
        self.tool_peek.copied = None;
        let _ = cx;
    }

    /// Cuánto bajó (0 a 1): bajo la tira arriba, al lado de la columna en un
    /// costado.
    pub(crate) fn tool_peek_amount(&self, now: Instant) -> f32 {
        self.tool_peek.amount.value(now).clamp(0.0, 1.2)
    }

    pub(crate) fn tool_peek_height(&self, now: Instant) -> f32 {
        self.tool_peek.height.value(now)
    }

    /// El ancho del tab con el vistazo: el de la tira abierta.
    pub(crate) fn tool_peek_length(length: f32, amount: f32) -> f32 {
        length + (peek_width().max(length) - length) * amount.clamp(0.0, 1.0)
    }

    pub(crate) fn tool_peek_animating(&self, now: Instant) -> bool {
        self.tool_peek.amount.is_running(now)
            || self.tool_peek.height.is_running(now)
            || (self.tool_peek.amount.value(now) > 0.01
                && now.duration_since(self.tool_peek.shown_at) < Duration::from_millis(1400))
            || self.tool_peek.cpu.is_running(now)
            || self.tool_peek.ram.is_running(now)
    }

    fn tool_peek_rect(&self, now: Instant) -> Option<Rect> {
        if self.tool_peek.amount.value(now) <= 0.01 {
            return None;
        }
        self.drawer_rect(Drawer::Tool, now)
    }

    pub(crate) fn tool_peek_over(&self, p: (f32, f32), now: Instant) -> bool {
        self.tool_peek_rect(now).is_some_and(|r| r.contains(p, 4.0))
    }

    /// El vistazo dibujado bajo la tira.
    pub(crate) fn tool_peek_element(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        let area = self.tool_peek_rect(now)?;
        let tool = self.tool_peek.tool?;
        let amount = self.tool_peek.amount.value(now);
        let palette = crate::Palette::dark();
        let c = Colors {
            text: palette.text,
            muted: palette.muted,
            faint: rgb(0x8f8f86).into(),
            ok: rgb(0x6faf88).into(),
        };
        let width = area.w;
        // En el bloque angosto de un costado: filas en columna y sin la
        // ayuda junto al título (no cabe).
        let narrow = self.side_drawers();
        let m = Motion {
            t: now.duration_since(self.tool_peek.shown_at).as_secs_f32(),
        };
        let (title, icon, body, footer) = match tool {
            t if t == crate::color::TOOL => ("Colores recientes", "icons/pipette.svg", self.color_body(narrow, &c, m, cx), "Tomar un color"),
            t if t == crate::capture::TOOL => ("Capturas recientes", "icons/crop.svg", self.shots_body(width, narrow, &c, m, cx), "Nueva captura"),
            t if t == crate::TEXTOS_TOOL => ("Textos", "icons/text-align-start.svg", self.texts_body(&c, m, cx), "Ver todos los textos"),
            t if t == crate::flip::TOOL => ("Tablero", "icons/flip.svg", self.pages_body(width, narrow, &c, m, cx), "Abrir Flip"),
            t if t == crate::SISTEMA_TOOL => ("Sistema", "icons/cpu.svg", self.system_body(&c, m, now, cx), "Abrir Sistema"),
            _ => return None,
        };
        let hint = match tool {
            t if t == crate::color::TOOL => "clic copia el código",
            t if t == crate::capture::TOOL => "clic copia · arrastra a otra app",
            t if t == crate::TEXTOS_TOOL => "clic pega donde estabas",
            t if t == crate::flip::TOOL => "clic abre la página",
            _ => "",
        };
        // El contenido entra cuando la franja ya casi bajó, y al cambiar de
        // herramienta el nuevo se funde (la franja cambia de alto a la vez).
        let reveal = segment(amount, 0.55, 0.45);
        let switched = segment(now.duration_since(self.tool_peek.shown_at).as_secs_f32(), 0.06, 0.18);
        let open_tool = cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            pill.open_from_peek(tool, window, cx);
        });
        Some(
            div()
                .id("tool-peek")
                .absolute()
                .left(px(area.x))
                .top(px(area.y))
                .w(px(area.w))
                .h(px(area.h))
                .overflow_hidden()
                .font_family("Segoe UI")
                .text_color(c.text)
                // Un clic en el fondo no arrastra el notch.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .opacity(reveal)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .opacity(switched)
                        .child(
                            div()
                                .h(px(TITLE_H))
                                .px(px(SIDE))
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(
                                    svg()
                                        .path(icon)
                                        .size(px(13.))
                                        .text_color(c.muted)
                                        .opacity(m.enter(0)),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(falling(title, m.t)),
                                )
                                .child(div().flex_1())
                                .when(!narrow, |el| {
                                    el.child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(c.faint)
                                            .opacity(ease_smooth_out(segment(m.t, 0.35, 0.4)))
                                            .child(hint),
                                    )
                                }),
                        )
                        .child(body)
                        .child(
                            div()
                                .h(px(FOOTER_H))
                                .px(px(SIDE))
                                .flex()
                                .items_center()
                                .opacity(ease_smooth_out(segment(m.t, 0.30, 0.4)))
                                .child(
                                div()
                                    .id("tool-peek-open")
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(11.))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, open_tool)
                                    .fx("tool-peek-open-fx", {
                                        let (faint, text) = (c.faint, c.text);
                                        move |el, h| {
                                            let fg = h.mix(faint, text);
                                            el.text_color(fg)
                                                .child(footer)
                                                .child(
                                                    svg()
                                                        .path("icons/arrow-up-right.svg")
                                                        .size(px(11.))
                                                        .text_color(fg)
                                                        .ml(px(2.0 * h.t)),
                                                )
                                        }
                                    }),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    fn open_from_peek(&mut self, tool: usize, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.close_tool_peek(now, cx);
        self.tool_peek.amount.snap(0.0);
        match tool {
            t if t == crate::TEXTOS_TOOL => self.open_notch(crate::NotchTool::Textos, window, cx),
            t if t == crate::SISTEMA_TOOL => self.open_notch(crate::NotchTool::Sistema, window, cx),
            _ => self.run_tool(tool, window, cx),
        }
    }

    fn mark_copied(&mut self, key: String, cx: &mut Context<Self>) {
        self.tool_peek.copied = Some((key, Instant::now()));
        cx.notify();
    }

    fn is_copied(&self, key: &str) -> bool {
        self.tool_peek.copied.as_ref().is_some_and(|(k, _)| k == key)
    }

    // --- Color --------------------------------------------------------------

    fn color_body(&self, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.tool_peek.data.colors.clone();
        if colors.is_empty() {
            return empty("Todavía no has tomado colores", c);
        }
        if narrow {
            return self.color_rows(colors, c, m, cx);
        }
        let shown = self
            .tool_peek
            .hovered_color
            .filter(|h| colors.contains(h))
            .unwrap_or(colors[0]);
        let copied = colors.iter().find(|&&color| self.is_copied(&hex(color))).copied();
        let shown = copied.unwrap_or(shown);
        let text = c.text;
        let swatches = colors.into_iter().take(10).enumerate().map(|(i, color)| {
            let key = hex(color);
            let on = Some(color) == copied;
            div()
                .id(("peek-color", i))
                .size(px(SWATCH))
                .relative()
                .cursor_pointer()
                .on_hover(cx.listener(move |pill, hovered: &bool, _, cx| {
                    if *hovered {
                        pill.tool_peek.hovered_color = Some(color);
                    } else if pill.tool_peek.hovered_color == Some(color) {
                        pill.tool_peek.hovered_color = None;
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.write_to_clipboard(ClipboardItem::new_string(key.clone()));
                        pill.mark_copied(key.clone(), cx);
                    }),
                )
                .fx(("peek-color-fx", i), move |el, h| {
                    // Crece con el cursor (como `.op-swatch` de la web) sin
                    // mover a sus vecinas; el copiado queda grande. Al abrir,
                    // cada uno brota desde su centro tras el anterior.
                    let e = m.enter(i);
                    let grow = if on { 3.0 } else { 3.0 * h.t - 2.0 * h.press } - (1.0 - e) * SWATCH * 0.45;
                    let el = el.opacity(e);
                    el.child(
                        div()
                            .absolute()
                            .inset(px(-grow))
                            .rounded(px(SWATCH / 2.0 + grow))
                            .bg(rgb(color))
                            .border_1()
                            .border_color(text.opacity(0.18 + 0.3 * h.t)),
                    )
                })
                .into_any_element()
        });
        let ok = c.ok;
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().h(px(SWATCH)).flex().gap(px(9.)).children(swatches))
            .child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .opacity(m.enter(3))
                    .child(div().size(px(10.)).rounded(px(5.)).bg(rgb(shown)))
                    .child(div().child(hex(shown)))
                    .child(div().text_color(c.faint).child(rgb_text(shown)))
                    .when(copied.is_some(), |el| el.child(div().text_color(ok).child("· Copiado"))),
            )
            .into_any_element()
    }

    /// Los colores en columna, para el bloque angosto de un costado: una fila
    /// por color con su código; clic lo copia.
    fn color_rows(&self, colors: Vec<u32>, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let (text, faint, ok) = (c.text, c.faint, c.ok);
        let rows = colors.into_iter().take(SIDE_COLORS).enumerate().map(|(i, color)| {
            let key = hex(color);
            let copied = self.is_copied(&key);
            let e = m.enter(i);
            div()
                .id(("peek-color-row", i))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(COLOR_ROW_H - 2.0))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(9.))
                .cursor_pointer()
                .text_size(px(12.))
                .on_mouse_down(MouseButton::Left, {
                    let key = key.clone();
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.write_to_clipboard(ClipboardItem::new_string(key.clone()));
                        pill.mark_copied(key.clone(), cx);
                    })
                })
                .child(
                    div()
                        .size(px(18.))
                        .flex_none()
                        .rounded(px(9.))
                        .bg(rgb(color))
                        .border_1()
                        .border_color(text.opacity(0.18)),
                )
                .child(div().child(key))
                .child(div().flex_1().min_w_0().truncate().text_color(faint).child(rgb_text(color)))
                .when(copied, |el| el.child(div().flex_none().text_color(ok).child("Copiado")))
                .hover_bg(("peek-color-row-fx", i), text.opacity(0.0), text.opacity(0.08))
                .into_any_element()
        });
        div()
            .px(px(SIDE - 8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows)
            .into_any_element()
    }

    // --- Capturas -----------------------------------------------------------

    fn shots_body(&self, width: f32, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let shots = self.tool_peek.data.shots.clone();
        if shots.is_empty() {
            return empty("Todavía no hay capturas", c);
        }
        // Angosto: una captura por fila, con la miniatura a la izquierda.
        let h = if narrow { SHOT_THUMB.1 } else { shot_height(width) };
        let (text, faint, ok) = (c.text, c.faint, c.ok);
        let cells = shots.into_iter().enumerate().map(|(i, shot)| {
            let key = shot.path.to_string_lossy().to_string();
            let copied = self.is_copied(&key);
            let path = shot.path.clone();
            div()
                .id(("peek-shot", i))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .when(narrow, |el| el.flex_row().items_center().gap(px(10.)).h(px(SHOT_ROW_H - 6.0)))
                .cursor_grab()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let at = (f32::from(event.position.x), f32::from(event.position.y));
                        pill.tool_peek.press = Some((path.clone(), at));
                    }),
                )
                .fx(("peek-shot-fx", i), move |el, hv| {
                    // La imagen se acerca un poco (`scale(1.04)` en la web) y
                    // su borde se enciende. Al abrir, cada una cae a su sitio
                    // tras la anterior, alejándose desde un poco más cerca.
                    let e = m.enter(i);
                    let zoom = 1.0 + 0.06 * hv.t - 0.03 * hv.press + 0.12 * (1.0 - e);
                    el.relative().top(px(-8.0 * (1.0 - e))).opacity(e).child(
                        div()
                            .h(px(h))
                            .w_full()
                            .when(narrow, |el| el.w(px(SHOT_THUMB.0)).flex_none())
                            .relative()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(text.opacity(0.06))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-(zoom - 1.0) * h / 2.0))
                                    .left(gpui::relative(-(zoom - 1.0) / 2.0))
                                    .w(gpui::relative(zoom))
                                    .h(px(h * zoom))
                                    .child(img(shot.path.clone()).size_full().object_fit(ObjectFit::Cover)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(text.opacity(0.12 + 0.35 * hv.t)),
                            ),
                    )
                    .child(
                        div()
                            .h(px(SHOT_LABEL_H - 3.0))
                            .flex()
                            .when(!narrow, |el| el.justify_center())
                            .when(narrow, |el| el.items_center().text_size(px(12.)))
                            .text_size(px(10.5))
                            .text_color(if copied { ok } else { hv.mix(faint, text) })
                            .child(if copied { "Copiada".to_string() } else { shot.label.clone() }),
                    )
                })
                .into_any_element()
        });
        div()
            .px(px(SIDE))
            .flex()
            .gap(px(SHOT_GAP))
            .when(narrow, |el| el.flex_col().gap(px(6.)))
            .children(cells)
            .into_any_element()
    }

    /// La captura como archivo hacia otra app. `DoDragDrop` corre un loop
    /// modal que sigue despachando a GPUI: en una tarea aparte, no dentro
    /// del manejador.
    fn drag_shot(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = path.to_string_lossy().into_owned();
        cx.spawn(async move |_, _| match crate::drag::drag_files(&[path]) {
            Ok(outcome) => println!("vistazo → arrastre de captura: soltado={}", outcome.dropped),
            Err(error) => eprintln!("vistazo: arrastre de captura: {error}"),
        })
        .detach();
    }

    fn copy_shot(&mut self, path: PathBuf, key: String, cx: &mut Context<Self>) {
        self.mark_copied(key, cx);
        cx.background_spawn(async move {
            let result = (|| -> Result<(), String> {
                let png = std::fs::read(&path).map_err(|e| e.to_string())?;
                let rgba = image::load_from_memory(&png).map_err(|e| e.to_string())?.to_rgba8();
                let (w, h) = rgba.dimensions();
                let mut bgra = rgba.into_raw();
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
                crate::clip_image::write(w, h, &bgra, &png)
            })();
            if let Err(error) = result {
                eprintln!("vistazo: no se pudo copiar la captura: {error}");
            }
        })
        .detach();
    }

    // --- Textos -------------------------------------------------------------

    fn texts_body(&self, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let texts = self.tool_peek.data.texts.clone();
        if texts.is_empty() {
            return empty("Todavía no tienes textos guardados", c);
        }
        let (text, faint) = (c.text, c.faint);
        let rows = texts.into_iter().enumerate().map(|(i, (name, body))| {
            let preview: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
            let e = m.enter(i);
            div()
                .id(("peek-text", i))
                .relative()
                .top(px(-7.0 * (1.0 - e)))
                .opacity(e)
                .h(px(TEXT_ROW_H - 2.0))
                .px(px(8.))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(1.))
                .rounded(px(10.))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.paste_text_from_peek(body.clone(), cx);
                    }),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child(name)),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(faint).child(preview)),
                )
                .hover_bg(("peek-text-fx", i), text.opacity(0.0), text.opacity(0.08))
                .into_any_element()
        });
        div()
            .px(px(SIDE - 8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows)
            .into_any_element()
    }

    /// Pega en la app donde estaba el usuario (el overlay no tiene el foco).
    fn paste_text_from_peek(&mut self, body: String, cx: &mut Context<Self>) {
        let clipboard = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
        let text = crate::snippets::expand(&body, &clipboard);
        let target = crate::paste::foreground_target();
        self.close_tool_peek(Instant::now(), cx);
        let item = ClipboardItem::new_string(text);
        cx.spawn(async move |_, cx| crate::paste_into(target, item, cx).await).detach();
    }

    // --- Flip ---------------------------------------------------------------

    fn pages_body(&self, width: f32, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let pages = self.tool_peek.data.pages.clone();
        // Angosto: una página por fila, la miniatura a la izquierda.
        let (w, h) = if narrow { PAGE_THUMB } else { page_size(width) };
        let (text, muted, faint) = (c.text, c.muted, c.faint);
        let cells = pages.into_iter().take(MAX_PAGES).enumerate().map(|(i, page)| {
            let summary = if page.summary.is_empty() {
                if !page.ink.is_empty() {
                    "Dibujo".to_string()
                } else if page.shapes.is_empty() {
                    "Vacía".to_string()
                } else {
                    "Sin texto".to_string()
                }
            } else {
                page.summary.clone()
            };
            div()
                .id(("peek-page", i))
                .w(px(w))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.))
                .when(narrow, |el| el.w_full().flex_row().items_center().gap(px(10.)).h(px(PAGE_ROW_H - 8.0)))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        crate::flip_board::OPEN_PAGE.store(i, std::sync::atomic::Ordering::Relaxed);
                        pill.open_from_peek(crate::flip::TOOL, window, cx);
                    }),
                )
                .fx(("peek-page-fx", i), move |el, hv| {
                    // Una hoja en miniatura con sus bloques donde están. Al
                    // abrir cae a su sitio y sus trazos se dibujan solos.
                    let e = m.enter(i);
                    let drawn = ease_smooth_out(segment(m.t, 0.18 + i as f32 * 0.08, 0.9));
                    let el = el.relative().top(px(-8.0 * (1.0 - e))).opacity(e);
                    let shapes = page.shapes.iter().map(|&(x, y, bw, bh, kind)| {
                        let fill: Hsla = match kind {
                            1 => text.opacity(0.28),
                            2 => Hsla::from(rgb(0x6fa3e0)).opacity(0.7),
                            _ => text.opacity(0.5),
                        };
                        div()
                            .absolute()
                            .left(px(x * w))
                            .top(px(y * h))
                            .w(px((bw * w).max(3.0)))
                            .h(px((bh * h).max(2.0)))
                            .rounded(px(1.5))
                            .bg(fill)
                    });
                    let ink: Vec<_> = page
                        .ink
                        .iter()
                        .map(|(points, color)| {
                            let n = ((points.len() as f32 * drawn).ceil() as usize).min(points.len());
                            (points[..n].to_vec(), *color)
                        })
                        .filter(|(points, _)| points.len() >= 2)
                        .collect();
                    let strokes = gpui::canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            for (points, (r, g, b, a)) in &ink {
                                let screen: Vec<gpui::Point<gpui::Pixels>> = points
                                    .iter()
                                    .map(|&(x, y)| {
                                        gpui::point(
                                            bounds.origin.x + bounds.size.width * x,
                                            bounds.origin.y + bounds.size.height * y,
                                        )
                                    })
                                    .collect();
                                let options = lyon::tessellation::StrokeOptions::default()
                                    .with_line_width(1.3)
                                    .with_line_cap(lyon::tessellation::LineCap::Round)
                                    .with_line_join(lyon::tessellation::LineJoin::Round);
                                let mut builder = gpui::PathBuilder::stroke(px(1.3))
                                    .with_style(gpui::PathStyle::Stroke(options));
                                builder.add_polygon(&screen, false);
                                if let Ok(path) = builder.build() {
                                    let color = gpui::Rgba {
                                        r: *r as f32 / 255.0,
                                        g: *g as f32 / 255.0,
                                        b: *b as f32 / 255.0,
                                        a: *a as f32 / 255.0,
                                    };
                                    window.paint_path(path, color);
                                }
                            }
                        },
                    )
                    .absolute()
                    .size_full();
                    el.child(
                        div()
                            .w(px(w))
                            .h(px(h))
                            .flex_none()
                            .relative()
                            .rounded(px(7.))
                            .overflow_hidden()
                            .bg(hv.mix(text.opacity(0.07), text.opacity(0.11)))
                            .border_1()
                            .border_color(text.opacity(0.10 + 0.3 * hv.t))
                            .children(shapes)
                            .child(strokes)
                            .child(
                                div()
                                    .absolute()
                                    .top(px(4.))
                                    .left(px(6.))
                                    .text_size(px(9.5))
                                    .text_color(muted)
                                    .child(format!("{}", i + 1)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .when(narrow, |el| el.flex_1().min_w_0())
                            .flex()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(if narrow { 12. } else { 10.5 }))
                                    .text_color(hv.mix(faint, text))
                                    .child(summary),
                            ),
                    )
                })
                .into_any_element()
        });
        div()
            .px(px(SIDE))
            .flex()
            .gap(px(PAGE_GAP))
            .when(narrow, |el| el.flex_col().gap(px(8.)))
            .children(cells)
            .into_any_element()
    }

    // --- Sistema ------------------------------------------------------------

    fn system_body(&self, c: &Colors, m: Motion, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let snap = self.system.read(cx).snapshot().clone();
        let procs = snap.procs.clone();
        let ram_total = procs.as_ref().map(|p| p.ram_total).unwrap_or(0);
        let gb = |b: u64| b as f64 / 1024.0 / 1024.0 / 1024.0;
        // Cuentan desde cero al abrir y entre lecturas se deslizan.
        let count = m.count();
        let cpu = self.tool_peek.cpu.value(now) * 100.0 * count;
        let ram = self.tool_peek.ram.value(now) * count;
        let ram_used = (ram_total as f64 * ram as f64) as u64;
        let warn: Hsla = rgb(0xd4a84b).into();
        let meter = |i: usize, label: &'static str, value: f32, shown: String, alert: f32| {
            let color = if value >= alert { warn } else { c.ok };
            let e = m.enter(i);
            div()
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(METER_H))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.))
                .child(div().w(px(66.)).text_color(c.muted).child(label))
                .child(
                    div()
                        .flex_1()
                        .h(px(5.))
                        .rounded(px(3.))
                        .bg(c.text.opacity(0.12))
                        .child(
                            div()
                                .h_full()
                                .rounded(px(3.))
                                .bg(color)
                                .w(gpui::relative(value.clamp(0.02, 1.0))),
                        ),
                )
                .child(div().w(px(70.)).flex().justify_end().child(shown))
        };
        let measured = procs.is_some();
        let apps: Vec<_> = procs.map(|p| p.apps).unwrap_or_default();
        let mut apps = apps;
        apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        let rows = apps.into_iter().take(3).enumerate().map(|(i, app)| {
            let e = m.enter(i + 3);
            let icon = self.system.update(cx, |panel, cx| panel.app_image(app.path.as_ref(), cx));
            let ram = if app.ram >= 1 << 30 {
                format!("{:.1} GB", gb(app.ram))
            } else {
                format!("{} MB", app.ram >> 20)
            };
            div()
                .id(("peek-app", i))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(APP_ROW_H))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.))
                .child(match icon {
                    Some(icon) => img(icon).size(px(16.)).flex_none().into_any_element(),
                    None => div().size(px(16.)).flex_none().rounded(px(4.)).bg(c.text.opacity(0.10)).into_any_element(),
                })
                .child(div().flex_1().min_w_0().truncate().text_color(c.muted).child(app.name.clone()))
                .child(div().w(px(56.)).flex().justify_end().text_color(c.faint).child(ram))
                .child(div().w(px(44.)).flex().justify_end().child(format!("{:.0} %", app.cpu * count)))
                .into_any_element()
        });
        let battery = snap.battery.map(|(p, plugged)| {
            if plugged {
                format!("Batería {p} % · enchufado")
            } else {
                format!("Batería {p} %")
            }
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .child(meter(
                0,
                "CPU",
                cpu / 100.0,
                if measured { format!("{cpu:.0} %") } else { "—".into() },
                0.85,
            ))
            .child(meter(
                1,
                "Memoria",
                ram,
                if measured { format!("{:.1} de {:.0} GB", gb(ram_used), gb(ram_total)) } else { "—".into() },
                0.90,
            ))
            .child(div().h(px(6.)))
            .child(
                div()
                    .h(px(SECTION_H))
                    .flex()
                    .items_center()
                    .text_size(px(10.5))
                    .text_color(c.faint)
                    .opacity(m.enter(2))
                    .child("Lo que más consume")
                    .child(div().flex_1())
                    .children(battery),
            )
            .children(rows)
            .into_any_element()
    }
}

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    ok: Hsla,
}

fn empty(text: &'static str, c: &Colors) -> AnyElement {
    div()
        .h(px(EMPTY_H))
        .px(px(SIDE))
        .flex()
        .items_center()
        .text_size(px(12.))
        .text_color(c.muted)
        .child(text)
        .into_any_element()
}

fn rgb_text(color: u32) -> String {
    format!("rgb({}, {}, {})", (color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_las_herramientas_con_vistazo() {
        assert!(has_peek(crate::color::TOOL));
        assert!(has_peek(crate::capture::TOOL));
        assert!(!has_peek(crate::CLIPBOARD_TOOL));
        assert!(!has_peek(crate::AGENTES_TOOL));
        assert!(!has_peek(crate::board::TOOL));
    }

    #[test]
    fn alto_segun_lo_que_hay() {
        let mut peek = ToolPeek::new();
        let empty = peek.content_height(crate::color::TOOL, 400.0, false);
        peek.data.colors = vec![0xff0000, 0x00ff00];
        assert!(peek.content_height(crate::color::TOOL, 400.0, false) > empty);
        peek.data.texts = vec![("a".into(), "b".into()); 3];
        assert_eq!(
            peek.content_height(crate::TEXTOS_TOOL, 400.0, false),
            TITLE_H + 3.0 * TEXT_ROW_H + FOOTER_H + BOTTOM
        );
    }

    #[test]
    fn en_un_costado_las_filas_van_en_columna() {
        let mut peek = ToolPeek::new();
        // Diez colores: arriba en una fila; al costado, seis filas.
        peek.data.colors = (0..10).collect();
        assert_eq!(
            peek.content_height(crate::color::TOOL, 300.0, true),
            TITLE_H + SIDE_COLORS as f32 * COLOR_ROW_H + FOOTER_H + BOTTOM
        );
        // Tres capturas: una por fila.
        peek.data.shots = (0..3)
            .map(|i| Shot { path: PathBuf::from(format!("{i}.png")), label: String::new() })
            .collect();
        assert_eq!(
            peek.content_height(crate::capture::TOOL, 300.0, true),
            TITLE_H + 3.0 * SHOT_ROW_H + FOOTER_H + BOTTOM
        );
    }

    #[test]
    fn codigos_de_color() {
        assert_eq!(rgb_text(0x1a2b3c), "rgb(26, 43, 60)");
    }
}
