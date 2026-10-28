//! La pestaña «Dispositivos» del panel de la terminal: el emulador de Android dentro
//! de Atic Code y el simulador de iOS.
//!
//! Primero dice qué falta en la máquina y lo consigue con un botón (en la terminal, a
//! la vista; la pestaña se revisa sola hasta que queda listo). Con todo listo, lista
//! los teléfonos virtuales: uno se enciende sin ventana y su pantalla se ve aquí, con
//! toques, teclado y la rueda (`emulator.rs`). Los que ya estaban encendidos (por
//! Android Studio, con gRPC) también se pueden ver.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{
    canvas, div, img, prelude::*, px, App, Bounds, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, RenderImage, ScrollWheelEvent, SharedString, Window,
};

use super::devices::{self, Android, Ios, Missing, Running};
use super::emulator::{self, Event as EmuEvent, Input, Session};
use super::style::t;
use crate::i18n::{t as tr, tf};

/// La pestaña está escondida: no tiene botón ni entrada en los menús. Se ve abriendo
/// Atic con `ATIC_DEVICES=1` (quedó como experimento).
pub fn enabled() -> bool {
    std::env::var_os("ATIC_DEVICES").is_some()
}

/// Mientras algo se instala en la terminal, cada cuánto se revisa y hasta cuándo.
const POLL: Duration = Duration::from_secs(3);
const POLL_FOR: Duration = Duration::from_secs(30 * 60);

/// Lo que la pestaña le pide a la terminal.
pub enum DevicesEvent {
    /// Correr un comando a la vista (instalar lo que falta).
    Run { command: String, title: String },
}

/// Un teléfono que se está viendo.
struct Live {
    avd: String,
    /// La resolución del teléfono (para llevar los toques a sus píxeles).
    device: (u32, u32),
    session: Session,
    image: Option<Arc<RenderImage>>,
    /// Los cuadros ya reemplazados: se sueltan de la GPU en el próximo cuadro.
    retired: Vec<Arc<RenderImage>>,
    /// El proceso del emulador, si lo encendió Atic (se apaga al cerrar).
    child: Option<std::process::Child>,
    running: Option<Running>,
    connected: bool,
    /// Tiene teclado: las teclas van por gRPC; si no, por `adb shell input`.
    keyboard: bool,
    error: Option<String>,
    started: Instant,
    /// El dedo está apoyado (se arrastra).
    pressed: bool,
}

pub struct DevicesView {
    focus: FocusHandle,
    android: Option<Android>,
    ios: Option<Ios>,
    checking: bool,
    /// Revisando cada `POLL` mientras se instala algo.
    polling_until: Option<Instant>,
    live: Option<Live>,
    /// Dónde se dibuja la pantalla (para el tamaño de los cuadros y los toques).
    screen: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    notice: Option<String>,
    /// Los teléfonos que se están apagando (guardan su estado: puede tardar).
    stopping: Vec<String>,
}

impl EventEmitter<DevicesEvent> for DevicesView {}

