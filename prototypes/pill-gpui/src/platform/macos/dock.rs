//! La pill como app de macOS: sin ícono en el Dock mientras solo está el
//! notch (como cualquier utilidad de la barra de menús) y con él cuando hay
//! una ventana normal abierta (Atic Code, Ajustes, Reuniones, el Espacio),
//! así aparece en Cmd+Tab y en Mission Control.

use std::sync::atomic::{AtomicIsize, Ordering};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSData};

/// `NSApplicationActivationPolicy`: 0 normal, 1 accesorio.
const REGULAR: isize = 0;
const ACCESSORY: isize = 1;

static POLICY: AtomicIsize = AtomicIsize::new(-1);

/// Revisa cada segundo si hay ventanas normales y ajusta el Dock.
pub fn start() {
    super::main_thread::run(|| apply(ACCESSORY));
    let _ = std::thread::Builder::new().name("dock".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        super::main_thread::run(|| apply(if normal_window_open() { REGULAR } else { ACCESSORY }));
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
    let png = include_bytes!("../../../../../apps/desktop/src-tauri/icons/128x128@2x.png");
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
