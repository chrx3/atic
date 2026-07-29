//! Notificación de captura tipo macOS (`capture-shelf`).
//!
//! Ventana estática y transparente (declarada en `tauri.conf.json`, como la
//! pill) que aparece en la esquina inferior del monitor correspondiente
//! mostrando la última captura para arrastrarla. Se crea oculta al arrancar y
//! aquí solo se muestra y reposiciona: crear ventanas WebView2 en caliente
//! crasheaba a wry.

use atic_core::MutexExt;
use tauri::{AppHandle, Manager, WebviewWindow};

const SHELF_LABEL: &str = "capture-shelf";

/// Muestra la notificación de captura.
///
/// `anchor` es un punto en coordenadas físicas del escritorio virtual (p. ej.
/// el centro del área capturada). El shelf se coloca en ese monitor; si no hay
/// ancla, usa el primario.
pub fn show_shelf(app: &AppHandle, anchor: Option<(i32, i32)>) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(SHELF_LABEL) else {
        tracing::warn!("la ventana de la notificación de captura no existe");
        return Ok(());
    };
    let _ = window.unminimize();
    // Forzar en runtime por si la config de la ventana no se aplicó: sin barra
    // de título ni botones, sin barra de tareas.
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    position_shelf(app, &window, anchor);
    window.show()?;
    let _ = window.set_always_on_top(true);
    Ok(())
}

/// Delega en `floating`: la esquina y el clamp al monitor son los mismos que
/// usa la pill. Antes esto reimplementaba la búsqueda de monitor y el margen.
fn position_shelf(app: &AppHandle, _window: &WebviewWindow, anchor: Option<(i32, i32)>) {
    let left_side = app
        .try_state::<crate::state::AppState>()
        .map(|state| state.config.lock_or_recover().capture_shelf_side.clone())
        .unwrap_or_else(|| "right".into())
        == "left";

    crate::floating::place(
        app,
        SHELF_LABEL,
        crate::floating::Anchor::BottomCorner {
            near: anchor,
            left_side,
        },
    );
}
