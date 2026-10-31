//! Lo nativo del flip en Windows: la ventana del frente, su marco, y
//! esconderla y devolverla a su sitio.

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowRect, IsWindow, IsZoomed, SetWindowPlacement, ShowWindow, SW_HIDE,
    SW_SHOWNOACTIVATE, WINDOWPLACEMENT,
};

use crate::geometry::Rect;

/// Dónde estaba la ventana escondida, para devolverla igual: `SW_HIDE` de una
/// maximizada la regresa restaurada y en el centro.
#[derive(Clone, Copy)]
struct Placement {
    show_cmd: u32,
    flags: u32,
    rect: RECT,
}

pub(super) struct Hidden {
    hwnd: isize,
    placement: Option<Placement>,
}

/// El marco visible de la ventana: `GetWindowRect` incluye bordes invisibles
/// de DWM (unos 7 px a cada lado) y la tarjeta quedaría desfasada.
pub(super) fn rect_of(hwnd: isize) -> Option<Rect> {
    use windows::Win32::Foundation::{HWND, RECT as WinRect};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};

    let mut frame = WinRect::default();
    let ok = unsafe {
        DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut _ as *mut _,
            std::mem::size_of::<WinRect>() as u32,
        )
    }
    .is_ok();
    let rect = if ok {
        RECT {
            left: frame.left,
            top: frame.top,
            right: frame.right,
            bottom: frame.bottom,
        }
    } else {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd as _, &mut rect) } == 0 {
            return None;
        }
        rect
    };
    Some(Rect::new(
        rect.left as f32,
        rect.top as f32,
        (rect.right - rect.left) as f32,
        (rect.bottom - rect.top) as f32,
    ))
}

/// El escritorio y la barra de tareas también son ventanas con foco, pero no
/// son lo que alguien quiere dar vuelta.
fn is_shell(hwnd: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    let class = String::from_utf16_lossy(&buffer[..len.max(0) as usize]);
    matches!(
        class.as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW"
    )
}

/// Nombre del ejecutable de un proceso, en minúsculas.
fn exe_name(pid: u32) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut buffer = [0u16; 520];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
    }
}

/// Una ventana que vale la pena dar vuelta: visible, con título, sin
/// minimizar, de tamaño real (Atic y otras apps dejan ventanas de 1×1 px que
/// cuentan como visibles), que Windows no tenga oculta (otro escritorio
/// virtual), que no sea del escritorio ni de la barra, ni de esta app.
pub(super) fn is_flippable(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        GWL_EXSTYLE, SM_CXSCREEN, SM_CYSCREEN, WS_EX_TOOLWINDOW,
    };

    let raw = hwnd as windows_sys::Win32::Foundation::HWND;
    unsafe {
        if IsWindowVisible(raw) == 0 || IsIconic(raw) != 0 || is_shell(hwnd) {
            return false;
        }
        if GetWindowLongW(raw, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(raw, &mut pid);
        // Ni esta app ni la de escritorio de Atic: su overlay transparente
        // ocupa toda la pantalla y siempre está al frente.
        if pid == std::process::id()
            || title_of(hwnd).is_empty()
            || matches!(exe_name(pid).as_str(), "atic-desktop.exe" | "atic.exe")
        {
            return false;
        }
        // Con una parte visible en el monitor principal: Atic deja ventanas
        // grandes aparcadas fuera de pantalla.
        let screen = (
            GetSystemMetrics(SM_CXSCREEN) as f32,
            GetSystemMetrics(SM_CYSCREEN) as f32,
        );
        let visible = |r: &Rect| {
            (r.right().min(screen.0) - r.x.max(0.0)).min(r.bottom().min(screen.1) - r.y.max(0.0))
        };
        let size_ok = |r: &Rect| {
            let w = (r.right().min(screen.0) - r.x.max(0.0)).max(0.0);
            let h = (r.bottom().min(screen.1) - r.y.max(0.0)).max(0.0);
            visible(r) > 0.0 && w >= 120.0 && h >= 80.0
        };
        if !rect_of(hwnd).is_some_and(|r| size_ok(&r)) {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        cloaked == 0
    }
}

/// La ventana real más al frente, en orden de apilado.
pub(super) fn frontmost_real_window() -> Option<isize> {
    use windows_sys::Win32::Foundation::{BOOL, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;

    unsafe extern "system" fn visit(hwnd: windows_sys::Win32::Foundation::HWND, found: LPARAM) -> BOOL {
        if is_flippable(hwnd as isize) {
            *(found as *mut Option<isize>) = Some(hwnd as isize);
            return 0;
        }
        1
    }

    let mut found: Option<isize> = None;
    unsafe {
        EnumWindows(Some(visit), &mut found as *mut _ as LPARAM);
    }
    found
}

pub(super) fn title_of(hwnd: isize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

pub(super) fn hide(hwnd: isize) -> Option<Hidden> {
    if unsafe { IsWindow(hwnd as _) } == 0 {
        return None;
    }
    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    let saved = (unsafe { GetWindowPlacement(hwnd as _, &mut placement) } != 0).then_some(
        Placement {
            show_cmd: placement.showCmd,
            flags: placement.flags,
            rect: placement.rcNormalPosition,
        },
    );
    unsafe {
        let _ = ShowWindow(hwnd as _, SW_HIDE);
    }
    Some(Hidden {
        hwnd,
        placement: saved,
    })
}

/// Cómo se anota en disco lo escondido: el `HWND`.
pub(super) fn key_of(hidden: &Hidden) -> String {
    hidden.hwnd.to_string()
}

/// Muestra de nuevo lo que quedó anotado (Atic se cerró a mitad del flip):
/// una ventana escondida no sale ni en la barra de tareas.
pub(super) fn restore_key(key: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    let Ok(hwnd) = key.trim().parse::<isize>() else {
        return;
    };
    if unsafe { IsWindow(hwnd as _) } != 0 && unsafe { IsWindowVisible(hwnd as _) } == 0 {
        unsafe {
            let _ = ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
        }
    }
}

pub(super) fn restore(hidden: &Hidden) {
    if unsafe { IsWindow(hidden.hwnd as _) } == 0 {
        return;
    }
    let Some(saved) = hidden.placement else {
        unsafe {
            let _ = ShowWindow(hidden.hwnd as _, SW_SHOWNOACTIVATE);
        }
        return;
    };
    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    placement.flags = saved.flags;
    placement.showCmd = saved.show_cmd;
    placement.rcNormalPosition = saved.rect;
    if unsafe { SetWindowPlacement(hidden.hwnd as _, &placement) } == 0 {
        unsafe {
            let _ = ShowWindow(hidden.hwnd as _, SW_SHOWNOACTIVATE);
        }
    }
}

pub(super) fn is_maximized(hwnd: isize) -> bool {
    unsafe { IsZoomed(hwnd as _) != 0 }
}
