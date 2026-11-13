//! La pill como app de macOS: sin ícono en el Dock mientras solo está el
//! notch (como cualquier utilidad de la barra de menús) y con él cuando hay
//! una ventana normal abierta (Atic Code, Ajustes, Reuniones, el Espacio),
//! así aparece en Cmd+Tab y en Mission Control. El ícono lleva una insignia
//! según la ventana de adelante (Atic Code, Reuniones o Ajustes), para saber
//! cuál es.

use std::sync::atomic::{AtomicIsize, AtomicU8, Ordering};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSData, NSPoint, NSRect, NSSize, NSString};

/// `NSApplicationActivationPolicy`: 0 normal, 1 accesorio.
const REGULAR: isize = 0;
const ACCESSORY: isize = 1;

static POLICY: AtomicIsize = AtomicIsize::new(-1);
/// La insignia puesta en el ícono (`Badge as u8`; 255: ninguna aún).
static BADGE: AtomicU8 = AtomicU8::new(255);

/// Qué ventana está adelante, para la insignia del ícono.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Badge {
    None = 0,
    Code = 1,
    Meetings = 2,
    Settings = 3,
}

/// Revisa cada segundo si hay ventanas normales y ajusta el Dock.
pub fn start() {
    super::main_thread::run(|| apply(ACCESSORY));
    let _ = std::thread::Builder::new().name("dock".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        super::main_thread::run(|| {
            let open = normal_window_open();
            apply(if open { REGULAR } else { ACCESSORY });
            if open {
                apply_badge(front_badge());
            }
        });
    });
}

fn apply(policy: isize) {
    if POLICY.swap(policy, Ordering::Relaxed) == policy {
        return;
    }
    // SAFETY: en el hilo principal.
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let _: Bool = msg_send![&app, setActivationPolicy: policy];
        if policy == REGULAR {
            // El ícono puesto antes de terminar de arrancar se pierde: se
            // repone cada vez que la pill entra al Dock.
            BADGE.store(255, Ordering::Relaxed);
            set_icon();
            // Al aparecer en el Dock, que la ventana nueva quede delante.
            #[allow(deprecated)]
            let _: () = msg_send![&app, activateIgnoringOtherApps: Bool::YES];
        }
    }
}

/// Una ventana con título visible que no es el overlay ni su vidrio.
fn normal_window_open() -> bool {
    // SAFETY: en el hilo principal; solo se leen las ventanas.
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let windows: Retained<NSArray<AnyObject>> = msg_send![&app, windows];
        windows.iter().any(|window| {
            let visible: Bool = msg_send![&*window, isVisible];
            let mask: usize = msg_send![&*window, styleMask];
            let main: Bool = msg_send![&*window, canBecomeMainWindow];
            // `NSWindowStyleMaskTitled`.
            visible.as_bool() && mask & 1 != 0 && main.as_bool()
        })
    }
}

/// Sin `.app`, el Dock mostraría el ícono genérico de un ejecutable.
fn set_icon() {
    if super::notify::bundled() {
        return;
    }
    let png = include_bytes!("../../../assets/app/dock.png");
    let data = NSData::with_bytes(png);
    // SAFETY: en el hilo principal; la imagen queda retenida por la app.
    unsafe {
        let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        let image: Option<Retained<AnyObject>> = msg_send![image, initWithData: &*data];
        if let Some(image) = image {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![&app, setApplicationIconImage: &*image];
        }
    }
}

