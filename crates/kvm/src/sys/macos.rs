//! macOS: se inyecta con `CGEventPost` y se escucha con un `CGEventTap` en la
//! capa HID, que puede tragarse los eventos antes de que lleguen al sistema.
//! Las dos cosas necesitan el permiso de Accesibilidad (y escuchar, el de
//! Monitoreo de entrada) para el programa que corre esto.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::keys;
use crate::layout::{Rect, Screens};
use crate::proto::Button;
use crate::source::{Grab, Verdict};

/// En macOS el cursor se deja quieto donde salió: el gancho se traga el
/// movimiento y los deltas vienen en el evento.
pub const PARK_INSET: f64 = 0.0;

type CGEventRef = *mut c_void;
type CFTypeRef = *mut c_void;

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

type TapCallback = extern "C" fn(proxy: *mut c_void, kind: u32, event: CGEventRef, user: *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CGEventRef;
    fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, at: CGPoint, button: u32) -> CGEventRef;
    fn CGEventCreateScrollWheelEvent(source: *const c_void, units: u32, count: u32, wheel1: i32, ...) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventSetType(event: CGEventRef, kind: u32);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, user: *mut c_void) -> CFTypeRef;
    fn CGEventTapEnable(tap: CFTypeRef, enable: bool);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: *const c_void);
    fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: CFTypeRef, order: isize) -> CFTypeRef;
    fn CFRunLoopGetCurrent() -> CFTypeRef;
    fn CFRunLoopAddSource(run_loop: CFTypeRef, source: CFTypeRef, mode: *const c_void);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: *const c_void;
}

// `CGEventType`.
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const SCROLL: u32 = 22;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
const TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;

// `CGEventField`.
const CLICK_STATE: u32 = 1;
const BUTTON_NUMBER: u32 = 3;
const DELTA_X: u32 = 4;
const DELTA_Y: u32 = 5;
const AUTOREPEAT: u32 = 8;
const KEYCODE: u32 = 9;
const SCROLL_LINES_Y: u32 = 11;
const SCROLL_LINES_X: u32 = 12;
const SOURCE_USER_DATA: u32 = 42;
const SCROLL_CONTINUOUS: u32 = 88;
const SCROLL_POINTS_Y: u32 = 96;
const SCROLL_POINTS_X: u32 = 97;

const HID_TAP: u32 = 0;

/// Marca de los eventos que inyecta esto: el gancho los deja pasar sin mirarlos.
const MINE: i64 = 0x4B56_4D41_5449; // "KVMATI"

// `CGEventFlags`: la bandera de cada modificador y la del lado (izquierdo o derecho).
const SHIFT: u64 = 1 << 17;
const CONTROL: u64 = 1 << 18;
const OPTION: u64 = 1 << 19;
const COMMAND: u64 = 1 << 20;
const CAPS: u64 = 1 << 16;
const NON_COALESCED: u64 = 0x100;

fn modifier_mask(hid: u16) -> u64 {
    match hid {
        0xE0 => CONTROL | 0x01,
        0xE1 => SHIFT | 0x02,
        0xE2 => OPTION | 0x20,
        0xE3 => COMMAND | 0x08,
        0xE4 => CONTROL | 0x2000,
        0xE5 => SHIFT | 0x04,
        0xE6 => OPTION | 0x40,
        0xE7 => COMMAND | 0x10,
        _ => 0,
    }
}

pub fn prepare() {}

// ---------------------------------------------------------------- recibir

/// Los modificadores apretados desde el otro equipo: el bit `i` es el HID `0xE0 + i`.
static MODS: AtomicU8 = AtomicU8::new(0);
/// Bloq Mayús prendido desde el otro equipo.
static CAPS_ON: AtomicBool = AtomicBool::new(false);
/// Los botones apretados (bit 0 izquierdo, 1 derecho, 2 los otros), para que
/// mover sea arrastrar.
static HELD: AtomicU8 = AtomicU8::new(0);

fn post(event: CGEventRef) {
    if event.is_null() {
        return;
    }
    // SAFETY: el evento viene de un `CGEventCreate…` y se suelta una sola vez.
    unsafe {
        CGEventSetIntegerValueField(event, SOURCE_USER_DATA, MINE);
        CGEventPost(HID_TAP, event);
        CFRelease(event);
    }
}

fn flags() -> u64 {
    let mods = MODS.load(Ordering::Relaxed);
    let held = (0..8).filter(|i| mods & (1 << i) != 0).fold(0, |f, i| f | modifier_mask(0xE0 + i));
    held | if CAPS_ON.load(Ordering::Relaxed) { CAPS } else { 0 } | NON_COALESCED
}

