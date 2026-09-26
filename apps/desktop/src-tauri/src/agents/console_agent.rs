//! Subagentes en su consola de verdad.
//!
//! Lo que el hub abre por MCP era un chat armado por Atic sobre el modo
//! estructurado del CLI (`stream-json`, `app-server`): se miraba y no se le
//! podía escribir. Acá el hijo es el mismo TUI que abrirías tú —`claude`,
//! `codex`— en una consola de la pizarra, y le escribes como a cualquiera.
//!
//! El padre igual necesita saber cuándo terminó el turno y qué contestó. Eso no
//! sale de la pantalla (son bytes de dibujo) sino del transcript que el CLI
//! escribe en disco, el mismo que ya leen los vigilantes de la pill: el JSONL
//! de Claude —con `--session-id` se sabe cuál es— y el rollout de Codex, que se
//! reconoce por carpeta y hora de arranque. De ahí salen `TurnStart` /
//! `TurnEnd` y el texto final, y el `TurnWatch` del hub no nota la diferencia.
//!
//! Solo Claude Code y Codex: son los que dejan un transcript con fin de turno
//! confiable. El resto sigue por el camino estructurado.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use atic_core::MutexExt;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::model::{AgentDelta, Item, ItemKind, Role, ThreadPatch, TurnStatus};
use super::watch_claude::Tail;
use super::{AgentSession, StartOptions};

/// Cada cuánto se lee el transcript.
const POLL: Duration = Duration::from_millis(500);
/// Si nadie escribe al arrancar, la consola se abre sola tras esta espera.
/// `delegate` manda la tarea al tiro y así viaja como argumento del CLI.
const SPAWN_WITHOUT_PROMPT_AFTER: Duration = Duration::from_millis(400);
/// Un TUI recién abierto está listo cuando deja de dibujar un rato.
const READY_MIN_AGE: Duration = Duration::from_millis(2500);
const READY_QUIET: Duration = Duration::from_millis(800);
const READY_GIVE_UP: Duration = Duration::from_secs(45);
/// Entre pegar el texto y el Enter: sin respiro, algunos TUI toman el `\r`
/// como parte del pegado.
const SUBMIT_DELAY: Duration = Duration::from_millis(400);
/// Codex junta lo que llega en ráfaga como un pegado y ahí el Enter es un
/// salto de línea, no «enviar»: la tarea quedaba escrita sin mandarse. Si el
/// transcript no la ve llegar, se repite el Enter —uno de más sobre la
/// entrada vacía no hace nada—.
const SUBMIT_RETRY_AFTER: Duration = Duration::from_secs(2);
const SUBMIT_RETRIES: usize = 3;
/// Tras Esc, cuánto se espera a que el transcript cierre el turno solo.
const INTERRUPT_GRACE: Duration = Duration::from_secs(4);

/// Rollouts de Codex ya tomados por algún subagente: dos hijos Codex en la
/// misma carpeta no pueden quedarse con el mismo.
static CLAIMED_ROLLOUTS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

pub(crate) fn supported(backend: &str) -> bool {
    matches!(backend, "claude-code" | "codex")
}

/// El nombre del CLI y sus argumentos, sin la tarea.
pub(crate) fn cli_and_args(
    backend: &str,
    options: &StartOptions,
) -> Option<(&'static str, Vec<String>)> {
    match backend {
        "claude-code" => Some(("claude", claude_args(options))),
        "codex" => Some(("codex", codex_args(options))),
        _ => None,
    }
}

/// El modo de permisos para el TUI. `manual` es del modo estructurado (ahí
/// pregunta por stdio); en la consola el equivalente es `default`.
fn claude_permission_mode(mode: Option<&str>) -> &'static str {
    match mode {
        Some("bypassPermissions") => "bypassPermissions",
        Some("acceptEdits") => "acceptEdits",
        Some("plan") => "plan",
        Some("dontAsk") => "dontAsk",
        _ => "default",
    }
}

/// `--mcp-config` y `--add-dir` aceptan varios valores y se comen lo que
/// venga detrás hasta la próxima opción: la tarea, al final, terminaba leída
/// como otro archivo de config. Van primero, y después opciones de un valor.
fn claude_args(options: &StartOptions) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(mcp) = &options.mcp_config {
        args.push("--mcp-config".into());
        args.push(mcp.clone());
    }
    for dir in &options.add_dirs {
        args.push("--add-dir".into());
        args.push(dir.clone());
    }
    if let Some(id) = &options.session_id {
        args.push("--session-id".into());
        args.push(id.clone());
    }
    if let Some(model) = &options.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    args.push("--permission-mode".into());
    args.push(claude_permission_mode(options.permission_mode.as_deref()).into());
    args
}

