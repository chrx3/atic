//! Antigravity (`agy`), el sucesor del Gemini CLI.
//!
//! Habla NDJSON en las dos direcciones y **no** es el `stream-json` de Claude
//! Code, aunque los flags se parezcan. Entra una línea por turno:
//!
//! ```json
//! {"event":"user","message":{"role":"user","content":"…"}}
//! ```
//!
//! y salen tres tipos de evento:
//!
//! - `init` una vez, con `conversation_id`, modelo y cwd;
//! - `step_update` por cada paso, numerado con `step_index`, en estados
//!   `ACTIVE` → `DONE`/`ERROR`. Los de `step_type: "agent_response"` traen el
//!   texto en `text_delta` **incremental** (no acumulado); los de
//!   `step_type: "tool"` traen `tool_info` con los parámetros y, al terminar,
//!   `output` o `error.message`;
//! - `result` al cerrar el turno, con `status` y la respuesta entera.
//!
//! El proceso sobrevive a los turnos: se le escribe una línea por mensaje y
//! contesta con su propio `result` cada vez, igual que Codex con `app-server`.
//!
//! Lo que no tiene, y por eso no está acá: cancelación (no acepta ningún evento
//! de interrupción por stdin) y servidores MCP por invocación —el suyo se
//! registra en su config global, ver [`registrar_mcp`]—.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::model::{
    AgentDelta, Item, ItemKind, Role, ThreadPatch, ToolKind, ToolStatus, TurnStatus,
};
use super::turns::{end_turn, ensure_turn, start_turn, Emit, Turns};
use super::{AgentBackend, AgentSession, StartOptions};

const PROGRAM: &str = "agy";

/// Cuánto de la salida de una herramienta se guarda. La de `run_command` puede
/// ser un volcado entero y la vista solo muestra el principio.
const MAX_OUTPUT: usize = 8 * 1024;

pub struct Antigravity;

impl AgentBackend for Antigravity {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn display_name(&self) -> &'static str {
        "Antigravity"
    }

    fn is_available(&self) -> bool {
        super::exe::resolve(PROGRAM).is_some()
    }

    fn signed_in(&self) -> Option<bool> {
        super::login::antigravity()
    }

    fn start(
        &self,
        options: StartOptions,
        on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String> {
        if options.mcp_config.is_some() {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                tracing::warn!(
                    backend = "antigravity",
                    "los servidores MCP del modal solo se aplican a Claude Code"
                )
            });
        }
        // El de orquestación es lo único que sí se le deja: `agy` no acepta
        // servidores por invocación, así que va a su config global.
        if let Some(atic) = &options.atic_mcp {
            registrar_mcp(atic);
        }

        let (program, prefix) = super::exe::launcher(PROGRAM).ok_or_else(|| {
            "no se encontró «agy» en el PATH. Instálalo y ábrelo una vez en la consola.".to_string()
        })?;

        let mut cmd = Command::new(program);
        cmd.args(prefix)
            .args(args(&options))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.envs(options.env.iter().cloned());
        if let Some(dir) = &options.cwd {
            cmd.current_dir(dir);
        }

        #[cfg(windows)]
        {
            // Sin esto, cada sesión abre una consola negra sobre la app.
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("no se pudo arrancar «agy»: {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "«agy» no dio stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "«agy» no dio stdout".to_string())?;
        let stderr = child.stderr.take();

        let emit = Emit::new(on_delta);
        let turns = Arc::new(Mutex::new(Turns::default()));
        let cerrando = Arc::new(AtomicBool::new(false));

        {
            let (emit, turns, cerrando) = (emit.clone(), turns.clone(), cerrando.clone());
            let mut tr = Traductor::nuevo();
            std::thread::spawn(move || {
                for linea in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let Ok(evento) = serde_json::from_str::<Value>(&linea) else {
                        continue;
                    };
                    emit.all(tr.traducir(&evento, &turns));
                }
                // Se acabó stdout. Si fue `stop`, es lo esperado; si el proceso
                // se cayó solo a mitad de un turno, hay que cerrarlo: quien
                // espera por `atic_wait` no despierta hasta que alguien diga
                // que el turno terminó.
                if cerrando.load(Ordering::SeqCst) {
                    return;
                }
                emit.all(cierre_por_caida(&turns));
            });
        }

        // El stderr de `agy` son avisos sueltos; sin leerlo, el pipe se llena y
        // el proceso se traba a mitad de un turno.
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                for linea in BufReader::new(stderr).lines().map_while(Result::ok) {
                    tracing::debug!(backend = "antigravity", linea, "stderr");
                }
            });
        }

        Ok(Box::new(Sesion {
            child,
            stdin: Some(stdin),
            emit,
            turns,
            cerrando,
        }))
    }
}

