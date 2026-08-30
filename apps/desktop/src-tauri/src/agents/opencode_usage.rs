//! Cupos del plan de OpenCode (Go).
//!
//! OpenCode no publica esto en el CLI —`opencode stats` cuenta tokens gastados,
//! que no es lo mismo que cuánto queda— pero el backend del plan sí lo expone
//! en `GET /zen/go/v1/usage`, autenticado con la misma clave que el CLI ya
//! guardó al hacer `/connect`. No hay OAuth ni refresh: es una clave larga.
//!
//! `auth.json` vive en `~/.local/share/opencode/` **también en Windows**: el
//! CLI usa esa ruta literal, no `%APPDATA%`. Verificado en la máquina de
//! desarrollo antes de escribir esto.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Los proveedores de `auth.json` que corresponden al plan de suscripción.
/// `opencode-go` es el actual; `opencode` queda por si renombran la entrada.
const PLAN_KEYS: [&str; 2] = ["opencode-go", "opencode"];

/// Una ventana de cupo. `kind` es el nombre que usa la API (`rolling`,
/// `weekly`, `monthly`) y viaja crudo: traducirlo es trabajo de la vista.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeUsageWindow {
    pub kind: String,
    /// Porcentaje ya consumido, 0..=100.
    pub percent: f64,
    /// RFC3339 UTC, tal como lo manda la API.
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeAccountUsage {
    pub windows: Vec<OpencodeUsageWindow>,
    pub fetched_at: i64,
}

pub fn auth_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json"),
    )
}

/// ¿Hay plan de OpenCode configurado en esta máquina?
///
/// Se pregunta por la clave y no por el binario: `opencode` instalado pero sin
/// plan (solo API keys de terceros) no tiene cupo que mostrar, y ofrecer una
/// fila vacía es peor que no ofrecer ninguna.
pub fn detected() -> bool {
    load_key().is_some()
}

fn load_key() -> Option<String> {
    let text = fs::read_to_string(auth_path()?).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    parse_key(&root)
}

fn parse_key(root: &Value) -> Option<String> {
    for name in PLAN_KEYS {
        let key = root
            .get(name)
            .and_then(|v| v.get("key"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(key) = key {
            return Some(key.to_string());
        }
    }
    None
}

pub fn fetch_account_usage() -> Result<OpencodeAccountUsage, String> {
    let key = load_key().ok_or_else(|| {
        "no hay plan de OpenCode conectado. Ejecuta `/connect` en OpenCode.".to_string()
    })?;

    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;

    let resp = client
        .get(USAGE_URL)
        .bearer_auth(&key)
        .send()
        .map_err(|e| format!("no se pudo consultar el uso de OpenCode: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("la clave de OpenCode no es válida. Vuelve a hacer `/connect`.".to_string());
    }
    if !status.is_success() {
        return Err(format!("la API de OpenCode respondió {status}"));
    }

    let body = resp
        .text()
        .map_err(|e| format!("respuesta de OpenCode ilegible: {e}"))?;
    parse_usage_body(&body)
}

#[derive(Debug, Deserialize)]
struct ApiWindow {
    percent: Option<f64>,
    #[serde(rename = "resetsAt")]
    resets_at: Option<String>,
}

/// El orden importa: es el que ve el usuario, de la ventana más corta a la más
/// larga. La API manda un objeto, y un objeto JSON no tiene orden garantizado.
const WINDOW_ORDER: [&str; 3] = ["rolling", "weekly", "monthly"];

fn parse_usage_body(body: &str) -> Result<OpencodeAccountUsage, String> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| format!("OpenCode: JSON inesperado ({e})"))?;
    let usage = root
        .get("usage")
        .ok_or_else(|| "OpenCode respondió sin datos de uso".to_string())?;

    let mut windows = Vec::new();
    for kind in WINDOW_ORDER {
        let Some(raw) = usage.get(kind) else { continue };
        let Ok(win) = serde_json::from_value::<ApiWindow>(raw.clone()) else {
            continue;
        };
        let Some(percent) = win.percent else { continue };
        windows.push(OpencodeUsageWindow {
            kind: kind.to_string(),
            percent: percent.clamp(0.0, 100.0),
            resets_at: win.resets_at.filter(|s| !s.is_empty()),
        });
    }

    if windows.is_empty() {
        return Err("el plan de OpenCode no reporta ventanas de uso".to_string());
    }
    Ok(OpencodeAccountUsage {
        windows,
        fetched_at: chrono::Utc::now().timestamp_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parsea_las_tres_ventanas_en_orden() {
        // Cuerpo real del endpoint, con las claves desordenadas a propósito.
        let body = r#"{"usage":{
            "monthly":{"status":"ok","percent":28,"resetsAt":"2026-09-19T18:53:43.086Z"},
            "rolling":{"status":"ok","percent":0,"resetsAt":"2026-08-29T07:24:09.086Z"},
            "weekly":{"status":"ok","percent":41,"resetsAt":"2026-08-31T00:00:00.086Z"}
        }}"#;
        let usage = parse_usage_body(body).unwrap();
        let kinds: Vec<&str> = usage.windows.iter().map(|w| w.kind.as_str()).collect();
        assert_eq!(kinds, ["rolling", "weekly", "monthly"]);
        assert_eq!(usage.windows[1].percent, 41.0);
        assert_eq!(
            usage.windows[0].resets_at.as_deref(),
            Some("2026-08-29T07:24:09.086Z")
        );
    }

    #[test]
    fn ventana_sin_porcentaje_se_omite_sin_tumbar_el_resto() {
        let body = r#"{"usage":{"rolling":{"status":"pending"},"weekly":{"percent":10}}}"#;
        let usage = parse_usage_body(body).unwrap();
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].kind, "weekly");
    }

    #[test]
    fn sin_ventanas_es_error_y_no_una_fila_vacia() {
        assert!(parse_usage_body(r#"{"usage":{}}"#).is_err());
        assert!(parse_usage_body(r#"{"error":"nope"}"#).is_err());
    }

    #[test]
    fn toma_la_clave_del_plan_y_no_la_de_otro_proveedor() {
        let root = json!({
            "minimax": { "type": "api", "key": "no-es-esta" },
            "opencode-go": { "type": "api", "key": "  si-es-esta  " }
        });
        assert_eq!(parse_key(&root).as_deref(), Some("si-es-esta"));
        assert!(parse_key(&json!({ "openai": { "type": "oauth" } })).is_none());
        assert!(parse_key(&json!({ "opencode-go": { "key": "" } })).is_none());
    }
}
