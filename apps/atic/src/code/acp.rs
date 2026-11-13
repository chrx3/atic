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
//!
//! Los procesos de las conversaciones no son hijos de la pill: se le piden al proceso del
//! agente (`sidecar.rs`, el daemon que ya corre a Claude) con `procSpawn`, y se les habla con
//! `procWrite`; lo que escriben vuelve como `procOut` y su salida como `procExit` por el mismo
//! canal de eventos. Así sobreviven a cerrar o reiniciar la pill, y al volver `adopt` rearma la
//! conversación con lo que el daemon guardó de ella. Sin daemon (sin Node, o uno viejo que no
//! sabe `procSpawn`) van en un tubo propio, como antes. La sesión de prueba va siempre así.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, OnceLock};

use crate::i18n::{t as tr, tf};
use std::time::{Duration, Instant};

use atic_core::MutexExt;
use futures::channel::{mpsc, oneshot};
use serde_json::{json, Value};

use super::sidecar::{Incoming, Reply, Sidecar};

mod codex;
mod diag;

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
    /// El proceso del agente: si está, los agentes corren en él y sobreviven a la pill.
    daemon: Option<Arc<Sidecar>>,
}

/// Lo que se rearma de una conversación que siguió en el daemon sin la pill.
pub struct Adopted {
    /// Los eventos para el chat (su historial reciente), en orden.
    pub events: Vec<(&'static str, Value)>,
    /// La sesión del agente, si se vio abrir.
    pub session_id: Option<String>,
    /// El `result` del último turno terminado, si hubo.
    pub last_result: Option<Value>,
}

impl Hub {
    /// Con los agentes en tubos propios (mueren con la pill).
    pub fn new(events: mpsc::UnboundedSender<Incoming>) -> Arc<Self> {
        Arc::new(Self { sessions: Mutex::default(), events, daemon: None })
    }

    /// Con los agentes en el proceso del agente (`procSpawn`), con el tubo propio de respaldo.
    pub fn with_daemon(sidecar: &Arc<Sidecar>) -> Arc<Self> {
        Arc::new(Self { sessions: Mutex::default(), events: sidecar.events(), daemon: Some(sidecar.clone()) })
    }

    fn session(&self, key: &str) -> Option<Arc<Session>> {
        self.sessions.lock_or_recover().get(key).cloned()
    }

    /// La conversación corre en el daemon (no en un tubo propio).
    pub fn remote(&self, key: &str) -> bool {
        self.session(key).is_some_and(|s| matches!(s.pipe.get(), Some(Pipe::Daemon { .. })))
    }

    /// La conversación corre en un tubo propio y su proceso sigue vivo.
    pub fn local_alive(&self, key: &str) -> bool {
        self.session(key).is_some_and(|s| matches!(s.pipe.get(), Some(Pipe::Local { .. })) && s.alive())
    }

    /// Un evento del daemon sobre sus agentes (`procOut`, `procExit`): va a su sesión.
    /// Devuelve si era uno de ellos (los demás siguen a la vista).
    pub fn route(&self, event: &str, key: Option<&str>, data: &Value) -> bool {
        if !is_proc_event(event) {
            return false;
        }
        let Some(session) = key.and_then(|key| self.session(key)).filter(|s| !matches!(s.pipe.get(), Some(Pipe::Local { .. }))) else {
            return true;
        };
        let line = data.get("line").and_then(Value::as_str);
        match (event, line) {
            ("procOut", Some(line)) => session.output(line),
            ("procErr", Some(line)) => session.note(line),
            ("procExit", _) => {
                session.note_exit(data);
                self.drop_remote(&session, true);
            }
            _ => {}
        }
        true
    }

    /// Se cortó la conexión con el daemon: sus agentes ya no responden.
    pub fn disconnected(&self) {
        let remote: Vec<Arc<Session>> = self.sessions.lock_or_recover().values().filter(|s| matches!(s.pipe.get(), Some(Pipe::Daemon { .. }))).cloned().collect();
        for session in remote {
            // Se perdió la conexión, no el agente: sigue en el daemon y se retoma al volver.
            self.drop_remote(&session, false);
        }
    }

    /// Un agente del daemon terminó (`gone`) o dejó de oírse: sale de la lista y la
    /// conversación se cierra.
    fn drop_remote(&self, session: &Arc<Session>, gone: bool) {
        if let Some(Pipe::Daemon { alive, .. }) = session.pipe.get() {
            alive.store(false, Ordering::SeqCst);
        }
        {
            let mut sessions = self.sessions.lock_or_recover();
            if sessions.get(&session.key).is_some_and(|s| Arc::ptr_eq(s, session)) {
                sessions.remove(&session.key);
            }
        }
        session.exited(gone);
    }

    /// Rearma una conversación que el daemon siguió atendiendo (la pill se cerró o se
    /// reinició): una sesión que habla con ese proceso, con lo que se vio de él (`lines`).
    /// `None` si no hay daemon, el agente no se conoce o ya está en la lista.
    pub fn adopt(&self, key: &str, cli: &str, busy: bool, lines: &[Value]) -> Option<Adopted> {
        let sidecar = self.daemon.clone()?;
        let agent = agent(cli)?;
        if self.session(key).is_some_and(|s| s.alive()) {
            return None;
        }
        let rebuilt = rebuild(agent, key, lines);
        let session = Session::new(agent, key, self.events.clone(), false);
        // Sus ids no chocan con los de la pill anterior, que pueden seguir en vuelo.
        session.next.store(id_base(), Ordering::Relaxed);
        let _ = session.pipe.set(Pipe::Daemon { sidecar, alive: AtomicBool::new(true) });
        {
            let mut translate = session.translate.lock_or_recover();
            *translate = rebuilt.translate;
            // Lo que llegue del turno en curso se dibuja; sin turno, no queda uno abierto.
            match (busy, translate.started.is_some()) {
                (true, false) => translate.begin_turn(),
                (false, true) => translate.started = None,
                _ => {}
            }
        }
        *session.codex.lock_or_recover() = rebuilt.codex;
        *session.asked.lock_or_recover() = rebuilt.asked;
        *session.session_id.lock_or_recover() = rebuilt.session_id.clone();
        self.sessions.lock_or_recover().insert(key.to_string(), session);
        let last_result = rebuilt.events.iter().rev().find(|(event, _)| *event == "result").map(|(_, data)| data.clone());
        Some(Adopted { events: rebuilt.events, session_id: rebuilt.session_id, last_result })
    }

    /// Lanza el proceso de una sesión: en el daemon si se puede, si no en un tubo propio.
    fn launch(&self, session: &Arc<Session>, cwd: &PathBuf) -> Result<(), String> {
        if let Some(sidecar) = &self.daemon {
            match session.launch_daemon(sidecar, cwd) {
                Ok(()) => return Ok(()),
                Err(error) => tracing::info!(%error, cli = session.agent.cli, "acp: el proceso del agente no lo lanzó; va en un tubo propio"),
            }
        }
        session.launch_local(cwd)
    }

