//! Sistema como notch: centro de control.
//!
//! Arriba, lo de todos los días: volumen y brillo en dos sliders gruesos, y
//! silencio, café, bloquear y suspender. Debajo, tres pestañas que se abren
//! a pedido (al abrir el notch quedan cerradas, para que sea compacto):
//! - **Audio**: la salida (parlantes, audífonos…), quién usa el micrófono o
//!   la cámara, y el volumen de cada app que tiene sonido.
//! - **Pantallas**: el brillo, la resolución y la frecuencia de cada pantalla,
//!   cuál es la principal, duplicar o extender, el modo oscuro y conectar una
//!   pantalla inalámbrica. Un cambio de resolución vuelve solo a la anterior
//!   si no se confirma en 15 s (por si la pantalla quedó en negro).
//! - **Rendimiento**: CPU, GPU, memoria y red con su último minuto como una
//!   tira que corre hacia la izquierda, temperaturas, discos y batería.
//! - **Procesos**: CPU, RAM y lo que más consume, para cerrarlo. Un buscador
//!   filtra por nombre, y mientras el cursor está sobre la lista el orden se
//!   congela: las filas no cambian de lugar justo antes del clic.
//!
//! Lo que no se deshace con un clic (bloquear, suspender, forzar el cierre)
//! pide confirmación: el primer clic arma el botón y el segundo lo ejecuta;
//! si no se confirma en 3 s, se desarma solo.
//!
//! Lo que habla con Windows está en `os.rs`, en su propio hilo.

#[cfg(target_os = "macos")]
mod mac;
pub(crate) mod os;
mod space;
mod space_view;
#[cfg(windows)]
mod win;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    actions, canvas, div, img, prelude::*, px, rgb, svg, Animation, AnimationExt, AnyElement, App, Entity,
    Bounds, ClickEvent,
    Context, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, ScrollWheelEvent, SharedString, Window,
};

use crate::clipboard::BAND_H;
use crate::text_input::{self, TextInput};
use crate::hover::{hover_fx, pin_button, HoverExt};
use crate::anim::Tween;
use os::{Backend, Cmd, Display, DisplayId, Mode, Snapshot, Vitals, Want};

const MARK_GAP: f32 = 40.0;
const SIDE: f32 = 12.0;
const SLIDER_H: f32 = 40.0;
const MINI_H: f32 = 22.0;
const GAP: f32 = 8.0;
const TILE_H: f32 = 58.0;
const TABS_H: f32 = 30.0;
const ROW_H: f32 = 34.0;
const PROCS_HEADER_H: f32 = 28.0;
/// El buscador de Procesos.
const SEARCH_H: f32 = 30.0;
const NOTICE_H: f32 = 22.0;
/// El título de una sección dentro de una pestaña («Salida», «Apps»).
const SECTION_H: f32 = 22.0;
/// Filas visibles en Audio y Procesos antes de cortar.
const MAX_ROWS: usize = 6;
/// Cada cuánto se relee el estado con el notch abierto (otra app puede
/// cambiar el volumen). Con Procesos abierto, la CPU se mide entre lecturas.
const REFRESH_EVERY: Duration = Duration::from_millis(1500);
const WHEEL_STEP: f32 = 0.05;
/// Lo que dura armado un botón que pide confirmación.
const ARM_FOR: Duration = Duration::from_secs(3);
const NOTICE_FOR: Duration = Duration::from_secs(4);

/// Lecturas que guarda Rendimiento: ~50 s a una cada 1,5 s.
const HISTORY: usize = 34;
/// Lo que tarda la tira en correrse un lugar cuando llega una lectura.
const SHIFT_FOR: Duration = Duration::from_millis(620);
/// Lo que tardan los números en llegar a la lectura nueva.
const DIAL_FOR: Duration = Duration::from_millis(700);
const STRIP_H: f32 = 20.0;
/// De una barra de la tira a la siguiente, y su ancho.
const PITCH: f32 = 4.0;
const BAR_W: f32 = 2.5;
/// Las columnas de la derecha de Rendimiento: el valor y su detalle.
const VALUE_W: f32 = 62.0;
const DETAIL_W: f32 = 58.0;
const FOOT_H: f32 = 28.0;
/// Pantallas: la barra de arriba, la línea del modo, las opciones y la
/// confirmación de una resolución nueva.
const TOOLBAR_H: f32 = 30.0;
const SUB_H: f32 = 26.0;
const CHIPS_H: f32 = 30.0;
const KEEP_H: f32 = 30.0;
const DIVIDER_H: f32 = 13.0;
/// Lo que espera una resolución nueva antes de volver a la anterior.
const KEEP_FOR: Duration = Duration::from_secs(15);
const MAX_CHIPS: usize = 5;
/// Los colores de cada medida, los mismos que Procesos (CPU ámbar, RAM azul).
const CPU_TINT: u32 = 0xe8b04b;
const GPU_TINT: u32 = 0xb59cf0;
const RAM_TINT: u32 = 0x6fa3e0;
const DOWN_TINT: u32 = 0x5fc79a;
const UP_TINT: u32 = 0xc9c9c0;
/// Liberar espacio: por ahora solo en macOS (`space.rs`).
const SPACE: bool = cfg!(target_os = "macos");
const DISK_KEYS: [&str; 4] = ["disk-0", "disk-1", "disk-2", "disk-3"];

actions!(system_panel, [Dismiss, ToggleMute, FindApp]);

const KEY_CONTEXT: &str = "SystemPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("m", ToggleMute, context),
        KeyBinding::new(&crate::keymap::primary("ctrl-f"), FindApp, context),
    ]);
}

pub enum SystemEvent {
    Close,
    /// Bloquear o suspender, ya confirmado: el notch se cierra antes.
    Action(crate::launcher::Action),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Audio,
    Displays,
    Vitals,
    Procs,
}

/// Rendimiento: las últimas lecturas (0 – 1, o bytes por segundo en la red).
#[derive(Default)]
struct History {
    cpu: VecDeque<f32>,
    gpu: VecDeque<f32>,
    ram: VecDeque<f32>,
    down: VecDeque<f32>,
    up: VecDeque<f32>,
    /// Cuándo llegó la última: la tira se corre un lugar desde ahí.
    at: Option<Instant>,
}

impl History {
    fn push(&mut self, v: &Vitals) {
        let ram = if v.ram_total > 0 { v.ram_used as f32 / v.ram_total as f32 } else { 0.0 };
        let fresh = [
            (&mut self.cpu, v.cpu / 100.0),
            (&mut self.gpu, v.gpu.unwrap_or(0.0) / 100.0),
            (&mut self.ram, ram),
            (&mut self.down, v.net_down as f32),
            (&mut self.up, v.net_up as f32),
        ];
        for (list, value) in fresh {
            list.push_back(value);
            while list.len() > HISTORY + 1 {
                list.pop_front();
            }
        }
        self.at = Some(Instant::now());
    }

    /// De 0 a 1 mientras la tira se corre un lugar (sale del estado: abrir
    /// la pestaña a mitad de camino la muestra donde va).
    fn shift(&self, now: Instant, reduce: bool) -> f32 {
        match self.at {
            Some(at) if !reduce => {
                let t = now.saturating_duration_since(at).as_secs_f32() / SHIFT_FOR.as_secs_f32();
                crate::anim::ease_smooth_out(t.clamp(0.0, 1.0))
            }
            _ => 1.0,
        }
    }
}

/// Una resolución recién puesta, que vuelve a la anterior si no se confirma.
#[derive(Clone)]
struct Trial {
    id: DisplayId,
    before: Mode,
    after: Mode,
    at: Instant,
}

/// Qué slider es: cada uno guarda dónde quedó para pasar del cursor a un
/// valor.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Volume,
    Brightness(DisplayId),
    App(u32),
}

/// Lo que está armado esperando el segundo clic.
#[derive(Clone, PartialEq)]
enum Arm {
    Lock,
    Sleep,
    Force(String),
}

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    track: Hsla,
    fill: Hsla,
    ink: Hsla,
    amber: Hsla,
    danger: Hsla,
}

pub struct SystemPanel {
    focus: FocusHandle,
    backend: Backend,
    state: Snapshot,
    /// Se relee mientras el notch muestra Sistema.
    pub active: bool,
    tab: Option<Tab>,
    /// El slider que se arrastra; `true` si es uno de los grandes (la misma
    /// pantalla tiene uno grande y uno chico, cada uno con su medida).
    drag: Option<(Key, bool)>,
    bounds: RefCell<HashMap<(Key, bool), Rc<Cell<Bounds<Pixels>>>>>,
    armed: Option<(Arm, Instant)>,
    notice: Option<(SharedString, bool, Instant)>,
    icons: Arc<std::sync::Mutex<HashMap<PathBuf, Option<Arc<RenderImage>>>>>,
    privacy: crate::privacy::Privacy,
    pending_icons: HashSet<PathBuf>,
    pub pinned: bool,
    /// El vistazo de la tira lo está mostrando: mide los procesos aunque la
    /// pestaña no esté abierta.
    peek: bool,
    /// Buscar una app de Procesos por nombre.
    search: Entity<TextInput>,
    _search_changed: gpui::Subscription,
    /// Lo buscado, ya plegado (sin tildes ni mayúsculas).
    query: String,
    /// El cursor está sobre la lista de Procesos: el orden queda quieto.
    list_hovered: bool,
    /// El orden de la lista la última vez que se movió (por `stem`).
    order: Vec<String>,
    colors: Colors,
    /// Cuántas veces se cambió de pestaña y hacia qué lado (-1 izquierda, 1
    /// derecha): el contenido nuevo entra deslizándose desde ese lado.
    tab_switch: (u64, f32),
    history: History,
    /// Los números de Rendimiento, que se deslizan hasta cada lectura.
    dials: HashMap<&'static str, Tween>,
    /// La pantalla con sus resoluciones a la vista.
    open_display: Option<DisplayId>,
    /// La flecha de cada pantalla: 0 cerrada, 0,5 de vuelta (abierta).
    chevrons: HashMap<DisplayId, Tween>,
    trial: Option<Trial>,
    /// «Reducir movimiento» del sistema, leído al abrir.
    reduce_motion: bool,
    /// Liberar espacio, abierto en lugar del panel.
    space: Option<space_view::SpaceView>,
    /// Cuántas veces se volvió de Liberar espacio: el panel entra de nuevo.
    space_closes: u64,
}

impl EventEmitter<SystemEvent> for SystemPanel {}

