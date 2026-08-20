//! OCR de capturas con Windows.Media.Ocr (WinRT).

use std::path::{Path, PathBuf};

use tauri::State;

use crate::capture;
use crate::state::AppState;

fn ocr_sidecar_path(capture_path: &Path) -> PathBuf {
    // Sidecar junto al PNG; quitar \\?\ para rutas normales en disco.
    let base = strip_verbatim_prefix(capture_path);
    PathBuf::from(format!("{}.ocr.txt", base.to_string_lossy()))
}

/// `canonicalize` en Windows añade `\\?\`; WinRT y algunos I/O lo rechazan.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn ensure_capture_path(state: &AppState, path: &str) -> Result<PathBuf, String> {
    capture::ensure_capture_in_dir(&state.dirs.captures_dir(), Path::new(path))
}

#[cfg(windows)]
fn ocr_image_at(path: &Path) -> Result<String, String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    // No usar StorageFile::GetFileFromPathAsync: falla con rutas `\\?\…`
    // (canonicalize) y produce UNABLE_TO_MASK_PATH / 0x800700A1.
    let bytes = std::fs::read(path).map_err(|e| {
        let shown = strip_verbatim_prefix(path);
        crate::ui_lang::msg(
            &format!("No se pudo leer la captura ({}): {e}", shown.display()),
            &format!("Could not read the capture ({}): {e}", shown.display()),
        )
    })?;
    if bytes.is_empty() {
        return Err(crate::ui_lang::msg(
            "La captura está vacía.",
            "The capture is empty.",
        ));
    }

    let stream = InMemoryRandomAccessStream::new().map_err(|e| e.to_string())?;
    {
        let writer = DataWriter::CreateDataWriter(&stream).map_err(|e| e.to_string())?;
        writer.WriteBytes(&bytes).map_err(|e| e.to_string())?;
        writer
            .StoreAsync()
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        writer
            .FlushAsync()
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        let _ = writer.DetachStream();
    }
    stream.Seek(0).map_err(|e| e.to_string())?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| e.to_string())?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let text = result.Text().map_err(|e| e.to_string())?.to_string();
    Ok(text.trim().to_string())
}

#[cfg(not(windows))]
fn ocr_image_at(_path: &Path) -> Result<String, String> {
    Err(crate::ui_lang::msg(
        "OCR solo disponible en Windows.",
        "OCR is only available on Windows.",
    ))
}

fn write_sidecar(capture_path: &Path, text: &str) -> Result<(), String> {
    let sidecar = ocr_sidecar_path(capture_path);
    std::fs::write(&sidecar, text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ocr_capture_text(state: State<AppState>, path: String) -> Result<String, String> {
    let resolved = ensure_capture_path(&state, &path)?;
    let text = ocr_image_at(&resolved)?;
    if !text.is_empty() {
        let _ = write_sidecar(&resolved, &text);
    }
    Ok(text)
}

#[tauri::command]
pub fn ocr_capture_and_copy(state: State<AppState>, path: String) -> Result<String, String> {
    let resolved = ensure_capture_path(&state, &path)?;
    let text = ocr_image_at(&resolved)?;
    if text.is_empty() {
        return Err(crate::ui_lang::msg(
            "No se detectó texto en la captura.",
            "No text was found in the capture.",
        ));
    }
    write_sidecar(&resolved, &text)?;
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(text.clone())
        .map_err(|e| e.to_string())?;
    Ok(text)
}

#[tauri::command]
pub fn read_capture_ocr_cache(state: State<AppState>, path: String) -> Option<String> {
    let resolved = ensure_capture_path(&state, &path).ok()?;
    let sidecar = ocr_sidecar_path(&resolved);
    let raw = std::fs::read_to_string(&sidecar).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
