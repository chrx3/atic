//! Prototipo de la pill de Atic en GPUI: tab acoplado a cualquier borde o gota
//! flotante, tira de herramientas al pasar el cursor, rueda con gotas
//! líquidas y el panel del Clipboard.

mod anim;
mod clipboard;
mod drag;
mod geometry;
mod liquid;
mod paste;
mod text_input;
mod win;

use std::borrow::Cow;
use std::f32::consts::{PI, TAU};
use std::time::{Duration, Instant};

use anyhow::Result;
use gpui::{
    canvas, div, point, prelude::*, px, rgb, size, App, Application, AssetSource, Bounds,
    BoxShadow, ClipboardItem, Context, Corners, Entity, Focusable, Hsla, MouseButton,
    MouseDownEvent, MouseUpEvent, SharedString, Subscription, TransformationMatrix, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};

use anim::{ease_back_out, ease_island, ease_smooth_out, lerp, segment, Tween};
use clipboard::{ClipboardPanel, Content, PanelEvent, PANEL_H, PANEL_W};
use geometry::{Edge, Rect};

// Medidas de `pillStage.ts`, `pillPlan.ts` y `wheelGeometry.ts`.
const TAB_LENGTH: f32 = 124.0;
const TAB_THICK: f32 = 40.0;
const TAB_RADIUS: f32 = 22.0;
const DISC_R: f32 = 26.0;
const MARK_SIZE: f32 = 32.0;
const TOOL_W: f32 = 44.0;
const TOOL_GAP: f32 = 2.0;
const TOOL_ICON: f32 = 16.0;
/// Desde el borde hasta el centro de la rueda cuando gotea del tab.
const WHEEL_DEPTH: f32 = TAB_THICK + 20.0 + 126.0;
const WHEEL_BOX: f32 = 252.0;
const WHEEL_RING: f32 = 0.28 * 232.0;
const WHEEL_BLOB_R: f32 = 28.0;
const WHEEL_CORE_R: f32 = 29.0;
const WHEEL_ICON: f32 = 21.0;
const WHEEL_HIT_R: f32 = 126.0;

// Línea de tiempo de la rueda, en ms. Más lenta que la real (110 ms + 16 ms
// por gota) para que el efecto líquido se alcance a ver.
const CORE_DROP_MS: f32 = 300.0;
const BLOB_START_MS: f32 = 150.0;
const BLOB_STAGGER_MS: f32 = 24.0;
const BLOB_MS: f32 = 300.0;
const ICON_DELAY_MS: f32 = 180.0;
const ICON_MS: f32 = 140.0;
const CLOSE_SPEED: f32 = 2.5;

// Apertura del panel (`ClipboardFloat.svelte`): una semilla de 40 px pegada a
// la pill, crece con la cara que mira a la pill fija y después se separa
// hasta 16 px, donde el cuello se corta. Algo más lenta que la real.
const PANEL_SEED_MS: f32 = 120.0;
const PANEL_GROW_MS: f32 = 160.0;
const PANEL_SEPARATE_MS: f32 = 110.0;
const PANEL_CONTENT_MS: f32 = 120.0;
const PANEL_CLOSE_SPEED: f32 = 1.6;
const PANEL_SEED: f32 = 40.0;
const PANEL_CORNER: f32 = 20.0;
const PANEL_FUSED_GAP: f32 = 2.0;
const PANEL_NECK_REACH: f32 = 12.0;
// Atic espera a que la ventana destino tenga el foco antes de escribir el
// portapapeles y pegar.
const PASTE_FOCUS_DELAY: Duration = Duration::from_millis(220);
const PASTE_KEY_DELAY: Duration = Duration::from_millis(80);
const CLIPBOARD_TOOL: usize = 1;

/// Movimiento con el botón apretado a partir del cual es arrastre y no clic.
const DRAG_THRESHOLD: f32 = 4.0;
/// "Aplastón" al acoplarse: el grosor baja a 86 % al 38 % del tramo.
const SEAT_MS: f32 = 125.0;
const SEAT_DEPTH: f32 = 0.14;
/// Muro líquido que se forma al acercar la gota a un borde (`INFLUENCE`).
const WALL_REACH: f32 = 24.0;
const WALL_R: f32 = 60.0;

const STRIP_LEAVE_GRACE: Duration = Duration::from_millis(150);
const WHEEL_HOVER_DELAY: Duration = Duration::from_millis(180);
const WHEEL_HOVER_GRACE: Duration = Duration::from_millis(150);
const WHEEL_CLICK_GRACE: Duration = Duration::from_millis(400);
const BLINK_MS: f32 = 140.0;

struct Tool {
    name: &'static str,
    icon: &'static str,
}

const TOOLS: [Tool; 9] = [
    Tool {
        name: "Reuniones",
        icon: "icons/circle-dot.svg",
    },
    Tool {
        name: "Clipboard",
        icon: "icons/clipboard.svg",
    },
    Tool {
        name: "Textos",
        icon: "icons/text-align-start.svg",
    },
    Tool {
        name: "Agentes",
        icon: "icons/square-terminal.svg",
    },
    Tool {
        name: "Sistema",
        icon: "icons/cpu.svg",
    },
    Tool {
        name: "Capturas",
        icon: "icons/crop.svg",
    },
    Tool {
        name: "Pizarra",
        icon: "icons/pencil.svg",
    },
    Tool {
        name: "Color",
        icon: "icons/pipette.svg",
    },
    Tool {
        name: "Más",
        icon: "icons/ellipsis.svg",
    },
];

fn wheel_open_ms() -> f32 {
    BLOB_START_MS + BLOB_STAGGER_MS * (TOOLS.len() - 1) as f32 + ICON_DELAY_MS + ICON_MS
}

fn panel_open_ms() -> f32 {
    PANEL_SEED_MS + PANEL_GROW_MS + PANEL_SEPARATE_MS
}

fn strip_open_length() -> f32 {
    TAB_LENGTH.max(MARK_SIZE + 12.0 + TOOLS.len() as f32 * (TOOL_W + TOOL_GAP))
}

fn wheel_angle(index: usize) -> f32 {
    index as f32 / TOOLS.len() as f32 * TAU - PI / 2.0
}

/// Centro y radio.
type Circle = ((f32, f32), f32);

