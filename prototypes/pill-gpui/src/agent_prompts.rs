//! Los permisos que piden los agentes de las consolas de la pill, para
//! contestarlos desde la bandeja.
//!
//! Cada consola de agente lleva una marca en su entorno y los hooks de Atic
//! (`atic_agents::hooks`, modo un archivo por consola): Claude y Codex anotan
//! lo que esperan en `atic-pill-<marca>.jsonl`. Un hilo lo lee y la marca dice
//! en qué consola está el diálogo; contestar es teclear ahí lo que teclearía
//! uno. Los archivos son solo de la pill: la app de Tauri no los lee y nada se
//! contesta dos veces.

use std::collections::HashMap;
use std::sync::{Mutex, Once};
use std::time::Duration;

use atic_agents::hooks::{self, Sink, CONSOLE_TOKEN_VAR, PILL_PREFIX};
use atic_agents::prompts::{self, Cli, Decision, Prompt, Prompts, Reply};
use atic_core::MutexExt;

use crate::space::console::Writer;

const EVERY: Duration = Duration::from_millis(500);

/// Una consola de agente con marca.
struct Console {
    writer: Writer,
    cli: Cli,
    /// `claude`, `codex`…: el de `agents::AGENTS`.
    agent: &'static str,
}

#[derive(Default)]
struct State {
    consoles: HashMap<String, Console>,
    /// Sesión del agente → marca de su consola.
    sessions: HashMap<String, String>,
    prompts: Prompts,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static DRAIN: Once = Once::new();

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE.lock_or_recover().get_or_insert_with(Default::default))
}

fn cli_of(agent: &str) -> Option<Cli> {
    match agent {
        "claude" => Some(Cli::Claude),
        "codex" => Some(Cli::Codex),
        _ => None,
    }
}

/// La línea y el entorno con que se lanza un agente: los hooks y la marca.
/// `None` si ese CLI no tiene hooks (la consola se abre igual, sin permisos en
/// la bandeja).
pub fn prepare(agent: &str, line: &str) -> Option<(String, String)> {
    let line = match cli_of(agent)? {
        Cli::Claude => hooks::with_claude_hooks(line, Sink::PerConsole),
        Cli::Codex => hooks::with_codex_hooks(line, Sink::PerConsole),
        _ => return None,
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some((line, format!("{nanos:x}")))
}

/// La variable de entorno de la marca.
pub fn token_var() -> &'static str {
    CONSOLE_TOKEN_VAR
}

/// La consola ya abierta con esa marca.
pub fn register(token: &str, agent: &'static str, writer: Writer) {
    let Some(cli) = cli_of(agent) else {
        return;
    };
    with_state(|state| {
        state.consoles.insert(token.to_string(), Console { writer, cli, agent });
    });
    start_drain();
}

/// Se cerró la consola: lo que esperaba ya no se puede contestar.
pub fn unregister(token: &str) {
    with_state(|state| {
        state.consoles.remove(token);
        let gone: Vec<String> =
            state.sessions.iter().filter(|(_, t)| t.as_str() == token).map(|(s, _)| s.clone()).collect();
        for session in gone {
            state.sessions.remove(&session);
            if let Some((id, _)) = state.prompts.pending(&session) {
                let _ = state.prompts.take(&session, &id);
            }
        }
    });
    let _ = std::fs::remove_file(std::env::temp_dir().join(format!("{PILL_PREFIX}{token}.jsonl")));
}

fn start_drain() {
    DRAIN.call_once(|| {
        // Lo de corridas anteriores no tiene consola a quién contestar.
        if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let ours = name.starts_with(PILL_PREFIX) && name.ends_with(".jsonl");
                let token = name.trim_start_matches(PILL_PREFIX).trim_end_matches(".jsonl");
                if ours && !with_state(|state| state.consoles.contains_key(token)) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let started = std::thread::Builder::new().name("permisos-agentes".into()).spawn(|| {
            let mut drain = hooks::Drain::default();
            loop {
                drain.prefixed(&std::env::temp_dir(), PILL_PREFIX, |token, v| {
                    with_state(|state| {
                        let Some(cli) = state.consoles.get(token).map(|c| c.cli) else {
                            return;
                        };
                        if let Some(session) = v.get("session_id").and_then(|s| s.as_str()) {
                            state.sessions.insert(session.to_string(), token.to_string());
                        }
                        state.prompts.observe(v, cli);
                    });
                });
                std::thread::sleep(EVERY);
            }
        });
        if let Err(error) = started {
            tracing::warn!(%error, "no arrancó la lectura de permisos de agentes");
        }
    });
}

/// Un permiso esperando respuesta.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub session: String,
    pub id: String,
    /// `claude`, `codex`…
    pub agent: &'static str,
    pub tool: String,
    /// Lo que va a hacer: el comando, la ruta… En una línea.
    pub detail: String,
}

/// Los permisos que esperan en las consolas abiertas. Las preguntas con
/// opciones se contestan en la consola.
pub fn waiting() -> Vec<Waiting> {
    with_state(|state| {
        let sessions = state.sessions.clone();
        state
            .prompts
            .all()
            .into_iter()
            .filter_map(|(session, id, _, prompt)| {
                let Prompt::Permission { tool, input, .. } = prompt else {
                    return None;
                };
                let console = state.consoles.get(sessions.get(&session)?)?;
                Some(Waiting { session, id, agent: console.agent, detail: summary(&input), tool })
            })
            .collect()
    })
}

/// Lo más revelador del input de una herramienta, en una línea.
fn summary(input: &serde_json::Value) -> String {
    let text = ["command", "file_path", "path", "url", "pattern", "query"]
        .iter()
        .find_map(|key| input.get(key).and_then(|v| v.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| input.to_string());
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(160).collect()
}

/// Permitir o negar: teclea la respuesta en la consola del agente.
pub fn decide(session: &str, id: &str, allow: bool) -> Result<(), String> {
    let (pending, writer) = with_state(|state| {
        let token = state.sessions.get(session).cloned().ok_or("la consola ya no está")?;
        let writer = state.consoles.get(&token).map(|c| c.writer.clone()).ok_or("la consola ya no está")?;
        Ok::<_, String>((state.prompts.take(session, id)?, writer))
    })?;
    let decision = if allow { Decision::Allow } else { Decision::Deny };
    match prompts::decide_reply(&pending, decision)? {
        Reply::Keys(steps) => {
            prompts::type_steps(steps, move |keys| writer.write(keys));
            Ok(())
        }
        _ => Err("respuesta que no es con teclas".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_detalle_es_lo_que_va_a_hacer() {
        assert_eq!(summary(&json!({ "command": "cargo  test\n -p x", "description": "y" })), "cargo test -p x");
        assert_eq!(summary(&json!({ "file_path": "src/main.rs" })), "src/main.rs");
        assert_eq!(summary(&json!({ "otra": 1 })), "{\"otra\":1}");
    }

    #[test]
    fn solo_claude_y_codex_llevan_hooks() {
        assert!(prepare("opencode", "opencode").is_none());
        let (line, token) = prepare("claude", "claude").unwrap();
        assert!(line.starts_with("claude"));
        assert!(!token.is_empty());
    }
}
