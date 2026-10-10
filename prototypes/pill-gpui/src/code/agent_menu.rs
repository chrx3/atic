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
pub(super) const TABS: [(&str, &str); 6] =
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
    pub(super) fn title(self) -> &'static str {
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

pub(super) enum Trailing {
    None,
    Value(String),
    Kbd(&'static str),
    Switch(bool),
    Effort,
    /// Botones chicos al final de la fila (los del MCP).
    Chips(Vec<Pick>),
}

/// Lo que hace un clic en una fila del menú.
pub(super) type Handler = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App)>;

pub(super) fn boxed(f: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static) -> Handler {
    Box::new(f)
}

/// Un botón chico dentro de una fila (Reconectar, Código, Ambos…).
pub(super) struct Pick {
    pub id: (&'static str, usize),
    pub label: &'static str,
    pub click: Handler,
}

/// Una fila del menú de acciones o de un submenú, sin dibujar. Expressive la
/// pasa a `MenuItem` (`m3_item`); Formal y Glass, a su propia fila (`menus.rs`).
pub(super) struct Line {
    pub id: (&'static str, usize),
    pub label: String,
    /// El nombre del ícono de gpui-m3 (`clip`, `cpu`…).
    pub icon: Option<&'static str>,
    pub sublabel: Option<String>,
    pub radio: Option<bool>,
    pub trailing: Trailing,
    /// El texto de abajo va al lado del título.
    pub inline_sub: bool,
    /// Sin clic ni realce (esfuerzo, servidores MCP).
    pub static_row: bool,
    /// Un punto de color al comienzo.
    pub dot: Option<gpui::Hsla>,
    pub click: Option<Handler>,
}

impl Line {
    pub fn new(id: (&'static str, usize), label: impl Into<String>) -> Self {
        Self { id, label: label.into(), icon: None, sublabel: None, radio: None, trailing: Trailing::None, inline_sub: false, static_row: false, dot: None, click: None }
    }
}

/// Lo que lista un menú.
pub(super) enum Entry {
    Section(&'static str),
    Separator,
    Line(Line),
    /// Un texto centrado cuando no hay nada.
    Empty(&'static str),
    /// Un título con botones debajo (Rewind).
    Block { label: String, picks: Vec<Pick> },
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

/// La conversación como Markdown (`exportConversation` de la referencia): lo que dijo
/// cada uno y, entre medio, cada herramienta con el comienzo de su input.
pub(super) fn export_markdown(title: &str, items: &[Item]) -> String {
    let mut text = format!("# {title}\n");
    for item in items {
        match item {
            Item::User { text: body, .. } => text.push_str(&format!("\n## Tú\n\n{body}\n")),
            Item::Text(body) => text.push_str(&format!("\n## Claude\n\n{body}\n")),
            Item::Tool(tool) => {
                let input = tool.input.as_ref().map(|input| {
                    let json = input.to_string();
                    let short: String = json.chars().take(200).collect();
                    format!(" `{}`", short.replace('`', "'"))
                });
                text.push_str(&format!("\n> **{}**{}\n", tool.name, input.unwrap_or_default()));
            }
            _ => {}
        }
    }
    text
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

    /// Una fila del menú, sin dibujar: Expressive la pasa a `MenuItem` y Formal
    /// y Glass a su propia fila (`menus.rs`).
    fn line_of(&self, id: usize, row: Row, cx: &mut Context<Self>) -> Line {
        let mut line = Line::new(("act", id), row.label);
        line.icon = Some(row.icon);
        line.click = row.act.map(|act| boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.menu_act(act, window, cx))));
        match row.trailing {
            Trailing::Effort => {
                line.sublabel = Some(config::effort_label(&self.chat_effort()).to_string());
                line.static_row = true;
                line.inline_sub = true;
                line.trailing = Trailing::Effort;
            }
            other => line.trailing = other,
        }
        line
    }

    /// Lo que lista el menú de acciones: con filtro, los comandos y las filas que
    /// coinciden de todas las pestañas; sin filtro, las de la pestaña de ahora.
    pub(super) fn menu_entries(&self, query: &str, tab: usize, cx: &mut Context<Self>) -> Vec<Entry> {
        let mut out = Vec::new();
        if query.is_empty() {
            for (index, row) in self.rows(tab).into_iter().enumerate() {
                out.push(Entry::Line(self.line_of(tab * 100 + index, row, cx)));
            }
            return out;
        }
        let commands: Vec<(usize, String, String)> = self
            .command_names()
            .into_iter()
            .enumerate()
            .filter(|(_, (name, _))| name.to_lowercase().contains(query.trim_start_matches('/')))
            .take(8)
            .map(|(index, (name, description))| (index, name, description))
            .collect();
        if !commands.is_empty() {
            out.push(Entry::Section("Comandos"));
            for (index, name, description) in commands {
                let mut line = Line::new(("filter-command", index), format!("/{name}"));
                line.icon = Some("command");
                line.inline_sub = true;
                line.sublabel = Some(description);
                line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.insert_command(&name, window, cx))));
                out.push(Entry::Line(line));
            }
        }
        for (tab, (_, section)) in TABS.iter().enumerate() {
            let rows: Vec<Row> = self
                .rows(tab)
                .into_iter()
                .filter(|row| row.label.to_lowercase().contains(query) || section.to_lowercase().contains(query))
                .collect();
            if rows.is_empty() {
                continue;
            }
            out.push(Entry::Section(section));
            for (index, row) in rows.into_iter().enumerate() {
                out.push(Entry::Line(self.line_of(tab * 100 + index, row, cx)));
            }
        }
        if out.is_empty() {
            out.push(Entry::Empty("Sin resultados"));
        }
        out
    }

