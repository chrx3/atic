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

/// La ventana de este proceso bajo el cursor (Atic Code) y el punto en su
/// contenido, en píxeles reales (puntos por la escala, como los físicos de
/// Windows): ahí suelta el texto un arrastre del portapapeles. La ventana va
/// como su puntero (`code::own_hwnd` da lo mismo).
pub fn own_window_under_cursor() -> Option<(Target, (i32, i32))> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSRect};

    objc2::MainThreadMarker::new()?;
    let point = NSEvent::mouseLocation();
    // SAFETY: AppKit en el hilo principal; las ventanas son de esta app.
    unsafe {
        let number: isize = msg_send![objc2::class!(NSWindow), windowNumberAtPoint: point, belowWindowWithWindowNumber: 0isize];
        let app: Retained<AnyObject> = msg_send![objc2::class!(NSApplication), sharedApplication];
        let window: Option<Retained<AnyObject>> = msg_send![&app, windowWithWindowNumber: number];
        let window = window?;
        // El overlay de la pill no es destino.
        if super::win::overlay_window().is_some_and(|o| Retained::as_ptr(&o) as usize == Retained::as_ptr(&window) as usize) {
            return None;
        }
        let at: NSPoint = msg_send![&window, convertPointFromScreen: point];
        let frame: NSRect = msg_send![&window, frame];
        let content: NSRect = msg_send![&window, contentRectForFrameRect: frame];
        let scale: f64 = msg_send![&window, backingScaleFactor];
        let y = content.size.height - at.y;
        Some((Retained::as_ptr(&window) as Target, ((at.x * scale) as i32, (y * scale) as i32)))
    }
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

/// La app es una terminal: ahí ⌘V solo pega texto, y una imagen se le pasa
/// a los agentes (Claude Code, Codex…) con ⌃V, que la leen del portapapeles.
pub fn is_terminal(target: Target) -> bool {
    const TERMINALS: [&str; 9] = [
        "com.mitchellh.ghostty",
        "com.apple.Terminal",
        "com.googlecode.iterm2",
        "dev.warp.Warp-Stable",
        "net.kovidgoyal.kitty",
        "org.alacritty",
        "com.github.wez.wezterm",
        "co.zeit.hyper",
        "dev.zed.Zed",
    ];
    NSRunningApplication::runningApplicationWithProcessIdentifier(target as i32)
        .and_then(|app| app.bundleIdentifier())
        .is_some_and(|id| TERMINALS.contains(&id.to_string().as_str()))
}

/// ⌃V sintético: pegar una imagen en un agente de terminal.
pub fn send_control_v() {
    send_chord(1 << 18);
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
    const COMMAND: u64 = 1 << 20;
    const SHIFT: u64 = 1 << 17;
    send_chord(if with_shift { COMMAND | SHIFT } else { COMMAND });
}

/// La V con esas banderas (`CGEventFlags`), bajar y subir.
fn send_chord(flags: u64) {
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
    const HID_TAP: u32 = 0;
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
