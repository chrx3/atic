//! «Texto» del estante: el OCR que trae Windows (`Windows.Media.Ocr`), en el
//! idioma del perfil del usuario. Sin modelos ni dependencias aparte.

use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Lee el texto de una imagen BGRA. Corre en un hilo de fondo.
pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, String> {
    let fail = |error: windows::core::Error| error.message().to_string();
    // El hilo de fondo puede no tener WinRT iniciado; si ya lo estaba, da igual.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(fail)?;
    let max = OcrEngine::MaxImageDimension().map_err(fail)?;
    if width > max || height > max {
        return Err(format!("la imagen pasa de {max} px por lado"));
    }
    let writer = DataWriter::new().map_err(fail)?;
    writer.WriteBytes(bgra).map_err(fail)?;
    let buffer = writer.DetachBuffer().map_err(fail)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(fail)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(fail)?.get().map_err(fail)?;
    Ok(result.Text().map_err(fail)?.to_string_lossy())
}
