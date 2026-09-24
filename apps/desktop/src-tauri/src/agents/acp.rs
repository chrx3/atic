//! Adaptador para agentes que hablan **ACP** (Agent Client Protocol).
//!
//! # Por qué uno solo para varios agentes
//!
//! OpenCode y Cursor no comparten nada salvo el protocolo, y con eso alcanza:
//! `opencode acp` y `cursor-agent acp` levantan el mismo JSON-RPC por stdio, así
//! que este archivo los atiende a los dos cambiando una constante. Cualquier
//! agente que adopte ACP —Gemini, los que vengan— entra igual.
//!
//! Es la razón por la que el modelo canónico de `model.rs` se moldeó sobre ACP:
//! acá la traducción es casi copiar campos, y el trabajo de verdad quedó del
//! lado de Claude Code, que habla lo suyo.
//!
//! # Cómo se enchufa con el resto
//!
//! El crate de ACP es asíncrono; la capa de agentes es de hilos bloqueantes.
//! El puente es un hilo dedicado que corre la conexión entera con
//! `block_on`, y dos canales:
//!
//! ```text
//!   AcpSession::send ──Cmd::Prompt──▶ hilo de conexión ──▶ send_request(prompt)
//!   AcpSession::respond_permission ──oneshot──▶ handler de permiso (que espera)
//! ```
//!
//! No hace falta tokio: el crate va con `futures` + `async-io`, que traen su
//! propio reactor. Comprobado contra `opencode acp` antes de escribir esto.
//!
//! # La trampa de Windows
//!
//! `opencode` se instala como shim de npm (`.cmd`, `.ps1`) y **no** tiene
//! `.exe`, así que `Command::new("opencode")` falla con «program not found»
//! donde `claude` —que sí es `claude.exe`— funciona. La ruta se resuelve con
//! [`super::exe::resolve`]. Y hay una segunda trampa encima: `AcpAgent::from_str`
//! parte la línea con `shell-words`, que usa reglas POSIX y se come las `\` de
//! las rutas de Windows; por eso acá se usa `AcpAgentConfig` con el programa y
//! los argumentos por separado.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    AuthenticateRequest, CancelNotification, ContentBlock, ContentChunk, EnvVariable, ImageContent,
    InitializeRequest, McpServer, McpServerStdio, NewSessionRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelect, SessionConfigSelectOption, SessionConfigSelectOptions, SessionId,
    SessionNotification, SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest,
    StopReason, TextContent, ToolCall, ToolCallStatus as AcpToolStatus, ToolCallUpdate,
    ToolKind as AcpToolKind, UsageUpdate,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, ConnectionTo, JsonRpcRequest, JsonRpcResponse,
};
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::{
    AgentDelta, Item, ItemId, ItemKind, ItemPatch, ModeInfo, ModelInfo, Origin, PermissionStatus,
    PlanEntry, PlanStatus, Role, ThreadPatch, ToolKind, ToolStatus, TurnStatus,
};
use super::turns::{end_turn, ensure_turn, start_turn, Emit, Turns};
use super::{AgentBackend, AgentSession, PermissionDecision, SlashCommand, StartOptions};
use atic_core::MutexExt;

/// Un agente ACP, descrito por cómo se lanza.
pub struct Acp {
    id: &'static str,
    display_name: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    /// Dónde mirar si hay sesión iniciada. Puntero y no `match` sobre `id`
    /// para que sumar un agente ACP siga siendo escribir una constante.
    signed_in: fn() -> Option<bool>,
}

/// Instalado con `npm i -g opencode-ai`. En Windows queda como shim `.cmd`.
pub const OPENCODE: Acp = Acp {
    id: "opencode",
    display_name: "OpenCode",
    program: "opencode",
    args: &["acp"],
    signed_in: super::login::opencode,
};

/// `cursor-agent` expone ACP con el subcomando `acp`, igual que OpenCode.
pub const CURSOR: Acp = Acp {
    id: "cursor",
    display_name: "Cursor",
    program: "cursor-agent",
    args: &["acp"],
    signed_in: super::login::cursor,
};

/// Grok no lo anuncia como «acp» pero `grok agent stdio` responde `initialize`
/// con `protocolVersion: 1` y capacidades ACP; su formato nativo son las
/// `session update` del protocolo. Se instala en `~/.grok/bin`.
pub const GROK: Acp = Acp {
    id: "grok",
    display_name: "Grok",
    program: "grok",
    args: &["agent", "stdio"],
    signed_in: super::login::grok,
};

/// Lo que la sesión le pide al hilo de conexión.
enum Cmd {
    /// Turno ya anunciado a la UI: el hilo de conexión solo habla con el agente.
    Prompt {
        turn: String,
        /// Texto ya limpio de rutas embebidas (el que ve el modelo).
        prompt: String,
        files: Vec<String>,
    },
    SetModel {
        model: String,
        effort: Option<String>,
        fast: Option<bool>,
    },
    SetMode {
        mode: String,
    },
    Stop,
}

/// Cursor pide input estructurado y **bloquea** el turno hasta la respuesta.
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "cursor/ask_question", response = CursorAskQuestionResponse)]
#[serde(rename_all = "camelCase")]
struct CursorAskQuestionRequest {
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    questions: Vec<CursorQuestion>,
}