/// Traza de eventos con `PILL_DEBUG=1`.
fn debug(message: impl FnOnce() -> String) {
    if std::env::var_os("PILL_DEBUG").is_some() {
        eprintln!("[pill] {}", message());
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn offset(p: (f32, f32), (dx, dy): (f32, f32), by: f32) -> (f32, f32) {
    (p.0 + dx * by, p.1 + dy * by)
}

struct Palette {
    skin: Hsla,
    text: Hsla,
    muted: Hsla,
}

impl Palette {
    /// Tema "atic" oscuro.
    fn dark() -> Self {
        Self {
            skin: rgb(0x1a1a18).into(),
            text: rgb(0xf0f0ea).into(),
            muted: rgb(0xa8a89e).into(),
        }
    }
}

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/circle-dot.svg" => include_bytes!("../assets/icons/circle-dot.svg"),
            "icons/clipboard.svg" => include_bytes!("../assets/icons/clipboard.svg"),
            "icons/text-align-start.svg" => include_bytes!("../assets/icons/text-align-start.svg"),
            "icons/square-terminal.svg" => include_bytes!("../assets/icons/square-terminal.svg"),
            "icons/cpu.svg" => include_bytes!("../assets/icons/cpu.svg"),
            "icons/crop.svg" => include_bytes!("../assets/icons/crop.svg"),
            "icons/pencil.svg" => include_bytes!("../assets/icons/pencil.svg"),
            "icons/pipette.svg" => include_bytes!("../assets/icons/pipette.svg"),
            "icons/ellipsis.svg" => include_bytes!("../assets/icons/ellipsis.svg"),
            "icons/x.svg" => include_bytes!("../assets/icons/x.svg"),
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/layers.svg" => include_bytes!("../assets/icons/layers.svg"),
            "icons/type.svg" => include_bytes!("../assets/icons/type.svg"),
            "icons/image.svg" => include_bytes!("../assets/icons/image.svg"),
            "icons/star.svg" => include_bytes!("../assets/icons/star.svg"),
            "icons/pin.svg" => include_bytes!("../assets/icons/pin.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// Generador pseudoaleatorio mínimo para los parpadeos; no amerita una crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Dónde vive la pill cuando nadie la arrastra.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Home {
    /// Tab pegado a un borde, centrado en `along`.
    Docked { edge: Edge, along: f32 },
    /// Gota de 52 px centrada en `center`.
    Floating { center: (f32, f32) },
}

/// La forma de la pill en este cuadro.
#[derive(Clone, Copy)]
enum PillShape {
    Tab {
        edge: Edge,
        rect: Rect,
        along: f32,
        thick: f32,
    },
    Disc {
        center: (f32, f32),
    },
}

#[derive(Clone, Copy, PartialEq)]
enum WheelOpener {
    Click,
    Hover,
    Shortcut,
}

#[derive(Clone, Copy, PartialEq)]
enum PressTarget {
    Mark,
    Tool(usize),
    Body,
}

/// Botón apretado sobre la pill: todavía puede ser clic o arrastre.
struct Press {
    cursor: (f32, f32),
    /// Posición de la pill al empezar (centro de la gota, o `along` del tab
    /// en `.0`).
    anchor: (f32, f32),
    target: PressTarget,
    dragging: bool,
}

struct Pill {
    overlay: Option<win::Overlay>,
    scale_factor: f32,
    cursor: Option<(f32, f32)>,
    born: Instant,
    last_frame: Instant,
    ticks: u64,
    fps: (Instant, u32),

    monitor: Rect,
    work: Rect,
    dockable: Vec<Edge>,
    home: Home,
    press: Option<Press>,
    seat_started: Option<Instant>,
    hover_since: Option<Instant>,

    strip: Tween,
    strip_leave_at: Option<Instant>,
    strip_hover: Vec<Tween>,
    strip_pulse: Option<(usize, Instant)>,

    wheel_target_open: bool,
    wheel_time: f32,
    wheel_opener: WheelOpener,
    wheel_leave_at: Option<Instant>,
    wheel_hover: Vec<Tween>,
    shortcut_was_down: bool,

    panel: Entity<ClipboardPanel>,
    panel_open: bool,
    panel_time: f32,
    paste_target: Option<paste::Target>,

    eyes: (f32, f32),
    next_blink: Instant,
    blink_started: Option<Instant>,
    double_blink: bool,
    rng: Rng,
    _subscriptions: Vec<Subscription>,
}

/// Dónde gotea la rueda: desde qué punto, unida a qué círculo de la pill y
/// con qué centro final.
struct WheelPlace {
    source: (f32, f32),
    source_r: f32,
    anchor: Circle,
    center: (f32, f32),
}

/// Geometría del panel en un instante de su línea de tiempo.
#[derive(Clone, Copy)]
struct PanelShape {
    rect: Rect,
    content: f32,
    /// Círculo de la pill y círculo del panel que une el cuello.
    anchor: Circle,
    near: Circle,
}

/// Todo lo que se dibuja en un cuadro, calculado antes de pintar.
struct Frame {
    palette: Palette,
    skin: Hsla,
    pill: PillShape,
    mark: (f32, f32),
    eyes: (f32, f32),
    blink: f32,
    strip_tools: Vec<IconDraw>,
    core: Option<(f32, f32, f32)>,
    core_anchor: Circle,
    blobs: Vec<(f32, f32, f32)>,
    wheel_icons: Vec<IconDraw>,
    wheel_shadow: Option<(f32, f32, f32, f32)>,
    panel: Option<PanelShape>,
    walls: Vec<Circle>,
}

struct IconDraw {
    path: &'static str,
    center: (f32, f32),
    size: f32,
    color: Hsla,
}

impl Pill {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        let seed = now.elapsed().as_nanos() as u64 ^ 0x9e37_79b9_7f4a_7c15;
        let panel = cx.new(ClipboardPanel::new);
        let subscriptions = vec![
            cx.subscribe(&panel, |pill, _, event: &PanelEvent, cx| match event {
                PanelEvent::Paste(entry) => pill.paste(entry.clone(), cx),
                PanelEvent::Drag(entry) => pill.start_drag(entry.clone(), cx),
                PanelEvent::Close => pill.close_panel(true, cx),
            }),
            // Clic fuera del panel: la ventana pierde el foco y el panel se cierra,
            // salvo que esté fijado.
            cx.observe_window_activation(window, |pill, window, cx| {
                if !window.is_window_active() && pill.panel_open && !pill.panel.read(cx).pinned {
                    pill.close_panel(false, cx);
                }
            }),
        ];

        let scale_factor = window.scale_factor();
        let viewport = window.viewport_size();
        let overlay = win::Overlay::attach(window);
        let monitor = overlay
            .as_ref()
            .and_then(|overlay| overlay.monitor_area(scale_factor))
            .unwrap_or_else(|| {
                Rect::new(
                    0.0,
                    0.0,
                    f32::from(viewport.width),
                    f32::from(viewport.height),
                )
            });
        let work = overlay
            .as_ref()
            .and_then(|overlay| overlay.work_area(scale_factor))
            .unwrap_or(monitor);
        let dockable = geometry::dockable_edges(&monitor, &work);
        let home = load_home(&work, &dockable).unwrap_or(Home::Docked {
            edge: Edge::Top,
            along: work.x + work.w / 2.0,
        });

        let mut pill = Self {
            overlay,
            scale_factor,
            cursor: None,
            born: now,
            last_frame: now,
            ticks: 0,
            fps: (now, 0),
            monitor,
            work,
            dockable,
            home,
            press: None,
            seat_started: None,
            hover_since: None,
            strip: Tween::new(0.0, Duration::from_millis(240), ease_island),
            strip_leave_at: None,
            strip_hover: (0..TOOLS.len())
                .map(|_| Tween::new(0.0, Duration::from_millis(120), ease_smooth_out))
                .collect(),
            strip_pulse: None,
            wheel_target_open: false,
            wheel_time: 0.0,
            wheel_opener: WheelOpener::Click,
            wheel_leave_at: None,
            wheel_hover: (0..TOOLS.len())
                .map(|_| Tween::new(0.0, Duration::from_millis(120), ease_smooth_out))
                .collect(),
            shortcut_was_down: false,
            panel,
            panel_open: false,
            panel_time: 0.0,
            paste_target: None,
            eyes: (0.0, 0.0),
            next_blink: now,
            blink_started: None,
            double_blink: false,
            rng: Rng(seed | 1),
            _subscriptions: subscriptions,
        };
        pill.schedule_blink(now);

        // Sondeo del cursor global: la ventana deja pasar los clics casi siempre,
        // así que no recibe eventos de mouse mientras el cursor está afuera.
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            if this
                .update_in(cx, |pill, window, cx| pill.tick(window, cx))
                .is_err()
            {
                break;
            }
        })
        .detach();

        pill
    }

    fn schedule_blink(&mut self, now: Instant) {
        let seconds = 2.6 + self.rng.next() * 6.4;
        self.next_blink = now + Duration::from_secs_f32(seconds);
    }

    // --- Forma de la pill -------------------------------------------------

    fn seat_scale(&self, now: Instant) -> f32 {
        let Some(started) = self.seat_started else {
            return 1.0;
        };
        let t = (now.duration_since(started).as_secs_f32() * 1000.0 / SEAT_MS).clamp(0.0, 1.0);
        let amount = if t < 0.38 {
            ease_smooth_out(t / 0.38)
        } else {
            1.0 - ease_smooth_out((t - 0.38) / 0.62)
        };
        1.0 - SEAT_DEPTH * amount
    }

    fn tab_length(&self, now: Instant) -> f32 {
        lerp(TAB_LENGTH, strip_open_length(), self.strip.value(now)).max(TAB_LENGTH * 0.96)
    }

    fn shape(&self, now: Instant) -> PillShape {
        match self.home {
            Home::Docked { edge, along } => {
                let length = self.tab_length(now);
                let thick = TAB_THICK * self.seat_scale(now);
                // Al abrirse la tira crece hacia los dos lados; si no cabe, se
                // corre para no salirse del borde.
                let along = geometry::clamp_along(edge, &self.work, along, length);
                PillShape::Tab {
                    edge,
                    rect: edge.tab_rect(&self.work, along, length, thick),
                    along,
                    thick,
                }
            }
            Home::Floating { center } => PillShape::Disc { center },
        }
    }

    /// Posición de la marca a lo largo del tab (o el centro de la gota).
    fn mark_along(&self, now: Instant) -> Option<f32> {
        let PillShape::Tab {
            rect, edge, along, ..
        } = self.shape(now)
        else {
            return None;
        };
        let open = self.strip.value(now).clamp(0.0, 1.0);
        let start = if edge.is_vertical() { rect.y } else { rect.x };
        Some(lerp(along, start + 6.0 + MARK_SIZE / 2.0, open))
    }

    fn mark_center(&self, now: Instant) -> (f32, f32) {
        match self.shape(now) {
            PillShape::Tab { edge, thick, .. } => {
                let along = self.mark_along(now).unwrap_or_default();
                edge.point(&self.work, along, thick / 2.0)
            }
            PillShape::Disc { center } => center,
        }
    }

    fn strip_tool_along(&self, index: usize, now: Instant) -> f32 {
        let open = self.strip.value(now).max(0.0);
        let first = self.mark_along(now).unwrap_or_default() + MARK_SIZE / 2.0 + 6.0;
        first + index as f32 * (TOOL_W + TOOL_GAP) * open + TOOL_W * open / 2.0
    }

    fn over_pill(&self, p: (f32, f32), now: Instant, margin: f32) -> bool {
        match self.shape(now) {
            PillShape::Tab { rect, .. } => rect.contains(p, margin),
            PillShape::Disc { center } => distance(p, center) <= DISC_R + margin,
        }
    }

    fn over_mark(&self, p: (f32, f32), now: Instant) -> bool {
        match self.shape(now) {
            PillShape::Tab { edge, rect, .. } => {
                rect.contains(p, 0.0)
                    && (edge.along_of(p) - self.mark_along(now).unwrap_or_default()).abs()
                        <= MARK_SIZE / 2.0 + 2.0
            }
            PillShape::Disc { center } => distance(p, center) <= DISC_R,
        }
    }

    fn strip_tool_at(&self, p: (f32, f32), now: Instant) -> Option<usize> {
        let PillShape::Tab { edge, rect, .. } = self.shape(now) else {
            return None;
        };
        if self.strip.value(now) < 0.85 || !rect.contains(p, 0.0) {
            return None;
        }
        let along = edge.along_of(p);
        (0..TOOLS.len()).find(|&index| {
            (along - self.strip_tool_along(index, now)).abs() <= (TOOL_W + TOOL_GAP) / 2.0
        })
    }

    // --- Rueda --------------------------------------------------------------

    fn wheel_place(&self, now: Instant) -> WheelPlace {
        let keep_inside = |center: (f32, f32)| {
            Rect::centered(center, WHEEL_BOX, WHEEL_BOX)
                .clamped_into(&self.work)
                .center()
        };
        match self.shape(now) {
            PillShape::Tab {
                edge, along, thick, ..
            } => WheelPlace {
                source: edge.point(&self.work, along, thick - 14.0),
                source_r: 10.0,
                anchor: (edge.point(&self.work, along, thick - 20.0), 20.0),
                center: keep_inside(edge.point(&self.work, along, WHEEL_DEPTH)),
            },
            PillShape::Disc { center } => WheelPlace {
                source: center,
                source_r: DISC_R,
                anchor: (center, DISC_R),
                center: keep_inside(center),
            },
        }
    }

    fn wheel_visible(&self) -> bool {
        self.wheel_target_open || self.wheel_time > 0.0
    }

    fn wheel_fully_open(&self) -> bool {
        self.wheel_target_open && self.wheel_time >= BLOB_START_MS + BLOB_MS
    }

    /// `None` fuera de la rueda, `Some(None)` en el centro, `Some(Some(i))` en
    /// el gajo de la herramienta `i`.
    fn wheel_target_at(&self, p: (f32, f32), now: Instant) -> Option<Option<usize>> {
        let center = self.wheel_place(now).center;
        let (dx, dy) = (p.0 - center.0, p.1 - center.1);
        let distance = dx.hypot(dy);
        if distance > WHEEL_HIT_R {
            return None;
        }
        if distance <= WHEEL_CORE_R {
            return Some(None);
        }
        let slice = TAU / TOOLS.len() as f32;
        let angle = (dy.atan2(dx) + PI / 2.0 + slice / 2.0).rem_euclid(TAU);
        Some(Some((angle / slice) as usize % TOOLS.len()))
    }

    fn open_wheel(&mut self, opener: WheelOpener) {
        self.wheel_target_open = true;
        self.wheel_opener = opener;
        self.wheel_leave_at = None;
    }

    fn toggle_wheel(&mut self, opener: WheelOpener) {
        if self.wheel_target_open {
            self.close_wheel();
        } else {
            self.open_wheel(opener);
        }
    }

    fn close_wheel(&mut self) {
        self.wheel_target_open = false;
        self.wheel_leave_at = None;
    }

    // --- Panel --------------------------------------------------------------

    fn panel_visible(&self) -> bool {
        self.panel_open || self.panel_time > 0.0
    }

    /// Cara del panel que mira a la pill, punto de la pill del que cuelga y
    /// círculo de la pill para el cuello.
    fn panel_attach(&self, now: Instant) -> (Edge, (f32, f32), Circle) {
        match self.shape(now) {
            PillShape::Tab {
                edge, along, thick, ..
            } => (
                edge,
                edge.point(&self.work, along, thick),
                (edge.point(&self.work, along, thick - 20.0), 20.0),
            ),
            PillShape::Disc { center } => {
                let disc = Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0);
                let face = geometry::panel_side(&self.work, &disc, PANEL_W, PANEL_H);
                (
                    face,
                    offset(center, face.inward(), DISC_R),
                    (center, DISC_R),
                )
            }
        }
    }

    fn panel_shape(&self, now: Instant) -> Option<PanelShape> {
        if !self.panel_visible() {
            return None;
        }
        let time = self.panel_time;
        let seed = ease_smooth_out(segment(time, 0.0, PANEL_SEED_MS));
        let grow = ease_smooth_out(segment(time, PANEL_SEED_MS, PANEL_GROW_MS));
        let separate = ease_smooth_out(segment(
            time,
            PANEL_SEED_MS + PANEL_GROW_MS,
            PANEL_SEPARATE_MS,
        ));
        let content = segment(time, PANEL_SEED_MS + PANEL_GROW_MS - 20.0, PANEL_CONTENT_MS);

        let (face, attach, anchor) = self.panel_attach(now);
        let normal = face.inward();
        let final_rect = geometry::panel_rect(
            &self.work,
            face,
            attach,
            geometry::PANEL_GAP,
            PANEL_W,
            PANEL_H,
        );
        let final_mid = geometry::face_midpoint(&final_rect, face);

        // Punto medio de la cara que mira a la pill: la semilla nace metida a
        // medias en la pill, baja hasta quedar pegada, se ensancha (y se corre
        // si el panel quedó desplazado para caber) y al final se separa.
        let fused = offset(attach, normal, PANEL_FUSED_GAP);
        let seed_mid = offset(
            attach,
            normal,
            lerp(-PANEL_SEED / 2.0, PANEL_FUSED_GAP, seed),
        );
        let grown_mid = (
            lerp(fused.0, final_mid.0, grow),
            lerp(fused.1, final_mid.1, grow),
        );
        let grown_mid = if face.is_vertical() {
            (fused.0, grown_mid.1)
        } else {
            (grown_mid.0, fused.1)
        };
        let mid = if grow <= 0.0 {
            seed_mid
        } else {
            (
                lerp(grown_mid.0, final_mid.0, separate),
                lerp(grown_mid.1, final_mid.1, separate),
            )
        };

        let width = lerp(PANEL_SEED, PANEL_W, grow);
        let height = lerp(PANEL_SEED, PANEL_H, grow);
        let rect = match face {
            Edge::Top => Rect::new(mid.0 - width / 2.0, mid.1, width, height),
            Edge::Bottom => Rect::new(mid.0 - width / 2.0, mid.1 - height, width, height),
            Edge::Left => Rect::new(mid.0, mid.1 - height / 2.0, width, height),
            Edge::Right => Rect::new(mid.0 - width, mid.1 - height / 2.0, width, height),
        };
        let near_r = PANEL_CORNER.min(width / 2.0).min(height / 2.0);
        let near_mid = (
            if face.is_vertical() { mid.0 } else { attach.0 },
            if face.is_vertical() { attach.1 } else { mid.1 },
        );
        Some(PanelShape {
            rect,
            content,
            anchor,
            near: (offset(near_mid, normal, near_r), near_r),
        })
    }

    fn over_panel(&self, p: (f32, f32), now: Instant, margin: f32) -> bool {
        self.panel_shape(now)
            .is_some_and(|shape| shape.rect.contains(p, margin))
    }

    fn open_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_wheel();
        if self.panel_open {
            return;
        }
        self.paste_target = paste::foreground_target();
        self.panel.update(cx, |panel, cx| panel.reset(cx));
        self.panel_open = true;
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(true);
        }
        let focus = self.panel.focus_handle(cx);
        window.focus(&focus);
        cx.notify();
    }

    fn close_panel(&mut self, restore_focus: bool, cx: &mut Context<Self>) {
        if !self.panel_open {
            return;
        }
        self.panel_open = false;
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(false);
        }
        let target = self.paste_target.take();
        if restore_focus {
            if let Some(target) = target {
                paste::force_foreground(target);
            }
        }
        cx.notify();
    }

    fn paste(&mut self, entry: clipboard::Entry, cx: &mut Context<Self>) {
        let target = self.paste_target;
        self.close_panel(false, cx);
        let item = match &entry.content {
            Content::Text(text) => ClipboardItem::new_string(text.to_string()),
            Content::Color(label, _) => ClipboardItem::new_string(label.to_string()),
            Content::Image(image) => ClipboardItem::new_image(image),
        };
        println!("pegar → entrada {}", entry.id);
        cx.spawn(async move |_, cx| paste_into(target, item, cx).await)
            .detach();
    }

    /// Texto como texto; imágenes como archivo PNG, igual que Atic.
    fn start_drag(&mut self, entry: clipboard::Entry, cx: &mut Context<Self>) {
        enum Payload {
            Text(String),
            Files(Vec<String>),
        }
        let payload = match &entry.content {
            Content::Text(text) => Payload::Text(text.to_string()),
            Content::Color(label, _) => Payload::Text(label.to_string()),
            Content::Image(image) => match write_drag_image(entry.id, image.bytes()) {
                Ok(path) => Payload::Files(vec![path]),
                Err(error) => {
                    eprintln!("no se pudo preparar la imagen para arrastrar: {error}");
                    return;
                }
            },
        };
        // `DoDragDrop` corre un loop modal que sigue despachando mensajes a
        // GPUI. Desde el manejador del evento reentraría en la app mientras
        // está prestada; en una tarea aparte corre cuando ese préstamo terminó.
        cx.spawn(async move |_, cx| {
            let (outcome, text) = match payload {
                Payload::Text(text) => (drag::drag_text(&text), Some(text)),
                Payload::Files(paths) => (drag::drag_files(&paths), None),
            };
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    eprintln!("arrastre → entrada {}: {error}", entry.id);
                    return;
                }
            };
            // Se mira apenas se suelta, con el cursor todavía sobre el destino.
            let under_cursor = paste::target_under_cursor();
            let web_terminal = under_cursor.is_some_and(paste::needs_ctrl_shift_v);
            let fallback =
                paste::drop_needs_paste_fallback(outcome.dropped, outcome.effect, web_terminal);
            println!(
                "arrastre → entrada {}: soltado={} efecto={} respaldo={}",
                entry.id,
                outcome.dropped,
                outcome.effect,
                fallback && text.is_some()
            );
            if let (true, Some(text), Some(target)) = (fallback, text, under_cursor) {
                paste_into(Some(target), ClipboardItem::new_string(text), cx).await;
            }
        })
        .detach();
    }

    // --- Arrastrar la pill --------------------------------------------------

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        // GPUI arma la posición con el último movimiento que vio, y mientras la
        // ventana dejaba pasar los clics no vio ninguno; el cursor global sí
        // está al día.
        let polled = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(self.scale_factor));
        let position = polled.unwrap_or((f32::from(event.position.x), f32::from(event.position.y)));

        debug(|| {
            format!(
                "down en {position:?} (evento {:?}) home={:?} sobre_pill={}",
                event.position,
                self.home,
                self.over_pill(position, now, 0.0)
            )
        });
        if self.panel_open && self.over_panel(position, now, 0.0) {
            // Los elementos del panel atienden sus propios clics.
            return;
        }
        if self.over_pill(position, now, 0.0) {
            let target = if self.over_mark(position, now) {
                PressTarget::Mark
            } else if let Some(index) = self.strip_tool_at(position, now) {
                PressTarget::Tool(index)
            } else {
                PressTarget::Body
            };
            let anchor = match self.home {
                Home::Docked { along, .. } => (along, 0.0),
                Home::Floating { center } => center,
            };
            self.press = Some(Press {
                cursor: position,
                anchor,
                target,
                dragging: false,
            });
        } else if self.wheel_fully_open() {
            match self.wheel_target_at(position, now) {
                Some(Some(index)) => {
                    println!("rueda → {}", TOOLS[index].name);
                    self.close_wheel();
                    if index == CLIPBOARD_TOOL {
                        self.open_panel(window, cx);
                    }
                }
                Some(None) => self.close_wheel(),
                None => {}
            }
        }
        cx.notify();
    }

    /// El botón se soltó: si no hubo arrastre fue un clic; si lo hubo, la pill
    /// se acopla o queda flotando.
    fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(press) = self.press.take() else {
            return;
        };
        let now = Instant::now();
        debug(|| {
            format!(
                "release arrastrando={} home={:?}",
                press.dragging, self.home
            )
        });
        if press.dragging {
            self.settle(now);
        } else {
            match press.target {
                // En Atic el clic en la gota no hace nada; la rueda se abre al
                // pasar el cursor. En el tab el clic en la marca la abre.
                PressTarget::Mark if matches!(self.home, Home::Docked { .. }) => {
                    self.toggle_wheel(WheelOpener::Click)
                }
                PressTarget::Tool(index) => {
                    println!("tira → {}", TOOLS[index].name);
                    self.strip_pulse = Some((index, now));
                    if index == CLIPBOARD_TOOL {
                        self.open_panel(window, cx);
                    }
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// Mueve la pill con el cursor. Devuelve si hay arrastre en curso.
    fn follow_drag(&mut self, now: Instant) -> bool {
        let Some(cursor) = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(self.scale_factor))
        else {
            return false;
        };
        let Some(press) = self.press.as_mut() else {
            return false;
        };
        if !press.dragging {
            if distance(cursor, press.cursor) < DRAG_THRESHOLD {
                return false;
            }
            press.dragging = true;
            self.wheel_target_open = false;
            self.strip.set(0.0, now);
        }

        match self.home {
            Home::Docked { edge, .. } => {
                if edge.depth_of(&self.work, cursor) > geometry::UNDOCK_DISTANCE {
                    // Se suelta del borde: pasa a ser gota bajo el cursor.
                    self.home = Home::Floating { center: cursor };
                    press.cursor = cursor;
                    press.anchor = cursor;
                } else {
                    let along =
                        press.anchor.0 + edge.along_of(cursor) - edge.along_of(press.cursor);
                    self.home = Home::Docked {
                        edge,
                        along: geometry::clamp_along(edge, &self.work, along, TAB_LENGTH),
                    };
                }
            }
            Home::Floating { .. } => {
                let center = (
                    press.anchor.0 + cursor.0 - press.cursor.0,
                    press.anchor.1 + cursor.1 - press.cursor.1,
                );
                let disc =
                    Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0).clamped_into(&self.monitor);
                self.home = Home::Floating {
                    center: disc.center(),
                };
            }
        }
        true
    }

    /// Al soltar: cerca de un borde acoplable se pega con el aplastón; si no,
    /// queda flotando donde cayó.
    fn settle(&mut self, now: Instant) {
        if let Home::Floating { center } = self.home {
            let disc = Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0);
            debug(|| {
                format!(
                    "settle disco={disc:?} trabajo={:?} acoplables={:?}",
                    self.work, self.dockable
                )
            });
            if let Some(edge) = geometry::dock_candidate(&self.work, &disc, &self.dockable) {
                self.home = Home::Docked {
                    edge,
                    along: geometry::dock_along(edge, &self.work, center, TAB_LENGTH),
                };
                self.seat_started = Some(now);
            }
        }
        save_home(&self.home, &self.work);
    }

    fn dragging(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.dragging)
    }

    // --- Sondeo -------------------------------------------------------------

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.ticks += 1;
        let cursor = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(self.scale_factor));
        let moved = cursor != self.cursor;
        self.cursor = cursor;

        // Respaldo por si GPUI no ve el `mouse up` (llegó a otra ventana).
        if self.press.is_some() && !win::left_button_down() {
            self.release(window, cx);
        }

        let shortcut_down = win::wheel_shortcut_down();
        if shortcut_down && !self.shortcut_was_down {
            self.toggle_wheel(WheelOpener::Shortcut);
        }
        self.shortcut_was_down = shortcut_down;

        let dragging = self.dragging();
        let docked = matches!(self.home, Home::Docked { .. });
        let over_pill = cursor.is_some_and(|c| self.over_pill(c, now, 0.0));

        // Tab acoplado: la tira se abre al pasar el cursor.
        if docked && over_pill && !dragging {
            self.strip.set(1.0, now);
            self.strip_leave_at = None;
        } else if self.strip.target() == 1.0 {
            let since = *self.strip_leave_at.get_or_insert(now);
            if !docked || dragging || now.duration_since(since) >= STRIP_LEAVE_GRACE {
                self.strip.set(0.0, now);
                self.strip_leave_at = None;
            }
        }

        // Gota flotante: la rueda se abre tras 180 ms con el cursor encima.
        if !docked && over_pill && !dragging && self.press.is_none() {
            let since = *self.hover_since.get_or_insert(now);
            if !self.wheel_target_open && now.duration_since(since) >= WHEEL_HOVER_DELAY {
                self.open_wheel(WheelOpener::Hover);
            }
        } else {
            self.hover_since = None;
        }

        let strip_hovered = cursor.and_then(|c| self.strip_tool_at(c, now));
        let wheel_hovered = if self.wheel_fully_open() {
            cursor.and_then(|c| self.wheel_target_at(c, now)).flatten()
        } else {
            None
        };
        for index in 0..TOOLS.len() {
            let strip_on = if strip_hovered == Some(index) {
                1.0
            } else {
                0.0
            };
            let wheel_on = if wheel_hovered == Some(index) {
                1.0
            } else {
                0.0
            };
            self.strip_hover[index].set(strip_on, now);
            self.wheel_hover[index].set(wheel_on, now);
        }

        let grace = match self.wheel_opener {
            WheelOpener::Click => Some(WHEEL_CLICK_GRACE),
            WheelOpener::Hover => Some(WHEEL_HOVER_GRACE),
            WheelOpener::Shortcut => None,
        };
        if let (true, Some(grace)) = (self.wheel_target_open, grace) {
            let near = cursor.is_some_and(|c| {
                self.wheel_target_at(c, now).is_some() || self.over_pill(c, now, 0.0)
            });
            if near {
                self.wheel_leave_at = None;
            } else {
                let since = *self.wheel_leave_at.get_or_insert(now);
                if now.duration_since(since) >= grace {
                    self.close_wheel();
                }
            }
        }

        let interactive = self.press.is_some()
            || cursor.is_some_and(|c| {
                self.over_pill(c, now, 6.0)
                    || (self.wheel_target_open && self.wheel_target_at(c, now).is_some())
                    || (self.panel_open && self.over_panel(c, now, 6.0))
            });
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_passthrough(!interactive);
        }

        // Fuera de las animaciones basta con ~20 cuadros/s para la respiración.
        if moved || self.ticks.is_multiple_of(3) {
            cx.notify();
        }
    }

    /// Avanza el estado dependiente del tiempo. Devuelve si hay una animación
    /// en curso que necesite el siguiente cuadro.
    fn advance(&mut self, now: Instant) -> bool {
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let dt_ms = dt * 1000.0;

        let dragging = self.follow_drag(now);

        let wheel_end = wheel_open_ms();
        let wheel_moving = if self.wheel_target_open {
            self.wheel_time = (self.wheel_time + dt_ms).min(wheel_end);
            self.wheel_time < wheel_end
        } else {
            self.wheel_time = (self.wheel_time - dt_ms * CLOSE_SPEED).max(0.0);
            self.wheel_time > 0.0
        };

        let panel_end = panel_open_ms();
        let panel_moving = if self.panel_open {
            self.panel_time = (self.panel_time + dt_ms).min(panel_end);
            self.panel_time < panel_end
        } else {
            self.panel_time = (self.panel_time - dt_ms * PANEL_CLOSE_SPEED).max(0.0);
            self.panel_time > 0.0
        };

        if self
            .seat_started
            .is_some_and(|at| now.duration_since(at).as_secs_f32() * 1000.0 > SEAT_MS)
        {
            self.seat_started = None;
        }

        let target = match self.cursor {
            Some((x, y)) => {
                let (mx, my) = self.mark_center(now);
                let (dx, dy) = (x - mx, y - my);
                let distance = dx.hypot(dy).max(1.0);
                let reach = (distance / 240.0).min(1.0);
                (dx / distance * reach, dy / distance * reach)
            }
            None => (0.0, 0.0),
        };
        let follow = 1.0 - (-dt * 14.0).exp();
        self.eyes.0 += (target.0 - self.eyes.0) * follow;
        self.eyes.1 += (target.1 - self.eyes.1) * follow;
        let eyes_moving =
            (target.0 - self.eyes.0).abs() > 0.01 || (target.1 - self.eyes.1).abs() > 0.01;

        if self.blink_started.is_none() && now >= self.next_blink {
            self.blink_started = Some(now);
            self.double_blink = self.rng.next() < 0.22;
            self.schedule_blink(now);
        }
        if let Some(started) = self.blink_started {
            let elapsed = now.duration_since(started).as_secs_f32() * 1000.0;
            let total = if self.double_blink {
                BLINK_MS + 160.0
            } else {
                BLINK_MS
            };
            if elapsed >= total {
                self.blink_started = None;
            }
        }
        if self
            .strip_pulse
            .is_some_and(|(_, at)| now.duration_since(at) > Duration::from_millis(260))
        {
            self.strip_pulse = None;
        }

        dragging
            || self.press.is_some()
            || wheel_moving
            || panel_moving
            || eyes_moving
            || self.seat_started.is_some()
            || self.blink_started.is_some()
            || self.strip_pulse.is_some()
            || self.strip.is_running(now)
            || self.strip_hover.iter().any(|t| t.is_running(now))
            || self.wheel_hover.iter().any(|t| t.is_running(now))
    }

    fn blink_amount(&self, now: Instant) -> f32 {
        let Some(started) = self.blink_started else {
            return 1.0;
        };
        let elapsed = now.duration_since(started).as_secs_f32() * 1000.0;
        let closedness = |t: f32| {
            let p = (t / BLINK_MS).clamp(0.0, 1.0);
            (p * PI).sin()
        };
        let mut amount = closedness(elapsed);
        if self.double_blink {
            amount = amount.max(closedness(elapsed - 160.0));
        }
        1.0 - amount * 0.9
    }

    // --- Cuadro -------------------------------------------------------------

    fn frame(&self, now: Instant) -> Frame {
        let palette = Palette::dark();
        let breath_t = now.duration_since(self.born).as_secs_f32() / 2.4 * TAU;
        let brightness = 1.04 - 0.04 * breath_t.cos();
        let mut skin = palette.skin;
        skin.l = (skin.l * brightness).min(1.0);

        let pill = self.shape(now);
        let open = self.strip.value(now);
        let reveal = open.clamp(0.0, 1.0);

        let strip_tools = match pill {
            PillShape::Tab { edge, thick, .. } if reveal > 0.02 => TOOLS
                .iter()
                .enumerate()
                .map(|(index, tool)| {
                    let hover = self.strip_hover[index].value(now);
                    let pulse = match self.strip_pulse {
                        Some((pulsed, at)) if pulsed == index => {
                            let t = now.duration_since(at).as_secs_f32() / 0.26;
                            1.0 - 0.18 * (t.clamp(0.0, 1.0) * PI).sin()
                        }
                        _ => 1.0,
                    };
                    let scale = (0.4 + 0.6 * open.max(0.0)) * (1.0 + 0.14 * hover) * pulse;
                    IconDraw {
                        path: tool.icon,
                        center: edge.point(
                            &self.work,
                            self.strip_tool_along(index, now),
                            thick / 2.0,
                        ),
                        size: TOOL_ICON * scale,
                        color: mix(palette.muted, palette.text, hover).opacity(reveal),
                    }
                })
                .collect(),
            _ => Vec::new(),
        };

        let place = self.wheel_place(now);
        let (core, blobs, wheel_icons, wheel_shadow) = self.wheel_frame(&place, &palette, now);

        // Muro líquido: al arrastrar la gota cerca de un borde acoplable, un
        // cuello la une con el borde, como aviso de que ahí se pega.
        let mut walls = Vec::new();
        if let (PillShape::Disc { center }, true) = (pill, self.dragging()) {
            for &edge in &self.dockable {
                let gap = edge.depth_of(&self.work, center) - DISC_R;
                if gap < WALL_REACH {
                    let along = edge.along_of(center);
                    walls.push((edge.point(&self.work, along, -WALL_R), WALL_R));
                }
            }
        }

        Frame {
            skin,
            pill,
            mark: self.mark_center(now),
            eyes: self.eyes,
            blink: self.blink_amount(now),
            strip_tools,
            core,
            core_anchor: place.anchor,
            blobs,
            wheel_icons,
            wheel_shadow,
            panel: self.panel_shape(now),
            walls,
            palette,
        }
    }

    #[allow(clippy::type_complexity)]
    fn wheel_frame(
        &self,
        place: &WheelPlace,
        palette: &Palette,
        now: Instant,
    ) -> (
        Option<(f32, f32, f32)>,
        Vec<(f32, f32, f32)>,
        Vec<IconDraw>,
        Option<(f32, f32, f32, f32)>,
    ) {
        if !self.wheel_visible() {
            return (None, Vec::new(), Vec::new(), None);
        }
        let time = self.wheel_time;

        // El núcleo gotea desde la pill hasta el centro de la rueda.
        let drop = segment(time, 0.0, CORE_DROP_MS);
        let travel = ease_back_out(drop);
        let core_x = lerp(place.source.0, place.center.0, travel);
        let core_y = lerp(place.source.1, place.center.1, travel);
        let core_r = lerp(place.source_r, WHEEL_CORE_R, ease_smooth_out(drop));

        let mut blobs = Vec::new();
        let mut icons = Vec::new();
        let mut extent = core_r;
        for (index, tool) in TOOLS.iter().enumerate() {
            let start = BLOB_START_MS + BLOB_STAGGER_MS * index as f32;
            let progress = segment(time, start, BLOB_MS);
            if progress <= 0.0 {
                continue;
            }
            let angle = wheel_angle(index);
            let reach = WHEEL_RING * ease_back_out(progress);
            let radius = lerp(12.0, WHEEL_BLOB_R, ease_smooth_out(progress));
            let (bx, by) = (core_x + angle.cos() * reach, core_y + angle.sin() * reach);
            blobs.push((bx, by, radius));
            extent = extent.max(reach + radius);

            let shown = segment(time, start + ICON_DELAY_MS, ICON_MS);
            if shown > 0.0 {
                let hover = self.wheel_hover[index].value(now);
                let scale = lerp(0.35, 1.0, ease_smooth_out(shown)) * (1.0 + 0.05 * hover);
                icons.push(IconDraw {
                    path: tool.icon,
                    center: (bx, by),
                    size: WHEEL_ICON * scale,
                    color: mix(palette.muted, palette.text, hover).opacity(shown),
                });
            }
        }
        let shadow_alpha = segment(time, 0.0, 200.0);
        (
            Some((core_x, core_y, core_r)),
            blobs,
            icons,
            Some((core_x, core_y, extent, shadow_alpha)),
        )
    }
}

