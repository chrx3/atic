//! El menú de acciones de Expressive, copia del de la referencia: un filtro, seis
//! pestañas (Contexto, Modelo, Personalizar, Integraciones, Sesión, Cuenta) y
//! submenús con «atrás» (modelo, permisos, estilo de salida, servidores MCP,
//! subagentes, comandos). El botón de modelo abre directo el submenú «Modelo».

use crate::i18n::tf;
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
    [("clip", "pill.code.agentMenu.tabContext"), ("cpu", "pill.code.agentMenu.tabModel"), ("palette", "pill.code.agentMenu.tabCustomize"), ("plug", "pill.code.agentMenu.tabIntegrations"), ("history", "pill.code.agentMenu.tabSession"), ("shield", "pill.code.agentMenu.tabAccount")];

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
            Sub::Model => crate::i18n::t("pill.code.agentMenu.model"),
            Sub::Permission => crate::i18n::t("pill.code.agentMenu.permissions"),
            Sub::Output => crate::i18n::t("pill.code.agentMenu.outputStyle"),
            Sub::Mcp => crate::i18n::t("pill.code.agentMenu.mcpServers"),
            Sub::Agents => crate::i18n::t("pill.code.agentMenu.subagents"),
            Sub::Commands => crate::i18n::t("pill.code.agentMenu.commandsSkills"),
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

/// Un texto con vida `'static` para los títulos de sección, que la lista pide así. Cada
/// texto distinto se guarda una vez (son pocos: los proveedores y opciones de los
/// agentes), no en cada cuadro.
pub(super) fn interned(text: &str) -> &'static str {
    static TEXTS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> = std::sync::OnceLock::new();
    let mut texts = TEXTS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = texts.get(text) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
    texts.insert(leaked);
    leaked
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
    ("default", "pill.code.agentMenu.permAsk"),
    ("acceptEdits", "pill.code.agentMenu.permAuto"),
    ("plan", "pill.code.agentMenu.permPlan"),
    ("bypassPermissions", "pill.code.agentMenu.permBypass"),
];

