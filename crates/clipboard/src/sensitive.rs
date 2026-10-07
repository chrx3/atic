/// ¿Lo que hay en el portapapeles pidió no quedar archivado?
///
/// Windows define dos formatos con los que quien copia declara que su contenido
/// es efímero. Los ponen los gestores de contraseñas (Bitwarden, 1Password,
/// KeePass) y algunos navegadores en campos de contraseña:
///
/// - `ExcludeClipboardContentFromMonitorProcessing` — su sola presencia
///   significa «ningún monitor debería tocar esto».
/// - `CanIncludeInClipboardHistory` — un DWORD; `0` es «no lo archives».
///
/// `arboard` no los mira: entrega el texto igual. Sin esta comprobación, una
/// contraseña copiada termina en `history.json` en claro y sobrevive al pegado,
/// que es exactamente lo que el gestor intentó evitar.
#[cfg(windows)]
pub fn clipboard_is_sensitive() -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    fn format_id(name: &str) -> u32 {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    }

    unsafe {
        let exclude = format_id("ExcludeClipboardContentFromMonitorProcessing");
        if exclude != 0 && IsClipboardFormatAvailable(exclude) != 0 {
            return true;
        }

        let can_include = format_id("CanIncludeInClipboardHistory");
        if can_include == 0 || IsClipboardFormatAvailable(can_include) == 0 {
            return false;
        }

        // El formato está: hay que leer el DWORD, porque un `1` es permiso
        // explícito y tratarlo como negativa perdería ítems legítimos.
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            // Otro proceso lo tiene abierto. Ante la duda no se guarda: perder
            // un ítem se nota y se rehace; archivar una contraseña, no.
            return true;
        }
        let handle = GetClipboardData(can_include);
        let mut opt_out = true;
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u32;
            if !ptr.is_null() {
                opt_out = std::ptr::read_unaligned(ptr) == 0;
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        opt_out
    }
}

/// En macOS la señal equivalente son los tipos de la convención nspasteboard.org
/// que agrega quien copia (1Password, Bitwarden, KeePassXC):
///
/// - `org.nspasteboard.ConcealedType` — es una contraseña.
/// - `org.nspasteboard.TransientType` — es efímero, no lo archives.
///
/// `arboard` tampoco los mira acá.
#[cfg(target_os = "macos")]
pub fn clipboard_is_sensitive() -> bool {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSString};

    // AppKit tiene que estar cargado para que `class!(NSPasteboard)` resuelva.
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    const SENSITIVE_TYPES: [&str; 2] = [
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.TransientType",
    ];

    autoreleasepool(|_| {
        // SAFETY: mensajes a AppKit; `generalPasteboard` y `types` son +0 y
        // viven en este pool.
        unsafe {
            let pasteboard: *mut AnyObject =
                objc2::msg_send![objc2::class!(NSPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                return false;
            }
            let types: *mut AnyObject = objc2::msg_send![pasteboard, types];
            if types.is_null() {
                return false;
            }
            let types: &NSArray<NSString> = &*types.cast();
            types
                .iter()
                .any(|kind| SENSITIVE_TYPES.contains(&kind.to_string().as_str()))
        }
    })
}

/// En el resto de plataformas no hay una convención que consultar.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn clipboard_is_sensitive() -> bool {
    false
}
