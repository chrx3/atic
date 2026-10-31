//! Los menús de la app activa en la barra de arriba (Archivo, Edición, …,
//! Ayuda), leídos por Accesibilidad. La pill fundida con el notch los usa
//! para no taparlos al crecer hacia la izquierda.

use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;

use super::win::{all_screens, Screen, RECT};

type CFTypeRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
    fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: *mut CFTypeRef) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: CFTypeRef);
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
}

/// `kAXValueCGPointType` y `kAXValueCGSizeType`.
const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;
/// Lo más que se espera a una app: una colgada no traba la pill.
const AX_TIMEOUT: f32 = 0.1;
/// Cada cuánto se vuelve a leer la barra de la misma app (los menús casi no
/// cambian; al cambiar de app se lee al tiro).
const REREAD: Duration = Duration::from_secs(2);

/// La app activa y hasta dónde llegan sus menús desde la orilla izquierda
/// del monitor (`None`: no se pudo leer).
static CACHE: Mutex<Option<(i32, Instant, Option<i32>)>> = Mutex::new(None);

/// Lo que ocupan los menús de la app activa en la barra de `screen`: desde su
/// orilla izquierda hasta el último (Ayuda). `None` si no se sabe: sin
/// permiso de Accesibilidad, o la app no expone su barra.
pub fn app_menus(screen: &Screen) -> Option<RECT> {
    let pid = NSWorkspace::sharedWorkspace().frontmostApplication()?.processIdentifier();
    let mut cache = CACHE.lock().ok()?;
    let reach = match *cache {
        Some((cached, at, reach)) if cached == pid && at.elapsed() < REREAD => reach,
        _ => {
            let reach = read_reach(pid);
            *cache = Some((pid, Instant::now(), reach));
            reach
        }
    };
    // La barra de cada monitor muestra los mismos menús, a la misma distancia
    // de su orilla.
    let monitor = screen.monitor;
    reach.map(|reach| RECT {
        left: monitor.left,
        top: monitor.top,
        right: monitor.left + reach,
        bottom: monitor.top + (screen.work.top - monitor.top).max(0),
    })
}

/// Hasta dónde llega el último menú de `pid`, medido desde la orilla
/// izquierda del monitor donde está su barra.
fn read_reach(pid: i32) -> Option<i32> {
    // SAFETY: consulta pura de TCC.
    if !unsafe { AXIsProcessTrusted() } {
        return None;
    }
    let menu_bar = NSString::from_str("AXMenuBar");
    let children = NSString::from_str("AXChildren");
    let position = NSString::from_str("AXPosition");
    let size = NSString::from_str("AXSize");
    // SAFETY: cada objeto que se copia se suelta antes de salir; los hijos
    // del arreglo son prestados y no se sueltan.
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        AXUIElementSetMessagingTimeout(app, AX_TIMEOUT);
        let bar = copy(app, &menu_bar);
        CFRelease(app);
        let bar = bar?;
        let items = copy(bar, &children);
        CFRelease(bar);
        let items = items?;
        let mut first: Option<(f64, f64)> = None;
        let mut right = f64::MIN;
        for index in 0..CFArrayGetCount(items) {
            let item = CFArrayGetValueAtIndex(items, index);
            let (Some((x, y)), Some((w, h))) = (value(item, &position, AX_POINT), value(item, &size, AX_SIZE)) else {
                continue;
            };
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            first.get_or_insert((x, y));
            right = right.max(x + w);
        }
        CFRelease(items);
        let Some((x, y)) = first else {
            // Una app sin menús: no ocupa nada.
            return Some(0);
        };
        // El monitor de la barra que se leyó (la del monitor activo).
        let home = all_screens().into_iter().find(|screen| {
            let m = screen.monitor;
            x >= m.left as f64 && x < m.right as f64 && y >= m.top as f64 && y < m.bottom as f64
        })?;
        Some((right - home.monitor.left as f64).ceil() as i32)
    }
}

/// `attribute` de `element`, copiado (hay que soltarlo).
unsafe fn copy(element: CFTypeRef, attribute: &Retained<NSString>) -> Option<CFTypeRef> {
    let mut out: CFTypeRef = std::ptr::null();
    let name = Retained::as_ptr(attribute) as CFTypeRef;
    let status = AXUIElementCopyAttributeValue(element, name, &mut out);
    (status == 0 && !out.is_null()).then_some(out)
}

/// Un punto o un tamaño (`kind`) de `element`, como `(a, b)`.
unsafe fn value(element: CFTypeRef, attribute: &Retained<NSString>, kind: u32) -> Option<(f64, f64)> {
    let raw = copy(element, attribute)?;
    let mut pair = [0.0f64; 2];
    let ok = AXValueGetValue(raw, kind, pair.as_mut_ptr() as *mut c_void);
    CFRelease(raw);
    ok.then_some((pair[0], pair[1]))
}
