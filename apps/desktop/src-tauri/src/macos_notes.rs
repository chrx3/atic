//! Utilidades macOS de la capa Tauri: foco/pegado y permisos TCC.
//!
//! El audio del sistema vive en `crates/audio` (`screencapturekit.rs`), no acá.
//!
//! Permisos TCC (ver `Info.plist`):
//! - NSMicrophoneUsageDescription
//! - NSAudioCaptureUsageDescription / NSScreenCaptureUsageDescription
//!
//! Acá viven los puentes de AppKit que no justifican un módulo propio: app en
//! primer plano y activación (para pegar), ocultar/mostrar apps (window flip),
//! simulación de Cmd+V (Accesibilidad) y apertura de paneles de Privacidad.

#![allow(dead_code)]

use std::ffi::c_void;

use objc2::runtime::AnyObject;

/// Paneles de Privacidad y seguridad que la app puede necesitar.
#[derive(Clone, Copy, Debug)]
pub enum PrivacyPane {
    Accesibilidad,
    GrabacionDePantalla,
    SupervisionDeEntrada,
}

impl PrivacyPane {
    fn anchor(self) -> &'static str {
        match self {
            PrivacyPane::Accesibilidad => "Privacy_Accessibility",
            PrivacyPane::GrabacionDePantalla => "Privacy_ScreenCapture",
            PrivacyPane::SupervisionDeEntrada => "Privacy_ListenEvent",
        }
    }
}

/// Abre el panel correspondiente de Ajustes del Sistema.
pub fn open_privacy_pane(pane: PrivacyPane) {
    let url = format!(
        "x-apple.systempreferences:com.apple.preference.security?{}",
        pane.anchor()
    );
    let _ = std::process::Command::new("open").arg(url).status();
}

/// PID de la aplicación en primer plano, o `None`.
///
/// Se consulta `NSWorkspace` y no el foco del overlay: la pill es
/// non-activating, así que el frente sigue siendo la app del usuario.
pub fn frontmost_app_pid() -> Option<i32> {
    // SAFETY: `sharedWorkspace` y `frontmostApplication` devuelven objetos
    // válidos de AppKit; solo se lee un entero.
    unsafe {
        let workspace: *mut AnyObject =
            objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut AnyObject = objc2::msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = objc2::msg_send![app, processIdentifier];
        Some(pid)
    }
}

/// Reactiva la aplicación `pid` para que reciba el Cmd+V simulado.
pub fn activate_app(pid: i32) {
    if pid <= 0 {
        return;
    }
    // SAFETY: se pide el NSRunningApplication por PID y se activa. Si ya no
    // existe, el puntero es nulo y no se toca nada.
    unsafe {
        let app: *mut AnyObject = objc2::msg_send![
            objc2::class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];
        if app.is_null() {
            return;
        }
        // NSApplicationActivateAllWindows = 1, IgnoringOtherApps = 2.
        // Deprecado en macOS 14 a favor de `activate`, pero sigue funcionando
        // y es el que cubre también versiones viejas.
        let _: bool = objc2::msg_send![app, activateWithOptions: 2u64];
    }
}

/// Nombre del ejecutable de la app `pid`, en minúsculas (identidad de notas).
pub fn app_exe_name(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    // SAFETY: se leen `executableURL` y su `lastPathComponent`; si alguno
    // falta el puntero es nulo y se corta.
    unsafe {
        let app: *mut AnyObject = objc2::msg_send![
            objc2::class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];
        if app.is_null() {
            return None;
        }
        let url: *mut AnyObject = objc2::msg_send![app, executableURL];
        if url.is_null() {
            return None;
        }
        let name: *mut AnyObject = objc2::msg_send![url, lastPathComponent];
        if name.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = objc2::msg_send![name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let text = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .to_ascii_lowercase();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
}

/// Oculta la app entera (`NSRunningApplication.hide`).
pub fn hide_app(pid: i32) {
    if pid <= 0 {
        return;
    }
    // SAFETY: mensaje a un NSRunningApplication válido; si no existe es nulo.
    unsafe {
        let app: *mut AnyObject = objc2::msg_send![
            objc2::class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];
        if !app.is_null() {
            let _: bool = objc2::msg_send![app, hide];
        }
    }
}

/// Deshace [`hide_app`].
pub fn unhide_app(pid: i32) {
    if pid <= 0 {
        return;
    }
    // SAFETY: mensaje a un NSRunningApplication válido; si no existe es nulo.
    unsafe {
        let app: *mut AnyObject = objc2::msg_send![
            objc2::class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];
        if !app.is_null() {
            let _: bool = objc2::msg_send![app, unhide];
        }
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
}

/// ¿El proceso tiene permiso de Accesibilidad (sintetizar teclas)?
pub fn accessibility_trusted() -> bool {
    // SAFETY: consulta pura de TCC, sin argumentos.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Pide Accesibilidad: el diálogo del sistema agrega Atic a la lista.
pub fn request_accessibility() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let value = CFBoolean::true_value();
    let options: CFDictionary<CFString, CFBoolean> =
        CFDictionary::from_CFType_pairs(&[(key, value)]);
    // SAFETY: el diccionario vive hasta el final de la llamada y la API solo
    // lo lee.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) != 0 }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// `kVK_ANSI_V`.
const KEY_V: u16 = 9;
/// `kCGEventFlagMaskCommand`.
const FLAG_COMMAND: u64 = 1 << 20;
/// `kCGHIDEventTap`.
const HID_TAP: u32 = 0;

/// Pega con Cmd+V en la app que tenga el foco.
///
/// Sin Accesibilidad, macOS descarta los eventos sintéticos en silencio: se
/// preflight-ea, se pide el permiso y se devuelve el mensaje para la UI.
pub fn paste_cmd_v() -> Result<(), String> {
    if !accessibility_trusted() {
        let _ = request_accessibility();
        open_privacy_pane(PrivacyPane::Accesibilidad);
        return Err(crate::ui_lang::msg(
            "Atic necesita permiso de Accesibilidad para pegar solo. Actívalo en Ajustes → Privacidad y seguridad → Accesibilidad y reinicia Atic.",
            "Atic needs Accessibility permission to paste. Enable it in Settings → Privacy & Security → Accessibility, then restart Atic.",
        ));
    }
    // SAFETY: eventos efímeros creados acá y liberados acá; la API de CG los
    // copia al postearlos.
    unsafe {
        let down = CGEventCreateKeyboardEvent(std::ptr::null(), KEY_V, true);
        let up = CGEventCreateKeyboardEvent(std::ptr::null(), KEY_V, false);
        if down.is_null() || up.is_null() {
            if !down.is_null() {
                CFRelease(down);
            }
            if !up.is_null() {
                CFRelease(up);
            }
            return Err(crate::ui_lang::msg(
                "No se pudo simular Cmd+V.",
                "Could not simulate Cmd+V.",
            ));
        }
        CGEventSetFlags(down, FLAG_COMMAND);
        CGEventSetFlags(up, FLAG_COMMAND);
        CGEventPost(HID_TAP, down);
        CGEventPost(HID_TAP, up);
        CFRelease(down);
        CFRelease(up);
    }
    Ok(())
}

/// Placeholder: comprobar permisos de micrófono / captura de audio.
pub fn permissions_needed() -> &'static [&'static str] {
    &[
        "micrófono",
        "captura de audio del sistema (ScreenCaptureKit)",
    ]
}

/// Estado documentado de la fase 4 (útil para UI de onboarding futura).
pub fn phase4_status() -> &'static str {
    "andamiaje: Info.plist + stub; falta captura real ScreenCaptureKit"
}
