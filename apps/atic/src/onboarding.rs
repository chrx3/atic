//! La bienvenida de la primera vez: una sola ventana por pasos (Hola,
//! Permisos en Mac, Voz e IA, Herramientas, La pill y Listo).
//!
//! - Los permisos que se activan en una lista de Ajustes del Sistema ponen la
//!   ventana al lado de Ajustes y ofrecen el ícono de Atic para arrastrarlo a
//!   la lista; un fantasma del ícono muestra el gesto, dibujado en la ventana
//!   de la pill, que cubre la pantalla (`overlay`).
//! - Las herramientas elegidas vuelan hasta la pill, y la tira se arma en el
//!   acto (`take_layout`).
//! - Cada permiso, la descarga del modelo local y cada paso del tour se marcan
//!   solos: se relee cada cuarto de segundo.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use atic_core::{AppDirs, Config, SecretKind};
use gpui::{
    canvas, div, prelude::*, px, size, svg, AnimationExt, AnyElement, App, Bounds, ClickEvent, Context, FontWeight, Hsla,
    MouseButton, MouseDownEvent, Pixels, SharedString, Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::i18n::{t, tf};
use crate::kit::chrome;
use crate::meetings::settings::{card, key_row, save_config, segmented, shown_key, ConfigPane};
use crate::pill_tools::{self, Layout};
use crate::settings::{hsla, text_button, AMBER, FAINT, FILL, INK, MUTED, TEXT};
#[cfg(target_os = "macos")]
use crate::platform::permissions::{self, Level, Permission, State};

const WINDOW_W: f32 = 820.0;
const WINDOW_H: f32 = 790.0;
/// Al lado de Ajustes del Sistema: angosta, para que entren las dos.
#[cfg(target_os = "macos")]
const SIDE_W: f64 = 380.0;
#[cfg(target_os = "macos")]
const SIDE_H: f64 = 540.0;
const TOP_H: f32 = 44.0;
const GUTTER: f32 = 32.0;
const GREEN: u32 = crate::theme::GREEN;
const RING_R: f32 = 165.0;
const RING_BOX: (f32, f32) = (440.0, 400.0);
const TICK: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Hola,
    Permisos,
    Voz,
    Herramientas,
    Tour,
    Listo,
}

fn steps() -> &'static [Step] {
    if cfg!(target_os = "macos") {
        &[Step::Hola, Step::Permisos, Step::Voz, Step::Herramientas, Step::Tour, Step::Listo]
    } else {
        &[Step::Hola, Step::Voz, Step::Herramientas, Step::Tour, Step::Listo]
    }
}

fn step_name(step: Step) -> &'static str {
    match step {
        Step::Hola => t("pill.onboarding.steps.hola"),
        Step::Permisos => t("pill.onboarding.steps.permissions"),
        Step::Voz => t("pill.onboarding.steps.voice"),
        Step::Herramientas => t("pill.onboarding.steps.tools"),
        Step::Tour => t("pill.onboarding.steps.tour"),
        Step::Listo => t("pill.onboarding.done"),
    }
}

// --- Lo que la pill lee de la bienvenida -------------------------------------------

/// Lo que se hace sobre la pill durante el tour (`tour`).
#[derive(Clone, Copy)]
pub(crate) enum Tour {
    /// Se abrió la tira al pasar el cursor.
    Strip = 1,
    /// Se abrió una herramienta.
    Tool = 2,
    /// Se abrió la rueda (arrastrando la pill fuera del borde).
    Wheel = 4,
    /// Se abrieron los Ajustes tocando la cara.
    Settings = 8,
    /// Se movió la pill arrastrándola.
    Move = 16,
    /// Se abrió el editor de la tira (mantener apretada una herramienta).
    Customize = 32,
}

/// Lo que se enseña solo desde Ajustes → Cómo se hace (no va en el tour).
/// Lleva las claves de los textos: se traducen en `task_text`.
const CUSTOMIZE: (Tour, &str, &str) = (
    Tour::Customize,
    "pill.onboarding.guides.customizeTitle",
    "pill.onboarding.guides.customizeHint",
);

/// Mover a Atic, en una pantalla con notch de cámara: ahí se junta con él.
const MOVE_NOTCH: &str = "pill.onboarding.tour.moveNotchHint";

/// La pantalla de Atic tiene notch de cámara (lo cuenta la pill).
static HAS_NOTCH: AtomicBool = AtomicBool::new(false);

/// El título y qué hacer de un paso.
fn task_text(task: u8) -> (&'static str, &'static str) {
    if task == Tour::Move as u8 && HAS_NOTCH.load(Ordering::Relaxed) {
        return (t("pill.onboarding.tour.moveTitle"), t(MOVE_NOTCH));
    }
    TASKS
        .iter()
        .chain(std::iter::once(&CUSTOMIZE))
        .find(|(t, ..)| *t as u8 == task)
        .map_or(("", ""), |(_, title, hint)| (t(*title), t(*hint)))
}

/// Una guía suelta (Ajustes → Cómo se hace): un solo paso, que se cierra al
/// lograrlo.
static GUIDE: Mutex<Option<(u8, Instant)>> = Mutex::new(None);
const GUIDE_FOR: Duration = Duration::from_secs(120);

/// Muestra cómo se hace `task` sobre la pill.
pub(crate) fn show_guide(task: Tour) {
    TOUR.fetch_and(!(task as u8), Ordering::Relaxed);
    if let Ok(mut guide) = GUIDE.lock() {
        *guide = Some((task as u8, Instant::now()));
    }
    set_demo(Some(task as u8));
}

/// Termina la guía: lograda (tras felicitar) o pasado su tiempo.
fn end_guide_if_done() -> bool {
    let Some((task, since)) = GUIDE.lock().ok().and_then(|g| *g) else {
        return false;
    };
    let done = TOUR.load(Ordering::Relaxed) & task != 0;
    let cheered = CHEER.lock().ok().and_then(|c| *c).is_none_or(|c| c.elapsed() >= CHEER_FOR);
    if (done && cheered) || since.elapsed() >= GUIDE_FOR {
        if let Ok(mut guide) = GUIDE.lock() {
            *guide = None;
        }
        set_demo(None);
    }
    true
}

/// Los pasos del tour, en orden: qué hacer y cómo (claves de los textos, que
/// se traducen en `task_text`).
const TASKS: [(Tour, &str, &str); 5] = [
    (Tour::Strip, "pill.onboarding.tour.stripTitle", "pill.onboarding.tour.stripHint"),
    (Tour::Tool, "pill.onboarding.tour.toolTitle", "pill.onboarding.tour.toolHint"),
    (Tour::Settings, "pill.onboarding.tour.settingsTitle", "pill.onboarding.tour.settingsHint"),
    (Tour::Move, "pill.onboarding.tour.moveTitle", "pill.onboarding.tour.moveHint"),
    (Tour::Wheel, "pill.onboarding.tour.wheelTitle", "pill.onboarding.tour.wheelHint"),
];

static TOUR: AtomicU8 = AtomicU8::new(0);

/// La pill avisa lo que se hizo; la bienvenida lo marca en el tour.
pub(crate) fn tour(task: Tour) {
    let before = TOUR.fetch_or(task as u8, Ordering::Relaxed);
    // Recién hecho, durante el tour: se felicita en pantalla.
    if before & task as u8 == 0 && DEMO.lock().is_ok_and(|d| d.is_some()) {
        if let Ok(mut cheer) = CHEER.lock() {
            *cheer = Some(Instant::now());
        }
    }
}

/// Cuándo se completó el último paso del tour, para el «¡Bien!».
static CHEER: Mutex<Option<Instant>> = Mutex::new(None);
const CHEER_FOR: Duration = Duration::from_millis(1600);

/// Lo que la pill cuenta en cada cuadro: si se está usando (el cursor cerca
/// o apretada) y si tiene algo abierto (el notch o la rueda).
static PILL_NEAR: AtomicBool = AtomicBool::new(false);
/// El notch abierto (un panel) y la rueda abierta.
static PILL_OPEN: AtomicBool = AtomicBool::new(false);
static PILL_WHEEL: AtomicBool = AtomicBool::new(false);
/// Lo que ocupa la pill (x, y, ancho, alto en su ventana) y el ancho de esa
/// ventana: el texto va al lado.
static PILL_TAKEN: Mutex<((f32, f32, f32, f32), f32)> = Mutex::new(((0.0, 0.0, 0.0, 0.0), 0.0));
/// Desde cuándo nadie la toca: el ejemplo vuelve un rato después.
static IDLE_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

pub(crate) fn pill_state(near: bool, open: bool, wheel: bool, taken: (f32, f32, f32, f32), width: f32, notch: bool) {
    HAS_NOTCH.store(notch, Ordering::Relaxed);
    PILL_NEAR.store(near, Ordering::Relaxed);
    PILL_OPEN.store(open, Ordering::Relaxed);
    PILL_WHEEL.store(wheel, Ordering::Relaxed);
    if let Ok(mut slot) = PILL_TAKEN.lock() {
        *slot = (taken, width);
    }
    if let Ok(mut idle) = IDLE_SINCE.lock() {
        if near {
            *idle = None;
        } else if idle.is_none() {
            *idle = Some(Instant::now());
        }
    }
}

/// Llegó una herramienta: la pill abre la tira un momento para mostrarla.
static LANDING: AtomicBool = AtomicBool::new(false);

pub(crate) fn take_landing() -> bool {
    LANDING.swap(false, Ordering::Relaxed)
}

/// La tira que se eligió, para que la pill la aplique (`layout_changed`).
static LAYOUT: Mutex<Option<Layout>> = Mutex::new(None);

pub(crate) fn take_layout() -> Option<Layout> {
    LAYOUT.lock().ok()?.take()
}

/// El ícono que muestra cómo arrastrar Atic a la lista de Ajustes: de dónde a
/// dónde, en puntos globales (origen arriba a la izquierda).
#[derive(Clone, Copy)]
struct Ghost {
    from: (f32, f32),
    to: (f32, f32),
    since: Instant,
}

static GHOST: Mutex<Option<Ghost>> = Mutex::new(None);

/// El ejemplo del tour sobre la pill: qué paso y desde cuándo.
static DEMO: Mutex<Option<(u8, Instant)>> = Mutex::new(None);
/// Desde cuándo se ve el ejemplo de los pasos que se callan mientras usas la
/// pill: al reaparecer arranca desde el principio, no a media animación.
static DEMO_SHOWN: Mutex<Option<Instant>> = Mutex::new(None);
/// La tarea del cartel y de qué lado de la pill va (0 derecha, 1 izquierda,
/// 2 arriba o abajo): se queda en ese lado mientras quepa (antes cruzaba al
/// otro de un cuadro al siguiente).
static COACH_SIDE: Mutex<Option<(u8, u8)>> = Mutex::new(None);
/// Lo que ocupa el cartel en la ventana de la pill (x, y, ancho, alto): ahí la
/// ventana deja de dejar pasar los clics, para que «Saltar tutorial» se pueda
/// tocar (`over_coach`). Se anota al dibujarlo.
static COACH_AT: Mutex<Option<(f32, f32, f32, f32)>> = Mutex::new(None);
/// Se tocó «Saltar tutorial» en el cartel: la bienvenida lo atiende en su tick.
static SKIP: AtomicBool = AtomicBool::new(false);

/// El velo que oscurece la pantalla durante el tour, con un hueco alrededor
/// de la pill. Se acerca cuadro a cuadro a lo que toca (desde su propio
/// estado, no desde cuándo se dibujó), así se puede redirigir sin saltos.
struct Veil {
    alpha: f32,
    /// x, y, ancho, alto en la ventana de la pill.
    hole: [f32; 4],
    at: Instant,
}

static VEIL: Mutex<Option<Veil>> = Mutex::new(None);
/// Cuánto oscurece el velo: deja ver el escritorio, pero la pill manda.
const VEIL_ALPHA: f32 = 0.6;
/// Tras cada vuelta del ejemplo, una pausa para que lo hagas tú.
const YOUR_TURN: f32 = 2.4;
/// Cuánto viaja el ejemplo al mover la pill por el borde y al sacarla de él.
const MOVE_REACH: f32 = 220.0;
const WHEEL_REACH: f32 = 190.0;

/// ¿Está el cursor sobre el cartel del tour? La pill no deja pasar los clics
/// ahí (`set_passthrough`): así «Saltar tutorial» se puede tocar.
pub(crate) fn over_coach(p: (f32, f32)) -> bool {
    COACH_AT
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|(x, y, w, h)| p.0 >= x - 6.0 && p.0 <= x + w + 6.0 && p.1 >= y - 6.0 && p.1 <= y + h + 6.0)
}

/// «Saltar tutorial» en el cartel: una guía suelta solo se cierra; el tour lo
/// termina la bienvenida (marca `onboarding_done` y pasa a Listo).
fn skip_from_coach() {
    let guide = GUIDE.lock().ok().is_some_and(|mut guide| guide.take().is_some());
    set_demo(None);
    if !guide {
        SKIP.store(true, Ordering::Relaxed);
    }
}

fn set_demo(task: Option<u8>) {
    if let Ok(mut slot) = DEMO.lock() {
        let same = slot.map(|(t, _)| t) == task;
        if !same {
            *slot = task.map(|t| (t, Instant::now()));
        }
    }
}

/// Una herramienta que vuela de la rueda de la bienvenida a la pill.
#[derive(Clone)]
struct Flight {
    icon: &'static str,
    from: (f32, f32),
    since: Instant,
}

static FLIGHTS: Mutex<Vec<Flight>> = Mutex::new(Vec::new());
const FLIGHT: Duration = Duration::from_millis(650);
const GHOST_LOOP: f32 = 2.8;

fn set_ghost(ghost: Option<Ghost>) {
    if let Ok(mut slot) = GHOST.lock() {
        *slot = ghost;
    }
}

