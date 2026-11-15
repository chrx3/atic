//! El hover no atraviesa otras ventanas.
//!
//! GPUI 0.2.2 sigue el mouse con un área «activa siempre»: macOS le manda los
//! movimientos a sus ventanas aunque otra (el Finder, por ejemplo) las tape,
//! y Atic Code mostraba hover y globitos debajo de ella (también bajo los
//! paneles de la pill, donde además le peleaba el cursor). Un monitor local
//! ve cada movimiento antes que GPUI: si la ventana que lo recibe está tapada
//! en ese punto por otra app, GPUI lo recibe lejos de la ventana y suelta el
//! hover; si la tapa la pill, no le llega.
//!
//! Solo en ventanas de nivel normal (Atic Code, Ajustes…): la pill flota
//! encima de todo.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSPoint, NSRect};

const MOUSE_MOVED: u64 = 1 << 5;
const MOUSE_MOVED_TYPE: usize = 5;

pub fn start() {
    let handler = RcBlock::new(|event: *mut AnyObject| -> *mut AnyObject {
        // SAFETY: AppKit llama al monitor en el hilo principal con un evento vivo.
        unsafe { guard(event) }
    });
    // SAFETY: el monitor queda puesto todo el proceso; AppKit retiene el bloque.
    unsafe {
        let monitor: Option<Retained<AnyObject>> =
            msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: MOUSE_MOVED, handler: &*handler];
        std::mem::forget(monitor);
    }
}

unsafe fn guard(event: *mut AnyObject) -> *mut AnyObject {
    let Some(event_ref) = (unsafe { event.as_ref() }) else {
        return event;
    };
    let window: Option<Retained<AnyObject>> = unsafe { msg_send![event_ref, window] };
    let window = match window {
        // Tapada por la pill con un panel abierto encima: el movimiento no le
        // llega. Mandarlo lejos no sirve aquí: en la ventana clave AppKit
        // vuelve a poner la flecha y el cursor parpadeaba sobre el panel.
        Some(window) => {
            let level: isize = unsafe { msg_send![&window, level] };
            let number: isize = unsafe { msg_send![&window, windowNumber] };
            // La ventana clave recibe los movimientos de toda la pantalla;
            // fuera de su marco GPUI ya no ve nada bajo el cursor.
            if level != 0 || !under_cursor(&window) || !covered(number) {
                return event;
            }
            return std::ptr::null_mut();
        }
        // Tapada por otra app: AppKit no le da ventana al movimiento (trae la
        // de la otra app), pero el área de GPUI lo recibe igual.
        None => match own_window_at_cursor() {
            Some(window) => window,
            None => return event,
        },
    };
    let number: isize = unsafe { msg_send![&window, windowNumber] };
    // El mismo movimiento, fuera de la ventana: GPUI no encuentra nada bajo el
    // cursor y apaga el hover.
    let far = NSPoint::new(-100_000.0, -100_000.0);
    let flags: usize = unsafe { msg_send![event_ref, modifierFlags] };
    let timestamp: f64 = unsafe { msg_send![event_ref, timestamp] };
    let replacement: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            class!(NSEvent),
            mouseEventWithType: MOUSE_MOVED_TYPE,
            location: far,
            modifierFlags: flags,
            timestamp: timestamp,
            windowNumber: number,
            context: std::ptr::null_mut::<AnyObject>(),
            eventNumber: 0isize,
            clickCount: 0isize,
            pressure: 0.0f32
        ]
    };
    match replacement {
        Some(replacement) => Retained::autorelease_ptr(replacement),
        None => event,
    }
}

/// Un clic en el cursor caería en otra ventana: de otra app o la pill con un
/// panel abierto encima. AppKit salta las que dejan pasar el mouse, como la
/// pill en reposo, que cubre la pantalla entera.
fn covered(number: isize) -> bool {
    // SAFETY: consultas de AppKit en el hilo principal.
    let top: isize = unsafe {
        let point: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        msg_send![class!(NSWindow), windowNumberAtPoint: point, belowWindowWithWindowNumber: 0isize]
    };
    top != 0 && top != number
}

/// La ventana normal de Atic que contiene el cursor, la de más adelante.
fn own_window_at_cursor() -> Option<Retained<AnyObject>> {
    // SAFETY: consultas de AppKit en el hilo principal.
    unsafe {
        let point: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let windows: Retained<NSArray<AnyObject>> = msg_send![&app, orderedWindows];
        windows.iter().find(|window| {
            let visible: Bool = msg_send![window, isVisible];
            let level: isize = msg_send![window, level];
            visible.as_bool() && level == 0 && under_cursor(window)
        })
    }
}

fn under_cursor(window: &AnyObject) -> bool {
    // SAFETY: consultas de AppKit en el hilo principal.
    let (point, frame): (NSPoint, NSRect) =
        unsafe { (msg_send![class!(NSEvent), mouseLocation], msg_send![window, frame]) };
    point.x >= frame.origin.x
        && point.x < frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y < frame.origin.y + frame.size.height
}
