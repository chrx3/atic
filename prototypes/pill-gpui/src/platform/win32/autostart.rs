//! Iniciar con Windows (`autostart` de `config.json`), como Atic: el valor
//! `Atic` de `HKCU\...\CurrentVersion\Run`. Es el mismo nombre que usa la app
//! de Tauri, así que apuntarlo al exe de la pill reemplaza su entrada y nunca
//! quedan las dos.
//!
//! Habilitar también limpia `StartupApproved\Run`: si alguien lo desactivó
//! desde el Administrador de tareas, Windows lo ignora aunque esté en `Run`.

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_BINARY, REG_SZ,
    RRF_RT_REG_SZ,
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const VALUE: &str = "Atic";
/// «Habilitado» en `StartupApproved` (lo que escribe Windows al activarlo).
const APPROVED_ON: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// La línea de comando con que arranca: el exe actual, entre comillas.
fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

/// Deja el registro como pide `enabled`, solo si hace falta.
pub fn sync(enabled: bool) {
    let Some(command) = command() else {
        return;
    };
    let result = sync_at(RUN, APPROVED, &command, enabled);
    if let Err(code) = result {
        tracing::warn!(code, enabled, "no se pudo actualizar el inicio con Windows");
    }
}

fn sync_at(run: &str, approved: &str, command: &str, enabled: bool) -> Result<(), u32> {
    let current = read(run)?;
    if enabled {
        if current.as_deref() != Some(command) {
            write_sz(run, command)?;
            tracing::info!("inicio con Windows: apunta a la pill");
        }
        write_binary(approved, &APPROVED_ON)
    } else if current.is_some() {
        delete(run)
    } else {
        Ok(())
    }
}

fn read(key: &str) -> Result<Option<String>, u32> {
    let (key, value) = (wide(key), wide(VALUE));
    let mut buf = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: búferes válidos durante la llamada; `size` en bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    match status {
        ERROR_SUCCESS => {
            let len = (size as usize / 2).saturating_sub(1);
            Ok(Some(String::from_utf16_lossy(&buf[..len])))
        }
        ERROR_FILE_NOT_FOUND => Ok(None),
        code => Err(code),
    }
}

fn write_sz(key: &str, text: &str) -> Result<(), u32> {
    let data = wide(text);
    set(key, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
}

fn write_binary(key: &str, data: &[u8]) -> Result<(), u32> {
    set(key, REG_BINARY, data.as_ptr().cast(), data.len() as u32)
}

fn set(key: &str, kind: u32, data: *const std::ffi::c_void, len: u32) -> Result<(), u32> {
    let (key, value) = (wide(key), wide(VALUE));
    // SAFETY: `data` apunta a `len` bytes válidos; crea la clave si falta.
    let status =
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), kind, data, len) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(status)
    }
}

fn delete(key: &str) -> Result<(), u32> {
    let (key, value) = (wide(key), wide(VALUE));
    // SAFETY: cadenas terminadas en 0.
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        code => Err(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enciende_apaga_y_no_reescribe_si_ya_esta() {
        // Una clave propia de prueba: nunca la `Run` de verdad.
        let run = format!(r"Software\atic-pill-test-{}\Run", std::process::id());
        let approved = format!(r"Software\atic-pill-test-{}\Approved", std::process::id());
        let command = "\"C:\\Atic\\pill-gpui.exe\"";

        sync_at(&run, &approved, command, true).unwrap();
        assert_eq!(read(&run).unwrap().as_deref(), Some(command));
        sync_at(&run, &approved, command, true).unwrap();
        sync_at(&run, &approved, command, false).unwrap();
        assert_eq!(read(&run).unwrap(), None);
        sync_at(&run, &approved, command, false).unwrap();

        let root = wide(&format!(r"Software\atic-pill-test-{}", std::process::id()));
        // SAFETY: borra solo la clave de prueba recién creada.
        unsafe { windows_sys::Win32::System::Registry::RegDeleteTreeW(HKEY_CURRENT_USER, root.as_ptr()) };
    }
}
