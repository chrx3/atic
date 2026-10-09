//! Lo de la caja de texto que viene del agente, como en la referencia: el anillo de
//! contexto con «Cuenta y uso», el mapa de agentes (subagentes y tareas en
//! segundo plano) y el chip de Remote Control.

use std::time::Instant;

use gpui::{anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, Context, Corner, FontWeight, MouseButton, SharedString};
use gpui_m3::{Chip, IconButton, MorphDot, DotShape, Popover, Ring, WavyProgress};
use serde_json::{json, Value};

use super::chat::Chat;
use super::style::t;
use super::CodeView;

/// El panel abierto sobre la caja de texto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pop {
    Usage,
    Agents,
}

/// Una tarea de la conversación: un subagente o algo en segundo plano.
#[derive(Clone, Debug)]
pub struct Task {
    pub id: String,
    pub tool_use_id: Option<String>,
    pub description: String,
    pub kind: String,
    pub background: bool,
    pub status: String,
    pub started: Instant,
    pub tokens: Option<u64>,
    pub tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
    pub summary: Option<String>,
}

/// Lo que muestra «Cuenta y uso».
#[derive(Clone, Default, Debug)]
pub struct UsageInfo {
    pub email: Option<String>,
    pub plan: Option<String>,
    /// Ventana, porcentaje y cuándo se reinicia.
    pub windows: Vec<(String, f32, String)>,
    pub error: Option<String>,
}

const WINDOWS: [(&str, &str); 6] = [
    ("five_hour", "Sesión (5 h)"),
    ("seven_day", "Semana (7 días)"),
    ("seven_day_opus", "Semana · Opus"),
    ("seven_day_sonnet", "Semana · Sonnet"),
    ("seven_day_fable", "Semana · Fable"),
    ("seven_day_oauth_apps", "Semana · apps"),
];

/// 1.23M, 12.3k o el número; «—» sin dato.
pub(super) fn fmt(value: Option<u64>) -> String {
    match value {
        None => "—".into(),
        Some(n) if n >= 1_000_000 => format!("{:.2}M", n as f64 / 1e6),
        Some(n) if n >= 1_000 => format!("{:.1}k", n as f64 / 1e3),
        Some(n) => n.to_string(),
    }
}

fn resets_in(value: &Value) -> String {
    let at = match value {
        Value::String(text) => chrono::DateTime::parse_from_rfc3339(text).ok().map(|d| d.timestamp()),
        Value::Number(n) => n.as_i64().map(|n| if n > 10_000_000_000 { n / 1000 } else { n }),
        _ => None,
    };
    let Some(at) = at else {
        return "Se reinicia pronto".into();
    };
    let hours = (at - chrono::Utc::now().timestamp()) / 3600;
    if hours >= 48 {
        format!("Se reinicia en {} d", hours / 24)
    } else if hours >= 1 {
        format!("Se reinicia en {hours} h")
    } else {
        "Se reinicia pronto".into()
    }
}

fn duration(ms: Option<u64>) -> String {
    let Some(ms) = ms.filter(|ms| *ms > 0) else {
        return String::new();
    };
    let seconds = ms / 1000;
    if seconds < 60 { format!("{seconds} s") } else { format!("{} min {} s", seconds / 60, seconds % 60) }
}

/// «claude-haiku-4-5-2025…» → «Haiku 4.5».
fn short_model(id: &str) -> String {
    let parts: Vec<&str> = id.trim_start_matches("claude-").split('-').collect();
    match parts.as_slice() {
        [family, major, minor, ..] if major.chars().all(|c| c.is_ascii_digit()) && minor.len() <= 2 => {
            let mut name = family.to_string();
            name[..1].make_ascii_uppercase();
            format!("{name} {major}.{minor}")
        }
        _ => id.to_string(),
    }
}

