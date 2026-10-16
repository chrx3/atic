//! Una sola pill por sesión de usuario: dos pills se pelearían los atajos, el
//! portapapeles y el borde de la pantalla.
//!
//! Un mutex con nombre en el espacio de la sesión (`Local\`): el primero que
//! lo crea es el dueño; si ya existía, hay otra pill corriendo. El sistema lo
//! suelta solo cuando el proceso termina, aunque sea por un pánico.

/// Mientras viva, esta es la pill de la sesión.
pub struct Guard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    /// macOS: el archivo con el candado (`flock`), que el sistema suelta al
    /// cerrarse el proceso.
    #[cfg(target_os = "macos")]
    _lock: Option<std::fs::File>,
}

impl Guard {
    /// Sin mutex: para una ventana sola que no compite con la pill.
    pub fn none() -> Self {
        Self {
            #[cfg(windows)]
            handle: std::ptr::null_mut(),
            #[cfg(target_os = "macos")]
            _lock: None,
        }
    }
}

#[cfg(windows)]
const NAME: &str = "Local\\atic-pill-gpui";

/// `None` si ya hay otra pill corriendo.
#[cfg(windows)]
pub fn acquire() -> Option<Guard> {
    // La de desarrollo tiene su propio candado: convive con la de siempre.
    acquire_named(if crate::dev::on() { "Local\\atic-pill-gpui-dev" } else { NAME })
}

#[cfg(windows)]
fn acquire_named(name: &str) -> Option<Guard> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `name` termina en 0 y vive durante la llamada.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        // Sin mutex no se puede saber; mejor arrancar que quedarse sin pill.
        return Some(Guard { handle });
    }
    // SAFETY: se lee justo después de `CreateMutexW`.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: `handle` es válido y no se usa después.
        unsafe { CloseHandle(handle) };
        return None;
    }
    Some(Guard { handle })
}

/// macOS: un candado exclusivo sobre `pill.lock` (o `pill-dev.lock`) en los
/// datos de la pill.
#[cfg(target_os = "macos")]
pub fn acquire() -> Option<Guard> {
    let name = if crate::dev::on() { "pill-dev.lock" } else { "pill.lock" };
    let Some(path) = crate::paths::file(name) else {
        return Some(Guard::none());
    };
    acquire_file(&path)
}

#[cfg(target_os = "macos")]
fn acquire_file(path: &std::path::Path) -> Option<Guard> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path) else {
        // Sin candado no se puede saber; mejor arrancar que quedarse sin pill.
        return Some(Guard::none());
    };
    match file.try_lock() {
        Ok(()) => Some(Guard { _lock: Some(file) }),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(_) => Some(Guard::none()),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn acquire() -> Option<Guard> {
    Some(Guard {})
}

#[cfg(windows)]
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: el handle es nuestro y se cierra una sola vez.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod mac_tests {
    use super::*;

    #[test]
    fn la_segunda_no_arranca_mientras_viva_la_primera() {
        let path = std::env::temp_dir().join(format!("atic-pill-test-{}.lock", std::process::id()));
        let first = acquire_file(&path);
        assert!(first.as_ref().is_some_and(|g| g._lock.is_some()));
        assert!(acquire_file(&path).is_none());
        drop(first);
        assert!(acquire_file(&path).is_some());
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn la_segunda_no_arranca_mientras_viva_la_primera() {
        // Otro nombre: la pill de verdad puede estar corriendo.
        let name = format!("Local\\atic-pill-gpui-test-{}", std::process::id());
        let first = acquire_named(&name);
        assert!(first.is_some());
        assert!(acquire_named(&name).is_none());
        drop(first);
        assert!(acquire_named(&name).is_some());
    }
}