/// Lo que la bienvenida dibuja en la ventana de la pill: el velo del tour, el
/// fantasma del arrastre, el ejemplo con su cartel y las herramientas en
/// vuelo. `origin` es la esquina de la ventana de la pill en puntos globales;
/// `target`, dónde aterrizan (en la ventana). `reduced`: movimiento reducido.
pub(crate) fn overlay(origin: (f32, f32), target: (f32, f32), reduced: bool, window: &mut Window) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let viewport = window.viewport_size();
    let screen = (f32::from(viewport.width), f32::from(viewport.height));
    // El cartel lo vuelve a anotar al dibujarse (`over_coach`).
    if let Ok(mut at) = COACH_AT.lock() {
        *at = None;
    }
    let ink = ink();
    let guide = end_guide_if_done();
    let wheel = PILL_WHEEL.load(Ordering::Relaxed);
    let (taken, _) = PILL_TAKEN.lock().map(|t| *t).unwrap_or(((0.0, 0.0, 0.0, 0.0), 0.0));
    let dir = demo_dir(target, screen);
    // Lo del tour: el ejemplo (si toca verlo ahora) y lo que necesita el cartel.
    let mut demo_out = Vec::new();
    let mut hole_goal = None;
    let mut pulse = None;
    let mut coach_args = None;
    if let Some((task, since)) = DEMO.lock().ok().and_then(|d| *d) {
        window.request_animation_frame();
        let near = PILL_NEAR.load(Ordering::Relaxed);
        let open = PILL_OPEN.load(Ordering::Relaxed);
        let idle = IDLE_SINCE.lock().ok().and_then(|i| *i).map_or(f32::MAX, |i| i.elapsed().as_secs_f32());
        // Con el notch abierto, tocar la cara lo cierra (no abre Ajustes), y
        // la pill no se arrastra: esos pasos piden cerrarlo primero.
        let blocked = (open || wheel)
            && matches!(task, t if t == Tour::Move as u8 || t == Tour::Wheel as u8 || t == Tour::Settings as u8);
        // Tocar una herramienta o la cara: el ejemplo sigue mostrando dónde,
        // aunque el cursor esté encima (la tira se abre al acercarse).
        // Arrastrar: calla mientras se usa la pill.
        let points = task == Tour::Tool as u8 || task == Tour::Settings as u8 || task == Tour::Customize as u8;
        let show = !blocked && if points { !wheel } else { !near && !open && idle > 1.2 };
        let shown_since = DEMO_SHOWN.lock().ok().map(|mut shown| {
            if show {
                *shown.get_or_insert_with(Instant::now)
            } else {
                *shown = None;
                Instant::now()
            }
        });
        let t = if points { since.elapsed().as_secs_f32() } else { shown_since.map_or(0.0, |at| at.elapsed().as_secs_f32()) };
        // Una vuelta del ejemplo y una pausa para que lo hagas tú.
        let span = cycle(task);
        let lap = t % (span + YOUR_TURN);
        let watching = show && lap < span;
        if watching {
            demo_out = demo(task, lap / span, target, dir, reduced, &ink);
        } else if !reduced {
            // Tu turno: el borde del hueco late, para decir «aquí».
            pulse = Some((since.elapsed().as_secs_f32() % 1.4) / 1.4);
        }
        hole_goal = Some(spot(task, taken, target, dir));
        coach_args = Some((task, blocked, watching));
    }
    // El velo va debajo de todo lo demás. Con la rueda abierta se va: es grande
    // y aparece donde esté el cursor.
    let mut hole = hole_goal;
    if let Some((alpha, now_hole, moving)) = veil_step(hole_goal, !wheel, reduced) {
        if moving {
            window.request_animation_frame();
        }
        hole = Some(now_hole);
        out.extend(veil(alpha, now_hole, screen, pulse, ink.accent));
    }
    let ghost = GHOST.lock().ok().and_then(|slot| *slot);
    if let Some(ghost) = ghost {
        window.request_animation_frame();
        let t = (ghost.since.elapsed().as_secs_f32() % GHOST_LOOP) / GHOST_LOOP;
        // Aparece, espera, viaja, se "suelta" y se desvanece.
        let travel = crate::anim::ease_smooth_out(crate::anim::segment(t, 0.18, 0.44));
        let alpha = crate::anim::segment(t, 0.0, 0.08) * (1.0 - crate::anim::segment(t, 0.72, 0.1));
        let scale = 1.06 - 0.46 * crate::anim::segment(t, 0.62, 0.2);
        let x = ghost.from.0 + (ghost.to.0 - ghost.from.0) * travel - origin.0;
        let y = ghost.from.1 + (ghost.to.1 - ghost.from.1) * travel - origin.1;
        let side = 68.0 * scale;
        out.push(
            div()
                .absolute()
                .left(px(x - side / 2.0))
                .top(px(y - side / 2.0))
                .size(px(side))
                .rounded(px(16.0 * scale))
                .bg(gpui::rgb(0x2a2a2d))
                .border_1()
                .border_color(gpui::white().opacity(0.18))
                .shadow_lg()
                .opacity(alpha)
                .flex()
                .items_center()
                .justify_center()
                .child(face(40.0 * scale, crate::face::Pose::REST, false, None))
                .child(
                    svg()
                        .path("icons/mouse-pointer-2.svg")
                        .absolute()
                        .right(px(6.0))
                        .bottom(px(4.0))
                        .size(px(20.0 * scale))
                        .text_color(gpui::white()),
                )
                .into_any_element(),
        );
    }
    out.extend(demo_out);
    if let (Some((task, blocked, watching)), Some(hole)) = (coach_args, hole) {
        out.push(coach(task, blocked, hole, screen, guide, watching, reduced, &ink));
    }
    let mut flights = FLIGHTS.lock().map(|f| f.clone()).unwrap_or_default();
    flights.retain(|f| f.since.elapsed() < FLIGHT);
    if let Ok(mut slot) = FLIGHTS.lock() {
        slot.retain(|f| f.since.elapsed() < FLIGHT);
    }
    if !flights.is_empty() {
        window.request_animation_frame();
    }
    for flight in flights {
        let t = flight.since.elapsed().as_secs_f32() / FLIGHT.as_secs_f32();
        // Arranca suave y acelera hacia la pill, como algo que cae en ella.
        let k = t * t;
        let from = (flight.from.0 - origin.0, flight.from.1 - origin.1);
        let x = from.0 + (target.0 - from.0) * k;
        let y = from.1 + (target.1 - from.1) * k;
        let side = 46.0 * (1.0 - 0.55 * k);
        out.push(
            div()
                .absolute()
                .left(px(x - side / 2.0))
                .top(px(y - side / 2.0))
                .size(px(side))
                .rounded_full()
                .bg(gpui::rgb(0xf0f0ea))
                .opacity(1.0 - 0.7 * crate::anim::segment(t, 0.7, 0.3))
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path(flight.icon).size(px(side * 0.44)).text_color(gpui::rgb(0x1a1a18)))
                .into_any_element(),
        );
    }
    out
}

/// Los colores del tour (el cartel y el cursor de ejemplo), del tema de ahora.
#[derive(Clone, Copy)]
struct Ink {
    card: Hsla,
    text: Hsla,
    muted: Hsla,
    line: Hsla,
    accent: Hsla,
    on_accent: Hsla,
    soft: Hsla,
    on_soft: Hsla,
    ok: Hsla,
    warn: Hsla,
}

fn ink() -> Ink {
    match crate::theme::expressive() {
        Some(t) => Ink {
            card: t.raised,
            text: t.text,
            muted: t.muted,
            line: t.border,
            accent: t.accent,
            on_accent: t.on_accent,
            soft: t.accent_soft,
            on_soft: t.on_accent_soft,
            ok: t.ok,
            warn: t.warn,
        },
        None => Ink {
            card: gpui::rgb(0x1d1d1b).into(),
            text: hsla(TEXT),
            muted: hsla(MUTED),
            line: hsla(TEXT).opacity(0.2),
            accent: hsla(FILL),
            on_accent: hsla(INK),
            soft: hsla(TEXT).opacity(0.12),
            on_soft: hsla(TEXT),
            ok: hsla(GREEN),
            warn: hsla(AMBER),
        },
    }
}

/// Avanza el velo un cuadro: hacia oscurecer con el hueco en `hole`, o hacia
/// irse. Devuelve cuánto oscurece, su hueco y si sigue moviéndose; nada si ya
/// se fue.
fn veil_step(hole: Option<[f32; 4]>, dim: bool, reduced: bool) -> Option<(f32, [f32; 4], bool)> {
    let mut slot = VEIL.lock().ok()?;
    let now = Instant::now();
    if slot.is_none() {
        *slot = Some(Veil { alpha: 0.0, hole: hole?, at: now });
    }
    let goal = if hole.is_some() && dim { VEIL_ALPHA } else { 0.0 };
    let veil = slot.as_mut()?;
    let dt = now.duration_since(veil.at).as_secs_f32().min(0.1);
    veil.at = now;
    // Movimiento reducido: el velo y su hueco llegan sin transición.
    let (fade, glide) = if reduced { (1.0, 1.0) } else { (1.0 - (-dt * 8.0).exp(), 1.0 - (-dt * 14.0).exp()) };
    veil.alpha += (goal - veil.alpha) * fade;
    let mut moving = (goal - veil.alpha).abs() > 0.004;
    if let Some(hole) = hole {
        for (now, goal) in veil.hole.iter_mut().zip(hole) {
            *now += (goal - *now) * glide;
            moving |= (goal - *now).abs() > 0.5;
        }
    }
    let state = (veil.alpha, veil.hole, moving);
    if hole.is_none() && veil.alpha < 0.01 {
        *slot = None;
        return None;
    }
    Some(state)
}

/// El velo: toda la ventana oscura menos un hueco redondeado alrededor de lo
/// que hay que tocar, con un borde de acento (que late si `pulse`).
///
/// GPUI no tiene máscaras: el velo es un solo cuadro con un borde enorme, cuyo
/// borde interior redondeado es el hueco (el radio de adentro es el de afuera
/// menos el ancho del borde). Es solo dibujo: la ventana de la pill deja pasar
/// los clics fuera de la pill y del cartel, así que no atrapa a nadie.
fn veil(alpha: f32, hole: [f32; 4], screen: (f32, f32), pulse: Option<f32>, accent: Hsla) -> Vec<AnyElement> {
    let [x, y, w, h] = hole;
    let radius = 22.0f32.min(w / 2.0).min(h / 2.0).max(0.0);
    let reach = screen.0.max(screen.1) + 200.0;
    let shade = gpui::black().opacity(alpha);
    let mut out = vec![canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let o = bounds.origin;
            let rect = Bounds::new(
                gpui::point(o.x + px(x - reach), o.y + px(y - reach)),
                size(px(w + 2.0 * reach), px(h + 2.0 * reach)),
            );
            window.paint_quad(
                gpui::fill(rect, gpui::transparent_black())
                    .corner_radii(px(radius + reach))
                    .border_widths(px(reach))
                    .border_color(shade),
            );
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()];
    // El borde sigue al velo: aparece y se va con él.
    let strength = (alpha / VEIL_ALPHA).clamp(0.0, 1.0);
    let ring = |grow: f32, a: f32| {
        div()
            .absolute()
            .left(px(x - grow))
            .top(px(y - grow))
            .w(px(w + 2.0 * grow))
            .h(px(h + 2.0 * grow))
            .rounded(px(radius + grow))
            .border_2()
            .border_color(accent.opacity(a))
            .into_any_element()
    };
    out.push(ring(2.0, 0.9 * strength));
    if let Some(p) = pulse {
        out.push(ring(2.0 + 16.0 * crate::anim::ease_smooth_out(p), 0.75 * (1.0 - p) * strength));
    }
    out
}

/// Hacia dónde viaja el ejemplo: a la derecha y hacia abajo, salvo que la pill
/// esté tan cerca de ese borde que se saldría de la pantalla.
fn demo_dir(at: (f32, f32), screen: (f32, f32)) -> (f32, f32) {
    let dx = if at.0 + MOVE_REACH + 80.0 > screen.0 { -1.0 } else { 1.0 };
    let dy = if at.1 + WHEEL_REACH + 120.0 > screen.1 { -1.0 } else { 1.0 };
    (dx, dy)
}

/// El hueco del velo para `task` (x, y, ancho, alto): lo que ocupa la pill más
/// por donde pasa el ejemplo.
fn spot(task: u8, taken: (f32, f32, f32, f32), at: (f32, f32), (dx, dy): (f32, f32)) -> [f32; 4] {
    let (x, y, w, h) = taken;
    let mut r = [x, y, x + w, y + h];
    let mut add = |(px_, py_): (f32, f32), pad: f32| {
        r[0] = r[0].min(px_ - pad);
        r[1] = r[1].min(py_ - pad);
        r[2] = r[2].max(px_ + pad);
        r[3] = r[3].max(py_ + pad);
    };
    add(at, 26.0);
    if task == Tour::Tool as u8 || task == Tour::Customize as u8 {
        add((at.0 + 64.0, at.1 + 4.0), 30.0);
    }
    // Por donde llega el cursor de ejemplo (con su etiqueta): el cartel va
    // fuera del hueco, así no lo tapa (antes quedaba encima del recorrido).
    if task != Tour::Move as u8 && task != Tour::Wheel as u8 {
        let goal_x = if task == Tour::Tool as u8 || task == Tour::Customize as u8 { at.0 + 64.0 } else { at.0 + 4.0 };
        add((goal_x + 150.0 * dx, at.1 + 4.0 + 170.0 * dy), 56.0);
    }
    if task == Tour::Move as u8 {
        add((at.0 + MOVE_REACH * dx, at.1), 40.0);
    }
    if task == Tour::Wheel as u8 {
        add((at.0, at.1 + WHEEL_REACH * dy), 92.0);
    }
    const PAD: f32 = 14.0;
    [r[0] - PAD, r[1] - PAD, r[2] - r[0] + 2.0 * PAD, r[3] - r[1] + 2.0 * PAD]
}

