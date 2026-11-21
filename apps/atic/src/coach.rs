//! Las guías de primera vez de las ventanas («coach marks»): la ventana se
//! oscurece, queda un hueco alrededor de lo que importa y una tarjeta dice qué
//! hacer ahí. Cada ventana declara su guía (`Guide`), marca sus anclas con
//! [`anchor`] y pone [`overlay`] al final de su vista raíz.
//!
//! - Se muestran una vez por guía (`config.json`: `coach_seen`), solo después de
//!   la bienvenida (`onboarding_done`) y nunca con `coach_off`.
//! - `COACH_FORCE=1` las muestra aunque estén vistas (para revisarlas); así no se
//!   anotan como vistas.
//! - Las anclas anotan dónde quedaron en el prepaint de cada cuadro. Atic Code
//!   dibuja partes en vistas cacheadas que no repiten su prepaint, así que lo
//!   anotado se conserva entre cuadros; al empezar la guía y al cambiar de marca
//!   la ventana se redibuja entera y se borran las anclas que ya no están.
//! - Mientras se ve, la capa tapa la ventana: ni clics ni teclas llegan abajo
//!   (Esc salta la guía; Enter o → avanza; ← vuelve).
//! - Lo que no es ventana (la mira, el estante) no se tapa: lleva un consejo
//!   de primera vez al lado ([`first_time`], [`tip`], [`tip_chip`]), con las
//!   mismas reglas de visto y apagado.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use atic_core::{AppDirs, Config};
use gpui::{
    canvas, deferred, div, fill, point, prelude::*, px, quad, size, svg, transparent_black, AnyElement, App, BorderStyle, Bounds,
    Corners, Edges, Element, ElementId, GlobalElementId, Hsla, InspectorElementId, LayoutId, MouseButton, Pixels, SharedString,
    Window, WindowId,
};
use gpui_m3::motion::{elapsed, request_animation_frame};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::{corner, Elevation, MotionSettings, Spring};

use crate::i18n::{t, tf};

/// Una marca de la guía: el ancla que se ilumina y sus textos (claves i18n).
pub struct Mark {
    pub anchor: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

/// La guía de una ventana o pestaña: su id (lo que se guarda como vista) y sus
/// marcas, de 3 a 5.
pub struct Guide {
    pub id: &'static str,
    pub marks: &'static [Mark],
}

/// El aire entre el ancla y el borde del hueco.
const PAD: f32 = 8.;
/// El radio del hueco (como mucho la mitad del lado corto).
const HOLE_RADIUS: f32 = 16.;
const CARD_W: f32 = 380.;
/// El alto que se le calcula a la tarjeta para elegir dónde cabe (no se mide).
const CARD_H: f32 = 240.;
const GAP: f32 = 16.;
const MARGIN: f32 = 16.;
/// El halo pulsa un rato al llegar a cada marca y después se queda quieto: un
/// pulso sin fin redibujaría la ventana entera todo el tiempo.
const PULSE_FOR: f32 = 1.8;
/// Lo que puede correrse un ancla (en px, por lado) sin que el hueco vuelva a
/// viajar desde cero.
const DRIFT: f32 = 24.0;

fn drift(a: Bounds<Pixels>, b: Bounds<Pixels>) -> f32 {
    let d = |x: Pixels, y: Pixels| (f32::from(x) - f32::from(y)).abs();
    d(a.origin.x, b.origin.x)
        .max(d(a.origin.y, b.origin.y))
        .max(d(a.size.width, b.size.width))
        .max(d(a.size.height, b.size.height))
}

/// Cuánto se persigue un ancla que se movió después de armar la capa.
const CHASE_FOR: f32 = 1.2;
const PULSE_PERIOD: f32 = 0.9;
/// Encima de menús y diálogos; por debajo del `scope_m3` que restaura el tema.
const PRIORITY: usize = 1_000;

// --- Preferencias -----------------------------------------------------------------

/// Lo de `config.json` que decide si se muestra una guía.
struct Prefs {
    seen: Vec<String>,
    off: bool,
    /// La bienvenida ya terminó: antes no hay guías de ventanas.
    ready: bool,
}

impl Prefs {
    fn of(cfg: &Config) -> Self {
        Self { seen: cfg.coach_seen.clone(), off: cfg.coach_off, ready: cfg.onboarding_done }
    }
}

static PREFS: Mutex<Option<Prefs>> = Mutex::new(None);

fn with_prefs<R>(f: impl FnOnce(&mut Prefs) -> R) -> R {
    let mut guard = PREFS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let prefs = guard.get_or_insert_with(|| {
        let cfg = AppDirs::new().map(|dirs| Config::load(&dirs.config_path())).unwrap_or_default();
        Prefs::of(&cfg)
    });
    f(prefs)
}

/// Lo que cambió en `config.json` (lo llama `config_watch` y Ajustes al guardar).
pub fn sync_config(cfg: &Config) {
    *PREFS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Prefs::of(cfg));
}

