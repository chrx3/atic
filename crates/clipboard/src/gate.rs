use std::sync::{Mutex, TryLockError};

use arboard::{Clipboard, ImageData};
use atic_core::MutexExt;

/// Un solo hilo a la vez puede tener el portapapeles de Windows abierto.
///
/// El watcher vive en un `thread::spawn` y las escrituras (copiar un dibujo,
/// una captura, pegar) corren en otros hilos. Sin este candado los dos
/// llaman `OpenClipboard` a la vez; arboard encodea el PNG *con el clipboard
/// ya abierto*, y `SetClipboardData` termina en `ERROR_CLIPBOARD_NOT_OPEN`
/// (1418, «El subproceso no tiene abierto un Portapapeles»).
static CLIPBOARD_GATE: Mutex<()> = Mutex::new(());

/// Toma el candado para una escritura. El watcher, si está leyendo, termina
/// esa vuelta y después se aparta: no se le deja `OpenClipboard` a mitad de
/// un `set_image`.
pub fn with_clipboard_write<R>(f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let _guard = CLIPBOARD_GATE.lock_or_recover();
    f()
}

/// Texto al portapapeles del sistema, por el mismo candado que las imágenes.
pub fn set_system_text(text: impl Into<String>) -> Result<(), String> {
    let text = text.into();
    with_clipboard_write(|| {
        let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text).map_err(|e| e.to_string())
    })
}

/// Una imagen RGBA al portapapeles del sistema, por el mismo candado.
pub fn set_system_image(width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
    with_clipboard_write(|| {
        let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
        clipboard
            .set_image(ImageData { width, height, bytes: rgba.into() })
            .map_err(|e| e.to_string())
    })
}

/// Intenta leer sin bloquear a quien está escribiendo. `None` = hay una
/// escritura en curso; esta vuelta del watcher se salta.
pub fn try_clipboard_read<R>(f: impl FnOnce() -> R) -> Option<R> {
    let _guard = match CLIPBOARD_GATE.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return None,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };
    Some(f())
}