    /// Una petición con la forma de las del sidecar. La respuesta llega por el
    /// `oneshot`, como allá.
    pub fn call(self: &Arc<Self>, method: &str, params: Value) -> oneshot::Receiver<Reply> {
        let (reply, receiver) = oneshot::channel();
        let key = params.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
        let session = self.sessions.lock_or_recover().get(&key).cloned();
        match method {
            "start" => {
                if session.as_ref().is_some_and(|s| s.alive()) {
                    let id = session.and_then(|s| s.session_id.lock_or_recover().clone());
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
                        // Los servidores MCP que se le suman a esta sesión (el editor de grabaciones).
                        let mcp = params.get("mcpServers").filter(|m| m.is_array()).cloned().unwrap_or_else(|| json!([]));
                        std::thread::spawn(move || {
                            let result = hub.launch(&session, &cwd).and_then(|()| session.open(&cwd, resume.as_deref(), want.as_ref(), &mcp));
                            if result.is_err() {
                                hub.sessions.lock_or_recover().remove(&session.key);
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
                    None => Err(tr("pill.code.acp.sessionNotOpen").into()),
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
                    None => Err(tr("pill.code.acp.sessionNotOpen").into()),
                };
                let _ = reply.send(result.map(|_| json!({ "ok": true })));
            }
            "close" => {
                if let Some(session) = self.sessions.lock_or_recover().remove(&key) {
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

    /// Deja la sesión en la lista, todavía sin proceso: se lanza en el hilo de `start`, y
    /// un `send` inmediato la encuentra y espera a que abra.
    fn spawn(self: &Arc<Self>, key: &str, params: &Value) -> Result<(Arc<Session>, PathBuf), String> {
        let cli = params.get("agent").and_then(Value::as_str).unwrap_or_default();
        let agent = agent(cli).ok_or_else(|| tf("pill.code.acp.unknownAgent", &[("cli", &cli)]))?;
        let cwd = PathBuf::from(params.get("cwd").and_then(Value::as_str).unwrap_or("."));
        if params.get("createCwd").and_then(Value::as_bool).unwrap_or(false) {
            let _ = std::fs::create_dir_all(&cwd);
        }
        let session = Session::new(agent, key, self.events.clone(), false);
        self.sessions.lock_or_recover().insert(key.to_string(), session.clone());
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
                let agent = agent(&cli).ok_or_else(|| tf("pill.code.acp.unknownAgent", &[("cli", &cli)]))?;
                let session = Session::new(agent, "acp-probe", events, true);
                session.launch_local(&cwd)?;
                let opened = session.open(&cwd, None, None, &json!([]));
                // Codex no avisa comandos: no hay que esperarlos.
                if opened.is_ok() && !agent.codex {
                    // Los comandos llegan como aviso, un poco después de abrir.
                    let deadline = Instant::now() + Duration::from_millis(1500);
                    while session.translate.lock_or_recover().commands.is_null() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                session.kill();
                opened?;
                let translate = session.translate.lock_or_recover();
                Ok(json!({ "configOptions": translate.config, "modes": translate.modes, "commands": translate.commands }))
            })();
            let _ = reply.send(result);
        });
        receiver
    }

}

impl Drop for Session {
    /// Al cerrarse la ventana (o la conversación), un proceso en tubo propio se va con ella.
    /// Uno del daemon sigue: la próxima ventana lo retoma (se cierra con `kill`).
    fn drop(&mut self) {
        if let Some(Pipe::Local { child, .. }) = self.pipe.get_mut() {
            if let Ok(child) = child.get_mut() {
                let _ = child.kill();
            }
        }
        // Al soltarse, `tree` se lleva al resto del árbol.
    }
}

/// Los eventos del daemon que son de sus agentes. `procErr` (una línea de su stderr) es
/// para un daemon que la mande: el de hoy descarta el stderr de los agentes.
fn is_proc_event(event: &str) -> bool {
    matches!(event, "procOut" | "procErr" | "procExit")
}

/// Deja en el log, una sola vez por tipo, un mensaje del agente que Atic no conoce: si
/// una CLI renombra o agrega algo, se ve en vez de perderse en silencio.
fn warn_unknown(agent: &str, what: &str, name: &str) {
    static SEEN: OnceLock<Mutex<diag::Seen>> = OnceLock::new();
    let first = SEEN.get_or_init(Mutex::default).lock_or_recover().first(&format!("{agent} {what} {name}"));
    if let Some(unknown) = first {
        tracing::warn!("acp: mensaje desconocido, se ignora: {unknown}");
    }
}

/// Desde dónde numera sus peticiones una sesión del daemon: alto y distinto en cada
/// ejecución, para no confundir sus respuestas con las de la pill anterior (cabe en un
/// número de JavaScript).
fn id_base() -> u64 {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    (millis % 1_000_000_000 + 1) * 1000
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

/// Por dónde se le habla al proceso del agente.
enum Pipe {
    /// Un hijo de la pill, por sus tubos (sin daemon, o la sesión de prueba).
    Local {
        child: Mutex<Child>,
        #[cfg(windows)]
        tree: Option<Tree>,
        stdin: Mutex<ChildStdin>,
    },
    /// Un proceso del daemon (`procSpawn`): sobrevive a la pill.
    Daemon { sidecar: Arc<Sidecar>, alive: AtomicBool },
}

struct Session {
    agent: &'static AcpAgent,
    key: String,
    /// Se fija al lanzar el proceso (`launch_local` o `launch_daemon`).
    pipe: OnceLock<Pipe>,
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
    /// Lo último que el proceso escribió fuera del protocolo, para explicar un fallo.
    diag: Arc<Mutex<diag::Tail>>,
    /// Desde dónde cuenta `diag` para el turno en curso (lo anterior es de otro turno).
    diag_mark: AtomicU64,
    /// La versión de ACP del agente si es más nueva que la de Atic; 0 si no.
    newer_protocol: AtomicU64,
    /// Con qué código salió un proceso del daemon, si lo dijo.
    exit_code: Mutex<Option<i64>>,
    /// Se cerró a propósito (`kill`): su salida no es un fallo que explicar.
    killed: AtomicBool,
    /// El proceso cerró su salida: terminó, aunque el sistema todavía no diga con qué código.
    gone: AtomicBool,
    events: mpsc::UnboundedSender<Incoming>,
}

impl Session {
    fn new(agent: &'static AcpAgent, key: &str, events: mpsc::UnboundedSender<Incoming>, quiet: bool) -> Arc<Self> {
        Arc::new(Self {
            agent,
            key: key.to_string(),
            pipe: OnceLock::new(),
            next: AtomicU64::new(0),
            pending: Arc::default(),
            session_id: Mutex::new(None),
            failed: std::sync::atomic::AtomicBool::new(false),
            quiet,
            asked: Arc::default(),
            translate: Arc::new(Mutex::new(Translator::new(key, agent.name))),
            codex: Mutex::default(),
            diag: Arc::default(),
            diag_mark: AtomicU64::new(0),
            newer_protocol: AtomicU64::new(0),
            exit_code: Mutex::new(None),
            killed: AtomicBool::new(false),
            gone: AtomicBool::new(false),
            events,
        })
    }

    /// El programa del agente y sus argumentos. `opencode` y `gemini` son shims de npm
    /// (`.cmd`): `launcher` los corre con cmd.
    fn command_line(&self) -> Result<(PathBuf, Vec<String>), String> {
        let (program, mut args) = atic_agents::exe::launcher(self.agent.cli)
            .ok_or_else(|| tf("pill.code.acp.notFound", &[("name", &self.agent.name)]))?;
        args.extend(self.agent.args.iter().map(|arg| arg.to_string()));
        Ok((program, args))
    }

    /// Le pide el proceso al daemon. Falla si no hay daemon o es uno viejo sin `procSpawn`.
    fn launch_daemon(&self, sidecar: &Arc<Sidecar>, cwd: &PathBuf) -> Result<(), String> {
        let (program, args) = self.command_line()?;
        let params = json!({
            "key": self.key,
            "program": program,
            "args": args,
            "cwd": cwd,
            "meta": { "cli": self.agent.cli, "cwd": cwd },
        });
        futures::executor::block_on(sidecar.call("procSpawn", params)).unwrap_or_else(|_| Err(tr("pill.code.acp.stopped").into()))?;
        self.next.store(id_base(), Ordering::Relaxed);
        let _ = self.pipe.set(Pipe::Daemon { sidecar: sidecar.clone(), alive: AtomicBool::new(true) });
        Ok(())
    }

    /// Lanza el proceso como hijo de la pill, con sus tubos.
    fn launch_local(self: &Arc<Self>, cwd: &PathBuf) -> Result<(), String> {
        let agent = self.agent;
        let (program, args) = self.command_line()?;
        let mut command = Command::new(program);
        command
            .args(args)
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
        let mut child = command.spawn().map_err(|e| tf("pill.code.acp.startFailed", &[("name", &agent.name), ("error", &e)]))?;
        #[cfg(windows)]
        let tree = Tree::new(&child);
        let stdin = child.stdin.take().ok_or_else(|| tr("pill.code.acp.noStdin").to_string())?;
        let stdout = child.stdout.take().ok_or_else(|| tr("pill.code.acp.noStdout").to_string())?;
        let stderr = child.stderr.take().ok_or_else(|| tr("pill.code.acp.noStderr").to_string())?;
        let pipe = Pipe::Local {
            child: Mutex::new(child),
            #[cfg(windows)]
            tree,
            stdin: Mutex::new(stdin),
        };
        if let Err(Pipe::Local { child, .. }) = self.pipe.set(pipe) {
            let _ = child.lock_or_recover().kill();
            return Err(tr("pill.code.acp.noStdin").into());
        }
        let reader = Arc::downgrade(self);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(session) = reader.upgrade() else {
                    return;
                };
                session.output(&line);
            }
            if let Some(session) = reader.upgrade() {
                session.exited(true);
            }
        });
        let tail = self.diag.clone();
        std::thread::spawn(move || {
            // Por bytes: una línea que no es UTF-8 no corta la lectura (con el tubo lleno,
            // el agente se quedaría esperando para escribir).
            let mut reader = BufReader::new(stderr);
            let mut raw = Vec::new();
            while reader.read_until(b'\n', &mut raw).is_ok_and(|read| read > 0) {
                let stored = tail.lock_or_recover().push(&String::from_utf8_lossy(&raw));
                if let Some(line) = stored {
                    tracing::debug!(cli = agent.cli, "acp: {line}");
                }
                raw.clear();
            }
            tail.lock_or_recover().close();
        });
        Ok(())
    }

    /// El proceso sigue (o todavía se está lanzando).
    fn alive(&self) -> bool {
        match self.pipe.get() {
            None => !self.failed.load(Ordering::Relaxed),
            Some(Pipe::Local { child, .. }) => child.lock_or_recover().try_wait().is_ok_and(|status| status.is_none()),
            Some(Pipe::Daemon { alive, .. }) => alive.load(Ordering::SeqCst),
        }
    }

    fn kill(&self) {
        self.killed.store(true, Ordering::Relaxed);
        match self.pipe.get() {
            None => {}
            Some(Pipe::Local { child, .. }) => {
                let _ = child.lock_or_recover().kill();
                #[cfg(windows)]
                if let Some(Pipe::Local { tree: Some(tree), .. }) = self.pipe.get() {
                    tree.kill();
                }
            }
            Some(Pipe::Daemon { sidecar, alive }) => {
                if alive.swap(false, Ordering::SeqCst) {
                    let _ = sidecar.call("procKill", json!({ "key": self.key }));
                }
            }
        }
    }

    fn write(&self, message: &Value) -> Result<(), String> {
        match self.pipe.get() {
            Some(Pipe::Local { stdin, .. }) => {
                let mut stdin = stdin.lock_or_recover();
                writeln!(stdin, "{message}").and_then(|_| stdin.flush()).map_err(|e| tf("pill.code.acp.noReply", &[("error", &e)]))
            }
            // Las líneas salen en orden: el hilo de la conexión las escribe una tras otra.
            Some(Pipe::Daemon { sidecar, alive }) if alive.load(Ordering::SeqCst) => {
                let _ = sidecar.call("procWrite", json!({ "key": self.key, "line": message.to_string() }));
                Ok(())
            }
            _ => Err(tf("pill.code.acp.noReply", &[("error", &tr("pill.code.acp.stopped"))])),
        }
    }

    /// Una petición nuestra; la respuesta llega por el canal que devuelve.
    fn request(&self, method: &str, params: Value) -> Result<std_mpsc::Receiver<Result<Value, Value>>, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (waiter, receiver) = std_mpsc::channel();
        self.pending.lock_or_recover().insert(id, waiter);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        Ok(receiver)
    }

    /// Una petición que se espera aquí mismo (el arranque).
    fn request_wait(&self, method: &str, params: Value) -> Result<Value, String> {
        let receiver = self.request(method, params).map_err(|error| self.explain(error))?;
        match receiver.recv_timeout(START_TIMEOUT) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(self.failure_text(&error)),
            Err(_) => Err(self.explain(tf("pill.code.acp.timeout", &[("name", &self.agent.name)]))),
        }
    }

