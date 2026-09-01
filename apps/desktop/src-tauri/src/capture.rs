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
    crate::clipboard_history::set_system_text(path)
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
    crate::clipboard_history::with_clipboard_write(|| {
        #[cfg(windows)]
        {
            set_clipboard_image_win(bytes, width, height, &rgba)
        }
        #[cfg(not(windows))]
        {
            set_clipboard_image_arboard(width, height, &rgba)
        }
    })
}

#[cfg(not(windows))]
fn set_clipboard_image_arboard(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..8u32 {
        let result = arboard::Clipboard::new()
            .map_err(|error| error.to_string())
            .and_then(|mut clipboard| {
                clipboard
                    .set_image(arboard::ImageData {
                        width: width as usize,
                        height: height as usize,
                        bytes: std::borrow::Cow::Borrowed(rgba),
                    })
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => return Ok(()),
            Err(error) => last = error,
        }
        std::thread::sleep(std::time::Duration::from_millis(10 + 15 * u64::from(attempt)));
    }
    Err(last)
}

/// Encabezado DIBV5 + píxeles BGRA bottom-up. Se arma ANTES de abrir el
/// portapapeles: arboard encodea el PNG con el clipboard ya abierto, y en
/// una pizarra de escritorio entero esa ventana es suficiente para que
/// otro hilo (nuestro watcher, o el historial de Windows) cierre el
/// clipboard y `SetClipboardData` falle con 1418.
#[repr(C)]
struct DibV5Header {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels: i32,
    y_pels: i32,
    clr_used: u32,
    clr_important: u32,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    alpha_mask: u32,
    cs_type: u32,
    endpoints: [u8; 36],
    gamma_red: u32,
    gamma_green: u32,
    gamma_blue: u32,
    intent: u32,
    profile_data: u32,
    profile_size: u32,
    reserved: u32,
}

fn encode_dibv5(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let w = width as usize;
    let h = height as usize;
    let pixel_bytes = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| crate::ui_lang::msg("Imagen demasiado grande", "Image is too large"))?;
    if rgba.len() != pixel_bytes {
        return Err(crate::ui_lang::msg(
            "El PNG no tiene el tamaño esperado.",
            "The PNG is not the expected size.",
        ));
    }
    let header_size = std::mem::size_of::<DibV5Header>();
    let mut out = vec![0u8; header_size + pixel_bytes];
    let header = DibV5Header {
        size: header_size as u32,
        width: width as i32,
        height: height as i32,
        planes: 1,
        bit_count: 32,
        compression: 3, // BI_BITFIELDS
        size_image: pixel_bytes as u32,
        x_pels: 0,
        y_pels: 0,
        clr_used: 0,
        clr_important: 0,
        red_mask: 0x00ff_0000,
        green_mask: 0x0000_ff00,
        blue_mask: 0x0000_00ff,
        alpha_mask: 0xff00_0000,
        cs_type: 0x7352_4742, // LCS_sRGB
        endpoints: [0; 36],
        gamma_red: 0,
        gamma_green: 0,
        gamma_blue: 0,
        intent: 4, // LCS_GM_IMAGES
        profile_data: 0,
        profile_size: 0,
        reserved: 0,
    };
    // SAFETY: `DibV5Header` es repr(C) de campos POD; el slice cubre exactamente
    // `header_size` bytes.
    out[..header_size].copy_from_slice(unsafe {
        std::slice::from_raw_parts((&header as *const DibV5Header).cast(), header_size)
    });
    let row = w * 4;
    for y in 0..h {
        let src_y = h - 1 - y;
        let src = &rgba[src_y * row..src_y * row + row];
        let dst = &mut out[header_size + y * row..header_size + y * row + row];
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn set_clipboard_image_win(
    png: &[u8],
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    let dib = encode_dibv5(width, height, rgba)?;
    let mut last = String::new();
    for attempt in 0..8u32 {
        match set_clipboard_image_win_once(png, &dib) {
            Ok(()) => return Ok(()),
            Err(error) => last = error,
        }
        std::thread::sleep(std::time::Duration::from_millis(
            10 + 15 * u64::from(attempt),
        ));
    }
    Err(last)
}

#[cfg(windows)]
fn set_clipboard_image_win_once(png: &[u8], dib: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };

    const CF_DIBV5: u32 = 17;

    // windows-sys 0.59 exporta GlobalAlloc/Lock/Unlock pero no GlobalFree.
    extern "system" {
        fn GlobalFree(
            hmem: windows_sys::Win32::Foundation::HGLOBAL,
        ) -> windows_sys::Win32::Foundation::HGLOBAL;
    }

    fn os_err(prefix: &str) -> String {
        let code = unsafe { GetLastError() } as i32;
        format!("{prefix}: {}", std::io::Error::from_raw_os_error(code))
    }

    unsafe fn alloc_hglobal(bytes: &[u8]) -> Result<windows_sys::Win32::Foundation::HGLOBAL, String> {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            return Err(os_err("GlobalAlloc failed"));
        }
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            let err = os_err("GlobalLock failed");
            GlobalFree(handle);
            return Err(err);
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast(), bytes.len());
        GlobalUnlock(handle);
        Ok(handle)
    }

    let mut opened = false;
    for _ in 0..8 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        return Err(os_err("OpenClipboard failed"));
    }

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    let _guard = ClipboardGuard;

    if unsafe { EmptyClipboard() } == 0 {
        return Err(os_err("EmptyClipboard failed"));
    }

    let png_fmt = {
        let wide: Vec<u16> = "PNG".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    };
    if png_fmt == 0 {
        return Err(os_err("RegisterClipboardFormatW(PNG) failed"));
    }

    let png_handle = unsafe { alloc_hglobal(png)? };
    if unsafe { SetClipboardData(png_fmt, png_handle) }.is_null() {
        let err = os_err("SetClipboardData failed with error");
        unsafe {
            GlobalFree(png_handle);
        }
        return Err(err);
    }

    let dib_handle = unsafe { alloc_hglobal(dib)? };
    if unsafe { SetClipboardData(CF_DIBV5, dib_handle) }.is_null() {
        let err = os_err("SetClipboardData failed with error");
        unsafe {
            GlobalFree(dib_handle);
        }
        return Err(err);
    }

    Ok(())
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
        .unwrap_or_else(|| {
            crate::ui_lang::pick(crate::ui_lang::english(), "Captura", "Capture").to_string()
        });
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

