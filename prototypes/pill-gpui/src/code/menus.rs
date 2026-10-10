//! Los menús flotantes de la caja de texto, como los de la referencia: modelo y
//! esfuerzo, «/» (acciones y comandos de Claude Code) y el modo de permisos.
//!
//! Se dibujan encima de todo (`deferred`), anclados donde se hizo clic y
//! abriendo hacia arriba; un clic fuera o Esc los cierra.

use gpui::{
    anchored, deferred, div, point, prelude::*, px, svg, AnyElement, ClickEvent, Context, Corner, Div, FontWeight,
    MouseButton, SharedString, Stateful,
};

use super::agent_menu::{Entry, Line, Sub, Trailing, TABS};
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
        .when_some(radio, |el, on| el.child(radio_mark(on)))
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

/// La marca de un radio.
fn radio_mark(on: bool) -> Div {
    let t = t();
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_2()
        .border_color(if on { t.accent } else { t.faint })
        .when(on, |el| el.child(div().size(px(8.)).rounded_full().bg(t.accent)))
}

/// Los íconos de gpui-m3 del menú de acciones, con el svg más parecido de Atic.
fn flat_icon(name: &str) -> &'static str {
    match name {
        "clip" => "icons/plus.svg",
        "at" | "file" => "icons/file.svg",
        "trash" => "icons/trash.svg",
        "history" => "icons/history.svg",
        "bookmark" => "icons/star.svg",
        "export" => "icons/arrow-up-right.svg",
        "copy" => "icons/copy.svg",
        "cpu" => "icons/cpu.svg",
        "gauge" => "icons/gauge.svg",
        "spark" => "icons/sparkles.svg",
        "brain" => "icons/brain.svg",
        "shield" => "icons/lock.svg",
        "bolt" => "icons/activity.svg",
        "palette" => "icons/highlighter.svg",
        "plug" | "layers" => "icons/layers.svg",
        "hook" => "icons/code.svg",
        "command" | "terminal" => "icons/square-terminal.svg",
        "box" => "icons/square.svg",
        "monitor" => "icons/monitor.svg",
        "gear" => "icons/settings-2.svg",
        "blocks" => "icons/layout-grid.svg",
        "refresh" => "icons/rotate-cw.svg",
        "x" => "icons/x.svg",
        "chevron-down" => "icons/chevron-down.svg",
        "chevron-right" => "icons/chevron-right.svg",
        "search" => "icons/search.svg",
        _ => "icons/circle.svg",
    }
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
    pub(super) fn menu_layer(&self, window: &mut gpui::Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Al cerrarse, el menú se sigue dibujando mientras se desvanece (`Presence`).
        let shown = self.menu_last.show("menu-presence", self.menu, window, cx)?;
        let (menu, at) = shown.value;
        let expressive = super::view::expressive();
        let content = match (menu, expressive) {
            // Formal y Glass tienen el mismo menú de acciones que Expressive, con sus
            // piezas (el Rewind, Esc Esc, es un submenú también aquí).
            (Menu::Model, false) => self.agent_menu_flat(Some(self.menu_sub.unwrap_or(Sub::Model)), cx),
            (Menu::Actions, false) => self.agent_menu_flat(self.menu_sub, cx),
            (Menu::Mode, false) => self.mode_menu(cx),
            (Menu::Model, true) => self.agent_menu(Some(self.menu_sub.unwrap_or(Sub::Model)), cx),
            (Menu::Actions, true) => self.agent_menu(self.menu_sub, cx),
            (Menu::Mode, true) => self.mode_menu_m3(cx),
            (Menu::Project, _) => self.project_menu_m3(cx),
        };
        // Los menús de la caja salen sobre ella, como en la referencia, y el del proyecto, bajo su botón.
        let (position, corner) = match (menu, self.composer_bounds.get()) {
            (Menu::Model | Menu::Actions, Some(bounds)) => (point(bounds.left(), bounds.top() - px(8.)), Corner::BottomLeft),
            (Menu::Project, _) => (point(at.x - px(20.), at.y + px(22.)), Corner::TopLeft),
            _ => (point(at.x - px(18.), at.y - px(22.)), Corner::BottomLeft),
        };
        // Los que abren hacia arriba se van hacia el ancla; el del proyecto, hacia abajo.
        let exit = if matches!(menu, Menu::Project) { gpui_m3::Exit::Sink } else { gpui_m3::Exit::Rise };
        let leaving = shown.leaving;
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                view.menu = None;
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored()
                            .position(position)
                            .anchor(corner)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .id("menu-card")
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .child(shown.wrap(exit, content)),
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

    // --- Formal y Glass: el menú de acciones con los tokens del estilo ------------------

    /// Los puntos del esfuerzo: uno por nivel, y un clic repite el nivel para dejarlo
    /// en el predeterminado.
    fn effort_dots(&self, cx: &mut Context<Self>) -> Div {
        let t = t();
        let level = config::EFFORTS.iter().position(|(id, _)| *id == self.chat_effort());
        let mut dots = div().flex().gap(px(4.)).flex_none();
        for (index, (id, label)) in config::EFFORTS.iter().enumerate() {
            let filled = level.is_some_and(|level| index <= level);
            let id = *id;
            dots = dots.child(
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
        dots
    }

    /// Un botón chico dentro de una fila (Reconectar, Código, Ambos…).
    fn flat_pick(pick: super::agent_menu::Pick) -> Stateful<Div> {
        let t = t();
        div()
            .id(pick.id)
            .h(px(24.))
            .px(px(10.))
            .flex()
            .flex_none()
            .items_center()
            .rounded(px(t.r_chip.min(12.)))
            .border_1()
            .border_color(t.border)
            .text_size(px(12.))
            .cursor_pointer()
            .hover(|el| el.bg(t.hover))
            .on_click(pick.click)
            .child(pick.label)
    }

    /// Una fila del menú de acciones en Formal o Glass.
    fn flat_line(&self, line: Line, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let interactive = !line.static_row && line.click.is_some();
        let subtitle = line.sublabel.filter(|text| !text.is_empty());
        let label = div().truncate().child(line.label);
        let body = if line.inline_sub {
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(label)
                .when_some(subtitle, |el, text| el.child(div().truncate().text_size(px(12.)).text_color(t.faint).child(text)))
        } else {
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(label)
                .when_some(subtitle, |el, text| el.child(div().truncate().text_size(px(12.)).text_color(t.faint).child(text)))
        };
        let trailing: Option<AnyElement> = match line.trailing {
            Trailing::None => None,
            Trailing::Value(value) => Some(div().flex_none().max_w(px(150.)).truncate().text_size(px(12.5)).text_color(t.faint).child(value).into_any_element()),
            Trailing::Kbd(kbd) => Some(div().flex_none().text_size(px(12.)).text_color(t.faint).child(kbd).into_any_element()),
            Trailing::Switch(on) => Some(switch(on).into_any_element()),
            Trailing::Effort => Some(self.effort_dots(cx).into_any_element()),
            Trailing::Chips(picks) => Some(div().flex().flex_none().gap(px(4.)).children(picks.into_iter().map(Self::flat_pick)).into_any_element()),
        };
        div()
            .id(line.id)
            .px(px(10.))
            .py(px(7.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(t.r_ctl.min(12.)))
            .when_some(line.dot, |el, color| el.child(div().size(px(8.)).flex_none().rounded_full().bg(color)))
            .when_some(line.radio, |el, on| el.child(radio_mark(on)))
            .when_some(line.icon.filter(|_| line.radio.is_none()), |el, icon| el.child(svg().path(flat_icon(icon)).size(px(16.)).flex_none().text_color(t.muted)))
            .child(body)
            .when_some(trailing, |el, trailing| el.child(trailing))
            .when_some(line.click.filter(|_| interactive), |el, click| el.cursor_pointer().hover(|el| el.bg(t.hover)).on_click(click))
            .into_any_element()
    }

    /// Las entradas de un menú, ya dibujadas.
    fn flat_entries(&self, entries: Vec<Entry>, cx: &mut Context<Self>) -> Div {
        let t = t();
        let mut list = div().flex().flex_col();
        for entry in entries {
            list = match entry {
                Entry::Section(title) => list.child(heading(title)),
                Entry::Separator => list.child(divider()),
                Entry::Empty(text) => list.child(div().p(px(16.)).flex().justify_center().text_color(t.faint).child(text)),
                Entry::Line(line) => list.child(self.flat_line(line, cx)),
                Entry::Block { label, picks } => list.child(
                    div()
                        .px(px(10.))
                        .py(px(7.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().truncate().text_size(px(13.)).child(label))
                        .child(div().flex().gap(px(4.)).children(picks.into_iter().map(Self::flat_pick))),
                ),
            };
        }
        list
    }

    /// El menú de acciones de Formal y Glass (o, con `sub`, uno de sus submenús): lo
    /// mismo que el de Expressive (`agent_menu`) con el aspecto de su estilo.
    pub(super) fn agent_menu_flat(&self, sub: Option<Sub>, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scroll = |id: &'static str, list: Div| div().id(id).max_h(px(MENU_MAX_H)).overflow_y_scroll().child(list);
        if let Some(sub) = sub {
            let back = div()
                .id("sub-back")
                .px(px(10.))
                .py(px(7.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(t.r_ctl.min(12.)))
                .cursor_pointer()
                .hover(|el| el.bg(t.hover))
                .on_click(Self::back_handler(cx))
                .child(
                    svg()
                        .path("icons/chevron-right.svg")
                        .size(px(14.))
                        .text_color(t.muted)
                        .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5))),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child(sub.title()));
            let entries = self.sub_entries(sub, cx);
            return card().child(back).child(divider()).child(scroll("agent-sub-scroll", self.flat_entries(entries, cx))).into_any_element();
        }
        let query = self.menu_filter.read(cx).text().trim().to_lowercase();
        let filtering = !query.is_empty();
        let mut tabs = div().flex().gap(px(2.)).pb(px(4.));
        for (index, (icon, label)) in TABS.iter().enumerate() {
            let on = self.menu_tab == index;
            tabs = tabs.child(
                div()
                    .id(("menu-tab", index))
                    .flex_1()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(t.r_ctl.min(10.)))
                    .cursor_pointer()
                    .when(on, |el| el.bg(t.sel))
                    .when(!on, |el| el.hover(|el| el.bg(t.hover)))
                    .tooltip(crate::hover::tip(label))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.menu_tab = index;
                        cx.notify();
                    }))
                    .child(svg().path(flat_icon(icon)).size(px(16.)).text_color(if on { t.accent } else { t.muted })),
            );
        }
        let entries = self.menu_entries(&query, self.menu_tab, cx);
        card()
            .child(div().px(px(4.)).pt(px(2.)).pb(px(6.)).child(self.menu_filter.clone()))
            .when(!filtering, |el| el.child(tabs).child(heading(TABS[self.menu_tab].1)))
            .child(scroll("agent-menu-scroll", self.flat_entries(entries, cx)))
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