/// La ventana normal más adelante de la pill, según su título.
fn front_badge() -> Badge {
    // SAFETY: en el hilo principal; solo se leen las ventanas.
    let title = unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let windows: Retained<NSArray<AnyObject>> = msg_send![&app, orderedWindows];
        windows.iter().find_map(|window| {
            let visible: Bool = msg_send![&*window, isVisible];
            let mask: usize = msg_send![&*window, styleMask];
            let main: Bool = msg_send![&*window, canBecomeMainWindow];
            if !(visible.as_bool() && mask & 1 != 0 && main.as_bool()) {
                return None;
            }
            let title: Option<Retained<NSString>> = msg_send![&*window, title];
            Some(title.map(|t| t.to_string()).unwrap_or_default())
        })
    };
    match title.as_deref() {
        // La de Atic Code lleva delante el proyecto («atic — Atic Code»).
        Some(t) if t.ends_with("Atic Code") => Badge::Code,
        Some(t) if t == crate::i18n::t("pill.meetings.windowTitle") => Badge::Meetings,
        Some(t) if t == crate::i18n::t("pill.settings.windowTitle") => Badge::Settings,
        _ => Badge::None,
    }
}

fn apply_badge(badge: Badge) {
    if BADGE.swap(badge as u8, Ordering::Relaxed) == badge as u8 {
        return;
    }
    let symbol = match badge {
        Badge::None => {
            set_icon_or_default();
            return;
        }
        Badge::Code => "chevron.left.forwardslash.chevron.right",
        Badge::Meetings => "waveform",
        Badge::Settings => "gearshape.fill",
    };
    // SAFETY: en el hilo principal; las imágenes quedan retenidas por la app.
    unsafe {
        if let Some(image) = badged_icon(symbol) {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![&app, setApplicationIconImage: &*image];
        }
    }
}

/// Sin insignia: el de la `.app` (nil lo repone) o, suelta, el de siempre.
fn set_icon_or_default() {
    if super::notify::bundled() {
        // SAFETY: en el hilo principal.
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let none: *const AnyObject = std::ptr::null();
            let _: () = msg_send![&app, setApplicationIconImage: none];
        }
    } else {
        set_icon();
    }
}

/// El ícono de Atic con un círculo blanco abajo a la derecha y el símbolo
/// del sistema `symbol` adentro.
unsafe fn badged_icon(symbol: &str) -> Option<Retained<AnyObject>> {
    let data = NSData::with_bytes(include_bytes!("../../../assets/app/dock.png"));
    let base: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
    let base: Retained<AnyObject> = msg_send![base, initWithData: &*data];
    let size = NSSize::new(512.0, 512.0);
    let out: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
    let out: Retained<AnyObject> = msg_send![out, initWithSize: size];
    let name = NSString::from_str(symbol);
    let none: *const AnyObject = std::ptr::null();
    let sym: Option<Retained<AnyObject>> = msg_send![class!(NSImage), imageWithSystemSymbolName: &*name, accessibilityDescription: none];
    let sym = sym?;
    // La parte visible del ícono deja aire alrededor (la grilla de macOS):
    // la insignia va en la esquina de esa parte.
    let d = 196.0;
    let circle = NSRect::new(NSPoint::new(512.0 - 56.0 - d, 40.0), NSSize::new(d, d));
    let config: Retained<AnyObject> = msg_send![class!(NSImageSymbolConfiguration), configurationWithPointSize: 92.0f64, weight: 0.4f64];
    let sym: Retained<AnyObject> = msg_send![&sym, imageWithSymbolConfiguration: &*config];
    let sym_size: NSSize = msg_send![&sym, size];

    let _: () = msg_send![&out, lockFocus];
    let all = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
    // `NSCompositingOperationSourceOver`.
    let _: () = msg_send![&base, drawInRect: all, fromRect: zero, operation: 2usize, fraction: 1.0f64];
    let white: Retained<AnyObject> = msg_send![class!(NSColor), whiteColor];
    let _: () = msg_send![&white, setFill];
    let oval: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: circle];
    let _: () = msg_send![&oval, fill];
    let at = NSRect::new(
        NSPoint::new(circle.origin.x + (d - sym_size.width) / 2.0, circle.origin.y + (d - sym_size.height) / 2.0),
        sym_size,
    );
    let _: () = msg_send![&sym, drawInRect: at, fromRect: zero, operation: 2usize, fraction: 1.0f64];
    let _: () = msg_send![&out, unlockFocus];
    Some(out)
}
