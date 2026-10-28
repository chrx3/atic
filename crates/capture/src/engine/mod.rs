//! Captura a memoria: monitor/región y ventana.
//!
//! - Windows: GDI `BitBlt` / `PrintWindow`.
//! - macOS: `CGDisplayCreateImage` / `CGWindowListCreateImage`.

use crate::frame::Frame;
use crate::monitors::MonitorInfo;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{
    capture_rect, capture_rect_without_layered, capture_window, capture_window_visual,
    print_window, print_window_tree, snapshot_cursor, CursorSnapshot,
};

#[cfg(target_os = "macos")]
pub use macos::{
    capture_rect, capture_rect_without_layered, capture_window, capture_window_visual,
    print_window, print_window_tree,
};

/// Un monitor entero para transmitirlo (la pantalla del PC en el celular): en macOS a su
/// resolución nominal, que cuesta mucho menos; en Windows, la captura de siempre.
pub fn capture_monitor_for_stream(monitor: &MonitorInfo) -> Option<Frame> {
    #[cfg(target_os = "macos")]
    {
        macos::capture_monitor_nominal(monitor)
    }
    #[cfg(windows)]
    {
        freeze_monitors(std::slice::from_ref(monitor), false).into_iter().next()
    }
}

/// Congela cada monitor a memoria de una sola vez (flujo «congelar primero»).
/// Las capturas que fallen se registran y se omiten.
pub fn freeze_monitors(monitors: &[MonitorInfo], include_cursor: bool) -> Vec<Frame> {
    monitors
        .iter()
        .filter_map(
            |monitor| match capture_rect(monitor.bounds, include_cursor) {
                Ok(frame) => Some(frame),
                Err(error) => {
                    tracing::warn!(%error, id = %monitor.id, "no se pudo congelar el monitor");
                    None
                }
            },
        )
        .collect()
}

/// `true` si todos los píxeles son (casi) negros; corta al primer no-negro.
///
/// Mira solo RGB: las capturas fuerzan alfa 255, así que incluir el alfa hacía
/// que un frame totalmente negro nunca se detectara (ni el permiso faltante ni
/// la pantalla negra).
pub(crate) fn is_black(bgra: &[u8]) -> bool {
    const THRESHOLD: u8 = 8;
    bgra.chunks_exact(4)
        .all(|px| px[0] <= THRESHOLD && px[1] <= THRESHOLD && px[2] <= THRESHOLD)
}
