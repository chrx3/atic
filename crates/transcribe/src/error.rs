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

impl TranscribeError {
    pub fn to_ui(&self, en: bool) -> String {
        match self {
            Self::InvalidPath => {
                if en {
                    "Invalid model path (not UTF-8)".into()
                } else {
                    self.to_string()
                }
            }
            Self::UnknownModel(id) => {
                if en {
                    format!("Unknown model: {id}")
                } else {
                    self.to_string()
                }
            }
            Self::ModelNotDownloaded(id) => {
                if en {
                    format!("Model '{id}' is not downloaded")
                } else {
                    self.to_string()
                }
            }
            Self::Http(err) => {
                if en {
                    format!("Network error: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::Io(err) => {
                if en {
                    format!("I/O error: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::Wav(err) => {
                if en {
                    format!("Error reading WAV: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::AudioDecode(err) => {
                if en {
                    format!("Error decoding audio: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::Whisper(err) => {
                if en {
                    format!("Whisper error: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::MissingApiKey(provider) => {
                if en {
                    format!("Missing {provider} API key")
                } else {
                    self.to_string()
                }
            }
            Self::BadResponse(msg) => msg.clone(),
        }
    }
}

pub type Result<T> = std::result::Result<T, TranscribeError>;
