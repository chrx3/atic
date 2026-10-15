//! Codex en el chat, por `codex app-server`: su propio protocolo JSON-RPC por stdio
//! (hilos, turnos e ítems en vez de las sesiones de ACP). Lo que avisa se traduce a
//! las actualizaciones de ACP (`agent_message_chunk`, `tool_call`, `plan`…) y pasa por
//! el mismo `Translator`, así el chat lo dibuja igual que a los demás agentes.
//!
//! El modelo y el razonamiento se eligen con las mismas opciones que manda un agente
//! ACP (`configOptions`), armadas con `model/list`; los modos son los de la CLI de
//! Codex (solo lectura, automático, acceso total). Sin uno elegido, vale el de
//! `~/.codex/config.toml`. Codex los recibe en cada turno.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{effort_name, Asked, Reply, Session, Translator};

/// Los modos de Codex, como en su CLI: (id, nombre, explicación). Los dos primeros
/// corren los comandos en el sandbox de Codex (en Windows hay que prepararlo).
const MODES: [(&str, &str, &str); 3] = [
    ("read-only", "Solo lectura", "Lee y responde; pide permiso para editar o ejecutar"),
    ("workspace-write", "Automático", "Edita y ejecuta en el proyecto, en el sandbox de Codex; pide permiso para salir de él"),
    ("full-access", "Acceso total", "Edita y ejecuta sin pedir permiso ni sandbox, también fuera del proyecto"),
];

/// El modo que corresponde a lo que Codex dice que usa (su `config.toml`, si no se
/// pidió otro): el sandbox y la política de aprobación de `thread/start`.
fn mode_of(opened: &Value) -> &'static str {
    match opened.pointer("/sandbox/type").and_then(Value::as_str) {
        Some("readOnly") => "read-only",
        Some("dangerFullAccess") | Some("externalSandbox") => "full-access",
        _ => "workspace-write",
    }
}

/// Lo que devuelve el sandbox de Windows de Codex cuando no está preparado.
fn sandbox_broken(output: &str) -> bool {
    output.contains("Failed to create unified exec process") || output.contains("setup refresh had errors")
}

/// Lo que la sesión de Codex recuerda entre mensajes.
#[derive(Default)]
pub(super) struct State {
    /// Los modelos de `model/list`, para armar las opciones.
    models: Vec<Value>,
    /// El turno en curso (para detenerlo).
    turn: Option<String>,
    /// El modo con que Codex trabaja ahora (si el usuario cambia otro, va en el turno).
    mode_applied: String,
    /// Los ítems cuyo texto llegó por trozos: al completarse no se repite.
    streamed: HashSet<String>,
    /// Los tokens de salida del hilo, ahora y al empezar el turno.
    output_total: u64,
    output_at_start: u64,
}

/// Qué hacer con un aviso de Codex.
#[derive(Debug, PartialEq)]
pub(super) enum Step {
    /// Una actualización con la forma de ACP para el `Translator`.
    Update(Value),
    /// Cierra el mensaje en curso (el siguiente texto va en una burbuja nueva).
    Finish,
    /// Fin del turno: la respuesta de `session/prompt` de ACP o un error.
    EndTurn(Value, Option<String>),
}

fn approval_policy(mode: &str) -> &'static str {
    if mode == "full-access" { "never" } else { "on-request" }
}

fn sandbox_mode(mode: &str) -> &'static str {
    match mode {
        "read-only" => "read-only",
        "full-access" => "danger-full-access",
        _ => "workspace-write",
    }
}

fn sandbox_policy(mode: &str) -> Value {
    match mode {
        "read-only" => json!({ "type": "readOnly" }),
        "full-access" => json!({ "type": "dangerFullAccess" }),
        _ => json!({ "type": "workspaceWrite" }),
    }
}

fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or_default()
}

/// Los modos con la forma de `modes` de ACP.
fn modes(current: &str) -> Value {
    let available: Vec<Value> = MODES.iter().map(|(id, name, description)| json!({ "id": id, "name": name, "description": description })).collect();
    json!({ "availableModes": available, "currentModeId": current })
}

