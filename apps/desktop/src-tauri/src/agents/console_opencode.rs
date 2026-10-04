//! OpenCode en una consola de Atic: lo que espera y cómo contestarlo.
//!
//! Su TUI corre un servidor HTTP propio. Atic lo lanza con `--port` (ver
//! `with_opencode_port` en [`super::console`]) y desde ahí lista las preguntas
//! (`GET /question`) y los permisos (`GET /permission`) pendientes, y los
//! contesta (`POST /question/{id}/reply` o `/reject`, `POST
//! /permission/{id}/reply`). Probado contra OpenCode 1.15.
//!
//! El servidor escucha solo en localhost, igual que el que OpenCode levanta
//! siempre en un puerto al azar: fijar el puerto no abre nada nuevo.

use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::console_prompts::{self, Prompt};

/// Cada cuánto se pregunta qué hay pendiente.
const POLL: Duration = Duration::from_secs(1);
/// Tantas consultas fallidas seguidas: la consola se cerró.
const GIVE_UP_AFTER: u32 = 30;

fn client() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder().timeout(Duration::from_secs(3)).build().ok()
}

fn base(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Un puerto libre para el servidor del OpenCode que se va a lanzar.
pub(crate) fn free_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?.local_addr().ok().map(|a| a.port())
}

/// Sigue el servidor de un OpenCode recién lanzado hasta que deja de contestar.
pub(crate) fn watch(port: u16) {
    thread::Builder::new()
        .name(format!("opencode-{port}"))
        .spawn(move || {
            let Some(http) = client() else { return };
            // También cubre el arranque: el servidor tarda unos segundos en abrir.
            let mut failures = 0;
            while failures < GIVE_UP_AFTER {
                thread::sleep(POLL);
                match pending(&http, port) {
                    Some(live) => {
                        failures = 0;
                        console_prompts::sync_opencode(port, live);
                    }
                    None => failures += 1,
                }
            }
            console_prompts::sync_opencode(port, Vec::new());
        })
        .ok();
}

fn get(http: &reqwest::blocking::Client, port: u16, path: &str) -> Option<Vec<Value>> {
    http.get(format!("{}{path}", base(port))).send().ok()?.error_for_status().ok()?.json().ok()
}

/// Preguntas y permisos pendientes: (sesión, id, qué es).
fn pending(http: &reqwest::blocking::Client, port: u16) -> Option<Vec<(String, String, Prompt)>> {
    let questions = get(http, port, "/question")?;
    let permissions = get(http, port, "/permission")?;
    Some(
        questions
            .iter()
            .filter_map(parse_question)
            .chain(permissions.iter().filter_map(parse_permission))
            .collect(),
    )
}

fn ids(v: &Value) -> Option<(String, String)> {
    Some((v.get("sessionID")?.as_str()?.to_string(), v.get("id")?.as_str()?.to_string()))
}

fn parse_question(v: &Value) -> Option<(String, String, Prompt)> {
    let (session, id) = ids(v)?;
    let questions = v.get("questions").filter(|q| q.is_array())?.clone();
    Some((session, id, Prompt::Question { input: json!({ "questions": questions }) }))
}

/// `bash` con `patterns: ["mkdir x"]` es «ejecutar `mkdir x`»; lo que el
/// celular muestra sale de `command`.
fn parse_permission(v: &Value) -> Option<(String, String, Prompt)> {
    let (session, id) = ids(v)?;
    let tool = v.get("permission")?.as_str()?.to_string();
    let patterns: Vec<&str> = v
        .get("patterns")
        .and_then(Value::as_array)
        .map(|p| p.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let can_always = v.get("always").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    let input = json!({ "command": patterns.join(" "), "description": tool });
    let tool = match tool.as_str() {
        "bash" => "Bash".to_string(),
        "edit" => "Edit".to_string(),
        "webfetch" => "WebFetch".to_string(),
        _ => tool,
    };
    Some((session, id, Prompt::Permission { tool, input, can_always }))
}

fn post(port: u16, path: &str, body: Value) -> Result<(), String> {
    let http = client().ok_or("sin cliente HTTP")?;
    let ok: bool = http
        .post(format!("{}{path}", base(port)))
        .json(&body)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| format!("OpenCode no contestó: {e}"))?;
    ok.then_some(()).ok_or_else(|| "OpenCode no aceptó la respuesta".into())
}

pub(crate) fn reply_question(port: u16, id: &str, answers: Vec<Vec<String>>) -> Result<(), String> {
    post(port, &format!("/question/{id}/reply"), json!({ "answers": answers }))
}

pub(crate) fn reject_question(port: u16, id: &str) -> Result<(), String> {
    post(port, &format!("/question/{id}/reject"), json!({}))
}

pub(crate) fn reply_permission(port: u16, id: &str, reply: &str) -> Result<(), String> {
    post(port, &format!("/permission/{id}/reply"), json!({ "reply": reply }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_preguntas_y_permisos_como_los_lista_opencode() {
        let (session, id, prompt) = parse_question(&json!({
            "id": "que_1", "sessionID": "ses_1",
            "questions": [{ "question": "¿Día?", "header": "Día", "options": [{ "label": "Lunes", "description": "" }] }],
        }))
        .unwrap();
        assert_eq!((session.as_str(), id.as_str()), ("ses_1", "que_1"));
        assert!(matches!(prompt, Prompt::Question { .. }));

        let (_, id, prompt) = parse_permission(&json!({
            "id": "per_1", "sessionID": "ses_1", "permission": "bash",
            "patterns": ["mkdir carpeta-oc"], "metadata": {}, "always": ["mkdir *"],
        }))
        .unwrap();
        assert_eq!(id, "per_1");
        assert_eq!(
            prompt,
            Prompt::Permission {
                tool: "Bash".into(),
                input: json!({ "command": "mkdir carpeta-oc", "description": "bash" }),
                can_always: true,
            }
        );
    }
}
