//! Cupos de la cuenta de Codex mediante el app-server oficial.
//!
//! No lee tokens ni llama endpoints privados: levanta `codex app-server`, hace
//! el handshake JSONL y consulta `account/rateLimits/read` con la sesión que el
//! propio CLI ya administra.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const ID_INITIALIZE: u64 = 1;
const ID_RATE_LIMITS: u64 = 2;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsageWindow {
    pub used_percent: f64,
    pub window_duration_mins: u64,
    /// Segundos Unix, igual que el protocolo de Codex.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountUsage {
    pub primary: Option<CodexUsageWindow>,
    pub secondary: Option<CodexUsageWindow>,
    pub plan: Option<String>,
    pub limit_name: Option<String>,
    pub fetched_at: u64,
}

pub fn fetch_account_usage() -> Result<CodexAccountUsage, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_inner());
    });
    rx.recv_timeout(REQUEST_TIMEOUT)
        .map_err(|_| "Codex tardó más de 15 segundos en leer el uso".to_string())?
}

fn fetch_inner() -> Result<CodexAccountUsage, String> {
    let (program, prefix) = super::exe::launcher("codex")
        .ok_or_else(|| "no se encontró «codex» en el PATH".to_string())?;

    let mut cmd = Command::new(program);
    cmd.args(prefix)
        .arg("app-server")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("no se pudo iniciar Codex: {e}"))?;
    let result = exchange(&mut child);
    kill_child(&mut child);
    result
}

fn exchange(child: &mut Child) -> Result<CodexAccountUsage, String> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Codex no expuso stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex no expuso stdout".to_string())?;
    let mut reader = BufReader::new(stdout);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": ID_INITIALIZE,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "atic",
                    "title": "Atic",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
    )?;
    read_response(&mut reader, ID_INITIALIZE)?;
    send(&mut stdin, json!({ "method": "initialized" }))?;
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": ID_RATE_LIMITS,
            "method": "account/rateLimits/read"
        }),
    )?;
    let response = read_response(&mut reader, ID_RATE_LIMITS)?;
    parse_response(&response)
}

fn send(stdin: &mut impl Write, value: Value) -> Result<(), String> {
    writeln!(stdin, "{value}").map_err(|e| format!("no se pudo enviar a Codex: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("no se pudo enviar a Codex: {e}"))
}

fn read_response(reader: &mut impl BufRead, target_id: u64) -> Result<Value, String> {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err("Codex cerró stdout antes de responder".to_string()),
            Ok(_) => {
                let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                if value.get("id").and_then(Value::as_u64) != Some(target_id) {
                    continue;
                }
                if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                    return Err(format!("Codex no pudo leer el uso: {message}"));
                }
                return Ok(value);
            }
            Err(e) => return Err(format!("no se pudo leer la respuesta de Codex: {e}")),
        }
    }
}

fn parse_response(response: &Value) -> Result<CodexAccountUsage, String> {
    let result = response
        .get("result")
        .ok_or_else(|| "Codex respondió sin datos de uso".to_string())?;
    let limits = result
        .pointer("/rateLimitsByLimitId/codex")
        .or_else(|| result.get("rateLimits"))
        .ok_or_else(|| "la cuenta de Codex no reporta cupos".to_string())?;

    let parsed = CodexAccountUsage {
        primary: parse_window(limits.get("primary")),
        secondary: parse_window(limits.get("secondary")),
        plan: limits
            .get("planType")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit_name: limits
            .get("limitName")
            .and_then(Value::as_str)
            .map(str::to_string),
        fetched_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    if parsed.primary.is_none() && parsed.secondary.is_none() {
        return Err("la cuenta de Codex no reporta ventanas de uso".to_string());
    }
    Ok(parsed)
}

fn parse_window(value: Option<&Value>) -> Option<CodexUsageWindow> {
    let value = value?;
    Some(CodexUsageWindow {
        used_percent: value.get("usedPercent")?.as_f64()?.clamp(0.0, 100.0),
        window_duration_mins: value.get("windowDurationMins")?.as_u64()?,
        resets_at: value.get("resetsAt").and_then(Value::as_i64),
    })
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_los_limites_de_codex() {
        let response = json!({
            "id": 2,
            "result": {
                "rateLimits": {
                    "planType": "plus",
                    "primary": {
                        "usedPercent": 47,
                        "windowDurationMins": 300,
                        "resetsAt": 1_800_000_000
                    },
                    "secondary": {
                        "usedPercent": 12.5,
                        "windowDurationMins": 10_080,
                        "resetsAt": null
                    }
                }
            }
        });
        let usage = parse_response(&response).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.primary.unwrap().used_percent, 47.0);
        assert_eq!(usage.secondary.unwrap().window_duration_mins, 10_080);
    }

    #[test]
    fn prefiere_el_bucket_codex_si_existe() {
        let response = json!({
            "id": 2,
            "result": {
                "rateLimits": { "primary": null, "secondary": null },
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitName": "Codex",
                        "primary": { "usedPercent": 20, "windowDurationMins": 300 }
                    }
                }
            }
        });
        let usage = parse_response(&response).unwrap();
        assert_eq!(usage.limit_name.as_deref(), Some("Codex"));
        assert_eq!(usage.primary.unwrap().used_percent, 20.0);
    }
}