/// Una pregunta de Cursor: se contesta con ids de opción, no con texto.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorQuestion {
    #[serde(default)]
    id: String,
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    options: Vec<CursorOption>,
    #[serde(default)]
    allow_multiple: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CursorOption {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct CursorAskQuestionResponse {
    outcome: Value,
}

/// Cursor pide aprobar un plan y también bloquea hasta contestar.
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "cursor/create_plan", response = CursorCreatePlanResponse)]
#[serde(rename_all = "camelCase")]
struct CursorCreatePlanRequest {
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    /// El plan en markdown.
    #[serde(default)]
    plan: String,
    #[serde(default)]
    todos: Vec<CursorTodo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CursorTodo {
    #[serde(default)]
    id: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct CursorCreatePlanResponse {
    outcome: Value,
}

/// El plan de Cursor con la forma del `ExitPlanMode` de Claude.
fn cursor_plan_input(req: &CursorCreatePlanRequest) -> Value {
    let todos: Vec<Value> = req
        .todos
        .iter()
        .map(|t| serde_json::json!({ "content": t.content, "status": t.status }))
        .collect();
    serde_json::json!({
        "plan": req.plan,
        "name": req.name,
        "overview": req.overview,
        "todos": todos,
    })
}

/// Los modos que el agente informa al abrir la sesión.
fn acp_modes(state: &agent_client_protocol::schema::v1::SessionModeState) -> Vec<ModeInfo> {
    state
        .available_modes
        .iter()
        .map(|m| ModeInfo {
            id: m.id.0.to_string(),
            name: m.name.clone(),
            description: m.description.clone().unwrap_or_default(),
        })
        .collect()
}

/// Una pregunta de Cursor pendiente: sus opciones y por dónde contestar.
struct CursorAsk {
    questions: Vec<CursorQuestion>,
    reply: oneshot::Sender<Option<Value>>,
}

/// Las preguntas de Cursor con la forma de las de Claude (`AskUserQuestion`).
fn cursor_questions_input(title: &str, questions: &[CursorQuestion]) -> Value {
    let questions: Vec<Value> = questions
        .iter()
        .map(|q| {
            let options: Vec<Value> = q
                .options
                .iter()
                .map(|o| serde_json::json!({ "label": o.label, "description": "" }))
                .collect();
            serde_json::json!({
                "question": q.prompt,
                "header": title,
                "multiSelect": q.allow_multiple,
                "options": options,
            })
        })
        .collect();
    serde_json::json!({ "questions": questions })
}

/// Lo elegido en la tarjeta (`answers`: texto de la pregunta → etiquetas
/// separadas por coma) a lo que Cursor espera: ids de opción por pregunta.
fn cursor_answers(questions: &[CursorQuestion], answers: Option<&Value>) -> Value {
    let answered: Vec<Value> = questions
        .iter()
        .map(|q| {
            let chosen = answers
                .and_then(|a| a.get(&q.prompt))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let picked: Vec<&str> = chosen.split(", ").map(str::trim).collect();
            let ids: Vec<&str> = q
                .options
                .iter()
                .filter(|o| picked.contains(&o.label.as_str()))
                .map(|o| o.id.as_str())
                .collect();
            serde_json::json!({ "questionId": q.id, "selectedOptionIds": ids })
        })
        .collect();
    serde_json::json!({ "outcome": "answered", "answers": answered })
}

/// Conexión + id ACP para `session/cancel` desde el hilo de la UI.
///
/// El bucle de `Cmd` queda bloqueado dentro de `prompt_turn`, así que
/// «Detener» no puede ir por ese canal: manda la notificación por acá.
struct LiveConn {
    conn: ConnectionTo<Agent>,
    session_id: SessionId,
}

/// Estado que comparten el hilo de conexión y la sesión.
struct Shared {
    turns: Mutex<Turns>,
    /// Permisos esperando respuesta: id del item → por dónde contestarle.
    ///
    /// El handler de ACP **se queda esperando** en el otro extremo, y por eso
    /// el turno del agente queda detenido de verdad hasta que el usuario
    /// conteste. Sin este canal habría que contestarle algo al toque y decidir
    /// por él, que es exactamente lo que la interfaz viene a evitar.
    pending: Mutex<HashMap<String, oneshot::Sender<PermissionDecision>>>,
    /// Preguntas de Cursor esperando respuesta, con sus opciones para
    /// traducir lo elegido a ids. `None` en el canal es «la saltó».
    asks: Mutex<HashMap<String, CursorAsk>>,
    /// Items de texto ya anunciados. Un chunk con id conocido continúa; uno
    /// nuevo abre.
    seen: Mutex<HashSet<ItemId>>,
    /// Los de arriba que siguen abiertos en el turno en curso.
    ///
    /// ACP no dice «este bloque terminó»: los chunks simplemente dejan de
    /// llegar y el turno cierra. Sin anotarlos para cerrarlos a mano, el item
    /// queda `streaming: true` para siempre y la vista lo dibuja con el cursor
    /// parpadeando sin que nadie escriba —el mismo agujero que `claude_code.rs`
    /// tapa en su rama de `result`—. Y lo que es peor, la pill nunca se entera
    /// de que el agente contestó, porque ese aviso cuelga del cierre.
    abiertos: Mutex<Vec<ItemId>>,
    /// Costo de la sesión: lo último que informó el agente, y lo ya atribuido
    /// a turnos anteriores.
    ///
    /// ACP manda el costo **acumulado por sesión** en cada `usage_update`, y el
    /// turno quiere lo suyo. Sin restar, cada turno reportaría todo lo gastado
    /// antes y el total de la conversación crecería al cuadrado.
    cost: Mutex<Costo>,
    /// Id ACP del selector de modelo, si el agente lo informó en `config_options`.
    model_config_id: Mutex<Option<String>>,
    /// Id ACP del selector de esfuerzo/razonamiento, si existe.
    effort_config_id: Mutex<Option<String>>,
    /// Plantillas ACP por id de grupo: `grok-4.5` → `grok-4.5[effort=high,fast=true]`.
    ///
    /// Cursor no acepta los slugs del CLI (`cursor-grok-4.5-high`); hay que
    /// mandar el value con parámetros entre corchetes y mutar effort/fast ahí.
    model_templates: Mutex<HashMap<String, String>>,
    /// Conexión viva para cancelar el prompt sin pasar por `Cmd`.
    live: Mutex<Option<LiveConn>>,
}

impl AgentBackend for Acp {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.display_name
    }

    fn is_available(&self) -> bool {
        super::exe::resolve(self.program).is_some()
    }

    fn signed_in(&self) -> Option<bool> {
        (self.signed_in)()
    }

    fn start(
        &self,
        options: StartOptions,
        on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String> {
        // `launcher` y no `resolve`: si el CLI resultó ser un guion `.cmd`, hay
        // que lanzarlo por el intérprete y no dárselo a Windows tal cual.
        let (program, prefijo) = super::exe::launcher(self.program).ok_or_else(|| {
            format!(
                "no se encontró «{}» en el PATH. Instálalo y ábrelo una vez en la consola.",
                self.program
            )
        })?;

        let emit = Emit::new(on_delta);
        let shared = Arc::new(Shared {
            turns: Mutex::new(Turns::default()),
            pending: Mutex::new(HashMap::new()),
            asks: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashSet::new()),
            abiertos: Mutex::new(Vec::new()),
            cost: Mutex::new(Costo::default()),
            model_config_id: Mutex::new(None),
            effort_config_id: Mutex::new(None),
            model_templates: Mutex::new(HashMap::new()),
            live: Mutex::new(None),
        });

        let (tx, rx) = mpsc::unbounded::<Cmd>();
        let cwd = options.cwd.clone().unwrap_or_else(|| ".".to_string());
        let desired_model = options.model.clone();
        let desired_effort = options.effort.clone();
        let desired_fast = options.fast;
        // El grafo de delegación viaja siempre, inyectes o no el MCP.
        let env = options.env.clone();
        let atic_mcp = options.atic_mcp.clone();
        // Los del modal vienen normalizados desde `bridge`: acá solo se
        // declaran en el `session/new`, que es donde ACP los acepta.
        let mcp_servers = options.mcp_servers.clone();
        let args: Vec<String> = prefijo
            .into_iter()
            .chain(self.args.iter().map(|a| a.to_string()))
            .collect();
        let name = self.display_name;
        let backend_id = self.id;

        {
            let emit = emit.clone();
            let shared = shared.clone();
            std::thread::spawn(move || {
                let result = futures::executor::block_on(connect(
                    Arranque {
                        program,
                        args,
                        cwd,
                        env,
                        atic_mcp,
                        mcp_servers,
                        backend_id,
                        desired_model,
                        desired_effort,
                        desired_fast,
                    },
                    rx,
                    emit.clone(),
                    shared,
                ));
                if let Err(e) = result {
                    emit.send(AgentDelta::Failed {
                        message: format!("{name}: {e}"),
                    });
                }
            });
        }

        Ok(Box::new(AcpSession { tx, emit, shared }))
    }
}

/// Los MCP que se le declaran al agente al abrir la sesión: el de orquestación
/// —si el hub corre— más los del modal, ya normalizados por `bridge`.
/// `mcp_servers` es parte de `session/new` en ACP, así que esto no le pide
/// nada raro a ningún CLI.
fn servidores_mcp(
    atic: Option<&super::hub::AticMcp>,
    extra: &[super::mcp_servers::McpServerDef],
) -> Vec<McpServer> {
    let mut out = Vec::new();
    if let Some(a) = atic {
        out.push(McpServer::Stdio(
            McpServerStdio::new("atic", a.command.clone()).args(a.args.clone()),
        ));
    }
    for s in extra {
        let env = s
            .env
            .iter()
            .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
            .collect();
        out.push(McpServer::Stdio(
            McpServerStdio::new(s.name.clone(), s.command.clone())
                .args(s.args.clone())
                .env(env),
        ));
    }
    out
}

/// Con qué arrancar la conexión: el proceso y lo que el usuario pidió antes de
/// que hubiera sesión donde pedirlo.
///
/// Van juntos en un struct y no sueltos porque son siete y se leen de a pares
/// mal apareados en el sitio de llamada; con nombres, un `desired_effort` en el
/// lugar de `cwd` deja de compilar en vez de fallar en runtime.
struct Arranque {
    program: std::path::PathBuf,
    args: Vec<String>,
    cwd: String,
    /// `ATIC_*` del grafo: viajan aunque el MCP no se inyecte.
    env: Vec<(String, String)>,
    /// El servidor de orquestación, para que este hijo pueda delegar a su vez.
    atic_mcp: Option<super::hub::AticMcp>,
    /// Los del modal, ya normalizados por `bridge`.
    mcp_servers: Vec<super::mcp_servers::McpServerDef>,
    backend_id: &'static str,
    desired_model: Option<String>,
    desired_effort: Option<String>,
    desired_fast: Option<bool>,
}

/// Corre la conexión entera. Vive lo que la sesión.
async fn connect(
    arranque: Arranque,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    emit: Emit,
    shared: Arc<Shared>,
) -> Result<(), String> {
    let Arranque {
        program,
        args,
        cwd,
        env,
        atic_mcp,
        mcp_servers,
        backend_id,
        desired_model,
        desired_effort,
        desired_fast,
    } = arranque;
    let agent = AcpAgent::new(AcpAgentConfig::new(&program).args(args).envs(env));

    let notif = {
        let (emit, shared) = (emit.clone(), shared.clone());
        move |n: SessionNotification| {
            let (emit, shared) = (emit.clone(), shared.clone());
            async move {
                emit.all(translate(&n.update, &shared));
                Ok(())
            }
        }
    };

    let perm = {
        let (emit, shared) = (emit.clone(), shared.clone());
        move |req: RequestPermissionRequest,
              responder: agent_client_protocol::Responder<RequestPermissionResponse>,
              cx: ConnectionTo<Agent>| {
            let (emit, shared) = (emit.clone(), shared.clone());
            async move {
                let id = format!("perm:{}", req.tool_call.tool_call_id.0);
                let (tx, wait) = oneshot::channel();
                shared.pending.lock_or_recover().insert(id.clone(), tx);

                let mut out = Vec::new();
                let turn = ensure_turn(&shared.turns, &mut out);
                out.push(AgentDelta::ItemAdd {
                    turn,
                    item: Item::new(
                        id.clone(),
                        ItemKind::Permission {
                            tool: req
                                .tool_call
                                .fields
                                .title
                                .clone()
                                .unwrap_or_else(|| "herramienta".into()),
                            description: describe(&req),
                            input: req
                                .tool_call
                                .fields
                                .raw_input
                                .clone()
                                .unwrap_or(Value::Null),
                            status: PermissionStatus::Pending,
                        },
                    ),
                });
                emit.all(out);

                // No await acá: el bus ACP es un solo task. Si esperamos al
                // usuario dentro del handler, se congelan los chunks y el
                // prompt queda muerto aunque la UI ya muestre el permiso.
                cx.spawn(async move {
                    let decision = wait.await.unwrap_or(PermissionDecision::Deny);
                    shared.pending.lock_or_recover().remove(&id);
                    emit.send(AgentDelta::ItemPatch {
                        item: id,
                        patch: ItemPatch {
                            status: serde_json::to_value(match decision {
                                PermissionDecision::Deny => PermissionStatus::Denied,
                                _ => PermissionStatus::Allowed,
                            })
                            .ok(),
                            ..Default::default()
                        },
                    });

                    match pick_option(&req, decision) {
                        Some(opt) => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(opt)),
                        )),
                        None => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    }?;
                    Ok(())
                })?;
                Ok(())
            }
        }
    };

    let ask = {
        let (emit, shared) = (emit.clone(), shared.clone());
        move |req: CursorAskQuestionRequest,
              responder: agent_client_protocol::Responder<CursorAskQuestionResponse>,
              cx: ConnectionTo<Agent>| {
            let (emit, shared) = (emit.clone(), shared.clone());
            async move {
                let mut out = Vec::new();
                let turn = ensure_turn(&shared.turns, &mut out);
                let n = shared.seen.lock_or_recover().len();
                let id = format!(
                    "ask:{}",
                    req.tool_call_id
                        .clone()
                        .unwrap_or_else(|| format!("{turn}-{n}"))
                );
                let title = req.title.clone().unwrap_or_default();
                let (tx, wait) = oneshot::channel();
                shared.asks.lock_or_recover().insert(
                    id.clone(),
                    CursorAsk {
                        questions: req.questions.clone(),
                        reply: tx,
                    },
                );
                // Con la forma de la pregunta de Claude: la interfaz la dibuja
                // con la misma tarjeta, sin saber de qué agente vino.
                out.push(AgentDelta::ItemAdd {
                    turn,
                    item: Item::new(
                        id.clone(),
                        ItemKind::Permission {
                            tool: "AskUserQuestion".to_string(),
                            description: title.clone(),
                            input: cursor_questions_input(&title, &req.questions),
                            status: PermissionStatus::Pending,
                        },
                    ),
                });
                emit.all(out);

                // Igual que un permiso: esperar al usuario dentro del handler
                // congela el bus ACP entero.
                cx.spawn(async move {
                    let answer = wait.await.ok().flatten();
                    shared.asks.lock_or_recover().remove(&id);
                    emit.send(AgentDelta::ItemPatch {
                        item: id,
                        patch: ItemPatch {
                            status: serde_json::to_value(if answer.is_some() {
                                PermissionStatus::Allowed
                            } else {
                                PermissionStatus::Denied
                            })
                            .ok(),
                            ..Default::default()
                        },
                    });
                    responder.respond(CursorAskQuestionResponse {
                        outcome: answer.unwrap_or_else(|| {
                            serde_json::json!({
                                "outcome": "skipped",
                                "reason": "El usuario saltó la pregunta."
                            })
                        }),
                    })?;
                    Ok(())
                })?;
                Ok(())
            }
        }
    };

    let plan = {
        let (emit, shared) = (emit.clone(), shared.clone());
        move |req: CursorCreatePlanRequest,
              responder: agent_client_protocol::Responder<CursorCreatePlanResponse>,
              cx: ConnectionTo<Agent>| {
            let (emit, shared) = (emit.clone(), shared.clone());
            async move {
                let mut out = Vec::new();
                let turn = ensure_turn(&shared.turns, &mut out);
                let n = shared.seen.lock_or_recover().len();
                let id = format!(
                    "plan:{}",
                    req.tool_call_id
                        .clone()
                        .unwrap_or_else(|| format!("{turn}-{n}"))
                );
                let (tx, wait) = oneshot::channel();
                shared.pending.lock_or_recover().insert(id.clone(), tx);
                // Con la forma del `ExitPlanMode` de Claude: una sola tarjeta de
                // plan para los dos, y se contesta con los botones de permiso.
                out.push(AgentDelta::ItemAdd {
                    turn,
                    item: Item::new(
                        id.clone(),
                        ItemKind::Permission {
                            tool: "ExitPlanMode".to_string(),
                            description: req
                                .name
                                .clone()
                                .or_else(|| req.overview.clone())
                                .unwrap_or_default(),
                            input: cursor_plan_input(&req),
                            status: PermissionStatus::Pending,
                        },
                    ),
                });
                emit.all(out);

                cx.spawn(async move {
                    let decision = wait.await.unwrap_or(PermissionDecision::Deny);
                    shared.pending.lock_or_recover().remove(&id);
                    let accepted = !matches!(decision, PermissionDecision::Deny);
                    emit.send(AgentDelta::ItemPatch {
                        item: id,
                        patch: ItemPatch {
                            status: serde_json::to_value(if accepted {
                                PermissionStatus::Allowed
                            } else {
                                PermissionStatus::Denied
                            })
                            .ok(),
                            ..Default::default()
                        },
                    });
                    responder.respond(CursorCreatePlanResponse {
                        outcome: if accepted {
                            serde_json::json!({ "outcome": "accepted" })
                        } else {
                            serde_json::json!({
                                "outcome": "rejected",
                                "reason": "El usuario rechazó el plan."
                            })
                        },
                    })?;
                    Ok(())
                })?;
                Ok(())
            }
        }
    };

    agent_client_protocol::Client
        .builder()
        .name("atic")
        .on_receive_notification(
            async move |n: SessionNotification, _cx| notif(n).await,
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, cx| {
                perm(req, responder, cx).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: CursorAskQuestionRequest, responder, cx| ask(req, responder, cx).await,
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: CursorCreatePlanRequest, responder, cx| plan(req, responder, cx).await,
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |conn: ConnectionTo<Agent>| async move {
            let init = conn
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;

            // Cursor (y otros) anuncian authMethods; la doc pide authenticate
            // antes de session/new. Sin login previo falla; con CLI ya logueado
            // es un no-op útil.
            if let Some(method) = init
                .auth_methods
                .iter()
                .find(|m| m.id().0.as_ref() == "cursor_login")
                .or_else(|| init.auth_methods.first())
            {
                let _ = conn
                    .send_request(AuthenticateRequest::new(method.id().clone()))
                    .block_task()
                    .await;
            }

            let session = conn
                .send_request(
                    NewSessionRequest::new(std::path::PathBuf::from(&cwd))
                        .mcp_servers(servidores_mcp(atic_mcp.as_ref(), &mcp_servers)),
                )
                .block_task()
                .await?;

            let mut patch = ThreadPatch {
                provider_session: Some(session.session_id.0.to_string()),
                cwd: Some(cwd.clone()),
                ..Default::default()
            };

            if let Some(modes) = &session.modes {
                patch.mode = Some(modes.current_mode_id.0.to_string());
                patch.modes = Some(acp_modes(modes));
            }

            if let Some(config_options) = &session.config_options {
                let effort_cfg = find_effort_config(config_options);
                if let Some(model_cfg) = find_model_config(config_options) {
                    *shared.model_config_id.lock_or_recover() = Some(model_cfg.config_id);
                    let (models, templates) =
                        normalize_cursor_acp_models(backend_id, model_cfg.models);
                    *shared.model_templates.lock_or_recover() = templates;
                    if !models.is_empty() {
                        let current = model_cfg.current;
                        let (group_id, effort_id, fast) =
                            resolve_grouped_selection(&models, &current);
                        patch.models = Some(with_session_efforts(models, effort_cfg.as_ref()));
                        patch.model = Some(group_id);
                        if let Some(e) = effort_id {
                            patch.effort = Some(e);
                        }
                        if let Some(f) = fast {
                            patch.fast = Some(f);
                        }
                    } else {
                        patch.model = Some(model_cfg.current);
                    }
                }
                if let Some(cfg) = effort_cfg {
                    *shared.effort_config_id.lock_or_recover() = Some(cfg.config_id);
                    // Solo pisa si no vino ya del agrupado Cursor.
                    if patch.effort.is_none() {
                        patch.effort = Some(cfg.current);
                    }
                }
            }

            emit.send(AgentDelta::ThreadPatch { patch });

            *shared.live.lock_or_recover() = Some(LiveConn {
                conn: conn.clone(),
                session_id: session.session_id.clone(),
            });

            if desired_model.is_some() || desired_effort.is_some() || desired_fast.is_some() {
                apply_config(
                    &conn,
                    &session.session_id,
                    &shared,
                    desired_model.as_deref(),
                    desired_effort.as_deref(),
                    desired_fast,
                    &emit,
                )
                .await?;
            }

            while let Some(cmd) = rx.next().await {
                match cmd {
                    Cmd::Stop => break,
                    Cmd::SetModel {
                        model,
                        effort,
                        fast,
                    } => {
                        apply_config(
                            &conn,
                            &session.session_id,
                            &shared,
                            Some(&model),
                            effort.as_deref(),
                            fast,
                            &emit,
                        )
                        .await?;
                    }
                    Cmd::SetMode { mode } => {
                        conn.send_request(SetSessionModeRequest::new(
                            session.session_id.clone(),
                            mode.clone(),
                        ))
                        .block_task()
                        .await?;
                        emit.send(AgentDelta::ThreadPatch {
                            patch: ThreadPatch {
                                mode: Some(mode),
                                ..Default::default()
                            },
                        });
                    }
                    Cmd::Prompt {
                        turn,
                        prompt,
                        files,
                    } => {
                        prompt_turn(
                            &conn,
                            &session.session_id,
                            turn,
                            prompt,
                            files,
                            &emit,
                            &shared,
                        )
                        .await?;
                    }
                }
            }
            *shared.live.lock_or_recover() = None;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
}

async fn prompt_turn(
    conn: &ConnectionTo<Agent>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
    turn: String,
    prompt: String,
    files: Vec<String>,
    emit: &Emit,
    shared: &Shared,
) -> agent_client_protocol::Result<()> {
    let mut blocks: Vec<ContentBlock> = Vec::new();
    for path in &files {
        match super::media::read_image_base64(std::path::Path::new(path)) {
            Ok((mime, data)) => {
                blocks.push(ContentBlock::Image(ImageContent::new(data, mime)));
            }
            Err(e) => {
                blocks.push(ContentBlock::Text(TextContent::new(format!(
                    "[no se pudo adjuntar {path}: {e}]"
                ))));
            }
        }
    }
    if !prompt.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(prompt)));
    }
    if blocks.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(String::new())));
    }

    let done = conn
        .send_request(PromptRequest::new(session_id.clone(), blocks))
        .block_task()
        .await;

    // Cerrar lo que quedó escribiéndose, ANTES de dar el turno por
    // terminado: el texto acumulado por trozos ya es el definitivo,
    // así que el parche solo apaga la señal de «sigue escribiendo».
    for id in shared.abiertos.lock_or_recover().drain(..) {
        emit.send(AgentDelta::ItemPatch {
            item: id,
            patch: ItemPatch {
                streaming: Some(false),
                ..Default::default()
            },
        });
    }

    let status = match &done {
        Ok(resp) if matches!(resp.stop_reason, StopReason::Cancelled) => TurnStatus::Cancelled,
        Ok(_) => TurnStatus::Done,
        Err(_) => TurnStatus::Failed,
    };
    emit.send(AgentDelta::TurnEnd {
        turn,
        status,
        cost_usd: shared.cost.lock_or_recover().del_turno(),
        duration_ms: None,
    });
    end_turn(&shared.turns);
    done?;
    Ok(())
}

