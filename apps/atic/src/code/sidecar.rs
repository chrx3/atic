//! El proceso del agente: `sidecar/agent.mjs`, un Node con el Claude Agent SDK (viene de la
//! referencia). Maneja todas las conversaciones y usa el `claude` instalado con la sesión del
//! usuario: nunca lee ni guarda credenciales.
//!
//! Corre como proceso propio (`--daemon`), desacoplado de la pill: cerrar la ventana, salir de
//! Atic o actualizarla no corta a los agentes, y al volver a abrir se retoman donde iban
//! (como t3code). La pill lo encuentra por `code-agent.json` en sus datos
//! (`{ port, token, pid, version }`) y se conecta por 127.0.0.1.
//!
//! Protocolo (una línea JSON por mensaje):
//! - `{ hello: { token, client, since, persist } }` → saludo; responde
//!   `{ hello: { ok, version, pid, seq, fresh, sessions, alive } }`. Con `since` (el último
//!   `seq` visto) reenvía lo perdido; sin él manda una foto de las sesiones vivas, que llega
//!   a la vista como el evento `attached` para rearmar los chats.
//! - `{ id, method, params }` → petición; `{ id, result }` o `{ id, error }` ← respuesta.
//! - `{ event, key?, data, seq }` ← evento (streaming, permisos, fin de turno…).
//!
//! También corre los agentes ACP de Atic Code (`acp.rs`): `procSpawn { key, program, args,
//! cwd, meta }`, `procWrite { key, line }`, `procKill { key }`, y los eventos `procOut { line }`
//! y `procExit { code }`. El saludo trae `procs` (los vivos, con su `tag`, si trabajan y, en la
//! foto, sus primeras y últimas líneas). `setTag { key, tag }` guarda un JSON de la pill por
//! conversación que vuelve en la foto. Un proceso viejo no los conoce: responde error y los
//! agentes van en un tubo propio.
//!
//! Si el proceso es de otra versión (otro `agent.mjs`), se le pide `retire`: sigue atendiendo
//! hasta que nada trabaje y entonces se va; la próxima petición levanta el nuevo. Sin clientes
//! ni trabajo, se cierra solo a los 10 minutos.
//!
//! La conexión vive en un hilo propio: abrir o levantar el proceso no traba la interfaz. Las
//! respuestas llegan por un `oneshot`; los eventos, por un canal que la vista lee en su hilo.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::{mpsc, oneshot};
use serde_json::{json, Value};

pub type Reply = Result<Value, String>;

pub enum Incoming {
    Event { event: String, key: Option<String>, data: Value },
    /// Se cortó la conexión con el proceso: las conversaciones vivas se perdieron.
    Exit,
}

/// Lo que el hilo de la conexión hace por encargo.
enum Order {
    Call { id: u64, line: String },
    /// Conectarse si el proceso ya existe, sin levantarlo (al abrir la ventana).
    Attach,
    /// Pedirle que se cierre (si nada trabaja) y soltar la conexión.
    Shutdown,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Reply>>>>;

pub struct Sidecar {
    outbox: Mutex<std_mpsc::Sender<Order>>,
    pending: Pending,
    next: AtomicU64,
    events: mpsc::UnboundedSender<Incoming>,
}

/// Un programa del PATH. En Windows también el PATH fresco del registro: lo que se
/// instala con Atic abierto (Node, Claude Code) se encuentra sin reiniciarla.
fn which(bin: &str) -> Option<PathBuf> {
    atic_agents::exe::resolve(bin)
}

/// El Node que corre el agente; en Windows también donde lo deja su instalador
/// (`winget`), por si el PATH todavía no lo tiene.
pub fn node_path() -> Option<PathBuf> {
    which("node").or_else(|| {
        let program_files = std::env::var_os("ProgramFiles").filter(|_| cfg!(windows))?;
        Some(PathBuf::from(program_files).join("nodejs").join("node.exe")).filter(|p| p.is_file())
    })
}

fn home() -> Option<PathBuf> {
    crate::paths::home().map(std::ffi::OsString::from).or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// El binario de Claude Code: el del PATH o el del instalador nativo.
pub fn claude_path() -> Option<PathBuf> {
    which("claude").or_else(|| {
        let name = if cfg!(windows) { "claude.exe" } else { "claude" };
        home().map(|h| h.join(".local").join("bin").join(name)).filter(|p| p.is_file())
    })
}

/// El script del agente: `ATIC_CODE_SIDECAR` si está, el empaquetado junto al
/// ejecutable, o el del repositorio en desarrollo.
fn script_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ATIC_CODE_SIDECAR").map(PathBuf::from) {
        return Some(path);
    }
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    // Junto al exe (Windows) o en `Contents/Resources` de la `.app` (macOS).
    let bundled = exe_dir.into_iter().flat_map(|dir| {
        [dir.join("sidecar").join("agent.mjs"), dir.join("../Resources/sidecar/agent.mjs")]
    });
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("sidecar").join("agent.mjs");
    bundled.chain(std::iter::once(dev)).find(|p| p.is_file())
}

