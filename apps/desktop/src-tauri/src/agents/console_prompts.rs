//! Lo que un agente en una consola de Atic espera del usuario: un permiso
//! («¿Ejecutar este comando?») o una pregunta con opciones. Hoy, Claude Code
//! y Codex: los dos tienen hooks que avisan con el input entero mientras el
//! diálogo ya está en pantalla (ver [`super::ping`]), con los mismos nombres
//! de evento y campos. Los TUI no escriben ninguno de los dos en su
//! transcript hasta que se contestan. Se contestan desde el celular
//! escribiendo en la consola las mismas teclas que usaría uno.
//!
//! Claude avisa los dos por `PermissionRequest`; Codex, los permisos igual y
//! sus preguntas (`request_user_input`) por `PreToolUse`.
//!
//! Teclas de Codex (0.157): permiso igual que Claude (Enter / ↓ Enter / Esc);
//! pregunta: ↓ hasta la opción y Enter; texto propio en «None of the above»
//! (la fila después de las opciones) con Tab para abrir la nota.
//!
//! El orden de las teclas de Claude se probó contra el TUI (v2.1.x):
//! - permiso: Enter es «Yes»; ↓ y Enter, «Yes, and always allow…» (solo existe
//!   si el hook trae sugerencias de regla); Esc lo rechaza, sin hook de vuelta;
//! - pregunta de una respuesta: ↓ hasta la opción y Enter, que pasa a la
//!   pregunta siguiente;
//! - de varias: Espacio marca cada una y Enter sobre «Submit» (la fila después
//!   de «Type something») pasa a la siguiente;
//! - escribir sobre «Type something» la reemplaza (y en las de varias la marca);
//! - con más de una pregunta queda una revisión cuyo Enter manda todo. Con una
//!   sola de una respuesta, su Enter ya la manda.

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use atic_core::MutexExt;
use serde_json::Value;

use super::PermissionDecision;

/// Algo sin contestar que sigue más de esto ya no es creíble.
const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
/// Entre tecla y tecla: el TUI redibuja y una ráfaga se toma como pegado.
const KEY_GAP: Duration = Duration::from_millis(90);
/// Tras pasar de pregunta, para que la siguiente ya esté dibujada.
const STEP_GAP: Duration = Duration::from_millis(350);

const DOWN: &str = "\x1b[B";
const ENTER: &str = "\r";
const SPACE: &str = " ";
const ESC: &str = "\x1b";

/// De qué CLI es la sesión: cambia qué teclas contestan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cli {
    Claude,
    Codex,
}

/// Lo que espera la consola.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Prompt {
    /// Una pregunta con opciones; el input entero de `AskUserQuestion`.
    Question { input: Value },
    /// Un permiso para una herramienta.
    Permission {
        tool: String,
        input: Value,
        /// El diálogo ofrece «Yes, and always allow…».
        can_always: bool,
    },
}

#[derive(Debug, Clone)]
struct Pending {
    id: String,
    cli: Cli,
    prompt: Prompt,
    at: Instant,
}

/// Por id de sesión de Claude (el mismo que usa la presencia).
static PENDING: Mutex<Option<HashMap<String, Pending>>> = Mutex::new(None);

fn with_pending<R>(f: impl FnOnce(&mut HashMap<String, Pending>) -> R) -> R {
    let mut guard = PENDING.lock_or_recover();
    f(guard.get_or_insert_with(HashMap::new))
}

/// `PermissionRequest` no trae el id de la herramienta: lo mismo pedido da el mismo id.
fn stable_id(prefix: &str, tool: &str, input: &Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tool.hash(&mut h);
    input.to_string().hash(&mut h);
    format!("{prefix}-{:016x}", h.finish())
}

/// Las herramientas que hacen preguntas con opciones.
fn is_question_tool(tool: &str) -> bool {
    matches!(tool, "AskUserQuestion" | "request_user_input")
}

