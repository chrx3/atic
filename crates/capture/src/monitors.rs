//! Enumeración de monitores vía Win32 (`EnumDisplayMonitors`).
//!
//! El motor usa esto para conocer los rectángulos físicos de cada pantalla
//! (para congelar frames) y el área útil (`rcWork`, sin barra de tareas) para
//! ubicar el shelf. La capa Tauri puede además usar `available_monitors()`
//! para la geometría de los overlays.

use serde::Serialize;

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::geometry::Rect;

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// Identificador estable dentro de una sesión (orden de enumeración).
    pub id: String,
    /// Rectángulo físico del monitor en coordenadas del escritorio virtual.
    pub bounds: Rect,
    /// Área útil (excluye la barra de tareas), para ubicar el shelf.
    pub work_area: Rect,
    pub is_primary: bool,
}

/// Rectángulo del escritorio virtual completo (incluye monitores en
/// coordenadas negativas).
pub fn virtual_screen() -> Rect {
    // SAFETY: GetSystemMetrics no tiene precondiciones y no toca memoria nuestra.
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(0) as u32;
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(0) as u32;
        Rect::new(x, y, width, height)
    }
}

/// Enumera los monitores activos.
pub fn enumerate() -> Vec<MonitorInfo> {
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
        });
    }
    // Continuar la enumeración.
    1
}

fn rect_from(r: RECT) -> Rect {
    Rect::from_ltrb(r.left, r.top, r.right, r.bottom)
}
