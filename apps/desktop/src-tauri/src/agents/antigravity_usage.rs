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
//! con BOM). En macOS es el llavero `gemini` / `antigravity`, a menudo
//! envuelto en `go-keyring-base64:`. Si el access token venció, se refresca
//! con Google OAuth y se escribe de vuelta: si no, el hover solo vería Agy
//! cuando el CLI acaba de correr.
//!
//! # Agrupación
//!
//! La API comparte una misma fracción semanal por grupo de modelos (igual que
//! el TUI: «Gemini models» y «Claude and GPT models»). Acá se reducen los
//! modelos a esos dos grupos; el peor porcentaje del grupo manda.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

const MODELS_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Client id público del CLI Gemini / Antigravity. El `id_token` trae el mismo.
const FALLBACK_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
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

static LAST_GOOD: OnceLock<Mutex<Option<AntigravityAccountUsage>>> = OnceLock::new();

fn last_good_slot() -> &'static Mutex<Option<AntigravityAccountUsage>> {
    LAST_GOOD.get_or_init(|| Mutex::new(None))
}

fn remember(value: &AntigravityAccountUsage) {
    if let Ok(mut guard) = last_good_slot().lock() {
        *guard = Some(value.clone());
    }
}

/// Último snapshot bueno. Si la API falla, la pill sigue pintando estas barras.
pub fn last_good() -> Option<AntigravityAccountUsage> {
    last_good_slot().lock().ok()?.clone()
}

/// ¿Hay sesión de `agy` en esta máquina? (credencial en el keyring)
pub fn detected() -> bool {
    read_credential_blob().is_some()
}

/// Consulta el cupo semanal por grupo de modelos.
pub fn fetch_account_usage() -> Result<AntigravityAccountUsage, String> {
    let result = fetch_account_usage_uncached();
    if let Ok(ref value) = result {
        remember(value);
    }
    result
}

fn fetch_account_usage_uncached() -> Result<AntigravityAccountUsage, String> {
    let raw = read_credential_raw()
        .ok_or_else(|| "no hay sesión de Antigravity (credencial ausente)".to_string())?;
    let blob = decode_keyring_blob(&raw)
        .ok_or_else(|| "credencial de Antigravity ilegible".to_string())?;
    let mut root = parse_credential_json(&blob)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;

    let mut token = match live_access_token(&root) {
        Ok(token) => token,
        Err(_) => refresh_google_token(&client, &mut root, &raw)?,
    };

    match call_models(&client, &token) {
        Ok(usage) => Ok(usage),
        Err(err) if err.contains("401") || err.contains("403") => {
            token = refresh_google_token(&client, &mut root, &raw)?;
            call_models(&client, &token)
        }
        Err(err) => Err(err),
    }
}

