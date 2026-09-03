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

    #[error("el modelo `{model}` no existe en este proveedor. Elige otro en Ajustes")]
    UnknownModel { model: String },

    #[error("la transcripción es demasiado larga para el cupo de este modelo")]
    RequestTooLarge {
        limit: Option<u32>,
        retry_secs: Option<u64>,
    },

    #[error("el proveedor pidió esperar {secs}s (límite de ritmo)")]
    RateLimited { secs: u64 },

    #[error("API rechazó la petición ({status}): {body}")]
    Api { status: u16, body: String },
}

impl SummarizeError {
    pub fn is_too_large(&self) -> bool {
        matches!(self, Self::RequestTooLarge { .. })
    }

    pub fn tpm_limit(&self) -> Option<u32> {
        match self {
            Self::RequestTooLarge { limit, .. } => *limit,
            _ => None,
        }
    }

    pub fn retry_secs(&self) -> Option<u64> {
        match self {
            Self::RequestTooLarge { retry_secs, .. } => *retry_secs,
            Self::RateLimited { secs } => Some(*secs),
            _ => None,
        }
    }

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
        if crate::chunk::is_payload_too_large(status, body) {
            return Self::RequestTooLarge {
                limit: crate::chunk::parse_tpm_limit(body),
                retry_secs: crate::chunk::parse_retry_secs(body),
            };
        }
        if status == 429 {
            let secs = crate::chunk::parse_retry_secs(body).unwrap_or(15);
            return Self::RateLimited { secs };
        }
        Self::Api {
            status,
            body: body.chars().take(500).collect(),
        }
    }

    pub fn to_ui(&self, en: bool) -> String {
        match self {
            Self::UnknownBackend(id) => {
                if en {
                    format!("Unknown summary backend: {id}")
                } else {
                    self.to_string()
                }
            }
            Self::MissingApiKey => {
                if en {
                    "The provider API key is missing (save it in Settings)".into()
                } else {
                    self.to_string()
                }
            }
            Self::UnknownTemplate(id) => {
                if en {
                    format!("Unknown template: {id}")
                } else {
                    self.to_string()
                }
            }
            Self::EmptyTranscript => {
                if en {
                    "The transcript is empty".into()
                } else {
                    self.to_string()
                }
            }
            Self::OllamaUnavailable(url) => {
                if en {
                    format!("Ollama is not available at {url}")
                } else {
                    self.to_string()
                }
            }
            Self::Http(err) => format!("{err}"),
            Self::BadResponse(msg) => msg.clone(),
            Self::UnknownModel { model } => {
                if en {
                    format!("Model `{model}` does not exist for this provider. Pick another in Settings")
                } else {
                    self.to_string()
                }
            }
            Self::RequestTooLarge { .. } => {
                if en {
                    "This transcript is too long for this model's quota. Pick another model or provider in Settings.".into()
                } else {
                    "La transcripción es demasiado larga para el cupo de este modelo. Elige otro modelo o proveedor en Ajustes.".into()
                }
            }
            Self::RateLimited { secs } => {
                if en {
                    format!("The provider asked to wait {secs}s (rate limit)")
                } else {
                    self.to_string()
                }
            }
            Self::Api { status, body } => {
                if en {
                    format!("The API rejected the request ({status}): {body}")
                } else {
                    self.to_string()
                }
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, SummarizeError>;
