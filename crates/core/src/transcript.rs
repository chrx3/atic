use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Hablante asociado a un segmento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    /// Pista del micrófono (el usuario).
    Me,
    /// Pista del sistema (los demás participantes).
    Others,
}

impl Speaker {
    /// Etiqueta legible en español para la UI.
    pub fn label(self) -> &'static str {
        match self {
            Speaker::Me => "Yo",
            Speaker::Others => "Los demás",
        }
    }
}

/// Un tramo de transcripción con marca temporal y hablante.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Inicio en milisegundos desde el comienzo de la grabación.
    pub start_ms: i64,
    /// Fin en milisegundos.
    pub end_ms: i64,
    pub speaker: Speaker,
    /// Nombre corregido por el usuario (p. ej. "Ana"). Ausente conserva Yo/Otros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_name: Option<String>,
    pub text: String,
}

impl Segment {
    pub fn speaker_label(&self) -> &str {
        self.speaker_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| self.speaker.label())
    }
}

/// Transcripción completa de una grabación (ambas pistas fusionadas).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    /// Idioma detectado o configurado (código ISO), si se conoce.
    pub language: Option<String>,
    /// Segmentos ordenados por `start_ms`.
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// Ordena los segmentos por tiempo de inicio (in-place).
    pub fn sort(&mut self) {
        self.segments.sort_by_key(|s| s.start_ms);
    }

    /// Texto plano con prefijo de hablante por línea (para copiar o resumir).
    pub fn to_plain_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| format!("{}: {}", s.speaker_label(), s.text.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Persiste la transcripción como JSON.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Carga una transcripción desde JSON, si existe.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}