impl Focusable for DevicesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DevicesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            focus: cx.focus_handle(),
            android: None,
            ios: None,
            checking: false,
            polling_until: None,
            live: None,
            screen: Default::default(),
            notice: None,
            stopping: Vec::new(),
        };
        view.check(cx);
        view
    }

    /// Lo que se ve en la pestaña.
    pub fn label(&self) -> String {
        match &self.live {
            Some(live) => live.avd.clone(),
            None => tr("pill.code.devices.title").to_string(),
        }
    }

    /// Revisa qué hay instalado (lee el disco y lanza procesos: fuera de la interfaz).
    fn check(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (android, ios) = cx.background_spawn(async move { (devices::check_android(), devices::check_ios()) }).await;
            let _ = this.update(cx, |view, cx| {
                view.checking = false;
                // El SDK y una imagen, sin teléfono: se crea solo (lo que hace avdmanager).
                if let (Some(sdk), Some(image), true) = (&android.sdk, &android.image, android.avds.is_empty() && android.emulator.is_some()) {
                    if devices::create_avd(&devices::avd_home(), sdk, image, devices::DEFAULT_AVD).is_ok() {
                        view.notice = Some(tf("pill.code.devices.created", &[("name", &devices::DEFAULT_AVD)]));
                        view.android = None;
                        view.check(cx);
                        return;
                    }
                }
                // Ya no está encendido: terminó de apagarse.
                view.stopping.retain(|avd| android.running.iter().any(|r| &r.avd == avd));
                if android.missing().is_none() && view.stopping.is_empty() {
                    view.polling_until = None;
                }
                view.android = Some(android);
                view.ios = Some(ios);
                cx.notify();
            });
        })
        .detach();
    }

    /// Instala lo que falta en la terminal y revisa cada `POLL` hasta que esté.
    fn install(&mut self, missing: Missing, cx: &mut Context<Self>) {
        let Some(android) = &self.android else {
            return;
        };
        match devices::install_command(android, missing) {
            Some(command) => {
                let title = tr(if missing == Missing::Sdk { "pill.code.devices.installStudio" } else { "pill.code.devices.preparePhone" }).to_string();
                cx.emit(DevicesEvent::Run { command, title });
            }
            None if missing == Missing::Sdk => cx.open_url(devices::STUDIO_URL),
            // Sin `sdkmanager`: el asistente de Android Studio lo hace (Device Manager).
            None => {
                if let Some(studio) = &android.studio {
                    open_path(studio);
                } else {
                    cx.open_url(devices::STUDIO_URL);
                }
            }
        }
        self.poll(cx);
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let first = self.polling_until.is_none();
        self.polling_until = Some(Instant::now() + POLL_FOR);
        if !first {
            return;
        }
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let go = this.update(cx, |view, cx| {
                let until = view.polling_until.filter(|until| Instant::now() < *until);
                if until.is_some() {
                    view.check(cx);
                } else {
                    view.polling_until = None;
                }
                until.is_some()
            });
            if !go.unwrap_or(false) {
                break;
            }
        })
        .detach();
    }

    /// Enciende `avd` sin ventana (o se conecta si ya estaba encendido) y muestra su pantalla.
    fn start(&mut self, avd: &str, cx: &mut Context<Self>) {
        let Some(android) = &self.android else {
            return;
        };
        let device = android.avds.iter().find(|a| a.name == avd).map_or((1080, 2400), |a| a.size);
        let already = android.running.iter().find(|r| r.avd == avd).cloned();
        let (port, child) = match &already {
            Some(running) => match running.grpc {
                Some(port) => (port, None),
                None => {
                    self.notice = Some(tr("pill.code.devices.noGrpc").to_string());
                    cx.notify();
                    return;
                }
            },
            None => {
                let (Some(emulator), Some(port)) = (android.emulator.clone(), devices::free_port()) else {
                    return;
                };
                devices::ensure_keyboard(avd);
                match devices::launch(&emulator, avd, port) {
                    Ok(child) => (port, Some(child)),
                    Err(error) => {
                        self.notice = Some(tf("pill.code.devices.launchFailed", &[("error", &error)]));
                        cx.notify();
                        return;
                    }
                }
            }
        };
        self.connect(avd.to_string(), device, port, child, already, cx);
    }

    /// Se conecta a su API gRPC; recién encendido, todavía no escucha: se reintenta.
    fn connect(&mut self, avd: String, device: (u32, u32), port: u16, child: Option<std::process::Child>, running: Option<Running>, cx: &mut Context<Self>) {
        let (events, mut inbox) = futures::channel::mpsc::unbounded();
        let size = self.frame_size(device);
        let session = Session::connect(port, size, events);
        let keyboard = devices::keyboard_enabled(&avd);
        self.live = Some(Live {
            keyboard,
            avd: avd.clone(),
            device,
            session,
            image: None,
            retired: Vec::new(),
            child,
            running,
            connected: false,
            error: None,
            started: Instant::now(),
            pressed: false,
        });
        self.notice = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(event) = inbox.next().await {
                let again = this.update(cx, |view, cx| view.on_event(event, cx));
                match again {
                    Ok(Some(retry)) => {
                        cx.background_executor().timer(Duration::from_millis(1500)).await;
                        let _ = this.update(cx, |view, cx| {
                            if let Some(live) = view.live.take().filter(|l| l.avd == retry) {
                                view.connect(live.avd, live.device, port, live.child, live.running, cx);
                            }
                        });
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .detach();
    }

    /// Lo que avisó la sesión. `Some(avd)`: se cortó mientras arrancaba, reintentar.
    fn on_event(&mut self, event: EmuEvent, cx: &mut Context<Self>) -> Option<String> {
        let live = self.live.as_mut()?;
        match event {
            EmuEvent::Connected => live.connected = true,
            EmuEvent::Frame => {
                let frame = live.session.shared.frame.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(frame) = frame {
                    if let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra) {
                        let image = Arc::new(RenderImage::new([image::Frame::new(buffer)]));
                        if let Some(old) = live.image.replace(image) {
                            live.retired.push(old);
                        }
                    }
                }
            }
            EmuEvent::Closed(error) => {
                // Arrancando (hasta 3 min): el emulador todavía no abre su puerto.
                let booting = live.image.is_none() && live.started.elapsed() < Duration::from_secs(180) && !child_exited(live);
                if booting {
                    return Some(live.avd.clone());
                }
                live.error = Some(if error.is_empty() { tr("pill.code.devices.closed").to_string() } else { error });
            }
        }
        cx.notify();
        None
    }

    /// El tamaño al que se piden los cuadros: el área de la pantalla en píxeles reales,
    /// con la proporción del teléfono (más chico: menos datos y menos CPU).
    fn frame_size(&self, device: (u32, u32)) -> (u32, u32) {
        let Some(area) = self.screen.get() else {
            return (device.0 / 3, device.1 / 3);
        };
        let scale = 2.0;
        let (w, h) = fit(device, (f32::from(area.size.width) * scale, f32::from(area.size.height) * scale));
        (w.max(64.) as u32, h.max(64.) as u32)
    }

    /// Apaga el teléfono (si lo encendió Atic) y vuelve a la lista.
    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(mut live) = self.live.take() {
            self.stopping.push(live.avd.clone());
            self.poll(cx);
            let adb = self.android.as_ref().and_then(|a| a.adb.clone());
            match (live.child.take(), live.running.clone()) {
                (Some(child), _) => {
                    std::thread::spawn(move || shut_down(adb, child));
                }
                // Encendido antes (por Atic en otra sesión o por Android Studio): por adb.
                (None, Some(running)) => {
                    std::thread::spawn(move || devices::stop(adb.as_deref(), &running));
                }
                (None, None) => {}
            }
        }
        self.check(cx);
    }

    /// Deja de mirar sin apagar (un teléfono que encendió otra herramienta).
    fn detach(&mut self, cx: &mut Context<Self>) {
        self.live = None;
        self.check(cx);
    }

    fn send(&self, input: Input) {
        let Some(live) = &self.live else {
            return;
        };
        // Sin teclado, el emulador descarta las teclas de gRPC: van por adb.
        if !live.keyboard {
            let key = match &input {
                Input::Key(name) => devices::android_keycode(name).map(|code| (Some(code), None)),
                Input::Text(text) => Some((None, Some(text.clone()))),
                _ => None,
            };
            let adb = self.android.as_ref().and_then(|a| a.adb.clone());
            let serial = live.running.as_ref().and_then(|r| r.console).map(|port| format!("emulator-{port}"));
            if let (Some((code, text)), Some(adb), Some(serial)) = (key, adb, serial) {
                std::thread::spawn(move || devices::adb_input(&adb, &serial, code, text.as_deref()));
                return;
            }
        }
        live.session.send(input);
    }

    /// Un punto de la ventana en píxeles del teléfono, si cae en su pantalla.
    fn device_point(&self, at: gpui::Point<Pixels>) -> Option<(i32, i32)> {
        let live = self.live.as_ref()?;
        let area = self.screen.get()?;
        let (w, h) = fit(live.device, (f32::from(area.size.width), f32::from(area.size.height)));
        let left = f32::from(area.origin.x) + (f32::from(area.size.width) - w) / 2.;
        let top = f32::from(area.origin.y) + (f32::from(area.size.height) - h) / 2.;
        emulator::to_device((f32::from(at.x) - left, f32::from(at.y) - top), (w, h), live.device)
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.live.is_none() {
            return;
        }
        let keys = &event.keystroke;
        if keys.modifiers.platform || keys.modifiers.control {
            return;
        }
        let named = match keys.key.as_str() {
            "enter" => Some("Enter"),
            "backspace" => Some("Backspace"),
            "delete" => Some("Delete"),
            "tab" => Some("Tab"),
            "escape" => Some("GoBack"),
            "left" => Some("ArrowLeft"),
            "right" => Some("ArrowRight"),
            "up" => Some("ArrowUp"),
            "down" => Some("ArrowDown"),
            "home" => Some("Home"),
            "end" => Some("End"),
            _ => None,
        };
        match (named, keys.key_char.as_ref()) {
            (Some(key), _) => self.send(Input::Key(key.into())),
            (None, Some(text)) => self.send(Input::Text(text.clone())),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Drop for DevicesView {
    fn drop(&mut self) {
        // Un teléfono que encendió Atic no se queda corriendo solo: se apaga guardando su
        // estado (arranca rápido la próxima vez) y, si no responde, se termina.
        if let Some(live) = self.live.as_mut() {
            if let Some(child) = live.child.take() {
                let adb = self.android.as_ref().and_then(|a| a.adb.clone());
                std::thread::spawn(move || shut_down(adb, child));
            }
        }
    }
}

/// Apaga un emulador que encendió Atic: `adb emu kill` y, si a los 15 s sigue, se mata.
fn shut_down(adb: Option<std::path::PathBuf>, mut child: std::process::Child) {
    if let Some(running) = devices::list_running().into_iter().find(|r| r.pid == child.id()) {
        devices::stop(adb.as_deref(), &running);
    }
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let _ = child.kill();
}

/// `device` escalado para caber en `area`, con su proporción.
fn fit(device: (u32, u32), area: (f32, f32)) -> (f32, f32) {
    let (dw, dh) = (device.0 as f32, device.1 as f32);
    let scale = (area.0 / dw).min(area.1 / dh);
    (dw * scale, dh * scale)
}

fn child_exited(live: &mut Live) -> bool {
    live.child.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(Some(_))))
}

fn open_path(path: &std::path::Path) {
    let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

impl Render for DevicesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Los cuadros ya reemplazados se sueltan de la GPU (si no, cada uno quedaba ahí).
        if let Some(live) = self.live.as_mut() {
            for old in live.retired.drain(..) {
                let _ = window.drop_image(old);
            }
        }
        // El área cambió de tamaño: los cuadros se piden al nuevo.
        if let Some(live) = &self.live {
            let size = self.frame_size(live.device);
            live.session.send(Input::Size(size.0, size.1));
        }
        let body = if self.live.is_some() { self.live_view(cx) } else { self.list_view(cx) };
        div()
            .id("devices")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .flex()
            .flex_col()
            .child(body)
    }
}