/// Mismo orden que `paste_clipboard_item` en Atic: dar el foco al destino,
/// esperar a que lo asiente, escribir el portapapeles, esperar y mandar el
/// atajo que esa app entiende.
async fn paste_into(target: Option<paste::Target>, item: ClipboardItem, cx: &mut gpui::AsyncApp) {
    if let Some(target) = target {
        paste::force_foreground(target);
    }
    cx.background_executor().timer(PASTE_FOCUS_DELAY).await;
    if cx.update(|cx| cx.write_to_clipboard(item)).is_err() {
        return;
    }
    cx.background_executor().timer(PASTE_KEY_DELAY).await;
    paste::send_paste_chord(target.is_some_and(paste::needs_ctrl_shift_v));
}

/// Las imágenes se arrastran como archivo: `CF_HDROP` lo entiende casi todo.
fn write_drag_image(id: usize, bytes: &[u8]) -> std::io::Result<String> {
    let dir = std::env::temp_dir().join("atic-gpui-drag");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("atic-imagen-{id}.png"));
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

// --- Posición guardada -------------------------------------------------------
//
// Atic guarda borde y posición relativa (0..1) para sobrevivir a cambios de
// resolución (`atic.pill.home`). Aquí, un archivo de texto en LOCALAPPDATA.

