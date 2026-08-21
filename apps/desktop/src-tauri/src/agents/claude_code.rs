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

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

use super::model::{
    AgentDelta, Item, ItemId, ItemKind, ItemPatch, Origin, PermissionStatus, Role, ThreadPatch,
    ToolKind, ToolStatus, TurnId, TurnStatus,
};
use super::turns::{end_turn, ensure_turn, start_turn, Emit, Turns};

use super::{
    AgentBackend, AgentSession, McpServerState, PermissionDecision, SlashCommand, StartOptions,
};

pub struct ClaudeCode;

/// Tope del resultado que se guarda como resumen de un subagente.
const MAX_COLLAB_SUMMARY: usize = 8 * 1024;

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
        on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String> {
        let claude_args = claude_cli_args(&options);
        let remote = options.remote.clone();
        let (mut cmd, askpass) = if let Some(remote) = &remote {
            let bin = remote
                .host
                .remote_agent_bin
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("claude");
            let remote_cmdline =
                super::ssh::remote_shell_command(options.cwd.as_deref(), bin, &claude_args);
            // Con passphrase en keyring hace falta askpass (no BatchMode).
            // Sin ella, BatchMode evita colgar la UI pidiendo password.
            let batch = !atic_core::secrets::has_ssh_host_secret(&remote.host.id, "passphrase");
            super::ssh::build_ssh_command(&remote.host, &remote_cmdline, batch)
                .map_err(|e| format!("no se pudo preparar SSH para Claude Code: {e}"))?
        } else {
            let mut cmd = Command::new("claude");
            cmd.args(&claude_args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(dir) = &options.cwd {
                cmd.current_dir(dir);
            }
            #[cfg(windows)]
            {
                // Sin esto, cada turno abre una consola negra sobre la app.
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            (cmd, None)
        };

        let mut child = cmd.spawn().map_err(|e| {
            if remote.is_some() {
                format!(
                    "no se pudo iniciar Claude Code vía SSH: {e}. \
¿Está `ssh` en el PATH y `claude` en el host remoto?"
                )
            } else {
                format!("no se pudo iniciar Claude Code: {e}")
            }
        })?;

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

        // Reglas sugeridas por pedido de permiso, puestas por el lector y leídas
        // al contestar. Un `Mutex` y no un canal: no hay que esperar nada, solo
        // consultar lo último que se vio.
        let rules: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
        let turns = Arc::new(Mutex::new(Turns::default()));
        let emit = Emit::new(on_delta);

        // stderr en su propio hilo, y FUERA del `if` que lo obtiene. Antes todo
        // el lector de stdout colgaba de `if let Some(stderr)`: sin stderr no se
        // leía tampoco stdout, y la sesión se veía viva sin recibir un solo
        // evento. Nunca pasó porque stderr siempre se canaliza, pero la
        // dependencia entre los dos flujos no tenía por qué existir.
        if let Some(stderr) = child.stderr.take() {
            let emit = emit.clone();
            let turns = turns.clone();
            thread::spawn(move || {
                // Contador propio con etiqueta propia: los ids son
                // `{turno}-{etiqueta}{n}`, así que `e1` nunca choca con los
                // `m1`/`n1` que acuña el traductor en el otro hilo.
                let mut seq = 0u64;
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    seq += 1;
                    let mut out = Vec::new();
                    let turn = ensure_turn(&turns, &mut out);
                    out.push(AgentDelta::ItemAdd {
                        turn: turn.clone(),
                        item: Item::new(format!("{turn}-e{seq}"), ItemKind::Notice { text: line }),
                    });
                    emit.all(out);
                }
            });
        }

        // Tras `--resume`, el CLI a veces reinyecta historial / compactaciones
        // por stdout. Eso no es el chat vivo de Atic: se silencia hasta el
        // primer `send` del usuario. ThreadPatch y permisos sí pasan.
        let mute_history = Arc::new(AtomicBool::new(options.resume.is_some()));
        let interrupted = Arc::new(AtomicBool::new(false));

        let died = stopping.clone();
        let mut tr = Translator::new(
            turns.clone(),
            rules.clone(),
            mute_history.clone(),
            interrupted.clone(),
        );
        let emit_out = emit.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                for delta in tr.translate(&line) {
                    emit_out.send(delta);
                }
            }
            // stdout cerrado = el proceso terminó. Si nadie lo pidió, se cayó, y
            // callarlo dejaría a la UI esperando para siempre.
            if !died.load(Ordering::SeqCst) {
                emit_out.send(AgentDelta::Failed {
                    message: "El agente terminó inesperadamente.".to_string(),
                });
            }
        });

        let mut session = ClaudeSession {
            child,
            stdin: Some(stdin),
            stopping,
            rules,
            turns,
            emit,
            mute_history,
            interrupted,
            // El script askpass tiene que vivir hasta que SSH autentique.
            _askpass: askpass,
        };
        // Handshake del canal de control. Abre la vía por la que después llegan
        // las pedidas de permiso; sin él, el turno se bloquearía esperando una
        // respuesta que nadie podría mandar.
        session.control(json!({ "subtype": "initialize" }))?;

        // Claude Code no tiene una llamada que liste modelos, así que los
        // informa el adaptador. No envejece como una lista de identificadores
        // completos porque son ALIAS: `opus` apunta siempre al último de su
        // familia, y esa indirección la mantiene Anthropic, no nosotros.
        let mode = cli_permission_mode(options.permission_mode.as_deref()).to_string();
        session.emit.send(AgentDelta::ThreadPatch {
            patch: ThreadPatch {
                models: Some(super::discover::claude_fallback_models()),
                mode: Some(mode),
                ..Default::default()
            },
        });
        Ok(Box::new(session))
    }
}

/// Traduce el stream-json de Claude Code al modelo canónico.
///
/// # Por qué tiene estado
///
/// La versión anterior era una función pura de línea a eventos, y podía serlo
/// porque su salida era un registro plano: cada línea producía eventos que no
/// sabían nada de los anteriores. Con items que **mutan**, hay tres cosas que
/// solo se saben mirando lo que ya pasó:
///
///  - a qué turno pertenece lo que llega,
///  - a qué item hay que pegarle un trozo de texto,
///  - qué item corresponde al `tool_result` que aparece varias líneas después
///    del `tool_use` que lo pidió.
///
/// Para lo tercero no hace falta tabla: el `tool_use.id` del CLI ya es estable
/// y único, así que se usa **ese** como `ItemId` y el emparejamiento sale gratis.
struct Translator {
    turns: Arc<Mutex<Turns>>,
    /// Reglas sugeridas por pedido de permiso, para poder contestar «siempre».
    ///
    /// Se guardan acá y no viajan a la interfaz a propósito: son la forma
    /// interna del CLI para «graba esta regla», y hacerlas ir y volver
    /// obligaría a las vistas a entender un detalle del protocolo para dibujar
    /// un botón.
    rules: Arc<Mutex<HashMap<String, Value>>>,
    /// Item de texto que está recibiendo trozos ahora mismo.
    ///
    /// Es lo que reemplaza al `streaming: String` que vivía suelto en el
    /// frontend: acá el trozo sabe a qué pertenece, así que dos bloques
    /// transmitiendo a la vez ya no se pisan.
    streaming: Option<ItemId>,
    /// Contador para los items que el CLI no nombra (mensajes, razonamiento).
    seq: u64,
    /// Ids que corresponden a `Task`/`Agent`, para parchear su resultado como
    /// colaboración y no como herramienta genérica.
    collab: HashSet<ItemId>,
    /// Silenciar transcript de replay tras `--resume` hasta el primer send.
    mute_history: Arc<AtomicBool>,
    /// El usuario pidió interrumpir: el próximo `result` cierra como cancelado.
    interrupted: Arc<AtomicBool>,
}