struct ModelConfig {
    config_id: String,
    models: Vec<ModelInfo>,
    current: String,
}

fn is_model_option(opt: &SessionConfigOption) -> bool {
    matches!(opt.category, Some(SessionConfigOptionCategory::Model)) || opt.id.0.contains("model")
}

fn is_effort_option(opt: &SessionConfigOption) -> bool {
    matches!(
        opt.category,
        Some(SessionConfigOptionCategory::ThoughtLevel)
    ) || {
        let id = opt.id.0.to_ascii_lowercase();
        id.contains("thought") || id.contains("effort") || id.contains("reasoning")
    }
}

fn select_option_to_model(opt: &SessionConfigSelectOption) -> ModelInfo {
    ModelInfo {
        id: opt.value.0.to_string(),
        name: opt.name.clone(),
        description: opt.description.clone().unwrap_or_default(),
        efforts: Vec::new(),
        default_effort: None,
        supports_fast: false,
    }
}

fn select_to_models(sel: &SessionConfigSelect) -> Vec<ModelInfo> {
    match &sel.options {
        SessionConfigSelectOptions::Ungrouped(opts) => {
            opts.iter().map(select_option_to_model).collect()
        }
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|g| g.options.iter().map(select_option_to_model))
            .collect(),
        _ => Vec::new(),
    }
}

