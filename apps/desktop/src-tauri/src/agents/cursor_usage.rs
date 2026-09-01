//! Cupo del período en Cursor.
//!
//! # De dónde sale el porcentaje
//!
//! Sumar `totalCents` de los eventos **no** es un cupo: es el valor de lista
//! de todo lo incluido (y el bonus de los proveedores). En Pro+ eso pasa los
//! mil dólares con un plan de US$60, y pintar esa cifra como «consumo» miente.
//!
//! El dashboard lee `POST /api/dashboard/get-current-period-usage`:
//! `autoPercentUsed` y `apiPercentUsed` son los mismos % que Cursor muestra
//! («You've used 81% of your included total usage»). `includedSpend` / `limit`
//! es el techo pagado (US$70 en Pro+); si faltan los % se usa esa fracción.
//! El on-demand —si está encendido— es `totalSpend − included − bonus`.
//!
//! # De dónde sale la credencial
//!
//! Del IDE, no del CLI: `~/.cursor/cli-config.json` guarda el `authId` pero no
//! el token. El único token local está en el `state.vscdb` del IDE, que es un
//! SQLite de VS Code (tabla `ItemTable`, clave/valor).
//!
//! La API web exige `Origin`: sin ese header responde 403 «Invalid origin for
//! state-changing request», aunque la cookie sea buena.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Months, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;

const PERIOD_URL: &str = "https://cursor.com/api/dashboard/get-current-period-usage";
const SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const AGG_URL: &str = "https://cursor.com/api/dashboard/get-aggregated-usage-events";
const USAGE_URL: &str = "https://cursor.com/api/usage";
const ORIGIN: &str = "https://cursor.com";
// 8 s y no 20: el fetch hace 3-4 requests EN SERIE y el hover de la pill
// espera al agente más lento; con Cursor caído, 20 s por request dejaban
// «Leyendo…» un minuto entero.
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorUsageWindow {
    /// `auto`, `api` o `monthly`. Viaja crudo: la vista pone el idioma.
    pub kind: String,
    /// Porcentaje ya consumido, 0..=100.
    pub used_percent: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorAccountUsage {
    /// On-demand (centavos). Cero si el plan no cobra extra.
    pub spend_cents: f64,
    /// `pro`, `pro_plus`, `ultra`, … tal como lo guarda el IDE.
    pub plan: Option<String>,
    /// RFC3339 UTC. Inicio del período de facturación, si se conoce.
    pub period_start: Option<String>,
    /// RFC3339 UTC. Corte del período.
    pub period_end: Option<String>,
    pub windows: Vec<CursorUsageWindow>,
    pub fetched_at: i64,
}

/// `%APPDATA%\Cursor\User\globalStorage\state.vscdb` y sus gemelos de otras
/// plataformas. Atic hoy solo corre en Windows, pero la ruta de macOS/Linux
/// cuesta dos líneas y evita que esto sea lo que rompa el día que se porte.
pub fn state_db_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"));

    Some(
        base?
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    )
}

pub fn detected() -> bool {
    state_db_path().map(|p| p.is_file()).unwrap_or(false)
}

struct Creds {
    token: String,
    auth_id: String,
    plan: Option<String>,
}

/// Abre el SQLite del IDE sin estorbarlo.
///
/// Solo lectura, y si Cursor lo tiene tomado (WAL con `-shm` bloqueado) se
/// trabaja sobre una copia. Perder los últimos escritos no importa: lo que se
/// busca es un token que cambia cada varias horas, no el estado de una sesión.
fn open_state_db(path: &Path) -> Result<Connection, String> {
    if let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        return Ok(conn);
    }
    let tmp = std::env::temp_dir().join(format!("atic-cursor-{}.vscdb", std::process::id()));
    std::fs::copy(path, &tmp).map_err(|e| format!("no se pudo leer el estado de Cursor: {e}"))?;
    let conn = Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("no se pudo abrir el estado de Cursor: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(conn)
}

fn load_creds() -> Result<Creds, String> {
    let path = state_db_path().ok_or_else(|| "no se encontró la carpeta de Cursor".to_string())?;
    if !path.is_file() {
        return Err("Cursor no está instalado en esta máquina.".to_string());
    }
    let conn = open_state_db(&path)?;
    let get = |key: &str| -> Option<String> {
        conn.query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
    };

    let token = get("cursorAuth/accessToken")
        .ok_or_else(|| "Cursor no tiene sesión iniciada.".to_string())?;
    let auth_id = get("cursorAuth/stripeMembershipAuthId")
        .ok_or_else(|| "Cursor no reporta la cuenta de la sesión.".to_string())?;
    Ok(Creds {
        token,
        auth_id,
        plan: get("cursorAuth/stripeMembershipType"),
    })
}

