//! El menú de acciones de Expressive, copia del de la referencia: un filtro, seis
//! pestañas (Contexto, Modelo, Personalizar, Integraciones, Sesión, Cuenta) y
//! submenús con «atrás» (modelo, permisos, estilo de salida, servidores MCP,
//! subagentes, comandos). El botón de modelo abre directo el submenú «Modelo».

use std::path::PathBuf;

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, SharedString, Window};
use gpui_m3::{IconTab, IconTabs, MenuItem, StopSlider, Switch};
use serde_json::{json, Value};

use super::chat::Item;
use super::rewind::Rewind;
use super::style::t;
use super::{config, CodeView, Menu};

const MENU_W: f32 = 340.;
const MENU_H: f32 = 440.;
const TABS: [(&str, &str); 6] =
    [("clip", "Contexto"), ("cpu", "Modelo"), ("palette", "Personalizar"), ("plug", "Integraciones"), ("history", "Sesión"), ("shield", "Cuenta")];

/// Un submenú: se abre en la misma superficie, con «atrás».
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sub {
    Model,
    Permission,
    Output,
    Mcp,
    Agents,
    Commands,
    /// Volver a un mensaje: el código, la conversación o ambos.
    Rewind,
}

impl Sub {
    fn title(self) -> &'static str {
        match self {
            Sub::Model => "Modelo",
            Sub::Permission => "Permisos",
            Sub::Output => "Estilo de salida",
            Sub::Mcp => "Servidores MCP",
            Sub::Agents => "Subagentes",
            Sub::Commands => "Comandos y skills",
            Sub::Rewind => "Rewind",
        }
    }
}

/// Lo que hace una fila del menú.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Attach,
    Mention,
    Clear,
    Bookmark,
    NextFlag,
    Export,
    CopyRemote,
    Open(Sub),
    Ultracode,
    Thinking,
    SwitchOnFlag,
    FastMode,
    Hooks,
    Memory,
    Sandbox,
    StatusLine,
    Personal,
    General,
    Chrome,
    RemoteTerminal,
    Design,
    Compact,
    Usage,
    Resume,
    Terminal,
    Plugins,
    ReloadPlugins,
    Login,
    Logout,
}

