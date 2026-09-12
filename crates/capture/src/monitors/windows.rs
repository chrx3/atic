//! Enumeración de monitores vía Win32 (`EnumDisplayMonitors`).

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use super::MonitorInfo;
use crate::geometry::Rect;

/// Fallback cuando `EnumDisplayMonitors` no devolvió nada.
///
/// Tras hibernar, las métricas del escritorio virtual a veces siguen en una
/// sola pantalla mientras los monitores ya volvieron; por eso `virtual_screen`
/// prefiere la unión de `enumerate`.
pub(super) fn metrics_virtual_screen() -> Rect {
    // SAFETY: GetSystemMetrics no tiene precondiciones y no toca memoria nuestra.
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(0) as u32;
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(0) as u32;
        Rect::new(x, y, width, height)
    }
}

pub(super) fn enumerate() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    // SAFETY: el puntero a `monitors` vive durante toda la llamada y solo se usa
    // dentro del callback en este mismo hilo.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            &mut monitors as *mut Vec<MonitorInfo> as LPARAM,
        );
    }
    monitors
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    param: LPARAM,
) -> i32 {
    let monitors = &mut *(param as *mut Vec<MonitorInfo>);
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) != 0 {
        monitors.push(MonitorInfo {
            id: format!("monitor-{}", monitors.len()),
            bounds: rect_from(info.rcMonitor),
            work_area: rect_from(info.rcWork),
            is_primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            scale: scale_of(monitor),
        });
    }
    1
}

/// Escala efectiva del monitor. 96 dpi = 100%.
///
/// `MDT_EFFECTIVE_DPI` y no `MDT_RAW_DPI`: el que interesa es el que el usuario
/// eligió en Configuración, que es con el que Windows escala las ventanas.
unsafe fn scale_of(monitor: HMONITOR) -> f64 {
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let mut dpi_x: u32 = 96;
    let mut dpi_y: u32 = 96;
    if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) == 0 {
        f64::from(dpi_x) / 96.0
    } else {
        1.0
    }
}

fn rect_from(r: RECT) -> Rect {
    Rect::from_ltrb(r.left, r.top, r.right, r.bottom)
}
