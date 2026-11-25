//! Los menús flotantes de la caja de texto, como los de la referencia: modelo y
//! esfuerzo, «/» (acciones y comandos de Claude Code) y el modo de permisos.
//!
//! Se dibujan encima de todo (`deferred`), anclados donde se hizo clic y
//! abriendo hacia arriba; un clic fuera o Esc los cierra.

use crate::i18n::tf;
use gpui::{
    anchored, deferred, div, point, prelude::*, px, svg, AnyElement, ClickEvent, Context, Corner, Div, FontWeight,
    MouseButton, SharedString, Stateful,
};
use serde_json::json;

use super::agent_menu::{Entry, Line, Sub, Trailing, TABS};
use super::config;
use super::style::{t, Style};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use super::{CodeView, Menu};

const MENU_W: f32 = 340.0;
const MENU_MAX_H: f32 = 460.0;
/// Desde cuánto espacio sobre la caja el menú abre hacia arriba (con la lista corta).
const MIN_UP: f32 = 300.0;
/// Lo que ocupan el buscador, las pestañas y los márgenes del menú, fuera de la lista.
const FIXED_H: f32 = 130.0;
/// La lista más corta que se deja: menos que esto ya no sirve para elegir.
const MIN_LIST_H: f32 = 150.0;
/// El margen interior de la tarjeta del menú.
const CARD_PAD: f32 = 6.0;

/// El radio de las filas y pestañas, concéntrico con la tarjeta: el de la tarjeta
/// menos su margen.
fn row_radius() -> f32 {
    (t().r_pop - CARD_PAD).max(4.)
}