/// PNG de capturas o del historial de portapapeles — las dos carpetas de las
/// que la app escribe imágenes.
pub(crate) fn ensure_app_image(state: &AppState, path: &Path) -> Result<PathBuf, String> {
    if let Ok(ok) = ensure_in_dir(&state.dirs.captures_dir(), path) {
        return Ok(ok);
    }
    ensure_in_dir(&state.dirs.clipboard_dir(), path).map_err(|_| {
        crate::ui_lang::msg(
            "Ruta fuera de capturas o portapapeles.",
            "Path is outside the captures or clipboard folders.",
        )
    })
}

/// Abre la imagen con el visor del sistema (el “en grande” del historial).
#[tauri::command]
pub fn open_managed_image(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let target = ensure_app_image(&state, Path::new(&path))?;
    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}

/// Verifica que `path` esté directamente dentro de `dir` (evita leer o borrar
/// archivos arbitrarios vía comando) y devuelve la ruta canónica.
fn ensure_in_dir(dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical_dir = std::fs::canonicalize(dir).map_err(|error| error.to_string())?;
    let resolved = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    if resolved.parent() == Some(canonical_dir.as_path()) {
        Ok(resolved)
    } else {
        Err(crate::ui_lang::msg(
            "Ruta fuera del directorio de capturas.",
            "Path is outside the captures folder.",
        ))
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
        .ok_or_else(|| {
            crate::ui_lang::msg("No se detectaron monitores.", "No monitors were detected.")
        })?;

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
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dibv5_header_pesa_124_bytes() {
        assert_eq!(std::mem::size_of::<DibV5Header>(), 124);
    }

    #[test]
    fn dibv5_de_un_pixel_pasa_rgba_a_bgra() {
        let rgba = [10u8, 20, 30, 255];
        let dib = encode_dibv5(1, 1, &rgba).expect("dib");
        assert_eq!(dib.len(), 124 + 4);
        assert_eq!(&dib[124..], &[30, 20, 10, 255]);
    }

    #[test]
    fn dibv5_voltea_filas() {
        // Arriba rojo, abajo azul. El DIB es bottom-up: primero el azul.
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 255];
        let dib = encode_dibv5(1, 2, &rgba).expect("dib");
        let pixels = &dib[124..];
        assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[4..8], &[0, 0, 255, 255]);
    }

    #[test]
    fn dibv5_rechaza_buffer_corto() {
        assert!(encode_dibv5(2, 2, &[0; 4]).is_err());
    }
}