/// Cursor ACP: `grok-4.5[effort=high,fast=true]`. OpenCode: ids planos.
fn normalize_cursor_acp_models(
    backend_id: &str,
    models: Vec<ModelInfo>,
) -> (Vec<ModelInfo>, HashMap<String, String>) {
    if backend_id != "cursor" {
        return (models, HashMap::new());
    }

    let mut templates = HashMap::new();
    let mut out = Vec::new();
    for m in models {
        let parsed = parse_acp_model_value(&m.id);
        templates.insert(parsed.base.clone(), m.id.clone());
        let (efforts, default_effort, supports_fast) = acp_efforts_from_params(&parsed.params);
        out.push(ModelInfo {
            id: parsed.base,
            name: if m.name.is_empty() {
                parsed_display_name(&m.id)
            } else {
                m.name
            },
            description: m.description,
            efforts,
            default_effort,
            supports_fast,
        });
    }
    (out, templates)
}

struct AcpModelParsed {
    base: String,
    params: Vec<(String, String)>,
}

fn parse_acp_model_value(raw: &str) -> AcpModelParsed {
    let raw = raw.trim();
    if let Some((base, rest)) = raw.split_once('[') {
        let inner = rest.trim_end_matches(']').trim();
        let params = if inner.is_empty() {
            Vec::new()
        } else {
            inner
                .split(',')
                .filter_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    Some((k.trim().to_string(), v.trim().to_string()))
                })
                .collect()
        };
        AcpModelParsed {
            base: base.trim().to_string(),
            params,
        }
    } else {
        AcpModelParsed {
            base: raw.to_string(),
            params: Vec::new(),
        }
    }
}

fn parsed_display_name(raw: &str) -> String {
    parse_acp_model_value(raw).base
}

fn acp_efforts_from_params(
    params: &[(String, String)],
) -> (Vec<super::model::EffortOption>, Option<String>, bool) {
    let has_effort = params
        .iter()
        .any(|(k, _)| k == "effort" || k == "reasoning");
    let supports_fast = params.iter().any(|(k, _)| k == "fast");
    let current = params
        .iter()
        .find(|(k, _)| k == "effort" || k == "reasoning")
        .map(|(_, v)| normalize_acp_effort(v));

    if !has_effort {
        return (Vec::new(), None, supports_fast);
    }

    // Niveles habituales en Cursor ACP. El CLI lista más variantes; acá el
    // selector muta el param del value entre corchetes.
    let levels = ["low", "medium", "high", "xhigh", "max"];
    let efforts = levels
        .iter()
        .map(|id| super::model::EffortOption {
            id: (*id).to_string(),
            description: match *id {
                "low" => "Contesta rápido. Para lo mecánico.".into(),
                "medium" => "El equilibrio de siempre.".into(),
                "high" => "Piensa antes. Para lo que tiene vueltas.".into(),
                "xhigh" => "Se toma su tiempo. Problemas difíciles.".into(),
                "max" => "Todo lo que puede. Lento y caro.".into(),
                _ => format!("Nivel «{id}»."),
            },
        })
        .collect();

    (efforts, current, supports_fast)
}