pub fn screens() -> Screens {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: `ids` tiene lugar para los 16 que se piden.
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0 {
        return Screens::default();
    }
    Screens(
        ids[..count as usize]
            .iter()
            .map(|&id| {
                // SAFETY: un id que acaba de dar el sistema.
                let r = unsafe { CGDisplayBounds(id) };
                Rect { x: r.origin.x, y: r.origin.y, w: r.size.x, h: r.size.y }
            })
            .collect(),
    )
}

pub fn cursor() -> (f64, f64) {
    // SAFETY: un evento vacío solo para leer dónde está el cursor.
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return (0.0, 0.0);
        }
        let at = CGEventGetLocation(event);
        CFRelease(event);
        (at.x, at.y)
    }
}

/// El cursor a `(x, y)`, con el movimiento en los campos de delta (algunas
/// apps, como los juegos y los visores 3D, miran solo eso). Con un botón
/// apretado es un arrastre.
pub fn move_to(x: f64, y: f64, dx: f64, dy: f64) {
    let held = HELD.load(Ordering::Relaxed);
    let (kind, button) = if held & 1 != 0 {
        (LEFT_DRAGGED, 0)
    } else if held & 2 != 0 {
        (RIGHT_DRAGGED, 1)
    } else if held & 4 != 0 {
        (OTHER_DRAGGED, 2)
    } else {
        (MOVED, 0)
    };
    // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
    unsafe {
        let event = CGEventCreateMouseEvent(std::ptr::null(), kind, CGPoint { x, y }, button);
        if !event.is_null() {
            CGEventSetIntegerValueField(event, DELTA_X, dx.round() as i64);
            CGEventSetIntegerValueField(event, DELTA_Y, dy.round() as i64);
            CGEventSetFlags(event, flags());
        }
        post(event);
    }
}

/// El último clic, para contar los dobles y triples: (botón, cuándo, dónde, cuántos).
type Click = (Button, Instant, (f64, f64), i64);
static LAST_CLICK: Mutex<Option<Click>> = Mutex::new(None);
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

pub fn button(button: Button, down: bool) {
    let (bit, down_kind, up_kind, number) = match button {
        Button::Left => (1, LEFT_DOWN, LEFT_UP, 0),
        Button::Right => (2, RIGHT_DOWN, RIGHT_UP, 1),
        Button::Middle => (4, OTHER_DOWN, OTHER_UP, 2),
        Button::Back => (4, OTHER_DOWN, OTHER_UP, 3),
        Button::Forward => (4, OTHER_DOWN, OTHER_UP, 4),
    };
    if down {
        HELD.fetch_or(bit, Ordering::Relaxed);
    } else {
        HELD.fetch_and(!bit, Ordering::Relaxed);
    }
    let at = cursor();
    // Los clics seguidos, en el mismo lugar y con el mismo botón, son un doble clic.
    let clicks = {
        let mut last = LAST_CLICK.lock().unwrap_or_else(|e| e.into_inner());
        if down {
            let count = match *last {
                Some((b, when, (lx, ly), n)) if b == button && when.elapsed() < DOUBLE_CLICK && (lx - at.0).abs() < 5.0 && (ly - at.1).abs() < 5.0 => n + 1,
                _ => 1,
            };
            *last = Some((button, Instant::now(), at, count));
            count
        } else {
            last.map(|(b, _, _, n)| if b == button { n } else { 1 }).unwrap_or(1)
        }
    };
    // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
    unsafe {
        let event = CGEventCreateMouseEvent(std::ptr::null(), if down { down_kind } else { up_kind }, CGPoint { x: at.0, y: at.1 }, number);
        if !event.is_null() {
            CGEventSetIntegerValueField(event, CLICK_STATE, clicks);
            CGEventSetIntegerValueField(event, BUTTON_NUMBER, number as i64);
            CGEventSetFlags(event, flags());
        }
        post(event);
    }
}

/// Lo que queda de la rueda entre eventos (los saltos van en líneas enteras).
static WHEEL_REST: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

pub fn wheel(dx: f32, dy: f32) {
    let (lx, ly) = {
        let mut rest = WHEEL_REST.lock().unwrap_or_else(|e| e.into_inner());
        rest.0 += dx;
        rest.1 += dy;
        let (lx, ly) = (rest.0.trunc(), rest.1.trunc());
        rest.0 -= lx;
        rest.1 -= ly;
        (lx as i32, ly as i32)
    };
    if lx == 0 && ly == 0 {
        return;
    }
    // En líneas; la segunda rueda positiva va a la izquierda.
    // SAFETY: dos ruedas, así que van dos valores después de `wheel1`.
    unsafe {
        let event = CGEventCreateScrollWheelEvent(std::ptr::null(), 1, 2, ly, -lx);
        if !event.is_null() {
            CGEventSetFlags(event, flags());
        }
        post(event);
    }
}