    /// Un error de JSON-RPC para el aviso; si es de sesión, cómo iniciarla.
    fn error_text(&self, error: &Value) -> String {
        self.error_parts(error).0
    }

    /// Un error que corta el arranque o un turno: el de `error_text` con lo último que
    /// escribió el proceso debajo. Si el log de OpenCode ya dio el motivo real, va solo:
    /// su stderr repetiría el mismo error sin traducir.
    fn failure_text(&self, error: &Value) -> String {
        let (text, from_log) = self.error_parts(error);
        if from_log {
            text
        } else {
            self.explain(text)
        }
    }

    /// El texto de un error de JSON-RPC, y si salió del log de OpenCode.
    fn error_parts(&self, error: &Value) -> (String, bool) {
        let message = error.get("message").and_then(Value::as_str).unwrap_or(tr("pill.code.acp.agentError"));
        // Algunas CLI contestan un error genérico y dicen que falta la sesión solo en su stderr.
        let auth = error.get("code").and_then(Value::as_i64) == Some(-32000)
            || message.to_lowercase().contains("auth")
            || diag::mentions_login(message)
            || self.recent_output().iter().any(|line| diag::mentions_login(line));
        // OpenCode responde «Authentication required» también cuando la cuenta está bien
        // pero el modelo pide una suscripción vencida: el motivo real queda en su log.
        if auth && self.agent.cli == "opencode" {
            if let Some(reason) = opencode_reason() {
                return (reason, true);
            }
        }
        if auth {
            (tf("pill.code.acp.signIn", &[("message", &message), ("login", &self.agent.login)]), false)
        } else {
            (message.to_string(), false)
        }
    }