fn codex_args(options: &StartOptions) -> Vec<String> {
    let mut args = Vec::new();
    if options.permission_mode.as_deref() == Some("bypassPermissions") {
        args.push("--dangerously-bypass-approvals-and-sandbox".into());
    }
    if let Some(model) = &options.model {
        args.push("-m".into());
        args.push(model.clone());
    }
    if let Some(effort) = &options.effort {
        args.push("-c".into());
        args.push(format!("model_reasoning_effort={}", json!(effort)));
    }
    args.extend(super::codex::overrides_mcp(
        options.atic_mcp.as_ref(),
        &options.mcp_servers,
    ));
    args
}

/// La tarea como último argumento. Un texto que empieza con `-` se leería como
/// opción: un espacio adelante no le cambia el sentido al agente.
fn prompt_arg(prompt: &str) -> String {
    if prompt.starts_with('-') {
        format!(" {prompt}")
    } else {
        prompt.to_string()
    }
}

/// Lo que se escribe en la consola para mandar `text`, sin el Enter.
///
/// Con pegado entre corchetes el TUI recibe el texto entero, saltos incluidos,
/// como un pegado. Sin él, cada salto sería un Enter y mandaría la tarea a
/// medias: se aplanan a espacios.
pub(crate) fn frame_input(text: &str, bracketed: bool) -> String {
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~")
    } else {
        text.split(['\r', '\n'])
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Qué dice una línea del transcript sobre el turno.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct LineEffect {
    /// Alguien escribió en la consola: empieza un turno (si no había uno).
    pub begins: bool,
    /// Texto de la respuesta que se suma al turno.
    pub append: Option<String>,
    /// El texto final completo, cuando la línea lo trae entero.
    pub replace: Option<String>,
    /// El turno terminó, y cómo.
    pub ends: Option<TurnStatus>,
}

fn claude_text(v: &Value) -> Option<String> {
    let blocks = v.pointer("/message/content")?.as_array()?;
    let text = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!text.is_empty()).then_some(text)
}

pub(crate) fn claude_effect(v: &Value) -> LineEffect {
    use super::watch_claude::LineKind;
    let mut effect = LineEffect::default();
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return effect;
    }
    if v.get("type").and_then(Value::as_str) == Some("assistant") {
        effect.append = claude_text(v);
    }
    match super::watch_claude::classify(v) {
        LineKind::Prompt => effect.begins = true,
        LineKind::EndTurn { .. } => effect.ends = Some(TurnStatus::Done),
        LineKind::Activity | LineKind::Ignore => {}
    }
    effect
}

pub(crate) fn codex_effect(v: &Value) -> LineEffect {
    use super::watch_codex::LineKind;
    let mut effect = LineEffect::default();
    match super::watch_codex::classify(v) {
        LineKind::Prompt => effect.begins = true,
        LineKind::EndTurn { .. } => {
            let aborted =
                v.pointer("/payload/type").and_then(Value::as_str) == Some("turn_aborted");
            effect.ends = Some(if aborted {
                TurnStatus::Cancelled
            } else {
                TurnStatus::Done
            });
            effect.replace = v
                .pointer("/payload/last_agent_message")
                .and_then(Value::as_str)
                .or_else(|| super::watch_codex::item_text(v))
                .map(str::to_string);
        }
        LineKind::Activity | LineKind::Ignore => {}
    }
    effect
}

#[derive(Default)]
struct TurnTrack {
    current: Option<String>,
    text: String,
}

struct Transcript {
    path: PathBuf,
    tail: Tail,
}

struct Shared {
    app: AppHandle,
    backend: &'static str,
    key: String,
    cwd: Option<String>,
    program: PathBuf,
    prefix: Vec<String>,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cli: &'static str,
    console: Mutex<Option<String>>,
    spawned_at: Mutex<Option<(Instant, SystemTime)>>,
    turn: Mutex<TurnTrack>,
    stopped: AtomicBool,
    /// Cuántas tareas vio llegar el transcript: así se sabe si un Enter mandó.
    prompts_seen: AtomicUsize,
    on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
}

