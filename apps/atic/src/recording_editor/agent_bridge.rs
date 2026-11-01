//! El puente entre los agentes de IA y el editor: un servidor TCP en localhost que traduce
//! los pedidos de `atic-mcp` (las herramientas `atic_editor_*`) a llamadas sobre el editor
//! abierto. Contrato en `docs/EDITOR_AGENTE.md`.
//!
//! - **Transporte** (sin GPUI): el primer editor que se abre levanta el servidor y escribe
//!   `editor-bridge.json` (puerto y token, junto a `hub.json`); el último que se cierra lo borra.
//!   Un pedido por conexión: una línea JSON de ida, una de vuelta.
//! - **Editor** (hilo de GPUI): cada editor se registra y recibe sus `Call` por un canal; las
//!   resuelve con `RecordingEditor::agent_call`. Lo que tarda (cuadros, transcripción, exportar)
//!   responde cuando termina, sin trabar la ventana, y siempre antes de 45 s.
//!
//! Lo que un agente cambia entra como **un solo paso de deshacer** y deja un aviso con «Deshacer».

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use gpui::{Context, Window};
use serde_json::{json, Value};

use super::captions::Transcript;
use super::{Item, NoticeAction, RecordingEditor, IDLE_MIN, LEVELS_PER_SEC, SPEED_IDLE};
use crate::edit_ops::{self, Effect};
use crate::i18n::tf;

/// Lo máximo que el puente se hace esperar (los hosts cortan las herramientas a los 60 s).
const ANSWER_LIMIT: Duration = Duration::from_secs(45);
/// El editor responde antes: si no, algo lo trabó.
const EDITOR_LIMIT: Duration = Duration::from_secs(55);
const MAX_REQUEST: u64 = 1 << 20;
const MAX_FRAMES: usize = 8;
const FRAME_HEIGHT: (u32, u32, u32) = (240, 480, 720);
const MAX_SEGMENTS: usize = 1500;

// --- Tipos ---------------------------------------------------------------------------------

pub struct Fault {
    pub code: &'static str,
    pub message: String,
}

impl Fault {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

pub struct Image {
    pub mime: &'static str,
    pub data: String,
    pub caption: String,
}

#[derive(Default)]
pub struct Answer {
    pub data: Value,
    pub images: Vec<Image>,
}

impl Answer {
    fn data(data: Value) -> Self {
        Self { data, images: Vec::new() }
    }
}

pub type Reply = Result<Answer, Fault>;

/// Un pedido para un editor: la herramienta, sus argumentos y por dónde responder.
pub struct Call {
    pub tool: String,
    pub args: Value,
    pub reply: mpsc::Sender<Reply>,
}

// --- Registro de editores y servidor ---------------------------------------------------------

struct Entry {
    id: u64,
    video: PathBuf,
    title: String,
    duration: f64,
    /// Cuándo tuvo el foco por última vez (mayor = más reciente).
    focus: u64,
    calls: UnboundedSender<Call>,
}

struct Server {
    port: u16,
    token: String,
}

struct State {
    entries: Vec<Entry>,
    next_id: u64,
    clock: u64,
    server: Option<Server>,
}

static STATE: Mutex<State> = Mutex::new(State { entries: Vec::new(), next_id: 1, clock: 0, server: None });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Un token que nadie adivina: dos hashers con las llaves al azar del sistema.
fn new_token() -> String {
    let (a, b) = (RandomState::new().build_hasher().finish(), RandomState::new().build_hasher().finish());
    format!("{a:016x}{b:016x}")
}

fn bridge_path() -> Option<PathBuf> {
    atic_core::AppDirs::new().ok().map(|dirs| dirs.hub_path().with_file_name("editor-bridge.json"))
}

fn write_bridge_file(server: &Server) {
    let Some(path) = bridge_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = json!({ "port": server.port, "token": server.token, "pid": std::process::id() }).to_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let written = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(error) = written {
            tracing::warn!(%error, "editor: no se pudo escribir editor-bridge.json");
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = std::fs::write(&path, text) {
        tracing::warn!(%error, "editor: no se pudo escribir editor-bridge.json");
    }
}

/// Borra el archivo si es de este proceso (no el de otra pill).
fn remove_bridge_file() {
    let Some(path) = bridge_path() else { return };
    let ours = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).is_some_and(|v| v["pid"] == json!(std::process::id()));
    if ours {
        let _ = std::fs::remove_file(path);
    }
}

fn ensure_server(state: &mut State) -> bool {
    if state.server.is_some() {
        return true;
    }
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, "editor: no se pudo abrir el puente para agentes");
            return false;
        }
    };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return false };
    let token = new_token();
    let for_thread = token.clone();
    let spawned = std::thread::Builder::new().name("editor-bridge".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let token = for_thread.clone();
            // Una conexión por hilo: un pedido lento (cuadros, exportar) no frena a los demás.
            let _ = std::thread::Builder::new().name("editor-bridge-call".into()).spawn(move || serve(stream, &token));
        }
    });
    if spawned.is_err() {
        return false;
    }
    state.server = Some(Server { port, token });
    true
}

