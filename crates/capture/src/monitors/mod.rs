//! Enumeración de monitores.
//!
//! El motor usa esto para los rectángulos físicos de cada pantalla (congelar
//! frames) y el área útil (`work_area`) para ubicar el shelf. La capa Tauri
//! puede además usar `available_monitors()` para la geometría de los overlays.
//!
//! - Windows: `EnumDisplayMonitors` (píxeles físicos del escritorio virtual).
//! - macOS: Core Graphics `CGDisplayBounds`, **en puntos** (el espacio en el
//!   que AppKit coloca ventanas y en el que vive el cursor). El origen es el
//!   mismo que en Windows: esquina superior izquierda de la pantalla
//!   principal, Y hacia abajo. `scale` guarda los píxeles nativos por punto
//!   de cada monitor, que solo usa el motor al capturar.

use serde::Serialize;

use crate::geometry::Rect;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// Identificador estable dentro de una sesión (orden de enumeración).
    pub id: String,
    /// Rectángulo físico del monitor en coordenadas del escritorio virtual.
    pub bounds: Rect,
    /// Área útil (excluye barra de tareas / menú), para ubicar el shelf.
    pub work_area: Rect,
    pub is_primary: bool,
    /// Escala del monitor (1.0 = 100%, 1.25 = 125%, 2.0 = Retina…).
    ///
    /// Por monitor y no por ventana: quien decide EN QUÉ monitor poner algo
    /// necesita saber la escala de cada uno antes de haberlo puesto ahí.
    pub scale: f64,
}

/// Rectángulo del escritorio virtual completo (incluye monitores en
/// coordenadas negativas).
///
/// Unión de los monitores enumerados. Si no hay ninguno, un recuadro mínimo
/// para no romper a quien calcula tamaños.
pub fn virtual_screen() -> Rect {
    if let Some(union) = Rect::union_all(enumerate().into_iter().map(|m| m.bounds)) {
        return union;
    }
    #[cfg(windows)]
    {
        return windows::metrics_virtual_screen();
    }
    #[cfg(not(windows))]
    Rect::new(0, 0, 1280, 720)
}

/// Enumera los monitores activos.
pub fn enumerate() -> Vec<MonitorInfo> {
    #[cfg(windows)]
    {
        windows::enumerate()
    }
    #[cfg(target_os = "macos")]
    {
        macos::enumerate()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Vec::new()
    }
}

/// Monitor que contiene un punto del escritorio virtual.
///
/// Se busca por `bounds` y no por `work_area`: un punto sobre la barra de
/// tareas sigue perteneciendo a ese monitor, y quien pregunta suele estar
/// resolviendo «dónde está el cursor» o «dónde está esta ventana».
pub fn from_point(x: i32, y: i32) -> Option<MonitorInfo> {
    let monitors = enumerate();
    monitors
        .iter()
        .find(|m| m.bounds.contains(x, y))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first())
        .cloned()
}
