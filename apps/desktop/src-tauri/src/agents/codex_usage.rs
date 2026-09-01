//! Cupos de la cuenta de Codex mediante el app-server oficial.
//!
//! No lee tokens ni llama endpoints privados: levanta `codex app-server`, hace
//! el handshake JSONL y consulta `account/rateLimits/read` con la sesión que el
//! propio CLI ya administra.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
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
    let child_slot: std::sync::Arc<Mutex<Option<Child>>> = Default::default();
    let slot = child_slot.clone();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_inner(&slot));
    });
    match rx.recv_timeout(REQUEST_TIMEOUT) {
        Ok(result) => result,
        Err(_) => {
            // El hilo sigue clavado en read_line: matar el proceso lo
            // desbloquea y no queda un app-server huérfano por cada timeout.
            if let Some(child) = lock_slot(&child_slot).as_mut() {
                kill_child(child);
            }
            Err("Codex tardó más de 15 segundos en leer el uso".to_string())
        }
    }
}

fn lock_slot(slot: &Mutex<Option<Child>>) -> std::sync::MutexGuard<'_, Option<Child>> {
    slot.lock().unwrap_or_else(|e| e.into_inner())
}

fn fetch_inner(slot: &Mutex<Option<Child>>) -> Result<CodexAccountUsage, String> {
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
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Codex no expuso stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex no expuso stdout".to_string())?;
    // Los pipes se sacan ANTES de compartir el handle: así el que espera el
    // timeout puede matar el proceso sin pelearse por el Child.
    *lock_slot(slot) = Some(child);
    let result = exchange(stdin, stdout);
    if let Some(mut child) = lock_slot(slot).take() {
        kill_child(&mut child);
    }
    result
}

fn exchange(
    mut stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
) -> Result<CodexAccountUsage, String> {
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
        fetched_at: now_ms(),
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

// ---------------------------------------------------------------------------
// Lectura desde disco
// ---------------------------------------------------------------------------

/// Cuánto se lee del final de un rollout buscando el último `rate_limits`.
/// Los `token_count` salen cada turno, así que la cola alcanza de sobra.
const TAIL_BYTES: u64 = 256 * 1024;
/// Cuántos rollouts recientes se miran antes de rendirse.
const ROLLOUTS_TO_SCAN: usize = 6;
/// Cuántas carpetas de día se recorren. Acota el listado en un `sessions/` con
/// meses de historia.
const DAYS_TO_SCAN: usize = 5;

/// Cupos leídos del disco, sin levantar `codex app-server`.
///
/// El CLI ya escribe los rate limits en cada evento `token_count` de su
/// rollout, así que el dato está en `~/.codex/sessions/…` antes de que se lo
/// pidamos. Para la pill —que refresca sola— esa es la diferencia entre leer
/// un archivo y arrancar un proceso Node cada minuto.
///
/// El precio es que el dato es tan fresco como tu último turno de Codex. Por
/// eso `fetched_at` trae el timestamp real de la línea y no `now()`: así la
/// vista puede decir «hace 3 h» en vez de presentar algo viejo como si
/// acabara de consultarse.
pub fn fetch_from_rollout() -> Result<CodexAccountUsage, String> {
    let root = super::watch_codex::sessions_root()
        .ok_or_else(|| "no se encontró ~/.codex/sessions".to_string())?;
    let files = recent_rollouts(&root);
    if files.is_empty() {
        return Err("todavía no hay sesiones de Codex en este equipo".to_string());
    }
    for path in files {
        if let Some(usage) = usage_from_rollout(&path) {
            return Ok(usage);
        }
    }
    Err("las sesiones recientes de Codex no traen cupos. Abre Codex una vez.".to_string())
}

/// ¿Hay Codex en esta máquina? Basta con que exista el directorio de sesiones:
/// preguntar por el binario obliga a resolver el PATH en cada chequeo.
pub fn detected() -> bool {
    super::watch_codex::sessions_root()
        .map(|p| p.is_dir())
        .unwrap_or(false)
        || super::exe::launcher("codex").is_some()
}

/// Nombres de subcarpeta ordenados del más nuevo al más viejo.
///
/// Se ordena por nombre y no por mtime porque el layout es `YYYY/MM/DD`: los
/// nombres ya están rellenados con ceros, así que el orden lexicográfico ES el
/// cronológico, y no cuesta un `stat` por carpeta.
fn dirs_newest_first(dir: &Path) -> Vec<PathBuf> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    names.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    names
}

fn recent_day_dirs(root: &Path) -> Vec<PathBuf> {
    let mut days = Vec::new();
    for year in dirs_newest_first(root) {
        for month in dirs_newest_first(&year) {
            for day in dirs_newest_first(&month) {
                days.push(day);
                if days.len() >= DAYS_TO_SCAN {
                    return days;
                }
            }
        }
    }
    days
}

