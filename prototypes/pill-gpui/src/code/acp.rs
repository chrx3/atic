//! Agentes que hablan ACP (Agent Client Protocol): OpenCode, Cursor y Grok en el chat
//! de Atic Code, junto a Claude (que va por el sidecar). Codex habla su propio
//! protocolo (`codex app-server`) y `codex.rs` lo traduce a lo mismo.
//!
//! ACP es JSON-RPC 2.0 por stdio, una línea JSON por mensaje, como el sidecar. Cada
//! conversación lanza su propio proceso (`opencode acp`, `gemini --acp`…), y lo que el
//! agente avisa (`session/update`) se traduce a los mismos eventos que manda el sidecar
//! (`stream`, `assistant`, `user`, `permission`, `result`): así el chat, las
//! herramientas, los permisos y la cola funcionan igual con cualquier agente.
//!
//! El cliente no ofrece archivos ni terminal (`clientCapabilities` en falso): cada
//! agente lee, edita y ejecuta con sus propias herramientas y pide permiso por
//! `session/request_permission`.
//!
//! Lo que cada agente deja elegir (su modelo, el nivel de razonamiento, sus modos como
//! Build y Plan) llega en `configOptions` y `modes` y va al chat como `acp_config`; sus
//! comandos con «/», como `acp_commands`. Se cambian con `session/set_config_option` y
//! `session/set_mode`.
//!
//! Para poder elegir antes del primer mensaje, `probe` abre una sesión de prueba
//! silenciosa (sin mandar nada, sin gastar cuota), lee esas opciones y la cierra. Lo que
//! el usuario elige entonces viaja en `start` (`want`) y se aplica al abrir la sesión,
//! antes de mandar el primer mensaje.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::{mpsc, oneshot};
use serde_json::{json, Value};

use super::sidecar::{Incoming, Reply};

mod codex;

/// Lo que se espera a que el agente arranque y abra la sesión (algunos, como
/// Gemini, tardan en cargar).
const START_TIMEOUT: Duration = Duration::from_secs(90);

/// Un agente ACP: su CLI y cómo se lo pone a hablar ACP.
pub struct AcpAgent {
    pub cli: &'static str,
    pub name: &'static str,
    args: &'static [&'static str],
    /// Cómo se inicia sesión si el agente la pide.
    login: &'static str,
    /// Habla el protocolo de `codex app-server`, no ACP.
    codex: bool,
}

/// Los agentes ACP que conoce Atic Code (probados con `initialize`, `session/new` y un
/// turno), y Codex por su `app-server`. Gemini no está: su `session/new` responde que el
/// cliente ya no se admite.
pub const AGENTS: [AcpAgent; 4] = [
    AcpAgent { cli: "codex", name: "Codex", args: &["app-server"], login: "codex login", codex: true },
    AcpAgent { cli: "opencode", name: "OpenCode", args: &["acp"], login: "opencode auth login", codex: false },
    AcpAgent { cli: "cursor-agent", name: "Cursor", args: &["acp"], login: "cursor-agent login", codex: false },
    AcpAgent { cli: "grok", name: "Grok", args: &["agent", "stdio"], login: "grok login", codex: false },
];

pub fn agent(cli: &str) -> Option<&'static AcpAgent> {
    AGENTS.iter().find(|agent| agent.cli == cli)
}

/// Los métodos del sidecar que un agente ACP atiende; el resto (uso de Claude, MCP,
/// Rewind…) responde vacío.
pub fn handles(method: &str) -> bool {
    matches!(method, "start" | "send" | "interrupt" | "permission" | "close" | "setConfig" | "setMode")
}

/// Las sesiones ACP de la ventana, por la clave de la conversación.
pub struct Hub {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    events: mpsc::UnboundedSender<Incoming>,
}

impl Hub {
    pub fn new(events: mpsc::UnboundedSender<Incoming>) -> Arc<Self> {
        Arc::new(Self { sessions: Mutex::default(), events })
    }

    /// Una petición con la forma de las del sidecar. La respuesta llega por el
    /// `oneshot`, como allá.
    pub fn call(self: &Arc<Self>, method: &str, params: Value) -> oneshot::Receiver<Reply> {
        let (reply, receiver) = oneshot::channel();
        let key = params.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
        let session = self.sessions.lock().unwrap().get(&key).cloned();
        match method {
            "start" => {
                if session.as_ref().is_some_and(|s| s.alive()) {
                    let id = session.and_then(|s| s.session_id.lock().unwrap().clone());
                    let _ = reply.send(Ok(json!({ "ok": true, "reused": true, "sessionId": id })));
                    return receiver;
                }
                // El proceso se lanza ya (así un `send` inmediato encuentra la sesión);
                // `initialize` y la sesión esperan respuesta: en su propio hilo.
                match self.spawn(&key, &params) {
                    Ok((session, cwd)) => {
                        let hub = self.clone();
                        let resume = params.get("resume").and_then(Value::as_str).map(str::to_string);
                        let want = params.get("want").cloned().filter(|w| !w.is_null());
                        std::thread::spawn(move || {
                            let result = session.open(&cwd, resume.as_deref(), want.as_ref());
                            if result.is_err() {
                                hub.sessions.lock().unwrap().remove(&session.key);
                                session.failed.store(true, Ordering::Relaxed);
                                session.kill();
                            }
                            let _ = reply.send(result);
                        });
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            "send" => {
                let result = match session {
                    Some(session) => session.prompt(&params).map(|_| json!({ "ok": true })),
                    None => Err("La sesión no está abierta".into()),
                };
                let _ = reply.send(result);
            }
            "interrupt" => {
                if let Some(session) = session {
                    session.cancel();
                }
                let _ = reply.send(Ok(json!({ "ok": true })));
            }
            "permission" => {
                if let Some(session) = session {
                    let request = params.get("requestId").and_then(Value::as_str).unwrap_or_default();
                    session.answer_permission(request, params.get("result").unwrap_or(&Value::Null));
                }
                let _ = reply.send(Ok(json!({ "ok": true })));
            }
            "setConfig" | "setMode" => {
                let result = match session {
                    Some(session) => session.configure(method, &params),
                    None => Err("La sesión no está abierta".into()),
                };
                let _ = reply.send(result.map(|_| json!({ "ok": true })));
            }
            "close" => {
                if let Some(session) = self.sessions.lock().unwrap().remove(&key) {
                    session.kill();
                }
                let _ = reply.send(Ok(json!({ "ok": true })));
            }
            _ => {
                let _ = reply.send(Ok(Value::Null));
            }
        }
        receiver
    }

    /// Lanza el proceso del agente y deja la sesión en la lista (todavía sin abrir).
    fn spawn(self: &Arc<Self>, key: &str, params: &Value) -> Result<(Arc<Session>, PathBuf), String> {
        let cli = params.get("agent").and_then(Value::as_str).unwrap_or_default();
        let agent = agent(cli).ok_or_else(|| format!("Agente desconocido: {cli}"))?;
        let cwd = PathBuf::from(params.get("cwd").and_then(Value::as_str).unwrap_or("."));
        if params.get("createCwd").and_then(Value::as_bool).unwrap_or(false) {
            let _ = std::fs::create_dir_all(&cwd);
        }
        let session = Session::spawn(agent, key, &cwd, self.events.clone(), false)?;
        self.sessions.lock().unwrap().insert(key.to_string(), session.clone());
        Ok((session, cwd))
    }

    /// Lo que el agente `cli` deja elegir (`configOptions`, `modes` y sus comandos),
    /// leído de una sesión de prueba en `cwd` que no manda nada y se cierra.
    pub fn probe(self: &Arc<Self>, cli: &str, cwd: PathBuf) -> oneshot::Receiver<Reply> {
        let (reply, receiver) = oneshot::channel();
        let events = self.events.clone();
        let cli = cli.to_string();
        std::thread::spawn(move || {
            let result = (|| {
                let agent = agent(&cli).ok_or_else(|| format!("Agente desconocido: {cli}"))?;
                let session = Session::spawn(agent, "acp-probe", &cwd, events, true)?;
                let opened = session.open(&cwd, None, None);
                // Codex no avisa comandos: no hay que esperarlos.
                if opened.is_ok() && !agent.codex {
                    // Los comandos llegan como aviso, un poco después de abrir.
                    let deadline = Instant::now() + Duration::from_millis(1500);
                    while session.translate.lock().unwrap().commands.is_null() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                session.kill();
                opened?;
                let translate = session.translate.lock().unwrap();
                Ok(json!({ "configOptions": translate.config, "modes": translate.modes, "commands": translate.commands }))
            })();
            let _ = reply.send(result);
        });
        receiver
    }

}

impl Drop for Session {
    /// Al cerrarse la ventana (o la conversación), el proceso del agente se va con ella.
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
        }
        // Al soltarse, `tree` se lleva al resto del árbol.
    }
}

/// El árbol de procesos de un agente. Cursor y los shims de npm corren tras
/// un `powershell`/`cmd`: matar al hijo directo dejaba vivo al `node` de
/// verdad (y a su `atic-mcp`), y si la pill moría de golpe no se mataba nada.
/// Un Job Object con `KILL_ON_JOB_CLOSE` mata el árbol entero al cerrarse su
/// handle, también cuando Windows cierra los handles de una pill que murió.
#[cfg(windows)]
struct Tree(isize);

#[cfg(windows)]
impl Tree {
    fn new(child: &Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Self(job as isize))
        }
    }