    /// Una línea del proceso: un mensaje del protocolo o, si no es JSON, algo que la CLI
    /// escribió suelto (suele ser el error con que se despide).
    fn output(&self, line: &str) {
        match serde_json::from_str::<Value>(line) {
            Ok(message) => self.incoming(message),
            Err(_) => self.note(line),
        }
    }

    /// Guarda una línea del proceso que no es del protocolo.
    fn note(&self, line: &str) {
        self.diag.lock_or_recover().push(line);
    }

    /// Lo que un `procExit` del daemon dice de la salida: el código y, si lo manda, el
    /// final de su stderr.
    fn note_exit(&self, data: &Value) {
        *self.exit_code.lock_or_recover() = data.get("code").and_then(Value::as_i64);
        for line in data.get("stderr").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            self.note(line);
        }
    }

    /// Si el proceso ya salió y con qué código (si se sabe).
    fn exit_state(&self) -> (bool, Option<i64>) {
        match self.pipe.get() {
            None => (false, None),
            Some(Pipe::Local { child, .. }) => match child.lock_or_recover().try_wait() {
                Ok(Some(status)) => (true, status.code().map(i64::from)),
                _ => (self.gone.load(Ordering::Relaxed), None),
            },
            Some(Pipe::Daemon { alive, .. }) => (!alive.load(Ordering::SeqCst), *self.exit_code.lock_or_recover()),
        }
    }

    /// Lo que el proceso escribió fuera del protocolo desde que empezó el turno (o desde
    /// el arranque); si ya salió, lo último que dijo.
    fn recent_output(&self) -> Vec<String> {
        let (gone, _) = self.exit_state();
        let mark = if gone { 0 } else { self.diag_mark.load(Ordering::Relaxed) };
        self.diag.lock_or_recover().since(mark, diag::SHOWN_LINES)
    }

