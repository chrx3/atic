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

use super::{AgentBackend, AgentEvent, AgentSession, McpServerState, SlashCommand, StartOptions};

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
            // Sin esto el modo `manual` NO pregunta: deniega la herramienta y
            // la lista en `permission_denials` del resultado. Comprobado: con
            // el flag llega `control_request/can_use_tool` y el turno espera.
            // No aparece en `claude --help`.
            .arg("--permission-prompt-tool")
            .arg("stdio")
            // Texto según se escribe. Sin esto la respuesta aparece de golpe al
            // cerrar el turno, y en una tarea larga eso se ve como un cuelgue.
            .arg("--include-partial-messages")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = &options.cwd {
            cmd.current_dir(dir);
        }
        if let Some(id) = &options.resume {
            cmd.arg("--resume").arg(id);
        }
        if let Some(model) = &options.model {
            cmd.arg("--model").arg(model);
        }
        // Sin modo explícito, se preguntan todos: es el default seguro para una
        // interfaz gráfica, donde el usuario está mirando y puede contestar.
        cmd.arg("--permission-mode")
            .arg(options.permission_mode.as_deref().unwrap_or("manual"));
        if let Some(mcp) = &options.mcp_config {
            cmd.arg("--mcp-config").arg(mcp);
        }
        for dir in &options.add_dirs {
            cmd.arg("--add-dir").arg(dir);
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

        let mut session = ClaudeSession {
            child,
            stdin: Some(stdin),
            stopping,
        };
        // Handshake del canal de control. Abre la vía por la que después llegan
        // las pedidas de permiso; sin él, el turno se bloquearía esperando una
        // respuesta que nadie podría mandar.
        session.control(json!({ "subtype": "initialize" }))?;
        Ok(Box::new(session))
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
        // El agente quedó esperando una respuesta nuestra.
        Some("control_request")
            if v.get("request")
                .and_then(|r| r.get("subtype"))
                .and_then(Value::as_str)
                == Some("can_use_tool") =>
        {
            let req = v.get("request").cloned().unwrap_or(Value::Null);
            vec![AgentEvent::Permission {
                // El id del control, no el del tool_use: es lo que hay que
                // devolver para desbloquear el turno.
                id: str_at(&v, "request_id"),
                tool: str_at(&req, "tool_name"),
                description: str_at(&req, "description"),
                input: req.get("input").cloned().unwrap_or(Value::Null),
                suggestions: req
                    .get("permission_suggestions")
                    .cloned()
                    .unwrap_or(Value::Null),
            }]
        }

        // La respuesta al handshake trae el catálogo de comandos, que es lo
        // único que aporta de todo el canal de control.
        Some("control_response") => v
            .get("response")
            .and_then(|r| r.get("response"))
            .and_then(|r| r.get("commands"))
            .and_then(Value::as_array)
            .map(|list| {
                vec![AgentEvent::Commands {
                    commands: list
                        .iter()
                        .map(|c| SlashCommand {
                            name: str_at(c, "name"),
                            description: str_at(c, "description"),
                            argument_hint: str_at(c, "argumentHint"),
                        })
                        .filter(|c| !c.name.is_empty())
                        .collect(),
                }]
            })
            .unwrap_or_default(),

        Some("control_cancel_request") => Vec::new(),

        // Escritura en curso. Solo interesa el texto: la apertura y el cierre
        // de bloque, el uso de tokens y los deltas de herramienta ya llegan
        // resueltos en el mensaje completo, y mandarlos como `Notice` ahogaría
        // el registro con una línea por palabra.
        Some("stream_event") => {
            let delta = v.get("event").and_then(|e| e.get("delta"));
            match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                Some("text_delta") => delta
                    .and_then(|d| d.get("text"))
                    .and_then(Value::as_str)
                    .map(|t| {
                        vec![AgentEvent::Delta {
                            text: t.to_string(),
                        }]
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        }

        Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
            vec![AgentEvent::Started {
                session_id: str_at(&v, "session_id"),
                tools: strings_at(&v, "tools"),
                cwd: str_at(&v, "cwd"),
                model: str_at(&v, "model"),
                slash_commands: strings_at(&v, "slash_commands"),
                mcp_servers: v
                    .get("mcp_servers")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|s| McpServerState {
                                name: str_at(s, "name"),
                                status: str_at(s, "status"),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }]
        }

        Some("assistant") => {
            let mut out: Vec<AgentEvent> = blocks(&v)
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
                    // El razonamiento sale, pero con su propio tipo: la vista
                    // lo pliega. Vacío es el caso normal cuando el modelo no
                    // expone el contenido, y no merece una tarjeta.
                    Some("thinking") => b
                        .get("thinking")
                        .and_then(Value::as_str)
                        .filter(|t| !t.trim().is_empty())
                        .map(|t| AgentEvent::Thinking {
                            text: t.to_string(),
                        }),
                    _ => None,
                })
                .collect();
            // El contexto se mide por mensaje, no al final del turno: lo que se
            // quiere ver es cómo sube mientras el agente trabaja.
            if let Some(tokens) = context_tokens(&v) {
                out.push(AgentEvent::Context { tokens });
            }
            out
        }

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

        // Resumen final del turno.
        //
        // El campo `type` aparece casi al FINAL de este objeto, después de
        // `usage`, `modelUsage` y varios más. Una captura truncada del CLI lo
        // deja fuera y hace parecer que el evento no tiene tipo — el motivo por
        // el que la primera versión de este traductor lo mandaba a `Notice` y
        // el turno nunca se daba por terminado. La rama sin `type` queda como
        // red por si algún día lo omiten de verdad.
        Some("result") | None if v.get("stop_reason").is_some() || v.get("is_error").is_some() => {
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

/// Una lista de strings, tolerando que el campo no esté.
fn strings_at(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Cuánto contexto ocupa el mensaje.
///
/// Se suman los tres tipos de entrada —nueva, leída de caché y escrita a
/// caché— porque las tres ocupan ventana. Mirar solo `input_tokens` daría un
/// número minúsculo en cualquier conversación con caché, que son todas.
fn context_tokens(v: &Value) -> Option<u64> {
    let usage = v.get("message")?.get("usage")?;
    let field = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let total = field("input_tokens")
        + field("cache_read_input_tokens")
        + field("cache_creation_input_tokens");
    (total > 0).then_some(total)
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

impl ClaudeSession {
    /// Escribe una línea en stdin del agente.
    fn write(&mut self, line: Value) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "la sesión ya está cerrada".to_string())?;
        writeln!(stdin, "{line}").map_err(|e| format!("no se pudo enviar: {e}"))?;
        stdin.flush().map_err(|e| format!("no se pudo enviar: {e}"))
    }

    /// Manda un pedido por el canal de control.
    fn control(&mut self, request: Value) -> Result<(), String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.write(json!({
            "type": "control_request",
            "request_id": id,
            "request": request,
        }))
    }
}

impl AgentSession for ClaudeSession {
    fn send(&mut self, text: &str) -> Result<(), String> {
        // Mismo sobre que la Messages API: el CLI espera un turno de usuario.
        self.write(json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] }
        }))
    }

    fn respond_permission(&mut self, id: &str, allow: bool) -> Result<(), String> {
        // El `request_id` de la respuesta tiene que ser el del pedido: es lo
        // que empareja la contestación con el turno detenido.
        let decision = if allow {
            // Sin `updatedInput` el CLI toma la herramienta como aprobada tal
            // cual la pidió. Modificarla es otra función, no esta.
            json!({ "behavior": "allow" })
        } else {
            json!({ "behavior": "deny", "message": "Permiso denegado desde Atic." })
        };
        self.write(json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": id,
                "response": decision,
            }
        }))
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
            r#"{"type":"system","subtype":"init","cwd":"C:\\p","session_id":"s1","tools":["Bash","Read"],"model":"claude-sonnet-5","slash_commands":["review"],"mcp_servers":[]}"#,
        );
        match &events[0] {
            AgentEvent::Started {
                session_id,
                tools,
                cwd,
                model,
                ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(tools, &["Bash".to_string(), "Read".to_string()]);
                assert_eq!(cwd, "C:\\p");
                assert_eq!(model, "claude-sonnet-5");
            }
            other => panic!("esperaba Started, salió {other:?}"),
        }
    }

    /// El permiso trae el id del CANAL DE CONTROL, no el del `tool_use`.
    ///
    /// Contestar con el del tool_use deja el turno colgado para siempre: el
    /// CLI empareja la respuesta por `request_id` y descarta lo que no reconoce.
    #[test]
    fn traduce_una_pedida_de_permiso() {
        let events = translate(
            r#"{"type":"control_request","request_id":"c7a3","request":{"subtype":"can_use_tool","tool_name":"Write","display_name":"Write","input":{"file_path":"a.txt"},"description":"a.txt","permission_suggestions":[{"type":"setMode","mode":"acceptEdits"}],"tool_use_id":"toolu_9"}}"#,
        );
        match &events[0] {
            AgentEvent::Permission {
                id,
                tool,
                description,
                ..
            } => {
                assert_eq!(id, "c7a3");
                assert_eq!(tool, "Write");
                assert_eq!(description, "a.txt");
            }
            other => panic!("esperaba Permission, salió {other:?}"),
        }
    }

    /// El contexto son las tres entradas sumadas. Con caché, `input_tokens`
    /// solo es una fracción minúscula y la barra parecería vacía siempre.
    #[test]
    fn el_contexto_suma_el_cache() {
        let events = translate(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hola"}],"usage":{"input_tokens":10,"cache_read_input_tokens":900,"cache_creation_input_tokens":90,"output_tokens":5}}}"#,
        );
        let tokens = events.iter().find_map(|e| match e {
            AgentEvent::Context { tokens } => Some(*tokens),
            _ => None,
        });
        assert_eq!(tokens, Some(1000));
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
    fn el_type_del_evento_final_llega_tarde_en_el_objeto() {
        // Orden real del CLI: `type` va después de `usage` y compañía. Leer una
        // captura truncada hizo creer que el evento no tenía tipo, y el turno
        // nunca se daba por terminado.
        let events = translate(
            r#"{"is_error":false,"num_turns":1,"stop_reason":"end_turn","usage":{"x":1},"subtype":"success","type":"result","total_cost_usd":0.12}"#,
        );
        assert!(
            matches!(events[0], AgentEvent::Finished { .. }),
            "el resumen final tiene que cerrar el turno, salió {:?}",
            events[0]
        );
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
