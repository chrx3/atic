//! El hub de Atic Code: un servidor HTTP mínimo en localhost por el que `atic-mcp`
//! (las herramientas `atic_delegate`, `atic_wait`…) le pide a Atic Code que abra
//! conversaciones hijas con otro agente. Escribe `hub.json` (puerto, token y pid) en
//! los datos de Atic; `atic-mcp` lo lee en cada llamada.
//!
//! - **Transporte** (este archivo, sin GPUI): un hilo acepta conexiones y otro por
//!   conexión lee el pedido (HTTP/1.1: línea, cabeceras y cuerpo por `Content-Length`),
//!   revisa el token y lo pasa como `HubCall` por un canal al hilo de la ventana.
//! - **Atic Code** (`delegate.rs`): resuelve cada `HubCall` y contesta por su `reply`.
//!   Lo que espera a un hijo (`mode: "wait"`, `/v1/wait`) contesta más tarde; la
//!   conexión espera hasta `waitS` más un margen.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use serde_json::{json, Value};

/// El tope del cuerpo de un pedido.
const MAX_BODY: usize = 1 << 20;
/// El tope de la línea de pedido y de cada cabecera, y cuántas cabeceras se leen.
const MAX_LINE: usize = 8 * 1024;
const MAX_HEADERS: usize = 64;
/// Lo que se espera a que Atic Code conteste un pedido que no espera a un hijo.
const QUICK_LIMIT: Duration = Duration::from_secs(30);
/// Lo que se espera a un hijo por defecto y como mucho (`waitS`).
pub const WAIT_DEFAULT: u64 = 600;
pub const WAIT_MAX: u64 = 3600;
/// Lo que la conexión espera de más sobre `waitS`: Atic Code contesta `timeout` antes.
const WAIT_MARGIN: u64 = 30;

/// Un error del hub: va como `{"error":{"code","message","data"?}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct HubFault {
    pub code: &'static str,
    pub message: String,
    pub data: Option<Value>,
}

impl HubFault {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    /// El estado HTTP de cada código.
    pub fn status(&self) -> u16 {
        match self.code {
            "unauthorized" => 401,
            "not_found" => 404,
            "method_not_allowed" => 405,
            "not_your_child" | "depth_exceeded" => 403,
            "busy" => 409,
            "too_many_children" => 429,
            "code_closed" | "backend_unavailable" => 503,
            "internal" => 500,
            // `bad_request`, `unknown_backend`…
            _ => 400,
        }
    }

    fn body(&self) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        json!({ "error": error })
    }
}

pub type HubReply = Result<Value, HubFault>;

/// Un pedido para Atic Code: qué ruta, sus parámetros y por dónde contestar.
pub struct HubCall {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub body: Value,
    pub reply: mpsc::Sender<HubReply>,
}

/// El servidor encendido. Al soltarlo (se cierra Atic Code) deja de atender y borra
/// `hub.json` si es de este proceso.
pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        remove_hub_file();
        // Despierta al hilo que acepta conexiones para que vea la marca y termine.
        let address = std::net::SocketAddr::from(([127u8, 0, 0, 1], self.port));
        let _ = TcpStream::connect_timeout(&address, Duration::from_millis(200));
    }
}

/// Un token que nadie adivina: dos hashers con las llaves al azar del sistema.
fn new_token() -> String {
    let (a, b) = (RandomState::new().build_hasher().finish(), RandomState::new().build_hasher().finish());
    format!("{a:016x}{b:016x}")
}

fn hub_path() -> Option<PathBuf> {
    atic_core::AppDirs::new().ok().map(|dirs| dirs.hub_path())
}

fn write_hub_file(port: u16, token: &str) {
    let Some(path) = hub_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = json!({ "port": port, "token": token, "pid": std::process::id() }).to_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let written = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(error) = written {
            tracing::warn!(%error, "atic code: no se pudo escribir hub.json");
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = std::fs::write(&path, text) {
        tracing::warn!(%error, "atic code: no se pudo escribir hub.json");
    }
}

/// Borra `hub.json` si es de este proceso (no el de otra pill).
fn remove_hub_file() {
    let Some(path) = hub_path() else { return };
    let ours = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).is_some_and(|v| v["pid"] == json!(std::process::id()));
    if ours {
        let _ = std::fs::remove_file(path);
    }
}

/// Enciende el hub: escucha en un puerto libre de localhost, escribe `hub.json` y
/// devuelve el canal por donde llegan los pedidos. `None` si no pudo escuchar.
pub fn start() -> Option<(Server, UnboundedReceiver<HubCall>)> {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, "atic code: no se pudo abrir el hub para delegar");
            return None;
        }
    };
    let port = listener.local_addr().ok()?.port();
    let token = new_token();
    let stop = Arc::new(AtomicBool::new(false));
    let (calls, incoming) = unbounded();
    let (thread_token, thread_stop) = (token.clone(), stop.clone());
    let spawned = std::thread::Builder::new().name("code-hub".into()).spawn(move || {
        for stream in listener.incoming() {
            if thread_stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let (token, calls) = (thread_token.clone(), calls.clone());
            // Una conexión por hilo: un `wait` largo no frena a los demás.
            let _ = std::thread::Builder::new().name("code-hub-call".into()).spawn(move || serve(stream, &token, &calls));
        }
    });
    if let Err(error) = spawned {
        tracing::warn!(%error, "atic code: no se pudo lanzar el hilo del hub");
        return None;
    }
    write_hub_file(port, &token);
    tracing::info!(port, "atic code: hub para delegar encendido");
    Some((Server { port, stop }, incoming))
}