impl Focusable for SystemPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SystemPanel {
    pub fn new(privacy: crate::privacy::Privacy, backend: Backend, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(
                crate::i18n::t("pill.system.searchApp"),
                rgb(0xf0f0ea).into(),
                rgb(0x6e6e66).into(),
                rgb(0xf0f0ea).into(),
                cx,
            )
        });
        let _search_changed = cx.subscribe(&search, |panel, search, _: &text_input::Changed, cx| {
            panel.query = crate::clipboard::fold(search.read(cx).text().trim());
            cx.notify();
        });
        Self {
            search,
            _search_changed,
            query: String::new(),
            list_hovered: false,
            order: Vec::new(),
            privacy,
            focus: cx.focus_handle(),
            backend,
            state: Snapshot::default(),
            active: false,
            // `PILL_SYSTEM_TAB=audio|pantallas|rendimiento|espacio|procesos`: abre esa pestaña,
            // para revisar el diseño con una captura.
            tab: match std::env::var("PILL_SYSTEM_TAB").as_deref() {
                Ok("audio") => Some(Tab::Audio),
                Ok("pantallas") => Some(Tab::Displays),
                Ok("rendimiento" | "espacio") => Some(Tab::Vitals),
                Ok("procesos") => Some(Tab::Procs),
                _ => None,
            },
            drag: None,
            bounds: Default::default(),
            armed: None,
            notice: None,
            icons: Default::default(),
            pending_icons: HashSet::new(),
            pinned: false,
            peek: false,
            colors: Colors {
                text: rgb(0xf0f0ea).into(),
                muted: rgb(0x9a9a90).into(),
                faint: rgb(0x6e6e66).into(),
                track: gpui::white().opacity(0.09),
                fill: rgb(0xe8e8e0).into(),
                ink: rgb(0x1a1a18).into(),
                amber: rgb(0xe8b04b).into(),
                danger: rgb(0xe06c6c).into(),
            },
            tab_switch: (0, 0.),
            history: History::default(),
            dials: HashMap::new(),
            open_display: None,
            chevrons: HashMap::new(),
            trial: None,
            reduce_motion: false,
            space: None,
            space_closes: 0,
        }
    }

    /// Al abrir: leer el estado y seguir leyéndolo mientras esté abierto.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        let first = !self.active;
        self.active = true;
        self.armed = None;
        if first {
            self.space = None;
            // Un historial de la vez anterior tendría un hueco al medio.
            self.history = History::default();
            self.dials.clear();
            self.reduce_motion = crate::code::style::system_reduced_motion();
        }
        // `PILL_SYSTEM_TAB=espacio`: Liberar espacio abierto, para revisarlo.
        if first && SPACE && std::env::var("PILL_SYSTEM_TAB").as_deref() == Ok("espacio") {
            self.open_space(cx);
        }
        self.refresh(cx);
        if first {
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(REFRESH_EVERY).await;
                let go = this
                    .update(cx, |panel, cx| {
                        if panel.active && panel.drag.is_none() {
                            panel.refresh(cx);
                        }
                        panel.active
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            })
            .detach();
        }
    }

    fn want(&self) -> Want {
        Want {
            audio: self.tab == Some(Tab::Audio),
            procs: self.tab == Some(Tab::Procs) || self.peek,
            displays: self.tab == Some(Tab::Displays),
            vitals: self.tab == Some(Tab::Vitals),
        }
    }

    /// El vistazo de Sistema empieza (lee y sigue leyendo) o termina.
    /// `keep` deja la lectura andando: el panel quedó abierto.
    pub fn set_peek(&mut self, on: bool, keep: bool, cx: &mut Context<Self>) {
        if on == self.peek {
            return;
        }
        self.peek = on;
        if on {
            self.reset(cx);
        } else if !keep {
            self.active = false;
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.state
    }

    /// El ícono de una app para el vistazo (se carga aparte y repinta).
    pub fn app_image(&mut self, path: Option<&PathBuf>, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        self.icon(path, cx)
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let reply = self.backend.read(self.want());
        cx.spawn(async move |this, cx| {
            if let Ok(snapshot) = reply.await {
                let _ = this.update(cx, |panel, cx| {
                    // Mientras se arrastra manda el dedo, no la lectura.
                    if panel.drag.is_none() {
                        panel.state = snapshot;
                        if let Some(trial) = panel.trial.as_ref() {
                            // Hasta que el SO termine de cambiarla, se ve la elegida.
                            let (id, after) = (trial.id, trial.after.clone());
                            if let Some(display) = panel.state.displays.iter_mut().find(|d| d.id == id) {
                                display.mode = Some(after);
                            }
                        }
                        panel.take_vitals();
                    }
                    panel.take_outcomes();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Lo que respondió el hilo al cerrar apps, como aviso en el pie.
    fn take_outcomes(&mut self) {
        let outcomes: Vec<_> = self
            .backend
            .outcomes
            .lock()
            .map(|mut o| o.drain(..).collect())
            .unwrap_or_default();
        if let Some(last) = outcomes.into_iter().last() {
            let (text, error) = match last {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            self.notice = Some((text.into(), error, Instant::now()));
        }
    }

    /// Las apps de Procesos que se ven: las que coinciden con la búsqueda y,
    /// con el cursor encima, en el orden de antes (las nuevas al final).
    fn shown_apps(&self) -> Vec<os::App> {
        let Some(procs) = self.state.procs.as_ref() else {
            return Vec::new();
        };
        let order = (self.list_hovered && !self.order.is_empty()).then_some(self.order.as_slice());
        pick_apps(&procs.apps, &self.query, order)
    }

    fn searching(&self) -> bool {
        !self.query.is_empty()
    }

    fn rows(&self) -> usize {
        match self.tab {
            None => 0,
            Some(Tab::Audio) => self.state.audio.len().clamp(1, MAX_ROWS) + self.state.outputs.len(),
            Some(Tab::Displays) => self.state.displays.len().max(1),
            Some(Tab::Vitals) => 0,
            Some(Tab::Procs) => self.shown_apps().len().clamp(1, MAX_ROWS),
        }
    }

    pub fn desired_height(&self) -> f32 {
        if self.space_open() {
            return BAND_H + 4.0 + self.space_height() + 4.0;
        }
        let base = BAND_H + 4.0 + SLIDER_H * 2.0 + GAP + 12.0 + TILE_H + 10.0 + TABS_H + SIDE;
        let content = match self.tab {
            None => 0.0,
            Some(Tab::Procs) => GAP + SEARCH_H + PROCS_HEADER_H + self.rows() as f32 * ROW_H,
            Some(Tab::Audio) => {
                let uses = if self.privacy.uses().is_empty() { 0.0 } else { ROW_H };
                GAP + uses + SECTION_H * 2.0 + self.rows() as f32 * ROW_H
            }
            Some(Tab::Displays) => GAP + self.displays_height(),
            Some(Tab::Vitals) => GAP + self.vitals_height(),
        };
        let notice = if self.notice_visible() { NOTICE_H } else { 0.0 };
        base + content + notice
    }

    fn notice_visible(&self) -> bool {
        self.notice
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() < NOTICE_FOR)
    }

    // --- Cambios ---------------------------------------------------------------

    fn value_of(&self, key: Key) -> Option<f32> {
        match key {
            Key::Volume => Some(if self.state.muted { 0.0 } else { self.state.volume }),
            Key::Brightness(id) => self
                .state
                .displays
                .iter()
                .find(|d| d.id == id)
                .and_then(|d| d.brightness),
            Key::App(pid) => self
                .state
                .audio
                .iter()
                .find(|a| a.pid == pid)
                .map(|a| a.volume),
        }
    }

    fn set(&mut self, key: Key, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.0);
        match key {
            Key::Volume => {
                self.state.volume = value;
                self.backend.send(Cmd::Volume(value));
                // Como en Windows: mover el volumen quita el silencio.
                if self.state.muted && value > 0.0 {
                    self.state.muted = false;
                    self.backend.send(Cmd::Mute(false));
                }
            }
            Key::Brightness(id) => {
                let Some(display) = self.state.displays.iter_mut().find(|d| d.id == id) else {
                    return;
                };
                if display.brightness.is_none() {
                    return;
                }
                display.brightness = Some(value);
                self.backend.send(Cmd::Brightness(id, value));
            }
            Key::App(pid) => {
                if let Some(app) = self.state.audio.iter_mut().find(|a| a.pid == pid) {
                    app.volume = value;
                }
                self.backend.send(Cmd::AppVolume(pid, value));
            }
        }
        cx.notify();
    }

    fn bounds_of(&self, key: Key, big: bool) -> Rc<Cell<Bounds<Pixels>>> {
        self.bounds.borrow_mut().entry((key, big)).or_default().clone()
    }

    /// Del cursor al valor: el centro de la punta redonda del relleno queda
    /// bajo el cursor.
    fn value_at(&self, key: Key, big: bool, x: Pixels) -> f32 {
        let bounds = self.bounds_of(key, big).get();
        let h = f32::from(bounds.size.height);
        let w = f32::from(bounds.size.width).max(h + 1.0);
        (f32::from(x - bounds.origin.x) - h / 2.0) / (w - h)
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.state.muted = !self.state.muted;
        self.backend.send(Cmd::Mute(self.state.muted));
        cx.notify();
    }

    fn toggle_awake(&mut self, cx: &mut Context<Self>) {
        self.state.awake = !self.state.awake;
        self.backend.send(Cmd::Awake(self.state.awake));
        cx.notify();
    }

    fn is_armed(&self, arm: &Arm) -> bool {
        self.armed
            .as_ref()
            .is_some_and(|(armed, at)| armed == arm && at.elapsed() < ARM_FOR)
    }

    /// Primer clic: arma. Segundo clic (dentro de 3 s): devuelve `true`.
    fn confirm(&mut self, arm: Arm, cx: &mut Context<Self>) -> bool {
        if self.is_armed(&arm) {
            self.armed = None;
            cx.notify();
            return true;
        }
        self.armed = Some((arm, Instant::now()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ARM_FOR).await;
            let _ = this.update(cx, |panel, cx| {
                if panel
                    .armed
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed() >= ARM_FOR)
                {
                    panel.armed = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        false
    }

    fn pick_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let next = if self.tab == Some(tab) { None } else { Some(tab) };
        // De una pestaña a otra: el contenido entra desde el lado de la nueva.
        if let (Some(from), Some(to)) = (self.tab, next) {
            let side = if (to as u8) > (from as u8) { 1. } else { -1. };
            self.tab_switch = (self.tab_switch.0 + 1, side);
        }
        if self.tab == Some(Tab::Vitals) && next != Some(Tab::Vitals) {
            self.history = History::default();
            self.dials.clear();
        }
        if next != Some(Tab::Displays) {
            self.open_display = None;
        }
        self.tab = next;
        self.list_hovered = false;
        self.refresh(cx);
        cx.notify();
    }

    fn icon(&mut self, path: Option<&PathBuf>, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        let path = path?;
        if let Some(hit) = self.icons.lock().ok()?.get(path) {
            return hit.clone();
        }
        // Sacar el ícono del ejecutable puede tardar: en otro hilo, y la fila
        // se repinta cuando llega.
        if self.pending_icons.insert(path.clone()) {
            let (path, icons) = (path.clone(), self.icons.clone());
            cx.spawn(async move |this, cx| {
                let found = cx
                    .background_spawn({
                        let path = path.clone();
                        async move { crate::app_icon::icon_for(&path) }
                    })
                    .await;
                icons.lock().map(|mut i| i.insert(path, found)).ok();
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        None
    }

    // --- Dibujo -------------------------------------------------------------

    fn render_band(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, muted, faint) = (self.colors.text, self.colors.muted, self.colors.faint);
        let battery = self.state.battery.map(|(percent, plugged)| {
            let icon = if plugged {
                "icons/battery-charging.svg"
            } else if percent >= 70 {
                "icons/battery-full.svg"
            } else if percent >= 30 {
                "icons/battery-medium.svg"
            } else {
                "icons/battery-low.svg"
            };
            let label = if plugged {
                crate::i18n::tf("pill.system.pluggedIn", &[("percent", &percent)])
            } else {
                format!("{percent} %")
            };
            (icon, label, percent < 15 && !plugged)
        });
        div()
            .h(px(BAND_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pr(px(SIDE - 4.))
            .child(
                div()
                    .id("system-mark")
                    .w(px(MARK_GAP))
                    .h_full()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SystemEvent::Close))),
            )
            .child(div().text_size(px(12.)).child(crate::i18n::t("pill.system.title")))
            .child(div().flex_1())
            .children(battery.map(|(icon, label, low)| {
                let color = if low { self.colors.danger } else { muted };
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(11.))
                    .text_color(color)
                    .child(svg().path(icon).size(px(14.)).text_color(color))
                    .child(label)
            }))
            .child(pin_button(
                "system-pin",
                self.pinned,
                text,
                faint,
                cx.listener(|panel, _: &ClickEvent, _, cx| {
                    panel.pinned = !panel.pinned;
                    cx.notify();
                }),
            ))
    }

    /// Un slider de relleno: crece desde la izquierda y su punta redonda es
    /// la perilla. `icon` va dentro, sobre el relleno (solo en los gruesos).
    fn slider(
        &self,
        key: Key,
        height: f32,
        icon: Option<&'static str>,
        label: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let c = &self.colors;
        let big = height >= SLIDER_H;
        let store = self.bounds_of(key, big);
        let value = self.value_of(key);
        let enabled = value.is_some();
        let v = value.unwrap_or(0.0).clamp(0.0, 1.0);
        let dragging = self.drag == Some((key, big));
        let width = f32::from(store.get().size.width);
        let fill = if width > height {
            px(height + (width - height) * v)
        } else {
            px(height)
        };
        let size = if big { "big" } else { "mini" };
        let id: SharedString = match key {
            Key::Volume => format!("slider-volume-{size}").into(),
            Key::Brightness(DisplayId::Internal) => format!("slider-bright-internal-{size}").into(),
            Key::Brightness(DisplayId::Monitor(h)) => format!("slider-bright-{h}-{size}").into(),
            Key::App(pid) => format!("slider-app-{pid}-{size}").into(),
        };
        div()
            .id(id)
            .relative()
            .h(px(height))
            .rounded(px(height / 2.0))
            .bg(c.track)
            .overflow_hidden()
            .when(enabled, |el| el.cursor_pointer())
            .child(
                canvas(move |bounds, _, _| store.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .when(enabled, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(fill)
                        .rounded(px(height / 2.0))
                        .bg(if dragging { gpui::white() } else { c.fill }),
                )
            })
            .children(icon.map(|icon| {
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size(px(height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(icon)
                            .size(px(height * 0.45))
                            .text_color(if enabled { c.ink } else { c.faint }),
                    )
            }))
            .children(label.map(|label| {
                div()
                    .absolute()
                    .right(px(14.))
                    .top_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(if enabled && v > 0.88 { c.ink } else { c.muted })
                    .child(label)
            }))
            .when(enabled, |el| {
                el.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |panel, event: &MouseDownEvent, _, cx| {
                        panel.drag = Some((key, big));
                        let value = panel.value_at(key, big, event.position.x);
                        panel.set(key, value, cx);
                    }),
                )
                .on_scroll_wheel(cx.listener(move |panel, event: &ScrollWheelEvent, _, cx| {
                    let dy = f32::from(event.delta.pixel_delta(px(20.)).y);
                    if dy != 0.0 {
                        let current = panel.value_of(key).unwrap_or(0.0);
                        panel.set(key, current + WHEEL_STEP * dy.signum(), cx);
                    }
                }))
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn tile(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        active: Option<Hsla>,
        armed: bool,
        armed_label: &'static str,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &self.colors;
        let bg = if armed { Some(c.danger) } else { active };
        let fg = if bg.is_some() { c.ink } else { c.text };
        let (track, muted, ink) = (c.track, c.muted, c.ink);
        let click = cx.listener(move |panel, _: &ClickEvent, _, cx| on_click(panel, cx));
        let label = if armed { armed_label } else { label };
        hover_fx(SharedString::from(format!("{id}-fx")), move |h| {
            // Encendida no cambia de color con el cursor: se aclara apenas.
            let surface = match bg {
                Some(bg) => h.mix(bg, bg.opacity(0.86)),
                None => h.mix(track, gpui::white().opacity(0.14)),
            };
            // Al apretar se hunde un poco, como el `scale(.96)` de la web.
            let inset = 2.5 * h.press;
            div()
                .id(id)
                .flex_1()
                .h(px(TILE_H))
                .relative()
                .cursor_pointer()
                .on_click(click)
                .child(
                    div()
                        .absolute()
                        .inset(px(inset))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(5.))
                        .rounded(px(16. - inset))
                        .bg(surface)
                        .child(svg().path(icon).size(px(18.)).text_color(fg))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(if bg.is_some() { ink } else { h.mix(muted, fg) })
                                .child(label),
                        ),
                )
                .into_any_element()
        })
    }

    fn tab_button(&self, tab: Tab, icon: &'static str, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.colors;
        let selected = self.tab == Some(tab);
        let (text, muted) = (c.text, c.muted);
        let click = cx.listener(move |panel, _: &ClickEvent, _, cx| panel.pick_tab(tab, cx));
        hover_fx(SharedString::from(format!("system-tab-btn-{label}")), move |h| {
            // El fondo de la elegida lo pone la píldora que se desliza (`tabs_glide`).
            let (rest, over) = if selected { (0.0, 0.03) } else { (0.0, 0.07) };
            let fg = if selected { text } else { h.mix(muted, text) };
            div()
                .id(label)
                .flex_1()
                .h(px(TABS_H))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded(px(TABS_H / 2.0))
                .text_size(px(11.))
                .text_color(fg)
                .bg(h.mix(gpui::white().opacity(rest), gpui::white().opacity(over)))
                .cursor_pointer()
                .on_click(click)
                .child(crate::glide::mark(crate::glide::named("system-tabs"), tab as u64))
                .child(svg().path(icon).size(px(13.)).text_color(fg))
                .child(label)
                .into_any_element()
        })
    }

    /// Una fila de las pestañas: ícono, nombre, y lo que va a la derecha.
    fn row(&self, leading: AnyElement, name: SharedString, trailing: AnyElement) -> gpui::Div {
        row(leading, name, trailing)
    }

    fn glyph(&self, path: &'static str) -> AnyElement {
        svg()
            .path(path)
            .size(px(16.))
            .text_color(self.colors.muted)
            .into_any_element()
    }

    fn app_icon(&mut self, path: Option<&PathBuf>, fallback: &'static str, cx: &mut Context<Self>) -> AnyElement {
        match self.icon(path, cx) {
            Some(icon) => img(icon).size(px(18.)).into_any_element(),
            None => self.glyph(fallback),
        }
    }

    fn mini(&self, key: Key, cx: &mut Context<Self>) -> AnyElement {
        let pct = self
            .value_of(key)
            .map(|v| format!("{:.0} %", v * 100.0))
            .unwrap_or_default();
        div()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(self.slider(key, MINI_H, None, None, cx).flex_1())
            .child(
                div()
                    .w(px(34.))
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(self.colors.muted)
                    .child(pct),
            )
            .into_any_element()
    }

    fn section(&self, title: &'static str) -> AnyElement {
        div()
            .h(px(SECTION_H))
            .flex()
            .items_end()
            .pb(px(4.))
            .text_size(px(10.))
            .text_color(self.colors.faint)
            .child(title)
            .into_any_element()
    }

    fn empty(&self, text: &'static str) -> gpui::Div {
        div()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .text_size(px(11.))
            .text_color(self.colors.muted)
            .child(text)
    }

    fn render_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tab = self.tab?;
        let body = match tab {
            Tab::Audio => {
                let mut sections: Vec<AnyElement> = Vec::new();
                // Quién usa el micrófono o la cámara ahora.
                let uses = self.privacy.uses();
                if !uses.is_empty() {
                    let camera = uses.iter().any(|u| u.device == crate::privacy::Device::Camera);
                    let names: Vec<String> = uses.iter().map(|u| u.app.clone()).collect();
                    let what = match (camera, uses.iter().any(|u| u.device == crate::privacy::Device::Microphone)) {
                        (true, true) => crate::i18n::t("pill.system.cameraAndMic"),
                        (true, false) => crate::i18n::t("pill.system.camera"),
                        _ => crate::i18n::t("pill.system.mic"),
                    };
                    let color: Hsla = if camera { rgb(0x4cd964).into() } else { rgb(0xf0a020).into() };
                    sections.push(
                        div()
                            .h(px(ROW_H))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(11.))
                            .child(div().size(px(20.)).flex().items_center().justify_center().child(
                                div().size(px(8.)).rounded(px(4.)).bg(color),
                            ))
                            .child(
                                div()
                                    .truncate()
                                    .text_color(self.colors.text)
                                    .child(crate::i18n::tf("pill.system.usesDevice", &[("apps", &names.join(", ")), ("what", &what)])),
                            )
                            .into_any_element(),
                    );
                }
                sections.push(self.section(crate::i18n::t("pill.system.output")));
                for output in self.state.outputs.clone() {
                    let lower = output.name.to_lowercase();
                    let icon = if ["auricular", "headphone", "headset", "airpods", "buds", "hands-free"]
                        .iter()
                        .any(|k| lower.contains(k))
                    {
                        "icons/headphones.svg"
                    } else {
                        "icons/speaker.svg"
                    };
                    let id = output.id.clone();
                    let check = if output.default {
                        svg()
                            .path("icons/circle-check.svg")
                            .size(px(15.))
                            .text_color(self.colors.text)
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    };
                    // «Altavoces (Realtek(R) Audio)»: el nombre corto y, en gris,
                    // la tarjeta.
                    let (short, card) = match output.name.split_once(" (") {
                        Some((short, rest)) => (short.to_string(), rest.trim_end_matches(')').to_string()),
                        None => (output.name.clone(), String::new()),
                    };
                    let line = div()
                        .h(px(ROW_H))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().child(self.glyph(icon)))
                        .child(div().flex_none().text_size(px(12.)).child(short))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(self.colors.faint)
                                .child(card),
                        )
                        .child(div().flex_none().pr(px(6.)).child(check));
                    let click = cx.listener(move |panel, _: &ClickEvent, _, cx| {
                        for o in &mut panel.state.outputs {
                            o.default = o.id == id;
                        }
                        panel.backend.send(Cmd::Output(id.clone()));
                        panel.refresh(cx);
                        cx.notify();
                    });
                    let row_id = SharedString::from(format!("output-{}", output.id));
                    sections.push(
                        hover_fx(SharedString::from(format!("{row_id}-fx")), move |h| {
                            div()
                                .id(row_id)
                                .rounded(px(10.))
                                .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.06)))
                                .cursor_pointer()
                                .on_click(click)
                                .child(line)
                                .into_any_element()
                        })
                        .into_any_element(),
                    );
                }
                sections.push(self.section(crate::i18n::t("pill.system.apps")));
                let apps = self.state.audio.clone();
                if apps.is_empty() {
                    sections.push(self.empty(crate::i18n::t("pill.system.noAudioApps")).into_any_element());
                } else {
                    for app in apps.iter().take(MAX_ROWS) {
                        let leading = self.app_icon(app.path.as_ref(), "icons/audio-lines.svg", cx);
                        let trailing = self.mini(Key::App(app.pid), cx);
                        sections.push(self.row(leading, app.name.clone().into(), trailing).into_any_element());
                    }
                }
                div().flex().flex_col().children(sections).into_any_element()
            }
            Tab::Displays => {
                let now = Instant::now();
                if self.chevrons.values().any(|t| t.is_running(now)) {
                    window.request_animation_frame();
                }
                self.render_displays(cx)
            }
            Tab::Vitals => self.render_vitals(window, cx),
            Tab::Procs => self.render_procs(cx),
        };
        let key = match tab {
            Tab::Audio => "audio",
            Tab::Displays => "pantallas",
            Tab::Vitals => "rendimiento",
            Tab::Procs => "procesos",
        };
        // Aparece mientras el notch crece: se funde y baja un poco a su sitio.
        Some(
            div()
                .relative()
                .px(px(SIDE + 4.))
                .pt(px(GAP))
                .child(body)
                .with_animation(
                    SharedString::from(format!("system-tab-{key}")),
                    Animation::new(Duration::from_millis(260)).with_easing(gpui::ease_out_quint()),
                    |el, t| el.opacity(t).top(px(-6.0 * (1.0 - t))),
                )
                .into_any_element(),
        )
    }

    fn render_procs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(procs) = self.state.procs.clone() else {
            return self.empty(crate::i18n::t("pill.system.measuring")).into_any_element();
        };
        let shown = self.shown_apps();
        // Lo que se ve queda como el orden a mantener mientras el cursor
        // entre a la lista.
        if !self.list_hovered {
            self.order = shown.iter().map(|app| app.stem.clone()).collect();
        }
        let searching = self.searching();
        let paused = self.list_hovered && !searching;
        let c = &self.colors;
        let gb = |bytes: u64| bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        let ram_pct = if procs.ram_total > 0 {
            procs.ram_used as f32 / procs.ram_total as f32
        } else {
            0.0
        };
        let meter = |label: String, value: f32, color: Hsla| {
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(11.))
                .child(div().w(px(118.)).flex_none().child(label))
                .child(
                    div()
                        .flex_1()
                        .h(px(4.))
                        .rounded(px(2.))
                        .bg(gpui::white().opacity(0.09))
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.))
                                .bg(color)
                                .w(gpui::relative(value.clamp(0.0, 1.0))),
                        ),
                )
        };
        let header = div()
            .h(px(PROCS_HEADER_H))
            .flex()
            .gap(px(16.))
            .text_color(c.muted)
            .child(meter(format!("CPU {:.0} %", procs.cpu), procs.cpu / 100.0, c.amber))
            .child(meter(
                format!("RAM {:.1} de {:.0} GB", gb(procs.ram_used), gb(procs.ram_total)),
                ram_pct,
                rgb(0x6fa3e0).into(),
            ));
        let (text, muted, faint) = (c.text, c.muted, c.faint);
        let search_row = div()
            .id("procs-search")
            .h(px(SEARCH_H - 4.))
            .mb(px(4.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(13.))
            .cursor_text()
            .on_click({
                let search = self.search.clone();
                move |_, window, cx| window.focus(&search.focus_handle(cx))
            })
            .child(svg().path("icons/search.svg").size(px(13.)).text_color(muted))
            .child(div().flex_1().min_w_0().text_size(px(12.)).child(self.search.clone()))
            .when(paused, |el| {
                el.child(div().flex_none().text_size(px(10.)).text_color(faint).child(crate::i18n::t("pill.system.sortPaused")))
            })
            .when(searching, |el| {
                el.child(
                    div()
                        .id("procs-search-clear")
                        .size(px(20.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(10.))
                        .cursor_pointer()
                        .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| {
                            panel.search.update(cx, |s, cx| s.clear(cx));
                        }))
                        .child(svg().path("icons/x.svg").size(px(11.)).text_color(muted))
                        .hover_bg("procs-search-clear-fx", text.opacity(0.0), text.opacity(0.1)),
                )
            })
            .hover_bg("procs-search-fx", text.opacity(0.06), text.opacity(0.09));
        let empty_search = searching && shown.is_empty();
        let rows: Vec<AnyElement> = shown
            .iter()
            .map(|app| {
                let leading = self.app_icon(app.path.as_ref(), "icons/activity.svg", cx);
                let row_id: SharedString = format!("proc-{}", app.stem).into();
                let armed = self.is_armed(&Arm::Force(app.stem.clone()));
                let ram = if app.ram >= 1 << 30 {
                    format!("{:.1} GB", gb(app.ram))
                } else {
                    format!("{} MB", app.ram >> 20)
                };
                let stem = app.stem.clone();
                let stem_force = app.stem.clone();
                let c = &self.colors;
                let (text, muted, amber, danger, ink) = (c.text, c.muted, c.amber, c.danger, c.ink);
                let close = cx.listener(move |panel, _: &ClickEvent, _, cx| {
                    panel.backend.send(Cmd::Close(stem.clone()));
                    panel.refresh(cx);
                });
                let force = cx.listener(move |panel, _: &ClickEvent, _, cx| {
                    if panel.confirm(Arm::Force(stem_force.clone()), cx) {
                        panel.backend.send(Cmd::Force(stem_force.clone()));
                        panel.refresh(cx);
                    }
                });
                let (cpu, name) = (app.cpu, SharedString::from(app.name.clone()));
                let id = row_id.clone();
                hover_fx(row_id, move |h| {
                    // Las acciones se funden con el cursor (o se quedan si
                    // «Forzar» está armado esperando el segundo clic).
                    let reveal = if armed { 1.0 } else { h.t };
                    // Las acciones toman el lugar de CPU y RAM, ancladas a la
                    // derecha: al lado no caben en un panel angosto.
                    let trailing = div()
                        .flex_1()
                        .relative()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .w(px(44.))
                                .text_size(px(11.))
                                .text_color(if cpu >= 20.0 { amber } else { muted })
                                .opacity(1.0 - reveal)
                                .child(format!("{cpu:.0} %")),
                        )
                        .child(
                            div()
                                .w(px(56.))
                                .text_size(px(11.))
                                .text_color(muted)
                                .opacity(1.0 - reveal)
                                .child(ram),
                        )
                        .child(
                            div()
                                .absolute()
                                .right(px(0.))
                                .top_0()
                                .bottom_0()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .opacity(reveal)
                                // Mientras no se ven, que no se puedan apretar.
                                .when(reveal < 0.05, |el| el.invisible())
                                .child(proc_action(
                                    format!("close-{id}").into(),
                                    crate::i18n::t("pill.system.close"),
                                    text,
                                    ink,
                                    false,
                                    close,
                                ))
                                .child(proc_action(
                                    format!("force-{id}").into(),
                                    if armed { crate::i18n::t("pill.system.forceAsk") } else { crate::i18n::t("pill.system.force") },
                                    danger,
                                    ink,
                                    armed,
                                    force,
                                )),
                        )
                        .into_any_element();
                    row(leading, name, trailing)
                        .mx(px(-6.))
                        .px(px(6.))
                        .rounded(px(10.))
                        .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.05)))
                        .into_any_element()
                })
                .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .child(search_row)
            .child(header)
            .child(
                div()
                    .id("procs-list")
                    .flex()
                    .flex_col()
                    // Con el cursor encima la lista no se reordena.
                    .on_hover(cx.listener(|panel, hovered: &bool, _, cx| {
                        panel.list_hovered = *hovered;
                        cx.notify();
                    }))
                    .children(rows)
                    .when(empty_search, |el| el.child(self.empty(crate::i18n::t("pill.system.noMatch")))),
            )
            .into_any_element()
    }

    // --- Rendimiento -------------------------------------------------------------

    /// Una lectura nueva: entra al historial y los números van hacia ella.
    /// La primera vez cuentan desde cero, como el vistazo.
    fn take_vitals(&mut self) {
        let Some(v) = self.state.vitals.clone() else {
            return;
        };
        let now = Instant::now();
        self.history.push(&v);
        let ram = if v.ram_total > 0 { v.ram_used as f32 / v.ram_total as f32 } else { 0.0 };
        // La escala de la red sigue al pico del último minuto, sin saltar.
        let peak = self.history.down.iter().chain(&self.history.up).copied().fold(0.0f32, f32::max);
        let mut targets: Vec<(&'static str, f32)> = vec![
            ("cpu", v.cpu),
            ("gpu", v.gpu.unwrap_or(0.0)),
            ("ram", ram),
            ("down", v.net_down as f32),
            ("up", v.net_up as f32),
            ("net-scale", peak.max(64.0 * 1024.0)),
            ("cpu-temp", v.cpu_temp.unwrap_or(0.0)),
            ("gpu-temp", v.gpu_temp.unwrap_or(0.0)),
        ];
        for (key, disk) in DISK_KEYS.iter().zip(&v.disks) {
            targets.push((key, 1.0 - disk.free as f32 / disk.total.max(1) as f32));
        }
        for (key, target) in targets {
            let dial = self
                .dials
                .entry(key)
                .or_insert_with(|| Tween::hover(0.0, DIAL_FOR, crate::anim::ease_smooth_out));
            if self.reduce_motion {
                dial.snap(target);
            } else {
                dial.set(target, now);
            }
        }
    }

    fn dial(&self, key: &str, now: Instant) -> f32 {
        self.dials.get(key).map_or(0.0, |d| d.value(now))
    }

    fn vitals_height(&self) -> f32 {
        let Some(v) = self.state.vitals.as_ref() else {
            return ROW_H;
        };
        let rows = 3 + v.gpu.is_some() as usize + v.disks.len() + v.power.is_some() as usize;
        rows as f32 * ROW_H + FOOT_H
    }

    fn render_vitals(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(v) = self.state.vitals.clone() else {
            return self.empty(crate::i18n::t("pill.system.measuring")).into_any_element();
        };
        let now = Instant::now();
        let shift = self.history.shift(now, self.reduce_motion);
        if shift < 1.0 || self.dials.values().any(|d| d.is_running(now)) {
            window.request_animation_frame();
        }
        let c = &self.colors;
        let (text, muted, faint, amber, danger) = (c.text, c.muted, c.faint, c.amber, c.danger);
        let heat = |t: f32| {
            if t >= 95.0 {
                danger
            } else if t >= 80.0 {
                amber
            } else {
                muted
            }
        };
        let temp = |key: &str, real: Option<f32>| -> AnyElement {
            match real {
                Some(real) => detail(format!("{:.0} °C", self.dial(key, now)), heat(real)),
                None => div().into_any_element(),
            }
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.push(
            vital_row(
                self.glyph("icons/cpu.svg"),
                "CPU".into(),
                strip(&self.history.cpu, shift, rgb(CPU_TINT).into()),
                value(format!("{:.0} %", self.dial("cpu", now)), text),
                temp("cpu-temp", v.cpu_temp),
            )
            .into_any_element(),
        );
        if v.gpu.is_some() {
            rows.push(
                vital_row(
                    self.glyph("icons/microchip.svg"),
                    "GPU".into(),
                    strip(&self.history.gpu, shift, rgb(GPU_TINT).into()),
                    value(format!("{:.0} %", self.dial("gpu", now)), text),
                    temp("gpu-temp", v.gpu_temp),
                )
                .into_any_element(),
            );
        }
        let ram_used = (self.dial("ram", now) as f64 * v.ram_total as f64) as u64;
        let ram_full = v.ram_total > 0 && v.ram_used as f64 / v.ram_total as f64 >= 0.9;
        rows.push(
            vital_row(
                self.glyph("icons/memory-stick.svg"),
                crate::i18n::t("pill.system.memory").into(),
                strip(&self.history.ram, shift, rgb(RAM_TINT).into()),
                value(bytes(ram_used), if ram_full { amber } else { text }),
                detail(crate::i18n::tf("pill.system.ofTotal", &[("total", &bytes(v.ram_total))]), faint),
            )
            .into_any_element(),
        );
        let arrow = |icon: &'static str, tint: u32, label: String, color: Hsla, size: f32| {
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(3.))
                .text_size(px(size))
                .text_color(color)
                .child(svg().path(icon).size(px(9.)).text_color(rgb(tint)))
                .child(label)
                .into_any_element()
        };
        rows.push(
            vital_row(
                self.glyph("icons/network.svg"),
                crate::i18n::t("pill.system.network").into(),
                net_strip(&self.history.down, &self.history.up, self.dial("net-scale", now).max(1.0), shift),
                arrow("icons/arrow-down.svg", DOWN_TINT, rate(self.dial("down", now) as f64), text, 12.),
                arrow("icons/arrow-up.svg", UP_TINT, rate(self.dial("up", now) as f64), muted, 11.),
            )
            .into_any_element(),
        );
        for (i, disk) in v.disks.iter().enumerate() {
            let used = DISK_KEYS.get(i).map_or(0.0, |key| self.dial(key, now));
            let low = disk.total > 0 && (disk.free as f64) < disk.total as f64 * 0.1;
            let fill: Hsla = if low { amber } else { c.fill.opacity(0.7) };
            let track = c.track;
            let summary = crate::i18n::tf("pill.system.diskFree", &[("free", &bytes(disk.free)), ("total", &bytes(disk.total))]);
            // El disco del sistema abre Liberar espacio (por ahora en macOS):
            // con el cursor, el resumen cede su lugar al botón; con poco
            // espacio, el botón queda a la vista.
            let cleanable = i == 0 && SPACE;
            let open = cx.listener(|panel, _: &ClickEvent, _, cx| panel.open_space(cx));
            let leading = self.glyph("icons/hard-drive.svg");
            let name: SharedString = disk.name.clone().into();
            let id = SharedString::from(format!("system-disk-{i}"));
            let disk_row = hover_fx(SharedString::from(format!("system-disk-{i}-fx")), move |h| {
                let reveal = if !cleanable { 0.0 } else if low { 1.0 } else { h.t };
                let trailing = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .h(px(5.))
                            .rounded(px(3.))
                            .bg(track)
                            .child(div().h_full().rounded(px(3.)).bg(fill).w(gpui::relative(used.clamp(0.0, 1.0)))),
                    )
                    .child(
                        div()
                            .w(px(VALUE_W + DETAIL_W + 10.))
                            .h(px(ROW_H))
                            .flex_none()
                            .relative()
                            .flex()
                            .items_center()
                            .justify_end()
                            .child(
                                div()
                                    .opacity(1.0 - reveal)
                                    .text_size(px(11.))
                                    .text_color(if low { amber } else { muted })
                                    .child(summary),
                            )
                            .when(cleanable, |el| {
                                let color = if low { amber } else { text };
                                el.child(
                                    div()
                                        .absolute()
                                        .right_0()
                                        .top_0()
                                        .bottom_0()
                                        .flex()
                                        .items_center()
                                        .opacity(reveal)
                                        .child(
                                            div()
                                                .h(px(22.))
                                                .px(px(10.))
                                                .flex()
                                                .items_center()
                                                .gap(px(5.))
                                                .rounded(px(11.))
                                                .bg(color.opacity(0.14 + 0.08 * h.t))
                                                .text_size(px(10.))
                                                .text_color(color)
                                                .child(svg().path("icons/sparkles.svg").size(px(11.)).text_color(color))
                                                .child(crate::i18n::t("pill.system.space.title")),
                                        ),
                                )
                            }),
                    )
                    .into_any_element();
                row(leading, name, trailing)
                    .id(id)
                    .mx(px(-6.))
                    .px(px(6.))
                    .rounded(px(10.))
                    .when(cleanable, |el| {
                        el.cursor_pointer()
                            .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.05)))
                            .on_click(open)
                    })
                    .into_any_element()
            });
            rows.push(disk_row.into_any_element());
        }
        if let Some(power) = v.power.as_ref() {
            let mut facts = Vec::new();
            if let Some(health) = power.health {
                facts.push(crate::i18n::tf("pill.system.health", &[("pct", &format!("{:.0}", health * 100.0))]));
            }
            if let Some(cycles) = power.cycles {
                facts.push(crate::i18n::tf("pill.system.cycles", &[("n", &cycles)]));
            }
            let watts = power.watts.filter(|w| w.abs() >= 0.1).map(|w| {
                let (sign, color) = if w > 0.0 { ("+", rgb(DOWN_TINT).into()) } else { ("−", muted) };
                value(format!("{sign}{:.1} W", w.abs()), color)
            });
            // El porcentaje ya está en la franja: aquí, la salud y el consumo.
            let trailing = div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(muted).child(facts.join(" · ")))
                .child(div().w(px(VALUE_W)).flex_none().flex().justify_end().children(watts))
                .into_any_element();
            rows.push(
                row(self.glyph("icons/battery-full.svg"), crate::i18n::t("pill.system.battery").into(), trailing)
                    .into_any_element(),
            );
        }
        let thermal = v.thermal.map(|state| {
            let (key, color) = match state {
                0 => ("pill.system.thermal0", faint),
                1 => ("pill.system.thermal1", muted),
                2 => ("pill.system.thermal2", amber),
                _ => ("pill.system.thermal3", danger),
            };
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_color(color)
                .child(svg().path("icons/thermometer.svg").size(px(12.)).text_color(color))
                .child(crate::i18n::t(key))
        });
        let foot = div()
            .h(px(FOOT_H))
            .flex()
            .items_center()
            .gap(px(14.))
            .text_size(px(10.))
            .text_color(faint)
            .children(thermal)
            .child(div().flex_1())
            .when(v.uptime > 0, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(svg().path("icons/timer.svg").size(px(12.)).text_color(faint))
                        .child(crate::i18n::tf("pill.system.uptime", &[("time", &since(v.uptime))])),
                )
            });
        rows.push(foot.into_any_element());
        // Las filas entran una tras otra, bajando apenas.
        let rows: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(i, el)| {
                crate::code::enter::Enter::new(SharedString::from(format!("system-vital-{i}")))
                    .delay(crate::code::enter::stagger(i, 0.035, 8))
                    .from(0., -5.)
                    .apply(div(), window, cx)
                    .child(el)
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().children(rows).into_any_element()
    }

    // --- Pantallas ---------------------------------------------------------------

    fn display_height(&self, display: &Display) -> f32 {
        let mut height = ROW_H + SUB_H;
        if self.trial.as_ref().is_some_and(|t| t.id == display.id) {
            height += KEEP_H;
        }
        if self.open_display == Some(display.id) {
            if let Some(mode) = display.mode.as_ref() {
                let (sizes, rates) = choices(&display.modes, mode);
                if sizes.len() > 1 {
                    height += CHIPS_H;
                }
                if rates.len() > 1 {
                    height += CHIPS_H;
                }
            }
        }
        height
    }

    fn displays_height(&self) -> f32 {
        let list = &self.state.displays;
        let blocks: f32 = list.iter().map(|d| self.display_height(d)).sum();
        TOOLBAR_H + 8.0 + blocks.max(ROW_H) + DIVIDER_H * list.len().saturating_sub(1) as f32
    }

    /// Se relee un poco después: el SO tarda en reacomodar las pantallas.
    fn refresh_soon(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            let _ = this.update(cx, |panel, cx| panel.refresh(cx));
        })
        .detach();
    }

    fn toggle_modes(&mut self, id: DisplayId, cx: &mut Context<Self>) {
        let open = self.open_display != Some(id);
        // Abrir una cierra la otra: las dos flechas giran.
        if let Some(before) = self.open_display.take() {
            self.turn_chevron(before, false);
        }
        if open {
            self.open_display = Some(id);
        }
        self.turn_chevron(id, open);
        cx.notify();
    }

    fn turn_chevron(&mut self, id: DisplayId, open: bool) {
        let target = if open { 0.5 } else { 0.0 };
        let chevron = self
            .chevrons
            .entry(id)
            .or_insert_with(|| Tween::hover(0.0, Duration::from_millis(260), crate::anim::ease_smooth_out));
        if self.reduce_motion {
            chevron.snap(target);
        } else {
            chevron.set(target, Instant::now());
        }
    }

    /// Pone una resolución y deja corriendo la cuenta para volver a la de
    /// antes. Elegir otra durante la cuenta guarda la original.
    fn try_mode(&mut self, id: DisplayId, mode: Mode, cx: &mut Context<Self>) {
        let Some(display) = self.state.displays.iter_mut().find(|d| d.id == id) else {
            return;
        };
        let Some(current) = display.mode.clone() else {
            return;
        };
        if current == mode {
            return;
        }
        display.mode = Some(mode.clone());
        self.backend.send(Cmd::Mode(id, mode.clone()));
        let before = match self.trial.as_ref() {
            Some(trial) if trial.id == id => trial.before.clone(),
            _ => current,
        };
        let at = Instant::now();
        if before == mode {
            self.trial = None;
        } else {
            self.trial = Some(Trial { id, before, after: mode, at });
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let go = this
                    .update(cx, |panel, cx| {
                        let Some(trial) = panel.trial.as_ref().filter(|t| t.at == at) else {
                            return false;
                        };
                        if trial.at.elapsed() >= KEEP_FOR {
                            panel.revert_mode(cx);
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            })
            .detach();
        }
        self.refresh_soon(cx);
        cx.notify();
    }

    fn revert_mode(&mut self, cx: &mut Context<Self>) {
        let Some(trial) = self.trial.take() else {
            return;
        };
        if let Some(display) = self.state.displays.iter_mut().find(|d| d.id == trial.id) {
            display.mode = Some(trial.before.clone());
        }
        self.backend.send(Cmd::Mode(trial.id, trial.before));
        self.refresh_soon(cx);
        cx.notify();
    }

    fn make_primary(&mut self, id: DisplayId, cx: &mut Context<Self>) {
        for display in &mut self.state.displays {
            display.primary = display.id == id;
        }
        self.backend.send(Cmd::Primary(id));
        self.refresh_soon(cx);
        cx.notify();
    }

    /// Opciones en una cápsula, con la elegida marcada por la píldora que se
    /// desliza (la misma de las pestañas).
    fn segmented(
        &self,
        name: &'static str,
        options: Vec<(u64, &'static str, Option<&'static str>)>,
        selected: u64,
        on_pick: impl Fn(&mut Self, u64, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let glide = crate::glide::named(name);
        let on_pick = Rc::new(on_pick);
        let (text, muted) = (self.colors.text, self.colors.muted);
        let mut capsule = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(px(26.))
            .p(px(2.))
            .rounded(px(13.))
            .bg(gpui::white().opacity(0.05))
            .child(crate::glide::indicator(
                glide.clone(),
                Some(selected),
                crate::glide::Look { fill: gpui::white().opacity(0.14), radius: 11., edge: None },
            ));
        for (key, icon, label) in options {
            let picked = key == selected;
            let on_pick = on_pick.clone();
            let click = cx.listener(move |panel, _: &ClickEvent, _, cx| on_pick(panel, key, cx));
            let mark = crate::glide::mark(glide.clone(), key);
            let id = SharedString::from(format!("{name}-{key}"));
            capsule = capsule.child(hover_fx(SharedString::from(format!("{name}-{key}-fx")), move |h| {
                let fg = if picked { text } else { h.mix(muted, text) };
                div()
                    .id(id)
                    .h(px(22.))
                    .px(px(if label.is_some() { 10. } else { 7. }))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(11.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(fg)
                    .on_click(click)
                    .child(mark)
                    .child(svg().path(icon).size(px(13.)).text_color(fg))
                    .children(label)
                    .into_any_element()
            }));
        }
        capsule.into_any_element()
    }

    fn render_displays(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.colors;
        let (text, muted) = (c.text, c.muted);
        let mut toolbar = div().h(px(TOOLBAR_H)).mb(px(8.)).flex().items_center().gap(px(8.));
        if let Some(dark) = self.state.dark {
            toolbar = toolbar.child(self.segmented(
                "system-look",
                vec![(0, "icons/sun.svg", None), (1, "icons/moon.svg", None)],
                dark as u64,
                |panel, key, cx| {
                    let dark = key == 1;
                    if panel.state.dark != Some(dark) {
                        panel.state.dark = Some(dark);
                        panel.backend.send(Cmd::Dark(dark));
                        cx.notify();
                    }
                },
                cx,
            ));
        }
        if self.state.screens >= 2 {
            toolbar = toolbar.child(self.segmented(
                "system-arrange",
                vec![
                    (0, "icons/columns-2.svg", Some(crate::i18n::t("pill.system.extend"))),
                    (1, "icons/copy.svg", Some(crate::i18n::t("pill.system.mirror"))),
                ],
                self.state.mirrored as u64,
                |panel, key, cx| {
                    let mirror = key == 1;
                    if panel.state.mirrored != mirror {
                        panel.state.mirrored = mirror;
                        panel.backend.send(Cmd::Mirror(mirror));
                        panel.refresh_soon(cx);
                        cx.notify();
                    }
                },
                cx,
            ));
        }
        let connect = cx.listener(|panel, _: &ClickEvent, _, _| panel.backend.send(Cmd::ConnectDisplay));
        toolbar = toolbar.child(div().flex_1()).child(hover_fx("system-connect-fx", move |h| {
            let fg = h.mix(muted, text);
            div()
                .id("system-connect")
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(13.))
                .bg(h.mix(gpui::white().opacity(0.05), gpui::white().opacity(0.1)))
                .cursor_pointer()
                .text_size(px(11.))
                .text_color(fg)
                .on_click(connect)
                .child(svg().path("icons/cast.svg").size(px(13.)).text_color(fg))
                .child(crate::i18n::t("pill.system.connect"))
                .into_any_element()
        }));
        let displays = self.state.displays.clone();
        let mut list: Vec<AnyElement> = Vec::new();
        for (i, display) in displays.iter().enumerate() {
            if i > 0 {
                list.push(div().h(px(DIVIDER_H)).flex().items_center().child(div().h(px(1.)).w_full().bg(gpui::white().opacity(0.06))).into_any_element());
            }
            list.push(self.render_display(display, displays.len(), cx));
        }
        if displays.is_empty() {
            list.push(self.empty(crate::i18n::t("pill.system.noDisplays")).into_any_element());
        }
        div().flex().flex_col().child(toolbar).children(list).into_any_element()
    }

    fn render_display(&mut self, display: &Display, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let id = display.id;
        let key = display_key(id);
        let now = Instant::now();
        let c = &self.colors;
        let (text, muted, faint, ink) = (c.text, c.muted, c.faint, c.ink);
        let icon = if id == DisplayId::Internal { "icons/laptop.svg" } else { "icons/monitor.svg" };
        let brightness = if display.brightness.is_some() {
            self.mini(Key::Brightness(id), cx)
        } else {
            div()
                .flex_1()
                .text_size(px(11.))
                .text_color(faint)
                .child(crate::i18n::t("pill.system.noBrightness"))
                .into_any_element()
        };
        let (sizes, rates) = display.mode.as_ref().map(|m| choices(&display.modes, m)).unwrap_or_default();
        let can_pick = sizes.len() > 1 || rates.len() > 1;
        let turn = self.chevrons.get(&id).map_or(0.0, |t| t.value(now));
        let open = self.open_display == Some(id);
        let mode_button = display.mode.as_ref().map(|mode| {
            let label = mode_label(mode, true);
            let toggle = cx.listener(move |panel, _: &ClickEvent, _, cx| panel.toggle_modes(id, cx));
            let button_id = SharedString::from(format!("system-mode-{key}"));
            hover_fx(SharedString::from(format!("system-mode-{key}-fx")), move |h| {
                let fg = if open { text } else { h.mix(muted, text) };
                div()
                    .id(button_id)
                    .flex_none()
                    .h(px(22.))
                    .mr(px(-8.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(11.))
                    .bg(h.mix(gpui::white().opacity(if open { 0.06 } else { 0.0 }), gpui::white().opacity(0.08)))
                    .text_size(px(11.))
                    .text_color(fg)
                    .when(can_pick, |el| el.cursor_pointer().on_click(toggle))
                    .child(label)
                    .when(can_pick, |el| {
                        el.child(
                            svg()
                                .path("icons/chevron-down.svg")
                                .size(px(11.))
                                .text_color(fg)
                                .with_transformation(gpui::Transformation::rotate(gpui::percentage(turn))),
                        )
                    })
                    .into_any_element()
            })
        });
        let primary = (count > 1).then(|| {
            if display.primary {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(10.))
                    .text_color(faint)
                    .child(svg().path("icons/star.svg").size(px(11.)).text_color(faint))
                    .child(crate::i18n::t("pill.system.isPrimary"))
                    .into_any_element()
            } else {
                proc_action(
                    format!("system-primary-{key}").into(),
                    crate::i18n::t("pill.system.makePrimary"),
                    text,
                    ink,
                    false,
                    cx.listener(move |panel, _: &ClickEvent, _, cx| panel.make_primary(id, cx)),
                )
                .into_any_element()
            }
        });
        // Arriba el nombre entero (es lo que se reconoce) y quién es la
        // principal; abajo, el brillo y la resolución.
        let head = div()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().child(self.glyph(icon)))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(display.name.clone()))
            .children(primary);
        let sub = div()
            .h(px(SUB_H))
            .pl(px(30.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().flex_1().min_w_0().flex().child(brightness))
            .children(mode_button);
        let trial = self.trial.clone().filter(|t| t.id == id).map(|trial| {
            let left = KEEP_FOR.saturating_sub(trial.at.elapsed()).as_secs_f32().ceil() as u32;
            div()
                .h(px(KEEP_H))
                .pl(px(30.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(c.amber)
                        .child(crate::i18n::tf(
                            "pill.system.keepMode",
                            &[("mode", &mode_label(&trial.after, false)), ("n", &left)],
                        )),
                )
                .child(proc_action(
                    format!("system-revert-{key}").into(),
                    crate::i18n::t("pill.system.revert"),
                    text,
                    ink,
                    false,
                    cx.listener(|panel, _: &ClickEvent, _, cx| panel.revert_mode(cx)),
                ))
                .child(proc_action(
                    format!("system-keep-{key}").into(),
                    crate::i18n::t("pill.system.keep"),
                    c.fill,
                    ink,
                    true,
                    cx.listener(|panel, _: &ClickEvent, _, cx| {
                        panel.trial = None;
                        cx.notify();
                    }),
                ))
        });
        let mut chips: Vec<AnyElement> = Vec::new();
        if open {
            let current = display.mode.clone();
            if sizes.len() > 1 {
                let selected = current
                    .as_ref()
                    .and_then(|m| sizes.iter().position(|s| s.width == m.width && s.height == m.height));
                let labels = sizes.iter().map(|m| mode_label(m, false)).collect();
                chips.push(self.chips(format!("system-sizes-{key}"), labels, selected, sizes.clone(), id, cx));
            }
            if rates.len() > 1 {
                let selected = current.as_ref().and_then(|m| rates.iter().position(|r| r.hz == m.hz));
                let labels = rates.iter().map(|m| format!("{} Hz", m.hz)).collect();
                chips.push(self.chips(format!("system-rates-{key}"), labels, selected, rates.clone(), id, cx));
            }
        }
        div()
            .flex()
            .flex_col()
            .child(head)
            .child(sub)
            .children(trial)
            .children(chips)
            .into_any_element()
    }

    /// Una fila de resoluciones o frecuencias: la elegida con la píldora
    /// que se desliza; al abrirse, entran una tras otra.
    fn chips(
        &self,
        name: String,
        labels: Vec<String>,
        selected: Option<usize>,
        modes: Vec<Mode>,
        id: DisplayId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let glide = crate::glide::named(name.clone());
        let (text, muted) = (self.colors.text, self.colors.muted);
        let mut line = div()
            .relative()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(crate::glide::indicator(
                glide.clone(),
                selected.map(|i| i as u64),
                crate::glide::Look { fill: gpui::white().opacity(0.14), radius: 12., edge: None },
            ));
        for (i, (label, mode)) in labels.into_iter().zip(modes).enumerate() {
            let picked = selected == Some(i);
            let click = cx.listener(move |panel, _: &ClickEvent, _, cx| panel.try_mode(id, mode.clone(), cx));
            let mark = crate::glide::mark(glide.clone(), i as u64);
            let chip_id = SharedString::from(format!("{name}-{i}"));
            line = line.child(
                hover_fx(SharedString::from(format!("{name}-{i}-fx")), move |h| {
                    let fg = if picked { text } else { h.mix(muted, text) };
                    div()
                        .id(chip_id)
                        .flex_1()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(12.))
                        .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(if picked { 0.0 } else { 0.06 })))
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(fg)
                        .on_click(click)
                        .child(mark)
                        .child(label)
                        .into_any_element()
                })
                .into_any_element(),
            );
        }
        let key = name.clone();
        div()
            .relative()
            .h(px(CHIPS_H))
            .pl(px(30.))
            .flex()
            .items_center()
            .child(line)
            .with_animation(
                SharedString::from(format!("{key}-in")),
                Animation::new(Duration::from_millis(320)).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(t).top(px(-4.0 * (1.0 - t))),
            )
            .into_any_element()
    }

    /// Esc: con algo buscado, primero borra la búsqueda.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.space_open() {
            self.close_space(cx);
        } else if self.tab == Some(Tab::Procs) && self.searching() {
            self.search.update(cx, |s, cx| s.clear(cx));
        } else {
            cx.emit(SystemEvent::Close);
        }
    }

    /// La «m» silencia, salvo que se esté escribiendo en el buscador.
    fn mute_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.focus_handle(cx).is_focused(window) {
            let text = format!("{}m", self.search.read(cx).text());
            self.query = crate::clipboard::fold(text.trim());
            self.search.update(cx, |s, cx| s.set_text(text, cx));
            cx.notify();
        } else {
            self.toggle_mute(cx);
        }
    }

    fn find_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab != Some(Tab::Procs) {
            self.pick_tab(Tab::Procs, cx);
        }
        window.focus(&self.search.focus_handle(cx));
    }
}

