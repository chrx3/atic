//! Notificación de captura tipo macOS (`capture-shelf`).
//!
//! Ventana estática y transparente (declarada en `tauri.conf.json`, como la
//! pill) que aparece en la esquina inferior del monitor correspondiente
//! mostrando la última captura para arrastrarla. Se crea oculta al arrancar y
//! aquí solo se muestra y reposiciona: crear ventanas WebView2 en caliente
//! crasheaba a wry.

use atic_core::MutexExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};

const SHELF_LABEL: &str = "capture-shelf";

/// Lo usa el overlay para quedar debajo del preview arrastrado.
pub const LABEL: &str = SHELF_LABEL;

/// Más que cualquier monitor razonable. `set_max_size(None)` no levanta el
/// tope de `tauri.conf.json` (288×120) y Windows recorta el HWND al toast.
const COVER_MAX: u32 = 16384;

/// Límites lógicos del toast, iguales a `tauri.conf.json`.
const TOAST_MIN_W: f64 = 200.0;
const TOAST_MIN_H: f64 = 88.0;
const TOAST_MAX_W: f64 = 288.0;
const TOAST_MAX_H: f64 = 120.0;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PhysicalBounds {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelfCover {
    pub rest: PhysicalBounds,
    pub mon_x: i32,
    pub mon_y: i32,
    pub mon_w: u32,
    pub mon_h: u32,
}

#[cfg(windows)]
fn snapshot_outer(window: &WebviewWindow) -> Option<PhysicalBounds> {
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(PhysicalBounds {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    })
}

/// Cubre el monitor del toast para que el preview pueda viajar con el mouse.
///
/// El estante nace 256×104, no redimensionable y con max 288×120. Un
/// `set_size` desde JS no agranda ese HWND: el ghost se pinta y se recorta
/// en la esquina. Acá se hace lo mismo que el overlay: tope enorme,
/// `SetWindowPos` de una y bounds de WebView2 al cliente nuevo.
#[tauri::command]
pub fn capture_shelf_cover_monitor(window: tauri::WebviewWindow) -> Option<ShelfCover> {
    if window.label() != SHELF_LABEL {
        return None;
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
    #[cfg(windows)]
    {
        let rest = snapshot_outer(&window)?;
        let cx = rest.x + rest.w as i32 / 2;
        let cy = rest.y + rest.h as i32 / 2;
        let mon = atic_capture::monitors::from_point(cx, cy)?;
        let cover = ShelfCover {
            rest,
            mon_x: mon.bounds.x,
            mon_y: mon.bounds.y,
            mon_w: mon.bounds.width.max(1),
            mon_h: mon.bounds.height.max(1),
        };
        apply_cover(&window, mon.bounds);
        Some(cover)
    }
}

/// Devuelve el toast a su tamaño y tope originales.
#[tauri::command]
pub fn capture_shelf_restore_bounds(
    window: tauri::WebviewWindow,
    rest: PhysicalBounds,
) -> Result<(), String> {
    if window.label() != SHELF_LABEL {
        return Err("solo el estante puede restaurarse".into());
    }
    apply_restore(&window, rest);
    Ok(())
}

#[cfg(windows)]
fn apply_cover(window: &WebviewWindow, bounds: atic_capture::Rect) {
    let _ = window.set_resizable(true);
    let _ = window.set_min_size(None::<tauri::LogicalSize<f64>>);
    let _ = window.set_max_size(Some(tauri::Size::Physical(tauri::PhysicalSize::new(
        COVER_MAX, COVER_MAX,
    ))));
    place_hwnd(
        window,
        bounds.x,
        bounds.y,
        bounds.width as i32,
        bounds.height as i32,
    );
}

fn apply_restore(window: &WebviewWindow, rest: PhysicalBounds) {
    place_hwnd(window, rest.x, rest.y, rest.w as i32, rest.h as i32);
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(TOAST_MIN_W, TOAST_MIN_H)));
    let _ = window.set_max_size(Some(tauri::LogicalSize::new(TOAST_MAX_W, TOAST_MAX_H)));
    let _ = window.set_resizable(false);
    #[cfg(windows)]
    crate::webview_tweaks::sync_controller_bounds(window);
}

fn place_hwnd(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
        };
        if let Ok(hwnd) = window.hwnd() {
            let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
            // SAFETY: HWND de Tauri vivo; SetWindowPos no retiene el handle.
            // Sin NOZORDER: el overlay también es topmost y si no subimos el
            // estante, el preview queda detrás de agentes.
            unsafe {
                SetWindowPos(
                    hwnd,
                    windows_sys::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
                    x,
                    y,
                    w.max(1),
                    h.max(1),
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
        }
        crate::webview_tweaks::sync_controller_bounds(window);
    }
    #[cfg(not(windows))]
    {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        let _ = window.set_size(tauri::PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
    }
}