/// El modelo y el razonamiento como `configOptions` de ACP. El razonamiento ofrece lo
/// que admite el modelo elegido; si el actual no está, el del modelo.
pub(super) fn config(models: &[Value], model: &str, effort: &str) -> Value {
    let mut choices: Vec<Value> = models
        .iter()
        .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .map(|m| json!({ "value": text(m, "id"), "name": text(m, "displayName"), "description": text(m, "description") }))
        .collect();
    let entry = models.iter().find(|m| text(m, "id") == model || text(m, "model") == model);
    if entry.is_none() && !model.is_empty() {
        choices.insert(0, json!({ "value": model, "name": model, "description": "" }));
    }
    let efforts: Vec<Value> = entry
        .and_then(|m| m.get("supportedReasoningEfforts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|e| {
            let id = text(e, "reasoningEffort");
            json!({ "value": id, "name": effort_name(id), "description": text(e, "description") })
        })
        .collect();
    let supported = |id: &str| efforts.iter().any(|e| text(e, "value") == id);
    let effort = if supported(effort) {
        effort.to_string()
    } else {
        entry.map(|m| text(m, "defaultReasoningEffort").to_string()).filter(|e| supported(e)).unwrap_or_default()
    };
    let mut options = vec![json!({ "id": "model", "name": "Modelo", "category": "model", "type": "select", "currentValue": model, "options": choices })];
    if !efforts.is_empty() {
        options.push(json!({ "id": "effort", "name": "Razonamiento", "category": "thought_level", "type": "select", "currentValue": effort, "options": efforts }));
    }
    Value::Array(options)
}

/// Vuelve a armar las opciones con el modelo y el razonamiento actuales (al cambiar
/// de modelo cambian los niveles que admite).
fn refit(translate: &mut Translator, models: &[Value]) {
    let current = |id: &str| translate.current(id).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let (model, effort) = (current("model"), current("effort"));
    translate.config = config(models, &model, &effort);
}

/// El comando que corre Codex sin el envoltorio de PowerShell
/// (`"…\powershell.exe" -Command 'echo hola'` → `echo hola`).
pub(super) fn shell_command(raw: &str) -> String {
    let Some((_, rest)) = raw.split_once(" -Command ") else {
        return raw.to_string();
    };
    let rest = rest.trim();
    for quote in ['\'', '"'] {
        if let Some(inner) = rest.strip_prefix(quote).and_then(|r| r.strip_suffix(quote)) {
            return inner.replace("''", "'");
        }
    }
    rest.to_string()
}

/// Lo de antes y lo de después de un diff unificado (las líneas de contexto en los dos).
fn diff_sides(diff: &str) -> (String, String) {
    let (mut old, mut new) = (Vec::new(), Vec::new());
    for line in diff.lines() {
        if line.starts_with("@@") || line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") || line.starts_with("index ") {
            continue;
        }
        match line.chars().next() {
            Some('-') => old.push(&line[1..]),
            Some('+') => new.push(&line[1..]),
            Some(' ') => {
                old.push(&line[1..]);
                new.push(&line[1..]);
            }
            _ => {}
        }
    }
    (old.join("\n"), new.join("\n"))
}

/// La salida de un comando; si el sandbox de Windows falló, con qué hacer.
fn command_output(output: &str) -> String {
    if sandbox_broken(output) {
        format!("{output}\n\nEl sandbox de Windows de Codex no está listo. Elige el modo Acceso total o abre `codex` en la terminal para que lo prepare.")
    } else {
        output.to_string()
    }
}

fn tool_status(status: &str) -> &'static str {
    match status {
        "completed" => "completed",
        "failed" | "declined" => "failed",
        _ => "in_progress",
    }
}