fn forced() -> bool {
    static FORCED: OnceLock<bool> = OnceLock::new();
    *FORCED.get_or_init(|| std::env::var("COACH_FORCE").is_ok_and(|v| v == "1"))
}

fn edit_config(apply: impl FnOnce(&mut Config)) {
    if let Ok(dirs) = AppDirs::new() {
        let cfg = crate::meetings::settings::save_config(&dirs.config_path(), apply);
        sync_config(&cfg);
    }
}

// --- Estado por ventana -----------------------------------------------------------

#[derive(Clone, Copy)]
struct Spot {
    bounds: Bounds<Pixels>,
    /// El cuadro de la ventana en que se anotó (`FRAMES`).
    frame: u64,
}

/// La guía que se está mostrando en una ventana.
struct Run {
    guide: &'static Guide,
    step: usize,
    opened: Instant,
    /// Cuándo empezó el viaje del hueco a la marca de ahora.
    moved: Instant,
    /// Dónde estaba el hueco al empezar el viaje.
    from: Option<Bounds<Pixels>>,
    /// Adónde va el hueco.
    aim: Option<Bounds<Pixels>>,
    /// El hueco dibujado en el último cuadro.
    shown: Option<Bounds<Pixels>>,
    /// En el próximo prepaint, borrar las anclas que no se anotaron en ese cuadro.
    sweep: bool,
}

