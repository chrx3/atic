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
const WINDOW_H: f32 = 700.0;
/// Al lado de Ajustes del Sistema: angosta, para que entren las dos.
#[cfg(target_os = "macos")]
const SIDE_W: f64 = 380.0;
#[cfg(target_os = "macos")]
const SIDE_H: f64 = 540.0;
const TOP_H: f32 = 44.0;
const GUTTER: f32 = 32.0;
const GREEN: u32 = crate::theme::GREEN;
/// Las que entran en la tira; desde la séptima van a «Más».
const STRIP_LIMIT: usize = 6;
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
    /// Se abrió la rueda manteniendo la pill.
    Wheel = 4,
}

static TOUR: AtomicU8 = AtomicU8::new(0);

/// La pill avisa lo que se hizo; la bienvenida lo marca en el tour.
pub(crate) fn tour(task: Tour) {
    TOUR.fetch_or(task as u8, Ordering::Relaxed);
}

/// Mientras se eligen herramientas, la tira queda abierta para verlas llegar.
static TOOLS_STEP: AtomicBool = AtomicBool::new(false);

pub(crate) fn showing_tools() -> bool {
    TOOLS_STEP.load(Ordering::Relaxed)
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
                .child(chrome::logo(38.0 * scale))
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
        titlebar: Some(chrome::titlebar(if only.is_some() { "Atic · Permisos" } else { "Te damos la bienvenida a Atic" }, TOP_H)),
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
    /// Las herramientas elegidas, en el orden en que se tocaron.
    tools: Vec<usize>,
    /// La pestaña de abajo en Herramientas: «En «Más»» en vez de «En la pill».
    tab_more: bool,
    /// Dónde quedó la rueda de herramientas en la ventana: de ahí despegan.
    ring_at: Rc<Cell<Bounds<Pixels>>>,
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
        let cfg = Config::load(&path);
        TOUR.store(0, Ordering::Relaxed);
        Self {
            path,
            cfg,
            step: only.unwrap_or(Step::Hola),
            only,
            notice: None,
            backdrop: None,
            engine: Some(Engine::Groq),
            groq_summary: false,
            tools: Vec::new(),
            tab_more: false,
            ring_at: Rc::default(),
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

    fn go(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        self.leave_side(window);
        self.step = step;
        TOOLS_STEP.store(step == Step::Herramientas, Ordering::Relaxed);
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
        TOOLS_STEP.store(false, Ordering::Relaxed);
        set_ghost(None);
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
            }
        }
        if !self.tools.is_empty() {
            let ring: Vec<usize> = self.tools.iter().copied().take(STRIP_LIMIT).collect();
            let more: Vec<usize> = (0..pill_tools::COUNT)
                .filter(|t| !ring.contains(t))
                .collect();
            let (ring, more) = order_more(ring, more, &self.tools);
            if let Ok(mut slot) = LAYOUT.lock() {
                *slot = Some(pill_tools::layout(&ring, &more));
            }
        }
        cx.notify();
    }
}

