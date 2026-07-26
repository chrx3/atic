//! Adaptador para Claude Code.
//!
//! Habla con el CLI en modo headless bidireccional:
//!
//! ```text
//! claude -p --input-format stream-json --output-format stream-json --verbose
//! ```
//!
//! Con `--input-format stream-json` el proceso queda **vivo** leyendo mensajes
//! de stdin, así que una conversación es un solo proceso y no uno por turno.
//! Eso importa: re-spawnear por mensaje perdería el contexto en caliente y
//! pagaría el arranque cada vez.
//!
//! # Sobre el login
//!
//! No se maneja autenticación acá a propósito. El CLI resuelve credenciales
//! solo —variable de entorno o el perfil OAuth en disco—, que es exactamente la
//! razón por la que este backend es el más barato de integrar. Si algún día
//! hiciera falta autenticar, sería un problema del CLI, no de Atic.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};

use super::{AgentBackend, AgentEvent, AgentSession, StartOptions};

pub struct ClaudeCode;

impl AgentBackend for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn is_available(&self) -> bool {
        Command::new("claude")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn start(
        &self,
        options: StartOptions,
        on_event: Box<dyn Fn(AgentEvent) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String> {
        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            // stream-json no emite el detalle sin esto.
            .arg("--verbose")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = &options.cwd {
            cmd.current_dir(dir);
        }
        if let Some(id) = &options.resume {
            cmd.arg("--resume").arg(id);
        }

        #[cfg(windows)]
        {
            // Sin esto, cada turno abre una consola negra sobre la app.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("no se pudo iniciar Claude Code: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "el proceso no expuso stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "el proceso no expuso stdout".to_string())?;

        // stderr por separado: un fallo de arranque (CLI no encontrado, login
        // vencido) sale por ahí y no por el stream de eventos. Sin leerlo, ese
        // caso se vería como una sesión que simplemente no responde.
        // Distingue un cierre pedido de una muerte del proceso. Sin esto, la
        // única señal de que el agente se cayó sería que deja de responder.
        let stopping = Arc::new(AtomicBool::new(false));

        if let Some(stderr) = child.stderr.take() {
            let emit = OnEvent(std::sync::Arc::new(on_event));
            let emit_err = emit.clone();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        emit_err.0(AgentEvent::Notice { text: line });
                    }
                }
            });

            let died = stopping.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    for event in translate(&line) {
                        emit.0(event);
                    }
                }
                // stdout cerrado = el proceso terminó. Si nadie lo pidió, se
                // cayó, y callarlo dejaría a la UI esperando para siempre.
                if !died.load(Ordering::SeqCst) {
                    emit.0(AgentEvent::Failed {
                        message: "El agente terminó inesperadamente.".to_string(),
                    });
                }
            });
        }

        Ok(Box::new(ClaudeSession {
            child,
            stdin: Some(stdin),
            stopping,
        }))
    }
}

