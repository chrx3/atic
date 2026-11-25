//! Cómo se ven los subagentes de Atic Code (`delegate.rs`), como en t3code:
//!
//! - En el padre, la llamada a `atic_delegate` (o `atic_prompt`) es la tarjeta del hijo,
//!   con su agente, su título y su estado en vivo, en vez de la fila de herramienta con el
//!   JSON; y su resultado (`<atic-subagent>`) es otra tarjeta, no una burbuja del usuario.
//! - En el hijo, una barra arriba dice de quién es y lleva a la conversación principal, y en
//!   lugar de la caja de texto va un aviso: sus mensajes vienen del padre.
//! - En el mapa de agentes, los hijos de la conversación con su estado.
//!
//! Las tres apariencias usan las mismas piezas: en Expressive la tarjeta tonal de gpui-m3 y
//! sus botones; en Formal y Glass, el contenedor con borde de las herramientas y botones de
//! texto del acento. Lo secundario va en `muted` (`faint` no se lee en claro).

use gpui::{div, prelude::*, px, svg, AnyElement, ClickEvent, Context, Div, FontWeight, SharedString};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::{Button, DotShape, ExpandableCard, Icon, MorphDot, Tone};
use serde_json::Value;
use std::time::Instant;

use super::chat::{Chat, TaskNote, ToolCall};
use super::delegate::{self, ChildState, DelegateTool};
use super::split::Fit;
use super::style::t;
use super::CodeView;
use crate::i18n::{t as tr, tf};

type OnClick = Box<dyn Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static>;

/// Un botón de texto chico, con su ícono: el de gpui-m3 en Expressive; en Formal y Glass,
/// texto del acento (como los del plan en `tools.rs`), que contrasta en claro y en oscuro.
fn action(id: SharedString, label: String, icon: &'static str, on_click: OnClick) -> AnyElement {
    if super::view::expressive() {
        return Button::new(id, label).text().icon(icon).on_click(move |event, window, cx| on_click(event, window, cx)).into_any_element();
    }
    let tokens = t();
    div()
        .id(id)
        .flex_none()
        .h(px(26.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(tokens.r_btn.min(13.)))
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(tokens.accent)
        .hover(|el| el.bg(tokens.hover))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(Icon::new(icon).size(px(13.)).color(tokens.accent))
        .child(label)
        .into_any_element()
}

/// Cuánto del encargo se ve al abrir la tarjeta del hijo.
const BRIEF_CHARS: usize = 600;

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect::<String>() + "…"
    }
}

/// El ícono de «Abrir subagente» en la cabecera de una tarjeta: no la pliega ni la abre.
fn open_icon(id: SharedString, on_click: OnClick) -> AnyElement {
    let tokens = t();
    div()
        .id(id)
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|el| el.bg(tokens.hover))
        .tooltip(crate::hover::tip(tr("pill.code.delegate.open")))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(Icon::new("export").size(px(13.)).color(tokens.muted))
        .into_any_element()
}

/// El logo del agente (16 px), o el ícono genérico si no tiene.
fn logo(backend: &str, size: f32) -> AnyElement {
    let tokens = t();
    match delegate::agent_logo(backend) {
        Some(path) => svg().path(path).size(px(size)).flex_none().text_color(tokens.muted).into_any_element(),
        None => Icon::new("cpu").size(px(size)).color(tokens.muted).into_any_element(),
    }
}

