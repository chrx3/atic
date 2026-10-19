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
    pooled(f)
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
    Some(pooled(f))
}

/// En macOS, arboard pasa por `NSPasteboard`, que entrega objetos
/// *autoreleased*. Los hilos de Rust no tienen un pool propio: sin este, cada
/// imagen leída o escrita quedaba en memoria para siempre (una captura de
/// pantalla completa dejaba cientos de MB).
fn pooled<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_os = "macos")]
    {
        objc2::rc::autoreleasepool(|_| f())
    }
    #[cfg(not(target_os = "macos"))]
    {
        f()
    }
}

/// La imagen que Atic acaba de copiar (RGBA, como la entrega arboard) y el
/// `clipboard_sequence` de ese momento.
type OwnImage = (u64, usize, usize, Vec<u8>);
static OWN_IMAGE: Mutex<Option<OwnImage>> = Mutex::new(None);

/// Atic acaba de copiar esta imagen (BGRA, una captura o un dibujo): el
/// watcher la suma al historial con estos píxeles en vez de leerla de vuelta.
/// Leerla decodifica la imagen entera y, en macOS, la conversión de
/// `NSPasteboard` deja decenas de MB que el sistema no recupera.
pub fn copied_own_image(width: usize, height: usize, bgra: &[u8]) {
    let Some(sequence) = clipboard_sequence() else {
        return;
    };
    let mut rgba = bgra.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    *OWN_IMAGE.lock_or_recover() = Some((sequence, width, height, rgba));
}

/// La imagen de Atic, si es lo que hay ahora en el portapapeles.
pub(crate) fn take_own_image(sequence: Option<u64>) -> Option<(usize, usize, Vec<u8>)> {
    let sequence = sequence?;
    let mut slot = OWN_IMAGE.lock_or_recover();
    match slot.take() {
        Some((own, width, height, rgba)) if own == sequence => Some((width, height, rgba)),
        // Ya se copió otra cosa encima: esa imagen no se va a leer.
        _ => None,
    }
}

/// Un número que cambia cada vez que alguien escribe en el portapapeles
/// (`changeCount` en macOS, `GetClipboardSequenceNumber` en Windows). Mirarlo
/// no abre el portapapeles ni lee su contenido. `None` si no se sabe:
/// entonces el watcher lee en cada vuelta.
pub fn clipboard_sequence() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;

        // AppKit tiene que estar cargado para que `class!(NSPasteboard)` resuelva.
        #[link(name = "AppKit", kind = "framework")]
        extern "C" {}

        objc2::rc::autoreleasepool(|_| {
            // SAFETY: mensajes a AppKit; `generalPasteboard` es +0 y vive en
            // este pool.
            unsafe {
                let pasteboard: *mut AnyObject =
                    objc2::msg_send![objc2::class!(NSPasteboard), generalPasteboard];
                if pasteboard.is_null() {
                    return None;
                }
                let count: isize = objc2::msg_send![pasteboard, changeCount];
                Some(count as u64)
            }
        })
    }
    #[cfg(windows)]
    {
        // SAFETY: sin argumentos; 0 es «sin acceso al portapapeles».
        let sequence = unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() };
        (sequence != 0).then_some(u64::from(sequence))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_imagen_propia_sale_solo_con_su_secuencia() {
        *OWN_IMAGE.lock_or_recover() = Some((7, 2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(take_own_image(Some(7)), Some((2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8])));
        // Se entrega una vez.
        assert_eq!(take_own_image(Some(7)), None);

        // Otra copia encima: la imagen de Atic ya no está y se descarta.
        *OWN_IMAGE.lock_or_recover() = Some((7, 2, 1, vec![0; 8]));
        assert_eq!(take_own_image(Some(8)), None);
        assert!(OWN_IMAGE.lock_or_recover().is_none());

        assert_eq!(take_own_image(None), None);
    }
}