fn home_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(
        std::path::PathBuf::from(base)
            .join("atic-gpui")
            .join("home.txt"),
    )
}

fn save_home(home: &Home, work: &Rect) {
    let Some(path) = home_file() else {
        return;
    };
    let fraction = |value: f32, start: f32, length: f32| ((value - start) / length).clamp(0.0, 1.0);
    let line = match *home {
        Home::Docked { edge, along } => {
            let (start, end) = edge.span(work);
            format!("{edge:?} {}", fraction(along, start, end - start))
        }
        Home::Floating { center } => format!(
            "Floating {} {}",
            fraction(center.0, work.x, work.w),
            fraction(center.1, work.y, work.h)
        ),
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, line);
}

fn load_home(work: &Rect, dockable: &[Edge]) -> Option<Home> {
    let text = std::fs::read_to_string(home_file()?).ok()?;
    let mut parts = text.split_whitespace();
    let kind = parts.next()?;
    let a: f32 = parts.next()?.parse().ok()?;
    if kind == "Floating" {
        let b: f32 = parts.next()?.parse().ok()?;
        return Some(Home::Floating {
            center: (work.x + a * work.w, work.y + b * work.h),
        });
    }
    let edge = match kind {
        "Top" => Edge::Top,
        "Bottom" => Edge::Bottom,
        "Left" => Edge::Left,
        "Right" => Edge::Right,
        _ => return None,
    };
    // Si ese borde ya no se puede usar (barra de tareas movida), vuelve al
    // lugar por defecto.
    if !dockable.contains(&edge) {
        return None;
    }
    let (start, end) = edge.span(work);
    Some(Home::Docked {
        edge,
        along: geometry::clamp_along(edge, work, start + a * (end - start), TAB_LENGTH),
    })
}

fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    Hsla {
        h: lerp(from.h, to.h, t),
        s: lerp(from.s, to.s, t),
        l: lerp(from.l, to.l, t),
        a: lerp(from.a, to.a, t),
    }
}

fn pill_shadow(alpha: f32) -> BoxShadow {
    BoxShadow {
        color: Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.0,
            a: 0.38 * alpha,
        },
        offset: point(px(0.), px(10.)),
        blur_radius: px(22.),
        spread_radius: px(0.),
    }
}

fn bounds_of(rect: &Rect) -> Bounds<gpui::Pixels> {
    Bounds::new(point(px(rect.x), px(rect.y)), size(px(rect.w), px(rect.h)))
}

/// Esquinas redondeadas del tab: solo las del lado que mira al escritorio.
fn tab_corners(edge: Edge, radius: f32) -> [f32; 4] {
    match edge {
        Edge::Top => [0.0, 0.0, radius, radius],
        Edge::Bottom => [radius, radius, 0.0, 0.0],
        Edge::Left => [0.0, radius, radius, 0.0],
        Edge::Right => [radius, 0.0, 0.0, radius],
    }
}

/// El tab se extiende 1 px fuera de la pantalla para que el antialias no deje
/// una línea clara pegada al borde; la sombra, un radio entero.
fn grow_outward(rect: &Rect, edge: Edge, by: f32) -> Rect {
    match edge {
        Edge::Top => Rect::new(rect.x, rect.y - by, rect.w, rect.h + by),
        Edge::Bottom => Rect::new(rect.x, rect.y, rect.w, rect.h + by),
        Edge::Left => Rect::new(rect.x - by, rect.y, rect.w + by, rect.h),
        Edge::Right => Rect::new(rect.x, rect.y, rect.w + by, rect.h),
    }
}

