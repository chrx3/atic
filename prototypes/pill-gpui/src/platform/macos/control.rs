//! Lo que el celular controla del Mac: el volumen, silenciar, bloquear la
//! pantalla, pasar diapositivas y suspender (`PcCommand`), y el panel táctil y
//! el teclado (`InputEvent`).
//!
//! El volumen va por AppleScript (`osascript`), que no pide permisos. Lo demás
//! son eventos de `CGEvent` a la app del frente, con el permiso de
//! Accesibilidad que la pill ya tiene para pegar.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, Ordering};

use super::super::control::{Action, InputEvent, MouseButton, PcKey};

pub fn run(action: Action) {
    match action {
        Action::VolumeUp => volume("set volume output volume ((output volume of (get volume settings)) + 6)"),
        Action::VolumeDown => volume("set volume output volume ((output volume of (get volume settings)) - 6)"),
        Action::Mute => volume("set volume output muted (not (output muted of (get volume settings)))"),
        // ⌃⌘Q: el bloqueo de pantalla del sistema.
        Action::LockScreen => key(Q, CONTROL | COMMAND),
        Action::SlideNext => key(RIGHT, 0),
        Action::SlidePrev => key(LEFT, 0),
        Action::Sleep => {
            if let Err(error) = std::process::Command::new("pmset").arg("sleepnow").spawn() {
                tracing::warn!(%error, "celular: no se pudo suspender");
            }
        }
    }
}

pub fn input(event: InputEvent) {
    match event {
        InputEvent::Move { dx, dy } => move_by(dx as f64, dy as f64),
        InputEvent::PointerTo { x, y } => pointer_to(x as f64, y as f64),
        InputEvent::Scroll { dx, dy } => scroll(dx, dy),
        InputEvent::Click { button } => {
            mouse_button(button, true);
            mouse_button(button, false);
        }
        InputEvent::Button { button, down } => mouse_button(button, down),
        InputEvent::Text { text } => type_text(&text),
        InputEvent::Key { key: pc_key } => match pc_key {
            PcKey::Home => key(115, 0),
            PcKey::Recents => {},
            PcKey::Delete => key(117, 0),
            PcKey::Enter => key(RETURN, 0),
            PcKey::Escape => key(ESCAPE, 0),
            PcKey::Backspace => key(DELETE, 0),
            PcKey::Tab => key(TAB, 0),
            PcKey::Space => key(SPACE, 0),
            PcKey::Up => key(UP, 0),
            PcKey::Down => key(DOWN, 0),
            PcKey::Left => key(LEFT, 0),
            PcKey::Right => key(RIGHT, 0),
            // El selector de apps necesita ⌘ apretado de verdad, no solo la bandera.
            PcKey::AppSwitch => chord(COMMAND_KEY, TAB, COMMAND),
            PcKey::Fullscreen => key(F, CONTROL | COMMAND),
            // F11 es «Mostrar escritorio» en Mission Control.
            PcKey::ShowDesktop => key(F11, 0),
            PcKey::BlackScreen => key(B, 0),
        },
    }
}

const Q: u16 = 0x0C;
const B: u16 = 0x0B;
const F: u16 = 0x03;
const RETURN: u16 = 0x24;
const TAB: u16 = 0x30;
const SPACE: u16 = 0x31;
const DELETE: u16 = 0x33;
const ESCAPE: u16 = 0x35;
const COMMAND_KEY: u16 = 0x37;
const F11: u16 = 0x67;
const LEFT: u16 = 0x7B;
const RIGHT: u16 = 0x7C;
const DOWN: u16 = 0x7D;
const UP: u16 = 0x7E;
const CONTROL: u64 = 1 << 18;
const COMMAND: u64 = 1 << 20;

type CGEventRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGPoint,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CGEventRef;
    fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, at: CGPoint, button: u32) -> CGEventRef;
    fn CGEventCreateScrollWheelEvent(source: *const c_void, units: u32, count: u32, wheel1: i32, ...) -> CGEventRef;
    fn CGEventKeyboardSetUnicodeString(event: CGEventRef, len: usize, text: *const u16);
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGMainDisplayID() -> u32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: *const c_void);
}

const HID_TAP: u32 = 0;

/// Manda el evento y lo suelta. Un evento nulo (sin permiso, sin memoria) no hace nada.
fn post(event: CGEventRef) {
    if event.is_null() {
        return;
    }
    // SAFETY: el evento viene de un `CGEventCreate…` y se suelta una sola vez.
    unsafe {
        CGEventPost(HID_TAP, event);
        CFRelease(event);
    }
}

/// La tecla con esas banderas (`CGEventFlags`), bajar y subir.
fn key(code: u16, flags: u64) {
    for down in [true, false] {
        // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if !event.is_null() {
                CGEventSetFlags(event, flags);
            }
            post(event);
        }
    }
}