impl Translator {
    fn new(
        turns: Arc<Mutex<Turns>>,
        rules: Arc<Mutex<HashMap<String, Value>>>,
        mute_history: Arc<AtomicBool>,
        interrupted: Arc<AtomicBool>,
    ) -> Self {
        Self {
            turns,
            rules,
            streaming: None,
            seq: 0,
            collab: HashSet::new(),
            mute_history,
            interrupted,
        }
    }

    fn history_muted(&self) -> bool {
        self.mute_history.load(Ordering::SeqCst)
    }

    fn turn(&mut self, out: &mut Vec<AgentDelta>) -> TurnId {
        ensure_turn(&self.turns, out)
    }

    /// Un id para un item que el CLI no nombra.
    fn next_id(&mut self, turn: &str, tag: &str) -> ItemId {
        self.seq += 1;
        format!("{turn}-{tag}{}", self.seq)
    }

    /// Cierra el bloque de texto que estuviera transmitiendo.
    ///
    /// El texto completo **reemplaza** lo acumulado en vez de sumarse: cuando el
    /// CLI manda el bloque cerrado, esa versión es la autoritativa y los trozos
    /// que fuimos juntando ya cumplieron su función de mostrar avance. Sumarlos
    /// escribiría la respuesta dos veces.
    fn close_streaming(&mut self, full: Option<&str>, out: &mut Vec<AgentDelta>) -> bool {
        let Some(id) = self.streaming.take() else {
            return false;
        };
        out.push(AgentDelta::ItemPatch {
            item: id,
            patch: ItemPatch {
                text: full.map(str::to_string),
                streaming: Some(false),
                ..Default::default()
            },
        });
        true
    }

