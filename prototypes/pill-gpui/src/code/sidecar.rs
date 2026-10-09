//! El proceso del agente: `sidecar/agent.mjs`, un Node siempre vivo con el
//! Claude Agent SDK (viene de la referencia). Maneja todas las conversaciones de la
//! ventana y usa el `claude` instalado con la sesión del usuario: nunca lee ni
//! guarda credenciales.
//!
//! Protocolo (una línea JSON por mensaje, por stdin/stdout):
//! - `{ id, method, params }` → petición.
//! - `{ id, result }` o `{ id, error }` ← respuesta.
//! - `{ event, key?, data }` ← evento (streaming, permisos, fin de turno…).
//!
//! El proceso se levanta con la primera petición y se vuelve a levantar si se
//! cayó. Las respuestas llegan por un `oneshot`; los eventos, por un canal que
//! la vista lee en su hilo.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures::channel::{mpsc, oneshot};
use serde_json::{json, Value};

pub type Reply = Result<Value, String>;

pub enum Incoming {
    Event { event: String, key: Option<String>, data: Value },
    /// El proceso terminó: las conversaciones vivas se perdieron.
    Exit,
}

struct Process {
    child: Child,
    stdin: ChildStdin,
}

pub struct Sidecar {
    process: Mutex<Option<Process>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Reply>>>>,
    next: AtomicU64,
    events: mpsc::UnboundedSender<Incoming>,
}

/// Un programa del PATH (en Windows, con `.exe` o `.cmd`).
fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    std::env::split_paths(&path)
        .find_map(|dir| exts.iter().map(|ext| dir.join(format!("{bin}{ext}"))).find(|p| p.is_file()))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
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
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sidecar").join("agent.mjs")));
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("sidecar").join("agent.mjs");
    bundled.filter(|p| p.is_file()).or_else(|| dev.is_file().then_some(dev))
}

impl Sidecar {
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Incoming>) {
        let (events, receiver) = mpsc::unbounded();
        let sidecar = Self {
            process: Mutex::new(None),
            pending: Arc::default(),
            next: AtomicU64::new(0),
            events,
        };
        (Arc::new(sidecar), receiver)
    }

    fn spawn(&self) -> Result<Process, String> {
        let node = which("node").ok_or("No se encontró Node.js. Instálalo para usar el agente.")?;
        let script = script_path().ok_or("No se encontró el proceso del agente (sidecar/agent.mjs).")?;
        let mut command = Command::new(node);
        command
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(claude) = claude_path() {
            command.env("ATIC_CLAUDE_PATH", claude);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| format!("No se pudo iniciar el agente: {e}"))?;
        let stdin = child.stdin.take().ok_or("el agente no tiene stdin")?;
        let stdout = child.stdout.take().ok_or("el agente no tiene stdout")?;
        let stderr = child.stderr.take().ok_or("el agente no tiene stderr")?;

        let pending = self.pending.clone();
        let events = self.events.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
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
            for (_, reply) in pending.lock().unwrap().drain() {
                let _ = reply.send(Err("El agente se detuvo".into()));
            }
            let _ = events.unbounded_send(Incoming::Exit);
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::debug!("agente: {line}");
            }
        });
        Ok(Process { child, stdin })
    }

    /// Manda una petición. La respuesta llega por el `oneshot`; si no se pudo
    /// mandar, llega el error de inmediato.
    pub fn call(&self, method: &str, params: Value) -> oneshot::Receiver<Reply> {
        let (reply, receiver) = oneshot::channel();
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let mut guard = self.process.lock().unwrap();
        let alive = guard.as_mut().is_some_and(|p| matches!(p.child.try_wait(), Ok(None)));
        if !alive {
            match self.spawn() {
                Ok(process) => *guard = Some(process),
                Err(error) => {
                    let _ = reply.send(Err(error));
                    return receiver;
                }
            }
        }
        self.pending.lock().unwrap().insert(id, reply);
        let process = guard.as_mut().expect("proceso recién levantado");
        let line = json!({ "id": id, "method": method, "params": params }).to_string();
        if let Err(error) = writeln!(process.stdin, "{line}").and_then(|_| process.stdin.flush()) {
            *guard = None;
            if let Some(reply) = self.pending.lock().unwrap().remove(&id) {
                let _ = reply.send(Err(format!("No se pudo hablar con el agente: {error}")));
            }
        }
        receiver
    }

    pub fn shutdown(&self) {
        if let Some(mut process) = self.process.lock().unwrap().take() {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.shutdown();
    }
}