fn normalize_acp_effort(v: &str) -> String {
    match v {
        "extra-high" | "extra_high" => "xhigh".into(),
        other => other.to_string(),
    }
}

fn format_acp_model_value(base: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return format!("{base}[]");
    }
    let body = params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{base}[{body}]")
}

/// Arma el value ACP a mandar: plantilla del base + effort/fast pedidos.
fn build_acp_model_wire(
    templates: &HashMap<String, String>,
    model: &str,
    effort: Option<&str>,
    fast: Option<bool>,
) -> String {
    let base = resolve_acp_base(templates, model);
    let template = templates
        .get(&base)
        .cloned()
        .unwrap_or_else(|| format!("{base}[]"));
    let mut parsed = parse_acp_model_value(&template);

    if let Some(level) = effort {
        let level = normalize_acp_effort(level);
        if level != "default" {
            let key = if parsed.params.iter().any(|(k, _)| k == "reasoning") {
                "reasoning"
            } else {
                "effort"
            };
            upsert_param(&mut parsed.params, key, &level);
        }
    }

    if let Some(f) = fast {
        if parsed.params.iter().any(|(k, _)| k == "fast") || f {
            upsert_param(&mut parsed.params, "fast", if f { "true" } else { "false" });
        }
    }

    format_acp_model_value(&parsed.base, &parsed.params)
}

fn upsert_param(params: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some(slot) = params.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value.to_string();
    } else {
        params.push((key.to_string(), value.to_string()));
    }
}

/// CLI usa `cursor-grok-4.5`; ACP usa `grok-4.5`.
fn resolve_acp_base(templates: &HashMap<String, String>, model: &str) -> String {
    if templates.contains_key(model) {
        return model.to_string();
    }
    let (cli_base, _, _) = super::discover::split_cursor_wire(model);
    if templates.contains_key(&cli_base) {
        return cli_base;
    }
    let stripped = cli_base.strip_prefix("cursor-").unwrap_or(&cli_base);
    if templates.contains_key(stripped) {
        return stripped.to_string();
    }
    // Substring: plantilla cuya base está contenida en el id pedido o viceversa.
    for key in templates.keys() {
        if model.contains(key) || key.contains(stripped) || stripped.contains(key.as_str()) {
            return key.clone();
        }
    }
    stripped.to_string()
}

fn find_model_config(options: &[SessionConfigOption]) -> Option<ModelConfig> {
    for opt in options {
        if !is_model_option(opt) {
            continue;
        }
        let SessionConfigKind::Select(sel) = &opt.kind else {
            continue;
        };
        let models = select_to_models(sel);
        if models.is_empty() {
            continue;
        }
        return Some(ModelConfig {
            config_id: opt.id.0.to_string(),
            models,
            current: sel.current_value.0.to_string(),
        });
    }
    None
}

/// El esfuerzo como opción de la sesión (OpenCode, Grok): aparte del modelo.
struct EffortConfig {
    config_id: String,
    current: String,
    /// Los niveles que ofrece. Sin ellos la UI sabe el valor actual pero no
    /// tiene qué ofrecer, y el selector de esfuerzo no aparecía.
    levels: Vec<super::model::EffortOption>,
}

fn find_effort_config(options: &[SessionConfigOption]) -> Option<EffortConfig> {
    for opt in options {
        if !is_effort_option(opt) {
            continue;
        }
        let SessionConfigKind::Select(sel) = &opt.kind else {
            continue;
        };
        let levels = select_to_models(sel)
            .into_iter()
            .map(|level| super::model::EffortOption {
                description: if level.description.is_empty() {
                    level.name
                } else {
                    level.description
                },
                id: level.id,
            })
            .collect();
        return Some(EffortConfig {
            config_id: opt.id.0.to_string(),
            current: sel.current_value.0.to_string(),
            levels,
        });
    }
    None
}

/// Los niveles de la sesión, en los modelos que no traen los suyos.
///
/// La UI lee el esfuerzo del modelo en uso (así funciona Cursor, que lo lleva
/// en el id). En OpenCode y Grok es de la sesión y vale para cualquier modelo:
/// se reparte a todos para que el selector lo ofrezca igual.
fn with_session_efforts(
    mut models: Vec<ModelInfo>,
    effort: Option<&EffortConfig>,
) -> Vec<ModelInfo> {
    let Some(cfg) = effort.filter(|cfg| !cfg.levels.is_empty()) else {
        return models;
    };
    for model in models.iter_mut().filter(|m| m.efforts.is_empty()) {
        model.efforts = cfg.levels.clone();
        model.default_effort = Some(cfg.current.clone());
    }
    models
}

async fn apply_config(
    conn: &ConnectionTo<Agent>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
    shared: &Shared,
    model: Option<&str>,
    effort: Option<&str>,
    fast: Option<bool>,
    emit: &Emit,
) -> agent_client_protocol::Result<()> {
    let mut patch = ThreadPatch::default();
    let has_effort_config = shared.effort_config_id.lock_or_recover().is_some();
    let templates = shared.model_templates.lock_or_recover().clone();

    // Cursor ACP: mutar params del value `base[effort=…,fast=…]`.
    // Sin plantillas (OpenCode u otros), mandar el id tal cual.
    let wire_model = if !has_effort_config {
        if let Some(m) = model {
            if !templates.is_empty() {
                Some(build_acp_model_wire(&templates, m, effort, fast))
            } else if let Some(level) = effort {
                // Fallback CLI-style (sin sesión ACP tipada).
                Some(super::discover::compose_cursor_wire(
                    m,
                    level,
                    fast.unwrap_or(false),
                    &[],
                ))
            } else if fast == Some(true) {
                Some(super::discover::compose_cursor_wire(
                    m,
                    "default",
                    true,
                    &[],
                ))
            } else {
                Some(m.to_string())
            }
        } else {
            None
        }
    } else {
        model.map(str::to_string)
    };

    // El `.clone()` sale a su propia sentencia a propósito: dentro de un `if let`
    // el temporal del guard vive TODO el cuerpo, y el cuerpo tiene un `.await`.
    // Ese mismo mutex lo escribe el handler de notificaciones (más arriba, al
    // leer los `config_id`), así que un `std::sync::Mutex` tomado mientras el
    // request está en vuelo no es una espera: es un deadlock.
    let model_config_id = shared.model_config_id.lock_or_recover().clone();
    if let Some(wire) = wire_model.as_deref() {
        if let Some(config_id) = model_config_id {
            conn.send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                config_id,
                wire,
            ))
            .block_task()
            .await?;
            if !has_effort_config {
                if let Some(m) = model {
                    let base = if templates.is_empty() {
                        m.to_string()
                    } else {
                        resolve_acp_base(&templates, m)
                    };
                    patch.model = Some(base);
                }
                if let Some(e) = effort {
                    patch.effort = Some(normalize_acp_effort(e));
                }
                if let Some(f) = fast {
                    patch.fast = Some(f);
                }
            } else {
                patch.model = Some(wire.to_string());
            }
        }
    }

    let effort_config_id = shared.effort_config_id.lock_or_recover().clone();
    if let Some(effort) = effort {
        if let Some(config_id) = effort_config_id {
            conn.send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                config_id,
                effort,
            ))
            .block_task()
            .await?;
            patch.effort = Some(effort.to_string());
        }
    }

    if patch.model.is_some() || patch.effort.is_some() || patch.fast.is_some() {
        emit.send(AgentDelta::ThreadPatch { patch });
    }

    Ok(())
}

/// Dado un value ACP / slug CLI / id de grupo → `(grupo, effort, fast)`.
fn resolve_grouped_selection(
    models: &[ModelInfo],
    wire_or_group: &str,
) -> (String, Option<String>, Option<bool>) {
    let parsed = parse_acp_model_value(wire_or_group);
    if !parsed.params.is_empty() || models.iter().any(|m| m.id == parsed.base) {
        let effort = parsed
            .params
            .iter()
            .find(|(k, _)| k == "effort" || k == "reasoning")
            .map(|(_, v)| normalize_acp_effort(v));
        let fast = parsed
            .params
            .iter()
            .find(|(k, _)| k == "fast")
            .map(|(_, v)| v == "true");
        if let Some(m) = models.iter().find(|m| m.id == parsed.base) {
            return (
                m.id.clone(),
                effort.or_else(|| m.default_effort.clone()),
                if m.supports_fast {
                    Some(fast.unwrap_or(false))
                } else {
                    None
                },
            );
        }
        return (parsed.base, effort, fast);
    }

    let (base, level, fast) = super::discover::split_cursor_wire(wire_or_group);
    for m in models {
        if m.id == wire_or_group {
            return (
                m.id.clone(),
                m.default_effort.clone(),
                if m.supports_fast { Some(false) } else { None },
            );
        }
        if m.id == base || m.id == base.strip_prefix("cursor-").unwrap_or(&base) {
            let effort = if m.efforts.iter().any(|e| e.id == level) {
                Some(level.clone())
            } else {
                m.default_effort.clone()
            };
            return (
                m.id.clone(),
                effort,
                if m.supports_fast { Some(fast) } else { None },
            );
        }
    }
    (wire_or_group.to_string(), None, None)
}