/// Un ítem de herramienta de Codex como `tool_call` de ACP; `None` si no es una.
fn tool_call(item: &Value) -> Option<Value> {
    let id = text(item, "id");
    let status = tool_status(text(item, "status"));
    let update = match text(item, "type") {
        "commandExecution" => {
            let command = shell_command(text(item, "command"));
            let actions = item.get("commandActions").and_then(Value::as_array).cloned().unwrap_or_default();
            let only = (actions.len() == 1).then(|| &actions[0]);
            let (kind, input) = match only.map(|a| text(a, "type")) {
                Some("read") => ("read", json!({ "path": only.map(|a| text(a, "path")) })),
                Some("search") => ("search", json!({ "pattern": only.map(|a| text(a, "query")), "path": only.and_then(|a| a.get("path")) })),
                _ => ("execute", json!({ "command": command })),
            };
            json!({
                "toolCallId": id, "kind": kind, "title": command, "rawInput": input, "status": status,
                "rawOutput": { "output": command_output(item.get("aggregatedOutput").and_then(Value::as_str).unwrap_or_default()) },
            })
        }
        "fileChange" => {
            let changes = item.get("changes").and_then(Value::as_array).cloned().unwrap_or_default();
            let content: Vec<Value> = changes
                .iter()
                .map(|change| {
                    let diff = text(change, "diff");
                    let added = change.pointer("/kind/type").and_then(Value::as_str) == Some("add");
                    let (old, new) = diff_sides(diff);
                    // Un archivo nuevo puede venir con su contenido tal cual, sin «+».
                    let (old, new) = if added { (String::new(), if new.is_empty() { diff.to_string() } else { new }) } else { (old, new) };
                    json!({ "type": "diff", "path": text(change, "path"), "oldText": old, "newText": new })
                })
                .collect();
            let title = match changes.as_slice() {
                [one] => text(one, "path").to_string(),
                many => format!("{} archivos", many.len()),
            };
            json!({ "toolCallId": id, "kind": "edit", "title": title, "content": content, "status": status })
        }
        "mcpToolCall" => {
            let output = match item.get("result") {
                Some(Value::Null) | None => item.get("error").map(|e| e.to_string()).unwrap_or_default(),
                Some(result) => result
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"))
                    .unwrap_or_else(|| result.to_string()),
            };
            json!({
                "toolCallId": id, "kind": "other", "title": format!("{} · {}", text(item, "server"), text(item, "tool")),
                "rawInput": item.get("arguments").cloned().unwrap_or(Value::Null), "status": status, "rawOutput": { "output": output },
            })
        }
        "webSearch" => json!({ "toolCallId": id, "kind": "web_search", "title": text(item, "query"), "rawInput": { "query": text(item, "query") }, "status": status }),
        _ => return None,
    };
    Some(json!({ "sessionUpdate": "tool_call", "toolCallId": update["toolCallId"], "title": update["title"], "kind": update["kind"], "status": update["status"], "rawInput": update.get("rawInput").cloned().unwrap_or(Value::Null), "rawOutput": update.get("rawOutput").cloned().unwrap_or(Value::Null), "content": update.get("content").cloned().unwrap_or(Value::Null) }))
}

fn chunk(thinking: bool, text: &str) -> Step {
    let kind = if thinking { "agent_thought_chunk" } else { "agent_message_chunk" };
    Step::Update(json!({ "sessionUpdate": kind, "content": { "type": "text", "text": text } }))
}

