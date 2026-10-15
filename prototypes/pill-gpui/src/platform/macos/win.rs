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
use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSView, NSWindow, NSWindowSharingType, NSWindowStyleMask};
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

/// Un monitor, en píxeles físicos de pantalla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub monitor: RECT,
    /// Sin la barra de menús ni el Dock.
    pub work: RECT,
}

impl Screen {
    fn of(screen: &NSScreen) -> Self {
        Self {
            monitor: to_physical(screen.frame()),
            work: to_physical(screen.visibleFrame()),
        }
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
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI da la `NSView` de contenido de su ventana, viva mientras
    // exista la ventana de GPUI que se nos prestó.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

/// Corre `work` en el hilo principal cuando termine lo que está haciendo,
/// fuera de cualquier préstamo de GPUI. Se llama desde el hilo principal.
fn later(work: impl FnOnce() + 'static) {
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
fn activate(window: &NSWindow) {
    if let Some(mtm) = mtm() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
    window.makeKeyAndOrderFront(None);
}

/// Trae una ventana de la pill al frente de las demás apps.
pub fn bring_to_front(window: &Window) {
    if let Some(window) = ns_window(window) {
        activate(&window);
    }
}

pub struct Overlay {
    window: Retained<NSWindow>,
    passthrough: bool,
}

impl Overlay {
    pub fn attach(window: &Window) -> Option<Self> {
        let window = ns_window(window)?;
        // GPUI la crea «con título» aunque no muestre barra, y AppKit no deja
        // que una ventana con título suba sobre la barra de menús: quedaba
        // corrida hacia abajo y la franja inferior fuera de la pantalla. La
        // clase de GPUI igual acepta el foco sin título.
        let untitled = window.clone();
        later(move || untitled.setStyleMask(untitled.styleMask() & !NSWindowStyleMask::Titled));
        // Sin la sombra que AppKit le pone a toda la ventana: solo se ve la pill.
        window.setHasShadow(false);
        let mut overlay = Self {
            window,
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
    pub fn set_focusable(&mut self, focusable: bool) {
        if focusable {
            activate(&self.window);
        } else if self.window.isKeyWindow() {
            self.window.resignKeyWindow();
        }
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
        self.window.orderFrontRegardless();
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