/// Texto corto para el pedido de permiso.
fn describe(req: &RequestPermissionRequest) -> String {
    if let Some(input) = &req.tool_call.fields.raw_input {
        if let Some(obj) = input.as_object() {
            for k in ["command", "file_path", "path", "url", "pattern"] {
                if let Some(v) = obj.get(k).and_then(Value::as_str) {
                    if !v.is_empty() {
                        return v.to_string();
                    }
                }
            }
        }
    }
    req.tool_call.fields.title.clone().unwrap_or_default()
}

/// Qué opción de ACP corresponde a la decisión del usuario.
///
/// Las etiquetas que manda el agente son libres, pero `kind` **no**: es un enum
/// cerrado (`AllowOnce`, `AllowAlways`, `RejectOnce`, `RejectAlways`), así que
/// los tres botones de la interfaz mapean sin tener que renderizar la lista que
/// venga. Cada caso cae al pariente más cercano si el agente no ofreció el
/// exacto: un «siempre» que valió una vez es mejor que un botón que no hace
/// nada, y un «denegar» tiene que denegar aunque solo exista la variante
/// permanente.
fn pick_option(
    req: &RequestPermissionRequest,
    decision: PermissionDecision,
) -> Option<agent_client_protocol::schema::v1::PermissionOptionId> {
    let orden: &[PermissionOptionKind] = match decision {
        PermissionDecision::Allow => &[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
        ],
        PermissionDecision::AllowAlways => &[
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::AllowOnce,
        ],
        PermissionDecision::Deny => &[
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ],
    };
    orden.iter().find_map(|k| {
        req.options
            .iter()
            .find(|o| &o.kind == k)
            .map(|o| o.option_id.clone())
    })
}

/// Traduce una notificación de ACP al modelo canónico.
///
/// Es casi copiar campos, y esa es la idea: `model.rs` se diseñó con esta forma
/// justamente para que el adaptador de un agente que ya habla ACP no tenga nada
/// interesante adentro.
fn translate(update: &SessionUpdate, shared: &Shared) -> Vec<AgentDelta> {
    let mut out = Vec::new();
    match update {
        SessionUpdate::AgentMessageChunk(c) => chunk(c, "m", Role::Assistant, shared, &mut out),
        SessionUpdate::AgentThoughtChunk(c) => chunk(c, "r", Role::Assistant, shared, &mut out),
        // El turno del usuario lo emite quien escribe, así que el eco del
        // agente se ignora: sumarlo lo mostraría dos veces.
        SessionUpdate::UserMessageChunk(_) => {}

        SessionUpdate::ToolCall(t) => out.push(tool_added(t, shared)),
        SessionUpdate::ToolCallUpdate(t) => out.push(tool_patched(t)),

        SessionUpdate::Plan(p) => {
            let turn = ensure_turn(&shared.turns, &mut out);
            // El plan es UNO por turno y se reemplaza entero: el id lo fija el
            // turno para que la segunda versión parchee la primera en vez de
            // apilar una lista nueva debajo.
            let id = format!("{turn}-plan");
            let entries = plan_entries(&p.entries);
            if shared.seen.lock_or_recover().insert(id.clone()) {
                out.push(AgentDelta::ItemAdd {
                    turn,
                    item: Item::new(id, ItemKind::Plan { entries }),
                });
            } else {
                out.push(AgentDelta::ItemPatch {
                    item: id,
                    patch: ItemPatch {
                        entries: Some(entries),
                        ..Default::default()
                    },
                });
            }
        }

        SessionUpdate::AvailableCommandsUpdate(u) => out.push(AgentDelta::ThreadPatch {
            patch: ThreadPatch {
                commands: Some(
                    u.available_commands
                        .iter()
                        .map(|c| SlashCommand {
                            name: c.name.clone(),
                            description: c.description.clone(),
                            argument_hint: String::new(),
                        })
                        .collect(),
                ),
                ..Default::default()
            },
        }),

        SessionUpdate::CurrentModeUpdate(m) => out.push(AgentDelta::ThreadPatch {
            patch: ThreadPatch {
                mode: Some(m.current_mode_id.0.to_string()),
                ..Default::default()
            },
        }),

        SessionUpdate::UsageUpdate(u) => out.push(usage(u, shared)),

        // Metadatos de sesión / config: no son conversación. Antes caían en
        // «ACP sin traducir» y ensuciaban el hilo (y el «trabajando…» se leía
        // como si no hubiera empezado nada).
        SessionUpdate::SessionInfoUpdate(_) | SessionUpdate::ConfigOptionUpdate(_) => {}

        // Lo que todavía no traducimos se nombra en vez de descartarse: el
        // protocolo va a crecer, y tragar en silencio hace que lo nuevo se vea
        // como si nada hubiera pasado.
        other => {
            let turn = ensure_turn(&shared.turns, &mut out);
            let n = shared.seen.lock_or_recover().len();
            out.push(AgentDelta::ItemAdd {
                turn: turn.clone(),
                item: Item::new(
                    format!("{turn}-n{n}"),
                    ItemKind::Notice {
                        text: format!("ACP sin traducir: {}", variant_name(other)),
                    },
                ),
            });
        }
    }
    out
}

/// Un trozo de texto: abre el item la primera vez y lo continúa después.
///
/// El id sale de `messageId` **más el tipo**, no de `messageId` solo: OpenCode
/// manda el razonamiento y la respuesta del mismo turno con el MISMO
/// `messageId` (comprobado), así que con esa sola clave el pensamiento y la
/// respuesta terminarían pegados en un mismo bloque.
fn chunk(c: &ContentChunk, prefix: &str, role: Role, shared: &Shared, out: &mut Vec<AgentDelta>) {
    let Some(text) = content_text(&c.content) else {
        return;
    };
    let turn = ensure_turn(&shared.turns, out);
    let key = c
        .message_id
        .as_ref()
        .map(|m| m.0.to_string())
        .unwrap_or_else(|| turn.clone());
    let id = format!("{prefix}:{key}");

    if shared.seen.lock_or_recover().insert(id.clone()) {
        let kind = if prefix == "r" {
            ItemKind::Reasoning {
                text: String::new(),
                streaming: true,
            }
        } else {
            ItemKind::Message {
                role,
                text: String::new(),
                streaming: true,
            }
        };
        out.push(AgentDelta::ItemAdd {
            turn,
            item: Item::new(id.clone(), kind),
        });
        shared.abiertos.lock_or_recover().push(id.clone());
    }
    out.push(AgentDelta::ItemChunk { item: id, text });
}

fn content_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text(t) => Some(t.text.clone()),
        _ => None,
    }
}

fn tool_added(t: &ToolCall, shared: &Shared) -> AgentDelta {
    let mut out = Vec::new();
    let turn = ensure_turn(&shared.turns, &mut out);
    AgentDelta::ItemAdd {
        turn,
        item: Item::new(
            t.tool_call_id.0.to_string(),
            ItemKind::Tool {
                name: t.title.clone(),
                title: t.title.clone(),
                tool_kind: map_kind(&t.kind),
                status: map_status(&t.status),
                input: t.raw_input.clone().unwrap_or(Value::Null),
                output: String::new(),
                locations: t
                    .locations
                    .iter()
                    .map(|l| l.path.display().to_string())
                    .collect(),
            },
        ),
    }
}

fn tool_patched(t: &ToolCallUpdate) -> AgentDelta {
    let f = &t.fields;
    AgentDelta::ItemPatch {
        item: t.tool_call_id.0.to_string(),
        patch: ItemPatch {
            status: f
                .status
                .as_ref()
                .and_then(|s| serde_json::to_value(map_status(s)).ok()),
            title: f.title.clone(),
            output: f.raw_output.as_ref().map(render_output),
            locations: f
                .locations
                .as_ref()
                .map(|ls| ls.iter().map(|l| l.path.display().to_string()).collect()),
            ..Default::default()
        },
    }
}