fn card() -> Div {
    let t = t();
    div()
        .w(px(MENU_W))
        .max_w_full()
        .p(px(CARD_PAD))
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
    div().px(px(10.)).pt(px(8.)).pb(px(4.)).type_style(type_scale::LABEL_MEDIUM).text_color(t.muted).child(text)
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
        .py(px(8.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(row_radius()))
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
                    el.child(div().truncate().text_size(px(12.)).text_color(t.muted).child(hint))
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
        "slash" => "icons/eye-off.svg",
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
        // Los menús de la caja abren hacia arriba, con el buscador y las pestañas junto a la
        // caja; la lista se achica al espacio que haya (`lista_h`). Solo abren hacia abajo si
        // arriba no caben ni con la lista corta (`MIN_UP`) y abajo hay más. El del proyecto,
        // bajo su botón.
        let mut lista_h = MENU_MAX_H;
        let (position, corner, hacia_abajo) = match (menu, self.composer_bounds.get()) {
            (Menu::Model | Menu::Actions, Some(bounds)) => {
                let alto = f32::from(window.viewport_size().height);
                let arriba = f32::from(bounds.top());
                let abajo = alto - f32::from(bounds.bottom());
                let hacia_abajo = arriba < MIN_UP && abajo > arriba;
                // Lo que queda para la lista: el espacio menos márgenes, buscador y pestañas.
                let espacio = if hacia_abajo { abajo } else { arriba };
                lista_h = (espacio - FIXED_H).clamp(MIN_LIST_H, MENU_MAX_H);
                if hacia_abajo {
                    (point(bounds.left(), bounds.bottom() + px(8.)), Corner::TopLeft, true)
                } else {
                    (point(bounds.left(), bounds.top() - px(8.)), Corner::BottomLeft, false)
                }
            }
            (Menu::Project, _) => (point(at.x - px(20.), at.y + px(22.)), Corner::TopLeft, true),
            _ => (point(at.x - px(18.), at.y - px(22.)), Corner::BottomLeft, false),
        };
        let expressive = super::view::expressive();
        let content = match (menu, expressive) {
            // Formal y Glass tienen el mismo menú de acciones que Expressive, con sus
            // piezas (el Rewind, Esc Esc, es un submenú también aquí).
            (Menu::Model, false) => self.agent_menu_flat(Some(self.menu_sub.unwrap_or(Sub::Model)), hacia_abajo, lista_h, cx),
            (Menu::Actions, false) => self.agent_menu_flat(self.menu_sub, hacia_abajo, lista_h, cx),
            (Menu::Mode, false) => self.mode_menu(cx),
            (Menu::Model, true) => self.agent_menu(Some(self.menu_sub.unwrap_or(Sub::Model)), hacia_abajo, lista_h, cx),
            (Menu::Actions, true) => self.agent_menu(self.menu_sub, hacia_abajo, lista_h, cx),
            (Menu::Mode, true) => self.mode_menu_m3(cx),
            (Menu::Project, _) => self.project_menu_m3(cx),
        };
        // Formal y Glass entraban de golpe (y salían con fundido): entran desde su ancla, como
        // los de Expressive.
        let content = if expressive {
            content
        } else {
            super::enter::Enter::new("menu-flat-in").from(0., if hacia_abajo { -8. } else { 8. }).apply(div(), window, cx).child(content).into_any_element()
        };
        let leaving = shown.leaving;
        Some(
            deferred(
                div()
                    .absolute()
                    .occlude()
                    .inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                if let Some((menu, _)) = view.menu.take() {
                                    view.note_dismiss(super::Dismissed::Menu(menu));
                                }
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
                                    // Se va encogiendo hacia la esquina por la que entró.
                                    .child(shown.shrink(corner, content)),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Los comandos de Claude Code para el menú «/».
    pub(super) fn command_names(&self) -> Vec<(String, String)> {
        // Un agente ACP tiene los suyos, más los que Atic hace por él.
        if let Some(cli) = self.current_agent() {
            let own = self.acp_cfg().map(|cfg| cfg.commands.clone()).unwrap_or_default();
            return acp_commands(cli, own);
        }
        let mut commands: Vec<(String, String)> = if self.commands.is_empty() {
            // Antes de la primera sesión el SDK todavía no dijo cuáles hay.
            [("compact", crate::i18n::t("pill.code.menus.cmdCompact")), ("context", crate::i18n::t("pill.code.menus.cmdContext")), ("cost", crate::i18n::t("pill.code.menus.cmdCost")), ("review", crate::i18n::t("pill.code.menus.cmdReview"))]
                .iter()
                .map(|(n, d)| (n.to_string(), d.to_string()))
                .collect()
        } else {
            self.commands.clone()
        };
        // Las skills instaladas, aunque la sesión no las haya listado todavía.
        let project = self.workspaces.active().and_then(|w| w.main().cloned());
        for (name, description) in super::agent_menu::local_skills(project.as_deref()) {
            if !commands.iter().any(|(n, _)| *n == name) {
                commands.push((name, description));
            }
        }
        commands
    }

    /// Una fila del menú de permisos: la elegida se tiñe y lleva un check, sin radio.
    fn mode_row(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, hint: Option<String>, on: bool) -> Stateful<Div> {
        let t = t();
        let row = item(id, label, hint, None);
        if on {
            row.bg(t.sel).child(svg().path("icons/check.svg").size(px(16.)).flex_none().text_color(t.accent))
        } else {
            row
        }
    }

    fn mode_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        // Un agente ACP: sus modos, con las piezas de este estilo.
        if self.current_agent().is_some() {
            let mut entries = Vec::new();
            self.acp_mode_entries(&mut entries, cx);
            let mut menu = card().child(heading(crate::i18n::t("pill.code.menus.mode")));
            for entry in entries {
                menu = match entry {
                    super::agent_menu::Entry::Line(line) => {
                        let row = Self::mode_row(SharedString::from(format!("{}-{}", line.id.0, line.id.1)), line.label, line.sublabel, line.chosen == Some(true));
                        match line.click {
                            Some(click) => menu.child(row.on_click(click)),
                            None => menu.child(row),
                        }
                    }
                    super::agent_menu::Entry::Empty(text) => menu.child(div().p(px(12.)).text_color(t.muted).child(text)),
                    _ => menu,
                };
            }
            return menu.into_any_element();
        }
        let config = self.config();
        let mut menu = card().child(heading(crate::i18n::t("pill.code.menus.permissions")));
        // De menos a más permisivo.
        for (id, _) in config::MODE_ORDER {
            let label = config::mode_name(id);
            let on = config.permission_mode == id;
            menu = menu.child(
                Self::mode_row(SharedString::from(format!("mode-{id}")), label, Some(config::mode_hint(id).to_string()), on).on_click(
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
        if t().style == Style::Glass {
            return self.effort_knob(cx);
        }
        let t = t();
        let levels = self.effort_levels();
        let now = self.effort_now();
        let level = levels.iter().position(|(id, _)| *id == now);
        let mut dots = div().flex().gap(px(4.)).flex_none();
        for (index, (id, label)) in levels.into_iter().enumerate() {
            let filled = level.is_some_and(|level| index <= level);
            let label = SharedString::from(label);
            dots = dots.child(
                div()
                    .id(("effort", index))
                    .w(px(22.))
                    .h(px(12.))
                    .rounded(px(if t.style == Style::Formal { 3. } else { 6. }))
                    .bg(if filled { t.accent } else { t.border })
                    .cursor_pointer()
                    .hover(|el| el.opacity(0.85))
                    .tooltip(crate::hover::tip_text(label))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let pick = if view.effort_now() == id { String::new() } else { id.clone() };
                        view.pick_effort(pick, cx);
                    })),
            );
        }
        dots
    }

    /// El esfuerzo en Glass, como `EffortControl` de la referencia: riel con relleno, una
    /// perilla blanca y una parada por nivel. Un clic en una parada elige ese nivel
    /// (en la actual, vuelve al predeterminado) y arrastrar con el botón apretado
    /// mueve la perilla de parada en parada. Sin nivel elegido no hay perilla.
    fn effort_knob(&self, cx: &mut Context<Self>) -> Div {
        const PITCH: f32 = 28.;
        let t = t();
        let levels = self.effort_levels();
        let now = self.effort_now();
        let level = levels.iter().position(|(id, _)| *id == now);
        let width = PITCH * levels.len().saturating_sub(1) as f32 + 20.;
        let mut knob = div()
            .relative()
            .w(px(width))
            .h(px(24.))
            .flex_none()
            .child(div().absolute().left(px(10.)).right(px(10.)).top(px(10.)).h(px(4.)).rounded(px(2.)).bg(t.border))
            .when_some(level, |el, level| {
                el.child(div().absolute().left(px(10.)).top(px(10.)).h(px(4.)).w(px(level as f32 * PITCH)).rounded(px(2.)).bg(t.accent)).child(
                    div().absolute().top(px(2.)).left(px(level as f32 * PITCH)).size(px(20.)).rounded_full().bg(gpui::white()).shadow_md(),
                )
            });
        for (index, (id, label)) in levels.into_iter().enumerate() {
            let label = SharedString::from(label);
            let drag_id = id.clone();
            knob = knob.child(
                div()
                    .id(("effort", index))
                    .absolute()
                    .top_0()
                    .left(px(index as f32 * PITCH))
                    .w(px(20.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .tooltip(crate::hover::tip_text(label))
                    .child(div().size(px(3.)).rounded_full().bg(t.faint))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| {
                            let pick = if view.effort_now() == id { String::new() } else { id.clone() };
                            view.pick_effort(pick, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(move |view, event: &gpui::MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(MouseButton::Left) && view.effort_now() != drag_id {
                            view.pick_effort(drag_id.clone(), cx);
                        }
                    })),
            );
        }
        knob
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
            // El nombre va entero (un comando o una skill) y lo que se recorta es la descripción.
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(label.flex_none().max_w(px(200.)))
                .when_some(subtitle, |el, text| el.child(div().flex_1().min_w(px(0.)).truncate().text_size(px(12.)).text_color(t.muted).child(text)))
        } else {
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(label)
                .when_some(subtitle, |el, text| el.child(div().truncate().text_size(px(12.)).text_color(t.muted).child(text)))
        };
        let trailing: Option<AnyElement> = match line.trailing {
            Trailing::None => None,
            Trailing::Value(value) => Some(div().flex_none().max_w(px(150.)).truncate().text_size(px(12.5)).text_color(t.muted).child(value).into_any_element()),
            // El atajo, como `kbd`: al final y siempre en el mismo lugar.
            Trailing::Kbd(kbd) => Some(
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .text_size(px(11.5))
                    .text_color(t.muted)
                    .child(kbd)
                    .into_any_element(),
            ),
            Trailing::Switch(on) => Some(switch(on).into_any_element()),
            Trailing::Effort => Some(self.effort_dots(cx).into_any_element()),
            // El interruptor de Expressive, aquí con el aspecto de siempre (los
            // dos botones chicos): mismo interruptor y mismo reintento.
            Trailing::Mcp { name, on, index, reconnect } => {
                let mut picks = Vec::new();
                if reconnect {
                    let retry = name.clone();
                    picks.push(super::agent_menu::Pick {
                        id: ("mcp-reconnect", index),
                        label: crate::i18n::t("pill.code.agentMenu.reconnect"),
                        click: Box::new(cx.listener(move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpReconnect", json!({ "name": retry }), cx))),
                    });
                }
                let toggle = name;
                picks.push(super::agent_menu::Pick {
                    id: ("mcp-toggle", index),
                    label: if on { crate::i18n::t("pill.code.agentMenu.disable") } else { crate::i18n::t("pill.code.agentMenu.enable") },
                    click: Box::new(cx.listener(move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpToggle", json!({ "name": toggle, "enabled": !on }), cx))),
                });
                Some(div().flex().flex_none().gap(px(4.)).children(picks.into_iter().map(Self::flat_pick)).into_any_element())
            }
        };
        div()
            .id(line.id)
            .when(line.picked, |el| el.bg(t.sel))
            // La fila destacada (el héroe del modelo): teñida como la elegida.
            .when(line.chosen == Some(true), |el| el.bg(t.sel))
            .px(px(10.))
            .py(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(row_radius()))
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
                Entry::Empty(text) => list.child(div().p(px(16.)).flex().justify_center().text_color(t.muted).child(text)),
                Entry::Line(line) => list.child(self.flat_line(line, cx)),
                Entry::Choice(picks) => list.child(div().px(px(10.)).py(px(4.)).flex().flex_wrap().gap(px(4.)).children(picks.into_iter().map(|(pick, _)| Self::flat_pick(pick)))),
                Entry::Block { title, subtitle, age, tags, picks } => list.child(
                    div()
                        .px(px(10.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(title))
                                .children(age.map(|age| div().flex_none().text_size(px(11.)).text_color(t.muted).child(age))),
                        )
                        .children(subtitle.map(|subtitle| div().truncate().text_size(px(12.)).text_color(t.muted).child(subtitle)))
                        .when(!tags.is_empty(), |el| {
                            el.child(
                                div().flex().flex_wrap().gap(px(4.)).children(tags.into_iter().map(|tag| {
                                    div()
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(t.border)
                                        .text_size(px(11.))
                                        .text_color(t.muted)
                                        .child(tag.label)
                                })),
                            )
                        })
                        .child(div().flex().gap(px(4.)).children(picks.into_iter().map(Self::flat_pick))),
                ),
            };
        }
        list
    }

    /// El menú de acciones de Formal y Glass (o, con `sub`, uno de sus submenús): lo
    /// mismo que el de Expressive (`agent_menu`) con el aspecto de su estilo.
    pub(super) fn agent_menu_flat(&self, sub: Option<Sub>, below: bool, list_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let scroll = |id: &'static str, list: Div| div().id(id).max_h(px(list_h)).overflow_y_scroll().child(list);
        if let Some(sub) = sub {
            let back = div()
                .id("sub-back")
                .px(px(10.))
                .py(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(row_radius()))
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
            let search = (sub == Sub::Model && self.model_search_shown()).then(|| div().px(px(4.)).pt(px(6.)).pb(px(2.)).child(self.model_filter.clone()));
            let list = scroll("agent-sub-scroll", self.flat_entries(entries, cx));
            if below {
                return card().child(back).child(divider()).children(search).child(list).into_any_element();
            }
            // Abriendo hacia arriba, el «atrás» y el filtro van abajo, junto al
            // cuadro de texto: la lista crece por arriba y ellos no se mueven.
            return card().child(list).child(back).child(divider()).children(search).into_any_element();
        }
        let query = self.menu_filter.read(cx).text().trim().to_lowercase();
        let filtering = !query.is_empty();
        // El fondo de la pestaña elegida es una píldora que se desliza.
        let glide = crate::glide::named("code-menu-tabs");
        let look = crate::glide::Look { fill: t.sel, radius: row_radius(), edge: None };
        let mut tabs = div().relative().flex().gap(px(2.)).pb(px(4.)).child(crate::glide::indicator(glide.clone(), Some(self.menu_tab as u64), look));
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
                    .rounded(px(row_radius()))
                    .cursor_pointer()
                    .when(!on, |el| el.hover(|el| el.bg(t.hover)))
                    .child(crate::glide::mark(glide.clone(), index as u64))
                    .tooltip(crate::hover::tip(crate::i18n::t(*label)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.menu_tab = index;
                        cx.notify();
                    }))
                    .child(svg().path(flat_icon(icon)).size(px(16.)).text_color(if on { t.accent } else { t.muted })),
            );
        }
        let entries = self.menu_entries(&query, self.menu_tab, cx);
        let field = div().px(px(4.)).on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| view.menu_filter_key(event, cx))).child(self.menu_filter.clone());
        let list = div().flex().flex_col().when(!filtering, |el| el.child(heading(crate::i18n::t(TABS[self.menu_tab].1)))).child(scroll("agent-menu-scroll", self.flat_entries(entries, cx)));
        if below {
            // Las pestañas quedan fijas también al filtrar; solo la lista se mueve.
            return card().child(field.pt(px(2.)).pb(px(6.))).child(tabs).child(list).into_any_element();
        }
        // Abriendo hacia arriba, las pestañas y el buscador van abajo, junto al cuadro de texto:
        // el menú crece o se achica por arriba y ellos no se mueven al filtrar.
        card()
            .child(list)
            .child(div().pt(px(4.)).child(tabs.pb(px(0.))))
            .child(field.pt(px(6.)).pb(px(2.)))
            .into_any_element()
    }

    // --- Expressive: los menús de gpui-m3 --------------------------------------------

    /// «Elegir proyecto» del inicio, en Expressive: el proyecto actual con su
    /// ruta y su rama, los demás con su ruta, el buscador fijo arriba y «Abrir
    /// carpeta…» en el pie. La rama y los cambios solo se enseñan del proyecto
    /// activo (son los `repos` ya cargados); de los demás no se saben sin
    /// preguntarle a git en cada cuadro.
    fn project_menu_m3(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.project_filter.read(cx).text().trim().to_string();
        let rows = self.project_rows(&query);
        let mut menu = gpui_m3::Menu::new("menu-project").width(320.).max_h(px(MENU_MAX_H)).origin(Corner::TopLeft).pin_to_anchor(true);
        // El buscador va fijo arriba: solo la lista se desplaza.
        let search = div()
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| view.project_filter_key(event, cx)))
            .child(self.project_filter.clone());
        menu = menu.header(search);
        if rows.is_empty() {
            // Sin proyectos, o sin los que coincidan con el filtro.
            if self.workspaces.list().is_empty() {
                menu = menu.item(
                    div()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .text_center()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::i18n::t("pill.code.project.empty")))
                        .child(div().text_size(px(12.5)).text_color(t().muted).child(crate::i18n::t("pill.code.project.emptyHint"))),
                );
            } else {
                menu = menu.item(
                    div()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .text_center()
                        .text_color(t().muted)
                        .child(tf("pill.code.project.noMatch", &[("q", &query)])),
                );
                menu = menu.item(
                    gpui_m3::MenuItem::new("project-clear", crate::i18n::t("pill.code.project.clearFilter")).icon("x").on_click(cx.listener(
                        |view, _: &ClickEvent, _, cx| {
                            view.project_filter.update(cx, |field, cx| field.set_text("", cx));
                        },
                    )),
                );
            }
        } else if query.trim().is_empty() {
            // Sin filtro: el actual aparte y los demás en su orden interno (el
            // último uso no se guarda; ver el informe).
            let active = self.workspaces.active_id().filter(|_| !self.in_loose_chat());
            if let Some(row) = rows.iter().find(|row| Some(row.0) == active) {
                menu = menu.section(crate::i18n::t("pill.code.project.current"));
                menu = menu.item(self.project_item(row, &query, false, cx));
            }
            let rest: Vec<_> = rows.iter().filter(|row| Some(row.0) != active).collect();
            if !rest.is_empty() {
                menu = menu.section(crate::i18n::t("pill.code.menus.recents"));
                for row in rest {
                    menu = menu.item(self.project_item(row, &query, false, cx));
                }
            }
            menu = menu.separator();
            menu = menu.item(self.project_loose_item(cx));
        } else {
            // Filtrando: los que coinciden, en el orden de la lista, con la fila
            // elegida con ↑↓ resaltada.
            menu = menu.section(crate::i18n::t("pill.code.project.results"));
            let count = rows.len();
            for (index, row) in rows.iter().enumerate() {
                let picked = index == self.menu_pick % count;
                menu = menu.item(self.project_item(row, &query, picked, cx));
            }
            menu = menu.separator();
            menu = menu.item(self.project_loose_item(cx));
        }
        // «Abrir carpeta…» va fijo en el pie, no entre las filas.
        let open = gpui_m3::MenuItem::new("project-open", crate::i18n::t("pill.code.menus.openFolder")).icon("folder").on_click(cx.listener(
            |view, _: &ClickEvent, _, cx| {
                view.menu = None;
                view.pick_folders(None, cx);
            },
        ));
        menu.footer(open).into_any_element()
    }

    /// Las filas del menú de proyecto que coinciden con el filtro (nombre, ruta
    /// o rama), en el orden de la lista: id, nombre, ruta abreviada y, solo
    /// del activo, la rama con si tiene cambios.
    fn project_rows(&self, query: &str) -> Vec<(u64, String, String, Option<(String, bool)>)> {
        let needle = query.trim().to_lowercase();
        let in_project = !self.in_loose_chat();
        let branch = in_project.then(|| self.repos.iter().find_map(|r| r.branch.clone())).flatten();
        let dirty = in_project && self.repos.iter().any(|r| !r.files.is_empty());
        let active = self.workspaces.active_id().filter(|_| in_project);
        let mut rows = Vec::new();
        for space in self.workspaces.list() {
            let path = project_path(space);
            let own = (Some(space.id) == active).then(|| branch.clone()).flatten().map(|name| (name, dirty));
            if !needle.is_empty() {
                let hay = format!("{} {path} {}", space.name, own.as_ref().map(|(name, _)| name.as_str()).unwrap_or_default()).to_lowercase();
                if !needle.split_whitespace().all(|word| hay.contains(word)) {
                    continue;
                }
            }
            rows.push((space.id, space.name.clone(), path, own));
        }
        rows
    }

    /// Una fila de proyecto: inicial con su color, nombre, ruta y, si es la
    /// activa, la rama con su punto y el check. La activa va teñida.
    fn project_item(&self, row: &(u64, String, String, Option<(String, bool)>), query: &str, picked: bool, cx: &mut Context<Self>) -> gpui_m3::MenuItem {
        let (id, name, path, own) = row;
        let id = *id;
        let active = Some(id) == self.workspaces.active_id().filter(|_| !self.in_loose_chat());
        let mut item = gpui_m3::MenuItem::new(("project", id as usize), name.clone())
            .leading(gpui_m3::Avatar::new(name.clone()).size(px(28.)))
            .min_h(px(44.))
            .sublabel(path.clone())
            .highlighted(picked)
            .selected(active);
        if !query.trim().is_empty() {
            item = item.label_highlights(gpui_m3::match_ranges(name, query.trim()));
        }
        if active {
            item = item.trailing(self.project_branch(own, id, cx));
        }
        item.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
            if Some(id) == view.workspaces.active_id().filter(|_| !view.in_loose_chat()) {
                view.close_menus(cx);
            } else {
                view.menu = None;
                view.select_workspace(id, cx);
            }
        }))
    }

    /// La rama del proyecto activo al final de su fila, con su punto (verde
    /// limpia, ámbar con cambios) y el check de elegida.
    fn project_branch(&self, own: &Option<(String, bool)>, id: u64, cx: &mut Context<Self>) -> AnyElement {
        let scheme = *gpui_m3::Theme::of(cx);
        let (text, tip) = match own {
            Some((branch, dirty)) => {
                let tip = tf(
                    if *dirty { "pill.code.project.branchDirty" } else { "pill.code.project.branchClean" },
                    &[("branch", branch)],
                );
                (branch.clone(), tip)
            }
            None => (crate::i18n::t("pill.code.project.noGit").to_string(), crate::i18n::t("pill.code.project.noGit").to_string()),
        };
        div()
            .id(("project-branch", id as usize))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .max_w(px(150.))
            .tooltip(gpui_m3::Tooltip::text(tip))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .min_w(px(0.))
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(if own.as_ref().is_some_and(|(_, dirty)| *dirty) { t().warn } else { t().ok }))
                    .child(div().min_w(px(0.)).truncate().child(text)),
            )
            .child(gpui_m3::Icon::new("check").size(px(17.)).color(scheme.on_secondary_container))
            .into_any_element()
    }

    /// El chat sin proyecto, o volver al proyecto desde uno.
    fn project_loose_item(&self, cx: &mut Context<Self>) -> gpui_m3::MenuItem {
        match self.workspaces.active().filter(|_| self.in_loose_chat()) {
            Some(workspace) => gpui_m3::MenuItem::new("project-back", workspace.name.clone())
                .icon("folder")
                .sublabel(project_path(workspace))
                .selected(true)
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                    view.menu = None;
                    view.new_conversation(window, cx);
                })),
            None => gpui_m3::MenuItem::new("project-loose", crate::i18n::t("pill.code.menus.looseChat"))
                .icon("chat")
                .sublabel(crate::i18n::t("pill.code.project.noFolder"))
                .selected(self.in_loose_chat())
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                    view.menu = None;
                    view.new_loose_chat(window, cx);
                })),
        }
    }

    /// ↑/↓ (o Tab/Shift+Tab) en el buscador del menú de proyecto mueven la fila elegida.
    pub(super) fn project_filter_key(&mut self, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let query = self.project_filter.read(cx).text().trim().to_string();
        if query.is_empty() {
            return;
        }
        let shift = event.keystroke.modifiers.shift;
        let step: isize = match event.keystroke.key.as_str() {
            "down" => 1,
            "up" => -1,
            "tab" if shift => -1,
            "tab" => 1,
            _ => return,
        };
        cx.stop_propagation();
        let count = self.project_rows(&query).len();
        if count == 0 {
            return;
        }
        let current = self.menu_pick % count;
        self.menu_pick = (current as isize + step).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    /// Enter en el buscador: hace lo mismo que un clic en la fila elegida.
    pub(super) fn activate_project_pick(&mut self, cx: &mut Context<Self>) {
        let query = self.project_filter.read(cx).text().trim().to_string();
        if query.is_empty() {
            return;
        }
        let rows = self.project_rows(&query);
        if rows.is_empty() {
            return;
        }
        let id = rows[self.menu_pick % rows.len()].0;
        if Some(id) == self.workspaces.active_id().filter(|_| !self.in_loose_chat()) {
            self.close_menus(cx);
        } else {
            self.menu = None;
            self.select_workspace(id, cx);
        }
    }

    fn mode_menu_m3(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_m3::MenuItem;
        let scheme = *gpui_m3::Theme::of(cx);
        let risky = "bypassPermissions";
        let mut menu = gpui_m3::Menu::new("menu-mode").width(MENU_W).origin(Corner::BottomLeft).pin_to_anchor(true);
        if self.current_agent().is_some() {
            // Un agente ACP: sus modos, en el orden del agente.
            let mut entries = Vec::new();
            self.acp_mode_entries(&mut entries, cx);
            menu = menu.section(crate::i18n::t("pill.code.menus.mode"));
            for entry in entries {
                menu = match entry {
                    super::agent_menu::Entry::Line(line) => {
                        let mut row = MenuItem::new(SharedString::from(format!("{}-{}", line.id.0, line.id.1)), line.label)
                            .selected(line.chosen == Some(true));
                        if let Some(sub) = line.sublabel {
                            row = row.sublabel(sub);
                        }
                        if let Some(click) = line.click {
                            row = row.on_click(click);
                        }
                        menu.item(row)
                    }
                    super::agent_menu::Entry::Empty(text) => menu.section(text),
                    _ => menu,
                };
            }
        } else {
            menu = menu.section(crate::i18n::t("pill.code.perm.orderTitle"));
            // De menos a más permisivo: solo el último es peligroso y pide dos toques.
            for (id, risk) in config::MODE_ORDER {
                let on = self.config().permission_mode == id;
                let mut row = MenuItem::new(SharedString::from(format!("mode-{id}")), config::mode_name(id))
                    .sublabel(config::mode_hint(id))
                    .risk(risk)
                    .selected(on)
                    .danger(id == risky);
                if id == risky && !on {
                    row = row.confirm(crate::i18n::t("pill.code.perm.confirmBypass"));
                }
                menu = menu.item(row.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.set_config(|c| c.permission_mode = id.into(), cx);
                    view.menu = None;
                    cx.notify();
                })));
            }
        }
        // «Permitir todo» vale solo en esta conversación: si está activo, el menú lo dice
        // (elegir un modo lo termina, como ya hace `update_config`).
        if self.active.as_deref().is_some_and(super::permissions::allows_all) {
            let t = t();
            menu = menu.header(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .child(svg().path("icons/activity.svg").size(px(16.)).flex_none().text_color(scheme.warning))
                    .child(div().text_size(px(12.5)).text_color(t.muted).child(crate::i18n::t("pill.code.perm.allowAllOn"))),
            );
        }
        // El modo se guarda y sigue entre proyectos.
        menu = menu.footer(div().px(px(12.)).py(px(6.)).text_size(px(12.)).text_color(t().muted).child(crate::i18n::t("pill.code.perm.scopeNote")));
        menu.into_any_element()
    }
}

