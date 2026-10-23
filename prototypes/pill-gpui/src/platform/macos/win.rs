//! Lo que GPUI no expone para un overlay en macOS. La ventana de la pill ya
//! sale de GPUI como `NSPanel` que no activa la app, en el nivel de los
//! popups y en todos los escritorios (`WindowKind::PopUp`); aquí se agrega
//! dejar pasar los clics, ocultarse de las capturas, tomar el foco cuando hace
//! falta y la geometría de los monitores.
//!
//! «Físico» en macOS: puntos con el origen arriba a la izquierda de la
//! pantalla principal, lo mismo que usan Quartz y `atic-capture` para Mac. En
//! GPUI para Mac un píxel lógico también es un punto, así que `to_logical` y
//! `cursor` no dividen por la escala de la ventana.

use gpui::Window;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::geometry::{Edge, Rect};

/// Un rectángulo de pantalla en «físicos» (puntos, ver arriba), con los
/// mismos campos que el `RECT` de Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// La ventana del overlay, para ubicar el vidrio debajo (`glass::place`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(usize);

impl Handle {
    /// La ventana, retenida. Vive mientras la pill: el overlay no se cierra.
    pub(crate) fn window(&self) -> Option<Retained<NSWindow>> {
        // SAFETY: el puntero es el de la ventana del overlay (`Overlay::hwnd`).
        unsafe { Retained::retain(self.0 as *mut NSWindow) }
    }
}

/// Un monitor, en píxeles físicos de pantalla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub monitor: RECT,
    /// Sin la barra de menús ni el Dock.
    pub work: RECT,
    /// El notch físico (la cámara de las MacBook), si la pantalla lo tiene.
    pub notch: Option<RECT>,
    /// El notch es de mentira (`PILL_FAKE_NOTCH`): la pill lo dibuja.
    pub fake_notch: bool,
}

impl Screen {
    /// El área de trabajo incluye la barra de menús: la pill se pega al
    /// borde de arriba sobre ella, como un notch (el centro de la barra casi
    /// nunca tiene nada). En una pantalla con notch físico (la cámara de las
    /// MacBook) empieza justo debajo: lo que cuelga de la pill no queda
    /// tapado, y la pill acoplada en él se funde hacia arriba. El Dock se
    /// respeta.
    fn of(screen: &NSScreen) -> Self {
        let monitor = to_physical(screen.frame());
        let mut work = to_physical(screen.visibleFrame());
        let origin = screen.frame().origin;
        let primary = origin.x == 0.0 && origin.y == 0.0;
        let (notch, fake_notch, top) = match fake_notch().filter(|_| primary) {
            Some((w, h)) => {
                let left = monitor.left + (monitor.right - monitor.left - w) / 2;
                let notch = RECT { left, top: monitor.top, right: left + w, bottom: monitor.top + h };
                (Some(notch), true, h)
            }
            None => {
                let top = screen.safeAreaInsets().top.round() as i32;
                (real_notch(screen, &monitor, top), false, top)
            }
        };
        work.top = monitor.top + top;
        Self { monitor, work, notch, fake_notch }
    }

    /// El monitor cuya esquina de arriba a la izquierda es `(x, y)`.
    pub fn at_origin(x: i32, y: i32) -> Option<Self> {
        all_screens()
            .into_iter()
            .find(|screen| screen.monitor.left == x && screen.monitor.top == y)
    }

    /// Bordes que dan al vacío, sin otro monitor pegado.
    pub fn outer_edges(&self) -> Vec<Edge> {
        let all: Vec<Rect> = all_screens().iter().map(|screen| physical(&screen.monitor)).collect();
        crate::geometry::outer_edges(&physical(&self.monitor), &all)
    }
}

/// El hueco de la cámara: entre las dos zonas libres de arriba
/// (`auxiliaryTopLeftArea` y `auxiliaryTopRightArea`, macOS 12+), con el alto
/// de `safeAreaInsets`. Solo se usan sus anchos: no dependen de en qué
/// coordenadas vengan.
fn real_notch(screen: &NSScreen, monitor: &RECT, height: i32) -> Option<RECT> {
    if height <= 0 {
        return None;
    }
    let left = screen.auxiliaryTopLeftArea().size.width;
    let right = screen.auxiliaryTopRightArea().size.width;
    let width = screen.frame().size.width - left - right;
    if left <= 0.0 || right <= 0.0 || width <= 1.0 {
        return None;
    }
    let x = monitor.left + left.round() as i32;
    Some(RECT { left: x, top: monitor.top, right: x + width.round() as i32, bottom: monitor.top + height })
}