/// El cuerpo de un paso, entrando con un fundido y una subida corta (antes
/// cambiaba de golpe). El id lleva el paso: solo se anima al cambiar de paso.
/// `reduced`: el movimiento reducido (`MotionSettings`, sincronizado con el sistema).
fn step_enter(body: AnyElement, step: Step, reduced: bool) -> AnyElement {
    let el = div().flex_1().min_h_0().flex().flex_col().child(body);
    if reduced {
        return el.into_any_element();
    }
    el.with_animation(
        SharedString::from(format!("onb-step-{step:?}")),
        gpui::Animation::new(std::time::Duration::from_millis(280)).with_easing(gpui::ease_out_quint()),
        |el, t| el.opacity(t).relative().top(px(8.0 * (1.0 - t))),
    )
    .into_any_element()
}

/// Lo que hay que hacer en un paso, en imperativo (el titular del cartel), y
/// qué pasa al hacerlo.
fn task_do(task: u8) -> (&'static str, &'static str) {
    match task {
        k if k == Tour::Strip as u8 => (t("pill.onboarding.tour.stripDo"), t("pill.onboarding.tour.stripWhy")),
        k if k == Tour::Tool as u8 => (t("pill.onboarding.tour.toolDo"), t("pill.onboarding.tour.toolWhy")),
        k if k == Tour::Settings as u8 => (t("pill.onboarding.tour.settingsDo"), t("pill.onboarding.tour.settingsWhy")),
        k if k == Tour::Move as u8 && HAS_NOTCH.load(Ordering::Relaxed) => {
            (t("pill.onboarding.tour.moveDo"), t("pill.onboarding.tour.moveNotchWhy"))
        }
        k if k == Tour::Move as u8 => (t("pill.onboarding.tour.moveDo"), t("pill.onboarding.tour.moveWhy")),
        k if k == Tour::Wheel as u8 => (t("pill.onboarding.tour.wheelDo"), t("pill.onboarding.tour.wheelWhy")),
        _ => (t("pill.onboarding.guides.customizeDo"), t("pill.onboarding.guides.customizeWhy")),
    }
}

/// El ícono de cada paso, en el cartel y en la lista de la bienvenida.
fn task_icon(task: u8) -> &'static str {
    match task {
        t if t == Tour::Strip as u8 => "icons/layout-grid.svg",
        t if t == Tour::Tool as u8 => "icons/sparkles.svg",
        t if t == Tour::Settings as u8 => "icons/settings-2.svg",
        t if t == Tour::Move as u8 => "icons/arrow-curve.svg",
        t if t == Tour::Wheel as u8 => "icons/bubble.svg",
        _ => "icons/sliders.svg",
    }
}

/// Cuánto dura una vuelta del ejemplo de cada paso, en segundos.
fn cycle(task: u8) -> f32 {
    match task {
        t if t == Tour::Customize as u8 => 3.0,
        t if t == Tour::Move as u8 => 3.6,
        t if t == Tour::Wheel as u8 => 3.4,
        _ => 2.8,
    }
}

/// El cartel del tutorial, al lado del hueco del velo (a la derecha si cabe;
/// si no, a la izquierda; si tampoco, abajo o arriba), sin taparlo. Texto
/// grande: qué hacer en imperativo, si ahora miras el ejemplo o te toca a ti,
/// y «Saltar tutorial» a la vista. `watching`: se está viendo el ejemplo.
#[allow(clippy::too_many_arguments)]
fn coach(
    task: u8,
    blocked: bool,
    hole: [f32; 4],
    screen: (f32, f32),
    guide: bool,
    watching: bool,
    reduced: bool,
    ink: &Ink,
) -> AnyElement {
    const W: f32 = 420.0;
    // El alto aproximado del cartel, para que no se salga de la pantalla.
    const H: f32 = 320.0;
    const GAP: f32 = 22.0;
    const EDGE: f32 = 16.0;
    let ink = *ink;
    let done = TOUR.load(Ordering::Relaxed);
    let index = TASKS.iter().position(|(t, ..)| *t as u8 == task).unwrap_or(0);
    let (title, _) = task_text(task);
    let (doing, why) = task_do(task);
    let cheer = CHEER.lock().ok().and_then(|c| *c).filter(|c| c.elapsed() < CHEER_FOR);
    let (head, body): (String, String) = match (cheer, blocked) {
        (Some(_), _) if guide => (t("pill.onboarding.tour.wellDone").into(), t("pill.onboarding.tour.guideDone").into()),
        (Some(_), _) => (t("pill.onboarding.tour.wellDone").into(), tf("pill.onboarding.tour.keepGoing", &[("title", &title)])),
        (None, true) => (doing.into(), t("pill.onboarding.tour.closeToolFirst").into()),
        (None, false) => (doing.into(), why.into()),
    };
    let [hx, hy, hw, hh] = hole;
    let (vw, vh) = screen;
    let right = hx + hw + GAP;
    let fits_right = right + W <= vw - EDGE;
    let fits_left = hx - GAP - W >= EDGE;
    let fits = |side: u8| match side {
        0 => fits_right,
        1 => fits_left,
        _ => true,
    };
    let side = {
        let mut slot = COACH_SIDE.lock().ok();
        let kept = slot.as_ref().and_then(|s| **s).filter(|(t, _)| *t == task).map(|(_, s)| s);
        let side = kept.filter(|s| fits(*s)).unwrap_or(if fits_right {
            0
        } else if fits_left {
            1
        } else {
            2
        });
        if let Some(slot) = slot.as_mut() {
            **slot = Some((task, side));
        }
        side
    };
    let beside_top = hy.clamp(EDGE, (vh - H - EDGE).max(EDGE));
    let (left, top) = match side {
        0 => (right, beside_top),
        1 => (hx - GAP - W, beside_top),
        _ => {
            let below = hy + hh + GAP;
            let top = if below + H <= vh - EDGE { below } else { (hy - GAP - H).max(EDGE) };
            ((hx + hw / 2.0 - W / 2.0).clamp(EDGE, (vw - W - EDGE).max(EDGE)), top)
        }
    };
    // El avance: un tramo por paso; los hechos en verde, el de ahora en acento.
    let progress = div().flex().gap(px(4.)).children(TASKS.iter().enumerate().map(|(i, (t, ..))| {
        let ok = done & *t as u8 != 0;
        div().h(px(6.)).flex_1().rounded_full().bg(if ok {
            ink.ok
        } else if i == index {
            ink.accent
        } else {
            ink.line
        })
    }));
    let header = div()
        .flex()
        .items_center()
        .gap(px(14.))
        .child(
            div()
                .size(px(52.))
                .flex_none()
                .rounded(px(16.))
                .bg(ink.soft)
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path(task_icon(task)).size(px(26.)).text_color(ink.on_soft)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(ink.muted).child(if guide {
                    t("pill.onboarding.tour.howTo").to_string()
                } else {
                    tf("pill.onboarding.tour.stepOf", &[("n", &(index + 1)), ("total", &TASKS.len())])
                }))
                .when(!guide, |el| el.child(progress)),
        );
    // Mientras corre el ejemplo: «Mira el ejemplo y luego hazlo tú»; después,
    // «Ahora tú», lleno, para que se note el cambio.
    let status = (cheer.is_none() && !blocked).then(|| {
        let (bg, fg, icon, text) = if watching {
            (ink.soft, ink.on_soft, "icons/play.svg", t("pill.onboarding.tour.watch"))
        } else {
            (ink.accent, ink.on_accent, "icons/mouse-pointer-2.svg", t("pill.onboarding.tour.yourTurn"))
        };
        div().flex().child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(34.))
                .px(px(14.))
                .rounded_full()
                .bg(bg)
                .text_color(fg)
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(svg().path(icon).size(px(16.)).text_color(fg))
                .child(text),
        )
    });
    let text = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(24.))
                .line_height(px(30.))
                .font_weight(FontWeight::BOLD)
                .text_color(ink.text)
                .when(cheer.is_some(), |el| el.child(svg().path("icons/check.svg").size(px(24.)).text_color(ink.ok)))
                .child(head),
        )
        .child(
            div()
                .text_size(px(17.))
                .line_height(px(25.))
                .text_color(if blocked && cheer.is_none() { ink.warn } else { ink.muted })
                .child(body),
        );
    // Al cambiar de tarea (o al felicitar) el texto se funde: antes se
    // reemplazaba de golpe.
    let text = if reduced {
        text.into_any_element()
    } else {
        text.with_animation(
            SharedString::from(format!("onb-coach-text-{task}-{}-{blocked}", cheer.is_some())),
            gpui::Animation::new(std::time::Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
            |el, t| el.opacity(0.2 + 0.8 * t),
        )
        .into_any_element()
    };
    let soft = ink.soft;
    let skip = div()
        .id("onb-coach-skip")
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(40.))
        .px(px(16.))
        .rounded_full()
        .border_1()
        .border_color(ink.line)
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ink.text)
        .cursor_pointer()
        .hover(move |style| style.bg(soft))
        // Al apretar, sin esperar a soltar: la pill vuelve a dejar pasar los
        // clics apenas se va el cartel.
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
            skip_from_coach();
        })
        .child(svg().path("icons/x.svg").size(px(16.)).text_color(ink.text))
        .child(if guide { t("pill.onboarding.tour.closeGuide") } else { t("pill.onboarding.tour.skipTutorial") });
    let footer = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .pt(px(4.))
        .child(div().flex_1().min_w_0().text_size(px(14.)).line_height(px(19.)).text_color(ink.muted).child(if guide {
            ""
        } else {
            t("pill.onboarding.tour.autoCheck")
        }))
        .child(skip);
    let card = div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(W))
        .p(px(22.))
        .rounded(px(28.))
        .bg(ink.card)
        .border_1()
        .border_color(ink.line.opacity(0.6))
        .shadow_lg()
        .font_family(crate::theme::font())
        .flex()
        .flex_col()
        .gap(px(14.))
        .text_color(ink.text)
        // Los clics en el cartel no siguen hasta la pill (que los tomaría como
        // un clic fuera de ella).
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            canvas(
                |bounds, _, _| {
                    if let Ok(mut at) = COACH_AT.lock() {
                        *at = Some((
                            f32::from(bounds.origin.x),
                            f32::from(bounds.origin.y),
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height),
                        ));
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(header)
        .children(status)
        .child(text)
        .child(footer);
    if reduced {
        return card.into_any_element();
    }
    // Aparece fundiéndose (antes saltaba a la vista).
    card.with_animation(
        "onb-coach",
        gpui::Animation::new(std::time::Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
        |el, t| el.opacity(t),
    )
    .into_any_element()
}

/// Dónde va el cursor de ejemplo en un momento de su vuelta (`p`, de 0 a 1).
#[derive(Clone, Copy)]
struct Pose {
    at: (f32, f32),
    pressed: bool,
    alpha: f32,
}

fn pose(task: u8, p: f32, at: (f32, f32), (dx, dy): (f32, f32)) -> Pose {
    use crate::anim::{ease_smooth_out, segment};
    let lerp = |a: (f32, f32), b: (f32, f32), k: f32| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
    if task == Tour::Customize as u8 {
        let goal = (at.0 + 64.0, at.1 + 4.0);
        let from = (goal.0 + 150.0 * dx, goal.1 + 170.0 * dy);
        let hold = segment(p, 0.4, 0.3);
        return Pose {
            at: lerp(from, goal, ease_smooth_out(segment(p, 0.05, 0.3))),
            pressed: hold > 0.0 && p < 0.75,
            alpha: segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.88, 0.12)),
        };
    }
    if task == Tour::Move as u8 {
        // Toma la pill y la lleva por el borde, y de vuelta.
        let go = ease_smooth_out(segment(p, 0.15, 0.3)) - ease_smooth_out(segment(p, 0.55, 0.3));
        return Pose {
            at: (at.0 + MOVE_REACH * dx * go + 6.0, at.1 + 6.0),
            pressed: (0.1..0.9).contains(&p),
            alpha: segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.9, 0.1)),
        };
    }
    if task == Tour::Wheel as u8 {
        // Toma la pill y la arrastra fuera del borde.
        let drag = ease_smooth_out(segment(p, 0.2, 0.36));
        let (x, y) = lerp(at, (at.0, at.1 + WHEEL_REACH * dy), drag);
        return Pose {
            at: (x + 6.0, y + 6.0),
            pressed: (0.12..0.62).contains(&p),
            alpha: segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.9, 0.1)),
        };
    }
    // Pasar el cursor por la pill, tocar una herramienta de la tira, o tocar
    // la cara (los Ajustes).
    let tool = task == Tour::Tool as u8;
    let clicks = tool || task == Tour::Settings as u8;
    let goal = if tool { (at.0 + 64.0, at.1 + 4.0) } else { (at.0 + 4.0, at.1 + 4.0) };
    let from = (goal.0 + 150.0 * dx, goal.1 + 170.0 * dy);
    Pose {
        at: lerp(from, goal, ease_smooth_out(segment(p, 0.06, 0.4))),
        pressed: clicks && (0.5..0.58).contains(&p),
        alpha: segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.88, 0.12)),
    }
}