    fn kill(&self) {
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0 as _, 1) };
    }
}

#[cfg(windows)]
impl Drop for Tree {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _) };
    }
}

/// Lo que espera una petición nuestra: su respuesta.
type Waiter = std_mpsc::Sender<Result<Value, Value>>;

/// Un permiso pedido por el agente: el id de su petición y las opciones que ofreció.
struct Asked {
    rpc_id: Value,
    options: Vec<Value>,
}

struct Session {
    agent: &'static AcpAgent,
    key: String,
    child: Mutex<Child>,
    #[cfg(windows)]
    tree: Option<Tree>,
    stdin: Mutex<ChildStdin>,
    next: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, Waiter>>>,
    session_id: Mutex<Option<String>>,
    /// No se pudo abrir la sesión: lo que esperaba que abriera deja de esperar.
    failed: std::sync::atomic::AtomicBool,
    /// La sesión de prueba (`probe`): no avisa nada al chat.
    quiet: bool,
    asked: Arc<Mutex<HashMap<String, Asked>>>,
    translate: Arc<Mutex<Translator>>,
    /// Lo propio de una sesión de Codex (sus modelos, el turno en curso…).
    codex: Mutex<codex::State>,
    events: mpsc::UnboundedSender<Incoming>,
}

impl Session {
    fn spawn(agent: &'static AcpAgent, key: &str, cwd: &PathBuf, events: mpsc::UnboundedSender<Incoming>, quiet: bool) -> Result<Arc<Self>, String> {
        // `opencode` y `gemini` son shims de npm (`.cmd`): `launcher` los corre con cmd.
        let (program, prefix) = atic_agents::exe::launcher(agent.cli)
            .ok_or_else(|| format!("No se encontró {} en este equipo.", agent.name))?;
        let mut command = Command::new(program);
        command
            .args(prefix)
            .args(agent.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| format!("No se pudo iniciar {}: {e}", agent.name))?;
        #[cfg(windows)]
        let tree = Tree::new(&child);
        let stdin = child.stdin.take().ok_or("el agente no tiene stdin")?;
        let stdout = child.stdout.take().ok_or("el agente no tiene stdout")?;
        let stderr = child.stderr.take().ok_or("el agente no tiene stderr")?;
        let session = Arc::new(Self {
            agent,
            key: key.to_string(),
            child: Mutex::new(child),
            #[cfg(windows)]
            tree,
            stdin: Mutex::new(stdin),
            next: AtomicU64::new(0),
            pending: Arc::default(),
            session_id: Mutex::new(None),
            failed: std::sync::atomic::AtomicBool::new(false),
            quiet,
            asked: Arc::default(),
            translate: Arc::new(Mutex::new(Translator::new(key, agent.name))),
            codex: Mutex::default(),
            events,
        });
        let reader = Arc::downgrade(&session);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(session) = reader.upgrade() else {
                    return;
                };
                session.incoming(message);
            }
            if let Some(session) = reader.upgrade() {
                session.exited();
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::debug!("acp: {line}");
            }
        });
        Ok(session)
    }

    fn alive(&self) -> bool {
        self.child.lock().unwrap().try_wait().is_ok_and(|status| status.is_none())
    }

    fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
        #[cfg(windows)]
        if let Some(tree) = &self.tree {
            tree.kill();
        }
    }

    fn write(&self, message: &Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap();
        writeln!(stdin, "{message}").and_then(|_| stdin.flush()).map_err(|e| format!("El agente no responde: {e}"))
    }

    /// Una petición nuestra; la respuesta llega por el canal que devuelve.
    fn request(&self, method: &str, params: Value) -> Result<std_mpsc::Receiver<Result<Value, Value>>, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (waiter, receiver) = std_mpsc::channel();
        self.pending.lock().unwrap().insert(id, waiter);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        Ok(receiver)
    }

    /// Una petición que se espera aquí mismo (el arranque).
    fn request_wait(&self, method: &str, params: Value) -> Result<Value, String> {
        let receiver = self.request(method, params)?;
        match receiver.recv_timeout(START_TIMEOUT) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(self.error_text(&error)),
            Err(_) => Err(format!("{} no respondió a tiempo.", self.agent.name)),
        }
    }

    /// Un error de JSON-RPC para el aviso; si es de sesión, cómo iniciarla.
    fn error_text(&self, error: &Value) -> String {
        let message = error.get("message").and_then(Value::as_str).unwrap_or("Error del agente");
        let auth = error.get("code").and_then(Value::as_i64) == Some(-32000)
            || message.to_lowercase().contains("auth");
        // OpenCode responde «Authentication required» también cuando la cuenta está bien
        // pero el modelo pide una suscripción vencida: el motivo real queda en su log.
        if auth && self.agent.cli == "opencode" {
            if let Some(reason) = opencode_reason() {
                return reason;
            }
        }
        if auth {
            format!("{message}. Inicia sesión con `{}` en la terminal.", self.agent.login)
        } else {
            message.to_string()
        }
    }

    /// `initialize` y la sesión: nueva, o la anterior con `session/load` (su historial
    /// ya está en el chat, así que lo que repite el agente no se muestra).
    fn open(&self, cwd: &PathBuf, resume: Option<&str>, want: Option<&Value>) -> Reply {
        if self.agent.codex {
            return self.codex_open(cwd, resume, want);
        }
        let capabilities = json!({ "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false });
        let init = self.request_wait("initialize", json!({ "protocolVersion": 1, "clientCapabilities": capabilities }))?;
        let can_load = init.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool).unwrap_or(false);
        let cwd = cwd.to_string_lossy().to_string();
        let (id, opened) = match resume.filter(|_| can_load) {
            Some(id) => {
                self.translate.lock().unwrap().muted = true;
                let loaded = self.request_wait("session/load", json!({ "sessionId": id, "cwd": cwd, "mcpServers": [] }));
                self.translate.lock().unwrap().muted = false;
                match loaded {
                    Ok(result) => (id.to_string(), result),
                    // Si ya no existe, una nueva.
                    Err(_) => self.new_session(&cwd)?,
                }
            }
            None => self.new_session(&cwd)?,
        };
        self.translate.lock().unwrap().opened(&opened);
        // Lo elegido antes del primer mensaje (modelo, esfuerzo, modo): se aplica ahora,
        // antes de que la sesión quede abierta y salga el mensaje que espera.
        if let Some(want) = want {
            self.apply_wanted(&id, want);
        }
        let event = self.translate.lock().unwrap().config_event();
        self.send_all(vec![event]);
        *self.session_id.lock().unwrap() = Some(id.clone());
        Ok(json!({ "ok": true, "reused": true, "sessionId": id }))
    }

    /// Aplica a la sesión recién abierta lo que difiera de `want` (`{ config, mode }`).
    fn apply_wanted(&self, session_id: &str, want: &Value) {
        for (config, value) in want.get("config").and_then(Value::as_object).into_iter().flatten() {
            if self.translate.lock().unwrap().current(config).as_ref() == Some(value) {
                continue;
            }
            let request = json!({ "sessionId": session_id, "configId": config, "value": value });
            if let Ok(result) = self.request_wait("session/set_config_option", request.clone()) {
                self.translate.lock().unwrap().configured(&request, &result);
            }
        }
        if let Some(mode) = want.get("mode").filter(|m| m.is_string()) {
            if self.translate.lock().unwrap().modes.get("currentModeId") != Some(mode) {
                let request = json!({ "sessionId": session_id, "modeId": mode });
                if let Ok(result) = self.request_wait("session/set_mode", request.clone()) {
                    self.translate.lock().unwrap().configured(&request, &result);
                }
            }
        }
    }

    fn new_session(&self, cwd: &str) -> Result<(String, Value), String> {
        let created = self.request_wait("session/new", json!({ "cwd": cwd, "mcpServers": [] }))?;
        let id = created.get("sessionId").and_then(Value::as_str).map(str::to_string).ok_or("El agente no abrió la sesión")?;
        Ok((id, created))
    }

    /// Cambia una opción (`setConfig`: el modelo, el razonamiento…) o el modo
    /// (`setMode`). La respuesta llega aparte y el menú se pone al día con ella.
    fn configure(self: &Arc<Self>, method: &str, params: &Value) -> Result<(), String> {
        if self.agent.codex {
            self.codex_configure(method, params);
            return Ok(());
        }
        let session_id = self.session_id.lock().unwrap().clone().ok_or("La sesión todavía está abriendo")?;
        let (acp_method, request) = if method == "setMode" {
            let mode = params.get("modeId").cloned().unwrap_or(Value::Null);
            ("session/set_mode", json!({ "sessionId": session_id, "modeId": mode }))
        } else {
            let config = params.get("configId").cloned().unwrap_or(Value::Null);
            let value = params.get("value").cloned().unwrap_or(Value::Null);
            ("session/set_config_option", json!({ "sessionId": session_id, "configId": config, "value": value }))
        };
        let receiver = self.request(acp_method, request.clone())?;
        let session = self.clone();
        std::thread::spawn(move || {
            let events = match receiver.recv() {
                Ok(Ok(result)) => session.translate.lock().unwrap().configured(&request, &result),
                Ok(Err(error)) => vec![("error", json!({ "message": session.error_text(&error) }))],
                Err(_) => Vec::new(),
            };
            session.send_all(events);
        });
        Ok(())
    }

    /// La sesión del agente, esperando a que abra si todavía está arrancando.
    fn wait_open(&self) -> Option<String> {
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Some(id) = self.session_id.lock().unwrap().clone() {
                return Some(id);
            }
            if self.failed.load(Ordering::Relaxed) || !self.alive() || Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Un mensaje del usuario: el eco para el chat y `session/prompt`, cuya respuesta
    /// (al terminar el turno) llega como `result`. Si la sesión todavía está abriendo,
    /// el mensaje la espera.
    fn prompt(self: &Arc<Self>, params: &Value) -> Result<(), String> {
        let text = params.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut prompt = Vec::new();
        for image in params.get("images").and_then(Value::as_array).into_iter().flatten() {
            prompt.push(json!({
                "type": "image",
                "mimeType": image.get("mediaType").cloned().unwrap_or(Value::Null),
                "data": image.get("data").cloned().unwrap_or(Value::Null),
            }));
        }
        prompt.push(json!({ "type": "text", "text": text }));
        // El eco: el chat deja de esperar la burbuja que acaba de poner.
        self.emit("user", json!({ "isReplay": true, "content": [{ "type": "text", "text": text }] }));
        self.translate.lock().unwrap().begin_turn();
        if self.agent.codex {
            let images = params.get("images").and_then(Value::as_array).cloned().unwrap_or_default();
            self.codex_prompt(text, images);
            return Ok(());
        }
        let session = self.clone();
        std::thread::spawn(move || {
            let reply = match session.wait_open() {
                Some(id) => match session.request("session/prompt", json!({ "sessionId": id, "prompt": prompt })) {
                    Ok(receiver) => receiver.recv().unwrap_or_else(|_| Err(json!({ "message": "El agente se detuvo" }))),
                    Err(error) => Err(json!({ "message": error })),
                },
                None => Err(json!({ "message": format!("No se pudo abrir la sesión de {}.", session.agent.name) })),
            };
            let events = {
                let mut translate = session.translate.lock().unwrap();
                match &reply {
                    Ok(result) => translate.end_turn(result, None),
                    Err(error) => translate.end_turn(&Value::Null, Some(session.error_text(error))),
                }
            };
            session.send_all(events);
        });
        Ok(())
    }

    /// Detener: `session/cancel` y los permisos pendientes, cancelados.
    fn cancel(&self) {
        if self.agent.codex {
            return self.codex_cancel();
        }
        if let Some(id) = self.session_id.lock().unwrap().clone() {
            let _ = self.write(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": id } }));
        }
        let asked: Vec<Asked> = self.asked.lock().unwrap().drain().map(|(_, asked)| asked).collect();
        for asked in asked {
            let _ = self.write(&json!({ "jsonrpc": "2.0", "id": asked.rpc_id, "result": { "outcome": { "outcome": "cancelled" } } }));
        }
    }

    /// La respuesta del usuario a un permiso: la opción del agente que corresponde.
    fn answer_permission(&self, request: &str, result: &Value) {
        let Some(asked) = self.asked.lock().unwrap().remove(request) else {
            return;
        };
        let allow = result.get("behavior").and_then(Value::as_str) == Some("allow");
        let always = result.get("updatedPermissions").and_then(Value::as_array).is_some_and(|rules| !rules.is_empty());
        if self.agent.codex {
            return self.codex_answer(asked, allow, always);
        }
        let wanted: &[&str] = match (allow, always) {
            (true, true) => &["allow_always", "allow_once"],
            (true, false) => &["allow_once", "allow_always"],
            (false, _) => &["reject_once", "reject_always"],
        };
        let option = wanted.iter().find_map(|kind| {
            asked.options.iter().find(|option| option.get("kind").and_then(Value::as_str) == Some(kind))
        });
        let outcome = match option.and_then(|o| o.get("optionId")) {
            Some(id) => json!({ "outcome": "selected", "optionId": id }),
            None => json!({ "outcome": "cancelled" }),
        };
        let _ = self.write(&json!({ "jsonrpc": "2.0", "id": asked.rpc_id, "result": { "outcome": outcome } }));
    }

    fn incoming(&self, message: Value) {
        if self.agent.codex {
            return self.codex_incoming(message);
        }
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned().filter(|id| !id.is_null());
        match (method, id) {
            // La respuesta a una petición nuestra.
            (None, Some(id)) => self.answered(&id, &message),
            // El agente pide permiso: se pregunta en el chat.
            (Some("session/request_permission"), Some(id)) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let options = params.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
                let (request, events) = self.translate.lock().unwrap().permission(&params);
                self.asked.lock().unwrap().insert(request, Asked { rpc_id: id, options });
                self.send_all(events);
            }
            // Otra petición del agente (archivos, terminal): no las ofrecemos.
            (Some(_), Some(id)) => {
                let _ = self.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "No disponible" } }));
            }
            (Some("session/update"), None) => {
                let update = message.pointer("/params/update").cloned().unwrap_or(Value::Null);
                let events = self.translate.lock().unwrap().update(&update);
                self.send_all(events);
            }
            _ => {}
        }
    }

    /// La respuesta a una petición nuestra: a quien la espera.
    fn answered(&self, id: &Value, message: &Value) {
        let Some(id) = id.as_u64() else {
            return;
        };
        if let Some(waiter) = self.pending.lock().unwrap().remove(&id) {
            let reply = match message.get("error").filter(|e| !e.is_null()) {
                Some(error) => Err(error.clone()),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = waiter.send(reply);
        }
    }

    /// El proceso terminó: lo pendiente falla y la conversación se cierra.
    fn exited(&self) {
        for (_, waiter) in self.pending.lock().unwrap().drain() {
            let _ = waiter.send(Err(json!({ "message": "El agente se detuvo" })));
        }
        self.emit("closed", json!({}));
    }

    fn emit(&self, event: &str, data: Value) {
        if self.quiet {
            return;
        }
        let _ = self.events.unbounded_send(Incoming::Event { event: event.to_string(), key: Some(self.key.clone()), data });
    }

    fn send_all(&self, events: Vec<(&'static str, Value)>) {
        for (event, data) in events {
            self.emit(event, data);
        }
    }
}

/// Una herramienta del agente, con lo último que se supo de ella.
#[derive(Default, Clone)]
struct Tool {
    title: String,
    kind: String,
    raw_input: Value,
    locations: Vec<Value>,
    content: Vec<Value>,
    raw_output: Value,
}

/// Traduce lo que avisa un agente ACP a los eventos del sidecar que entiende el chat.
struct Translator {
    model: &'static str,
    /// El mensaje del asistente que se está escribiendo y sus bloques (texto o
    /// razonamiento), en orden.
    message: Option<String>,
    blocks: Vec<(bool, String)>,
    serial: u64,
    tools: HashMap<String, Tool>,
    permissions: u64,
    started: Option<Instant>,
    /// Lo que el agente deja elegir (`configOptions`) y sus modos (`modes`).
    config: Value,
    modes: Value,
    /// Los comandos con «/» (lo último que avisó el agente).
    commands: Value,
    /// Mientras se carga una sesión anterior: el agente repite su historial, que el
    /// chat ya tiene.
    muted: bool,
    #[allow(dead_code)]
    key: String,
}

impl Translator {
    fn new(key: &str, model: &'static str) -> Self {
        Self {
            model,
            message: None,
            blocks: Vec::new(),
            serial: 0,
            tools: HashMap::new(),
            permissions: 0,
            started: None,
            config: Value::Null,
            modes: Value::Null,
            commands: Value::Null,
            muted: false,
            key: key.to_string(),
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.serial += 1;
        format!("acp-{prefix}-{}", self.serial)
    }

    fn begin_turn(&mut self) {
        self.started = Some(Instant::now());
    }

    /// Lo que el menú del chat ofrece de este agente.
    fn config_event(&self) -> (&'static str, Value) {
        ("acp_config", json!({ "configOptions": self.config, "modes": self.modes }))
    }

    /// El valor actual de la opción `config`.
    fn current(&self, config: &str) -> Option<Value> {
        self.config
            .as_array()?
            .iter()
            .find(|option| option.get("id").and_then(Value::as_str) == Some(config))
            .and_then(|option| option.get("currentValue").cloned())
    }

    /// La sesión abierta (`session/new` o `session/load`): sus opciones y modos.
    fn opened(&mut self, result: &Value) -> Vec<(&'static str, Value)> {
        if let Some(config) = result.get("configOptions").filter(|v| v.is_array()) {
            self.config = config.clone();
        }
        if let Some(modes) = result.get("modes").filter(|v| v.is_object()) {
            self.modes = modes.clone();
        }
        vec![self.config_event()]
    }

    /// Respuesta a `set_config_option` o `set_mode`: lo nuevo que diga el agente o, si
    /// no dice nada, lo que se pidió.
    fn configured(&mut self, request: &Value, result: &Value) -> Vec<(&'static str, Value)> {
        if let Some(config) = result.get("configOptions").filter(|v| v.is_array()) {
            self.config = config.clone();
        } else if let (Some(id), Some(value)) = (request.get("configId").and_then(Value::as_str), request.get("value")) {
            for option in self.config.as_array_mut().into_iter().flatten() {
                if option.get("id").and_then(Value::as_str) == Some(id) {
                    option["currentValue"] = value.clone();
                }
            }
        }
        if let (Some(mode), true) = (request.get("modeId"), self.modes.is_object()) {
            self.modes["currentModeId"] = mode.clone();
        }
        vec![self.config_event()]
    }

    fn update(&mut self, update: &Value) -> Vec<(&'static str, Value)> {
        let kind = update.get("sessionUpdate").and_then(Value::as_str).unwrap_or_default();
        // Las opciones, el modo y los comandos valen también al cargar una sesión.
        match kind {
            "config_option_update" => {
                if let Some(config) = update.get("configOptions").filter(|v| v.is_array()) {
                    self.config = config.clone();
                }
                return vec![self.config_event()];
            }
            "current_mode_update" => {
                if let (Some(mode), true) = (update.get("currentModeId"), self.modes.is_object()) {
                    self.modes["currentModeId"] = mode.clone();
                }
                return vec![self.config_event()];
            }
            "available_commands_update" => {
                let commands = update.get("availableCommands").cloned().unwrap_or(Value::Null);
                self.commands = commands.clone();
                return vec![("acp_commands", json!({ "commands": commands }))];
            }
            _ => {}
        }
        if self.muted {
            return Vec::new();
        }
        match kind {
            "agent_message_chunk" | "agent_thought_chunk" => {
                let text = update.pointer("/content/text").and_then(Value::as_str).unwrap_or_default();
                if text.is_empty() {
                    return Vec::new();
                }
                self.chunk(kind == "agent_thought_chunk", text)
            }
            "tool_call" | "tool_call_update" => self.tool(update),
            "plan" => self.plan(update),
            _ => Vec::new(),
        }
    }

    /// Un trozo de texto o de razonamiento: abre el mensaje y el bloque si hace falta.
    fn chunk(&mut self, thinking: bool, text: &str) -> Vec<(&'static str, Value)> {
        let mut ops = Vec::new();
        let msg = match &self.message {
            Some(msg) => msg.clone(),
            None => {
                let msg = self.next_id("msg");
                ops.push(json!({ "op": "message", "msg": msg, "parent": null, "model": self.model }));
                self.message = Some(msg.clone());
                msg
            }
        };
        if self.blocks.last().is_none_or(|(kind, _)| *kind != thinking) {
            if !self.blocks.is_empty() {
                ops.push(json!({ "op": "stop", "msg": msg, "index": self.blocks.len() - 1 }));
            }
            self.blocks.push((thinking, String::new()));
            let kind = if thinking { "thinking" } else { "text" };
            ops.push(json!({ "op": "start", "msg": msg, "index": self.blocks.len() - 1, "kind": kind, "parent": null }));
        }
        let index = self.blocks.len() - 1;
        self.blocks[index].1.push_str(text);
        ops.push(json!({ "op": "delta", "msg": msg, "index": index, "text": text }));
        vec![("stream", Value::Array(ops))]
    }

    /// Cierra el mensaje en curso con su versión final (`assistant`).
    fn finish_message(&mut self) -> Vec<(&'static str, Value)> {
        let Some(msg) = self.message.take() else {
            return Vec::new();
        };
        let blocks = std::mem::take(&mut self.blocks);
        let mut events = Vec::new();
        if !blocks.is_empty() {
            events.push(("stream", json!([{ "op": "stop", "msg": msg, "index": blocks.len() - 1 }])));
        }
        let content: Vec<Value> = blocks
            .into_iter()
            .map(|(thinking, text)| {
                if thinking {
                    json!({ "type": "thinking", "thinking": text })
                } else {
                    json!({ "type": "text", "text": text })
                }
            })
            .collect();
        events.push(("assistant", json!({ "id": msg, "model": self.model, "parent": null, "content": content })));
        events
    }

    fn tool(&mut self, update: &Value) -> Vec<(&'static str, Value)> {
        let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
            return Vec::new();
        };
        let mut events = self.finish_message();
        let tool = self.tools.entry(id.to_string()).or_default();
        merge(tool, update);
        let tool = tool.clone();
        let (name, input) = claude_shape(&tool);
        let msg = self.next_id("tool");
        events.push(("assistant", json!({ "id": msg, "parent": null, "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] })));
        let status = update.get("status").and_then(Value::as_str).unwrap_or_default();
        if matches!(status, "completed" | "failed") {
            events.push((
                "user",
                json!({ "content": [{ "type": "tool_result", "tool_use_id": id, "content": result_text(&tool), "is_error": status == "failed" }] }),
            ));
        }
        events
    }

    /// El plan del agente se muestra como la lista de tareas de Claude (`TodoWrite`).
    fn plan(&mut self, update: &Value) -> Vec<(&'static str, Value)> {
        let todos: Vec<Value> = update
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| {
                let content = entry.get("content").and_then(Value::as_str).unwrap_or_default();
                let status = entry.get("status").and_then(Value::as_str).unwrap_or("pending");
                json!({ "content": content, "activeForm": content, "status": status })
            })
            .collect();
        let mut events = self.finish_message();
        let msg = self.next_id("plan");
        events.push(("assistant", json!({ "id": msg, "parent": null, "content": [{ "type": "tool_use", "id": "acp-plan", "name": "TodoWrite", "input": { "todos": todos } }] })));
        events.push(("user", json!({ "content": [{ "type": "tool_result", "tool_use_id": "acp-plan", "content": "", "is_error": false }] })));
        events
    }

    /// Un pedido de permiso: la herramienta (si aún no se mostró) y la solicitud.
    fn permission(&mut self, params: &Value) -> (String, Vec<(&'static str, Value)>) {
        self.permissions += 1;
        let request = format!("acp-perm-{}", self.permissions);
        let call = params.get("toolCall").cloned().unwrap_or(Value::Null);
        let mut events = if call.get("toolCallId").is_some() { self.tool(&call) } else { self.finish_message() };
        let id = call.get("toolCallId").and_then(Value::as_str).unwrap_or_default();
        let tool = self.tools.get(id).cloned().unwrap_or_default();
        let (name, input) = claude_shape(&tool);
        // «Permitir siempre» solo si el agente lo ofrece: va como una regla sobre la
        // herramienta (lo que se ve en su globito).
        let always = params
            .get("options")
            .and_then(Value::as_array)
            .is_some_and(|options| options.iter().any(|o| o.get("kind").and_then(Value::as_str) == Some("allow_always")));
        let content = ["command", "file_path", "url", "pattern"].iter().find_map(|field| input.get(*field).and_then(Value::as_str)).map(str::to_string);
        let suggestions = always.then(|| json!([{ "rules": [{ "toolName": name, "ruleContent": content }] }]));
        events.push((
            "permission",
            json!({
                "requestId": request,
                "toolName": name,
                "input": input,
                "toolUseID": id,
                "suggestions": suggestions,
                "suppressAlwaysAllowRule": !always,
            }),
        ));
        (request, events)
    }

    /// Fin del turno: el mensaje en curso se cierra y llega el `result` del sidecar.
    fn end_turn(&mut self, result: &Value, error: Option<String>) -> Vec<(&'static str, Value)> {
        let mut events = self.finish_message();
        let duration = self.started.take().map(|at| at.elapsed().as_millis() as u64).unwrap_or(0);
        let stop = result.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn");
        let output = result.pointer("/usage/outputTokens").cloned().unwrap_or(Value::Null);
        let usage = json!({ "output_tokens": output });
        let data = match (error, stop) {
            (Some(error), _) => json!({ "subtype": "error", "isError": true, "errors": [error], "durationMs": duration }),
            (None, "end_turn") => json!({ "subtype": "success", "isError": false, "durationMs": duration, "usage": usage }),
            (None, "cancelled") => json!({ "subtype": "error_during_execution", "isError": true, "errors": [], "durationMs": duration }),
            (None, other) => {
                let why = match other {
                    "max_tokens" => "Se alcanzó el máximo de tokens de la respuesta.",
                    "max_turn_requests" => "Se alcanzó el máximo de pasos del turno.",
                    "refusal" => "El agente se negó a continuar.",
                    _ => "El turno terminó de forma inesperada.",
                };
                json!({ "subtype": "error", "isError": true, "errors": [why], "durationMs": duration, "usage": usage })
            }
        };
        events.push(("result", data));
        events
    }
}

// --- Cómo se muestra lo que manda cada agente ----------------------------------------

/// Un id de modelo legible: «claude-opus-5-5» → «Claude Opus 5.5», «gemini-4-argon» →
/// «Gemini 4 Argon». Los números seguidos se unen con punto; lo que ya trae mayúsculas
/// queda como está (`MiniMax-M2.5`).
pub fn pretty_model(id: &str) -> String {
    let upper = ["gpt", "glm", "oss", "ai", "xai", "gpu", "api", "llm", "r1", "v3", "v4"];
    let mut words: Vec<String> = Vec::new();
    for token in id.split(['-', '_']).filter(|t| !t.is_empty()) {
        let numeric = token.chars().all(|c| c.is_ascii_digit());
        let last_numeric = words.last().is_some_and(|w| w.chars().last().is_some_and(|c| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_digit() || c == '.'));
        if numeric && last_numeric {
            let last = words.last_mut().unwrap();
            last.push('.');
            last.push_str(token);
            continue;
        }
        let word = if upper.contains(&token.to_ascii_lowercase().as_str()) {
            token.to_ascii_uppercase()
        } else if token.chars().any(|c| c.is_ascii_uppercase()) {
            token.to_string()
        } else {
            let mut chars = token.chars();
            chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        };
        words.push(word);
    }
    words.join(" ")
}

/// El nombre de un proveedor de OpenCode («opencode-go» → «OpenCode Go»). OpenCode solo
/// lista los proveedores con credenciales y los modelos gratuitos de Zen (`-free`), que
/// no piden suscripción.
pub fn provider_name(id: &str) -> String {
    match id {
        "opencode" => "OpenCode Zen · gratis".into(),
        "opencode-go" => "OpenCode Go".into(),
        "minimax" => "MiniMax".into(),
        "minimax-cn" => "MiniMax (China)".into(),
        "minimax-coding-plan" => "MiniMax Coding Plan".into(),
        "minimax-cn-coding-plan" => "MiniMax Coding Plan (China)".into(),
        "nvidia" => "NVIDIA".into(),
        "anthropic" => "Anthropic".into(),
        "openai" => "OpenAI".into(),
        "google" => "Google".into(),
        "deepseek" => "DeepSeek".into(),
        "xai" => "xAI".into(),
        "openrouter" => "OpenRouter".into(),
        "github-copilot" => "GitHub Copilot".into(),
        "moonshotai" => "Moonshot AI".into(),
        "zai" | "zhipuai" => "Z.ai".into(),
        other => pretty_model(other),
    }
}

/// Un modelo de la lista de un agente: su nombre, el detalle que va debajo y el grupo
/// (el proveedor, en OpenCode). Cursor manda variantes con sus parámetros en el valor
/// (`claude-opus-5[thinking=true,context=300k,effort=high,fast=false]`).
pub fn model_entry(value: &str, name: &str) -> (String, Option<String>, Option<String>) {
    // OpenCode: «proveedor/modelo» en el valor, y el nombre ya legible o como id
    // («opencode-go/Cat Jam Free», «minimax/MiniMax-M2»). Los gratuitos terminan en
    // `-free` (y los de Zen lo son todos).
    if let Some((provider, model)) = value.split_once('/').or_else(|| name.split_once('/')) {
        let shown = name.split_once('/').map_or(name, |(_, rest)| rest);
        let shown = if shown.is_empty() { model } else { shown };
        let free = value.ends_with("-free") || provider == "opencode";
        let clean = shown.strip_suffix("-free").or_else(|| shown.strip_suffix(" Free")).unwrap_or(shown);
        let label = if clean.contains(' ') { clean.to_string() } else { pretty_model(clean) };
        return (label, free.then(|| "Gratis".to_string()), Some(provider_name(provider)));
    }
    let label = if name.chars().any(|c| c.is_ascii_uppercase()) || name.contains(' ') { name.to_string() } else { pretty_model(name) };
    let params = value.split_once('[').and_then(|(_, rest)| rest.strip_suffix(']')).unwrap_or_default();
    let mut detail = Vec::new();
    for pair in params.split(',').filter(|p| !p.is_empty()) {
        let (key, val) = pair.split_once('=').unwrap_or((pair, ""));
        match (key, val) {
            ("context", size) => detail.push(format!("{size} de contexto")),
            ("effort" | "reasoning_effort", level) => detail.push(format!("esfuerzo {}", effort_name(level).to_lowercase())),
            ("thinking", "true") => detail.push("razonamiento".into()),
            ("fast", "true") => detail.push("rápido".into()),
            _ => {}
        }
    }
    (label, (!detail.is_empty()).then(|| detail.join(" · ")), None)
}

/// El orden de los grupos de modelos: primero la suscripción de OpenCode, después sus
/// gratuitos y luego el resto por nombre.
pub fn group_rank(group: &str) -> (usize, String) {
    let rank = match group {
        "OpenCode Go" => 0,
        "OpenCode Zen · gratis" => 1,
        _ => 2,
    };
    (rank, group.to_lowercase())
}

/// Cómo se actualiza cada CLI (Antigravity no tiene comando: se repite su instalador).
pub fn update_command(cli: &str) -> Option<&'static str> {
    match cli {
        "claude" => Some("claude update"),
        "opencode" => Some("opencode upgrade"),
        "codex" => Some("codex update"),
        "cursor-agent" => Some("cursor-agent update"),
        "grok" => Some("grok update"),
        "gemini" => Some("npm install -g @google/gemini-cli@latest"),
        _ => None,
    }
}

/// El orden de un nivel de esfuerzo (para dibujarlos de menor a mayor).
pub fn effort_rank(id: &str) -> usize {
    match id {
        "minimal" | "none" => 0,
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "xhigh" => 4,
        "max" => 5,
        "ultra" => 6,
        _ => 7,
    }
}

/// El nombre de un nivel de esfuerzo, como los de Claude.
pub fn effort_name(id: &str) -> String {
    match id {
        "minimal" | "none" => "Mínimo".into(),
        "low" => "Bajo".into(),
        "medium" => "Medio".into(),
        "high" => "Alto".into(),
        "xhigh" => "Muy alto".into(),
        "max" => "Máximo".into(),
        "default" | "" => "Predeterminado".into(),
        other => pretty_model(other),
    }
}

/// El nombre y la explicación de un modo conocido (los de OpenCode y Cursor).
pub fn mode_text(id: &str, name: &str, description: &str) -> (String, String) {
    let (label, hint) = match id {
        "build" => ("Build", "Trabaja con las herramientas según sus permisos"),
        "agent" => ("Agente", "Trabaja con todas sus herramientas"),
        "plan" => ("Plan", "Solo lectura: explora y propone un plan antes de tocar nada"),
        "ask" => ("Preguntar", "Responde preguntas, sin editar ni ejecutar"),
        _ => ("", ""),
    };
    if label.is_empty() {
        (name.to_string(), description.to_string())
    } else {
        (label.to_string(), hint.to_string())
    }
}

/// El motivo del último error que OpenCode escribió en su log (si lo escribió hace
/// menos de un minuto), en palabras del chat.
fn opencode_reason() -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let home = crate::paths::home().map(std::ffi::OsString::from).or_else(|| std::env::var_os("HOME"))?;
    let path = PathBuf::from(home).join(".local/share/opencode/log/opencode.log");
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    if modified.elapsed().ok()? > Duration::from_secs(60) {
        return None;
    }
    // El log crece sin límite: solo su final.
    let mut file = std::fs::File::open(&path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(64 * 1024))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    let line = tail.lines().rev().find(|line| line.contains("level=ERROR") && line.contains("cause="))?;
    opencode_cause(line)
}

/// La causa de una línea de error del log de OpenCode, traducida si se conoce.
fn opencode_cause(line: &str) -> Option<String> {
    let cause = line.split_once("cause=\"")?.1;
    let cause = cause.split_once("Upstream request failed: ").map_or(cause, |(_, rest)| rest);
    let cause = cause.split(['"', '\n']).next()?.trim();
    if cause.is_empty() {
        return None;
    }
    if cause.contains("OpenCode Go subscription is required") {
        return Some("Tu suscripción de OpenCode Go no está activa, así que sus modelos no responden. Elige un modelo gratis de Zen o renueva la suscripción.".into());
    }
    Some(format!("OpenCode: {cause}"))
}

/// Lo nuevo de un `tool_call` o `tool_call_update` sobre lo que ya se sabía.
fn merge(tool: &mut Tool, update: &Value) {
    if let Some(title) = update.get("title").and_then(Value::as_str) {
        tool.title = title.to_string();
    }
    if let Some(kind) = update.get("kind").and_then(Value::as_str) {
        tool.kind = kind.to_string();
    }
    if let Some(input) = update.get("rawInput").filter(|v| v.as_object().is_some_and(|o| !o.is_empty())) {
        tool.raw_input = input.clone();
    }
    if let Some(locations) = update.get("locations").and_then(Value::as_array).filter(|l| !l.is_empty()) {
        tool.locations = locations.clone();
    }
    if let Some(content) = update.get("content").and_then(Value::as_array).filter(|c| !c.is_empty()) {
        tool.content = content.clone();
    }
    if let Some(output) = update.get("rawOutput").filter(|v| !v.is_null()) {
        tool.raw_output = output.clone();
    }
}

/// Una herramienta ACP con el nombre y la entrada de su par en Claude Code, para que
/// el chat la dibuje igual (`Bash` con su comando, `Read` con su archivo, `Edit` con
/// su diff…). Lo que no tiene par va con su título.
fn claude_shape(tool: &Tool) -> (String, Value) {
    let input = &tool.raw_input;
    let text = |names: &[&str]| names.iter().find_map(|name| input.get(*name).and_then(Value::as_str)).map(str::to_string);
    let path = tool
        .locations
        .first()
        .and_then(|l| l.get("path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| text(&["path", "file_path", "filePath", "absolute_path"]));
    let diff = tool.content.iter().find(|c| c.get("type").and_then(Value::as_str) == Some("diff"));
    match tool.kind.as_str() {
        "execute" => {
            let command = text(&["command", "cmd"]).unwrap_or_else(|| tool.title.clone());
            ("Bash".into(), json!({ "command": command, "description": tool.title }))
        }
        "read" => ("Read".into(), json!({ "file_path": path.unwrap_or_else(|| tool.title.clone()) })),
        "edit" => match diff {
            Some(diff) => {
                let file = diff.get("path").and_then(Value::as_str).map(str::to_string).or(path).unwrap_or_default();
                let old = diff.get("oldText").and_then(Value::as_str).unwrap_or_default();
                let new = diff.get("newText").and_then(Value::as_str).unwrap_or_default();
                if old.is_empty() {
                    ("Write".into(), json!({ "file_path": file, "content": new }))
                } else {
                    ("Edit".into(), json!({ "file_path": file, "old_string": old, "new_string": new }))
                }
            }
            None => ("Edit".into(), json!({ "file_path": path.unwrap_or_else(|| tool.title.clone()) })),
        },
        "search" => {
            let pattern = text(&["pattern", "query", "regex"]).unwrap_or_else(|| tool.title.clone());
            ("Grep".into(), json!({ "pattern": pattern, "path": path }))
        }
        "fetch" => {
            let url = text(&["url", "query"]).unwrap_or_else(|| tool.title.clone());
            ("WebFetch".into(), json!({ "url": url }))
        }
        "web_search" => ("WebSearch".into(), json!({ "query": text(&["query"]).unwrap_or_else(|| tool.title.clone()) })),
        _ => {
            let name = if tool.title.is_empty() { "Herramienta".to_string() } else { tool.title.clone() };
            (name, input.clone())
        }
    }
}

/// El resultado de una herramienta como texto: lo que mostró el agente, o lo que
/// devolvió crudo.
fn result_text(tool: &Tool) -> String {
    let mut parts = Vec::new();
    for item in &tool.content {
        match item.get("type").and_then(Value::as_str) {
            Some("content") => {
                if let Some(text) = item.pointer("/content/text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            Some("diff") => {
                let path = item.get("path").and_then(Value::as_str).unwrap_or_default();
                parts.push(format!("Editado {path}"));
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        if let Some(output) = tool.raw_output.get("output").and_then(Value::as_str) {
            parts.push(output.to_string());
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events_named<'a>(events: &'a [(&'static str, Value)], name: &str) -> Vec<&'a Value> {
        events.iter().filter(|(event, _)| *event == name).map(|(_, data)| data).collect()
    }

    #[test]
    fn los_trozos_abren_mensaje_y_bloques_en_orden() {
        let mut t = Translator::new("c", "OpenCode");
        let first = t.update(&json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "pienso" } }));
        let ops = first[0].1.as_array().unwrap();
        assert_eq!(ops[0]["op"], "message");
        assert_eq!(ops[1]["kind"], "thinking");
        let second = t.update(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "hola" } }));
        let ops = second[0].1.as_array().unwrap();
        // Cambia el tipo: se cierra el razonamiento y se abre el texto.
        assert_eq!(ops[0]["op"], "stop");
        assert_eq!(ops[1]["kind"], "text");
        assert_eq!(ops[2]["text"], "hola");
        let end = t.end_turn(&json!({ "stopReason": "end_turn", "usage": { "outputTokens": 7 } }), None);
        let assistant = events_named(&end, "assistant");
        assert_eq!(assistant[0]["content"][0]["thinking"], "pienso");
        assert_eq!(assistant[0]["content"][1]["text"], "hola");
        let result = events_named(&end, "result");
        assert_eq!(result[0]["subtype"], "success");
        assert_eq!(result[0]["usage"]["output_tokens"], 7);
    }

    #[test]
    fn una_herramienta_se_muestra_como_su_par_de_claude() {
        let mut t = Translator::new("c", "OpenCode");
        let start = t.update(&json!({ "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "read", "kind": "read", "status": "pending" }));
        assert_eq!(events_named(&start, "assistant")[0]["content"][0]["name"], "Read");
        let done = t.update(&json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
            "locations": [{ "path": "C:\\repo\\nota.txt" }],
            "content": [{ "type": "content", "content": { "type": "text", "text": "1: hola" } }]
        }));
        let tool_use = &events_named(&done, "assistant")[0]["content"][0];
        assert_eq!(tool_use["input"]["file_path"], "C:\\repo\\nota.txt");
        let result = &events_named(&done, "user")[0]["content"][0];
        assert_eq!(result["tool_use_id"], "t1");
        assert_eq!(result["content"], "1: hola");
        assert_eq!(result["is_error"], false);
    }

    #[test]
    fn un_diff_es_una_edicion_y_sin_texto_viejo_una_escritura() {
        let tool = Tool {
            kind: "edit".into(),
            content: vec![json!({ "type": "diff", "path": "a.rs", "oldText": "x", "newText": "y" })],
            ..Tool::default()
        };
        assert_eq!(claude_shape(&tool).0, "Edit");
        let new_file = Tool { content: vec![json!({ "type": "diff", "path": "b.rs", "oldText": null, "newText": "z" })], ..tool };
        let (name, input) = claude_shape(&new_file);
        assert_eq!(name, "Write");
        assert_eq!(input["content"], "z");
    }

    #[test]
    fn el_permiso_lleva_su_herramienta_y_si_ofrece_siempre() {
        let mut t = Translator::new("c", "Gemini");
        let (request, events) = t.permission(&json!({
            "toolCall": { "toolCallId": "t9", "title": "rm -rf build", "kind": "execute", "rawInput": { "command": "rm -rf build" } },
            "options": [{ "optionId": "a", "kind": "allow_once" }, { "optionId": "r", "kind": "reject_once" }]
        }));
        assert_eq!(request, "acp-perm-1");
        let permission = events_named(&events, "permission")[0];
        assert_eq!(permission["toolName"], "Bash");
        assert_eq!(permission["input"]["command"], "rm -rf build");
        assert_eq!(permission["suppressAlwaysAllowRule"], true);
    }

    #[test]
    fn detener_y_errores_llegan_como_en_el_sidecar() {
        let mut t = Translator::new("c", "Grok");
        let cancelled = t.end_turn(&json!({ "stopReason": "cancelled" }), None);
        assert_eq!(events_named(&cancelled, "result")[0]["subtype"], "error_during_execution");
        let failed = t.end_turn(&Value::Null, Some("Sin sesión".into()));
        assert_eq!(events_named(&failed, "result")[0]["errors"][0], "Sin sesión");
    }

    #[test]
    fn las_opciones_del_agente_llegan_al_chat_y_se_ponen_al_dia() {
        let mut t = Translator::new("c", "OpenCode");
        let opened = t.opened(&json!({
            "sessionId": "s",
            "configOptions": [{ "id": "model", "category": "model", "type": "select", "currentValue": "a", "options": [{ "value": "a", "name": "A" }, { "value": "b", "name": "B" }] }],
            "modes": { "currentModeId": "build", "availableModes": [{ "id": "build", "name": "Build" }, { "id": "plan", "name": "Plan" }] }
        }));
        assert_eq!(opened[0].0, "acp_config");
        assert_eq!(opened[0].1["configOptions"][0]["currentValue"], "a");
        // Sin respuesta con la lista nueva: se toma lo pedido.
        let changed = t.configured(&json!({ "configId": "model", "value": "b" }), &json!({}));
        assert_eq!(changed[0].1["configOptions"][0]["currentValue"], "b");
        let moded = t.update(&json!({ "sessionUpdate": "current_mode_update", "currentModeId": "plan" }));
        assert_eq!(moded[0].1["modes"]["currentModeId"], "plan");
        // Los comandos llegan aunque se esté cargando una sesión.
        t.muted = true;
        let commands = t.update(&json!({ "sessionUpdate": "available_commands_update", "availableCommands": [{ "name": "review" }] }));
        assert_eq!(commands[0].0, "acp_commands");
    }

    #[test]
    fn los_modelos_se_ven_con_nombres_legibles() {
        assert_eq!(pretty_model("claude-opus-5-5"), "Claude Opus 5.5");
        assert_eq!(pretty_model("gemini-4-argon-contributor"), "Gemini 4 Argon Contributor");
        assert_eq!(pretty_model("gpt-5.5-codex"), "GPT 5.5 Codex");
        assert_eq!(pretty_model("MiniMax-M2.5-highspeed"), "MiniMax M2.5 Highspeed");
        // OpenCode: el proveedor va como grupo; los gratuitos de Zen, rotulados.
        let (label, detail, group) = model_entry("opencode-go/gemini-4-argon-contributor", "opencode-go/gemini-4-argon-contributor");
        assert_eq!((label.as_str(), detail, group.as_deref()), ("Gemini 4 Argon Contributor", None, Some("OpenCode Go")));
        let (label, detail, group) = model_entry("opencode/space-bunny-free", "opencode/space-bunny-free");
        assert_eq!((label.as_str(), detail.as_deref(), group.as_deref()), ("Space Bunny", Some("Gratis"), Some("OpenCode Zen · gratis")));
        // Como los manda OpenCode: el nombre legible, con el proveedor delante.
        let (label, detail, group) = model_entry("opencode-go/cat-jam-free", "opencode-go/Cat Jam Free");
        assert_eq!((label.as_str(), detail.as_deref(), group.as_deref()), ("Cat Jam", Some("Gratis"), Some("OpenCode Go")));
        assert_eq!(model_entry("minimax/MiniMax-M2.5", "minimax/MiniMax-M2.5").0, "MiniMax M2.5");
        let mut groups = vec!["NVIDIA", "OpenCode Zen · gratis", "MiniMax", "OpenCode Go"];
        groups.sort_by_key(|g| group_rank(g));
        assert_eq!(groups, vec!["OpenCode Go", "OpenCode Zen · gratis", "MiniMax", "NVIDIA"]);
        // Cursor: los parámetros de la variante van debajo.
        let (label, detail, _) = model_entry("claude-opus-5[thinking=true,context=300k,effort=high,fast=false]", "claude-opus-5");
        assert_eq!(label, "Claude Opus 5");
        assert_eq!(detail.as_deref(), Some("razonamiento · 300k de contexto · esfuerzo alto"));
        // Grok ya los manda bonitos.
        assert_eq!(model_entry("grok-4.7-build-fast", "Grok 4.7 Fast").0, "Grok 4.7 Fast");
    }

    #[test]
    fn los_niveles_y_modos_se_ven_como_los_de_claude() {
        let mut levels = vec!["xhigh", "low", "high", "medium"];
        levels.sort_by_key(|id| effort_rank(id));
        assert_eq!(levels, vec!["low", "medium", "high", "xhigh"]);
        assert_eq!(effort_name("xhigh"), "Muy alto");
        assert_eq!(mode_text("plan", "Plan", "Read-only").1, "Solo lectura: explora y propone un plan antes de tocar nada");
        assert_eq!(mode_text("custom", "Custom", "x"), ("Custom".into(), "x".into()));
    }

    #[test]
    fn al_cargar_una_sesion_no_se_repite_el_historial() {
        let mut t = Translator::new("c", "OpenCode");
        t.muted = true;
        assert!(t.update(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "viejo" } })).is_empty());
    }

    #[test]
    fn el_log_de_opencode_dice_el_motivo_real() {
        let line = r#"timestamp=2026-10-14T15:06:11.414Z level=ERROR run=a85 message="Failed to drain Session" cause="AI.Error: Upstream request failed: An active OpenCode Go subscription is required to use Go models."#;
        assert!(opencode_cause(line).unwrap().starts_with("Tu suscripción de OpenCode Go no está activa"));
        let other = r#"level=ERROR message="x" cause="Rate limit exceeded" more=1"#;
        assert_eq!(opencode_cause(other).unwrap(), "OpenCode: Rate limit exceeded");
        assert_eq!(opencode_cause("level=ERROR message=\"x\""), None);
    }
}
