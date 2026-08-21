//! Uso / cupos de la cuenta Claude (suscripción Pro/Max).
//!
//! El CLI solo muestra esto con `/usage` en una sesión interactiva. Atic lee
//! el mismo endpoint OAuth no documentado que usa el CLI
//! (`GET /api/oauth/usage`) con el token de `~/.claude/.credentials.json`.
//!
//! Si el access token está vencido o la API responde 401, se refresca una vez
//! (client_id público de Claude Code) y se escribe el token rotado de vuelta
//! preservando el resto del JSON — Claude Code comparte ese archivo.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::skills::config_dir;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_URL_PRIMARY: &str = "https://platform.claude.com/v1/oauth/token";
const TOKEN_URL_LEGACY: &str = "https://console.anthropic.com/v1/oauth/token";
const FALLBACK_CLI_VERSION: &str = "2.1.223";
/// Skew antes de considerar el access token vencido.
const EXPIRY_SKEW_MS: i64 = 60_000;
/// Evita golpear la API en cada tick del poll del modal (~10–15 s).
const CACHE_TTL: Duration = Duration::from_secs(12);

/// Una ventana de cupo (sesión 5 h, semanal, etc.).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Porcentaje ya consumido, 0..=100.
    pub utilization: f64,
    /// RFC3339 UTC, si la API lo manda.
    pub resets_at: Option<String>,
}

/// Uso extra de pago (créditos opcionales).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
    pub currency: Option<String>,
}

/// Snapshot de cupos de la cuenta, listo para la UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountUsage {
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub seven_day_opus: Option<UsageWindow>,
    pub seven_day_sonnet: Option<UsageWindow>,
    pub extra_usage: Option<ExtraUsage>,
    /// Plan legible (`max`, `pro`, `max 20x`, …) si figura en credenciales.
    pub plan: Option<String>,
    /// Epoch ms en que se obtuvo este snapshot.
    pub fetched_at: i64,
}

struct Cache {
    at: Instant,
    value: Result<ClaudeAccountUsage, String>,
}

static CACHE: OnceLock<Mutex<Option<Cache>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<Cache>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Consulta el uso de la cuenta. Cachea ~12 s para el poll del modal.
pub fn fetch_account_usage() -> Result<ClaudeAccountUsage, String> {
    {
        let guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < CACHE_TTL {
                return c.value.clone();
            }
        }
    }

    let result = fetch_account_usage_uncached();
    if let Ok(mut guard) = cache().lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            value: result.clone(),
        });
    }
    result
}

fn fetch_account_usage_uncached() -> Result<ClaudeAccountUsage, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;

    let creds_path = credentials_path();
    let (mut token, plan) = load_access_token(&client, creds_path.as_deref())?;

    match call_usage(&client, &token) {
        Ok(mut usage) => {
            usage.plan = plan.or(usage.plan);
            Ok(usage)
        }
        Err(UsageErr::Unauthorized) => {
            let path = creds_path.ok_or_else(|| {
                "no hay credenciales OAuth de Claude. Abre Claude Code y ejecuta `claude auth login`."
                    .to_string()
            })?;
            token = refresh_and_persist(&client, &path)?;
            let mut usage = call_usage(&client, &token).map_err(|e| e.to_string())?;
            usage.plan = plan.or(usage.plan);
            Ok(usage)
        }
        Err(UsageErr::Other(msg)) => Err(msg),
    }
}

enum UsageErr {
    Unauthorized,
    Other(String),
}

impl UsageErr {
    fn to_string(self) -> String {
        match self {
            UsageErr::Unauthorized => {
                "sesión Claude vencida. Ejecuta `claude auth login`.".to_string()
            }
            UsageErr::Other(m) => m,
        }
    }
}