/// Un aviso de Codex como pasos para el chat.
pub(super) fn notification(state: &mut State, method: &str, params: &Value) -> Vec<Step> {
    match method {
        "item/agentMessage/delta" => {
            state.streamed.insert(text(params, "itemId").to_string());
            vec![chunk(false, text(params, "delta"))]
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            state.streamed.insert(text(params, "itemId").to_string());
            vec![chunk(true, text(params, "delta"))]
        }
        "item/started" | "item/completed" => {
            let item = params.get("item").unwrap_or(&Value::Null);
            let done = method == "item/completed";
            let id = text(item, "id");
            match text(item, "type") {
                "agentMessage" if done => {
                    let mut steps = Vec::new();
                    let message = text(item, "text");
                    if !state.streamed.contains(id) && !message.is_empty() {
                        steps.push(chunk(false, message));
                    }
                    steps.push(Step::Finish);
                    steps
                }
                "reasoning" if done && !state.streamed.contains(id) => {
                    let summary: Vec<&str> = item
                        .get("summary")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|part| part.as_str().or_else(|| part.get("text").and_then(Value::as_str)))
                        .collect();
                    let summary = summary.join("\n\n");
                    if summary.trim().is_empty() { Vec::new() } else { vec![chunk(true, &summary)] }
                }
                _ => tool_call(item).map(Step::Update).into_iter().collect(),
            }
        }
        "turn/plan/updated" => {
            let entries: Vec<Value> = params
                .get("plan")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|step| {
                    let status = match text(step, "status") {
                        "inProgress" => "in_progress",
                        "completed" => "completed",
                        _ => "pending",
                    };
                    json!({ "content": text(step, "step"), "status": status })
                })
                .collect();
            vec![Step::Update(json!({ "sessionUpdate": "plan", "entries": entries }))]
        }
        "thread/tokenUsage/updated" => {
            if let Some(total) = params.pointer("/tokenUsage/total/outputTokens").and_then(Value::as_u64) {
                state.output_total = total;
            }
            Vec::new()
        }
        "turn/completed" => {
            state.turn = None;
            state.streamed.clear();
            let turn = params.get("turn").unwrap_or(&Value::Null);
            let output = state.output_total.saturating_sub(state.output_at_start);
            match text(turn, "status") {
                "interrupted" => vec![Step::EndTurn(json!({ "stopReason": "cancelled" }), None)],
                "failed" => {
                    let message = turn.pointer("/error/message").and_then(Value::as_str).unwrap_or("Codex no pudo terminar el turno.");
                    vec![Step::EndTurn(Value::Null, Some(message.to_string()))]
                }
                _ => vec![Step::EndTurn(json!({ "stopReason": "end_turn", "usage": { "outputTokens": output } }), None)],
            }
        }
        _ => Vec::new(),
    }
}