/// Cuánta salida de herramienta se guarda.
///
/// Un `read` de OpenCode devuelve el archivo COMPLETO en `raw_output`, más una
/// copia en `metadata.preview` y otra en `display.text` — leer un README de 200
/// líneas produjo ~30 KB de JSON para una tarjeta que se muestra plegada. Eso
/// va al disco con cada turno y al frontend con cada delta.
const MAX_SALIDA: usize = 8 * 1024;

/// La salida cruda de una herramienta, como texto y con tope.
fn render_output(v: &Value) -> String {
    let full = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    if full.len() <= MAX_SALIDA {
        return full;
    }
    // Se corta por carácter y no por byte: `full` es UTF-8 y partirlo a la
    // mitad de una tilde daría una cadena inválida.
    let corte = full
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|i| *i <= MAX_SALIDA)
        .last()
        .unwrap_or(0);
    format!(
        "{}

… recortado, {} caracteres más",
        &full[..corte],
        full.chars().count() - full[..corte].chars().count()
    )
}

/// Lo que lleva gastado la sesión, y cuánto de eso ya se le contó a un turno.
#[derive(Default)]
struct Costo {
    acumulado: f64,
    atribuido: f64,
}

impl Costo {
    /// Lo gastado desde el último cierre de turno, y lo da por atribuido.
    fn del_turno(&mut self) -> Option<f64> {
        let delta = self.acumulado - self.atribuido;
        self.atribuido = self.acumulado;
        (delta > 0.0).then_some(delta)
    }
}

fn usage(u: &UsageUpdate, shared: &Shared) -> AgentDelta {
    if let Some(c) = &u.cost {
        shared.cost.lock_or_recover().acumulado = c.amount;
    }
    AgentDelta::ThreadPatch {
        patch: ThreadPatch {
            tokens: Some(u.used),
            context_size: Some(u.size),
            ..Default::default()
        },
    }
}

fn plan_entries(entries: &[agent_client_protocol::schema::v1::PlanEntry]) -> Vec<PlanEntry> {
    entries
        .iter()
        .map(|e| PlanEntry {
            text: e.content.clone(),
            status: match e.status {
                agent_client_protocol::schema::v1::PlanEntryStatus::Pending => PlanStatus::Pending,
                agent_client_protocol::schema::v1::PlanEntryStatus::InProgress => {
                    PlanStatus::InProgress
                }
                _ => PlanStatus::Completed,
            },
        })
        .collect()
}

fn map_kind(k: &AcpToolKind) -> ToolKind {
    match k {
        AcpToolKind::Read => ToolKind::Read,
        AcpToolKind::Edit => ToolKind::Edit,
        AcpToolKind::Delete => ToolKind::Delete,
        AcpToolKind::Move => ToolKind::Move,
        AcpToolKind::Search => ToolKind::Search,
        AcpToolKind::Execute => ToolKind::Execute,
        AcpToolKind::Think => ToolKind::Think,
        AcpToolKind::Fetch => ToolKind::Fetch,
        AcpToolKind::SwitchMode => ToolKind::SwitchMode,
        _ => ToolKind::Other,
    }
}

fn map_status(s: &AcpToolStatus) -> ToolStatus {
    match s {
        AcpToolStatus::Pending => ToolStatus::Pending,
        AcpToolStatus::InProgress => ToolStatus::InProgress,
        AcpToolStatus::Completed => ToolStatus::Completed,
        _ => ToolStatus::Failed,
    }
}

/// El nombre de una variante que todavía no traducimos.
///
/// Solo las estables: `plan_update` y `plan_removed` viven tras la feature
/// `unstable` del crate y ni siquiera existen en el enum sin activarla.
fn variant_name(u: &SessionUpdate) -> &'static str {
    match u {
        SessionUpdate::ConfigOptionUpdate(_) => "config_option_update",
        SessionUpdate::SessionInfoUpdate(_) => "session_info_update",
        _ => "desconocido",
    }
}

struct AcpSession {
    tx: mpsc::UnboundedSender<Cmd>,
    emit: Emit,
    shared: Arc<Shared>,
}

impl AgentSession for AcpSession {
    fn send(&mut self, text: &str, origin: Option<Origin>) -> Result<(), String> {
        let files = origin.as_ref().map(|o| o.files.clone()).unwrap_or_default();
        let prompt = {
            let stripped = super::media::strip_embedded_paths(text, &files);
            if stripped.is_empty() && !files.is_empty() {
                "Mira esta imagen.".to_string()
            } else {
                stripped
            }
        };

        // Anunciar el turno YA: el hilo ACP puede tardar en conectar (cmd.exe
        // + cursor-agent). Sin esto la UI queda en «trabajando…» vacía y parece
        // que Cursor no responde, cuando ni siquiera empezó el prompt.
        let turn = start_turn(&self.shared.turns, &self.emit);
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

        self.tx
            .unbounded_send(Cmd::Prompt {
                turn,
                prompt,
                files,
            })
            .map_err(|_| "la sesión ya está cerrada".to_string())
    }

    fn set_model(
        &mut self,
        model: &str,
        effort: Option<&str>,
        fast: Option<bool>,
    ) -> Result<(), String> {
        self.tx
            .unbounded_send(Cmd::SetModel {
                model: model.to_string(),
                effort: effort.map(str::to_string),
                fast,
            })
            .map_err(|_| "la sesión ya está cerrada".to_string())
    }

    fn respond_permission(&mut self, id: &str, decision: PermissionDecision) -> Result<(), String> {
        // Una pregunta de Cursor contestada con los botones de permiso: sin
        // respuestas elegidas, cuenta como saltada.
        if let Some(ask) = self.shared.asks.lock_or_recover().remove(id) {
            let _ = ask.reply.send(None);
            return Ok(());
        }
        let tx = self.shared.pending.lock_or_recover().remove(id);
        match tx {
            Some(tx) => tx
                .send(decision)
                .map_err(|_| "el agente dejó de esperar esa respuesta".to_string()),
            // Contestar dos veces el mismo permiso no es un error: la ventana
            // pudo quedar con el botón a la vista después de que otra lo
            // resolviera.
            None => Ok(()),
        }
    }

    fn set_mode(&mut self, mode: &str) -> Result<(), String> {
        self.tx
            .unbounded_send(Cmd::SetMode {
                mode: mode.to_string(),
            })
            .map_err(|_| "la sesión ya está cerrada".to_string())
    }

    fn answer_permission(&mut self, id: &str, updated_input: Value) -> Result<(), String> {
        let ask = self
            .shared
            .asks
            .lock_or_recover()
            .remove(id)
            .ok_or_else(|| "esa pregunta ya no está esperando respuesta".to_string())?;
        let outcome = cursor_answers(&ask.questions, updated_input.get("answers"));
        ask.reply
            .send(Some(outcome))
            .map_err(|_| "el agente dejó de esperar esa respuesta".to_string())
    }

    fn interrupt(&mut self) -> Result<(), String> {
        let live = self.shared.live.lock_or_recover();
        let Some(live) = live.as_ref() else {
            // Todavía conectando o ya cerrada: no hay turno que cortar.
            return Ok(());
        };
        live.conn
            .send_notification(CancelNotification::new(live.session_id.clone()))
            .map_err(|e| format!("no se pudo interrumpir: {e}"))
    }

    fn stop(&mut self) {
        let _ = self.tx.unbounded_send(Cmd::Stop);
        self.tx.close_channel();
        // Cerrar el canal termina el bucle, que al salir cierra la conexión y,
        // con ella, mata el proceso del agente.
        let _ = &self.emit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        PermissionOption, PermissionOptionId, ToolCallUpdateFields,
    };