/// Lo que dice un hook sobre lo que espera su sesión.
pub(crate) fn observe(v: &Value, cli: Cli) {
    let Some(session) = v.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
        return;
    };
    let event = v.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
    let tool = v.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let input = v.get("tool_input").cloned().unwrap_or(Value::Null);
    let prompt = match event {
        "PermissionRequest" | "PreToolUse" if is_question_tool(tool) => {
            if !input.get("questions").is_some_and(Value::is_array) {
                return;
            }
            Prompt::Question { input }
        }
        "PermissionRequest" if !tool.is_empty() => {
            let can_always = v
                .get("permission_suggestions")
                .and_then(Value::as_array)
                .is_some_and(|s| !s.is_empty());
            Prompt::Permission { tool: tool.to_string(), input, can_always }
        }
        // Se contestó (en la consola o desde acá), o la conversación siguió.
        "PostToolUse" | "UserPromptSubmit" | "Stop" => {
            with_pending(|map| map.remove(session));
            return;
        }
        _ => return,
    };
    let id = match &prompt {
        Prompt::Question { input } => stable_id("q", tool, input),
        Prompt::Permission { tool, input, .. } => stable_id("p", tool, input),
    };
    with_pending(|map| {
        map.insert(session.to_string(), Pending { id, cli, prompt, at: Instant::now() });
    });
}

/// Lo que espera una sesión: su id y qué es.
pub(crate) fn pending(session: &str) -> Option<(String, Prompt)> {
    with_pending(|map| {
        map.retain(|_, p| p.at.elapsed() < STALE_AFTER);
        map.get(session).map(|p| (p.id.clone(), p.prompt.clone()))
    })
}

/// Contesta una pregunta tecleando en la consola donde corre la sesión.
pub(crate) fn answer(session: &str, id: &str, answers: &[String]) -> Result<(), String> {
    let (cli, prompt) = take(session, id)?;
    let Prompt::Question { input } = prompt else {
        return Err("eso no era una pregunta".into());
    };
    let questions = shape(&input);
    if questions.is_empty() {
        return Err("la pregunta no tiene la forma esperada".into());
    }
    let console = console_of(session)?;
    let steps = match cli {
        Cli::Claude => keystrokes(&questions, answers),
        Cli::Codex => codex_keystrokes(&questions, answers),
    };
    type_steps(console, steps);
    Ok(())
}

/// Contesta un permiso, o descarta una pregunta (rechazar es Esc en los dos).
pub(crate) fn decide(session: &str, id: &str, decision: PermissionDecision) -> Result<(), String> {
    let (_, prompt) = take(session, id)?;
    let console = console_of(session)?;
    let keys: &[&str] = match (&prompt, decision) {
        (_, PermissionDecision::Deny) => &[ESC],
        (Prompt::Question { .. }, _) => return Err("una pregunta se contesta con opciones".into()),
        (Prompt::Permission { can_always: true, .. }, PermissionDecision::AllowAlways) => &[DOWN, ENTER],
        (Prompt::Permission { .. }, _) => &[ENTER],
    };
    type_steps(console, keys.iter().map(|k| Step::Keys(k.to_string())).collect());
    Ok(())
}

fn take(session: &str, id: &str) -> Result<(Cli, Prompt), String> {
    with_pending(|map| match map.get(session) {
        Some(p) if p.id == id => Ok(map.remove(session).map(|p| (p.cli, p.prompt)).expect("recién visto")),
        _ => Err("eso ya se contestó".to_string()),
    })
}

fn console_of(session: &str) -> Result<String, String> {
    super::console::console_for_presence(session.to_string())
        .ok_or_else(|| "la sesión no corre en una consola de Atic".to_string())
}

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Keys(String),
    Pause(Duration),
}

fn type_steps(console: String, steps: Vec<Step>) {
    thread::spawn(move || {
        for step in steps {
            match step {
                Step::Keys(k) => {
                    if let Err(err) = super::console::write_input(&console, &k) {
                        tracing::warn!(%err, "no se pudo contestar en la consola");
                        return;
                    }
                    thread::sleep(KEY_GAP);
                }
                Step::Pause(d) => thread::sleep(d),
            }
        }
    });
}