/// La ruta que enseña una fila del menú de proyecto: la carpeta principal con
/// `~` en vez de la carpeta del usuario y, si el espacio trae más carpetas,
/// cuántas más (`~/dev/tienda +2`).
fn project_path(space: &crate::kit::workspaces::Workspace) -> String {
    let Some(main) = space.main() else {
        return String::new();
    };
    let mut path = project_tilde(main);
    if space.folders.len() > 1 {
        path.push_str(&format!(" +{}", space.folders.len() - 1));
    }
    path
}

/// La ruta con `~` en vez de la carpeta del usuario.
fn project_tilde(path: &std::path::Path) -> String {
    match crate::paths::home().and_then(|home| path.strip_prefix(&home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// El menú «/» de un agente ACP: `/clear` (lo hace Atic con cualquiera), los de Codex que
/// Atic traduce (`codex app-server` no anuncia comandos) y los que anunció el agente.
fn acp_commands(cli: &str, own: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut list = vec![("clear".to_string(), crate::i18n::t("pill.code.menus.cmdClear").to_string())];
    if cli == "codex" {
        list.push(("compact".to_string(), crate::i18n::t("pill.code.menus.cmdCompact").to_string()));
        list.push(("review".to_string(), crate::i18n::t("pill.code.menus.cmdReview").to_string()));
    }
    for (name, description) in own {
        if !list.iter().any(|(n, _)| *n == name) {
            list.push((name, description));
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_comandos_de_un_agente_acp_llevan_los_de_atic() {
        let own = vec![("init".to_string(), "Crea AGENTS.md".to_string()), ("clear".to_string(), "Del agente".to_string())];
        let names = |list: Vec<(String, String)>| list.into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(names(acp_commands("opencode", own.clone())), ["clear", "init"]);
        assert_eq!(names(acp_commands("codex", Vec::new())), ["clear", "compact", "review"]);
    }
}