impl Render for SystemPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.state.clone();
        let volume_icon = if s.muted || s.volume <= 0.001 {
            "icons/volume-x.svg"
        } else if s.volume < 0.5 {
            "icons/volume-1.svg"
        } else {
            "icons/volume-2.svg"
        };
        // El slider grande es la primera pantalla con brillo (la del equipo,
        // si la hay).
        let main_display = s.displays.iter().find(|d| d.brightness.is_some());
        let bright = main_display.and_then(|d| d.brightness);
        let sun = if bright.unwrap_or(1.0) < 0.5 {
            "icons/sun-dim.svg"
        } else {
            "icons/sun.svg"
        };
        let bright_key = Key::Brightness(main_display.map_or(DisplayId::Internal, |d| d.id));
        let output = s
            .outputs
            .iter()
            .find(|o| o.default)
            .map(|o| {
                // «Altavoces (Realtek(R) Audio)» → «Altavoces».
                let short = o.name.split(" (").next().unwrap_or(&o.name);
                let short: String = short.chars().take(24).collect();
                format!(" · {short}")
            })
            .unwrap_or_default();
        let volume_label: SharedString =
            format!("{:.0} %{output}", if s.muted { 0.0 } else { s.volume * 100.0 }).into();
        let bright_label: SharedString = match bright {
            Some(v) => format!("{:.0} %", v * 100.0).into(),
            None => crate::i18n::t("pill.system.displayNoBrightness").into(),
        };
        let lock_armed = self.is_armed(&Arm::Lock);
        let sleep_armed = self.is_armed(&Arm::Sleep);
        // Al cambiar de pestaña, lo nuevo se desliza desde su lado y se funde. La
        // altura ya la anima el notch (que sigue a `desired_height`).
        let (switches, side) = self.tab_switch;
        let tab = self.render_tab(window, cx).map(|tab| {
            crate::code::enter::Enter::new("system-tab-body")
                .from(18. * side, 0.)
                .on_change(switches)
                .apply(div(), window, cx)
                .child(tab)
                .into_any_element()
        });
        let notice = self
            .notice
            .clone()
            .filter(|(_, _, at)| at.elapsed() < NOTICE_FOR)
            .map(|(text, error, _)| {
                div()
                    .h(px(NOTICE_H))
                    .px(px(SIDE + 4.))
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .text_color(if error { self.colors.danger } else { self.colors.muted })
                    .child(text)
            });

        // Liberar espacio toma el lugar del panel (bajo la franja) y entra
        // desde la derecha; al volver, el panel entra desde la izquierda.
        let body = if self.space_open() {
            let space = self.render_space(window, cx);
            crate::code::enter::Enter::new("system-space")
                .from(28., 0.)
                .apply(div(), window, cx)
                .child(div().h(px(4.)))
                .child(space)
                .into_any_element()
        } else {
            crate::code::enter::Enter::new("system-main")
                .from(-28., 0.)
                .on_change(self.space_closes)
                .apply(div(), window, cx)
                .flex()
                .flex_col()
                .child(div().h(px(4.)))
                .child(
                    div()
                        .px(px(SIDE))
                        .child(self.slider(Key::Volume, SLIDER_H, Some(volume_icon), Some(volume_label), cx)),
                )
                .child(div().h(px(GAP)))
                .child(
                    div()
                        .px(px(SIDE))
                        .child(self.slider(bright_key, SLIDER_H, Some(sun), Some(bright_label), cx)),
                )
                .child(div().h(px(12.)))
                .child(
                    div()
                        .flex()
                        .gap(px(GAP))
                        .px(px(SIDE))
                        .child(self.tile(
                            "system-mute",
                            "icons/volume-x.svg",
                            crate::i18n::t("pill.system.mute"),
                            s.muted.then_some(self.colors.fill),
                            false,
                            "",
                            |panel, cx| panel.toggle_mute(cx),
                            cx,
                        ))
                        .child(self.tile(
                            "system-awake",
                            "icons/coffee.svg",
                            crate::i18n::t("pill.system.coffee"),
                            s.awake.then_some(self.colors.amber),
                            false,
                            "",
                            |panel, cx| panel.toggle_awake(cx),
                            cx,
                        ))
                        .child(self.tile(
                            "system-mic",
                            if s.mic_muted == Some(true) { "icons/mic-off.svg" } else { "icons/mic.svg" },
                            if s.mic_muted == Some(true) { crate::i18n::t("pill.system.micOff") } else { crate::i18n::t("pill.system.microphone") },
                            (s.mic_muted == Some(true)).then_some(self.colors.danger),
                            false,
                            "",
                            |panel, cx| {
                                if let Some(muted) = panel.state.mic_muted {
                                    panel.state.mic_muted = Some(!muted);
                                    panel.backend.send(Cmd::MicMute(!muted));
                                    cx.notify();
                                }
                            },
                            cx,
                        ))
                        .child(self.tile(
                            "system-lock",
                            "icons/lock.svg",
                            crate::i18n::t("pill.system.lock"),
                            None,
                            lock_armed,
                            crate::i18n::t("pill.system.lockAsk"),
                            |panel, cx| {
                                if panel.confirm(Arm::Lock, cx) {
                                    cx.emit(SystemEvent::Action(crate::launcher::Action::Lock));
                                }
                            },
                            cx,
                        ))
                        .child(self.tile(
                            "system-sleep",
                            "icons/moon.svg",
                            crate::i18n::t("pill.system.sleep"),
                            None,
                            sleep_armed,
                            crate::i18n::t("pill.system.sleepAsk"),
                            |panel, cx| {
                                if panel.confirm(Arm::Sleep, cx) {
                                    cx.emit(SystemEvent::Action(crate::launcher::Action::Sleep));
                                }
                            },
                            cx,
                        )),
                )
                .child(div().h(px(10.)))
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .px(px(SIDE))
                        .relative()
                        .child(crate::glide::indicator(
                            crate::glide::named("system-tabs"),
                            self.tab.map(|tab| tab as u64),
                            crate::glide::Look { fill: gpui::white().opacity(0.12), radius: TABS_H / 2.0, edge: None },
                        ))
                        .child(self.tab_button(Tab::Audio, "icons/audio-lines.svg", crate::i18n::t("pill.system.tabAudio"), cx))
                        .child(self.tab_button(Tab::Displays, "icons/monitor.svg", crate::i18n::t("pill.system.tabDisplays"), cx))
                        .child(self.tab_button(Tab::Vitals, "icons/gauge.svg", crate::i18n::t("pill.system.tabVitals"), cx))
                        .child(self.tab_button(Tab::Procs, "icons/cpu.svg", crate::i18n::t("pill.system.tabProcs"), cx)),
                )
                .children(tab)
                .children(notice)
                .into_any_element()
        };

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|panel, _: &Dismiss, _, cx| panel.dismiss(cx)))
            .on_action(cx.listener(|panel, _: &ToggleMute, window, cx| panel.mute_key(window, cx)))
            .on_action(cx.listener(|panel, _: &FindApp, window, cx| panel.find_app(window, cx)))
            .on_mouse_move(cx.listener(|panel, event: &MouseMoveEvent, _, cx| {
                if let Some((key, big)) = panel.drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let value = panel.value_at(key, big, event.position.x);
                        panel.set(key, value, cx);
                    } else {
                        panel.drag = None;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|panel, _: &MouseUpEvent, _, cx| {
                    panel.drag = None;
                    cx.notify();
                }),
            )
            // Su altura final, no la del notch: mientras el notch crece o se
            // achica, el recorte destapa o tapa por abajo en vez de aplastar
            // lo de arriba.
            .w_full()
            .h(px(self.desired_height()))
            .flex_none()
            .flex()
            .flex_col()
            .font_family(crate::fonts::UI)
            .text_color(self.colors.text)
            .child(self.render_band(cx))
            .child(body)
    }
}