/// El ícono y el texto de cada estado del hijo; `elapsed` va junto a «Trabajando».
fn state_badge(state: &ChildState, elapsed: Option<u64>) -> Div {
    let tokens = t();
    let m3 = super::view::expressive();
    let (icon, label, color): (AnyElement, String, gpui::Hsla) = match state {
        ChildState::Starting | ChildState::Working => {
            let marker = if m3 {
                super::spinners::slot(14.).into_any_element()
            } else {
                svg().path("icons/circle-dot.svg").size(px(13.)).text_color(tokens.warn).into_any_element()
            };
            let label = if *state == ChildState::Starting { tr("pill.code.delegate.stateStarting").to_string() } else { tr("pill.code.delegate.stateWorking").to_string() };
            let label = match super::usage::duration(elapsed) {
                time if time.is_empty() => label,
                time => format!("{label} · {time}"),
            };
            (marker, label, tokens.muted)
        }
        ChildState::Permission(_) => (Icon::new("shield").size(px(13.)).color(tokens.accent).into_any_element(), tr("pill.code.delegate.statePermission").to_string(), tokens.accent),
        ChildState::Done => (Icon::new("check").size(px(13.)).color(tokens.ok).into_any_element(), tr("pill.code.delegate.stateDone").to_string(), tokens.muted),
        ChildState::Failed => (Icon::new("x").size(px(13.)).color(tokens.bad).into_any_element(), tr("pill.code.delegate.stateFailed").to_string(), tokens.bad),
        ChildState::Stopped => (svg().path("icons/circle-dot.svg").size(px(13.)).text_color(tokens.muted).into_any_element(), tr("pill.code.delegate.stateStopped").to_string(), tokens.muted),
        ChildState::Queued => (svg().path("icons/circle-dot.svg").size(px(13.)).text_color(tokens.muted).into_any_element(), tr("pill.code.delegate.stateQueued").to_string(), tokens.muted),
        ChildState::Idle => (svg().path("icons/circle-dot.svg").size(px(13.)).text_color(tokens.muted).into_any_element(), tr("pill.code.delegate.stateIdle").to_string(), tokens.muted),
        ChildState::Gone => (svg().path("icons/circle-dot.svg").size(px(13.)).text_color(tokens.muted).into_any_element(), tr("pill.code.delegate.stateGone").to_string(), tokens.muted),
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(color)
        .child(div().size(px(16.)).flex().items_center().justify_center().child(icon))
        .child(label)
}

impl CodeView {
    /// Abre la conversación `key` (un hijo o su padre) desde una tarjeta o una barra.
    fn open_chat_listener(&self, key: String, cx: &mut Context<Self>) -> OnClick {
        Box::new(cx.listener(move |view, _: &ClickEvent, window, cx| {
            cx.stop_propagation();
            view.history_page = false;
            view.select_chat(key.clone(), window, cx);
        }))
    }

    /// Una tarjeta plegable del alto de las de herramientas (`tools.rs`): la cabecera en una
    /// línea; `under` se ve siempre (un permiso, un error) y `body` solo al abrirla. `label`
    /// es su título en la cabecera flotante (`fold_bar.rs`) mientras está abierta.
    fn collapsible(&self, id: String, label: String, header: Div, under: Option<Div>, body: Div, cx: &mut Context<Self>) -> AnyElement {
        let open = self.expanded.contains(&id);
        let fold = open.then(|| super::fold_bar::Fold::new(id.clone(), label));
        let toggle = id.clone();
        if super::view::expressive() {
            let flip = cx.listener(move |view, _: &bool, _, cx| {
                if !view.expanded.remove(&toggle) {
                    view.expanded.insert(toggle.clone());
                }
                cx.notify();
            });
            let mut card = ExpandableCard::new(SharedString::from(id))
                .open(open)
                .tone(Tone::Secondary)
                .header(header)
                .on_toggle(move |open, window, cx| flip(&open, window, cx))
                .child(body);
            if let Some(under) = under {
                card = card.under(under);
            }
            return super::fold_bar::track(card.into_any_element(), fold, super::fold_bar::HEAD);
        }
        let card_id = id.clone();
        let card = super::tools::flat_card(&card_id, header, body, under, open, false, cx.listener(move |view, _: &ClickEvent, _, cx| {
            if !view.expanded.remove(&toggle) {
                view.expanded.insert(toggle.clone());
            }
            cx.notify();
        }));
        super::fold_bar::track(card, fold, super::fold_bar::HEAD)
    }

    /// La llamada a `atic_delegate` (o `atic_prompt`) en el padre, en una línea: el agente, el
    /// título y el estado en vivo del hijo, y el ícono para abrirlo. Plegada por defecto; al
    /// abrirla, el rol, la duración y el encargo. Un permiso pendiente o un error se ven
    /// aunque esté plegada.
    pub(super) fn delegate_card(&self, chat: &Chat, tool: &ToolCall, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let input = tool.input.clone().unwrap_or(Value::Null);
        let field = |name: &str| input.get(name).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let outcome = tool.result.as_deref().filter(|_| !tool.is_error).and_then(delegate::delegate_outcome);
        let text_of = |name: &str| outcome.as_ref().and_then(|o| o.get(name)).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
        let session = text_of("session").or_else(|| (DelegateTool::of(&tool.name) == Some(DelegateTool::Prompt)).then(|| field("session")).flatten());
        let child = session.as_deref().and_then(|s| self.child_of(&chat.key, s));
        let delegation = child.and_then(|c| c.delegation.as_ref());

        let backend = delegation
            .map(|d| d.backend.clone())
            .or_else(|| text_of("backend"))
            .or_else(|| field("backend").filter(|b| b != "auto"))
            .unwrap_or_else(|| delegate::backend_of(chat.acp).to_string());
        let title = child
            .map(|c| c.title.clone())
            .or_else(|| field("label"))
            .or_else(|| field("text").map(|text| delegate::first_words(&text)))
            .unwrap_or_else(|| tr("pill.code.tools.subagent").to_string());
        let role = delegation.and_then(|d| d.role.clone()).or_else(|| field("role"));
        let error = tool.is_error.then(|| tool.result.clone().unwrap_or_default());
        let state = if error.is_some() {
            ChildState::Failed
        } else if tool.result.is_none() {
            ChildState::Starting
        } else {
            self.child_state_of(child)
        };
        // El tiempo: en vivo mientras trabaja; si ya terminó, el de su último turno.
        let elapsed = match (&state, delegation) {
            (ChildState::Working | ChildState::Permission(_), Some(d)) => Some(d.started.elapsed().as_millis() as u64),
            (_, Some(d)) => d.last.as_ref().and_then(|v| v.get("elapsed_s")).and_then(Value::as_u64).map(|s| s * 1000),
            _ => None,
        };

        let id = format!("delegate-{}-{}", chat.key, tool.id);
        let label = format!("{} \u{b7} {title}", delegate::agent_label(&backend));
        let header = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .min_w(px(0.))
            .text_size(px(12.5))
            .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(logo(&backend, 14.)))
            .child(div().flex_none().type_emphasized(type_scale::LABEL_MEDIUM).child(delegate::agent_label(&backend)))
            .child(div().flex_1().min_w(px(0.)).truncate().text_color(tokens.muted).child(title))
            .child(state_badge(&state, elapsed))
            .when_some(child, |el, child| el.child(open_icon(SharedString::from(format!("{id}-open")), self.open_chat_listener(child.key.clone(), cx))));

        // Lo que se ve aunque esté plegada: el permiso que espera o el error de la llamada.
        let mut under: Option<Div> = None;
        if let (ChildState::Permission(tool_name), Some(child)) = (&state, child) {
            let row = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(4.))
                .rounded(px(12.))
                .bg(tokens.accent_soft)
                .text_color(tokens.on_accent_soft)
                .text_size(px(12.5))
                .child(Icon::new("shield").size(px(14.)).color(tokens.on_accent_soft))
                .child(div().flex_1().min_w(px(0.)).truncate().child(tf("pill.code.delegate.permissionAsk", &[("tool", tool_name)])))
                .child(action(SharedString::from(format!("{id}-answer")), tr("pill.code.delegate.answerPermission").to_string(), "shield", self.open_chat_listener(child.key.clone(), cx)));
            under = Some(div().px(px(14.)).pb(px(10.)).child(row));
        }
        // El tope de subagentes lo frenó (error) o espera un cupo (en cola): ofrece subirlo.
        let error = error.filter(|e| !e.trim().is_empty());
        let at_limit = error.as_ref().is_some_and(|e| e.contains("too_many_children")) || state == ChildState::Queued;
        let current = self.configs.subagent_limit();
        let next = (current + super::config::SUBAGENT_LIMIT_STEP).min(super::config::MAX_SUBAGENT_LIMIT);
        let raise = (at_limit && next > current).then(|| {
            let listener = cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.set_subagent_limit(next, cx);
                view.show_toast(tf("pill.code.delegate.limitRaised", &[("n", &next.to_string())]), cx);
            });
            action(SharedString::from(format!("{id}-raise")), tf("pill.code.delegate.raiseLimit", &[("n", &next.to_string())]), "plus", Box::new(move |event, window, cx| listener(event, window, cx)))
        });
        if error.is_some() || raise.is_some() {
            under = Some(
                div()
                    .px(px(14.))
                    .pb(px(10.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(4.))
                    .children(error.map(|error| div().w_full().child(super::tools::output(format!("{id}-error"), &error, 4, None, true))))
                    .children(raise),
            );
        }

        let meta = [
            delegate::role_label(role.as_deref()).map(str::to_string),
            match state {
                ChildState::Done | ChildState::Failed | ChildState::Stopped => Some(super::usage::duration(elapsed)).filter(|d| !d.is_empty()),
                _ => None,
            },
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let body = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_size(px(12.5))
            .when(!meta.is_empty(), |el| el.child(div().text_size(px(12.)).text_color(tokens.muted).child(meta)))
            .when_some(field("text"), |el, text| el.child(div().text_color(tokens.muted).child(clip_chars(&text, BRIEF_CHARS))));
        self.collapsible(id, label, header, under, body, cx)
    }

    /// El resultado de un subagente en el padre (`<atic-subagent>`), en una línea: quién
    /// terminó y cómo, cuánto tardó y el ícono para abrirlo. Al abrirla, la respuesta en
    /// Markdown.
    pub(super) fn subagent_note(&self, chat: &Chat, index: usize, note: &TaskNote, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let backend = note.agent.clone().unwrap_or_else(|| "claude".into());
        let (icon, color) = match note.status.as_str() {
            "failed" | "error" => ("x", tokens.bad),
            "cancelled" | "stopped" => ("stop", tokens.muted),
            _ => ("check", tokens.ok),
        };
        let id = format!("subagent-note-{}-{index}", chat.key);
        let meta = note.meta();
        let header = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .min_w(px(0.))
            .text_size(px(12.5))
            .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(Icon::new(icon).size(px(13.)).color(color)))
            .child(logo(&backend, 14.))
            .child(div().flex_1().min_w(px(0.)).truncate().font_weight(FontWeight::SEMIBOLD).child(note.heading()))
            .when(!meta.is_empty(), |el| el.child(div().flex_none().text_size(px(12.)).text_color(tokens.muted).child(meta)))
            .when_some(self.child_of(&chat.key, &note.task_id), |el, child| {
                el.child(open_icon(SharedString::from(format!("{id}-open")), self.open_chat_listener(child.key.clone(), cx)))
            });
        let mut body = div().flex().flex_col();
        if let Some(result) = note.result.as_deref().filter(|_| self.expanded.contains(&id)) {
            body = body.child(super::view::markdown(&format!("{}-{index}-subagent", chat.key), result, cx));
        }
        self.collapsible(id, note.heading(), header, None, body, cx)
    }

    /// La barra fina arriba de un subagente: «Subagente de «X» · Codex · revisión» y el
    /// botón para volver a la conversación principal. `None` si no es un subagente.
    pub(super) fn subagent_bar(&self, chat: &Chat, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tokens = t();
        let delegation = chat.delegation.as_ref()?;
        let parent = self.chats.iter().find(|c| c.key == delegation.parent);
        let lead = match parent {
            Some(parent) => tf("pill.code.delegate.childOf", &[("parent", &parent.title)]),
            None => tr("pill.code.delegate.childExternal").to_string(),
        };
        let text = [Some(lead), Some(delegate::agent_label(&delegation.backend).to_string()), delegate::role_label(delegation.role.as_deref()).map(str::to_string)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(34.))
                .px(px(16.))
                .border_b_1()
                .border_color(tokens.border)
                .text_size(px(12.5))
                .text_color(tokens.muted)
                .child(logo(&delegation.backend, 14.))
                .child(div().flex_1().min_w(px(0.)).truncate().child(text))
                .when_some(parent, |el, parent| {
                    el.child(action(SharedString::from(format!("subagent-parent-{}", chat.key)), tr("pill.code.delegate.goToParent").to_string(), "arrow-left", self.open_chat_listener(parent.key.clone(), cx)))
                })
                .into_any_element(),
        )
    }

    /// En lugar de la caja de un subagente: sus mensajes vienen de la conversación principal
    /// (como en t3code). Sus permisos se siguen contestando en las tarjetas del hilo.
    pub(super) fn subagent_composer(&self, chat: &Chat, fit: Fit, cx: &mut Context<Self>) -> Div {
        let tokens = t();
        let side = match fit {
            Fit::Wide => 32.,
            Fit::Narrow => 20.,
            Fit::Tight => 14.,
        };
        let parent = chat.delegation.as_ref().and_then(|d| self.chats.iter().find(|c| c.key == d.parent));
        let width = super::style::chat_width();
        div().w_full().when(width > 0., |el| el.max_w(px(width)).mx_auto()).px(px(side)).pb(px(14.)).child(
            div()
                .id(SharedString::from(format!("subagent-composer-{}", chat.key)))
                .min_h(px(52.))
                .px(px(18.))
                .py(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(if super::view::expressive() { 28. } else { super::view::r_card() }))
                .bg(tokens.control)
                .text_color(tokens.muted)
                .text_size(px(13.))
                .child(div().flex_1().min_w(px(0.)).child(tr("pill.code.delegate.composerLocked")))
                .when_some(parent, |el, parent| {
                    el.child(action(SharedString::from(format!("subagent-composer-parent-{}", chat.key)), tr("pill.code.delegate.goToParent").to_string(), "arrow-left", self.open_chat_listener(parent.key.clone(), cx)))
                }),
        )
    }

    /// Los hijos de la conversación que trabajan (para el chip «N agentes» y la pestaña).
    pub(super) fn busy_children(&self, key: &str) -> usize {
        self.children_of(key).into_iter().filter(|c| c.busy).count()
    }

    /// Los subagentes de Atic de la conversación para el mapa de agentes: (trabajando,
    /// terminados), cada fila con su agente, su título, su estado y su tiempo en vivo, del alto
    /// y la forma de las filas de tareas para que la lista sea una sola. Lleva además el
    /// instante de inicio para mezclarlos con las tareas del SDK. Un clic los abre. `None`
    /// si no tiene.
    /// El modelo y el esfuerzo con que trabaja un hijo, para su fila: «Opus 5.5 · Alto». Un hijo
    /// Claude usa el modelo que de verdad respondió o, si aún no, el pedido; uno ACP, lo que
    /// dicen sus opciones. Vacío si no se sabe.
    fn child_setup(&self, child: &Chat) -> String {
        let (model, effort) = match child.acp {
            None => {
                let config = self.configs.get(child.workspace);
                let model = child.model.clone().filter(|m| !m.is_empty() && !m.starts_with('<')).or_else(|| Some(child.chosen_model(&config)).filter(|m| !m.is_empty())).map(|m| self.model_label(&m));
                let effort = Some(child.chosen_effort(&config)).filter(|e| !e.is_empty()).map(|e| super::config::effort_label(&e).to_string());
                (model, effort)
            }
            Some(_) => {
                let current = |category: &str| child.acp_cfg.options.iter().find(|o| o.category == category).map(|o| (o.current.clone(), o.choices.iter().find(|c| c.value == o.current).map(|c| c.name.clone())));
                let model = current("model").filter(|(value, _)| !value.is_empty()).map(|(value, name)| name.unwrap_or_else(|| super::acp::pretty_model(&value)));
                let effort = current("thought_level").filter(|(value, _)| !value.is_empty() && value != "default").map(|(value, _)| super::acp::effort_name(&value));
                (model, effort)
            }
        };
        [model, effort].into_iter().flatten().collect::<Vec<_>>().join(" · ")
    }

    pub(super) fn subagent_map(&self, chat: &Chat, cx: &mut Context<Self>) -> Option<(Vec<(Instant, AnyElement)>, Vec<(Instant, AnyElement)>)> {
        let tokens = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let children = self.children_of(&chat.key);
        if children.is_empty() {
            return None;
        }
        let mut working = Vec::new();
        let mut done = Vec::new();
        for child in children.into_iter().rev() {
            let Some(delegation) = child.delegation.as_ref() else { continue };
            let state = self.child_state_of(Some(child));
            let live = matches!(state, ChildState::Working | ChildState::Permission(_) | ChildState::Starting);
            let elapsed = match &state {
                ChildState::Working | ChildState::Permission(_) | ChildState::Starting => Some(delegation.started.elapsed().as_millis() as u64),
                _ => delegation.last.as_ref().and_then(|v| v.get("elapsed_s")).and_then(Value::as_u64).map(|s| s * 1000),
            };
            let state_text = match &state {
                ChildState::Starting => tr("pill.code.delegate.stateStarting").to_string(),
                ChildState::Working => tr("pill.code.delegate.stateWorking").to_string(),
                ChildState::Permission(tool) => tf("pill.code.delegate.permissionAsk", &[("tool", tool)]),
                ChildState::Done => tr("pill.code.delegate.stateDone").to_string(),
                ChildState::Failed => tr("pill.code.delegate.stateFailed").to_string(),
                ChildState::Stopped => tr("pill.code.delegate.stateStopped").to_string(),
                ChildState::Queued => tr("pill.code.delegate.stateQueued").to_string(),
                ChildState::Idle => tr("pill.code.delegate.stateIdle").to_string(),
                ChildState::Gone => tr("pill.code.delegate.stateGone").to_string(),
            };
            let time = super::usage::duration(elapsed);
            let third = if time.is_empty() { state_text } else { format!("{state_text} · {time}") };
            let color = match &state {
                ChildState::Working | ChildState::Permission(_) | ChildState::Starting => scheme.primary,
                ChildState::Done => scheme.success,
                ChildState::Failed => scheme.error,
                _ => scheme.outline,
            };
            let kind = match delegate::role_label(delegation.role.as_deref()) {
                Some(role) => format!("{} · {role}", delegate::agent_label(&delegation.backend)),
                None => delegate::agent_label(&delegation.backend).to_string(),
            };
            let kind = match self.child_setup(child) {
                setup if setup.is_empty() => kind,
                setup => format!("{kind} · {setup}"),
            };
            // El hueco lo dibuja la capa de indicadores del popover (`pop_layer`), como en
            // las filas de tareas; en reposo, el punto cambia de forma según el estado.
            let marker = if live && super::view::expressive() {
                super::spinners::slot(16.).into_any_element()
            } else {
                MorphDot::new(SharedString::from(format!("subagent-dot-{}", child.key)), if live { DotShape::Square } else { DotShape::Circle })
                    .size(px(8.))
                    .color(color)
                    .into_any_element()
            };
            let open = self.open_chat_listener(child.key.clone(), cx);
            let row = div()
                .id(SharedString::from(format!("subagent-row-{}", child.key)))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(6.))
                .min_h(px(44.))
                .rounded(px(12.))
                .cursor_pointer()
                .hover(|el| el.bg(tokens.hover))
                .tooltip(crate::hover::tip(tr("pill.code.delegate.open")))
                .on_click(move |event, window, cx| open(event, window, cx))
                .child(div().w(px(16.)).flex_none().flex().justify_center().child(marker))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(child.title.clone()))
                        .child(div().text_size(px(11.5)).text_color(tokens.muted).truncate().child(kind))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(if matches!(state, ChildState::Permission(_)) { tokens.accent } else { tokens.muted })
                                .truncate()
                                .child(third),
                        ),
                )
                .into_any_element();
            if live {
                working.push((delegation.started, row));
            } else {
                done.push((delegation.started, row));
            }
        }
        if working.is_empty() && done.is_empty() {
            return None;
        }
        Some((working, done))
    }
}
