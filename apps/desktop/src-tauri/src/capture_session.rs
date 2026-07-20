//! Sesión de captura con overlay de selección (Fase 2).
//!
//! Flujo «congelar primero»: al abrir, se congela todo el escritorio virtual a
//! memoria y a un PNG temporal, y se crea UNA ventana overlay **opaca** que
//! cubre el escritorio virtual mostrando ese frame. El usuario selecciona una
//! ventana (clic), una región (arrastre) o un monitor (Espacio). La captura se
//! recorta del frame congelado (región/monitor) o se re-renderiza con
//! `PrintWindow` (ventana), de modo que el overlay nunca aparece en el
//! resultado.
//!
//! El overlay es opaco a propósito: las ventanas transparentes de WebView2
//! hacen crashear a wry en `WM_SETFOCUS` al recibir un clic.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const OVERLAY_LABEL: &str = "capture-overlay";

#[cfg(windows)]
pub struct OverlaySession {
    /// Frame congelado de todo el escritorio virtual (coords físicas).
    frame: atic_capture::Frame,
    /// Ventanas candidatas (coords físicas globales), z-order topmost-first.
    candidates: Vec<atic_capture::windows::WindowCandidate>,
    /// Monitores, para la selección de monitor completo.
    monitors: Vec<atic_capture::monitors::MonitorInfo>,
    /// PNG temporal del frame congelado (se borra al terminar).
    frame_path: std::path::PathBuf,
}

#[cfg(not(windows))]
pub struct OverlaySession;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCandidate {
    /// `HWND` como entero (cabe en el rango seguro de JS).
    pub hwnd: i64,
    pub title: String,
    // Coordenadas LÓGICAS locales al overlay (px CSS).
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    /// Ruta absoluta del PNG congelado (el frontend la pasa por convertFileSrc).
    pub frame_path: String,
    /// Tamaño lógico del overlay (px CSS).
    pub width: f64,
    pub height: f64,
    pub candidates: Vec<OverlayCandidate>,
}

#[tauri::command]
pub fn start_capture_session(app: AppHandle) -> Result<(), String> {
    start_impl(&app)
}

/// Disparador para el atajo global y el tray (no pasa por `invoke`).
pub fn trigger(app: &AppHandle) -> Result<(), String> {
    start_impl(app)
}

#[tauri::command]
pub fn overlay_info(app: AppHandle) -> Result<OverlayInfo, String> {
    overlay_info_impl(&app)
}

#[tauri::command]
pub fn complete_window_capture(app: AppHandle, hwnd: i64) -> Result<String, String> {
    let (path, anchor) = window_capture_impl(&app, hwnd)?;
    finish(&app, &path, anchor);
    Ok(path)
}

#[tauri::command]
pub fn complete_region_capture(
    app: AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<String, String> {
    let (path, anchor) = region_capture_impl(&app, left, top, width, height)?;
    finish(&app, &path, anchor);
    Ok(path)
}

#[tauri::command]
pub fn complete_monitor_capture(app: AppHandle, x: f64, y: f64) -> Result<String, String> {
    let (path, anchor) = monitor_capture_impl(&app, x, y)?;
    finish(&app, &path, anchor);
    Ok(path)
}

#[tauri::command]
pub fn cancel_capture_session(app: AppHandle) {
    end_session(&app);
}

/// Cierra el overlay, copia al portapapeles, muestra el shelf y notifica.
fn finish(app: &AppHandle, path: &str, shelf_anchor: (i32, i32)) {
    end_session(app);
    crate::capture::notify_capture_ready(app, path, Some(shelf_anchor));
}

fn end_session(app: &AppHandle) {
    // Ocultar (no cerrar): destruir la ventana provoca un crash de wry cuando
    // recibe WM_SETFOCUS durante su destrucción. Se reutiliza en la próxima
    // sesión.
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.hide();
    }
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        let taken = state.overlay_session.lock().unwrap().take();
        end_session_cleanup(taken);
    }
}

#[cfg(windows)]
fn end_session_cleanup(session: Option<OverlaySession>) {
    if let Some(session) = session {
        let _ = std::fs::remove_file(&session.frame_path);
    }
}

#[cfg(not(windows))]
fn end_session_cleanup(_session: Option<OverlaySession>) {}

// ---------------------------------------------------------------------------
// Implementación Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn start_impl(app: &AppHandle) -> Result<(), String> {
    use atic_capture::{engine, monitors, windows as capwin};

    // Cancela cualquier sesión previa (solo una activa).
    end_session(app);

    let state = app.state::<crate::state::AppState>();
    let include_cursor = state.config.lock().unwrap().capture_include_cursor;

    let virtual_screen = monitors::virtual_screen();
    let frame = engine::capture_rect(virtual_screen, include_cursor).map_err(|e| e.to_string())?;
    let png = frame.to_png().map_err(|e| e.to_string())?;

    let frame_path = state.dirs.overlay_frames_dir().join("overlay.png");
    std::fs::write(&frame_path, &png).map_err(|e| e.to_string())?;

    let monitors = monitors::enumerate();
    let candidates = capwin::enumerate_candidates(std::process::id(), &monitors);

    *state.overlay_session.lock().unwrap() = Some(OverlaySession {
        frame,
        candidates,
        monitors,
        frame_path,
    });

    // La ventana `capture-overlay` es estática (creada oculta al arrancar).
    // Se muestra PRIMERO y luego se dimensiona: `set_size`/`set_position` sobre
    // una ventana oculta no tomaba efecto, dejándola en 800x600 con scroll.
    let window = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("la ventana del overlay no existe")?;
    let position = tauri::PhysicalPosition::new(virtual_screen.x, virtual_screen.y);
    let size = tauri::PhysicalSize::new(virtual_screen.width, virtual_screen.height);
    let _ = window.set_decorations(false);
    let _ = window.show();
    let _ = window.set_position(position);
    let _ = window.set_size(size);
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    // El webview persiste; re-inicializa con los datos de la nueva sesión.
    let _ = app.emit("overlay-session-started", ());
    Ok(())
}