thread_local! {
    static ANCHORS: RefCell<HashMap<(WindowId, &'static str), Spot>> = RefCell::default();
    static FRAMES: RefCell<HashMap<WindowId, u64>> = RefCell::default();
    static RUNS: RefCell<HashMap<WindowId, Run>> = RefCell::default();
    /// Cerradas en esta sesión (aunque no se haya podido guardar, o con `COACH_FORCE`).
    static DISMISSED: RefCell<HashSet<String>> = RefCell::default();
    /// Pedidas con «Ver guía de nuevo»: se muestran aunque las guías estén apagadas.
    static REPLAYS: RefCell<HashSet<String>> = RefCell::default();
    static INTERCEPTING: Cell<bool> = const { Cell::new(false) };
}

fn frame_of(window: WindowId) -> u64 {
    FRAMES.with(|frames| frames.borrow().get(&window).copied().unwrap_or(0))
}

fn spot(window: WindowId, anchor: &'static str) -> Option<Bounds<Pixels>> {
    ANCHORS.with(|anchors| anchors.borrow().get(&(window, anchor)).map(|spot| spot.bounds))
}

/// La misma posición del ancla salvo medio píxel: el redondeo no pide otro cuadro.
fn same_spot(a: Option<Bounds<Pixels>>, b: Option<Bounds<Pixels>>) -> bool {
    let near = |x: Pixels, y: Pixels| (f32::from(x) - f32::from(y)).abs() <= 0.5;
    match (a, b) {
        (Some(a), Some(b)) => {
            near(a.origin.x, b.origin.x)
                && near(a.origin.y, b.origin.y)
                && near(a.size.width, b.size.width)
                && near(a.size.height, b.size.height)
        }
        (None, None) => true,
        _ => false,
    }
}

fn running(window: WindowId) -> bool {
    RUNS.with(|runs| runs.borrow().contains_key(&window))
}

/// Sin guía en la ventana, sus anclas y su cuenta de cuadros sobran.
fn forget_window(window: WindowId) {
    ANCHORS.with(|anchors| anchors.borrow_mut().retain(|(id, _), _| *id != window));
    FRAMES.with(|frames| frames.borrow_mut().remove(&window));
}

/// Lo de las ventanas que ya se cerraron (con una guía a medias, por ejemplo).
fn forget_closed(cx: &App) {
    let open: HashSet<WindowId> = cx.windows().iter().map(|handle| handle.window_id()).collect();
    RUNS.with(|runs| runs.borrow_mut().retain(|id, _| open.contains(id)));
    ANCHORS.with(|anchors| anchors.borrow_mut().retain(|(id, _), _| open.contains(id)));
    FRAMES.with(|frames| frames.borrow_mut().retain(|id, _| open.contains(id)));
}

fn wanted(id: &str) -> bool {
    if DISMISSED.with(|dismissed| dismissed.borrow().contains(id)) {
        return false;
    }
    if forced() || REPLAYS.with(|replays| replays.borrow().contains(id)) {
        return true;
    }
    with_prefs(|prefs| prefs.ready && !prefs.off && !prefs.seen.iter().any(|seen| seen == id))
}

/// La guía quedó vista: no vuelve a salir (salvo con «Volver a mostrar»).
fn mark_seen(id: &'static str) {
    DISMISSED.with(|dismissed| dismissed.borrow_mut().insert(id.to_string()));
    REPLAYS.with(|replays| replays.borrow_mut().remove(id));
    if forced() {
        return;
    }
    with_prefs(|prefs| {
        if !prefs.seen.iter().any(|seen| seen == id) {
            prefs.seen.push(id.to_string());
        }
    });
    edit_config(|cfg| {
        if !cfg.coach_seen.iter().any(|seen| seen == id) {
            cfg.coach_seen.push(id.to_string());
        }
    });
}

/// Vuelve a mostrar una guía (para «Ver guía de nuevo»), aunque estén apagadas.
pub fn replay(id: &str, cx: &mut App) {
    DISMISSED.with(|dismissed| dismissed.borrow_mut().remove(id));
    REPLAYS.with(|replays| replays.borrow_mut().insert(id.to_string()));
    edit_config(|cfg| cfg.coach_seen.retain(|seen| seen != id));
    cx.refresh_windows();
}

/// Vuelve a mostrar todas las guías (Ajustes → General).
pub fn reset_all(cx: &mut App) {
    DISMISSED.with(|dismissed| dismissed.borrow_mut().clear());
    TIPS.with(|tips| tips.borrow_mut().clear());
    edit_config(|cfg| cfg.coach_seen.clear());
    cx.refresh_windows();
}

/// La herramienta que dibujaba la guía se cerró (la Pizarra y el Flip viven en la
/// ventana de la pill): sin esto, la pill seguiría tragándose las teclas.
pub fn close(id: &str) {
    let closed: Vec<WindowId> = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let closed = runs.iter().filter(|(_, run)| run.guide.id == id).map(|(window, _)| *window).collect();
        runs.retain(|_, run| run.guide.id != id);
        closed
    });
    for window in closed {
        forget_window(window);
    }
}

/// El botón «?» que vuelve a mostrar la guía de la ventana, para su barra de
/// arriba. `size` es el lado del botón, como los otros íconos de esa barra.
pub fn help_button(guide: &'static Guide, size: Pixels) -> AnyElement {
    let id = guide.id;
    gpui_m3::IconButton::new(ElementId::Name(SharedString::from(format!("coach-help-{id}"))), "icons/circle-help.svg")
        .size(size)
        .tooltip(t("pill.coach.replay"))
        .on_click(move |_, _, cx| replay(id, cx))
        .into_any_element()
}

// --- Consejos de primera vez ------------------------------------------------------

/// Lo que sigue a la vista un consejo desde que salió: una o dos capturas
/// seguidas, no para siempre.
const TIP_FOR: Duration = Duration::from_secs(90);

thread_local! {
    /// Los consejos de esta sesión: desde cuándo se ven (`None`: ya terminó).
    static TIPS: RefCell<HashMap<&'static str, Option<Instant>>> = RefCell::default();
}

/// Un consejo de primera vez para lo que no es ventana (la mira, el estante):
/// `true` solo la primera vez. Queda anotado como visto (`coach_seen`), se
/// apaga con las guías (`coach_off`) y no sale antes de la bienvenida.
pub fn first_time(id: &'static str) -> bool {
    if TIPS.with(|tips| tips.borrow().contains_key(id)) || !wanted(id) {
        return false;
    }
    TIPS.with(|tips| tips.borrow_mut().insert(id, Some(Instant::now())));
    mark_seen(id);
    true
}

