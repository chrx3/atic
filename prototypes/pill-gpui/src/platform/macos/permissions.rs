//! Los permisos de privacidad de macOS (TCC) que usa la pill, uno por
//! herramienta:
//!
//! - **Accesibilidad**: pegar (Cmd+V sintético), las teclas multimedia y
//!   traer ventanas de otras apps.
//! - **Monitoreo de entrada**: sondear el teclado para el dictado mantenido y
//!   la rueda (los atajos de Carbon no lo necesitan).
//! - **Grabación de pantalla**: Capturas, Pizarra, Flip, grabar, el
//!   cuentagotas y los títulos de las ventanas (detectar reuniones).
//! - **Micrófono**: dictado y Reuniones.
//! - **Automatización**: mover la canción en Spotify y Música, vaciar la
//!   papelera (macOS lo pregunta solo la primera vez, por app).
//!
//! Con la pill empaquetada (`Atic.app`) el permiso queda a su nombre; con el
//! binario suelto, a nombre de la terminal que la abrió.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Accessibility,
    InputMonitoring,
    ScreenRecording,
    Microphone,
    SystemAudio,
}

impl Permission {
    pub const ALL: [Permission; 5] = [
        Permission::Accessibility,
        Permission::InputMonitoring,
        Permission::ScreenRecording,
        Permission::Microphone,
        Permission::SystemAudio,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Permission::Accessibility => "Accesibilidad",
            Permission::InputMonitoring => "Monitoreo de entrada",
            Permission::ScreenRecording => "Grabación de pantalla",
            Permission::Microphone => "Micrófono",
            Permission::SystemAudio => "Grabación de audio del sistema",
        }
    }

    pub fn why(self) -> &'static str {
        match self {
            Permission::Accessibility => "Pegar lo que copias o dictas en otras apps, las teclas de música y traer ventanas al frente.",
            Permission::InputMonitoring => "Saber si mantienes el atajo del dictado o de la rueda mientras usas otra app.",
            Permission::ScreenRecording => "Capturas, Pizarra, Flip, grabar la pantalla, el cuentagotas y ver qué llamada está abierta.",
            Permission::Microphone => "Dictar y grabar reuniones.",
            Permission::SystemAudio => "La onda de la pill con lo que suena de verdad y el volumen de cada app en Sistema.",
        }
    }

    /// Cuánto le hace falta a Atic (la pantalla de permisos lo marca).
    pub fn level(self) -> Level {
        match self {
            Permission::Accessibility => Level::Required,
            Permission::InputMonitoring | Permission::ScreenRecording | Permission::Microphone => Level::Recommended,
            Permission::SystemAudio => Level::Optional,
        }
    }

    /// Qué va a pasar al pedirlo y qué hacer, para quien lo está pidiendo.
    /// `holder` es el nombre con que Atic aparece en Ajustes del Sistema.
    pub fn how(self, state: State, holder: &str) -> String {
        let list = format!(
            "Se abre Ajustes del Sistema: activa «{holder}» en la lista y vuelve aquí. \
             ¿No aparece? Toca + y elígelo en Aplicaciones."
        );
        match (self, state) {
            (Permission::Microphone | Permission::SystemAudio, State::NotAsked) => {
                "macOS te va a preguntar en un diálogo: elige «Permitir».".to_string()
            }
            (Permission::Microphone | Permission::SystemAudio, _) => {
                format!("Ya se respondió que no: en Ajustes del Sistema, activa «{holder}» y vuelve aquí.")
            }
            (Permission::ScreenRecording, _) => format!("{list} Si macOS ofrece salir y volver a abrir Atic, acepta."),
            _ => list,
        }
    }

    /// El panel de Ajustes del Sistema donde se concede.
    fn pane(self) -> &'static str {
        match self {
            Permission::Accessibility => "Privacy_Accessibility",
            Permission::InputMonitoring => "Privacy_ListenEvent",
            Permission::ScreenRecording => "Privacy_ScreenCapture",
            Permission::Microphone => "Privacy_Microphone",
            Permission::SystemAudio => "Privacy_AudioCapture",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Sin él lo básico no anda (pegar lo copiado o dictado).
    Required,
    /// Lo usa una herramienta principal.
    Recommended,
    /// Mejora algo que igual funciona sin él.
    Optional,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Required => "Requerido",
            Level::Recommended => "Recomendado",
            Level::Optional => "Opcional",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Granted,
    Denied,
    /// macOS todavía no lo preguntó.
    NotAsked,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

/// `kIOHIDRequestTypeListenEvent`.
const LISTEN_EVENT: u32 = 1;

pub fn state(permission: Permission) -> State {
    // SAFETY: consultas puras de TCC.
    unsafe {
        match permission {
            Permission::Accessibility => {
                if AXIsProcessTrusted() {
                    State::Granted
                } else {
                    State::Denied
                }
            }
            // `kIOHIDAccessTypeGranted` = 0, `Denied` = 1, `Unknown` = 2.
            Permission::InputMonitoring => match IOHIDCheckAccess(LISTEN_EVENT) {
                0 => State::Granted,
                1 => State::Denied,
                _ => State::NotAsked,
            },
            Permission::ScreenRecording => {
                if CGPreflightScreenCaptureAccess() {
                    State::Granted
                } else {
                    State::Denied
                }
            }
            // Solo lo dice el `TCC.framework` privado; sin él, «sin preguntar».
            Permission::SystemAudio => match tcc::preflight() {
                Some(0) => State::Granted,
                Some(1) => State::Denied,
                _ => State::NotAsked,
            },
            Permission::Microphone => {
                let Some(class) = objc2::runtime::AnyClass::get(c"AVCaptureDevice") else {
                    return State::NotAsked;
                };
                let audio = NSString::from_str("soun");
                // `AVAuthorizationStatus`: 0 sin preguntar, 1 restringido,
                // 2 negado, 3 concedido.
                let status: isize = msg_send![class, authorizationStatusForMediaType: &*audio];
                match status {
                    3 => State::Granted,
                    0 => State::NotAsked,
                    _ => State::Denied,
                }
            }
        }
    }
}

/// Lo pide: el diálogo de macOS si todavía no se preguntó; si ya se negó,
/// abre su panel en Ajustes del Sistema.
pub fn request(permission: Permission) {
    if state(permission) == State::Granted {
        return;
    }
    // SAFETY: llamadas de TCC; el diccionario vive durante la llamada.
    let asked = unsafe {
        match permission {
            Permission::Accessibility => {
                let key: &NSString = &*(kAXTrustedCheckOptionPrompt as *const NSString);
                let yes = NSNumber::new_bool(true);
                let options = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[yes.as_ref()]);
                AXIsProcessTrustedWithOptions(Retained::as_ptr(&options).cast());
                true
            }
            Permission::InputMonitoring => {
                let first = state(permission) == State::NotAsked;
                IOHIDRequestAccess(LISTEN_EVENT);
                first
            }
            Permission::ScreenRecording => {
                CGRequestScreenCaptureAccess();
                false
            }
            Permission::SystemAudio => {
                let first = state(permission) == State::NotAsked;
                if !tcc::request() {
                    // Sin el SPI: arrancar un tap muestra el diálogo del sistema
                    // (la `.app` trae `NSAudioCaptureUsageDescription`).
                    let _ = super::audio_tap::Meter::start();
                }
                first
            }
            Permission::Microphone => {
                let first = state(permission) == State::NotAsked;
                if let Some(class) = objc2::runtime::AnyClass::get(c"AVCaptureDevice") {
                    let audio = NSString::from_str("soun");
                    let done = block2::RcBlock::new(|_granted: Bool| {});
                    let _: () = msg_send![class, requestAccessForMediaType: &*audio, completionHandler: &*done];
                }
                first
            }
        }
    };
    if !asked {
        open_settings(permission);
    }
}

