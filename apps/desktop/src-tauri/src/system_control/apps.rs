//! Cerrar (graceful) y forzar el cierre de una app del snapshot.

use super::can_control_stem;

pub fn close(id: &str) -> Result<u32, String> {
    if !can_control_stem(id) {
        return Err("esa app no se puede cerrar desde acá".into());
    }
    let n = imp::close(id);
    Ok(n as u32)
}

pub fn force(id: &str) -> Result<(), String> {
    if !can_control_stem(id) {
        return Err("esa app no se puede forzar desde acá".into());
    }
    imp::force(id)
}

/// Trae la app al frente sin tocar su estado.
pub fn focus(id: &str) -> Result<(), String> {
    imp::focus(id)
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    fn stem_of_pid(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
        }
    }

    fn pids_for_stem(stem: &str) -> Vec<u32> {
        let want = stem.trim_end_matches(".exe").to_ascii_lowercase();
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        unsafe {
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let pid = entry.th32ProcessID;
                    if pid > 0 {
                        if let Some(s) = stem_of_pid(pid) {
                            if s == want {
                                out.push(pid);
                            }
                        }
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        out
    }

    struct CloseState {
        pids: Vec<u32>,
        closed: usize,
    }

    pub fn close(id: &str) -> usize {
        let pids = pids_for_stem(id);
        if pids.is_empty() {
            return 0;
        }
        let mut state = CloseState { pids, closed: 0 };
        unsafe {
            EnumWindows(Some(close_cb), &mut state as *mut CloseState as LPARAM);
        }
        state.closed
    }

    struct FocusState {
        pids: Vec<u32>,
        hwnd: HWND,
    }

    /// Trae al frente la primera ventana visible de esa app.
    ///
    /// El traído al frente lo hace `clipboard_history::force_foreground`, que
    /// ya paga el precio de Windows (`AllowSetForegroundWindow` +
    /// `AttachThreadInput`): desde un overlay sin foco, `SetForegroundWindow`
    /// solo no alcanza.
    pub fn focus(id: &str) -> Result<(), String> {
        let pids = pids_for_stem(id);
        if pids.is_empty() {
            return Err("esa app ya no está abierta".into());
        }
        let mut state = FocusState {
            pids,
            hwnd: std::ptr::null_mut(),
        };
        unsafe {
            EnumWindows(Some(focus_cb), &mut state as *mut FocusState as LPARAM);
        }
        if state.hwnd.is_null() {
            return Err("esa app no tiene ventanas".into());
        }
        crate::clipboard_history::force_foreground(state.hwnd);
        Ok(())
    }

    unsafe extern "system" fn focus_cb(hwnd: HWND, data: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let state = &mut *(data as *mut FocusState);
        if !state.hwnd.is_null() {
            return TRUE;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if state.pids.contains(&pid) {
            state.hwnd = hwnd;
        }
        TRUE
    }

    unsafe extern "system" fn close_cb(hwnd: HWND, data: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let state = &mut *(data as *mut CloseState);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if state.pids.contains(&pid) && PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0 {
            state.closed += 1;
        }
        TRUE
    }

    pub fn force(id: &str) -> Result<(), String> {
        let self_pid = std::process::id();
        let pids = pids_for_stem(id);
        if pids.is_empty() {
            return Err("esa app ya no está abierta".into());
        }
        let mut killed = 0usize;
        for pid in pids {
            if pid == self_pid {
                continue;
            }
            unsafe {
                let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
                if handle.is_null() {
                    continue;
                }
                if TerminateProcess(handle, 1) != 0 {
                    killed += 1;
                }
                CloseHandle(handle);
            }
        }
        if killed == 0 {
            return Err("no se pudo forzar el cierre".into());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyObject, Bool};
    use objc2_foundation::{NSArray, NSString};
    use std::path::Path;

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    const ACTIVATION_POLICY_REGULAR: isize = 0;

    fn matches_stem(app: &AnyObject, stem: &str) -> bool {
        let want = stem.to_ascii_lowercase();
        unsafe {
            let bundle: *mut AnyObject = objc2::msg_send![app, bundleURL];
            if bundle.is_null() {
                return false;
            }
            let path_ns: *mut AnyObject = objc2::msg_send![bundle, path];
            if path_ns.is_null() {
                return false;
            }
            let path: &NSString = &*path_ns.cast();
            Path::new(&path.to_string())
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case(&want))
                .unwrap_or(false)
        }
    }

    fn each_matching(stem: &str, mut visit: impl FnMut(*mut AnyObject)) {
        autoreleasepool(|_| unsafe {
            let workspace: *mut AnyObject =
                objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return;
            }
            let apps: *mut AnyObject = objc2::msg_send![workspace, runningApplications];
            if apps.is_null() {
                return;
            }
            let apps: &NSArray<AnyObject> = &*apps.cast();
            let self_pid = std::process::id() as i32;
            for app in apps.iter() {
                let policy: isize = objc2::msg_send![&*app, activationPolicy];
                if policy != ACTIVATION_POLICY_REGULAR {
                    continue;
                }
                let pid: i32 = objc2::msg_send![&*app, processIdentifier];
                if pid <= 0 || pid == self_pid {
                    continue;
                }
                if matches_stem(&*app, stem) {
                    visit(&*app as *const AnyObject as *mut AnyObject);
                }
            }
        });
    }

    use super::super::snapshot;

    const SIGTERM: i32 = 15;
    const SIGKILL: i32 = 9;

    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    /// Señal a los pids que el último barrido vio bajo esa clave.
    ///
    /// Es el camino de lo que NO es una app: `node`, `cargo`, un helper
    /// suelto. No hay `NSRunningApplication` a quien pedirle nada, solo un
    /// pid — y un pid se recicla, así que antes de firmar se vuelve a leer la
    /// ruta y se comprueba que siga siendo el mismo programa.
    fn signal_background(id: &str, sig: i32) -> usize {
        let mut hechos = 0usize;
        for pid in snapshot::pids_for(id) {
            if snapshot::key_of_pid(pid).as_deref() != Some(id) {
                continue;
            }
            if unsafe { kill(pid as i32, sig) } == 0 {
                hechos += 1;
            }
        }
        hechos
    }

    pub fn close(id: &str) -> usize {
        let mut asked = 0usize;
        each_matching(id, |app| unsafe {
            let _: Bool = objc2::msg_send![app, terminate];
            asked += 1;
        });
        if asked == 0 {
            asked = signal_background(id, SIGTERM);
        }
        asked
    }

    pub fn force(id: &str) -> Result<(), String> {
        let mut asked = 0usize;
        each_matching(id, |app| unsafe {
            let _: Bool = objc2::msg_send![app, forceTerminate];
            asked += 1;
        });
        if asked == 0 {
            asked = signal_background(id, SIGKILL);
        }
        if asked == 0 {
            return Err("ese proceso ya no está".into());
        }
        Ok(())
    }

    /// `activateWithOptions:` con `ActivateAllWindows` (1 << 0) e
    /// `IgnoringOtherApps` (1 << 1): traer TODAS sus ventanas, aunque el foco
    /// lo tenga otra app. Sin la segunda, activar desde un overlay sin foco no
    /// hace nada visible.
    pub fn focus(id: &str) -> Result<(), String> {
        const ACTIVATE_ALL_WINDOWS: usize = 1 << 0;
        const ACTIVATE_IGNORING_OTHER_APPS: usize = 1 << 1;
        let mut hechos = 0usize;
        each_matching(id, |app| unsafe {
            let opciones = ACTIVATE_ALL_WINDOWS | ACTIVATE_IGNORING_OTHER_APPS;
            let _: Bool = objc2::msg_send![app, activateWithOptions: opciones];
            hechos += 1;
        });
        if hechos == 0 {
            return Err("esa app no tiene ventanas".into());
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn close(_: &str) -> usize {
        0
    }
    pub fn force(_: &str) -> Result<(), String> {
        Err("no soportado".into())
    }
    pub fn focus(_: &str) -> Result<(), String> {
        Err("no soportado".into())
    }
}