impl Frame {
    fn paint(self, window: &mut Window, cx: &mut App) {
        // Sombras primero: así la de una gota no oscurece a su vecina.
        match self.pill {
            PillShape::Tab { edge, rect, .. } => window.paint_shadows(
                bounds_of(&grow_outward(&rect, edge, TAB_RADIUS)),
                Corners::all(px(TAB_RADIUS)),
                &[pill_shadow(1.0)],
            ),
            PillShape::Disc { center } => window.paint_shadows(
                bounds_of(&Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0)),
                Corners::all(px(DISC_R)),
                &[pill_shadow(1.0)],
            ),
        }
        if let Some((x, y, r, alpha)) = self.wheel_shadow {
            window.paint_shadows(
                bounds_of(&Rect::centered((x, y), r * 2.0, r * 2.0)),
                Corners::all(px(r)),
                &[pill_shadow(alpha)],
            );
        }
        if let Some(panel) = &self.panel {
            window.paint_shadows(
                bounds_of(&panel.rect),
                Corners::all(px(PANEL_CORNER)),
                &[pill_shadow(1.0)],
            );
        }

        let mut skin = liquid::Silhouette::new();
        match self.pill {
            PillShape::Tab { edge, rect, .. } => {
                let radius = TAB_RADIUS.min(rect.w.min(rect.h) / 2.0);
                skin.rounded_rect_corners(
                    &grow_outward(&rect, edge, 1.0),
                    tab_corners(edge, radius),
                );
            }
            PillShape::Disc { center } => {
                skin.circle(center, DISC_R);
                for &(wall, wall_r) in &self.walls {
                    skin.neck_within(center, DISC_R, wall, wall_r, WALL_REACH);
                }
            }
        }
        if let Some(panel) = &self.panel {
            let r = &panel.rect;
            skin.rounded_rect(r.x, r.y, r.w, r.h, PANEL_CORNER);
            // El cuello une la pill con la cara del panel y se corta cuando el
            // panel se aleja más de 12 px.
            let (anchor, anchor_r) = panel.anchor;
            let (near, near_r) = panel.near;
            skin.neck_within(anchor, anchor_r, near, near_r, PANEL_NECK_REACH);
        }
        if let Some((x, y, r)) = self.core {
            let (anchor, anchor_r) = self.core_anchor;
            skin.neck(anchor, anchor_r, (x, y), r);
            skin.circle((x, y), r);
            for &(bx, by, br) in &self.blobs {
                skin.neck((x, y), r, (bx, by), br);
                skin.circle((bx, by), br);
            }
        }
        if let Some(path) = skin.build() {
            window.paint_path(path, self.skin);
        }