pub fn key(hid: u16, down: bool, repeat: bool) {
    let Some(code) = keys::mac_from_hid(hid) else { return };
    let mask = modifier_mask(hid);
    if mask != 0 {
        let bit = 1u8 << (hid - 0xE0);
        if down {
            MODS.fetch_or(bit, Ordering::Relaxed);
        } else {
            MODS.fetch_and(!bit, Ordering::Relaxed);
        }
    }
    if hid == keys::CAPS_LOCK && down {
        CAPS_ON.fetch_xor(true, Ordering::Relaxed);
    }
    // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
    unsafe {
        let event = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
        if !event.is_null() {
            if mask != 0 {
                CGEventSetType(event, FLAGS_CHANGED);
            }
            if repeat {
                CGEventSetIntegerValueField(event, AUTOREPEAT, 1);
            }
            CGEventSetFlags(event, flags());
        }
        post(event);
    }
}

// ---------------------------------------------------------------- mandar

/// A quién le llega lo que captura el gancho. Se puede cambiar: una app que
/// apaga y vuelve a prender el servicio reusa el gancho ya instalado.
static GRAB: Mutex<Option<Arc<Grab>>> = Mutex::new(None);
static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn grab() -> Option<Arc<Grab>> {
    GRAB.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn start_capture(grab: Arc<Grab>) -> Result<()> {
    {
        // El gancho ya está: lo que capture va al `Grab` nuevo.
        let mut current = GRAB.lock().unwrap_or_else(|e| e.into_inner());
        if current.is_some() {
            *current = Some(grab);
            return Ok(());
        }
    }
    let (ready, started) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("atic-kvm-tap".into()).spawn(move || {
        let kinds = [
            LEFT_DOWN, LEFT_UP, RIGHT_DOWN, RIGHT_UP, MOVED, LEFT_DRAGGED, RIGHT_DRAGGED, KEY_DOWN, KEY_UP, FLAGS_CHANGED, SCROLL, OTHER_DOWN,
            OTHER_UP, OTHER_DRAGGED,
        ];
        let mask = kinds.iter().fold(0u64, |m, &k| m | (1 << k));
        // SAFETY: el gancho y su fuente viven lo que dura el hilo (todo el programa).
        unsafe {
            let tap = CGEventTapCreate(HID_TAP, 0, 0, mask, on_event, std::ptr::null_mut());
            if tap.is_null() {
                let _ = ready.send(Err(()));
                return;
            }
            TAP.store(tap, Ordering::Relaxed);
            // Recién con el gancho instalado: sin permiso, se puede volver a intentar.
            *GRAB.lock().unwrap_or_else(|e| e.into_inner()) = Some(grab);
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            let _ = ready.send(Ok(()));
            CFRunLoopRun();
        }
    })?;
    match started.recv() {
        Ok(Ok(())) => Ok(()),
        _ => bail!(
            "macOS no deja escuchar el teclado y el mouse: dale permiso de Accesibilidad y de Monitoreo de entrada a la app que corre esto (la Terminal) en Ajustes del Sistema → Privacidad y seguridad"
        ),
    }
}

extern "C" fn on_event(_proxy: *mut c_void, kind: u32, event: CGEventRef, _user: *mut c_void) -> CGEventRef {
    if kind == TAP_DISABLED_TIMEOUT || kind == TAP_DISABLED_USER {
        // El sistema apaga el gancho si tarda; se vuelve a prender.
        let tap = TAP.load(Ordering::Relaxed);
        if !tap.is_null() {
            // SAFETY: el gancho que se creó en `start_capture`, que no se suelta.
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    let Some(grab) = grab() else { return event };
    // SAFETY: `event` es válido mientras dura la llamada.
    let verdict = unsafe {
        if CGEventGetIntegerValueField(event, SOURCE_USER_DATA) == MINE {
            return event;
        }
        let field = |f| CGEventGetIntegerValueField(event, f);
        match kind {
            MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => {
                let at = CGEventGetLocation(event);
                grab.on_move(at.x, at.y, field(DELTA_X) as f64, field(DELTA_Y) as f64, true)
            }
            LEFT_DOWN | LEFT_UP => {
                if kind == LEFT_DOWN {
                    mark_drag();
                }
                grab.on_button(Button::Left, kind == LEFT_DOWN)
            }
            RIGHT_DOWN | RIGHT_UP => grab.on_button(Button::Right, kind == RIGHT_DOWN),
            OTHER_DOWN | OTHER_UP => {
                let button = match field(BUTTON_NUMBER) {
                    3 => Button::Back,
                    4 => Button::Forward,
                    _ => Button::Middle,
                };
                grab.on_button(button, kind == OTHER_DOWN)
            }
            SCROLL => {
                // Un panel táctil da píxeles; una rueda, líneas. Un salto son unos 40 px.
                let (dx, dy) = if field(SCROLL_CONTINUOUS) != 0 {
                    (-(field(SCROLL_POINTS_X) as f32) / 40.0, field(SCROLL_POINTS_Y) as f32 / 40.0)
                } else {
                    (-(field(SCROLL_LINES_X) as f32), field(SCROLL_LINES_Y) as f32)
                };
                grab.on_wheel(dx, dy)
            }
            KEY_DOWN | KEY_UP => match keys::hid_from_mac(field(KEYCODE) as u16) {
                Some(hid) => grab.on_key(hid, kind == KEY_DOWN),
                None => Verdict::Pass,
            },
            FLAGS_CHANGED => match keys::hid_from_mac(field(KEYCODE) as u16) {
                Some(hid) if hid == keys::CAPS_LOCK => {
                    // Bloq Mayús llega una vez por pulsación: abajo y arriba.
                    let v = grab.on_key(hid, true);
                    grab.on_key(hid, false);
                    v
                }
                Some(hid) => {
                    let down = CGEventGetFlags(event) & modifier_mask(hid) & 0xFFFF & !NON_COALESCED != 0;
                    grab.on_key(hid, down)
                }
                None => Verdict::Pass,
            },
            _ => Verdict::Pass,
        }
    };
    match verdict {
        Verdict::Pass => event,
        Verdict::Swallow => std::ptr::null_mut(),
    }
}

/// El cursor queda quieto aquí mientras se usa en el otro equipo.
pub fn hold_cursor(_x: f64, _y: f64) {
    // SAFETY: sin precondiciones.
    unsafe { CGAssociateMouseAndMouseCursorPosition(0) };
}

/// El cursor vuelve a moverse, desde `(x, y)`. Se lleva con un evento y no con
/// `CGWarpMouseCursorPosition`, que congela el mouse un cuarto de segundo.
pub fn release_cursor(x: f64, y: f64) {
    // SAFETY: sin precondiciones.
    unsafe { CGAssociateMouseAndMouseCursorPosition(1) };
    // SAFETY: el evento se crea aquí y `post` lo manda y lo suelta.
    post(unsafe { CGEventCreateMouseEvent(std::ptr::null(), MOVED, CGPoint { x, y }, 0) });
}

// ---------------------------------------------------------------- arrastrar

/// El contador del portapapeles de arrastre al apretar el botón: si al llegar
/// al borde cambió, este arrastre puso algo ahí (si no, lo que hay es de uno
/// anterior, y lo que se arrastra es una ventana o un texto).
static DRAG_MARK: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

fn drag_pasteboard() -> *mut objc2::runtime::AnyObject {
    // AppKit tiene que estar cargado para que `class!(NSPasteboard)` resuelva.
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    let name = objc2_foundation::NSString::from_str("Apple CFPasteboard drag");
    // SAFETY: `pasteboardWithName:` devuelve un objeto +0 que vive en el pool del que llama.
    unsafe { objc2::msg_send![objc2::class!(NSPasteboard), pasteboardWithName: &*name] }
}

fn mark_drag() {
    objc2::rc::autoreleasepool(|_| {
        let pasteboard = drag_pasteboard();
        if !pasteboard.is_null() {
            // SAFETY: un `NSPasteboard` válido dentro del pool.
            let count: isize = unsafe { objc2::msg_send![pasteboard, changeCount] };
            DRAG_MARK.store(count, Ordering::Relaxed);
        }
    });
}

/// Los archivos que se están arrastrando ahora (vacío si el arrastre no es de archivos).
pub fn dragged_files() -> Vec<std::path::PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| {
        let pasteboard = drag_pasteboard();
        if pasteboard.is_null() {
            return Vec::new();
        }
        // SAFETY: mensajes a objetos de AppKit válidos dentro del pool; los +0 viven en él.
        unsafe {
            let count: isize = objc2::msg_send![pasteboard, changeCount];
            if count == DRAG_MARK.load(Ordering::Relaxed) {
                return Vec::new();
            }
            let items: *mut AnyObject = objc2::msg_send![pasteboard, pasteboardItems];
            if items.is_null() {
                return Vec::new();
            }
            let n: usize = objc2::msg_send![items, count];
            let kind = NSString::from_str("public.file-url");
            let mut out = Vec::new();
            for i in 0..n {
                let item: *mut AnyObject = objc2::msg_send![items, objectAtIndex: i];
                let url: *mut NSString = objc2::msg_send![item, stringForType: &*kind];
                if url.is_null() {
                    continue;
                }
                let url: *mut AnyObject = objc2::msg_send![objc2::class!(NSURL), URLWithString: url];
                if url.is_null() {
                    continue;
                }
                let path: *mut NSString = objc2::msg_send![url, path];
                if !path.is_null() {
                    out.push(std::path::PathBuf::from((*path).to_string()));
                }
            }
            out
        }
    })
}
