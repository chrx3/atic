//! Configuración en Expressive: un diálogo con las pestañas «General» (conversaciones,
//! avisos, chat y barra lateral), «Claude» (versión, cuenta y los valores por defecto
//! del proyecto), «Apariencia», «Atajos», «Agentes» y «Teléfono» (el mismo panel de los
//! Ajustes de la pill). Lo que antes estaba suelto en el menú `/` y la paleta también
//! está aquí, sin dejar de estar allá.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString, Window};
use gpui_m3::{
    Button, ButtonSize, ColorSwatches, Dialog, HsvPicker, IconButton, ListGroup, LoadingIndicator, NavItem, Segment, SegmentedButtons, SelectCard, Shape,
    ShapeName, Tone,
};
use serde_json::{json, Value};

use super::enter::Enter;
use super::style::{t, Mode, Style};
use crate::i18n::{t as tr, tf};
use super::CodeView;

/// Icono y clave del título de cada pestaña (se traduce al dibujarla).
/// El orden es el de `settings_search` (`GENERAL_TAB`, `CLAUDE_TAB`…).
const TABS: [(&str, &str); 6] = [
    ("gear", "pill.code.settingsM3.tabGeneral"),
    ("spark", "pill.code.settingsM3.tabClaude"),
    ("palette", "pill.code.settingsM3.tabAppearance"),
    ("command", "pill.code.settingsM3.tabShortcuts"),
    ("plug", "pill.code.settingsM3.tabAgents"),
    ("smartphone", "pill.code.settingsM3.tabPhone"),
];

/// Lo que dice `claudeInfo`: versión instalada, la última publicada y dónde está.
#[derive(Clone, Default, Debug)]
pub struct ClaudeInfo {
    pub checking: bool,
    pub version: Option<String>,
    pub latest: Option<String>,
    pub path: Option<String>,
    pub target: Option<String>,
    pub error: Option<String>,
}

/// Cómo está la instalación de Claude Code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Health {
    Checking,
    Failed,
    Missing,
    Unchecked,
    Outdated,
    Current,
}

impl Health {
    pub(super) fn color(self, t: &super::style::Tokens) -> gpui::Hsla {
        match self {
            Health::Checking | Health::Unchecked => t.faint,
            Health::Failed | Health::Missing => t.bad,
            Health::Outdated => t.warn,
            Health::Current => t.ok,
        }
    }
}

/// La publicada es más nueva que la instalada. Puede ser más vieja: `claude`
/// se actualiza desde su canal (`next` va antes que `latest` en npm).
pub(super) fn is_outdated(info: &ClaudeInfo) -> bool {
    let (Some(v), Some(l)) = (&info.version, &info.latest) else {
        return false;
    };
    match (version_parts(v), version_parts(l)) {
        (Some(v), Some(l)) => l > v,
        _ => v != l,
    }
}

/// «2.1.319 (Claude Code)» → [2, 1, 319].
fn version_parts(text: &str) -> Option<Vec<u64>> {
    let core = text.split_whitespace().next()?.split(['-', '+']).next()?;
    core.split('.').map(|part| part.parse().ok()).collect()
}

/// El estado de la instalación y su texto (el mismo en los tres estilos).
pub(super) fn claude_health(info: &ClaudeInfo) -> (Health, String) {
    if info.checking {
        (Health::Checking, tr("pill.code.settingsM3.checking").into())
    } else if let Some(error) = &info.error {
        (Health::Failed, error.clone())
    } else if info.version.is_none() {
        (Health::Missing, tr("pill.code.settingsM3.notFound").into())
    } else if info.latest.is_none() {
        (Health::Unchecked, tr("pill.code.settingsM3.cannotCheck").into())
    } else if is_outdated(info) {
        (Health::Outdated, tf("pill.code.settingsM3.newVersion", &[("version", &info.latest.clone().unwrap_or_default())]))
    } else {
        (Health::Current, tr("pill.code.settingsM3.upToDate").into())
    }
}

pub(super) fn plan_label(plan: &str) -> String {
    match plan {
        "max" => "Max".into(),
        "pro" => "Pro".into(),
        "team" => "Team".into(),
        "enterprise" => "Enterprise".into(),
        "free" => tr("pill.code.settingsM3.planFree").into(),
        other => other.to_string(),
    }
}

