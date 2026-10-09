//! Una conversación con Claude Code: lo que llega del sidecar convertido en
//! filas para dibujar.
//!
//! El texto llega en trozos (`stream`) y después entero (`assistant`). Lo que
//! se vio en trozos no se vuelve a agregar; del mensaje entero solo se toma el
//! `input` final de las herramientas. Lo que no pasó por el streaming (el
//! historial de una conversación guardada) se agrega del mensaje entero.
//!
//! Los subagentes (mensajes con `parent`) no se muestran: su trabajo aparece
//! como la herramienta que los lanzó.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::Value;

use super::config::{model_name, same_model, ClaudeConfig};

/// Lo que se guarda del resultado de una herramienta.
const MAX_RESULT: usize = 6000;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// El JSON del input mientras llega en trozos.
    pub partial: String,
    pub input: Option<Value>,
    pub result: Option<String>,
    pub is_error: bool,
}

impl ToolCall {
    /// Lo más corto que dice qué hace: la ruta, el comando o el patrón.
    pub fn summary(&self) -> String {
        let Some(input) = &self.input else {
            return String::new();
        };
        let field = |name: &str| input.get(name).and_then(Value::as_str).map(str::to_string);
        field("file_path")
            .or_else(|| field("notebook_path"))
            .or_else(|| field("command"))
            .or_else(|| field("pattern"))
            .or_else(|| field("url"))
            .or_else(|| field("query"))
            .or_else(|| field("description"))
            .or_else(|| field("prompt"))
            .map(|text| text.lines().next().unwrap_or_default().to_string())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User(String),
    Text(String),
    Thinking(String),
    Tool(ToolCall),
    /// El cierre de un turno: cuánto tardó y cuánto costó.
    Turn(String),
    /// La marca de cambio de modelo («Cambiado a Opus 4.5»).
    Model(String),
    Notice { text: String, error: bool },
}

#[derive(Clone, Debug)]
pub struct Permission {
    pub request_id: String,
    pub tool: String,
    pub input: Value,
    pub title: Option<String>,
    pub description: Option<String>,
    pub decision_reason: Option<String>,
    pub display_name: Option<String>,
    pub blocked_path: Option<String>,
    /// Claude Code no ofrece «Permitir siempre» para esta solicitud.
    pub suppress_always: bool,
    pub suggestions: Option<Value>,
}

impl Permission {
    fn from(data: &Value) -> Option<Self> {
        let text = |name: &str| data.get(name).and_then(Value::as_str).map(str::to_string);
        Some(Self {
            request_id: text("requestId")?,
            tool: text("toolName").unwrap_or_default(),
            input: data.get("input").cloned().unwrap_or(Value::Null),
            title: text("title"),
            description: text("description").or_else(|| text("decisionReason")),
            decision_reason: text("decisionReason"),
            display_name: text("displayName"),
            blocked_path: text("blockedPath"),
            suppress_always: data.get("suppressAlwaysAllowRule").and_then(Value::as_bool).unwrap_or(false),
            suggestions: data.get("suggestions").filter(|s| s.as_array().is_some_and(|a| !a.is_empty())).cloned(),
        })
    }
}

/// El modelo y el esfuerzo aplicados al proceso de una conversación (vacío:
/// los de Claude Code).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub model: String,
    pub effort: String,
}