/// El cursor de ejemplo: una flecha grande del color de acento con borde
/// blanco y la etiqueta «Ejemplo» pegada, para que no se confunda con el
/// cursor de verdad.
fn ghost_pointer(pose: Pose, ink: &Ink) -> AnyElement {
    const SCALE: f32 = 2.3;
    let k = if pose.pressed { 0.88 } else { 1.0 };
    // `cursor-fill.svg` mide 14 × 21 y su punta está en (1, 1).
    let (w, h) = (14.0 * SCALE * k, 21.0 * SCALE * k);
    let tip = SCALE * k;
    let a = pose.alpha;
    div()
        .absolute()
        .left(px(pose.at.0 - tip))
        .top(px(pose.at.1 - tip))
        .flex()
        .items_start()
        .child(
            div()
                .relative()
                .flex_none()
                .w(px(w))
                .h(px(h))
                .child(svg().path("icons/cursor-fill.svg").absolute().left(px(2.)).top(px(3.)).size_full().text_color(gpui::black().opacity(0.35 * a)))
                .child(svg().path("icons/cursor-fill.svg").absolute().top_0().left_0().size_full().text_color(ink.accent.opacity(a)))
                .child(svg().path("icons/cursor-edge.svg").absolute().top_0().left_0().size_full().text_color(gpui::white().opacity(a))),
        )
        .child(
            div()
                .flex_none()
                .mt(px(h - 8.0))
                .ml(px(-6.0))
                .px(px(10.))
                .py(px(3.))
                .rounded_full()
                .bg(ink.accent.opacity(a))
                .border_2()
                .border_color(gpui::white().opacity(a))
                .text_size(px(15.))
                .font_weight(FontWeight::BOLD)
                .text_color(ink.on_accent.opacity(a))
                .font_family(crate::theme::font())
                .child(t("pill.onboarding.tour.example")),
        )
        .into_any_element()
}

/// Un anillo que se abre: el clic, o la pill que reacciona.
fn ripple(x: f32, y: f32, t: f32, max: f32) -> AnyElement {
    let r = 8.0 + max * t;
    div()
        .absolute()
        .left(px(x - r))
        .top(px(y - r))
        .size(px(r * 2.0))
        .rounded_full()
        .border_2()
        .border_color(gpui::white().opacity(0.8 * (1.0 - t)))
        .into_any_element()
}

/// El ejemplo de un paso del tour, sobre la pill (`at`, en su ventana), en el
/// momento `p` (de 0 a 1) de su vuelta: lo que se mueve, el rastro del cursor
/// de ejemplo y el cursor.
fn demo(task: u8, p: f32, at: (f32, f32), dir: (f32, f32), reduced: bool, ink: &Ink) -> Vec<AnyElement> {
    use crate::anim::{ease_smooth_out, segment};
    let mut out = Vec::new();
    let now = pose(task, p, at, dir);
    // La pill que el ejemplo arrastra, con borde para leerse sobre el velo.
    let carried = |x: f32, y: f32, w: f32, h: f32, alpha: f32| {
        div()
            .absolute()
            .left(px(x - w / 2.0))
            .top(px(y - h / 2.0))
            .w(px(w))
            .h(px(h))
            .rounded(px(h / 2.0))
            .bg(gpui::rgb(0x1a1a18))
            .border_1()
            .border_color(gpui::white().opacity(0.35))
            .opacity(alpha)
            .flex()
            .items_center()
            .justify_center()
            .child(face(h * 0.66, crate::face::Pose::REST, false, None))
            .into_any_element()
    };
    if task == Tour::Customize as u8 {
        let hold = segment(p, 0.4, 0.3);
        if hold > 0.0 {
            // El anillo se llena mientras se mantiene apretada.
            let goal = (at.0 + 64.0, at.1 + 4.0);
            let r = 22.0;
            out.push(
                div()
                    .absolute()
                    .left(px(goal.0 - r))
                    .top(px(goal.1 - r))
                    .size(px(r * 2.0))
                    .rounded_full()
                    .border_2()
                    .border_color(gpui::white().opacity(0.25 + 0.6 * hold))
                    .bg(gpui::white().opacity(0.12 * hold))
                    .into_any_element(),
            );
        }
    } else if task == Tour::Move as u8 {
        let (x, y) = (now.at.0 - 6.0, now.at.1 - 6.0);
        if (x - at.0).abs() > 2.0 {
            out.push(carried(x, y, 80.0, 32.0, now.alpha * 0.9));
        }
    } else if task == Tour::Wheel as u8 {
        let (x, y) = (now.at.0 - 6.0, now.at.1 - 6.0);
        if (y - at.1).abs() > 0.5 {
            out.push(carried(x, y, 46.0, 46.0, now.alpha * 0.95));
        }
        // Y al soltarla aparece la rueda.
        let ring = ease_smooth_out(segment(p, 0.6, 0.18));
        if ring > 0.0 {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0 - std::f32::consts::FRAC_PI_2;
                let r = 30.0 + 50.0 * ring;
                let s = 22.0 * ring;
                out.push(
                    div()
                        .absolute()
                        .left(px(x + r * a.cos() - s / 2.0))
                        .top(px(y + r * a.sin() - s / 2.0))
                        .size(px(s))
                        .rounded_full()
                        .bg(gpui::white().opacity(0.85 * now.alpha))
                        .into_any_element(),
                );
            }
        }
    } else {
        let tool = task == Tour::Tool as u8;
        let goal = if tool { (at.0 + 64.0, at.1 + 4.0) } else { (at.0 + 4.0, at.1 + 4.0) };
        let pulse = segment(p, 0.5, 0.3);
        if pulse > 0.0 && pulse < 1.0 {
            out.push(ripple(goal.0, goal.1, pulse, if tool { 22.0 } else { 34.0 }));
        }
    }
    // El rastro: dónde estuvo el cursor de ejemplo hace un instante. Deja claro
    // que se mueve solo (no es el tuyo). Sin él con movimiento reducido.
    if !reduced {
        for k in 1..=5 {
            let q = p - k as f32 * 0.022;
            if q < 0.0 {
                break;
            }
            let past = pose(task, q, at, dir);
            if (past.at.0 - now.at.0).hypot(past.at.1 - now.at.1) < 4.0 {
                continue;
            }
            let r = 7.0 - k as f32;
            out.push(
                div()
                    .absolute()
                    .left(px(past.at.0 - r))
                    .top(px(past.at.1 - r))
                    .size(px(2.0 * r))
                    .rounded_full()
                    .bg(ink.accent.opacity(0.45 * (1.0 - k as f32 / 6.0) * past.alpha))
                    .into_any_element(),
            );
        }
    }
    out.push(ghost_pointer(now, ink));
    out
}

// --- La ventana ----------------------------------------------------------------------

/// Al arrancar: la primera vez (sin `onboarding_done`). `PILL_OPEN=onboarding`
/// la abre igual, para revisarla. Devuelve si se abrió.
pub fn open_if_first_run(cx: &mut App) -> bool {
    let forced = std::env::var("PILL_OPEN").as_deref() == Ok("onboarding");
    let first = AppDirs::new().is_ok_and(|dirs| !Config::load(&dirs.config_path()).onboarding_done);
    if forced || first {
        open(cx);
        return true;
    }
    false
}

pub fn open(cx: &mut App) {
    open_at(None, cx);
}

/// Solo el paso de Permisos: cuando falta uno al abrir la pill, o desde
/// Ajustes → Permisos. Al terminar se cierra.
#[cfg(target_os = "macos")]
pub fn open_permissions(cx: &mut App) {
    open_at(Some(Step::Permisos), cx);
}

/// Solo el tour de la pill (Ajustes → General → Repetir el tour).
pub fn open_tour(cx: &mut App) {
    open_at(Some(Step::Tour), cx);
}

fn open_at(only: Option<Step>, cx: &mut App) {
    if let Some(handle) = cx.windows().into_iter().find_map(|w| w.downcast::<Onboarding>()) {
        if handle.update(cx, |_, window, _| crate::win::bring_to_front(window)).is_ok() {
            return;
        }
    }
    let Ok(dirs) = AppDirs::new() else {
        return;
    };
    let path = dirs.config_path();
    let options = WindowOptions {
        titlebar: Some(chrome::titlebar(
            match only {
                Some(Step::Permisos) => t("pill.onboarding.windowPermissions"),
                Some(_) => "Atic · Tour",
                None => t("pill.onboarding.title"),
            },
            TOP_H,
        )),
        window_min_size: Some(size(px(360.), px(480.))),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_W), px(WINDOW_H)), cx))),
        focus: crate::win::FOCUS_ON_OPEN,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| Onboarding::new(path, only, window, cx))
    });
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => tracing::warn!(%error, "bienvenida: no se pudo abrir la ventana"),
    }
}

/// El motor de voz e IA que se elige.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Engine {
    Groq,
    Local,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Apple,
}

impl Engine {
    fn backend(self) -> &'static str {
        match self {
            Engine::Groq => "groq",
            Engine::Local => "parakeet",
            Engine::Apple => "apple",
        }
    }

    fn all() -> &'static [Engine] {
        if !crate::local_stt::SUPPORTED {
            &[Engine::Groq, Engine::Apple]
        } else if cfg!(target_os = "macos") {
            &[Engine::Groq, Engine::Local, Engine::Apple]
        } else {
            &[Engine::Groq, Engine::Local]
        }
    }
}

struct EngineInfo {
    title: &'static str,
    chip: &'static str,
    icon: &'static str,
    /// Dictar, transcribir reuniones, resumir.
    can: [bool; 3],
    pros: [&'static str; 2],
    cons: [&'static str; 2],
}

fn info(engine: Engine) -> EngineInfo {
    match engine {
        Engine::Groq => EngineInfo {
            title: "Groq",
            chip: t("pill.onboarding.engine.recommended"),
            icon: "icons/cloud.svg",
            can: [true, true, true],
            pros: [t("pill.onboarding.engine.groqPro1"), t("pill.onboarding.engine.groqPro2")],
            cons: [t("pill.onboarding.engine.groqCon1"), t("pill.onboarding.engine.groqCon2")],
        },
        Engine::Local => EngineInfo {
            title: "Local",
            chip: t("pill.onboarding.engine.private"),
            icon: "icons/cpu.svg",
            can: [true, true, false],
            pros: [t("pill.onboarding.engine.localPro1"), t("pill.onboarding.engine.localPro2")],
            cons: [t("pill.onboarding.engine.noSummary"), t("pill.onboarding.engine.localCon2")],
        },
        Engine::Apple => EngineInfo {
            title: t("pill.onboarding.engine.appleTitle"),
            chip: t("pill.onboarding.engine.included"),
            icon: "icons/mic.svg",
            can: [true, true, false],
            pros: [t("pill.onboarding.engine.applePro1"), t("pill.onboarding.engine.applePro2")],
            cons: [t("pill.onboarding.engine.noSummary"), t("pill.onboarding.engine.appleCon2")],
        },
    }
}

/// Lo que puede hacer un motor: dictar, transcribir reuniones, resumir.
fn can() -> [&'static str; 3] {
    [t("pill.onboarding.engine.canDictate"), t("pill.onboarding.engine.canTranscribe"), t("pill.onboarding.engine.canSummarize")]
}

type GetKey = fn(&Config) -> &String;
type SetKey = fn(&mut Config, String);

/// Los atajos que se muestran (y se cambian) al terminar, con la clave de su
/// nombre (se traduce al dibujar).
const KEYS: [(&str, GetKey, SetKey); 6] = [
    ("pill.onboarding.keys.dictate", |c| &c.dictation_shortcut, |c, v| c.dictation_shortcut = v),
    ("pill.onboarding.clipboard", |c| &c.clipboard_shortcut, |c, v| c.clipboard_shortcut = v),
    ("pill.onboarding.keys.snippets", |c| &c.snippets_shortcut, |c, v| c.snippets_shortcut = v),
    ("pill.onboarding.keys.screenshot", |c| &c.screenshot_shortcut, |c, v| c.screenshot_shortcut = v),
    ("pill.onboarding.keys.board", |c| &c.board_shortcut, |c, v| c.board_shortcut = v),
    ("pill.onboarding.keys.launcher", |c| &c.launcher_shortcut, |c, v| c.launcher_shortcut = v),
];

/// El nombre en español de cada herramienta del catálogo (`pill_tools`).
fn tool_name(tool: usize) -> &'static str {
    match pill_tools::name(tool) {
        "Clipboard" => t("pill.onboarding.clipboard"),
        other => other,
    }
}

pub struct Onboarding {
    path: PathBuf,
    cfg: Config,
    step: Step,
    /// Abierta para un solo paso (los permisos): sin el resto ni los puntos.
    only: Option<Step>,
    notice: Option<(SecretKind, String, bool)>,
    backdrop: Option<bool>,
    engine: Option<Engine>,
    /// Groq solo para resumir, con otro motor para la voz.
    groq_summary: bool,
    /// El atajo que se está grabando (índice de `KEYS`).
    recording: Option<usize>,
    focus: gpui::FocusHandle,
    /// Las herramientas elegidas, en el orden en que se tocaron.
    tools: Vec<usize>,
    /// La pestaña de abajo en Herramientas: «En «Más»» en vez de «En la pill».
    tab_more: bool,
    /// En el paso del tour: ya se tocó «Empezar el tutorial» (antes se explica
    /// cómo funciona, y el ejemplo no aparece sin aviso).
    tour_started: bool,
    /// Dónde quedó la rueda de herramientas en la ventana: de ahí despegan.
    ring_at: Rc<Cell<Bounds<Pixels>>>,
    /// Dónde está el mouse en la ventana (fuera de Mac, solo dentro de ella).
    cursor: Option<gpui::Point<Pixels>>,
    /// Mark, que mira al mouse con todo el cuerpo, y dónde quedó dibujada.
    face: crate::face::Face,
    face_at: FaceAt,
    /// El último cursor visto en la pantalla, para despertar solo si se movió.
    seen: Option<(f32, f32)>,
    /// Lo de afuera que se vio en el último aviso del tick (`outside_key`) y cuándo.
    tick_seen: (String, Instant),
    /// Dónde quedó el ícono para arrastrar: de ahí sale el fantasma.
    icon_at: Rc<Cell<Bounds<Pixels>>>,
    #[cfg(target_os = "macos")]
    asking: Option<Permission>,
    /// Faltaba Grabación de pantalla al abrir: concedida, hay que reabrir Atic.
    #[cfg(target_os = "macos")]
    screen_missing: bool,
    /// El marco de antes de ponerse al lado de Ajustes, para volver.
    #[cfg(target_os = "macos")]
    home: Option<(f64, f64, f64, f64)>,
    /// El marco de Ajustes del Sistema junto al que se puso la ventana.
    #[cfg(target_os = "macos")]
    beside: Option<(f64, f64, f64, f64)>,
    /// Cómo resultó «Quitar» un permiso, y si falló.
    #[cfg(target_os = "macos")]
    reset_note: Option<(String, bool)>,
}

impl Drop for Onboarding {
    fn drop(&mut self) {
        // Cerrada a medio grabar, los atajos de Atic no quedan en pausa.
        if self.recording.is_some() {
            crate::platform::hotkeys::set_paused(false);
        }
        set_demo(None);
        set_ghost(None);
    }
}

impl ConfigPane for Onboarding {
    fn edit_config(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut Config)) {
        self.cfg = save_config(&self.path, apply);
        cx.notify();
    }