    /// La fila de Expressive para una línea.
    fn m3_item(&self, line: Line, cx: &mut Context<Self>) -> MenuItem {
        let muted = t().muted;
        let mut item = MenuItem::new(line.id, line.label).dense(true);
        if let Some(icon) = line.icon {
            item = item.icon(icon);
        }
        if let Some(on) = line.radio {
            item = item.radio(on);
        }
        if line.static_row {
            item = item.static_row(true);
        }
        if line.inline_sub {
            item = item.inline_sublabel(true);
        }
        if let Some(sublabel) = line.sublabel {
            item = item.sublabel(sublabel);
        }
        if let Some(color) = line.dot {
            item = item.leading(div().size(px(8.)).rounded_full().bg(color));
        }
        item = match line.trailing {
            Trailing::None => item,
            Trailing::Value(value) => item.trailing(div().text_size(px(11.5)).text_color(muted).child(value)),
            Trailing::Kbd(kbd) => item.shortcut(kbd),
            Trailing::Switch(on) => item.trailing(Switch::new(("act-switch", line.id.1), on).compact(true)),
            Trailing::Effort => item.trailing(self.effort_slider(cx)),
            Trailing::Chips(picks) => item.trailing(div().flex().gap(px(4.)).children(picks.into_iter().map(|pick| gpui_m3::Chip::new(pick.id, pick.label).on_click(pick.click)))),
        };
        match line.click {
            Some(click) => item.on_click(click),
            None => item,
        }
    }

    /// Una entrada más en un menú de gpui-m3.
    fn m3_add(&self, menu: gpui_m3::Menu, entry: Entry, cx: &mut Context<Self>) -> gpui_m3::Menu {
        match entry {
            Entry::Section(title) => menu.section(title),
            Entry::Separator => menu.separator(),
            Entry::Empty(text) => menu.item(div().p(px(16.)).text_center().text_color(t().muted).child(text)),
            Entry::Line(line) => menu.item(self.m3_item(line, cx)),
            // TODO(gpui-m3): MenuItem con acciones bajo el texto (una fila
            // estática de dos líneas); por ahora, la fila de aquí.
            Entry::Block { label, picks } => menu.item(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().truncate().text_size(px(13.)).child(label))
                    .child(div().flex().gap(px(4.)).children(picks.into_iter().map(|pick| gpui_m3::Chip::new(pick.id, pick.label).on_click(pick.click)))),
            ),
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
        for entry in self.menu_entries(&query, self.menu_tab, cx) {
            menu = self.m3_add(menu, entry, cx);
        }
        menu.into_any_element()
    }

