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
}

impl Guard {
    /// Sin mutex: para una ventana sola que no compite con la pill.
    pub fn none() -> Self {
        Self {
            #[cfg(windows)]
            handle: std::ptr::null_mut(),
        }
    }
}

#[cfg(windows)]
const NAME: &str = "Local\\atic-pill-gpui";

/// `None` si ya hay otra pill corriendo.
#[cfg(windows)]
pub fn acquire() -> Option<Guard> {
    acquire_named(NAME)
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

#[cfg(not(windows))]
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