    /// Los tipos del esquema son `#[non_exhaustive]`: se construyen con sus
    /// builders y no con literales, para que agregar un campo en el protocolo
    /// no rompa a quien los arma.
    fn pedido(kinds: &[PermissionOptionKind]) -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            agent_client_protocol::schema::v1::SessionId::new("s"),
            ToolCallUpdate::new("tc1", ToolCallUpdateFields::new()),
            kinds
                .iter()
                .enumerate()
                .map(|(i, k)| PermissionOption::new(format!("o{i}"), format!("op{i}"), *k))
                .collect(),
        )
    }

    #[test]
    fn sin_hub_no_se_declara_ningun_mcp() {
        assert!(servidores_mcp(None, &[]).is_empty());
    }

    #[test]
    fn el_mcp_de_orquestacion_va_como_stdio_en_la_sesion() {
        let atic = super::super::hub::AticMcp {
            command: std::path::PathBuf::from("/opt/atic-mcp"),
            args: vec!["--host".into(), "grok".into()],
        };
        let servidores = servidores_mcp(Some(&atic), &[]);
        let [McpServer::Stdio(uno)] = &servidores[..] else {
            panic!("se esperaba un único servidor stdio");
        };
        assert_eq!(uno.name, "atic");
        assert_eq!(uno.command, std::path::PathBuf::from("/opt/atic-mcp"));
        assert_eq!(uno.args, ["--host", "grok"]);
    }

    #[test]
    fn los_servidores_del_modal_van_al_lado_del_de_orquestacion() {
        let extra = vec![super::super::mcp_servers::McpServerDef {
            name: "fs".into(),
            command: "/usr/bin/npx".into(),
            args: vec!["-y".into(), "server-fs".into()],
            env: vec![("TOKEN".into(), "abc".into())],
        }];
        let servidores = servidores_mcp(None, &extra);
        let [McpServer::Stdio(uno)] = &servidores[..] else {
            panic!("se esperaba un único servidor stdio");
        };
        assert_eq!(uno.name, "fs");
        assert_eq!(uno.command, std::path::PathBuf::from("/usr/bin/npx"));
        assert_eq!(uno.args, ["-y", "server-fs"]);
        assert_eq!(uno.env.len(), 1);
        assert_eq!(uno.env[0].name, "TOKEN");
        assert_eq!(uno.env[0].value, "abc");
    }

    #[test]
    fn permitir_elige_la_opcion_de_una_vez() {
        let req = pedido(&[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::RejectOnce,
        ]);
        assert_eq!(
            pick_option(&req, PermissionDecision::Allow),
            Some(PermissionOptionId::new("o0"))
        );
        assert_eq!(
            pick_option(&req, PermissionDecision::AllowAlways),
            Some(PermissionOptionId::new("o1"))
        );
        assert_eq!(
            pick_option(&req, PermissionDecision::Deny),
            Some(PermissionOptionId::new("o2"))
        );
    }

    /// «Siempre» que valió una vez es mejor que un botón que no hace nada.
    #[test]
    fn cae_al_pariente_mas_cercano_si_falta_la_opcion() {
        let req = pedido(&[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::RejectAlways,
        ]);
        assert_eq!(
            pick_option(&req, PermissionDecision::AllowAlways),
            Some(PermissionOptionId::new("o0")),
            "sin «permitir siempre», permitir una vez"
        );
        assert_eq!(
            pick_option(&req, PermissionDecision::Deny),
            Some(PermissionOptionId::new("o1")),
            "denegar tiene que denegar aunque solo exista la variante permanente"
        );
    }

    #[test]
    fn sin_opciones_no_hay_nada_que_elegir() {
        assert_eq!(pick_option(&pedido(&[]), PermissionDecision::Allow), None);
    }

    fn compartido() -> Shared {
        Shared {
            turns: Mutex::new(Turns::default()),
            pending: Mutex::new(HashMap::new()),
            asks: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashSet::new()),
            abiertos: Mutex::new(Vec::new()),
            cost: Mutex::new(Costo::default()),
            model_config_id: Mutex::new(None),
            effort_config_id: Mutex::new(None),
            model_templates: Mutex::new(HashMap::new()),
            live: Mutex::new(None),
        }
    }

    fn trozo(id: &str, texto: &str) -> ContentChunk {
        let mut c = ContentChunk::new(ContentBlock::Text(TextContent::new(texto)));
        c.message_id = Some(agent_client_protocol::schema::v1::MessageId::new(id));
        c
    }

    /// Sin esto el item queda escribiéndose para siempre: ACP no manda un
    /// «terminé», solo deja de mandar trozos.
    #[test]
    fn un_bloque_abierto_queda_anotado_para_cerrarlo() {
        let shared = compartido();
        let mut out = Vec::new();

        chunk(&trozo("m1", "ho"), "m", Role::Assistant, &shared, &mut out);
        chunk(&trozo("m1", "la"), "m", Role::Assistant, &shared, &mut out);

        assert_eq!(
            shared.abiertos.lock_or_recover().as_slice(),
            ["m:m1"],
            "el bloque se anota UNA vez, no una por trozo"
        );
    }

    /// El razonamiento y la respuesta de OpenCode comparten `messageId`, así
    /// que hay que cerrar los dos y no uno.
    #[test]
    fn el_razonamiento_y_la_respuesta_se_cierran_por_separado() {
        let shared = compartido();
        let mut out = Vec::new();

        chunk(
            &trozo("m1", "pienso"),
            "r",
            Role::Assistant,
            &shared,
            &mut out,
        );
        chunk(
            &trozo("m1", "digo"),
            "m",
            Role::Assistant,
            &shared,
            &mut out,
        );

        assert_eq!(
            shared.abiertos.lock_or_recover().as_slice(),
            ["r:m1", "m:m1"]
        );
    }

    #[test]
    fn niveles_de_sesion_van_a_los_modelos_sin_los_suyos() {
        let model = |id: &str, efforts: &[&str]| ModelInfo {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            efforts: efforts
                .iter()
                .map(|e| super::super::model::EffortOption {
                    id: (*e).into(),
                    description: String::new(),
                })
                .collect(),
            default_effort: None,
            supports_fast: false,
        };
        let cfg = EffortConfig {
            config_id: "thought_level".into(),
            current: "medium".into(),
            levels: vec![
                super::super::model::EffortOption {
                    id: "low".into(),
                    description: "Low".into(),
                },
                super::super::model::EffortOption {
                    id: "medium".into(),
                    description: "Medium".into(),
                },
            ],
        };
        let out = with_session_efforts(vec![model("a", &[]), model("b", &["high"])], Some(&cfg));
        let ids = |m: &ModelInfo| m.efforts.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&out[0]), ["low", "medium"]);
        assert_eq!(out[0].default_effort.as_deref(), Some("medium"));
        // El que ya trae los suyos no se toca.
        assert_eq!(ids(&out[1]), ["high"]);
        // Sin opción de esfuerzo, todo queda igual.
        assert!(with_session_efforts(vec![model("c", &[])], None)[0]
            .efforts
            .is_empty());
    }

    #[test]
    fn las_preguntas_de_cursor_se_contestan_con_ids() {
        let option = |id: &str, label: &str| CursorOption {
            id: id.into(),
            label: label.into(),
        };
        let questions = vec![
            CursorQuestion {
                id: "q1".into(),
                prompt: "¿Color?".into(),
                options: vec![option("r", "Rojo"), option("a", "Azul")],
                allow_multiple: false,
            },
            CursorQuestion {
                id: "q2".into(),
                prompt: "¿Tests?".into(),
                options: vec![option("u", "Unitarios"), option("e", "E2E")],
                allow_multiple: true,
            },
        ];
        let input = cursor_questions_input("Título", &questions);
        assert_eq!(input["questions"][0]["question"], "¿Color?");
        assert_eq!(input["questions"][1]["multiSelect"], true);
        assert_eq!(input["questions"][0]["options"][1]["label"], "Azul");

        let answers = serde_json::json!({ "¿Color?": "Azul", "¿Tests?": "Unitarios, E2E" });
        let out = cursor_answers(&questions, Some(&answers));
        assert_eq!(out["outcome"], "answered");
        assert_eq!(out["answers"][0]["questionId"], "q1");
        assert_eq!(
            out["answers"][0]["selectedOptionIds"],
            serde_json::json!(["a"])
        );
        assert_eq!(
            out["answers"][1]["selectedOptionIds"],
            serde_json::json!(["u", "e"])
        );
    }

    #[test]
    fn el_plan_de_cursor_llega_como_el_de_claude() {
        let req = CursorCreatePlanRequest {
            tool_call_id: Some("t1".into()),
            name: Some("Migrar".into()),
            overview: None,
            plan: "# Pasos\n1. Uno".into(),
            todos: vec![CursorTodo {
                id: "a".into(),
                content: "Uno".into(),
                status: "pending".into(),
            }],
        };
        let input = cursor_plan_input(&req);
        assert_eq!(input["plan"], "# Pasos\n1. Uno");
        assert_eq!(input["name"], "Migrar");
        assert_eq!(input["todos"][0]["content"], "Uno");
    }
}