impl Session {
    /// `initialize`, los modelos y el hilo: nuevo, o el anterior con `thread/resume`. Lo
    /// elegido antes del primer mensaje (`want`) va al abrir: el modelo y el modo en el
    /// hilo, el razonamiento en las opciones (Codex lo recibe en cada turno).
    pub(super) fn codex_open(&self, cwd: &PathBuf, resume: Option<&str>, want: Option<&Value>) -> Reply {
        let client = json!({ "name": "atic", "title": "Atic Code", "version": env!("CARGO_PKG_VERSION") });
        self.request_wait("initialize", json!({ "clientInfo": client }))?;
        self.write(&json!({ "method": "initialized" }))?;
        let models = self.request_wait("model/list", json!({})).ok().and_then(|r| r.get("data").and_then(Value::as_array).cloned()).unwrap_or_default();
        let wanted = |id: &str| want.and_then(|w| w.pointer(&format!("/config/{id}"))).and_then(Value::as_str).map(str::to_string);
        // Un modo elegido en el chat va al hilo; sin él, Codex usa el de su config.
        let chosen = want.and_then(|w| w.get("mode")).and_then(Value::as_str).filter(|m| MODES.iter().any(|(id, ..)| id == m)).map(str::to_string);
        let mut start = json!({ "cwd": cwd.to_string_lossy() });
        if let Some(mode) = &chosen {
            start["approvalPolicy"] = approval_policy(mode).into();
            start["sandbox"] = sandbox_mode(mode).into();
        }
        if let Some(model) = wanted("model") {
            start["model"] = model.into();
        }
        // La sesión de prueba no deja un hilo guardado.
        if self.quiet {
            start["ephemeral"] = true.into();
        }
        let opened = match resume {
            Some(id) => {
                let mut params = start.clone();
                params["threadId"] = id.into();
                params["excludeTurns"] = true.into();
                // Si ya no existe, uno nuevo.
                self.request_wait("thread/resume", params).or_else(|_| self.request_wait("thread/start", start))?
            }
            None => self.request_wait("thread/start", start)?,
        };
        let id = opened.pointer("/thread/id").and_then(Value::as_str).map(str::to_string).ok_or("Codex no abrió la conversación")?;
        let model = wanted("model").or_else(|| opened.get("model").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
        let effort = wanted("effort").or_else(|| opened.get("reasoningEffort").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
        let mode = chosen.unwrap_or_else(|| mode_of(&opened).to_string());
        let event = {
            let mut translate = self.translate.lock().unwrap();
            translate.config = config(&models, &model, &effort);
            translate.modes = modes(&mode);
            translate.config_event()
        };
        {
            let mut state = self.codex.lock().unwrap();
            state.models = models;
            state.mode_applied = mode;
        }
        self.send_all(vec![event]);
        *self.session_id.lock().unwrap() = Some(id.clone());
        Ok(json!({ "ok": true, "reused": true, "sessionId": id }))
    }

    /// Cambiar el modelo, el razonamiento o el modo: queda aquí y viaja en el próximo turno.
    pub(super) fn codex_configure(&self, method: &str, params: &Value) {
        let request = if method == "setMode" {
            json!({ "modeId": params.get("modeId").cloned().unwrap_or(Value::Null) })
        } else {
            json!({ "configId": params.get("configId").cloned().unwrap_or(Value::Null), "value": params.get("value").cloned().unwrap_or(Value::Null) })
        };
        let models = self.codex.lock().unwrap().models.clone();
        let event = {
            let mut translate = self.translate.lock().unwrap();
            translate.configured(&request, &Value::Null);
            refit(&mut translate, &models);
            translate.config_event()
        };
        self.send_all(vec![event]);
    }

    /// Un mensaje: `turn/start` con el modelo, el razonamiento y, si cambió, el modo. El
    /// fin del turno llega aparte (`turn/completed`).
    pub(super) fn codex_prompt(self: &Arc<Self>, text: String, images: Vec<Value>) {
        let mut input: Vec<Value> = images
            .iter()
            .map(|image| {
                let media = image.get("mediaType").and_then(Value::as_str).unwrap_or("image/png");
                let data = image.get("data").and_then(Value::as_str).unwrap_or_default();
                json!({ "type": "image", "url": format!("data:{media};base64,{data}") })
            })
            .collect();
        input.push(json!({ "type": "text", "text": text, "text_elements": [] }));
        let session = self.clone();
        std::thread::spawn(move || {
            let Some(thread) = session.wait_open() else {
                let error = format!("No se pudo abrir la sesión de {}.", session.agent.name);
                let events = session.translate.lock().unwrap().end_turn(&Value::Null, Some(error));
                session.send_all(events);
                return;
            };
            let mut params = json!({ "threadId": thread, "input": input });
            {
                let translate = session.translate.lock().unwrap();
                for (option, field) in [("model", "model"), ("effort", "effort")] {
                    if let Some(value) = translate.current(option).filter(|v| v.as_str().is_some_and(|s| !s.is_empty())) {
                        params[field] = value;
                    }
                }
                let mut state = session.codex.lock().unwrap();
                let mode = translate.modes.get("currentModeId").and_then(Value::as_str).unwrap_or(&state.mode_applied).to_string();
                if mode != state.mode_applied {
                    params["approvalPolicy"] = approval_policy(&mode).into();
                    params["sandboxPolicy"] = sandbox_policy(&mode);
                    state.mode_applied = mode;
                }
                state.output_at_start = state.output_total;
            }
            let reply = match session.request("turn/start", params) {
                Ok(receiver) => receiver.recv().unwrap_or_else(|_| Err(json!({ "message": "Codex se detuvo" }))),
                Err(error) => Err(json!({ "message": error })),
            };
            match reply {
                Ok(result) => {
                    session.codex.lock().unwrap().turn = result.pointer("/turn/id").and_then(Value::as_str).map(str::to_string);
                }
                Err(error) => {
                    let events = session.translate.lock().unwrap().end_turn(&Value::Null, Some(session.error_text(&error)));
                    session.send_all(events);
                }
            }
        });
    }

    /// Detener: `turn/interrupt` y los permisos pendientes, cancelados.
    pub(super) fn codex_cancel(&self) {
        let thread = self.session_id.lock().unwrap().clone();
        let turn = self.codex.lock().unwrap().turn.clone();
        if let (Some(thread), Some(turn)) = (thread, turn) {
            let _ = self.request("turn/interrupt", json!({ "threadId": thread, "turnId": turn }));
        }
        let asked: Vec<Asked> = self.asked.lock().unwrap().drain().map(|(_, asked)| asked).collect();
        for asked in asked {
            let _ = self.write(&json!({ "id": asked.rpc_id, "result": { "decision": "cancel" } }));
        }
    }

    /// La respuesta del usuario a un permiso de Codex.
    pub(super) fn codex_answer(&self, asked: Asked, allow: bool, always: bool) {
        let decision = match (allow, always) {
            (true, true) => "acceptForSession",
            (true, false) => "accept",
            (false, _) => "decline",
        };
        let _ = self.write(&json!({ "id": asked.rpc_id, "result": { "decision": decision } }));
    }

    pub(super) fn codex_incoming(&self, message: Value) {
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned().filter(|id| !id.is_null());
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method, id) {
            (None, Some(id)) => self.answered(&id, &message),
            // Un permiso: la herramienta (ya avisada al empezar) y la pregunta en el chat.
            (Some("item/commandExecution/requestApproval" | "item/fileChange/requestApproval"), Some(id)) => {
                let command = params.get("command").and_then(Value::as_str).map(shell_command);
                let mut call = json!({ "toolCallId": text(&params, "itemId") });
                if let Some(command) = command {
                    call["kind"] = "execute".into();
                    call["title"] = command.clone().into();
                    call["rawInput"] = json!({ "command": command });
                }
                let options = json!([{ "kind": "allow_once" }, { "kind": "allow_always" }, { "kind": "reject_once" }]);
                let (request, events) = self.translate.lock().unwrap().permission(&json!({ "toolCall": call, "options": options }));
                self.asked.lock().unwrap().insert(request, Asked { rpc_id: id, options: Vec::new() });
                self.send_all(events);
            }
            (Some(_), Some(id)) => {
                let _ = self.write(&json!({ "id": id, "error": { "code": -32601, "message": "No disponible" } }));
            }
            (Some(method), None) => {
                let steps = notification(&mut self.codex.lock().unwrap(), method, &params);
                for step in steps {
                    let events = {
                        let mut translate = self.translate.lock().unwrap();
                        match step {
                            Step::Update(update) => translate.update(&update),
                            Step::Finish => translate.finish_message(),
                            Step::EndTurn(result, error) => translate.end_turn(&result, error),
                        }
                    };
                    self.send_all(events);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_comando_se_ve_sin_powershell() {
        let raw = r#""C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -Command 'echo hola'"#;
        assert_eq!(shell_command(raw), "echo hola");
        assert_eq!(shell_command("ls -la"), "ls -la");
        assert_eq!(shell_command(r#"pwsh -Command 'echo ''x'''"#), "echo 'x'");
    }

    #[test]
    fn sin_modo_elegido_vale_el_de_la_config_de_codex() {
        assert_eq!(mode_of(&json!({ "sandbox": { "type": "dangerFullAccess" } })), "full-access");
        assert_eq!(mode_of(&json!({ "sandbox": { "type": "readOnly" } })), "read-only");
        assert_eq!(mode_of(&json!({ "sandbox": { "type": "workspaceWrite" } })), "workspace-write");
        assert!(command_output("Failed to create unified exec process: helper_unknown_error: setup refresh had errors").contains("Acceso total"));
        assert_eq!(command_output("hola"), "hola");
    }

    #[test]
    fn el_razonamiento_es_el_del_modelo() {
        let models = vec![
            json!({ "id": "a", "model": "a", "displayName": "A", "description": "", "hidden": false, "defaultReasoningEffort": "medium",
                    "supportedReasoningEfforts": [{ "reasoningEffort": "low", "description": "" }, { "reasoningEffort": "medium", "description": "" }] }),
            json!({ "id": "b", "model": "b", "displayName": "B", "description": "", "hidden": true, "defaultReasoningEffort": "low",
                    "supportedReasoningEfforts": [{ "reasoningEffort": "low", "description": "" }] }),
        ];
        let options = config(&models, "a", "ultra");
        assert_eq!(options[0]["options"].as_array().unwrap().len(), 1, "los ocultos no se ofrecen");
        assert_eq!(options[1]["currentValue"], "medium", "un nivel que el modelo no admite vuelve al suyo");
        assert_eq!(config(&models, "a", "low")[1]["currentValue"], "low");
        let unknown = config(&models, "z", "");
        assert_eq!(unknown[0]["options"][0]["value"], "z");
        assert_eq!(unknown.as_array().unwrap().len(), 1);
    }

    #[test]
    fn los_avisos_llegan_como_los_de_acp() {
        let mut state = State::default();
        let steps = notification(&mut state, "item/agentMessage/delta", &json!({ "itemId": "m", "delta": "Ho" }));
        assert_eq!(steps, vec![chunk(false, "Ho")]);
        // El mensaje completo ya llegó por trozos: solo se cierra.
        let done = notification(&mut state, "item/completed", &json!({ "item": { "type": "agentMessage", "id": "m", "text": "Hola" } }));
        assert_eq!(done, vec![Step::Finish]);
        // Sin trozos, el texto llega entero.
        let whole = notification(&mut state, "item/completed", &json!({ "item": { "type": "agentMessage", "id": "n", "text": "Listo" } }));
        assert_eq!(whole, vec![chunk(false, "Listo"), Step::Finish]);

        let started = notification(
            &mut state,
            "item/started",
            &json!({ "item": { "type": "commandExecution", "id": "c", "command": "pwsh -Command 'cargo test'", "commandActions": [], "status": "inProgress" } }),
        );
        let Step::Update(update) = &started[0] else { panic!() };
        assert_eq!(update["kind"], "execute");
        assert_eq!(update["rawInput"]["command"], "cargo test");
        assert_eq!(update["status"], "in_progress");

        state.output_total = 120;
        state.output_at_start = 20;
        let end = notification(&mut state, "turn/completed", &json!({ "turn": { "status": "completed" } }));
        assert_eq!(end, vec![Step::EndTurn(json!({ "stopReason": "end_turn", "usage": { "outputTokens": 100 } }), None)]);
        let stopped = notification(&mut state, "turn/completed", &json!({ "turn": { "status": "interrupted" } }));
        assert_eq!(stopped, vec![Step::EndTurn(json!({ "stopReason": "cancelled" }), None)]);
    }

    #[test]
    fn un_cambio_de_archivo_es_una_edicion() {
        let item = json!({ "type": "fileChange", "id": "f", "status": "completed", "changes": [
            { "path": "a.txt", "kind": { "type": "update" }, "diff": "@@ -1,2 +1,2 @@\n uno\n-dos\n+tres\n" },
        ] });
        let update = tool_call(&item).unwrap();
        assert_eq!(update["kind"], "edit");
        assert_eq!(update["content"][0]["oldText"], "uno\ndos");
        assert_eq!(update["content"][0]["newText"], "uno\ntres");
        let added = json!({ "type": "fileChange", "id": "g", "status": "completed", "changes": [
            { "path": "b.txt", "kind": { "type": "add" }, "diff": "+x\n" },
        ] });
        assert_eq!(tool_call(&added).unwrap()["content"][0]["oldText"], "");
    }
}
