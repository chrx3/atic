//! Café: mantener el equipo despierto mientras dura algo largo.
//!
//! No se persiste a propósito. Un "no te duermas" que sobrevive a un reinicio
//! es una batería vacía al día siguiente sin que nadie sepa por qué; esto vale
//! para esta sesión y se apaga solo al cerrar Atic.

use std::sync::Mutex;

static ON: Mutex<bool> = Mutex::new(false);

pub fn is_on() -> bool {
    ON.lock().map(|g| *g).unwrap_or(false)
}

pub fn set(on: bool) -> Result<(), String> {
    let mut guard = ON
        .lock()
        .map_err(|_| "café: estado bloqueado".to_string())?;
    if *guard == on {
        return Ok(());
    }
    imp::set(on)?;
    *guard = on;
    Ok(())
}

/// Al cerrar la app: soltar la retención pase lo que pase.
pub fn release() {
    let _ = imp::set(false);
    if let Ok(mut guard) = ON.lock() {
        *guard = false;
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    /// `caffeinate` en vez de `IOPMAssertionCreateWithName`.
    ///
    /// Es el binario del sistema que hace exactamente esto, y la retención
    /// muere con el proceso: si Atic se cae, el equipo vuelve a dormirse solo.
    /// Una aserción de IOKit mal soltada se queda pegada hasta reiniciar.
    static HIJO: Mutex<Option<Child>> = Mutex::new(None);

    pub fn set(on: bool) -> Result<(), String> {
        let mut guard = HIJO
            .lock()
            .map_err(|_| "café: proceso bloqueado".to_string())?;
        if let Some(mut hijo) = guard.take() {
            let _ = hijo.kill();
            let _ = hijo.wait();
        }
        if !on {
            return Ok(());
        }
        // -d pantalla, -i sistema, -m disco, -s mientras haya corriente.
        let hijo = Command::new("/usr/bin/caffeinate")
            .args(["-dims"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("café: {e}"))?;
        *guard = Some(hijo);
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };

    /// `ES_CONTINUOUS` vale para el HILO que lo pide, y este comando corre en
    /// el hilo del runtime de Tauri, que vive tanto como la app: alcanza con
    /// llamarlo acá y devolverlo a `ES_CONTINUOUS` solo al apagar.
    pub fn set(on: bool) -> Result<(), String> {
        let flags = if on {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        let prev = unsafe { SetThreadExecutionState(flags) };
        if prev == 0 {
            return Err("no se pudo cambiar el estado de energía".into());
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn set(_on: bool) -> Result<(), String> {
        Err("no soportado en esta plataforma todavía".into())
    }
}