fn home() -> Option<PathBuf> {
    crate::paths::home().map(std::ffi::OsString::from).or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
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
            Item::User { text: body, .. } => text.push_str(&format!("\n## {}\n\n{body}\n", crate::i18n::t("pill.code.agentMenu.exportYou"))),
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
            crate::i18n::t("pill.code.agentMenu.default").to_string()
        } else {
            config.output_style.clone()
        };
        let chat_model = self.chat_model();
        let model = if chat_model.is_empty() { crate::i18n::t("pill.code.agentMenu.default").to_string() } else { self.model_label(&chat_model) };
        let session = self.active_chat().and_then(|c| c.session_id.clone());
        let bookmark = if session.is_some_and(|s| self.is_bookmarked(&s)) { crate::i18n::t("pill.code.agentMenu.bmRemove") } else { crate::i18n::t("pill.code.agentMenu.bmAdd") };
        let flags = self.flag_count();
        match tab {
            0 => vec![
                row("clip", crate::i18n::t("pill.code.agentMenu.attachFile"), Trailing::Kbd(crate::keymap::hint("Ctrl+U")), Act::Attach),
                row("at", crate::i18n::t("pill.code.agentMenu.mentionFile"), Trailing::Kbd("@"), Act::Mention),
                row("trash", crate::i18n::t("pill.code.agentMenu.clearChat"), Trailing::None, Act::Clear),
                row("history", "Rewind", Trailing::Kbd("Esc Esc"), Act::Open(Sub::Rewind)),
                row("bookmark", crate::i18n::t("pill.code.agentMenu.bookmark"), Trailing::Value(bookmark.into()), Act::Bookmark),
                row(
                    "bookmark",
                    crate::i18n::t("pill.code.agentMenu.nextFlagged"),
                    if flags > 0 { Trailing::Value(flags.to_string()) } else { Trailing::None },
                    Act::NextFlag,
                ),
                row("export", crate::i18n::t("pill.code.agentMenu.exportChat"), Trailing::None, Act::Export),
                row("copy", crate::i18n::t("pill.code.agentMenu.copyRemote"), Trailing::None, Act::CopyRemote),
            ],
            1 => vec![
                row("cpu", crate::i18n::t("pill.code.agentMenu.changeModel"), Trailing::Value(model), Act::Open(Sub::Model)),
                Row { icon: "gauge", label: crate::i18n::t("pill.code.agentMenu.effort"), trailing: Trailing::Effort, act: None },
                row("spark", "Ultracode", Trailing::Switch(config.ultracode), Act::Ultracode),
                row("brain", "Thinking", Trailing::Switch(config.thinking), Act::Thinking),
                row("shield", crate::i18n::t("pill.code.agentMenu.switchOnFlag"), Trailing::Switch(config.switch_model_on_flag), Act::SwitchOnFlag),
                row("bolt", crate::i18n::t("pill.code.agentMenu.fastMode"), Trailing::Switch(config.fast_mode), Act::FastMode),
                row("gauge", crate::i18n::t("pill.code.agentMenu.accountUsage"), Trailing::None, Act::Usage),
            ],
            2 => vec![
                row("palette", crate::i18n::t("pill.code.agentMenu.outputStyle"), Trailing::Value(output), Act::Open(Sub::Output)),
                row("shield", crate::i18n::t("pill.code.agentMenu.permissions"), Trailing::Value(config.mode_label().into()), Act::Open(Sub::Permission)),
                row("plug", crate::i18n::t("pill.code.agentMenu.mcpServers"), Trailing::None, Act::Open(Sub::Mcp)),
                row("hook", "Hooks", Trailing::Value("settings.json".into()), Act::Hooks),
                row("layers", crate::i18n::t("pill.code.agentMenu.subagents"), Trailing::Value(self.agents.len().to_string()), Act::Open(Sub::Agents)),
                row("command", crate::i18n::t("pill.code.agentMenu.commandsSkills"), Trailing::Value(self.commands.len().to_string()), Act::Open(Sub::Commands)),
                row("file", crate::i18n::t("pill.code.agentMenu.memory"), Trailing::None, Act::Memory),
                row("box", "Sandbox", Trailing::Switch(config.sandbox), Act::Sandbox),
                row("monitor", crate::i18n::t("pill.code.agentMenu.statusLine"), Trailing::Switch(self.configs.status_line), Act::StatusLine),
                row("file", crate::i18n::t("pill.code.agentMenu.personal"), Trailing::Value("~/.claude/CLAUDE.md".into()), Act::Personal),
                row("gear", crate::i18n::t("pill.code.agentMenu.generalSettings"), Trailing::Value("settings.json".into()), Act::General),
            ],
            3 => vec![
                row("monitor", "Claude in Chrome", Trailing::Switch(config.chrome), Act::Chrome),
                row("plug", "Remote Control", Trailing::None, Act::RemoteTerminal),
                row("palette", "Claude Design", Trailing::None, Act::Design),
            ],
            4 => vec![
                row("history", crate::i18n::t("pill.code.agentMenu.resume"), Trailing::None, Act::Resume),
                row("layers", crate::i18n::t("pill.code.agentMenu.compact"), Trailing::None, Act::Compact),
                row("terminal", crate::i18n::t("pill.code.agentMenu.openTerminal"), Trailing::None, Act::Terminal),
                row("blocks", crate::i18n::t("pill.code.agentMenu.managePlugins"), Trailing::None, Act::Plugins),
                row("refresh", crate::i18n::t("pill.code.agentMenu.reloadPlugins"), Trailing::None, Act::ReloadPlugins),
            ],
            _ => vec![row("shield", crate::i18n::t("pill.code.agentMenu.switchAccount"), Trailing::None, Act::Login), row("x", crate::i18n::t("pill.code.agentMenu.logout"), Trailing::None, Act::Logout)],
        }
    }

    fn effort_slider(&self, cx: &mut Context<Self>) -> StopSlider {
        let levels = self.effort_levels();
        let effort = self.effort_now();
        let stop = levels.iter().position(|(id, _)| *id == effort);
        let count = levels.len();
        let set = cx.listener(move |view, pick: &usize, _, cx| {
            if let Some((id, _)) = levels.get(*pick) {
                view.pick_effort(id.clone(), cx);
            }
        });
        StopSlider::new("effort-slider", count, stop).compact(true).on_change(move |pick, window, cx| set(&pick, window, cx))
    }

    /// Los niveles de esfuerzo de la conversación a la vista (id, nombre), de menor a
    /// mayor: los de Claude o los del agente ACP (sin «Predeterminado», que es no elegir
    /// ninguno). Vacío si el agente no tiene.
    pub(super) fn effort_levels(&self) -> Vec<(String, String)> {
        match self.acp_effort_option() {
            Some(option) => {
                let mut levels: Vec<(String, String)> = option
                    .choices
                    .iter()
                    .filter(|c| c.value != "default")
                    .map(|c| (c.value.clone(), super::acp::effort_name(&c.value)))
                    .collect();
                levels.sort_by_key(|(id, _)| super::acp::effort_rank(id));
                levels
            }
            None if self.current_agent().is_some() => Vec::new(),
            None => config::EFFORTS.iter().map(|(id, _)| (id.to_string(), config::effort_name(id).to_string())).collect(),
        }
    }

    /// El esfuerzo elegido (vacío: el predeterminado).
    pub(super) fn effort_now(&self) -> String {
        match self.acp_effort_option() {
            Some(option) if option.current != "default" => option.current.clone(),
            Some(_) => String::new(),
            None if self.current_agent().is_some() => String::new(),
            None => self.chat_effort(),
        }
    }

    /// El nombre del esfuerzo elegido, para la fila y el chip.
    pub(super) fn effort_text(&self) -> String {
        let now = self.effort_now();
        if self.acp_effort_option().is_some() {
            super::acp::effort_name(&now)
        } else {
            config::effort_label(&now).to_string()
        }
    }

    /// Elige un nivel (vacío: el predeterminado), en Claude o en el agente ACP.
    pub(super) fn pick_effort(&mut self, id: String, cx: &mut Context<Self>) {
        match self.acp_effort_option().map(|o| (o.id.clone(), o.choices.iter().any(|c| c.value == "default"))) {
            Some((config, has_default)) => {
                let value = if id.is_empty() && has_default { "default".to_string() } else { id };
                if !value.is_empty() {
                    self.set_acp_option(config, value, cx);
                }
            }
            None => self.set_config(|c| c.effort = id, cx),
        }
    }

    /// El submenú «Modelo» de un agente ACP: su esfuerzo (si tiene), sus modelos con
    /// nombres legibles —primero los del grupo del actual, el resto con «Más modelos»,
    /// agrupados por proveedor— y otras opciones que mande.
    fn acp_model_entries(&self, out: &mut Vec<Entry>, cx: &mut Context<Self>) {
        let Some(cfg) = self.acp_cfg().filter(|cfg| !cfg.options.is_empty()) else {
            let name = self.current_agent().and_then(super::acp::agent).map_or(crate::i18n::t("pill.code.agentMenu.theAgent"), |agent| agent.name);
            let text = if self.acp_loading() { tf("pill.code.agentMenu.readingModels", &[("name", &name)]) } else { tf("pill.code.agentMenu.noModels", &[("name", &name)]) };
            out.push(Entry::Empty(interned(&text)));
            return;
        };
        if !self.effort_levels().is_empty() {
            let mut effort = Line::new(("sub-effort", 0), crate::i18n::t("pill.code.agentMenu.effort"));
            effort.icon = Some("gauge");
            effort.static_row = true;
            effort.inline_sub = true;
            effort.sublabel = Some(self.effort_text());
            effort.trailing = Trailing::Effort;
            out.push(Entry::Line(effort));
            out.push(Entry::Separator);
        }
        let mut n = 0;
        let query = self.model_filter.read(cx).text().trim().to_lowercase();
        for option in cfg.options.iter().filter(|o| o.category != "thought_level") {
            let config = option.id.clone();
            let mut entries: Vec<(String, String, Option<String>, Option<String>)> = option
                .choices
                .iter()
                .map(|c| {
                    let (label, detail, group) = if option.category == "model" {
                        super::acp::model_entry(&c.value, &c.name)
                    } else {
                        (c.name.clone(), None, None)
                    };
                    (c.value.clone(), label, detail.or_else(|| (!c.description.is_empty()).then(|| c.description.clone())), group)
                })
                .collect();
            // Por grupo (la suscripción primero, sus gratuitos y el resto por nombre) y,
            // dentro, en el orden del agente.
            entries.sort_by_key(|e| e.3.as_deref().map(super::acp::group_rank));
            // El buscador: por nombre, proveedor o id.
            if option.category == "model" && !query.is_empty() {
                entries.retain(|e| {
                    let hay = format!("{} {} {}", e.1, e.3.as_deref().unwrap_or_default(), e.0).to_lowercase();
                    query.split_whitespace().all(|word| hay.contains(word))
                });
                if entries.is_empty() {
                    out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noModelMatch")));
                    continue;
                }
            }
            if option.category != "model" {
                out.push(Entry::Section(interned(&option.name)));
            }
            let current_group = entries.iter().find(|e| e.0 == option.current).and_then(|e| e.3.clone());
            // Pocos: todos. Muchos (OpenCode tiene 181): el grupo del actual y, con «Más
            // modelos», todos por proveedor.
            let many = entries.len() > 12 && query.is_empty();
            let shown: Vec<&(String, String, Option<String>, Option<String>)> = if many && !self.more_models {
                let mut near: Vec<_> = entries.iter().filter(|e| e.3 == current_group || e.0 == option.current).take(10).collect();
                if near.is_empty() {
                    near = entries.iter().take(10).collect();
                }
                near
            } else {
                entries.iter().collect()
            };
            let mut group: Option<&Option<String>> = None;
            for (value, label, detail, entry_group) in shown.iter().map(|e| (&e.0, &e.1, &e.2, &e.3)) {
                if entry_group.is_some() && group != Some(entry_group) && (self.more_models || !many) {
                    out.push(Entry::Section(interned(entry_group.as_deref().unwrap_or_default())));
                }
                group = Some(entry_group);
                n += 1;
                let mut line = Line::new(("acp-opt", n), label.clone());
                line.radio = Some(*value == option.current);
                line.sublabel = detail.clone().or_else(|| (many && !self.more_models).then(|| entry_group.clone()).flatten());
                let (config, value) = (config.clone(), value.clone());
                line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.set_acp_option(config.clone(), value.clone(), cx);
                    view.close_menus(cx);
                })));
                out.push(Entry::Line(line));
            }
            if many {
                let hidden = entries.len() - shown.len();
                let (label, icon) =
                    if self.more_models { (crate::i18n::t("pill.code.agentMenu.fewerModels").to_string(), "chevron-down") } else { (tf("pill.code.agentMenu.moreModels", &[("n", &hidden)]), "chevron-right") };
                let mut line = Line::new(("model-more", n), label);
                line.icon = Some(icon);
                line.click = Some(boxed(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.more_models = !view.more_models;
                    cx.notify();
                })));
                out.push(Entry::Line(line));
            }
        }
    }

    /// Los modos del agente ACP (Build y Plan; Agente, Plan y Preguntar), como los
    /// permisos de Claude.
    pub(super) fn acp_mode_entries(&self, out: &mut Vec<Entry>, cx: &mut Context<Self>) {
        let Some(cfg) = self.acp_cfg().filter(|cfg| !cfg.modes.is_empty()) else {
            let name = self.current_agent().and_then(super::acp::agent).map_or(crate::i18n::t("pill.code.agentMenu.theAgent"), |agent| agent.name);
            let text = if self.acp_loading() { tf("pill.code.agentMenu.readingModes", &[("name", &name)]) } else { tf("pill.code.agentMenu.noModes", &[("name", &name)]) };
            out.push(Entry::Empty(interned(&text)));
            return;
        };
        for (index, (id, name, description)) in cfg.modes.iter().enumerate() {
            let (label, hint) = super::acp::mode_text(id, name, description);
            let mut line = Line::new(("acp-mode", index), label);
            line.radio = Some(cfg.mode.as_deref() == Some(id.as_str()));
            line.sublabel = (!hint.is_empty()).then_some(hint);
            let id = id.clone();
            line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.set_acp_mode(id.clone(), cx);
                view.close_menus(cx);
            })));
            out.push(Entry::Line(line));
        }
    }

    /// El buscador de modelos va cuando el agente tiene muchos (OpenCode, Cursor).
    pub(super) fn model_search_shown(&self) -> bool {
        self.acp_cfg().is_some_and(|cfg| cfg.options.iter().any(|o| o.category == "model" && o.choices.len() > 12))
    }

    /// La opción de esfuerzo del agente ACP de la conversación a la vista.
    fn acp_effort_option(&self) -> Option<&super::chat::AcpOption> {
        self.acp_cfg()?.options.iter().find(|o| o.category == "thought_level")
    }

    /// Una fila del menú, sin dibujar: Expressive la pasa a `MenuItem` y Formal
    /// y Glass a su propia fila (`menus.rs`).
    fn line_of(&self, id: usize, row: Row, cx: &mut Context<Self>) -> Line {
        let mut line = Line::new(("act", id), row.label);
        line.icon = Some(row.icon);
        line.click = row.act.map(|act| boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.menu_act(act, window, cx))));
        match row.trailing {
            Trailing::Effort => {
                line.sublabel = Some(self.effort_text());
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
            out.push(Entry::Section(crate::i18n::t("pill.code.agentMenu.commands")));
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
            let section = crate::i18n::t(*section);
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
            out.push(Entry::Empty(crate::i18n::t("overlay.noResults")));
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
        let tabs = TABS.iter().map(|(icon, label)| IconTab::new(*icon, crate::i18n::t(*label))).collect::<Vec<_>>();
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
        if sub == Sub::Model && self.model_search_shown() {
            menu = menu.header(div().pb(px(6.)).child(self.model_filter.clone()));
        }
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
            // Un agente ACP: sus modos, con lo que hace cada uno.
            Sub::Permission if self.current_agent().is_some() => {
                self.acp_mode_entries(&mut out, cx);
            }
            Sub::Model => {
                // El agente: Claude o uno ACP instalado. Se elige al empezar la
                // conversación; después queda fijo.
                let current = self.current_agent();
                let visible = self.visible_acp();
                if !visible.is_empty() || current.is_some() {
                    out.push(Entry::Section(crate::i18n::t("pill.code.agentMenu.agent")));
                    let fixed = self.agent_fixed();
                    let options = std::iter::once((None, "Claude Code"))
                        .chain(visible.iter().filter_map(|cli| super::acp::agent(cli)).map(|a| (Some(a.cli), a.name)));
                    for (index, (cli, name)) in options.enumerate() {
                        if fixed && cli != current {
                            continue;
                        }
                        let mut line = Line::new(("agent", index), name);
                        line.radio = Some(cli == current);
                        if fixed {
                            line.static_row = true;
                            line.sublabel = Some(crate::i18n::t("pill.code.agentMenu.fixedInChat").into());
                        } else {
                            line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.choose_agent(cli, cx);
                                view.close_menus(cx);
                            })));
                        }
                        out.push(Entry::Line(line));
                    }
                    out.push(Entry::Separator);
                }
                // Un agente ACP: como Claude, el esfuerzo arriba y los modelos debajo (los
                // modos van en el menú de la píldora del modo). Antes de abrir la sesión el
                // agente todavía no dijo cuáles tiene.
                if current.is_some() {
                    self.acp_model_entries(&mut out, cx);
                    return out;
                }
                let mut effort = Line::new(("sub-effort", 0), crate::i18n::t("pill.code.agentMenu.effort"));
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
                        if self.more_models { (crate::i18n::t("pill.code.agentMenu.fewerModels").to_string(), "chevron-down") } else { (tf("pill.code.agentMenu.moreModels", &[("n", &more.len())]), "chevron-right") };
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
                    let mut line = Line::new(("permission", index), crate::i18n::t(*label));
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
                    let mut line = Line::new(("output", index), if style == "default" { crate::i18n::t("pill.code.agentMenu.default").to_string() } else { style.clone() });
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
                    out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.nothingYet")));
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
                    out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noMessagesToRewind")));
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
                    let picks = vec![pick("rewind-code", crate::i18n::t("pill.code.agentMenu.code"), Rewind::Code), pick("rewind-chat", crate::i18n::t("pill.code.agentMenu.conversation"), Rewind::Conversation), pick("rewind-both", crate::i18n::t("pill.code.agentMenu.both"), Rewind::Both)];
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
            out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.loading")));
            return;
        };
        if servers.is_empty() {
            out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noMcp")));
            return;
        }
        for (index, (name, status)) in servers.iter().enumerate() {
            let (color, hint) = match status.as_str() {
                "connected" => (ok, crate::i18n::t("pill.code.agentMenu.connected")),
                "failed" => (warn, crate::i18n::t("pill.code.agentMenu.connectFailed")),
                "needs-auth" => (warn, crate::i18n::t("pill.code.agentMenu.needsAuth")),
                "pending" => (off, crate::i18n::t("pill.code.agentMenu.connecting")),
                "disabled" => (off, crate::i18n::t("pill.code.agentMenu.disabled")),
                _ => (off, ""),
            };
            let disabled = status == "disabled";
            let reconnect = matches!(status.as_str(), "failed" | "needs-auth" | "pending");
            let (toggle_name, reconnect_name) = (name.clone(), name.clone());
            let mut picks = Vec::new();
            if reconnect {
                picks.push(Pick {
                    id: ("mcp-reconnect", index),
                    label: crate::i18n::t("pill.code.agentMenu.reconnect"),
                    click: boxed(cx.listener(move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpReconnect", json!({ "name": reconnect_name }), cx))),
                });
            }
            picks.push(Pick {
                id: ("mcp-toggle", index),
                label: if disabled { crate::i18n::t("pill.code.agentMenu.enable") } else { crate::i18n::t("pill.code.agentMenu.disable") },
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
                None => self.show_toast(crate::i18n::t("pill.code.agentMenu.sendFirst"), cx),
            },
            Act::NextFlag => {
                if !self.next_flagged(cx) {
                    self.show_toast(crate::i18n::t("pill.code.agentMenu.flagHint"), cx);
                }
            }
            Act::Export => self.export_chat(cx),
            Act::CopyRemote => match self.active_chat().and_then(|c| c.remote_url.clone()) {
                Some(url) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                    self.show_toast(crate::i18n::t("pill.code.agentMenu.linkCopied"), cx);
                }
                None => self.show_toast(crate::i18n::t("pill.code.agentMenu.enableRemote"), cx),
            },
            Act::Open(Sub::Mcp) => {
                if self.live_key().is_none() {
                    self.show_toast(crate::i18n::t("pill.code.agentMenu.startFirst"), cx);
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
                    self.show_toast(tf("pill.code.agentMenu.chromeState", &[("state", &if on { crate::i18n::t("pill.code.agentMenu.stateOn") } else { crate::i18n::t("pill.code.agentMenu.stateOff") })]), cx);
                } else {
                    let verb = if on { crate::i18n::t("pill.code.agentMenu.verbOn") } else { crate::i18n::t("pill.code.agentMenu.verbOff") };
                    self.show_toast(tf("pill.code.agentMenu.chromeNext", &[("verb", &verb)]), cx);
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
                    self.show_toast(crate::i18n::t("pill.code.agentMenu.designUnavailable"), cx);
                }
            }
            Act::Compact => {
                if self.live_key().is_none() {
                    self.show_toast(crate::i18n::t("pill.code.agentMenu.startFirst"), cx);
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
                    Ok(_) => view.show_toast(crate::i18n::t("pill.code.agentMenu.pluginsReloaded"), cx),
                    Err(error) => view.show_toast(error, cx),
                }),
                None => self.show_toast(crate::i18n::t("pill.code.agentMenu.startFirst"), cx),
            },
            Act::Login => self.open_terminal("auth login", crate::i18n::t("pill.code.agentMenu.accountTab"), cwd, window, cx),
            Act::Logout => self.open_terminal("auth logout", crate::i18n::t("pill.code.agentMenu.accountTab"), cwd, window, cx),
        }
        cx.notify();
    }

    /// Abre un archivo de texto con su programa; si no existe, lo crea con `initial`.
    fn open_text_file(&mut self, path: Option<PathBuf>, initial: &str, cx: &mut Context<Self>) {
        let Some(path) = path else {
            self.show_toast(crate::i18n::t("pill.code.agentMenu.openProjectFirst"), cx);
            return;
        };
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(error) = std::fs::write(&path, initial) {
                self.show_toast(tf("pill.code.agentMenu.createFailed", &[("path", &path.display()), ("error", &error)]), cx);
                return;
            }
        }
        if let Err(error) = crate::launcher::shell_open(&path.to_string_lossy()) {
            self.show_toast(tf("pill.code.agentMenu.openFailed", &[("path", &path.display()), ("error", &error)]), cx);
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
            self.show_toast(crate::i18n::t("pill.code.agentMenu.nothingToExport"), cx);
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
                Ok(()) => view.show_toast(crate::i18n::t("pill.code.agentMenu.exported"), cx),
                Err(error) => view.show_toast(tf("pill.code.agentMenu.exportFailed", &[("error", &error)]), cx),
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