fn call_models(
    client: &reqwest::blocking::Client,
    token: &str,
) -> Result<AntigravityAccountUsage, String> {
    let response = client
        .post(MODELS_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .map_err(|e| format!("no se pudo consultar el cupo: {e}"))?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(format!("la API de Antigravity respondió {status}"));
    }
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

fn parse_credential_json(blob: &str) -> Result<Value, String> {
    serde_json::from_str(blob.trim_start_matches('\u{feff}'))
        .map_err(|_| "credencial de Antigravity ilegible".to_string())
}

/// El access token del blob del keyring, validando que no esté vencido.
fn access_token_from_blob(blob: &str) -> Result<String, String> {
    live_access_token(&parse_credential_json(blob)?)
}

fn live_access_token(root: &Value) -> Result<String, String> {
    let token = root
        .pointer("/token/access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "credencial de Antigravity sin access token".to_string())?;
    if token_expired(root) {
        return Err("la sesión de Antigravity venció; abre agy para renovarla".to_string());
    }
    Ok(token.to_string())
}

fn token_expired(root: &Value) -> bool {
    let Some(expiry) = root.pointer("/token/expiry").and_then(Value::as_str) else {
        return false;
    };
    let Ok(when) = DateTime::parse_from_rfc3339(expiry) else {
        return false;
    };
    when.with_timezone(&Utc).timestamp_millis() - Utc::now().timestamp_millis() < EXPIRY_SKEW_MS
}

fn refresh_google_token(
    client: &reqwest::blocking::Client,
    root: &mut Value,
    original_raw: &str,
) -> Result<String, String> {
    let refresh = root
        .pointer("/token/refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "la sesión de Antigravity venció; abre agy para renovarla".to_string())?
        .to_string();
    let client_id = google_client_id(root);
    let resp = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .map_err(|e| format!("no se pudo renovar Antigravity: {e}"))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err("la sesión de Antigravity venció; abre agy para renovarla".into());
    }
    let body: Value =
        serde_json::from_str(&text).map_err(|e| format!("refresh de Antigravity ilegible: {e}"))?;
    let access = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "refresh de Antigravity sin access_token".to_string())?
        .to_string();
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let expiry = (Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339();

    {
        let token_obj = root
            .pointer_mut("/token")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "credencial de Antigravity sin objeto token".to_string())?;
        token_obj.insert("access_token".into(), Value::String(access.clone()));
        token_obj.insert("expiry".into(), Value::String(expiry));
        if let Some(r) = body.get("refresh_token").and_then(Value::as_str) {
            if !r.is_empty() {
                token_obj.insert("refresh_token".into(), Value::String(r.to_string()));
            }
        }
    }
    if let Some(id_token) = body.get("id_token").and_then(Value::as_str) {
        if !id_token.is_empty() {
            if let Some(obj) = root.as_object_mut() {
                obj.insert("id_token".into(), Value::String(id_token.to_string()));
            }
        }
    }
    let _ = persist_credential_blob(root, original_raw);
    Ok(access)
}

fn google_client_id(root: &Value) -> String {
    root.get("id_token")
        .and_then(Value::as_str)
        .and_then(azp_from_jwt)
        .unwrap_or_else(|| FALLBACK_CLIENT_ID.to_string())
}

fn azp_from_jwt(jwt: &str) -> Option<String> {
    use base64::Engine;
    let payload = jwt.split('.').nth(1)?;
    let mut padded = payload.replace('-', "+").replace('_', "/");
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(padded)
        .ok()?;
    let body: Value = serde_json::from_slice(&bytes).ok()?;
    body.get("azp")
        .or_else(|| body.get("aud"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn persist_credential_blob(root: &Value, original_raw: &str) -> Result<(), String> {
    let json = serde_json::to_string(root).map_err(|e| format!("serializar Antigravity: {e}"))?;
    write_credential_raw(&encode_keyring_blob(&json, original_raw))
}

fn encode_keyring_blob(json: &str, original_raw: &str) -> String {
    let raw = original_raw.trim().trim_start_matches('\u{feff}');
    if raw.starts_with("go-keyring-base64:") {
        use base64::Engine;
        format!(
            "go-keyring-base64:{}",
            base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
        )
    } else {
        json.to_string()
    }
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

/// El blob de la credencial, como JSON de `agy`.
///
/// En Windows es Credential Manager (`gemini:antigravity`). En macOS, el
/// llavero `gemini` / `antigravity`. Go a veces lo envuelve en
/// `go-keyring-base64:`.
fn read_credential_blob() -> Option<String> {
    let raw = read_credential_raw()?;
    decode_keyring_blob(&raw)
}

/// Quita el envoltorio de `github.com/zalando/go-keyring` si viene.
fn decode_keyring_blob(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_start_matches('\u{feff}');
    const PREFIX: &str = "go-keyring-base64:";
    if let Some(rest) = raw.strip_prefix(PREFIX) {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(rest.trim())
            .ok()?;
        String::from_utf8(bytes).ok()
    } else if raw.starts_with('{') {
        Some(raw.to_string())
    } else {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
        let text = String::from_utf8(bytes).ok()?;
        text.trim().starts_with('{').then_some(text)
    }
}

/// El blob de la credencial `gemini:antigravity`, como texto crudo.
#[cfg(windows)]
fn read_credential_raw() -> Option<String> {
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

#[cfg(target_os = "macos")]
fn read_credential_raw() -> Option<String> {
    super::os_keychain::generic_password(
        super::os_keychain::AGY_KEYCHAIN_SERVICE,
        super::os_keychain::AGY_KEYCHAIN_ACCOUNT,
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
fn read_credential_raw() -> Option<String> {
    None
}

fn write_credential_raw(secret: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        super::os_keychain::set_generic_password(
            super::os_keychain::AGY_KEYCHAIN_SERVICE,
            super::os_keychain::AGY_KEYCHAIN_ACCOUNT,
            secret,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = secret;
        Ok(())
    }
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

    #[test]
    fn go_keyring_base64_se_desenvuelve() {
        use base64::Engine;
        let json = r#"{"token":{"access_token":"tok"}}"#;
        let wrapped = format!(
            "go-keyring-base64:{}",
            base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
        );
        assert_eq!(decode_keyring_blob(&wrapped).as_deref(), Some(json));
        assert_eq!(decode_keyring_blob(json).as_deref(), Some(json));
        assert_eq!(
            decode_keyring_blob(&base64::engine::general_purpose::STANDARD.encode(json.as_bytes()))
                .as_deref(),
            Some(json)
        );
        assert!(decode_keyring_blob("secreto-opaco").is_none());
        assert_eq!(encode_keyring_blob(json, &wrapped), wrapped);
    }

    #[test]
    fn el_client_id_sale_del_id_token() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"azp":"abc.apps.googleusercontent.com","aud":"other"}"#);
        let jwt = format!("eyJhbGciOiJub25lIn0.{payload}.sig");
        assert_eq!(
            azp_from_jwt(&jwt).as_deref(),
            Some("abc.apps.googleusercontent.com")
        );
        let root = json!({ "id_token": jwt });
        assert_eq!(google_client_id(&root), "abc.apps.googleusercontent.com");
        assert_eq!(google_client_id(&json!({})), FALLBACK_CLIENT_ID);
    }
}