/// Como [`first_time`], pero sigue a la vista `TIP_FOR` desde que salió: para
/// lo que se dibuja de nuevo en cada cuadro, como la mira.
pub fn tip(id: &'static str) -> bool {
    match TIPS.with(|tips| tips.borrow().get(id).copied()) {
        Some(since) => since.is_some_and(|at| at.elapsed() < TIP_FOR),
        None => first_time(id),
    }
}

/// Ya se usó lo que decía el consejo: no hace falta (ni ahora ni después).
pub fn tip_done(id: &'static str) {
    let known = TIPS.with(|tips| tips.borrow_mut().insert(id, None)).is_some();
    if !known && wanted(id) {
        mark_seen(id);
    }
}

/// El globito de un consejo sobre la pantalla: `tertiary_container`, el rol de
/// lo nuevo que conviene mirar, con su tecla (si tiene) en una tecla rellena.
pub fn tip_chip(scheme: Option<&gpui_m3::Scheme>, key: Option<&'static str>, text: impl Into<SharedString>) -> AnyElement {
    let (bg, fg, key_bg, key_fg) = match scheme {
        Some(s) => (s.tertiary_container, s.on_tertiary_container, s.tertiary, s.on_tertiary),
        None => (gpui::black().opacity(0.82), gpui::white(), gpui::white(), gpui::black()),
    };
    div()
        .max_w(px(420.))
        .min_h(px(32.))
        .pl(px(10.))
        .pr(px(14.))
        .py(px(6.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(corner::L)
        .bg(bg)
        .when_some(scheme, |el, s| el.shadow(Elevation::Level2.shadow(s)))
        .font_family(crate::fonts::UI)
        .text_color(fg)
        .child(svg().path("icons/sparkles.svg").size(px(16.)).flex_none().text_color(fg))
        .children(key.map(|key| {
            div()
                .flex_none()
                .min_w(px(22.))
                .h(px(22.))
                .px(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(corner::S)
                .bg(key_bg)
                .text_color(key_fg)
                .type_emphasized(type_scale::LABEL_LARGE)
                .child(key)
        }))
        .child(div().min_w_0().type_style(type_scale::BODY_MEDIUM).child(text.into()))
        .into_any_element()
}

// --- Navegación -------------------------------------------------------------------

/// Pasa a la marca siguiente; en la última, la guía termina.
fn advance(window_id: WindowId, window: &mut Window, cx: &mut App) {
    let finished = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let run = runs.get_mut(&window_id)?;
        if run.step + 1 >= run.guide.marks.len() {
            return Some(true);
        }
        run.step += 1;
        run.sweep = true;
        Some(false)
    });
    match finished {
        Some(true) => finish(window_id, window, cx),
        Some(false) => window.refresh(),
        None => {}
    }
}

fn back(window_id: WindowId, window: &mut Window) {
    let moved = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let run = runs.get_mut(&window_id)?;
        (run.step > 0).then(|| {
            run.step -= 1;
            run.sweep = true;
        })
    });
    if moved.is_some() {
        window.refresh();
    }
}

/// Termina (o salta) la guía de la ventana y la anota como vista.
fn finish(window_id: WindowId, window: &mut Window, _: &mut App) {
    let run = RUNS.with(|runs| runs.borrow_mut().remove(&window_id));
    if let Some(run) = run {
        mark_seen(run.guide.id);
    }
    forget_window(window_id);
    window.refresh();
}

/// Las teclas, antes que cualquier acción de la ventana: con la guía a la vista
/// no llegan a lo de abajo. Los atajos con modificadores sí pasan (cerrar la
/// ventana, salir).
fn intercept_keys(cx: &mut App) {
    if INTERCEPTING.with(|on| on.replace(true)) {
        return;
    }
    cx.intercept_keystrokes(|event, window, cx| {
        let window_id = window.window_handle().window_id();
        if !RUNS.with(|runs| runs.borrow().contains_key(&window_id)) {
            return;
        }
        let modifiers = &event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => finish(window_id, window, cx),
            "enter" | "right" | "space" => advance(window_id, window, cx),
            "left" => back(window_id, window),
            _ => {}
        }
        cx.stop_propagation();
    })
    .detach();
}

// --- Anclas -----------------------------------------------------------------------

/// Envuelve un elemento ancla y registra sus bounds en cada cuadro (por ventana).
/// No agrega una caja: el layout del elemento queda igual.
pub fn anchor(id: &'static str, el: impl IntoElement) -> AnyElement {
    Anchor { id, child: Some(el.into_any_element()) }.into_any_element()
}