/// `PILL_FAKE_NOTCH`: un notch de mentira en la pantalla principal, para
/// probar la pill fundida en un Mac sin cámara en la pantalla. `ANCHOxALTO`
/// en puntos (`185x32`), o `1` para el de una MacBook Pro de 14".
fn fake_notch() -> Option<(i32, i32)> {
    static FAKE: std::sync::OnceLock<Option<(i32, i32)>> = std::sync::OnceLock::new();
    *FAKE.get_or_init(|| parse_fake_notch(&std::env::var("PILL_FAKE_NOTCH").ok()?))
}

fn parse_fake_notch(text: &str) -> Option<(i32, i32)> {
    let text = text.trim();
    if text == "1" {
        return Some((185, 32));
    }
    let (w, h) = text.split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse::<i32>().ok()?, h.trim().parse::<i32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn physical(rect: &RECT) -> Rect {
    Rect::new(
        rect.left as f32,
        rect.top as f32,
        (rect.right - rect.left) as f32,
        (rect.bottom - rect.top) as f32,
    )
}

fn mtm() -> Option<MainThreadMarker> {
    MainThreadMarker::new()
}

/// La pantalla principal (la de la barra de menús): su esquina de abajo a la
/// izquierda es el origen de AppKit.
fn primary() -> Option<Retained<NSScreen>> {
    let screens = NSScreen::screens(mtm()?);
    screens.firstObject()
}

fn primary_height() -> f64 {
    primary().map(|screen| screen.frame().size.height).unwrap_or(0.0)
}

/// El alto de la pantalla principal en puntos: para pasar de AppKit (origen
/// abajo) a las coordenadas de Quartz (origen arriba).
pub(crate) fn primary_height_points() -> f64 {
    primary_height()
}

/// De AppKit (puntos, origen abajo) a físico (origen arriba).
fn to_physical(rect: NSRect) -> RECT {
    let top = primary_height() - (rect.origin.y + rect.size.height);
    let left = rect.origin.x.round() as i32;
    let top = top.round() as i32;
    RECT {
        left,
        top,
        right: left + rect.size.width.round() as i32,
        bottom: top + rect.size.height.round() as i32,
    }
}

/// De físico a AppKit.
fn to_appkit(rect: &RECT) -> NSRect {
    let width = (rect.right - rect.left) as f64;
    let height = (rect.bottom - rect.top) as f64;
    let x = rect.left as f64;
    let y = primary_height() - rect.top as f64 - height;
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

/// El cursor en físico.
fn cursor_physical() -> (f64, f64) {
    let point = NSEvent::mouseLocation();
    (point.x, primary_height() - point.y)
}

fn all_screens() -> Vec<Screen> {
    let Some(mtm) = mtm() else {
        return Vec::new();
    };
    NSScreen::screens(mtm).iter().map(|screen| Screen::of(&screen)).collect()
}

fn contains(rect: &RECT, (x, y): (f64, f64)) -> bool {
    x >= rect.left as f64 && x < rect.right as f64 && y >= rect.top as f64 && y < rect.bottom as f64
}

fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    ns_view(window)?.window()
}

/// La vista de GPUI (la que recibe `keyDown:`).
fn ns_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI da la `NSView` de contenido de su ventana, viva mientras
    // exista la ventana de GPUI que se nos prestó.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    Some(objc2::Message::retain(view))
}

/// La vista de GPUI de `window`, para poner el vidrio debajo
/// (`window_glass.rs`).
pub fn glass_view(window: &Window) -> Option<Retained<objc2::runtime::AnyObject>> {
    // SAFETY: una `NSView` es un objeto de Objective-C.
    ns_view(window).map(|view| unsafe { Retained::cast_unchecked(view) })
}

/// Mueve `window` con el clic en curso, como su barra de título. GPUI tapa la
/// barra nativa y solo hace esto en Windows (`WindowControlArea::Drag`): en
/// macOS las zonas de arrastre no hacían nada y la ventana solo se movía desde
/// algún resto de la barra nativa que quedara sin tapar. Va fuera del
/// manejador de GPUI (`later`): AppKit sigue el arrastre en su propio loop
/// hasta que se suelta, y ajusta a los bordes como cualquier ventana.
pub fn start_move(window: &Window) {
    let (Some(ns), Some(marker)) = (ns_window(window), mtm()) else {
        return;
    };
    let Some(event) = NSApplication::sharedApplication(marker).currentEvent() else {
        return;
    };
    later(move || {
        // SAFETY: AppKit en el hilo principal; la ventana y el evento viven en
        // el cierre.
        let _: () = unsafe { objc2::msg_send![&*ns, performWindowDragWithEvent: &*event] };
    });
}