/// `WorkosCursorSessionToken=<authId url-encoded>%3A%3A<jwt>`.
///
/// El separador es `::` ya percent-encoded: así lo escribe el navegador y así
/// lo espera el backend.
fn session_cookie(creds: &Creds) -> String {
    format!(
        "WorkosCursorSessionToken={}%3A%3A{}",
        urlencode(&creds.auth_id),
        creds.token
    )
}

fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn fetch_account_usage() -> Result<CursorAccountUsage, String> {
    let creds = load_creds()?;
    let cookie = session_cookie(&creds);
    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;

    let fetched_at = Utc::now().timestamp_millis();
    let mut last_err: String;
    match post_json(&client, &cookie, PERIOD_URL, serde_json::json!({})) {
        Ok(body) => {
            if let Some(parsed) = parse_period_body(&body) {
                return Ok(parsed.into_usage(creds.plan, fetched_at));
            }
            last_err = "Cursor respondió sin datos de cupo".into();
        }
        Err(err) => last_err = err,
    }

    match get_text(&client, &cookie, SUMMARY_URL) {
        Ok(body) => {
            if let Some(parsed) = parse_summary_body(&body) {
                return Ok(parsed.into_usage(creds.plan, fetched_at));
            }
        }
        Err(err) => last_err = err,
    }

    // Último recurso: la suma de eventos, que no es un cupo. Solo si no hubo %.
    let spend_cents = match post_json(&client, &cookie, AGG_URL, serde_json::json!({})) {
        Ok(body) => parse_spend_cents(&body)?,
        Err(_) => return Err(last_err),
    };
    let period_start = fetch_period_start(&client, &cookie, &creds.auth_id);
    Ok(CursorAccountUsage {
        spend_cents,
        plan: creds.plan,
        period_end: period_start.as_deref().and_then(plus_one_month),
        period_start,
        windows: Vec::new(),
        fetched_at,
    })
}

struct ParsedPeriod {
    windows: Vec<CursorUsageWindow>,
    spend_cents: f64,
    period_start: Option<String>,
    period_end: Option<String>,
}

impl ParsedPeriod {
    fn into_usage(self, plan: Option<String>, fetched_at: i64) -> CursorAccountUsage {
        CursorAccountUsage {
            spend_cents: self.spend_cents,
            plan,
            period_start: self.period_start,
            period_end: self.period_end,
            windows: self.windows,
            fetched_at,
        }
    }
}

fn auth_error(status: reqwest::StatusCode) -> Option<String> {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        Some("la sesión de Cursor expiró. Vuelve a iniciar sesión en el IDE.".to_string())
    } else {
        None
    }
}

fn post_json(
    client: &reqwest::blocking::Client,
    cookie: &str,
    url: &str,
    body: Value,
) -> Result<String, String> {
    let resp = client
        .post(url)
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ORIGIN, ORIGIN)
        .header(reqwest::header::REFERER, "https://cursor.com/dashboard")
        .json(&body)
        .send()
        .map_err(|e| format!("no se pudo consultar el uso de Cursor: {e}"))?;
    let status = resp.status();
    if let Some(err) = auth_error(status) {
        return Err(err);
    }
    if !status.is_success() {
        return Err(format!("la API de Cursor respondió {status}"));
    }
    resp.text()
        .map_err(|e| format!("respuesta de Cursor ilegible: {e}"))
}

fn get_text(
    client: &reqwest::blocking::Client,
    cookie: &str,
    url: &str,
) -> Result<String, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ORIGIN, ORIGIN)
        .send()
        .map_err(|e| format!("no se pudo consultar el uso de Cursor: {e}"))?;
    let status = resp.status();
    if let Some(err) = auth_error(status) {
        return Err(err);
    }
    if !status.is_success() {
        return Err(format!("la API de Cursor respondió {status}"));
    }
    resp.text()
        .map_err(|e| format!("respuesta de Cursor ilegible: {e}"))
}

fn parse_epoch_ms(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s
            .parse::<i64>()
            .ok()
            .or_else(|| rfc3339_millis(s)),
        _ => None,
    }
}