/// Las apps de Procesos que se muestran: las que coinciden con `query` (ya
/// plegada) y, si hay `order`, en ese orden (las que no estaban, al final).
fn pick_apps(apps: &[os::App], query: &str, order: Option<&[String]>) -> Vec<os::App> {
    let mut picked: Vec<os::App> = apps
        .iter()
        .filter(|app| {
            query.is_empty() || crate::clipboard::fold(&app.name).contains(query) || app.stem.contains(query)
        })
        .cloned()
        .collect();
    if let Some(order) = order {
        // `sort_by_key` es estable: las nuevas quedan al final en su orden.
        picked.sort_by_key(|app| order.iter().position(|s| *s == app.stem).unwrap_or(usize::MAX));
    }
    picked.truncate(MAX_ROWS);
    picked
}

/// Una fila de las pestañas: ícono, nombre, y lo que va a la derecha.
fn row(leading: AnyElement, name: SharedString, trailing: AnyElement) -> gpui::Div {
    div()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().child(leading))
        .child(
            div()
                .w(px(118.))
                .flex_none()
                .truncate()
                .text_size(px(12.))
                .child(name),
        )
        .child(trailing)
}

/// «Cerrar» y «Forzar» en una fila de Procesos.
fn proc_action(
    id: SharedString,
    label: &'static str,
    color: Hsla,
    ink: Hsla,
    filled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    hover_fx(SharedString::from(format!("{id}-fx")), move |h| {
        let bg = if filled {
            h.mix(color, color.opacity(0.85))
        } else {
            h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.1 + 0.06 * h.press))
        };
        div()
            .id(id)
            .h(px(22.))
            .px(px(9.))
            .flex()
            .items_center()
            .rounded(px(11.))
            .text_size(px(10.))
            .text_color(if filled { ink } else { color })
            .bg(bg)
            .cursor_pointer()
            .on_click(on_click)
            .child(label)
            .into_any_element()
    })
}

