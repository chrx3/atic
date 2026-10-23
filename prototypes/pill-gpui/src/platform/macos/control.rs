//! Lo que el celular controla del Mac (`PcCommand` con `feature::CONTROL`):
//! el volumen, silenciar, bloquear la pantalla y pasar diapositivas.
//!
//! El volumen va por AppleScript (`osascript`), que no pide permisos. Bloquear
//! (⌃⌘Q) y las diapositivas (flechas) son teclas a la app del frente, con el
//! permiso de Accesibilidad que la pill ya tiene para pegar.

use super::super::control::Action;

pub fn run(action: Action) {
    match action {
        Action::VolumeUp => volume("set volume output volume ((output volume of (get volume settings)) + 6)"),
        Action::VolumeDown => volume("set volume output volume ((output volume of (get volume settings)) - 6)"),
        Action::Mute => volume("set volume output muted (not (output muted of (get volume settings)))"),
        // ⌃⌘Q: el bloqueo de pantalla del sistema.
        Action::LockScreen => key(Q, CONTROL | COMMAND),
        Action::SlideNext => key(RIGHT, 0),
        Action::SlidePrev => key(LEFT, 0),
    }
}

const Q: u16 = 0x0C;
const RIGHT: u16 = 0x7C;
const LEFT: u16 = 0x7B;
const CONTROL: u64 = 1 << 18;
const COMMAND: u64 = 1 << 20;

fn volume(script: &str) {
    if let Err(error) = std::process::Command::new("osascript").args(["-e", script]).spawn() {
        tracing::warn!(%error, "celular: no se pudo cambiar el volumen");
    }
}

/// La tecla con esas banderas (`CGEventFlags`), bajar y subir.
fn key(code: u16, flags: u64) {
    type CGEventRef = *mut std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateKeyboardEvent(source: *const std::ffi::c_void, key: u16, down: bool) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventPost(tap: u32, event: CGEventRef);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const std::ffi::c_void);
    }
    const HID_TAP: u32 = 0;
    for down in [true, false] {
        // SAFETY: el evento se crea, se manda y se suelta aquí mismo.
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if event.is_null() {
                return;
            }
            CGEventSetFlags(event, flags);
            CGEventPost(HID_TAP, event);
            CFRelease(event);
        }
    }
}