fn recent_rollouts(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for day in recent_day_dirs(root) {
        for entry in std::fs::read_dir(&day).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            files.push((modified, path));
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files
        .into_iter()
        .take(ROLLOUTS_TO_SCAN)
        .map(|f| f.1)
        .collect()
}

fn usage_from_rollout(path: &Path) -> Option<CodexAccountUsage> {
    let tail = tail_text(path)?;
    // De atrás para adelante: interesa el ÚLTIMO cupo del archivo. La primera
    // línea del buffer puede venir cortada al medio; el parseo la descarta
    // sola, sin necesidad de contarla aparte.
    for line in tail.lines().rev() {
        if !line.contains("\"rate_limits\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let stamp = value.get("timestamp").and_then(Value::as_str);
        let limits = find_key(&value, "rate_limits")?;
        if let Some(usage) = parse_rollout_limits(limits, stamp) {
            return Some(usage);
        }
    }
    None
}

fn tail_text(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Busca una clave a cualquier profundidad.
///
/// El rollout anida los cupos dentro del payload del evento, y esa ruta es
/// interna de Codex: ya cambió una vez. Buscar por nombre cuesta un recorrido
/// de un objeto chico y sobrevive a que la muevan de lugar.
fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(hit) = map.get(key) {
                if !hit.is_null() {
                    return Some(hit);
                }
            }
            map.values().find_map(|v| find_key(v, key))
        }
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

/// El rollout usa `snake_case` y el app-server `camelCase` para lo mismo.
/// Son dos formatos distintos del mismo dato, así que son dos parsers.
fn parse_rollout_limits(limits: &Value, stamp: Option<&str>) -> Option<CodexAccountUsage> {
    let window = |key: &str| -> Option<CodexUsageWindow> {
        let raw = limits.get(key)?;
        Some(CodexUsageWindow {
            used_percent: raw.get("used_percent")?.as_f64()?.clamp(0.0, 100.0),
            window_duration_mins: raw.get("window_minutes")?.as_u64()?,
            resets_at: raw.get("resets_at").and_then(Value::as_i64),
        })
    };
    let primary = window("primary");
    let secondary = window("secondary");
    if primary.is_none() && secondary.is_none() {
        return None;
    }
    Some(CodexAccountUsage {
        primary,
        secondary,
        plan: limits
            .get("plan_type")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit_name: limits
            .get("limit_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        fetched_at: stamp.and_then(epoch_ms).unwrap_or_else(now_ms),
    })
}

fn epoch_ms(stamp: &str) -> Option<u64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(stamp).ok()?;
    u64::try_from(parsed.timestamp_millis()).ok()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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

    /// Línea real de un rollout, recortada a lo que importa.
    const ROLLOUT_LINE: &str = r#"{"timestamp":"2026-08-26T20:18:06.843Z","ordinal":259,"type":"event_msg","payload":{"type":"token_count","info":{"model_context_window":258400},"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":14.0,"window_minutes":300,"resets_at":1787792182},"secondary":{"used_percent":13.0,"window_minutes":10080,"resets_at":1788272114},"credits":{"has_credits":false,"balance":"0"},"plan_type":"plus"}}}"#;

    #[test]
    fn lee_los_cupos_del_rollout_con_su_timestamp() {
        let line: Value = serde_json::from_str(ROLLOUT_LINE).unwrap();
        let limits = find_key(&line, "rate_limits").unwrap();
        let stamp = line.get("timestamp").and_then(Value::as_str);
        let usage = parse_rollout_limits(limits, stamp).unwrap();

        assert_eq!(usage.primary.as_ref().unwrap().used_percent, 14.0);
        assert_eq!(usage.primary.as_ref().unwrap().window_duration_mins, 300);
        assert_eq!(usage.secondary.as_ref().unwrap().used_percent, 13.0);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        // El sello es el de la línea, no el del momento de leerla: si fuera
        // `now()` la vista no podría avisar que el dato está viejo.
        assert_eq!(usage.fetched_at, 1787775486843);
    }

    #[test]
    fn el_rollout_usa_snake_case_y_el_app_server_camel_case() {
        // El parser del app-server no entiende la forma del disco. Que sean
        // dos es a propósito; este test lo deja escrito.
        let line: Value = serde_json::from_str(ROLLOUT_LINE).unwrap();
        let limits = find_key(&line, "rate_limits").unwrap();
        assert!(parse_window(limits.get("primary")).is_none());
        assert!(parse_rollout_limits(limits, None).is_some());
    }

    #[test]
    fn busca_la_clave_aunque_cambie_de_profundidad() {
        let movido = json!({ "a": { "b": [{ "rate_limits": { "primary": { "used_percent": 5.0, "window_minutes": 300 } } }] } });
        let limits = find_key(&movido, "rate_limits").unwrap();
        assert_eq!(
            parse_rollout_limits(limits, None)
                .unwrap()
                .primary
                .unwrap()
                .used_percent,
            5.0
        );
        assert!(find_key(&json!({ "rate_limits": null }), "rate_limits").is_none());
    }

    #[test]
    fn sin_ventanas_el_rollout_no_cuenta_como_dato() {
        let vacio = json!({ "limit_id": "codex", "primary": null, "secondary": null });
        assert!(parse_rollout_limits(&vacio, None).is_none());
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
