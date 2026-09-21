//! Cupos de la cuenta Grok (CLI `grok login`).
//!
//! El TUI los muestra con `/usage`. Atic lee el mismo endpoint que usa ese
//! comando: `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`,
//! autenticado con el bearer de `~/.grok/auth.json`. Si el access token
//! venció, se refresca con el `refresh_token` OIDC y se escribe de vuelta.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

const BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const EXPIRY_SKEW_SECS: i64 = 60;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokUsageWindow {
    pub kind: String,
    pub percent: f64,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokAccountUsage {
    pub windows: Vec<GrokUsageWindow>,
    pub plan: Option<String>,
    pub fetched_at: i64,
}

pub fn auth_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("GROK_HOME") {
        let home = home.trim();
        if !home.is_empty() {
            return Some(PathBuf::from(home).join("auth.json"));
        }
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".grok").join("auth.json"))
}

pub fn detected() -> bool {
    let Some(path) = auth_path() else {
        return false;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    session_from_auth(&root).is_some()
}

#[derive(Debug, Clone)]
struct GrokSession {
    key: String,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    oidc_issuer: Option<String>,
    oidc_client_id: Option<String>,
    slot: String,
}

pub fn fetch_account_usage() -> Result<GrokAccountUsage, String> {
    let path = auth_path().ok_or_else(|| "no se encontró ~/.grok".to_string())?;
    let text =
        fs::read_to_string(&path).map_err(|e| format!("no se pudo leer ~/.grok/auth.json: {e}"))?;
    let mut root: Value =
        serde_json::from_str(&text).map_err(|_| "auth.json de Grok ilegible".to_string())?;
    let mut session =
        session_from_auth(&root).ok_or_else(|| "Grok no tiene sesión en auth.json".to_string())?;

    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;

    if session.needs_refresh() {
        refresh_session(&client, &mut session)?;
        persist_session(&path, &mut root, &session)?;
    }

    match call_billing(&client, &session.key) {
        Ok(usage) => Ok(usage),
        Err(err) if err.contains("401") || err.contains("403") => {
            refresh_session(&client, &mut session)?;
            persist_session(&path, &mut root, &session)?;
            call_billing(&client, &session.key)
        }
        Err(err) => Err(err),
    }
}

impl GrokSession {
    fn needs_refresh(&self) -> bool {
        let Some(when) = self.expires_at else {
            return false;
        };
        when.timestamp() <= Utc::now().timestamp() + EXPIRY_SKEW_SECS
    }
}

/// Elige la entrada OIDC más nueva. Las claves son URLs `https://auth.x.ai::uuid`.
fn session_from_auth(root: &Value) -> Option<GrokSession> {
    let obj = root.as_object()?;
    let mut best: Option<(String, &Value, i64)> = None;
    for (slot, entry) in obj {
        if !entry.is_object() {
            continue;
        }
        let stamp = entry
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp())
            .or_else(|| {
                entry
                    .get("create_time")
                    .and_then(Value::as_str)
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp())
            })
            .unwrap_or(0);
        if best.as_ref().is_none_or(|(_, _, s)| stamp >= *s) {
            best = Some((slot.clone(), entry, stamp));
        }
    }
    let (slot, entry, _) = best?;
    let key = entry
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    Some(GrokSession {
        key,
        refresh_token: entry
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        expires_at: entry
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc)),
        oidc_issuer: entry
            .get("oidc_issuer")
            .and_then(Value::as_str)
            .map(str::to_string),
        oidc_client_id: entry
            .get("oidc_client_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        slot,
    })
}

