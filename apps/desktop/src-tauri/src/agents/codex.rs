//! Adaptador para **Codex**, sobre `codex app-server`.
//!
//! # Qué es esto por debajo
//!
//! JSON-RPC 2.0 por stdio, **una línea por mensaje** (JSONL, sin cabeceras
//! `Content-Length`). Comprobado contra `codex-cli 0.145.0` antes de escribir
//! esto: el encuadre, el orden de los mensajes y los nombres de los campos
//! salen de una sonda real, no del `--help`.
//!
//! El propio subcomando se marca `[experimental]` y su superficie es enorme —89
//! peticiones y 70 notificaciones—, así que acá se traduce **el subconjunto que
//! el modelo canónico sabe mostrar** y el resto se ignora en silencio.
//!
//! # Por qué el protocolo v2 encaja tan bien
//!
//! El plan suponía que Codex mandaba pares `exec_command_begin` / `…_end`, como
//! su protocolo viejo. El v2 es otra cosa: `item/started` → `item/completed`
//! sobre un **item con id estable**, que es exactamente la forma de `model.rs`.
//! La traducción quedó parecida a la de ACP y no a la de Claude Code.
//!
//! # El handshake es lento, y eso manda el diseño
//!
//! `thread/start` tardó **8 segundos** en la máquina donde se probó, porque
//! antes de contestar levanta todos los servidores MCP que el usuario tenga
//! configurados. Hacer ese ida y vuelta dentro de [`AgentBackend::start`]
//! congelaría la interfaz mientras se abre la burbuja.
//!
//! Por eso el handshake corre en el hilo lector y [`CodexSession::send`] no
//! espera a nadie: si el hilo todavía no existe, el mensaje queda **encolado** y
//! sale solo cuando el backend contesta. Para quien escribe, la sesión está
//! lista desde el primer momento.
//!
//! # Lo que Codex no da
//!
//! Costo en dólares. Manda tokens (`thread/tokenUsage/updated`) y el tamaño real
//! de la ventana, que es mejor que la constante que había escrita a mano, pero
//! `cost_usd` va en `None` y la vista no muestra plata para este backend.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

use super::model::{
    AgentDelta, Item, ItemId, ItemKind, ItemPatch, Origin, PermissionStatus, PlanEntry, PlanStatus,
    Role, ThreadPatch, ToolKind, ToolStatus, TurnStatus,
};
use super::turns::{end_turn, ensure_turn, start_turn, Emit, Turns};
use super::{AgentBackend, AgentSession, McpServerState, PermissionDecision, StartOptions};

pub struct Codex;

const PROGRAM: &str = "codex";

/// Tope de la salida de una herramienta que se guarda en el item.
///
/// Misma razón que en `acp.rs`: un `read` de un archivo mediano vuelve con
/// decenas de KB, y el hilo entero se persiste en una columna de SQLite.
const MAX_OUTPUT: usize = 8 * 1024;

/// Ids de las peticiones que hace el adaptador. Fijos porque son dos y el
/// lector tiene que reconocer sus respuestas sin llevar una tabla.
const ID_INITIALIZE: u64 = 1;
const ID_OPEN_THREAD: u64 = 2;
/// La lista de modelos se pide una vez, apenas se abre el hilo.
const ID_MODELS: u64 = 3;

/// Un turno pendiente de `threadId` (texto + imágenes ya leídas).
struct PendingPrompt {
    text: String,
    attachments: Vec<Value>,
}

/// Estado compartido entre la sesión (que escribe) y el hilo lector.
struct Shared {
    /// Quién abre el turno: lo abre quien escribe, y el lector se cuelga de él.
    turns: Mutex<Turns>,
    /// El stdin del proceso. Escriben los dos lados: la sesión manda turnos y
    /// el lector completa el handshake y contesta permisos.
    stdin: Mutex<Option<ChildStdin>>,
    /// Id del hilo **del lado de Codex**. Hasta que llega no se puede mandar.
    thread_id: Mutex<Option<String>>,
    /// Lo que el usuario escribió antes de que el hilo estuviera listo.
    ///
    /// El handshake tarda segundos; sin esta cola, escribir apenas se abre la
    /// burbuja daría un error de «sesión no lista» que no le importa a nadie.
    queued: Mutex<Vec<PendingPrompt>>,
    /// Turno abierto del lado de Codex, que es lo que hace falta para
    /// interrumpirlo. El nuestro (`t1`, `t2`) no le sirve al backend.
    provider_turn: Mutex<Option<String>>,
    /// Permiso pendiente: id de nuestro item → id de la petición JSON-RPC.
    ///
    /// Codex se queda esperando esa respuesta, así que el turno está detenido
    /// de verdad mientras el usuario decide.
    pending: Mutex<HashMap<ItemId, Value>>,
    /// Servidores MCP. Llegan de a uno y el parche los quiere todos juntos.
    mcp: Mutex<HashMap<String, String>>,
    /// Salida acumulada de cada herramienta que la manda por trozos.
    output: Mutex<HashMap<ItemId, String>>,
    /// Items ya anunciados. Lo que vuelve a llegar se parchea, no se agrega.
    seen: Mutex<HashSet<ItemId>>,
    /// Modelo y esfuerzo en curso.
    ///
    /// Codex no tiene un `thread/setModel`: los cambios viajan como override en
    /// `turn/start`, y ahí valen «para este turno y los siguientes». Así que
    /// hay que recordarlos y mandarlos con cada turno.
    model: Mutex<Option<String>>,
    effort: Mutex<Option<String>>,
    seq: AtomicU64,
}

impl Shared {
    fn write(&self, msg: &Value) -> Result<(), String> {
        let mut guard = self.stdin.lock().map_err(|_| "sesión rota".to_string())?;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| "la sesión ya está cerrada".to_string())?;
        writeln!(stdin, "{msg}").map_err(|e| format!("no se pudo enviar: {e}"))?;
        stdin.flush().map_err(|e| format!("no se pudo enviar: {e}"))
    }

    fn next_id(&self) -> u64 {
        // Arranca arriba de los dos fijos del handshake.
        self.seq.fetch_add(1, Ordering::SeqCst) + 10
    }

    /// Manda un turno, o lo encola si el hilo todavía no existe.
    fn prompt(&self, text: &str, attachments: Vec<Value>) -> Result<(), String> {
        let thread = self.thread_id.lock().ok().and_then(|g| g.clone());
        let Some(thread) = thread else {
            if let Ok(mut q) = self.queued.lock() {
                q.push(PendingPrompt {
                    text: text.to_string(),
                    attachments,
                });
            }
            return Ok(());
        };
        let mut params = json!({
            "threadId": thread,
            "input": [{ "type": "text", "text": text }],
        });
        if !attachments.is_empty() {
            params["attachments"] = Value::Array(attachments);
        }
        if let Some(m) = self.model.lock().ok().and_then(|g| g.clone()) {
            params["model"] = json!(m);
        }
        if let Some(e) = self.effort.lock().ok().and_then(|g| g.clone()) {
            params["effort"] = json!(e);
        }
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "turn/start",
            "params": params,
        }))
    }
}

