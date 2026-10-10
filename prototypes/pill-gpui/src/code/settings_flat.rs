//! Configuración en Formal y Glass: el mismo diálogo de la referencia que la de Expressive
//! (`settings_m3`), con las pestañas «Claude» (versión de Claude Code, actualizarla,
//! la instalación, la cuenta y los valores del proyecto) y «Apariencia» (estilo,
//! modo de color y acento), dibujado con los tokens del estilo.
//!
//! Los datos y las acciones son los mismos (`ClaudeInfo`, `claude_health`,
//! `update_claude`, `accent_picker_ui`); aquí solo cambia cómo se ven.

use gpui::{div, prelude::*, px, svg, AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, SharedString, Stateful};

use super::config;
use super::settings_m3::{claude_health, is_outdated, plan_label, short_home, style_swatch};
use super::style::{t, Mode, Style};
use super::view::{chip, float_shadow, r_card};
use super::CodeView;

const TABS: [(&str, &str); 3] =
    [("icons/sparkles.svg", "Claude"), ("icons/highlighter.svg", "Apariencia"), ("icons/square-terminal.svg", "Atajos")];

/// Un grupo de la pestaña: un título y lo suyo, sobre un fondo apenas distinto.
fn group(title: &'static str) -> Div {
    let t = t();
    div()
        .p(px(16.))
        .rounded(px(r_card().min(16.)))
        .border_1()
        .border_color(if t.style == Style::Glass { t.highlight.opacity(0.25) } else { t.border })
        .bg(if t.style == Style::Glass { t.control } else { t.pane })
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(t.accent).child(title))
}

