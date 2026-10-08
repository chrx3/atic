//! Lo que un agente en una consola espera del usuario: un permiso («¿Ejecutar
//! este comando?») o una pregunta con opciones. Claude Code y Codex los avisan
//! por hooks con el input entero mientras el diálogo ya está en pantalla (ver
//! [`crate::hooks`]), con los mismos nombres de evento y campos. Los TUI no
//! escriben ninguno de los dos en su transcript hasta que se contestan. Se
//! contestan escribiendo en la consola las mismas teclas que usaría uno.
//!
//! Claude avisa los dos por `PermissionRequest`; Codex, los permisos igual y
//! sus preguntas (`request_user_input`) por `PreToolUse`.
//!
//! OpenCode no necesita teclas: su TUI levanta un servidor HTTP que lista lo
//! pendiente y lo contesta; aquí solo se decide qué mandarle ([`Reply`]).
//!
//! Kimi (1.41) avisa sus preguntas por `PreToolUse` y se contesta con
//! teclas: ↓ y Enter; Espacio marca en las de varias; «Other» (la fila después
//! de las opciones) abre con Enter un campo para escribir. Sus permisos no:
//! su hook llega antes de cada herramienta, también de las ya aprobadas, y no
//! hay cómo saber si el diálogo está en pantalla.
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
use std::time::{Duration, Instant};

use serde_json::Value;

/// Algo sin contestar que sigue más de esto ya no es creíble.
const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
/// Entre tecla y tecla: el TUI redibuja y una ráfaga se toma como pegado.
pub const KEY_GAP: Duration = Duration::from_millis(90);
/// Tras pasar de pregunta, para que la siguiente ya esté dibujada.
const STEP_GAP: Duration = Duration::from_millis(350);

const DOWN: &str = "\x1b[B";
const ENTER: &str = "\r";
const SPACE: &str = " ";
const ESC: &str = "\x1b";

/// De qué CLI es la sesión: cambia qué teclas contestan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    Claude,
    Codex,
    OpenCode,
    Kimi,
}

/// Qué se contesta a un permiso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Aceptar y grabar la regla que sugirió el agente, por esta sesión.
    AllowAlways,
    Deny,
}

/// Lo que espera la consola.
#[derive(Debug, Clone, PartialEq)]
pub enum Prompt {
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
pub struct Pending {
    pub id: String,
    pub cli: Cli,
    /// OpenCode: el puerto de su servidor, por donde se contesta.
    pub port: Option<u16>,
    pub prompt: Prompt,
    at: Instant,
}

/// Un paso al contestar con teclas.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Keys(String),
    Pause(Duration),
}

/// Cómo se contesta lo pendiente.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// Teclear en la consola, en orden (ver [`KEY_GAP`] entre teclas).
    Keys(Vec<Step>),
    /// OpenCode: las respuestas por pregunta (`/question/{id}/reply`).
    OpenCodeAnswer(Vec<Vec<String>>),
    /// OpenCode: descartar la pregunta (`/question/{id}/reject`).
    OpenCodeReject,
    /// OpenCode: `once`, `always` o `reject` (`/permission/{id}/reply`).
    OpenCodePermission(&'static str),
}

