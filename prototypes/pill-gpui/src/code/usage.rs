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

/// El nombre de un límite del formato nuevo de Claude Code (`limitLabel` de
/// la referencia): incluye los acotados a un modelo («Semana · Fable»).
fn limit_label(limit: &Value) -> String {
    let text = |value: Option<&Value>| value.and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string);
    let scope = limit.get("scope");
    let scoped = text(scope.and_then(|s| s.get("model")).and_then(|m| m.get("display_name"))).or_else(|| text(scope.and_then(|s| s.get("surface"))));
    let kind = text(limit.get("kind")).unwrap_or_default();
    let group = text(limit.get("group"));
    match (kind.as_str(), group.as_deref(), scoped) {
        ("session", _, _) => "Sesión (5 h)".into(),
        ("weekly_all", _, _) => "Semana (7 días)".into(),
        (_, Some("weekly"), Some(scoped)) => format!("Semana · {scoped}"),
        (_, Some("weekly"), None) => "Semana".into(),
        (_, Some("session"), Some(scoped)) => format!("Sesión · {scoped}"),
        (_, Some("session"), None) => "Sesión".into(),
        (kind, group, Some(scoped)) => format!("{} · {scoped}", group.unwrap_or(kind)),
        (kind, group, None) => group.unwrap_or(kind).to_string(),
    }
}

/// Las ventanas de uso de `rate_limits`: el formato nuevo (`limits`, con los
/// límites de cada modelo) o, si no viene, las ventanas de siempre.
/// Devuelve nombre, porcentaje y cuándo se reinicia.
pub(super) fn usage_windows(limits: &Value) -> Vec<(String, f32, String)> {
    if let Some(list) = limits.get("limits").and_then(Value::as_array).filter(|l| !l.is_empty()) {
        return list
            .iter()
            .filter_map(|limit| {
                let percent = limit.get("percent").and_then(Value::as_f64)? as f32;
                Some((limit_label(limit), percent, resets_in(limit.get("resets_at").unwrap_or(&Value::Null))))
            })
            .collect();
    }
    WINDOWS
        .iter()
        .filter_map(|(id, label)| {
            let window = limits.get(id)?;
            let value = window.get("utilization").and_then(Value::as_f64)? as f32;
            Some((label.to_string(), value, resets_in(window.get("resets_at").unwrap_or(&Value::Null))))
        })
        .collect()
}

/// El aviso de límite que dejó el último evento `rate_limit`: ventana, porcentaje
/// y si ya se rechazó. Solo existe mientras el servidor avisa (no «allowed»).
#[derive(Clone, Debug, PartialEq)]
pub struct RateAlert {
    pub label: String,
    pub percent: f32,
    pub rejected: bool,
}

/// El nombre de la ventana de un evento `rate_limit` (`rateLimitType`).
fn rate_label(kind: &str) -> String {
    WINDOWS
        .iter()
        .find(|(id, _)| *id == kind)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| match kind {
            "seven_day_overage_included" => "Semana · uso extra incluido".into(),
            "overage" => "Uso extra".into(),
            other => other.replace('_', " "),
        })
}

/// Aplica un evento `rate_limit` (`rate_limit_info` del SDK) a las ventanas de
/// «Cuenta y uso»: pone el porcentaje y el reinicio de la ventana que nombra,
/// o la agrega. `utilization` llega como fracción (0 a 1), no como porcentaje.
/// Devuelve el aviso que corresponde, o `None` si el servidor dice «allowed».
pub(super) fn apply_rate_limit(windows: &mut Vec<(String, f32, String)>, info: &Value) -> Option<RateAlert> {
    let label = rate_label(info.get("rateLimitType").and_then(Value::as_str)?);
    // El SDK no dice la escala: los encabezados de Claude Code la dan como fracción y el
    // método `usage` como porcentaje (29). Hasta 1 se toma por fracción.
    let percent = info.get("utilization").and_then(Value::as_f64).map(|u| if u > 1. { u as f32 } else { (u * 100.) as f32 });
    if let Some(percent) = percent {
        let resets = resets_in(info.get("resetsAt").unwrap_or(&Value::Null));
        match windows.iter_mut().find(|w| w.0 == label) {
            Some(window) => {
                window.1 = percent;
                if info.get("resetsAt").is_some() {
                    window.2 = resets;
                }
            }
            None => windows.push((label.clone(), percent, resets)),
        }
    }
    let rejected = match info.get("status").and_then(Value::as_str) {
        Some("rejected") => true,
        Some("allowed_warning") => false,
        _ => return None,
    };
    let percent = percent.or_else(|| windows.iter().find(|w| w.0 == label).map(|w| w.1)).unwrap_or(if rejected { 100. } else { 0. });
    Some(RateAlert { label, percent, rejected })
}

