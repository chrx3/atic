use thiserror::Error;

#[derive(Debug, Error)]
pub enum SummarizeError {
    #[error("backend de resumen desconocido: {0}")]
    UnknownBackend(String),

    #[error("falta la API key del proveedor (guárdala en Ajustes)")]
    MissingApiKey,

    #[error("plantilla desconocida: {0}")]
    UnknownTemplate(String),

    #[error("la transcripción está vacía")]
    EmptyTranscript,

    #[error("Ollama no está disponible en {0}")]
    OllamaUnavailable(String),

    #[error("error de red: {0}")]
    Http(#[from] reqwest::Error),

    #[error("respuesta inválida del modelo: {0}")]
    BadResponse(String),

    #[error("API rechazó la petición ({status}): {body}")]
    Api { status: u16, body: String },
}

pub type Result<T> = std::result::Result<T, SummarizeError>;