    /// Traduce una línea a cero o más deltas.
    ///
    /// Una línea que no se entiende NO se descarta: sale como `Notice`. El
    /// formato del CLI va a crecer, y un adaptador que traga en silencio lo que
    /// no conoce hace que los eventos nuevos se vean como si nada pasara.
    fn translate(&mut self, line: &str) -> Vec<AgentDelta> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return self.notice(line);
        };
        let mut out = Vec::new();

        match v.get("type").and_then(Value::as_str) {
            // ── El agente quedó esperando una respuesta nuestra ────────────
            Some("control_request")
                if v.get("request")
                    .and_then(|r| r.get("subtype"))
                    .and_then(Value::as_str)
                    == Some("can_use_tool") =>
            {
                let req = v.get("request").cloned().unwrap_or(Value::Null);
                // El id del control, no el del tool_use: es lo que hay que
                // devolver para desbloquear el turno.
                let id = str_at(&v, "request_id");
                if let Some(s) = req.get("permission_suggestions") {
                    if let Ok(mut map) = self.rules.lock() {
                        map.insert(id.clone(), s.clone());
                    }
                }
                let turn = self.turn(&mut out);
                out.push(AgentDelta::ItemAdd {
                    turn,
                    item: Item::new(
                        id,
                        ItemKind::Permission {
                            tool: str_at(&req, "tool_name"),
                            description: str_at(&req, "description"),
                            input: req.get("input").cloned().unwrap_or(Value::Null),
                            status: PermissionStatus::Pending,
                        },
                    ),
                });
            }

            // ── Catálogo de comandos (respuesta al handshake) ──────────────
            Some("control_response") => {
                if let Some(cmds) = parse_control_commands(&v) {
                    out.push(AgentDelta::ThreadPatch {
                        patch: ThreadPatch {
                            commands: Some(cmds),
                            ..Default::default()
                        },
                    });
                }
            }

            Some("control_cancel_request") => {}

            // ── Escritura en curso ────────────────────────────────────────
            //
            // Solo interesa el texto: la apertura y el cierre de bloque, el uso
            // de tokens y los deltas de herramienta ya llegan resueltos en el
            // mensaje completo.
            Some("stream_event") => {
                if self.history_muted() {
                    return Vec::new();
                }
                let delta = v.get("event").and_then(|e| e.get("delta"));
                if delta.and_then(|d| d.get("type")).and_then(Value::as_str) == Some("text_delta") {
                    if let Some(text) = delta.and_then(|d| d.get("text")).and_then(Value::as_str) {
                        // El primer trozo crea el item; los demás se le pegan.
                        let id = match &self.streaming {
                            Some(id) => id.clone(),
                            None => {
                                let turn = self.turn(&mut out);
                                let id = self.next_id(&turn, "m");
                                out.push(AgentDelta::ItemAdd {
                                    turn,
                                    item: Item::new(
                                        id.clone(),
                                        ItemKind::Message {
                                            role: Role::Assistant,
                                            text: String::new(),
                                            streaming: true,
                                        },
                                    ),
                                });
                                self.streaming = Some(id.clone());
                                id
                            }
                        };
                        out.push(AgentDelta::ItemChunk {
                            item: id,
                            text: text.to_string(),
                        });
                    }
                }
            }

            // ── Arranque de la sesión ─────────────────────────────────────
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
                // `slash_commands` del init son solo nombres; el handshake
                // `initialize` trae description/hint después y pisa esto.
                let names = strings_at(&v, "slash_commands");
                let interim = (!names.is_empty()).then(|| {
                    names
                        .into_iter()
                        .filter(|n| !n.is_empty())
                        .map(|name| SlashCommand {
                            name,
                            description: String::new(),
                            argument_hint: String::new(),
                        })
                        .collect::<Vec<_>>()
                });
                out.push(AgentDelta::ThreadPatch {
                    patch: ThreadPatch {
                        provider_session: Some(str_at(&v, "session_id")),
                        cwd: Some(str_at(&v, "cwd")),
                        model: Some(str_at(&v, "model")),
                        tools: Some(strings_at(&v, "tools")),
                        mcp_servers: Some(
                            v.get("mcp_servers")
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
                        ),
                        commands: interim,
                        ..Default::default()
                    },
                });
            }

            // ── Mensaje del asistente ─────────────────────────────────────
            Some("assistant") => {
                if self.history_muted() {
                    return Vec::new();
                }
                let turn = self.turn(&mut out);
                for b in blocks(&v) {
                    match b.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let text = b.get("text").and_then(Value::as_str).unwrap_or_default();
                            // Si veníamos transmitiendo, este bloque ES el que
                            // se estaba escribiendo: se cierra en su sitio en
                            // vez de agregar un mensaje repetido debajo.
                            if !self.close_streaming(Some(text), &mut out) {
                                let id = self.next_id(&turn, "m");
                                out.push(AgentDelta::ItemAdd {
                                    turn: turn.clone(),
                                    item: Item::new(
                                        id,
                                        ItemKind::Message {
                                            role: Role::Assistant,
                                            text: text.to_string(),
                                            streaming: false,
                                        },
                                    ),
                                });
                            }
                        }

                        // Vacío es el caso normal cuando el modelo no expone el
                        // contenido, y no merece una tarjeta.
                        Some("thinking") => {
                            let text = b
                                .get("thinking")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            if !text.trim().is_empty() {
                                let id = self.next_id(&turn, "r");
                                out.push(AgentDelta::ItemAdd {
                                    turn: turn.clone(),
                                    item: Item::new(
                                        id,
                                        ItemKind::Reasoning {
                                            text: text.to_string(),
                                            streaming: false,
                                        },
                                    ),
                                });
                            }
                        }

                        Some("tool_use") => {
                            let name = str_at(&b, "name");
                            let input = b.get("input").cloned().unwrap_or(Value::Null);
                            let id = str_at(&b, "id");
                            let kind = if matches!(name.as_str(), "Task" | "Agent") {
                                self.collab.insert(id.clone());
                                let title = input
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or(&name)
                                    .to_string();
                                let subagent_type = input
                                    .get("subagent_type")
                                    .or_else(|| input.get("agent"))
                                    .and_then(Value::as_str)
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("task")
                                    .to_string();
                                ItemKind::Collab {
                                    name,
                                    title,
                                    subagent_type,
                                    status: ToolStatus::InProgress,
                                    summary: String::new(),
                                    parent_turn_id: Some(turn.clone()),
                                    creation_source: "provider_native".to_string(),
                                }
                            } else {
                                ItemKind::Tool {
                                    title: tool_title(&input),
                                    tool_kind: ToolKind::guess(&name),
                                    name,
                                    status: ToolStatus::InProgress,
                                    locations: tool_locations(&input),
                                    input,
                                    output: String::new(),
                                }
                            };
                            out.push(AgentDelta::ItemAdd {
                                turn: turn.clone(),
                                // El id del CLI es el id del item: el
                                // `tool_result` que llega después lo trae, y
                                // así el parche encuentra su tarjeta sin tabla
                                // de equivalencias.
                                item: Item::new(id, kind),
                            });
                        }
                        _ => {}
                    }
                }
                // El contexto se mide por mensaje, no al final del turno: lo que
                // se quiere ver es cómo sube mientras el agente trabaja.
                if let Some(tokens) = context_tokens(&v) {
                    out.push(AgentDelta::ThreadPatch {
                        patch: ThreadPatch::tokens(tokens),
                    });
                }
            }

            // ── Resultados de herramienta (llegan como turno de usuario) ───
            //
            // También llega acá el resumen sintético de `/compact`
            // (`isCompactSummary`): no es un mensaje del usuario, es el
            // checkpoint que Claude Code usa como contexto post-compactación.
            Some("user") => {
                if self.history_muted() {
                    return Vec::new();
                }
                for b in blocks(&v) {
                    if b.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let is_error = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    let id = b
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let result = render_content(b.get("content"));
                    let collab = self.collab.contains(&id);
                    out.push(AgentDelta::ItemPatch {
                        item: id,
                        patch: ItemPatch {
                            output: (!collab).then_some(result.clone()),
                            summary: collab.then(|| cap_collab_summary(&result)),
                            ..ItemPatch::tool_status(if is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Completed
                            })
                        },
                    });
                }
                if v.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
                    let text = user_text(&v);
                    if !text.trim().is_empty() {
                        let turn = self.turn(&mut out);
                        let id = self.next_id(&turn, "n");
                        out.push(AgentDelta::ItemAdd {
                            turn,
                            item: Item::new(
                                id,
                                ItemKind::Notice {
                                    text: format!("Resumen del contexto\n\n{text}"),
                                },
                            ),
                        });
                    }
                }
            }

            // ── Límite de uso ─────────────────────────────────────────────
            Some("rate_limit_event") => {
                let status = v
                    .get("rate_limit_info")
                    .and_then(|i| i.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("desconocido");
                // Solo molestar cuando no es el caso normal.
                if status != "allowed" {
                    return self.notice(&format!("Límite de uso: {status}"));
                }
            }

            // ── Cierre del turno ──────────────────────────────────────────
            //
            // El campo `type` aparece casi al FINAL de este objeto, después de
            // `usage`, `modelUsage` y varios más. Una captura truncada del CLI
            // lo deja fuera y hace parecer que el evento no tiene tipo — el
            // motivo por el que la primera versión de este traductor lo mandaba
            // a `Notice` y el turno nunca se daba por terminado. La rama sin
            // `type` queda como red por si algún día lo omiten de verdad.
            Some("result") | None
                if v.get("stop_reason").is_some() || v.get("is_error").is_some() =>
            {
                if self.history_muted() {
                    // Alinear el contador de turnos sin pintar nada en la UI.
                    end_turn(&self.turns);
                    return Vec::new();
                }
                // Un turno que cierra con texto a medio escribir deja el item
                // marcado como vivo para siempre, y la vista lo dibujaría con
                // el cursor parpadeando sin que nadie escriba.
                self.close_streaming(None, &mut out);
                let turn = self.turn(&mut out);
                let failed = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let cancelled = self.interrupted.swap(false, Ordering::SeqCst);
                out.push(AgentDelta::TurnEnd {
                    turn,
                    status: if cancelled {
                        TurnStatus::Cancelled
                    } else if failed {
                        TurnStatus::Failed
                    } else {
                        TurnStatus::Done
                    },
                    cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
                });
                // Cerrado: el próximo item abre uno nuevo.
                end_turn(&self.turns);
            }

            // ── Compactación de contexto (`/compact`) ─────────────────────
            //
            // Marca el checkpoint: a partir de acá el CLI manda el resumen
            // sintético y no el historial completo. Hay que pintarlo claro —
            // no como `sistema: compact_boundary`— porque durante el compact
            // el stream puede callarse un rato y la UI parece colgada.
            Some("system")
                if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") =>
            {
                if self.history_muted() {
                    return Vec::new();
                }
                let pre = v
                    .get("compactMetadata")
                    .and_then(|m| m.get("preTokens"))
                    .and_then(Value::as_u64);
                let text = match pre {
                    Some(n) => format!("Contexto compactado (antes ~{n} tokens)."),
                    None => "Contexto compactado.".to_string(),
                };
                // `preTokens` es el tamaño *antes* del compact: solo informativo.
                let notice = self.notice(&text);
                out.extend(notice);
            }

            // ── Telemetría del propio CLI ─────────────────────────────────
            //
            // En qué anda y cuánto lleva pensando. Llega muchas veces por turno
            // y no es conversación. Cayendo en el comodín se volcaba el JSON
            // crudo en medio del diálogo —visto en pantalla—, que es justo lo
            // contrario de lo que el comodín intenta lograr.
            Some("system")
                if matches!(
                    v.get("subtype").and_then(Value::as_str),
                    Some("status" | "thinking_tokens")
                ) => {}

            // Otro `system` que todavía no traducimos: se avisa que pasó, con el
            // nombre y no con la línea entera. Enterarse importa —así aparecieron
            // los dos de arriba—; leer JSON a mano, no.
            Some("system") => {
                if self.history_muted() {
                    return Vec::new();
                }
                return self.notice(&format!("sistema: {}", str_at(&v, "subtype")));
            }

            _ => {
                if self.history_muted() {
                    return Vec::new();
                }
                return self.notice(line);
            }
        }

        out
    }

    fn notice(&mut self, text: &str) -> Vec<AgentDelta> {
        let mut out = Vec::new();
        let turn = self.turn(&mut out);
        let id = self.next_id(&turn, "n");
        out.push(AgentDelta::ItemAdd {
            turn,
            item: Item::new(
                id,
                ItemKind::Notice {
                    text: text.to_string(),
                },
            ),
        });
        out
    }
}