/// `C:\Users\x\…` → `~\…`.
pub(super) fn short_home(path: &str) -> String {
    match crate::paths::home().map(|h| h.to_string_lossy().into_owned()).ok_or(std::env::VarError::NotPresent) {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

impl CodeView {
    /// Pide la versión de Claude Code (al abrir Configuración o al volver a comprobar).
    pub(super) fn check_claude(&mut self, cx: &mut Context<Self>) {
        self.claude_info = Some(ClaudeInfo { checking: true, ..Default::default() });
        self.request("claudeInfo", json!({}), cx, |view, reply, _| {
            let text = |value: &Value, name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
            view.claude_info = Some(match reply {
                Ok(info) => ClaudeInfo {
                    checking: false,
                    version: text(&info, "version"),
                    latest: text(&info, "latest"),
                    path: text(&info, "path"),
                    target: text(&info, "target"),
                    error: None,
                },
                Err(error) => ClaudeInfo { error: Some(error), ..Default::default() },
            });
        });
    }

    pub(super) fn update_claude(&mut self, cx: &mut Context<Self>) {
        self.updating_claude = true;
        self.update_log.clear();
        self.update_result = None;
        self.request("claudeUpdate", json!({}), cx, |view, reply, cx| {
            view.updating_claude = false;
            view.update_result = Some(match reply {
                Ok(result) if result.get("ok").and_then(Value::as_bool).unwrap_or(false) => {
                    let version = result.get("version").and_then(Value::as_str).unwrap_or("?");
                    (true, tf("pill.code.settingsM3.updated", &[("version", &version)]))
                }
                Ok(result) => {
                    let message = result.get("message").and_then(Value::as_str).map(str::to_string);
                    let code = result.get("code").map(|c| c.to_string()).unwrap_or_else(|| "?".into());
                    (false, message.unwrap_or_else(|| tf("pill.code.settingsM3.updateFailed", &[("code", &code)])))
                }
                Err(error) => (false, error),
            });
            view.check_claude(cx);
            // La conversación vacía visible usa la versión nueva (y su lista de modelos).
            if view.update_result.as_ref().is_some_and(|(ok, _)| *ok) {
                view.restart_idle_chat(cx);
            }
        });
    }

    pub(super) fn settings_m3(&self, progress: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let mut nav = div()
            .w(px(232.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(12.))
            .py(px(22.))
            // GPUI recorta a los hijos con un rectángulo, no con las esquinas de la tarjeta:
            // sin estas esquinas, el fondo de la columna asomaba cuadrado a la izquierda.
            .rounded_tl(px(28.))
            .rounded_bl(px(28.))
            .bg(scheme.primary.opacity(0.05))
            .child(div().px(px(16.)).pb(px(16.)).text_size(px(22.)).font_weight(FontWeight::BOLD).text_color(t.text).child(tr("pill.code.settingsM3.title")));
        for (index, (icon, label)) in TABS.iter().enumerate() {
            nav = nav.child(
                NavItem::new(("settings-tab", index), tr(*label)).icon(*icon).large(true).tone(Tone::Primary).selected(self.settings_tab == index).on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.settings_tab = index;
                        view.clear_settings_search(cx);
                        if index == super::settings_search::AGENTS_TAB {
                            view.load_agent_versions(cx);
                        }
                        cx.notify();
                    }),
                ),
            );
        }
        // Con algo escrito en el buscador, los resultados de todas las pestañas reemplazan la pestaña.
        let query = self.settings_query(cx);
        let searching = !query.is_empty();
        let content = if searching {
            self.settings_results(&query, true, cx)
        } else {
            use super::settings_search::{AGENTS_TAB, APPEARANCE_TAB, CLAUDE_TAB, GENERAL_TAB, PHONE_TAB};
            match self.settings_tab {
                GENERAL_TAB => self.general_tab(cx),
                CLAUDE_TAB => self.claude_tab(cx),
                APPEARANCE_TAB => self.appearance_tab(cx),
                AGENTS_TAB => self.agents_tab(cx),
                PHONE_TAB => self.phone_tab(cx),
                _ => self.shortcuts_tab(cx),
            }
        };
        // El contenido de cada pestaña sube al abrirla (`expressive.css:989`); en la referencia cada
        // sección entra con su retardo, aquí entra la pestaña entera.
        let content = Enter::new(SharedString::from(format!("settings-in-{}", if searching { 9 } else { self.settings_tab })))
            .from(0., 12.)
            .delay(0.04)
            .apply(div(), window, cx)
            .child(content);
        let body = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .pl(px(26.))
                    .pr(px(14.))
                    .pt(px(18.))
                    .pb(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).text_color(t.text).child(if searching { tr("pill.code.settingsM3.results") } else { tr(TABS[self.settings_tab.min(TABS.len() - 1)].1) }))
                    .child(IconButton::new("settings-close", "x").size(px(32.)).tooltip(tr("pill.code.settingsM3.closeTooltip")).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.settings_open = false;
                        cx.notify();
                    }))),
            )
            .child(div().pl(px(26.)).pr(px(22.)).pt(px(4.)).pb(px(6.)).child(div().rounded_full().bg(scheme.surface_container_highest).child(self.settings_search.clone())))
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pl(px(26.))
                    .pr(px(22.))
                    .pt(px(10.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(content),
            );
        Dialog::new("settings")
            .exit(progress)
            // Más grande que antes (820×600): casi todo quedaba bajo el scroll.
            .sized(px(960.), px(700.))
            .padding(px(0.))
            .radius(px(28.))
            .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.settings_open = false;
                cx.notify();
            }))
            .child(div().size_full().flex().child(nav).child(body))
            .into_any_element()
    }

    fn claude_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let info = self.claude_info.clone().unwrap_or_default();
        let outdated = is_outdated(&info);
        let (bg, fg, mark) =
            if outdated { (scheme.tertiary_container, scheme.on_tertiary_container, scheme.tertiary) } else { (scheme.primary_container, scheme.on_primary_container, scheme.primary) };
        let (health, status) = claude_health(&info);
        let status_color = health.color(&t);
        // Compacta: lo que más se mira en esta pestaña no es la versión.
        let mut shape = Shape::new(ShapeName::Sunny).size(px(40.)).color(mark).child(gpui_m3::Icon::new("spark").size(px(17.)).color(scheme.on_primary));
        if self.updating_claude {
            shape = shape.breathe(ShapeName::Cookie9, 1.6);
        }
        let action: AnyElement = if self.updating_claude {
            div().h(px(48.)).flex().items_center().gap(px(8.)).font_weight(FontWeight::BOLD).child(LoadingIndicator::new().size(px(28.))).child(tr("pill.code.settingsM3.updating")).into_any_element()
        } else if outdated {
            Button::new("claude-update", tf("pill.code.settingsM3.updateTo", &[("version", &info.latest.clone().unwrap_or_default())]))
                .filled()
                .icon("up")
                .size(ButtonSize::Medium)
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.update_claude(cx)))
                .into_any_element()
        } else {
            Button::new("claude-check", tr("pill.code.settingsM3.checkUpdate"))
                .tonal()
                .icon("refresh")
                .size(ButtonSize::Medium)
                .disabled(info.version.is_none())
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.check_claude(cx)))
                .into_any_element()
        };
        let hero = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .pl(px(14.))
            .pr(px(16.))
            .py(px(12.))
            .rounded(px(24.))
            .bg(bg)
            .text_color(fg)
            .child(shape)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::BOLD).opacity(0.8).child("Claude Code"))
                    .child(div().text_size(px(22.)).font_weight(FontWeight::EXTRA_BOLD).line_height(px(26.)).child(info.version.clone().unwrap_or_else(|| "—".into())))
                    .child(
                        div().flex().min_w(px(0.)).child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .mt(px(4.))
                                .pl(px(9.))
                                .pr(px(12.))
                                .py(px(4.))
                                .rounded_full()
                                .bg(status_color.opacity(0.12))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(status_color)
                                .child(div().size(px(7.)).flex_none().rounded_full().bg(status_color))
                                .child(div().min_w(px(0.)).truncate().child(status)),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(4.))
                    .child(action)
                    .child(
                        Button::new("claude-recheck", if info.checking { tr("pill.code.settingsM3.checking") } else { tr("pill.code.settingsM3.recheck") })
                            .text()
                            .size(ButtonSize::Small)
                            .disabled(info.checking || self.updating_claude)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.check_claude(cx))),
                    ),
            );
        let mono = |text: String| div().font_family(gpui_m3::theme::MONO_FONT_FAMILY).text_size(px(12.)).font_weight(FontWeight::BOLD).child(text);
        let path = info.path.clone().map(|p| short_home(&p)).unwrap_or_else(|| "—".into());
        let install = ListGroup::new()
            .title(tr("pill.code.settingsM3.install"))
            .row(tr("pill.code.settingsM3.installedVersion"), mono(info.version.clone().unwrap_or_else(|| "—".into())))
            .row(tr("pill.code.settingsM3.latestVersion"), mono(info.latest.clone().unwrap_or_else(|| "—".into())))
            .row(tr("pill.code.settingsM3.executable"), div().id("claude-path").max_w(px(280.)).truncate().tooltip(crate::hover::tip_text(SharedString::from(info.target.clone().or(info.path.clone()).unwrap_or_default()))).child(mono(path)));
        let mut column = div().flex().flex_col().gap(px(16.)).child(hero);
        if !self.update_log.is_empty() || self.update_result.is_some() {
            let mut group = div().flex().flex_col().gap(px(8.));
            if let Some((ok, text)) = &self.update_result {
                group = group.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .text_color(if *ok { t.ok } else { t.bad })
                        .child(gpui_m3::Icon::new(if *ok { "check" } else { "x" }).size(px(15.)).color(if *ok { t.ok } else { t.bad }))
                        .child(div().flex_1().min_w(px(0.)).child(text.clone())),
                );
            }
            if !self.update_log.is_empty() {
                group = group.child(
                    div()
                        .id("update-log")
                        .max_h(px(180.))
                        .overflow_y_scroll()
                        .p(px(10.))
                        .rounded(px(12.))
                        .bg(t.editor)
                        .font_family(gpui_m3::theme::MONO_FONT_FAMILY)
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .child(self.update_log.clone()),
                );
            }
            column = column.child(group);
        }
        column = column.child(install);
        if let Some((email, plan, organization)) = &self.account {
            let mut account = ListGroup::new().title(tr("pill.code.settingsM3.account"));
            if let Some(email) = email {
                account = account.row(tr("pill.code.settingsM3.signedIn"), div().max_w(px(280.)).truncate().font_weight(FontWeight::BOLD).child(email.clone()));
            }
            if let Some(plan) = plan {
                account = account.row(tr("pill.code.settingsM3.plan"), div().font_weight(FontWeight::BOLD).child(plan_label(plan)));
            }
            if let Some(organization) = organization {
                account = account.row(tr("pill.code.settingsM3.organization"), div().max_w(px(280.)).truncate().font_weight(FontWeight::BOLD).child(organization.clone()));
            }
            column = column.child(account);
        }
        // Lo que de verdad se configura de Claude: los valores del proyecto abierto.
        column = column.child(self.project_defaults_m3(cx));
        column
            .child(div().px(px(6.)).text_size(px(12.5)).line_height(px(19.)).text_color(t.faint).child(
                tr("pill.code.settingsM3.sameClaude"),
            ))
            .into_any_element()
    }

    /// «General»: conversaciones, avisos, chat y barra lateral (los ajustes de Atic Code que
    /// no son de un proyecto).
    fn general_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::config::NotifyMode;
        let view = cx.weak_entity();
        let follow = {
            let view = view.clone();
            SegmentedButtons::new(
                "follow-up-mode",
                vec![Segment::label(tr("pill.code.settingsSearch.followUpSteer")), Segment::label(tr("pill.code.settingsSearch.followUpQueue"))],
                if self.config().follow_up_steer { 0 } else { 1 },
            )
            .on_change(move |index, _, cx| {
                let _ = view.update(cx, |view, cx| view.set_defaults(|c| c.follow_up_steer = index == 0, cx));
            })
        };
        let compact = {
            let view = view.clone();
            gpui_m3::Switch::new("compact-idle-switch", self.config().compact_idle).on_toggle(move |on, _, cx| {
                let _ = view.update(cx, |view, cx| view.set_defaults(|c| c.compact_idle = on, cx));
            })
        };
        let conversations = ListGroup::new()
            .title(tr("pill.code.settingsSearch.conversations"))
            .custom_row(gpui_m3::SettingRow::new("follow-up-row", tr("pill.code.settingsSearch.followUp")).description(tr("pill.code.settingsSearch.followUpDesc")).control(follow))
            .custom_row(gpui_m3::SettingRow::new("compact-idle-row", tr("pill.code.settingsSearch.compactIdle")).description(tr("pill.code.settingsSearch.compactIdleDesc")).control(compact));
        let notify_labels = [tr("pill.code.settingsM3.notifySystem"), tr("pill.code.settingsM3.notifySound"), tr("pill.code.settingsM3.notifyBoth"), tr("pill.code.settingsM3.notifyOff")];
        let current_notify = NotifyMode::ALL.iter().position(|m| *m == self.configs.notify).unwrap_or(0);
        let notify = {
            let view = view.clone();
            SegmentedButtons::new("notify-mode", notify_labels.iter().map(|l| Segment::label(*l)).collect::<Vec<_>>(), current_notify).on_change(move |index, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.configs.notify = NotifyMode::ALL[index];
                    view.configs.save();
                    cx.notify();
                });
            })
        };
        let alerts = ListGroup::new()
            .title(tr("pill.code.settingsM3.notifyTitle"))
            .custom_row(gpui_m3::SettingRow::new("notify-row", tr("pill.code.settingsM3.notifyRow")).description(tr("pill.code.settingsM3.notifyRowDesc")))
            .custom_row(div().pb(px(12.)).flex().child(notify));
        let ctrl_send = self.configs.shortcuts.get("send").is_some_and(|k| k == "ctrl-enter");
        let send = {
            let view = view.clone();
            SegmentedButtons::new("send-key", vec![Segment::label("Enter"), Segment::label("Ctrl+Enter")], if ctrl_send { 1 } else { 0 }).on_change(move |index, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    let mut next = view.configs.shortcuts.clone();
                    if index == 1 {
                        next.insert("send".into(), "ctrl-enter".into());
                        next.insert("newline".into(), "enter".into());
                    } else {
                        next.remove("send");
                        next.remove("newline");
                    }
                    view.set_shortcuts(next, cx);
                    cx.notify();
                });
            })
        };
        let hide_ok = {
            let view = view.clone();
            gpui_m3::Switch::new("hide-ok-switch", self.configs.hide_ok_tools).on_toggle(move |on, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.configs.hide_ok_tools = on;
                    view.configs.save();
                    cx.notify();
                });
            })
        };
        let chat = ListGroup::new()
            .title(tr("pill.code.settingsM3.chatTitle"))
            .custom_row(gpui_m3::SettingRow::new("send-key-row", tr("pill.code.settingsM3.sendWith")).description(tr("pill.code.settingsM3.sendWithDesc")).control(send))
            .custom_row(gpui_m3::SettingRow::new("hide-ok-row", tr("pill.code.settingsM3.hideOkTools")).description(tr("pill.code.settingsM3.hideOkToolsDesc")).control(hide_ok));
        let sidebar_list = {
            let view = view.clone();
            gpui_m3::Switch::new("sidebar-list-switch", self.configs.sidebar_list).on_toggle(move |on, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.configs.sidebar_list = on;
                    view.configs.save();
                    cx.notify();
                });
            })
        };
        let status_line = gpui_m3::Switch::new("status-line-switch", self.configs.status_line).on_toggle(move |on, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.configs.status_line = on;
                view.configs.save();
                cx.notify();
            });
        });
        let sidebar = ListGroup::new()
            .title(tr("pill.code.settingsM3.sidebarTitle"))
            .custom_row(gpui_m3::SettingRow::new("sidebar-list-row", tr("pill.code.settingsM3.sidebarList")).description(tr("pill.code.settingsM3.sidebarListDesc")).control(sidebar_list))
            .custom_row(gpui_m3::SettingRow::new("status-line-row", tr("pill.code.settingsM3.statusLine")).description(tr("pill.code.settingsM3.statusLineDesc")).control(status_line));
        div().flex().flex_col().gap(px(16.)).child(conversations).child(alerts).child(chat).child(sidebar).into_any_element()
    }

    /// Los valores de Claude del proyecto abierto (los de las conversaciones nuevas y de las
    /// que no eligieron otros): modelo, permisos, esfuerzo, razonamiento, modo rápido,
    /// cambiar de modelo al marcar y Remote Control.
    fn project_defaults_m3(&self, cx: &mut Context<Self>) -> ListGroup {
        use super::config;
        let Some(name) = self.workspaces.active().map(|w| w.name.clone()) else {
            return ListGroup::new()
                .title(tr("pill.code.settingsSearch.projectDefaults"))
                .custom_row(gpui_m3::SettingRow::new("defaults-empty", tr("pill.code.settingsFlat.createProject")));
        };
        let config = self.config();
        let view = cx.weak_entity();
        // Modelo: los principales como chips y «Más modelos» para el resto.
        let (shown, more) = config::split_models(&self.models, &config.model);
        let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
        let mut models = div().flex().flex_wrap().items_center().gap(px(6.));
        for index in visible {
            let (id, label) = &self.models[index];
            let id = id.clone();
            models = models.child(gpui_m3::Chip::new(("default-model", index), label.clone()).filter(config.model == id).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                let id = id.clone();
                view.set_defaults(|c| c.model = id, cx)
            })));
        }
        if !more.is_empty() {
            let label = if self.more_models { tr("pill.code.settingsFlat.fewerModels").to_string() } else { tf("pill.code.settingsFlat.moreModels", &[("count", &more.len())]) };
            models = models.child(Button::new("default-more-models", label).text().size(ButtonSize::Small).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.more_models = !view.more_models;
                cx.notify();
            })));
        }
        let modes = {
            let view = view.clone();
            let current = config::MODES.iter().position(|(id, _)| *id == config.permission_mode).unwrap_or(0);
            SegmentedButtons::new("default-mode", config::MODES.iter().map(|(id, _)| Segment::label(config::mode_name(id))).collect::<Vec<_>>(), current).on_change(move |index, _, cx| {
                let id = config::MODES[index].0;
                let _ = view.update(cx, |view, cx| view.set_config(|c| c.permission_mode = id.into(), cx));
            })
        };
        let efforts = {
            let view = view.clone();
            let mut labels = vec![Segment::label(tr("pill.code.settingsFlat.default"))];
            labels.extend(config::EFFORTS.iter().map(|(id, _)| Segment::label(config::effort_name(id))));
            let current = config::EFFORTS.iter().position(|(id, _)| *id == config.effort).map_or(0, |at| at + 1);
            SegmentedButtons::new("default-effort", labels, current).on_change(move |index, _, cx| {
                let effort = if index == 0 { String::new() } else { config::EFFORTS[index - 1].0.to_string() };
                let _ = view.update(cx, |view, cx| view.set_defaults(|c| c.effort = effort.clone(), cx));
            })
        };
        let toggle = |id: &'static str, on: bool, set: fn(&mut config::ClaudeConfig, bool)| {
            let view = view.clone();
            gpui_m3::Switch::new(id, on).on_toggle(move |on, _, cx| {
                let _ = view.update(cx, |view, cx| view.set_config(|c| set(c, on), cx));
            })
        };
        ListGroup::new()
            .title(tf("pill.code.settingsM3.projectDefaultsFor", &[("name", &name)]))
            .custom_row(gpui_m3::SettingRow::new("default-model-row", tr("pill.code.settingsM3.model")).description(tr("pill.code.settingsSearch.modelDesc")))
            .custom_row(div().pb(px(12.)).child(models))
            .custom_row(gpui_m3::SettingRow::new("default-mode-row", tr("pill.code.settingsFlat.permissions")).description(tr("pill.code.settingsSearch.permissionsDesc")))
            .custom_row(div().pb(px(12.)).flex().child(modes))
            .custom_row(gpui_m3::SettingRow::new("default-effort-row", tr("pill.code.settingsSearch.effort")).description(tr("pill.code.settingsSearch.effortDesc")))
            .custom_row(div().pb(px(12.)).flex().child(efforts))
            .custom_row(gpui_m3::SettingRow::new("default-thinking-row", tr("pill.code.settingsSearch.reasoning")).description(tr("pill.code.settingsSearch.reasoningDesc")).control(toggle("default-thinking", config.thinking, |c, on| c.thinking = on)))
            .custom_row(gpui_m3::SettingRow::new("default-fast-row", tr("pill.code.agentMenu.fastMode")).control(toggle("default-fast", config.fast_mode, |c, on| c.fast_mode = on)))
            .custom_row(gpui_m3::SettingRow::new("default-flag-row", tr("pill.code.agentMenu.switchOnFlag")).control(toggle("default-flag", config.switch_model_on_flag, |c, on| c.switch_model_on_flag = on)))
            .custom_row(gpui_m3::SettingRow::new("default-remote-row", "Remote Control").description(tr("pill.code.usage.rcOffTip")).control(toggle("default-remote", config.remote_control, |c, on| c.remote_control = on)))
    }

    /// «Teléfono»: el mismo panel de los Ajustes de la pill (vincular, dispositivos y
    /// permisos), creado la primera vez que se abre la pestaña.
    fn phone_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let existing = self.phone_pane.borrow().clone();
        let pane = match existing {
            Some(pane) => pane,
            None => {
                let pane = crate::phone_settings::phone_pane(cx);
                *self.phone_pane.borrow_mut() = Some(pane.clone());
                pane
            }
        };
        pane.into_any_element()
    }

    pub(super) fn appearance_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let group = |title: &'static str| {
            div()
                .p(px(18.))
                .rounded(px(28.))
                .bg(scheme.primary.opacity(0.06))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(t.accent).child(title))
        };
        let hints = [tr("pill.code.settingsM3.hintFormal"), tr("pill.code.settingsM3.hintExpressive"), tr("pill.code.settingsM3.hintGlass")];
        let mut cards = div().flex().gap(px(8.));
        for (index, (style, label)) in Style::ALL.iter().enumerate() {
            let style = *style;
            cards = cards.child(
                // `min_w_0`: sin él la tarjeta no baja de su contenido y se sale del grupo.
                div().flex_1().min_w_0().child(
                    SelectCard::new(("style-card", index), *label)
                        .preview(style_swatch(style))
                        .hint(hints[index])
                        .selected(self.configs.style == style)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            let mode = view.configs.mode;
                            view.set_appearance(style, mode, window, cx);
                        })),
                ),
            );
        }
        let modes = [Mode::Light, Mode::Dark, Mode::System];
        let current = modes.iter().position(|m| *m == self.configs.mode).unwrap_or(2);
        let set_mode = cx.listener(move |view, index: &usize, window, cx| {
            let style = view.configs.style;
            view.set_appearance(style, modes[*index], window, cx);
        });
        let segmented = SegmentedButtons::new("color-mode", vec![Segment::label(tr("pill.code.settingsM3.light")), Segment::label(tr("pill.code.settingsM3.dark")), Segment::label(tr("pill.code.settingsM3.system"))], current)
            .on_change(move |index, window, cx| set_mode(&index, window, cx));
        let fonts = super::config::CodeFont::ALL;
        let current_font = fonts.iter().position(|f| *f == self.configs.font).unwrap_or(0);
        let set_font = cx.listener(move |view, index: &usize, window, cx| view.set_code_font(fonts[*index], window, cx));
        let font_buttons = SegmentedButtons::new("code-font", fonts.iter().map(|f| Segment::label(f.label())), current_font).on_change(move |index, window, cx| set_font(&index, window, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            // Con un solo estilo (`appearance::ONLY_EXPRESSIVE`) no hay nada que elegir.
            .when(!crate::appearance::ONLY_EXPRESSIVE, |el| el.child(group(tr("pill.code.settingsM3.style")).child(cards)))
            .child(group(tr("pill.code.settingsM3.colorMode")).child(div().flex().child(segmented)))
            .child(group(tr("pill.code.config.font")).child(div().flex().child(font_buttons)))
            .child(group(tr("pill.code.settingsM3.chatWidth")).child(self.chat_width_cards(cx)))
            .child(group(tr("pill.code.settingsM3.accent")).child(self.accent_picker_ui(cx)))
            .into_any_element()
    }

    /// Las tarjetas del ajuste «Ancho del chat»: el adelanto de la columna y su nombre.
    /// (Estrecho, Medio, Amplio y Muy amplio son anchos de lectura; Completo, todo el ancho.)
    pub(super) fn chat_width_cards(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let labels = [tr("pill.code.settingsM3.widthFull"), tr("pill.code.settingsM3.widthNarrow"), tr("pill.code.settingsM3.widthMedium"), tr("pill.code.settingsM3.widthWide"), tr("pill.code.settingsM3.widthXWide")];
        let current = super::config::chat_width_index(self.configs.chat_width);
        let mut row = div().flex().gap(px(8.));
        for (index, label) in labels.iter().enumerate() {
            let on = index == current;
            let width = super::config::CHAT_WIDTHS[index];
            let tip: gpui::SharedString = if width <= 0. {
                tr("pill.code.settingsM3.widthFullTip").into()
            } else {
                tf("pill.code.settingsM3.widthTip", &[("width", &format!("{width:.0}"))]).into()
            };
            row = row.child(
                div()
                    .id(("chat-width", index))
                    .flex_1()
                    .min_w(px(0.))
                    .p(px(6.))
                    .rounded(px(t.r_ctl.min(14.)))
                    .border_2()
                    .border_color(if on { t.accent } else { t.border })
                    .when(on, |el| el.bg(t.accent_soft))
                    .when(!on, |el| el.hover(|el| el.bg(t.hover)))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip_text(tip))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_chat_width(width, cx)))
                    .child(chat_width_swatch(width))
                    .child(
                        div()
                            .pt(px(5.))
                            .text_center()
                            .truncate()
                            .text_size(px(11.5))
                            .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(if on { t.on_accent_soft } else { t.text })
                            .child(*label),
                    ),
            );
        }
        row.into_any_element()
    }

    /// El acento del espacio: los nueve sugeridos de la referencia, uno propio con el
    /// selector HSV y «Restablecer». Vale para el estilo de ahora.
    pub(super) fn accent_picker_ui(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let Some(workspace) = self.config_workspace() else {
            return div().text_size(px(12.5)).text_color(t.muted).child(tr("pill.code.settingsM3.openSpace")).into_any_element();
        };
        let style = self.configs.style;
        let presets = style.accent_presets();
        let custom = self.configs.accent(workspace, style);
        let current = custom.unwrap_or(presets[0]);
        let place = if workspace == super::LOOSE {
            tr("pill.code.settingsM3.looseChats").to_string()
        } else {
            self.workspaces.get(workspace).map(|w| format!("«{}»", w.name)).unwrap_or_default()
        };
        let set = cx.listener(|view, color: &gpui::Hsla, _, cx| view.set_accent_color(Some(super::style::rgb_of(*color)), cx));
        let pick = cx.listener(|view, color: &gpui::Hsla, _, cx| view.set_accent_color(Some(super::style::rgb_of(*color)), cx));
        let swatches = ColorSwatches::new("accent-swatches", presets.iter().map(|rgb| super::style::hsla_of(*rgb)))
            .current(super::style::hsla_of(current))
            .custom(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.accent_picker = !view.accent_picker;
                cx.notify();
            }))
            .on_change(move |color, window, cx| set(&color, window, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().text_size(px(12.5)).text_color(t.muted).child(tf("pill.code.settingsM3.onlyFor", &[("place", &place)])))
            .child(swatches)
            .when(self.accent_picker, |el| {
                el.child(HsvPicker::new("accent-hsv", super::style::hsla_of(current)).width(px(280.)).on_change(move |color, window, cx| pick(&color, window, cx)))
            })
            .child(
                div().flex().child(
                    Button::new("accent-reset", tr("pill.code.settingsM3.reset"))
                        .text()
                        .icon("refresh")
                        .size(ButtonSize::Small)
                        .disabled(custom.is_none())
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.accent_picker = false;
                            view.set_accent_color(None, cx);
                        })),
                ),
            )
            .into_any_element()
    }
}