/// Una fila de Rendimiento: el gráfico al medio y, a la derecha, el valor y
/// su detalle (la temperatura, el total).
fn vital_row(leading: AnyElement, name: SharedString, graph: AnyElement, value: AnyElement, detail: AnyElement) -> gpui::Div {
    let trailing = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex_1().min_w_0().child(graph))
        .child(div().w(px(VALUE_W)).flex_none().flex().justify_end().child(value))
        .child(div().w(px(DETAIL_W)).flex_none().flex().justify_end().child(detail))
        .into_any_element();
    row(leading, name, trailing)
}

fn value(text: String, color: Hsla) -> AnyElement {
    div().text_size(px(12.)).text_color(color).child(text).into_any_element()
}

fn detail(text: String, color: Hsla) -> AnyElement {
    div().text_size(px(11.)).text_color(color).child(text).into_any_element()
}

/// Qué tan encendida se ve una barra según cuántos lugares lleva desde la
/// derecha (`slot`, con decimales mientras se corre): la recién llegada a
/// pleno y las viejas apagándose, sin saltos al pasar de un lugar al otro.
fn bar_alpha(slot: f32) -> f32 {
    if slot <= 0.0 {
        1.0
    } else if slot < 1.0 {
        crate::anim::lerp(1.0, 0.62, slot)
    } else {
        (0.62 * (1.0 - (slot - 1.0) / HISTORY as f32)).max(0.1)
    }
}

