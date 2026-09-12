//! Enumeración y filtrado de ventanas candidatas para capturar.
//!
//! - Windows: `EnumWindows` + DWM (marco visual, sin sombra).
//! - macOS: `CGWindowListCopyWindowInfo` (capa 0, en pantalla).
//!
//! El campo `hwnd` es el `HWND` en Windows y el `CGWindowID` en macOS.

use serde::Serialize;

use crate::geometry::Rect;
use crate::monitors::MonitorInfo;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win32;

#[cfg(target_os = "macos")]
pub use macos::{enumerate_candidates, foreground_window, window_bounds, window_outer_bounds};
#[cfg(windows)]
pub use win32::{enumerate_candidates, foreground_window, window_bounds, window_outer_bounds};

/// Lado mínimo (px físicos) para considerar una ventana seleccionable.
pub(crate) const MIN_WINDOW_SIDE: u32 = 40;

#[derive(Debug, Clone, Serialize)]
pub struct WindowCandidate {
    /// Handle nativo como entero: `HWND` en Windows, `CGWindowID` en macOS.
    pub hwnd: isize,
    pub title: String,
    /// Límites visuales en coordenadas físicas del escritorio virtual.
    pub visual_bounds: Rect,
    pub process_id: u32,
    /// Posición en el orden Z (0 = más al frente).
    pub z_index: usize,
    pub monitor_id: String,
}

/// Índice del candidato más al frente que contiene el punto físico, o `None`.
pub fn topmost_at(candidates: &[WindowCandidate], x: i32, y: i32) -> Option<usize> {
    candidates
        .iter()
        .position(|c| c.visual_bounds.contains(x, y))
}

/// Monitor cuyo rectángulo tiene mayor intersección con la ventana.
pub(crate) fn monitor_for(bounds: &Rect, monitors: &[MonitorInfo]) -> String {
    monitors
        .iter()
        .filter_map(|m| m.bounds.intersection(bounds).map(|i| (i.area(), &m.id)))
        .max_by_key(|(area, _)| *area)
        .map(|(_, id)| id.clone())
        .unwrap_or_default()
}