fn call_usage(
    client: &reqwest::blocking::Client,
    token: &str,
) -> Result<ClaudeAccountUsage, UsageErr> {
    let resp = client
        .get(USAGE_URL)
        .header(reqwest::header::USER_AGENT, user_agent())
        .header("x-app", "cli")
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", OAUTH_BETA)
        .header("anthropic-dangerous-direct-browser-access", "true")
        .bearer_auth(token)
        .send()
        .map_err(|e| UsageErr::Other(format!("no se pudo consultar el uso: {e}")))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(UsageErr::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(UsageErr::Other(
            "la API de uso está limitando consultas. Prueba de nuevo en un minuto.".to_string(),
        ));
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        let hint = body.chars().take(160).collect::<String>();
        return Err(UsageErr::Other(format!(
            "la API de uso respondió {status}{}",
            if hint.is_empty() {
                String::new()
            } else {
                format!(": {hint}")
            }
        )));
    }

    let body = resp
        .text()
        .map_err(|e| UsageErr::Other(format!("respuesta de uso ilegible: {e}")))?;
    parse_usage_body(&body).map_err(UsageErr::Other)
}

#[derive(Debug, Deserialize)]
struct ApiWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiExtra {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    five_hour: Option<ApiWindow>,
    seven_day: Option<ApiWindow>,
    seven_day_opus: Option<ApiWindow>,
    seven_day_sonnet: Option<ApiWindow>,
    extra_usage: Option<ApiExtra>,
}

fn parse_usage_body(body: &str) -> Result<ClaudeAccountUsage, String> {
    let raw: ApiUsage =
        serde_json::from_str(body).map_err(|e| format!("uso: JSON inesperado ({e})"))?;
    let map_win = |w: Option<ApiWindow>| -> Option<UsageWindow> {
        let w = w?;
        Some(UsageWindow {
            utilization: w.utilization.unwrap_or(0.0).clamp(0.0, 100.0),
            resets_at: w.resets_at.filter(|s| !s.is_empty()),
        })
    };
    let extra = raw.extra_usage.map(|e| ExtraUsage {
        is_enabled: e.is_enabled.unwrap_or(false),
        monthly_limit: e.monthly_limit,
        used_credits: e.used_credits,
        utilization: e.utilization,
        currency: e.currency,
    });

    let usage = ClaudeAccountUsage {
        five_hour: map_win(raw.five_hour),
        seven_day: map_win(raw.seven_day),
        seven_day_opus: map_win(raw.seven_day_opus),
        seven_day_sonnet: map_win(raw.seven_day_sonnet),
        extra_usage: extra,
        plan: None,
        fetched_at: chrono::Utc::now().timestamp_millis(),
    };

    if usage.five_hour.is_none()
        && usage.seven_day.is_none()
        && usage.seven_day_opus.is_none()
        && usage.seven_day_sonnet.is_none()
        && usage.extra_usage.is_none()
    {
        return Err(
            "la cuenta no reporta cupos de suscripción (¿modo API key?). `/usage` solo aplica a Pro/Max."
                .to_string(),
        );
    }
    Ok(usage)
}

fn credentials_path() -> Option<PathBuf> {
    Some(config_dir()?.join(".credentials.json"))
}

