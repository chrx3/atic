//! Detectar una llamada en curso (Zoom, Teams, Meet, Webex) por el título de
//! sus ventanas, como `meeting_detection.rs` de Atic: local, sin red, y solo si
//! está activado en Ajustes (`detect_meetings`). Mira cada 5 s en un hilo
//! propio y deja lo último que vio para quien lo pida.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::data::Paths;

/// Cada cuánto se miran las ventanas.
const EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub provider: String,
    pub title: String,
}

/// Lo último que vio el detector; `None` sin llamada o con la detección
/// apagada.
pub type Seen = Arc<Mutex<Option<Call>>>;

/// Arranca el hilo que mira las ventanas. Sin datos de Atic no hay
/// configuración que lo active: no arranca.
pub fn spawn(paths: Option<Paths>) -> Seen {
    let seen: Seen = Arc::default();
    let Some(paths) = paths else {
        return seen;
    };
    let out = seen.clone();
    let started = std::thread::Builder::new()
        .name("meetings-detect".into())
        .spawn(move || loop {
            let enabled = atic_core::Config::load(&paths.config_path()).detect_meetings;
            let call = if enabled { detect() } else { None };
            if let Ok(mut slot) = out.lock() {
                *slot = call;
            }
            std::thread::sleep(EVERY);
        });
    if let Err(error) = started {
        eprintln!("reuniones: no se pudo iniciar el detector de llamadas: {error}");
    }
    seen
}

fn detect() -> Option<Call> {
    windows::visible_windows()
        .into_iter()
        .find_map(|window| classify_window(&window.process, &window.title))
}

/// Las mismas reglas que Atic: el proceso y palabras del título.
fn classify_window(process: &str, title: &str) -> Option<Call> {
    let process = process.to_ascii_lowercase();
    let normalized = title.to_lowercase();
    let provider = if (process == "zoom.exe" || process == "zoom")
        && (normalized.contains("zoom meeting")
            || normalized.contains("reunión de zoom")
            || normalized.contains("zoom reunión"))
    {
        "Zoom"
    } else if (process.contains("teams")
        && ["meeting", "reunión", "llamada", "call"]
            .iter()
            .any(|word| normalized.contains(word)))
        || normalized.contains("microsoft teams meeting")
    {
        "Teams"
    } else if normalized.contains("google meet") || normalized.contains("meet.google.com") {
        "Meet"
    } else if process.contains("webex")
        && ["meeting", "reunión", "webinar"]
            .iter()
            .any(|word| normalized.contains(word))
    {
        "Webex"
    } else {
        return None;
    };
    Some(Call {
        provider: provider.into(),
        title: title.chars().take(180).collect(),
    })
}

mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible,
    };

    pub struct WindowInfo {
        pub process: String,
        pub title: String,
    }

    pub fn visible_windows() -> Vec<WindowInfo> {
        let mut windows = Vec::new();
        // SAFETY: el puntero vive durante EnumWindows y el callback solo lo
        // usa ahí.
        unsafe {
            let _ = EnumWindows(Some(collect_window), &mut windows as *mut Vec<WindowInfo> as LPARAM);
        }
        windows
    }

    unsafe extern "system" fn collect_window(hwnd: HWND, parameter: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return 1;
            }
            let length = GetWindowTextLengthW(hwnd);
            if length < 3 {
                return 1;
            }
            let mut title = vec![0u16; length as usize + 1];
            let copied = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            if copied <= 0 {
                return 1;
            }
            title.truncate(copied as usize);
            let title = String::from_utf16_lossy(&title);
            let process = process_name(hwnd).unwrap_or_default();
            let windows = &mut *(parameter as *mut Vec<WindowInfo>);
            windows.push(WindowInfo { process, title });
            1
        }
    }

    unsafe fn process_name(hwnd: HWND) -> Option<String> {
        unsafe {
            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, &mut process_id);
            if process_id == 0 {
                return None;
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
            if handle.is_null() {
                return None;
            }
            let mut buffer = vec![0u16; 1024];
            let mut length = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            if ok == 0 || length == 0 {
                return None;
            }
            buffer.truncate(length as usize);
            let path = String::from_utf16_lossy(&buffer);
            Some(path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconoce_las_ventanas_de_llamada() {
        assert_eq!(
            classify_window("chrome.exe", "Planificación semanal - Google Meet").unwrap().provider,
            "Meet"
        );
        assert!(classify_window("Zoom.exe", "Zoom Workplace").is_none());
        assert!(classify_window("ms-teams.exe", "Chat | Microsoft Teams").is_none());
        assert_eq!(
            classify_window("ms-teams.exe", "Reunión semanal | Microsoft Teams").unwrap().provider,
            "Teams"
        );
    }
}
