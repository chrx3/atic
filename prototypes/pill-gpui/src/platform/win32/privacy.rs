//! Quién usa el micrófono o la cámara ahora, como el punto de macOS.
//!
//! Windows lo anota en el registro (`CapabilityAccessManager\ConsentStore`):
//! cada app tiene la hora de inicio y de fin de su último uso, y mientras lo
//! usa el fin vale 0. Un hilo lo revisa cada segundo; la pill solo mira el
//! resultado.

use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Microphone,
    Camera,
}

/// Una app usando un dispositivo ahora.
#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    pub device: Device,
    pub app: String,
}

#[derive(Clone, Default)]
pub struct Privacy {
    uses: Arc<Mutex<Vec<Use>>>,
}

impl Privacy {
    pub fn start() -> Self {
        let privacy = Self::default();
        let shared = privacy.uses.clone();
        std::thread::Builder::new()
            .name("privacidad".into())
            .spawn(move || loop {
                let now = imp::scan();
                if let Ok(mut slot) = shared.lock() {
                    *slot = now;
                }
                std::thread::sleep(Duration::from_secs(1));
            })
            .expect("hilo de privacidad");
        privacy
    }

    pub fn uses(&self) -> Vec<Use> {
        self.uses.lock().map(|u| u.clone()).unwrap_or_default()
    }

}

/// El nombre legible de una entrada del registro: una ruta con `#` en vez de
/// `\` (`C:#Program Files#Zoom#bin#Zoom.exe`) o un paquete de la Store
/// (`Microsoft.WindowsCamera_8wekyb3d8bbwe`).
pub fn app_name(key: &str) -> String {
    if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        let stem = file.trim_end_matches(".exe").trim_end_matches(".EXE");
        let mut chars = stem.chars();
        return match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => key.to_string(),
        };
    }
    let package = key.split('_').next().unwrap_or(key);
    let name = package.rsplit('.').next().unwrap_or(package);
    // `WindowsCamera` → `Windows Camera`.
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_uppercase() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_READ,
    };

    const BASE: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore";

    pub fn scan() -> Vec<Use> {
        let mut out = Vec::new();
        for (device, folder) in [(Device::Microphone, "microphone"), (Device::Camera, "webcam")] {
            let path = format!("{BASE}\\{folder}");
            for (key, in_use) in children(&path).into_iter().chain(children(&format!("{path}\\NonPackaged"))) {
                if in_use && key != "NonPackaged" {
                    out.push(Use {
                        device,
                        app: app_name(&key),
                    });
                }
            }
        }
        out.dedup();
        out
    }

    /// Las subclaves de `path` y si cada una está en uso (inicio ≠ 0 y fin = 0).
    fn children(path: &str) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        unsafe {
            let mut parent = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), Some(0), KEY_READ, &mut parent)
                .is_err()
            {
                return out;
            }
            let mut index = 0;
            loop {
                let mut name = [0u16; 512];
                let mut len = name.len() as u32;
                if RegEnumKeyExW(
                    parent,
                    index,
                    Some(windows::core::PWSTR(name.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
                .is_err()
                {
                    break;
                }
                index += 1;
                let key = String::from_utf16_lossy(&name[..len as usize]);
                let mut child = HKEY::default();
                if RegOpenKeyExW(parent, PCWSTR(name.as_ptr()), Some(0), KEY_READ, &mut child).is_ok() {
                    let start = qword(child, "LastUsedTimeStart");
                    let stop = qword(child, "LastUsedTimeStop");
                    out.push((key, start.is_some_and(|s| s != 0) && stop == Some(0)));
                    let _ = RegCloseKey(child);
                }
            }
            let _ = RegCloseKey(parent);
        }
        out
    }

    fn qword(key: HKEY, name: &str) -> Option<u64> {
        let mut value = 0u64;
        let mut size = std::mem::size_of::<u64>() as u32;
        unsafe {
            RegQueryValueExW(
                key,
                &HSTRING::from(name),
                None,
                None,
                Some(&mut value as *mut u64 as *mut u8),
                Some(&mut size),
            )
            .ok()
            .ok()?;
        }
        Some(value)
    }
}

