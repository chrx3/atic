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

    #[error("el modelo `{model}` no existe en este proveedor. Elegí otro en Ajustes")]
    UnknownModel { model: String },

    #[error("API rechazó la petición ({status}): {body}")]
    Api { status: u16, body: String },
}

impl SummarizeError {
    /// 404 de modelo inexistente vs. el JSON crudo que tapaba el toast.
    pub fn from_http(status: u16, body: &str, model: &str) -> Self {
        let missing_model = status == 404
            && !model.trim().is_empty()
            && (body.contains("The model")
                || body.contains("model_not_found")
                || body.contains("does not exist")
                || body.contains("not_found_error"));
        if missing_model {
            return Self::UnknownModel {
                model: model.to_string(),
            };
        }
        Self::Api {
            status,
            body: body.chars().take(500).collect(),
        }
    }
}

pub type Result<T> = std::result::Result<T, SummarizeError>;