/// Un modificador apretado de verdad alrededor de la tecla.
fn chord(modifier: u16, code: u16, flags: u64) {
    // SAFETY: cada evento se crea aquí y `post` lo manda y lo suelta.
    unsafe {
        let down = CGEventCreateKeyboardEvent(std::ptr::null(), modifier, true);
        if !down.is_null() {
            CGEventSetFlags(down, flags);
        }
        post(down);
        key(code, flags);
        post(CGEventCreateKeyboardEvent(std::ptr::null(), modifier, false));
    }
}

/// Texto tal cual, sin pasar por la distribución del teclado. `CGEvent` lleva
/// hasta 20 unidades UTF-16 por evento.
fn type_text(text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    for chunk in units.chunks(20) {
        for down in [true, false] {
            // SAFETY: `chunk` vive durante la llamada y su largo es el que se pasa.
            unsafe {
                let event = CGEventCreateKeyboardEvent(std::ptr::null(), 0, down);
                if !event.is_null() {
                    CGEventKeyboardSetUnicodeString(event, chunk.len(), chunk.as_ptr());
                }
                post(event);
            }
        }
    }
}

/// Los botones apretados (bit 0 izquierdo, 1 derecho, 2 el del medio), para
/// que mover sea arrastrar mientras siguen abajo.
static HELD: AtomicU8 = AtomicU8::new(0);

fn cursor() -> CGPoint {
    // SAFETY: un evento vacío solo para leer dónde está el puntero.
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return CGPoint { x: 0.0, y: 0.0 };
        }
        let at = CGEventGetLocation(event);
        CFRelease(event);
        at
    }
}

/// El punto dentro de alguna pantalla: fuera de todas, el puntero se pierde.
fn clamp_to_displays(p: CGPoint) -> CGPoint {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: `ids` tiene lugar para los 16 que se piden.
    let ok = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } == 0;
    if !ok || count == 0 {
        return p;
    }
    let mut best = p;
    let mut best_d = f64::MAX;
    for &id in &ids[..count as usize] {
        // SAFETY: un id que acaba de dar el sistema.
        let r = unsafe { CGDisplayBounds(id) };
        let x = p.x.clamp(r.origin.x, r.origin.x + r.size.x - 1.0);
        let y = p.y.clamp(r.origin.y, r.origin.y + r.size.y - 1.0);
        let d = (x - p.x).powi(2) + (y - p.y).powi(2);
        if d < best_d {
            best_d = d;
            best = CGPoint { x, y };
        }
    }
    best
}

fn move_by(dx: f64, dy: f64) {
    let from = cursor();
    move_to(clamp_to_displays(CGPoint { x: from.x + dx, y: from.y + dy }));
}

/// `(x, y)` de 0 a 1 sobre la pantalla principal, la que se comparte con el celular
/// (`phone_screen.rs`): un toque en su imagen cae en el mismo punto.
fn pointer_to(x: f64, y: f64) {
    // SAFETY: el id de la pantalla principal y sus límites, del sistema.
    let r = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    let to = CGPoint {
        x: r.origin.x + x.clamp(0.0, 1.0) * (r.size.x - 1.0),
        y: r.origin.y + y.clamp(0.0, 1.0) * (r.size.y - 1.0),
    };
    move_to(to);
}

/// El puntero a `to` (o el arrastre, con un botón apretado).
fn move_to(to: CGPoint) {
    let held = HELD.load(Ordering::Relaxed);
    // kCGEventMouseMoved, o el arrastre del botón que esté abajo.
    let (kind, button) = if held & 1 != 0 {
        (6, 0)
    } else if held & 2 != 0 {
        (7, 1)
    } else if held & 4 != 0 {
        (27, 2)
    } else {
        (5, 0)
    };
    // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
    post(unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, to, button) });
}

fn mouse_button(button: MouseButton, down: bool) {
    // (bajar, subir, número de botón) de `CGEventType` y `CGMouseButton`.
    let (bit, down_kind, up_kind, number) = match button {
        MouseButton::Left => (1, 1, 2, 0),
        MouseButton::Right => (2, 3, 4, 1),
        MouseButton::Middle => (4, 25, 26, 2),
    };
    if down {
        HELD.fetch_or(bit, Ordering::Relaxed);
    } else {
        HELD.fetch_and(!bit, Ordering::Relaxed);
    }
    let kind = if down { down_kind } else { up_kind };
    // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
    post(unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, cursor(), number) });
}

fn scroll(dx: f32, dy: f32) {
    // En píxeles; la rueda positiva sube por la página, al revés que `dy`.
    let (wy, wx) = (-dy.round() as i32, -dx.round() as i32);
    if wy == 0 && wx == 0 {
        return;
    }
    // SAFETY: dos ruedas, así que van dos valores después de `wheel1`.
    post(unsafe { CGEventCreateScrollWheelEvent(std::ptr::null(), 0, 2, wy, wx) });
}

fn volume(script: &str) {
    if let Err(error) = std::process::Command::new("osascript").args(["-e", script]).spawn() {
        tracing::warn!(%error, "celular: no se pudo cambiar el volumen");
    }
}
