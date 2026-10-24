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
    canvas, div, prelude::*, px, size, svg, AnyElement, App, Bounds, ClickEvent, Context, FontWeight, Hsla,
    MouseButton, MouseDownEvent, Pixels, SharedString, Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::i18n::t;
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
        Step::Hola => "Hola",
        Step::Permisos => "Permisos",
        Step::Voz => "Voz e IA",
        Step::Herramientas => "Herramientas",
        Step::Tour => "La pill",
        Step::Listo => "Listo",
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
const CUSTOMIZE: (Tour, &str, &str) = (
    Tour::Customize,
    "Edita la tira",
    "Mantén apretada una herramienta de la tira medio segundo: se abre el editor. Ahí arrastras cada una entre «A la vista», «Más» y «Fuera de la pill».",
);

/// El título y qué hacer de un paso.
fn task_text(task: u8) -> (&'static str, &'static str) {
    TASKS
        .iter()
        .chain(std::iter::once(&CUSTOMIZE))
        .find(|(t, ..)| *t as u8 == task)
        .map_or(("", ""), |(_, title, hint)| (*title, *hint))
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

/// Los pasos del tour, en orden: qué hacer y cómo.
const TASKS: [(Tour, &str, &str); 5] = [
    (Tour::Strip, "Pasa el cursor por la pill", "Arriba, en el borde: se abre la tira con tus herramientas."),
    (Tour::Tool, "Toca una herramienta", "Se abre en el notch. Para cerrarla, presiona Esc o toca la cara."),
    (Tour::Settings, "Abre los Ajustes", "Toca la cara de Atic (con el notch cerrado). Ahí cambias atajos, motor de voz y apariencia."),
    (Tour::Move, "Mueve la pill", "Arrástrala por el borde de arriba: se junta con el notch, se pega a los lados o pasa a otra pantalla."),
    (Tour::Wheel, "Sácala del borde", "Arrástrala hacia abajo y suéltala: queda como gota y, al pasar el cursor, aparece la rueda. Para devolverla, llévala al borde."),
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

pub(crate) fn pill_state(near: bool, open: bool, wheel: bool, taken: (f32, f32, f32, f32), width: f32) {
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

/// Lo que la bienvenida dibuja en la ventana de la pill: el fantasma del
/// arrastre y las herramientas en vuelo. `origin` es la esquina de la ventana
/// de la pill en puntos globales; `target`, dónde aterrizan (en la ventana).
pub(crate) fn overlay(origin: (f32, f32), target: (f32, f32), window: &mut Window) -> Vec<AnyElement> {
    let mut out = Vec::new();
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
    let guide = end_guide_if_done();
    if let Some((task, since)) = DEMO.lock().ok().and_then(|d| *d) {
        window.request_animation_frame();
        let near = PILL_NEAR.load(Ordering::Relaxed);
        let open = PILL_OPEN.load(Ordering::Relaxed);
        let wheel = PILL_WHEEL.load(Ordering::Relaxed);
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
        if show {
            let t = if points { since.elapsed().as_secs_f32() } else { since.elapsed().as_secs_f32().max(idle - 1.2) };
            out.extend(demo(task, t, target));
        }
        let (taken, width) = PILL_TAKEN.lock().map(|t| *t).unwrap_or(((0.0, 0.0, 0.0, 0.0), 0.0));
        out.push(coach(task, blocked, taken, width, guide));
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

/// El texto del tutorial, flotando al lado de lo que ocupa la pill (a la
/// derecha si cabe; si no, a la izquierda), sin taparla.
fn coach(task: u8, blocked: bool, taken: (f32, f32, f32, f32), width: f32, guide: bool) -> AnyElement {
    const W: f32 = 300.0;
    const GAP: f32 = 22.0;
    let done = TOUR.load(Ordering::Relaxed);
    let index = TASKS.iter().position(|(t, ..)| *t as u8 == task).unwrap_or(0);
    let (title, hint) = task_text(task);
    let cheer = CHEER.lock().ok().and_then(|c| *c).filter(|c| c.elapsed() < CHEER_FOR);
    let finished = TASKS.iter().filter(|(t, ..)| done & *t as u8 != 0).count();
    let (head, body): (String, String) = match (cheer, blocked) {
        (Some(_), _) if guide => ("¡Bien hecho!".into(), "Ya sabes cómo. Lo repasas cuando quieras en Ajustes → Cómo se hace.".into()),
        (Some(_), _) => ("¡Bien hecho!".into(), format!("Sigue con: {title}.")),
        (None, true) => (title.into(), "Primero cierra lo que está abierto: presiona Esc o toca fuera de la pill.".into()),
        (None, false) => (title.into(), hint.into()),
    };
    let (x, y, w, _) = taken;
    let right = x + w + GAP;
    let left = if right + W <= width - 12.0 { right } else { (x - GAP - W).max(12.0) };
    let ink: Hsla = gpui::rgb(0xf0f0ea).into();
    let dots = div().flex().gap(px(5.)).children(TASKS.iter().enumerate().map(|(i, (t, ..))| {
        let ok = done & *t as u8 != 0;
        div()
            .h(px(5.))
            .w(px(if i == index { 16. } else { 5. }))
            .rounded_full()
            .bg(if ok { hsla(GREEN) } else if i == index { ink } else { ink.opacity(0.22) })
    }));
    div()
        .absolute()
        .left(px(left))
        .top(px(y.max(10.0) + 6.0))
        .w(px(W))
        .p(px(14.))
        .rounded(px(18.))
        .bg(gpui::rgb(0x161615))
        .border_1()
        .border_color(ink.opacity(0.1))
        .shadow_lg()
        .flex()
        .gap(px(12.))
        .text_color(ink)
        .child(
            div()
                .size(px(38.))
                .flex_none()
                .rounded(px(11.))
                .bg(gpui::rgb(0x242422))
                .flex()
                .items_center()
                .justify_center()
                .child(face(26.0, crate::face::Pose::REST, false, None)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(!guide, |el| el.child(dots))
                        .when(guide, |el| el.child(div().text_size(px(10.5)).text_color(ink.opacity(0.45)).child("Cómo se hace")))
                        .child(div().flex_1())
                        .when(!guide, |el| el.child(div().text_size(px(10.5)).text_color(ink.opacity(0.45)).child(format!("{finished} de {}", TASKS.len())))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .when(cheer.is_some(), |el| el.child(svg().path("icons/check.svg").size(px(15.)).text_color(hsla(GREEN))))
                        .child(head),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .text_color(if blocked && cheer.is_none() { hsla(AMBER) } else { ink.opacity(0.7) })
                        .child(body),
                )
                .child(div().text_size(px(10.5)).text_color(ink.opacity(0.35)).child(if guide {
                    "Se cierra sola al lograrlo, o con «Detener» en Ajustes."
                } else {
                    "Omitir: «Saltar el tour» en la bienvenida"
                })),
        )
        .into_any_element()
}

/// El cursor de ejemplo, con un halo para leerse sobre cualquier fondo.
fn pointer(x: f32, y: f32, pressed: bool, alpha: f32) -> AnyElement {
    let k = if pressed { 0.86 } else { 1.0 };
    div()
        .absolute()
        .left(px(x - 6.0))
        .top(px(y - 4.0))
        .size(px(30.0 * k))
        .rounded_full()
        .bg(gpui::black().opacity(0.35))
        .opacity(alpha)
        .flex()
        .items_center()
        .justify_center()
        .child(svg().path("icons/mouse-pointer-2.svg").size(px(20.0 * k)).text_color(gpui::white()))
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
        .border_color(gpui::white().opacity(0.7 * (1.0 - t)))
        .into_any_element()
}

/// El ejemplo de un paso del tour, sobre la pill (`at`, en su ventana).
fn demo(task: u8, t: f32, at: (f32, f32)) -> Vec<AnyElement> {
    use crate::anim::{ease_smooth_out, segment};
    let lerp = |a: (f32, f32), b: (f32, f32), k: f32| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
    let mut out = Vec::new();
    if task == Tour::Customize as u8 {
        let p = (t % 3.0) / 3.0;
        let alpha = segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.88, 0.12));
        let goal = (at.0 + 64.0, at.1 + 4.0);
        let from = (goal.0 + 150.0, goal.1 + 170.0);
        let (x, y) = lerp(from, goal, ease_smooth_out(segment(p, 0.05, 0.3)));
        let hold = segment(p, 0.4, 0.3);
        if hold > 0.0 {
            // El anillo se llena mientras se mantiene apretada.
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
        out.push(pointer(x, y, hold > 0.0 && p < 0.75, alpha));
        return out;
    }
    if task == Tour::Move as u8 {
        // Toma la pill y la lleva por el borde de arriba, y de vuelta.
        let p = (t % 3.6) / 3.6;
        let alpha = segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.9, 0.1));
        let go = ease_smooth_out(segment(p, 0.15, 0.3)) - ease_smooth_out(segment(p, 0.55, 0.3));
        let (x, y) = (at.0 + 220.0 * go, at.1);
        if go > 0.01 {
            out.push(
                div()
                    .absolute()
                    .left(px(x - 40.0))
                    .top(px(y - 16.0))
                    .w(px(80.))
                    .h(px(32.))
                    .rounded(px(16.))
                    .bg(gpui::rgb(0x1a1a18))
                    .opacity(alpha * 0.85)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(face(22.0, crate::face::Pose::REST, false, None))
                    .into_any_element(),
            );
        }
        out.push(pointer(x + 6.0, y + 6.0, (0.1..0.9).contains(&p), alpha));
        return out;
    }
    if task == Tour::Wheel as u8 {
        // Toma la pill, la arrastra fuera del borde y aparece la rueda.
        let p = (t % 3.4) / 3.4;
        let alpha = segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.9, 0.1));
        let pressed = (0.12..0.62).contains(&p);
        let drag = ease_smooth_out(segment(p, 0.2, 0.36));
        let to = (at.0, at.1 + 190.0);
        let (x, y) = lerp(at, to, drag);
        if drag > 0.0 {
            let side = 46.0;
            out.push(
                div()
                    .absolute()
                    .left(px(x - side / 2.0))
                    .top(px(y - side / 2.0))
                    .size(px(side))
                    .rounded_full()
                    .bg(gpui::rgb(0x1a1a18))
                    .opacity(alpha * 0.92)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(face(30.0, crate::face::Pose::REST, false, None))
                    .into_any_element(),
            );
        }
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
                        .bg(gpui::white().opacity(0.85 * alpha))
                        .into_any_element(),
                );
            }
        }
        out.push(pointer(x + 6.0, y + 6.0, pressed, alpha));
        return out;
    }
    // Pasar el cursor por la pill, tocar una herramienta de la tira, o tocar
    // la cara (los Ajustes).
    let tool = task == Tour::Tool as u8;
    let clicks = tool || task == Tour::Settings as u8;
    let p = (t % 2.8) / 2.8;
    let alpha = segment(p, 0.0, 0.08) * (1.0 - segment(p, 0.88, 0.12));
    let goal = if tool { (at.0 + 64.0, at.1 + 4.0) } else { (at.0 + 4.0, at.1 + 4.0) };
    let from = (goal.0 + 150.0, goal.1 + 170.0);
    let (x, y) = lerp(from, goal, ease_smooth_out(segment(p, 0.06, 0.4)));
    let pulse = segment(p, 0.5, 0.3);
    if pulse > 0.0 && pulse < 1.0 {
        out.push(ripple(goal.0, goal.1, pulse, if tool { 22.0 } else { 34.0 }));
    }
    out.push(pointer(x, y, clicks && (0.5..0.58).contains(&p), alpha));
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
                Some(Step::Permisos) => "Atic · Permisos",
                Some(_) => "Atic · Tour",
                None => "Te damos la bienvenida a Atic",
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
        if cfg!(target_os = "macos") {
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
            chip: "Recomendado",
            icon: "icons/cloud.svg",
            can: [true, true, true],
            pros: ["El más preciso, y resume con IA", "Uno solo para todo, con plan gratis"],
            cons: ["Necesita internet y una llave", "El audio se envía a Groq"],
        },
        Engine::Local => EngineInfo {
            title: "Local",
            chip: "Privado",
            icon: "icons/cpu.svg",
            can: [true, true, false],
            pros: ["El audio no sale del equipo", "Sin internet y sin límites"],
            cons: ["No resume", "Se descarga una vez (~670 MB)"],
        },
        Engine::Apple => EngineInfo {
            title: "Voz de macOS",
            chip: "Ya incluido",
            icon: "icons/mic.svg",
            can: [true, true, false],
            pros: ["Viene en tu Mac: nada que bajar", "Sin internet ni llave"],
            cons: ["No resume", "Solo en Mac"],
        },
    }
}