/// Corre `work` en el hilo principal cuando termine lo que está haciendo,
/// fuera de cualquier préstamo de GPUI. Se llama desde el hilo principal.
pub(super) fn later(work: impl FnOnce() + 'static) {
    type Work = Box<dyn FnOnce()>;
    extern "C" {
        static _dispatch_main_q: std::ffi::c_void;
        fn dispatch_async_f(
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: extern "C" fn(*mut std::ffi::c_void),
        );
    }
    extern "C" fn run(context: *mut std::ffi::c_void) {
        // SAFETY: `context` es el `Box` que se armó abajo, usado una vez.
        let work = unsafe { Box::from_raw(context.cast::<Work>()) };
        work();
    }
    let context = Box::into_raw(Box::new(Box::new(work) as Work));
    // SAFETY: la cola principal corre `run` en el mismo hilo que nos llamó,
    // así que lo capturado no cruza hilos.
    unsafe { dispatch_async_f(&raw const _dispatch_main_q, context.cast(), run) };
}

/// Activa la app y trae `window` al frente con el foco del teclado.
///
/// El foco se da recién con la app activa: GPUI toma un «se volvió la
/// ventana activa» con la app inactiva como un aviso falso y lo deshace con
/// `resignKeyWindow` mientras tiene tomado el candado de esa ventana, y el
/// aviso que eso dispara espera el mismo candado (la pill se quedaba pegada al
/// abrir Ajustes o Reuniones). La activación es asíncrona desde macOS 14: se
/// espera a que llegue, hasta un segundo.
fn activate(window: &NSWindow) {
    let Some(marker) = mtm() else {
        return;
    };
    let app = NSApplication::sharedApplication(marker);
    if app.isActive() {
        make_key(window);
        return;
    }
    window.orderFrontRegardless();
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    // La ventana viaja retenida como número: se suelta en el hilo principal.
    let raw = Retained::into_raw(objc2::Message::retain(window)) as usize;
    let _ = std::thread::Builder::new().name("activar".into()).spawn(move || {
        for attempt in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let last = attempt == 39;
            let done = super::main_thread::sync(move || {
                let Some(mtm) = mtm() else {
                    return true;
                };
                let active = NSApplication::sharedApplication(mtm).isActive();
                if active || last {
                    // SAFETY: el puntero es el retenido arriba; se suelta una vez.
                    let window = unsafe { Retained::from_raw(raw as *mut NSWindow) };
                    if let Some(window) = window {
                        if active {
                            make_key(&window);
                        }
                    }
                    return true;
                }
                false
            });
            if done != Some(false) {
                return;
            }
        }
    });
}

/// Le da el foco del teclado a `window` sin que GPUI se trabe.
///
/// GPUI (`window_did_change_key_status`) toma un «se volvió la ventana activa»
/// en que la ventana todavía no figura como activa por un aviso falso y lo
/// deshace con `resignKeyWindow` sin soltar el candado de esa ventana: el
/// aviso que eso dispara espera el mismo candado y la pill queda pegada. Pasa
/// al abrir Reuniones o Ajustes desde el notch, cuya ventana tenía el foco.
/// Aquí el cambio se hace con el delegado de GPUI desconectado y después se le
/// avisa a mano, solo si la ventana quedó activa de verdad.
fn make_key(window: &NSWindow) {
    use objc2::runtime::AnyObject;
    // SAFETY: el delegado es el objeto de GPUI de esta ventana; se repone en
    // el acto y su método no usa el argumento de la notificación.
    unsafe {
        let delegate: Option<Retained<AnyObject>> = objc2::msg_send![window, delegate];
        let none: *const AnyObject = std::ptr::null();
        let _: () = objc2::msg_send![window, setDelegate: none];
        window.makeKeyAndOrderFront(None);
        let _: () = objc2::msg_send![window, setDelegate: delegate.as_deref()];
        tracing::debug!(clave = window.isKeyWindow(), "foco: ventana");
        if let Some(delegate) = delegate.filter(|_| window.isKeyWindow()) {
            let _: () = objc2::msg_send![&delegate, windowDidBecomeKey: none];
        }
    }
}

