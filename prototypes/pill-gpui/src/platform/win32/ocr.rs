//! «Texto» del estante: el OCR que trae Windows (`Windows.Media.Ocr`), en el
//! idioma del perfil del usuario. Sin modelos ni dependencias aparte.

use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Una palabra leída y su caja, en píxeles de la imagen.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Lee el texto de una imagen BGRA. Corre en un hilo de fondo.
pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, String> {
    let result = run(width, height, bgra)?;
    result.Text().map(|text| text.to_string_lossy()).map_err(|error| error.message().to_string())
}

/// Para lo que se guarda (el texto de las imágenes del portapapeles), en un
/// hilo de fondo de baja prioridad. Windows no tiene modo rápido y preciso:
/// es lo mismo que `recognize`.
pub fn recognize_in_background(width: u32, height: u32, bgra: &[u8]) -> Result<String, String> {
    recognize(width, height, bgra)
}

/// Las líneas leídas, cada una con sus palabras y dónde están (para tapar
/// justo lo que hay que tapar). Corre en un hilo de fondo.
pub fn recognize_lines(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<Vec<Word>>, String> {
    let fail = |error: windows::core::Error| error.message().to_string();
    let result = run(width, height, bgra)?;
    let mut lines = Vec::new();
    for line in result.Lines().map_err(fail)? {
        let mut words = Vec::new();
        for word in line.Words().map_err(fail)? {
            let rect = word.BoundingRect().map_err(fail)?;
            words.push(Word {
                text: word.Text().map_err(fail)?.to_string_lossy(),
                x: rect.X,
                y: rect.Y,
                w: rect.Width,
                h: rect.Height,
            });
        }
        lines.push(words);
    }
    Ok(lines)
}

fn run(width: u32, height: u32, bgra: &[u8]) -> Result<windows::Media::Ocr::OcrResult, String> {
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
    engine.RecognizeAsync(&bitmap).map_err(fail)?.get().map_err(fail)
}