const CAN: [&str; 3] = ["Dictar", "Transcribir reuniones", "Resumir"];

type GetKey = fn(&Config) -> &String;
type SetKey = fn(&mut Config, String);

/// Los atajos que se muestran (y se cambian) al terminar.
const KEYS: [(&str, GetKey, SetKey); 6] = [
    ("Dictar (mantén apretado)", |c| &c.dictation_shortcut, |c, v| c.dictation_shortcut = v),
    ("Portapapeles", |c| &c.clipboard_shortcut, |c, v| c.clipboard_shortcut = v),
    ("Fragmentos de texto", |c| &c.snippets_shortcut, |c, v| c.snippets_shortcut = v),
    ("Captura", |c| &c.screenshot_shortcut, |c, v| c.screenshot_shortcut = v),
    ("Pizarra", |c| &c.board_shortcut, |c, v| c.board_shortcut = v),
    ("Lanzador", |c| &c.launcher_shortcut, |c, v| c.launcher_shortcut = v),
];

/// El nombre en español de cada herramienta del catálogo (`pill_tools`).
fn tool_name(tool: usize) -> &'static str {
    match pill_tools::name(tool) {
        "Clipboard" => "Portapapeles",
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
    /// Dónde quedó la rueda de herramientas en la ventana: de ahí despegan.
    ring_at: Rc<Cell<Bounds<Pixels>>>,
    /// Dónde está el mouse en la ventana (fuera de Mac, solo dentro de ella).
    cursor: Option<gpui::Point<Pixels>>,
    /// Mark, que mira al mouse con todo el cuerpo, y dónde quedó dibujada.
    face: crate::face::Face,
    face_at: FaceAt,
    /// El último cursor visto en la pantalla, para despertar solo si se movió.
    seen: Option<(f32, f32)>,
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
            ring_at: Rc::default(),
            cursor: None,
            face: crate::face::Face::new(),
            face_at: Rc::default(),
            seen: None,
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

    fn on_key(&mut self, event: &gpui::KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.recording else {
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
        }
        let _ = window;
        cx.notify();
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
        // En el tour, el ejemplo del primer paso que falta.
        let done = TOUR.load(Ordering::Relaxed);
        let next = TASKS.iter().map(|(t, ..)| *t as u8).find(|t| done & t == 0);
        if self.step == Step::Tour {
            set_demo(next);
        } else if GUIDE.lock().is_ok_and(|g| g.is_none()) {
            set_demo(None);
        }
        #[cfg(target_os = "macos")]
        self.follow_settings(window);
        let _ = window;
        cx.notify();
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
            Ok(()) => (format!("Se quitó {}. Si sigue marcado, reabre Atic: macOS lo aplica al volver a abrirla.", permission.title()), false),
            Err(error) => (format!("No se pudo quitar {}: {error}", permission.title()), true),
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
    div().text_size(px(24.)).font_weight(FontWeight::BOLD).child(text)
}

fn lead(text: impl Into<SharedString>) -> impl IntoElement {
    div().text_size(px(13.)).line_height(px(19.)).text_color(hsla(MUTED)).child(text.into())
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
        .child(div().flex_1().text_size(px(11.)).text_color(hsla(FAINT)).text_right().child(note.into()))
        .child(action)
}

impl Onboarding {
    fn back_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        text_button("onb-back", "Atrás", cx.listener(|view, _: &ClickEvent, window, cx| view.back(window, cx)))
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
                    .h(px(6.))
                    .w(px(if i == at { 22. } else { 6. }))
                    .rounded_full()
                    .bg(hsla(TEXT).opacity(if i <= at { 0.95 } else { 0.18 }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.go(step, window, cx)))
            }))
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(hsla(FAINT))
                    .child(format!("{} de {} · {}", at + 1, all.len(), step_name(self.step))),
            )
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
                .text_size(px(13.))
                .text_color(hsla(TEXT).opacity(0.85))
                .child(check(hsla(GREEN)))
                .child(text)
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(face(76., self.face.pose(), true, Some(self.face_at.clone())))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(title("Te damos la bienvenida a Atic"))
                    .child(lead("Una pill que vive en el borde de tu pantalla: copia, dicta, captura y controla la música sin cambiar de app.")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(claim(t("onboarding.claim1")))
                    .child(claim(t("onboarding.claim2")))
                    .child(claim(t("onboarding.claim3"))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child("Idioma"))
                    .child(segmented("onb-language", &languages, &self.cfg.ui_language, |cfg, v| cfg.ui_language = v.into(), cx)),
            )
            .child(div().flex_1())
            .child(footer(None, "", self.next_button("Empezar", cx)))
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
                                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(permission.title()))
                                    .child(
                                        div()
                                            .px(px(7.))
                                            .rounded(px(8.))
                                            .bg(hsla(TEXT).opacity(0.06))
                                            .text_size(px(10.))
                                            .text_color(hsla(level_color))
                                            .child(level.label()),
                                    ),
                            )
                            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(permission.why())),
                    )
                    .child(if ok {
                        // Ya concedido: abrir su panel para quitarlo a mano, o
                        // quitarlo de una, para probar cómo se pide.
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().text_size(px(12.)).text_color(hsla(GREEN)).child("Listo"))
                            .child(text_button(
                                leak(format!("onb-open-{permission:?}")),
                                "Abrir Ajustes",
                                move |_, _, _| permissions::open_settings(permission),
                            ))
                            .child(text_button(
                                leak(format!("onb-reset-{permission:?}")),
                                "Quitar",
                                cx.listener(move |view, _: &ClickEvent, _, cx| view.reset(permission, cx)),
                            ))
                            .into_any_element()
                    } else {
                        text_button(
                            leak(format!("onb-perm-{permission:?}")),
                            "Permitir",
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
            .child(title("Lo que Atic necesita de tu Mac"))
            .child(lead("Cada permiso dice para qué es. Te llevamos al lugar exacto de Ajustes y se marca solo cuando queda listo."))
            .child(rows)
            .when_some(self.reset_note.clone(), |el, (note, failed)| {
                el.child(div().text_size(px(12.)).text_color(hsla(if failed { AMBER } else { MUTED })).child(note))
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
                        .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(hsla(AMBER)).child("La pantalla empieza a funcionar cuando Atic se vuelve a abrir. Al reabrir, sigues aquí."))
                        .child(primary(
                            "onb-restart",
                            "Reabrir Atic",
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
                format!("{granted} de {} listos", Permission::ALL.len()),
                self.next_button(
                    match (self.only.is_some(), paste) {
                        (true, true) => "Listo",
                        (true, false) => "Más tarde",
                        (false, true) => "Continuar",
                        (false, false) => "Seguir sin pegar",
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
                    .text_size(px(12.))
                    .text_color(hsla(MUTED))
                    .child(svg().path("icons/lock.svg").size(px(13.)).text_color(hsla(MUTED)))
                    .child(format!("Permiso: {}", permission.title())),
            )
            .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child(format!("Enciende «{holder}» en la lista")))
            .child(lead("Si no aparece, arrastra este ícono a la lista de Ajustes y después enciende su switch. macOS puede pedirte tu contraseña o Touch ID."))
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
                    .child(div().text_size(px(12.)).text_color(hsla(TEXT).opacity(0.85)).child("Arrástralo a la lista de la izquierda")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(hsla(FAINT))
                    .child(div().size(px(8.)).rounded_full().bg(hsla(AMBER)))
                    .child("Esperando a macOS… se marca solo."),
            )
            .child(text_button(
                "onb-side-back",
                "Volver a la lista",
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
                    .text_size(px(10.5))
                    .bg(if ok { hsla(GREEN).opacity(0.14) } else { hsla(TEXT).opacity(0.05) })
                    .text_color(if ok { hsla(GREEN) } else { hsla(FAINT) })
                    .when(!ok, |el| el.line_through())
                    .child(format!("{} {}", if ok { "✓" } else { "✕" }, CAN[k]))
            });
            let line = |sign: &'static str, text: &'static str, color: u32| {
                div()
                    .flex()
                    .gap(px(6.))
                    .text_size(px(11.5))
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
                                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(i.title))
                                .child(div().text_size(px(10.5)).text_color(hsla(if engine == Engine::Groq { AMBER } else { MUTED })).child(i.chip)),
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
            "Puedes elegirlo después".to_string()
        } else if !ready {
            "Falta terminar de configurarlo".to_string()
        } else if self.engine == Some(Engine::Groq) || groq {
            "Dictar, transcribir y resumir: listo".to_string()
        } else {
            "Dictar y transcribir: listo · Resumir: sin configurar".to_string()
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(title("Elige el motor de voz e IA"))
            .child(lead("Atic lo usa para dictar, transcribir tus reuniones y resumirlas. Es opcional: puedes elegirlo después en Ajustes."))
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
                        .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(hsla(MUTED)).child("Para los resúmenes de reuniones hace falta un modelo de IA. Puedes sumar una llave de Groq solo para eso."))
                        .child(text_button(
                            "onb-groq-summary",
                            "Sumar Groq para resumir",
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
                self.next_button(if ready { "Continuar" } else { "Ahora no" }, cx),
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
                    .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(hsla(TEXT).opacity(0.85)).child("1. Crea una llave gratis en Groq (entras con Google o GitHub)."))
                    .child(text_button(
                        "onb-groq-console",
                        "Abrir console.groq.com ↗",
                        |_, _, cx| cx.open_url("https://console.groq.com/keys"),
                    )),
            )
            .child(key_row(self, "2. Cópiala y pégala aquí", SecretKind::GroqApiKey, cx))
            .child(
                div()
                    .px(px(16.))
                    .pb(px(10.))
                    .text_size(px(11.))
                    .text_color(hsla(FAINT))
                    .child("El audio se envía a Groq para transcribirlo. La llave queda en el llavero, no en un archivo."),
            )
    }

    fn local_box(&self, state: crate::local_stt::Download, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::local_stt::Download;
        let body = match state {
            Download::Ready => div().text_size(px(12.)).text_color(hsla(GREEN)).child("Listo: dictas y transcribes sin internet.").into_any_element(),
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
                    .child(div().text_size(px(11.)).text_color(hsla(MUTED)).child(format!(
                        "Bajando el modelo… {} % · {} de {} MB",
                        (f * 100.0).round(),
                        done / 1_000_000,
                        total / 1_000_000
                    )))
                    .into_any_element()
            }
            Download::Idle | Download::Failed(_) => {
                let failed = matches!(state, Download::Failed(_));
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(hsla(if failed { AMBER } else { MUTED })).child(match &state {
                        Download::Failed(error) => format!("No se pudo bajar: {error}"),
                        _ => "Parakeet v3 de NVIDIA: rápido en el procesador y bueno en español. Se baja una vez.".to_string(),
                    }))
                    .child(primary(
                        "onb-local-download",
                        if failed { "Reintentar" } else { "Bajar modelo · 671 MB" },
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
            .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(hsla(if mic { GREEN } else { AMBER })).child(if mic {
                "Listo para usar con el micrófono que diste en Permisos."
            } else {
                "Falta el permiso de micrófono para dictar."
            }))
            .when(!mic, |el| {
                el.child(text_button(
                    "onb-apple-mic",
                    "Dar el micrófono",
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
                        .text_size(px(11.))
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
                .rounded(px(7.))
                .flex()
                .items_center()
                .text_size(px(11.5))
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
            .child(title("¿Qué herramientas vas a usar?"))
            .child(lead("Toca las que quieras a mano: vuelan a la pill. Las demás quedan en «Más», a un toque."))
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
                                    .child(tab("onb-tab-strip", format!("En la pill · {}", strip.len()), !self.tab_more, false, cx))
                                    .child(tab("onb-tab-more", format!("En «Más» · {}", more.len()), self.tab_more, true, cx)),
                            )
                            .child(div().flex_1().text_size(px(11.)).text_color(hsla(FAINT)).text_right().child(if self.tab_more {
                                "La segunda pestaña: la abres con ⋯ en la pill"
                            } else {
                                "Las que se ven al pasar el cursor por la pill"
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
                                    .text_size(px(12.))
                                    .child(svg().path(pill_tools::icon(tool)).size(px(14.)).text_color(hsla(TEXT)))
                                    .child(tool_name(tool))
                            }))
                            .when(shown.is_empty(), |el| {
                                el.child(div().text_size(px(12.)).text_color(hsla(FAINT)).child(if self.tab_more {
                                    "Elegiste todas: no queda nada en «Más»."
                                } else {
                                    "Todavía no eliges ninguna: toca las de la rueda."
                                }))
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(footer(Some(self.back_button(cx)), "Lo cambias cuando quieras en Personalizar.", self.next_button("Continuar", cx)))
            .into_any_element()
    }

    fn tour(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let done = TOUR.load(Ordering::Relaxed);
        let tasks = TASKS;
        let all = tasks.iter().all(|(task, ..)| done & (*task as u8) != 0);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(title("Conoce la pill"))
            .child(lead("Está en el borde de la pantalla. Mira el ejemplo sobre la pill y hazlo: cada paso se marca al hacerlo."))
            .children(tasks.iter().map(|(task, name, hint)| {
                let ok = done & (*task as u8) != 0;
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .p(px(12.))
                    .rounded(px(12.))
                    .bg(if ok { hsla(GREEN).opacity(0.08) } else { hsla(TEXT).opacity(0.04) })
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(ok, |el| el.bg(hsla(GREEN)).child(svg().path("icons/check.svg").size(px(12.)).text_color(gpui::rgb(0x1a1a18))))
                            .when(!ok, |el| el.border_1().border_color(hsla(TEXT).opacity(0.3))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(*name))
                            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(*hint)),
                    )
            }))
            .child(div().flex_1())
            .child(footer(
                (self.only.is_none()).then(|| self.back_button(cx)),
                "",
                self.next_button(
                    match (all, self.only.is_some()) {
                        (true, true) => "Listo",
                        (true, false) => "Continuar",
                        (false, _) => "Saltar el tour",
                    },
                    cx,
                ),
            ))
            .into_any_element()
    }

    fn listo(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::dictation_settings::shortcut_label;
        let cfg = &self.cfg;
        let groq = self.groq_ready(cx);
        let voice = match cfg.dictation_backend.as_str() {
            "groq" if groq => "Groq dicta, transcribe tus reuniones y las resume. Lo cambias en Ajustes.".to_string(),
            "parakeet" if crate::local_stt::ready() => format!(
                "El motor local dicta y transcribe sin internet{}",
                if groq { "; Groq resume." } else { ". Para resumir, suma una llave de Groq en Ajustes." }
            ),
            "apple" if cfg!(target_os = "macos") => format!(
                "La voz de macOS dicta y transcribe{}",
                if groq { "; Groq resume." } else { ". Para resumir, suma una llave de Groq en Ajustes." }
            ),
            _ => "Falta elegir el motor de voz e IA: hazlo cuando quieras en Ajustes.".to_string(),
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(title("Todo listo"))
            .child(lead("Estos atajos funcionan desde cualquier app. Toca uno para cambiarlo: presiona la combinación nueva, o Esc para dejarlo como estaba."))
            .child(
                div().flex().flex_wrap().gap(px(8.)).children(KEYS.iter().enumerate().map(|(i, (what, get, _))| {
                    let recording = self.recording == Some(i);
                    let key = get(cfg);
                    let label = if recording {
                        "Presiona el atajo…".to_string()
                    } else if key.is_empty() {
                        "Sin atajo".to_string()
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
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .child(div().flex_1().text_size(px(12.)).text_color(hsla(TEXT).opacity(0.85)).child(*what))
                        .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(if recording { "Esc cancela" } else { "Cambiar" }))
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
                    .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(hsla(MUTED)).child(voice)),
            )
            .child(div().flex_1())
            .child(footer(
                Some(self.back_button(cx)),
                "Puedes volver a ver esto en Ajustes → General",
                primary("onb-finish", "Empezar a usar Atic", cx.listener(|view, _: &ClickEvent, window, cx| view.finish(window, cx))),
            ))
            .into_any_element()
    }
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
    let list = all.into_iter().fold(card(), |list, (task, title, hint)| {
        let on = running == Some(task as u8);
        list.child(crate::meetings::settings::row(
            title,
            hint,
            crate::meetings::settings::button(
                SharedString::from(format!("guide-{}", task as u8)),
                if on { "Mostrando…" } else { "Muéstrame" },
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
                "Detener la guía",
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
            .on_mouse_move(cx.listener(|view, event: &gpui::MouseMoveEvent, _, cx| {
                view.cursor = Some(event.position);
                cx.notify();
            }))
            .bg(crate::theme::window_bg())
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
                    .child(body),
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
        assert_eq!(more, vec![6, 8, 10]);
    }

    #[test]
    fn mac_pide_permisos_y_windows_no() {
        assert_eq!(steps().contains(&Step::Permisos), cfg!(target_os = "macos"));
        assert_eq!(steps().first(), Some(&Step::Hola));
        assert_eq!(steps().last(), Some(&Step::Listo));
    }
}