    fn key_notice(&self) -> Option<&(SecretKind, String, bool)> {
        self.notice.as_ref()
    }

    fn set_key_notice(&mut self, notice: (SecretKind, String, bool), cx: &mut Context<Self>) {
        self.notice = Some(notice);
        cx.notify();
    }
}

impl Onboarding {
    fn new(path: PathBuf, only: Option<Step>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.spawn_in(window, async move |view, cx| loop {
            cx.background_executor().timer(TICK).await;
            if view.update_in(cx, |view, window, cx| view.tick(window, cx)).is_err() {
                break;
            }
        })
        .detach();
        cx.spawn_in(window, async move |view, cx| loop {
            cx.background_executor().timer(Duration::from_millis(60)).await;
            let alive = view.update_in(cx, |view, window, cx| {
                let now = Instant::now();
                let cursor = view.cursor_in(window);
                if cursor != view.seen || now >= view.face.next_blink() {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        let cfg = Config::load(&path);
        TOUR.store(0, Ordering::Relaxed);
        SKIP.store(false, Ordering::Relaxed);
        Self {
            path,
            cfg,
            step: {
                if only == Some(Step::Tour) {
                    TOUR.store(0, Ordering::Relaxed);
                }
                only.unwrap_or(Step::Hola)
            },
            only,
            notice: None,
            backdrop: None,
            engine: Some(Engine::Groq),
            groq_summary: false,
            recording: None,
            focus: cx.focus_handle(),
            // Repitiendo la bienvenida, parte con la tira que ya tienes.
            tools: if pill_tools::configured() { pill_tools::load().ring } else { Vec::new() },
            tab_more: false,
            tour_started: false,
            ring_at: Rc::default(),
            cursor: None,
            face: crate::face::Face::new(),
            face_at: Rc::default(),
            seen: None,
            tick_seen: (String::new(), Instant::now()),
            icon_at: Rc::default(),
            #[cfg(target_os = "macos")]
            asking: None,
            #[cfg(target_os = "macos")]
            screen_missing: permissions::state(Permission::ScreenRecording) != State::Granted,
            #[cfg(target_os = "macos")]
            home: None,
            #[cfg(target_os = "macos")]
            beside: None,
            #[cfg(target_os = "macos")]
            reset_note: None,
        }
    }

    /// Graba un atajo nuevo: los de Atic quedan en pausa mientras tanto, si no
    /// la combinación abriría su herramienta en vez de grabarse.
    fn record(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording == Some(index) {
            return self.stop_recording(cx);
        }
        self.recording = Some(index);
        crate::platform::hotkeys::set_paused(true);
        window.focus(&self.focus);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            crate::platform::hotkeys::set_paused(false);
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.recording else {
            // En el tour, Esc lo salta.
            if event.keystroke.key == "escape" && self.step == Step::Tour {
                cx.stop_propagation();
                self.skip_tour(window, cx);
            }
            return;
        };
        let stroke = &event.keystroke;
        if stroke.key == "escape" {
            return self.stop_recording(cx);
        }
        let mods = &stroke.modifiers;
        let Some(text) =
            crate::shortcuts_settings::chord_text(mods.control, mods.alt, mods.shift, mods.platform, &stroke.key)
        else {
            return;
        };
        let set = KEYS[index].2;
        self.edit_config(cx, |cfg| set(cfg, text));
        cx.stop_propagation();
        self.stop_recording(cx);
    }

    /// El mouse en coordenadas de la ventana: en Mac, aunque esté afuera.
    fn cursor_in(&self, window: &Window) -> Option<(f32, f32)> {
        #[cfg(target_os = "macos")]
        {
            let (fx, fy, _, _) = crate::win::frame(window)?;
            let (x, y) = crate::win::cursor_global();
            Some(((x - fx) as f32, (y - fy) as f32))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            self.cursor.map(|p| (f32::from(p.x), f32::from(p.y)))
        }
    }

    fn go(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        self.leave_side(window);
        self.step = step;
        if step == Step::Tour {
            TOUR.store(0, Ordering::Relaxed);
            self.tour_started = false;
            // Para que Esc llegue y salte el tour.
            window.focus(&self.focus);
        }
        cx.notify();
    }

    /// «Empezar el tutorial»: desde el próximo tick el ejemplo aparece sobre la pill.
    fn start_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        TOUR.store(0, Ordering::Relaxed);
        self.tour_started = true;
        window.focus(&self.focus);
        cx.notify();
    }

    /// Salta el tour (el botón del cartel, el de la bienvenida o Esc): no se
    /// vuelve a abrir la bienvenida, se apaga el ejemplo y se pasa a Listo (o
    /// se cierra, si se abrió solo para el tour).
    fn skip_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_config(cx, |cfg| cfg.onboarding_done = true);
        self.tour_started = false;
        set_demo(None);
        if self.only.is_some() {
            set_ghost(None);
            window.remove_window();
            return;
        }
        self.go(Step::Listo, window, cx);
        crate::win::bring_to_front(window);
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.only.is_some() {
            #[cfg(target_os = "macos")]
            {
                self.leave_side(window);
                crate::permission_setup::remember_missing();
            }
            set_ghost(None);
            window.remove_window();
            let _ = cx;
            return;
        }
        let all = steps();
        if let Some(i) = all.iter().position(|s| *s == self.step) {
            if let Some(next) = all.get(i + 1) {
                self.go(*next, window, cx);
            }
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let all = steps();
        if let Some(i) = all.iter().position(|s| *s == self.step).filter(|i| *i > 0) {
            self.go(all[i - 1], window, cx);
        }
    }

    /// Cada cuarto de segundo: permisos, la descarga, el tour y Ajustes.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Se tocó «Saltar tutorial» en el cartel de la pill.
        if SKIP.swap(false, Ordering::Relaxed) {
            if self.step == Step::Tour {
                self.skip_tour(window, cx);
            }
            return;
        }
        // En el tour, el ejemplo del primer paso que falta.
        let done = TOUR.load(Ordering::Relaxed);
        let next = TASKS.iter().map(|(t, ..)| *t as u8).find(|t| done & t == 0);
        if self.step == Step::Tour && self.tour_started {
            set_demo(next);
        } else if GUIDE.lock().is_ok_and(|g| g.is_none()) {
            set_demo(None);
        }
        #[cfg(target_os = "macos")]
        self.follow_settings(window);
        let _ = window;
        // Cada aviso rehace la ventana entera: solo si cambió algo de afuera que se
        // muestra, y por si algo no está en `outside_key`, una vez por segundo.
        let key = self.outside_key();
        if key != self.tick_seen.0 || self.tick_seen.1.elapsed() >= Duration::from_secs(1) {
            self.tick_seen = (key, Instant::now());
            cx.notify();
        }
    }

    /// Lo que la vista muestra y cambia por fuera de ella: el tour, la guía, la
    /// felicitación, los permisos y la descarga del modelo local.
    fn outside_key(&self) -> String {
        let guide = GUIDE.lock().ok().and_then(|g| *g).map(|(task, _)| task);
        let cheer = CHEER.lock().ok().and_then(|c| *c).is_some_and(|c| c.elapsed() < CHEER_FOR);
        #[cfg(target_os = "macos")]
        let granted: Vec<State> = Permission::ALL.iter().map(|p| permissions::state(*p)).collect();
        #[cfg(not(target_os = "macos"))]
        let granted: Vec<()> = Vec::new();
        format!("{:?}", (TOUR.load(Ordering::Relaxed), guide, cheer, granted, crate::local_stt::state()))
    }

    /// Termina: no se vuelve a abrir sola, ni la pantalla de permisos por lo
    /// que ya faltaba aquí.
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_config(cx, |cfg| cfg.onboarding_done = true);
        #[cfg(target_os = "macos")]
        crate::permission_setup::remember_missing();
        set_ghost(None);
        set_demo(None);
        window.remove_window();
    }

    // --- Voz e IA ---

    fn pick_engine(&mut self, engine: Engine, cx: &mut Context<Self>) {
        self.engine = Some(engine);
        let backend = engine.backend();
        self.edit_config(cx, |cfg| {
            cfg.dictation_backend = backend.into();
            cfg.meeting_backend = backend.into();
        });
        if engine == Engine::Groq {
            self.use_groq_for_summaries(cx);
        }
    }

    fn use_groq_for_summaries(&mut self, cx: &mut Context<Self>) {
        self.edit_config(cx, |cfg| {
            cfg.summary_backend = "groq".into();
            cfg.summary_model = "openai/gpt-oss-120b".into();
        });
    }

    fn groq_ready(&self, cx: &mut Context<Self>) -> bool {
        matches!(shown_key(SecretKind::GroqApiKey, cx), Some(Some(_)))
    }

    fn engine_ready(&self, cx: &mut Context<Self>) -> bool {
        match self.engine {
            Some(Engine::Groq) => self.groq_ready(cx),
            Some(Engine::Local) => crate::local_stt::ready(),
            #[cfg(target_os = "macos")]
            Some(Engine::Apple) => permissions::state(Permission::Microphone) == State::Granted,
            #[cfg(not(target_os = "macos"))]
            Some(Engine::Apple) => false,
            None => false,
        }
    }

    // --- Herramientas ---

    fn toggle_tool(&mut self, tool: usize, at: (f32, f32), window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.tools.iter().position(|t| *t == tool) {
            self.tools.remove(i);
        } else {
            self.tools.push(tool);
            if let Some(from) = to_screen(window, at) {
                if let Ok(mut flights) = FLIGHTS.lock() {
                    flights.push(Flight { icon: pill_tools::icon(tool), from, since: Instant::now() });
                }
                LANDING.store(true, Ordering::Relaxed);
            }
        }
        if !self.tools.is_empty() {
            let (ring, more) = split(&self.tools);
            if let Ok(mut slot) = LAYOUT.lock() {
                *slot = Some(pill_tools::layout(&ring, &more));
            }
        }
        cx.notify();
    }
}

/// La tira con todas las elegidas, en el orden en que se tocaron; «Más»
/// con el resto, en el del catálogo.
fn split(chosen: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let more = (0..pill_tools::COUNT).filter(|t| !chosen.contains(t)).collect();
    (chosen.to_vec(), more)
}

/// Un punto de la ventana en puntos globales (origen arriba a la izquierda).
fn to_screen(window: &Window, (x, y): (f32, f32)) -> Option<(f32, f32)> {
    #[cfg(target_os = "macos")]
    {
        let (fx, fy, _, _) = crate::win::frame(window)?;
        Some((fx as f32 + x, fy as f32 + y))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let origin = window.bounds().origin;
        Some((f32::from(origin.x) + x, f32::from(origin.y) + y))
    }
}

// --- Permisos (Mac) --------------------------------------------------------------------

#[cfg(target_os = "macos")]
impl Onboarding {
    /// Los que se activan en una lista de Ajustes (o uno ya negado): la
    /// ventana se pone al lado y ofrece el ícono para arrastrar.
    fn needs_list(permission: Permission) -> bool {
        matches!(permission, Permission::Accessibility | Permission::InputMonitoring | Permission::ScreenRecording)
            || permissions::state(permission) == State::Denied
    }

    fn ask(&mut self, permission: Permission, window: &mut Window, cx: &mut Context<Self>) {
        permissions::request(permission);
        if Self::needs_list(permission) {
            permissions::open_settings(permission);
            self.asking = Some(permission);
            self.home = self.home.or_else(|| crate::win::frame(window));
            self.beside = None;
        }
        cx.notify();
    }

    /// Ajustes del Sistema en pantalla: la ventana se pone a su lado (una vez
    /// por posición de Ajustes) y el fantasma va del ícono a su lista.
    fn follow_settings(&mut self, window: &mut Window) {
        let Some(permission) = self.asking else {
            return;
        };
        if permissions::state(permission) == State::Granted {
            self.leave_side(window);
            return;
        }
        let Some(settings) = settings_frame() else {
            set_ghost(None);
            return;
        };
        if self.beside != Some(settings) {
            self.beside = Some(settings);
            let (sx, sy, sw, _) = settings;
            let right = sx + sw + 16.0;
            let x = if right + SIDE_W <= crate::win::primary_width() { right } else { (sx - SIDE_W - 16.0).max(0.0) };
            crate::win::set_frame(window, (x, sy.max(40.0), SIDE_W, SIDE_H));
        }
        let icon = self.icon_at.get();
        let (Some((fx, fy, _, _)), true) = (crate::win::frame(window), f32::from(icon.size.width) > 0.0) else {
            return;
        };
        let center = icon.center();
        let from = (fx as f32 + f32::from(center.x), fy as f32 + f32::from(center.y));
        let (sx, sy, sw, sh) = settings;
        // El panel de Pantalla tiene dos listas: arriba «audio del sistema y
        // pantalla», abajo «Sólo grabación de audio del sistema». Cada
        // permiso apunta a la suya; los demás, a su única lista.
        let down = if permission == Permission::SystemAudio { sh * 0.72 } else { (sh * 0.33).min(260.0) };
        let to = ((sx + sw * 0.62) as f32, (sy + down) as f32);
        let since = GHOST.lock().ok().and_then(|g| g.map(|g| g.since)).unwrap_or_else(Instant::now);
        set_ghost(Some(Ghost { from, to, since }));
    }

    /// Vuelve a su tamaño y lugar, y deja de mostrar el arrastre.
    fn leave_side(&mut self, window: &mut Window) {
        self.asking = None;
        self.beside = None;
        set_ghost(None);
        if let Some(home) = self.home.take() {
            crate::win::set_frame(window, home);
        }
    }

    /// Quita el permiso (`tccutil reset`) y avisa cómo resultó.
    fn reset(&mut self, permission: Permission, cx: &mut Context<Self>) {
        self.reset_note = Some(match permissions::reset(permission) {
            Ok(()) => (tf("pill.onboarding.perm.removed", &[("name", &permission.title())]), false),
            Err(error) => (tf("pill.onboarding.perm.removeFailed", &[("name", &permission.title()), ("error", &error)]), true),
        });
        cx.notify();
    }

    fn drag_app(&mut self, cx: &mut Context<Self>) {
        let Some(app) = crate::platform::bundle_path() else {
            return;
        };
        let path = app.to_string_lossy().into_owned();
        set_ghost(None);
        cx.spawn(async move |_, _| {
            if let Err(error) = crate::drag::drag_files(&[path]) {
                tracing::info!(%error, "bienvenida: arrastre de Atic");
            }
        })
        .detach();
    }
}

/// El marco de la ventana de Ajustes del Sistema, si está en pantalla.
#[cfg(target_os = "macos")]
fn settings_frame() -> Option<(f64, f64, f64, f64)> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSArray, NSString};
    // SAFETY: consultas de AppKit sin efectos; el arreglo vive en el bloque.
    let pids: Vec<i32> = unsafe {
        let id = NSString::from_str("com.apple.systempreferences");
        let apps: Retained<NSArray<AnyObject>> =
            msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier: &*id];
        apps.iter().map(|app| msg_send![&*app, processIdentifier]).collect()
    };
    crate::platform::window_list::on_screen()
        .into_iter()
        .find(|w| w.layer == 0 && pids.contains(&w.pid) && w.width > 300.0)
        .map(|w| (w.x, w.y, w.width, w.height))
}