/// Cómo se traducen nuestros modos de permiso a los de Codex.
///
/// Codex separa en dos ejes lo que la interfaz de Atic muestra como uno:
/// **a quién le pregunta** (`approvalPolicy`) y **qué puede tocar sin permiso**
/// (`sandbox`). Un modo nuestro fija los dos, porque ofrecer cuatro
/// combinaciones que solo Codex entiende rompería la promesa de que la vista no
/// sabe con quién habla.
fn policy(mode: Option<&str>) -> (&'static str, &'static str) {
    match mode.unwrap_or("manual") {
        // Solo planificar: que pregunte por todo y que no pueda escribir nada.
        "plan" => ("untrusted", "read-only"),
        "acceptEdits" => ("on-request", "workspace-write"),
        "bypassPermissions" => ("never", "danger-full-access"),
        _ => ("untrusted", "workspace-write"),
    }
}

impl AgentBackend for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn is_available(&self) -> bool {
        super::exe::resolve(PROGRAM).is_some()
    }

    fn start(
        &self,
        options: StartOptions,
        on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String> {
        // `launcher` y no `resolve`: hoy `codex` es un `.exe` de verdad, pero si
        // mañana se instala como shim hay que lanzarlo por el intérprete.
        let (program, prefix) = super::exe::launcher(PROGRAM).ok_or_else(|| {
            "no se encontró «codex» en el PATH. Instálalo y ábrelo una vez en la consola."
                .to_string()
        })?;

        let mut cmd = Command::new(program);
        cmd.args(prefix)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &options.cwd {
            cmd.current_dir(dir);
        }

        #[cfg(windows)]
        {
            // Sin esto, cada sesión abre una consola negra sobre la app.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("no se pudo iniciar Codex: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "el proceso no expuso stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "el proceso no expuso stdout".to_string())?;

        let emit = Emit::new(on_delta);
        let shared = Arc::new(Shared {
            turns: Mutex::new(Turns::default()),
            stdin: Mutex::new(Some(stdin)),
            thread_id: Mutex::new(None),
            queued: Mutex::new(Vec::new()),
            provider_turn: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            mcp: Mutex::new(HashMap::new()),
            output: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashSet::new()),
            model: Mutex::new(options.model.clone()),
            effort: Mutex::new(options.effort.clone()),
            seq: AtomicU64::new(0),
        });
        let stopping = Arc::new(AtomicBool::new(false));

        // stderr aparte, y fuera de cualquier `if` que condicione a stdout: son
        // dos flujos independientes y un fallo de arranque —sesión vencida, CLI
        // roto— sale por acá y no por el canal de eventos.
        if let Some(stderr) = child.stderr.take() {
            let emit = emit.clone();
            let shared = shared.clone();
            thread::spawn(move || {
                let mut seq = 0u64;
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    // Codex escribe sus trazas por stderr, así que solo se
                    // muestra lo que dice ser un error.
                    if !line.contains("ERROR") {
                        continue;
                    }
                    // Y de esos, no los de los servidores MCP del usuario. Un
                    // token de OAuth vencido en su Atlassian es un error real,
                    // pero no de esta conversación: sale en CADA turno y
                    // enterraría lo que el agente está diciendo. Se filtra por
                    // el módulo que lo emite y no por el texto, que cambia.
                    if line.contains("codex_rmcp_client") {
                        continue;
                    }
                    let line = sin_color(&line);
                    seq += 1;
                    let mut out = Vec::new();
                    let turn = ensure_turn(&shared.turns, &mut out);
                    out.push(AgentDelta::ItemAdd {
                        turn: turn.clone(),
                        item: Item::new(format!("{turn}-e{seq}"), ItemKind::Notice { text: line }),
                    });
                    emit.all(out);
                }
            });
        }

        {
            let emit = emit.clone();
            let shared = shared.clone();
            let died = stopping.clone();
            let mut tr = Translator::new(shared, options);
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    emit.all(tr.line(&line));
                }
                if !died.load(Ordering::SeqCst) {
                    emit.send(AgentDelta::Failed {
                        message: "Codex terminó inesperadamente.".to_string(),
                    });
                }
            });
        }

        // El handshake arranca acá y termina en el hilo lector: la respuesta de
        // `initialize` dispara `thread/start`, y la de este el vaciado de la cola.
        shared.write(&json!({
            "jsonrpc": "2.0",
            "id": ID_INITIALIZE,
            "method": "initialize",
            "params": { "clientInfo": { "name": "atic", "version": env!("CARGO_PKG_VERSION") } }
        }))?;

        Ok(Box::new(CodexSession {
            child,
            shared,
            stopping,
            emit,
        }))
    }
}

/// Traduce el JSONL de Codex al modelo canónico.
///
/// No emite: **devuelve** los deltas y los manda quien lo llama. Es lo que
/// permite probarlo sin proceso y sin canal — los tests le pasan una línea y
/// miran lo que sale.
struct Translator {
    shared: Arc<Shared>,
    options: StartOptions,
}

impl Translator {
    fn new(shared: Arc<Shared>, options: StartOptions) -> Self {
        Self { shared, options }
    }