/// Dónde va (desde la derecha) la barra `k` (0 = la última lectura): entra
/// por el borde derecho y en `shift` = 1 queda en su lugar. La que antes era
/// la última queda justo donde estaba, así que nada salta al llegar una.
fn bar_slot(k: usize, shift: f32) -> f32 {
    k as f32 - 1.0 + shift
}

/// El último minuto de una medida (0 – 1) como barras que corren hacia la
/// izquierda.
fn strip(values: &VecDeque<f32>, shift: f32, color: Hsla) -> AnyElement {
    let bars = values.iter().rev().enumerate().map(|(k, v)| {
        let slot = bar_slot(k, shift);
        div()
            .absolute()
            .bottom_0()
            .right(px(PITCH * slot))
            .w(px(BAR_W))
            .h(px((v.clamp(0.0, 1.0) * STRIP_H).max(2.0)))
            .rounded(px(BAR_W / 2.0))
            .bg(color.opacity(bar_alpha(slot)))
    });
    div().relative().h(px(STRIP_H)).w_full().overflow_hidden().children(bars).into_any_element()
}

/// La red: lo que baja crece hacia arriba desde la línea del medio y lo que
/// sube, hacia abajo. `scale` son los bytes por segundo de la barra entera.
fn net_strip(down: &VecDeque<f32>, up: &VecDeque<f32>, scale: f32, shift: f32) -> AnyElement {
    let half = STRIP_H / 2.0 - 0.5;
    let bars = |values: &VecDeque<f32>, tint: u32, upward: bool| -> Vec<gpui::Div> {
        let color: Hsla = rgb(tint).into();
        values
            .iter()
            .rev()
            .enumerate()
            .map(|(k, v)| {
                let slot = bar_slot(k, shift);
                let h = (v / scale * half).clamp(1.0, half);
                let bar = div()
                    .absolute()
                    .right(px(PITCH * slot))
                    .w(px(BAR_W))
                    .h(px(h))
                    .rounded(px(BAR_W / 2.0))
                    .bg(color.opacity(bar_alpha(slot)));
                if upward {
                    bar.bottom(px(half + 1.0))
                } else {
                    bar.top(px(half + 1.0))
                }
            })
            .collect()
    };
    div()
        .relative()
        .h(px(STRIP_H))
        .w_full()
        .overflow_hidden()
        .children(bars(down, DOWN_TINT, true))
        .children(bars(up, UP_TINT, false))
        .into_any_element()
}