impl Chat {
    /// Lo que la vista guarda de los eventos del agente: tokens y costo del
    /// turno, tareas y el estado de Remote Control.
    pub fn track(&mut self, event: &str, data: &Value) {
        let text = |name: &str| data.get(name).and_then(Value::as_str).map(str::to_string);
        match event {
            "result" => {
                self.total_cost += data.get("costUsd").and_then(Value::as_f64).unwrap_or(0.);
                if let Some(usage) = data.get("usage") {
                    let n = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
                    self.tokens.0 += n("input_tokens") + n("cache_creation_input_tokens");
                    self.tokens.1 += n("output_tokens");
                    self.tokens.2 += n("cache_read_input_tokens");
                }
            }
            "system" => match data.get("subtype").and_then(Value::as_str) {
                Some("bridge_state") => self.remote_state = text("state"),
                Some("task_started") => {
                    let Some(id) = text("task_id") else {
                        return;
                    };
                    self.tasks.retain(|t| t.id != id);
                    self.tasks.push(Task {
                        id,
                        tool_use_id: text("tool_use_id"),
                        description: text("description").unwrap_or_default(),
                        kind: text("subagent_type").or_else(|| text("task_type")).unwrap_or_default(),
                        background: data.get("is_backgrounded").and_then(Value::as_bool).unwrap_or(false),
                        status: "running".into(),
                        started: Instant::now(),
                        tokens: None,
                        tool_uses: None,
                        duration_ms: None,
                        summary: None,
                    });
                }
                Some(kind @ ("task_progress" | "task_notification")) => {
                    let Some(task) = text("task_id").and_then(|id| self.tasks.iter_mut().find(|t| t.id == id)) else {
                        return;
                    };
                    if let Some(usage) = data.get("usage") {
                        task.tokens = usage.get("total_tokens").and_then(Value::as_u64);
                        task.tool_uses = usage.get("tool_uses").and_then(Value::as_u64);
                        task.duration_ms = usage.get("duration_ms").and_then(Value::as_u64);
                    }
                    if kind == "task_notification" {
                        task.status = text("status").unwrap_or_else(|| "completed".into());
                        task.summary = text("summary");
                    } else if let Some(description) = text("description") {
                        task.description = description;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub fn running_tasks(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == "running").count()
    }
}

impl CodeView {
    /// Pide cuánto contexto lleva la conversación (no gasta tokens).
    pub(super) fn refresh_context(&mut self, key: &str, cx: &mut Context<Self>) {
        let key = key.to_string();
        self.request("context", json!({ "key": key }), cx, move |view, reply, _| {
            let Ok(reply) = reply else {
                return;
            };
            let n = |a: &str, b: &str| reply.get(a).or_else(|| reply.get(b)).and_then(Value::as_u64);
            if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
                chat.context = Some((n("totalTokens", "total_tokens").unwrap_or(0), n("maxTokens", "max_tokens").unwrap_or(200_000)));
            }
        });
    }

    pub(super) fn toggle_pop(&mut self, pop: Pop, cx: &mut Context<Self>) {
        self.menu = None;
        self.pop = if self.pop == Some(pop) { None } else { Some(pop) };
        if self.pop == Some(Pop::Usage) {
            self.load_usage(cx);
        }
        cx.notify();
    }

    fn load_usage(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_chat().filter(|c| c.live).map(|c| c.key.clone()) else {
            self.usage = Some(UsageInfo { error: Some("Empieza una conversación para ver el uso.".into()), ..Default::default() });
            return;
        };
        self.usage = Some(UsageInfo::default());
        self.refresh_context(&key, cx);
        self.request("account", json!({ "key": key }), cx, |view, reply, _| {
            let info = view.usage.get_or_insert_with(UsageInfo::default);
            match reply {
                Ok(account) => {
                    info.email = account.get("email").and_then(Value::as_str).map(str::to_string);
                    if info.plan.is_none() {
                        info.plan = account.get("subscriptionType").and_then(Value::as_str).map(plan_name);
                    }
                }
                Err(error) => info.error = Some(error),
            }
        });
        self.request("usage", json!({ "key": key }), cx, |view, reply, _| {
            let info = view.usage.get_or_insert_with(UsageInfo::default);
            let Ok(usage) = reply else {
                return;
            };
            if let Some(plan) = usage.get("subscription_type").and_then(Value::as_str) {
                info.plan = Some(plan_name(plan));
            }
            let limits = usage.get("rate_limits").cloned().unwrap_or(Value::Null);
            info.windows = WINDOWS
                .iter()
                .filter_map(|(id, label)| {
                    let window = limits.get(id)?;
                    let value = window.get("utilization").and_then(Value::as_f64)? as f32;
                    Some((label.to_string(), value, resets_in(window.get("resets_at").unwrap_or(&Value::Null))))
                })
                .collect();
        });
    }

    /// El anillo de contexto junto a enviar.
    pub(super) fn context_ring(&self, cx: &mut Context<Self>) -> AnyElement {
        let (used, max) = self.active_chat().and_then(|c| c.context).unwrap_or((0, 200_000));
        let value = if max > 0 { used as f32 / max as f32 } else { 0. };
        let pct = (value * 100.).round() as u32;
        div()
            .id("composer-context")
            .size(px(32.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(14.))
            .cursor_pointer()
            .when(self.pop == Some(Pop::Usage), |el| el.bg(t().hover))
            .tooltip(crate::hover::tip_text(format!("Contexto {pct}%").into()))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_pop(Pop::Usage, cx)))
            .child(Ring::new("context-ring", value).size(px(16.)).stroke(2.5))
            .into_any_element()
    }

    /// Los chips de agentes y Remote Control bajo la caja.
    pub(super) fn agent_chips(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let scheme = *gpui_m3::Theme::of(cx);
        let config = self.config();
        let chat = self.active_chat();
        let running = chat.map(Chat::running_tasks).unwrap_or(0);
        let label = if running == 1 { "1 agente".to_string() } else { format!("{running} agentes") };
        let agents = div().id("agents-chip-wrap").tooltip(crate::hover::tip("Mapa de agentes: subagentes y tareas en segundo plano")).child(
            Chip::new("agents-chip", label)
                .icon("layers")
                .filter(self.pop == Some(Pop::Agents))
                .show_check(false)
                .pulse_icon(running > 0)
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_pop(Pop::Agents, cx))),
        );
        let connected = chat.is_some_and(|c| c.remote_url.is_some() || c.remote_state.as_deref() == Some("connected"));
        let (dot, tip) = match (config.remote_control, connected) {
            (false, _) => (scheme.outline, "Activar Remote Control: seguir tus conversaciones desde claude.ai/code y la app móvil"),
            (true, true) => (scheme.success, "Conectada: esta conversación aparece en claude.ai/code y la app móvil (clic para desactivar)"),
            (true, false) => (scheme.warning, "Remote Control activo; conectando esta conversación…"),
        };
        let remote = div().id("rc-chip-wrap").tooltip(crate::hover::tip(tip)).child(
            Chip::new("rc-chip", "Remote Control")
                .leading(MorphDot::new("rc-dot", DotShape::Circle).size(px(7.)).color(dot))
                .filter(config.remote_control)
                .show_check(false)
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    let on = !view.config().remote_control;
                    view.set_config(|c| c.remote_control = on, cx);
                    view.show_toast(
                        if on { "Remote Control activado: tus conversaciones aparecen en claude.ai/code y la app" } else { "Remote Control desactivado" },
                        cx,
                    );
                })),
        );
        vec![agents.into_any_element(), remote.into_any_element()]
    }

    /// «Cuenta y uso» o el mapa de agentes, sobre la caja de texto a la derecha.
    pub(super) fn pop_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pop = self.pop?;
        let bounds = self.composer_bounds.get()?;
        let content = match pop {
            Pop::Usage => self.usage_popover(cx),
            Pop::Agents => self.agent_map(cx),
        };
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| {
                            view.pop = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        anchored()
                            .position(point(bounds.right(), bounds.top() - px(8.)))
                            .anchor(Corner::BottomRight)
                            .snap_to_window_with_margin(px(8.))
                            .child(div().id("pop-card").on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(content)),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn close_button(id: &'static str, cx: &mut Context<Self>) -> IconButton {
        IconButton::new(id, "x").size(px(32.)).tooltip("Cerrar").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
            view.pop = None;
            cx.notify();
        }))
    }

    fn usage_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let chat = self.active_chat();
        let context = chat.and_then(|c| c.context);
        let info = self.usage.clone().unwrap_or_default();
        let cell = |label: &'static str, value: String, bg: gpui::Hsla, pad: f32| {
            div()
                .flex_1()
                .min_w(px(0.))
                .px(px(pad + 2.))
                .py(px(8.))
                .rounded(px(14.))
                .bg(bg)
                .flex()
                .flex_col()
                .child(div().text_size(px(11.)).text_color(t.muted).child(label))
                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(value))
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(Ring::new("usage-ring", context.map_or(0., |(u, m)| if m > 0 { u as f32 / m as f32 } else { 0. })).size(px(56.)).stroke(7.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(20.)).font_weight(FontWeight(750.)).child("Cuenta y uso"))
                    .child(div().text_size(px(12.)).text_color(t.muted).child(match context {
                        Some((used, max)) => format!("Contexto · {} de {} tokens", fmt(Some(used)), fmt(Some(max))),
                        None => "Contexto · —".into(),
                    })),
            )
            .child(Self::close_button("usage-close", cx));
        let mut popover = Popover::new("usage").width(px(380.)).padding(px(18.)).radius(px(28.)).gap(px(14.)).max_h(px(520.)).child(head);
        if let Some(error) = info.error.clone() {
            popover = popover.child(div().text_size(px(12.)).text_color(t.muted).child(error));
        }
        if info.email.is_some() || info.plan.is_some() {
            popover = popover.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(cell("Cuenta", info.email.clone().unwrap_or_else(|| "—".into()), t.hover, 8.))
                    .child(cell("Plan", info.plan.clone().unwrap_or_else(|| "—".into()), t.hover, 8.)),
            );
        }
        for (index, (label, value, resets)) in info.windows.iter().enumerate() {
            popover = popover.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().flex().justify_between().child(label.clone()).child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{}%", value.round()))))
                    .child(WavyProgress::new(("usage-wave", index), Some(value / 100.)))
                    .child(div().text_size(px(12.)).text_color(t.muted).child(resets.clone())),
            );
        }
        let tokens = chat.map(|c| c.tokens);
        popover
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(cell("Entrada", fmt(tokens.map(|t| t.0)), t.pane, 6.))
                    .child(cell("Salida", fmt(tokens.map(|t| t.1)), t.pane, 6.))
                    .child(cell("Caché", fmt(tokens.map(|t| t.2)), t.pane, 6.))
                    .child(cell("Costo eq.", chat.map(|c| format!("${:.2}", c.total_cost)).unwrap_or_else(|| "—".into()), t.pane, 6.)),
            )
            .into_any_element()
    }

    fn agent_map(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let chat = self.active_chat();
        let tasks: Vec<super::usage::Task> = chat.map(|c| c.tasks.clone()).unwrap_or_default();
        let active = tasks.iter().filter(|t| t.status == "running").count();
        let head = div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(20.)).font_weight(FontWeight(750.)).child("Mapa de agentes"))
                    .child(div().text_size(px(12.)).text_color(t.muted).child(format!("{active} activos · {} en total", tasks.len() + 1))),
            )
            .child(Self::close_button("agents-close", cx));
        let title = chat.map(|c| c.title.clone()).filter(|t| t != "Nueva conversación").unwrap_or_else(|| "Conversación principal".into());
        let model = chat.and_then(|c| c.model.clone()).map(|m| short_model(&m)).unwrap_or_default();
        let main = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(14.))
            .bg(t.hover)
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().size(px(8.)).rounded_full().bg(if chat.is_some_and(|c| c.busy) { scheme.primary } else { scheme.success }))
            .child(div().flex_1().min_w(px(0.)).truncate().child(title))
            .child(div().text_size(px(11.5)).text_color(t.muted).font_weight(FontWeight::NORMAL).child(model));
        let popover = Popover::new("agent-map").width(px(420.)).padding(px(16.)).radius(px(28.)).gap(px(12.)).max_h(px(520.)).child(head).child(main);
        if tasks.is_empty() {
            return popover.child(div().text_color(t.muted).child("Sin subagentes ni tareas en segundo plano todavía")).into_any_element();
        }
        let mut branches = div().pl(px(22.)).flex().flex_col().gap(px(4.)).border_l(px(1.5)).border_color(t.border).ml(px(10.));
        for task in tasks {
            let (color, state) = match task.status.as_str() {
                "running" => (scheme.primary, "Trabajando"),
                "completed" => (scheme.success, "Listo"),
                "failed" => (scheme.error, "Falló"),
                _ => (scheme.outline, "Detenido"),
            };
            let open = self.expanded.contains(&format!("task-{}", task.id));
            let toggle = format!("task-{}", task.id);
            let duration_ms = task.duration_ms.or_else(|| (task.status == "running").then(|| task.started.elapsed().as_millis() as u64));
            let meta = [duration(duration_ms), task.tokens.map(|t| format!("{} tok", fmt(Some(t)))).unwrap_or_default()]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let mut hint = vec![task.kind.clone()];
            if task.background {
                hint.push("segundo plano".into());
            }
            hint.push(state.into());
            let running = task.status == "running";
            let (stop_id, background_id) = (task.id.clone(), task.tool_use_id.clone());
            branches = branches.child(
                div()
                    .id(SharedString::from(format!("task-row-{}", task.id)))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(14.))
                    .cursor_pointer()
                    .hover(|el| el.bg(t.hover))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        if !view.expanded.remove(&toggle) {
                            view.expanded.insert(toggle.clone());
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(MorphDot::new(SharedString::from(format!("task-dot-{}", task.id)), if running { DotShape::Square } else { DotShape::Circle }).size(px(8.)).color(color))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(div().line_clamp(2).child(if task.description.is_empty() { task.kind.clone() } else { task.description.clone() }))
                                    .child(div().text_size(px(11.5)).text_color(t.muted).child(hint.join(" · "))),
                            )
                            .child(div().text_size(px(11.5)).text_color(t.muted).child(meta)),
                    )
                    .when(open, |el| {
                        el.when_some(task.summary.clone(), |el, summary| el.child(div().text_size(px(12.5)).child(summary)))
                            .when_some(task.tool_uses, |el, n| el.child(div().text_size(px(12.)).text_color(t.muted).child(format!("{n} herramientas usadas"))))
                            .when(running, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .gap(px(6.))
                                        .when(!task.background && background_id.is_some(), |el| {
                                            let id = background_id.clone().unwrap_or_default();
                                            el.child(Chip::new(SharedString::from(format!("task-bg-{}", task.id)), "Pasar a segundo plano").on_click(cx.listener(
                                                move |view, _: &ClickEvent, _, cx| view.task_call("backgroundTasks", json!({ "toolUseId": id }), cx),
                                            )))
                                        })
                                        .child(Chip::new(SharedString::from(format!("task-stop-{}", task.id)), "Detener").on_click(cx.listener(
                                            move |view, _: &ClickEvent, _, cx| view.task_call("stopTask", json!({ "taskId": stop_id }), cx),
                                        ))),
                                )
                            })
                    }),
            );
        }
        popover.child(branches).into_any_element()
    }

    fn task_call(&mut self, method: &str, mut params: Value, cx: &mut Context<Self>) {
        let Some(key) = self.active_chat().filter(|c| c.live).map(|c| c.key.clone()) else {
            return;
        };
        params["key"] = json!(key);
        self.request(method, params, cx, |view, reply, cx| {
            if let Err(error) = reply {
                view.show_toast(error, cx);
            }
        });
    }
}

fn plan_name(plan: &str) -> String {
    let mut name = plan.to_string();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    format!("Claude {name}")
}