/// Si las ventanas normales se abren con el foco. En macOS no: lo da
/// `bring_to_front` cuando la app ya está activa (ver `activate`).
pub const FOCUS_ON_OPEN: bool = false;

/// Trae una ventana de la pill al frente de las demás apps.
/// En la próxima vuelta del hilo principal, fuera del cuadro de GPUI (ver
/// `Overlay::set_focusable`).
pub fn bring_to_front(window: &Window) {
    if let Some(window) = ns_window(window) {
        later(move || activate(&window));
    }
}

/// La ventana del overlay, para empezar arrastres desde ella (`drag.rs`).
pub(crate) static OVERLAY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// La ventana del overlay, retenida.
pub(crate) fn overlay_window() -> Option<Retained<NSWindow>> {
    Handle(OVERLAY.load(std::sync::atomic::Ordering::Acquire)).window()
}

pub struct Overlay {
    window: Retained<NSWindow>,
    /// La vista de GPUI: tiene que ser el first responder para que las
    /// teclas lleguen a GPUI.
    view: Retained<NSView>,
    passthrough: bool,
}

impl Overlay {
    pub fn attach(window: &Window) -> Option<Self> {
        let view = ns_view(window)?;
        let window = view.window()?;
        OVERLAY.store(Retained::as_ptr(&window) as usize, std::sync::atomic::Ordering::Release);
        // GPUI la crea «con título» aunque no muestre barra, y AppKit no deja
        // que una ventana con título suba sobre la barra de menús: quedaba
        // corrida hacia abajo y la franja inferior fuera de la pantalla. La
        // clase de GPUI igual acepta el foco sin título.
        //
        // Cambiar el `styleMask` deja a la ventana misma como first responder:
        // las teclas morían ahí (con el pitido de AppKit por cada una) y nunca
        // llegaban a GPUI. Se repone la vista.
        let (untitled, gpui_view) = (window.clone(), view.clone());
        later(move || {
            untitled.setStyleMask(untitled.styleMask() & !NSWindowStyleMask::Titled);
            // En todos los escritorios y sobre apps a pantalla completa (lo de
            // GPUI), y además quieta en Mission Control y fuera de ⌘`.
            untitled.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
            untitled.makeFirstResponder(Some(&**gpui_view));
        });
        // Sin la sombra que AppKit le pone a toda la ventana: solo se ve la pill.
        window.setHasShadow(false);
        let mut overlay = Self {
            window,
            view,
            passthrough: false,
        };
        overlay.set_passthrough(true);
        Some(overlay)
    }

    /// Con `true` los clics atraviesan la ventana hacia lo que haya debajo.
    pub fn set_passthrough(&mut self, passthrough: bool) {
        if self.passthrough == passthrough {
            return;
        }
        self.passthrough = passthrough;
        self.window.setIgnoresMouseEvents(passthrough);
    }

    /// La ventana del overlay nunca toma el foco, salvo mientras un panel con
    /// campo de texto está abierto.
    ///
    /// En la próxima vuelta del hilo principal: el cambio de foco le avisa a
    /// GPUI en el acto y, en medio de un cuadro, GPUI pierde el aviso
    /// («RefCell already borrowed») y la ventana queda sin teclado.
    pub fn set_focusable(&mut self, focusable: bool) {
        let window = self.window.clone();
        let view = self.view.clone();
        later(move || {
            if focusable {
                // El overlay es un panel que no activa la app (como Spotlight):
                // toma el teclado sin activarla, y macOS 14 no deja activar
                // una app que no recibió un clic o una tecla.
                window.orderFrontRegardless();
                make_key(&window);
                // Y las teclas van a la vista de GPUI, no a la ventana.
                window.makeFirstResponder(Some(&**view));
            } else if window.isKeyWindow() {
                window.resignKeyWindow();
            }
        });
    }

    /// Invisible para las capturas de pantalla mientras se congela.
    pub fn exclude_from_capture(&self, exclude: bool) {
        let sharing = if exclude {
            NSWindowSharingType::None
        } else {
            NSWindowSharingType::ReadOnly
        };
        self.window.setSharingType(sharing);
    }