#[cfg(windows)]
fn overlay_info_impl(app: &AppHandle) -> Result<OverlayInfo, String> {
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock().unwrap();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    let scale = overlay_scale(app);
    let bounds = session.frame.bounds;

    let candidates = session
        .candidates
        .iter()
        .map(|candidate| {
            let visual = candidate.visual_bounds;
            OverlayCandidate {
                hwnd: candidate.hwnd as i64,
                title: candidate.title.clone(),
                left: f64::from(visual.x - bounds.x) / scale,
                top: f64::from(visual.y - bounds.y) / scale,
                width: f64::from(visual.width) / scale,
                height: f64::from(visual.height) / scale,
            }
        })
        .collect();

    Ok(OverlayInfo {
        frame_path: session.frame_path.to_string_lossy().into_owned(),
        width: f64::from(bounds.width) / scale,
        height: f64::from(bounds.height) / scale,
        candidates,
    })
}

#[cfg(windows)]
fn window_capture_impl(app: &AppHandle, hwnd: i64) -> Result<(String, (i32, i32)), String> {
    use atic_capture::{engine, windows as capwin};

    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock().unwrap();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    // PrintWindow renderiza solo la ventana; si falla/negro, recorta del frame
    // congelado (nunca de la pantalla, para no capturar el overlay).
    let frame = match engine::print_window(hwnd as isize).map_err(|e| e.to_string())? {
        Some(frame) => frame,
        None => {
            let win_bounds =
                capwin::window_bounds(hwnd as isize).ok_or("ventana sin límites")?;
            session
                .frame
                .crop(win_bounds)
                .ok_or("la ventana quedó fuera del área capturada")?
        }
    };
    drop(guard);
    save_capture(app, &frame)
}

#[cfg(windows)]
fn region_capture_impl(
    app: &AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<(String, (i32, i32)), String> {
    use atic_capture::Rect;

    let scale = overlay_scale(app);
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock().unwrap();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;
    let bounds = session.frame.bounds;

    let region = Rect::new(
        bounds.x + (left * scale).round() as i32,
        bounds.y + (top * scale).round() as i32,
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    let frame = session
        .frame
        .crop(region)
        .ok_or("la región quedó fuera del área capturada")?;
    drop(guard);
    save_capture(app, &frame)
}

#[cfg(windows)]
fn monitor_capture_impl(app: &AppHandle, x: f64, y: f64) -> Result<(String, (i32, i32)), String> {
    let scale = overlay_scale(app);
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock().unwrap();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;
    let bounds = session.frame.bounds;

    let point_x = bounds.x + (x * scale).round() as i32;
    let point_y = bounds.y + (y * scale).round() as i32;
    let monitor = session
        .monitors
        .iter()
        .find(|monitor| monitor.bounds.contains(point_x, point_y))
        .or_else(|| session.monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| session.monitors.first())
        .ok_or("no se encontró el monitor")?;
    let frame = session
        .frame
        .crop(monitor.bounds)
        .ok_or("el monitor quedó fuera del área capturada")?;
    drop(guard);
    save_capture(app, &frame)
}

#[cfg(windows)]
fn save_capture(app: &AppHandle, frame: &atic_capture::Frame) -> Result<(String, (i32, i32)), String> {
    use atic_capture::naming;
    let state = app.state::<crate::state::AppState>();
    let png = frame.to_png().map_err(|e| e.to_string())?;
    let dir = state.dirs.captures_dir();
    let path = dir.join(naming::unique_capture_filename(&dir));
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    let anchor = (
        frame.bounds.x + frame.bounds.width as i32 / 2,
        frame.bounds.y + frame.bounds.height as i32 / 2,
    );
    Ok((path.to_string_lossy().into_owned(), anchor))
}

#[cfg(windows)]
fn overlay_scale(app: &AppHandle) -> f64 {
    app.get_webview_window(OVERLAY_LABEL)
        .and_then(|window| window.scale_factor().ok())
        .unwrap_or(1.0)
}

// ---------------------------------------------------------------------------
// Stubs para plataformas no-Windows (la captura solo existe en Windows).
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn start_impl(_app: &AppHandle) -> Result<(), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn overlay_info_impl(_app: &AppHandle) -> Result<OverlayInfo, String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn window_capture_impl(_app: &AppHandle, _hwnd: i64) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn region_capture_impl(
    _app: &AppHandle,
    _left: f64,
    _top: f64,
    _width: f64,
    _height: f64,
) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn monitor_capture_impl(_app: &AppHandle, _x: f64, _y: f64) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}
