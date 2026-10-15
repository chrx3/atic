//! Pegar en otra app desde macOS: activar la app destino y mandarle Cmd+V con
//! `CGEvent`. Necesita el permiso de Accesibilidad (Ajustes del Sistema →
//! Privacidad y seguridad); sin él macOS descarta las teclas sin avisar.
//!
//! El destino es una app (su pid), no una ventana: activar la app trae al
//! frente su última ventana, que es donde estaba el foco.

use objc2_app_kit::{NSApplicationActivationOptions, NSEvent, NSRunningApplication, NSWorkspace};

/// La app destino (su pid), guardada como entero para cruzar tareas async.
pub type Target = isize;

fn own_pid() -> i32 {
    std::process::id() as i32
}

/// La app activa, si no es la pill.
pub fn foreground_target() -> Option<Target> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    (pid > 0 && pid != own_pid()).then_some(pid as Target)
}

/// Dónde pega el dictado: la app activa, aunque sea esta (Atic Code, el
/// Espacio, Reuniones). El overlay de la pill no activa la app, así que nunca
/// es el destino.
pub fn dictation_target() -> Option<Target> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    (pid > 0).then_some(pid as Target)
}

/// La ventana de este proceso bajo el cursor y el punto en ella, para soltar
/// texto arrastrado desde el portapapeles. Pendiente en macOS.
pub fn own_window_under_cursor() -> Option<(Target, (i32, i32))> {
    None
}

/// La app dueña de la ventana bajo el cursor, si no es la pill.
pub fn target_under_cursor() -> Option<Target> {
    // AppKit da el cursor con el origen abajo; Quartz, arriba.
    let point = NSEvent::mouseLocation();
    let y = super::win::primary_height_points() - point.y;
    let window = super::window_list::on_screen()
        .into_iter()
        .find(|window| window.layer == 0 && window.contains(point.x, y))?;
    (window.pid > 0 && window.pid != own_pid()).then_some(window.pid as Target)
}

/// En macOS Cmd+V pega en todas las terminales.
pub fn needs_ctrl_shift_v(_target: Target) -> bool {
    false
}

/// ¿Soltar texto necesita el pegado de respaldo? (ver `win32/paste.rs`).
pub fn drop_needs_paste_fallback(dropped: bool, effect: u32, web_terminal: bool) -> bool {
    dropped && (effect == 0 || web_terminal)
}

/// Activa la app destino.
pub fn force_foreground(target: Target) {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(target as i32) else {
        return;
    };
    #[allow(deprecated)]
    app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
}

/// Cmd+V (o Cmd+Shift+V) sintético. Las banderas van en el evento mismo, así
/// que un Opción o un Shift que siga apretado no se suma al atajo.
pub fn send_paste_chord(with_shift: bool) {
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
    const V: u16 = 0x09;
    const COMMAND: u64 = 1 << 20;
    const SHIFT: u64 = 1 << 17;
    const HID_TAP: u32 = 0;

    let flags = if with_shift { COMMAND | SHIFT } else { COMMAND };
    for down in [true, false] {
        // SAFETY: el evento se crea, se manda y se suelta aquí mismo.
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), V, down);
            if event.is_null() {
                return;
            }
            CGEventSetFlags(event, flags);
            CGEventPost(HID_TAP, event);
            CFRelease(event);
        }
    }
}