pub(crate) struct ConsoleAgent {
    shared: Arc<Shared>,
}

/// Arranca el subagente en consola. La consola en sí se abre con la primera
/// tarea (que viaja como argumento) o, si no llega ninguna, sola al rato.
pub(crate) fn start(
    app: &AppHandle,
    backend: &str,
    options: StartOptions,
    on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
) -> Result<Box<dyn AgentSession>, String> {
    let (cli, args) =
        cli_and_args(backend, &options).ok_or_else(|| format!("{backend} no abre en consola"))?;
    let backend: &'static str = if backend == "codex" {
        "codex"
    } else {
        "claude-code"
    };
    let (program, prefix) =
        super::exe::launcher(cli).ok_or_else(|| format!("no se encontró «{cli}» en el PATH"))?;
    let key = options
        .session_id
        .clone()
        .ok_or_else(|| "falta el id de la sesión".to_string())?;
    let shared = Arc::new(Shared {
        app: app.clone(),
        backend,
        key,
        cwd: options.cwd.clone(),
        program,
        prefix,
        args,
        env: options.env.clone(),
        cli,
        console: Mutex::new(None),
        spawned_at: Mutex::new(None),
        turn: Mutex::new(TurnTrack::default()),
        stopped: AtomicBool::new(false),
        prompts_seen: AtomicUsize::new(0),
        on_delta,
    });

    let lazy = Arc::clone(&shared);
    thread::spawn(move || {
        thread::sleep(SPAWN_WITHOUT_PROMPT_AFTER);
        if let Err(message) = lazy.ensure_console(None) {
            (lazy.on_delta)(AgentDelta::Failed { message });
        }
    });
    let watch = Arc::clone(&shared);
    thread::Builder::new()
        .name(format!("console-agent-{}", shared.key))
        .spawn(move || watch.follow())
        .map_err(|e| format!("no se pudo seguir la consola: {e}"))?;

    Ok(Box::new(ConsoleAgent { shared }))
}

impl Shared {
    /// Abre la consola si todavía no existe. Devuelve si la tarea ya viajó
    /// como argumento (entonces no hay que escribirla).
    fn ensure_console(&self, prompt: Option<&str>) -> Result<bool, String> {
        let mut console = self.console.lock_or_recover();
        if console.is_some() || self.stopped.load(Ordering::SeqCst) {
            return Ok(false);
        }
        // Por `cmd /C` (un shim de npm) el argumento pasaría por el parser de
        // cmd: `%`, `^` y comillas cambiarían la tarea. Ahí se escribe.
        let as_arg = prompt.filter(|_| self.prefix.is_empty());
        let mut args = self.args.clone();
        if let Some(p) = as_arg {
            args.push(prompt_arg(p));
        }
        let id = super::console::spawn_agent_pty(
            &self.app,
            &self.program,
            &self.prefix,
            &args,
            &self.env,
            self.cwd.as_deref(),
            self.cli,
        )?;
        *self.spawned_at.lock_or_recover() = Some((Instant::now(), SystemTime::now()));
        *console = Some(id);
        drop(console);
        // Un delta cualquiera para que la vista pida los datos de la sesión y
        // se entere de la consola: sin él, un hijo sin tarea no avisaría nada.
        self.emit(AgentDelta::ThreadPatch {
            patch: ThreadPatch {
                cwd: self.cwd.clone(),
                ..Default::default()
            },
        });
        Ok(as_arg.is_some())
    }

    fn console_id(&self) -> Option<String> {
        self.console.lock_or_recover().clone()
    }

    fn emit(&self, delta: AgentDelta) {
        (self.on_delta)(delta);
    }