/// La línea de comandos de una sesión.
///
/// `--print=` pegado al flag y vacío es a propósito: en modo stream el prompt
/// llega por stdin, pero `--print` separado se come el flag siguiente como si
/// fuera el texto («--print took "--input-format" as its prompt»).
fn args(options: &StartOptions) -> Vec<String> {
    let mut args = vec![
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--print=".to_string(),
    ];
    if let Some(model) = &options.model {
        args.push("--model".to_string());
        args.push(model.clone());
    }
    match options.permission_mode.as_deref() {
        // `accept-edits` no alcanza para `run_command`: sigue pidiendo permiso
        // y, sin nadie que conteste, la herramienta muere con TOOL_ERROR.
        Some("bypassPermissions") => args.push("--dangerously-skip-permissions".to_string()),
        Some("plan") => {
            args.push("--mode".to_string());
            args.push("plan".to_string());
        }
        Some("acceptEdits") => {
            args.push("--mode".to_string());
            args.push("accept-edits".to_string());
        }
        _ => {}
    }
    for dir in &options.add_dirs {
        args.push("--add-dir".to_string());
        args.push(dir.clone());
    }
    if let Some(id) = &options.resume {
        args.push("--conversation".to_string());
        args.push(id.clone());
    }
    args
}

/// Deja el servidor `atic` en la config global de `agy`.
///
/// Es el único backend donde Atic escribe fuera de su propia casa, y es porque
/// no hay otra: `agy` no acepta servidores por invocación. `mcp add` es «add or
/// update», así que repetirlo no duplica nada. Queda puesto después de cerrar
/// Atic; el sidecar sin hub contesta que Atic no está abierto, que es
/// exactamente lo que pasaría con el snippet pegado a mano.
fn registrar_mcp(atic: &super::hub::AticMcp) {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let Some((program, prefix)) = super::exe::launcher(PROGRAM) else {
            return;
        };
        let mut cmd = Command::new(program);
        cmd.args(prefix)
            .args(["mcp", "add", "atic"])
            .arg(&atic.command)
            .args(&atic.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.status() {
            Ok(s) if s.success() => tracing::info!(backend = "antigravity", "servidor «atic» registrado"),
            Ok(s) => tracing::warn!(backend = "antigravity", code = s.code(), "«agy mcp add» falló"),
            Err(e) => tracing::warn!(backend = "antigravity", error = %e, "no se pudo correr «agy mcp add»"),
        }
    });
}

/// Traduce los eventos de `agy` a deltas.
///
/// Guarda qué pasos ya se anunciaron porque el `text_delta` es incremental: el
/// primero abre el item y los siguientes son trozos del mismo.
struct Traductor {
    abiertos: std::collections::HashSet<u64>,
}

impl Traductor {
    fn nuevo() -> Self {
        Self {
            abiertos: std::collections::HashSet::new(),
        }
    }

    fn traducir(&mut self, evento: &Value, turns: &Mutex<Turns>) -> Vec<AgentDelta> {
        match evento.get("event").and_then(Value::as_str) {
            Some("init") => self.init(evento.get("init")),
            Some("step_update") => self.paso(evento.get("step_update"), turns),
            Some("result") => self.resultado(evento.get("result"), turns),
            _ => Vec::new(),
        }
    }