// --- Dibujo ------------------------------------------------------------------------------

/// La cara de la pill, viva (`face.rs`): con `pose`, mira con todo el
/// cuerpo; con `tile`, va sobre su cuadro oscuro, como el ícono de la app.
/// `at` anota dónde quedó (centro y tamaño, en la ventana) para apuntarla.
fn face(size: f32, pose: crate::face::Pose, tile: bool, at: Option<FaceAt>) -> impl IntoElement {
    let ink: Hsla = gpui::rgb(0xf0f0ea).into();
    let side = if tile { size } else { size * 0.9 };
    div()
        .size(px(side))
        .flex_none()
        .when(tile, |el| el.rounded(px(size * 0.24)).bg(gpui::rgb(0x1a1a18)).border_1().border_color(gpui::white().opacity(0.08)))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let center = bounds.center();
                    let (cx, cy) = (f32::from(center.x), f32::from(center.y));
                    let glyph = if tile { size * 0.82 } else { size };
                    if let Some(at) = &at {
                        at.set(Some((cx, cy, glyph)));
                    }
                    crate::face::paint(window, (cx, cy), glyph, pose, ink);
                },
            )
            .size_full(),
        )
}

type FaceAt = Rc<Cell<Option<(f32, f32, f32)>>>;

fn title(text: &'static str) -> impl IntoElement {
    div().text_size(px(30.)).line_height(px(36.)).font_weight(FontWeight::BOLD).child(text)
}

fn lead(text: impl Into<SharedString>) -> impl IntoElement {
    div().text_size(px(16.)).line_height(px(23.)).text_color(hsla(MUTED)).child(text.into())
}

