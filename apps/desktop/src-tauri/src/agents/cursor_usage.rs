//! Consumo del período en Cursor.
//!
//! # Por qué esto no es un porcentaje
//!
//! Los otros tres agentes publican «llevas X% de tu ventana». Cursor no: en los
//! planes Pro/Pro+ el freno son *rate limits* que no expone ningún endpoint. Lo
//! que sí se puede leer es cuánto valor consumiste en el período —los eventos
//! vienen marcados `USAGE_EVENT_KIND_INCLUDED_IN_PRO_PLUS`, o sea incluidos— y
//! contra qué plan. Medido en la máquina de desarrollo: US$1213 de consumo en
//! un plan de US$60, que es la prueba de que ese número **no** es un cupo. Por
//! eso el tipo de acá no tiene `percent` y la vista lo pinta sin barra: una
//! barra insinuaría un techo que no existe.
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

const AGG_URL: &str = "https://cursor.com/api/dashboard/get-aggregated-usage-events";
const USAGE_URL: &str = "https://cursor.com/api/usage";
const ORIGIN: &str = "https://cursor.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorAccountUsage {
    /// Consumo del período en centavos de dólar. No es un cupo: ver el módulo.
    pub spend_cents: f64,
    /// `pro`, `pro_plus`, `ultra`, … tal como lo guarda el IDE.
    pub plan: Option<String>,
    /// RFC3339 UTC. Inicio del período de facturación, si la API lo manda.
    pub period_start: Option<String>,
    /// Un mes después de `period_start`, que es cuando se reinicia la cuenta.
    pub period_end: Option<String>,
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

    let agg = client
        .post(AGG_URL)
        .header(reqwest::header::COOKIE, &cookie)
        .header(reqwest::header::ORIGIN, ORIGIN)
        .header(reqwest::header::REFERER, "https://cursor.com/dashboard")
        .json(&serde_json::json!({}))
        .send()
        .map_err(|e| format!("no se pudo consultar el uso de Cursor: {e}"))?;

    let status = agg.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("la sesión de Cursor expiró. Vuelve a iniciar sesión en el IDE.".to_string());
    }
    if !status.is_success() {
        return Err(format!("la API de Cursor respondió {status}"));
    }
    let body = agg
        .text()
        .map_err(|e| format!("respuesta de Cursor ilegible: {e}"))?;
    let spend_cents = parse_spend_cents(&body)?;

    // El período es un extra: si falla, se muestra el consumo igual. No vale
    // perder el dato principal por no saber cuándo corta el mes.
    let period_start = fetch_period_start(&client, &cookie, &creds.auth_id);

    Ok(CursorAccountUsage {
        spend_cents,
        plan: creds.plan,
        period_end: period_start.as_deref().and_then(plus_one_month),
        period_start,
        fetched_at: Utc::now().timestamp_millis(),
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
}