/// Lo pendiente por id de sesión. Cada proceso lleva el suyo.
#[derive(Default)]
pub struct Prompts {
    map: HashMap<String, Pending>,
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

impl Prompts {
    /// Lo que dice un hook sobre lo que espera su sesión.
    pub fn observe(&mut self, v: &Value, cli: Cli) {
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
                self.map.remove(session);
                return;
            }
            _ => return,
        };
        let id = match &prompt {
            Prompt::Question { input } => stable_id("q", tool, input),
            Prompt::Permission { tool, input, .. } => stable_id("p", tool, input),
        };
        self.map
            .insert(session.to_string(), Pending { id, cli, port: None, prompt, at: Instant::now() });
    }

    /// Lo pendiente que lista el servidor de un OpenCode. Lo que ese puerto ya
    /// no lista, se contestó.
    pub fn sync_opencode(&mut self, port: u16, live: Vec<(String, String, Prompt)>) {
        self.map.retain(|_, p| p.port != Some(port) || live.iter().any(|(_, id, _)| *id == p.id));
        for (session, id, prompt) in live {
            if self.map.get(&session).is_some_and(|p| p.id == id) {
                continue;
            }
            self.map
                .insert(session, Pending { id, cli: Cli::OpenCode, port: Some(port), prompt, at: Instant::now() });
        }
    }

    fn forget_stale(&mut self) {
        self.map.retain(|_, p| p.at.elapsed() < STALE_AFTER);
    }

    /// Lo que espera una sesión: su id y qué es.
    pub fn pending(&mut self, session: &str) -> Option<(String, Prompt)> {
        self.forget_stale();
        self.map.get(session).map(|p| (p.id.clone(), p.prompt.clone()))
    }

    /// Todo lo pendiente: sesión, id, CLI y qué es.
    pub fn all(&mut self) -> Vec<(String, String, Cli, Prompt)> {
        self.forget_stale();
        self.map
            .iter()
            .map(|(session, p)| (session.clone(), p.id.clone(), p.cli, p.prompt.clone()))
            .collect()
    }

    /// El puerto de OpenCode de lo pendiente de una sesión: `Some(None)` si se
    /// contesta con teclas, `None` si no hay nada pendiente.
    pub fn port(&self, session: &str) -> Option<Option<u16>> {
        self.map.get(session).map(|p| p.port)
    }

    /// Saca lo pendiente si sigue siendo `id`, para contestarlo.
    pub fn take(&mut self, session: &str, id: &str) -> Result<Pending, String> {
        match self.map.get(session) {
            Some(p) if p.id == id => Ok(self.map.remove(session).expect("recién visto")),
            _ => Err("eso ya se contestó".to_string()),
        }
    }
}

/// Cómo contestar una pregunta con estas respuestas (una por pregunta).
pub fn answer_reply(pending: &Pending, answers: &[String]) -> Result<Reply, String> {
    let Prompt::Question { input } = &pending.prompt else {
        return Err("eso no era una pregunta".into());
    };
    let questions = shape(input);
    if questions.is_empty() {
        return Err("la pregunta no tiene la forma esperada".into());
    }
    if pending.port.is_some() {
        // Por pregunta, las opciones elegidas o lo escrito.
        let lists = questions
            .iter()
            .enumerate()
            .map(|(i, (labels, multi))| {
                let answer = answers.get(i).map(String::as_str).unwrap_or("").trim();
                match picks(labels, answer, *multi) {
                    Some(p) => p.into_iter().map(|i| labels[i].clone()).collect(),
                    None if answer.is_empty() => Vec::new(),
                    None => vec![clean(answer)],
                }
            })
            .collect();
        return Ok(Reply::OpenCodeAnswer(lists));
    }
    Ok(Reply::Keys(match pending.cli {
        Cli::Codex => codex_keystrokes(&questions, answers),
        Cli::Kimi => kimi_keystrokes(&questions, answers),
        _ => keystrokes(&questions, answers),
    }))
}

/// Cómo contestar un permiso, o descartar una pregunta (rechazar es Esc en
/// los dos).
pub fn decide_reply(pending: &Pending, decision: Decision) -> Result<Reply, String> {
    if pending.port.is_some() {
        return Ok(match pending.prompt {
            Prompt::Question { .. } => Reply::OpenCodeReject,
            Prompt::Permission { .. } => Reply::OpenCodePermission(match decision {
                Decision::Allow => "once",
                Decision::AllowAlways => "always",
                Decision::Deny => "reject",
            }),
        });
    }
    let keys: &[&str] = match (&pending.prompt, decision) {
        (_, Decision::Deny) => &[ESC],
        (Prompt::Question { .. }, _) => return Err("una pregunta se contesta con opciones".into()),
        (Prompt::Permission { can_always: true, .. }, Decision::AllowAlways) => &[DOWN, ENTER],
        (Prompt::Permission { .. }, _) => &[ENTER],
    };
    Ok(Reply::Keys(keys.iter().map(|k| Step::Keys(k.to_string())).collect()))
}

