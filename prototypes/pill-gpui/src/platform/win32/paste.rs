//! Pegar en otra app como lo hace Atic (`clipboard_history.rs`): dar el foco a
//! la ventana destino, elegir Ctrl+V o Ctrl+Shift+V según la app y, al soltar
//! un arrastre que el destino no aceptó, pegar como respaldo.

use windows_sys::Win32::Foundation::{CloseHandle, HWND, POINT};
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, GetAncestor, GetCursorPos, GetForegroundWindow,
    GetWindowThreadProcessId, IsWindow, SetForegroundWindow, WindowFromPoint, ASFW_ANY, GA_ROOT,
};

/// Ventana destino guardada como entero para poder cruzar tareas async.
pub type Target = isize;

/// La ventana con el foco, si no es de este proceso.
pub fn foreground_target() -> Option<Target> {
    let hwnd = unsafe { GetForegroundWindow() };
    usable(hwnd).then_some(hwnd as Target)
}

/// La ventana raíz bajo el cursor, si no es de este proceso.
pub fn target_under_cursor() -> Option<Target> {
    let hwnd = unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let under = WindowFromPoint(pt);
        if under.is_null() {
            return None;
        }
        // La raíz, no el control bajo el puntero: el foco y el atajo se deciden
        // sobre la ventana de nivel superior.
        GetAncestor(under, GA_ROOT)
    };
    usable(hwnd).then_some(hwnd as Target)
}

fn usable(hwnd: HWND) -> bool {
    !hwnd.is_null() && unsafe { IsWindow(hwnd) } != 0 && !is_own(hwnd)
}

fn is_own(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid != 0 && pid == unsafe { GetCurrentProcessId() }
}

fn process_exe_name(hwnd: HWND) -> Option<String> {
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
        let path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
        Some(
            path.rsplit(['\\', '/'])
                .next()
                .unwrap_or(&path)
                .to_ascii_lowercase(),
        )
    }
}

/// Terminales Electron/WebView2 donde Ctrl+V se lo come Chromium y el pegado
/// del xterm es Ctrl+Shift+V. Las consolas nativas aceptan Ctrl+V.
pub fn needs_ctrl_shift_v(target: Target) -> bool {
    process_exe_name(target as HWND).is_some_and(|exe| {
        matches!(
            exe.as_str(),
            "terax.exe" | "hyper.exe" | "tabby.exe" | "terminus.exe" | "electerm.exe"
        )
    })
}

/// ¿Soltar texto necesita el pegado de respaldo?
///
/// Sí si el destino lo rechazó (efecto 0), como las consolas que solo aceptan
/// archivos, y también en terminales web: Chromium dice que lo copió pero el
/// xterm de adentro no lo inserta.
pub fn drop_needs_paste_fallback(dropped: bool, effect: u32, web_terminal: bool) -> bool {
    dropped && (effect == 0 || web_terminal)
}

/// Lleva la ventana al frente aunque Windows limite quién puede robar el foco.
pub fn force_foreground(target: Target) {
    let hwnd = target as HWND;
    unsafe {
        if IsWindow(hwnd) == 0 {
            return;
        }
        let mut target_pid = 0u32;
        let target_tid = GetWindowThreadProcessId(hwnd, &mut target_pid);
        if target_pid != 0 {
            let _ = AllowSetForegroundWindow(target_pid);
        } else {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }

        let fg = GetForegroundWindow();
        let cur_tid = GetCurrentThreadId();
        let mut fg_pid = 0u32;
        let fg_tid = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, &mut fg_pid)
        };

        let attached_fg =
            fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, 1) != 0;
        let attached_tgt = target_tid != 0
            && target_tid != cur_tid
            && target_tid != fg_tid
            && AttachThreadInput(cur_tid, target_tid, 1) != 0;

        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);

        if attached_tgt {
            let _ = AttachThreadInput(cur_tid, target_tid, 0);
        }
        if attached_fg {
            let _ = AttachThreadInput(cur_tid, fg_tid, 0);
        }
    }
}

/// Ctrl+V o Ctrl+Shift+V sintéticos.
///
/// Antes suelta los modificadores que sigan físicamente apretados y no son del
/// atajo: con un Alt colgado la app recibe Ctrl+Alt+V y no pega.
pub fn send_paste_chord(with_shift: bool) {
    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let mut stray: Vec<VIRTUAL_KEY> = vec![VK_LWIN, VK_RWIN, VK_LMENU, VK_RMENU];
    if !with_shift {
        stray.extend([VK_LSHIFT, VK_RSHIFT]);
    }
    let mut inputs: Vec<INPUT> = stray
        .into_iter()
        .filter(|k| unsafe { GetAsyncKeyState(*k as i32) } < 0)
        .map(|k| key(k, true))
        .collect();
    if with_shift {
        inputs.extend([
            key(VK_CONTROL, false),
            key(VK_SHIFT, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_SHIFT, true),
            key(VK_CONTROL, true),
        ]);
    } else {
        inputs.extend([
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ]);
    }
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}