/// El editor en el registro: al soltarlo (cerrar la ventana) se da de baja.
pub struct Link {
    id: u64,
}

impl Drop for Link {
    fn drop(&mut self) {
        let mut state = state();
        state.entries.retain(|e| e.id != self.id);
        if state.entries.is_empty() {
            remove_bridge_file();
        }
    }
}

/// Registra un editor y lo pone a atender sus pedidos en el hilo de GPUI.
pub fn attach(video: &Path, title: &str, duration: f64, window: &mut Window, cx: &mut Context<RecordingEditor>) -> Option<Link> {
    let (calls, mut incoming): (UnboundedSender<Call>, UnboundedReceiver<Call>) = unbounded();
    let id = {
        let mut state = state();
        if !ensure_server(&mut state) {
            return None;
        }
        let id = state.next_id;
        state.next_id += 1;
        state.clock += 1;
        let focus = state.clock;
        state.entries.push(Entry { id, video: video.to_path_buf(), title: title.to_string(), duration, focus, calls });
        if let Some(server) = &state.server {
            write_bridge_file(server);
        }
        id
    };
    cx.spawn_in(window, async move |view, cx| {
        while let Some(call) = incoming.next().await {
            if view.update_in(cx, |view, window, cx| view.agent_call(call, window, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    cx.observe_window_activation(window, move |_, window, _| {
        if window.is_window_active() {
            let mut state = state();
            state.clock += 1;
            let clock = state.clock;
            if let Some(entry) = state.entries.iter_mut().find(|e| e.id == id) {
                entry.focus = clock;
            }
        }
    })
    .detach();
    Some(Link { id })
}

/// Una conexión: lee la línea, la resuelve y escribe la respuesta.
fn serve(stream: TcpStream, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    let read = BufReader::new((&stream).take(MAX_REQUEST)).read_line(&mut line);
    let (id, result) = match read {
        Ok(n) if n > 0 => match serde_json::from_str::<Value>(&line) {
            Ok(request) => {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                let ok = request.get("token").and_then(Value::as_str).is_some_and(|t| t == token);
                let result = if ok { route(&request) } else { Err(Fault::new("bad_token", "el token no es el de esta sesión de Atic; abre el editor de nuevo")) };
                (id, result)
            }
            Err(error) => (Value::Null, Err(Fault::new("bad_args", format!("el pedido no es JSON: {error}")))),
        },
        _ => return,
    };
    let response = match result {
        Ok(answer) => {
            let images: Vec<Value> = answer.images.into_iter().map(|i| json!({ "mime": i.mime, "data": i.data, "caption": i.caption })).collect();
            json!({ "id": id, "ok": true, "data": answer.data, "images": images })
        }
        Err(fault) => json!({ "id": id, "ok": false, "error": { "code": fault.code, "message": fault.message } }),
    };
    let mut out = response.to_string();
    out.push('\n');
    let _ = (&stream).write_all(out.as_bytes());
}

fn route(request: &Value) -> Reply {
    let tool = request.get("tool").and_then(Value::as_str).unwrap_or_default().to_string();
    let args = request.get("args").cloned().unwrap_or_else(|| json!({}));
    if tool == "list" {
        let state = state();
        let newest = state.entries.iter().map(|e| e.focus).max();
        let editors: Vec<Value> = state
            .entries
            .iter()
            .map(|e| json!({ "video": e.video, "title": e.title, "duration": e.duration, "focused": Some(e.focus) == newest }))
            .collect();
        return Ok(Answer::data(json!({ "editors": editors })));
    }
    let (calls, video) = {
        let state = state();
        let wanted = args.get("video").and_then(Value::as_str).filter(|v| !v.is_empty());
        let entry = match wanted {
            Some(wanted) => state.entries.iter().find(|e| e.video == Path::new(wanted) || e.title == wanted),
            None => state.entries.iter().max_by_key(|e| e.focus),
        };
        match entry {
            Some(entry) => (entry.calls.clone(), entry.video.clone()),
            None if wanted.is_some() => {
                let open: Vec<String> = state.entries.iter().map(|e| e.video.display().to_string()).collect();
                return Err(Fault::new("no_editor", format!("esa grabación no está abierta en el editor; abiertas: {}", open.join(", "))));
            }
            None => return Err(Fault::new("no_editor", "Abre una grabación en el editor de Atic para poder editarla.")),
        }
    };
    let (reply, wait) = mpsc::channel();
    if calls.unbounded_send(Call { tool, args, reply }).is_err() {
        return Err(Fault::new("no_editor", format!("el editor de {} se cerró", video.display())));
    }
    wait.recv_timeout(EDITOR_LIMIT).unwrap_or_else(|_| Err(Fault::new("internal", "el editor no respondió a tiempo")))
}

// --- Lo que hace el editor con cada herramienta ----------------------------------------------

fn bad_args(message: impl Into<String>) -> Fault {
    Fault::new("bad_args", message)
}

fn arg_f64(args: &Value, key: &str) -> Result<Option<f64>, Fault> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| bad_args(format!("`{key}` debe ser un número"))),
    }
}

/// Los segundos que se piden en `frames`: `times`, o `count` repartidos entre `start` y `end`.
fn frame_times(args: &Value, duration: f64) -> Result<Vec<f64>, Fault> {
    let valid = |t: f64| {
        if (0.0..=duration + 0.05).contains(&t) {
            Ok(t.min(duration))
        } else {
            Err(bad_args(format!("el tiempo {t} está fuera del video (0 a {duration:.2} s)")))
        }
    };
    if let Some(times) = args.get("times").and_then(Value::as_array) {
        if times.is_empty() || times.len() > MAX_FRAMES {
            return Err(bad_args(format!("`times` lleva de 1 a {MAX_FRAMES} tiempos")));
        }
        return times.iter().map(|t| t.as_f64().ok_or_else(|| bad_args("`times` son números (segundos)")).and_then(valid)).collect();
    }
    let start = arg_f64(args, "start")?.unwrap_or(0.0);
    let end = arg_f64(args, "end")?.unwrap_or(duration);
    let count = args.get("count").and_then(Value::as_u64).unwrap_or(6) as usize;
    if count == 0 || count > MAX_FRAMES {
        return Err(bad_args(format!("`count` va de 1 a {MAX_FRAMES}")));
    }
    let (start, end) = (valid(start)?, valid(end)?);
    if end <= start {
        return Err(bad_args("`end` debe ser mayor que `start`"));
    }
    Ok((0..count).map(|i| start + (end - start) * (i as f64 + 0.5) / count as f64).collect())
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect::<String>() + "…"
    }
}

impl RecordingEditor {
    fn agent_ctx(&self) -> edit_ops::Ctx<'_> {
        edit_ops::Ctx {
            duration: self.duration,
            track: self.track.as_ref(),
            levels: &self.levels,
            levels_per_sec: LEVELS_PER_SEC,
            levels_ready: self.levels_ready,
            idle_min: IDLE_MIN,
            idle_rate: SPEED_IDLE,
        }
    }

    /// El proyecto más lo que solo sabe el editor: cabezal, selección, deshacer, exportación y transcripción.
    fn agent_state(&self) -> Value {
        let mut state = edit_ops::describe(&self.project, &self.agent_ctx());
        let (w, h) = self.player.size();
        state["video"] = json!(self.path);
        state["title"] = json!(self.title);
        state["size"] = json!({ "width": w, "height": h });
        state["playhead"] = json!(self.now());
        state["playing"] = json!(self.player.playing());
        state["selected"] = match self.selected {
            Some(Item::Zoom(i)) => json!({ "kind": "zoom", "index": i }),
            Some(Item::Text(i)) => json!({ "kind": "text", "index": i }),
            Some(Item::Cut(i)) => json!({ "kind": "cut", "index": i }),
            Some(Item::Speed(i)) => json!({ "kind": "speed", "index": i }),
            Some(Item::Blur(i)) => json!({ "kind": "blur", "index": i }),
            None => Value::Null,
        };
        state["history"] = json!({ "can_undo": self.history.can_undo(), "can_redo": self.history.can_redo() });
        state["export_job"] = self.export_status();
        state["transcript"] = match self.transcript_status() {
            Transcript::Idle => json!({ "status": "idle" }),
            Transcript::Running(progress) => json!({ "status": "running", "progress": progress }),
            Transcript::Ready(segments) => json!({ "status": "ready", "segments": segments.len() }),
            Transcript::Failed(error) => json!({ "status": "failed", "error": error }),
        };
        state
    }

    pub(super) fn agent_call(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        let Call { tool, args, reply } = call;
        match tool.as_str() {
            "state" => {
                let _ = reply.send(Ok(Answer::data(self.agent_state())));
            }
            "activity" => {
                let _ = reply.send(Ok(Answer::data(edit_ops::activity(&self.project, &self.agent_ctx()))));
            }
            "edit" => {
                let result = self.agent_edit(&args, window, cx);
                let _ = reply.send(result);
            }
            "frames" => self.agent_frames(&args, reply),
            "transcript" => self.agent_transcript(reply, cx),
            "export" => self.agent_export(&args, reply, cx),
            other => {
                let _ = reply.send(Err(bad_args(format!("herramienta `{other}` desconocida"))));
            }
        }
    }

    /// Aplica un lote de operaciones: todo o nada, y un solo paso de deshacer.
    fn agent_edit(&mut self, args: &Value, window: &mut Window, cx: &mut Context<Self>) -> Reply {
        if self.export_busy() {
            return Err(Fault::new("busy", "hay una exportación en curso; espera a que termine (atic_editor_export) antes de editar"));
        }
        let ops = args.get("ops").and_then(Value::as_array).ok_or_else(|| bad_args("falta `ops`: una lista de operaciones"))?;
        let ctx = edit_ops::Ctx {
            duration: self.duration,
            track: self.track.as_ref(),
            levels: &self.levels,
            levels_per_sec: LEVELS_PER_SEC,
            levels_ready: self.levels_ready,
            idle_min: IDLE_MIN,
            idle_rate: SPEED_IDLE,
        };
        let outcome = edit_ops::apply(&mut self.project, ops, &ctx).map_err(|e| Fault::new("bad_op", e.text()))?;
        let changed = !self.history.unchanged(&self.project);
        if changed {
            self.changed(cx);
            self.select(None, window, cx);
            let count = outcome.results.len();
            self.say_with(tf("pill.editor.agentApplied", &[("count", &count)]), false, Some(NoticeAction::Undo), cx);
        }
        let mut notes = outcome.warnings;
        for effect in outcome.effects {
            match effect {
                Effect::Seek(time) => self.seek(time, cx),
                Effect::Play => {
                    if !self.player.playing() {
                        self.toggle_play(cx);
                    }
                }
                Effect::Pause => {
                    if self.player.playing() {
                        self.toggle_play(cx);
                    }
                }
                Effect::Undo => {
                    if self.history.can_undo() {
                        self.undo(window, cx);
                    } else {
                        notes.push("no hay nada que deshacer".into());
                    }
                }
                Effect::Redo => {
                    if self.history.can_redo() {
                        self.redo(window, cx);
                    } else {
                        notes.push("no hay nada que rehacer".into());
                    }
                }
                Effect::GenerateCaptions => {
                    self.start_captions(cx);
                    notes.push("generando subtítulos en segundo plano; mira `state.transcript`".into());
                }
            }
        }
        Ok(Answer::data(json!({ "applied": outcome.results, "notes": notes, "changed": changed, "state": self.agent_state() })))
    }

    /// Los cuadros pedidos, del archivo que el usuario ve (con su cursor), codificados en otro hilo.
    fn agent_frames(&mut self, args: &Value, reply: mpsc::Sender<Reply>) {
        let times = match frame_times(args, self.duration) {
            Ok(times) => times,
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let height = match arg_f64(args, "height") {
            Ok(h) => h.map_or(FRAME_HEIGHT.1, |h| (h as u32).clamp(FRAME_HEIGHT.0, FRAME_HEIGHT.2)),
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        let source = if self.path.exists() { self.path.clone() } else { self.source.clone() };
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| atic_capture::stream::frames_at(&source, &times, height))
                .unwrap_or_else(|_| Err("el lector de video falló".to_string()))
                .map_err(|e| Fault::new("internal", format!("no se pudieron leer los cuadros: {e}")))
                .and_then(|frames| {
                    if frames.is_empty() {
                        return Err(Fault::new("internal", "no se pudo leer ningún cuadro"));
                    }
                    let mut images = Vec::new();
                    for (time, frame) in frames {
                        let jpeg = frame.to_jpeg(72).map_err(|e| Fault::new("internal", format!("no se pudo codificar el cuadro: {e}")))?;
                        images.push(Image {
                            mime: "image/jpeg",
                            data: base64::engine::general_purpose::STANDARD.encode(jpeg),
                            caption: format!("{} s ({}×{})", super::clock_tenths(time), frame.width(), frame.height()),
                        });
                    }
                    let times: Vec<f64> = times.iter().map(|t| (t * 100.0).round() / 100.0).collect();
                    Ok(Answer {
                        data: json!({ "times": times, "composed": false, "note": "cuadros del video original, sin zoom, textos ni fondo del editor" }),
                        images,
                    })
                });
            let _ = reply.send(result);
        });
    }

    fn agent_transcript(&mut self, reply: mpsc::Sender<Reply>, cx: &mut Context<Self>) {
        if matches!(self.transcript_status(), Transcript::Idle) {
            self.start_transcript(cx);
        }
        let deadline = Instant::now() + ANSWER_LIMIT;
        cx.spawn(async move |view, cx| loop {
            let status = view.update(cx, |view, _| view.transcript_status());
            let answer = match status {
                Err(_) => Err(Fault::new("no_editor", "el editor se cerró")),
                Ok(Transcript::Ready(segments)) => {
                    let total = segments.len();
                    let list: Vec<Value> = segments.iter().take(MAX_SEGMENTS).map(|(a, b, text)| json!({ "start": (a * 100.0).round() / 100.0, "end": (b * 100.0).round() / 100.0, "text": clip(text, 300) })).collect();
                    let note = if total == 0 { "no se detectó voz en el audio" } else { "" };
                    Ok(Answer::data(json!({ "status": "ready", "count": total, "truncated": total > MAX_SEGMENTS, "note": note, "segments": list })))
                }
                Ok(Transcript::Failed(error)) => Ok(Answer::data(json!({ "status": "failed", "error": error }))),
                Ok(Transcript::Idle) => Ok(Answer::data(json!({ "status": "idle" }))),
                Ok(Transcript::Running(progress)) if Instant::now() >= deadline => {
                    Ok(Answer::data(json!({ "status": "running", "progress": progress, "note": "sigue transcribiendo; vuelve a pedirla en unos segundos" })))
                }
                Ok(Transcript::Running(_)) => {
                    cx.background_executor().timer(Duration::from_millis(300)).await;
                    continue;
                }
            };
            let _ = reply.send(answer);
            break;
        })
        .detach();
    }

    /// Exporta con los ajustes del proyecto (o devuelve la exportación que ya está hecha).
    fn agent_export(&mut self, args: &Value, reply: mpsc::Sender<Reply>, cx: &mut Context<Self>) {
        let wait = match arg_f64(args, "wait") {
            Ok(w) => w.unwrap_or(30.0).clamp(0.0, 40.0),
            Err(fault) => {
                let _ = reply.send(Err(fault));
                return;
            }
        };
        self.agent_export_start(cx);
        let deadline = Instant::now() + Duration::from_secs_f64(wait);
        cx.spawn(async move |view, cx| loop {
            let status = view.update(cx, |view, _| view.export_status());
            let running = matches!(&status, Ok(s) if s["status"] == "running");
            if running && Instant::now() < deadline {
                cx.background_executor().timer(Duration::from_millis(300)).await;
                continue;
            }
            let _ = reply.send(status.map(Answer::data).map_err(|_| Fault::new("no_editor", "el editor se cerró")));
            break;
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_cuadros_se_reparten_o_se_piden_por_tiempo() {
        let times = frame_times(&json!({ "count": 4 }), 40.0).ok().unwrap();
        assert_eq!(times, vec![5.0, 15.0, 25.0, 35.0]);
        let times = frame_times(&json!({ "start": 10, "end": 20, "count": 2 }), 40.0).ok().unwrap();
        assert_eq!(times, vec![12.5, 17.5]);
        assert_eq!(frame_times(&json!({ "times": [0, 12.3] }), 40.0).ok().unwrap(), vec![0.0, 12.3]);
    }

    #[test]
    fn los_cuadros_rechazan_lo_que_se_sale() {
        assert!(frame_times(&json!({ "times": [41] }), 40.0).is_err());
        assert!(frame_times(&json!({ "times": [] }), 40.0).is_err());
        assert!(frame_times(&json!({ "count": 9 }), 40.0).is_err());
        assert!(frame_times(&json!({ "start": 20, "end": 10 }), 40.0).is_err());
        // Pasarse unos centésimos del final se perdona.
        assert_eq!(frame_times(&json!({ "times": [40.03] }), 40.0).ok().unwrap(), vec![40.0]);
    }

    #[test]
    fn el_token_es_largo_y_distinto() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn un_pedido_sin_editor_abierto_dice_que_lo_abras() {
        // Sin editores registrados (este test no abre ventanas), `route` contesta `no_editor`.
        let fault = route(&json!({ "tool": "state", "args": {} })).err().unwrap();
        assert_eq!(fault.code, "no_editor");
        assert!(fault.message.contains("Abre una grabación"));
        // `list` sin editores es una lista vacía.
        let list = route(&json!({ "tool": "list" })).ok().unwrap();
        assert_eq!(list.data["editors"], json!([]));
    }
}
