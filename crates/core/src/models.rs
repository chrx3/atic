use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Estado del ciclo de vida de una grabación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStatus {
    /// Audio capturado, todavía sin transcribir.
    Recorded,
    /// Transcripción en curso.
    Transcribing,
    /// Transcripción lista.
    Transcribed,
    /// Generación de resumen en curso.
    Summarizing,
    /// Resumen listo.
    Summarized,
    /// Falló algún paso; requiere atención del usuario.
    Error,
}

impl RecordingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RecordingStatus::Recorded => "recorded",
            RecordingStatus::Transcribing => "transcribing",
            RecordingStatus::Transcribed => "transcribed",
            RecordingStatus::Summarizing => "summarizing",
            RecordingStatus::Summarized => "summarized",
            RecordingStatus::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Result<Self, Error> {
        Ok(match value {
            "recorded" => RecordingStatus::Recorded,
            "transcribing" => RecordingStatus::Transcribing,
            "transcribed" => RecordingStatus::Transcribed,
            "summarizing" => RecordingStatus::Summarizing,
            "summarized" => RecordingStatus::Summarized,
            "error" => RecordingStatus::Error,
            other => {
                return Err(Error::InvalidValue {
                    field: "status",
                    value: other.to_string(),
                })
            }
        })
    }
}

/// Una sesión de grabación con sus dos pistas (micrófono y sistema).
///
/// `mic_path` y `system_path` son rutas relativas a la carpeta de la
/// grabación (`recordings/<id>/`), por ejemplo `"mic.wav"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub duration_secs: i64,
    pub mic_path: Option<String>,
    pub system_path: Option<String>,
    pub status: RecordingStatus,
}

impl Recording {
    /// Crea una grabación nueva con un id aleatorio y un título por defecto.
    pub fn new(started_at: DateTime<Utc>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let title = format!("Grabación {}", started_at.format("%Y-%m-%d %H:%M"));
        Self {
            id,
            title,
            started_at,
            duration_secs: 0,
            mic_path: None,
            system_path: None,
            status: RecordingStatus::Recorded,
        }
    }
}