    /// El nivel de popup de GPUI no lo pierde la ventana como el «siempre
    /// visible» de Windows; basta con que siga en pantalla.
    pub fn keep_topmost(&self) -> bool {
        if self.window.isVisible() {
            return false;
        }
        let window = self.window.clone();
        later(move || window.orderFrontRegardless());
        true
    }

    pub fn hwnd(&self) -> Handle {
        Handle(Retained::as_ptr(&self.window) as usize)
    }

    /// Esquina del contenido en pantalla, en píxeles físicos.
    pub fn origin(&self) -> Option<(i32, i32)> {
        let rect = self.client_rect();
        Some((rect.left, rect.top))
    }

    fn client_rect(&self) -> RECT {
        to_physical(self.window.contentRectForFrameRect(self.window.frame()))
    }

    /// El monitor en que está la ventana.
    pub fn screen(&self) -> Option<Screen> {
        self.window.screen().map(|screen| Screen::of(&screen))
    }

    /// El monitor bajo el cursor.
    pub fn screen_under_cursor(&self) -> Option<Screen> {
        let cursor = cursor_physical();
        all_screens().into_iter().find(|screen| contains(&screen.monitor, cursor))
    }

    /// Pasa la ventana a cubrir `screen` entero.
    pub fn move_to(&self, screen: &Screen) {
        self.cover(screen.monitor);
    }

    /// Como `move_to`, pero a cualquier rectángulo de pantalla (físico).
    ///
    /// En la próxima vuelta del hilo principal: mover la ventana le avisa a
    /// GPUI en el acto (`windowDidResize`) y GPUI está ocupado en este cuadro.
    pub fn cover(&self, target: RECT) {
        let window = self.window.clone();
        later(move || {
            let frame = window.frameRectForContentRect(to_appkit(&target));
            window.setFrame_display(frame, true);
        });
    }

    /// El contenido ya es `target`.
    pub fn covers(&self, target: &RECT) -> bool {
        self.client_rect() == *target
    }

    /// Un rectángulo de pantalla en coordenadas lógicas de la ventana.
    pub fn to_logical(&self, rect: &RECT, _scale_factor: f32) -> Option<Rect> {
        let origin = self.client_rect();
        Some(Rect::new(
            (rect.left - origin.left) as f32,
            (rect.top - origin.top) as f32,
            (rect.right - rect.left) as f32,
            (rect.bottom - rect.top) as f32,
        ))
    }

    /// Cursor global en coordenadas lógicas de la ventana.
    pub fn cursor(&self, _scale_factor: f32) -> Option<(f32, f32)> {
        let origin = self.client_rect();
        let (x, y) = cursor_physical();
        Some(((x - origin.left as f64) as f32, (y - origin.top as f64) as f32))
    }
}

/// `CGEventSourceKeyState` con `kCGEventSourceStateCombinedSessionState`.
pub(crate) fn key_down(keycode: u16) -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
    // SAFETY: función pura de consulta.
    unsafe { CGEventSourceKeyState(0, keycode) }
}

/// Botón izquierdo apretado ahora mismo, aunque el evento lo reciba otra app.
pub fn left_button_down() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

/// Opción+Z: la rueda (el Alt+Z de Windows). Se sondea porque la ventana
/// nunca tiene el foco.
pub fn wheel_shortcut_down() -> bool {
    const OPTION: u16 = 0x3A;
    const RIGHT_OPTION: u16 = 0x3D;
    const Z: u16 = 0x06;
    (key_down(OPTION) || key_down(RIGHT_OPTION)) && key_down(Z)
}

/// Espacio apretado ahora mismo, aunque la ventana no tenga el foco.
pub fn space_down() -> bool {
    const SPACE: u16 = 0x31;
    key_down(SPACE)
}

/// Mueve el cursor a `(x, y)` de pantalla, en puntos con el origen arriba.
pub fn set_cursor(x: i32, y: i32) {
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
    }
    // SAFETY: solo mueve el cursor.
    unsafe {
        CGWarpMouseCursorPosition(CGPoint { x: x as f64, y: y as f64 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_notch_de_mentira_se_lee_de_la_variable() {
        assert_eq!(parse_fake_notch("1"), Some((185, 32)));
        assert_eq!(parse_fake_notch("200x38"), Some((200, 38)));
        assert_eq!(parse_fake_notch(" 160X30 "), Some((160, 30)));
        assert_eq!(parse_fake_notch("0"), None);
        assert_eq!(parse_fake_notch("x32"), None);
        assert_eq!(parse_fake_notch("0x32"), None);
    }
}