enum Trailing {
    None,
    Value(String),
    Kbd(&'static str),
    Switch(bool),
    Effort,
}

struct Row {
    icon: &'static str,
    label: &'static str,
    trailing: Trailing,
    act: Option<Act>,
}

fn row(icon: &'static str, label: &'static str, trailing: Trailing, act: Act) -> Row {
    Row { icon, label, trailing, act: Some(act) }
}

/// Los permisos con sus nombres largos, como en el submenú de la referencia.
const PERMISSIONS: [(&str, &str); 4] = [
    ("default", "Preguntar antes de editar"),
    ("acceptEdits", "Editar automáticamente"),
    ("plan", "Modo plan"),
    ("bypassPermissions", "Omitir permisos"),
];

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

fn home_claude(file: &str) -> Option<PathBuf> {
    home().map(|home| home.join(".claude").join(file))
}

impl CodeView {
    fn rows(&self, tab: usize) -> Vec<Row> {
        let config = self.config();
        let output = if config.output_style.is_empty() || config.output_style == "default" {
            "Predeterminado".to_string()
        } else {
            config.output_style.clone()
        };
        let chat_model = self.chat_model();
        let model = if chat_model.is_empty() { "Predeterminado".to_string() } else { self.model_label(&chat_model) };
        let session = self.active_chat().and_then(|c| c.session_id.clone());
        let bookmark = if session.is_some_and(|s| self.is_bookmarked(&s)) { "Quitar" } else { "Agregar" };
        let flags = self.flag_count();
        match tab {
            0 => vec![
                row("clip", "Adjuntar archivo…", Trailing::Kbd("Ctrl+U"), Act::Attach),
                row("at", "Mencionar archivo del proyecto…", Trailing::Kbd("@"), Act::Mention),
                row("trash", "Limpiar conversación", Trailing::None, Act::Clear),
                row("history", "Rewind", Trailing::Kbd("Esc Esc"), Act::Open(Sub::Rewind)),
                row("bookmark", "Marcador", Trailing::Value(bookmark.into()), Act::Bookmark),
                row(
                    "bookmark",
                    "Siguiente mensaje marcado",
                    if flags > 0 { Trailing::Value(flags.to_string()) } else { Trailing::None },
                    Act::NextFlag,
                ),
                row("export", "Exportar conversación", Trailing::None, Act::Export),
                row("copy", "Copiar enlace de Remote Control", Trailing::None, Act::CopyRemote),
            ],
            1 => vec![
                row("cpu", "Cambiar modelo…", Trailing::Value(model), Act::Open(Sub::Model)),
                Row { icon: "gauge", label: "Esfuerzo", trailing: Trailing::Effort, act: None },
                row("spark", "Ultracode", Trailing::Switch(config.ultracode), Act::Ultracode),
                row("brain", "Thinking", Trailing::Switch(config.thinking), Act::Thinking),
                row("shield", "Cambiar de modelo al marcar un mensaje", Trailing::Switch(config.switch_model_on_flag), Act::SwitchOnFlag),
                row("bolt", "Modo rápido", Trailing::Switch(config.fast_mode), Act::FastMode),
                row("gauge", "Cuenta y uso…", Trailing::None, Act::Usage),
            ],
            2 => vec![
                row("palette", "Estilo de salida", Trailing::Value(output), Act::Open(Sub::Output)),
                row("shield", "Permisos", Trailing::Value(config.mode_label().into()), Act::Open(Sub::Permission)),
                row("plug", "Servidores MCP", Trailing::None, Act::Open(Sub::Mcp)),
                row("hook", "Hooks", Trailing::Value("settings.json".into()), Act::Hooks),
                row("layers", "Subagentes", Trailing::Value(self.agents.len().to_string()), Act::Open(Sub::Agents)),
                row("command", "Comandos y skills", Trailing::Value(self.commands.len().to_string()), Act::Open(Sub::Commands)),
                row("file", "Memoria · CLAUDE.md", Trailing::None, Act::Memory),
                row("box", "Sandbox", Trailing::Switch(config.sandbox), Act::Sandbox),
                row("monitor", "Línea de estado", Trailing::Switch(self.configs.status_line), Act::StatusLine),
                row("file", "Instrucciones personales", Trailing::Value("~/.claude/CLAUDE.md".into()), Act::Personal),
                row("gear", "Configuración general…", Trailing::Value("settings.json".into()), Act::General),
            ],
            3 => vec![
                row("monitor", "Claude in Chrome", Trailing::Switch(config.chrome), Act::Chrome),
                row("plug", "Remote Control", Trailing::None, Act::RemoteTerminal),
                row("palette", "Claude Design", Trailing::None, Act::Design),
            ],
            4 => vec![
                row("history", "Reanudar conversación…", Trailing::None, Act::Resume),
                row("layers", "Compactar contexto", Trailing::None, Act::Compact),
                row("terminal", "Abrir Claude en la terminal", Trailing::None, Act::Terminal),
                row("blocks", "Administrar plugins", Trailing::None, Act::Plugins),
                row("refresh", "Recargar plugins", Trailing::None, Act::ReloadPlugins),
            ],
            _ => vec![row("shield", "Cambiar de cuenta", Trailing::None, Act::Login), row("x", "Cerrar sesión", Trailing::None, Act::Logout)],
        }
    }

    fn effort_slider(&self, cx: &mut Context<Self>) -> StopSlider {
        let effort = self.chat_effort();
        let stop = config::EFFORTS.iter().position(|(id, _)| *id == effort);
        let set = cx.listener(|view, pick: &usize, _, cx| {
            let effort = config::EFFORTS[*pick].0.to_string();
            view.set_config(|c| c.effort = effort, cx);
        });
        StopSlider::new("effort-slider", config::EFFORTS.len(), stop).compact(true).on_change(move |pick, window, cx| set(&pick, window, cx))
    }