/// Texto legible para la tarjeta: la ruta, el comando, el patrón.
///
/// Claude Code no manda un título —ACP sí, en `title`—, así que acá hay que
/// armarlo. Es el único sitio donde se adivina, y se adivina UNA vez y en el
/// backend: antes lo hacía la vista en cada render, sobre la entrada cruda.
fn tool_title(input: &Value) -> String {
    let Some(o) = input.as_object() else {
        return String::new();
    };
    for key in [
        "file_path",
        "command",
        "pattern",
        "path",
        "url",
        "query",
        "description",
        "prompt",
    ] {
        if let Some(s) = o.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

/// Archivos que la herramienta toca, para poder seguirla desde la interfaz.
fn tool_locations(input: &Value) -> Vec<String> {
    let Some(o) = input.as_object() else {
        return Vec::new();
    };
    ["file_path", "path", "notebook_path"]
        .iter()
        .filter_map(|k| o.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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

/// Comandos del `control_response` del handshake `initialize`.
///
/// El shape ha variado: a veces van en `response.response.commands`, a veces
/// colgados de `response.commands`. Aceptamos ambos.
fn parse_control_commands(v: &Value) -> Option<Vec<SlashCommand>> {
    let resp = v.get("response")?;
    let list = resp
        .get("response")
        .and_then(|inner| inner.get("commands"))
        .or_else(|| resp.get("commands"))
        .and_then(Value::as_array)?;
    let cmds: Vec<SlashCommand> = list
        .iter()
        .map(|c| {
            let hint = str_at(c, "argumentHint");
            SlashCommand {
                name: str_at(c, "name"),
                description: str_at(c, "description"),
                argument_hint: if hint.is_empty() {
                    str_at(c, "argument_hint")
                } else {
                    hint
                },
            }
        })
        .filter(|c| !c.name.is_empty())
        .collect();
    (!cmds.is_empty()).then_some(cmds)
}

/// Texto legible de un turno `user` (string suelto o bloques `text`).
fn user_text(v: &Value) -> String {
    let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
        return String::new();
    };
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(Value::as_str) == Some("text") {
                    b.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
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

fn cap_collab_summary(text: &str) -> String {
    if text.len() <= MAX_COLLAB_SUMMARY {
        return text.to_string();
    }
    let mut end = MAX_COLLAB_SUMMARY;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

struct ClaudeSession {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Puesto en `stop`/`drop` para que el lector no reporte un fallo cuando
    /// el cierre fue pedido.
    stopping: Arc<AtomicBool>,
    /// Regla sugerida por pedido de permiso, para contestar «siempre».
    rules: Arc<Mutex<HashMap<String, Value>>>,
    /// Compartido con el lector: acá se ABRE el turno, allá se cuelga de él.
    turns: Arc<Mutex<Turns>>,
    /// Para anunciar el turno del usuario. La sesión emite además de leer.
    emit: Emit,
    /// Compartido con el traductor: se apaga en el primer `send`.
    mute_history: Arc<AtomicBool>,
    /// Pedido de interrupt pendiente; el traductor lo consume al cerrar el turno.
    interrupted: Arc<AtomicBool>,
    /// Mantiene el helper SSH_ASKPASS en disco mientras viva la sesión remota.
    _askpass: Option<super::ssh::AskpassGuard>,
}

/// Flags de `claude -p …` compartidos entre spawn local y remoto.
fn claude_cli_args(options: &StartOptions) -> Vec<String> {
    let mut args = vec![
        "-p".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        // stream-json no emite el detalle sin esto.
        "--verbose".into(),
        // Sin esto el modo `manual` NO pregunta: deniega la herramienta y
        // la lista en `permission_denials` del resultado.
        "--permission-prompt-tool".into(),
        "stdio".into(),
        // Texto según se escribe.
        "--include-partial-messages".into(),
    ];
    if let Some(id) = &options.resume {
        args.push("--resume".into());
        args.push(id.clone());
        if options.fork {
            args.push("--fork-session".into());
        }
    } else if let Some(id) = &options.session_id {
        args.push("--session-id".into());
        args.push(id.clone());
    }
    if let Some(model) = &options.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(effort) = &options.effort {
        args.push("--effort".into());
        args.push(effort.clone());
    }
    args.push("--permission-mode".into());
    args.push(cli_permission_mode(options.permission_mode.as_deref()).to_string());
    if let Some(mcp) = &options.mcp_config {
        args.push("--mcp-config".into());
        args.push(mcp.clone());
    }
    for dir in &options.add_dirs {
        args.push("--add-dir".into());
        args.push(dir.clone());
    }
    args
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

    fn emit_notice(&self, turn: &str, text: &str) {
        self.emit.send(AgentDelta::ItemAdd {
            turn: turn.to_string(),
            item: Item::new(
                format!("{turn}-n"),
                ItemKind::Notice {
                    text: text.to_string(),
                },
            ),
        });
    }
}

/// Cómo tratar un slash de control en `send`.
enum ControlSlash {
    /// Cambia el modo vía `set_permission_mode` (sin burbuja ni reenvío).
    SetMode { mode: String, notice: String },
    /// Solo aviso local; la UI debía haber abierto un selector.
    NoticeOnly { text: String },
    /// Aviso + reenviar el slash al CLI (compact / effort / model / clear…).
    NoticeForward {
        text: String,
        patch: Option<ThreadPatch>,
    },
}

fn wire_permission_mode(mode: &str) -> &str {
    cli_permission_mode(Some(mode))
}

/// Modos que acepta `claude --permission-mode`. Cualquier otro valor tumba el CLI.
fn cli_permission_mode(mode: Option<&str>) -> &'static str {
    match mode {
        Some("acceptEdits") => "acceptEdits",
        Some("bypassPermissions") => "bypassPermissions",
        Some("plan") => "plan",
        Some("auto") => "auto",
        Some("dontAsk") => "dontAsk",
        Some("manual") | Some("default") | None => "manual",
        Some(_) => "manual",
    }
}

fn mode_notice(mode: &str) -> String {
    match mode {
        "plan" => "Plan mode activado.".to_string(),
        "acceptEdits" => "Permisos: Edits".to_string(),
        "bypassPermissions" => "Permisos: Bypass".to_string(),
        "manual" | "default" => "Permisos: Manual".to_string(),
        other => format!("Permisos: {other}"),
    }
}

fn known_permission_mode(mode: &str) -> bool {
    matches!(
        mode,
        "manual" | "default" | "acceptEdits" | "plan" | "bypassPermissions"
    )
}

/// Clasifica slashes de control para no pintarlos como mensaje de usuario.
fn classify_control_slash(prompt: &str) -> Option<ControlSlash> {
    let t = prompt.trim();
    if t == "/compact" || t.starts_with("/compact ") {
        return Some(ControlSlash::NoticeForward {
            text: "Compactando el contexto… Esto puede tardar un momento.".to_string(),
            patch: None,
        });
    }
    if let Some(rest) = t.strip_prefix("/effort") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Some(ControlSlash::NoticeOnly {
                text: "Esfuerzo: elige un nivel en el selector.".to_string(),
            });
        }
        if !rest.contains('\n') {
            return Some(ControlSlash::NoticeForward {
                text: format!("Esfuerzo: {rest}"),
                patch: Some(ThreadPatch {
                    effort: Some(rest.to_string()),
                    ..Default::default()
                }),
            });
        }
    }
    if let Some(rest) = t.strip_prefix("/model") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Some(ControlSlash::NoticeOnly {
                text: "Modelo: elige uno en el selector.".to_string(),
            });
        }
        if !rest.contains('\n') {
            return Some(ControlSlash::NoticeForward {
                text: format!("Modelo: {rest}"),
                patch: None,
            });
        }
    }
    if t == "/plan" {
        return Some(ControlSlash::SetMode {
            mode: "plan".to_string(),
            notice: mode_notice("plan"),
        });
    }
    if let Some(rest) = t.strip_prefix("/permissions") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Some(ControlSlash::NoticeOnly {
                text: "Permisos: elige un modo en el selector.".to_string(),
            });
        }
        if !rest.contains('\n') && known_permission_mode(rest) {
            let mode = if rest == "default" {
                "manual".to_string()
            } else {
                rest.to_string()
            };
            return Some(ControlSlash::SetMode {
                notice: mode_notice(&mode),
                mode,
            });
        }
    }
    if t == "/clear" {
        return Some(ControlSlash::NoticeForward {
            text: "Limpiando la conversación…".to_string(),
            patch: None,
        });
    }
    if t == "/context" {
        return Some(ControlSlash::NoticeForward {
            text: "Consultando uso de contexto…".to_string(),
            patch: None,
        });
    }
    if t == "/cost" || t == "/usage" {
        return Some(ControlSlash::NoticeForward {
            text: if t == "/cost" {
                "Consultando costo de la sesión…".to_string()
            } else {
                "Consultando uso de la cuenta…".to_string()
            },
            patch: None,
        });
    }
    None
}

