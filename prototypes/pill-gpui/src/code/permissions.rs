//! Las solicitudes de permiso en Expressive, como en la referencia: todas apiladas al
//! final del hilo. La general (permitir, siempre, todo, rechazar con una
//! indicación), la de preguntas (AskUserQuestion) y la del plan (ExitPlanMode).

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, Div, Entity, FontWeight, SharedString};
use gpui_m3::{Button, ButtonSize, Card, ChoiceRow, TextField, Tone};
use serde_json::{json, Value};

use super::chat::{Chat, Permission, ToolCall};
use super::style::t;
use super::CodeView;

const DENIED: &str = "El usuario rechazó esta acción.";

/// Lo elegido en una tarjeta de preguntas: por pregunta, las opciones marcadas
/// y si está marcada «Otro».
#[derive(Default, Clone)]
pub struct AskPicks {
    pub picks: Vec<Vec<usize>>,
    pub other: Vec<bool>,
}

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// El título por defecto de la tarjeta, según la herramienta.
fn title_for(permission: &Permission) -> String {
    if let Some(title) = &permission.title {
        return title.clone();
    }
    let field = |name: &str| permission.input.get(name).and_then(Value::as_str).map(base_name).unwrap_or_default();
    match permission.tool.as_str() {
        "Bash" => "Claude quiere ejecutar un comando".into(),
        "Edit" | "MultiEdit" => format!("Claude quiere editar {}", field("file_path")),
        "NotebookEdit" => format!("Claude quiere editar {}", field("notebook_path")),
        "Write" => format!("Claude quiere escribir {}", field("file_path")),
        "Read" => format!("Claude quiere leer {}", field("file_path")),
        "WebFetch" => {
            let url = permission.input.get("url").and_then(Value::as_str).unwrap_or_default();
            match url.split("://").nth(1).and_then(|rest| rest.split('/').next()).filter(|host| !host.is_empty()) {
                Some(host) => format!("Claude quiere abrir {host}"),
                None => "Claude quiere abrir una página web".into(),
            }
        }
        "WebSearch" => "Claude quiere buscar en la web".into(),
        _ => format!("Claude quiere usar {}", permission.display_name.clone().unwrap_or_else(|| permission.tool.clone())),
    }
}

/// Las reglas que agrega «Permitir siempre», en texto: `Bash(cargo test:*)`.
pub(super) fn describe_suggestions(suggestions: &Value) -> String {
    let mut parts = Vec::new();
    for suggestion in suggestions.as_array().into_iter().flatten() {
        if let Some(rules) = suggestion.get("rules").and_then(Value::as_array) {
            for rule in rules {
                let tool = rule.get("toolName").and_then(Value::as_str).unwrap_or_default();
                match rule.get("ruleContent").and_then(Value::as_str) {
                    Some(content) => parts.push(format!("{tool}({content})")),
                    None => parts.push(tool.to_string()),
                }
            }
        } else if let Some(mode) = suggestion.get("mode").and_then(Value::as_str) {
            parts.push(format!("modo {mode}"));
        } else if let Some(dirs) = suggestion.get("directories").and_then(Value::as_array) {
            parts.extend(dirs.iter().filter_map(Value::as_str).map(str::to_string));
        }
    }
    parts.join(", ")
}

fn questions(permission: &Permission) -> Vec<&Value> {
    permission.input.get("questions").and_then(Value::as_array).map(|list| list.iter().collect()).unwrap_or_default()
}

fn options(question: &Value) -> Vec<(String, Option<String>)> {
    question
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|option| {
            let label = option.get("label").and_then(Value::as_str).unwrap_or_default().to_string();
            let description = option.get("description").and_then(Value::as_str).filter(|d| !d.is_empty()).map(str::to_string);
            (label, description)
        })
        .collect()
}

/// Una tecla sobre las solicitudes de permiso (Permission.tsx:90-96, 209-217, 305-312).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermKey {
    Enter,
    /// Ctrl+Enter: envía las respuestas de una pregunta desde cualquier parte.
    CtrlEnter,
    Escape,
}

/// Dónde se pulsó: en la caja del chat vacía o en el campo de la tarjeta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermAt {
    Composer,
    Field { empty: bool },
}

