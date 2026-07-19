//! Detección local y opt-in de ventanas de reuniones.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MeetingInfo {
    provider: String,
    title: String,
}

#[derive(Clone, Serialize)]
struct MeetingDetectionPayload {
    active: bool,
    provider: Option<String>,
    title: Option<String>,
}

pub fn spawn_detector(app: AppHandle) {
    let result = std::thread::Builder::new()
        .name("meeting-detector".into())
        .spawn(move || {
            let mut previous: Option<MeetingInfo> = None;
            loop {
                let enabled = app
                    .try_state::<AppState>()
                    .is_some_and(|state| state.config.lock().unwrap().detect_meetings);
                let current = if enabled { detect_meeting() } else { None };
                if current != previous {
                    let payload = match &current {
                        Some(meeting) => MeetingDetectionPayload {
                            active: true,
                            provider: Some(meeting.provider.clone()),
                            title: Some(meeting.title.clone()),
                        },
                        None => MeetingDetectionPayload {
                            active: false,
                            provider: None,
                            title: None,
                        },
                    };
                    let _ = app.emit("meeting-detection", payload);
                    previous = current;
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
    if let Err(error) = result {
        tracing::warn!(%error, "no se pudo iniciar detector de reuniones");
    }
}

#[cfg(windows)]
fn detect_meeting() -> Option<MeetingInfo> {
    windows::visible_windows()
        .into_iter()
        .find_map(|window| classify_window(&window.process, &window.title))
}

#[cfg(not(windows))]
fn detect_meeting() -> Option<MeetingInfo> {
    None
}

fn classify_window(process: &str, title: &str) -> Option<MeetingInfo> {
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
        "Microsoft Teams"
    } else if normalized.contains("google meet") || normalized.contains("meet.google.com") {
        "Google Meet"
    } else if process.contains("webex")
        && ["meeting", "reunión", "webinar"]
            .iter()
            .any(|word| normalized.contains(word))
    {
        "Webex"
    } else {
        return None;
    };
    Some(MeetingInfo {
        provider: provider.into(),
        title: title.chars().take(180).collect(),
    })
}

#[cfg(windows)]
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
        // SAFETY: el puntero vive durante EnumWindows y callback solo lo usa allí.
        unsafe {
            let _ = EnumWindows(
                Some(collect_window),
                &mut windows as *mut Vec<WindowInfo> as LPARAM,
            );
        }
        windows
    }

    unsafe extern "system" fn collect_window(hwnd: HWND, parameter: LPARAM) -> BOOL {
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

    unsafe fn process_name(hwnd: HWND) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_meeting_windows() {
        assert_eq!(
            classify_window("chrome.exe", "Planificación semanal - Google Meet")
                .unwrap()
                .provider,
            "Google Meet"
        );
        assert!(classify_window("Zoom.exe", "Zoom Workplace").is_none());
        assert!(classify_window("ms-teams.exe", "Chat | Microsoft Teams").is_none());
        assert_eq!(
            classify_window("ms-teams.exe", "Reunión semanal | Microsoft Teams")
                .unwrap()
                .provider,
            "Microsoft Teams"
        );
    }
}