fn refresh_session(
    client: &reqwest::blocking::Client,
    session: &mut GrokSession,
) -> Result<(), String> {
    let refresh = session
        .refresh_token
        .as_deref()
        .ok_or_else(|| "la sesión de Grok venció; ejecuta `grok login`".to_string())?;
    let issuer = session
        .oidc_issuer
        .as_deref()
        .unwrap_or("https://auth.x.ai")
        .trim_end_matches('/');
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh.to_string()),
    ];
    if let Some(id) = session.oidc_client_id.as_deref() {
        form.push(("client_id", id.to_string()));
    }
    let resp = client
        .post(format!("{issuer}/oauth2/token"))
        .form(&form)
        .send()
        .map_err(|e| format!("no se pudo renovar Grok: {e}"))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err("la sesión de Grok venció; ejecuta `grok login`".into());
    }
    let body: Value =
        serde_json::from_str(&text).map_err(|e| format!("refresh de Grok ilegible: {e}"))?;
    let access = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "refresh de Grok sin access_token".to_string())?;
    session.key = access.to_string();
    if let Some(r) = body.get("refresh_token").and_then(Value::as_str) {
        if !r.is_empty() {
            session.refresh_token = Some(r.to_string());
        }
    }
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    session.expires_at = Some(Utc::now() + chrono::Duration::seconds(expires_in));
    Ok(())
}

fn persist_session(
    path: &std::path::Path,
    root: &mut Value,
    session: &GrokSession,
) -> Result<(), String> {
    let entry = root
        .get_mut(&session.slot)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "auth.json de Grok cambió durante el refresh".to_string())?;
    entry.insert("key".into(), json!(session.key));
    if let Some(r) = session.refresh_token.as_deref() {
        entry.insert("refresh_token".into(), json!(r));
    }
    if let Some(when) = session.expires_at {
        entry.insert("expires_at".into(), json!(when.to_rfc3339()));
    }
    let body = serde_json::to_string_pretty(root).map_err(|e| format!("serializar Grok: {e}"))?;
    fs::write(path, body).map_err(|e| format!("escribir ~/.grok/auth.json: {e}"))?;
    Ok(())
}

fn call_billing(
    client: &reqwest::blocking::Client,
    token: &str,
) -> Result<GrokAccountUsage, String> {
    let resp = client
        .get(BILLING_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("x-xai-token-auth", "xai-grok-cli")
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("no se pudo consultar el cupo de Grok: {e}"))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(format!("Grok respondió {status}"));
    }
    if !status.is_success() {
        return Err(format!("la API de Grok respondió {status}"));
    }
    parse_billing_body(&text)
}

fn parse_billing_body(text: &str) -> Result<GrokAccountUsage, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|_| "respuesta de Grok ilegible".to_string())?;
    let config = root.get("config").unwrap_or(&root);
    let percent = config
        .get("creditUsagePercent")
        .and_then(Value::as_f64)
        .ok_or_else(|| "Grok no reporta cupo semanal".to_string())?;
    let resets = config
        .pointer("/currentPeriod/end")
        .or_else(|| config.get("billingPeriodEnd"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(GrokAccountUsage {
        windows: vec![GrokUsageWindow {
            kind: "weekly".into(),
            percent: percent.clamp(0.0, 100.0),
            resets_at: resets,
        }],
        plan: None,
        fetched_at: Utc::now().timestamp_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_el_cupo_semanal() {
        let body = r#"{
            "config": {
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "end": "2026-09-27T10:46:52.856Z"
                },
                "creditUsagePercent": 46.0
            }
        }"#;
        let usage = parse_billing_body(body).unwrap();
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].kind, "weekly");
        assert!((usage.windows[0].percent - 46.0).abs() < 1e-6);
        assert_eq!(
            usage.windows[0].resets_at.as_deref(),
            Some("2026-09-27T10:46:52.856Z")
        );
    }

    #[test]
    fn elige_la_entrada_oidc_mas_nueva() {
        let root = serde_json::json!({
            "https://auth.x.ai::old": {
                "key": "old-key",
                "expires_at": "2020-01-01T00:00:00Z"
            },
            "https://auth.x.ai::new": {
                "key": "new-key",
                "refresh_token": "rt",
                "expires_at": "2030-01-01T00:00:00Z",
                "oidc_issuer": "https://auth.x.ai"
            }
        });
        let session = session_from_auth(&root).unwrap();
        assert_eq!(session.key, "new-key");
        assert_eq!(session.slot, "https://auth.x.ai::new");
        assert_eq!(session.refresh_token.as_deref(), Some("rt"));
    }
}