/// Un renglón de «Instalación» o «Cuenta»: lo que es y su valor.
fn info_row(label: &'static str, value: impl IntoElement) -> Div {
    let t = t();
    div()
        .py(px(6.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .text_size(px(13.))
        .child(div().text_color(t.muted).child(label))
        .child(div().min_w(px(0.)).font_weight(FontWeight::SEMIBOLD).child(value))
}

/// Un botón: lleno con el acento, o con borde.
fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    primary: bool,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
    let t = t();
    div()
        .id(id)
        .h(px(34.))
        .px(px(16.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(t.r_btn.min(17.)))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(13.))
        .map(|el| {
            if primary {
                el.bg(t.accent).text_color(t.on_accent)
            } else {
                el.border_1().border_color(t.border).text_color(t.text).hover(|el| el.bg(t.hover))
            }
        })
        .map(|el| if disabled { el.opacity(0.45) } else { el.cursor_pointer().on_click(on_click) })
        .child(label.into())
}

impl CodeView {
    /// El diálogo de Configuración de Formal y Glass.
    pub(super) fn settings_flat(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let mut nav = div()
            .w(px(200.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(12.))
            .bg(if t.style == Style::Glass { t.control } else { t.pane })
            .child(div().px(px(10.)).pt(px(6.)).pb(px(14.)).text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child("Configuración"));
        for (index, (icon, label)) in TABS.iter().enumerate() {
            let on = self.settings_tab == index;
            nav = nav.child(
                div()
                    .id(("settings-tab", index))
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(t.r_ctl.min(12.)))
                    .cursor_pointer()
                    .text_color(if on { t.on_accent_soft } else { t.text })
                    .when(on, |el| el.bg(t.accent_soft))
                    .when(!on, |el| el.hover(|el| el.bg(t.hover)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.settings_tab = index;
                        view.clear_settings_search(cx);
                        cx.notify();
                    }))
                    .child(svg().path(*icon).size(px(16.)).text_color(if on { t.on_accent_soft } else { t.muted }))
                    .child(*label),
            );
        }
        // Con algo escrito en el buscador, los resultados de todas las pestañas reemplazan la pestaña.
        let query = self.settings_query(cx);
        let searching = !query.is_empty();
        let content = if searching {
            self.settings_results(&query, false, cx)
        } else {
            match self.settings_tab {
                0 => self.claude_tab_flat(cx),
                1 => self.appearance_tab_flat(cx),
                _ => self.shortcuts_tab(cx),
            }
        };
        let body = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .pl(px(22.))
                    .pr(px(12.))
                    .pt(px(16.))
                    .pb(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(if searching { "Resultados" } else { TABS[self.settings_tab].1 }))
                    .child(super::view::icon_button("settings-close", "icons/x.svg", "Cerrar (Esc)").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.settings_open = false;
                        cx.notify();
                    }))),
            )
            .child(div().pl(px(22.)).pr(px(18.)).pt(px(2.)).pb(px(6.)).child(self.settings_search.clone()))
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pl(px(22.))
                    .pr(px(18.))
                    .pt(px(8.))
                    .pb(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(content),
            );
        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .p(px(24.))
            .bg(gpui::black().opacity(0.35))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.settings_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("settings-card")
                    .w(px(780.))
                    .h(px(560.))
                    .max_w_full()
                    .max_h_full()
                    .overflow_hidden()
                    .rounded(px(t.r_pop))
                    .shadow(float_shadow())
                    .border_1()
                    .border_color(if t.style == Style::Glass { t.highlight.opacity(0.4) } else { t.border })
                    .bg(t.raised)
                    .flex()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(nav)
                    .child(body),
            )
            .into_any_element()
    }

    /// «Claude»: la tarjeta de la versión, la instalación, la cuenta y los valores del proyecto.
    fn claude_tab_flat(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let info = self.claude_info.clone().unwrap_or_default();
        let outdated = is_outdated(&info);
        let (health, status) = claude_health(&info);
        let status_color: gpui::Hsla = health.color(&t);
        let action: AnyElement = if self.updating_claude {
            div()
                .h(px(34.))
                .flex()
                .items_center()
                .gap(px(8.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(gpui_m3::LoadingIndicator::new().size(px(20.)))
                .child("Actualizando…")
                .into_any_element()
        } else if outdated {
            button("claude-update", format!("Actualizar a {}", info.latest.clone().unwrap_or_default()), true, false, cx.listener(|view, _: &ClickEvent, _, cx| view.update_claude(cx)))
                .into_any_element()
        } else {
            button("claude-check", "Buscar actualización", false, info.version.is_none(), cx.listener(|view, _: &ClickEvent, _, cx| view.check_claude(cx)))
                .into_any_element()
        };
        let (bg, fg) = if outdated { (t.attention, t.on_attention) } else { (t.accent_soft, t.on_accent_soft) };
        let hero = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .p(px(18.))
            .rounded(px(r_card()))
            .bg(bg)
            .text_color(fg)
            .child(
                div()
                    .size(px(52.))
                    .flex_none()
                    .rounded_full()
                    .bg(t.accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(svg().path("icons/sparkles.svg").size(px(24.)).text_color(t.on_accent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).opacity(0.8).child("Claude Code"))
                    .child(div().text_size(px(30.)).font_weight(FontWeight::BOLD).line_height(px(34.)).child(info.version.clone().unwrap_or_else(|| "—".into())))
                    .child(
                        div().flex().child(
                            div()
                                .mt(px(4.))
                                .px(px(10.))
                                .py(px(3.))
                                .rounded_full()
                                .bg(t.raised.opacity(0.6))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(status_color)
                                .child(div().size(px(7.)).rounded_full().bg(status_color))
                                .child(status),
                        ),
                    ),
            )
            .child(
                div().flex().flex_col().items_end().gap(px(6.)).child(action).child(
                    button(
                        "claude-recheck",
                        if info.checking { "Comprobando…" } else { "Volver a comprobar" },
                        false,
                        info.checking || self.updating_claude,
                        cx.listener(|view, _: &ClickEvent, _, cx| view.check_claude(cx)),
                    )
                    .h(px(28.))
                    .text_size(px(12.)),
                ),
            );
        let mono = |text: String| div().font_family(t.mono).text_size(px(12.)).child(text);
        let path = info.path.clone().map(|p| short_home(&p)).unwrap_or_else(|| "—".into());
        let install = group("Instalación")
            .child(info_row("Versión instalada", mono(info.version.clone().unwrap_or_else(|| "—".into()))))
            .child(info_row("Última publicada", mono(info.latest.clone().unwrap_or_else(|| "—".into()))))
            .child(info_row(
                "Ejecutable",
                div()
                    .id("claude-path")
                    .max_w(px(300.))
                    .truncate()
                    .tooltip(crate::hover::tip_text(SharedString::from(info.target.clone().or(info.path.clone()).unwrap_or_default())))
                    .child(mono(path)),
            ));
        let mut column = div().flex().flex_col().gap(px(14.)).child(hero);
        if !self.update_log.is_empty() || self.update_result.is_some() {
            let mut log = div().flex().flex_col().gap(px(8.));
            if let Some((ok, text)) = &self.update_result {
                let color = if *ok { t.ok } else { t.bad };
                log = log.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .text_size(px(13.))
                        .text_color(color)
                        .child(svg().path(if *ok { "icons/check.svg" } else { "icons/x.svg" }).size(px(15.)).flex_none().text_color(color))
                        .child(text.clone()),
                );
            }
            if !self.update_log.is_empty() {
                log = log.child(
                    div()
                        .id("update-log")
                        .max_h(px(180.))
                        .overflow_y_scroll()
                        .p(px(10.))
                        .rounded(px(8.))
                        .bg(t.editor)
                        .font_family(t.mono)
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .child(self.update_log.clone()),
                );
            }
            column = column.child(log);
        }
        column = column.child(install);
        if let Some((email, plan, organization)) = &self.account {
            let mut account = group("Cuenta");
            if let Some(email) = email {
                account = account.child(info_row("Sesión iniciada", email.clone()));
            }
            if let Some(plan) = plan {
                account = account.child(info_row("Plan", plan_label(plan)));
            }
            if let Some(organization) = organization {
                account = account.child(info_row("Organización", organization.clone()));
            }
            column = column.child(account);
        }
        column
            .child(self.project_defaults_flat(cx))
            .child(div().px(px(4.)).text_size(px(12.5)).line_height(px(19.)).text_color(t.faint).child(
                "Atic Code usa el mismo Claude Code que tu terminal. La extensión de VS Code trae su propia copia, por eso a veces va adelantada (y ve modelos nuevos antes).",
            ))
            .into_any_element()
    }

    /// Los valores de Claude del proyecto abierto: modelo, permisos, esfuerzo y
    /// razonamiento de las conversaciones nuevas.
    fn project_defaults_flat(&self, cx: &mut Context<Self>) -> Div {
        let t = t();
        let Some(name) = self.workspaces.active().map(|w| w.name.clone()) else {
            return group("Valores del proyecto").child(div().text_size(px(13.)).text_color(t.muted).child("Crea un proyecto para configurarlo."));
        };
        let config = self.config();
        let row = |label: &'static str, chips: Div| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(label))
                .child(chips.flex().flex_wrap().gap(px(6.)))
        };
        let (shown, more) = config::split_models(&self.models, &config.model);
        let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
        let mut models = div();
        for index in visible {
            let (id, model) = &self.models[index];
            let id = id.clone();
            models = models.child(
                chip(("set-model", index), model.clone(), config.model == id).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    view.set_defaults(|c| c.model = id, cx)
                })),
            );
        }
        if !more.is_empty() {
            let label = if self.more_models { "Menos modelos".to_string() } else { format!("Más modelos ({})", more.len()) };
            models = models.child(
                div()
                    .id("set-more-models")
                    .px(px(10.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .rounded(px(t.r_ctl.min(12.)))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(t.accent)
                    .hover(|el| el.bg(t.hover))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.more_models = !view.more_models;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        let mut modes = div();
        for (id, label) in config::MODES {
            modes = modes.child(
                chip(SharedString::from(format!("set-mode-{id}")), label, config.permission_mode == id)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_config(|c| c.permission_mode = id.into(), cx))),
            );
        }
        let mut efforts = div().child(
            chip("set-effort-default", "Predeterminado", config.effort.is_empty())
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_defaults(|c| c.effort = String::new(), cx))),
        );
        for (id, label) in config::EFFORTS {
            efforts = efforts.child(
                chip(SharedString::from(format!("set-effort-{id}")), label, config.effort == id)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_defaults(|c| c.effort = id.into(), cx))),
            );
        }
        let thinking = div()
            .child(chip("set-think-on", "Mostrar", config.thinking).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_config(|c| c.thinking = true, cx))))
            .child(chip("set-think-off", "Ocultar", !config.thinking).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_config(|c| c.thinking = false, cx))));
        group("Valores del proyecto")
            .child(div().text_size(px(12.5)).text_color(t.muted).child(format!("«{name}». Valen para las conversaciones nuevas; modelo, permisos y razonamiento se aplican también a las abiertas, y el esfuerzo, desde la próxima.")))
            .child(row("Modelo", models))
            .child(row("Permisos", modes))
            .child(row("Esfuerzo", efforts))
            .child(row("Razonamiento", thinking))
    }

    /// «Apariencia»: las tarjetas de estilo, el modo de color y el acento.
    pub(super) fn appearance_tab_flat(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let hints = ["Sobrio, denso y rápido", "Color, formas y movimiento", "Vidrio, luz y profundidad"];
        let mut cards = div().flex().gap(px(8.));
        for (index, (style, label)) in Style::ALL.iter().enumerate() {
            let style = *style;
            let on = self.configs.style == style;
            cards = cards.child(
                div()
                    .id(("style-card", index))
                    .flex_1()
                    .p(px(8.))
                    .rounded(px(t.r_ctl.min(14.)))
                    .border_2()
                    .border_color(if on { t.accent } else { t.border })
                    .when(on, |el| el.bg(t.accent_soft))
                    .when(!on, |el| el.hover(|el| el.bg(t.hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        let mode = view.configs.mode;
                        view.set_appearance(style, mode, window, cx);
                    }))
                    .child(style_swatch(style))
                    .child(div().px(px(2.)).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(if on { t.on_accent_soft } else { t.text }).child(*label))
                    .child(div().px(px(2.)).text_size(px(11.5)).text_color(if on { t.on_accent_soft } else { t.muted }).child(hints[index])),
            );
        }
        // El modo de color: un segmentado con tres opciones.
        let mut segments = div().flex().rounded(px(t.r_btn.min(16.))).border_1().border_color(t.border).overflow_hidden();
        for (index, (mode, label)) in Mode::ALL.iter().enumerate() {
            let mode = *mode;
            let on = self.configs.mode == mode;
            segments = segments.child(
                div()
                    .id(("color-mode", index))
                    .h(px(32.))
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .cursor_pointer()
                    .when(on, |el| el.bg(t.accent_soft).text_color(t.on_accent_soft).font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |el| el.hover(|el| el.bg(t.hover)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        let style = view.configs.style;
                        view.set_appearance(style, mode, window, cx);
                    }))
                    .child(*label),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(group("Estilo").child(cards))
            .child(group("Modo de color").child(div().flex().child(segments)))
            .child(group("Color de acento").child(self.accent_picker_ui(cx)))
            .into_any_element()
    }
}
