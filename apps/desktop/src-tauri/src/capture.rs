//! Integración Tauri del motor de capturas (`atic-capture`): captura de
//! pantalla, gestión de los PNG en disco y comandos del shelf.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::TimeZone;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;
use atic_core::MutexExt;

/// Máximo de capturas que muestra el shelf.
const SHELF_LIMIT: usize = 5;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureItem {
    /// Nombre de archivo, usado como id dentro de la carpeta de capturas.
    pub id: String,
    /// Etiqueta corta para la UI (p. ej. `18:10`).
    pub label: String,
    /// Ruta absoluta del PNG.
    pub path: String,
    /// Fecha de creación en milisegundos desde epoch (mtime del archivo).
    pub created_at_ms: u64,
    pub width: u32,
    pub height: u32,
}

/// Captura el monitor primario, lo guarda, notifica y muestra el shelf.
#[tauri::command]
pub fn capture_primary_monitor(app: AppHandle) -> Result<String, String> {
    capture_and_show(&app)
}

/// Captura + guarda + portapapeles + shelf + emite `screenshot-created`.
/// Compartido por el comando y el disparador del tray.
pub fn capture_and_show(app: &AppHandle) -> Result<String, String> {
    let (path, anchor) = capture_primary(app)?;
    notify_capture_ready(app, &path, Some(anchor));
    Ok(path)
}

/// Tras guardar un PNG: copiar al portapapeles, mostrar shelf y emitir evento.
///
/// `shelf_anchor` (coords físicas) elige en qué monitor aparece el shelf.
pub fn notify_capture_ready(app: &AppHandle, path: &str, shelf_anchor: Option<(i32, i32)>) {
    let item = capture_item(Path::new(path));
    // Antes de copiar, no después: el historial tiene que quedarse con ESTA
    // identidad (`capture:<id>`, apuntando al PNG del dir de capturas) y no con
    // la que el watcher improvisaría al ver la imagen en el portapapeles.
    if let Some(item) = item.as_ref() {
        crate::clipboard_history::record_capture(app, item);
    }
    if let Err(error) = copy_png_to_clipboard(Path::new(path)) {
        tracing::warn!(%error, "no se pudo copiar la captura al portapapeles");
    }
    let _ = crate::capture_shelf::show_shelf(app, shelf_anchor);
    if let Some(item) = item {
        let _ = app.emit("screenshot-created", item);
    }

    let state = app.state::<AppState>();
    let (ui_sounds, output_device_id, sound_voice) = {
        let cfg = state.config.lock_or_recover();
        (
            cfg.ui_sounds,
            cfg.output_device_id.clone(),
            cfg.sound_capture.clone(),
        )
    };
    if ui_sounds {
        crate::beep::play(
            crate::beep::SoundAction::Capture,
            &sound_voice,
            &output_device_id,
        );
    }
}

#[tauri::command]
pub fn list_recent_captures(state: State<AppState>) -> Vec<CaptureItem> {
    recent_captures(&state.dirs.captures_dir())
}

#[tauri::command]
pub fn delete_capture(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    let target = ensure_in_dir(&state.dirs.captures_dir(), Path::new(&path))?;
    std::fs::remove_file(&target).map_err(|error| error.to_string())?;
    if let Some(name) = target.file_name().and_then(|n| n.to_str()) {
        crate::clipboard_history::dismiss_capture(&app, name);
    }
    let _ = app.emit("screenshot-shelf-updated", ());
    Ok(())
}