        self.paint_mark(window);
        for icon in self.strip_tools.iter().chain(&self.wheel_icons) {
            let _ = window.paint_svg(
                bounds_of(&Rect::centered(icon.center, icon.size, icon.size)),
                icon.path.into(),
                TransformationMatrix::unit(),
                icon.color,
                cx,
            );
        }
    }

    /// Marca de Atic: una "a" minúscula con ojos que siguen al cursor.
    fn paint_mark(&self, window: &mut Window) {
        let unit = MARK_SIZE / 24.0;
        let origin = (self.mark.0 - 12.0 * unit, self.mark.1 - 12.0 * unit);
        let at = |x: f32, y: f32| (origin.0 + x * unit, origin.1 + y * unit);
        let color = self.palette.text;
        let stroke = 1.5 * unit;

        let (cx, cy) = at(12.0, 12.0);
        if let Some(ring) = ring_path(cx, cy, 5.5 * unit, stroke) {
            window.paint_path(ring, color);
        }
        let (sx, top) = at(17.5, 6.5);
        let (_, bottom) = at(17.5, 17.5);
        let mut stem = gpui::PathBuilder::stroke(px(stroke));
        stem.move_to(point(px(sx), px(top)));
        stem.line_to(point(px(sx), px(bottom)));
        if let Ok(path) = stem.build() {
            window.paint_path(path, color);
        }
        for y in [top, bottom] {
            if let Some(cap) = liquid::ellipse(sx, y, stroke / 2.0, stroke / 2.0) {
                window.paint_path(cap, color);
            }
        }

        for eye_x in [10.3, 13.7] {
            let (ex, ey) = at(eye_x + self.eyes.0 * 1.1, 11.6 + self.eyes.1 * 1.1);
            if let Some(eye) = liquid::ellipse(ex, ey, 0.88 * unit, 1.32 * unit * self.blink) {
                window.paint_path(eye, color);
            }
        }
    }
}