/// Dónde el proceso deja cómo encontrarlo.
fn discovery_path() -> Option<PathBuf> {
    crate::paths::file("code-agent.json")
}

/// La versión de un `agent.mjs`: un hash de su contenido. Si cambia (una actualización de
/// Atic), el proceso que corre se reemplaza cuando quede libre.
fn version_of(bytes: &[u8]) -> String {
    // FNV-1a de 64 bits: estable entre ejecuciones (no como el `DefaultHasher`).
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Lo que dice `code-agent.json`.
#[derive(Debug, PartialEq)]
struct Discovery {
    port: u16,
    token: String,
    pid: u32,
    version: String,
}

fn parse_discovery(text: &str) -> Option<Discovery> {
    let value: Value = serde_json::from_str(text).ok()?;
    Some(Discovery {
        port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
        token: value.get("token")?.as_str()?.to_string(),
        pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
        version: value.get("version").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

fn read_discovery() -> Option<Discovery> {
    parse_discovery(&std::fs::read_to_string(discovery_path()?).ok()?)
}

/// Desde qué evento pedir lo perdido: solo si es el mismo proceso de la vez anterior (los
/// números de un proceso nuevo empiezan de cero); si no, una foto de sus sesiones.
fn since_for(last_pid: Option<u32>, pid: u32, last_seq: u64) -> u64 {
    if last_pid == Some(pid) {
        last_seq
    } else {
        0
    }
}

/// El proceso que corre es de otro `agent.mjs` y hay que reemplazarlo cuando quede libre.
fn should_retire(running: &str, expected: &str) -> bool {
    !expected.is_empty() && running != expected
}

fn hello_line(token: &str, client: &str, since: u64, persist: bool) -> String {
    json!({ "hello": { "token": token, "client": client, "since": since, "persist": persist } }).to_string()
}

/// Levanta el proceso desacoplado de la pill: sin consola, en su propio grupo y fuera del Job
/// de la pill (si la pill corre en uno), para que no muera con ella. Devuelve su pid.
fn spawn_daemon(script: &Path, version: &str, discovery: &Path) -> Result<u32, String> {
    let node = node_path().ok_or_else(|| crate::i18n::t("pill.code.sidecar.noNode").to_string())?;
    let build = |breakaway: bool| {
        let mut command = Command::new(&node);
        command
            .arg(script)
            .arg("--daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // No en la carpeta de la pill: el instalador tiene que poder reemplazarla.
            .current_dir(std::env::temp_dir())
            .env("ATIC_SIDECAR_VERSION", version)
            .env("ATIC_SIDECAR_DISCOVERY", discovery);
        if let Some(claude) = claude_path() {
            command.env("ATIC_CLAUDE_PATH", claude);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
            let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | if breakaway { CREATE_BREAKAWAY_FROM_JOB } else { 0 };
            command.creation_flags(flags);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let _ = breakaway;
            command.process_group(0);
        }
        command
    };
    // Un Job que no deja salir rechaza `CREATE_BREAKAWAY_FROM_JOB`: se intenta sin él.
    let mut child = build(true).spawn().or_else(|_| build(false).spawn()).map_err(|e| crate::i18n::tf("pill.code.sidecar.startFailed", &[("error", &e)]))?;
    let pid = child.id();
    // Se recoge al salir (en Unix quedaría como zombi mientras la pill viva).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

/// Solo un hilo levanta el proceso a la vez (Atic Code y el chat del notch lo comparten).
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// El hilo de la conexión: la abre (o levanta el proceso), escribe lo que se pide y lanza un
/// lector por conexión.
struct Link {
    client: &'static str,
    persist: bool,
    pending: Pending,
    events: mpsc::UnboundedSender<Incoming>,
    stream: Option<TcpStream>,
    alive: Arc<AtomicBool>,
    last_seq: Arc<AtomicU64>,
    last_pid: Option<u32>,
}

impl Link {
    fn connected(&self) -> bool {
        self.stream.is_some() && self.alive.load(Ordering::SeqCst)
    }

    fn fail(&self, id: u64, error: String) {
        if let Some(reply) = self.pending.lock().unwrap().remove(&id) {
            let _ = reply.send(Err(error));
        }
    }

    /// Se conecta al proceso que corre; si no hay y `spawn`, lo levanta y espera su archivo.
    fn connect(&mut self, spawn: bool) -> Result<(), String> {
        let script = script_path().ok_or_else(|| crate::i18n::t("pill.code.sidecar.noScript").to_string())?;
        let expected = std::fs::read(&script).map(|bytes| version_of(&bytes)).unwrap_or_default();
        let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(found) = read_discovery() {
            match self.open(&found, &expected) {
                Ok(()) => return Ok(()),
                Err(error) => tracing::debug!(%error, "atic code: el agente anotado no responde"),
            }
        }
        if !spawn {
            return Err(String::new());
        }
        let discovery = discovery_path().ok_or_else(|| crate::i18n::t("pill.code.sidecar.noScript").to_string())?;
        let pid = spawn_daemon(&script, &expected, &discovery)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            std::thread::sleep(Duration::from_millis(100));
            if let Some(found) = read_discovery().filter(|d| d.pid == pid) {
                return self.open(&found, &expected);
            }
            if Instant::now() > deadline {
                return Err(crate::i18n::t("pill.code.sidecar.notReady").into());
            }
        }
    }

    /// Abre la conexión, saluda y deja un lector corriendo. La foto de las sesiones llega a la
    /// vista antes que cualquier otro evento de esta conexión.
    fn open(&mut self, found: &Discovery, expected: &str) -> Result<(), String> {
        let address = SocketAddr::from(([127, 0, 0, 1], found.port));
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        let since = since_for(self.last_pid, found.pid, self.last_seq.load(Ordering::SeqCst));
        writeln!(stream, "{}", hello_line(&found.token, self.client, since, self.persist)).map_err(|e| e.to_string())?;
        // La foto lee el historial de cada sesión viva: puede tardar unos segundos.
        stream.set_read_timeout(Some(Duration::from_secs(60))).map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let hello = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("hello").cloned()).filter(|h| h.get("ok").and_then(Value::as_bool) == Some(true));
        let Some(hello) = hello else {
            return Err("saludo rechazado".into());
        };
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        // En Windows la copia de `try_clone` no hereda el cambio: sin esto, el lector cortaba
        // la conexión tras 60 s sin eventos y los chats quedaban como detenidos.
        reader.get_ref().set_read_timeout(None).map_err(|e| e.to_string())?;
        if self.last_pid != Some(found.pid) {
            self.last_seq.store(0, Ordering::SeqCst);
        }
        if let Some(seq) = hello.get("seq").and_then(Value::as_u64) {
            self.last_seq.fetch_max(seq, Ordering::SeqCst);
        }
        self.last_pid = Some(found.pid);
        let running = hello.get("version").and_then(Value::as_str).unwrap_or_default();
        // Solo la Atic Code de verdad (la que deja sesiones) pide el reemplazo; una ventana de
        // capturas o el notch no deciden por ella.
        if self.persist && should_retire(running, expected) {
            tracing::info!(running, expected, "atic code: el agente es de otra versión; se reemplaza cuando quede libre");
            let _ = writeln!(stream, "{}", json!({ "id": 0, "method": "retire", "params": {} }));
        }
        let _ = self.events.unbounded_send(Incoming::Event { event: "attached".into(), key: None, data: hello });

        let alive = Arc::new(AtomicBool::new(true));
        let (pending, events, last_seq, reading) = (self.pending.clone(), self.events.clone(), self.last_seq.clone(), alive.clone());
        std::thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(seq) = message.get("seq").and_then(Value::as_u64) {
                    last_seq.fetch_max(seq, Ordering::SeqCst);
                }
                if let Some(id) = message.get("id").and_then(Value::as_u64) {
                    if let Some(reply) = pending.lock().unwrap().remove(&id) {
                        let result = match message.get("error") {
                            Some(error) if !error.is_null() => Err(error.as_str().unwrap_or("error").to_string()),
                            _ => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = reply.send(result);
                    }
                } else if let Some(event) = message.get("event").and_then(Value::as_str) {
                    let key = message.get("key").and_then(Value::as_str).map(str::to_string);
                    let data = message.get("data").cloned().unwrap_or(Value::Null);
                    let _ = events.unbounded_send(Incoming::Event { event: event.to_string(), key, data });
                }
            }
            reading.store(false, Ordering::SeqCst);
            for (_, reply) in pending.lock().unwrap().drain() {
                let _ = reply.send(Err(crate::i18n::t("pill.code.sidecar.stopped").into()));
            }
            let _ = events.unbounded_send(Incoming::Exit);
        });
        self.alive = alive;
        self.stream = Some(stream);
        Ok(())
    }

    fn write(&mut self, line: &str) -> std::io::Result<()> {
        let stream = self.stream.as_mut().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotConnected))?;
        writeln!(stream, "{line}").and_then(|_| stream.flush())
    }

    fn close(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn run(mut self, inbox: std_mpsc::Receiver<Order>) {
        for command in inbox {
            match command {
                Order::Call { id, line } => {
                    if !self.connected() {
                        if let Err(error) = self.connect(true) {
                            self.fail(id, error);
                            continue;
                        }
                    }
                    if let Err(error) = self.write(&line) {
                        self.close();
                        self.fail(id, crate::i18n::tf("pill.code.sidecar.talkFailed", &[("error", &error)]));
                    }
                }
                Order::Attach => {
                    if !self.connected() {
                        let _ = self.connect(false);
                    }
                }
                Order::Shutdown => {
                    if self.connected() {
                        let _ = self.write(&json!({ "id": 0, "method": "shutdown", "params": {} }).to_string());
                    }
                }
            }
        }
        // La vista se cerró: se suelta la conexión; el proceso y sus agentes siguen.
        self.close();
    }
}

impl Sidecar {
    /// `client` separa las sesiones de cada ventana en el proceso compartido («code»,
    /// «notch»); sin `persist`, las suyas se cierran cuando se desconecta.
    pub fn new(client: &'static str, persist: bool) -> (Arc<Self>, mpsc::UnboundedReceiver<Incoming>) {
        let (events, receiver) = mpsc::unbounded();
        let (outbox, inbox) = std_mpsc::channel();
        let pending: Pending = Arc::default();
        let link = Link {
            client,
            persist,
            pending: pending.clone(),
            events: events.clone(),
            stream: None,
            alive: Arc::new(AtomicBool::new(false)),
            last_seq: Arc::new(AtomicU64::new(0)),
            last_pid: None,
        };
        std::thread::Builder::new().name(format!("atic-agente-{client}")).spawn(move || link.run(inbox)).expect("hilo del agente");
        let sidecar = Self { outbox: Mutex::new(outbox), pending, next: AtomicU64::new(0), events };
        (Arc::new(sidecar), receiver)
    }

    /// El canal por donde llegan los eventos a la vista (ACP manda los suyos por aquí).
    pub fn events(&self) -> mpsc::UnboundedSender<Incoming> {
        self.events.clone()
    }

    /// Se conecta si el proceso ya corre (con agentes de antes de cerrar la ventana o
    /// reiniciar Atic): sus conversaciones llegan como `attached`. No lo levanta.
    pub fn reattach(&self) {
        let _ = self.outbox.lock().unwrap().send(Order::Attach);
    }

    /// Manda una petición. La respuesta llega por el `oneshot`; si no se pudo mandar, llega
    /// el error.
    pub fn call(&self, method: &str, params: Value) -> oneshot::Receiver<Reply> {
        let (reply, receiver) = oneshot::channel();
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.pending.lock().unwrap().insert(id, reply);
        let line = json!({ "id": id, "method": method, "params": params }).to_string();
        if self.outbox.lock().unwrap().send(Order::Call { id, line }).is_err() {
            if let Some(reply) = self.pending.lock().unwrap().remove(&id) {
                let _ = reply.send(Err(crate::i18n::t("pill.code.sidecar.stopped").into()));
            }
        }
        receiver
    }

    /// Le pide al proceso que se cierre si nada trabaja (p. ej. recién se instaló Claude Code
    /// y hay que levantarlo con la ruta nueva). La próxima petición levanta otro.
    pub fn shutdown(&self) {
        let _ = self.outbox.lock().unwrap().send(Order::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_archivo_de_descubrimiento() {
        let found = parse_discovery(r#"{"port":51234,"token":"abc","pid":42,"version":"v1"}"#);
        assert_eq!(found, Some(Discovery { port: 51234, token: "abc".into(), pid: 42, version: "v1".into() }));
        assert_eq!(parse_discovery(r#"{"port":70000,"token":"abc","pid":42}"#), None, "puerto inválido");
        assert_eq!(parse_discovery(r#"{"token":"abc","pid":42}"#), None);
        assert_eq!(parse_discovery("roto"), None);
    }

    #[test]
    fn reanuda_por_secuencia_solo_con_el_mismo_proceso() {
        assert_eq!(since_for(Some(7), 7, 120), 120);
        assert_eq!(since_for(Some(7), 8, 120), 0, "un proceso nuevo numera desde cero");
        assert_eq!(since_for(None, 7, 0), 0, "la primera vez pide la foto");
    }

    #[test]
    fn reemplaza_el_proceso_solo_si_es_de_otra_version() {
        assert!(should_retire("aaa", "bbb"));
        assert!(!should_retire("aaa", "aaa"));
        assert!(!should_retire("aaa", ""), "sin script no se sabe: se deja");
    }

    #[test]
    fn la_version_es_estable_y_cambia_con_el_script() {
        assert_eq!(version_of(b"hola"), version_of(b"hola"));
        assert_ne!(version_of(b"hola"), version_of(b"hola!"));
        assert_eq!(version_of(b"").len(), 16);
    }

    #[test]
    fn el_saludo_lleva_token_cliente_y_secuencia() {
        let hello: Value = serde_json::from_str(&hello_line("t0k", "code", 9, true)).unwrap();
        assert_eq!(hello, json!({ "hello": { "token": "t0k", "client": "code", "since": 9, "persist": true } }));
    }
}