fn bytes(n: u64) -> String {
    let gb = n as f64 / 1024.0 / 1024.0 / 1024.0;
    if gb >= 1000.0 {
        format!("{:.1} TB", gb / 1024.0)
    } else if gb >= 100.0 {
        format!("{gb:.0} GB")
    } else if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", n >> 20)
    }
}

/// Bytes por segundo, corto.
fn rate(per_sec: f64) -> String {
    let kb = per_sec / 1024.0;
    if kb < 1.0 {
        "0 KB/s".into()
    } else if kb < 1000.0 {
        format!("{kb:.0} KB/s")
    } else if kb < 10.0 * 1024.0 {
        format!("{:.1} MB/s", kb / 1024.0)
    } else {
        format!("{:.0} MB/s", kb / 1024.0)
    }
}

/// «3 d 4 h», «5 h 12 min», «12 min».
fn since(secs: u64) -> String {
    let (days, hours, minutes) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    if days > 0 {
        format!("{days} d {hours} h")
    } else if hours > 0 {
        format!("{hours} h {minutes} min")
    } else {
        format!("{} min", minutes.max(1))
    }
}

/// «1440 × 900», con « · 60 Hz» si `hz`.
fn mode_label(mode: &Mode, hz: bool) -> String {
    if hz && mode.hz > 0 {
        format!("{} × {} · {} Hz", mode.width, mode.height, mode.hz)
    } else {
        format!("{} × {}", mode.width, mode.height)
    }
}