/// Handle clonable para compartir el callback entre los dos hilos lectores.
#[derive(Clone)]
struct OnEvent(std::sync::Arc<Box<dyn Fn(AgentEvent) + Send + Sync + 'static>>);

/// Traduce una línea del CLI a cero o más eventos normalizados.
///
/// Devuelve varios porque un solo mensaje del asistente puede traer texto y
/// llamadas a herramientas mezclados en su lista de bloques.
///
/// Una línea que no se entiende NO se descarta: sale como `Notice`. El formato
/// del CLI va a crecer, y un adaptador que traga en silencio lo que no conoce
/// hace que los eventos nuevos se vean como si nada hubiera pasado.
fn translate(line: &str) -> Vec<AgentEvent> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![AgentEvent::Notice {
            text: line.to_string(),
        }];
    };

    match v.get("type").and_then(Value::as_str) {
        Some("system") => {
            // El evento final del turno no trae `type`, así que se reconoce por
            // sus campos; ver la rama `None` de abajo.
            vec![AgentEvent::Started {
                session_id: str_at(&v, "session_id"),
                tools: v
                    .get("tools")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                cwd: str_at(&v, "cwd"),
            }]
        }

        Some("assistant") => blocks(&v)
            .iter()
            .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    b.get("text")
                        .and_then(Value::as_str)
                        .map(|t| AgentEvent::Message {
                            text: t.to_string(),
                        })
                }
                Some("tool_use") => Some(AgentEvent::ToolCall {
                    id: str_at(b, "id"),
                    name: str_at(b, "name"),
                    input: b.get("input").cloned().unwrap_or(Value::Null),
                }),
                // Bloques que esta capa no modela (thinking, etc.): se ignoran
                // en silencio porque no aportan a la conversación visible.
                _ => None,
            })
            .collect(),

        // Los resultados de herramienta llegan como un turno de usuario.
        Some("user") => blocks(&v)
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            .map(|b| AgentEvent::ToolResult {
                id: b
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                output: render_content(b.get("content")),
                is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect(),

        Some("rate_limit_event") => {
            let status = v
                .get("rate_limit_info")
                .and_then(|i| i.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("desconocido");
            // Solo molestar cuando no es el caso normal.
            if status == "allowed" {
                Vec::new()
            } else {
                vec![AgentEvent::Notice {
                    text: format!("Límite de uso: {status}"),
                }]
            }
        }

        // Sin `type` pero con `stop_reason`: es el resumen final del turno.
        None if v.get("stop_reason").is_some() || v.get("is_error").is_some() => {
            vec![AgentEvent::Finished {
                stop_reason: v
                    .get("stop_reason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
            }]
        }

        _ => vec![AgentEvent::Notice {
            text: line.to_string(),
        }],
    }
}

fn str_at(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Los bloques de contenido de un mensaje, vengan como lista o como texto.
fn blocks(v: &Value) -> Vec<Value> {
    v.get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// El contenido de un `tool_result` puede ser texto suelto o una lista de
/// bloques. Se aplana a texto porque la UI lo muestra como salida.
fn render_content(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

struct ClaudeSession {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Puesto en `stop`/`drop` para que el lector no reporte un fallo cuando
    /// el cierre fue pedido.
    stopping: Arc<AtomicBool>,
}

impl AgentSession for ClaudeSession {
    fn send(&mut self, text: &str) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "la sesión ya está cerrada".to_string())?;
        // Mismo sobre que la Messages API: el CLI espera un turno de usuario.
        let line = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] }
        });
        writeln!(stdin, "{line}").map_err(|e| format!("no se pudo enviar: {e}"))?;
        stdin.flush().map_err(|e| format!("no se pudo enviar: {e}"))
    }

    fn stop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Cerrar stdin primero: el CLI termina solo al ver EOF, lo que le deja
        // vaciar lo que tenga pendiente. Matarlo de entrada perdería eventos.
        self.stdin.take();
        let _ = self.child.wait();
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.stdin.take();
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduce_el_init_del_sistema() {
        let events = translate(
            r#"{"type":"system","subtype":"init","cwd":"C:\\p","session_id":"s1","tools":["Bash","Read"]}"#,
        );
        match &events[0] {
            AgentEvent::Started {
                session_id,
                tools,
                cwd,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(tools, &["Bash".to_string(), "Read".to_string()]);
                assert_eq!(cwd, "C:\\p");
            }
            other => panic!("esperaba Started, salió {other:?}"),
        }
    }

    #[test]
    fn un_mensaje_puede_traer_texto_y_herramienta() {
        let events = translate(
            r#"{"type":"assistant","message":{"content":[
                {"type":"text","text":"voy a mirar"},
                {"type":"tool_use","id":"t1","name":"Read","input":{"file":"a.rs"}}
            ]}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], AgentEvent::Message { .. }));
        assert!(matches!(events[1], AgentEvent::ToolCall { .. }));
    }

    #[test]
    fn el_evento_final_no_trae_type() {
        // El CLI cierra el turno con un objeto sin `type`; reconocerlo por sus
        // campos es la única forma de no perder el fin de turno.
        let events =
            translate(r#"{"is_error":false,"stop_reason":"end_turn","total_cost_usd":0.08}"#);
        match &events[0] {
            AgentEvent::Finished {
                stop_reason,
                is_error,
                cost_usd,
            } => {
                assert_eq!(stop_reason.as_deref(), Some("end_turn"));
                assert!(!is_error);
                assert_eq!(*cost_usd, Some(0.08));
            }
            other => panic!("esperaba Finished, salió {other:?}"),
        }
    }

    #[test]
    fn lo_desconocido_se_reporta_en_vez_de_descartarse() {
        // El formato del CLI va a crecer. Tragar en silencio lo que no se
        // conoce hace que un evento nuevo se vea como si nada hubiera pasado.
        let events = translate(r#"{"type":"algo_que_no_existe_todavia","x":1}"#);
        assert!(matches!(events[0], AgentEvent::Notice { .. }));

        let rotos = translate("esto no es json");
        assert!(matches!(rotos[0], AgentEvent::Notice { .. }));
    }

    #[test]
    fn el_ruido_de_limite_permitido_no_llega_a_la_ui() {
        let events =
            translate(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#);
        assert!(events.is_empty());
    }
}