/// Abre Ajustes del Sistema en el panel del permiso.
pub fn open_settings(permission: Permission) {
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{}", permission.pane());
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Antes de capturar la pantalla: si falta el permiso lo pide y devuelve
/// `false` (la captura saldría sin las otras apps).
pub fn ensure_screen_capture() -> bool {
    if state(Permission::ScreenRecording) == State::Granted {
        return true;
    }
    request(Permission::ScreenRecording);
    false
}

/// La app de Atic que ve el usuario en Ajustes del Sistema.
pub fn holder() -> String {
    if super::notify::bundled() {
        // SAFETY: lectura de `NSBundle.mainBundle`.
        let name: Option<Retained<NSString>> = unsafe {
            let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
            let key = NSString::from_str("CFBundleName");
            msg_send![&bundle, objectForInfoDictionaryKey: &*key]
        };
        return name.map(|n| n.to_string()).unwrap_or_else(|| "Atic".into());
    }
    "la terminal que abrió la pill".into()
}

/// `TCC.framework` (privado): lo único que dice si está el permiso de audio
/// del sistema, y lo pide. Si falta el símbolo, se asume «sin preguntar».
mod tcc {
    use std::ffi::{c_char, c_void, CStr};

    use objc2_foundation::NSString;

    extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    const SERVICE: &str = "kTCCServiceAudioCapture";

    fn symbol(name: &CStr) -> Option<*mut c_void> {
        // SAFETY: ruta y nombres constantes; la biblioteca queda cargada.
        unsafe {
            let lib = dlopen(c"/System/Library/PrivateFrameworks/TCC.framework/TCC".as_ptr(), 2);
            if lib.is_null() {
                return None;
            }
            let sym = dlsym(lib, name.as_ptr());
            (!sym.is_null()).then_some(sym)
        }
    }

    /// 0 concedido, 1 negado, 2 sin preguntar.
    pub fn preflight() -> Option<i32> {
        type F = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;
        // SAFETY: la firma de `TCCAccessPreflight(CFStringRef, CFDictionaryRef)`.
        let f: F = unsafe { std::mem::transmute(symbol(c"TCCAccessPreflight")?) };
        let service = NSString::from_str(SERVICE);
        // SAFETY: `NSString` es puente gratuito con `CFStringRef`.
        Some(unsafe { f(objc2::rc::Retained::as_ptr(&service).cast(), std::ptr::null()) })
    }

    /// Muestra el diálogo. `false` si el SPI no está.
    pub fn request() -> bool {
        type F = unsafe extern "C" fn(*const c_void, *const c_void, *const c_void);
        let Some(sym) = symbol(c"TCCAccessRequest") else {
            return false;
        };
        // SAFETY: la firma de `TCCAccessRequest(CFStringRef, CFDictionaryRef, bloque)`.
        let f: F = unsafe { std::mem::transmute(sym) };
        let service = NSString::from_str(SERVICE);
        let done = block2::RcBlock::new(|_granted: objc2::runtime::Bool| {});
        // SAFETY: el bloque vive durante la llamada (TCC lo copia).
        unsafe { f(objc2::rc::Retained::as_ptr(&service).cast(), std::ptr::null(), (&*done as *const block2::Block<dyn Fn(objc2::runtime::Bool)>).cast()) };
        true
    }
}