#[tauri::command]
pub fn copy_capture_path(path: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard.set_text(path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn copy_capture_image(state: State<AppState>, path: String) -> Result<(), String> {
    let target = ensure_in_dir(&state.dirs.captures_dir(), Path::new(&path))?;
    copy_png_to_clipboard(&target)
}

/// Copia el PNG al portapapeles del sistema (imagen, no la ruta).
pub fn copy_png_to_clipboard(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    copy_png_bytes(&bytes)
}

/// Igual, pero desde memoria: lo anotado todavía no es un archivo.
pub fn copy_png_bytes(bytes: &[u8]) -> Result<(), String> {
    let (width, height, rgba) =
        atic_capture::encoding::png_to_rgba(bytes).map_err(|error| error.to_string())?;
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Owned(rgba),
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reveal_capture(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let target = ensure_in_dir(&state.dirs.captures_dir(), Path::new(&path))?;
    app.opener()
        .reveal_item_in_dir(target)
        .map_err(|error| error.to_string())
}

/// Acción al hacer clic en la miniatura: abre el editor de anotaciones, la
/// vista previa (imagen) o la ubicación (carpeta), según `capture_click_action`.
#[tauri::command]
pub fn activate_capture(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let target = ensure_in_dir(&state.dirs.captures_dir(), Path::new(&path))?;
    let action = state.config.lock_or_recover().capture_click_action.clone();
    match action.as_str() {
        "location" => app
            .opener()
            .reveal_item_in_dir(target)
            .map_err(|error| error.to_string()),
        "annotate" => crate::annotate::open_annotator_path(
            &app,
            &state.dirs.captures_dir(),
            &target.to_string_lossy(),
        ),
        _ => app
            .opener()
            .open_path(target.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|error| error.to_string()),
    }
}

/// Limpia ahora las capturas más antiguas que `capture_retention_hours`.
#[tauri::command]
pub fn cleanup_captures_now(state: State<AppState>) -> Result<usize, String> {
    let hours = state.config.lock_or_recover().capture_retention_hours;
    let result = atic_capture::retention::cleanup_captures(
        &state.dirs.captures_dir(),
        hours,
        std::time::SystemTime::now(),
    );
    Ok(result.deleted)
}

#[tauri::command]
pub fn open_captures_dir(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    crate::commands::open_data_dir_kind(&app, &state.dirs, "captures")
}

/// Limpieza automática de capturas al iniciar (según retención configurada).
pub fn run_capture_cleanup(app: &AppHandle) {
    let state = app.state::<AppState>();
    let hours = state.config.lock_or_recover().capture_retention_hours;
    if hours == 0 {
        return;
    }
    let result = atic_capture::retention::cleanup_captures(
        &state.dirs.captures_dir(),
        hours,
        std::time::SystemTime::now(),
    );
    if result.deleted > 0 || !result.errors.is_empty() {
        tracing::info!(
            deleted = result.deleted,
            errors = result.errors.len(),
            "limpieza automática de capturas"
        );
    }
}

fn recent_captures(dir: &Path) -> Vec<CaptureItem> {
    recent_captures_limited(dir, SHELF_LIMIT)
}

/// Listado de capturas recientes (para shelf o merge con historial de clipboard).
pub(crate) fn recent_captures_limited(dir: &Path, limit: usize) -> Vec<CaptureItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<CaptureItem> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .filter_map(|path| capture_item(&path))
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.created_at_ms));
    items.truncate(limit);
    items
}

pub(crate) fn capture_item(path: &Path) -> Option<CaptureItem> {
    let metadata = std::fs::metadata(path).ok()?;
    let created_at_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    // El IHDR va al inicio del PNG; leer un prefijo basta para las dimensiones.
    let header = read_prefix(path, 1024)?;
    let (width, height) = atic_capture::encoding::png_dimensions(&header).ok()?;
    let label = chrono::Local
        .timestamp_millis_opt(created_at_ms as i64)
        .single()
        .map(|dt| atic_capture::naming::shelf_label(&dt.naive_local()))
        .unwrap_or_else(|| "Captura".into());
    Some(CaptureItem {
        id: path.file_name()?.to_string_lossy().into_owned(),
        label,
        path: path.to_string_lossy().into_owned(),
        created_at_ms,
        width,
        height,
    })
}

fn read_prefix(path: &Path, bytes: usize) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; bytes];
    let read = file.read(&mut buffer).ok()?;
    buffer.truncate(read);
    Some(buffer)
}

/// Verifica que `path` esté directamente dentro de `dir` (evita leer o borrar
/// archivos arbitrarios vía comando) y devuelve la ruta canónica.
pub(crate) fn ensure_capture_in_dir(dir: &Path, path: &Path) -> Result<PathBuf, String> {
    ensure_in_dir(dir, path)
}

/// Verifica que `path` esté directamente dentro de `dir` (evita leer o borrar
/// archivos arbitrarios vía comando) y devuelve la ruta canónica.
fn ensure_in_dir(dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical_dir = std::fs::canonicalize(dir).map_err(|error| error.to_string())?;
    let resolved = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    if resolved.parent() == Some(canonical_dir.as_path()) {
        Ok(resolved)
    } else {
        Err("Ruta fuera del directorio de capturas.".into())
    }
}

#[cfg(windows)]
fn capture_primary(app: &AppHandle) -> Result<(String, (i32, i32)), String> {
    use atic_capture::{engine, monitors, naming};

    let state = app.state::<AppState>();
    let dir = state.dirs.captures_dir();

    let monitors = monitors::enumerate();
    let target = monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .or_else(|| monitors.first())
        .ok_or_else(|| "No se detectaron monitores.".to_string())?;

    let frame = engine::capture_rect(target.bounds, false).map_err(|error| error.to_string())?;
    let png = frame.to_png().map_err(|error| error.to_string())?;
    let anchor = rect_center(frame.bounds);

    let path = dir.join(naming::unique_capture_filename(&dir));
    std::fs::write(&path, &png).map_err(|error| error.to_string())?;
    Ok((path.to_string_lossy().into_owned(), anchor))
}

#[cfg(windows)]
fn rect_center(bounds: atic_capture::Rect) -> (i32, i32) {
    (
        bounds.x + bounds.width as i32 / 2,
        bounds.y + bounds.height as i32 / 2,
    )
}

#[cfg(not(windows))]
fn capture_primary(_app: &AppHandle) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}