    /// Una línea a cero o más deltas.
    fn line(&mut self, line: &str) -> Vec<AgentDelta> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };

        // Respuesta a algo que pedimos nosotros.
        if v.get("method").is_none() {
            return self.response(&v);
        }
        // Petición del servidor: lleva `id` y espera contestación.
        if v.get("id").is_some() {
            return self.server_request(&v);
        }
        self.notification(&v)
    }

    /// Respuestas del backend. Solo importan las dos del handshake.
    fn response(&mut self, v: &Value) -> Vec<AgentDelta> {
        let id = v.get("id").and_then(Value::as_u64);
        if let Some(err) = v.get("error") {
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("error del backend");
            return vec![AgentDelta::Failed {
                message: format!("Codex: {message}"),
            }];
        }

        match id {
            Some(ID_INITIALIZE) => {
                let (approval, sandbox) = policy(self.options.permission_mode.as_deref());
                let mut params = json!({
                    "approvalPolicy": approval,
                    "sandbox": sandbox,
                });
                if let Some(cwd) = &self.options.cwd {
                    params["cwd"] = json!(cwd);
                }
                if let Some(model) = &self.options.model {
                    params["model"] = json!(model);
                }
                // Reanudar es el MISMO handshake con otro método: el id del hilo
                // viejo entra por parámetro y todo lo demás sigue igual.
                let method = match &self.options.resume {
                    Some(id) => {
                        params["threadId"] = json!(id);
                        "thread/resume"
                    }
                    None => "thread/start",
                };
                let _ = self.shared.write(&json!({
                    "jsonrpc": "2.0",
                    "id": ID_OPEN_THREAD,
                    "method": method,
                    "params": params,
                }));
                // Y la lista de modelos, que no depende del hilo. Se pide acá y
                // no al abrir la burbuja porque es la primera vez que hay un
                // proceso con quien hablar.
                let _ = self.shared.write(&json!({
                    "jsonrpc": "2.0",
                    "id": ID_MODELS,
                    "method": "model/list",
                    "params": {},
                }));
                Vec::new()
            }

            Some(ID_MODELS) => {
                let models = super::discover::parse_codex_model_list(v);
                if models.is_empty() {
                    Vec::new()
                } else {
                    vec![AgentDelta::ThreadPatch {
                        patch: ThreadPatch {
                            models: Some(models),
                            ..Default::default()
                        },
                    }]
                }
            }

            Some(ID_OPEN_THREAD) => {
                let thread = v.pointer("/result/thread");
                let Some(id) = thread
                    .and_then(|t| t.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    return vec![AgentDelta::Failed {
                        message: "Codex abrió el hilo sin decir con qué id.".to_string(),
                    }];
                };
                if let Ok(mut slot) = self.shared.thread_id.lock() {
                    *slot = Some(id.clone());
                }

                let patch = ThreadPatch {
                    provider_session: Some(id),
                    cwd: v
                        .pointer("/result/cwd")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    model: v
                        .pointer("/result/model")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    ..Default::default()
                };

                // Lo que se escribió durante el handshake sale ahora, en orden.
                let queued: Vec<PendingPrompt> = self
                    .shared
                    .queued
                    .lock()
                    .map(|mut q| q.drain(..).collect())
                    .unwrap_or_default();
                for pending in queued {
                    let _ = self.shared.prompt(&pending.text, pending.attachments);
                }

                vec![AgentDelta::ThreadPatch { patch }]
            }

            _ => Vec::new(),
        }
    }

    /// Los pedidos de permiso: Codex espera la respuesta y el turno no avanza.
    fn server_request(&mut self, v: &Value) -> Vec<AgentDelta> {
        let method = v.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let (tool, kind) = match method {
            "item/commandExecution/requestApproval" => ("Shell", "un comando"),
            "item/fileChange/requestApproval" => ("Edit", "cambios en archivos"),
            "item/permissions/requestApproval" => ("Permisos", "permisos nuevos"),
            // No hay una forma de contestar formularios o texto libre desde la
            // tarjeta de permisos actual. Se rechazan explícitamente: ignorar
            // una petición JSON-RPC deja al backend esperando para siempre.
            "mcpServer/elicitation/request" => {
                if let Some(id) = v.get("id") {
                    let _ = self.shared.write(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "action": "decline", "content": null },
                    }));
                }
                return self.request_notice(method);
            }
            _ => {
                if let Some(id) = v.get("id") {
                    let _ = self.shared.write(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("Atic no admite la solicitud {method}"),
                        },
                    }));
                }
                return self.request_notice(method);
            }
        };

        // El id del item de permiso NO puede ser el del item que lo motiva: son
        // dos cosas distintas en la conversación y compartir id las pisaría.
        // Misma lección que dejó OpenCode repitiendo el `messageId`.
        let target = params
            .get("itemId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let id: ItemId = format!("perm:{target}");

        if let (Ok(mut pending), Some(rpc_id)) = (self.shared.pending.lock(), v.get("id")) {
            pending.insert(id.clone(), rpc_id.clone());
        }

        let description = params
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                params
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("Quiere ejecutar {kind}."));

        let mut out = Vec::new();
        let turn = ensure_turn(&self.shared.turns, &mut out);
        out.push(AgentDelta::ItemAdd {
            turn,
            item: Item::new(
                id,
                ItemKind::Permission {
                    tool: tool.to_string(),
                    description,
                    input: params,
                    status: PermissionStatus::Pending,
                },
            ),
        });
        out
    }

    fn request_notice(&self, method: &str) -> Vec<AgentDelta> {
        let mut out = Vec::new();
        let turn = ensure_turn(&self.shared.turns, &mut out);
        let id = format!("{turn}-n{}", self.shared.next_id());
        out.push(AgentDelta::ItemAdd {
            turn,
            item: Item::new(
                id,
                ItemKind::Notice {
                    text: format!(
                        "Codex pidió «{method}»; Atic lo rechazó porque aún no puede responderlo."
                    ),
                },
            ),
        });
        out
    }

    fn notification(&mut self, v: &Value) -> Vec<AgentDelta> {
        let method = v.get("method").and_then(Value::as_str).unwrap_or_default();
        let p = v.get("params").unwrap_or(&Value::Null);
        let mut out = Vec::new();

        match method {
            "thread/started" => {
                if let Some(id) = p.pointer("/thread/id").and_then(Value::as_str) {
                    if let Ok(mut slot) = self.shared.thread_id.lock() {
                        *slot = Some(id.to_string());
                    }
                }
            }

            "turn/started" => {
                if let Some(id) = p.pointer("/turn/id").and_then(Value::as_str) {
                    if let Ok(mut slot) = self.shared.provider_turn.lock() {
                        *slot = Some(id.to_string());
                    }
                }
                // El turno nuestro ya lo abrió quien escribió. Solo se garantiza
                // que exista, para el caso de un agente que habla sin que nadie
                // le haya escrito (al reanudar, típicamente).
                ensure_turn(&self.shared.turns, &mut out);
            }

            "turn/completed" => {
                let status = match p.pointer("/turn/status").and_then(Value::as_str) {
                    Some("completed") => TurnStatus::Done,
                    Some("failed") => TurnStatus::Failed,
                    Some(s) if s.contains("ancel") || s.contains("bort") => TurnStatus::Cancelled,
                    _ => TurnStatus::Done,
                };
                let turn = ensure_turn(&self.shared.turns, &mut out);
                out.push(AgentDelta::TurnEnd {
                    turn,
                    status,
                    // Codex informa tokens, no dinero.
                    cost_usd: None,
                });
                end_turn(&self.shared.turns);
                if let Ok(mut slot) = self.shared.provider_turn.lock() {
                    *slot = None;
                }
            }

            "item/started" => {
                if let Some((id, kind)) = self.item(p.get("item"), true) {
                    let turn = ensure_turn(&self.shared.turns, &mut out);
                    out.push(AgentDelta::ItemAdd {
                        turn,
                        item: Item::new(id, kind),
                    });
                }
            }

            "item/completed" => {
                if let Some(patch) = self.completed(p.get("item")) {
                    out.push(patch);
                }
            }

            "item/agentMessage/delta" => {
                if let (Some(item), Some(text)) = (
                    p.get("itemId").and_then(Value::as_str),
                    p.get("delta").and_then(Value::as_str),
                ) {
                    out.push(AgentDelta::ItemChunk {
                        item: format!("msg:{item}"),
                        text: text.to_string(),
                    });
                }
            }

            "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
                if let (Some(item), Some(text)) = (
                    p.get("itemId").and_then(Value::as_str),
                    p.get("delta").and_then(Value::as_str),
                ) {
                    out.push(AgentDelta::ItemChunk {
                        item: format!("rsn:{item}"),
                        text: text.to_string(),
                    });
                }
            }

            // La salida de un comando llega por trozos, pero `ItemChunk` solo
            // sabe acumular en texto y razonamiento —es lo que el modelo define—,
            // así que acá se junta y se entrega entera al cerrar el item.
            "item/commandExecution/outputDelta" => {
                if let (Some(item), Some(text)) = (
                    p.get("itemId").and_then(Value::as_str),
                    p.get("delta").and_then(Value::as_str),
                ) {
                    if let Ok(mut map) = self.shared.output.lock() {
                        let buf = map.entry(format!("tool:{item}")).or_default();
                        if buf.len() < MAX_OUTPUT {
                            buf.push_str(text);
                        }
                    }
                }
            }

            "thread/tokenUsage/updated" => {
                let used = p
                    .pointer("/tokenUsage/total/totalTokens")
                    .and_then(Value::as_u64);
                let size = p
                    .pointer("/tokenUsage/modelContextWindow")
                    .and_then(Value::as_u64);
                if used.is_some() || size.is_some() {
                    out.push(AgentDelta::ThreadPatch {
                        patch: ThreadPatch {
                            tokens: used,
                            context_size: size,
                            ..Default::default()
                        },
                    });
                }
            }

            "turn/plan/updated" => {
                let entries: Vec<PlanEntry> = p
                    .get("plan")
                    .and_then(Value::as_array)
                    .map(|steps| {
                        steps
                            .iter()
                            .map(|s| PlanEntry {
                                text: s
                                    .get("step")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                status: match s.get("status").and_then(Value::as_str) {
                                    Some("completed") => PlanStatus::Completed,
                                    Some("inProgress") => PlanStatus::InProgress,
                                    _ => PlanStatus::Pending,
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                // Un plan por turno, que se reemplaza entero. El id sale del
                // turno de Codex para que las revisiones sucesivas parcheen el
                // mismo item en vez de apilar planes.
                let key = p.get("turnId").and_then(Value::as_str).unwrap_or("t");
                let id = format!("plan:{key}");
                let primero = self
                    .shared
                    .seen
                    .lock()
                    .map(|mut s| s.insert(id.clone()))
                    .unwrap_or(true);
                if primero {
                    let turn = ensure_turn(&self.shared.turns, &mut out);
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

            "mcpServer/startupStatus/updated" => {
                let (Some(name), Some(status)) = (
                    p.get("name").and_then(Value::as_str),
                    p.get("status").and_then(Value::as_str),
                ) else {
                    return out;
                };
                let servers = {
                    let Ok(mut map) = self.shared.mcp.lock() else {
                        return out;
                    };
                    map.insert(name.to_string(), status.to_string());
                    let mut list: Vec<McpServerState> = map
                        .iter()
                        .map(|(name, status)| McpServerState {
                            name: name.clone(),
                            status: status.clone(),
                        })
                        .collect();
                    // Un `HashMap` no tiene orden, y sin ordenar la lista se
                    // reordenaría sola en cada actualización.
                    list.sort_by(|a, b| a.name.cmp(&b.name));
                    list
                };
                out.push(AgentDelta::ThreadPatch {
                    patch: ThreadPatch {
                        mcp_servers: Some(servers),
                        ..Default::default()
                    },
                });
            }

            "error" => {
                let message = p
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("error del backend")
                    .to_string();
                // Con reintento en camino no es el final de nada: decirlo como
                // fallo dejaría la sesión marcada como muerta y siguió andando.
                if p.get("willRetry").and_then(Value::as_bool) == Some(true) {
                    let turn = ensure_turn(&self.shared.turns, &mut out);
                    let id = format!("{turn}-w{}", self.shared.next_id());
                    out.push(AgentDelta::ItemAdd {
                        turn,
                        item: Item::new(id, ItemKind::Notice { text: message }),
                    });
                } else {
                    out.push(AgentDelta::Failed { message });
                }
            }

            // El resto del vocabulario —cuentas, límites de uso, plugins, watchers
            // de disco, sesiones de voz— se ignora **en silencio**, al revés que
            // en `claude_code.rs`, donde lo desconocido sale como `Notice`. Allá
            // el vocabulario es chico y una línea nueva vale la pena verla; acá
            // son 70 notificaciones y la mayoría no habla de la conversación:
            // mostrarlas enterraría lo que el agente está diciendo.
            _ => {}
        }
        out
    }

    /// Un item de Codex a nuestra forma. `nuevo` distingue started de completed.
    fn item(&self, item: Option<&Value>, nuevo: bool) -> Option<(ItemId, ItemKind)> {
        let item = item?;
        let raw = item.get("id").and_then(Value::as_str)?;
        let text_of = |k: &str| {
            item.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };

        match item.get("type").and_then(Value::as_str)? {
            // El turno del usuario ya lo emitió `send`, que es quien lo tiene
            // completo y quien abre el turno. Tomarlo también de acá lo
            // duplicaría en la conversación y en el disco.
            "userMessage" => None,

            "agentMessage" => Some((
                format!("msg:{raw}"),
                ItemKind::Message {
                    role: Role::Assistant,
                    text: text_of("text"),
                    streaming: nuevo,
                },
            )),

            "reasoning" => Some((
                format!("rsn:{raw}"),
                ItemKind::Reasoning {
                    text: item
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| text_of("content")),
                    streaming: nuevo,
                },
            )),

            "commandExecution" => {
                let command = text_of("command");
                Some((
                    format!("tool:{raw}"),
                    ItemKind::Tool {
                        name: "Shell".to_string(),
                        title: command.clone(),
                        tool_kind: ToolKind::Execute,
                        status: status_of(item),
                        input: json!({ "command": command, "cwd": item.get("cwd") }),
                        output: self.output_of(item, &format!("tool:{raw}")),
                        locations: Vec::new(),
                    },
                ))
            }

            "fileChange" => {
                let paths: Vec<String> = item
                    .get("changes")
                    .and_then(Value::as_array)
                    .map(|cs| {
                        cs.iter()
                            .filter_map(|c| c.get("path").and_then(Value::as_str))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                Some((
                    format!("tool:{raw}"),
                    ItemKind::Tool {
                        name: "Edit".to_string(),
                        title: title_for_paths(&paths),
                        tool_kind: ToolKind::Edit,
                        status: status_of(item),
                        input: item.get("changes").cloned().unwrap_or(Value::Null),
                        output: String::new(),
                        locations: paths,
                    },
                ))
            }

            "mcpToolCall" => {
                let tool = text_of("tool");
                let server = text_of("server");
                Some((
                    format!("tool:{raw}"),
                    ItemKind::Tool {
                        name: tool.clone(),
                        title: if server.is_empty() {
                            tool
                        } else {
                            format!("{server} · {tool}")
                        },
                        tool_kind: ToolKind::Other,
                        status: status_of(item),
                        input: item.get("arguments").cloned().unwrap_or(Value::Null),
                        output: cap(&stringify(item.get("result"))),
                        locations: Vec::new(),
                    },
                ))
            }

            "dynamicToolCall" => {
                let tool = text_of("tool");
                Some((
                    format!("tool:{raw}"),
                    ItemKind::Tool {
                        name: tool.clone(),
                        title: tool,
                        tool_kind: ToolKind::Other,
                        status: status_of(item),
                        input: item.get("arguments").cloned().unwrap_or(Value::Null),
                        output: cap(&stringify(item.get("contentItems"))),
                        locations: Vec::new(),
                    },
                ))
            }

            "webSearch" => {
                let query = text_of("query");
                Some((
                    format!("tool:{raw}"),
                    ItemKind::Tool {
                        name: "WebSearch".to_string(),
                        title: query.clone(),
                        tool_kind: ToolKind::Fetch,
                        status: if nuevo {
                            ToolStatus::InProgress
                        } else {
                            ToolStatus::Completed
                        },
                        input: json!({ "query": query }),
                        output: cap(&stringify(item.get("results"))),
                        locations: Vec::new(),
                    },
                ))
            }

            "imageView" => {
                let path = text_of("path");
                Some((
                    format!("tool:{raw}"),
                    ItemKind::Tool {
                        name: "Read".to_string(),
                        title: path.clone(),
                        tool_kind: ToolKind::Read,
                        status: if nuevo {
                            ToolStatus::InProgress
                        } else {
                            ToolStatus::Completed
                        },
                        input: json!({ "path": path }),
                        output: String::new(),
                        locations: vec![path],
                    },
                ))
            }

            // El nombre cambió en el protocolo experimental; se aceptan ambas
            // formas para poder abrir hilos guardados por versiones vecinas.
            "collabToolCall" | "collabAgentToolCall" => {
                let tool = text_of("tool");
                let title = item
                    .get("prompt")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.chars().take(120).collect())
                    .unwrap_or_else(|| tool.clone());
                Some((
                    format!("collab:{raw}"),
                    ItemKind::Collab {
                        name: tool.clone(),
                        title,
                        subagent_type: if tool == "spawnAgent" || tool == "spawn_agent" {
                            "task".to_string()
                        } else {
                            "other".to_string()
                        },
                        status: status_of(item),
                        summary: collab_summary(item),
                        parent_turn_id: None,
                        creation_source: "provider_native".to_string(),
                    },
                ))
            }

            "subAgentActivity" => {
                let kind = text_of("kind");
                let agent_path = text_of("agentPath");
                Some((
                    format!("collab:{raw}"),
                    ItemKind::Collab {
                        name: "subAgentActivity".to_string(),
                        title: if agent_path.is_empty() {
                            "Actividad de subagente".to_string()
                        } else {
                            agent_path
                        },
                        subagent_type: "task".to_string(),
                        status: match kind.as_str() {
                            "interrupted" => ToolStatus::Failed,
                            _ if nuevo => ToolStatus::InProgress,
                            _ => ToolStatus::Completed,
                        },
                        summary: kind,
                        parent_turn_id: None,
                        creation_source: "provider_native".to_string(),
                    },
                ))
            }

            // `plan` como item se ignora: el plan estructurado llega por
            // `turn/plan/updated`, y tomarlo de los dos lados lo mostraría
            // duplicado. El modo revisión y la compactación todavía no tienen
            // forma propia en este modelo.
            _ => None,
        }
    }

    /// El parche de un `item/completed`.
    fn completed(&self, item: Option<&Value>) -> Option<AgentDelta> {
        let (id, kind) = self.item(item, false)?;
        let patch = match kind {
            ItemKind::Message { text, .. } => ItemPatch {
                // El texto cerrado es el autoritativo y REEMPLAZA lo acumulado:
                // sumarlo escribiría la respuesta dos veces.
                text: Some(text),
                streaming: Some(false),
                ..Default::default()
            },
            ItemKind::Reasoning { text, .. } => ItemPatch {
                text: if text.is_empty() { None } else { Some(text) },
                streaming: Some(false),
                ..Default::default()
            },
            ItemKind::Tool {
                status,
                output,
                title,
                locations,
                ..
            } => ItemPatch {
                status: serde_json::to_value(status).ok(),
                output: Some(output),
                title: Some(title),
                locations: Some(locations),
                ..Default::default()
            },
            ItemKind::Collab {
                status,
                summary,
                title,
                subagent_type,
                ..
            } => ItemPatch {
                status: serde_json::to_value(status).ok(),
                summary: Some(summary),
                title: Some(title),
                subagent_type: Some(subagent_type),
                ..Default::default()
            },
            _ => return None,
        };
        Some(AgentDelta::ItemPatch { item: id, patch })
    }

    /// La salida de una herramienta: la del item, o la que se fue juntando.
    fn output_of(&self, item: &Value, id: &str) -> String {
        let aggregated = item
            .get("aggregatedOutput")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !aggregated.is_empty() {
            return cap(aggregated);
        }
        self.shared
            .output
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
            .map(|s| cap(&s))
            .unwrap_or_default()
    }
}

/// El estado de un item que lo trae, con el mismo vocabulario en todos.
fn status_of(item: &Value) -> ToolStatus {
    match item.get("status").and_then(Value::as_str) {
        Some("completed") => ToolStatus::Completed,
        // «declined» es el permiso denegado por el usuario: para la tarjeta es
        // lo mismo que un fallo —no se hizo—, y decirlo distinto obligaría a la
        // vista a conocer un estado que solo tiene este backend.
        Some("failed") | Some("declined") => ToolStatus::Failed,
        Some("inProgress") => ToolStatus::InProgress,
        _ => ToolStatus::Pending,
    }
}

fn collab_summary(item: &Value) -> String {
    let messages = item
        .get("agentsStates")
        .or_else(|| item.get("agentStates"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|states| states.values())
        .filter_map(|state| state.get("message").and_then(Value::as_str))
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    cap(&messages)
}

/// Un título legible para un cambio que toca varios archivos.
fn title_for_paths(paths: &[String]) -> String {
    match paths.len() {
        0 => "Cambios en archivos".to_string(),
        1 => paths[0].clone(),
        n => format!("{} y {} más", paths[0], n - 1),
    }
}

fn stringify(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn cap(s: &str) -> String {
    if s.len() <= MAX_OUTPUT {
        return s.to_string();
    }
    // Cortar por límite de carácter y no de byte: `s[..MAX]` entra en pánico si
    // cae en medio de una secuencia UTF-8, y la salida de un comando trae acentos.
    let mut end = MAX_OUTPUT;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Saca los códigos de color de una línea de traza.
///
/// Codex escribe stderr con color aunque del otro lado no haya terminal, así
/// que el texto llega con secuencias `ESC[…m` incrustadas. Sin limpiarlas el
/// aviso se dibuja con la basura a la vista —`^[[31mERROR^[[0m`— y encima se
/// guarda así en el hilo, que queda sucio para siempre.
fn sin_color(linea: &str) -> String {
    let mut out = String::with_capacity(linea.len());
    let mut chars = linea.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // Secuencia CSI: `ESC [`, parámetros, y termina en el primer byte
        // entre `@` y `~`. Lo que no siga esa forma se descarta igual: es
        // control, y nada de eso tiene sentido dentro de un texto.
        if chars.next() != Some('[') {
            continue;
        }
        for c in chars.by_ref() {
            if ('@'..='~').contains(&c) {
                break;
            }
        }
    }
    out
}

struct CodexSession {
    child: Child,
    shared: Arc<Shared>,
    /// Distingue un cierre pedido de una muerte del proceso.
    stopping: Arc<AtomicBool>,
    emit: Emit,
}

impl AgentSession for CodexSession {
    fn send(&mut self, text: &str, origin: Option<Origin>) -> Result<(), String> {
        // El turno lo abre quien escribe, y el mensaje del usuario es un item
        // más: sin esto la conversación guardada se lee como un monólogo.
        let files = origin.as_ref().map(|o| o.files.clone()).unwrap_or_default();
        let prompt = {
            let stripped = super::media::strip_embedded_paths(text, &files);
            if stripped.is_empty() && !files.is_empty() {
                "Mira esta imagen.".to_string()
            } else {
                stripped
            }
        };
        let mut attachments = Vec::new();
        for path in &files {
            match super::media::codex_image_attachment(std::path::Path::new(path)) {
                Ok(a) => attachments.push(a),
                Err(e) => tracing::warn!(%path, %e, "adjunto Codex omitido"),
            }
        }

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
        self.shared.prompt(&prompt, attachments)
    }

    /// No hay `thread/setModel`: se anota y sale con el próximo turno, que es
    /// donde Codex acepta el override.
    fn set_model(
        &mut self,
        model: &str,
        effort: Option<&str>,
        _fast: Option<bool>,
    ) -> Result<(), String> {
        if let Ok(mut g) = self.shared.model.lock() {
            *g = Some(model.to_string());
        }
        if let Ok(mut g) = self.shared.effort.lock() {
            *g = effort.map(str::to_string);
        }
        Ok(())
    }

    fn respond_permission(&mut self, id: &str, decision: PermissionDecision) -> Result<(), String> {
        let rpc_id = self
            .shared
            .pending
            .lock()
            .ok()
            .and_then(|mut m| m.remove(id))
            .ok_or_else(|| "ese permiso ya no está esperando respuesta".to_string())?;

        // Los tres botones que ya tiene la interfaz mapean uno a uno. `cancel`
        // —denegar Y cortar el turno— existe en Codex y no se ofrece: es otra
        // decisión, y la que hay significa «esto no, seguí».
        let value = match decision {
            PermissionDecision::Allow => "accept",
            PermissionDecision::AllowAlways => "acceptForSession",
            PermissionDecision::Deny => "decline",
        };
        self.shared.write(&json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "result": { "decision": value },
        }))?;

        self.emit.send(AgentDelta::ItemPatch {
            item: id.to_string(),
            patch: ItemPatch {
                status: serde_json::to_value(match decision {
                    PermissionDecision::Deny => PermissionStatus::Denied,
                    _ => PermissionStatus::Allowed,
                })
                .ok(),
                ..Default::default()
            },
        });
        Ok(())
    }

    fn interrupt(&mut self) -> Result<(), String> {
        let (thread, turn) = (
            self.shared.thread_id.lock().ok().and_then(|g| g.clone()),
            self.shared
                .provider_turn
                .lock()
                .ok()
                .and_then(|g| g.clone()),
        );
        match (thread, turn) {
            (Some(thread), Some(turn)) => self.shared.write(&json!({
                "jsonrpc": "2.0",
                "id": self.shared.next_id(),
                "method": "turn/interrupt",
                "params": { "threadId": thread, "turnId": turn },
            })),
            // Nada en vuelo: Detener es idempotente.
            _ => Ok(()),
        }
    }

    fn stop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Interrumpir antes de cerrar: un turno corriendo con herramientas a
        // medias termina más limpio si se le avisa que si se le cierra la boca.
        let _ = self.interrupt();
        if let Ok(mut slot) = self.shared.stdin.lock() {
            slot.take();
        }
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

impl Drop for CodexSession {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.shared.stdin.lock() {
            slot.take();
        }
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::MutexExt;

    /// Un traductor sin proceso detrás.
    ///
    /// `stdin` en `None` es justo lo que hace falta: lo que el traductor
    /// escribiría —el `thread/start`— se pierde sin romper nada, y lo que se
    /// comprueba son los deltas, que es lo que ve la vista.
    fn tr() -> Translator {
        let shared = Arc::new(Shared {
            turns: Mutex::new(Turns::default()),
            stdin: Mutex::new(None),
            thread_id: Mutex::new(None),
            queued: Mutex::new(Vec::new()),
            provider_turn: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            mcp: Mutex::new(HashMap::new()),
            output: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashSet::new()),
            model: Mutex::new(None),
            effort: Mutex::new(None),
            seq: AtomicU64::new(0),
        });
        Translator::new(shared, StartOptions::default())
    }

    /// Los items que agrega una tanda. Se mira esto y no el índice 0 porque
    /// abrir el turno puede meter un `TurnStart` delante.
    fn added(ds: &[AgentDelta]) -> Vec<&Item> {
        ds.iter()
            .filter_map(|d| match d {
                AgentDelta::ItemAdd { item, .. } => Some(item),
                _ => None,
            })
            .collect()
    }

    fn note(method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
    }

    #[test]
    fn el_mensaje_del_agente_abre_acumula_y_cierra() {
        let mut t = tr();

        let ds = t.line(&note(
            "item/started",
            json!({ "item": { "type": "agentMessage", "id": "msg_1", "text": "" } }),
        ));
        let items = added(&ds);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "msg:msg_1");
        assert!(matches!(
            items[0].kind,
            ItemKind::Message {
                streaming: true,
                ..
            }
        ));

        let ds = t.line(&note(
            "item/agentMessage/delta",
            json!({ "itemId": "msg_1", "delta": "hola" }),
        ));
        assert!(
            matches!(&ds[0], AgentDelta::ItemChunk { item, text } if item == "msg:msg_1" && text == "hola"),
            "el trozo tiene que apuntar al mismo item, salió {ds:?}"
        );

        let ds = t.line(&note(
            "item/completed",
            json!({ "item": { "type": "agentMessage", "id": "msg_1", "text": "hola." } }),
        ));
        match &ds[0] {
            AgentDelta::ItemPatch { item, patch } => {
                assert_eq!(item, "msg:msg_1");
                assert_eq!(patch.text.as_deref(), Some("hola."));
                assert_eq!(patch.streaming, Some(false));
            }
            otro => panic!("se esperaba un parche, salió {otro:?}"),
        }
    }

    /// La lección que dejó OpenCode: el tipo tiene que ir en el id.
    ///
    /// Si el razonamiento y la respuesta comparten el id crudo, los dos bloques
    /// terminan pegados en el mismo item.
    #[test]
    fn el_razonamiento_no_pisa_al_mensaje_con_el_mismo_id() {
        let mut t = tr();
        let a = t.line(&note(
            "item/started",
            json!({ "item": { "type": "agentMessage", "id": "x", "text": "" } }),
        ));
        let b = t.line(&note(
            "item/started",
            json!({ "item": { "type": "reasoning", "id": "x", "summary": "" } }),
        ));
        assert_eq!(added(&a)[0].id, "msg:x");
        assert_eq!(added(&b)[0].id, "rsn:x");
    }

    /// El turno del usuario lo emite `send`, que es quien lo tiene entero.
    #[test]
    fn el_mensaje_del_usuario_no_se_duplica() {
        let mut t = tr();
        let ds = t.line(&note(
            "item/started",
            json!({ "item": { "type": "userMessage", "id": "u1",
                              "content": [{ "type": "text", "text": "hola" }] } }),
        ));
        assert!(added(&ds).is_empty(), "salió {ds:?}");
    }

    #[test]
    fn un_comando_es_una_herramienta_de_ejecucion() {
        let mut t = tr();
        let ds = t.line(&note(
            "item/started",
            json!({ "item": { "type": "commandExecution", "id": "c1",
                              "command": "cargo test", "status": "inProgress" } }),
        ));
        match &added(&ds)[0].kind {
            ItemKind::Tool {
                title,
                tool_kind,
                status,
                ..
            } => {
                assert_eq!(title, "cargo test");
                assert_eq!(*tool_kind, ToolKind::Execute);
                assert_eq!(*status, ToolStatus::InProgress);
            }
            otro => panic!("se esperaba una herramienta, salió {otro:?}"),
        }
    }

    /// La salida llega por trozos y se entrega entera al cerrar el item.
    #[test]
    fn la_salida_por_trozos_se_junta_para_el_cierre() {
        let mut t = tr();
        t.line(&note(
            "item/started",
            json!({ "item": { "type": "commandExecution", "id": "c1", "command": "ls" } }),
        ));
        t.line(&note(
            "item/commandExecution/outputDelta",
            json!({ "itemId": "c1", "delta": "primera" }),
        ));
        t.line(&note(
            "item/commandExecution/outputDelta",
            json!({ "itemId": "c1", "delta": " y segunda" }),
        ));
        let ds = t.line(&note(
            "item/completed",
            json!({ "item": { "type": "commandExecution", "id": "c1",
                              "command": "ls", "status": "completed" } }),
        ));
        match &ds[0] {
            AgentDelta::ItemPatch { patch, .. } => {
                assert_eq!(patch.output.as_deref(), Some("primera y segunda"));
            }
            otro => panic!("se esperaba un parche, salió {otro:?}"),
        }
    }

    #[test]
    fn un_cambio_de_archivos_trae_sus_ubicaciones() {
        let mut t = tr();
        let ds = t.line(&note(
            "item/started",
            json!({ "item": { "type": "fileChange", "id": "f1", "status": "inProgress",
                              "changes": [ { "path": "a.rs", "kind": "update", "diff": "" },
                                           { "path": "b.rs", "kind": "add", "diff": "" } ] } }),
        ));
        match &added(&ds)[0].kind {
            ItemKind::Tool {
                title,
                locations,
                tool_kind,
                ..
            } => {
                assert_eq!(*tool_kind, ToolKind::Edit);
                assert_eq!(locations, &["a.rs", "b.rs"]);
                assert_eq!(title, "a.rs y 1 más");
            }
            otro => panic!("se esperaba una herramienta, salió {otro:?}"),
        }
    }

    #[test]
    fn una_colaboracion_se_agrega_y_se_parchea() {
        let mut t = tr();
        let abre = t.line(&note(
            "item/started",
            json!({ "item": { "type": "collabAgentToolCall", "id": "a1",
                              "tool": "spawnAgent", "status": "inProgress",
                              "senderThreadId": "h", "receiverThreadIds": ["sub1"],
                              "prompt": "Revisar el cambio" } }),
        ));
        match &added(&abre)[0].kind {
            ItemKind::Collab {
                name,
                title,
                subagent_type,
                status,
                ..
            } => {
                assert_eq!(name, "spawnAgent");
                assert_eq!(title, "Revisar el cambio");
                assert_eq!(subagent_type, "task");
                assert_eq!(*status, ToolStatus::InProgress);
            }
            other => panic!("se esperaba Collab, llegó {other:?}"),
        }

        let cierra = t.line(&note(
            "item/completed",
            json!({ "item": { "type": "collabAgentToolCall", "id": "a1",
                              "tool": "spawnAgent", "status": "completed",
                              "senderThreadId": "h", "receiverThreadIds": ["sub1"],
                              "agentsStates": { "sub1": {
                                  "status": "completed", "message": "Sin hallazgos"
                              } } } }),
        ));
        match &cierra[0] {
            AgentDelta::ItemPatch { item, patch } => {
                assert_eq!(item, "collab:a1");
                assert_eq!(patch.summary.as_deref(), Some("Sin hallazgos"));
                assert_eq!(
                    patch.status.as_ref(),
                    serde_json::to_value(ToolStatus::Completed).ok().as_ref()
                );
            }
            other => panic!("se esperaba un parche, llegó {other:?}"),
        }
    }

    #[test]
    fn el_permiso_queda_esperando_con_id_propio() {
        let mut t = tr();
        let ds = t.line(
            &json!({ "jsonrpc": "2.0", "id": 77,
                     "method": "item/commandExecution/requestApproval",
                     "params": { "itemId": "c1", "command": "rm -rf /", "reason": "peligroso" } })
            .to_string(),
        );
        let items = added(&ds);
        assert_eq!(
            items[0].id, "perm:c1",
            "no puede compartir id con el comando"
        );
        match &items[0].kind {
            ItemKind::Permission {
                status,
                description,
                ..
            } => {
                assert_eq!(*status, PermissionStatus::Pending);
                assert_eq!(description, "peligroso");
            }
            otro => panic!("se esperaba un permiso, salió {otro:?}"),
        }
        assert_eq!(
            t.shared.pending.lock_or_recover().get("perm:c1"),
            Some(&json!(77)),
            "hay que recordar a qué petición contestarle"
        );
    }

    #[test]
    fn una_solicitud_desconocida_no_queda_ignorada() {
        let mut t = tr();
        let ds = t.line(
            &json!({ "jsonrpc": "2.0", "id": 88,
                     "method": "item/tool/requestUserInput",
                     "params": { "questions": [] } })
            .to_string(),
        );
        assert!(
            matches!(&added(&ds)[0].kind, ItemKind::Notice { text }
                if text.contains("requestUserInput")),
            "salió {ds:?}"
        );
    }

    #[test]
    fn el_fin_de_turno_cierra_y_no_inventa_costo() {
        let mut t = tr();
        let ds = t.line(&note(
            "turn/completed",
            json!({ "threadId": "h", "turn": { "id": "T1", "status": "completed" } }),
        ));
        match ds.iter().find(|d| matches!(d, AgentDelta::TurnEnd { .. })) {
            Some(AgentDelta::TurnEnd {
                status, cost_usd, ..
            }) => {
                assert_eq!(*status, TurnStatus::Done);
                assert!(cost_usd.is_none(), "Codex informa tokens, no dinero");
            }
            _ => panic!("se esperaba el cierre del turno, salió {ds:?}"),
        }
    }

    /// El tamaño de la ventana lo dice el agente: era una constante a mano.
    #[test]
    fn el_uso_trae_lo_gastado_y_el_tamano_de_la_ventana() {
        let mut t = tr();
        let ds = t.line(&note(
            "thread/tokenUsage/updated",
            json!({ "threadId": "h", "turnId": "T1", "tokenUsage": {
                "total": { "totalTokens": 22488 },
                "last": { "totalTokens": 10 },
                "modelContextWindow": 258400 } }),
        ));
        match &ds[0] {
            AgentDelta::ThreadPatch { patch } => {
                assert_eq!(patch.tokens, Some(22488));
                assert_eq!(patch.context_size, Some(258400));
            }
            otro => panic!("se esperaba un parche del hilo, salió {otro:?}"),
        }
    }

    #[test]
    fn el_plan_se_agrega_una_vez_y_despues_se_parchea() {
        let mut t = tr();
        let paso = |estado: &str| {
            json!({ "threadId": "h", "turnId": "T1",
                    "plan": [ { "step": "leer", "status": estado } ] })
        };
        let a = t.line(&note("turn/plan/updated", paso("inProgress")));
        let b = t.line(&note("turn/plan/updated", paso("completed")));
        assert_eq!(added(&a).len(), 1, "la primera vez se agrega");
        assert!(added(&b).is_empty(), "la segunda se parchea, salió {b:?}");
        assert!(matches!(&b[0], AgentDelta::ItemPatch { patch, .. }
            if patch.entries.as_ref().is_some_and(|e| e[0].status == PlanStatus::Completed)));
    }

    /// Un error con reintento en camino no mata la sesión.
    #[test]
    fn el_error_reintentable_es_un_aviso_y_no_un_fallo() {
        let mut t = tr();
        let ds = t.line(&note(
            "error",
            json!({ "threadId": "h", "turnId": "T1", "willRetry": true,
                    "error": { "message": "se cortó la red" } }),
        ));
        assert!(matches!(&added(&ds)[0].kind, ItemKind::Notice { .. }));

        let ds = t.line(&note(
            "error",
            json!({ "threadId": "h", "turnId": "T1", "willRetry": false,
                    "error": { "message": "sesión vencida" } }),
        ));
        assert!(matches!(&ds[0], AgentDelta::Failed { .. }), "salió {ds:?}");
    }

    /// Son 70 notificaciones y la mayoría no habla de la conversación.
    #[test]
    fn lo_que_no_se_traduce_no_ensucia_el_registro() {
        let mut t = tr();
        for m in [
            "account/rateLimits/updated",
            "fs/changed",
            "thread/status/changed",
            "remoteControl/status/changed",
        ] {
            assert!(
                t.line(&note(m, json!({}))).is_empty(),
                "{m} no debería salir"
            );
        }
    }

    #[test]
    fn los_servidores_mcp_se_juntan_y_van_ordenados() {
        let mut t = tr();
        t.line(&note(
            "mcpServer/startupStatus/updated",
            json!({ "name": "zeta", "status": "starting" }),
        ));
        let ds = t.line(&note(
            "mcpServer/startupStatus/updated",
            json!({ "name": "alfa", "status": "ready" }),
        ));
        match &ds[0] {
            AgentDelta::ThreadPatch { patch } => {
                let s = patch.mcp_servers.as_ref().unwrap();
                assert_eq!(s.len(), 2, "se acumulan, no se reemplazan");
                assert_eq!(s[0].name, "alfa", "ordenados, o la lista baila sola");
            }
            otro => panic!("se esperaba un parche del hilo, salió {otro:?}"),
        }
    }

    /// Los dos ejes de Codex salen de un solo modo nuestro.
    #[test]
    fn los_modos_fijan_permiso_y_caja_de_arena() {
        assert_eq!(policy(Some("plan")), ("untrusted", "read-only"));
        assert_eq!(
            policy(Some("acceptEdits")),
            ("on-request", "workspace-write")
        );
        assert_eq!(
            policy(Some("bypassPermissions")),
            ("never", "danger-full-access")
        );
        assert_eq!(policy(None), ("untrusted", "workspace-write"));
    }

    /// Cortar por byte revienta si cae dentro de un acento.
    #[test]
    fn el_tope_de_salida_no_parte_un_caracter() {
        let largo = "á".repeat(MAX_OUTPUT);
        let corto = cap(&largo);
        assert!(corto.len() <= MAX_OUTPUT + 4, "tiene que recortar");
        assert!(corto.ends_with('…'));
    }

    /// Las trazas vienen con color aunque nadie las mire en una terminal.
    #[test]
    fn el_aviso_sale_sin_codigos_de_color() {
        let crudo = "\x1b[2m2026-07-27T17:17:25Z\x1b[0m \x1b[31mERROR\x1b[0m algo se rompió";
        assert_eq!(
            sin_color(crudo),
            "2026-07-27T17:17:25Z ERROR algo se rompió"
        );
    }

    #[test]
    fn una_linea_sin_color_no_se_toca() {
        assert_eq!(sin_color("ERROR pelado"), "ERROR pelado");
    }

    /// Escribir antes de que el hilo exista no puede perder el mensaje.
    #[test]
    fn lo_escrito_durante_el_handshake_queda_encolado() {
        let t = tr();
        t.shared.prompt("hola", Vec::new()).unwrap();
        let q = t.shared.queued.lock_or_recover();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].text, "hola");
    }
}