// --- HTTP ------------------------------------------------------------------------------

/// Un pedido HTTP ya leído.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Los nombres van en minúscula.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Una línea terminada en `\n` (sin `\r\n`), con tope.
fn read_line(reader: &mut impl BufRead) -> Result<String, String> {
    let mut line = Vec::new();
    let read = reader.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', &mut line).map_err(|e| format!("no se pudo leer el pedido: {e}"))?;
    if read == 0 {
        return Err("conexión cerrada".into());
    }
    if line.len() > MAX_LINE {
        return Err("línea demasiado larga".into());
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| "el pedido no es UTF-8".to_string())
}

/// `%XX` y `+` de un parámetro de la URL.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match (bytes[i], hex) {
            (_, Some(byte)) => {
                out.push(byte);
                i += 3;
                continue;
            }
            (b'+', None) => out.push(b' '),
            (byte, None) => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1&b=dos` → mapa.
pub fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(name), percent_decode(value))
        })
        .collect()
}

/// Lee un pedido HTTP/1.1: la línea, las cabeceras y el cuerpo por `Content-Length`.
pub fn read_request(reader: &mut impl BufRead) -> Result<Request, String> {
    let line = read_line(reader)?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err("línea de pedido inválida".into());
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = HashMap::new();
    loop {
        let header = read_line(reader)?;
        if header.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err("demasiadas cabeceras".into());
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length = match headers.get("content-length") {
        Some(value) => value.parse::<usize>().map_err(|_| "Content-Length inválido".to_string())?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err("el cuerpo es demasiado grande".into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| format!("el cuerpo llegó incompleto: {e}"))?;
    Ok(Request { method: method.to_ascii_uppercase(), path: path.to_string(), query: parse_query(query), headers, body })
}

/// ¿Trae `Authorization: Bearer <token>`?
pub fn authorized(request: &Request, token: &str) -> bool {
    request.headers.get("authorization").and_then(|v| v.strip_prefix("Bearer ")).is_some_and(|given| given.trim() == token)
}

/// Las rutas que se atienden: `Ok(true)` si la atiende Atic Code, `Ok(false)` si la
/// contesta el transporte (`health`).
pub fn route(method: &str, path: &str) -> Result<bool, HubFault> {
    const GETS: [&str; 4] = ["/v1/health", "/v1/agents", "/v1/models", "/v1/sessions"];
    const POSTS: [&str; 9] = ["/v1/delegate", "/v1/prompt", "/v1/wait", "/v1/cancel", "/v1/close", "/v1/permission", "/v1/spawn", "/v1/rename", "/v1/health"];
    let known = GETS.contains(&path) || POSTS.contains(&path);
    if !known {
        return Err(HubFault::new("not_found", format!("no existe la ruta {path}")));
    }
    let allowed = match method {
        "GET" => GETS.contains(&path),
        "POST" => POSTS.contains(&path),
        _ => false,
    };
    if !allowed {
        return Err(HubFault::new("method_not_allowed", format!("{method} no vale para {path}")));
    }
    Ok(path != "/v1/health")
}

/// Cuánto puede tardar Atic Code en contestar: lo que se espera al hijo y un margen.
pub fn answer_limit(path: &str, body: &Value) -> Duration {
    match path {
        "/v1/delegate" | "/v1/prompt" | "/v1/wait" => Duration::from_secs(wait_seconds(body) + WAIT_MARGIN),
        _ => QUICK_LIMIT,
    }
}

/// El `waitS` pedido, con su valor por defecto y su tope.
pub fn wait_seconds(body: &Value) -> u64 {
    body.get("waitS").and_then(Value::as_u64).unwrap_or(WAIT_DEFAULT).clamp(1, WAIT_MAX)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

/// La respuesta entera, lista para escribir.
pub fn response(reply: &HubReply) -> Vec<u8> {
    let (status, body) = match reply {
        Ok(value) => (200, value.to_string()),
        Err(fault) => (fault.status(), fault.body().to_string()),
    };
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Una conexión: lee el pedido, lo pasa a Atic Code y escribe lo que conteste.
fn serve(stream: TcpStream, token: &str, calls: &UnboundedSender<HubCall>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(&stream);
    let reply = match read_request(&mut reader) {
        Err(error) if error == "conexión cerrada" => return,
        Err(error) => Err(HubFault::new("bad_request", error)),
        Ok(request) => answer(request, token, calls),
    };
    let _ = (&stream).write_all(&response(&reply));
    let _ = (&stream).flush();
}

fn answer(request: Request, token: &str, calls: &UnboundedSender<HubCall>) -> HubReply {
    if !authorized(&request, token) {
        return Err(HubFault::new("unauthorized", "el token no es el de esta sesión de Atic Code"));
    }
    if !route(&request.method, &request.path)? {
        return Ok(json!({ "ok": true }));
    }
    let body = if request.body.iter().all(u8::is_ascii_whitespace) {
        json!({})
    } else {
        serde_json::from_slice::<Value>(&request.body).map_err(|e| HubFault::new("bad_request", format!("el cuerpo no es JSON: {e}")))?
    };
    let limit = answer_limit(&request.path, &body);
    let (reply, wait) = mpsc::channel();
    let call = HubCall { method: request.method, path: request.path, query: request.query, body, reply };
    if calls.unbounded_send(call).is_err() {
        return Err(HubFault::new("code_closed", "Atic Code se cerró: ábrelo para delegar"));
    }
    wait.recv_timeout(limit).unwrap_or_else(|_| Err(HubFault::new("internal", "Atic Code no respondió a tiempo")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Request, String> {
        read_request(&mut std::io::Cursor::new(text.as_bytes().to_vec()))
    }

    #[test]
    fn un_pedido_http_se_lee_entero() {
        let body = r#"{"text":"hola"}"#;
        let raw = format!("POST /v1/delegate HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer abc\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let request = read(&raw).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/delegate");
        assert_eq!(request.headers["content-type"], "application/json");
        assert_eq!(request.body, body.as_bytes());
        assert!(authorized(&request, "abc"));
        assert!(!authorized(&request, "otro"));
    }

    #[test]
    fn un_get_trae_su_consulta_y_no_tiene_cuerpo() {
        let request = read("GET /v1/models?backend=codex&x=a%20b+c HTTP/1.1\r\nauthorization: Bearer t\r\n\r\n").unwrap();
        assert_eq!(request.path, "/v1/models");
        assert_eq!(request.query["backend"], "codex");
        assert_eq!(request.query["x"], "a b c");
        assert!(request.body.is_empty());
        assert!(authorized(&request, "t"));
    }

    #[test]
    fn los_pedidos_rotos_se_rechazan() {
        assert!(read("").is_err());
        assert!(read("GET\r\n\r\n").is_err());
        assert!(read("POST /v1/wait HTTP/1.1\r\nContent-Length: diez\r\n\r\n").is_err());
        // El cuerpo anuncia más de lo que llega.
        assert!(read("POST /v1/wait HTTP/1.1\r\nContent-Length: 10\r\n\r\n{}").is_err());
        let huge = format!("POST /v1/wait HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        assert!(read(&huge).is_err());
        // Sin token, o con otro esquema, no pasa.
        let request = read("GET /v1/agents HTTP/1.1\r\nAuthorization: Basic abc\r\n\r\n").unwrap();
        assert!(!authorized(&request, "abc"));
    }

    #[test]
    fn las_rutas_conocidas_y_sus_metodos() {
        assert_eq!(route("GET", "/v1/health"), Ok(false));
        assert_eq!(route("GET", "/v1/agents"), Ok(true));
        assert_eq!(route("GET", "/v1/sessions"), Ok(true));
        assert_eq!(route("POST", "/v1/delegate"), Ok(true));
        assert_eq!(route("POST", "/v1/permission"), Ok(true));
        assert_eq!(route("GET", "/v1/nada").unwrap_err().code, "not_found");
        assert_eq!(route("GET", "/v1/delegate").unwrap_err().code, "method_not_allowed");
        assert_eq!(route("DELETE", "/v1/agents").unwrap_err().status(), 405);
    }

    #[test]
    fn la_espera_tiene_tope_y_margen() {
        assert_eq!(wait_seconds(&json!({})), WAIT_DEFAULT);
        assert_eq!(wait_seconds(&json!({ "waitS": 90 })), 90);
        assert_eq!(wait_seconds(&json!({ "waitS": 99999 })), WAIT_MAX);
        assert_eq!(answer_limit("/v1/wait", &json!({ "waitS": 60 })), Duration::from_secs(60 + WAIT_MARGIN));
        assert_eq!(answer_limit("/v1/cancel", &json!({ "waitS": 60 })), QUICK_LIMIT);
    }

    #[test]
    fn la_respuesta_lleva_largo_y_cierra() {
        let ok = String::from_utf8(response(&Ok(json!({ "ok": true })))).unwrap();
        assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(ok.contains("Content-Type: application/json\r\n"));
        assert!(ok.contains("Content-Length: 11\r\n"));
        assert!(ok.contains("Connection: close\r\n"));
        assert!(ok.ends_with("\r\n\r\n{\"ok\":true}"));
        let fault = String::from_utf8(response(&Err(HubFault::new("too_many_children", "basta")))).unwrap();
        assert!(fault.starts_with("HTTP/1.1 429 "));
        assert!(fault.ends_with(r#"{"error":{"code":"too_many_children","message":"basta"}}"#));
    }

    #[test]
    fn el_token_es_largo_y_distinto() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