/// Etiquetas de cada pregunta y si admite varias respuestas.
fn shape(input: &Value) -> Vec<(Vec<String>, bool)> {
    input
        .get("questions")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|q| {
                    let labels = q
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|o| {
                            o.iter()
                                .filter_map(|x| x.get("label").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let flag = |k: &str| q.get(k).and_then(Value::as_bool).unwrap_or(false);
                    (labels, flag("multiSelect") || flag("multiple"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Qué opciones nombra una respuesta. `None` si es texto propio. Varias
/// elegidas llegan separadas por coma (así las junta el celular).
fn picks(labels: &[String], answer: &str, multi: bool) -> Option<Vec<usize>> {
    let index = |s: &str| labels.iter().position(|l| l.trim() == s.trim());
    if let Some(i) = index(answer) {
        return Some(vec![i]);
    }
    if !multi {
        return None;
    }
    let mut out: Vec<usize> = answer.split(", ").map(index).collect::<Option<_>>()?;
    out.sort_unstable();
    out.dedup();
    Some(out)
}

/// Lo escrito a mano no puede traer teclas de control: un Enter o un Esc
/// colado contestaría otra cosa.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn keystrokes(questions: &[(Vec<String>, bool)], answers: &[String]) -> Vec<Step> {
    let down = |n: usize| Step::Keys(DOWN.repeat(n));
    let mut steps = Vec::new();
    for (i, (labels, multi)) in questions.iter().enumerate() {
        let answer = answers.get(i).map(String::as_str).unwrap_or("").trim();
        let n = labels.len();
        match (picks(labels, answer, *multi), *multi) {
            (Some(p), false) => {
                steps.push(down(p.first().copied().unwrap_or(0)));
                steps.push(Step::Keys(ENTER.into()));
            }
            (None, false) => {
                steps.push(down(n));
                steps.push(Step::Keys(clean(answer)));
                steps.push(Step::Keys(ENTER.into()));
            }
            (chosen, true) => {
                let mut cursor = 0;
                for idx in chosen.clone().unwrap_or_default() {
                    steps.push(down(idx - cursor));
                    steps.push(Step::Keys(SPACE.into()));
                    cursor = idx;
                }
                if chosen.is_none() && !answer.is_empty() {
                    steps.push(down(n - cursor));
                    steps.push(Step::Keys(clean(answer)));
                    cursor = n;
                }
                // «Submit» va justo después de «Type something».
                steps.push(down(n + 1 - cursor));
                steps.push(Step::Keys(ENTER.into()));
            }
        }
        steps.push(Step::Pause(STEP_GAP));
    }
    // Con una sola pregunta de una respuesta, su Enter ya mandó. Si no, queda
    // la revisión, con «Submit answers» elegido. Un Enter de más cae en la
    // entrada vacía del TUI, que no hace nada.
    let single = questions.len() == 1 && !questions[0].1;
    if !single {
        steps.push(Step::Keys(ENTER.into()));
    }
    // `down(0)` no teclea nada.
    steps.retain(|s| !matches!(s, Step::Keys(k) if k.is_empty()));
    steps
}

/// Codex: cada pregunta es de una respuesta. Lo escrito va como nota sobre
/// «None of the above». Si son varias, el último Enter cae en la entrada
/// vacía, que no hace nada.
fn codex_keystrokes(questions: &[(Vec<String>, bool)], answers: &[String]) -> Vec<Step> {
    let mut steps = Vec::new();
    for (i, (labels, _)) in questions.iter().enumerate() {
        let answer = answers.get(i).map(String::as_str).unwrap_or("").trim();
        match picks(labels, answer, false) {
            Some(p) => steps.push(Step::Keys(DOWN.repeat(p.first().copied().unwrap_or(0)))),
            None => {
                steps.push(Step::Keys(DOWN.repeat(labels.len())));
                steps.push(Step::Keys("\t".into()));
                steps.push(Step::Keys(clean(answer)));
            }
        }
        steps.push(Step::Keys(ENTER.into()));
        steps.push(Step::Pause(STEP_GAP));
    }
    steps.retain(|s| !matches!(s, Step::Keys(k) if k.is_empty()));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keys(steps: &[Step]) -> String {
        steps
            .iter()
            .filter_map(|s| match s {
                Step::Keys(k) if k == SPACE => Some("␣".to_string()),
                Step::Keys(k) => Some(k.replace(DOWN, "↓").replace(ENTER, "⏎")),
                Step::Pause(_) => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn q(labels: &[&str], multi: bool) -> (Vec<String>, bool) {
        (labels.iter().map(|s| s.to_string()).collect(), multi)
    }

    #[test]
    fn una_pregunta_de_una_respuesta_se_manda_con_su_enter() {
        let steps = keystrokes(&[q(&["Lunes", "Martes"], false)], &["Martes".into()]);
        assert_eq!(keys(&steps), "↓⏎");
    }

    #[test]
    fn varias_preguntas_terminan_en_la_revision() {
        let questions = [q(&["Rojo", "Verde", "Azul"], false), q(&["Manzana", "Pera", "Uva"], true)];
        let steps = keystrokes(&questions, &["Verde".into(), "Manzana, Uva".into()]);
        // Verde; marcar Manzana y Uva; bajar a Submit; mandar la revisión.
        assert_eq!(keys(&steps), "↓⏎␣↓↓␣↓↓⏎⏎");
    }

    #[test]
    fn lo_escrito_va_sobre_type_something_sin_teclas_de_control() {
        let questions = [q(&["Rojo", "Verde", "Azul"], false), q(&["Manzana", "Pera", "Uva"], true)];
        let steps = keystrokes(&questions, &["Morado\nclaro".into(), "Kiwi".into()]);
        assert_eq!(keys(&steps), "↓↓↓Morado claro⏎↓↓↓Kiwi↓⏎⏎");
    }

    #[test]
    fn una_etiqueta_con_coma_gana_al_corte() {
        let labels = q(&["Sí, ahora", "No"], true);
        assert_eq!(picks(&labels.0, "Sí, ahora", true), Some(vec![0]));
        assert_eq!(picks(&labels.0, "No, Sí, ahora", true), None);
        assert_eq!(picks(&labels.0, "otra cosa", false), None);
    }

    #[test]
    fn el_hook_anota_preguntas_y_permisos_y_los_borra() {
        let input = json!({ "questions": [{ "question": "¿Qué día?", "options": [{ "label": "Lunes" }] }] });
        observe(&json!({
            "session_id": "s-test", "hook_event_name": "PermissionRequest",
            "tool_name": "AskUserQuestion", "tool_input": input,
        }), Cli::Claude);
        let (id, prompt) = pending("s-test").unwrap();
        assert!(matches!(prompt, Prompt::Question { .. }));
        // La misma pregunta repetida da el mismo id.
        observe(&json!({
            "session_id": "s-test", "hook_event_name": "PermissionRequest",
            "tool_name": "AskUserQuestion", "tool_input": input,
        }), Cli::Claude);
        assert_eq!(pending("s-test").unwrap().0, id);

        observe(&json!({
            "session_id": "s-test", "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": { "command": "mkdir x" }, "permission_suggestions": [{ "type": "addRules" }],
        }), Cli::Claude);
        let (pid, prompt) = pending("s-test").unwrap();
        assert_ne!(pid, id);
        assert_eq!(
            prompt,
            Prompt::Permission { tool: "Bash".into(), input: json!({ "command": "mkdir x" }), can_always: true }
        );
        observe(&json!({ "session_id": "s-test", "hook_event_name": "PostToolUse", "tool_name": "Bash" }), Cli::Claude);
        assert!(pending("s-test").is_none());
        assert!(take("s-test", &pid).is_err());
    }

    #[test]
    fn codex_contesta_con_su_selector() {
        let questions = [q(&["Rojo", "Verde", "Azul"], false)];
        assert_eq!(keys(&codex_keystrokes(&questions, &["Verde".into()])), "↓⏎");
        assert_eq!(keys(&codex_keystrokes(&questions, &["Morado".into()])), "↓↓↓\tMorado⏎");
    }

    #[test]
    fn codex_avisa_sus_preguntas_por_pre_tool_use() {
        let input = json!({ "questions": [{ "id": "c", "question": "¿Color?", "options": [{ "label": "Rojo" }] }] });
        observe(&json!({
            "session_id": "s-codex", "hook_event_name": "PreToolUse",
            "tool_name": "request_user_input", "tool_input": input,
        }), Cli::Codex);
        assert!(matches!(pending("s-codex").unwrap().1, Prompt::Question { .. }));
        // Otra herramienta antes de contestar no es una pregunta ni la borra.
        observe(&json!({ "session_id": "s-codex", "hook_event_name": "PreToolUse", "tool_name": "Bash" }), Cli::Codex);
        assert!(pending("s-codex").is_some());
    }

    #[test]
    fn sin_sugerencias_no_hay_permitir_siempre() {
        observe(&json!({
            "session_id": "s-sin", "hook_event_name": "PermissionRequest", "tool_name": "Edit",
            "tool_input": { "file_path": "a.rs" },
        }), Cli::Claude);
        assert!(matches!(pending("s-sin").unwrap().1, Prompt::Permission { can_always: false, .. }));
    }
}
