//! Notificación de captura tipo macOS (`capture-shelf`).
//!
//! Ventana estática y transparente (declarada en `tauri.conf.json`, como la
//! pill) que aparece en la esquina inferior derecha mostrando la última
//! captura para arrastrarla. Se crea oculta al arrancar y aquí solo se muestra
//! y reposiciona: crear ventanas WebView2 en caliente crasheaba a wry.

use tauri::{AppHandle, Manager, WebviewWindow};

const SHELF_LABEL: &str = "capture-shelf";

/// Muestra la notificación de captura en la esquina inferior derecha del área
/// útil del monitor primario.
pub fn show_shelf(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(SHELF_LABEL) else {
        tracing::warn!("la ventana de la notificación de captura no existe");
        return Ok(());
    };
    let _ = window.unminimize();
    // Forzar en runtime por si la config de la ventana no se aplicó: sin barra
    // de título ni botones, sin barra de tareas.
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    position_shelf(&window);
    window.show()?;
    let _ = window.set_always_on_top(true);
    Ok(())
}

#[cfg(windows)]
fn position_shelf(window: &WebviewWindow) {
    use atic_capture::monitors;

    let monitors = monitors::enumerate();
    let Some(primary) = monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .or_else(|| monitors.first())
    else {
        return;
    };
    let work = primary.work_area;
    if let Ok(size) = window.outer_size() {
        const MARGIN: i32 = 16;
        let x = work.x + work.width as i32 - size.width as i32 - MARGIN;
        let y = work.y + work.height as i32 - size.height as i32 - MARGIN;
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

#[cfg(not(windows))]
fn position_shelf(_window: &WebviewWindow) {}