    fn begin_turn(&self, user_text: Option<&str>) {
        let mut turn = self.turn.lock_or_recover();
        if turn.current.is_some() {
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        turn.current = Some(id.clone());
        turn.text.clear();
        drop(turn);
        self.emit(AgentDelta::TurnStart { turn: id.clone() });
        if let Some(text) = user_text {
            self.emit(AgentDelta::ItemAdd {
                turn: id.clone(),
                item: Item::new(
                    format!("{id}-user"),
                    ItemKind::Message {
                        role: Role::User,
                        text: text.to_string(),
                        streaming: false,
                    },
                ),
            });
        }
    }

    fn end_turn(&self, status: TurnStatus) {
        let mut turn = self.turn.lock_or_recover();
        let Some(id) = turn.current.take() else {
            return;
        };
        let text = std::mem::take(&mut turn.text);
        drop(turn);
        if !text.trim().is_empty() {
            self.emit(AgentDelta::ItemAdd {
                turn: id.clone(),
                item: Item::new(
                    format!("{id}-answer"),
                    ItemKind::Message {
                        role: Role::Assistant,
                        text,
                        streaming: false,
                    },
                ),
            });
        }
        self.emit(AgentDelta::TurnEnd {
            turn: id,
            status,
            cost_usd: None,
            duration_ms: None,
        });
    }

    fn apply(&self, effect: LineEffect) {
        if effect.begins {
            self.prompts_seen.fetch_add(1, Ordering::SeqCst);
            self.begin_turn(None);
        }
        {
            let mut turn = self.turn.lock_or_recover();
            if turn.current.is_some() {
                if let Some(text) = effect.append {
                    if !turn.text.is_empty() {
                        turn.text.push_str("\n\n");
                    }
                    turn.text.push_str(&text);
                }
                if let Some(text) = effect.replace {
                    turn.text = text;
                }
            }
        }
        if let Some(status) = effect.ends {
            self.end_turn(status);
        }
    }

    /// Espera a que el TUI recién abierto deje de dibujar, y escribe.
    fn type_when_ready(&self, console: &str, text: &str) -> Result<(), String> {
        let started = self
            .spawned_at
            .lock_or_recover()
            .map(|(at, _)| at)
            .unwrap_or_else(Instant::now);
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                return Ok(());
            }
            let age = started.elapsed();
            let signal = super::console::output_signal(console);
            let bracketed = signal.is_some_and(|s| s.bracketed_paste);
            let quiet = signal.is_some_and(|s| s.last_output.elapsed() >= READY_QUIET);
            if bracketed || (age >= READY_MIN_AGE && quiet) || age >= READY_GIVE_UP {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let bracketed = super::console::output_signal(console).is_some_and(|s| s.bracketed_paste);
        let seen_before = self.prompts_seen.load(Ordering::SeqCst);
        super::console::write_input(console, &frame_input(text, bracketed))?;
        thread::sleep(SUBMIT_DELAY);
        super::console::write_input(console, "\r")?;
        for _ in 0..SUBMIT_RETRIES {
            thread::sleep(SUBMIT_RETRY_AFTER);
            let sent = self.prompts_seen.load(Ordering::SeqCst) > seen_before;
            let idle = self.turn.lock_or_recover().current.is_none();
            if sent || idle || self.stopped.load(Ordering::SeqCst) {
                return Ok(());
            }
            super::console::write_input(console, "\r")?;
        }
        Ok(())
    }

    /// El hilo que lee el transcript y cuenta los turnos.
    fn follow(self: Arc<Self>) {
        let mut transcript: Option<Transcript> = None;
        loop {
            thread::sleep(POLL);
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            let Some(console) = self.console_id() else {
                continue;
            };
            if !super::console::is_alive(&console) {
                self.end_turn(TurnStatus::Failed);
                self.emit(AgentDelta::Failed {
                    message: "La consola del agente se cerró.".to_string(),
                });
                break;
            }
            if transcript.is_none() {
                transcript = self.find_transcript();
                if let Some(t) = &transcript {
                    if self.backend == "codex" {
                        if let Some(id) = t
                            .path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .and_then(super::watch_codex::session_id_from_stem)
                        {
                            // Con su id de Codex anotado, la pill no la lista
                            // aparte como otra sesión suelta.
                            self.emit(AgentDelta::ThreadPatch {
                                patch: ThreadPatch {
                                    provider_session: Some(id.to_string()),
                                    ..Default::default()
                                },
                            });
                        }
                    }
                }
            }
            let Some(t) = transcript.as_mut() else {
                continue;
            };
            let lines = if self.backend == "codex" {
                super::watch_codex::read_new_lines(&t.path, &mut t.tail)
            } else {
                super::watch_claude::read_new_lines(&t.path, &mut t.tail)
            };
            for line in lines {
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let effect = if self.backend == "codex" {
                    codex_effect(&v)
                } else {
                    claude_effect(&v)
                };
                self.apply(effect);
            }
        }
    }

    fn find_transcript(&self) -> Option<Transcript> {
        let path = if self.backend == "codex" {
            let (_, since) = (*self.spawned_at.lock_or_recover())?;
            find_codex_rollout(self.cwd.as_deref(), since)?
        } else {
            find_claude_jsonl(&self.key)?
        };
        Some(Transcript {
            path,
            tail: Tail::default(),
        })
    }
}

/// `~/.claude/projects/<carpeta>/<id>.jsonl`. La carpeta es el cwd escapado
/// a la manera de Claude; buscar por nombre de archivo evita imitarla.
fn find_claude_jsonl(session_id: &str) -> Option<PathBuf> {
    let root = super::claude_sessions::projects_root()?;
    let name = format!("{session_id}.jsonl");
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|dir| dir.path().join(&name))
        .find(|path| path.is_file())
}

