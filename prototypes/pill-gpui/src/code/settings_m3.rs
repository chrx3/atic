//! Configuración en Expressive, como la de la referencia: un diálogo de 820×600 con
//! las pestañas «Claude» (versión de Claude Code, actualizarla, la instalación
//! y la cuenta) y «Apariencia» (estilo y modo de color).

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString};
use gpui_m3::{Button, ButtonSize, Dialog, IconButton, ListGroup, LoadingIndicator, NavItem, Segment, SegmentedButtons, Shape, ShapeName, Tone};
use serde_json::{json, Value};

use super::style::{t, Mode, Style};
use super::CodeView;

const TABS: [(&str, &str); 2] = [("spark", "Claude"), ("palette", "Apariencia")];

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

fn plan_label(plan: &str) -> String {
    match plan {
        "max" => "Max".into(),
        "pro" => "Pro".into(),
        "team" => "Team".into(),
        "enterprise" => "Enterprise".into(),
        "free" => "Gratis".into(),
        other => other.to_string(),
    }
}

/// `C:\Users\x\…` → `~\…`.
fn short_home(path: &str) -> String {
    match std::env::var("USERPROFILE") {
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

    fn update_claude(&mut self, cx: &mut Context<Self>) {
        self.updating_claude = true;
        self.update_log.clear();
        self.update_result = None;
        self.request("claudeUpdate", json!({}), cx, |view, reply, cx| {
            view.updating_claude = false;
            view.update_result = Some(match reply {
                Ok(result) if result.get("ok").and_then(Value::as_bool).unwrap_or(false) => {
                    let version = result.get("version").and_then(Value::as_str).unwrap_or("?");
                    (true, format!("Listo: Claude Code {version}. Las conversaciones nuevas ya usan esta versión; las abiertas, al reabrirlas."))
                }
                Ok(result) => {
                    let message = result.get("message").and_then(Value::as_str).map(str::to_string);
                    let code = result.get("code").map(|c| c.to_string()).unwrap_or_else(|| "?".into());
                    (false, message.unwrap_or_else(|| format!("claude update terminó con error ({code})")))
                }
                Err(error) => (false, error),
            });
            view.check_claude(cx);
        });
    }

    pub(super) fn settings_m3(&self, cx: &mut Context<Self>) -> AnyElement {
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
            .bg(scheme.primary.opacity(0.05))
            .child(div().px(px(16.)).pb(px(16.)).text_size(px(22.)).font_weight(FontWeight::BOLD).text_color(t.text).child("Configuración"));
        for (index, (icon, label)) in TABS.iter().enumerate() {
            nav = nav.child(
                NavItem::new(("settings-tab", index), *label).icon(*icon).large(true).tone(Tone::Primary).selected(self.settings_tab == index).on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.settings_tab = index;
                        cx.notify();
                    }),
                ),
            );
        }
        let content = if self.settings_tab == 0 { self.claude_tab(cx) } else { self.appearance_tab(cx) };
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
                    .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).text_color(t.text).child(TABS[self.settings_tab].1))
                    .child(IconButton::new("settings-close", "x").size(px(32.)).tooltip("Cerrar (Esc)").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.settings_open = false;
                        cx.notify();
                    }))),
            )
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
            .sized(px(820.), px(600.))
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
        let outdated = matches!((&info.version, &info.latest), (Some(v), Some(l)) if v != l);
        let (bg, fg, mark) =
            if outdated { (scheme.tertiary_container, scheme.on_tertiary_container, scheme.tertiary) } else { (scheme.primary_container, scheme.on_primary_container, scheme.primary) };
        let (status_color, status) = if info.checking {
            (t.faint, "Comprobando…".to_string())
        } else if let Some(error) = &info.error {
            (t.bad, error.clone())
        } else if info.version.is_none() {
            (t.bad, "No se encontró Claude Code".to_string())
        } else if info.latest.is_none() {
            (t.faint, "No se pudo comprobar si hay versión nueva".to_string())
        } else if outdated {
            (t.warn, format!("Hay una versión nueva: {}", info.latest.clone().unwrap_or_default()))
        } else {
            (t.ok, "Al día".to_string())
        };
        let mut shape = Shape::new(ShapeName::Sunny).size(px(64.)).color(mark).child(gpui_m3::Icon::new("spark").size(px(26.)).color(scheme.on_primary));
        if self.updating_claude {
            shape = shape.breathe(ShapeName::Cookie9, 1.6);
        }
        let action: AnyElement = if self.updating_claude {
            div().h(px(48.)).flex().items_center().gap(px(8.)).font_weight(FontWeight::BOLD).child(LoadingIndicator::new().size(px(28.))).child("Actualizando…").into_any_element()
        } else if outdated {
            Button::new("claude-update", format!("Actualizar a {}", info.latest.clone().unwrap_or_default()))
                .filled()
                .icon("up")
                .size(ButtonSize::Large)
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.update_claude(cx)))
                .into_any_element()
        } else {
            Button::new("claude-check", "Buscar actualización")
                .tonal()
                .icon("refresh")
                .size(ButtonSize::Large)
                .disabled(info.version.is_none())
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.check_claude(cx)))
                .into_any_element()
        };
        let hero = div()
            .flex()
            .items_center()
            .gap(px(20.))
            .pl(px(20.))
            .pr(px(22.))
            .py(px(22.))
            .rounded(px(28.))
            .bg(bg)
            .text_color(fg)
            .child(shape)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::BOLD).opacity(0.8).child("Claude Code"))
                    .child(div().text_size(px(40.)).font_weight(FontWeight::EXTRA_BOLD).line_height(px(42.)).child(info.version.clone().unwrap_or_else(|| "—".into())))
                    .child(
                        div().flex().child(
                            div()
                                .mt(px(6.))
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
                                .child(div().size(px(7.)).rounded_full().bg(status_color))
                                .child(status),
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
                        Button::new("claude-recheck", if info.checking { "Comprobando…" } else { "Volver a comprobar" })
                            .text()
                            .size(ButtonSize::Small)
                            .disabled(info.checking || self.updating_claude)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.check_claude(cx))),
                    ),
            );
        let mono = |text: String| div().font_family(gpui_m3::theme::MONO_FONT_FAMILY).text_size(px(12.)).font_weight(FontWeight::BOLD).child(text);
        let path = info.path.clone().map(|p| short_home(&p)).unwrap_or_else(|| "—".into());
        let install = ListGroup::new()
            .title("Instalación")
            .row("Versión instalada", mono(info.version.clone().unwrap_or_else(|| "—".into())))
            .row("Última publicada", mono(info.latest.clone().unwrap_or_else(|| "—".into())))
            .row("Ejecutable", div().id("claude-path").max_w(px(280.)).truncate().tooltip(crate::hover::tip_text(SharedString::from(info.target.clone().or(info.path.clone()).unwrap_or_default()))).child(mono(path)));
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
                        .child(text.clone()),
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
            let mut account = ListGroup::new().title("Cuenta");
            if let Some(email) = email {
                account = account.row("Sesión iniciada", div().font_weight(FontWeight::BOLD).child(email.clone()));
            }
            if let Some(plan) = plan {
                account = account.row("Plan", div().font_weight(FontWeight::BOLD).child(plan_label(plan)));
            }
            if let Some(organization) = organization {
                account = account.row("Organización", div().font_weight(FontWeight::BOLD).child(organization.clone()));
            }
            column = column.child(account);
        }
        column
            .child(div().px(px(6.)).text_size(px(12.5)).line_height(px(19.)).text_color(t.faint).child(
                "Atic Code usa el mismo Claude Code que tu terminal. La extensión de VS Code trae su propia copia, por eso a veces va adelantada (y ve modelos nuevos antes).",
            ))
            .into_any_element()
    }

    fn appearance_tab(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let hints = ["Sobrio, denso y rápido", "Color, formas y movimiento", "Vidrio, luz y profundidad"];
        let mut cards = div().flex().gap(px(8.));
        for (index, (style, label)) in Style::ALL.iter().enumerate() {
            let style = *style;
            let on = self.configs.style == style;
            cards = cards.child(
                div()
                    .id(("style-card", index))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .p(px(8.))
                    .rounded(px(18.))
                    .border(px(if on { 2. } else { 1. }))
                    .border_color(if on { t.accent } else { t.border })
                    .cursor_pointer()
                    .hover(|el| el.bg(t.hover))
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        let mode = view.configs.mode;
                        view.set_appearance(style, mode, window, cx);
                    }))
                    .child(style_swatch(style))
                    .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(*label))
                    .child(div().text_size(px(11.5)).text_color(t.muted).child(hints[index])),
            );
        }
        let modes = [Mode::Light, Mode::Dark, Mode::System];
        let current = modes.iter().position(|m| *m == self.configs.mode).unwrap_or(2);
        let set_mode = cx.listener(move |view, index: &usize, window, cx| {
            let style = view.configs.style;
            view.set_appearance(style, modes[*index], window, cx);
        });
        let segmented = SegmentedButtons::new("color-mode", vec![Segment::label("Claro"), Segment::label("Oscuro"), Segment::label("Sistema")], current)
            .on_change(move |index, window, cx| set_mode(&index, window, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(group("Estilo").child(cards))
            .child(group("Modo de color").child(div().flex().child(segmented)))
            .into_any_element()
    }
}

/// La muestra de cada estilo, como en la referencia: tres bloques que lo imitan.
fn style_swatch(style: Style) -> AnyElement {
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
            .child(div().h_full().w(px(18.)).rounded(px(10.)).bg(hex(0xeaddff)))
            .child(div().h_full().flex_grow().rounded(px(10.)).bg(hex(0xfef7ff)))
            .child(div().h_full().flex().items_end().child(div().size(px(24.)).rounded(px(12.)).bg(hex(0x6750a4))))
            .into_any_element(),
        Style::Glass => base
            .rounded(px(12.))
            .bg(hex(0x1b1442))
            .child(div().h_full().flex_grow().rounded(px(8.)).bg(gpui::white().opacity(0.16)))
            .child(div().h_full().flex_grow().rounded(px(8.)).bg(gpui::white().opacity(0.16)))
            .child(div().h_full().flex_grow().rounded(px(8.)).bg(gpui::white().opacity(0.16)))
            .into_any_element(),
    }
}
