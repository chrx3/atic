//! La implementación de macOS de `platform` (ver `platform/mod.rs`).
//!
//! Coordenadas: el contrato habla de píxeles físicos con el origen arriba a
//! la izquierda, como Windows. En macOS ese espacio son puntos con el origen
//! arriba a la izquierda de la pantalla principal (el de Quartz y
//! `atic-capture`); `win.rs` convierte desde AppKit, que tiene el origen
//! abajo. Lo que en Windows multiplica por la escala para pasar de lógicos a
//! físicos no debe hacerlo en macOS: ahí los dos son puntos.
//!
//! `hotkeys`, `privacy` y `running` son los mismos archivos de Windows, con
//! su parte de macOS adentro (comparten la lectura de `config.json` y los
//! tipos). Lo demás es propio: `carbon` (atajos), `keys` (el teclado),
//! `coreaudio`, `permissions`, `notify`, `dock` y `main_thread`.

pub mod audio_tap;
pub mod autostart;
mod carbon;
pub mod coreaudio;
mod dock;
pub mod drag;
pub mod glass;
#[path = "../win32/hotkeys.rs"]
pub mod hotkeys;
pub mod keys;
pub mod main_thread;
mod notify;
pub mod ocr;
pub mod paste;
pub mod permissions;
#[path = "../win32/privacy.rs"]
pub mod privacy;
#[path = "../win32/running.rs"]
pub mod running;
pub mod screen_record;
pub mod tray_icon;
pub mod win;
pub mod window_glass;
pub mod window_list;

/// El nombre de una app por su pid (el de `NSRunningApplication`) o, si no
/// tiene, por su identificador de paquete.
pub fn running_name(pid: i32, bundle: Option<&str>) -> String {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    let name = objc2::rc::autoreleasepool(|_| unsafe {
        let app: Option<Retained<AnyObject>> =
            msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        let name: Option<Retained<NSString>> = app.and_then(|app| msg_send![&app, localizedName]);
        name.map(|n| n.to_string())
    });
    name.filter(|n| !n.is_empty())
        .or_else(|| bundle.map(|b| b.rsplit('.').next().unwrap_or(b).to_string()))
        .unwrap_or_default()
}

/// Antes de abrir la app: el PATH del shell del usuario. Abierta desde el
/// Finder o al iniciar sesión, la pill hereda el PATH mínimo del sistema
/// (`/usr/bin:/bin…`) y Atic Code no encontraría `node`, `claude` ni `git` de
/// Homebrew.
pub fn startup() {
    capture_output();
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".into());
    let child = std::process::Command::new(&shell)
        .args(["-l", "-c", "printf %s \"$PATH\""])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn();
    let Ok(child) = child else {
        return;
    };
    // Un `.zprofile` lento no frena el arranque más de 3 s.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let Ok(Ok(out)) = rx.recv_timeout(std::time::Duration::from_secs(3)) else {
        return;
    };
    let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if let Some(path) = merge_path(&std::env::var("PATH").unwrap_or_default(), &login) {
        // SAFETY (edición 2021): todavía no corre ningún hilo que lea el entorno
        // por fuera de `std`.
        std::env::set_var("PATH", path);
    }
}

/// El PATH actual más lo del de inicio de sesión que le falte, en orden.
fn merge_path(current: &str, login: &str) -> Option<String> {
    let mut parts: Vec<&str> = login.split(':').filter(|p| !p.is_empty()).collect();
    for part in current.split(':').filter(|p| !p.is_empty()) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    let merged = parts.join(":");
    (!merged.is_empty() && merged != current).then_some(merged)
}

/// Con la pill ya abierta: el Dock. Los permisos se piden en su pantalla
/// (`permission_setup.rs`).
pub fn ready() {
    dock::start();
}

/// El identificador de la `.app` que corre (`com.ciat.atic` la publicada,
/// `com.ciat.atic.pill` la de desarrollo).
pub fn bundle_id() -> Option<String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    if !bundled() {
        return None;
    }
    // SAFETY: lectura de `NSBundle.mainBundle`.
    let id: Option<Retained<NSString>> = unsafe {
        let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
        msg_send![&bundle, bundleIdentifier]
    };
    id.map(|id| id.to_string())
}

/// La carpeta `.app` de la pill, si corre empaquetada.
pub fn bundle_path() -> Option<std::path::PathBuf> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    if !bundled() {
        return None;
    }
    // SAFETY: lectura de `NSBundle.mainBundle`.
    let path: Option<Retained<NSString>> = unsafe {
        let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
        msg_send![&bundle, bundlePath]
    };
    path.map(|path| std::path::PathBuf::from(path.to_string()))
}

/// Deja lista la vuelta: cuando este proceso termine, se abre `app` (la misma
/// `.app` u otra en su lugar). Quien llama cierra la pill después.
pub fn reopen_after_exit(app: &std::path::Path) -> std::io::Result<()> {
    let script = "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; sleep 0.3; open \"$2\"";
    std::process::Command::new("/bin/sh")
        .args(["-c", script, "atic-reopen", &std::process::id().to_string()])
        .arg(app)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    #[test]
    fn el_path_de_inicio_de_sesion_va_primero() {
        assert_eq!(
            super::merge_path("/usr/bin:/bin", "/opt/homebrew/bin:/usr/bin:/bin").as_deref(),
            Some("/opt/homebrew/bin:/usr/bin:/bin")
        );
        assert_eq!(super::merge_path("/usr/bin", "/usr/bin"), None);
    }
}

/// `CGImageRef` y `CGEventRef` para `msg_send!`: objc2 revisa (en debug) que
/// el tipo de cada argumento y retorno coincida con el del método, y un
/// `*mut c_void` no es un `^{CGImage=}`.
pub mod cg {
    use objc2::encode::{Encoding, RefEncode};

    #[repr(C)]
    pub struct CGImage {
        _private: [u8; 0],
    }

    // SAFETY: tipo opaco de CoreGraphics, solo por puntero.
    unsafe impl RefEncode for CGImage {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
    }

    #[repr(C)]
    pub struct CGEvent {
        _private: [u8; 0],
    }

    // SAFETY: tipo opaco de CoreGraphics, solo por puntero.
    unsafe impl RefEncode for CGEvent {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
    }
}

/// La pill corre empaquetada como `.app` (no el binario suelto).
pub fn bundled() -> bool {
    notify::bundled()
}

/// Empaquetada y abierta desde el Finder, lo que la pill escribe con
/// `println!`/`eprintln!` se perdía: va a `logs/pill-salida.log` (se rehace
/// en cada arranque).
fn capture_output() {
    use std::os::fd::AsRawFd;
    if !bundled() {
        return;
    }
    let Some(dir) = atic_core::AppDirs::new().ok().map(|dirs| dirs.logs_dir()) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::File::create(dir.join("pill-salida.log")) else {
        return;
    };
    extern "C" {
        fn dup2(from: i32, to: i32) -> i32;
    }
    // SAFETY: descriptores válidos; el archivo queda abierto (se suelta con
    // `forget`) mientras vive el proceso.
    unsafe {
        dup2(file.as_raw_fd(), 1);
        dup2(file.as_raw_fd(), 2);
    }
    std::mem::forget(file);
}
