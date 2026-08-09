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

/// Secretos parametrizados (p.ej. passphrase SSH por host).
///
/// El enum fijo no escala a N hosts; estas claves van como
/// `ssh_host_{id}_passphrase` / `ssh_host_{id}_password`.

fn named_entry(name: &str) -> Result<Entry> {
    Entry::new(SERVICE, name).map_err(|err| Error::Secret(err.to_string()))
}

/// Valida el id de host antes de armar la clave del llavero.
pub fn validate_ssh_host_id(host_id: &str) -> Result<()> {
    let ok = !host_id.is_empty()
        && host_id.len() <= 64
        && host_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidValue {
            field: "ssh_host_id",
            value: host_id.to_string(),
        })
    }
}

fn ssh_host_secret_name(host_id: &str, kind: &str) -> Result<String> {
    validate_ssh_host_id(host_id)?;
    match kind {
        "passphrase" | "password" => Ok(format!("ssh_host_{host_id}_{kind}")),
        other => Err(Error::InvalidValue {
            field: "ssh_secret_kind",
            value: other.to_string(),
        }),
    }
}

/// Guarda (o borra si vacío) un secreto SSH por host. Nunca se expone al frontend.
pub fn set_ssh_host_secret(host_id: &str, kind: &str, value: &str) -> Result<()> {
    let name = ssh_host_secret_name(host_id, kind)?;
    let value = value.trim();
    if value.is_empty() {
        return delete_named_secret(&name);
    }
    named_entry(&name)?
        .set_password(value)
        .map_err(|err| Error::Secret(err.to_string()))
}

pub fn get_ssh_host_secret(host_id: &str, kind: &str) -> Result<Option<String>> {
    let name = ssh_host_secret_name(host_id, kind)?;
    match named_entry(&name)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(Error::Secret(err.to_string())),
    }
}

pub fn has_ssh_host_secret(host_id: &str, kind: &str) -> bool {
    matches!(get_ssh_host_secret(host_id, kind), Ok(Some(v)) if !v.is_empty())
}

pub fn delete_ssh_host_secret(host_id: &str, kind: &str) -> Result<()> {
    let name = ssh_host_secret_name(host_id, kind)?;
    delete_named_secret(&name)
}

/// Borra passphrase y password de un host (p.ej. al eliminar el registro).
pub fn delete_all_ssh_host_secrets(host_id: &str) -> Result<()> {
    let _ = delete_ssh_host_secret(host_id, "passphrase");
    let _ = delete_ssh_host_secret(host_id, "password");
    Ok(())
}

fn delete_named_secret(name: &str) -> Result<()> {
    match named_entry(name)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(Error::Secret(err.to_string())),
    }
}
