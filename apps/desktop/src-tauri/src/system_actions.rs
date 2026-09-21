//! Acciones de sistema que el launcher expone como resultados propios.
//!
//! Regla del producto: **nunca forzar acciones destructivas**. Por eso vaciar la
//! papelera conserva el diálogo de confirmación de Windows (no se le pasa
//! `SHERB_NOCONFIRMATION`) y en macOS el panel de sistema confirma antes.
//! El force-quit de apps vive en el panel de sistema, con diálogo propio.

/// Bloquea la sesión (equivalente a Win+L).
pub fn lock_screen() -> Result<(), String> {
    imp::lock_screen()
}

/// Suspende el equipo. No hiberna.
pub fn sleep() -> Result<(), String> {
    imp::sleep()
}

/// Silencia o reactiva la salida de audio (misma tecla que el teclado).
pub fn toggle_mute() -> Result<(), String> {
    imp::toggle_mute()
}

/// Vacía la papelera. Windows pregunta antes: es irreversible.
pub fn empty_trash() -> Result<(), String> {
    imp::empty_trash()
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Power::SetSuspendState;
    use windows_sys::Win32::System::Shutdown::LockWorkStation;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_VOLUME_MUTE,
    };
    use windows_sys::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOPROGRESSUI, SHERB_NOSOUND};

    pub fn lock_screen() -> Result<(), String> {
        let ok = unsafe { LockWorkStation() };
        if ok == 0 {
            return Err("no se pudo bloquear la sesión".into());
        }
        Ok(())
    }

    pub fn sleep() -> Result<(), String> {
        // (hibernar, forzar, despertar por eventos): suspender, sin forzar.
        let ok = unsafe { SetSuspendState(0, 0, 0) };
        if ok == 0 {
            return Err("no se pudo suspender el equipo".into());
        }
        Ok(())
    }

    pub fn toggle_mute() -> Result<(), String> {
        let mut inputs: [INPUT; 2] = unsafe { std::mem::zeroed() };
        for (index, flags) in [0u32, KEYEVENTF_KEYUP].into_iter().enumerate() {
            inputs[index].r#type = INPUT_KEYBOARD;
            inputs[index].Anonymous.ki = KEYBDINPUT {
                wVk: VK_VOLUME_MUTE,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            };
        }
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent != inputs.len() as u32 {
            return Err("no se pudo enviar la tecla de silencio".into());
        }
        Ok(())
    }

    pub fn empty_trash() -> Result<(), String> {
        // Sin `SHERB_NOCONFIRMATION`: el usuario confirma en el diálogo del SO.
        let result = unsafe {
            SHEmptyRecycleBinW(
                std::ptr::null_mut(),
                std::ptr::null(),
                SHERB_NOPROGRESSUI | SHERB_NOSOUND,
            )
        };
        if result != 0 {
            return Err("no se pudo vaciar la papelera".into());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CString;
    use std::os::raw::c_void;
    use std::process::Command;

    const RTLD_LAZY: i32 = 1;

    extern "C" {
        fn dlopen(filename: *const i8, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
    }

    type LockFn = unsafe extern "C" fn();

    pub fn lock_screen() -> Result<(), String> {
        let path = CString::new(
            "/System/Library/PrivateFrameworks/login.framework/Versions/Current/login",
        )
        .map_err(|_| "lock".to_string())?;
        let sym = CString::new("SACLockScreenImmediate").map_err(|_| "lock".to_string())?;
        unsafe {
            let handle = dlopen(path.as_ptr(), RTLD_LAZY);
            if !handle.is_null() {
                let ptr = dlsym(handle, sym.as_ptr());
                if !ptr.is_null() {
                    let lock: LockFn = std::mem::transmute(ptr);
                    lock();
                    return Ok(());
                }
            }
        }
        let status = Command::new("pmset")
            .arg("displaysleepnow")
            .status()
            .map_err(|e| format!("bloquear: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("no se pudo bloquear la sesión".into())
        }
    }

    pub fn sleep() -> Result<(), String> {
        let status = Command::new("pmset")
            .arg("sleepnow")
            .status()
            .map_err(|e| format!("suspender: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("no se pudo suspender el equipo".into())
        }
    }

    pub fn toggle_mute() -> Result<(), String> {
        let audio = crate::system_control::system_audio()?;
        crate::system_control::system_set_muted(!audio.muted)
    }

    pub fn empty_trash() -> Result<(), String> {
        // Finder pregunta antes si el usuario no apagó esa advertencia.
        let status = Command::new("osascript")
            .args(["-e", "tell application \"Finder\" to empty the trash"])
            .status()
            .map_err(|e| format!("papelera: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("no se pudo vaciar la papelera".into())
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    const NO_SOPORTADO: &str = "no soportado en esta plataforma todavía";

    pub fn lock_screen() -> Result<(), String> {
        Err(NO_SOPORTADO.into())
    }

    pub fn sleep() -> Result<(), String> {
        Err(NO_SOPORTADO.into())
    }

    pub fn toggle_mute() -> Result<(), String> {
        Err(NO_SOPORTADO.into())
    }

    pub fn empty_trash() -> Result<(), String> {
        Err(NO_SOPORTADO.into())
    }
}
