use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Resumen generado (o editado) a partir de una transcripción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Identificador de plantilla: `executive_minutes`, `action_items`, `followup_email`.
    pub template: String,
    /// Título corto del resumen.
    pub title: String,
    /// Cuerpo en texto plano (editable por el usuario).
    pub body: String,
    /// Asunto sugerido (útil para correos de seguimiento).
    pub subject: Option<String>,
    /// Backend que lo generó (`claude`, `ollama` o `manual`).
    pub backend: String,
    pub created_at: DateTime<Utc>,
}

impl Summary {
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}
