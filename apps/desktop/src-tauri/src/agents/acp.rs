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
    ContentBlock, ContentChunk, InitializeRequest, NewSessionRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent, ToolCall,
    ToolCallStatus as AcpToolStatus, ToolCallUpdate, ToolKind as AcpToolKind, UsageUpdate,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use serde_json::Value;

use super::model::{
    AgentDelta, Item, ItemId, ItemKind, ItemPatch, PermissionStatus, PlanEntry, PlanStatus, Role,
    ThreadPatch, ToolKind, ToolStatus, TurnStatus,
};
use super::turns::{end_turn, ensure_turn, start_turn, Emit, Turns};
use super::{AgentBackend, AgentSession, PermissionDecision, SlashCommand, StartOptions};

/// Un agente ACP, descrito por cómo se lanza.
pub struct Acp {
    id: &'static str,
    display_name: &'static str,
    program: &'static str,
    args: &'static [&'static str],
}

/// Instalado con `npm i -g opencode-ai`. En Windows queda como shim `.cmd`.
pub const OPENCODE: Acp = Acp {
    id: "opencode",
    display_name: "OpenCode",
    program: "opencode",
    args: &["acp"],
};

/// `cursor-agent` expone ACP con el subcomando `acp`, igual que OpenCode.
pub const CURSOR: Acp = Acp {
    id: "cursor",
    display_name: "Cursor",
    program: "cursor-agent",
    args: &["acp"],
};

/// Lo que la sesión le pide al hilo de conexión.
enum Cmd {
    Prompt(String),
    Stop,
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
    /// Items de texto ya anunciados. Un chunk con id conocido continúa; uno
    /// nuevo abre.
    seen: Mutex<HashSet<ItemId>>,
    /// Costo de la sesión: lo último que informó el agente, y lo ya atribuido
    /// a turnos anteriores.
    ///
    /// ACP manda el costo **acumulado por sesión** en cada `usage_update`, y el
    /// turno quiere lo suyo. Sin restar, cada turno reportaría todo lo gastado
    /// antes y el total de la conversación crecería al cuadrado.
    cost: Mutex<Costo>,
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
            seen: Mutex::new(HashSet::new()),
            cost: Mutex::new(Costo::default()),
        });

        let (tx, rx) = mpsc::unbounded::<Cmd>();
        let cwd = options.cwd.clone().unwrap_or_else(|| ".".to_string());
        let args: Vec<String> = prefijo
            .into_iter()
            .chain(self.args.iter().map(|a| a.to_string()))
            .collect();
        let name = self.display_name;

        {
            let emit = emit.clone();
            let shared = shared.clone();
            std::thread::spawn(move || {
                let result = futures::executor::block_on(connect(
                    program,
                    args,
                    cwd,
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

/// Corre la conexión entera. Vive lo que la sesión.
async fn connect(
    program: std::path::PathBuf,
    args: Vec<String>,
    cwd: String,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    emit: Emit,
    shared: Arc<Shared>,
) -> Result<(), String> {
    let agent = AcpAgent::new(AcpAgentConfig::new(&program).args(args));

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
        move |req: RequestPermissionRequest, responder: agent_client_protocol::Responder<_>| {
            let (emit, shared) = (emit.clone(), shared.clone());
            async move {
                let id = format!("perm:{}", req.tool_call.tool_call_id.0);
                let (tx, wait) = oneshot::channel();
                shared.pending.lock().unwrap().insert(id.clone(), tx);

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

                // Acá se detiene el turno del agente hasta que el usuario decida.
                let decision = wait.await.unwrap_or(PermissionDecision::Deny);
                shared.pending.lock().unwrap().remove(&id);
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
                    // El agente no ofreció ninguna opción utilizable: cancelar
                    // es lo honesto, y deja el turno cerrado en vez de colgado.
                    None => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                }
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
            async move |req: RequestPermissionRequest, responder, _conn| perm(req, responder).await,
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |conn: ConnectionTo<Agent>| async move {
            conn.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;

            let session = conn
                .send_request(NewSessionRequest::new(std::path::PathBuf::from(&cwd)))
                .block_task()
                .await?;

            emit.send(AgentDelta::ThreadPatch {
                patch: ThreadPatch {
                    provider_session: Some(session.session_id.0.to_string()),
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                },
            });

            while let Some(cmd) = rx.next().await {
                let text = match cmd {
                    Cmd::Stop => break,
                    Cmd::Prompt(t) => t,
                };

                let turn = start_turn(&shared.turns, &emit);
                emit.send(AgentDelta::ItemAdd {
                    turn: turn.clone(),
                    item: Item::new(
                        format!("{turn}-u"),
                        ItemKind::Message {
                            role: Role::User,
                            text: text.clone(),
                            streaming: false,
                        },
                    ),
                });

                let done = conn
                    .send_request(PromptRequest::new(
                        session.session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(text))],
                    ))
                    .block_task()
                    .await;

                let status = match &done {
                    Ok(_) => TurnStatus::Done,
                    Err(_) => TurnStatus::Failed,
                };
                emit.send(AgentDelta::TurnEnd {
                    turn,
                    status,
                    cost_usd: shared.cost.lock().unwrap().del_turno(),
                });
                end_turn(&shared.turns);
                done?;
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
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
            if shared.seen.lock().unwrap().insert(id.clone()) {
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

        // Lo que todavía no traducimos se nombra en vez de descartarse: el
        // protocolo va a crecer, y tragar en silencio hace que lo nuevo se vea
        // como si nada hubiera pasado.
        other => {
            let turn = ensure_turn(&shared.turns, &mut out);
            let n = shared.seen.lock().unwrap().len();
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

    if shared.seen.lock().unwrap().insert(id.clone()) {
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
        shared.cost.lock().unwrap().acumulado = c.amount;
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
    fn send(&mut self, text: &str) -> Result<(), String> {
        self.tx
            .unbounded_send(Cmd::Prompt(text.to_string()))
            .map_err(|_| "la sesión ya está cerrada".to_string())
    }

    fn respond_permission(&mut self, id: &str, decision: PermissionDecision) -> Result<(), String> {
        let tx = self.shared.pending.lock().unwrap().remove(id);
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
}
