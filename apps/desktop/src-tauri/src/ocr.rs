//! OCR de capturas con Windows.Media.Ocr (WinRT).

use std::path::{Path, PathBuf};

use tauri::State;

use crate::capture;
use crate::state::AppState;

fn ocr_sidecar_path(capture_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.ocr.txt", capture_path.to_string_lossy()))
}

fn ensure_capture_path(state: &AppState, path: &str) -> Result<PathBuf, String> {
    capture::ensure_capture_in_dir(&state.dirs.captures_dir(), Path::new(path))
}

#[cfg(windows)]
fn ocr_image_at(path: &Path) -> Result<String, String> {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    let path_str = path.to_string_lossy();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path_str.as_ref()))
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
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
    let text = result
        .Text()
        .map_err(|e| e.to_string())?
        .to_string();
    Ok(text.trim().to_string())
}

#[cfg(not(windows))]
fn ocr_image_at(_path: &Path) -> Result<String, String> {
    Err("OCR solo disponible en Windows.".into())
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
        return Err("No se detectó texto en la captura.".into());
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