    fn render_row(&self, id: usize, row: Row, cx: &mut Context<Self>) -> MenuItem {
        let muted = t().muted;
        let mut item = MenuItem::new(("act", id), row.label).dense(true).icon(row.icon);
        item = match row.trailing {
            Trailing::None => item,
            Trailing::Value(value) => item.trailing(div().text_size(px(11.5)).text_color(muted).child(value)),
            Trailing::Kbd(kbd) => item.shortcut(kbd),
            Trailing::Switch(on) => item.trailing(Switch::new(("act-switch", id), on).compact(true)),
            Trailing::Effort => {
                let label = config::effort_label(&self.chat_effort());
                item.static_row(true).inline_sublabel(true).sublabel(label).trailing(self.effort_slider(cx))
            }
        };
        match row.act {
            Some(act) => item.on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.menu_act(act, window, cx))),
            None => item,
        }
    }

    /// El menú de acciones (o, con `sub`, uno de sus submenús).
    pub(super) fn agent_menu(&self, sub: Option<Sub>, cx: &mut Context<Self>) -> AnyElement {
        if let Some(sub) = sub {
            return self.sub_menu(sub, cx);
        }
        let query = self.menu_filter.read(cx).text().trim().to_lowercase();
        let filtering = !query.is_empty();
        let set_tab = cx.listener(|view, tab: &usize, _, cx| {
            view.menu_tab = *tab;
            cx.notify();
        });
        let tabs = TABS.iter().map(|(icon, label)| IconTab::new(*icon, *label)).collect::<Vec<_>>();
        let header = div()
            .flex()
            .flex_col()
            .child(self.menu_filter.clone())
            .when(!filtering, |el| {
                el.child(div().pt(px(8.)).pb(px(6.)).child(IconTabs::new("menu-tabs", tabs, self.menu_tab).on_change(move |tab, window, cx| set_tab(&tab, window, cx))))
            });
        let mut menu = gpui_m3::Menu::new("agent-menu")
            .width(MENU_W)
            .max_h(px(MENU_H))
            .header(header)
            .content_key(if filtering { 99 } else { self.menu_tab as u64 });
        if filtering {
            let mut found = false;
            let commands: Vec<(usize, String, String)> = self
                .command_names()
                .into_iter()
                .enumerate()
                .filter(|(_, (name, _))| name.to_lowercase().contains(query.trim_start_matches('/')))
                .take(8)
                .map(|(index, (name, description))| (index, name, description))
                .collect();
            if !commands.is_empty() {
                found = true;
                menu = menu.section("Comandos");
                for (index, name, description) in commands {
                    let insert = name.clone();
                    menu = menu.item(
                        MenuItem::new(("filter-command", index), format!("/{name}"))
                            .dense(true)
                            .icon("command")
                            .inline_sublabel(true)
                            .sublabel(description)
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.insert_command(&insert, window, cx))),
                    );
                }
            }
            for (tab, (_, section)) in TABS.iter().enumerate() {
                let rows: Vec<Row> = self
                    .rows(tab)
                    .into_iter()
                    .filter(|row| row.label.to_lowercase().contains(&query) || section.to_lowercase().contains(&query))
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                found = true;
                menu = menu.section(*section);
                for (index, row) in rows.into_iter().enumerate() {
                    menu = menu.item(self.render_row(tab * 100 + index, row, cx));
                }
            }
            if !found {
                menu = menu.item(div().p(px(16.)).text_center().text_color(t().muted).child("Sin resultados"));
            }
        } else {
            for (index, row) in self.rows(self.menu_tab).into_iter().enumerate() {
                menu = menu.item(self.render_row(self.menu_tab * 100 + index, row, cx));
            }
        }
        menu.into_any_element()
    }

    fn sub_menu(&self, sub: Sub, cx: &mut Context<Self>) -> AnyElement {
        let back = cx.listener(|view, _: &ClickEvent, _, cx| {
            view.menu_sub = None;
            if let Some((menu, at)) = view.menu {
                if menu == Menu::Model {
                    view.menu = Some((Menu::Actions, at));
                }
            }
            cx.notify();
        });
        let muted = t().muted;
        let config = self.config();
        let mut menu = gpui_m3::Menu::new("agent-sub").width(MENU_W).max_h(px(MENU_H)).back(sub.title(), back).content_key(100 + sub as u64);
        let empty = |text: &'static str| div().p(px(16.)).text_center().text_color(muted).child(text);
        match sub {
            Sub::Model => {
                let label = config::effort_label(&self.chat_effort());
                menu = menu
                    .item(MenuItem::new("sub-effort", "Esfuerzo").dense(true).icon("gauge").static_row(true).inline_sublabel(true).sublabel(label).trailing(self.effort_slider(cx)))
                    .separator();
                // El modelo de la conversación visible (puede venir como id de una
                // sesión retomada: se compara por nombre, como `isCurrent` de la referencia).
                let current = self.chat_model();
                let selected = self.current_model(&current);
                let (shown, more) = config::split_models(&self.models, selected.unwrap_or(&current));
                let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
                for index in visible {
                    let (id, name) = &self.models[index];
                    let on = selected == Some(id.as_str());
                    let id = id.clone();
                    let mut item = MenuItem::new(("model", index), name.clone()).dense(true).radio(on);
                    if let Some(description) = self.model_info.get(&id) {
                        item = item.sublabel(description.clone());
                    }
                    menu = menu.item(item.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let id = id.clone();
                        view.set_config(|c| c.model = id, cx);
                        view.close_menus(cx);
                    })));
                }
                if !more.is_empty() {
                    let (label, icon) =
                        if self.more_models { ("Menos modelos".to_string(), "chevron-down") } else { (format!("Más modelos ({})", more.len()), "chevron-right") };
                    menu = menu.item(MenuItem::new("model-more", label).dense(true).icon(icon).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.more_models = !view.more_models;
                        cx.notify();
                    })));
                }
            }
            Sub::Permission => {
                for (index, (id, label)) in PERMISSIONS.iter().enumerate() {
                    menu = menu.item(MenuItem::new(("permission", index), *label).dense(true).radio(config.permission_mode == *id).on_click(cx.listener(
                        move |view, _: &ClickEvent, _, cx| {
                            view.set_config(|c| c.permission_mode = id.to_string(), cx);
                            view.close_menus(cx);
                        },
                    )));
                }
            }
            Sub::Output => {
                let styles = if self.output_styles.is_empty() { vec!["default".to_string()] } else { self.output_styles.clone() };
                for (index, style) in styles.into_iter().enumerate() {
                    let label = if style == "default" { "Predeterminado".to_string() } else { style.clone() };
                    let on = config.output_style == style;
                    menu = menu.item(MenuItem::new(("output", index), label).dense(true).radio(on).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let style = style.clone();
                        view.set_config(|c| c.output_style = style, cx);
                        view.close_menus(cx);
                    })));
                }
            }
            Sub::Agents => {
                if self.agents.is_empty() {
                    menu = menu.item(empty("Nada por aquí todavía"));
                }
                for (index, (name, description)) in self.agents.iter().enumerate() {
                    let prompt = format!("Usa el subagente {name} para ");
                    menu = menu.item(MenuItem::new(("agent", index), name.clone()).dense(true).sublabel(description.clone()).on_click(cx.listener(
                        move |view, _: &ClickEvent, window, cx| {
                            view.close_menus(cx);
                            view.insert(&prompt, window, cx);
                        },
                    )));
                }
            }
            Sub::Commands => {
                for (index, (name, description)) in self.command_names().into_iter().enumerate() {
                    let insert = name.clone();
                    menu = menu.item(
                        MenuItem::new(("command", index), format!("/{name}"))
                            .dense(true)
                            .sublabel(description)
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.insert_command(&insert, window, cx))),
                    );
                }
            }
            Sub::Mcp => menu = self.mcp_rows(menu, cx),
            Sub::Rewind => {
                let turns = self.active_chat().map(|c| c.user_turns()).unwrap_or_default();
                if turns.is_empty() {
                    menu = menu.item(empty("Todavía no hay mensajes a los que volver"));
                }
                for (index, (uuid, text)) in turns.into_iter().enumerate() {
                    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
                    let mut label: String = line.chars().take(70).collect();
                    if line.chars().count() > 70 {
                        label.push('…');
                    }
                    let chip = |id: &'static str, name: &'static str, what: Rewind| {
                        let (uuid, text) = (uuid.clone(), text.clone());
                        gpui_m3::Chip::new((id, index), name).on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            view.rewind(uuid.clone(), text.clone(), what, window, cx)
                        }))
                    };
                    let actions = div()
                        .flex()
                        .gap(px(4.))
                        .child(chip("rewind-code", "Código", Rewind::Code))
                        .child(chip("rewind-chat", "Conversación", Rewind::Conversation))
                        .child(chip("rewind-both", "Ambos", Rewind::Both));
                    // TODO(gpui-m3): MenuItem con acciones bajo el texto (una fila
                    // estática de dos líneas); por ahora, la fila de aquí.
                    menu = menu.item(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().truncate().text_size(px(13.)).child(label))
                            .child(actions),
                    );
                }
            }
        }
        menu.into_any_element()
    }

    fn mcp_rows(&self, mut menu: gpui_m3::Menu, cx: &mut Context<Self>) -> gpui_m3::Menu {
        let scheme = *gpui_m3::Theme::of(cx);
        let muted = t().muted;
        let Some(servers) = &self.mcp else {
            return menu.item(div().p(px(16.)).text_center().text_color(muted).child("Cargando…"));
        };
        if servers.is_empty() {
            return menu.item(div().p(px(16.)).text_center().text_color(muted).child("No hay servidores MCP configurados"));
        }
        for (index, (name, status)) in servers.iter().enumerate() {
            let (color, hint) = match status.as_str() {
                "connected" => (scheme.success, "Conectado"),
                "failed" => (scheme.warning, "Error al conectar"),
                "needs-auth" => (scheme.warning, "Requiere autenticación"),
                "pending" => (scheme.outline, "Conectando…"),
                "disabled" => (scheme.outline, "Desactivado"),
                _ => (scheme.outline, ""),
            };
            let disabled = status == "disabled";
            let reconnect = matches!(status.as_str(), "failed" | "needs-auth" | "pending");
            let (toggle_name, reconnect_name) = (name.clone(), name.clone());
            let actions = div()
                .flex()
                .gap(px(4.))
                .when(reconnect, |el| {
                    el.child(gpui_m3::Chip::new(("mcp-reconnect", index), "Reconectar").on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.mcp_call("mcpReconnect", json!({ "name": reconnect_name }), cx)
                    })))
                })
                .child(gpui_m3::Chip::new(("mcp-toggle", index), if disabled { "Activar" } else { "Desactivar" }).on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpToggle", json!({ "name": toggle_name, "enabled": disabled }), cx),
                )));
            menu = menu.item(
                MenuItem::new(("mcp", index), name.clone())
                    .dense(true)
                    .static_row(true)
                    .leading(div().size(px(8.)).rounded_full().bg(color))
                    .sublabel(hint)
                    .trailing(actions),
            );
        }
        menu
    }

    /// Una acción sobre un servidor MCP; después se vuelve a pedir la lista.
    fn mcp_call(&mut self, method: &str, mut params: Value, cx: &mut Context<Self>) {
        let Some(key) = self.live_key() else {
            return;
        };
        params["key"] = json!(key);
        self.request(method, params, cx, |view, reply, cx| {
            if let Err(error) = reply {
                view.show_toast(error, cx);
            }
            view.load_mcp(cx);
        });
    }

    fn load_mcp(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.live_key() else {
            return;
        };
        self.request("mcpStatus", json!({ "key": key }), cx, |view, reply, cx| match reply {
            Ok(list) => {
                view.mcp = Some(
                    list.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|server| {
                            let name = server.get("name").and_then(Value::as_str)?;
                            let status = server.get("status").and_then(Value::as_str).unwrap_or_default();
                            Some((name.to_string(), status.to_string()))
                        })
                        .collect(),
                );
            }
            Err(error) => {
                view.mcp = Some(Vec::new());
                view.show_toast(error, cx);
            }
        });
    }

    /// La conversación activa, si su sesión está abierta en el sidecar.
    fn live_key(&self) -> Option<String> {
        self.active_chat().filter(|c| c.live).map(|c| c.key.clone())
    }

    pub(super) fn close_menus(&mut self, cx: &mut Context<Self>) {
        // `menu_sub` se queda: el menú sale con lo que mostraba. Al abrirse se reinicia.
        self.menu = None;
        cx.notify();
    }

    fn menu_act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let stays_open = matches!(
            act,
            Act::Open(_) | Act::Ultracode | Act::Thinking | Act::SwitchOnFlag | Act::FastMode | Act::Sandbox | Act::StatusLine | Act::Chrome
        );
        if !stays_open {
            self.close_menus(cx);
        }
        let cwd = self.active_chat().map(|c| c.cwd.clone()).or_else(|| self.workspaces.active().and_then(|w| w.main().cloned()));
        let session = self.active_chat().and_then(|c| c.session_id.clone());
        match act {
            Act::Attach => self.pick_attachments(cx),
            Act::Mention => self.start_mention(window, cx),
            Act::Clear => {
                if let Some(workspace) = self.active_workspace() {
                    self.new_chat(workspace, window, cx);
                }
            }
            Act::Bookmark => match self.active_chat().and_then(|c| c.session_id.clone()) {
                Some(session) => self.toggle_bookmark(&session, cx),
                None => self.show_toast("Envía un mensaje primero", cx),
            },
            Act::NextFlag => {
                if !self.next_flagged(cx) {
                    self.show_toast("Marca un mensaje con el botón Marcar, bajo cada mensaje", cx);
                }
            }
            Act::Export => self.export_chat(cx),
            Act::CopyRemote => match self.active_chat().and_then(|c| c.remote_url.clone()) {
                Some(url) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                    self.show_toast("Enlace copiado", cx);
                }
                None => self.show_toast("Activa Remote Control para obtener el enlace", cx),
            },
            Act::Open(Sub::Mcp) => {
                if self.live_key().is_none() {
                    self.show_toast("Empieza una conversación primero", cx);
                    return;
                }
                self.mcp = None;
                self.menu_sub = Some(Sub::Mcp);
                self.load_mcp(cx);
            }
            Act::Open(sub) => self.menu_sub = Some(sub),
            Act::Ultracode => self.set_config(|c| c.ultracode = !c.ultracode, cx),
            Act::Thinking => self.set_config(|c| c.thinking = !c.thinking, cx),
            Act::SwitchOnFlag => self.set_config(|c| c.switch_model_on_flag = !c.switch_model_on_flag, cx),
            Act::FastMode => self.set_config(|c| c.fast_mode = !c.fast_mode, cx),
            Act::Sandbox => self.set_config(|c| c.sandbox = !c.sandbox, cx),
            Act::StatusLine => {
                self.configs.status_line = !self.configs.status_line;
                self.configs.save();
            }
            Act::Chrome => {
                let on = !self.config().chrome;
                self.set_config(|c| c.chrome = on, cx);
                let verb = if on { "activará" } else { "desactivará" };
                self.show_toast(format!("Claude in Chrome se {verb} en la próxima conversación"), cx);
            }
            Act::Hooks | Act::General => self.open_text_file(home_claude("settings.json"), "{\n}\n", cx),
            Act::Personal => self.open_text_file(home_claude("CLAUDE.md"), "# Instrucciones personales para Claude\n\n", cx),
            Act::Memory => self.open_text_file(cwd.map(|dir| dir.join("CLAUDE.md")), "# Instrucciones para Claude\n\n", cx),
            Act::RemoteTerminal => {
                let command = match &session {
                    Some(id) => format!("--resume {id} --remote-control"),
                    None => "--remote-control".into(),
                };
                self.open_terminal(&command, cwd, cx);
            }
            Act::Design => {
                if self.commands.iter().any(|(name, _)| name == "design") {
                    self.insert_command("design", window, cx);
                } else {
                    self.show_toast("Claude Design no está disponible en esta cuenta", cx);
                }
            }
            Act::Compact => {
                if self.live_key().is_none() {
                    self.show_toast("Empieza una conversación primero", cx);
                    return;
                }
                self.composer.update(cx, |area, cx| area.set_text("/compact", cx));
                self.send(&super::Send, window, cx);
            }
            Act::Usage => self.toggle_pop(super::usage::Pop::Usage, cx),
            Act::Resume => {
                self.history_page = true;
                self.load_history(cx);
            }
            Act::Terminal => {
                let command = session.map(|id| format!("--resume {id}")).unwrap_or_default();
                self.open_terminal(&command, cwd, cx);
            }
            Act::Plugins => self.open_terminal("plugin list", cwd, cx),
            Act::ReloadPlugins => match self.live_key() {
                Some(key) => self.request("reloadPlugins", json!({ "key": key }), cx, |view, reply, cx| match reply {
                    Ok(_) => view.show_toast("Plugins recargados", cx),
                    Err(error) => view.show_toast(error, cx),
                }),
                None => self.show_toast("Empieza una conversación primero", cx),
            },
            Act::Login => self.open_terminal("auth login", cwd, cx),
            Act::Logout => self.open_terminal("auth logout", cwd, cx),
        }
        cx.notify();
    }

    /// Abre un archivo de texto con su programa; si no existe, lo crea con `initial`.
    fn open_text_file(&mut self, path: Option<PathBuf>, initial: &str, cx: &mut Context<Self>) {
        let Some(path) = path else {
            self.show_toast("Abre un proyecto primero", cx);
            return;
        };
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(error) = std::fs::write(&path, initial) {
                self.show_toast(format!("No se pudo crear {}: {error}", path.display()), cx);
                return;
            }
        }
        if let Err(error) = crate::launcher::shell_open(&path.to_string_lossy()) {
            self.show_toast(format!("No se pudo abrir {}: {error}", path.display()), cx);
        }
    }

    /// Corre `claude {args}` en una terminal aparte, en la carpeta del proyecto.
    fn open_terminal(&mut self, args: &str, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        let claude = self.claude_path.as_ref().map(|p| format!("\"{}\"", p.display())).unwrap_or_else(|| "claude".into());
        let command = format!("{claude} {args}").trim().to_string();
        let dir = cwd.or_else(home).unwrap_or_else(|| PathBuf::from("."));
        let terminal = std::process::Command::new("wt.exe").arg("-d").arg(&dir).args(["cmd", "/k", &command]).spawn();
        if terminal.is_err() {
            let fallback = std::process::Command::new("cmd").current_dir(&dir).args(["/c", "start", "", "cmd", "/k", &command]).spawn();
            if let Err(error) = fallback {
                self.show_toast(format!("No se pudo abrir la terminal: {error}"), cx);
            }
        }
    }

    /// Guarda la conversación activa como Markdown.
    fn export_chat(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat().filter(|c| !c.items.is_empty()) else {
            self.show_toast("No hay nada que exportar todavía", cx);
            return;
        };
        let mut text = format!("# {}\n", chat.title);
        for item in &chat.items {
            match item {
                Item::User { text: body, .. } => text.push_str(&format!("\n## Tú\n\n{body}\n")),
                Item::Text(body) => text.push_str(&format!("\n## Claude\n\n{body}\n")),
                _ => {}
            }
        }
        let dir = chat.cwd.clone();
        let name = format!("{}.md", chat.title.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-"));
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let result = std::fs::write(&path, text);
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.show_toast("Conversación exportada", cx),
                Err(error) => view.show_toast(format!("No se pudo exportar: {error}"), cx),
            });
        })
        .detach();
    }

    /// Un aviso abajo al centro, que se va solo a los 3,2 s (el toast de la referencia).
    pub(super) fn show_toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast_gen += 1;
        let generation = self.toast_gen;
        self.toast = Some(text.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(3200)).await;
            let _ = this.update(cx, |view, cx| {
                if view.toast_gen == generation {
                    view.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