/// Teclea los pasos en un hilo aparte, con [`KEY_GAP`] entre teclas. Para en
/// el primer error de `write`.
pub fn type_steps(steps: Vec<Step>, mut write: impl FnMut(&str) -> Result<(), String> + Send + 'static) {
    std::thread::spawn(move || {
        for step in steps {
            match step {
                Step::Keys(keys) => {
                    if let Err(err) = write(&keys) {
                        tracing::warn!(%err, "no se pudo contestar en la consola");
                        return;
                    }
                    std::thread::sleep(KEY_GAP);
                }
                Step::Pause(pause) => std::thread::sleep(pause),
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
                    (labels, flag("multiSelect") || flag("multiple") || flag("multi_select"))
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

/// Kimi: Enter manda cada pregunta y pasa a la siguiente (no hay revisión).
/// Lo escrito va en el campo que abre Enter sobre «Other»; en las de varias,
/// «Other» se marca con Espacio y se suma a lo elegido.
fn kimi_keystrokes(questions: &[(Vec<String>, bool)], answers: &[String]) -> Vec<Step> {
    let down = |n: usize| Step::Keys(DOWN.repeat(n));
    let mut steps = Vec::new();
    for (i, (labels, multi)) in questions.iter().enumerate() {
        let answer = answers.get(i).map(String::as_str).unwrap_or("").trim();
        let n = labels.len();
        match (picks(labels, answer, *multi), *multi) {
            (Some(p), false) => steps.push(down(p.first().copied().unwrap_or(0))),
            (Some(p), true) => {
                let mut cursor = 0;
                for idx in p {
                    steps.push(down(idx - cursor));
                    steps.push(Step::Keys(SPACE.into()));
                    cursor = idx;
                }
            }
            (None, multi) => {
                steps.push(down(n));
                if multi {
                    steps.push(Step::Keys(SPACE.into()));
                }
                steps.push(Step::Keys(ENTER.into()));
                // El campo para escribir tarda en abrir.
                steps.push(Step::Pause(STEP_GAP));
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
        let mut prompts = Prompts::default();
        let input = json!({ "questions": [{ "question": "¿Qué día?", "options": [{ "label": "Lunes" }] }] });
        prompts.observe(&json!({
            "session_id": "s-test", "hook_event_name": "PermissionRequest",
            "tool_name": "AskUserQuestion", "tool_input": input,
        }), Cli::Claude);
        let (id, prompt) = prompts.pending("s-test").unwrap();
        assert!(matches!(prompt, Prompt::Question { .. }));
        // La misma pregunta repetida da el mismo id.
        prompts.observe(&json!({
            "session_id": "s-test", "hook_event_name": "PermissionRequest",
            "tool_name": "AskUserQuestion", "tool_input": input,
        }), Cli::Claude);
        assert_eq!(prompts.pending("s-test").unwrap().0, id);

        prompts.observe(&json!({
            "session_id": "s-test", "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": { "command": "mkdir x" }, "permission_suggestions": [{ "type": "addRules" }],
        }), Cli::Claude);
        let (pid, prompt) = prompts.pending("s-test").unwrap();
        assert_ne!(pid, id);
        assert_eq!(
            prompt,
            Prompt::Permission { tool: "Bash".into(), input: json!({ "command": "mkdir x" }), can_always: true }
        );
        prompts.observe(&json!({ "session_id": "s-test", "hook_event_name": "PostToolUse", "tool_name": "Bash" }), Cli::Claude);
        assert!(prompts.pending("s-test").is_none());
        assert!(prompts.take("s-test", &pid).is_err());
    }

    #[test]
    fn codex_contesta_con_su_selector() {
        let questions = [q(&["Rojo", "Verde", "Azul"], false)];
        assert_eq!(keys(&codex_keystrokes(&questions, &["Verde".into()])), "↓⏎");
        assert_eq!(keys(&codex_keystrokes(&questions, &["Morado".into()])), "↓↓↓\tMorado⏎");
    }

    #[test]
    fn codex_avisa_sus_preguntas_por_pre_tool_use() {
        let mut prompts = Prompts::default();
        let input = json!({ "questions": [{ "id": "c", "question": "¿Color?", "options": [{ "label": "Rojo" }] }] });
        prompts.observe(&json!({
            "session_id": "s-codex", "hook_event_name": "PreToolUse",
            "tool_name": "request_user_input", "tool_input": input,
        }), Cli::Codex);
        assert!(matches!(prompts.pending("s-codex").unwrap().1, Prompt::Question { .. }));
        // Otra herramienta antes de contestar no es una pregunta ni la borra.
        prompts.observe(&json!({ "session_id": "s-codex", "hook_event_name": "PreToolUse", "tool_name": "Bash" }), Cli::Codex);
        assert!(prompts.pending("s-codex").is_some());
    }

    #[test]
    fn opencode_lo_que_su_servidor_deja_de_listar_se_contesto() {
        let mut prompts = Prompts::default();
        let q = Prompt::Question { input: json!({ "questions": [{ "question": "¿Día?", "options": [] }] }) };
        prompts.sync_opencode(4599, vec![("ses_a".into(), "que_1".into(), q.clone())]);
        assert_eq!(prompts.port("ses_a"), Some(Some(4599)));
        assert_eq!(prompts.pending("ses_a").unwrap().0, "que_1");
        // Otro puerto no la toca; el suyo, sin listarla, la borra.
        prompts.sync_opencode(4600, vec![]);
        assert!(prompts.pending("ses_a").is_some());
        prompts.sync_opencode(4599, vec![]);
        assert!(prompts.pending("ses_a").is_none());
    }

    #[test]
    fn kimi_contesta_con_su_panel() {
        let one = [q(&["Rojo", "Verde", "Azul"], false)];
        assert_eq!(keys(&kimi_keystrokes(&one, &["Verde".into()])), "↓⏎");
        assert_eq!(keys(&kimi_keystrokes(&one, &["Morado".into()])), "↓↓↓⏎Morado⏎");
        let many = [q(&["Manzana", "Pera", "Uva"], true)];
        assert_eq!(keys(&kimi_keystrokes(&many, &["Manzana, Uva".into()])), "␣↓↓␣⏎");
    }

    #[test]
    fn sin_sugerencias_no_hay_permitir_siempre() {
        let mut prompts = Prompts::default();
        prompts.observe(&json!({
            "session_id": "s-sin", "hook_event_name": "PermissionRequest", "tool_name": "Edit",
            "tool_input": { "file_path": "a.rs" },
        }), Cli::Claude);
        assert!(matches!(prompts.pending("s-sin").unwrap().1, Prompt::Permission { can_always: false, .. }));
    }

    #[test]
    fn decidir_un_permiso_teclea_o_va_por_http() {
        let mut prompts = Prompts::default();
        prompts.observe(&json!({
            "session_id": "s", "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": { "command": "ls" }, "permission_suggestions": [{}],
        }), Cli::Claude);
        let (id, _) = prompts.pending("s").unwrap();
        let pending = prompts.take("s", &id).unwrap();
        assert_eq!(keys(match &decide_reply(&pending, Decision::AllowAlways).unwrap() {
            Reply::Keys(steps) => steps,
            other => panic!("{other:?}"),
        }), "↓⏎");
        assert_eq!(decide_reply(&pending, Decision::Deny).unwrap(), Reply::Keys(vec![Step::Keys(ESC.into())]));

        let p = Prompt::Permission { tool: "bash".into(), input: json!({}), can_always: false };
        prompts.sync_opencode(1, vec![("o".into(), "per_1".into(), p)]);
        let pending = prompts.take("o", "per_1").unwrap();
        assert_eq!(decide_reply(&pending, Decision::Allow).unwrap(), Reply::OpenCodePermission("once"));
    }
}