/// macOS: el micrófono por Core Audio, con las apps que graban (macOS 14.2+,
/// por proceso), y la cámara por Core Media IO, que solo dice si alguien la
/// usa. La pill misma no cuenta: su dictado ya se ve en el notch.
#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::platform::coreaudio as ca;

    pub fn scan() -> Vec<Use> {
        let own = std::process::id() as i32;
        let mut out = Vec::new();
        let mic_busy = ca::default_device(true)
            .and_then(|device| ca::get::<u32>(device, ca::Address::new(ca::RUNNING_SOMEWHERE, ca::SCOPE_GLOBAL)))
            .is_some_and(|v| v != 0);
        if mic_busy {
            let processes = ca::processes();
            let mine = processes.iter().any(|p| p.input && p.pid == own);
            let mut apps: Vec<String> = processes
                .iter()
                .filter(|p| p.input && p.pid != own)
                .map(|p| crate::platform::running_name(p.pid, p.bundle.as_deref()))
                .collect();
            // Sin la lista por proceso (antes de macOS 14.2) se sabe que está
            // en uso, no por quién.
            if apps.is_empty() && !mine {
                apps.push(String::new());
            }
            for app in apps {
                out.push(Use { device: Device::Microphone, app });
            }
        }
        if camera_busy() {
            out.push(Use { device: Device::Camera, app: String::new() });
        }
        out.dedup();
        out
    }

    fn camera_busy() -> bool {
        #[repr(C)]
        struct Address {
            selector: u32,
            scope: u32,
            element: u32,
        }
        #[link(name = "CoreMediaIO", kind = "framework")]
        extern "C" {
            fn CMIOObjectGetPropertyDataSize(object: u32, address: *const Address, qs: u32, q: *const std::ffi::c_void, size: *mut u32) -> i32;
            fn CMIOObjectGetPropertyData(
                object: u32,
                address: *const Address,
                qs: u32,
                q: *const std::ffi::c_void,
                size: u32,
                used: *mut u32,
                data: *mut std::ffi::c_void,
            ) -> i32;
        }
        let four = |c: &[u8; 4]| u32::from_be_bytes(*c);
        let devices = Address { selector: four(b"dev#"), scope: four(b"glob"), element: 0 };
        // SAFETY: tamaños anunciados y búferes de ese tamaño.
        unsafe {
            let mut size = 0u32;
            if CMIOObjectGetPropertyDataSize(1, &devices, 0, std::ptr::null(), &mut size) != 0 || size == 0 {
                return false;
            }
            let mut ids = vec![0u32; size as usize / 4];
            let mut used = 0u32;
            if CMIOObjectGetPropertyData(1, &devices, 0, std::ptr::null(), size, &mut used, ids.as_mut_ptr().cast()) != 0 {
                return false;
            }
            ids.truncate(used as usize / 4);
            let running = Address { selector: four(b"gone"), scope: four(b"glob"), element: 0 };
            ids.into_iter().any(|id| {
                let mut on = 0u32;
                let mut used = 0u32;
                CMIOObjectGetPropertyData(id, &running, 0, std::ptr::null(), 4, &mut used, (&mut on as *mut u32).cast()) == 0
                    && on != 0
            })
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;
    pub fn scan() -> Vec<Use> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_legibles() {
        assert_eq!(app_name("C:#Program Files#Zoom#bin#Zoom.exe"), "Zoom");
        assert_eq!(app_name("C:#Users#x#AppData#Local#Discord#app-1.0#Discord.exe"), "Discord");
        assert_eq!(app_name("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "Windows Camera");
        assert_eq!(app_name("OpenAI.Codex_2p2nqsd0c76g0"), "Codex");
    }
}
