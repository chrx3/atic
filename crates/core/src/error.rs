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

pub type Result<T> = std::result::Result<T, Error>;