impl AgentSession for ClaudeSession {
    fn send(&mut self, text: &str, origin: Option<Origin>) -> Result<(), String> {
        // A partir de acá el chat vivo es de Atic: dejar pasar el stream.
        self.mute_history.store(false, Ordering::SeqCst);

        // El turno lo abre QUIEN ESCRIBE, no quien lee: un turno es un ciclo
        // usuario → agente, así que empieza acá y no cuando el agente contesta.
        //
        // Y el mensaje del usuario es un item más. Antes no existía en ninguna
        // parte: el registro solo tenía lo que venía del backend, así que la
        // conversación se leía como un monólogo del agente y, al guardarla, la
        // mitad de cada intercambio no llegaba al disco.
        let files = origin.as_ref().map(|o| o.files.clone()).unwrap_or_default();
        let prompt = {
            let stripped = super::media::strip_embedded_paths(text, &files);
            if stripped.is_empty() && !files.is_empty() {
                "Mira esta imagen.".to_string()
            } else {
                stripped
            }
        };

        // Slash de control: no son chat. Se pintan como aviso, no como burbuja.
        let turn = start_turn(&self.turns, &self.emit);
        match classify_control_slash(&prompt) {
            Some(ControlSlash::SetMode { mode, notice }) => {
                self.emit_notice(&turn, &notice);
                self.emit.send(AgentDelta::ThreadPatch {
                    patch: ThreadPatch {
                        mode: Some(mode.clone()),
                        ..Default::default()
                    },
                });
                // Canal de control del CLI (no UI interactiva de `/permissions`).
                self.control(json!({
                    "subtype": "set_permission_mode",
                    "mode": wire_permission_mode(&mode),
                }))?;
                end_turn(&self.turns);
                self.emit.send(AgentDelta::TurnEnd {
                    turn,
                    status: TurnStatus::Done,
                    cost_usd: None,
                });
                return Ok(());
            }
            Some(ControlSlash::NoticeOnly { text }) => {
                self.emit_notice(&turn, &text);
                end_turn(&self.turns);
                self.emit.send(AgentDelta::TurnEnd {
                    turn,
                    status: TurnStatus::Done,
                    cost_usd: None,
                });
                return Ok(());
            }
            Some(ControlSlash::NoticeForward { text, patch }) => {
                self.emit_notice(&turn, &text);
                if let Some(patch) = patch {
                    self.emit.send(AgentDelta::ThreadPatch { patch });
                }
            }
            None => {
                self.emit.send(AgentDelta::ItemAdd {
                    turn: turn.clone(),
                    item: Item::new(
                        format!("{turn}-u"),
                        ItemKind::Message {
                            role: Role::User,
                            text: prompt.clone(),
                            streaming: false,
                        },
                    )
                    .con_origen(origin),
                });
            }
        }

        // Content multimodal: imágenes primero, texto después (como Messages API).
        let mut content = Vec::new();
        for path in &files {
            match super::media::claude_image_block(std::path::Path::new(path)) {
                Ok(block) => content.push(block),
                Err(e) => {
                    content.push(json!({
                        "type": "text",
                        "text": format!("[no se pudo adjuntar {path}: {e}]"),
                    }));
                }
            }
        }
        if !prompt.is_empty() {
            content.push(json!({ "type": "text", "text": prompt }));
        }

        // Mismo sobre que la Messages API: el CLI espera un turno de usuario.
        self.write(json!({
            "type": "user",
            "message": { "role": "user", "content": content }
        }))
    }