impl DevicesView {
    /// El teléfono en vivo: su barra (atrás, inicio, apps, apagar) y la pantalla.
    fn live_view(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = t();
        let live = self.live.as_ref().expect("live");
        let launched = live.child.is_some();
        let button = |id: &'static str, icon: &'static str, tip: &'static str| gpui_m3::IconButton::new(id, icon).size(px(28.)).tooltip(tr(tip));
        let key = |name: &'static str| cx.listener(move |view: &mut Self, _: &ClickEvent, _, _| view.send(Input::Key(name.into())));
        let bar = div()
            .flex_none()
            .h(px(36.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(12.5)).text_color(t.muted).child(live.avd.clone()))
            .child(button("dev-back", "arrow-left", "pill.code.devices.back").on_click(key("GoBack")))
            .child(button("dev-home", "box", "pill.code.devices.home").on_click(key("GoHome")))
            .child(button("dev-apps", "layers", "pill.code.devices.apps").on_click(key("AppSwitch")))
            .child(button("dev-stop", "stop", "pill.code.devices.stop").on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.stop(cx))))
            .when(!launched, |el| {
                el.child(button("dev-detach", "x", "pill.code.devices.detach").on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.detach(cx))))
            });
        let screen = self.screen.clone();
        let status: Option<SharedString> = match (&live.error, &live.image) {
            (Some(error), _) => Some(error.clone().into()),
            (None, None) => Some(tr(if launched { "pill.code.devices.booting" } else { "pill.code.devices.connecting" }).into()),
            _ => None,
        };
        let area = div()
            .id("dev-screen")
            .flex_1()
            .min_h(px(0.))
            .relative()
            .mx(px(8.))
            .mb(px(8.))
            .cursor_pointer()
            .child(
                canvas(
                    move |bounds, window, _| {
                        if screen.get() != Some(bounds) {
                            screen.set(Some(bounds));
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when_some(live.image.clone(), |el, image| {
                el.child(img(image).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Contain))
            })
            .when_some(status, |el, status| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(10.))
                        .px(px(24.))
                        .text_center()
                        .when(live.error.is_none(), |el| el.child(gpui_m3::LoadingIndicator::new().size(px(28.))))
                        .child(div().text_size(px(13.)).text_color(t.text).child(status))
                        .when(live.error.is_none() && launched, |el| el.child(div().text_size(px(12.)).text_color(t.muted).child(tr("pill.code.devices.bootingHint")))),
                )
            })
            // El mouse es un dedo: apoyar, arrastrar y levantar; la rueda desplaza.
            .on_mouse_down(MouseButton::Left, cx.listener(|view, event: &MouseDownEvent, window, cx| {
                view.focus.focus(window);
                if let Some((x, y)) = view.device_point(event.position) {
                    view.send(Input::Touch { x, y, down: true });
                    if let Some(live) = view.live.as_mut() {
                        live.pressed = true;
                    }
                }
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, _| {
                if view.live.as_ref().is_some_and(|l| l.pressed) && event.pressed_button == Some(MouseButton::Left) {
                    if let Some((x, y)) = view.device_point(event.position) {
                        view.send(Input::Touch { x, y, down: true });
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, event: &MouseUpEvent, _, _| {
                if view.live.as_ref().is_some_and(|l| l.pressed) {
                    let at = view.device_point(event.position).unwrap_or((0, 0));
                    view.send(Input::Touch { x: at.0, y: at.1, down: false });
                    if let Some(live) = view.live.as_mut() {
                        live.pressed = false;
                    }
                }
            }))
            .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, _, _| {
                let dy = f32::from(event.delta.pixel_delta(px(20.)).y);
                let clicks = (-dy / 20.).round() as i32;
                if clicks != 0 {
                    view.send(Input::Wheel(clicks));
                }
            }));
        div().flex_1().min_h(px(0.)).flex().flex_col().child(bar).child(area).into_any_element()
    }

    /// Sin teléfono a la vista: qué falta, o la lista para encender uno.
    fn list_view(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = t();
        let mut list = div().id("devices-list").flex_1().min_h(px(0.)).overflow_y_scroll().p(px(14.)).flex().flex_col().gap(px(10.));
        let header = |title: &'static str| div().pt(px(4.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.muted).child(tr(title));
        let Some(android) = self.android.clone() else {
            return list.child(row_text(tr("pill.code.devices.checking").into(), None)).into_any_element();
        };
        if let Some(notice) = &self.notice {
            list = list.child(div().text_size(px(12.)).text_color(t.muted).child(notice.clone()));
        }
        list = list.child(header("pill.code.devices.android"));
        match android.missing() {
            Some(missing) => {
                let (title, desc, action) = match missing {
                    Missing::Sdk => ("pill.code.devices.needStudio", "pill.code.devices.needStudioDesc", "pill.code.devices.installStudio"),
                    Missing::Phone if android.sdkmanager.is_some() => ("pill.code.devices.needPhone", "pill.code.devices.needPhoneDesc", "pill.code.devices.preparePhone"),
                    Missing::Phone => ("pill.code.devices.needPhone", "pill.code.devices.needPhoneStudio", "pill.code.devices.openStudio"),
                };
                let waiting = self.polling_until.is_some();
                list = list.child(step_card(
                    tr(title).into(),
                    tr(if waiting { "pill.code.devices.waitingInstall" } else { desc }).into(),
                    Some(
                        gpui_m3::Button::new("dev-install", tr(action))
                            .tonal()
                            .size(gpui_m3::ButtonSize::Small)
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.install(missing, cx)))
                            .into_any_element(),
                    ),
                ));
            }
            None => {
                for (index, avd) in android.avds.iter().enumerate() {
                    let running = android.running.iter().find(|r| r.avd == avd.name);
                    let name = avd.name.clone();
                    let stopping = self.stopping.contains(&avd.name);
                    let detail: SharedString = match running {
                        _ if stopping => tr("pill.code.devices.stopping").into(),
                        Some(r) if r.grpc.is_none() => tr("pill.code.devices.runningElsewhere").into(),
                        Some(_) => tr("pill.code.devices.running").into(),
                        None => format!("{} × {}", avd.size.0, avd.size.1).into(),
                    };
                    let action = if running.is_some() { "pill.code.devices.view" } else { "pill.code.devices.start" };
                    list = list.child(device_row(
                        ("dev-avd", index),
                        "icons/smartphone.svg",
                        avd.name.clone().into(),
                        detail,
                        gpui_m3::Button::new(("dev-start", index), tr(action))
                            .tonal()
                            .size(gpui_m3::ButtonSize::Small)
                            .disabled(stopping || running.is_some_and(|r| r.grpc.is_none()))
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                view.focus.focus(window);
                                view.start(&name, cx);
                            }))
                            .into_any_element(),
                    ));
                }
            }
        }
        // iOS: el simulador viene con Xcode (solo macOS); se abre en su ventana.
        if cfg!(target_os = "macos") {
            list = list.child(header("pill.code.devices.ios"));
            match &self.ios {
                Some(ios) if !ios.xcode => {
                    list = list.child(step_card(
                        tr("pill.code.devices.needXcode").into(),
                        tr("pill.code.devices.needXcodeDesc").into(),
                        Some(
                            gpui_m3::Button::new("dev-xcode", tr("pill.code.devices.openAppStore"))
                                .tonal()
                                .size(gpui_m3::ButtonSize::Small)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    cx.open_url(devices::XCODE_URL);
                                    view.poll(cx);
                                }))
                                .into_any_element(),
                        ),
                    ));
                }
                Some(ios) => {
                    for (index, sim) in ios.simulators.iter().enumerate() {
                        let udid = sim.udid.clone();
                        list = list.child(device_row(
                            ("dev-sim", index),
                            "icons/smartphone.svg",
                            sim.name.clone().into(),
                            tr(if sim.booted { "pill.code.devices.running" } else { "pill.code.devices.opensWindow" }).into(),
                            gpui_m3::Button::new(("dev-sim-open", index), tr("pill.code.devices.open"))
                                .tonal()
                                .size(gpui_m3::ButtonSize::Small)
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    let udid = udid.clone();
                                    cx.background_spawn(async move { devices::boot_simulator(&udid) }).detach();
                                }))
                                .into_any_element(),
                        ));
                    }
                }
                None => {}
            }
        }
        list.child(
            div().pt(px(4.)).child(
                gpui_m3::Button::new("dev-recheck", tr(if self.checking { "pill.code.devices.checking" } else { "pill.code.devices.recheck" }))
                    .text()
                    .size(gpui_m3::ButtonSize::Small)
                    .icon("refresh")
                    .disabled(self.checking)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.check(cx))),
            ),
        )
        .into_any_element()
    }
}