    /// El error con lo que ayuda a entenderlo: el código de salida, la versión de ACP si
    /// es más nueva que la de Atic y las últimas líneas del proceso. Con un proceso propio
    /// que ya salió espera un momento, así que no se llama desde el hilo de la interfaz
    /// (con uno del daemon no espera).
    fn explain(&self, error: String) -> String {
        if self.killed.load(Ordering::Relaxed) {
            return error;
        }
        // El stderr se lee en otro hilo: al salir el proceso, sus últimas líneas (las que
        // dicen por qué) pueden venir todavía en camino.
        if self.exit_state().0 && matches!(self.pipe.get(), Some(Pipe::Local { .. })) {
            let deadline = Instant::now() + Duration::from_millis(300);
            while !self.diag.lock_or_recover().closed() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let (_, code) = self.exit_state();
        let newer = Some(self.newer_protocol.load(Ordering::Relaxed)).filter(|version| *version > 0);
        diag::with_details(error, code, newer, &self.recent_output())
    }

    /// `initialize` y la sesión: nueva, o la anterior con `session/load` (su historial
    /// ya está en el chat, así que lo que repite el agente no se muestra).
    fn open(&self, cwd: &PathBuf, resume: Option<&str>, want: Option<&Value>, mcp: &Value) -> Reply {
        if self.agent.codex {
            return self.codex_open(cwd, resume, want, mcp);
        }
        let capabilities = json!({ "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false });
        let init = self.request_wait("initialize", json!({ "protocolVersion": diag::PROTOCOL_VERSION, "clientCapabilities": capabilities }))?;
        let version = diag::protocol_version(&init).map_err(|problem| self.explain(tf("pill.code.acp.noReply", &[("error", &problem)])))?;
        // Un agente con un ACP más nuevo puede mandar cosas que Atic no entiende: se sigue,
        // y si la sesión falla después el aviso lo dice.
        if let Some(newer) = diag::newer_protocol(version) {
            tracing::warn!(cli = self.agent.cli, agent = newer, atic = diag::PROTOCOL_VERSION, "acp: el agente habla una versión más nueva del protocolo");
            self.newer_protocol.store(newer, Ordering::Relaxed);
        }
        let can_load = init.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool).unwrap_or(false);
        let cwd = cwd.to_string_lossy().to_string();
        let (id, opened) = match resume.filter(|_| can_load) {
            Some(id) => {
                self.translate.lock_or_recover().muted = true;
                let loaded = self.request_wait("session/load", json!({ "sessionId": id, "cwd": cwd, "mcpServers": mcp }));
                self.translate.lock_or_recover().muted = false;
                match loaded {
                    Ok(result) => (id.to_string(), result),
                    // Si ya no existe, una nueva.
                    Err(_) => self.new_session(&cwd, mcp)?,
                }
            }
            None => self.new_session(&cwd, mcp)?,
        };
        self.translate.lock_or_recover().opened(&opened);
        // Lo elegido antes del primer mensaje (modelo, esfuerzo, modo): se aplica ahora,
        // antes de que la sesión quede abierta y salga el mensaje que espera.
        if let Some(want) = want {
            self.apply_wanted(&id, want);
        }
        let event = self.translate.lock_or_recover().config_event();
        self.send_all(vec![event]);
        *self.session_id.lock_or_recover() = Some(id.clone());
        Ok(json!({ "ok": true, "reused": true, "sessionId": id }))
    }

    /// Aplica a la sesión recién abierta lo que difiera de `want` (`{ config, mode }`).
    fn apply_wanted(&self, session_id: &str, want: &Value) {
        for (config, value) in want.get("config").and_then(Value::as_object).into_iter().flatten() {
            if self.translate.lock_or_recover().current(config).as_ref() == Some(value) {
                continue;
            }
            let request = json!({ "sessionId": session_id, "configId": config, "value": value });
            if let Ok(result) = self.request_wait("session/set_config_option", request.clone()) {
                self.translate.lock_or_recover().configured(&request, &result);
            }
        }
        if let Some(mode) = want.get("mode").filter(|m| m.is_string()) {
            if self.translate.lock_or_recover().modes.get("currentModeId") != Some(mode) {
                let request = json!({ "sessionId": session_id, "modeId": mode });
                if let Ok(result) = self.request_wait("session/set_mode", request.clone()) {
                    self.translate.lock_or_recover().configured(&request, &result);
                }
            }
        }
    }

    fn new_session(&self, cwd: &str, mcp: &Value) -> Result<(String, Value), String> {
        let created = self.request_wait("session/new", json!({ "cwd": cwd, "mcpServers": mcp }))?;
        let id = created.get("sessionId").and_then(Value::as_str).map(str::to_string).ok_or_else(|| tr("pill.code.acp.noSession").to_string())?;
        Ok((id, created))
    }

    /// Cambia una opción (`setConfig`: el modelo, el razonamiento…) o el modo
    /// (`setMode`). La respuesta llega aparte y el menú se pone al día con ella.
    fn configure(self: &Arc<Self>, method: &str, params: &Value) -> Result<(), String> {
        if self.agent.codex {
            self.codex_configure(method, params);
            return Ok(());
        }
        let session_id = self.session_id.lock_or_recover().clone().ok_or_else(|| tr("pill.code.acp.opening").to_string())?;
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
                Ok(Ok(result)) => session.translate.lock_or_recover().configured(&request, &result),
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
            if let Some(id) = self.session_id.lock_or_recover().clone() {
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
        let mark = self.diag.lock_or_recover().mark();
        self.diag_mark.store(mark, Ordering::Relaxed);
        self.translate.lock_or_recover().begin_turn();
        if self.agent.codex {
            let images = params.get("images").and_then(Value::as_array).cloned().unwrap_or_default();
            self.codex_prompt(text, images);
            return Ok(());
        }
        let session = self.clone();
        std::thread::spawn(move || {
            let reply = match session.wait_open() {
                Some(id) => match session.request("session/prompt", json!({ "sessionId": id, "prompt": prompt })) {
                    Ok(receiver) => receiver.recv().unwrap_or_else(|_| Err(json!({ "message": tr("pill.code.acp.stopped") }))),
                    Err(error) => Err(json!({ "message": error })),
                },
                None => Err(json!({ "message": tf("pill.code.acp.openFailed", &[("name", &session.agent.name)]) })),
            };
            // El texto del error se arma antes de tomar el traductor: `failure_text` puede
            // esperar un momento las últimas líneas del proceso.
            let events = match reply {
                Ok(result) => session.translate.lock_or_recover().end_turn(&result, None),
                Err(error) => {
                    let text = session.failure_text(&error);
                    session.translate.lock_or_recover().end_turn(&Value::Null, Some(text))
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
        if let Some(id) = self.session_id.lock_or_recover().clone() {
            let _ = self.write(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": id } }));
        }
        let asked: Vec<Asked> = self.asked.lock_or_recover().drain().map(|(_, asked)| asked).collect();
        for asked in asked {
            let _ = self.write(&json!({ "jsonrpc": "2.0", "id": asked.rpc_id, "result": { "outcome": { "outcome": "cancelled" } } }));
        }
    }

    /// La respuesta del usuario a un permiso: la opción del agente que corresponde.
    fn answer_permission(&self, request: &str, result: &Value) {
        let Some(asked) = self.asked.lock_or_recover().remove(request) else {
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
                let (request, events) = self.translate.lock_or_recover().permission(&params);
                self.asked.lock_or_recover().insert(request, Asked { rpc_id: id, options });
                self.send_all(events);
            }
            // Otra petición del agente (archivos, terminal): no las ofrecemos.
            (Some(_), Some(id)) => {
                let _ = self.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "No disponible" } }));
            }
            (Some("session/update"), None) => {
                let update = message.pointer("/params/update").cloned().unwrap_or(Value::Null);
                let events = self.translate.lock_or_recover().update(&update);
                self.send_all(events);
            }
            (Some(method), None) => warn_unknown(self.agent.name, "method", method),
            (None, None) => warn_unknown(self.agent.name, "message", "sin método ni id"),
        }
    }

    /// La respuesta a una petición nuestra: a quien la espera. Una que nadie espera es de la
    /// pill anterior (la sesión se retomó del daemon): si cierra un turno, se cierra igual.
    fn answered(&self, id: &Value, message: &Value) {
        let waiter = id.as_u64().and_then(|id| self.pending.lock_or_recover().remove(&id));
        match waiter {
            Some(waiter) => {
                let reply = match message.get("error").filter(|e| !e.is_null()) {
                    Some(error) => Err(error.clone()),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = waiter.send(reply);
            }
            None => {
                let events = orphan_reply(&mut self.translate.lock_or_recover(), self.agent.codex, message);
                self.send_all(events);
            }
        }
    }

    /// El proceso terminó (`gone`) o se perdió la conexión con el daemon que lo corre: lo
    /// pendiente falla y la conversación se cierra.
    fn exited(&self, gone: bool) {
        if gone {
            self.gone.store(true, Ordering::Relaxed);
        }
        if let Some(Pipe::Daemon { alive, .. }) = self.pipe.get() {
            alive.store(false, Ordering::SeqCst);
        }
        for (_, waiter) in self.pending.lock_or_recover().drain() {
            let _ = waiter.send(Err(json!({ "message": tr("pill.code.acp.stopped") })));
        }
        if self.agent.codex {
            let events = self.codex_exited(gone);
            self.send_all(events);
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
        if !Self::known_update(kind) {
            warn_unknown(self.model, "sessionUpdate", kind);
            return Vec::new();
        }
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
            // Cuánto de la ventana de contexto va usado (`used` de `size` tokens): el
            // mismo medidor que Claude, que lo pide aparte (`usage.rs`).
            "usage_update" => {
                let used = update.get("used").and_then(Value::as_u64);
                let size = update.get("size").and_then(Value::as_u64).filter(|size| *size > 0);
                return match (used, size) {
                    (Some(used), Some(size)) => vec![("acp_usage", json!({ "used": used, "size": size }))],
                    _ => Vec::new(),
                };
            }
            _ => {}
        }
        // Lo que llega sin un turno en curso (restos tras detener o terminar) no
        // abre un mensaje: el chat quedaría «trabajando» sin un `result` que lo
        // cierre.
        if self.muted || self.started.is_none() {
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

    /// Los `sessionUpdate` que Atic dibuja o ignora a sabiendas (`user_message_chunk` es
    /// el eco de lo que escribió el usuario, que el chat ya tiene).
    fn known_update(kind: &str) -> bool {
        matches!(
            kind,
            "config_option_update" | "current_mode_update" | "available_commands_update" | "usage_update" | "agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update" | "plan" | "user_message_chunk"
        )
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
                    "max_tokens" => tr("pill.code.acp.maxTokens"),
                    "max_turn_requests" => tr("pill.code.acp.maxTurns"),
                    "refusal" => tr("pill.code.acp.refusal"),
                    _ => tr("pill.code.acp.unexpectedEnd"),
                };
                json!({ "subtype": "error", "isError": true, "errors": [why], "durationMs": duration, "usage": usage })
            }
        };
        events.push(("result", data));
        events
    }
}

// --- Retomar una conversación del daemon ---------------------------------------------

/// Una respuesta que ninguna petición de esta pill espera: la de un turno que la pill
/// anterior mandó antes de cerrarse. Si cierra el turno (ACP: trae `stopReason`, o es un
/// error con un turno abierto), el chat recibe su `result`; si no, el chat quedaría
/// trabajando para siempre. Codex cierra sus turnos con un aviso, no con una respuesta.
fn orphan_reply(translate: &mut Translator, codex: bool, message: &Value) -> Vec<(&'static str, Value)> {
    if codex {
        return Vec::new();
    }
    let error = message.get("error").filter(|e| !e.is_null());
    match (message.pointer("/result/stopReason"), error) {
        (Some(_), _) => translate.end_turn(&message["result"], None),
        (None, Some(error)) if translate.started.is_some() => {
            let text = error.get("message").and_then(Value::as_str).unwrap_or(tr("pill.code.acp.agentError")).to_string();
            translate.end_turn(&Value::Null, Some(text))
        }
        _ => Vec::new(),
    }
}

/// El id de un mensaje JSON-RPC como texto (los agentes lo mandan como número o texto).
fn id_text(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// El texto de un mensaje del usuario: `prompt` de ACP o `input` de Codex, sin las
/// instrucciones para delegar que lleva delante el primero.
fn prompt_text(params: &Value) -> String {
    let parts: Vec<&str> = params
        .get("prompt")
        .or_else(|| params.get("input"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect();
    super::delegate::without_guide(&parts.join("\n")).to_string()
}

/// Lo que se rearma de las líneas que guardó el daemon.
struct Rebuilt {
    translate: Translator,
    codex: codex::State,
    asked: HashMap<String, Asked>,
    events: Vec<(&'static str, Value)>,
    session_id: Option<String>,
}

/// Repasa lo que la pill escribió (`d: "in"`) y lo que el agente contestó (`d: "out"`) con
/// un `Translator` nuevo: los eventos que rearman el chat (mensajes, herramientas, permisos
/// sin contestar, opciones) y el estado para seguir la conversación desde ahí.
fn rebuild(agent: &'static AcpAgent, key: &str, lines: &[Value]) -> Rebuilt {
    let mut r = Rebuilt { translate: Translator::new(key, agent.name), codex: codex::State::default(), asked: HashMap::new(), events: Vec::new(), session_id: None };
    // Las peticiones de la pill que esperan respuesta: id → (método, parámetros).
    let mut sent: HashMap<String, (String, Value)> = HashMap::new();
    for entry in lines {
        let Some(message) = entry.get("line").and_then(Value::as_str).and_then(|line| serde_json::from_str::<Value>(line).ok()) else {
            continue;
        };
        let ours = entry.get("d").and_then(Value::as_str) == Some("in");
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").filter(|id| !id.is_null());
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (ours, method, id) {
            (true, Some(method), Some(id)) => {
                match method {
                    "session/prompt" | "turn/start" => {
                        r.events.push(("user", json!({ "content": [{ "type": "text", "text": prompt_text(&params) }] })));
                        r.translate.begin_turn();
                        if method == "turn/start" {
                            codex::turn_began(&mut r.codex, &mut r.translate, &params);
                        }
                    }
                    "session/load" => r.translate.muted = true,
                    _ => {}
                }
                sent.insert(id_text(id), (method.to_string(), params));
            }
            // La pill contestó una petición del agente (un permiso): ya no espera.
            (true, None, Some(id)) => {
                let id = id_text(id);
                let answered: Vec<String> = r.asked.iter().filter(|(_, asked)| id_text(&asked.rpc_id) == id).map(|(request, _)| request.clone()).collect();
                for request in answered {
                    r.asked.remove(&request);
                    r.events.push(("permission_cancel", json!({ "requestId": request })));
                }
            }
            (true, ..) => {}
            // La respuesta del agente a una petición de la pill.
            (false, None, Some(id)) => {
                let Some((method, request)) = sent.remove(&id_text(id)) else {
                    continue;
                };
                let error = message.get("error").filter(|e| !e.is_null());
                let result = message.get("result").cloned().unwrap_or(Value::Null);
                let reply = match error {
                    Some(error) => Err(error),
                    None => Ok(&result),
                };
                if agent.codex {
                    let (events, thread) = codex::replayed(&mut r.codex, &mut r.translate, &method, &request, reply);
                    r.events.extend(events);
                    if thread.is_some() {
                        r.session_id = thread;
                    }
                    continue;
                }
                match (method.as_str(), reply) {
                    ("session/new", Ok(result)) => {
                        r.session_id = result.get("sessionId").and_then(Value::as_str).map(str::to_string);
                        r.events.extend(r.translate.opened(result));
                    }
                    ("session/load", reply) => {
                        r.translate.muted = false;
                        if let Ok(result) = reply {
                            r.session_id = request.get("sessionId").and_then(Value::as_str).map(str::to_string);
                            r.events.extend(r.translate.opened(result));
                        }
                    }
                    ("session/set_config_option" | "session/set_mode", Ok(result)) => r.events.extend(r.translate.configured(&request, result)),
                    ("session/prompt", Ok(result)) => r.events.extend(r.translate.end_turn(result, None)),
                    ("session/prompt", Err(error)) => {
                        let text = error.get("message").and_then(Value::as_str).unwrap_or(tr("pill.code.acp.agentError")).to_string();
                        r.events.extend(r.translate.end_turn(&Value::Null, Some(text)));
                    }
                    _ => {}
                }
            }
            // El agente pide permiso.
            (false, Some(method), Some(id)) => {
                let call = match method {
                    "session/request_permission" if !agent.codex => Some(params.clone()),
                    "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" if agent.codex => Some(codex::approval(&params)),
                    _ => None,
                };
                if let Some(call) = call {
                    let options = if agent.codex { Vec::new() } else { call.get("options").and_then(Value::as_array).cloned().unwrap_or_default() };
                    let (request, events) = r.translate.permission(&call);
                    r.asked.insert(request, Asked { rpc_id: id.clone(), options });
                    r.events.extend(events);
                }
            }
            // Un aviso del agente.
            (false, Some(method), None) => {
                if agent.codex {
                    let steps = codex::notification(&mut r.codex, method, &params);
                    r.events.extend(codex::run_steps(&mut r.translate, steps));
                } else if method == "session/update" {
                    let update = params.get("update").cloned().unwrap_or(Value::Null);
                    r.events.extend(r.translate.update(&update));
                }
            }
            _ => {}
        }
    }
    r
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

/// El nombre de un proveedor de OpenCode («opencode-go» → «OpenCode Go»). OpenCode lista
/// los proveedores con credenciales; los de Zen (`opencode/…`) cobran del saldo de la cuenta,
/// salvo los `-free`.
pub fn provider_name(id: &str) -> String {
    match id {
        "opencode" => "OpenCode Zen".into(),
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
    // `-free`. Los demás de Zen (`opencode/…`) cobran del saldo: no son gratis.
    if let Some((provider, model)) = value.split_once('/').or_else(|| name.split_once('/')) {
        let shown = name.split_once('/').map_or(name, |(_, rest)| rest);
        let shown = if shown.is_empty() { model } else { shown };
        let free = value.ends_with("-free");
        let clean = shown.strip_suffix("-free").or_else(|| shown.strip_suffix(" Free")).unwrap_or(shown);
        let label = if clean.contains(' ') { clean.to_string() } else { pretty_model(clean) };
        return (label, free.then(|| tr("pill.code.acp.free").to_string()), Some(provider_name(provider)));
    }
    let label = if name.chars().any(|c| c.is_ascii_uppercase()) || name.contains(' ') { name.to_string() } else { pretty_model(name) };
    let params = value.split_once('[').and_then(|(_, rest)| rest.strip_suffix(']')).unwrap_or_default();
    let mut detail = Vec::new();
    for pair in params.split(',').filter(|p| !p.is_empty()) {
        let (key, val) = pair.split_once('=').unwrap_or((pair, ""));
        match (key, val) {
            ("context", size) => detail.push(tf("pill.code.acp.context", &[("size", &size)])),
            ("effort" | "reasoning_effort", level) => detail.push(tf("pill.code.acp.effort", &[("level", &effort_name(level).to_lowercase())])),
            ("thinking", "true") => detail.push(tr("pill.code.acp.reasoning").into()),
            ("fast", "true") => detail.push(tr("pill.code.acp.fast").into()),
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
        g if g == provider_name("opencode") => 1,
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
        "minimal" | "none" => tr("pill.code.acp.effMin").into(),
        "low" => tr("pill.code.acp.effLow").into(),
        "medium" => tr("pill.code.acp.effMedium").into(),
        "high" => tr("pill.code.acp.effHigh").into(),
        "xhigh" => tr("pill.code.acp.effXhigh").into(),
        "max" => tr("pill.code.acp.effMax").into(),
        "default" | "" => tr("pill.code.acp.effDefault").into(),
        other => pretty_model(other),
    }
}

/// El nombre y la explicación de un modo conocido (los de OpenCode y Cursor).
pub fn mode_text(id: &str, name: &str, description: &str) -> (String, String) {
    let (label, hint) = match id {
        "build" => ("Build", tr("pill.code.acp.modeBuildHint")),
        "agent" => (tr("pill.code.acp.modeAgent"), tr("pill.code.acp.modeAgentHint")),
        "plan" => ("Plan", tr("pill.code.acp.modePlanHint")),
        "ask" => (tr("pill.code.acp.modeAsk"), tr("pill.code.acp.modeAskHint")),
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
pub(super) fn opencode_reason() -> Option<String> {
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
        return Some(tr("pill.code.acp.goExpired").into());
    }
    if cause.contains("Insufficient account funds") {
        return Some(tr("pill.code.acp.zenNoFunds").into());
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
                parts.push(tf("pill.code.acp.edited", &[("path", &path)]));
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
    fn lo_que_llega_sin_turno_no_deja_el_chat_trabajando() {
        let mut t = Translator::new("c", "OpenCode");
        let late = t.update(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "tarde" } }));
        assert!(late.is_empty());
    }

    #[test]
    fn el_uso_del_contexto_llega_al_medidor() {
        let mut t = Translator::new("c", "OpenCode");
        let events = t.update(&json!({ "sessionUpdate": "usage_update", "used": 12_000, "size": 200_000 }));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "acp_usage");
        assert_eq!(events[0].1["used"], 12_000);
        // Sin el tamaño de la ventana no hay porcentaje que mostrar.
        assert!(t.update(&json!({ "sessionUpdate": "usage_update", "used": 10 })).is_empty());
    }

    #[test]
    fn un_aviso_de_tipo_desconocido_no_dibuja_nada() {
        let mut t = Translator::new("c", "OpenCode");
        t.begin_turn();
        assert!(t.update(&json!({ "sessionUpdate": "algo_nuevo", "used": 10 })).is_empty());
        assert!(t.update(&json!({})).is_empty());
        assert!(Translator::known_update("tool_call_update") && !Translator::known_update("algo_nuevo"));
        // Después sigue traduciendo lo que conoce.
        assert_eq!(t.update(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "hola" } })).len(), 1);
    }

    #[test]
    fn un_fallo_lleva_lo_ultimo_que_escribio_el_proceso() {
        let session = Session::new(agent("grok").unwrap(), "c", mpsc::unbounded().0, true);
        // Lo que no es JSON se guarda (sin secretos); lo que sí, sigue al protocolo.
        session.output("Error: not logged in, token=abc123");
        session.output(r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#);
        let text = session.failure_text(&json!({ "message": "boom" }));
        assert!(text.starts_with("boom. ") && text.contains("grok login"), "{text}");
        assert!(text.ends_with("\n\n> Error: not logged in, token=***"), "{text}");
        // Lo de antes del turno no se le cuelga a un error de este turno.
        let mark = session.diag.lock().unwrap().mark();
        session.diag_mark.store(mark, Ordering::Relaxed);
        assert_eq!(session.failure_text(&json!({ "message": "boom" })), "boom");
        // Un agente con un ACP más nuevo: el fallo lo dice.
        session.newer_protocol.store(2, Ordering::Relaxed);
        assert_eq!(session.failure_text(&json!({ "message": "boom" })), "boom\n\n(ACP v2 > v1)");
        // Cerrada a propósito, no hay nada que explicar.
        session.kill();
        assert_eq!(session.explain("boom".into()), "boom");
    }

    #[test]
    fn lo_que_dice_el_daemon_de_la_salida_se_guarda() {
        let session = Session::new(agent("grok").unwrap(), "c", mpsc::unbounded().0, true);
        session.note_exit(&json!({ "code": 3, "stderr": ["\u{1b}[31mfatal\u{1b}[0m: sin cuota", 7] }));
        assert_eq!(*session.exit_code.lock().unwrap(), Some(3));
        assert_eq!(session.diag.lock().unwrap().since(0, diag::SHOWN_LINES), vec!["fatal: sin cuota"]);
        // Un daemon de hoy solo manda el código (o la señal, como texto).
        session.note_exit(&json!({ "code": "SIGKILL" }));
        assert_eq!(*session.exit_code.lock().unwrap(), None);
    }

    #[test]
    fn los_trozos_abren_mensaje_y_bloques_en_orden() {
        let mut t = Translator::new("c", "OpenCode");
        t.begin_turn();
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
        t.begin_turn();
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
        t.begin_turn();
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
        t.begin_turn();
        let cancelled = t.end_turn(&json!({ "stopReason": "cancelled" }), None);
        assert_eq!(events_named(&cancelled, "result")[0]["subtype"], "error_during_execution");
        let failed = t.end_turn(&Value::Null, Some("Sin sesión".into()));
        assert_eq!(events_named(&failed, "result")[0]["errors"][0], "Sin sesión");
    }

    #[test]
    fn las_opciones_del_agente_llegan_al_chat_y_se_ponen_al_dia() {
        let mut t = Translator::new("c", "OpenCode");
        t.begin_turn();
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
        // OpenCode: el proveedor va como grupo; los gratuitos (`-free`), rotulados.
        let (label, detail, group) = model_entry("opencode-go/gemini-4-argon-contributor", "opencode-go/gemini-4-argon-contributor");
        assert_eq!((label.as_str(), detail, group.as_deref()), ("Gemini 4 Argon Contributor", None, Some("OpenCode Go")));
        let (label, detail, group) = model_entry("opencode/space-bunny-free", "opencode/space-bunny-free");
        assert_eq!((label.as_str(), detail.as_deref(), group.as_deref()), ("Space Bunny", Some("Gratis"), Some("OpenCode Zen")));
        // Como los manda OpenCode: el nombre legible, con el proveedor delante.
        let (label, detail, group) = model_entry("opencode-go/cat-jam-free", "opencode-go/Cat Jam Free");
        assert_eq!((label.as_str(), detail.as_deref(), group.as_deref()), ("Cat Jam", Some("Gratis"), Some("OpenCode Go")));
        assert_eq!(model_entry("minimax/MiniMax-M2.5", "minimax/MiniMax-M2.5").0, "MiniMax M2.5");
        let mut groups = vec!["NVIDIA", "OpenCode Zen", "MiniMax", "OpenCode Go"];
        groups.sort_by_key(|g| group_rank(g));
        assert_eq!(groups, vec!["OpenCode Go", "OpenCode Zen", "MiniMax", "NVIDIA"]);
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
        t.begin_turn();
        t.muted = true;
        assert!(t.update(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "viejo" } })).is_empty());
    }

    /// Una línea guardada por el daemon: `in` la escribió la pill, `out` el agente.
    fn line(d: &str, message: Value) -> Value {
        json!({ "d": d, "line": message.to_string() })
    }

    #[test]
    fn los_eventos_de_los_agentes_del_daemon_van_a_su_sesion() {
        assert!(is_proc_event("procOut") && is_proc_event("procExit") && is_proc_event("procErr"));
        assert!(!is_proc_event("stream") && !is_proc_event("attached"));
        let hub = Hub::new(mpsc::unbounded().0);
        // Sin sesión con esa clave se consumen igual: no llegan a la vista como de Claude.
        assert!(hub.route("procOut", Some("c9"), &json!({ "line": "{}" })));
        assert!(!hub.route("result", Some("c9"), &json!({})));
        assert!(!hub.remote("c9") && !hub.local_alive("c9"));
    }

    #[test]
    fn una_respuesta_de_la_pill_anterior_cierra_el_turno_igual() {
        let mut t = Translator::new("c", "OpenCode");
        t.begin_turn();
        t.update(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "listo" } }));
        let events = orphan_reply(&mut t, false, &json!({ "jsonrpc": "2.0", "id": 4, "result": { "stopReason": "end_turn" } }));
        assert_eq!(events_named(&events, "assistant")[0]["content"][0]["text"], "listo");
        assert_eq!(events_named(&events, "result")[0]["subtype"], "success");
        assert!(t.started.is_none());
        // Una respuesta cualquiera (sin `stopReason`) no toca nada.
        assert!(orphan_reply(&mut t, false, &json!({ "id": 5, "result": {} })).is_empty());
        // Un error con un turno abierto también lo cierra.
        t.begin_turn();
        let failed = orphan_reply(&mut t, false, &json!({ "id": 6, "error": { "message": "se cayó" } }));
        assert_eq!(events_named(&failed, "result")[0]["errors"][0], "se cayó");
        // Codex cierra con `turn/completed`, no con una respuesta.
        t.begin_turn();
        assert!(orphan_reply(&mut t, true, &json!({ "id": 7, "result": { "stopReason": "end_turn" } })).is_empty());
    }

