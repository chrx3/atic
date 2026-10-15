//! «Texto» del estante y tapar datos. Pendiente en macOS (irá con Vision,
//! `VNRecognizeTextRequest`).

/// Una palabra leída y su caja, en píxeles de la imagen.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const PENDING: &str = "el reconocimiento de texto todavía no está en macOS";

pub fn recognize(_width: u32, _height: u32, _bgra: &[u8]) -> Result<String, String> {
    Err(PENDING.into())
}

pub fn recognize_lines(_width: u32, _height: u32, _bgra: &[u8]) -> Result<Vec<Vec<Word>>, String> {
    Err(PENDING.into())
}
