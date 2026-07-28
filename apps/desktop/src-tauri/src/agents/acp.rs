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
    ContentBlock, ContentChunk, ImageContent, InitializeRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelect,
    SessionConfigSelectOption, SessionConfigSelectOptions, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, TextContent, ToolCall, ToolCallStatus as AcpToolStatus,
    ToolCallUpdate, ToolKind as AcpToolKind, UsageUpdate,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use serde_json::Value;

use super::model::{
    AgentDelta, Item, ItemId, ItemKind, ItemPatch, ModelInfo, Origin, PermissionStatus, PlanEntry,
    PlanStatus, Role, ThreadPatch, ToolKind, ToolStatus, TurnStatus,
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
    /// El texto y, si entró por un puente de Atic, por cuál.
    Prompt(String, Option<Origin>),
    SetModel {
        model: String,
        effort: Option<String>,
        fast: Option<bool>,
    },
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
            abiertos: Mutex::new(Vec::new()),
            cost: Mutex::new(Costo::default()),
            model_config_id: Mutex::new(None),
            effort_config_id: Mutex::new(None),
            model_templates: Mutex::new(HashMap::new()),
        });

        let (tx, rx) = mpsc::unbounded::<Cmd>();
        let cwd = options.cwd.clone().unwrap_or_else(|| ".".to_string());
        let desired_model = options.model.clone();
        let desired_effort = options.effort.clone();
        let desired_fast = options.fast;
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
                    program,
                    args,
                    cwd,
                    backend_id,
                    desired_model,
                    desired_effort,
                    desired_fast,
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
    backend_id: &'static str,
    desired_model: Option<String>,
    desired_effort: Option<String>,
    desired_fast: Option<bool>,
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

            let mut patch = ThreadPatch {
                provider_session: Some(session.session_id.0.to_string()),
                cwd: Some(cwd.clone()),
                ..Default::default()
            };

            if let Some(config_options) = &session.config_options {
                if let Some(model_cfg) = find_model_config(config_options) {
                    *shared.model_config_id.lock().unwrap() = Some(model_cfg.config_id);
                    let (models, templates) =
                        normalize_cursor_acp_models(backend_id, model_cfg.models);
                    *shared.model_templates.lock().unwrap() = templates;
                    if !models.is_empty() {
                        let current = model_cfg.current;
                        let (group_id, effort_id, fast) =
                            resolve_grouped_selection(&models, &current);
                        patch.models = Some(models);
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
                if let Some((effort_id, current)) = find_effort_config(config_options) {
                    *shared.effort_config_id.lock().unwrap() = Some(effort_id);
                    // Solo pisa si no vino ya del agrupado Cursor.
                    if patch.effort.is_none() {
                        patch.effort = Some(current);
                    }
                }
            }

            emit.send(AgentDelta::ThreadPatch { patch });

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
                    Cmd::Prompt(text, origin) => {
                        prompt_turn(&conn, &session.session_id, text, origin, &emit, &shared)
                            .await?;
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
}

async fn prompt_turn(
    conn: &ConnectionTo<Agent>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
    text: String,
    origin: Option<Origin>,
    emit: &Emit,
    shared: &Shared,
) -> agent_client_protocol::Result<()> {
    let files = origin
        .as_ref()
        .map(|o| o.files.clone())
        .unwrap_or_default();
    let prompt = {
        let stripped = super::media::strip_embedded_paths(&text, &files);
        if stripped.is_empty() && !files.is_empty() {
            "Mira esta imagen.".to_string()
        } else {
            stripped
        }
    };

    let turn = start_turn(&shared.turns, emit);
    emit.send(AgentDelta::ItemAdd {
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
    for id in shared.abiertos.lock().unwrap().drain(..) {
        emit.send(AgentDelta::ItemPatch {
            item: id,
            patch: ItemPatch {
                streaming: Some(false),
                ..Default::default()
            },
        });
    }

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
    Ok(())
}

struct ModelConfig {
    config_id: String,
    models: Vec<ModelInfo>,
    current: String,
}

fn is_model_option(opt: &SessionConfigOption) -> bool {
    matches!(opt.category, Some(SessionConfigOptionCategory::Model))
        || opt.id.0.contains("model")
}

fn is_effort_option(opt: &SessionConfigOption) -> bool {
    matches!(opt.category, Some(SessionConfigOptionCategory::ThoughtLevel))
        || {
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
    let has_effort = params.iter().any(|(k, _)| k == "effort" || k == "reasoning");
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
            upsert_param(
                &mut parsed.params,
                "fast",
                if f { "true" } else { "false" },
            );
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

fn find_effort_config(options: &[SessionConfigOption]) -> Option<(String, String)> {
    for opt in options {
        if !is_effort_option(opt) {
            continue;
        }
        let SessionConfigKind::Select(sel) = &opt.kind else {
            continue;
        };
        return Some((opt.id.0.to_string(), sel.current_value.0.to_string()));
    }
    None
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
    let has_effort_config = shared.effort_config_id.lock().unwrap().is_some();
    let templates = shared.model_templates.lock().unwrap().clone();

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
                Some(super::discover::compose_cursor_wire(m, "default", true, &[]))
            } else {
                Some(m.to_string())
            }
        } else {
            None
        }
    } else {
        model.map(str::to_string)
    };

    if let Some(wire) = wire_model.as_deref() {
        if let Some(config_id) = shared.model_config_id.lock().unwrap().clone() {
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

    if let Some(effort) = effort {
        if let Some(config_id) = shared.effort_config_id.lock().unwrap().clone() {
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
        shared.abiertos.lock().unwrap().push(id.clone());
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
    fn send(&mut self, text: &str, origin: Option<Origin>) -> Result<(), String> {
        self.tx
            .unbounded_send(Cmd::Prompt(text.to_string(), origin))
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

    fn compartido() -> Shared {
        Shared {
            turns: Mutex::new(Turns::default()),
            pending: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashSet::new()),
            abiertos: Mutex::new(Vec::new()),
            cost: Mutex::new(Costo::default()),
            model_config_id: Mutex::new(None),
            effort_config_id: Mutex::new(None),
            model_templates: Mutex::new(HashMap::new()),
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
            shared.abiertos.lock().unwrap().as_slice(),
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

        assert_eq!(shared.abiertos.lock().unwrap().as_slice(), ["r:m1", "m:m1"]);
    }
}