    #[test]
    fn lo_guardado_en_el_daemon_rearma_el_chat_acp() {
        let agent = agent("opencode").unwrap();
        let guided = super::super::delegate::with_guide("revisa el parser");
        let lines = vec![
            line("in", json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })),
            line("out", json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": {} } })),
            line("in", json!({ "jsonrpc": "2.0", "id": 2, "method": "session/new", "params": { "cwd": "C:\\repo" } })),
            line("out", json!({ "jsonrpc": "2.0", "id": 2, "result": { "sessionId": "ses-1", "modes": { "currentModeId": "build", "availableModes": [] } } })),
            line("in", json!({ "jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": { "sessionId": "ses-1", "prompt": [{ "type": "text", "text": guided }] } })),
            line("out", json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Hecho" } } } })),
            line("out", json!({ "jsonrpc": "2.0", "id": 3, "result": { "stopReason": "end_turn" } })),
            line("in", json!({ "jsonrpc": "2.0", "id": 4, "method": "session/prompt", "params": { "sessionId": "ses-1", "prompt": [{ "type": "text", "text": "y ahora borra build" }] } })),
            line("out", json!({ "jsonrpc": "2.0", "id": "p1", "method": "session/request_permission", "params": {
                "toolCall": { "toolCallId": "t1", "kind": "execute", "title": "rm -rf build", "rawInput": { "command": "rm -rf build" } },
                "options": [{ "optionId": "a", "kind": "allow_once" }] } })),
            line("out", "no es json".into()),
        ];
        let r = rebuild(agent, "c1", &lines);
        assert_eq!(r.session_id.as_deref(), Some("ses-1"));
        let users = events_named(&r.events, "user");
        assert_eq!(users[0]["content"][0]["text"], "revisa el parser", "sin las instrucciones para delegar");
        assert_eq!(users[1]["content"][0]["text"], "y ahora borra build");
        assert_eq!(events_named(&r.events, "result").len(), 1);
        assert_eq!(events_named(&r.events, "acp_config")[0]["modes"]["currentModeId"], "build");
        // El segundo turno sigue abierto y su permiso espera respuesta: se puede contestar.
        assert!(r.translate.started.is_some());
        assert_eq!(events_named(&r.events, "permission")[0]["toolName"], "Bash");
        let asked = r.asked.get("acp-perm-1").unwrap();
        assert_eq!(asked.rpc_id, json!("p1"));

        // Si la pill ya lo había contestado, el permiso no queda pendiente.
        let mut answered = lines.clone();
        answered.push(line("in", json!({ "jsonrpc": "2.0", "id": "p1", "result": { "outcome": { "outcome": "selected", "optionId": "a" } } })));
        let r = rebuild(agent, "c1", &answered);
        assert!(r.asked.is_empty());
        assert_eq!(events_named(&r.events, "permission_cancel")[0]["requestId"], "acp-perm-1");
    }