pub struct Chat {
    /// La clave de la sesión en el sidecar.
    pub key: String,
    pub workspace: u64,
    pub cwd: PathBuf,
    pub session_id: Option<String>,
    pub title: String,
    pub items: Vec<Item>,
    /// Hay un turno en curso.
    pub busy: bool,
    /// La sesión está abierta en el sidecar.
    pub live: bool,
    pub permissions: Vec<Permission>,
    pub model: Option<String>,
    /// Modelo y esfuerzo elegidos para esta conversación (cada chat tiene los
    /// suyos, como en la referencia). Sin elegir, los del espacio.
    pub want_model: Option<String>,
    pub want_effort: Option<String>,
    /// Lo que tiene de verdad el proceso de esta conversación: `None` mientras
    /// no esté abierto o `start` no haya respondido.
    pub applied: Option<Applied>,
    /// El modelo elegido con el que se envió el último mensaje: si cambia,
    /// aparece la marca.
    pub sent_model: Option<String>,
    /// Contexto usado y máximo, según Claude Code.
    pub context: Option<(u64, u64)>,
    /// Costo y tokens (entrada, salida, caché) de toda la conversación.
    pub total_cost: f64,
    pub tokens: (u64, u64, u64),
    /// Subagentes y tareas en segundo plano (el mapa de agentes).
    pub tasks: Vec<super::usage::Task>,
    /// El estado del puente de Remote Control.
    pub remote_state: Option<String>,
    /// Terminó un turno mientras no se miraba (el punto de «sin leer»).
    pub unread: bool,
    /// El enlace de Remote Control, si está conectada.
    pub remote_url: Option<String>,
    /// Desde cuándo la ve Atic Code (el chip de duración, como en la referencia).
    pub seen_at: std::time::Instant,
    /// (mensaje, índice del bloque) → fila.
    blocks: HashMap<(String, u64), usize>,
    /// Mensajes que llegaron en trozos.
    streamed: HashSet<String>,
    /// Mensajes de subagentes.
    nested: HashSet<String>,
    /// id de la herramienta → fila.
    tools: HashMap<String, usize>,
    /// Avisos de «responde otro modelo» ya dados (respondido, elegido).
    model_warned: HashSet<(String, String)>,
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| (b.get("type").and_then(Value::as_str) == Some("text")).then(|| b.get("text")?.as_str()))
            .flatten()
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Lo que Claude Code mete como mensaje del usuario sin que lo haya escrito.
fn is_meta(text: &str) -> bool {
    let text = text.trim_start();
    text.is_empty()
        || text.starts_with("<command-")
        || text.starts_with("<local-command")
        || text.starts_with("<system-reminder>")
        || text.starts_with("Caveat:")
}

