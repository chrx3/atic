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

/// macOS: las apps normales abiertas (las del Dock), por la ruta de su
/// `.app` en minúsculas, como la anota el lanzador. Lo que se guarda por app
/// es su pid.
#[cfg(target_os = "macos")]
pub fn scan() -> Running {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSArray, NSString, NSURL};

    let mut running = Running::default();
    objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: Retained<NSArray<AnyObject>> = msg_send![&workspace, runningApplications];
        let own = std::process::id() as i32;
        for app in apps.iter() {
            // `NSApplicationActivationPolicyRegular`: las que tienen ventanas y Dock.
            let policy: isize = msg_send![&*app, activationPolicy];
            let pid: i32 = msg_send![&*app, processIdentifier];
            if policy != 0 || pid == own {
                continue;
            }
            let url: Option<Retained<NSURL>> = msg_send![&*app, bundleURL];
            let Some(path) = url.and_then(|url| url.path()).map(|p: Retained<NSString>| p.to_string().to_lowercase()) else {
                continue;
            };
            let active: objc2::runtime::Bool = msg_send![&*app, isActive];
            if active.as_bool() {
                running.foreground = Some(path.clone());
            }
            running.windows.entry(path).or_default().push(pid as isize);
        }
    });
    running
}

/// macOS: pide a cada app que termine, como Cmd+Q (la app pregunta si hay
/// algo sin guardar). Devuelve a cuántas se les pidió.
#[cfg(target_os = "macos")]
pub fn close(pids: &[isize]) -> usize {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    objc2::rc::autoreleasepool(|_| {
        pids.iter()
            .filter(|&&pid| unsafe {
                let app: Option<Retained<AnyObject>> =
                    msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid as i32];
                app.is_some_and(|app| {
                    let ok: objc2::runtime::Bool = msg_send![&app, terminate];
                    ok.as_bool()
                })
            })
            .count()
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn scan() -> Running {
    Running::default()
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn close(_: &[isize]) -> usize {
    0
}