fn rfc3339_millis(stamp: &str) -> Option<i64> {
    Some(
        DateTime::parse_from_rfc3339(stamp)
            .ok()?
            .with_timezone(&Utc)
            .timestamp_millis(),
    )
}

fn millis_to_rfc3339(ms: i64) -> Option<String> {
    DateTime::from_timestamp_millis(ms).map(|dt| dt.to_rfc3339())
}

fn windows_from_plan(plan: &Value) -> Vec<CursorUsageWindow> {
    let mut windows = Vec::new();
    let push = |kind: &str, percent: Option<f64>, out: &mut Vec<CursorUsageWindow>| {
        if let Some(percent) = percent {
            out.push(CursorUsageWindow {
                kind: kind.to_string(),
                used_percent: percent,
            });
        }
    };
    push("auto", plan.get("autoPercentUsed").and_then(Value::as_f64), &mut windows);
    push("api", plan.get("apiPercentUsed").and_then(Value::as_f64), &mut windows);
    if windows.is_empty() {
        let included = plan
            .get("includedSpend")
            .and_then(Value::as_f64)
            .or_else(|| {
                plan.get("breakdown")
                    .and_then(|b| b.get("included"))
                    .and_then(Value::as_f64)
            });
        let limit = plan.get("limit").and_then(Value::as_f64);
        if let (Some(included), Some(limit)) = (included, limit) {
            if limit > 0.0 {
                windows.push(CursorUsageWindow {
                    kind: "monthly".to_string(),
                    used_percent: (included / limit) * 100.0,
                });
            }
        }
    }
    windows
}

fn on_demand_cents(root: &Value, plan: &Value) -> f64 {
    if let Some(od) = root
        .get("individualUsage")
        .and_then(|u| u.get("onDemand"))
    {
        if od.get("enabled").and_then(Value::as_bool) == Some(true) {
            return od.get("used").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        }
    }
    let included = plan.get("includedSpend").and_then(Value::as_f64).unwrap_or(0.0);
    let bonus = plan.get("bonusSpend").and_then(Value::as_f64).unwrap_or(0.0);
    let total = plan.get("totalSpend").and_then(Value::as_f64).unwrap_or(0.0);
    (total - included - bonus).max(0.0)
}

fn parse_period_body(body: &str) -> Option<ParsedPeriod> {
    let root: Value = serde_json::from_str(body).ok()?;
    let plan = root.get("planUsage")?;
    let windows = windows_from_plan(plan);
    if windows.is_empty() && on_demand_cents(&root, plan) <= 0.0 {
        return None;
    }
    let start_ms = parse_epoch_ms(root.get("billingCycleStart"));
    let end_ms = parse_epoch_ms(root.get("billingCycleEnd"));
    Some(ParsedPeriod {
        spend_cents: on_demand_cents(&root, plan),
        windows,
        period_start: start_ms.and_then(millis_to_rfc3339),
        period_end: end_ms.and_then(millis_to_rfc3339),
    })
}