fn display_key(id: DisplayId) -> String {
    match id {
        DisplayId::Internal => "internal".into(),
        DisplayId::Monitor(handle) => handle.to_string(),
    }
}

/// Lo que se ofrece para una pantalla, como lo arma Ajustes: las
/// resoluciones de su misma forma (y, en macOS, de su misma clase, Retina o
/// no), a lo más cinco repartidas de chica a grande y siempre con la de
/// ahora; y las frecuencias de la resolución de ahora, de mayor a menor.
fn choices(modes: &[Mode], current: &Mode) -> (Vec<Mode>, Vec<Mode>) {
    let aspect = |m: &Mode| m.width as f32 / m.height.max(1) as f32;
    let shape = aspect(current);
    let mut sizes: Vec<Mode> = Vec::new();
    for m in modes.iter().filter(|m| m.hidpi == current.hidpi && (aspect(m) - shape).abs() < 0.02) {
        match sizes.iter_mut().find(|s| s.width == m.width && s.height == m.height) {
            // Por tamaño, la de la frecuencia de ahora; si no hay, la más alta.
            Some(s) => {
                if s.hz != current.hz && (m.hz == current.hz || m.hz > s.hz) {
                    *s = m.clone();
                }
            }
            None => sizes.push(m.clone()),
        }
    }
    match sizes.iter_mut().find(|s| s.width == current.width && s.height == current.height) {
        Some(s) => *s = current.clone(),
        None => sizes.push(current.clone()),
    }
    sizes.sort_by_key(|m| (m.width, m.height));
    let sizes = spread(sizes, current, MAX_CHIPS);
    let mut rates: Vec<Mode> = modes
        .iter()
        .filter(|m| m.width == current.width && m.height == current.height && m.hidpi == current.hidpi && m.hz > 0)
        .cloned()
        .collect();
    if !rates.iter().any(|m| m.hz == current.hz) && current.hz > 0 {
        rates.push(current.clone());
    }
    rates.sort_by(|a, b| b.hz.cmp(&a.hz));
    rates.dedup_by_key(|m| m.hz);
    rates.truncate(MAX_CHIPS);
    (sizes, rates)
}

/// A lo más `max` de una lista ordenada, repartidas de punta a punta y sin
/// perder la de ahora.
fn spread(list: Vec<Mode>, current: &Mode, max: usize) -> Vec<Mode> {
    if list.len() <= max || max < 2 {
        return list;
    }
    let last = list.len() - 1;
    let mut picks: Vec<usize> = (0..max).map(|i| (i * last + (max - 1) / 2) / (max - 1)).collect();
    if let Some(here) = list.iter().position(|m| m.width == current.width && m.height == current.height) {
        if !picks.contains(&here) {
            // Reemplaza a la más cercana, sin tocar las puntas si se puede.
            let nearest = (0..picks.len()).min_by_key(|&i| (picks[i] as isize - here as isize).unsigned_abs()).unwrap_or(0);
            picks[nearest] = here;
        }
    }
    picks.sort_unstable();
    picks.dedup();
    picks.into_iter().map(|i| list[i].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(stem: &str, name: &str) -> os::App {
        os::App {
            stem: stem.into(),
            name: name.into(),
            path: None,
            cpu: 0.0,
            ram: 0,
        }
    }

    fn mode(width: u32, height: u32, hz: u32, hidpi: bool) -> Mode {
        Mode { width, height, hz, hidpi, key: 0 }
    }

    #[test]
    fn resoluciones_como_ajustes_del_mac() {
        // Un MacBook Air M1: las Retina de su misma forma, sin las de otra
        // proporción ni las que no son Retina.
        let modes = [
            mode(825, 525, 60, true),
            mode(1024, 640, 60, true),
            mode(1024, 768, 60, false),
            mode(1280, 800, 60, true),
            mode(1280, 800, 60, false),
            mode(1440, 900, 60, true),
            mode(1680, 1050, 60, true),
            mode(2560, 1600, 60, false),
        ];
        let (sizes, rates) = choices(&modes, &mode(1440, 900, 60, true));
        let widths: Vec<u32> = sizes.iter().map(|m| m.width).collect();
        assert_eq!(widths, [1024, 1280, 1440, 1680]);
        assert!(sizes.iter().all(|m| m.hidpi));
        // Una sola frecuencia: no se ofrece la fila.
        assert_eq!(rates.len(), 1);
    }

    #[test]
    fn un_monitor_con_muchas_resoluciones_ofrece_cinco_y_la_de_ahora() {
        let mut modes = Vec::new();
        for (w, h) in [(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1440), (3200, 1800), (3840, 2160), (1280, 1024)] {
            for hz in [60, 120, 144] {
                modes.push(mode(w, h, hz, false));
            }
        }
        let current = mode(1600, 900, 144, false);
        let (sizes, rates) = choices(&modes, &current);
        assert_eq!(sizes.len(), MAX_CHIPS);
        assert!(sizes.contains(&current), "{sizes:?}");
        assert_eq!(sizes.first().map(|m| m.width), Some(1280));
        assert_eq!(sizes.last().map(|m| m.width), Some(3840));
        // Cada tamaño con la frecuencia de ahora, y la 5:4 fuera.
        assert!(sizes.iter().all(|m| m.hz == 144 && m.height != 1024));
        let hz: Vec<u32> = rates.iter().map(|m| m.hz).collect();
        assert_eq!(hz, [144, 120, 60]);
    }

    #[test]
    fn la_tira_no_salta_al_llegar_una_lectura() {
        // La que era la última (k = 0 al terminar de correrse) queda en el
        // mismo lugar y con el mismo brillo cuando pasa a ser k = 1.
        assert_eq!(bar_slot(0, 1.0), bar_slot(1, 0.0));
        assert_eq!(bar_alpha(bar_slot(0, 1.0)), bar_alpha(bar_slot(1, 0.0)));
        // Y se apaga de a poco, sin saltos, mientras se corre.
        let steps: Vec<f32> = (0..=10).map(|i| bar_alpha(bar_slot(1, i as f32 / 10.0))).collect();
        assert!(steps.windows(2).all(|w| w[1] <= w[0] && w[0] - w[1] < 0.05), "{steps:?}");
    }

    #[test]
    fn formatos_cortos() {
        assert_eq!(rate(500.0), "0 KB/s");
        assert_eq!(rate(40.0 * 1024.0), "40 KB/s");
        assert_eq!(rate(1.3 * 1024.0 * 1024.0), "1.3 MB/s");
        assert_eq!(bytes(9_542_713_344), "8.9 GB");
        assert_eq!(bytes(245_107_195_904), "228 GB");
        assert_eq!(since(127_148), "1 d 11 h");
        assert_eq!(since(30), "1 min");
    }

    #[test]
    fn busca_por_nombre_sin_tildes() {
        let apps = [app("code", "Visual Studio Code"), app("winword", "Microsoft Word"), app("zen", "Zen")];
        let names = |q: &str| -> Vec<String> {
            pick_apps(&apps, &crate::clipboard::fold(q), None).into_iter().map(|a| a.stem).collect()
        };
        assert_eq!(names("word"), ["winword"]);
        assert_eq!(names("VISUAL"), ["code"]);
        // Por el ejecutable también.
        assert_eq!(names("winw"), ["winword"]);
        assert_eq!(names(""), ["code", "winword", "zen"]);
        assert!(names("nada").is_empty());
    }

    #[test]
    fn con_el_cursor_encima_el_orden_no_cambia() {
        // La lectura nueva trae a Zen primero; la lista mantiene el orden
        // anterior y lo nuevo va al final.
        let fresh = [app("zen", "Zen"), app("new", "Nueva"), app("code", "Code")];
        let order = vec!["code".to_string(), "zen".to_string()];
        let stems: Vec<String> = pick_apps(&fresh, "", Some(&order)).into_iter().map(|a| a.stem).collect();
        assert_eq!(stems, ["code", "zen", "new"]);
    }
}