/// En «Más», primero las elegidas que no cupieron, después el resto.
fn order_more(ring: Vec<usize>, more: Vec<usize>, chosen: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut first: Vec<usize> = chosen.iter().copied().filter(|t| more.contains(t)).collect();
    first.extend(more.into_iter().filter(|t| !chosen.contains(t)));
    (ring, first)
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
            .child(chrome::logo(64.))
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
                        .child(div().flex_1().text_size(px(12.)).text_color(hsla(AMBER)).child("La pantalla empieza a funcionar cuando Atic se vuelve a abrir. Al reabrir, sigues aquí."))
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
                            .child(chrome::logo(44.))
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
                        .child(div().flex_1().text_size(px(12.)).text_color(hsla(MUTED)).child("Para los resúmenes de reuniones hace falta un modelo de IA. Puedes sumar una llave de Groq solo para eso."))
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
                    .child(div().flex_1().text_size(px(12.)).text_color(hsla(TEXT).opacity(0.85)).child("1. Crea una llave gratis en Groq (entras con Google o GitHub)."))
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
                    .child(div().flex_1().text_size(px(12.)).text_color(hsla(if failed { AMBER } else { MUTED })).child(match &state {
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
            .child(div().flex_1().text_size(px(12.)).text_color(hsla(if mic { GREEN } else { AMBER })).child(if mic {
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
        let strip: Vec<usize> = self.tools.iter().copied().take(STRIP_LIMIT).collect();
        let more: Vec<usize> = (0..pill_tools::COUNT).filter(|t| !strip.contains(t)).collect();
        let (_, more) = order_more(strip.clone(), more, &self.tools);
        let buttons = (0..pill_tools::COUNT).map(|tool| {
            let angle = -std::f32::consts::FRAC_PI_2 + tool as f32 * std::f32::consts::TAU / pill_tools::COUNT as f32;
            let (cx_, cy) = (w / 2.0 + RING_R * angle.cos(), h / 2.0 + RING_R * angle.sin());
            let chosen = self.tools.contains(&tool);
            let in_strip = strip.contains(&tool);
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
                        .bg(if chosen { hsla(TEXT) } else { hsla(TEXT).opacity(0.07) })
                        .border_1()
                        .border_color(hsla(TEXT).opacity(if chosen { 0.0 } else { 0.1 }))
                        .when(chosen && !in_strip, |el| el.opacity(0.55))
                        .child(
                            svg()
                                .path(pill_tools::icon(tool))
                                .size(px(20.))
                                .text_color(if chosen { gpui::rgb(0x1a1a18).into() } else { hsla(TEXT) }),
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
                .bg(if on { hsla(TEXT) } else { hsla(TEXT).opacity(0.0) })
                .text_color(if on { gpui::rgb(0x1a1a18).into() } else { hsla(MUTED) })
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
                        .child(
                            div()
                                .absolute()
                                .left(px(w / 2.0 - RING_R))
                                .top(px(h / 2.0 - RING_R))
                                .size(px(RING_R * 2.0))
                                .rounded_full()
                                .border_1()
                                .border_dashed()
                                .border_color(hsla(TEXT).opacity(0.1)),
                        )
                        .child(div().absolute().left(px(w / 2.0 - 46.0)).top(px(h / 2.0 - 46.0)).child(chrome::logo(92.)))
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
                                    .child(tab("onb-tab-strip", format!("En la pill · {}/{STRIP_LIMIT}", strip.len()), !self.tab_more, false, cx))
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
        let tasks = [
            (Tour::Strip, "Pasa el cursor por la pill", "Arriba, en el borde: se abre la tira con tus herramientas."),
            (Tour::Tool, "Toca una herramienta", "Por ejemplo el portapapeles: lo último que copiaste."),
            (Tour::Wheel, "Mantén apretada la pill", "Sale la rueda con todo; suelta sobre lo que quieras."),
        ];
        let all = tasks.iter().all(|(task, ..)| done & (*task as u8) != 0);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(title("Conoce la pill"))
            .child(lead("Está en el borde de la pantalla. Pruébala de verdad: cada paso se marca al hacerlo."))
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
            .child(footer(Some(self.back_button(cx)), "", self.next_button(if all { "Continuar" } else { "Saltar el tour" }, cx)))
            .into_any_element()
    }

    fn listo(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::dictation_settings::shortcut_label;
        let cfg = &self.cfg;
        let keys = [
            (&cfg.dictation_shortcut, "Dictar (mantén apretado)"),
            (&cfg.clipboard_shortcut, "Portapapeles"),
            (&cfg.snippets_shortcut, "Fragmentos de texto"),
            (&cfg.screenshot_shortcut, "Captura"),
            (&cfg.board_shortcut, "Pizarra"),
            (&cfg.launcher_shortcut, "Lanzador"),
        ];
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
            .child(lead("Estos atajos funcionan desde cualquier app. Los cambias en Ajustes → Atajos."))
            .child(
                div().flex().flex_wrap().gap(px(8.)).children(keys.iter().filter(|(key, _)| !key.is_empty()).map(|(key, what)| {
                    div()
                        .w(px(360.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(10.))
                        .bg(hsla(TEXT).opacity(0.05))
                        .child(
                            div()
                                .px(px(7.))
                                .py(px(3.))
                                .rounded(px(6.))
                                .bg(hsla(TEXT).opacity(0.1))
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(shortcut_label(key.as_str())),
                        )
                        .child(div().text_size(px(12.)).text_color(hsla(TEXT).opacity(0.85)).child(*what))
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
                    .child(div().flex_1().text_size(px(12.)).text_color(hsla(MUTED)).child(voice)),
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
                    .overflow_y_scroll()
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
    fn en_mas_van_primero_las_elegidas_que_no_cupieron() {
        let chosen = [3, 1, 4, 0, 2, 5, 9, 7];
        let ring: Vec<usize> = chosen.iter().copied().take(STRIP_LIMIT).collect();
        let more: Vec<usize> = (0..pill_tools::COUNT).filter(|t| !ring.contains(t)).collect();
        let (ring, more) = order_more(ring, more, &chosen);
        assert_eq!(ring, vec![3, 1, 4, 0, 2, 5]);
        assert_eq!(&more[..2], &[9, 7]);
        assert_eq!(more.len(), pill_tools::COUNT - STRIP_LIMIT);
    }

    #[test]
    fn mac_pide_permisos_y_windows_no() {
        assert_eq!(steps().contains(&Step::Permisos), cfg!(target_os = "macos"));
        assert_eq!(steps().first(), Some(&Step::Hola));
        assert_eq!(steps().last(), Some(&Step::Listo));
    }
}