    /// `/model <alias>` y `/effort <nivel>` en caliente.
    ///
    /// El esfuerzo también arranca con `--effort`, pero el CLI acepta
    /// `/effort` a mitad de sesión (low|medium|high|xhigh|max|auto).
    fn set_model(
        &mut self,
        model: &str,
        effort: Option<&str>,
        _fast: Option<bool>,
    ) -> Result<(), String> {
        if !model.trim().is_empty() {
            self.send(&format!("/model {}", model.trim()), None)?;
        }
        if let Some(level) = effort.map(str::trim).filter(|s| !s.is_empty()) {
            self.send(&format!("/effort {level}"), None)?;
        }
        Ok(())
    }

    fn interrupt(&mut self) -> Result<(), String> {
        // Canal de control del CLI: aborta el turno/tool loop y deja el proceso
        // listo para el próximo prompt. Sin esto, «Detener» tenía que matar la
        // sesión entera.
        self.interrupted.store(true, Ordering::SeqCst);
        self.control(json!({ "subtype": "interrupt" }))
    }

    fn respond_permission(&mut self, id: &str, decision: PermissionDecision) -> Result<(), String> {
        // El `request_id` de la respuesta tiene que ser el del pedido: es lo
        // que empareja la contestación con el turno detenido.
        let decision = match decision {
            // Sin `updatedInput` el CLI toma la herramienta como aprobada tal
            // cual la pidió. Modificarla es otra función, no esta.
            PermissionDecision::Allow => json!({ "behavior": "allow" }),
            PermissionDecision::AllowAlways => {
                let rule = self
                    .rules
                    .lock()
                    .ok()
                    .and_then(|map| map.get(id).cloned())
                    .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()));
                match rule {
                    Some(rule) => json!({ "behavior": "allow", "updatedPermissions": rule }),
                    // El agente no sugirió ninguna regla para este caso. Aceptar
                    // la invocación es lo correcto igual: la alternativa sería
                    // fallar, y para el usuario «siempre» que no hace ni «sí»
                    // es peor que un «siempre» que valió por una vez.
                    None => json!({ "behavior": "allow" }),
                }
            }
            PermissionDecision::Deny => {
                json!({ "behavior": "deny", "message": "Permiso denegado desde Atic." })
            }
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
        // Cerrar stdin: el CLI suele terminar solo al ver EOF. Si no lo hace
        // (proceso colgado sin leer), `wait` eterno congelaba `agent_stop` y
        // cualquier otra sesión que necesitara el lock de SESSIONS.
        self.stdin.take();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(40));
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
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
    use atic_core::MutexExt;

    fn tr() -> Translator {
        Translator::new(
            Arc::new(Mutex::new(Turns::default())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn tr_muted() -> Translator {
        Translator::new(
            Arc::new(Mutex::new(Turns::default())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        )
    }

    /// Los items que agrega una tanda de deltas.
    ///
    /// Casi todas las aserciones miran esto y no el índice 0: abrir el turno
    /// puede meter un `TurnStart` delante, y encadenar posiciones haría que
    /// cualquier delta nuevo rompiera tests que no tienen nada que ver.
    fn added(ds: &[AgentDelta]) -> Vec<&Item> {
        ds.iter()
            .filter_map(|d| match d {
                AgentDelta::ItemAdd { item, .. } => Some(item),
                _ => None,
            })
            .collect()
    }

    fn patches<'p>(ds: &'p [AgentDelta], id: &str) -> Vec<&'p ItemPatch> {
        ds.iter()
            .filter_map(|d| match d {
                AgentDelta::ItemPatch { item, patch } if item == id => Some(patch),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn resume_muteado_ignora_historial_pero_deja_init() {
        let mut t = tr_muted();
        let init = t.translate(
            r#"{"type":"system","subtype":"init","cwd":"C:\\p","session_id":"s1","tools":["Bash"],"model":"opus","slash_commands":[],"mcp_servers":[]}"#,
        );
        assert!(
            init.iter()
                .any(|d| matches!(d, AgentDelta::ThreadPatch { .. })),
            "init debe pasar con mute: {init:?}"
        );
        let hist = t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"mensaje viejo"}]}}"#,
        );
        assert!(
            hist.is_empty(),
            "historial muteado no debe pintar: {hist:?}"
        );
        t.mute_history.store(false, Ordering::SeqCst);
        let live = t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"mensaje nuevo"}]}}"#,
        );
        assert!(
            !added(&live).is_empty(),
            "tras unmute sí debe pintar: {live:?}"
        );
    }

    #[test]
    fn el_init_configura_el_hilo_y_no_es_conversacion() {
        let ds = tr().translate(
            r#"{"type":"system","subtype":"init","cwd":"C:\\p","session_id":"s1","tools":["Bash","Read"],"model":"claude-sonnet-5","slash_commands":["review"],"mcp_servers":[]}"#,
        );
        assert!(
            added(&ds).is_empty(),
            "el arranque no agrega items al registro"
        );
        let AgentDelta::ThreadPatch { patch } = &ds[0] else {
            panic!("se esperaba ThreadPatch, salió {:?}", ds[0]);
        };
        assert_eq!(patch.provider_session.as_deref(), Some("s1"));
        assert_eq!(patch.cwd.as_deref(), Some("C:\\p"));
        assert_eq!(patch.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(patch.tools.as_ref().unwrap(), &["Bash", "Read"]);
    }

    /// El permiso trae el id del CANAL DE CONTROL, no el del `tool_use`.
    ///
    /// Contestar con el del tool_use deja el turno colgado para siempre: el CLI
    /// empareja la respuesta por `request_id` y descarta lo que no reconoce.
    #[test]
    fn el_permiso_usa_el_id_del_canal_de_control() {
        let ds = tr().translate(
            r#"{"type":"control_request","request_id":"c7a3","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"a.txt"},"description":"a.txt","permission_suggestions":[{"type":"setMode","mode":"acceptEdits"}],"tool_use_id":"toolu_9"}}"#,
        );
        let it = added(&ds)[0];
        assert_eq!(it.id, "c7a3", "el id del item es el del control_request");
        let ItemKind::Permission {
            tool,
            description,
            status,
            ..
        } = &it.kind
        else {
            panic!("se esperaba Permission, salió {:?}", it.kind);
        };
        assert_eq!(tool, "Write");
        assert_eq!(description, "a.txt");
        assert_eq!(*status, PermissionStatus::Pending);
    }

    /// Las sugerencias se guardan de paso y NO viajan a la interfaz: son la
    /// forma interna del CLI para «graba esta regla».
    #[test]
    fn la_regla_sugerida_queda_guardada_sin_salir_a_la_ui() {
        let rules = Arc::new(Mutex::new(HashMap::new()));
        let mut t = Translator::new(
            Arc::new(Mutex::new(Turns::default())),
            rules.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let ds = t.translate(
            r#"{"type":"control_request","request_id":"c1","request":{"subtype":"can_use_tool","tool_name":"Write","permission_suggestions":[{"type":"setMode","mode":"acceptEdits"}]}}"#,
        );
        assert!(rules.lock_or_recover().contains_key("c1"));
        let json = serde_json::to_string(&ds).unwrap();
        assert!(
            !json.contains("setMode"),
            "la sugerencia no debe cruzar hacia el frontend: {json}"
        );
    }

    /// El contexto son las tres entradas sumadas. Con caché, `input_tokens`
    /// solo es una fracción minúscula y la barra parecería vacía siempre.
    #[test]
    fn el_contexto_suma_el_cache() {
        let ds = tr().translate(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hola"}],"usage":{"input_tokens":10,"cache_read_input_tokens":900,"cache_creation_input_tokens":90,"output_tokens":5}}}"#,
        );
        let tokens = ds.iter().find_map(|d| match d {
            AgentDelta::ThreadPatch { patch } => patch.tokens,
            _ => None,
        });
        assert_eq!(tokens, Some(1000));
    }

    #[test]
    fn un_mensaje_puede_traer_texto_y_herramienta() {
        let ds = tr().translate(
            r#"{"type":"assistant","message":{"content":[
                {"type":"text","text":"voy a mirar"},
                {"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"a.rs"}}
            ]}}"#,
        );
        let items = added(&ds);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].kind, ItemKind::Message { .. }));
        assert!(matches!(items[1].kind, ItemKind::Tool { .. }));
    }

    #[test]
    fn task_se_traduce_como_colaboracion_y_su_resultado_la_actualiza() {
        let mut t = tr();
        let abre = t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"agent_1","name":"Task","input":{"description":"Revisar cambios","subagent_type":"review","prompt":"texto largo"}}]}}"#,
        );
        let it = added(&abre)[0];
        match &it.kind {
            ItemKind::Collab {
                name,
                title,
                subagent_type,
                status,
                summary,
                creation_source,
                ..
            } => {
                assert_eq!(name, "Task");
                assert_eq!(title, "Revisar cambios");
                assert_eq!(subagent_type, "review");
                assert_eq!(*status, ToolStatus::InProgress);
                assert!(summary.is_empty(), "el prompt no es un resumen");
                assert_eq!(creation_source, "provider_native");
            }
            other => panic!("se esperaba Collab, llegó {other:?}"),
        }

        let cierra = t.translate(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"agent_1","content":"Revisión completa","is_error":false}]}}"#,
        );
        let p = patches(&cierra, "agent_1")[0];
        assert_eq!(p.summary.as_deref(), Some("Revisión completa"));
        assert!(p.output.is_none());
        assert_eq!(
            p.status.as_ref(),
            serde_json::to_value(ToolStatus::Completed).ok().as_ref()
        );
    }

    /// El id del CLI es el id del item, así el `tool_result` que llega después
    /// encuentra su tarjeta sin tabla de equivalencias.
    #[test]
    fn la_herramienta_es_un_item_que_despues_se_parchea() {
        let mut t = tr();
        let abre = t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_7","name":"Read","input":{"file_path":"src/main.rs"}}]}}"#,
        );
        let it = added(&abre)[0];
        assert_eq!(it.id, "toolu_7");
        let ItemKind::Tool {
            status,
            title,
            tool_kind,
            locations,
            ..
        } = &it.kind
        else {
            panic!("se esperaba Tool");
        };
        assert_eq!(*status, ToolStatus::InProgress);
        assert_eq!(
            *tool_kind,
            ToolKind::Read,
            "Read se clasifica sin adivinar en la vista"
        );
        assert_eq!(title, "src/main.rs");
        assert_eq!(locations, &["src/main.rs"]);

        let cierra = t.translate(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_7","content":"ok","is_error":false}]}}"#,
        );
        assert!(
            added(&cierra).is_empty(),
            "el resultado NO crea un item nuevo"
        );
        let p = patches(&cierra, "toolu_7")[0];
        assert_eq!(p.output.as_deref(), Some("ok"));
        assert_eq!(
            p.status.as_ref().unwrap(),
            &serde_json::to_value(ToolStatus::Completed).unwrap()
        );
    }

    #[test]
    fn una_herramienta_que_falla_queda_marcada() {
        let mut t = tr();
        t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu_1","name":"Bash","input":{"command":"cargo check"}}]}}"#,
        );
        let ds = t.translate(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu_1","content":"error[E0433]","is_error":true}]}}"#,
        );
        assert_eq!(
            patches(&ds, "tu_1")[0].status.as_ref().unwrap(),
            &serde_json::to_value(ToolStatus::Failed).unwrap()
        );
    }

    /// La regresión que motivó todo el modelo: lo que se escribió en vivo y el
    /// bloque completo son EL MISMO mensaje, no dos.
    #[test]
    fn el_texto_en_vivo_se_cierra_en_su_sitio_sin_duplicarse() {
        let mut t = tr();
        let a = t.translate(
            r#"{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"ho"}}}"#,
        );
        let id = added(&a)[0].id.clone();
        assert!(matches!(
            added(&a)[0].kind,
            ItemKind::Message {
                streaming: true,
                ..
            }
        ));

        let b = t.translate(
            r#"{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"la"}}}"#,
        );
        assert!(added(&b).is_empty(), "el segundo trozo no crea otro item");
        assert!(matches!(b[0], AgentDelta::ItemChunk { .. }));

        let c = t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hola"}]}}"#,
        );
        assert!(
            added(&c).is_empty(),
            "el bloque cerrado parchea el item vivo en vez de agregar otro"
        );
        let p = patches(&c, &id)[0];
        assert_eq!(p.text.as_deref(), Some("hola"));
        assert_eq!(p.streaming, Some(false));
    }

    /// Un turno que cierra con texto a medio escribir dejaría el cursor
    /// parpadeando para siempre sin que nadie escriba.
    #[test]
    fn el_fin_de_turno_apaga_el_texto_en_vivo() {
        let mut t = tr();
        let a = t.translate(
            r#"{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"ho"}}}"#,
        );
        let id = added(&a)[0].id.clone();
        let ds = t.translate(r#"{"is_error":false,"stop_reason":"end_turn"}"#);
        assert_eq!(patches(&ds, &id)[0].streaming, Some(false));
    }

    #[test]
    fn el_evento_final_no_trae_type() {
        // El CLI cierra el turno con un objeto sin `type`; reconocerlo por sus
        // campos es la única forma de no perder el fin de turno.
        let ds =
            tr().translate(r#"{"is_error":false,"stop_reason":"end_turn","total_cost_usd":0.08}"#);
        let fin = ds.iter().find_map(|d| match d {
            AgentDelta::TurnEnd {
                status, cost_usd, ..
            } => Some((status, cost_usd)),
            _ => None,
        });
        assert_eq!(fin, Some((&TurnStatus::Done, &Some(0.08))));
    }

    #[test]
    fn el_type_del_evento_final_llega_tarde_en_el_objeto() {
        // Orden real del CLI: `type` va después de `usage` y compañía. Leer una
        // captura truncada hizo creer que el evento no tenía tipo, y el turno
        // nunca se daba por terminado.
        let ds = tr().translate(
            r#"{"is_error":false,"num_turns":1,"stop_reason":"end_turn","usage":{"x":1},"subtype":"success","type":"result","total_cost_usd":0.12}"#,
        );
        assert!(
            ds.iter().any(|d| matches!(d, AgentDelta::TurnEnd { .. })),
            "el resumen final tiene que cerrar el turno, salió {ds:?}"
        );
    }

    /// Cerrado el turno, lo próximo abre uno nuevo en vez de colgarse del viejo.
    #[test]
    fn despues_del_cierre_se_abre_otro_turno() {
        let mut t = tr();
        t.translate(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a"}]}}"#);
        t.translate(r#"{"is_error":false,"stop_reason":"end_turn"}"#);
        let ds = t.translate(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"b"}]}}"#,
        );
        assert!(
            ds.iter().any(|d| matches!(d, AgentDelta::TurnStart { .. })),
            "tenía que abrir un turno nuevo, salió {ds:?}"
        );
    }

    #[test]
    fn lo_desconocido_se_reporta_en_vez_de_descartarse() {
        // El formato del CLI va a crecer. Tragar en silencio lo que no se
        // conoce hace que un evento nuevo se vea como si nada hubiera pasado.
        let ds = tr().translate(r#"{"type":"algo_que_no_existe_todavia","x":1}"#);
        assert!(matches!(added(&ds)[0].kind, ItemKind::Notice { .. }));

        let rotos = tr().translate("esto no es json");
        assert!(matches!(added(&rotos)[0].kind, ItemKind::Notice { .. }));
    }

    #[test]
    fn la_telemetria_del_cli_no_ensucia_la_conversacion() {
        // Visto en pantalla: estos dos llegan muchas veces por turno y se
        // volcaban como JSON crudo en medio del diálogo.
        assert!(tr()
            .translate(r#"{"type":"system","subtype":"status","status":"requesting"}"#)
            .is_empty());
        assert!(tr()
            .translate(r#"{"type":"system","subtype":"thinking_tokens","estimated_tokens":50}"#)
            .is_empty());
    }

    #[test]
    fn un_system_nuevo_se_nombra_en_vez_de_volcarse() {
        // Enterarse de que existe, sí; leer su JSON a mano, no.
        let ds = tr().translate(r#"{"type":"system","subtype":"algo_nuevo","payload":{"a":1}}"#);
        match &added(&ds)[0].kind {
            ItemKind::Notice { text } => assert_eq!(text, "sistema: algo_nuevo"),
            other => panic!("se esperaba Notice, llegó {other:?}"),
        }
    }

    #[test]
    fn el_ruido_de_limite_permitido_no_llega_a_la_ui() {
        assert!(tr()
            .translate(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#)
            .is_empty());
    }

    #[test]
    fn el_catalogo_de_comandos_configura_el_hilo() {
        let ds = tr().translate(
            r#"{"type":"control_response","response":{"response":{"commands":[{"name":"review","description":"revisa","argumentHint":"[pr]"},{"name":""}]}}}"#,
        );
        let AgentDelta::ThreadPatch { patch } = &ds[0] else {
            panic!("se esperaba ThreadPatch");
        };
        let cmds = patch.commands.as_ref().unwrap();
        assert_eq!(cmds.len(), 1, "los comandos sin nombre se descartan");
        assert_eq!(cmds[0].name, "review");
        assert_eq!(cmds[0].argument_hint, "[pr]");
    }

    /// El razonamiento vacío es el caso normal cuando el modelo no lo expone, y
    /// no merece una tarjeta.
    #[test]
    fn el_razonamiento_vacio_no_genera_item() {
        let ds = tr().translate(
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"   "}]}}"#,
        );
        assert!(added(&ds).is_empty());
    }

    #[test]
    fn compact_boundary_sale_como_aviso_legible() {
        let ds = tr().translate(
            r#"{"type":"system","subtype":"compact_boundary","compactMetadata":{"trigger":"manual","preTokens":37418}}"#,
        );
        let ItemKind::Notice { text } = &added(&ds)[0].kind else {
            panic!("se esperaba Notice, llegó {:?}", added(&ds)[0].kind);
        };
        assert!(
            text.contains("Contexto compactado") && text.contains("37418"),
            "aviso inesperado: {text}"
        );
    }

    #[test]
    fn el_resumen_de_compact_no_es_mensaje_de_usuario() {
        let ds = tr().translate(
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"Se trabajó en auth.ts y quedó pendiente el middleware."}]}}"#,
        );
        let ItemKind::Notice { text } = &added(&ds)[0].kind else {
            panic!("se esperaba Notice, llegó {:?}", added(&ds)[0].kind);
        };
        assert!(text.starts_with("Resumen del contexto"));
        assert!(text.contains("auth.ts"));
    }

    #[test]
    fn control_commands_acepta_commands_colgados_de_response() {
        let ds = tr().translate(
            r#"{"type":"control_response","response":{"subtype":"success","commands":[{"name":"compact","description":"resume","argumentHint":""}]}}"#,
        );
        let AgentDelta::ThreadPatch { patch } = &ds[0] else {
            panic!("se esperaba ThreadPatch");
        };
        let cmds = patch.commands.as_ref().unwrap();
        assert_eq!(cmds[0].name, "compact");
        assert_eq!(cmds[0].description, "resume");
    }

    #[test]
    fn init_trae_nombres_de_slash_como_catalogo_interino() {
        let ds = tr().translate(
            r#"{"type":"system","subtype":"init","cwd":"/p","session_id":"s","model":"opus","tools":[],"slash_commands":["compact","clear"],"mcp_servers":[]}"#,
        );
        let AgentDelta::ThreadPatch { patch } = &ds[0] else {
            panic!("se esperaba ThreadPatch");
        };
        let cmds = patch.commands.as_ref().unwrap();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "compact");
    }

    #[test]
    fn slash_de_permisos_y_plan_son_control() {
        assert!(matches!(
            classify_control_slash("/plan"),
            Some(ControlSlash::SetMode { mode, .. }) if mode == "plan"
        ));
        assert!(matches!(
            classify_control_slash("/permissions acceptEdits"),
            Some(ControlSlash::SetMode { mode, .. }) if mode == "acceptEdits"
        ));
        assert!(matches!(
            classify_control_slash("/permissions"),
            Some(ControlSlash::NoticeOnly { .. })
        ));
        assert!(matches!(
            classify_control_slash("/effort high"),
            Some(ControlSlash::NoticeForward { .. })
        ));
        assert!(classify_control_slash("hola").is_none());
    }

    #[test]
    fn permission_mode_invalido_cae_a_manual() {
        // "full" es un ResumeMode de la UI; nunca debe llegar al CLI.
        assert_eq!(cli_permission_mode(Some("full")), "manual");
        assert_eq!(cli_permission_mode(Some("summary")), "manual");
        assert_eq!(cli_permission_mode(Some("acceptEdits")), "acceptEdits");
        assert_eq!(cli_permission_mode(Some("plan")), "plan");
        assert_eq!(cli_permission_mode(None), "manual");
    }
}