/// La muestra de cada estilo, como en la referencia: tres bloques que lo imitan.
pub(super) fn style_swatch(style: Style) -> AnyElement {
    let hex = |c: u32| gpui::rgb(c);
    let block = |color: gpui::Rgba, radius: f32| div().h_full().flex_1().rounded(px(radius)).bg(color);
    let base = div().h(px(52.)).mb(px(4.)).p(px(6.)).flex().gap(px(5.));
    match style {
        Style::Formal => base
            .rounded(px(6.))
            .bg(hex(0x0e0f11))
            .child(block(hex(0x1a1c20), 3.))
            .child(div().h_full().flex_grow().rounded(px(3.)).bg(hex(0x131417)).border_t_2().border_color(hex(0x4c8dff)))
            .child(block(hex(0x1a1c20), 3.))
            .into_any_element(),
        Style::Expressive => base
            .rounded(px(14.))
            .bg(hex(0xf3edf7))
            .child(div().h_full().w(px(18.)).rounded(px(8.)).bg(hex(0xeaddff)))
            .child(div().h_full().flex_grow().rounded(px(8.)).bg(hex(0xfef7ff)))
            .child(div().h_full().flex().items_end().child(div().size(px(24.)).rounded(px(12.)).bg(hex(0x6750a4))))
            .into_any_element(),
        Style::Glass => base
            .rounded(px(12.))
            .bg(hex(0x1b1442))
            .child(div().h_full().flex_grow().rounded(px(6.)).bg(gpui::white().opacity(0.16)))
            .child(div().h_full().flex_grow().rounded(px(6.)).bg(gpui::white().opacity(0.16)))
            .child(div().h_full().flex_grow().rounded(px(6.)).bg(gpui::white().opacity(0.16)))
            .into_any_element(),
    }
}