fn load_access_token(
    client: &reqwest::blocking::Client,
    path: Option<&Path>,
) -> Result<(String, Option<String>), String> {
    if let Ok(env_token) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        let t = env_token.trim().to_string();
        if !t.is_empty() {
            return Ok((t, None));
        }
    }

    let path = path.ok_or_else(|| {
        "no se encontró ~/.claude. ¿Claude Code está instalado y con sesión iniciada?".to_string()
    })?;
    if !path.is_file() {
        return Err(
            "no hay credenciales OAuth de Claude. Ejecuta `claude auth login` en una terminal."
                .to_string(),
        );
    }

    let text = fs::read_to_string(path)
        .map_err(|e| format!("no se pudieron leer las credenciales de Claude: {e}"))?;
    let root: Value = serde_json::from_str(&text)
        .map_err(|_| "credenciales de Claude ilegibles (JSON inválido)".to_string())?;
    let oauth = root.get("claudeAiOauth").ok_or_else(|| {
        "Claude está en modo API key (sin OAuth). El uso de cupo Pro/Max no está disponible."
            .to_string()
    })?;

    let tier_field = |key: &str| -> Option<String> {
        oauth.get(key).and_then(|v| {
            v.as_str().map(str::to_string).or_else(|| {
                v.as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
    };
    // Preferimos el tier fino (`rateLimitTier` / plural); si no, `subscriptionType`.
    let plan = tier_field(concat!("rateLimit", "Tier"))
        .or_else(|| tier_field(concat!("rateLimit", "Tiers")))
        .or_else(|| {
            oauth
                .get("subscriptionType")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|s| prettify_plan(&s));

    let access = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "credenciales Claude sin access token".to_string())?
        .to_string();

    let expires_at = oauth.get("expiresAt").and_then(as_epoch_ms);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let expiring = expires_at
        .map(|e| e <= now_ms + EXPIRY_SKEW_MS)
        .unwrap_or(false);

    if expiring {
        match refresh_and_persist(client, path) {
            Ok(t) => return Ok((t, plan)),
            Err(e) => {
                // Si el refresh falla pero el token aún no venció del todo, seguir.
                if expires_at.map(|e| e > now_ms).unwrap_or(false) {
                    tracing::warn!("claude usage: refresh falló, se reusa el token: {e}");
                } else {
                    return Err(e);
                }
            }
        }
    }

    Ok((access, plan))
}

fn as_epoch_ms(v: &Value) -> Option<i64> {
    let n = v.as_i64().or_else(|| v.as_u64().map(|u| u as i64))?;
    // ≥ 1e11 → ms; si no, segundos.
    Some(if n >= 100_000_000_000 { n } else { n * 1000 })
}

fn prettify_plan(raw: &str) -> String {
    raw.trim_start_matches("default_")
        .trim_start_matches("claude_")
        .replace('_', " ")
}

fn refresh_and_persist(client: &reqwest::blocking::Client, path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("no se pudieron leer las credenciales: {e}"))?;
    let mut root: Value = serde_json::from_str(&text)
        .map_err(|_| "credenciales ilegibles al refrescar".to_string())?;
    let oauth = root
        .get("claudeAiOauth")
        .cloned()
        .ok_or_else(|| "sin bloque OAuth para refrescar".to_string())?;
    let refresh = oauth
        .get("refreshToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "no hay refresh token. Ejecuta `claude auth login`.".to_string())?
        .to_string();

    let scopes: Vec<String> = oauth
        .get("scopes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh,
        "client_id": CLIENT_ID,
    });
    if !scopes.is_empty() {
        body["scope"] = json!(scopes.join(" "));
    }

    let mtime_before = fs::metadata(path).and_then(|m| m.modified()).ok();

    let mut last_err = String::from("refresh falló");
    let mut parsed: Option<Value> = None;
    for (i, url) in [TOKEN_URL_PRIMARY, TOKEN_URL_LEGACY].iter().enumerate() {
        let resp = client
            .post(*url)
            .header(reqwest::header::USER_AGENT, user_agent())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .map_err(|e| format!("refresh OAuth: {e}"))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if status.is_success() {
            parsed = Some(
                serde_json::from_str(&text)
                    .map_err(|e| format!("respuesta de refresh inválida: {e}"))?,
            );
            break;
        }
        let moved = status.as_u16() == 404 || status.as_u16() == 405;
        last_err = format!("refresh OAuth → {status}");
        if moved && i + 1 < 2 {
            continue;
        }
        if status.as_u16() == 400 || status.as_u16() == 401 {
            return Err("la sesión de Claude expiró. Ejecuta `claude auth login`.".to_string());
        }
        return Err(last_err);
    }

    let resp = parsed.ok_or(last_err)?;
    let access = resp
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "refresh sin access_token".to_string())?
        .to_string();
    let expires_in = resp
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(28_800);
    let expires_ms = chrono::Utc::now().timestamp_millis() + expires_in * 1000;

    // Si Claude Code rotó el archivo mientras pedíamos refresh, no pisar.
    let mtime_after = fs::metadata(path).and_then(|m| m.modified()).ok();
    if mtime_changed(mtime_before, mtime_after) {
        // Releer y usar el access token nuevo si ya está.
        if let Ok((t, _)) = load_access_token_no_refresh(path) {
            return Ok(t);
        }
        return Err("credenciales cambiaron durante el refresh; reintentá".to_string());
    }

    let oauth_obj = root
        .get_mut("claudeAiOauth")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "credenciales sin objeto claudeAiOauth".to_string())?;
    oauth_obj.insert("accessToken".into(), json!(access));
    oauth_obj.insert("expiresAt".into(), json!(expires_ms));
    if let Some(r) = resp.get("refresh_token").and_then(Value::as_str) {
        if !r.is_empty() {
            oauth_obj.insert("refreshToken".into(), json!(r));
        }
    }
    if let Some(sc) = resp.get("scope").and_then(Value::as_str) {
        let list: Vec<&str> = sc.split_whitespace().collect();
        if !list.is_empty() {
            oauth_obj.insert("scopes".into(), json!(list));
        }
    }

    atomic_write_json(path, &root)?;
    Ok(access)
}

