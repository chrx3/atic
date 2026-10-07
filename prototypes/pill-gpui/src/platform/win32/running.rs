//! Qué apps tienen ventanas abiertas, para marcar «En uso» en el lanzador y
//! cerrarlas con Ctrl+Enter. Como Atic (`launcher_quit`): se pide cerrar con
//! `WM_CLOSE`, igual que el aspa de la ventana; nunca se mata un proceso.

use std::collections::HashMap;

/// Ventanas de usuario por ejecutable (ruta en minúsculas).
#[derive(Default)]
pub struct Running {
    pub windows: HashMap<String, Vec<isize>>,
    pub foreground: Option<String>,
}

impl Running {
    pub fn is_running(&self, exe: &str) -> bool {
        self.windows.contains_key(exe)
    }

    pub fn is_foreground(&self, exe: &str) -> bool {
        self.foreground.as_deref() == Some(exe)
    }
}

#[cfg(windows)]
pub fn scan() -> Running {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
        IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        let found = &mut *(data.0 as *mut Vec<HWND>);
        let user_window = IsWindowVisible(hwnd).as_bool()
            && GetWindow(hwnd, GW_OWNER).map_or(true, |owner| owner.0.is_null())
            && GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize == 0
            && GetWindowTextLengthW(hwnd) > 0;
        // Las ventanas «ocultas» de otros escritorios virtuales o suspendidas.
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        if user_window && cloaked == 0 {
            found.push(hwnd);
        }
        BOOL(1)
    }

    let mut found: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    let mut running = Running::default();
    for hwnd in found {
        if let Some(exe) = exe_of(hwnd) {
            running.windows.entry(exe).or_default().push(hwnd.0 as isize);
        }
    }
    running.foreground = exe_of(unsafe { GetForegroundWindow() });
    running
}

#[cfg(windows)]
fn exe_of(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase())
    }
}

/// Pide cerrar las ventanas: la app decide (y pregunta si hay algo sin
/// guardar). Devuelve cuántas se pidió cerrar.
#[cfg(windows)]
pub fn close(windows: &[isize]) -> usize {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    windows
        .iter()
        .filter(|&&hwnd| unsafe {
            PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok()
        })
        .count()
}

#[cfg(not(windows))]
pub fn scan() -> Running {
    Running::default()
}

#[cfg(not(windows))]
pub fn close(_: &[isize]) -> usize {
    0
}
