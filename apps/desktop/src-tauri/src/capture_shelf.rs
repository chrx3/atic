//! Notificación de captura tipo macOS (`capture-shelf`).
//!
//! Ventana estática y transparente (declarada en `tauri.conf.json`, como la
//! pill) que aparece en la esquina inferior del monitor correspondiente
//! mostrando la última captura para arrastrarla. Se crea oculta al arrancar y
//! aquí solo se muestra y reposiciona: crear ventanas WebView2 en caliente
//! crasheaba a wry.

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

#[cfg(windows)]
fn position_shelf(app: &AppHandle, window: &WebviewWindow, anchor: Option<(i32, i32)>) {
    use atic_capture::monitors;

    let monitors = monitors::enumerate();
    if monitors.is_empty() {
        return;
    }

    let target = anchor
        .and_then(|(x, y)| {
            monitors
                .iter()
                .find(|monitor| monitor.bounds.contains(x, y))
        })
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| monitors.first());

    let Some(target) = target else {
        return;
    };

    let side = app
        .try_state::<crate::state::AppState>()
        .map(|state| {
            state
                .config
                .lock()
                .unwrap()
                .capture_shelf_side
                .clone()
        })
        .unwrap_or_else(|| "right".into());

    let work = target.work_area;
    if let Ok(size) = window.outer_size() {
        const MARGIN: i32 = 16;
        let x = if side == "left" {
            work.x + MARGIN
        } else {
            work.x + work.width as i32 - size.width as i32 - MARGIN
        };
        let y = work.y + work.height as i32 - size.height as i32 - MARGIN;
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

#[cfg(not(windows))]
fn position_shelf(_app: &AppHandle, _window: &WebviewWindow, _anchor: Option<(i32, i32)>) {}