fn parse_summary_body(body: &str) -> Option<ParsedPeriod> {
    let root: Value = serde_json::from_str(body).ok()?;
    let plan = root.get("individualUsage")?.get("plan")?;
    let windows = windows_from_plan(plan);
    if windows.is_empty() && on_demand_cents(&root, plan) <= 0.0 {
        return None;
    }
    Some(ParsedPeriod {
        spend_cents: on_demand_cents(&root, plan),
        windows,
        period_start: root
            .get("billingCycleStart")
            .and_then(Value::as_str)
            .map(str::to_string),
        period_end: root
            .get("billingCycleEnd")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn fetch_period_start(
    client: &reqwest::blocking::Client,
    cookie: &str,
    auth_id: &str,
) -> Option<String> {
    let resp = client
        .get(format!("{USAGE_URL}?user={}", urlencode(auth_id)))
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ORIGIN, ORIGIN)
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let root: Value = serde_json::from_str(&resp.text().ok()?).ok()?;
    root.get("startOfMonth")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn plus_one_month(start: &str) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(start)
        .ok()?
        .with_timezone(&Utc);
    Some(parsed.checked_add_months(Months::new(1))?.to_rfc3339())
}

/// Suma `totalCents` de todos los modelos del período.
///
/// La API ignora los parámetros de fecha —probado: con rango y sin rango
/// devuelve exactamente lo mismo— así que el corte lo pone ella, no nosotros.
fn parse_spend_cents(body: &str) -> Result<f64, String> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| format!("Cursor: JSON inesperado ({e})"))?;
    let rows = root
        .get("aggregations")
        .and_then(Value::as_array)
        .ok_or_else(|| "Cursor respondió sin datos de consumo".to_string())?;
    Ok(rows
        .iter()
        .filter_map(|r| r.get("totalCents").and_then(Value::as_f64))
        .sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suma_el_consumo_de_todos_los_modelos() {
        let body = r#"{"aggregations":[
            {"modelIntent":"cursor-grok-4.6-high","totalCents":17173.557945,"tier":2},
            {"modelIntent":"sand-default","totalCents":100.5,"tier":2},
            {"modelIntent":"sin-costo"}
        ]}"#;
        let cents = parse_spend_cents(body).unwrap();
        assert!((cents - 17274.057945).abs() < 1e-6);
    }

    #[test]
    fn periodo_vacio_es_cero_y_no_error() {
        assert_eq!(parse_spend_cents(r#"{"aggregations":[]}"#).unwrap(), 0.0);
    }

    #[test]
    fn respuesta_sin_agregaciones_es_error() {
        assert!(parse_spend_cents(r#"{"error":"nope"}"#).is_err());
    }

    #[test]
    fn el_corte_es_un_mes_despues_del_inicio() {
        let end = plus_one_month("2026-08-01T18:44:34.000Z").unwrap();
        assert!(end.starts_with("2026-09-01T18:44:34"), "fue {end}");
        assert!(plus_one_month("no-es-una-fecha").is_none());
    }

    #[test]
    fn la_cookie_escapa_la_barra_del_auth_id() {
        let creds = Creds {
            token: "jwt.tok.en".into(),
            auth_id: "auth0|user_01ABC".into(),
            plan: None,
        };
        assert_eq!(
            session_cookie(&creds),
            "WorkosCursorSessionToken=auth0%7Cuser_01ABC%3A%3Ajwt.tok.en"
        );
    }

    const PERIOD_JSON: &str = r#"{
        "billingCycleStart": 1785609874000,
        "billingCycleEnd": 1788288274000,
        "planUsage": {
            "totalSpend": 106357,
            "includedSpend": 7000,
            "bonusSpend": 99357,
            "limit": 7000,
            "autoPercentUsed": 80.5825,
            "apiPercentUsed": 87.8,
            "totalPercentUsed": 81.1885
        }
    }"#;

    #[test]
    fn el_periodo_trae_auto_y_api_no_el_valor_de_lista() {
        let parsed = parse_period_body(PERIOD_JSON).expect("cupo");
        assert_eq!(parsed.windows.len(), 2);
        assert_eq!(parsed.windows[0].kind, "auto");
        assert!((parsed.windows[0].used_percent - 80.5825).abs() < 1e-6);
        assert_eq!(parsed.windows[1].kind, "api");
        assert!((parsed.windows[1].used_percent - 87.8).abs() < 1e-6);
        // included + bonus = total → on-demand cero. El 106357 no es un cupo.
        assert_eq!(parsed.spend_cents, 0.0);
        assert!(
            parsed
                .period_end
                .as_deref()
                .unwrap()
                .starts_with("2026-09-01T18:44:34")
        );
    }

    #[test]
    fn sin_porcentajes_cae_al_incluido_sobre_el_techo() {
        let body = r#"{
            "billingCycleEnd": 1788288274000,
            "planUsage": { "includedSpend": 3500, "limit": 7000, "totalSpend": 3500, "bonusSpend": 0 }
        }"#;
        let parsed = parse_period_body(body).expect("cupo");
        assert_eq!(parsed.windows.len(), 1);
        assert_eq!(parsed.windows[0].kind, "monthly");
        assert!((parsed.windows[0].used_percent - 50.0).abs() < 1e-6);
    }

    #[test]
    fn el_resumen_lee_on_demand_solo_si_esta_encendido() {
        let body = r#"{
            "billingCycleStart": "2026-08-01T18:44:34.000Z",
            "billingCycleEnd": "2026-09-01T18:44:34.000Z",
            "individualUsage": {
                "plan": { "autoPercentUsed": 10.0, "apiPercentUsed": 20.0, "limit": 7000, "used": 7000 },
                "onDemand": { "enabled": true, "used": 1234 }
            }
        }"#;
        let parsed = parse_summary_body(body).expect("cupo");
        assert_eq!(parsed.spend_cents, 1234.0);
        assert_eq!(parsed.windows[0].kind, "auto");
        assert_eq!(parsed.period_end.as_deref(), Some("2026-09-01T18:44:34.000Z"));
    }
}