fn ring_path(cx: f32, cy: f32, r: f32, width: f32) -> Option<gpui::Path<gpui::Pixels>> {
    let mut builder = gpui::PathBuilder::stroke(px(width));
    builder.move_to(point(px(cx + r), px(cy)));
    builder.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        true,
        point(px(cx - r), px(cy)),
    );
    builder.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        true,
        point(px(cx + r), px(cy)),
    );
    builder.close();
    builder.build().ok()
}

impl Render for Pill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scale_factor = window.scale_factor();
        let now = Instant::now();
        let animating = self.advance(now);
        if animating {
            window.request_animation_frame();
        }
        if std::env::var_os("PILL_FPS").is_some() {
            self.fps.1 += 1;
            if now.duration_since(self.fps.0) >= Duration::from_secs(1) {
                eprintln!(
                    "fps {} animando={animating} rueda={:.0}ms",
                    self.fps.1, self.wheel_time
                );
                self.fps = (now, 0);
            }
        }
        let frame = self.frame(now);
        // El contenido se monta apenas se abre (para que el buscador reciba el
        // foco) y aparece cuando el panel ya tiene su tamaño.
        let panel_content = frame
            .panel
            .as_ref()
            .map(|shape| (shape.rect.x, shape.rect.y, shape.content));

        div()
            .size_full()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pill, event: &MouseDownEvent, window, cx| {
                    pill.on_mouse_down(event, window, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pill, _: &MouseUpEvent, window, cx| pill.release(window, cx)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| frame.paint(window, cx),
                )
                .size_full(),
            )
            .when_some(panel_content, |root, (left, top, alpha)| {
                root.child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(PANEL_W))
                        .h(px(PANEL_H))
                        .opacity(alpha)
                        .child(self.panel.clone()),
                )
            })
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        text_input::bind_keys(cx);
        clipboard::bind_keys(cx);
        let display = cx.primary_display();
        let screen = display
            .as_ref()
            .map(|display| display.bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.))));
        // Una sola ventana transparente que cubre el monitor: la pill se mueve
        // dentro, como en Atic, y fuera de ella los clics la atraviesan.
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(screen)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.map(|display| display.id()),
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        cx.open_window(options, |window, cx| cx.new(|cx| Pill::new(window, cx)))
            .expect("no se pudo abrir la ventana de la pill");
    });
}
