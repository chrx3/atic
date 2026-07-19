use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranscribeError {
    #[error("ruta de modelo inválida (no es UTF-8)")]
    InvalidPath,
    #[error("modelo desconocido: {0}")]
    UnknownModel(String),
    #[error("el modelo '{0}' no está descargado")]
    ModelNotDownloaded(String),
    #[error("error de red: {0}")]
    Http(#[from] reqwest::Error),
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("error leyendo WAV: {0}")]
    Wav(#[from] hound::Error),
    #[error("error decodificando audio: {0}")]
    AudioDecode(String),
    #[error("error de Whisper: {0}")]
    Whisper(#[from] whisper_rs::WhisperError),
    #[error("falta la API key de {0}")]
    MissingApiKey(String),
    #[error("{0}")]
    BadResponse(String),
}

pub type Result<T> = std::result::Result<T, TranscribeError>;