/// «12 s», «3 min 4 s», «1 h 5 min» (el `duration` de la referencia).
fn duration(ms: Option<u64>) -> String {
    let Some(ms) = ms.filter(|ms| *ms > 0) else {
        return String::new();
    };
    let seconds = (ms as f64 / 1000.).round() as u64;
    let minutes = seconds / 60;
    if seconds < 60 {
        format!("{seconds} s")
    } else if minutes < 60 {
        format!("{minutes} min {} s", seconds % 60)
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

/// El tipo de una tarea en palabras (`TASK_KINDS` de la referencia).
fn task_kind(kind: &str, background: bool) -> String {
    let base = match kind {
        "local_bash" => "Comando".to_string(),
        "local_agent" => "Subagente".to_string(),
        "remote_agent" => "Subagente remoto".to_string(),
        "monitor" => "Monitor".to_string(),
        "" => "Tarea".to_string(),
        "general-purpose" => "Subagente · General".to_string(),
        other => format!("Subagente · {other}"),
    };
    if background { format!("{base} en segundo plano") } else { base }
}

fn task_state(status: &str) -> &'static str {
    match status {
        "running" => "Trabajando",
        "completed" => "Listo",
        "failed" => "Falló",
        _ => "Detenido",
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
                if let Some(ms) = data.get("durationMs").and_then(Value::as_u64) {
                    self.last_duration_ms = Some(ms);
                }
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

    /// El nombre de una tarea: la descripción corta que Claude le puso a la
    /// herramienta si la tiene (los comandos en segundo plano traen el comando
    /// entero como descripción), si no la de la tarea.
    pub fn task_label(&self, task: &Task) -> String {
        let from_tool = task.tool_use_id.as_deref().and_then(|id| {
            self.items.iter().find_map(|item| match item {
                super::chat::Item::Tool(tool) if tool.id == id => {
                    tool.input.as_ref()?.get("description")?.as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string)
                }
                _ => None,
            })
        });
        from_tool.unwrap_or_else(|| if task.description.is_empty() { task_kind(&task.kind, task.background) } else { task.description.clone() })
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
        if self.was_dismissed(super::Dismissed::Pop(pop)) {
            return;
        }
        self.menu = None;
        self.pop = if self.pop == Some(pop) { None } else { Some(pop) };
        if self.pop == Some(Pop::Usage) {
            self.load_usage(cx);
        }
        if self.pop == Some(Pop::Agents) {
            self.start_agent_clock(cx);
        }
        cx.notify();
    }

    /// El reloj del mapa de agentes: redibuja cada segundo mientras está
    /// abierto y algo trabaja (el tiempo en vivo de cada fila).
    fn start_agent_clock(&mut self, cx: &mut Context<Self>) {
        self.agent_clock += 1;
        let generation = self.agent_clock;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            let alive = this.update(cx, |view, cx| {
                if view.agent_clock != generation || view.pop != Some(Pop::Agents) {
                    return false;
                }
                if view.active_chat().is_some_and(|c| c.running_tasks() > 0) {
                    cx.notify();
                }
                true
            });
            if !matches!(alive, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    /// El evento `rate_limit` del sidecar: actualiza «Cuenta y uso» si está
    /// abierto o ya cargado, y deja o quita el chip de aviso (`agent.ts:383`).
    pub(super) fn read_rate_limit(&mut self, info: &Value) {
        let mut windows = self.usage.as_ref().map(|u| u.windows.clone()).unwrap_or_default();
        let alert = apply_rate_limit(&mut windows, info);
        // Sin la cuenta cargada, las ventanas llegarán enteras con la primera
        // lectura de `usage`: no se arma un `UsageInfo` a medias.
        if let Some(usage) = self.usage.as_mut() {
            usage.windows = windows;
        }
        match alert {
            Some(alert) => self.rate_alert = Some(alert),
            None => {
                let label = info.get("rateLimitType").and_then(Value::as_str).map(rate_label);
                if self.rate_alert.as_ref().is_some_and(|a| Some(&a.label) == label.as_ref()) {
                    self.rate_alert = None;
                }
            }
        }
    }

    fn load_usage(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.info_key() else {
            self.ensure_probe(cx);
            self.usage = Some(UsageInfo { error: Some("Conectando con Claude Code…".into()), ..Default::default() });
            return;
        };
        self.usage = Some(UsageInfo::default());
        // El contexto es el de una conversación; la de sondeo no tiene.
        if key != super::PROBE {
            self.refresh_context(&key, cx);
        }
        self.request("account", json!({ "key": key }), cx, |view, reply, _| {
            let info = view.usage.get_or_insert_with(UsageInfo::default);
            match reply {
                Ok(account) => {
                    info.email = account.get("email").and_then(Value::as_str).map(super::config::mask_emails);
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
            info.windows = usage_windows(usage.get("rate_limits").unwrap_or(&Value::Null));
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
        let mut chips = vec![agents.into_any_element(), remote.into_any_element()];
        // El servidor avisó que la ventana se acerca al límite o ya lo alcanzó.
        if let Some(alert) = &self.rate_alert {
            let tip = if alert.rejected { "Límite alcanzado: clic para ver cuándo se reinicia" } else { "Te acercas al límite de uso: clic para ver el detalle" };
            let chip = Chip::new("rate-chip", format!("{} · {}%", alert.label, alert.percent.round()))
                .icon("gauge")
                .filter(self.pop == Some(Pop::Usage))
                .show_check(false)
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_pop(Pop::Usage, cx)));
            chips.push(div().id("rate-chip-wrap").tooltip(crate::hover::tip(tip)).child(chip).into_any_element());
        }
        chips
    }

    /// «Cuenta y uso» o el mapa de agentes, sobre la caja de texto a la derecha.
    pub(super) fn pop_layer(&self, window: &mut gpui::Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.pop_last.show("pop-presence", self.pop.filter(|_| self.composer_bounds.get().is_some()), window, cx)?;
        let pop = shown.value;
        let bounds = self.composer_bounds.get()?;
        let leaving = shown.leaving;
        let content = match pop {
            Pop::Usage => self.usage_popover(cx),
            Pop::Agents => self.agent_map(cx),
        };
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                if let Some(pop) = view.pop.take() {
                                    view.note_dismiss(super::Dismissed::Pop(pop));
                                }
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored()
                            .position(point(bounds.right(), bounds.top() - px(8.)))
                            .anchor(Corner::BottomRight)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .id("pop-card")
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .child(shown.wrap(gpui_m3::Exit::Rise, content)),
                            ),
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
                    .min_w(px(0.))
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
        // Lo más reciente primero, como en la referencia.
        let running: Vec<&Task> = tasks.iter().filter(|t| t.status == "running").rev().collect();
        let finished: Vec<&Task> = tasks.iter().filter(|t| t.status != "running").rev().collect();
        let failed = finished.iter().filter(|t| t.status == "failed").count();
        let count = |text: String, color: gpui::Hsla, dot: bool| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(px(12.))
                .text_color(color)
                .when(dot, |el| el.child(div().size(px(6.)).rounded_full().bg(color)))
                .child(text)
        };
        let head = div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(20.)).font_weight(FontWeight(750.)).child("Agentes"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(count(format!("{} trabajando", running.len()), if running.is_empty() { t.muted } else { scheme.primary }, true))
                            .child(count(format!("{} terminados", finished.len() - failed), t.muted, false))
                            .when(failed > 0, |el| el.child(count(format!("{failed} fallaron"), scheme.error, false))),
                    ),
            )
            .child(Self::close_button("agents-close", cx));
        let title = chat.map(|c| c.title.clone()).filter(|t| t != "Nueva conversación").unwrap_or_else(|| "Conversación principal".into());
        let model = chat.and_then(|c| c.model.clone()).map(|m| super::config::model_name(&[], &m)).unwrap_or_default();
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
            .child(div().flex_none().text_size(px(11.5)).text_color(t.muted).font_weight(FontWeight::NORMAL).child(model));
        let popover = Popover::new("agent-map").width(px(420.)).padding(px(16.)).radius(px(28.)).gap(px(12.)).max_h(px(520.)).child(head).child(main);
        if tasks.is_empty() {
            return popover.child(div().text_color(t.muted).child("Sin subagentes ni tareas en segundo plano todavía")).into_any_element();
        }
        let section = |label: String| div().px(px(4.)).text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(t.accent).child(label);
        let mut working = div().flex().flex_col().gap(px(4.));
        if running.is_empty() {
            working = working.child(div().px(px(10.)).text_size(px(12.5)).text_color(t.muted).child("Nada trabajando ahora"));
        }
        for task in &running {
            working = working.child(self.task_row(task, cx));
        }
        let mut popover = popover.child(section("Trabajando ahora".into())).child(working);
        if !finished.is_empty() {
            let open = self.agents_done_open;
            popover = popover.child(
                div()
                    .id("agents-done")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.agents_done_open = !view.agents_done_open;
                        cx.notify();
                    }))
                    .child(section(format!("Terminados ({})", finished.len())))
                    .child(gpui_m3::Icon::new(if open { "chevron-down" } else { "chevron-right" }).size(px(13.)).color(t.accent)),
            );
            if open {
                let mut done = div().flex().flex_col().gap(px(4.));
                for task in &finished {
                    done = done.child(self.task_row(task, cx));
                }
                popover = popover.child(done);
            }
        }
        popover.into_any_element()
    }

    /// Una fila del mapa de agentes: lo que hace, su tipo, el tiempo (en vivo
    /// si trabaja) y los tokens; Detener a la mano y el detalle al abrirla.
    fn task_row(&self, task: &Task, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let chat = self.active_chat();
        let live = task.status == "running";
        let color = match task.status.as_str() {
            "running" => scheme.primary,
            "completed" => scheme.success,
            "failed" => scheme.error,
            _ => scheme.outline,
        };
        let open = self.expanded.contains(&format!("task-{}", task.id));
        let toggle = format!("task-{}", task.id);
        let duration_ms = if live { Some(task.started.elapsed().as_millis() as u64) } else { task.duration_ms };
        let meta = [duration(duration_ms), task.tokens.map(|t| format!("{} tok", fmt(Some(t)))).unwrap_or_default()]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let mut sub = task_kind(&task.kind, task.background);
        if !live {
            sub = format!("{sub} · {}", task_state(&task.status));
        }
        let label = chat.map(|c| c.task_label(task)).unwrap_or_else(|| task.description.clone());
        let (stop_id, background_id) = (task.id.clone(), task.tool_use_id.clone());
        let can_background = live && !task.background && background_id.is_some();
        let has_detail = task.summary.is_some() || task.tool_uses.is_some() || can_background;
        let marker = if live && super::view::expressive() {
            gpui_m3::LoadingIndicator::new().size(px(16.)).into_any_element()
        } else {
            MorphDot::new(SharedString::from(format!("task-dot-{}", task.id)), if live { DotShape::Square } else { DotShape::Circle })
                .size(px(8.))
                .color(color)
                .into_any_element()
        };
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
            .tooltip(crate::hover::tip_text(SharedString::from(task.description.clone())))
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
                    .child(div().w(px(16.)).flex_none().flex().justify_center().child(marker))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(div().line_clamp(2).child(label))
                            .child(div().text_size(px(11.5)).text_color(t.muted).child(sub)),
                    )
                    .child(div().flex_none().text_size(px(11.5)).text_color(t.muted).child(meta))
                    .when(live, |el| {
                        el.child(
                            IconButton::new(SharedString::from(format!("task-stop-{}", task.id)), "stop").size(px(28.)).tooltip("Detener").on_click(cx.listener(
                                move |view, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    view.task_call("stopTask", json!({ "taskId": stop_id }), cx);
                                },
                            )),
                        )
                    }),
            )
            .when(open, |el| {
                el.when_some(task.summary.clone(), |el, summary| el.child(div().text_size(px(12.5)).child(summary)))
                    .when_some(task.tool_uses, |el, n| el.child(div().text_size(px(12.)).text_color(t.muted).child(format!("{n} herramientas usadas"))))
                    .when(can_background, |el| {
                        let id = background_id.clone().unwrap_or_default();
                        el.child(div().flex().child(Chip::new(SharedString::from(format!("task-bg-{}", task.id)), "Pasar a segundo plano").on_click(
                            cx.listener(move |view, _: &ClickEvent, _, cx| view.task_call("backgroundTasks", json!({ "toolUseId": id }), cx)),
                        )))
                    })
                    .when(!has_detail, |el| el.child(div().text_size(px(12.)).text_color(t.muted).child(task.description.clone())))
            })
            .into_any_element()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_formato_nuevo_de_los_limites() {
        let limits = json!({
            "five_hour": { "utilization": 99.0 },
            "limits": [
                { "kind": "session", "group": "session", "percent": 12.0, "resets_at": null },
                { "kind": "weekly_all", "group": "weekly", "percent": 40.5 },
                { "kind": "weekly_model", "group": "weekly", "percent": 70.0, "scope": { "model": { "display_name": "Fable" } } },
                { "kind": "weekly_surface", "group": "weekly", "percent": 5.0, "scope": { "model": null, "surface": "apps" } },
                { "kind": "other", "percent": null }
            ]
        });
        let windows: Vec<(String, f32)> = usage_windows(&limits).into_iter().map(|(label, value, _)| (label, value)).collect();
        // Con `limits` no se miran las ventanas viejas, y los sin porcentaje se saltan.
        assert_eq!(
            windows,
            vec![("Sesión (5 h)".into(), 12.0), ("Semana (7 días)".into(), 40.5), ("Semana · Fable".into(), 70.0), ("Semana · apps".into(), 5.0)]
        );
    }

    #[test]
    fn el_evento_rate_limit_actualiza_las_ventanas_y_avisa() {
        let mut windows = vec![("Sesión (5 h)".to_string(), 30.0, "Se reinicia en 2 h".to_string())];
        // Aviso en una ventana que ya está: cambia su porcentaje (la fracción se vuelve %).
        let info = json!({ "status": "allowed_warning", "rateLimitType": "five_hour", "utilization": 0.85, "resetsAt": chrono::Utc::now().timestamp() + 3 * 3600 + 60 });
        let alert = apply_rate_limit(&mut windows, &info).unwrap();
        assert_eq!((alert.label.as_str(), alert.percent.round(), alert.rejected), ("Sesión (5 h)", 85., false));
        assert_eq!(windows.len(), 1);
        assert_eq!((windows[0].1.round(), windows[0].2.as_str()), (85., "Se reinicia en 3 h"));
        // Una ventana nueva se agrega; rechazada sin porcentaje conocido es 100 %.
        let info = json!({ "status": "rejected", "rateLimitType": "seven_day_opus" });
        let alert = apply_rate_limit(&mut windows, &info).unwrap();
        assert!(alert.rejected && alert.percent == 100.);
        assert_eq!(windows.len(), 1);
        let info = json!({ "status": "allowed", "rateLimitType": "seven_day", "utilization": 0.4 });
        assert!(apply_rate_limit(&mut windows, &info).is_none());
        assert_eq!(windows[1].0, "Semana (7 días)");
        assert_eq!(windows[1].1.round(), 40.);
        // Sin tipo no hay a qué ventana aplicarlo.
        assert!(apply_rate_limit(&mut windows, &json!({ "status": "rejected" })).is_none());
    }

    #[test]
    fn sin_limits_quedan_las_ventanas_de_siempre() {
        let limits = json!({ "five_hour": { "utilization": 30.0 }, "seven_day_opus": { "utilization": 10.0 }, "limits": [] });
        let windows: Vec<(String, f32)> = usage_windows(&limits).into_iter().map(|(label, value, _)| (label, value)).collect();
        assert_eq!(windows, vec![("Sesión (5 h)".into(), 30.0), ("Semana · Opus".into(), 10.0)]);
        assert!(usage_windows(&Value::Null).is_empty());
    }

    #[test]
    fn el_tipo_y_el_nombre_de_cada_tarea() {
        assert_eq!(task_kind("local_bash", true), "Comando en segundo plano");
        assert_eq!(task_kind("general-purpose", false), "Subagente · General");
        assert_eq!(task_kind("Explore", false), "Subagente · Explore");
        assert_eq!(task_kind("", false), "Tarea");
        assert_eq!(duration(Some(3_725_000)), "1 h 2 min");

        let mut chat = Chat::new("c1".into(), 0, std::path::PathBuf::from("."));
        chat.items.push(super::super::chat::Item::Tool(super::super::chat::ToolCall::new(
            "tool-1".into(),
            "Bash".into(),
            Some(json!({ "command": "npm run build -- --watch", "description": "Compila en modo vigilancia" })),
        )));
        let task = |tool: Option<&str>, description: &str| Task {
            id: "t".into(),
            tool_use_id: tool.map(str::to_string),
            description: description.into(),
            kind: "local_bash".into(),
            background: true,
            status: "running".into(),
            started: Instant::now(),
            tokens: None,
            tool_uses: None,
            duration_ms: None,
            summary: None,
        };
        // La descripción corta de la herramienta gana al comando entero.
        assert_eq!(chat.task_label(&task(Some("tool-1"), "npm run build -- --watch")), "Compila en modo vigilancia");
        assert_eq!(chat.task_label(&task(None, "Revisar tests")), "Revisar tests");
        assert_eq!(chat.task_label(&task(None, "")), "Comando en segundo plano");
    }
}
