//! Los menús flotantes de la caja de texto, como los de la referencia: modelo y
//! esfuerzo, «/» (acciones y comandos de Claude Code) y el modo de permisos.
//!
//! Se dibujan encima de todo (`deferred`), anclados donde se hizo clic y
//! abriendo hacia arriba; un clic fuera o Esc los cierra.

use gpui::{
    anchored, deferred, div, point, prelude::*, px, svg, AnyElement, ClickEvent, Context, Corner, Div, FontWeight,
    MouseButton, SharedString, Stateful,
};

use super::config;
use super::style::{t, Style};
use super::{CodeView, Menu};

const MENU_W: f32 = 340.0;
const MENU_MAX_H: f32 = 460.0;

fn card() -> Div {
    let t = t();
    div()
        .w(px(MENU_W))
        .p(px(6.))
        .rounded(px(t.r_pop))
        .bg(t.raised)
        .border_1()
        .border_color(if t.style == Style::Glass { t.highlight.opacity(0.4) } else { t.border })
        .shadow(super::view::float_shadow())
        .flex()
        .flex_col()
        .text_size(px(13.5))
        .text_color(t.text)
}

fn heading(text: &'static str) -> Div {
    let t = t();
    div().px(px(10.)).pt(px(8.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.faint).child(text)
}

fn divider() -> Div {
    div().mx(px(8.)).my(px(6.)).h(px(1.)).bg(t().border)
}

/// Una fila del menú, con su marca de radio si `radio` es `Some`.
fn item(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, hint: Option<String>, radio: Option<bool>) -> Stateful<Div> {
    let t = t();
    div()
        .id(id)
        .px(px(10.))
        .py(px(7.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(t.r_ctl.min(12.)))
        .cursor_pointer()
        .hover(|el| el.bg(t.hover))
        .when_some(radio, |el, on| {
            el.child(
                div()
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_2()
                    .border_color(if on { t.accent } else { t.faint })
                    .when(on, |el| el.child(div().size(px(8.)).rounded_full().bg(t.accent))),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(div().truncate().child(label.into()))
                .when_some(hint.filter(|h| !h.is_empty()), |el, hint| {
                    el.child(div().truncate().text_size(px(12.)).text_color(t.faint).child(hint))
                }),
        )
}

/// Un interruptor encendido o apagado.
pub(super) fn switch(on: bool) -> Div {
    let t = t();
    div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .p(px(2.))
        .rounded_full()
        .flex()
        .when(on, |el| el.bg(t.accent).justify_end())
        .when(!on, |el| el.bg(t.control2))
        .child(div().size(px(16.)).rounded_full().bg(if on { t.on_accent } else { t.faint }))
}

impl CodeView {
    pub(super) fn menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (menu, at) = self.menu?;
        let expressive = super::view::expressive();
        let content = match (menu, expressive) {
            (Menu::Model, false) => self.model_menu(cx),
            // El Rewind (Esc Esc) es el submenú de Expressive también aquí.
            (Menu::Actions, false) if self.menu_sub == Some(super::agent_menu::Sub::Rewind) => self.agent_menu(self.menu_sub, cx),
            (Menu::Actions, false) => self.actions_menu(cx),
            (Menu::Mode, false) => self.mode_menu(cx),
            (Menu::Model, true) => self.agent_menu(Some(self.menu_sub.unwrap_or(super::agent_menu::Sub::Model)), cx),
            (Menu::Actions, true) => self.agent_menu(self.menu_sub, cx),
            (Menu::Mode, true) => self.mode_menu_m3(cx),
            (Menu::Project, _) => self.project_menu_m3(cx),
        };
        // En Expressive, como en la referencia, los menús de la caja salen sobre ella y
        // el del proyecto, bajo su botón.
        let (position, corner) = match (menu, expressive, self.composer_bounds.get()) {
            (Menu::Model | Menu::Actions, true, Some(bounds)) => (point(bounds.left(), bounds.top() - px(8.)), Corner::BottomLeft),
            (Menu::Actions, false, Some(bounds)) if self.menu_sub.is_some() => (point(bounds.left(), bounds.top() - px(8.)), Corner::BottomLeft),
            (Menu::Project, _, _) => (point(at.x - px(20.), at.y + px(22.)), Corner::TopLeft),
            _ => (point(at.x - px(18.), at.y - px(22.)), Corner::BottomLeft),
        };
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| {
                            view.menu = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        anchored()
                            .position(position)
                            .anchor(corner)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .id("menu-card")
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .child(content),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Los comandos de Claude Code para el menú «/».
    pub(super) fn command_names(&self) -> Vec<(String, String)> {
        if self.commands.is_empty() {
            // Antes de la primera sesión el SDK todavía no dijo cuáles hay.
            [("compact", "Resume la conversación para liberar contexto"), ("context", "Cuánto contexto se está usando"), ("cost", "Lo que lleva la sesión"), ("review", "Revisa los cambios")]
                .iter()
                .map(|(n, d)| (n.to_string(), d.to_string()))
                .collect()
        } else {
            self.commands.clone()
        }
    }

    fn model_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let chat_effort = self.chat_effort();
        let level = config::EFFORTS.iter().position(|(id, _)| *id == chat_effort);
        let mut effort = div().flex().gap(px(4.)).flex_none();
        for (index, (id, label)) in config::EFFORTS.iter().enumerate() {
            let filled = level.is_some_and(|level| index <= level);
            let id = *id;
            effort = effort.child(
                div()
                    .id(("effort", index))
                    .w(px(22.))
                    .h(px(12.))
                    .rounded(px(if t.style == Style::Formal { 3. } else { 6. }))
                    .bg(if filled { t.accent } else { t.border })
                    .cursor_pointer()
                    .hover(|el| el.opacity(0.85))
                    .tooltip(crate::hover::tip(label))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let pick = if view.chat_effort() == id { String::new() } else { id.to_string() };
                        view.set_config(|c| c.effort = pick, cx);
                    })),
            );
        }
        let current = self.chat_model();
        let selected = self.current_model(&current);
        let (shown, more) = config::split_models(&self.models, selected.unwrap_or(&current));
        let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
        let mut list = div().id("model-list").max_h(px(260.)).overflow_y_scroll().flex().flex_col();
        for index in visible {
            let (id, name) = &self.models[index];
            let on = selected == Some(id.as_str());
            let id = id.clone();
            list = list.child(item(("model", index), name.clone(), None, Some(on)).on_click(cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    view.set_config(|c| c.model = id, cx);
                    view.menu = None;
                    cx.notify();
                },
            )));
        }
        if !more.is_empty() {
            let label = if self.more_models { "Menos modelos".to_string() } else { format!("Más modelos ({})", more.len()) };
            list = list.child(
                div()
                    .id("model-more")
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(t.r_ctl.min(12.)))
                    .cursor_pointer()
                    .text_color(t.accent)
                    .hover(|el| el.bg(t.hover))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.more_models = !view.more_models;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        card()
            .child(heading("Modelo"))
            .child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(svg().path("icons/gauge.svg").size(px(16.)).text_color(t.muted))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child("Esfuerzo")
                            .child(div().text_size(px(12.)).text_color(t.faint).child(config::effort_label(&chat_effort))),
                    )
                    .child(effort),
            )
            .child(
                div()
                    .id("model-thinking")
                    .px(px(10.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(t.r_ctl.min(12.)))
                    .cursor_pointer()
                    .hover(|el| el.bg(t.hover))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        let on = !view.config().thinking;
                        view.set_config(|c| c.thinking = on, cx);
                    }))
                    .child(svg().path("icons/brain.svg").size(px(16.)).text_color(t.muted))
                    .child(div().flex_1().child("Razonamiento visible"))
                    .child(switch(self.config().thinking)),
            )
            .child(divider())
            .child(list)
            .into_any_element()
    }

    fn mode_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let config = self.config();
        let mut menu = card().child(heading("Permisos"));
        for (id, label) in config::MODES {
            let on = config.permission_mode == id;
            menu = menu.child(
                item(SharedString::from(format!("mode-{id}")), label, Some(config::mode_hint(id).to_string()), Some(on)).on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_config(|c| c.permission_mode = id.into(), cx);
                        view.menu = None;
                        cx.notify();
                    }),
                ),
            );
        }
        menu.into_any_element()
    }

    fn actions_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let config = self.config();
        let action = |id: &'static str, icon: &'static str, label: &'static str, value: Option<String>| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(7.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(t.r_ctl.min(12.)))
                .cursor_pointer()
                .hover(|el| el.bg(t.hover))
                .child(svg().path(icon).size(px(16.)).flex_none().text_color(t.muted))
                .child(div().flex_1().child(label))
                .when_some(value, |el, value| el.child(div().text_size(px(12.5)).text_color(t.faint).child(value)))
        };
        let chat_model = self.chat_model();
        let model_label = if chat_model.is_empty() { "Predeterminado".to_string() } else { self.model_label(&chat_model) };
        let mut commands = div().flex().flex_col();
        for (index, (name, description)) in self.command_names().into_iter().enumerate() {
            let insert = name.clone();
            commands = commands.child(
                item(("command", index), format!("/{name}"), Some(description), None)
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.insert_command(&insert, window, cx))),
            );
        }
        card()
            .child(
                div()
                    .id("actions-scroll")
                    .max_h(px(MENU_MAX_H))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(heading("Contexto"))
                    .child(
                        action("act-attach", "icons/plus.svg", "Adjuntar archivo…", Some("Ctrl+V pega imágenes".into()))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.pick_attachments(cx))),
                    )
                    .child(
                        action("act-rewind", "icons/history.svg", "Rewind", Some("Esc Esc".into()))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.open_rewind(cx))),
                    )
                    .child(heading("Modelo"))
                    .child(
                        action("act-model", "icons/cpu.svg", "Cambiar modelo…", Some(model_label)).on_click(cx.listener(
                            |view, event: &ClickEvent, _, cx| {
                                view.menu = Some((Menu::Model, event.position()));
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        action("act-thinking", "icons/brain.svg", "Razonamiento visible", None)
                            .child(switch(config.thinking))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                let on = !view.config().thinking;
                                view.set_config(|c| c.thinking = on, cx);
                            })),
                    )
                    .child(heading("Personalizar"))
                    .child(
                        action("act-mode", "icons/lock.svg", "Permisos", Some(config.mode_label().to_string())).on_click(cx.listener(
                            |view, event: &ClickEvent, _, cx| {
                                view.menu = Some((Menu::Mode, event.position()));
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        action("act-settings", "icons/settings-2.svg", "Configuración y apariencia…", None).on_click(cx.listener(
                            |view, _: &ClickEvent, _, cx| {
                                view.menu = None;
                                view.settings_open = true;
                                cx.notify();
                            },
                        )),
                    )
                    .child(heading("Comandos y skills"))
                    .child(commands),
            )
            .into_any_element()
    }

    // --- Expressive: los menús de gpui-m3 --------------------------------------------

    /// «Elegir proyecto» del inicio: los otros proyectos y abrir una carpeta.
    fn project_menu_m3(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_m3::MenuItem;
        let active = self.workspaces.active_id();
        let others: Vec<(u64, String)> =
            self.workspaces.list().iter().filter(|w| Some(w.id) != active).map(|w| (w.id, w.name.clone())).take(6).collect();
        let mut menu = gpui_m3::Menu::new("menu-project").width(240.).max_h(px(MENU_MAX_H));
        if !others.is_empty() {
            menu = menu.section("Recientes");
            for (index, (id, name)) in others.into_iter().enumerate() {
                menu = menu.item(MenuItem::new(("project", index), name).icon("folder").on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.menu = None;
                        view.select_workspace(id, cx);
                    },
                )));
            }
            menu = menu.separator();
        }
        // El chat sin proyecto, o volver al proyecto desde uno (como el selector de la referencia).
        menu = match self.workspaces.active().filter(|_| self.in_loose_chat()) {
            Some(workspace) => menu.item(MenuItem::new("project-back", workspace.name.clone()).icon("folder").on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| {
                    view.menu = None;
                    view.new_conversation(window, cx);
                },
            ))),
            None => menu.item(MenuItem::new("project-loose", "Chat sin proyecto").icon("chat").on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| {
                    view.menu = None;
                    view.new_loose_chat(window, cx);
                },
            ))),
        };
        menu.item(MenuItem::new("project-open", "Abrir carpeta…").icon("folder").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
            view.menu = None;
            view.pick_folders(None, cx);
        })))
        .into_any_element()
    }

    fn mode_menu_m3(&self, cx: &mut Context<Self>) -> AnyElement {
        let config = self.config();
        let mut menu = gpui_m3::Menu::new("menu-mode").width(MENU_W).section("Permisos");
        for (id, label) in config::MODES {
            let on = config.permission_mode == id;
            menu = menu.item(
                gpui_m3::MenuItem::new(SharedString::from(format!("mode-{id}")), label)
                    .sublabel(config::mode_hint(id))
                    .selected(on)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_config(|c| c.permission_mode = id.into(), cx);
                        view.menu = None;
                        cx.notify();
                    })),
            );
        }
        menu.into_any_element()
    }
}
