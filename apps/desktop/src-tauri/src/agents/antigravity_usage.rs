//! Uso / cupos de la cuenta Antigravity (CLI `agy`).
//!
//! El CLI solo muestra esto en su vista TUI «Models & Quota». Atic consulta el
//! mismo endpoint privado que usa el cliente oficial
//! (`POST cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels`), que
//! devuelve por modelo un `quotaInfo { remainingFraction, resetTime }`.
//!
//! # De dónde sale el token
//!
//! `agy` guarda su OAuth en el keyring nativo; en Windows es el Credential
//! Manager, entrada genérica `gemini:antigravity`, con un JSON
//! `{ token: { access_token, refresh_token, expiry }, auth_method }` (a veces
//! con BOM). No se refresca desde acá: `agy` rota el token cada vez que corre,
//! así que si venció se muestra el motivo y listo — abrir `agy` lo renueva.
//!
//! # Agrupación
//!
//! La API comparte una misma fracción semanal por grupo de modelos (igual que
//! el TUI: «Gemini models» y «Claude and GPT models»). Acá se reducen los
//! modelos a esos dos grupos; el peor porcentaje del grupo manda.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

const MODELS_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
/// UA con la forma del cliente oficial (`antigravity/<ver> <so>/<arch>`).
const USER_AGENT: &str = "antigravity/1.1.23 windows/amd64";
/// Entrada del Credential Manager donde `agy` guarda su OAuth.
#[cfg(windows)]
const CRED_TARGET: &str = "gemini:antigravity";
/// Skew antes de considerar el access token vencido.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// Una ventana semanal de un grupo de modelos, cruda.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupWindow {
    /// `gemini` o `claude+GPT` (así, listo para que la vista lo capitalice).
    pub group: String,
    /// Porcentaje ya consumido, 0..=100.
    pub used_percent: f64,
    /// RFC3339 UTC del reinicio, si la API lo manda.
    pub resets_at: Option<String>,
}

/// Snapshot de cupos de la cuenta, listo para `quota.rs`.
#[derive(Debug, Clone, Serialize)]
pub struct AntigravityAccountUsage {
    pub windows: Vec<GroupWindow>,
    /// Epoch ms en que se obtuvo este snapshot.
    pub fetched_at: i64,
}

/// ¿Hay sesión de `agy` en esta máquina? (credencial en el keyring)
pub fn detected() -> bool {
    read_credential_blob().is_some()
}

/// Consulta el cupo semanal por grupo de modelos.
pub fn fetch_account_usage() -> Result<AntigravityAccountUsage, String> {
    let blob = read_credential_blob()
        .ok_or_else(|| "no hay sesión de Antigravity (credencial ausente)".to_string())?;
    let token = access_token_from_blob(&blob)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;
    let response = client
        .post(MODELS_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .map_err(|e| format!("no se pudo consultar el cupo: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("la API de Antigravity respondió {status}"));
    }
    let body: Value = response
        .json()
        .map_err(|e| format!("respuesta ilegible: {e}"))?;

    let windows = group_windows(&body);
    if windows.is_empty() {
        return Err("la respuesta no trae cupos por modelo".into());
    }
    Ok(AntigravityAccountUsage {
        windows,
        fetched_at: Utc::now().timestamp_millis(),
    })
}

/// El access token del blob del keyring, validando que no esté vencido.
fn access_token_from_blob(blob: &str) -> Result<String, String> {
    // El blob puede venir con BOM (lo escribe Go con el JSON tal cual).
    let root: Value = serde_json::from_str(blob.trim_start_matches('\u{feff}'))
        .map_err(|_| "credencial de Antigravity ilegible".to_string())?;
    let token = root
        .pointer("/token/access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "credencial de Antigravity sin access token".to_string())?;
    if let Some(expiry) = root.pointer("/token/expiry").and_then(Value::as_str) {
        if let Ok(when) = DateTime::parse_from_rfc3339(expiry) {
            let left = when.with_timezone(&Utc).timestamp_millis()
                - Utc::now().timestamp_millis();
            if left < EXPIRY_SKEW_MS {
                return Err(
                    "la sesión de Antigravity venció; abre agy para renovarla".to_string(),
                );
            }
        }
    }
    Ok(token.to_string())
}

/// Reduce `models` (mapa id → modelo) a las ventanas por grupo.
///
/// Los ids internos (`tab_*`, `chat_*`, …) no entran: solo los que empiezan
/// con el nombre de un grupo conocido, que son los que el TUI también agrupa.
fn group_windows(body: &Value) -> Vec<GroupWindow> {
    let Some(models) = body.get("models").and_then(Value::as_object) else {
        return Vec::new();
    };
    // (grupo visible, peor uso, reset de ese peor)
    let mut buckets: Vec<(&'static str, f64, Option<String>)> = Vec::new();
    for (id, model) in models {
        let group = if id.starts_with("gemini") {
            "gemini"
        } else if id.starts_with("claude") || id.starts_with("gpt") {
            "claude+GPT"
        } else {
            continue;
        };
        let Some(quota) = model.get("quotaInfo") else {
            continue;
        };
        let Some(remaining) = quota.get("remainingFraction").and_then(Value::as_f64) else {
            continue;
        };
        let used = ((1.0 - remaining) * 100.0).clamp(0.0, 100.0);
        let resets = quota
            .get("resetTime")
            .and_then(Value::as_str)
            .map(str::to_string);
        match buckets.iter_mut().find(|(g, _, _)| *g == group) {
            Some(bucket) => {
                if used > bucket.1 {
                    bucket.1 = used;
                    bucket.2 = resets;
                }
            }
            None => buckets.push((group, used, resets)),
        }
    }
    buckets
        .into_iter()
        .map(|(group, used_percent, resets_at)| GroupWindow {
            group: group.to_string(),
            used_percent,
            resets_at,
        })
        .collect()
}

/// El blob de la credencial `gemini:antigravity`, como texto.
#[cfg(windows)]
fn read_credential_blob() -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target: Vec<u16> = OsStr::new(CRED_TARGET)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) == 0 || cred.is_null() {
            return None;
        }
        let blob = {
            let c = &*cred;
            if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                None
            } else {
                let bytes =
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
                String::from_utf8(bytes.to_vec()).ok()
            }
        };
        CredFree(cred as *mut _);
        blob
    }
}

