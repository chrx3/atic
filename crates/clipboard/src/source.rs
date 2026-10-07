/// Quién copió lo que hay en el portapapeles.
///
/// `GetClipboardOwner` es la ventana que lo escribió, aunque ya no esté al
/// frente. Algunas apps lo escriben sin ventana; para esas, la activa es la
/// mejor pista.
#[cfg(windows)]
pub fn clipboard_source_app() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::GetClipboardOwner;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let owner = unsafe { GetClipboardOwner() };
    let hwnd = if owner.is_null() {
        unsafe { GetForegroundWindow() }
    } else {
        owner
    };
    if hwnd.is_null() {
        return None;
    }
    process_exe_path(hwnd)
}

#[cfg(not(windows))]
pub fn clipboard_source_app() -> Option<String> {
    None
}

/// Ruta completa del ejecutable dueño de una ventana.
#[cfg(windows)]
pub fn process_exe_path(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut path_buf = [0u16; 1024];
        let mut path_len = path_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, path_buf.as_mut_ptr(), &mut path_len);
        CloseHandle(handle);
        if ok == 0 || path_len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&path_buf[..path_len as usize]))
    }
}