    fn init(&mut self, init: Option<&Value>) -> Vec<AgentDelta> {
        let patch = ThreadPatch {
            provider_session: texto(init, "conversation_id"),
            model: texto(init, "model"),
            cwd: texto(init, "cwd"),
            ..Default::default()
        };
        vec![AgentDelta::ThreadPatch { patch }]
    }

    fn paso(&mut self, paso: Option<&Value>, turns: &Mutex<Turns>) -> Vec<AgentDelta> {
        let Some(paso) = paso else {
            return Vec::new();
        };
        let Some(indice) = paso.get("step_index").and_then(Value::as_u64) else {
            return Vec::new();
        };
        let estado = paso.get("state").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();
        match paso.get("step_type").and_then(Value::as_str) {
            // El eco del mensaje que acabamos de mandar: el item del usuario ya
            // lo creó `send`, y anunciarlo otra vez lo duplicaría en la vista.
            Some("user_input") => {}
            Some("agent_response") => {
                let texto = paso
                    .get("text_delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                // Un paso puede ser solo razonamiento y no traer texto; sin
                // esto la vista se llena de mensajes vacíos.
                if texto.is_empty() && !self.abiertos.contains(&indice) {
                    return out;
                }
                let id = format!("m{indice}");
                let turn = ensure_turn(turns, &mut out);
                if self.abiertos.insert(indice) {
                    out.push(AgentDelta::ItemAdd {
                        turn,
                        item: Item::new(
                            id,
                            ItemKind::Message {
                                role: Role::Assistant,
                                text: texto.to_string(),
                                streaming: estado == "ACTIVE",
                            },
                        ),
                    });
                } else {
                    if !texto.is_empty() {
                        out.push(AgentDelta::ItemChunk {
                            item: id.clone(),
                            text: texto.to_string(),
                        });
                    }
                    if estado != "ACTIVE" {
                        out.push(AgentDelta::ItemPatch {
                            item: id,
                            patch: super::model::ItemPatch {
                                streaming: Some(false),
                                ..Default::default()
                            },
                        });
                    }
                }
            }
            Some("tool") => {
                let info = paso.get("tool_info");
                let nombre = paso
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("herramienta");
                let entrada = info
                    .and_then(|i| i.get("parameters"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let salida = info
                    .and_then(|i| {
                        i.get("output")
                            .and_then(Value::as_str)
                            .or_else(|| i.get("error").and_then(|e| e.get("message"))?.as_str())
                    })
                    .map(recortar)
                    .unwrap_or_default();
                let id = format!("t{indice}");
                let turn = ensure_turn(turns, &mut out);
                let status = match estado {
                    "ACTIVE" => ToolStatus::InProgress,
                    "ERROR" => ToolStatus::Failed,
                    _ => ToolStatus::Completed,
                };
                if self.abiertos.insert(indice) {
                    out.push(AgentDelta::ItemAdd {
                        turn,
                        item: Item::new(
                            id,
                            ItemKind::Tool {
                                name: nombre.to_string(),
                                title: titulo(nombre, &entrada),
                                tool_kind: categoria(nombre),
                                status,
                                input: entrada,
                                output: salida,
                                locations: Vec::new(),
                            },
                        ),
                    });
                } else {
                    out.push(AgentDelta::ItemPatch {
                        item: id,
                        patch: super::model::ItemPatch {
                            // Sin tipo en el parche: el mismo campo lleva el
                            // estado de una herramienta o el de un permiso.
                            status: Some(json!(status)),
                            output: Some(salida),
                            ..Default::default()
                        },
                    });
                }
            }
            _ => {}
        }
        out
    }

    fn resultado(&mut self, resultado: Option<&Value>, turns: &Mutex<Turns>) -> Vec<AgentDelta> {
        let mut out = Vec::new();
        let turn = ensure_turn(turns, &mut out);
        let status = match resultado
            .and_then(|r| r.get("status"))
            .and_then(Value::as_str)
        {
            Some("SUCCESS") => TurnStatus::Done,
            Some("CANCELLED") => TurnStatus::Cancelled,
            _ => TurnStatus::Failed,
        };
        // Un error de protocolo (una línea mal armada) no deja ningún paso, así
        // que el turno terminaría sin decir qué pasó.
        if status == TurnStatus::Failed {
            if let Some(error) = resultado
                .and_then(|r| r.get("error"))
                .and_then(Value::as_str)
                .filter(|e| !e.is_empty())
            {
                out.push(AgentDelta::ItemAdd {
                    turn: turn.clone(),
                    item: Item::new(
                        format!("e{}", self.abiertos.len()),
                        ItemKind::Message {
                            role: Role::Assistant,
                            text: error.to_string(),
                            streaming: false,
                        },
                    ),
                });
            }
        }
        out.push(AgentDelta::TurnEnd {
            turn,
            status,
            cost_usd: None,
        });
        end_turn(turns);
        // Los índices vuelven a empezar en cada turno.
        self.abiertos.clear();
        out
    }
}

fn texto(v: Option<&Value>, clave: &str) -> Option<String> {
    v.and_then(|v| v.get(clave))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn recortar(s: &str) -> String {
    if s.len() <= MAX_OUTPUT {
        return s.to_string();
    }
    let corte = (0..=MAX_OUTPUT)
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0);
    format!("{}\n[…recortado…]", &s[..corte])
}

/// Nombres de `agy`, que no son los de Claude: `ToolKind::guess` no sirve acá.
fn categoria(nombre: &str) -> ToolKind {
    match nombre {
        "view_file" | "read_resource" | "read_url_content" | "notebook_execution" => ToolKind::Read,
        "write_to_file"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "sed_file"
        | "notebook_edit" => ToolKind::Edit,
        "find_by_name" | "grep_search" | "list_dir" => ToolKind::Search,
        "run_command" | "command_status" | "send_command_input" => ToolKind::Execute,
        "search_web" | "open_browser_url" | "read_browser_page" => ToolKind::Fetch,
        "invoke_subagent" | "define_subagent" | "manage_subagents" | "browser_subagent" => {
            ToolKind::Collab
        }
        "manage_task" | "schedule" | "finish" => ToolKind::Think,
        "ask_permission" | "ask_custom_permission" | "ask_question" => ToolKind::Collab,
        _ => ToolKind::Other,
    }
}

/// Una línea legible para la vista: el parámetro que dice qué se tocó.
fn titulo(nombre: &str, entrada: &Value) -> String {
    for clave in [
        "CommandLine",
        "DirectoryPath",
        "AbsolutePath",
        "TargetFile",
        "Query",
        "Url",
    ] {
        if let Some(v) = entrada.get(clave).and_then(Value::as_str) {
            return v.to_string();
        }
    }
    nombre.to_string()
}

/// Cierra el turno abierto porque el proceso se fue sin avisar.
fn cierre_por_caida(turns: &Mutex<Turns>) -> Vec<AgentDelta> {
    let mut out = Vec::new();
    let abierto = match turns.lock() {
        Ok(t) => t.current.clone(),
        Err(e) => e.into_inner().current.clone(),
    };
    if let Some(turn) = abierto {
        out.push(AgentDelta::TurnEnd {
            turn,
            status: TurnStatus::Failed,
            cost_usd: None,
        });
        end_turn(turns);
    }
    out.push(AgentDelta::Failed {
        message: "«agy» se cerró en mitad del turno.".to_string(),
    });
    out
}

struct Sesion {
    child: Child,
    stdin: Option<ChildStdin>,
    emit: Emit,
    turns: Arc<Mutex<Turns>>,
    /// Lo enciende `stop` para que el fin de stdout no se lea como una caída.
    cerrando: Arc<AtomicBool>,
}

impl AgentSession for Sesion {
    fn send(&mut self, text: &str, origin: Option<super::model::Origin>) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "la sesión de Antigravity ya está cerrada".to_string())?;
        let turn = start_turn(&self.turns, &self.emit);
        self.emit.send(AgentDelta::ItemAdd {
            turn,
            item: Item::new(
                format!("u{}", now_ms()),
                ItemKind::Message {
                    role: Role::User,
                    text: text.to_string(),
                    streaming: false,
                },
            )
            .con_origen(origin),
        });
        let linea = json!({
            "event": "user",
            "message": { "role": "user", "content": text },
        });
        writeln!(stdin, "{linea}").map_err(|e| format!("no se pudo escribirle a «agy»: {e}"))?;
        stdin
            .flush()
            .map_err(|e| format!("no se pudo escribirle a «agy»: {e}"))
    }

    fn stop(&mut self) {
        self.cerrando.store(true, Ordering::SeqCst);
        // Cerrar stdin primero: `agy` termina solo al ver el EOF y así guarda la
        // conversación; el kill queda de red por si no lo hace.
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::MutexExt;

    fn opciones() -> StartOptions {
        StartOptions {
            cwd: Some("c:/repo".into()),
            ..Default::default()
        }
    }

    #[test]
    fn el_print_va_pegado_y_vacio() {
        let args = args(&opciones());
        assert!(
            args.contains(&"--print=".to_string()),
            "separado, «agy» toma el flag siguiente como prompt: {args:?}"
        );
        assert!(args
            .windows(2)
            .any(|p| p == ["--input-format", "stream-json"]));
        assert!(args
            .windows(2)
            .any(|p| p == ["--output-format", "stream-json"]));
    }

    #[test]
    fn bypass_salta_los_permisos_y_plan_no() {
        let mut o = opciones();
        o.permission_mode = Some("bypassPermissions".into());
        assert!(args(&o).contains(&"--dangerously-skip-permissions".to_string()));
        o.permission_mode = Some("plan".into());
        let args = args(&o);
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.windows(2).any(|p| p == ["--mode", "plan"]));
    }

    #[test]
    fn reanudar_pide_la_conversacion() {
        let mut o = opciones();
        o.resume = Some("abc-123".into());
        assert!(args(&o)
            .windows(2)
            .any(|p| p == ["--conversation", "abc-123"]));
    }

    /// Traduce una línea como la que manda `agy` y devuelve los deltas.
    fn deltas(tr: &mut Traductor, turns: &Mutex<Turns>, linea: &str) -> Vec<AgentDelta> {
        tr.traducir(&serde_json::from_str(linea).unwrap(), turns)
    }

    #[test]
    fn el_texto_del_asistente_se_abre_una_vez_y_despues_son_trozos() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":5,"state":"ACTIVE","step_type":"agent_response","text_delta":"En el "}}"#,
        );
        // Abre turno e item.
        assert!(matches!(out[0], AgentDelta::TurnStart { .. }));
        let AgentDelta::ItemAdd { item, .. } = &out[1] else {
            panic!("se esperaba el item: {out:?}");
        };
        assert!(matches!(
            &item.kind,
            ItemKind::Message { text, streaming: true, .. } if text == "En el "
        ));

        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":5,"state":"DONE","step_type":"agent_response","text_delta":"directorio"}}"#,
        );
        assert!(matches!(&out[0], AgentDelta::ItemChunk { text, .. } if text == "directorio"));
        assert!(
            matches!(&out[1], AgentDelta::ItemPatch { patch, .. } if patch.streaming == Some(false))
        );
    }

    #[test]
    fn un_paso_sin_texto_no_deja_mensaje_vacio() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"agent_response","usage":{"output_tokens":300}}}"#,
        );
        assert!(out.is_empty(), "solo razonamiento: {out:?}");
    }

    #[test]
    fn la_herramienta_se_abre_activa_y_se_cierra_con_su_salida() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"Get-Location"}}}}"#,
        );
        let AgentDelta::ItemAdd { item, .. } = &out[1] else {
            panic!("se esperaba el item: {out:?}");
        };
        assert!(matches!(
            &item.kind,
            ItemKind::Tool { title, tool_kind: ToolKind::Execute, status: ToolStatus::InProgress, .. }
                if title == "Get-Location"
        ));

        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"Get-Location"},"output":"c:/repo"}}}"#,
        );
        let AgentDelta::ItemPatch { patch, .. } = &out[0] else {
            panic!("se esperaba el parche: {out:?}");
        };
        assert_eq!(patch.status, Some(json!(ToolStatus::Completed)));
        assert_eq!(patch.output.as_deref(), Some("c:/repo"));
    }

    #[test]
    fn el_error_de_la_herramienta_queda_como_salida() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"parameters":{}}}}"#,
        );
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":2,"state":"ERROR","step_type":"tool","tool_name":"run_command","tool_info":{"parameters":{},"error":{"type":"TOOL_ERROR","message":"user denied permission"}}}}"#,
        );
        let AgentDelta::ItemPatch { patch, .. } = &out[0] else {
            panic!("se esperaba el parche: {out:?}");
        };
        assert_eq!(patch.status, Some(json!(ToolStatus::Failed)));
        assert_eq!(patch.output.as_deref(), Some("user denied permission"));
    }

    #[test]
    fn el_resultado_cierra_el_turno_y_reinicia_los_indices() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"hola"}}"#,
        );
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"result","result":{"status":"SUCCESS","response":"hola"}}"#,
        );
        assert!(matches!(
            out.last(),
            Some(AgentDelta::TurnEnd {
                status: TurnStatus::Done,
                ..
            })
        ));
        assert!(
            turns.lock_or_recover().current.is_none(),
            "el turno queda cerrado"
        );
        assert!(tr.abiertos.is_empty(), "los step_index vuelven a empezar");
    }

    #[test]
    fn un_error_de_protocolo_se_cuenta_en_el_turno() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"result","result":{"status":"ERROR","response":"","error":"stream input message is missing the \"event\" field"}}"#,
        );
        let AgentDelta::ItemAdd { item, .. } = &out[1] else {
            panic!("se esperaba el mensaje del error: {out:?}");
        };
        assert!(matches!(
            &item.kind,
            ItemKind::Message { text, .. } if text.contains("missing the")
        ));
        assert!(matches!(
            out.last(),
            Some(AgentDelta::TurnEnd {
                status: TurnStatus::Failed,
                ..
            })
        ));
    }

    #[test]
    fn si_el_proceso_se_cae_el_turno_abierto_se_cierra() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        deltas(
            &mut tr,
            &turns,
            r#"{"event":"step_update","step_update":{"step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"empiezo"}}"#,
        );
        let out = cierre_por_caida(&turns);
        assert!(matches!(
            out.first(),
            Some(AgentDelta::TurnEnd {
                status: TurnStatus::Failed,
                ..
            })
        ));
        assert!(matches!(out.last(), Some(AgentDelta::Failed { .. })));
        assert!(turns.lock_or_recover().current.is_none());
    }

    #[test]
    fn sin_turno_abierto_la_caida_solo_avisa() {
        let turns = Mutex::new(Turns::default());
        let out = cierre_por_caida(&turns);
        assert_eq!(out.len(), 1, "no inventa un turno para cerrarlo: {out:?}");
        assert!(matches!(out[0], AgentDelta::Failed { .. }));
    }

    #[test]
    fn el_init_informa_la_conversacion() {
        let mut tr = Traductor::nuevo();
        let turns = Mutex::new(Turns::default());
        let out = deltas(
            &mut tr,
            &turns,
            r#"{"event":"init","init":{"conversation_id":"c1","model":"gemini-3.8-flash-low","cwd":"c:/repo"}}"#,
        );
        let AgentDelta::ThreadPatch { patch } = &out[0] else {
            panic!("se esperaba el parche del hilo: {out:?}");
        };
        assert_eq!(patch.provider_session.as_deref(), Some("c1"));
        assert_eq!(patch.model.as_deref(), Some("gemini-3.8-flash-low"));
    }
}
