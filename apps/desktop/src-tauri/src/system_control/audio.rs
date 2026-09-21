//! Volumen maestro y, en Windows, sesiones por app.

#[cfg(windows)]
use super::AudioSession;
use super::SystemAudio;

pub fn read() -> Result<SystemAudio, String> {
    imp::read()
}

pub fn set_master(volume: f32) -> Result<(), String> {
    imp::set_master(volume)
}

pub fn set_muted(muted: bool) -> Result<(), String> {
    imp::set_muted(muted)
}

pub fn set_session(id: &str, volume: f32) -> Result<(), String> {
    imp::set_session(id, volume)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioEndpointVolume, IAudioSessionControl, IAudioSessionControl2,
        IAudioSessionEnumerator, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    fn com_ok() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn endpoint() -> Result<IAudioEndpointVolume, String> {
        com_ok();
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("audio: {e}"))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("salida de audio: {e}"))?;
            device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .map_err(|e| format!("volumen: {e}"))
        }
    }

    pub fn read() -> Result<SystemAudio, String> {
        let vol = endpoint()?;
        let mut level = 0f32;
        let mut muted = windows::core::BOOL(0);
        unsafe {
            vol.GetMasterVolumeLevelScalar(&mut level)
                .map_err(|e| format!("leer volumen: {e}"))?;
            vol.GetMute(&mut muted)
                .map_err(|e| format!("leer silencio: {e}"))?;
        }
        let sessions = sessions().unwrap_or_default();
        Ok(SystemAudio {
            volume: level.clamp(0.0, 1.0),
            muted: muted.as_bool(),
            per_app: true,
            sessions,
        })
    }

    pub fn set_master(volume: f32) -> Result<(), String> {
        let vol = endpoint()?;
        unsafe {
            vol.SetMasterVolumeLevelScalar(volume, None)
                .map_err(|e| format!("volumen: {e}"))
        }
    }

    pub fn set_muted(muted: bool) -> Result<(), String> {
        let vol = endpoint()?;
        unsafe {
            vol.SetMute(windows::core::BOOL::from(muted), None)
                .map_err(|e| format!("silencio: {e}"))
        }
    }

    pub fn set_session(id: &str, volume: f32) -> Result<(), String> {
        let sessions = sessions()?;
        if !sessions.iter().any(|s| s.id == id) {
            return Err("sesión de audio no encontrada".into());
        }
        set_session_inner(id, volume)
    }

    fn set_session_inner(id: &str, volume: f32) -> Result<(), String> {
        com_ok();
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("audio: {e}"))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("salida: {e}"))?;
            let manager: IAudioSessionManager2 = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("sesiones: {e}"))?;
            let list: IAudioSessionEnumerator = manager
                .GetSessionEnumerator()
                .map_err(|e| format!("sesiones: {e}"))?;
            let count = list.GetCount().map_err(|e| format!("sesiones: {e}"))?;
            for i in 0..count {
                let control: IAudioSessionControl =
                    list.GetSession(i).map_err(|e| format!("sesión: {e}"))?;
                let control2: IAudioSessionControl2 =
                    control.cast().map_err(|e| format!("sesión: {e}"))?;
                let pid = control2.GetProcessId().unwrap_or(0);
                if pid.to_string() != id {
                    continue;
                }
                let simple: ISimpleAudioVolume = control
                    .cast()
                    .map_err(|e| format!("volumen de sesión: {e}"))?;
                return simple
                    .SetMasterVolume(volume, None)
                    .map_err(|e| format!("volumen de sesión: {e}"));
            }
        }
        Err("sesión de audio no encontrada".into())
    }

    fn sessions() -> Result<Vec<AudioSession>, String> {
        com_ok();
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("audio: {e}"))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("salida: {e}"))?;
            let _ = DEVICE_STATE_ACTIVE;
            let manager: IAudioSessionManager2 = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("sesiones: {e}"))?;
            let list: IAudioSessionEnumerator = manager
                .GetSessionEnumerator()
                .map_err(|e| format!("sesiones: {e}"))?;
            let count = list.GetCount().map_err(|e| format!("sesiones: {e}"))?;
            let mut out = Vec::new();
            for i in 0..count {
                let control: IAudioSessionControl = match list.GetSession(i) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let control2: IAudioSessionControl2 = match control.cast() {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let pid = control2.GetProcessId().unwrap_or(0);
                if pid == 0 {
                    continue;
                }
                let simple: ISimpleAudioVolume = match control.cast() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut level = 0f32;
                let mut muted = windows::core::BOOL(0);
                let _ = simple.GetMasterVolume(&mut level);
                let _ = simple.GetMute(&mut muted);
                let name = control
                    .GetDisplayName()
                    .ok()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("App {pid}"));
                out.push(AudioSession {
                    id: pid.to_string(),
                    name,
                    volume: level.clamp(0.0, 1.0),
                    muted: muted.as_bool(),
                });
            }
            Ok(out)
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::mem::size_of;

    const KAUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
    const KAUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = 0x644F7574; // 'dOut'
    const KAUDIO_DEVICE_PROPERTY_VOLUME_SCALAR: u32 = 0x766F6C6D; // 'volm'
    const KAUDIO_DEVICE_PROPERTY_MUTE: u32 = 0x6D757465; // 'mute'
    const KAUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: u32 = 0x6F757470; // 'outp'
    const KAUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const u8,
            data_size: *mut u32,
            data: *mut u8,
        ) -> i32;
        fn AudioObjectSetPropertyData(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const u8,
            data_size: u32,
            data: *const u8,
        ) -> i32;
    }

    fn default_output() -> Result<u32, String> {
        let addr = AudioObjectPropertyAddress {
            selector: KAUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
            scope: 0x676C6F62, // 'glob'
            element: KAUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut device: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                KAUDIO_OBJECT_SYSTEM_OBJECT,
                &addr,
                0,
                std::ptr::null(),
                &mut size,
                &mut device as *mut u32 as *mut u8,
            )
        };
        if status != 0 || device == 0 {
            return Err("sin salida de audio".into());
        }
        Ok(device)
    }

    fn volume_addr() -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector: KAUDIO_DEVICE_PROPERTY_VOLUME_SCALAR,
            scope: KAUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            element: KAUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        }
    }

    fn mute_addr() -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector: KAUDIO_DEVICE_PROPERTY_MUTE,
            scope: KAUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            element: KAUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        }
    }

    pub fn read() -> Result<SystemAudio, String> {
        let device = default_output()?;
        let addr = volume_addr();
        let mut volume = 0f32;
        let mut size = size_of::<f32>() as u32;
        let vol_ok = unsafe {
            AudioObjectGetPropertyData(
                device,
                &addr,
                0,
                std::ptr::null(),
                &mut size,
                &mut volume as *mut f32 as *mut u8,
            )
        };
        if vol_ok != 0 {
            return Err("no se pudo leer el volumen".into());
        }
        let mut muted_i = 0u32;
        size = size_of::<u32>() as u32;
        let mute_addr = mute_addr();
        let mute_ok = unsafe {
            AudioObjectGetPropertyData(
                device,
                &mute_addr,
                0,
                std::ptr::null(),
                &mut size,
                &mut muted_i as *mut u32 as *mut u8,
            )
        };
        Ok(SystemAudio {
            volume: volume.clamp(0.0, 1.0),
            muted: mute_ok == 0 && muted_i != 0,
            per_app: false,
            sessions: Vec::new(),
        })
    }

    pub fn set_master(volume: f32) -> Result<(), String> {
        let device = default_output()?;
        let addr = volume_addr();
        let mut value = volume;
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                &addr,
                0,
                std::ptr::null(),
                size_of::<f32>() as u32,
                &mut value as *mut f32 as *const u8,
            )
        };
        if status != 0 {
            return Err("no se pudo cambiar el volumen".into());
        }
        Ok(())
    }

    pub fn set_muted(muted: bool) -> Result<(), String> {
        let device = default_output()?;
        let addr = mute_addr();
        let mut value: u32 = if muted { 1 } else { 0 };
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                &addr,
                0,
                std::ptr::null(),
                size_of::<u32>() as u32,
                &mut value as *mut u32 as *const u8,
            )
        };
        if status != 0 {
            return Err("no se pudo cambiar el silencio".into());
        }
        Ok(())
    }

    pub fn set_session(_id: &str, _volume: f32) -> Result<(), String> {
        Err("en este sistema el volumen se controla para todo el equipo".into())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;
    pub fn read() -> Result<SystemAudio, String> {
        Err("no soportado en esta plataforma todavía".into())
    }
    pub fn set_master(_: f32) -> Result<(), String> {
        Err("no soportado".into())
    }
    pub fn set_muted(_: bool) -> Result<(), String> {
        Err("no soportado".into())
    }
    pub fn set_session(_: &str, _: f32) -> Result<(), String> {
        Err("no soportado".into())
    }
}
