//! Enumeración Win32: `EnumWindows` + DWM (marco visual, sin sombra).

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE,
    WS_EX_TOOLWINDOW,
};

use super::{monitor_for, WindowCandidate, MIN_WINDOW_SIDE};
use crate::geometry::Rect;
use crate::monitors::MonitorInfo;

struct Collector<'a> {
    exclude_pid: u32,
    monitors: &'a [MonitorInfo],
    out: Vec<WindowCandidate>,
}

/// Construye la lista de ventanas candidatas, excluyendo las del proceso
/// `exclude_pid` (las propias ventanas de Atic).
pub fn enumerate_candidates(exclude_pid: u32, monitors: &[MonitorInfo]) -> Vec<WindowCandidate> {
    let mut collector = Collector {
        exclude_pid,
        monitors,
        out: Vec::new(),
    };
    // SAFETY: `collector` vive durante toda la llamada y solo se usa dentro del
    // callback, en este hilo.
    unsafe {
        EnumWindows(
            Some(collect_window),
            &mut collector as *mut Collector as LPARAM,
        );
    }
    collector.out
}

/// `HWND` de la ventana en primer plano, como entero (`0` si no hay).
pub fn foreground_window() -> isize {
    // SAFETY: sin precondiciones.
    unsafe { GetForegroundWindow() as isize }
}

/// Límites visuales de una ventana (extended frame bounds, con fallback a
/// `GetWindowRect`).
pub fn window_bounds(hwnd: isize) -> Option<Rect> {
    // SAFETY: se valida el HRESULT y el rect antes de usarlos.
    unsafe { extended_frame_bounds(hwnd as HWND) }
}

/// `GetWindowRect`: incluye la sombra invisible de DWM.
///
/// `PrintWindow` pinta en este rectángulo. Si el lienzo es el marco visual
/// (más chico, desplazado), la foto nace con la sombra a la izquierda y el
/// contenido corrido: al tapar la ventana se ve una franja del escritorio.
pub fn window_outer_bounds(hwnd: isize) -> Option<Rect> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd as HWND, &mut rect) } == 0 {
        return None;
    }
    let bounds = Rect::from_ltrb(rect.left, rect.top, rect.right, rect.bottom);
    if bounds.is_empty() {
        None
    } else {
        Some(bounds)
    }
}

unsafe extern "system" fn collect_window(hwnd: HWND, param: LPARAM) -> i32 {
    let collector = &mut *(param as *mut Collector);

    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return 1;
    }
    if is_cloaked(hwnd) {
        return 1;
    }
    // Descarta ventanas-herramienta (tooltips, paletas flotantes, etc.).
    let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if exstyle & WS_EX_TOOLWINDOW != 0 {
        return 1;
    }

    let mut process_id = 0u32;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    if process_id == collector.exclude_pid {
        return 1;
    }

    let Some(visual_bounds) = extended_frame_bounds(hwnd) else {
        return 1;
    };
    if visual_bounds.width < MIN_WINDOW_SIDE || visual_bounds.height < MIN_WINDOW_SIDE {
        return 1;
    }

    let z_index = collector.out.len();
    let monitor_id = monitor_for(&visual_bounds, collector.monitors);
    collector.out.push(WindowCandidate {
        hwnd: hwnd as isize,
        title: window_title(hwnd),
        visual_bounds,
        process_id,
        z_index,
        monitor_id,
    });
    1
}

unsafe fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let hr = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED as u32,
        &mut cloaked as *mut u32 as *mut core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
    hr == 0 && cloaked != 0
}

unsafe fn extended_frame_bounds(hwnd: HWND) -> Option<Rect> {
    let mut rect: RECT = std::mem::zeroed();
    let hr = DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS as u32,
        &mut rect as *mut RECT as *mut core::ffi::c_void,
        std::mem::size_of::<RECT>() as u32,
    );
    if hr != 0 {
        // Fallback: rect clásico (incluye la sombra, pero es mejor que nada).
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
    }
    let bounds = Rect::from_ltrb(rect.left, rect.top, rect.right, rect.bottom);
    if bounds.is_empty() {
        None
    } else {
        Some(bounds)
    }
}

unsafe fn window_title(hwnd: HWND) -> String {
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if copied <= 0 {
        return String::new();
    }
    buffer.truncate(copied as usize);
    String::from_utf16_lossy(&buffer)
}
