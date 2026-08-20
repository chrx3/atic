use thiserror::Error;

/// Errores del dominio y la capa de almacenamiento.
#[derive(Debug, Error)]
pub enum Error {
    #[error("no se pudo determinar el directorio de datos de la aplicación")]
    NoDataDir,

    #[error("error de base de datos: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("error de serialización: {0}")]
    Json(#[from] serde_json::Error),

    #[error("valor inválido para '{field}': {value}")]
    InvalidValue { field: &'static str, value: String },

    #[error("grabación no encontrada: {0}")]
    RecordingNotFound(String),

    #[error("error del llavero: {0}")]
    Secret(String),
}

impl Error {
    pub fn to_ui(&self, en: bool) -> String {
        match self {
            Self::NoDataDir => {
                if en {
                    "Could not determine the app data directory".into()
                } else {
                    self.to_string()
                }
            }
            Self::Db(err) => {
                if en {
                    format!("Database error: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::Io(err) => format!("{err}"),
            Self::Json(err) => {
                if en {
                    format!("Serialization error: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::InvalidValue { field, value } => {
                if en {
                    format!("Invalid value for '{field}': {value}")
                } else {
                    self.to_string()
                }
            }
            Self::RecordingNotFound(_) => {
                if en {
                    "Recording not found.".into()
                } else {
                    "Grabación no encontrada.".into()
                }
            }
            Self::Secret(err) => {
                if en {
                    format!("Keychain error: {err}")
                } else {
                    self.to_string()
                }
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