fn row_text(text: SharedString, detail: Option<SharedString>) -> gpui::Div {
    let t = t();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .min_w(px(0.))
        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(text))
        .when_some(detail, |el, detail| el.child(div().text_size(px(12.)).text_color(t.muted).child(detail)))
}

/// Un paso que falta: qué es, para qué, y su botón.
fn step_card(title: SharedString, desc: SharedString, action: Option<gpui::AnyElement>) -> gpui::Div {
    let t = t();
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(t.r_ctl.min(14.)))
        .bg(t.control)
        .child(div().flex_1().min_w(px(0.)).child(row_text(title, Some(desc))))
        .children(action.map(|a| div().flex_none().child(a)))
}

fn device_row(id: impl Into<gpui::ElementId>, icon: &'static str, name: SharedString, detail: SharedString, action: gpui::AnyElement) -> gpui::Stateful<gpui::Div> {
    let t = t();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(t.r_ctl.min(14.)))
        .bg(t.control)
        .child(gpui::svg().path(icon).size(px(18.)).flex_none().text_color(t.muted))
        .child(div().flex_1().min_w(px(0.)).child(row_text(name, Some(detail))))
        .child(div().flex_none().child(action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_pantalla_cabe_con_su_proporcion() {
        assert_eq!(fit((1080, 2400), (540., 2000.)), (540., 1200.));
        assert_eq!(fit((1080, 2400), (2000., 600.)), (270., 600.));
    }
}