struct Anchor {
    id: &'static str,
    child: Option<AnyElement>,
}

impl IntoElement for Anchor {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Anchor {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self.child.take().expect("coach::anchor se dibuja una vez");
        let layout = child.request_layout(window, cx);
        (layout, child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let window_id = window.window_handle().window_id();
        // Sin guía en la ventana no hace falta anotarla: al empezar una, la
        // ventana se redibuja entera y las anota todas (`overlay`).
        if !running(window_id) {
            child.prepaint(window, cx);
            return;
        }
        // Lo que se ve: dentro de una lista con scroll, solo la parte a la vista.
        let visible = bounds.intersect(&window.content_mask().bounds);
        let frame = frame_of(window_id);
        let key = (window_id, self.id);
        ANCHORS.with(|anchors| {
            let mut anchors = anchors.borrow_mut();
            if visible.size.width > px(4.) && visible.size.height > px(4.) {
                anchors.insert(key, Spot { bounds: visible, frame });
            } else {
                anchors.remove(&key);
            }
        });
        child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

// --- La capa ----------------------------------------------------------------------

fn lerp_bounds(from: Bounds<Pixels>, to: Bounds<Pixels>, p: f32) -> Bounds<Pixels> {
    let mix = |a: Pixels, b: Pixels| px(f32::from(a) + (f32::from(b) - f32::from(a)) * p);
    Bounds {
        origin: point(mix(from.origin.x, to.origin.x), mix(from.origin.y, to.origin.y)),
        size: size(mix(from.size.width, to.size.width).max(px(0.)), mix(from.size.height, to.size.height).max(px(0.))),
    }
}

/// Progreso 0 → 1 de un resorte `t` segundos después de empezar (1 ya asentado).
fn progress(spring: Spring, t: f32) -> (f32, bool) {
    if t >= spring.settle_time().as_secs_f32() {
        (1., false)
    } else {
        (spring.step(t.max(0.)), true)
    }
}

/// Dónde va la tarjeta: debajo del hueco si cabe, si no arriba, al lado, o abajo
/// al centro. Sin hueco, centrada en la ventana.
enum Place {
    /// `left` y `top`.
    Top(f32, f32),
    /// `left` y `bottom`.
    Bottom(f32, f32),
    Center,
}

fn place(hole: Option<Bounds<Pixels>>, viewport: gpui::Size<Pixels>, card_w: f32) -> Place {
    let Some(hole) = hole else {
        return Place::Center;
    };
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    let (left, top) = (f32::from(hole.origin.x), f32::from(hole.origin.y));
    let (right, bottom) = (left + f32::from(hole.size.width), top + f32::from(hole.size.height));
    let center_x = ((left + right) / 2. - card_w / 2.).clamp(MARGIN, (vw - card_w - MARGIN).max(MARGIN));
    let center_y = ((top + bottom) / 2. - CARD_H / 2.).clamp(MARGIN, (vh - CARD_H - MARGIN).max(MARGIN));
    if vh - bottom - GAP >= CARD_H + MARGIN {
        Place::Top(center_x, bottom + GAP)
    } else if top - GAP >= CARD_H + MARGIN {
        Place::Bottom(center_x, vh - top + GAP)
    } else if vw - right - GAP >= card_w + MARGIN {
        Place::Top(right + GAP, center_y)
    } else if left - GAP >= card_w + MARGIN {
        Place::Top(left - GAP - card_w, center_y)
    } else {
        Place::Bottom(((vw - card_w) / 2.).max(MARGIN), MARGIN)
    }
}

/// Si la guía no se ha visto (ni las guías están apagadas), devuelve la capa a
/// poner al final de la vista raíz (`.children(...)`). Si no, `None`. Llámala en
/// cada `render` de la raíz, sin condiciones, después de `theme::scope_m3`.
pub fn overlay(guide: &'static Guide, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let window_id = window.window_handle().window_id();
    if guide.marks.is_empty() || !wanted(guide.id) {
        let idle = RUNS.with(|runs| {
            let mut runs = runs.borrow_mut();
            if runs.get(&window_id).is_some_and(|run| run.guide.id == guide.id) {
                runs.remove(&window_id);
            }
            !runs.contains_key(&window_id)
        });
        if idle {
            forget_window(window_id);
        }
        return None;
    }
    let now = Instant::now();
    // Una guía a la vez por ventana: si ya hay otra, esta espera.
    let fresh = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let running = runs.get(&window_id).map(|run| run.guide.id);
        match running {
            Some(id) if id != guide.id => None,
            Some(_) => Some(false),
            None => {
                runs.insert(
                    window_id,
                    Run { guide, step: 0, opened: now, moved: now, from: None, aim: None, shown: None, sweep: false },
                );
                Some(true)
            }
        }
    })?;
    if fresh {
        forget_closed(cx);
        intercept_keys(cx);
        // Un cuadro entero (sin vistas cacheadas): anota todas las anclas y borra las viejas.
        window.on_next_frame(move |window, _| {
            RUNS.with(|runs| {
                if let Some(run) = runs.borrow_mut().get_mut(&window_id) {
                    run.sweep = true;
                }
            });
            window.refresh();
        });
    }

