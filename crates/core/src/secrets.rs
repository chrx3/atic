//! Secretos del usuario en el llavero del sistema operativo.
//!
//! Nunca se persisten en `config.json`. La UI solo pregunta si existen y
//! permite establecerlos o borrarlos; no se reenvían al frontend tras guardar.
//!
//! Lo leído queda en memoria: cada llave se lee del llavero una sola vez por
//! proceso. En macOS cada lectura puede pedir permiso, así que la app llama a
//! `preload` al iniciar y después nadie vuelve a preguntar.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use keyring::Entry;

use crate::error::{Error, Result};

const SERVICE: &str = "com.ciat.atic";

/// Claves conocidas en el llavero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Todas las claves conocidas.
    pub const ALL: [SecretKind; 7] = [
        SecretKind::ClaudeApiKey,
        SecretKind::OpenAiApiKey,
        SecretKind::OpenRouterApiKey,
        SecretKind::GroqApiKey,
        SecretKind::MiniMaxApiKey,
        SecretKind::CustomApiKey,
        SecretKind::SmtpPassword,
    ];

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

/// Lo ya leído del llavero (`None`: no hay valor guardado).
static CACHE: Mutex<Option<HashMap<SecretKind, Option<String>>>> = Mutex::new(None);
/// Una lectura del llavero a la vez: dos hilos que piden la misma llave no
/// muestran dos avisos de permiso.
static READING: Mutex<()> = Mutex::new(());
/// El espejo de desarrollo (`enable_dev_mirror`).
static MIRROR: OnceLock<PathBuf> = OnceLock::new();

fn cached(kind: SecretKind) -> Option<Option<String>> {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.as_ref()?.get(&kind).cloned()
}

fn remember(kind: SecretKind, value: Option<String>) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.get_or_insert_with(HashMap::new).insert(kind, value);
}

/// Guarda (o sobrescribe) un secreto.
pub fn set_secret(kind: SecretKind, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return delete_secret(kind);
    }
    entry(kind)?
        .set_password(value)
        .map_err(|err| Error::Secret(err.to_string()))?;
    remember(kind, Some(value.to_string()));
    save_mirror();
    Ok(())
}

/// Lee un secreto, si existe. Del llavero solo la primera vez.
pub fn get_secret(kind: SecretKind) -> Result<Option<String>> {
    if let Some(value) = cached(kind) {
        return Ok(value);
    }
    let _reading = READING.lock().unwrap_or_else(|e| e.into_inner());
    // Otro hilo pudo leerla mientras esperábamos.
    if let Some(value) = cached(kind) {
        return Ok(value);
    }
    let value = match entry(kind)?.get_password() {
        Ok(value) => Some(value),
        Err(keyring::Error::NoEntry) => None,
        Err(err) => return Err(Error::Secret(err.to_string())),
    };
    remember(kind, value.clone());
    Ok(value)
}

/// Lee todas las llaves de una vez (al iniciar la app): los permisos que pida
/// el sistema salen juntos al principio y no al abrir una pantalla después.
/// Con el espejo de desarrollo, las toma de ahí si existe.
pub fn preload() {
    if load_mirror() {
        return;
    }
    for kind in SecretKind::ALL {
        if let Err(err) = get_secret(kind) {
            tracing::warn!("llavero: no se pudo leer {}: {err}", kind.as_str());
        }
    }
    save_mirror();
}

/// Solo para compilaciones de desarrollo en macOS: cada compilación tiene otra
/// firma y el llavero la trata como una app nueva, así que pediría permiso por
/// cada llave en cada build. Con esto, lo leído se copia a `path` (solo lo lee
/// el usuario) y las siguientes lo toman de ahí. Va antes de `preload`.
pub fn enable_dev_mirror(path: PathBuf) {
    let _ = MIRROR.set(path);
}

fn load_mirror() -> bool {
    let Some(path) = MIRROR.get() else {
        return false;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(values) = serde_json::from_str::<HashMap<String, String>>(&raw) else {
        return false;
    };
    for kind in SecretKind::ALL {
        remember(kind, values.get(kind.as_str()).cloned());
    }
    true
}

fn save_mirror() {
    let Some(path) = MIRROR.get() else {
        return;
    };
    let values: HashMap<&str, String> = {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(cache) = cache.as_ref() else {
            return;
        };
        SecretKind::ALL
            .iter()
            .filter_map(|kind| Some((kind.as_str(), cache.get(kind)?.clone()?)))
            .collect()
    };
    let Ok(json) = serde_json::to_string(&values) else {
        return;
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, json.as_bytes()));
    if let Err(err) = written {
        tracing::warn!("llavero: no se pudo guardar el espejo de desarrollo: {err}");
    }
}

/// ¿Hay un valor guardado para esta clave?
pub fn has_secret(kind: SecretKind) -> bool {
    matches!(get_secret(kind), Ok(Some(v)) if !v.is_empty())
}

/// Elimina un secreto del llavero (no falla si no existía).
pub fn delete_secret(kind: SecretKind) -> Result<()> {
    match entry(kind)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            remember(kind, None);
            save_mirror();
            Ok(())
        }
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

/// Identidad del PC en el canal con el celular (`atic-sync`): 32 bytes en hex.
/// Quien la tenga se hace pasar por este PC ante los celulares pareados.
const PHONE_SYNC_KEY: &str = "phone_sync_key";

pub fn get_phone_sync_key() -> Result<Option<String>> {
    match named_entry(PHONE_SYNC_KEY)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(Error::Secret(err.to_string())),
    }
}

pub fn set_phone_sync_key(hex: &str) -> Result<()> {
    named_entry(PHONE_SYNC_KEY)?
        .set_password(hex)
        .map_err(|err| Error::Secret(err.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// El espejo de desarrollo devuelve lo leído sin pasar por el llavero, y
    /// las llaves que no estaban quedan como «sin valor» (no se vuelven a pedir).
    #[test]
    fn dev_mirror_round_trip() {
        let path = std::env::temp_dir().join(format!("atic-secrets-mirror-{}.json", std::process::id()));
        enable_dev_mirror(path.clone());
        remember(SecretKind::GroqApiKey, Some("gsk_prueba".into()));
        save_mirror();
        *CACHE.lock().unwrap() = None;
        assert!(load_mirror());
        assert_eq!(get_secret(SecretKind::GroqApiKey).unwrap(), Some("gsk_prueba".into()));
        assert_eq!(cached(SecretKind::ClaudeApiKey), Some(None));
        let _ = std::fs::remove_file(path);
    }
}