    /// El botón «atrás» de un submenú: vuelve al menú de acciones.
    pub(super) fn back_handler(cx: &mut Context<Self>) -> Handler {
        boxed(cx.listener(|view, _: &ClickEvent, _, cx| {
            view.menu_sub = None;
            if let Some((menu, at)) = view.menu {
                if menu == Menu::Model {
                    view.menu = Some((Menu::Actions, at));
                }
            }
            cx.notify();
        }))
    }

    fn sub_menu(&self, sub: Sub, cx: &mut Context<Self>) -> AnyElement {
        let back = Self::back_handler(cx);
        let mut menu = gpui_m3::Menu::new("agent-sub").width(MENU_W).max_h(px(MENU_H)).back(sub.title(), back).content_key(100 + sub as u64);
        for entry in self.sub_entries(sub, cx) {
            menu = self.m3_add(menu, entry, cx);
        }
        menu.into_any_element()
    }

    /// Lo que lista un submenú, para los tres estilos.
    pub(super) fn sub_entries(&self, sub: Sub, cx: &mut Context<Self>) -> Vec<Entry> {
        let config = self.config();
        let mut out = Vec::new();
        match sub {
            Sub::Model => {
                let mut effort = Line::new(("sub-effort", 0), "Esfuerzo");
                effort.icon = Some("gauge");
                effort.static_row = true;
                effort.inline_sub = true;
                effort.sublabel = Some(config::effort_label(&self.chat_effort()).to_string());
                effort.trailing = Trailing::Effort;
                out.push(Entry::Line(effort));
                out.push(Entry::Separator);
                // El modelo de la conversación visible (puede venir como id de una
                // sesión retomada: se compara por nombre, como `isCurrent` de la referencia).
                let current = self.chat_model();
                let selected = self.current_model(&current);
                let (shown, more) = config::split_models(&self.models, selected.unwrap_or(&current));
                let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
                for index in visible {
                    let (id, name) = &self.models[index];
                    let mut line = Line::new(("model", index), name.clone());
                    line.radio = Some(selected == Some(id.as_str()));
                    line.sublabel = self.model_info.get(id).cloned();
                    let id = id.clone();
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let id = id.clone();
                        view.set_config(|c| c.model = id, cx);
                        view.close_menus(cx);
                    })));
                    out.push(Entry::Line(line));
                }
                if !more.is_empty() {
                    let (label, icon) =
                        if self.more_models { ("Menos modelos".to_string(), "chevron-down") } else { (format!("Más modelos ({})", more.len()), "chevron-right") };
                    let mut line = Line::new(("model-more", 0), label);
                    line.icon = Some(icon);
                    line.click = Some(boxed(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.more_models = !view.more_models;
                        cx.notify();
                    })));
                    out.push(Entry::Line(line));
                }
            }
            Sub::Permission => {
                for (index, (id, label)) in PERMISSIONS.iter().enumerate() {
                    let mut line = Line::new(("permission", index), *label);
                    line.radio = Some(config.permission_mode == *id);
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_config(|c| c.permission_mode = id.to_string(), cx);
                        view.close_menus(cx);
                    })));
                    out.push(Entry::Line(line));
                }
            }
            Sub::Output => {
                let styles = if self.output_styles.is_empty() { vec!["default".to_string()] } else { self.output_styles.clone() };
                for (index, style) in styles.into_iter().enumerate() {
                    let mut line = Line::new(("output", index), if style == "default" { "Predeterminado".to_string() } else { style.clone() });
                    line.radio = Some(config.output_style == style);
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let style = style.clone();
                        view.set_config(|c| c.output_style = style, cx);
                        view.close_menus(cx);
                    })));
                    out.push(Entry::Line(line));
                }
            }
            Sub::Agents => {
                if self.agents.is_empty() {
                    out.push(Entry::Empty("Nada por aquí todavía"));
                }
                for (index, (name, description)) in self.agents.iter().enumerate() {
                    let prompt = format!("Usa el subagente {name} para ");
                    let mut line = Line::new(("agent", index), name.clone());
                    line.sublabel = Some(description.clone());
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        view.close_menus(cx);
                        view.insert(&prompt, window, cx);
                    })));
                    out.push(Entry::Line(line));
                }
            }
            Sub::Commands => {
                for (index, (name, description)) in self.command_names().into_iter().enumerate() {
                    let mut line = Line::new(("command", index), format!("/{name}"));
                    line.sublabel = Some(description);
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.insert_command(&name, window, cx))));
                    out.push(Entry::Line(line));
                }
            }
            Sub::Mcp => self.mcp_entries(&mut out, cx),
            Sub::Rewind => {
                let turns = self.active_chat().map(|c| c.user_turns()).unwrap_or_default();
                if turns.is_empty() {
                    out.push(Entry::Empty("Todavía no hay mensajes a los que volver"));
                }
                for (index, (uuid, text)) in turns.into_iter().enumerate() {
                    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
                    let mut label: String = line.chars().take(70).collect();
                    if line.chars().count() > 70 {
                        label.push('…');
                    }
                    let pick = |id: &'static str, name: &'static str, what: Rewind| {
                        let (uuid, text) = (uuid.clone(), text.clone());
                        Pick {
                            id: (id, index),
                            label: name,
                            click: boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.rewind(uuid.clone(), text.clone(), what, window, cx))),
                        }
                    };
                    let picks = vec![pick("rewind-code", "Código", Rewind::Code), pick("rewind-chat", "Conversación", Rewind::Conversation), pick("rewind-both", "Ambos", Rewind::Both)];
                    out.push(Entry::Block { label, picks });
                }
            }
        }
        out
    }

    fn mcp_entries(&self, out: &mut Vec<Entry>, cx: &mut Context<Self>) {
        let scheme = *gpui_m3::Theme::of(cx);
        let tokens = t();
        let (ok, warn, off) = if tokens.style == super::style::Style::Expressive {
            (scheme.success, scheme.warning, scheme.outline)
        } else {
            (tokens.ok, tokens.warn, tokens.faint)
        };
        let Some(servers) = &self.mcp else {
            out.push(Entry::Empty("Cargando…"));
            return;
        };
        if servers.is_empty() {
            out.push(Entry::Empty("No hay servidores MCP configurados"));
            return;
        }
        for (index, (name, status)) in servers.iter().enumerate() {
            let (color, hint) = match status.as_str() {
                "connected" => (ok, "Conectado"),
                "failed" => (warn, "Error al conectar"),
                "needs-auth" => (warn, "Requiere autenticación"),
                "pending" => (off, "Conectando…"),
                "disabled" => (off, "Desactivado"),
                _ => (off, ""),
            };
            let disabled = status == "disabled";
            let reconnect = matches!(status.as_str(), "failed" | "needs-auth" | "pending");
            let (toggle_name, reconnect_name) = (name.clone(), name.clone());
            let mut picks = Vec::new();
            if reconnect {
                picks.push(Pick {
                    id: ("mcp-reconnect", index),
                    label: "Reconectar",
                    click: boxed(cx.listener(move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpReconnect", json!({ "name": reconnect_name }), cx))),
                });
            }
            picks.push(Pick {
                id: ("mcp-toggle", index),
                label: if disabled { "Activar" } else { "Desactivar" },
                click: boxed(cx.listener(move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpToggle", json!({ "name": toggle_name, "enabled": disabled }), cx))),
            });
            let mut line = Line::new(("mcp", index), name.clone());
            line.static_row = true;
            line.dot = Some(color);
            line.sublabel = Some(hint.to_string());
            line.trailing = Trailing::Chips(picks);
            out.push(Entry::Line(line));
        }
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
                // Chrome se fija al iniciar la sesión: una conversación vacía se reinicia ya;
                // las demás lo aplican desde la próxima (`store.ts:405` de la referencia).
                if self.restart_idle_chat(cx) {
                    self.show_toast(format!("Claude in Chrome {}", if on { "activado" } else { "desactivado" }), cx);
                } else {
                    let verb = if on { "activará" } else { "desactivará" };
                    self.show_toast(format!("Claude in Chrome se {verb} en la próxima conversación"), cx);
                }
            }
            Act::Hooks | Act::General => self.open_text_file(home_claude("settings.json"), "{\n}\n", cx),
            Act::Personal => self.open_text_file(home_claude("CLAUDE.md"), "# Instrucciones personales para Claude\n\n", cx),
            Act::Memory => self.open_text_file(cwd.map(|dir| dir.join("CLAUDE.md")), "# Instrucciones para Claude\n\n", cx),
            Act::RemoteTerminal => {
                let command = match &session {
                    Some(id) => format!("--resume {id} --remote-control"),
                    None => "--remote-control".into(),
                };
                self.open_terminal(&command, "remote", cwd, window, cx);
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
                self.open_terminal(&command, "claude", cwd, window, cx);
            }
            Act::Plugins => self.open_terminal("plugin list", "plugins", cwd, window, cx),
            Act::ReloadPlugins => match self.live_key() {
                Some(key) => self.request("reloadPlugins", json!({ "key": key }), cx, |view, reply, cx| match reply {
                    Ok(_) => view.show_toast("Plugins recargados", cx),
                    Err(error) => view.show_toast(error, cx),
                }),
                None => self.show_toast("Empieza una conversación primero", cx),
            },
            Act::Login => self.open_terminal("auth login", "cuenta", cwd, window, cx),
            Act::Logout => self.open_terminal("auth logout", "cuenta", cwd, window, cx),
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

    /// Corre `claude {args}` en una pestaña de la terminal integrada, en la
    /// carpeta del proyecto (`inTerminal` de la referencia). El shell es PowerShell:
    /// una ruta entre comillas se llama con `&`.
    fn open_terminal(&mut self, args: &str, name: &str, cwd: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let claude = self.claude_path.as_ref().map(|p| format!("& \"{}\"", p.display())).unwrap_or_else(|| "claude".into());
        let command = format!("{claude} {args}").trim().to_string();
        self.run_in_terminal(command, name, cwd.or_else(home), window, cx);
    }

    /// Guarda la conversación activa como Markdown.
    fn export_chat(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat().filter(|c| !c.items.is_empty()) else {
            self.show_toast("No hay nada que exportar todavía", cx);
            return;
        };
        let text = export_markdown(&chat.title, &chat.items);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exportar_incluye_las_herramientas() {
        let tool = |name: &str, input: Value| Item::Tool(super::super::chat::ToolCall::new("t".into(), name.into(), Some(input)));
        let items = vec![
            Item::user("Revisa el README"),
            tool("Read", json!({ "file_path": "README.md" })),
            tool("Bash", json!({ "command": "echo `hola`" })),
            Item::Thinking("no se exporta".into()),
            Item::Text("Listo.".into()),
        ];
        let md = export_markdown("Mi chat", &items);
        assert_eq!(
            md,
            "# Mi chat\n\n## Tú\n\nRevisa el README\n\n> **Read** `{\"file_path\":\"README.md\"}`\n\n> **Bash** `{\"command\":\"echo 'hola'\"}`\n\n## Claude\n\nListo.\n"
        );
        // El input se corta a 200 caracteres.
        let long = export_markdown("t", &[tool("Write", json!({ "content": "x".repeat(500) }))]);
        assert!(long.lines().nth(2).unwrap().chars().count() < 230);
    }
}