fn normalize_cwd(path: &str) -> String {
    path.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// La primera línea del rollout, que en Codex es `session_meta`.
fn first_json_line(path: &Path) -> Option<Value> {
    let file = File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file.take(256 * 1024))
        .read_line(&mut line)
        .ok()?;
    serde_json::from_str(line.trim()).ok()
}

/// El rollout que abrió el TUI que lanzamos: de `codex-tui`, en nuestra
/// carpeta, creado después del arranque y sin dueño. Solo se miran hoy y ayer.
fn find_codex_rollout(cwd: Option<&str>, since: SystemTime) -> Option<PathBuf> {
    let root = super::watch_codex::sessions_root()?;
    let want = cwd.map(normalize_cwd);
    let floor = since.checked_sub(Duration::from_secs(5)).unwrap_or(since);
    let now = chrono::Local::now();
    let days = [now, now - chrono::Duration::days(1)];
    let mut claimed = CLAIMED_ROLLOUTS.lock_or_recover();
    let taken = claimed.get_or_insert_with(HashSet::new);
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for day in days {
        let dir = root
            .join(day.format("%Y").to_string())
            .join(day.format("%m").to_string())
            .join(day.format("%d").to_string());
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") || taken.contains(&path) {
                continue;
            }
            let Some(created) = file
                .metadata()
                .ok()
                .and_then(|m| m.created().or_else(|_| m.modified()).ok())
            else {
                continue;
            };
            if created < floor {
                continue;
            }
            let Some(meta) = first_json_line(&path) else {
                continue;
            };
            if super::watch_codex::origin_of(&meta) != Some(true) {
                continue;
            }
            let cwd_here = meta
                .pointer("/payload/cwd")
                .and_then(Value::as_str)
                .map(normalize_cwd);
            if want.is_some() && cwd_here != want {
                continue;
            }
            if best.as_ref().is_none_or(|(at, _)| created < *at) {
                best = Some((created, path));
            }
        }
    }
    let (_, path) = best?;
    taken.insert(path.clone());
    Some(path)
}

impl AgentSession for ConsoleAgent {
    fn send(&mut self, text: &str, _origin: Option<super::model::Origin>) -> Result<(), String> {
        let shared = &self.shared;
        let sent_as_arg = shared.ensure_console(Some(text))?;
        // El turno se abre antes de volver: quien espera en el hub vería la
        // sesión quieta y se iría con un «listo» vacío.
        shared.begin_turn(Some(text));
        if sent_as_arg {
            return Ok(());
        }
        let console = shared
            .console_id()
            .ok_or_else(|| "la consola no arrancó".to_string())?;
        let writer = Arc::clone(shared);
        let text = text.to_string();
        thread::spawn(move || {
            if let Err(message) = writer.type_when_ready(&console, &text) {
                writer.end_turn(TurnStatus::Failed);
                writer.emit(AgentDelta::Failed { message });
            }
        });
        Ok(())
    }

