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
    capture::ensure_app_image(state, Path::new(path))
}

/// Windows.Media.Ocr falla en recortes de una línea o de pocos píxeles de
/// alto: el motor espera texto ~12 px y margen alrededor. Agranda y rellena
/// con el color del borde; si el decode falla, se manda el PNG original.
fn prepare_ocr_png(bytes: &[u8]) -> Vec<u8> {
    use image::imageops::{self, FilterType};
    use image::{DynamicImage, GenericImageView, ImageFormat, RgbImage};
    use std::io::Cursor;

    let Ok(img) = image::load_from_memory(bytes) else {
        return bytes.to_vec();
    };
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return bytes.to_vec();
    }

    const TARGET_MIN: u32 = 160;
    const TARGET_H: u32 = 72;
    const MAX_DIM: u32 = 2400;
    const PAD: u32 = 24;

    let mut scale = 1.0_f32;
    if w.min(h) < TARGET_MIN {
        scale = scale.max(TARGET_MIN as f32 / w.min(h) as f32);
    }
    if h < TARGET_H {
        scale = scale.max(TARGET_H as f32 / h as f32);
    }
    scale = scale.clamp(1.0, 4.0);

    let mut nw = ((w as f32) * scale).round().max(1.0) as u32;
    let mut nh = ((h as f32) * scale).round().max(1.0) as u32;
    let longest = nw.max(nh);
    if longest > MAX_DIM {
        let cap = MAX_DIM as f32 / longest as f32;
        nw = ((nw as f32) * cap).round().max(1.0) as u32;
        nh = ((nh as f32) * cap).round().max(1.0) as u32;
    }

    let scaled = if nw != w || nh != h {
        img.resize_exact(nw, nh, FilterType::Lanczos3)
    } else {
        img
    };

    let rgb = scaled.to_rgb8();
    let fill = *rgb.get_pixel(0, 0);
    let mut canvas: RgbImage =
        image::ImageBuffer::from_pixel(rgb.width() + PAD * 2, rgb.height() + PAD * 2, fill);
    imageops::replace(&mut canvas, &rgb, i64::from(PAD), i64::from(PAD));

    let mut out = Cursor::new(Vec::new());
    if DynamicImage::ImageRgb8(canvas)
        .write_to(&mut out, ImageFormat::Png)
        .is_err()
    {
        return bytes.to_vec();
    }
    out.into_inner()
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
    let bytes = prepare_ocr_png(&bytes);

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
