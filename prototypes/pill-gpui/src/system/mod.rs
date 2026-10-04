//! Sistema como notch: centro de control.
//!
//! Arriba, lo de todos los días: volumen y brillo en dos sliders gruesos, y
//! silencio, café, bloquear y suspender. Debajo, tres pestañas que se abren
//! a pedido (al abrir el notch quedan cerradas, para que sea compacto):
//! - **Audio**: la salida (parlantes, audífonos…), quién usa el micrófono o
//!   la cámara, y el volumen de cada app que tiene sonido.
//! - **Pantallas**: el brillo de cada pantalla.
//! - **Procesos**: CPU, RAM y lo que más consume, para cerrarlo.
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
    actions, canvas, div, img, prelude::*, px, rgb, svg, AnyElement, App, Bounds, ClickEvent,
    Context, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, ScrollWheelEvent, SharedString, Window,
};

use crate::clipboard::BAND_H;
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

actions!(system_panel, [Dismiss, ToggleMute]);

const KEY_CONTEXT: &str = "SystemPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("m", ToggleMute, context),
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
        Self {
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
            procs: self.tab == Some(Tab::Procs),
        }
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

    fn rows(&self) -> usize {
        match self.tab {
            None => 0,
            Some(Tab::Audio) => self.state.audio.len().clamp(1, MAX_ROWS) + self.state.outputs.len(),
            Some(Tab::Displays) => self.state.displays.len().max(1),
            Some(Tab::Procs) => self
                .state
                .procs
                .as_ref()
                .map_or(1, |p| p.apps.len().clamp(1, MAX_ROWS)),
        }
    }

    pub fn desired_height(&self) -> f32 {
        let base = BAND_H + 4.0 + SLIDER_H * 2.0 + GAP + 12.0 + TILE_H + 10.0 + TABS_H + SIDE;
        let content = match self.tab {
            None => 0.0,
            Some(Tab::Procs) => GAP + PROCS_HEADER_H + self.rows() as f32 * ROW_H,
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
            .child(
                div()
                    .id("system-pin")
                    .size(px(26.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(13.))
                    .when(self.pinned, |el| el.bg(text.opacity(0.14)))
                    .hover(|el| el.bg(text.opacity(0.08)))
                    .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| {
                        panel.pinned = !panel.pinned;
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path("icons/pin.svg")
                            .size(px(13.))
                            .text_color(if self.pinned { text } else { faint }),
                    ),
            )
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
        div()
            .id(id)
            .flex_1()
            .h(px(TILE_H))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(5.))
            .rounded(px(16.))
            .bg(bg.unwrap_or(c.track))
            .when(bg.is_none(), |el| el.hover(|el| el.bg(gpui::white().opacity(0.14))))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| on_click(panel, cx)))
            .child(svg().path(icon).size(px(18.)).text_color(fg))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(if bg.is_some() { c.ink } else { c.muted })
                    .child(if armed { armed_label } else { label }),
            )
    }

    fn tab_button(&self, tab: Tab, icon: &'static str, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.colors;
        let selected = self.tab == Some(tab);
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
            .text_color(if selected { c.text } else { c.muted })
            .when(selected, |el| el.bg(gpui::white().opacity(0.12)))
            .hover(|el| el.bg(gpui::white().opacity(0.07)))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.pick_tab(tab, cx)))
            .child(svg().path(icon).size(px(13.)).text_color(if selected { c.text } else { c.muted }))
            .child(label)
    }

    /// Una fila de las pestañas: ícono, nombre, y lo que va a la derecha.
    fn row(&self, leading: AnyElement, name: SharedString, trailing: AnyElement) -> gpui::Div {
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
                    sections.push(
                        div()
                            .id(SharedString::from(format!("output-{}", output.id)))
                            .rounded(px(10.))
                            .hover(|el| el.bg(gpui::white().opacity(0.06)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                for o in &mut panel.state.outputs {
                                    o.default = o.id == id;
                                }
                                panel.backend.send(Cmd::Output(id.clone()));
                                panel.refresh(cx);
                                cx.notify();
                            }))
                            .child(line)
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
        Some(
            div()
                .px(px(SIDE + 4.))
                .pt(px(GAP))
                .child(body)
                .into_any_element(),
        )
    }

    fn render_procs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(procs) = self.state.procs.clone() else {
            return self.empty("Midiendo…").into_any_element();
        };
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
        let apps = procs.apps.clone();
        let rows: Vec<AnyElement> = apps
            .iter()
            .take(MAX_ROWS)
            .map(|app| {
                let leading = self.app_icon(app.path.as_ref(), "icons/activity.svg", cx);
                let group: SharedString = format!("proc-{}", app.stem).into();
                let armed = self.is_armed(&Arm::Force(app.stem.clone()));
                let ram = if app.ram >= 1 << 30 {
                    format!("{:.1} GB", gb(app.ram))
                } else {
                    format!("{} MB", app.ram >> 20)
                };
                let stem = app.stem.clone();
                let stem_force = app.stem.clone();
                let c = &self.colors;
                let action = |id: SharedString, label: &'static str, color: Hsla, filled: bool| {
                    div()
                        .id(id)
                        .h(px(22.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .rounded(px(11.))
                        .text_size(px(10.))
                        .text_color(if filled { c.ink } else { color })
                        .when(filled, |el| el.bg(color))
                        .when(!filled, |el| el.hover(|el| el.bg(gpui::white().opacity(0.1))))
                        .cursor_pointer()
                        .child(label)
                };
                let trailing = div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(44.))
                            .text_size(px(11.))
                            .text_color(if app.cpu >= 20.0 { c.amber } else { c.muted })
                            .child(format!("{:.0} %", app.cpu)),
                    )
                    .child(div().w(px(56.)).text_size(px(11.)).text_color(c.muted).child(ram))
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .when(!armed, |el| el.invisible().group_hover(group.clone(), |el| el.visible()))
                            .child(
                                action(format!("close-{stem}").into(), "Cerrar", c.text, false).on_click(
                                    cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                        panel.backend.send(Cmd::Close(stem.clone()));
                                        panel.refresh(cx);
                                    }),
                                ),
                            )
                            .child(
                                action(
                                    format!("force-{stem_force}").into(),
                                    if armed { "¿Forzar?" } else { "Forzar" },
                                    c.danger,
                                    armed,
                                )
                                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    if panel.confirm(Arm::Force(stem_force.clone()), cx) {
                                        panel.backend.send(Cmd::Force(stem_force.clone()));
                                        panel.refresh(cx);
                                    }
                                })),
                            ),
                    )
                    .into_any_element();
                self.row(leading, app.name.clone().into(), trailing)
                    .group(group)
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .child(header)
            .children(rows)
            .into_any_element()
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
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(SystemEvent::Close)))
            .on_action(cx.listener(|panel, _: &ToggleMute, _, cx| panel.toggle_mute(cx)))
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
            .size_full()
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
