//! Sistema como notch: centro de control.
//!
//! Arriba, lo de todos los días: volumen y brillo en dos sliders gruesos, y
//! silencio, café, bloquear y suspender. Debajo, tres pestañas que se abren
//! a pedido (al abrir el notch quedan cerradas, para que sea compacto):
//! - **Audio**: la salida (parlantes, audífonos…), quién usa el micrófono o
//!   la cámara, y el volumen de cada app que tiene sonido.
//! - **Pantallas**: el brillo de cada pantalla.
//! - **Procesos**: CPU, RAM y lo que más consume, para cerrarlo. Un buscador
//!   filtra por nombre, y mientras el cursor está sobre la lista el orden se
//!   congela: las filas no cambian de lugar justo antes del clic.
//!
//! Lo que no se deshace con un clic (bloquear, suspender, forzar el cierre)
//! pide confirmación: el primer clic arma el botón y el segundo lo ejecuta;
//! si no se confirma en 3 s, se desarma solo.
//!
//! Lo que habla con Windows está en `os.rs`, en su propio hilo.

pub(crate) mod os;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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
use os::{Backend, Cmd, DisplayId, Snapshot, Want};

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

actions!(system_panel, [Dismiss, ToggleMute, FindApp]);

const KEY_CONTEXT: &str = "SystemPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("m", ToggleMute, context),
        KeyBinding::new("ctrl-f", FindApp, context),
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
    Procs,
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
                "Buscar una app…",
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
            // `PILL_SYSTEM_TAB=audio|pantallas|procesos`: abre esa pestaña,
            // para revisar el diseño con una captura.
            tab: match std::env::var("PILL_SYSTEM_TAB").as_deref() {
                Ok("audio") => Some(Tab::Audio),
                Ok("pantallas") => Some(Tab::Displays),
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
        }
    }

    /// Al abrir: leer el estado y seguir leyéndolo mientras esté abierto.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        let first = !self.active;
        self.active = true;
        self.armed = None;
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
            Some(Tab::Procs) => self.shown_apps().len().clamp(1, MAX_ROWS),
        }
    }

    pub fn desired_height(&self) -> f32 {
        let base = BAND_H + 4.0 + SLIDER_H * 2.0 + GAP + 12.0 + TILE_H + 10.0 + TABS_H + SIDE;
        let content = match self.tab {
            None => 0.0,
            Some(Tab::Procs) => GAP + SEARCH_H + PROCS_HEADER_H + self.rows() as f32 * ROW_H,
            Some(Tab::Audio) => {
                let uses = if self.privacy.uses().is_empty() { 0.0 } else { ROW_H };
                GAP + uses + SECTION_H * 2.0 + self.rows() as f32 * ROW_H
            }
            Some(_) => GAP + self.rows() as f32 * ROW_H,
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
        self.tab = if self.tab == Some(tab) { None } else { Some(tab) };
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
                format!("{percent} % · enchufado")
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
            .child(div().text_size(px(12.)).child("Sistema"))
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
            let (rest, over) = if selected { (0.12, 0.15) } else { (0.0, 0.07) };
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

    fn render_tab(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
                        (true, true) => "la cámara y el micrófono",
                        (true, false) => "la cámara",
                        _ => "el micrófono",
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
                                    .child(format!("{} usa {what}", names.join(", "))),
                            )
                            .into_any_element(),
                    );
                }
                sections.push(self.section("Salida"));
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
                sections.push(self.section("Apps"));
                let apps = self.state.audio.clone();
                if apps.is_empty() {
                    sections.push(self.empty("Ninguna app tiene el sonido abierto.").into_any_element());
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
                let displays = self.state.displays.clone();
                if displays.is_empty() {
                    self.empty("No encontré pantallas.").into_any_element()
                } else {
                    let rows: Vec<AnyElement> = displays
                        .iter()
                        .map(|display| {
                            let icon = if display.id == DisplayId::Internal {
                                "icons/laptop.svg"
                            } else {
                                "icons/monitor.svg"
                            };
                            let name: SharedString = if display.primary {
                                format!("{} · principal", display.name).into()
                            } else {
                                display.name.clone().into()
                            };
                            let trailing = if display.brightness.is_some() {
                                self.mini(Key::Brightness(display.id), cx)
                            } else {
                                div()
                                    .flex_1()
                                    .text_size(px(11.))
                                    .text_color(self.colors.faint)
                                    .child("No deja cambiar el brillo")
                                    .into_any_element()
                            };
                            self.row(self.glyph(icon), name, trailing).into_any_element()
                        })
                        .collect();
                    div().flex().flex_col().children(rows).into_any_element()
                }
            }
            Tab::Procs => self.render_procs(cx),
        };
        let key = match tab {
            Tab::Audio => "audio",
            Tab::Displays => "pantallas",
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
            return self.empty("Midiendo…").into_any_element();
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
                el.child(div().flex_none().text_size(px(10.)).text_color(faint).child("orden en pausa"))
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
                    let trailing = div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .w(px(44.))
                                .text_size(px(11.))
                                .text_color(if cpu >= 20.0 { amber } else { muted })
                                .child(format!("{cpu:.0} %")),
                        )
                        .child(div().w(px(56.)).text_size(px(11.)).text_color(muted).child(ram))
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .opacity(reveal)
                                // Mientras no se ven, que no se puedan apretar.
                                .when(reveal < 0.05, |el| el.invisible())
                                .child(proc_action(
                                    format!("close-{id}").into(),
                                    "Cerrar",
                                    text,
                                    ink,
                                    false,
                                    close,
                                ))
                                .child(proc_action(
                                    format!("force-{id}").into(),
                                    if armed { "¿Forzar?" } else { "Forzar" },
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
                    .when(empty_search, |el| el.child(self.empty("Ninguna app coincide."))),
            )
            .into_any_element()
    }

    /// Esc: con algo buscado, primero borra la búsqueda.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.tab == Some(Tab::Procs) && self.searching() {
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            None => "Esta pantalla no deja cambiar el brillo".into(),
        };
        let lock_armed = self.is_armed(&Arm::Lock);
        let sleep_armed = self.is_armed(&Arm::Sleep);
        let tab = self.render_tab(cx);
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
            .font_family("Segoe UI")
            .text_color(self.colors.text)
            .child(self.render_band(cx))
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
                        "Silencio",
                        s.muted.then_some(self.colors.fill),
                        false,
                        "",
                        |panel, cx| panel.toggle_mute(cx),
                        cx,
                    ))
                    .child(self.tile(
                        "system-awake",
                        "icons/coffee.svg",
                        "Café",
                        s.awake.then_some(self.colors.amber),
                        false,
                        "",
                        |panel, cx| panel.toggle_awake(cx),
                        cx,
                    ))
                    .child(self.tile(
                        "system-mic",
                        if s.mic_muted == Some(true) { "icons/mic-off.svg" } else { "icons/mic.svg" },
                        if s.mic_muted == Some(true) { "Mic. apagado" } else { "Micrófono" },
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
                        "Bloquear",
                        None,
                        lock_armed,
                        "¿Bloquear?",
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
                        "Suspender",
                        None,
                        sleep_armed,
                        "¿Suspender?",
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
                    .child(self.tab_button(Tab::Audio, "icons/audio-lines.svg", "Audio", cx))
                    .child(self.tab_button(Tab::Displays, "icons/monitor.svg", "Pantallas", cx))
                    .child(self.tab_button(Tab::Procs, "icons/cpu.svg", "Procesos", cx)),
            )
            .children(tab)
            .children(notice)
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