fn load_access_token_no_refresh(path: &Path) -> Result<(String, Option<String>), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let root: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let oauth = root.get("claudeAiOauth").ok_or("sin oauth")?;
    let access = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("sin token")?
        .to_string();
    Ok((access, None))
}

fn mtime_changed(before: Option<SystemTime>, after: Option<SystemTime>) -> bool {
    match (before, after) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

fn atomic_write_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "ruta de credenciales sin directorio".to_string())?;
    let tmp = parent.join(format!(".credentials.{}.tmp", std::process::id()));
    let body =
        serde_json::to_string_pretty(value).map_err(|e| format!("serializar credenciales: {e}"))?;
    fs::write(&tmp, body.as_bytes()).map_err(|e| format!("escribir temp credenciales: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("actualizar credenciales: {e}")
    })?;
    Ok(())
}

fn user_agent() -> String {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        let ver = detect_claude_version().unwrap_or_else(|| FALLBACK_CLI_VERSION.to_string());
        format!("claude-cli/{ver} (external, cli)")
    })
    .clone()
}

fn detect_claude_version() -> Option<String> {
    let (prog, prefix) = super::exe::launcher("claude")?;
    let mut cmd = std::process::Command::new(prog);
    for a in prefix {
        cmd.arg(a);
    }
    let out = cmd.arg("-v").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // "2.1.223 (Claude Code)" → "2.1.223"
    text.split_whitespace()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_usage_typical() {
        let body = r#"{
            "five_hour": { "utilization": 12.5, "resets_at": "2026-08-06T22:00:00Z" },
            "seven_day": { "utilization": 40.0, "resets_at": "2026-08-10T12:00:00Z" },
            "seven_day_opus": null,
            "seven_day_sonnet": { "utilization": 5.0, "resets_at": "2026-08-10T12:00:00Z" },
            "extra_usage": { "is_enabled": false }
        }"#;
        let u = parse_usage_body(body).unwrap();
        assert_eq!(u.five_hour.as_ref().unwrap().utilization, 12.5);
        assert!(u.seven_day_opus.is_none());
        assert!(!u.extra_usage.unwrap().is_enabled);
    }

    #[test]
    fn epoch_ms_detection() {
        assert_eq!(
            as_epoch_ms(&json!(1_759_700_000_000_i64)),
            Some(1_759_700_000_000)
        );
        assert_eq!(
            as_epoch_ms(&json!(1_759_700_000_i64)),
            Some(1_759_700_000_000)
        );
    }

    #[test]
    fn prettify() {
        assert_eq!(prettify_plan("default_claude_max_20x"), "max 20x");
        assert_eq!(prettify_plan("pro"), "pro");
    }
}