/// Lo que hace la tecla.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermAction {
    Allow,
    /// Rechaza (con la indicación del campo, si la hay).
    Deny,
    /// Envía las respuestas de AskUserQuestion.
    Submit,
    /// Cancela una pregunta.
    Cancel,
    /// «No, seguir planificando» (con la indicación, si la hay).
    KeepPlanning,
}

/// Como en la referencia: en un permiso, Enter permite (salvo `defaultToNo`), Esc
/// rechaza y Enter en el campo rechaza con el texto; en una pregunta, Esc
/// cancela y Enter en «Otro» o Ctrl+Enter envían si está completa; en el
/// plan, Esc y Enter en el campo con texto siguen planificando.
pub fn permission_key(tool: &str, default_to_no: bool, key: PermKey, at: PermAt, complete: bool) -> Option<PermAction> {
    match (tool, key, at) {
        ("AskUserQuestion", PermKey::Escape, _) => Some(PermAction::Cancel),
        ("AskUserQuestion", PermKey::CtrlEnter, _) | ("AskUserQuestion", PermKey::Enter, PermAt::Field { .. }) => {
            complete.then_some(PermAction::Submit)
        }
        ("AskUserQuestion", PermKey::Enter, PermAt::Composer) => None,
        ("ExitPlanMode", PermKey::Escape, _) => Some(PermAction::KeepPlanning),
        ("ExitPlanMode", PermKey::Enter | PermKey::CtrlEnter, PermAt::Field { empty: false }) => Some(PermAction::KeepPlanning),
        ("ExitPlanMode", _, _) => None,
        (_, PermKey::Escape, _) => Some(PermAction::Deny),
        (_, PermKey::Enter | PermKey::CtrlEnter, PermAt::Field { empty }) => (!empty).then_some(PermAction::Deny),
        (_, PermKey::Enter | PermKey::CtrlEnter, PermAt::Composer) => (!default_to_no).then_some(PermAction::Allow),
    }
}