fn clip(text: String) -> String {
    if text.len() <= MAX_RESULT {
        return text;
    }
    let mut end = MAX_RESULT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn title_for(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("Conversación").trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

impl Chat {
    pub fn new(key: String, workspace: u64, cwd: PathBuf) -> Self {
        Self {
            key,
            workspace,
            cwd,
            session_id: None,
            title: "Nueva conversación".into(),
            items: Vec::new(),
            busy: false,
            live: false,
            permissions: Vec::new(),
            model: None,
            want_model: None,
            want_effort: None,
            applied: None,
            sent_model: None,
            context: None,
            total_cost: 0.,
            tokens: (0, 0, 0),
            tasks: Vec::new(),
            remote_state: None,
            unread: false,
            remote_url: None,
            seen_at: std::time::Instant::now(),
            blocks: HashMap::new(),
            streamed: HashSet::new(),
            nested: HashSet::new(),
            tools: HashMap::new(),
            model_warned: HashSet::new(),
        }
    }

    /// El modelo de esta conversación: el elegido en ella o, si no, el del espacio.
    pub fn chosen_model(&self, config: &ClaudeConfig) -> String {
        self.want_model.clone().unwrap_or_else(|| config.model.clone())
    }

    /// El esfuerzo de esta conversación: el elegido en ella o, si no, el del espacio.
    pub fn chosen_effort(&self, config: &ClaudeConfig) -> String {
        self.want_effort.clone().unwrap_or_else(|| config.effort.clone())
    }

    /// Al enviar con otro modelo que el del último mensaje, deja la marca
    /// «Cambiado a X» y recuerda el nuevo (`noteModelForSend` de la referencia).
    pub fn note_model_for_send(&mut self, list: &[(String, String)], model: &str) {
        if !self.items.is_empty() && self.sent_model.as_deref().is_some_and(|sent| !same_model(list, sent, model)) {
            self.items.push(Item::Model(model_name(list, model)));
        }
        self.sent_model = Some(model.to_string());
    }

    /// Se eligió otro modelo con la conversación ya empezada: el nombre que
    /// usará el próximo mensaje (`pendingModel` de la referencia).
    pub fn pending_model(&self, list: &[(String, String)], config: &ClaudeConfig) -> Option<String> {
        let sent = self.sent_model.as_deref()?;
        let chosen = self.chosen_model(config);
        (!self.items.is_empty() && !self.busy && !same_model(list, &chosen, sent)).then(|| model_name(list, &chosen))
    }

    /// Una respuesta de la conversación principal con otro modelo que el
    /// aplicado: avisa una vez y devuelve el modelo que hay que volver a
    /// aplicar (`checkModel` de la referencia).
    pub fn check_model(&mut self, list: &[(String, String)], model: &str) -> Option<String> {
        let want = self.applied.as_ref().map(|a| a.model.clone()).filter(|m| !m.is_empty() && m != "default")?;
        if model.is_empty() || model.starts_with('<') || same_model(list, model, &want) {
            return None;
        }
        if !self.model_warned.insert((model.to_string(), want.clone())) {
            return None;
        }
        let (got, chosen) = (model_name(list, model), model_name(list, &want));
        self.notice(format!("Esta respuesta la está dando {got}, no {chosen} (el elegido). Se volvió a aplicar {chosen} para lo que sigue."), true);
        Some(want)
    }

    pub fn push_user(&mut self, text: &str) {
        if self.items.iter().all(|i| !matches!(i, Item::User(_))) {
            self.title = title_for(text);
        }
        self.items.push(Item::User(text.to_string()));
        self.busy = true;
    }

    pub fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.items.push(Item::Notice { text: text.into(), error });
    }

    /// Un evento del sidecar para esta conversación.
    pub fn apply(&mut self, event: &str, data: &Value) {
        match event {
            "stream" => {
                for op in data.as_array().into_iter().flatten() {
                    self.stream(op);
                }
            }
            "assistant" => self.assistant(data, false),
            "user" => self.user(data, false),
            "result" => self.result(data),
            "permission" => {
                if let Some(permission) = Permission::from(data) {
                    self.permissions.push(permission);
                }
            }
            "permission_cancel" => {
                let id = data.get("requestId").and_then(Value::as_str);
                self.permissions.retain(|p| Some(p.request_id.as_str()) != id);
            }
            "session" => {
                self.session_id = data.get("sessionId").and_then(Value::as_str).map(str::to_string);
            }
            "init" => {
                self.model = data.get("model").and_then(Value::as_str).map(str::to_string);
            }
            "closed" => {
                self.live = false;
                self.applied = None;
                self.busy = false;
                self.permissions.clear();
                if let Some(error) = data.get("error").and_then(Value::as_str) {
                    self.notice(format!("La sesión se cerró: {error}"), true);
                }
            }
            "error" => {
                let message = data.get("message").and_then(Value::as_str).unwrap_or("Error del agente");
                self.notice(message, true);
            }
            _ => {}
        }
    }

    /// El historial de una conversación guardada (`getSessionMessages`).
    pub fn load_history(&mut self, messages: &[Value]) {
        for message in messages {
            let parent = message.get("parent_tool_use_id").cloned().unwrap_or(Value::Null);
            let inner = message.get("message").cloned().unwrap_or(Value::Null);
            let content = inner.get("content").cloned().unwrap_or(Value::Null);
            match message.get("type").and_then(Value::as_str) {
                Some("assistant") => {
                    let data = serde_json::json!({ "id": inner.get("id"), "content": content, "parent": parent });
                    self.assistant(&data, true);
                }
                Some("user") => {
                    let data = serde_json::json!({ "content": content, "parent": parent });
                    self.user(&data, true);
                }
                _ => {}
            }
        }
        if let Some(Item::User(first)) = self.items.iter().find(|i| matches!(i, Item::User(_))) {
            self.title = title_for(first);
        }
    }

    fn stream(&mut self, op: &Value) {
        let msg = op.get("msg").and_then(Value::as_str).unwrap_or_default().to_string();
        let index = op.get("index").and_then(Value::as_u64).unwrap_or(0);
        let nested = op.get("parent").is_some_and(|p| !p.is_null());
        match op.get("op").and_then(Value::as_str) {
            Some("message") => {
                if nested {
                    self.nested.insert(msg);
                } else {
                    self.streamed.insert(msg);
                }
            }
            Some("start") if !nested && !self.nested.contains(&msg) => {
                let item = match op.get("kind").and_then(Value::as_str) {
                    Some("text") => Item::Text(String::new()),
                    Some("thinking") => Item::Thinking(String::new()),
                    Some("tool_use") => {
                        let id = op.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                        self.tools.insert(id.clone(), self.items.len());
                        Item::Tool(ToolCall {
                            id,
                            name: op.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            partial: String::new(),
                            input: None,
                            result: None,
                            is_error: false,
                        })
                    }
                    _ => return,
                };
                self.streamed.insert(msg.clone());
                self.blocks.insert((msg, index), self.items.len());
                self.items.push(item);
            }
            Some("delta") => {
                let text = op.get("text").and_then(Value::as_str).unwrap_or_default();
                if let Some(&row) = self.blocks.get(&(msg, index)) {
                    match &mut self.items[row] {
                        Item::Text(t) | Item::Thinking(t) => t.push_str(text),
                        Item::Tool(tool) => tool.partial.push_str(text),
                        _ => {}
                    }
                }
            }
            Some("stop") => {
                if let Some(&row) = self.blocks.get(&(msg, index)) {
                    if let Item::Tool(tool) = &mut self.items[row] {
                        if tool.input.is_none() {
                            tool.input = serde_json::from_str(&tool.partial).ok();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn assistant(&mut self, data: &Value, history: bool) {
        if data.get("parent").is_some_and(|p| !p.is_null()) {
            return;
        }
        let id = data.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let streamed = !history && self.streamed.contains(&id);
        for block in data.get("content").and_then(Value::as_array).into_iter().flatten() {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let tool_id = block.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let input = block.get("input").cloned();
                    if let Some(&row) = self.tools.get(&tool_id) {
                        if let Item::Tool(tool) = &mut self.items[row] {
                            tool.input = input;
                        }
                    } else {
                        self.tools.insert(tool_id.clone(), self.items.len());
                        self.items.push(Item::Tool(ToolCall {
                            id: tool_id,
                            name: block.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            partial: String::new(),
                            input,
                            result: None,
                            is_error: false,
                        }));
                    }
                }
                Some("text") if !streamed => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
                    if !text.trim().is_empty() {
                        self.items.push(Item::Text(text.to_string()));
                    }
                }
                Some("thinking") if !streamed => {
                    let text = block.get("thinking").and_then(Value::as_str).unwrap_or_default();
                    if !text.trim().is_empty() {
                        self.items.push(Item::Thinking(text.to_string()));
                    }
                }
                _ => {}
            }
        }
    }

    fn user(&mut self, data: &Value, history: bool) {
        if data.get("parent").is_some_and(|p| !p.is_null()) {
            return;
        }
        let content = data.get("content").cloned().unwrap_or(Value::Null);
        for block in content.as_array().into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
            if let Some(&row) = self.tools.get(id) {
                if let Item::Tool(tool) = &mut self.items[row] {
                    let result = block.get("content").map(text_of).unwrap_or_default();
                    tool.result = Some(clip(result));
                    tool.is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                }
            }
        }
        // El eco de lo que se escribió aquí ya está en la lista.
        let replay = data.get("isReplay").and_then(Value::as_bool).unwrap_or(false);
        let synthetic = data.get("isSynthetic").and_then(Value::as_bool).unwrap_or(false);
        if replay || synthetic {
            return;
        }
        let text = text_of(&content);
        if !is_meta(&text) {
            self.items.push(Item::User(text));
            if !history {
                self.busy = true;
            }
        }
    }

    fn result(&mut self, data: &Value) {
        self.busy = false;
        self.permissions.clear();
        let is_error = data.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let subtype = data.get("subtype").and_then(Value::as_str).unwrap_or("success");
        if is_error || subtype != "success" {
            let message = data
                .get("errors")
                .and_then(Value::as_array)
                .and_then(|errors| errors.first())
                .and_then(Value::as_str)
                .or_else(|| data.get("result").and_then(Value::as_str))
                .unwrap_or(subtype);
            self.notice(message, true);
        }
        let seconds = data.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0) / 1000.0;
        let mut summary = format!("{seconds:.1} s");
        if let Some(cost) = data.get("costUsd").and_then(Value::as_f64) {
            summary.push_str(&format!(" · US$ {cost:.4}"));
        }
        self.items.push(Item::Turn(summary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat() -> Chat {
        Chat::new("c1".into(), 0, PathBuf::from(r"C:\repo"))
    }

    fn models() -> Vec<(String, String)> {
        [("", "Predeterminado"), ("opus", "Opus 4.5"), ("sonnet", "Sonnet 4.5")].iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
    }

    #[test]
    fn cada_conversacion_usa_su_modelo_o_el_del_espacio() {
        let config = ClaudeConfig { model: "sonnet".into(), effort: "high".into(), ..Default::default() };
        let mut chat = chat();
        assert_eq!((chat.chosen_model(&config), chat.chosen_effort(&config)), ("sonnet".into(), "high".into()));
        chat.want_model = Some("opus".into());
        chat.want_effort = Some("low".into());
        assert_eq!((chat.chosen_model(&config), chat.chosen_effort(&config)), ("opus".into(), "low".into()));
        // Elegir «Predeterminado» en la conversación no vuelve al del espacio.
        chat.want_model = Some(String::new());
        assert_eq!(chat.chosen_model(&config), "");
    }

    #[test]
    fn la_marca_de_modelo_aparece_al_enviar_con_otro() {
        let list = models();
        let config = ClaudeConfig::default();
        let mut chat = chat();
        chat.note_model_for_send(&list, "opus");
        assert!(chat.items.is_empty(), "el primer mensaje no lleva marca");
        chat.push_user("hola");
        chat.busy = false;
        // El id de la sesión retomada y el alias son el mismo modelo.
        chat.sent_model = Some("claude-opus-4-5-20251101".into());
        chat.want_model = Some("opus".into());
        assert_eq!(chat.pending_model(&list, &config), None);
        chat.note_model_for_send(&list, "opus");
        assert!(!chat.items.iter().any(|i| matches!(i, Item::Model(_))));
        chat.want_model = Some("sonnet".into());
        assert_eq!(chat.pending_model(&list, &config).as_deref(), Some("Sonnet 4.5"));
        chat.note_model_for_send(&list, "sonnet");
        assert_eq!(chat.items.last(), Some(&Item::Model("Sonnet 4.5".into())));
        assert_eq!(chat.pending_model(&list, &config), None);
    }

    #[test]
    fn otro_modelo_respondiendo_avisa_una_vez() {
        let list = models();
        let mut chat = chat();
        // Sin modelo aplicado (o el predeterminado) no hay con qué comparar.
        assert_eq!(chat.check_model(&list, "claude-sonnet-4-5"), None);
        chat.applied = Some(Applied { model: "opus".into(), effort: String::new() });
        assert_eq!(chat.check_model(&list, "claude-opus-4-5-20251101"), None);
        assert_eq!(chat.check_model(&list, "<synthetic>"), None);
        assert_eq!(chat.check_model(&list, "claude-sonnet-4-5").as_deref(), Some("opus"));
        assert_eq!(chat.check_model(&list, "claude-sonnet-4-5"), None);
        assert_eq!(chat.items.iter().filter(|i| matches!(i, Item::Notice { error: true, .. })).count(), 1);
    }

    #[test]
    fn el_texto_en_trozos_no_se_repite_con_el_mensaje_entero() {
        let mut chat = chat();
        chat.push_user("hola");
        chat.apply(
            "stream",
            &json!([
                { "op": "message", "msg": "m1", "parent": null },
                { "op": "start", "msg": "m1", "index": 0, "kind": "text", "parent": null },
                { "op": "delta", "msg": "m1", "index": 0, "text": "Hola, " },
                { "op": "delta", "msg": "m1", "index": 0, "text": "¿qué hacemos?" },
                { "op": "stop", "msg": "m1", "index": 0 }
            ]),
        );
        chat.apply("assistant", &json!({ "id": "m1", "parent": null, "content": [{ "type": "text", "text": "Hola, ¿qué hacemos?" }] }));
        assert_eq!(chat.items, vec![Item::User("hola".into()), Item::Text("Hola, ¿qué hacemos?".into())]);
        assert!(chat.busy);
        chat.apply("result", &json!({ "subtype": "success", "isError": false, "durationMs": 1500, "costUsd": 0.01 }));
        assert!(!chat.busy);
        assert_eq!(chat.items.last(), Some(&Item::Turn("1.5 s · US$ 0.0100".into())));
    }

    #[test]
    fn una_herramienta_junta_input_y_resultado() {
        let mut chat = chat();
        chat.apply(
            "stream",
            &json!([
                { "op": "message", "msg": "m1", "parent": null },
                { "op": "start", "msg": "m1", "index": 0, "kind": "tool_use", "id": "t1", "name": "Read", "parent": null },
                { "op": "delta", "msg": "m1", "index": 0, "text": "{\"file_path\":" },
                { "op": "delta", "msg": "m1", "index": 0, "text": "\"C:\\\\repo\\\\a.rs\"}" },
                { "op": "stop", "msg": "m1", "index": 0 }
            ]),
        );
        chat.apply("user", &json!({ "parent": null, "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": [{ "type": "text", "text": "fn main() {}" }] }] }));
        let Item::Tool(tool) = &chat.items[0] else { panic!("no es herramienta") };
        assert_eq!(tool.summary(), r"C:\repo\a.rs");
        assert_eq!(tool.result.as_deref(), Some("fn main() {}"));
        assert!(!tool.is_error);
        // El resultado de una herramienta no es un mensaje del usuario.
        assert_eq!(chat.items.len(), 1);
    }

    #[test]
    fn los_subagentes_y_los_ecos_no_aparecen() {
        let mut chat = chat();
        chat.apply("stream", &json!([
            { "op": "message", "msg": "sub", "parent": "t9" },
            { "op": "start", "msg": "sub", "index": 0, "kind": "text", "parent": "t9" },
            { "op": "delta", "msg": "sub", "index": 0, "text": "interno" }
        ]));
        chat.apply("user", &json!({ "parent": null, "isReplay": true, "content": "hola" }));
        chat.apply("assistant", &json!({ "id": "x", "parent": "t9", "content": [{ "type": "text", "text": "interno" }] }));
        assert!(chat.items.is_empty());
    }

    #[test]
    fn el_historial_arma_la_conversacion_sin_lo_interno() {
        let mut chat = chat();
        chat.load_history(&[
            json!({ "type": "user", "parent_tool_use_id": null, "message": { "role": "user", "content": "<command-name>/clear</command-name>" } }),
            json!({ "type": "user", "parent_tool_use_id": null, "message": { "role": "user", "content": "arregla el bug\ndel login" } }),
            json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "id": "m1", "content": [
                { "type": "text", "text": "Voy." },
                { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "cargo test" } }
            ] } }),
            json!({ "type": "user", "parent_tool_use_id": null, "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "ok", "is_error": true }] } }),
        ]);
        assert_eq!(chat.title, "arregla el bug");
        assert_eq!(chat.items.len(), 3);
        let Item::Tool(tool) = &chat.items[2] else { panic!("no es herramienta") };
        assert_eq!(tool.summary(), "cargo test");
        assert!(tool.is_error);
        assert!(!chat.busy);
    }

    #[test]
    fn un_permiso_llega_y_se_cancela() {
        let mut chat = chat();
        chat.apply("permission", &json!({ "requestId": "p1", "toolName": "Bash", "input": { "command": "rm x" }, "suggestions": [] }));
        assert_eq!(chat.permissions.len(), 1);
        assert!(chat.permissions[0].suggestions.is_none());
        chat.apply("permission_cancel", &json!({ "requestId": "p1" }));
        assert!(chat.permissions.is_empty());
    }
}
