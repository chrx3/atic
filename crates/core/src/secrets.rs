//! Secretos del usuario en el llavero del sistema operativo.
//!
//! Nunca se persisten en `config.json`. La UI solo pregunta si existen y
//! permite establecerlos o borrarlos; no se reenvían al frontend tras guardar.

use keyring::Entry;

use crate::error::{Error, Result};

const SERVICE: &str = "com.ciat.atic";

/// Claves conocidas en el llavero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    ClaudeApiKey,
    OpenAiApiKey,
    OpenRouterApiKey,
    GroqApiKey,
    MiniMaxApiKey,
    CustomApiKey,
    SmtpPassword,
}

impl SecretKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SecretKind::ClaudeApiKey => "claude_api_key",
            SecretKind::OpenAiApiKey => "openai_api_key",
            SecretKind::OpenRouterApiKey => "openrouter_api_key",
            SecretKind::GroqApiKey => "groq_api_key",
            SecretKind::MiniMaxApiKey => "minimax_api_key",
            SecretKind::CustomApiKey => "custom_api_key",
            SecretKind::SmtpPassword => "smtp_password",
        }
    }

    /// Id de proveedor de resumen asociado, si aplica.
    pub fn provider_id(self) -> Option<&'static str> {
        match self {
            SecretKind::ClaudeApiKey => Some("claude"),
            SecretKind::OpenAiApiKey => Some("openai"),
            SecretKind::OpenRouterApiKey => Some("openrouter"),
            SecretKind::GroqApiKey => Some("groq"),
            SecretKind::MiniMaxApiKey => Some("minimax"),
            SecretKind::CustomApiKey => Some("custom"),
            SecretKind::SmtpPassword => None,
        }
    }

    /// Resuelve el secreto de API key para un id de proveedor de resumen.
    pub fn for_summary_provider(provider_id: &str) -> Option<Self> {
        match provider_id {
            "claude" => Some(SecretKind::ClaudeApiKey),
            "openai" => Some(SecretKind::OpenAiApiKey),
            "openrouter" => Some(SecretKind::OpenRouterApiKey),
            "groq" => Some(SecretKind::GroqApiKey),
            "minimax" => Some(SecretKind::MiniMaxApiKey),
            "custom" => Some(SecretKind::CustomApiKey),
            _ => None,
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "claude_api_key" => SecretKind::ClaudeApiKey,
            "openai_api_key" => SecretKind::OpenAiApiKey,
            "openrouter_api_key" => SecretKind::OpenRouterApiKey,
            "groq_api_key" => SecretKind::GroqApiKey,
            "minimax_api_key" => SecretKind::MiniMaxApiKey,
            "custom_api_key" => SecretKind::CustomApiKey,
            "smtp_password" => SecretKind::SmtpPassword,
            other => {
                return Err(Error::InvalidValue {
                    field: "secret",
                    value: other.to_string(),
                })
            }
        })
    }

    /// Secretos de API key de proveedores de resumen.
    pub fn summary_api_keys() -> &'static [SecretKind] {
        &[
            SecretKind::ClaudeApiKey,
            SecretKind::OpenAiApiKey,
            SecretKind::OpenRouterApiKey,
            SecretKind::GroqApiKey,
            SecretKind::MiniMaxApiKey,
            SecretKind::CustomApiKey,
        ]
    }
}

fn entry(kind: SecretKind) -> Result<Entry> {
    Entry::new(SERVICE, kind.as_str()).map_err(|err| Error::Secret(err.to_string()))
}

/// Guarda (o sobrescribe) un secreto.
pub fn set_secret(kind: SecretKind, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return delete_secret(kind);
    }
    entry(kind)?
        .set_password(value)
        .map_err(|err| Error::Secret(err.to_string()))
}

/// Lee un secreto, si existe.
pub fn get_secret(kind: SecretKind) -> Result<Option<String>> {
    match entry(kind)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(Error::Secret(err.to_string())),
    }
}

/// ¿Hay un valor guardado para esta clave?
pub fn has_secret(kind: SecretKind) -> bool {
    matches!(get_secret(kind), Ok(Some(v)) if !v.is_empty())
}

/// Elimina un secreto del llavero (no falla si no existía).
pub fn delete_secret(kind: SecretKind) -> Result<()> {
    match entry(kind)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(Error::Secret(err.to_string())),
    }
}