/// El adelanto del ajuste «Ancho del chat»: la ventana con la columna del hilo.
fn chat_width_swatch(width: f32) -> gpui::Div {
    let t = t();
    // La barra crece con el ancho elegido; «Completo» llena la ventana entera.
    let ratio = if width <= 0. { 1. } else { (0.28 + width / 1220. * 0.52).min(0.84) };
    div()
        .h(px(46.))
        .w_full()
        .p(px(5.))
        .rounded(px(t.r_ctl.min(10.)))
        .bg(t.editor)
        .border_1()
        .border_color(t.border)
        .flex()
        .justify_center()
        // Concéntrica con la caja: su radio menos el margen y el borde.
        .child(div().w(gpui::relative(ratio)).h_full().rounded(px((t.r_ctl.min(10.) - 6.).max(2.))).bg(t.accent.opacity(0.75)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(version: Option<&str>, latest: Option<&str>) -> ClaudeInfo {
        ClaudeInfo { version: version.map(str::to_string), latest: latest.map(str::to_string), ..Default::default() }
    }

    #[test]
    fn el_estado_de_claude_code() {
        assert_eq!(claude_health(&ClaudeInfo { checking: true, ..Default::default() }).0, Health::Checking);
        assert_eq!(claude_health(&ClaudeInfo { error: Some("falló".into()), ..Default::default() }), (Health::Failed, "falló".into()));
        assert_eq!(claude_health(&info(None, None)).0, Health::Missing);
        assert_eq!(claude_health(&info(Some("2.1.0"), None)).0, Health::Unchecked);
        assert_eq!(claude_health(&info(Some("2.1.0"), Some("2.1.0"))), (Health::Current, "Al día".into()));
        let (health, text) = claude_health(&info(Some("2.1.0"), Some("2.2.0")));
        assert_eq!((health, text.as_str()), (Health::Outdated, "Hay una versión nueva: 2.2.0"));
        assert!(is_outdated(&info(Some("2.1.0"), Some("2.2.0"))));
        assert!(!is_outdated(&info(Some("2.1.0"), Some("2.1.0"))));
        // Instalada desde `next`: la publicada en `latest` es más vieja.
        assert!(!is_outdated(&info(Some("2.1.319"), Some("2.1.318"))));
        assert_eq!(claude_health(&info(Some("2.1.319"), Some("2.1.318"))).0, Health::Current);
        assert!(is_outdated(&info(Some("2.1.9"), Some("2.1.10"))));
    }
}