impl CodeView {
    /// Responde un permiso con el resultado que arma `result`.
    fn reply_permission(&mut self, key: &str, request_id: &str, result: impl FnOnce(&Permission) -> Value, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
            return;
        };
        let Some(index) = chat.permissions.iter().position(|p| p.request_id == request_id) else {
            return;
        };
        let permission = chat.permissions.remove(index);
        let result = result(&permission);
        self.asks.remove(request_id);
        self.perm_feedback.update(cx, |field, cx| field.set_text("", cx));
        self.plan_feedback.update(cx, |field, cx| field.set_text("", cx));
        self.fire("permission", json!({ "key": key, "requestId": request_id, "result": result }), cx);
        cx.notify();
    }

    /// Una tecla sobre la solicitud `request` (o la primera) de la conversación
    /// visible; devuelve si hizo algo.
    pub(super) fn permission_keypress(&mut self, key: PermKey, at: PermAt, request: Option<&str>, cx: &mut Context<Self>) -> bool {
        let Some(chat) = self.active_chat() else {
            return false;
        };
        let Some(permission) = chat.permissions.iter().find(|p| request.is_none_or(|id| p.request_id == id)) else {
            return false;
        };
        let complete = permission.tool != "AskUserQuestion" || self.ask_complete(permission, cx);
        let Some(action) = permission_key(&permission.tool, permission.default_to_no, key, at, complete) else {
            return false;
        };
        let (key, request) = (chat.key.clone(), permission.request_id.clone());
        match action {
            PermAction::Allow => self.reply_permission(&key, &request, |p| json!({ "behavior": "allow", "updatedInput": p.input }), cx),
            PermAction::Deny => {
                let message = self.deny_message(&self.perm_feedback.clone(), DENIED, cx);
                self.reply_permission(&key, &request, |_| json!({ "behavior": "deny", "message": message }), cx)
            }
            PermAction::Submit => self.submit_answers(&key, &request, cx),
            PermAction::Cancel => {
                self.ask_other.retain(|(id, _), _| *id != request);
                self.reply_permission(&key, &request, |_| json!({ "behavior": "deny", "message": DENIED }), cx)
            }
            PermAction::KeepPlanning => {
                let message = self.deny_message(&self.plan_feedback.clone(), "El usuario quiere seguir planificando.", cx);
                self.reply_permission(&key, &request, |_| json!({ "behavior": "deny", "message": message }), cx)
            }
        }
        true
    }

    /// Cada pregunta tiene una opción o un «Otro» con texto.
    fn ask_complete(&self, permission: &Permission, cx: &Context<Self>) -> bool {
        let picks = self.asks.get(&permission.request_id).cloned().unwrap_or_default();
        (0..questions(permission).len()).all(|q| {
            let other_on = picks.other.get(q).copied().unwrap_or(false);
            let other_text = self.ask_other.get(&(permission.request_id.clone(), q)).map(|f| f.read(cx).text().trim().to_string()).unwrap_or_default();
            picks.picks.get(q).is_some_and(|p| !p.is_empty()) || (other_on && !other_text.is_empty())
        })
    }

    /// Enfocar el campo «Otro» de una pregunta lo deja elegido (como en la referencia).
    pub(super) fn pick_focused_other(&mut self, window: &gpui::Window, cx: &mut Context<Self>) {
        let focused: Vec<(String, usize)> =
            self.ask_other.iter().filter(|(_, field)| field.read(cx).is_focused(window)).map(|(slot, _)| slot.clone()).collect();
        for (request, q) in focused {
            let multi = self
                .active_chat()
                .and_then(|c| c.permissions.iter().find(|p| p.request_id == request))
                .and_then(|p| questions(p).get(q).and_then(|question| question.get("multiSelect")).and_then(Value::as_bool))
                .unwrap_or(false);
            let entry = self.asks.entry(request).or_default();
            entry.picks.resize(entry.picks.len().max(q + 1), Vec::new());
            entry.other.resize(entry.other.len().max(q + 1), false);
            if !entry.other[q] {
                entry.other[q] = true;
                if !multi {
                    entry.picks[q].clear();
                }
            }
        }
    }

    /// Los campos «Otro…»: Enter envía las respuestas y Esc cancela la pregunta.
    fn ask_field_event(&mut self, request: &str, event: &gpui_m3::TextFieldEvent, cx: &mut Context<Self>) {
        match event {
            gpui_m3::TextFieldEvent::Submitted(_) => {
                self.permission_keypress(PermKey::Enter, PermAt::Field { empty: false }, Some(request), cx);
            }
            gpui_m3::TextFieldEvent::Cancelled => {
                self.permission_keypress(PermKey::Escape, PermAt::Field { empty: true }, Some(request), cx);
            }
            gpui_m3::TextFieldEvent::Changed(_) => {}
        }
        cx.notify();
    }

    fn deny_message(&self, field: &Entity<TextField>, fallback: &str, cx: &Context<Self>) -> String {
        let text = field.read(cx).text().trim().to_string();
        if text.is_empty() { fallback.to_string() } else { text }
    }

    /// Las tarjetas de las solicitudes pendientes de `chat`, al final del hilo.
    pub(super) fn permission_cards_m3(&self, chat: &Chat, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(10.));
        for (index, permission) in chat.permissions.iter().enumerate() {
            // El campo de respuesta es uno: va en la primera tarjeta.
            let first = index == 0;
            list = list.child(match permission.tool.as_str() {
                "AskUserQuestion" => self.ask_card(&chat.key, permission, cx),
                "ExitPlanMode" => self.plan_card(&chat.key, permission, first, cx),
                _ => self.general_card(&chat.key, permission, first, cx),
            });
        }
        list
    }

    fn card_shell(id: SharedString, title: String) -> Card {
        Card::tonal(Tone::Tertiary)
            .id(id)
            .entrance(true)
            .padding_xy(px(18.), px(16.))
            .gap(px(10.))
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(title))
    }

    fn general_card(&self, key: &str, permission: &Permission, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let request = permission.request_id.clone();
        let mono = gpui_m3::theme::MONO_FONT_FAMILY;
        let tool = ToolCall::new(String::new(), permission.tool.clone(), (!permission.input.is_null()).then(|| permission.input.clone()));
        let description = permission
            .description
            .clone()
            .or_else(|| (permission.tool == "Bash").then(|| permission.input.get("description").and_then(Value::as_str).map(str::to_string)).flatten());
        let rules = (!permission.suppress_always).then(|| permission.suggestions.as_ref().map(describe_suggestions)).flatten().filter(|r| !r.is_empty());
        let feedback = first.then(|| self.perm_feedback.read(cx).text().trim().to_string()).unwrap_or_default();
        let notes = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .text_size(px(12.))
            .opacity(0.78)
            .when_some(description, |el, text| el.child(text))
            .when_some(permission.decision_reason.clone().filter(|r| Some(r) != permission.description.as_ref()), |el, text| el.child(text))
            .when_some(permission.blocked_path.clone(), |el, path| el.child(format!("Ruta: {path}")));

        let allow = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.reply_permission(&key, &request, |p| json!({ "behavior": "allow", "updatedInput": p.input }), cx)
            })
        };
        let always = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.reply_permission(
                    &key,
                    &request,
                    |p| json!({ "behavior": "allow", "updatedInput": p.input, "updatedPermissions": p.suggestions.clone().unwrap_or(json!([])) }),
                    cx,
                )
            })
        };
        let all = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.reply_permission(&key, &request, |p| json!({ "behavior": "allow", "updatedInput": p.input }), cx);
                view.set_config(|c| c.permission_mode = "bypassPermissions".into(), cx);
            })
        };
        let deny = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                let message = view.deny_message(&view.perm_feedback.clone(), DENIED, cx);
                view.reply_permission(&key, &request, |_| json!({ "behavior": "deny", "message": message }), cx)
            })
        };
        let id = |name: &str| SharedString::from(format!("perm-{name}-{request}"));
        // Con `defaultToNo`, lo destacado es rechazar y Enter no permite.
        let no_first = permission.default_to_no;
        let allow_button = Button::new(id("allow"), "Permitir").size(ButtonSize::Comfortable).on_click(allow);
        let deny_label = if feedback.is_empty() { "Rechazar" } else { "Rechazar y responder" };
        let deny_button = Button::new(id("deny"), deny_label).size(ButtonSize::Comfortable).on_click(deny);
        let actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(if no_first { allow_button.tonal() } else { allow_button.filled() })
            .when(rules.is_some(), |el| el.child(Button::new(id("always"), "Permitir siempre").tonal().size(ButtonSize::Comfortable).on_click(always)))
            .child(Button::new(id("all"), "Permitir todo").tonal().size(ButtonSize::Comfortable).on_click(all))
            .child(if no_first { deny_button.filled() } else { deny_button.outlined() })
            .when_some(rules, |el, rules| {
                el.child(div().flex_1().min_w(px(0.)).truncate().font_family(mono).text_size(px(11.)).text_color(t().faint).child(rules))
            });
        Self::card_shell(id("card"), title_for(permission))
            .child(super::view::tool_input(&tool))
            .child(notes)
            .when(first, |card| card.child(self.perm_feedback.clone()))
            .child(actions)
            .into_any_element()
    }

    fn plan_card(&self, key: &str, permission: &Permission, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let request = permission.request_id.clone();
        let plan = permission.input.get("plan").and_then(Value::as_str).unwrap_or_default().to_string();
        let plan_md = super::view::markdown(&format!("plan-{request}"), &plan, cx);
        let accept = |mode: &'static str| {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.reply_permission(&key, &request, |p| json!({ "behavior": "allow", "updatedInput": p.input }), cx);
                view.set_config(|c| c.permission_mode = mode.into(), cx);
            })
        };
        let keep = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                let message = view.deny_message(&view.plan_feedback.clone(), "El usuario quiere seguir planificando.", cx);
                view.reply_permission(&key, &request, |_| json!({ "behavior": "deny", "message": message }), cx)
            })
        };
        let id = |name: &str| SharedString::from(format!("plan-{name}-{request}"));
        let body = div()
            .id(id("body"))
            .max_h(px(380.))
            .overflow_y_scroll()
            .px(px(12.))
            .py(px(10.))
            .rounded(px(16.))
            .bg(t().editor)
            .text_color(t().text)
            .child(plan_md);
        Self::card_shell(id("card"), "Claude terminó de planificar. ¿Continuar?".into())
            .child(body)
            .when(first, |card| card.child(self.plan_feedback.clone()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(Button::new(id("edits"), "Sí, y aceptar ediciones").filled().size(ButtonSize::Comfortable).on_click(accept("acceptEdits")))
                    .child(Button::new(id("review"), "Sí, revisando cada edición").tonal().size(ButtonSize::Comfortable).on_click(accept("default")))
                    .child(Button::new(id("keep"), "No, seguir planificando").outlined().size(ButtonSize::Comfortable).on_click(keep)),
            )
            .into_any_element()
    }

    fn ask_card(&self, key: &str, permission: &Permission, cx: &mut Context<Self>) -> AnyElement {
        let request = permission.request_id.clone();
        let picks = self.asks.get(&request).cloned().unwrap_or_default();
        let list = questions(permission);
        let mut complete = true;
        let mut body = div().flex().flex_col().gap(px(10.));
        for (q, question) in list.iter().enumerate() {
            let multi = question.get("multiSelect").and_then(Value::as_bool).unwrap_or(false);
            let header = question.get("header").and_then(Value::as_str).unwrap_or_default().to_string();
            let text = question.get("question").and_then(Value::as_str).unwrap_or_default().to_string();
            let chosen = picks.picks.get(q).cloned().unwrap_or_default();
            let other_on = picks.other.get(q).copied().unwrap_or(false);
            let other_text = self.ask_other.get(&(request.clone(), q)).map(|f| f.read(cx).text().trim().to_string()).unwrap_or_default();
            if chosen.is_empty() && !(other_on && !other_text.is_empty()) {
                complete = false;
            }
            let mut block = div().flex().flex_col().gap(px(4.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mb(px(6.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(!header.is_empty(), |el| el.child(gpui_m3::Badge::tonal(Tone::Secondary, header).height(px(20.))))
                    .child(text),
            );
            for (o, (label, description)) in options(question).into_iter().enumerate() {
                let on = chosen.contains(&o);
                let row_id = SharedString::from(format!("ask-{request}-{q}-{o}"));
                let mut row = if multi { ChoiceRow::checkbox(row_id, label, on) } else { ChoiceRow::radio(row_id, label, on) };
                if let Some(description) = description {
                    row = row.description(description);
                }
                let request = request.clone();
                block = block.child(row.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    let entry = view.asks.entry(request.clone()).or_default();
                    entry.picks.resize(entry.picks.len().max(q + 1), Vec::new());
                    entry.other.resize(entry.other.len().max(q + 1), false);
                    let picked = &mut entry.picks[q];
                    if multi {
                        match picked.iter().position(|p| *p == o) {
                            Some(at) => {
                                picked.remove(at);
                            }
                            None => picked.push(o),
                        }
                    } else {
                        *picked = vec![o];
                        entry.other[q] = false;
                    }
                    cx.notify();
                })));
            }
            if let Some(field) = self.ask_other.get(&(request.clone(), q)) {
                let row_id = SharedString::from(format!("ask-{request}-{q}-other"));
                let row = if multi { ChoiceRow::checkbox(row_id, "Otro", other_on) } else { ChoiceRow::radio(row_id, "Otro", other_on) };
                let request = request.clone();
                block = block.child(row.trailing(div().flex_1().child(field.clone())).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    let entry = view.asks.entry(request.clone()).or_default();
                    entry.picks.resize(entry.picks.len().max(q + 1), Vec::new());
                    entry.other.resize(entry.other.len().max(q + 1), false);
                    entry.other[q] = !entry.other[q] || !multi;
                    if !multi {
                        entry.picks[q].clear();
                    }
                    cx.notify();
                })));
            }
            body = body.child(block);
        }
        let submit = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| view.submit_answers(&key, &request, cx))
        };
        let cancel = {
            let (key, request) = (key.to_string(), request.clone());
            cx.listener(move |view, _: &ClickEvent, _, cx| view.reply_permission(&key, &request, |_| json!({ "behavior": "deny", "message": DENIED }), cx))
        };
        let id = |name: &str| SharedString::from(format!("ask-{name}-{request}"));
        let title = permission.title.clone().unwrap_or_else(|| "Claude tiene preguntas".into());
        Self::card_shell(id("card"), title)
            .child(body)
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(Button::new(id("send"), "Enviar respuestas").filled().size(ButtonSize::Comfortable).disabled(!complete).on_click(submit))
                    .child(Button::new(id("cancel"), "Cancelar").outlined().size(ButtonSize::Comfortable).on_click(cancel)),
            )
            .into_any_element()
    }

    /// Manda las respuestas de AskUserQuestion: `answers` con «a, b, texto de Otro» por pregunta.
    fn submit_answers(&mut self, key: &str, request: &str, cx: &mut Context<Self>) {
        let Some(permission) = self.chats.iter().find(|c| c.key == key).and_then(|c| c.permissions.iter().find(|p| p.request_id == request)) else {
            return;
        };
        let picks = self.asks.get(request).cloned().unwrap_or_default();
        let mut answers = serde_json::Map::new();
        for (q, question) in questions(permission).iter().enumerate() {
            let labels: Vec<String> = options(question).into_iter().map(|(label, _)| label).collect();
            let mut chosen: Vec<String> = picks.picks.get(q).into_iter().flatten().filter_map(|o| labels.get(*o).cloned()).collect();
            if picks.other.get(q).copied().unwrap_or(false) {
                if let Some(field) = self.ask_other.get(&(request.to_string(), q)) {
                    let text = field.read(cx).text().trim().to_string();
                    if !text.is_empty() {
                        chosen.push(text);
                    }
                }
            }
            let text = question.get("question").and_then(Value::as_str).unwrap_or_default().to_string();
            answers.insert(text, json!(chosen.join(", ")));
        }
        let answers = Value::Object(answers);
        self.ask_other.retain(|(id, _), _| id != request);
        self.reply_permission(
            key,
            request,
            move |p| {
                let mut input = p.input.clone();
                input["answers"] = answers;
                json!({ "behavior": "allow", "updatedInput": input })
            },
            cx,
        );
    }

    /// Al llegar una pregunta se crean sus campos «Otro…».
    pub(super) fn prepare_permission(&mut self, key: &str, cx: &mut Context<Self>) {
        let pending: Vec<(String, usize)> = self
            .chats
            .iter()
            .filter(|c| c.key == key)
            .flat_map(|c| c.permissions.iter())
            .filter(|p| p.tool == "AskUserQuestion")
            .flat_map(|p| (0..questions(p).len()).map(move |q| (p.request_id.clone(), q)))
            .collect();
        for slot in pending {
            if self.ask_other.contains_key(&slot) {
                continue;
            }
            let field = cx.new(|cx| TextField::new(cx).bare().placeholder("Otro…"));
            let request = slot.0.clone();
            cx.subscribe(&field, move |view, _, event: &gpui_m3::TextFieldEvent, cx| view.ask_field_event(&request, event, cx)).detach();
            self.ask_other.insert(slot, field);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_teclado_de_un_permiso() {
        let key = |no: bool, key, at| permission_key("Bash", no, key, at, true);
        assert_eq!(key(false, PermKey::Enter, PermAt::Composer), Some(PermAction::Allow));
        // Si Claude Code sugiere rechazar, Enter no permite.
        assert_eq!(key(true, PermKey::Enter, PermAt::Composer), None);
        assert_eq!(key(false, PermKey::Escape, PermAt::Composer), Some(PermAction::Deny));
        assert_eq!(key(true, PermKey::Escape, PermAt::Field { empty: true }), Some(PermAction::Deny));
        // En el campo, Enter rechaza con el texto; vacío no hace nada.
        assert_eq!(key(false, PermKey::Enter, PermAt::Field { empty: false }), Some(PermAction::Deny));
        assert_eq!(key(false, PermKey::Enter, PermAt::Field { empty: true }), None);
    }

    #[test]
    fn el_teclado_de_una_pregunta_y_del_plan() {
        let ask = |key, at, complete| permission_key("AskUserQuestion", false, key, at, complete);
        assert_eq!(ask(PermKey::Escape, PermAt::Composer, false), Some(PermAction::Cancel));
        assert_eq!(ask(PermKey::Enter, PermAt::Composer, true), None);
        assert_eq!(ask(PermKey::CtrlEnter, PermAt::Composer, true), Some(PermAction::Submit));
        assert_eq!(ask(PermKey::CtrlEnter, PermAt::Composer, false), None);
        assert_eq!(ask(PermKey::Enter, PermAt::Field { empty: false }, true), Some(PermAction::Submit));
        let plan = |key, at| permission_key("ExitPlanMode", false, key, at, true);
        assert_eq!(plan(PermKey::Escape, PermAt::Composer), Some(PermAction::KeepPlanning));
        assert_eq!(plan(PermKey::Enter, PermAt::Composer), None);
        assert_eq!(plan(PermKey::Enter, PermAt::Field { empty: false }), Some(PermAction::KeepPlanning));
        assert_eq!(plan(PermKey::Enter, PermAt::Field { empty: true }), None);
    }
}