    let reduced = MotionSettings::get(cx).reduced;
    let scheme = *gpui_m3::Theme::of(cx);
    let viewport = window.viewport_size();
    let step = RUNS.with(|runs| runs.borrow().get(&window_id).map_or(0, |run| run.step)).min(guide.marks.len() - 1);
    let mark = &guide.marks[step];
    let target = spot(window_id, mark.anchor);
    let hole = target.map(|b| b.dilate(px(PAD)));

    // El hueco viaja de donde estaba a la marca nueva; sin hueco antes, se abre desde el centro.
    let Some((from, moved, opened)) = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let run = runs.get_mut(&window_id)?;
        // Un ancla que se corre de a poco (un campo que crece, una entrada
        // animada) no reinicia el viaje del hueco: si no, cada cuadro volvía a
        // pedir otro y la ventana no dejaba de redibujarse.
        if let (Some(aim), Some(new)) = (run.aim, hole) {
            if aim != new && drift(aim, new) < DRIFT {
                run.aim = hole;
                run.from = hole;
            }
        }
        if run.aim != hole {
            run.from = run.shown.or_else(|| hole.map(|h| Bounds::centered_at(h.center(), size(px(0.), px(0.)))));
            run.moved = now;
            run.aim = hole;
        }
        Some((run.from, run.moved, run.opened))
    }) else {
        return None;
    };
    let since_open = elapsed(opened, cx);
    let since_move = elapsed(moved, cx);
    let (fade, fading) = if reduced { (1., false) } else { progress(Spring::EFFECTS, since_open) };
    let (travel, traveling) = if reduced { (1., false) } else { progress(Spring::SPATIAL, since_move) };
    let (card_in, card_moving) = if reduced { (1., false) } else { progress(Spring::EFFECTS, since_move) };
    let (rise, rising) = if reduced { (1., false) } else { progress(Spring::SPATIAL, since_move) };
    let pulsing = !reduced && hole.is_some() && since_move < PULSE_FOR;
    let shown = match (from, hole) {
        (Some(from), Some(to)) => Some(lerp_bounds(from, to, travel)),
        (_, to) => to,
    };
    RUNS.with(|runs| {
        if let Some(run) = runs.borrow_mut().get_mut(&window_id) {
            run.shown = shown;
        }
    });
    if fading || traveling || card_moving || rising || pulsing {
        request_animation_frame(window);
    }
    let pulse = if pulsing { 0.5 - 0.5 * (since_move / PULSE_PERIOD * std::f32::consts::TAU).cos() } else { 0. };

    // La capa oscura con el hueco: un borde enorme alrededor del hueco, con el radio del
    // hueco por dentro (GPUI dibuja el borde con radio interior = radio − ancho).
    let scrim = Hsla { a: 0.6 * fade.clamp(0., 1.), ..scheme.scrim };
    let halo = Hsla { a: (0.95 - 0.45 * pulse) * fade.clamp(0., 1.), ..scheme.primary };
    let anchor_id = mark.anchor;
    // Solo al llegar a una marca se persigue un ancla que se mueve: si el ancla
    // vive en algo que se mueve siempre (un campo que crece, una animación de
    // entrada que se repite), pedir otro cuadro cada vez dejaba la ventana
    // redibujándose entera sin parar (en Windows, hasta colgarse).
    let settling = since_move < CHASE_FOR;
    let layer = canvas(
        move |_, window, _| {
            // Ya se anotaron las anclas de este cuadro (la capa va diferida, al final).
            let frame = frame_of(window_id);
            let sweep = RUNS.with(|runs| runs.borrow_mut().get_mut(&window_id).is_some_and(|run| std::mem::take(&mut run.sweep)));
            if sweep {
                ANCHORS.with(|anchors| anchors.borrow_mut().retain(|(id, _), spot| *id != window_id || spot.frame == frame));
            }
            // El ancla se movió (o apareció) después de armar la capa: otro
            // cuadro de la vista que la arma. Las anclas ya quedaron anotadas en
            // este, así que no hace falta redibujar las vistas cacheadas.
            if settling && !same_spot(spot(window_id, anchor_id), target) {
                request_animation_frame(window);
            }
            FRAMES.with(|frames| *frames.borrow_mut().entry(window_id).or_insert(0) += 1);
        },
        move |_, _, window, _| {
            let viewport = window.viewport_size();
            let Some(hole) = shown else {
                window.paint_quad(fill(Bounds { origin: point(px(0.), px(0.)), size: viewport }, scrim));
                return;
            };
            let radius = HOLE_RADIUS.min(f32::from(hole.size.width) / 2.).min(f32::from(hole.size.height) / 2.).max(0.);
            let reach = 3. * f32::from(viewport.width).max(f32::from(viewport.height)) + 64.;
            window.paint_quad(quad(
                hole.dilate(px(reach)),
                Corners::all(px(radius + reach)),
                transparent_black(),
                Edges::all(px(reach)),
                scrim,
                BorderStyle::default(),
            ));
            let grow = 3. + 5. * pulse;
            window.paint_quad(quad(
                hole.dilate(px(grow)),
                Corners::all(px(radius + grow)),
                transparent_black(),
                Edges::all(px(2.5)),
                halo,
                BorderStyle::default(),
            ));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();

    let total = guide.marks.len();
    let last = step + 1 == total;
    let card_w = CARD_W.min((f32::from(viewport.width) - 2. * MARGIN).max(200.));
    let dots = div().flex().items_center().gap(px(6.)).children((0..total).map(|i| {
        let on = i == step;
        div().h(px(6.)).w(px(if on { 18. } else { 6. })).rounded_full().bg(if on { scheme.primary } else { scheme.outline_variant })
    }));
    let next = gpui_m3::Button::new("coach-next", if last { t("pill.coach.done") } else { t("pill.coach.next") })
        .filled()
        .m3_size(gpui_m3::M3ButtonSize::S)
        .on_click(move |_, window, cx| advance(window_id, window, cx));
    let skip = gpui_m3::Button::new("coach-skip", t("pill.coach.skip"))
        .text()
        .m3_size(gpui_m3::M3ButtonSize::S)
        .on_click(move |_, window, cx| finish(window_id, window, cx));
    let card = div()
        .id("coach-card")
        .w(px(card_w))
        .p(px(24.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .rounded(corner::XL)
        .bg(scheme.surface_container_high)
        .shadow(Elevation::Level3.shadow(&scheme))
        .text_color(scheme.on_surface)
        .opacity((card_in * fade).clamp(0., 1.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .type_style(type_scale::LABEL_LARGE)
                        .text_color(scheme.on_surface_variant)
                        .child(tf("pill.coach.step", &[("n", &(step + 1)), ("total", &total)])),
                )
                .child(dots),
        )
        .child(div().type_emphasized(type_scale::TITLE_LARGE).text_color(scheme.on_surface).child(t(mark.title)))
        .child(div().type_style(type_scale::BODY_LARGE).text_color(scheme.on_surface_variant).child(t(mark.body)))
        .child(div().mt(px(4.)).flex().items_center().justify_between().gap(px(8.)).child(skip).child(next));
    // Entra subiendo unos píxeles en cada marca.
    let lift = (1. - rise) * 10.;
    let card_box = match place(hole, viewport, card_w) {
        Place::Top(x, y) => div().absolute().left(px(x)).top(px(y + lift)).child(card),
        Place::Bottom(x, y) => div().absolute().left(px(x)).bottom(px(y - lift)).child(card),
        Place::Center => div().absolute().top_0().left_0().size_full().flex().items_center().justify_center().child(div().mt(px(lift)).child(card)),
    };

    let root = div()
        .id(ElementId::Name(SharedString::from(format!("coach-{}", guide.id))))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        // Tapa la ventana: un clic en lo oscuro no hace nada (ni cierra la guía por accidente).
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(layer)
        .child(card_box);
    Some(deferred(root).with_priority(PRIORITY).into_any_element())
}