/// Lo que gana el usuario con el paso, en una frase y destacado.
fn gain(text: &'static str) -> impl IntoElement {
    let (bg, fg) = crate::theme::expressive().map_or((hsla(TEXT).opacity(0.08), hsla(TEXT)), |t| (t.accent_soft, t.on_accent_soft));
    div().flex().child(
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .py(px(9.))
            .rounded(px(14.))
            .bg(bg)
            .text_color(fg)
            .text_size(px(15.))
            .line_height(px(21.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(svg().path("icons/sparkles.svg").size(px(18.)).flex_none().text_color(fg))
            .child(text),
    )
}

fn check(color: Hsla) -> impl IntoElement {
    svg().path("icons/check.svg").size(px(14.)).text_color(color)
}

/// Atrás, una nota y el botón para seguir.
fn footer(
    back: Option<gpui::AnyElement>,
    note: impl Into<SharedString>,
    action: gpui::AnyElement,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .children(back)
        .child(div().flex_1().text_size(px(13.)).text_color(hsla(MUTED)).text_right().child(note.into()))
        .child(action)
}

impl Onboarding {
    fn back_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        text_button("onb-back", t("pill.onboarding.back"), cx.listener(|view, _: &ClickEvent, window, cx| view.back(window, cx)))
            .into_any_element()
    }

    fn next_button(&self, label: &'static str, cx: &mut Context<Self>) -> gpui::AnyElement {
        primary("onb-next", label, cx.listener(|view, _: &ClickEvent, window, cx| view.next(window, cx)))
    }

    fn dots(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let all = steps();
        let at = all.iter().position(|s| *s == self.step).unwrap_or(0);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .children(all.iter().enumerate().map(|(i, step)| {
                let step = *step;
                div()
                    .id(("onb-dot", i))
                    .h(px(8.))
                    .w(px(if i == at { 26. } else { 8. }))
                    .rounded_full()
                    .bg(hsla(TEXT).opacity(if i <= at { 0.95 } else { 0.18 }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.go(step, window, cx)))
            }))
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hsla(MUTED))
                    .child(format!("{} · {}", tf("pill.onboarding.tour.stepOf", &[("n", &(at + 1)), ("total", &all.len())]), step_name(self.step))),
            )
            // Salir de la bienvenida en cualquier paso: no se vuelve a abrir sola.
            .when(self.step != Step::Listo, |el| {
                el.child(text_button(
                    "onb-skip-all",
                    t("pill.onboarding.skipAll"),
                    cx.listener(|view, _: &ClickEvent, window, cx| view.finish(window, cx)),
                ))
            })
    }

    fn hola(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let languages = [
            (t("settings.language.system"), "system"),
            (t("settings.language.es"), "es"),
            (t("settings.language.en"), "en"),
        ];
        let claim = |text: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(15.))
                .text_color(hsla(TEXT).opacity(0.9))
                .child(check(hsla(GREEN)))
                .child(text)
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(face(96., self.face.pose(), true, Some(self.face_at.clone())))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(title(t("pill.onboarding.title")))
                    .child(lead(t("pill.onboarding.hola.lead")))
                    .child(div().mt(px(6.)).child(gain(t("pill.onboarding.gain.hola")))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(claim(t("onboarding.claim1")))
                    .child(claim(t("onboarding.claim2")))
                    .child(claim(t("onboarding.claim3"))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .mt(px(8.))
                    .items_start()
                    .child(div().text_size(px(14.)).text_color(hsla(MUTED)).child(t("pill.onboarding.language")))
                    .child(segmented("onb-language", &languages, &self.cfg.ui_language, |cfg, v| cfg.ui_language = v.into(), cx)),
            )
            .child(div().flex_1())
            .child(footer(None, "", self.next_button(t("pill.onboarding.start"), cx)))
            .into_any_element()
    }

    #[cfg(target_os = "macos")]
    fn permisos(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if let Some(permission) = self.asking {
            return self.beside_settings(permission, cx);
        }
        let granted = Permission::ALL.iter().filter(|p| permissions::state(**p) == State::Granted).count();
        let paste = permissions::state(Permission::Accessibility) == State::Granted;
        let restart = self.screen_missing && permissions::state(Permission::ScreenRecording) == State::Granted;
        let rows = Permission::ALL.into_iter().enumerate().fold(card(), |list, (i, permission)| {
            let state = permissions::state(permission);
            let ok = state == State::Granted;
            let level = permission.level();
            let level_color = match level {
                Level::Required => AMBER,
                Level::Recommended => TEXT,
                Level::Optional => FAINT,
            };
            list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .px(px(16.))
                    .py(px(11.))
                    .child(
                        div()
                            .size(px(28.))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if ok { hsla(GREEN).opacity(0.18) } else { hsla(TEXT).opacity(0.08) })
                            .when(ok, |el| el.child(check(hsla(GREEN))))
                            .when(!ok, |el| el.text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(format!("{}", i + 1))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(permission.title()))
                                    .child(
                                        div()
                                            .px(px(7.))
                                            .rounded(px(8.))
                                            .bg(hsla(TEXT).opacity(0.06))
                                            .text_size(px(12.))
                                            .text_color(hsla(level_color))
                                            .child(level.label()),
                                    ),
                            )
                            .child(div().text_size(px(13.5)).line_height(px(19.)).text_color(hsla(MUTED)).child(permission.why())),
                    )
                    .child(if ok {
                        // Ya concedido: abrir su panel para quitarlo a mano, o
                        // quitarlo de una, para probar cómo se pide.
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().text_size(px(13.5)).text_color(hsla(GREEN)).child(t("pill.onboarding.perm.granted")))
                            .child(text_button(
                                leak(format!("onb-open-{permission:?}")),
                                t("pill.onboarding.perm.openSettings"),
                                move |_, _, _| permissions::open_settings(permission),
                            ))
                            .child(text_button(
                                leak(format!("onb-reset-{permission:?}")),
                                t("pill.onboarding.perm.remove"),
                                cx.listener(move |view, _: &ClickEvent, _, cx| view.reset(permission, cx)),
                            ))
                            .into_any_element()
                    } else {
                        text_button(
                            leak(format!("onb-perm-{permission:?}")),
                            t("pill.onboarding.perm.allow"),
                            cx.listener(move |view, _: &ClickEvent, window, cx| view.ask(permission, window, cx)),
                        )
                        .into_any_element()
                    }),
            )
        });
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(title(t("pill.onboarding.perm.title")))
            .child(gain(t("pill.onboarding.gain.permissions")))
            .child(lead(t("pill.onboarding.perm.lead")))
            .child(rows)
            .when_some(self.reset_note.clone(), |el, (note, failed)| {
                el.child(div().text_size(px(13.5)).text_color(hsla(if failed { AMBER } else { MUTED })).child(note))
            })
            .when(restart, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(px(10.))
                        .bg(hsla(AMBER).opacity(0.12))
                        .child(div().flex_1().min_w_0().text_size(px(13.5)).text_color(hsla(AMBER)).child(t("pill.onboarding.perm.restartNote")))
                        .child(primary(
                            "onb-restart",
                            t("pill.onboarding.perm.reopen"),
                            cx.listener(|_, _: &ClickEvent, _, cx| {
                                if let Some(app) = crate::platform::bundle_path() {
                                    if crate::platform::reopen_after_exit(&app).is_ok() {
                                        cx.quit();
                                    }
                                }
                            }),
                        )),
                )
            })
            .child(div().flex_1())
            .child(footer(
                (self.only.is_none()).then(|| self.back_button(cx)),
                tf("pill.onboarding.perm.count", &[("granted", &granted), ("total", &Permission::ALL.len())]),
                self.next_button(
                    match (self.only.is_some(), paste) {
                        (true, true) => t("pill.onboarding.done"),
                        (true, false) => t("pill.onboarding.later"),
                        (false, true) => t("pill.onboarding.continue"),
                        (false, false) => t("pill.onboarding.perm.skipPaste"),
                    },
                    cx,
                ),
            ))
            .into_any_element()
    }

    /// La ventana angosta, al lado de Ajustes: qué tocar y el ícono para
    /// arrastrar a la lista.
    #[cfg(target_os = "macos")]
    fn beside_settings(&self, permission: Permission, cx: &mut Context<Self>) -> gpui::AnyElement {
        let holder = permissions::holder();
        let icon_at = self.icon_at.clone();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.5))
                    .text_color(hsla(MUTED))
                    .child(svg().path("icons/lock.svg").size(px(14.)).text_color(hsla(MUTED)))
                    .child(tf("pill.onboarding.perm.label", &[("name", &permission.title())])),
            )
            .child(div().text_size(px(22.)).line_height(px(28.)).font_weight(FontWeight::BOLD).child(tf("pill.onboarding.perm.turnOn", &[("holder", &holder)])))
            .child(lead(t("pill.onboarding.perm.dragLead")))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .p(px(18.))
                    .rounded(px(14.))
                    .border_1()
                    .border_dashed()
                    .border_color(hsla(TEXT).opacity(0.18))
                    .child(
                        div()
                            .id("onb-drag-app")
                            .relative()
                            .size(px(80.))
                            .rounded(px(18.))
                            .bg(gpui::rgb(0x2a2a2d))
                            .border_1()
                            .border_color(hsla(TEXT).opacity(0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor(gpui::CursorStyle::OpenHand)
                            .on_mouse_down(MouseButton::Left, cx.listener(|view, _: &MouseDownEvent, _, cx| view.drag_app(cx)))
                            .child(face(46., self.face.pose(), false, Some(self.face_at.clone())))
                            .child(
                                canvas(move |bounds, _, _| icon_at.set(bounds), |_, _, _, _| {})
                                    .absolute()
                                    .size_full(),
                            ),
                    )
                    .child(div().text_size(px(14.)).text_color(hsla(TEXT).opacity(0.9)).child(t("pill.onboarding.perm.dragHere"))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .child(div().size(px(8.)).rounded_full().bg(hsla(AMBER)))
                    .child(t("pill.onboarding.perm.waiting")),
            )
            .child(text_button(
                "onb-side-back",
                t("pill.onboarding.perm.backToList"),
                cx.listener(|view, _: &ClickEvent, window, cx| {
                    view.leave_side(window);
                    cx.notify();
                }),
            ))
            .into_any_element()
    }

    fn voz(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let ready = self.engine_ready(cx);
        let groq = self.groq_ready(cx);
        let cards = Engine::all().iter().map(|&engine| {
            let i = info(engine);
            let on = self.engine == Some(engine);
            let caps = (0..3).map(|k| {
                let ok = i.can[k];
                div()
                    .px(px(7.))
                    .py(px(2.))
                    .rounded(px(8.))
                    .text_size(px(12.))
                    .bg(if ok { hsla(GREEN).opacity(0.14) } else { hsla(TEXT).opacity(0.05) })
                    .text_color(if ok { hsla(GREEN) } else { hsla(FAINT) })
                    .when(!ok, |el| el.line_through())
                    .child(format!("{} {}", if ok { "✓" } else { "✕" }, can()[k]))
            });
            let line = |sign: &'static str, text: &'static str, color: u32| {
                div()
                    .flex()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .text_color(hsla(color))
                    .child(div().font_weight(FontWeight::BOLD).child(sign))
                    .child(text)
            };
            div()
                .id(SharedString::from(format!("onb-engine-{engine:?}")))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(14.))
                .rounded(px(14.))
                .cursor_pointer()
                .bg(hsla(TEXT).opacity(if on { 0.08 } else { 0.035 }))
                .border_1()
                .border_color(hsla(TEXT).opacity(if on { 0.6 } else { 0.06 }))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.pick_engine(engine, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .size(px(34.))
                                .flex_none()
                                .rounded(px(10.))
                                .bg(hsla(TEXT).opacity(0.1))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(svg().path(i.icon).size(px(18.)).text_color(hsla(TEXT))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(i.title))
                                .child(div().text_size(px(12.)).text_color(hsla(if engine == Engine::Groq { AMBER } else { MUTED })).child(i.chip)),
                        )
                        .child(
                            div()
                                .size(px(18.))
                                .rounded_full()
                                .border_1()
                                .border_color(hsla(TEXT).opacity(if on { 0.95 } else { 0.3 }))
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(on, |el| el.child(div().size(px(8.)).rounded_full().bg(hsla(TEXT)))),
                        ),
                )
                .child(div().flex().flex_wrap().gap(px(4.)).children(caps))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .children(i.pros.iter().map(|p| line("+", p, GREEN)))
                        .children(i.cons.iter().map(|c| line("−", c, MUTED))),
                )
        }).collect::<Vec<_>>();
        let local = crate::local_stt::state();
        let detail = match self.engine {
            Some(Engine::Local) => Some(self.local_box(local, cx)),
            #[cfg(target_os = "macos")]
            Some(Engine::Apple) => Some(self.apple_box(cx)),
            _ => None,
        };
        let wants_groq = self.engine == Some(Engine::Groq) || self.groq_summary;
        let summary_hint = matches!(self.engine, Some(Engine::Local | Engine::Apple)) && !self.groq_summary;
        let note = if self.engine.is_none() {
            t("pill.onboarding.voice.chooseLater").to_string()
        } else if !ready {
            t("pill.onboarding.voice.unfinished").to_string()
        } else if self.engine == Some(Engine::Groq) || groq {
            t("pill.onboarding.voice.allReady").to_string()
        } else {
            t("pill.onboarding.voice.noSummaryReady").to_string()
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(title(t("pill.onboarding.voice.title")))
            .child(gain(t("pill.onboarding.gain.voice")))
            .child(lead(t("pill.onboarding.voice.lead")))
            .child(div().flex().gap(px(10.)).children(cards))
            .children(detail)
            .when(summary_hint, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(px(10.))
                        .bg(hsla(TEXT).opacity(0.04))
                        .child(div().flex_1().min_w_0().text_size(px(13.5)).text_color(hsla(MUTED)).child(t("pill.onboarding.voice.summaryHint")))
                        .child(text_button(
                            "onb-groq-summary",
                            t("pill.onboarding.voice.addGroq"),
                            cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.groq_summary = true;
                                view.use_groq_for_summaries(cx);
                            }),
                        )),
                )
            })
            .when(wants_groq, |el| el.child(self.groq_box(cx)))
            .child(div().flex_1())
            .child(footer(
                Some(self.back_button(cx)),
                note,
                self.next_button(if ready { t("pill.onboarding.continue") } else { t("pill.onboarding.notNow") }, cx),
            ))
            .into_any_element()
    }

    fn groq_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        card()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .child(div().flex_1().min_w_0().text_size(px(13.5)).text_color(hsla(TEXT).opacity(0.9)).child(t("pill.onboarding.groq.step1")))
                    .child(text_button(
                        "onb-groq-console",
                        t("pill.onboarding.groq.openConsole"),
                        |_, _, cx| cx.open_url("https://console.groq.com/keys"),
                    )),
            )
            .child(key_row(self, t("pill.onboarding.groq.step2"), SecretKind::GroqApiKey, cx))
            .child(
                div()
                    .px(px(16.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .text_color(hsla(MUTED))
                    .child(t("pill.onboarding.groq.note")),
            )
    }

    fn local_box(&self, state: crate::local_stt::Download, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::local_stt::Download;
        let body = match state {
            Download::Ready => div().text_size(px(13.5)).text_color(hsla(GREEN)).child(t("pill.onboarding.local.ready")).into_any_element(),
            Download::Downloading { done, total } => {
                let f = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .h(px(6.))
                            .w_full()
                            .rounded_full()
                            .bg(hsla(TEXT).opacity(0.1))
                            .child(div().h_full().w(gpui::relative(f.clamp(0.0, 1.0))).rounded_full().bg(hsla(TEXT))),
                    )
                    .child(div().text_size(px(12.5)).text_color(hsla(MUTED)).child(tf(
                        "pill.onboarding.local.downloading",
                        &[
                            ("percent", &(f * 100.0).round()),
                            ("done", &(done / 1_000_000)),
                            ("total", &(total / 1_000_000)),
                        ],
                    )))
                    .into_any_element()
            }
            Download::Idle | Download::Failed(_) => {
                let failed = matches!(state, Download::Failed(_));
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().text_size(px(13.5)).text_color(hsla(if failed { AMBER } else { MUTED })).child(match &state {
                        Download::Failed(error) => tf("pill.onboarding.local.failed", &[("error", error)]),
                        _ => t("pill.onboarding.local.about").to_string(),
                    }))
                    .child(primary(
                        "onb-local-download",
                        if failed { t("pill.onboarding.local.retry") } else { t("pill.onboarding.local.download") },
                        cx.listener(|_, _: &ClickEvent, _, cx| {
                            crate::local_stt::start_download();
                            cx.notify();
                        }),
                    ))
                    .into_any_element()
            }
        };
        div().px(px(16.)).py(px(12.)).rounded(px(12.)).bg(hsla(TEXT).opacity(0.04)).child(body).into_any_element()
    }

    #[cfg(target_os = "macos")]
    fn apple_box(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mic = permissions::state(Permission::Microphone) == State::Granted;
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .py(px(10.))
            .rounded(px(10.))
            .bg(hsla(if mic { GREEN } else { AMBER }).opacity(0.12))
            .child(div().flex_1().min_w_0().text_size(px(13.5)).text_color(hsla(if mic { GREEN } else { AMBER })).child(if mic {
                t("pill.onboarding.apple.ready")
            } else {
                t("pill.onboarding.apple.needsMic")
            }))
            .when(!mic, |el| {
                el.child(text_button(
                    "onb-apple-mic",
                    t("pill.onboarding.apple.giveMic"),
                    cx.listener(|_, _: &ClickEvent, _, cx| {
                        permissions::request(Permission::Microphone);
                        cx.notify();
                    }),
                ))
            })
            .into_any_element()
    }

    fn herramientas(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let ring_at = self.ring_at.clone();
        let (w, h) = RING_BOX;
        let (strip, more) = split(&self.tools);
        let buttons = (0..pill_tools::COUNT).map(|tool| {
            let angle = -std::f32::consts::FRAC_PI_2 + tool as f32 * std::f32::consts::TAU / pill_tools::COUNT as f32;
            let (cx_, cy) = (w / 2.0 + RING_R * angle.cos(), h / 2.0 + RING_R * angle.sin());
            let chosen = self.tools.contains(&tool);
            let ring_at = self.ring_at.clone();
            div()
                .id(("onb-tool", tool))
                .absolute()
                .left(px(cx_ - 42.0))
                .top(px(cy - 32.0))
                .w(px(84.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(5.))
                .cursor_pointer()
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    let origin = ring_at.get().origin;
                    let at = (f32::from(origin.x) + cx_, f32::from(origin.y) + cy - 9.0);
                    view.toggle_tool(tool, at, window, cx);
                }))
                .child(
                    div()
                        .size(px(46.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if chosen { hsla(FILL) } else { hsla(TEXT).opacity(0.06) })
                        .border_1()
                        .border_color(hsla(TEXT).opacity(if chosen { 0.0 } else { 0.12 }))
                        .child(
                            svg()
                                .path(pill_tools::icon(tool))
                                .size(px(21.))
                                .text_color(if chosen { hsla(INK) } else { hsla(TEXT).opacity(0.85) }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(hsla(if chosen { TEXT } else { MUTED }))
                        .when(chosen, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .child(tool_name(tool)),
                )
        }).collect::<Vec<_>>();
        let shown: Vec<usize> = if self.tab_more { more.clone() } else { strip.clone() };
        let tab = |id: &'static str, label: String, on: bool, more: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(26.))
                .px(px(10.))
                // Concéntrica con su riel (9 menos el margen de 3).
                .rounded(px(6.))
                .flex()
                .items_center()
                .text_size(px(13.))
                .cursor_pointer()
                .bg(if on { hsla(FILL) } else { hsla(TEXT).opacity(0.0) })
                .text_color(if on { hsla(INK) } else { hsla(MUTED) })
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.tab_more = more;
                    cx.notify();
                }))
                .child(label)
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(title(t("pill.onboarding.tools.title")))
            .child(gain(t("pill.onboarding.gain.tools")))
            .child(lead(t("pill.onboarding.tools.lead")))
            .child(
                div().flex().justify_center().child(
                    div()
                        .relative()
                        .w(px(w))
                        .h(px(h))
                        .child(canvas(move |bounds, _, _| ring_at.set(bounds), |_, _, _, _| {}).absolute().size_full())
                        .child(div().absolute().left(px(w / 2.0 - 48.0)).top(px(h / 2.0 - 48.0)).child(face(96., self.face.pose(), true, Some(self.face_at.clone()))))
                        .children(buttons),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(12.))
                    .bg(hsla(TEXT).opacity(0.04))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(3.))
                                    .p(px(3.))
                                    .rounded(px(9.))
                                    .bg(hsla(TEXT).opacity(0.06))
                                    .child(tab("onb-tab-strip", tf("pill.onboarding.tools.tabStrip", &[("count", &strip.len())]), !self.tab_more, false, cx))
                                    .child(tab("onb-tab-more", tf("pill.onboarding.tools.tabMore", &[("count", &more.len())]), self.tab_more, true, cx)),
                            )
                            .child(div().flex_1().text_size(px(12.5)).text_color(hsla(MUTED)).text_right().child(if self.tab_more {
                                t("pill.onboarding.tools.moreHint")
                            } else {
                                t("pill.onboarding.tools.stripHint")
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .min_h(px(28.))
                            .items_center()
                            .children(shown.iter().map(|&tool| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .h(px(28.))
                                    .px(px(10.))
                                    .rounded_full()
                                    .bg(hsla(TEXT).opacity(0.07))
                                    .text_size(px(13.))
                                    .child(svg().path(pill_tools::icon(tool)).size(px(14.)).text_color(hsla(TEXT)))
                                    .child(tool_name(tool))
                            }))
                            .when(shown.is_empty(), |el| {
                                el.child(div().text_size(px(13.)).text_color(hsla(MUTED)).child(if self.tab_more {
                                    t("pill.onboarding.tools.allChosen")
                                } else {
                                    t("pill.onboarding.tools.noneChosen")
                                }))
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(footer(Some(self.back_button(cx)), t("pill.onboarding.tools.changeLater"), self.next_button(t("pill.onboarding.continue"), cx)))
            .into_any_element()
    }

    /// El paso del tour. Antes de empezar explica, con dibujos, que va a
    /// aparecer un cursor de ejemplo y que hay que imitarlo con el mouse; ya
    /// empezado, la lista de pasos con el de ahora destacado.
    fn tour(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let done = TOUR.load(Ordering::Relaxed);
        let all = TASKS.iter().all(|(task, ..)| done & (*task as u8) != 0);
        let (soft, on_soft, accent, on_accent) = crate::theme::expressive().map_or(
            (hsla(TEXT).opacity(0.08), hsla(TEXT), hsla(FILL), hsla(INK)),
            |t| (t.accent_soft, t.on_accent_soft, t.accent, t.on_accent),
        );
        // Cómo funciona, en tres cuadros: mira el ejemplo, hazlo tú, se marca solo.
        let how = |art: AnyElement, n: usize, title: &'static str, body: &'static str| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(16.))
                .rounded(px(20.))
                .bg(hsla(TEXT).opacity(0.04))
                .child(
                    div()
                        .h(px(64.))
                        .rounded(px(14.))
                        .bg(soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(art),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .size(px(24.))
                                .flex_none()
                                .rounded_full()
                                .bg(accent)
                                .text_color(on_accent)
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(format!("{n}")),
                        )
                        .child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).child(title)),
                )
                .child(div().text_size(px(13.5)).line_height(px(19.)).text_color(hsla(MUTED)).child(body))
        };
        // El cursor de ejemplo en chico, igual al que aparece sobre la pill.
        let example = div()
            .flex()
            .items_start()
            .child(
                div()
                    .relative()
                    .w(px(20.))
                    .h(px(30.))
                    .child(svg().path("icons/cursor-fill.svg").absolute().top_0().left_0().size_full().text_color(accent))
                    .child(svg().path("icons/cursor-edge.svg").absolute().top_0().left_0().size_full().text_color(gpui::white())),
            )
            .child(
                div()
                    .mt(px(22.))
                    .ml(px(-4.))
                    .px(px(8.))
                    .py(px(1.))
                    .rounded_full()
                    .bg(accent)
                    .border_1()
                    .border_color(gpui::white())
                    .text_size(px(12.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(on_accent)
                    .child(t("pill.onboarding.tour.example")),
            )
            .into_any_element();
        let yours = svg().path("icons/mouse-pointer-2.svg").size(px(30.)).text_color(on_soft).into_any_element();
        let checks = svg().path("icons/circle-check.svg").size(px(30.)).text_color(on_soft).into_any_element();
        let explainer = div()
            .flex()
            .gap(px(12.))
            .child(how(example, 1, t("pill.onboarding.tour.how1Title"), t("pill.onboarding.tour.how1Body")))
            .child(how(yours, 2, t("pill.onboarding.tour.how2Title"), t("pill.onboarding.tour.how2Body")))
            .child(how(checks, 3, t("pill.onboarding.tour.how3Title"), t("pill.onboarding.tour.how3Body")));
        let current = TASKS.iter().map(|(t, ..)| *t as u8).find(|t| done & t == 0);
        let rows = TASKS.iter().map(|(task, ..)| {
            let task = *task as u8;
            let (name, _) = task_text(task);
            let (doing, _) = task_do(task);
            let ok = done & task != 0;
            let now = self.tour_started && current == Some(task);
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(16.))
                .bg(if ok {
                    hsla(GREEN).opacity(0.1)
                } else if now {
                    soft
                } else {
                    hsla(TEXT).opacity(0.04)
                })
                .border_1()
                .border_color(if now { accent } else { hsla(TEXT).opacity(0.0) })
                .child(
                    div()
                        .size(px(36.))
                        .flex_none()
                        .rounded(px(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if ok { hsla(GREEN) } else { hsla(TEXT).opacity(0.08) })
                        .child(
                            svg()
                                .path(if ok { "icons/check.svg" } else { task_icon(task) })
                                .size(px(18.))
                                .text_color(if ok { Hsla::from(gpui::rgb(0x1a1a18)) } else { hsla(TEXT) }),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(name))
                        .child(div().text_size(px(13.5)).text_color(hsla(MUTED)).child(doing)),
                )
                .when(now, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .px(px(10.))
                            .py(px(3.))
                            .rounded_full()
                            .bg(accent)
                            .text_color(on_accent)
                            .text_size(px(12.5))
                            .font_weight(FontWeight::BOLD)
                            .child(t("pill.onboarding.tour.now")),
                    )
                })
        });
        let action = if !self.tour_started {
            primary("onb-tour-start", t("pill.onboarding.tour.start"), cx.listener(|view, _: &ClickEvent, window, cx| view.start_tour(window, cx)))
        } else if all {
            self.next_button(if self.only.is_some() { t("pill.onboarding.done") } else { t("pill.onboarding.continue") }, cx)
        } else {
            text_button(
                "onb-tour-skip",
                t("pill.onboarding.tour.skipTutorial"),
                cx.listener(|view, _: &ClickEvent, window, cx| view.skip_tour(window, cx)),
            )
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(title(t("pill.onboarding.tour.title")))
            .child(gain(t("pill.onboarding.gain.tour")))
            // Antes de empezar, cómo funciona; ya empezado, los pasos.
            .when(!self.tour_started, |el| el.child(explainer))
            .when(self.tour_started, |el| el.children(rows))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .children((self.only.is_none()).then(|| self.back_button(cx)))
                    .child(div().flex_1().text_size(px(13.)).text_color(hsla(MUTED)).text_right().child(if all {
                        ""
                    } else {
                        t("pill.onboarding.tour.exitNote")
                    }))
                    // Sin empezar, también se puede saltar de una.
                    .when(!self.tour_started, |el| {
                        el.child(text_button(
                            "onb-tour-skip",
                            t("pill.onboarding.tour.skipTutorial"),
                            cx.listener(|view, _: &ClickEvent, window, cx| view.skip_tour(window, cx)),
                        ))
                    })
                    .child(action),
            )
            .into_any_element()
    }

    fn listo(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::dictation_settings::shortcut_label;
        let cfg = &self.cfg;
        let groq = self.groq_ready(cx);
        let voice = match cfg.dictation_backend.as_str() {
            "groq" if groq => t("pill.onboarding.finish.groqVoice").to_string(),
            "parakeet" if crate::local_stt::ready() => format!(
                "{}{}",
                t("pill.onboarding.finish.localVoice"),
                if groq { t("pill.onboarding.finish.groqSummarizes") } else { t("pill.onboarding.finish.addGroq") }
            ),
            "apple" if cfg!(target_os = "macos") => format!(
                "{}{}",
                t("pill.onboarding.finish.appleVoice"),
                if groq { t("pill.onboarding.finish.groqSummarizes") } else { t("pill.onboarding.finish.addGroq") }
            ),
            _ => t("pill.onboarding.finish.noEngine").to_string(),
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(title(t("pill.onboarding.finish.title")))
            .child(gain(t("pill.onboarding.gain.done")))
            .child(lead(t("pill.onboarding.finish.lead")))
            .child(
                div().flex().flex_wrap().gap(px(8.)).children(KEYS.iter().enumerate().map(|(i, (what, get, _))| {
                    let recording = self.recording == Some(i);
                    let key = get(cfg);
                    let label = if recording {
                        t("pill.onboarding.finish.pressShortcut").to_string()
                    } else if key.is_empty() {
                        t("pill.onboarding.finish.noShortcut").to_string()
                    } else {
                        shortcut_label(key)
                    };
                    div()
                        .id(("onb-key", i))
                        .w(px(360.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(10.))
                        .cursor_pointer()
                        .bg(hsla(TEXT).opacity(if recording { 0.1 } else { 0.05 }))
                        .border_1()
                        .border_color(hsla(TEXT).opacity(if recording { 0.5 } else { 0.0 }))
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.record(i, window, cx)))
                        .child(
                            div()
                                .px(px(7.))
                                .py(px(3.))
                                .rounded(px(6.))
                                .bg(if recording { hsla(FILL) } else { hsla(TEXT).opacity(0.1) })
                                .text_color(if recording { hsla(INK) } else { hsla(TEXT) })
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .child(div().flex_1().text_size(px(14.)).text_color(hsla(TEXT).opacity(0.9)).child(t(*what)))
                        .child(div().text_size(px(12.5)).text_color(hsla(MUTED)).child(if recording { t("pill.onboarding.finish.escCancels") } else { t("pill.onboarding.finish.change") }))
                })),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(10.))
                    .bg(hsla(TEXT).opacity(0.04))
                    .child(svg().path("icons/mic.svg").size(px(16.)).text_color(hsla(TEXT)))
                    .child(div().flex_1().min_w_0().text_size(px(14.)).line_height(px(20.)).text_color(hsla(MUTED)).child(voice)),
            )
            .child(code_card(cfg))
            .child(div().flex_1())
            .child(footer(
                Some(self.back_button(cx)),
                t("pill.onboarding.finish.seeAgain"),
                primary("onb-finish", t("pill.onboarding.finish.start"), cx.listener(|view, _: &ClickEvent, window, cx| view.finish(window, cx))),
            ))
            .into_any_element()
    }
}