    #[test]
    fn lo_guardado_en_el_daemon_rearma_el_chat_de_codex() {
        let agent = agent("codex").unwrap();
        let lines = vec![
            line("in", json!({ "id": 1, "method": "model/list", "params": {} })),
            line("out", json!({ "id": 1, "result": { "data": [{ "id": "gpt-5.5", "model": "gpt-5.5", "displayName": "GPT 5.5", "description": "", "hidden": false,
                "defaultReasoningEffort": "medium", "supportedReasoningEfforts": [{ "reasoningEffort": "medium", "description": "" }] }] } })),
            line("in", json!({ "id": 2, "method": "thread/start", "params": { "cwd": "C:\\repo" } })),
            line("out", json!({ "id": 2, "result": { "thread": { "id": "th-1" }, "model": "gpt-5.5", "reasoningEffort": "medium", "sandbox": { "type": "readOnly" } } })),
            line("in", json!({ "id": 3, "method": "turn/start", "params": { "threadId": "th-1", "input": [{ "type": "text", "text": "hola" }] } })),
            line("out", json!({ "id": 3, "result": { "turn": { "id": "turn-1" } } })),
            line("out", json!({ "method": "item/agentMessage/delta", "params": { "itemId": "m1", "delta": "Ho" } })),
        ];
        let r = rebuild(agent, "c2", &lines);
        assert_eq!(r.session_id.as_deref(), Some("th-1"));
        assert_eq!(events_named(&r.events, "user")[0]["content"][0]["text"], "hola");
        assert_eq!(events_named(&r.events, "acp_config")[0]["modes"]["currentModeId"], "read-only");
        assert_eq!(r.translate.current("model"), Some(json!("gpt-5.5")));
        // El turno sigue: lo escrito hasta ahora está en el chat y se puede detener.
        assert!(r.translate.started.is_some());
        assert_eq!(events_named(&r.events, "stream").len(), 1);
        let mut more = lines;
        more.push(line("out", json!({ "method": "turn/completed", "params": { "turn": { "status": "completed" } } })));
        let r = rebuild(agent, "c2", &more);
        assert_eq!(events_named(&r.events, "result")[0]["subtype"], "success");
        assert!(r.translate.started.is_none());
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