    fn interrupt(&mut self) -> Result<(), String> {
        let Some(console) = self.shared.console_id() else {
            return Ok(());
        };
        let turn = self.shared.turn.lock_or_recover().current.clone();
        super::console::write_input(&console, "\x1b")?;
        // Claude no deja una línea de «turno cortado» en el transcript: si no
        // cierra solo, se da por cancelado.
        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            thread::sleep(INTERRUPT_GRACE);
            let still = shared.turn.lock_or_recover().current.clone();
            if still.is_some() && still == turn {
                shared.end_turn(TurnStatus::Cancelled);
            }
        });
        Ok(())
    }

    fn console(&self) -> Option<String> {
        self.shared.console_id()
    }

    fn stop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        if let Some(console) = self.shared.console_id() {
            super::console::close_session(&console);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_arranca_con_su_id_y_sin_preguntar() {
        let options = StartOptions {
            session_id: Some("k1".into()),
            permission_mode: Some("bypassPermissions".into()),
            model: Some("opus".into()),
            ..Default::default()
        };
        let (cli, args) = cli_and_args("claude-code", &options).unwrap();
        assert_eq!(cli, "claude");
        assert_eq!(
            args,
            [
                "--session-id",
                "k1",
                "--model",
                "opus",
                "--permission-mode",
                "bypassPermissions"
            ]
        );
    }

    #[test]
    fn las_opciones_de_varios_valores_no_quedan_antes_de_la_tarea() {
        let options = StartOptions {
            mcp_config: Some("{}".into()),
            add_dirs: vec!["C:/x".into()],
            ..Default::default()
        };
        let (_, args) = cli_and_args("claude-code", &options).unwrap();
        assert_eq!(args[..4], ["--mcp-config", "{}", "--add-dir", "C:/x"]);
        assert_eq!(args[args.len() - 2], "--permission-mode");
    }

    #[test]
    fn claude_sin_modo_usa_el_default_del_tui() {
        assert_eq!(claude_permission_mode(None), "default");
        assert_eq!(claude_permission_mode(Some("manual")), "default");
    }

    #[test]
    fn codex_sin_permisos_salta_aprobaciones_y_sandbox() {
        let options = StartOptions {
            permission_mode: Some("bypassPermissions".into()),
            ..Default::default()
        };
        let (cli, args) = cli_and_args("codex", &options).unwrap();
        assert_eq!(cli, "codex");
        assert_eq!(args, ["--dangerously-bypass-approvals-and-sandbox"]);
    }

    #[test]
    fn otros_backends_no_abren_en_consola() {
        assert!(cli_and_args("opencode", &StartOptions::default()).is_none());
        assert!(!supported("cursor"));
    }

    #[test]
    fn la_tarea_que_parece_opcion_no_se_lee_como_opcion() {
        assert_eq!(prompt_arg("-h ayuda"), " -h ayuda");
        assert_eq!(prompt_arg("hola"), "hola");
    }

    #[test]
    fn el_texto_se_pega_entero_o_se_aplana() {
        assert_eq!(frame_input("a\nb", true), "\x1b[200~a\nb\x1b[201~");
        assert_eq!(frame_input("a\r\n\nb", false), "a b");
    }

    #[test]
    fn claude_abre_con_el_prompt_y_cierra_con_el_fin_de_turno() {
        let prompt = json!({"type": "user", "promptSource": "cli", "message": {}});
        assert!(claude_effect(&prompt).begins);

        let tool = json!({"type": "assistant", "message": {
            "stop_reason": "tool_use",
            "content": [{"type": "text", "text": "reviso"}]
        }});
        let effect = claude_effect(&tool);
        assert_eq!(effect.ends, None);
        assert_eq!(effect.append.as_deref(), Some("reviso"));

        let end = json!({"type": "assistant", "message": {
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "listo"}]
        }});
        let effect = claude_effect(&end);
        assert_eq!(effect.ends, Some(TurnStatus::Done));
        assert_eq!(effect.append.as_deref(), Some("listo"));

        let side = json!({"type": "assistant", "isSidechain": true, "message": {
            "stop_reason": "end_turn", "content": [{"type": "text", "text": "x"}]
        }});
        assert_eq!(claude_effect(&side), LineEffect::default());
    }

    #[test]
    fn codex_cierra_con_su_ultimo_mensaje_o_cancelado() {
        let done = json!({"type": "event_msg", "payload": {
            "type": "task_complete", "last_agent_message": "hecho\ncon detalle"
        }});
        let effect = codex_effect(&done);
        assert_eq!(effect.ends, Some(TurnStatus::Done));
        assert_eq!(effect.replace.as_deref(), Some("hecho\ncon detalle"));

        let aborted = json!({"type": "event_msg", "payload": {"type": "turn_aborted"}});
        assert_eq!(codex_effect(&aborted).ends, Some(TurnStatus::Cancelled));

        let started = json!({"type": "event_msg", "payload": {"type": "task_started"}});
        assert!(codex_effect(&started).begins);
    }

    #[test]
    fn las_carpetas_se_comparan_sin_importar_barras_ni_mayusculas() {
        assert_eq!(normalize_cwd("C:/Users/X/"), normalize_cwd("c:\\users\\x"));
    }
}