/// En «Listo»: Atic Code, la ventana de agentes, que no está a la vista en la
/// pill si no se puso en la tira. Su atajo y un botón para abrirla.
fn code_card(cfg: &Config) -> impl IntoElement {
    let keys = cfg.code_shortcut.trim();
    let body = if keys.is_empty() {
        t("pill.onboarding.finish.codeBodyNoKeys").to_string()
    } else {
        tf("pill.onboarding.finish.codeBody", &[("keys", &crate::dictation_settings::shortcut_label(keys))])
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(10.))
        .bg(hsla(TEXT).opacity(0.04))
        .child(svg().path("icons/code-xml.svg").size(px(18.)).flex_none().text_color(hsla(TEXT)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(hsla(TEXT)).child(t("pill.onboarding.finish.codeTitle")))
                .child(div().text_size(px(13.5)).line_height(px(19.)).text_color(hsla(MUTED)).child(body)),
        )
        .child(text_button("onb-open-code", t("pill.onboarding.finish.codeOpen"), |_, _, cx| {
            cx.defer(crate::Pill::open_code_from_app)
        }))
}

/// El botón principal: relleno en M3 y en el vidrio, claro en «Atic».
fn primary(id: &'static str, label: &'static str, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> gpui::AnyElement {
    if crate::theme::styled().is_some() {
        return gpui_m3::Button::new(id, label)
            .variant(gpui_m3::ButtonVariant::Filled)
            .on_click(on_click)
            .into_any_element();
    }
    div()
        .id(id)
        .flex_none()
        .px(px(16.))
        .py(px(8.))
        .rounded_full()
        .bg(hsla(FILL))
        .text_color(hsla(INK))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

/// Ajustes → Cómo se hace: cada gesto con «Muéstrame», que lo enseña sobre
/// la pill con el cursor de ejemplo.
pub(crate) fn guides(cx: &mut App) -> impl IntoElement {
    let _ = cx;
    let all = TASKS.iter().chain(std::iter::once(&CUSTOMIZE)).copied().collect::<Vec<_>>();
    let running = GUIDE.lock().ok().and_then(|g| *g).map(|(t, _)| t);
    let list = all.into_iter().fold(card(), |list, (task, ..)| {
        let (title, hint) = task_text(task as u8);
        let on = running == Some(task as u8);
        list.child(crate::meetings::settings::row(
            title,
            hint,
            crate::meetings::settings::button(
                SharedString::from(format!("guide-{}", task as u8)),
                if on { t("pill.onboarding.guides.showing") } else { t("pill.onboarding.guides.showMe") },
                move |_, _, _| show_guide(task),
            ),
        ))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(list)
        .when(running.is_some(), |el| {
            el.child(div().flex().child(crate::meetings::settings::button(
                "guide-stop".into(),
                t("pill.onboarding.guides.stop"),
                |_, _, _| {
                    if let Ok(mut guide) = GUIDE.lock() {
                        *guide = None;
                    }
                    set_demo(None);
                },
            )))
        })
}

/// Los botones piden un id `'static`; hay pocos y fijos (uno por permiso).
#[cfg(target_os = "macos")]
fn leak(id: String) -> &'static str {
    static IDS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut ids = IDS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = ids.iter().find(|known| **known == id) {
        return found;
    }
    let leaked: &'static str = Box::leak(id.into_boxed_str());
    ids.push(leaked);
    leaked
}

impl Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        let m3_scope = crate::theme::scope_m3(cx);
        // Mark mira al mouse; mientras se acomoda, pide cuadros.
        let cursor = self.cursor_in(window);
        self.seen = cursor;
        if let (Some((x, y)), Some((cx_, cy, size))) = (cursor, self.face_at.get()) {
            self.face.aim((x - cx_, y - cy), size);
        }
        if self.face.step(Instant::now()) {
            window.request_animation_frame();
        }
        #[cfg(target_os = "macos")]
        let side = self.asking.is_some();
        #[cfg(not(target_os = "macos"))]
        let side = false;
        let body = match self.step {
            Step::Hola => self.hola(cx),
            #[cfg(target_os = "macos")]
            Step::Permisos => self.permisos(cx),
            #[cfg(not(target_os = "macos"))]
            Step::Permisos => self.hola(cx),
            Step::Voz => self.voz(cx),
            Step::Herramientas => self.herramientas(cx),
            Step::Tour => self.tour(cx),
            Step::Listo => self.listo(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Sin aviso: el sondeo de 60 ms (`new`) redibuja si cambió lo que mira Mark;
            // avisar con cada movimiento rehacía la ventana a la frecuencia del mouse.
            .on_mouse_move(cx.listener(|view, event: &gpui::MouseMoveEvent, _, _| {
                view.cursor = Some(event.position);
            }))
            .bg(crate::theme::window_bg())
            .map(|el| crate::theme::window_frame(el, window))
            .font_family(crate::theme::font())
            .text_color(hsla(TEXT))
            .child(
                div()
                    .h(px(TOP_H))
                    .flex_none()
                    .pl(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(chrome::lights())
                    .child(chrome::drag(TOP_H))
                    .child(chrome::controls(window.is_maximized(), TOP_H)),
            )
            .child(
                div()
                    .id("onb-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .px(px(if side { 22. } else { GUTTER }))
                    .pb(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .when(!side && self.only.is_none(), |el| el.child(self.dots(cx)))
                    .child(step_enter(body, self.step, gpui_m3::MotionSettings::get(cx).reduced)),
            )
            .children(m3_scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_tira_lleva_todas_las_elegidas_y_mas_el_resto() {
        let chosen = [3, 1, 4, 0, 2, 5, 9, 7];
        let (ring, more) = split(&chosen);
        assert_eq!(ring, chosen.to_vec());
        // Las que no se eligieron, en orden (Computadores es la última).
        assert_eq!(more, vec![6, 8, 10, 11, 12]);
    }

    #[test]
    fn mac_pide_permisos_y_windows_no() {
        assert_eq!(steps().contains(&Step::Permisos), cfg!(target_os = "macos"));
        assert_eq!(steps().first(), Some(&Step::Hola));
        assert_eq!(steps().last(), Some(&Step::Listo));
    }
}