#[cfg(not(windows))]
fn read_credential_blob() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn agrupa_modelos_en_gemini_y_claude_gpt() {
        let body = json!({
            "models": {
                "gemini-3.7-flash-tiered": {
                    "quotaInfo": { "remainingFraction": 0.9292, "resetTime": "2026-09-08T14:22:38Z" }
                },
                "gemini-3.1-pro-high": {
                    "quotaInfo": { "remainingFraction": 0.9292, "resetTime": "2026-09-08T14:22:38Z" }
                },
                "claude-opus-4-6-thinking": {
                    "quotaInfo": { "remainingFraction": 1.0, "resetTime": "2026-09-08T14:31:44Z" }
                },
                "gpt-oss-120b-medium": {
                    "quotaInfo": { "remainingFraction": 0.8, "resetTime": "2026-09-08T14:31:44Z" }
                },
                // Ids internos: no son un grupo del TUI y no entran.
                "tab_flash_lite_preview": {
                    "quotaInfo": { "remainingFraction": 0.1, "resetTime": "2026-09-08T14:00:00Z" }
                }
            }
        });
        let mut windows = group_windows(&body);
        windows.sort_by(|a, b| a.group.cmp(&b.group));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].group, "claude+GPT");
        // Manda el peor del grupo: GPT-OSS al 20% le gana a Opus al 0%.
        assert!((windows[0].used_percent - 20.0).abs() < 1e-6);
        assert_eq!(windows[1].group, "gemini");
        assert!((windows[1].used_percent - 7.08).abs() < 1e-6);
        assert_eq!(
            windows[1].resets_at.as_deref(),
            Some("2026-09-08T14:22:38Z")
        );
    }

    #[test]
    fn sin_modelos_no_hay_ventanas() {
        assert!(group_windows(&json!({})).is_empty());
        assert!(group_windows(&json!({ "models": {} })).is_empty());
    }

    #[test]
    fn el_blob_tolera_bom_y_valida_vencimiento() {
        let vivo = format!(
            "\u{feff}{}",
            json!({
                "token": {
                    "access_token": "tok",
                    "expiry": (Utc::now() + chrono::Duration::hours(1)).to_rfc3339()
                },
                "auth_method": "consumer"
            })
        );
        assert_eq!(access_token_from_blob(&vivo).as_deref(), Ok("tok"));

        let vencido = json!({
            "token": { "access_token": "tok", "expiry": "2020-01-01T00:00:00Z" }
        })
        .to_string();
        let err = access_token_from_blob(&vencido).unwrap_err();
        assert!(err.contains("venció"), "{err}");

        assert!(access_token_from_blob("no-json").is_err());
    }
}
