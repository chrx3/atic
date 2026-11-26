//! El menú de acciones de Expressive, copia del de la referencia: un filtro, seis
//! pestañas (Contexto, Modelo, Personalizar, Integraciones, Sesión, Cuenta) y
//! submenús con «atrás» (modelo, permisos, estilo de salida, servidores MCP,
//! subagentes, comandos). El botón de modelo abre directo el submenú «Modelo».

use crate::i18n::{t as tr, tf};
use std::path::PathBuf;

use gpui::{div, prelude::*, px, svg, AnyElement, ClickEvent, Context, SharedString, Window};
use gpui_m3::{Avatar, Icon, IconTab, IconTabs, MenuItem, StopSlider, Switch, Theme, Tooltip};
use serde_json::{json, Value};

use super::chat::Item;
use super::rewind::Rewind;
use super::style::t;
use super::{config, CodeView, Menu};

const MENU_W: f32 = 340.;
const MENU_H: f32 = 440.;

/// La esquina del menú junto a la caja de texto, desde la que crece al abrirse: abajo a la
/// izquierda si abre hacia arriba, arriba a la izquierda si abre hacia abajo.
fn menu_origin(below: bool) -> gpui::Corner {
    if below { gpui::Corner::TopLeft } else { gpui::Corner::BottomLeft }
}
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
    /// Los borradores apartados del compositor.
    Stash,
    /// Elegir varios modelos para mandarles el mismo mensaje.
    Multi,
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
            Sub::Stash => crate::i18n::t("pill.code.stash.title"),
            Sub::Multi => crate::i18n::t("pill.code.multi.title"),
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
    StashDraft,
    SideChat,
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
    /// Un interruptor con un botón opcional al lado (los servidores MCP): la
    /// fila es estática, el interruptor activa o desactiva y el botón reintenta.
    Mcp { name: String, on: bool, index: usize, reconnect: bool },
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
    /// La fila elegida con el teclado mientras se filtra (↑/↓ y Enter).
    pub picked: bool,
    /// Elegida, sin radio: la fila se tiñe y lleva un check (la lista de modelos).
    pub chosen: Option<bool>,
    /// Inicial con el tono del nombre, en vez del ícono (los subagentes).
    pub avatar: Option<String>,
    /// Medidor de riesgo de 1 a 4 (los modos de permisos).
    pub risk: Option<u8>,
    /// El texto completo, para una fila cuyo texto se corta con elipsis.
    pub tooltip: Option<String>,
    /// Acción en dos pasos: el primer toque arma la fila y muestra este texto
    /// («Toca de nuevo para…»); el segundo ejecuta (Rewind destructivo).
    pub confirm: Option<String>,
    /// Trozos de la etiqueta que coinciden con el filtro (resaltado del buscador).
    pub highlights: Vec<std::ops::Range<usize>>,
}

impl Line {
    pub fn new(id: (&'static str, usize), label: impl Into<String>) -> Self {
        Self { id, label: label.into(), icon: None, sublabel: None, radio: None, trailing: Trailing::None, inline_sub: false, static_row: false, dot: None, click: None, picked: false, chosen: None, avatar: None, risk: None, tooltip: None, confirm: None, highlights: Vec::new() }
    }
}

/// Qué adjunto muestra un chip de solo lectura en una tarjeta de borrador.
pub(super) enum TagKind {
    File,
    Image,
    Pasted,
    Quote,
}

/// Un chip chico de solo lectura en una tarjeta (un adjunto del borrador).
pub(super) struct Tag {
    pub id: (&'static str, usize),
    pub kind: TagKind,
    pub label: String,
}

/// Lo que lista un menú.
pub(super) enum Entry {
    Section(&'static str),
    Separator,
    Line(Line),
    /// Un texto centrado cuando no hay nada.
    Empty(&'static str),
    /// Una tarjeta con título, segunda línea, hora y chips de adjuntos (borradores).
    Block { title: String, subtitle: Option<String>, age: Option<String>, tags: Vec<Tag>, picks: Vec<Pick> },
    /// Opciones en una sola fila de chips (la elegida marcada), que pasan a la línea
    /// siguiente si no caben: elegir el agente sin gastar una fila por cada uno.
    Choice(Vec<(Pick, bool)>),
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

fn home() -> Option<PathBuf> {
    crate::paths::home().map(std::ffi::OsString::from).or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// Las skills instaladas (`~/.claude/skills` y `.claude/skills` del proyecto), con su
/// descripción: el menú «/» las lista aunque la sesión todavía no diga sus comandos. Se
/// relee cada pocos segundos, no en cada cuadro.
pub(super) fn local_skills(project: Option<&std::path::Path>) -> Vec<(String, String)> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    type Cached = (Option<PathBuf>, Instant, Vec<(String, String)>);
    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
    let project = project.map(PathBuf::from);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, when, skills)) = cache.as_ref() {
        if *at == project && when.elapsed() < Duration::from_secs(10) {
            return skills.clone();
        }
    }
    let mut skills: Vec<(String, String)> = Vec::new();
    let roots = project.iter().map(|p| p.join(".claude").join("skills")).chain(home().map(|h| h.join(".claude").join("skills")));
    for root in roots {
        let Ok(dirs) = std::fs::read_dir(&root) else {
            continue;
        };
        for dir in dirs.flatten() {
            let Ok(text) = std::fs::read_to_string(dir.path().join("SKILL.md")) else {
                continue;
            };
            let (name, description) = skill_front_matter(&text);
            let name = name.unwrap_or_else(|| dir.file_name().to_string_lossy().into_owned());
            if !skills.iter().any(|(n, _)| *n == name) {
                skills.push((name, description.unwrap_or_default()));
            }
        }
    }
    skills.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    *cache = Some((project, Instant::now(), skills.clone()));
    skills
}

/// `name` y `description` del encabezado `---` de un SKILL.md (la descripción, de una línea
/// y corta: es la segunda línea de la fila del menú).
fn skill_front_matter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let (mut name, mut description) = (None, None);
    for line in lines.take_while(|l| l.trim() != "---") {
        let clean = |value: &str| value.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(clean(value)).filter(|v| !v.is_empty());
        } else if let Some(value) = line.strip_prefix("description:") {
            let value = clean(value);
            let short: String = value.chars().take(120).collect();
            description = Some(if short.len() < value.len() { format!("{}…", short.trim_end()) } else { short }).filter(|v| !v.is_empty());
        }
    }
    (name, description)
}

fn home_claude(file: &str) -> Option<PathBuf> {
    home().map(|home| home.join(".claude").join(file))
}

/// La conversación como Markdown (`exportConversation` de la referencia): lo que dijo
/// cada uno y, entre medio, cada herramienta con el comienzo de su input. `agent` es el
/// nombre de quien responde (Claude, Codex, OpenCode…).
pub(super) fn export_markdown(title: &str, agent: &str, items: &[Item]) -> String {
    let mut text = format!("# {title}\n");
    for item in items {
        match item {
            Item::User { text: body, .. } => text.push_str(&format!("\n## {}\n\n{body}\n", crate::i18n::t("pill.code.agentMenu.exportYou"))),
            Item::Text(body) => text.push_str(&format!("\n## {agent}\n\n{body}\n")),
            Item::Peer(note) => text.push_str(&format!("\n## {}\n\n{}\n", note.label(), note.body)),
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
        let mut rows = match tab {
            0 => vec![
                row("clip", crate::i18n::t("pill.code.agentMenu.attachFile"), Trailing::Kbd(crate::keymap::hint("Ctrl+U")), Act::Attach),
                row("at", crate::i18n::t("pill.code.agentMenu.mentionFile"), Trailing::Kbd("@"), Act::Mention),
                row("trash", crate::i18n::t("pill.code.agentMenu.clearChat"), Trailing::None, Act::Clear),
                row("history", "Rewind", Trailing::Kbd("Esc Esc"), Act::Open(Sub::Rewind)),
                row("chat", crate::i18n::t("pill.code.side.action"), Trailing::None, Act::SideChat),
                row("cpu", crate::i18n::t("pill.code.multi.title"), Trailing::None, Act::Open(Sub::Multi)),
                row("bookmark", crate::i18n::t("pill.code.stash.save"), Trailing::None, Act::StashDraft),
                row(
                    "layers",
                    crate::i18n::t("pill.code.stash.title"),
                    if self.stash.is_empty() { Trailing::None } else { Trailing::Value(self.stash.len().to_string()) },
                    Act::Open(Sub::Stash),
                ),
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
                row("shield", crate::i18n::t("pill.code.agentMenu.permissions"), Trailing::Value(self.mode_label_now(&config)), Act::Open(Sub::Permission)),
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
        };
        // Con un agente ACP no van los interruptores que solo entiende Claude Code ni lo que
        // necesita su sesión (Rewind, preguntar al margen): no harían nada o dirían que no
        // hay mensajes. El esfuerzo, solo si el agente lo ofrece.
        if self.current_agent().is_some() {
            let effort = self.acp_effort_option().is_some();
            rows.retain(|row| {
                !matches!(row.act, Some(Act::Open(Sub::Rewind) | Act::SideChat | Act::Ultracode | Act::Thinking | Act::SwitchOnFlag | Act::FastMode))
                    && (effort || !matches!(row.trailing, Trailing::Effort))
            });
        }
        rows
    }

    /// La sección estática de una fila del menú de acciones (`None`: lista plana).
    /// Solo criterios fijos (qué hace la fila), sin telemetría ni orden por uso.
    fn row_section(tab: usize, act: Option<Act>) -> Option<&'static str> {
        match (tab, act) {
            (0, Some(Act::Attach | Act::Mention)) => Some("pill.code.actions.add"),
            (0, Some(Act::Clear | Act::Open(Sub::Rewind) | Act::SideChat | Act::Open(Sub::Multi))) => Some("pill.code.actions.conversation"),
            (0, Some(Act::StashDraft | Act::Open(Sub::Stash) | Act::Bookmark | Act::NextFlag | Act::Export | Act::CopyRemote)) => Some("pill.code.actions.saveShare"),
            (2, Some(Act::Open(Sub::Output) | Act::Open(Sub::Permission) | Act::Open(Sub::Mcp))) => Some("pill.code.actions.modelPerms"),
            (2, Some(Act::Hooks | Act::Open(Sub::Agents) | Act::Open(Sub::Commands) | Act::Memory)) => Some("pill.code.actions.project"),
            (2, Some(Act::Sandbox | Act::StatusLine | Act::Personal | Act::General)) => Some("pill.code.actions.files"),
            _ => None,
        }
    }

    /// Lo que un agente ACP oculta en una pestaña, con sus nombres: el aviso lo
    /// dice en vez de quitar las filas en silencio.
    fn acp_hidden_labels(&self, tab: usize) -> Vec<String> {
        if self.current_agent().is_none() {
            return Vec::new();
        }
        match tab {
            0 => vec!["Rewind".to_string(), crate::i18n::t("pill.code.side.action").to_string()],
            1 => {
                let mut hidden = vec![
                    "Ultracode".to_string(),
                    "Thinking".to_string(),
                    crate::i18n::t("pill.code.agentMenu.switchOnFlag").to_string(),
                    crate::i18n::t("pill.code.agentMenu.fastMode").to_string(),
                ];
                if self.effort_levels().is_empty() {
                    hidden.push(crate::i18n::t("pill.code.agentMenu.effort").to_string());
                }
                hidden
            }
            _ => Vec::new(),
        }
    }

    /// El aviso de qué filas se ocultan con el agente ACP de ahora (`None` sin
    /// agente o sin nada oculto en la pestaña).
    fn acp_banner(&self, tab: usize) -> Option<Entry> {
        let cli = self.current_agent()?;
        let names = self.acp_hidden_labels(tab);
        if names.is_empty() {
            return None;
        }
        let agent = super::acp::agent(cli).map(|info| info.name.to_string()).unwrap_or_else(|| "Claude Code".to_string());
        let mut line = Line::new(("act-acp", tab), tf("pill.code.actions.acpHidden", &[("agent", &agent), ("n", &names.len())]));
        line.icon = Some("slash");
        line.static_row = true;
        line.sublabel = Some(names.join(", "));
        Some(Entry::Line(line))
    }

    /// La pestaña «Modelo» del menú: la fila héroe (agente + modelo + esfuerzo,
    /// que abre el submenú), el esfuerzo y el resto bajo su sección. La fila de
    /// esfuerzo la dibuja cada estilo (`effort_block` en Expressive, puntitos
    /// en Formal y Glass); la de «Cambiar modelo» la reemplaza el héroe.
    fn model_tab_entries(&self, cx: &mut Context<Self>) -> Vec<Entry> {
        let mut out = Vec::new();
        if let Some(banner) = self.acp_banner(1) {
            out.push(banner);
        }
        let chat_model = self.chat_model();
        let model = if chat_model.is_empty() { crate::i18n::t("pill.code.agentMenu.default").to_string() } else { self.model_label(&chat_model) };
        let agent = self.current_agent().and_then(super::acp::agent).map(|info| info.name.to_string()).unwrap_or_else(|| "Claude Code".to_string());
        let label = if self.effort_levels().is_empty() {
            model.clone()
        } else {
            tf("pill.code.actions.heroLabel", &[("model", &model), ("effort", &self.effort_text())])
        };
        let mut hero = Line::new(("act-hero", 1), label);
        hero.icon = Some("cpu");
        hero.chosen = Some(true);
        hero.sublabel = Some(tf("pill.code.actions.heroSub", &[("agent", &agent), ("change", &crate::i18n::t("pill.code.agentMenu.changeModel"))]));
        hero.click = Some(boxed(cx.listener(|view, _: &ClickEvent, window, cx| view.menu_act(Act::Open(Sub::Model), window, cx))));
        out.push(Entry::Line(hero));
        let mut section = false;
        for (index, row) in self.rows(1).into_iter().enumerate() {
            if matches!(row.act, Some(Act::Open(Sub::Model))) {
                continue;
            }
            if !section {
                out.push(Entry::Section(crate::i18n::t("pill.code.actions.modelSettings")));
                section = true;
            }
            out.push(Entry::Line(self.line_of(100 + index, row, cx)));
        }
        out
    }

    /// Lo fijo del submenú «Modelo»: el agente y el esfuerzo. Solo la lista de modelos se
    /// desplaza debajo.
    fn model_header(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = *Theme::of(cx);
        let mut header = div().flex().flex_col().px(px(8.)).pb(px(4.));
        let visible = self.visible_acp();
        if !visible.is_empty() || self.current_agent().is_some() {
            header = header
                .child(div().pt(px(6.)).pb(px(6.)).px(px(4.)).text_size(px(12.)).font_weight(gpui::FontWeight(700.)).text_color(theme.primary).child(crate::i18n::t("pill.code.agentMenu.agent")))
                .child(self.agent_segments(&visible, cx));
        }
        header.children(self.effort_block(cx))
    }

    /// Los agentes en una sola fila: el logo de cada uno y, en el elegido, su nombre. Con la
    /// conversación empezada el agente queda fijo: los demás se apagan.
    fn agent_segments(&self, visible: &[&'static str], cx: &mut Context<Self>) -> gpui::Div {
        let theme = *Theme::of(cx);
        let current = self.current_agent();
        let fixed = self.agent_fixed();
        let options = std::iter::once((None, "Claude Code")).chain(visible.iter().filter_map(|cli| super::acp::agent(cli)).map(|a| (Some(a.cli), a.name)));
        let mut group = div().flex().gap(px(3.)).p(px(3.)).rounded(px(21.)).bg(theme.surface_container_high).when(fixed, |el| el.mb(px(2.)));
        for (index, (cli, name)) in options.enumerate() {
            let selected = cli == current;
            let locked = fixed && !selected;
            let color = if selected { theme.on_primary_container } else { theme.on_surface_variant };
            let logo = match super::delegate::agent_logo(cli.unwrap_or("claude")) {
                Some(path) => svg().path(path).size(px(18.)).flex_none().text_color(color).into_any_element(),
                None => Icon::new("cpu").size(px(18.)).color(color).into_any_element(),
            };
            let segment = div()
                .id(("agent-segment", index))
                .h(px(40.))
                .min_w(px(40.))
                .px(px(11.))
                .rounded(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .text_color(color)
                .when(selected, |el| el.bg(theme.primary_container))
                .when(locked, |el| el.opacity(0.45))
                .child(logo)
                .when(selected, |el| el.child(div().text_size(px(13.)).font_weight(gpui::FontWeight(600.)).child(name)));
            let segment = if locked {
                segment.tooltip(Tooltip::text(crate::i18n::t("pill.code.agentMenu.fixedInChat")))
            } else if selected {
                segment
            } else {
                let hover = theme.surface_container_highest;
                segment
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .tooltip(Tooltip::text(name))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.choose_agent(cli, cx);
                        view.configs.set_last_agent(cli);
                    }))
            };
            group = group.child(segment);
        }
        if fixed {
            let muted = theme.on_surface_variant;
            return div()
                .flex()
                .flex_col()
                .child(group)
                .child(div().flex().items_center().gap(px(6.)).px(px(4.)).pb(px(4.)).text_size(px(12.)).text_color(muted).child(Icon::new("lock").size(px(14.)).color(muted)).child(crate::i18n::t("pill.code.agentMenu.fixedInChat")));
        }
        group
    }

    /// La pista de un nivel de esfuerzo, para los que se conocen.
    fn effort_hint(id: &str) -> Option<&'static str> {
        Some(match id {
            "low" => crate::i18n::t("pill.code.agentMenu.effortHintLow"),
            "medium" => crate::i18n::t("pill.code.agentMenu.effortHintMedium"),
            "high" => crate::i18n::t("pill.code.agentMenu.effortHintHigh"),
            "xhigh" => crate::i18n::t("pill.code.agentMenu.effortHintXhigh"),
            "max" => crate::i18n::t("pill.code.agentMenu.effortHintMax"),
            _ => return None,
        })
    }

    /// El esfuerzo como bloque propio: el nombre del nivel arriba, la barra de ancho completo
    /// que se arrastra, y qué significa el nivel debajo. Si el agente no tiene esfuerzo, el
    /// bloque queda apagado y lo dice. `None` mientras el agente aún no mandó sus opciones.
    fn effort_block(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let theme = *Theme::of(cx);
        if self.current_agent().is_some() && !self.acp_cfg().is_some_and(|cfg| !cfg.options.is_empty()) {
            return None;
        }
        let levels = self.effort_levels();
        let frame = || div().flex().flex_col().mx(px(4.)).my(px(4.)).py(px(8.)).border_t_1().border_b_1().border_color(theme.outline_variant);
        let title = |value: Option<String>, color: gpui::Hsla| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(2.))
                .child(Icon::new("gauge").size(px(20.)).color(color))
                .child(div().text_size(px(14.)).font_weight(gpui::FontWeight(500.)).child(crate::i18n::t("pill.code.agentMenu.effort")))
                .children(value.map(|value| div().ml_auto().text_size(px(13.)).font_weight(gpui::FontWeight(600.)).text_color(theme.primary).child(value)))
        };
        if levels.is_empty() {
            let muted = theme.on_surface_variant;
            return Some(frame().opacity(0.6).child(title(None, muted)).child(div().px(px(2.)).pt(px(4.)).text_size(px(12.)).text_color(muted).child(crate::i18n::t("pill.code.agentMenu.effortNone"))));
        }
        let now = self.effort_now();
        let stop = levels.iter().position(|(id, _)| *id == now);
        let hint = stop.and_then(|i| Self::effort_hint(&levels[i].0)).unwrap_or("");
        let count = levels.len();
        let set = cx.listener(move |view, pick: &usize, _, cx| {
            if let Some((id, _)) = levels.get(*pick) {
                // Arrastrando llegan muchos avisos por el mismo nivel: solo cambia al cruzar a otro.
                if view.effort_now() != *id {
                    view.pick_effort(id.clone(), cx);
                }
            }
        });
        let slider = StopSlider::new("effort-slider", count, stop).fill().draggable().on_change(move |pick, window, cx| set(&pick, window, cx));
        Some(
            frame()
                .child(title(Some(self.effort_text()), theme.on_surface_variant))
                .child(div().px(px(2.)).child(slider))
                .child(div().px(px(2.)).h(px(18.)).text_size(px(12.)).text_color(theme.on_surface_variant).child(hint)),
        )
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

    /// Lo que dice la fila «Predeterminado»: el modelo que de verdad responde en esta
    /// conversación si ya se sabe (Claude Code decide cuál es el predeterminado, y puede no ser
    /// el que describe su lista), y si no, la descripción de la lista.
    fn default_model_note(&self) -> Option<String> {
        // Solo si esta conversación usa el predeterminado: con otro modelo elegido, el que
        // responde es ese y la fila no tiene nada que decir de él.
        let actual = self
            .active_chat()
            .filter(|_| self.chat_model().is_empty())
            .and_then(|chat| chat.model.clone())
            .filter(|model| !model.is_empty() && !model.starts_with('<'));
        match actual {
            Some(model) => Some(tf("pill.code.agentMenu.defaultInUse", &[("model", &self.model_label(&model))])),
            None => self.model_info.get("").cloned(),
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
                    self.remember_acp_choice(Some(config.as_str()), &value);
                    self.set_acp_option(config, value, cx);
                }
            }
            None => self.set_config(|c| c.effort = id, cx),
        }
    }

    /// El submenú «Modelo» de un agente ACP: su esfuerzo (si tiene), sus modelos con
    /// nombres legibles —primero los del grupo del actual, el resto con «Más modelos»,
    /// agrupados por proveedor— y otras opciones que mande.
    fn acp_model_entries(&self, out: &mut Vec<Entry>, with_effort: bool, cx: &mut Context<Self>) {
        let Some(cfg) = self.acp_cfg().filter(|cfg| !cfg.options.is_empty()) else {
            let name = self.current_agent().and_then(super::acp::agent).map_or(crate::i18n::t("pill.code.agentMenu.theAgent"), |agent| agent.name);
            let text = if self.acp_loading() { tf("pill.code.agentMenu.readingModels", &[("name", &name)]) } else { tf("pill.code.agentMenu.noModels", &[("name", &name)]) };
            out.push(Entry::Empty(interned(&text)));
            return;
        };
        if with_effort && !self.effort_levels().is_empty() {
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
                line.chosen = Some(*value == option.current);
                line.sublabel = detail.clone().or_else(|| (many && !self.more_models).then(|| entry_group.clone()).flatten());
                let (config, value) = (config.clone(), value.clone());
                // Elegir el modelo cierra el menú; las demás opciones (variante, etc.) lo dejan abierto.
                let is_model = option.category == "model";
                line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.remember_acp_choice(Some(config.as_str()), &value);
                    view.set_acp_option(config.clone(), value.clone(), cx);
                    if is_model {
                        view.close_menus(cx);
                    }
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

    /// Lo elegido en el agente ACP a la vista (`config` o, sin ella, el modo) queda también en
    /// su borrador y en disco: la próxima conversación con él parte igual, como con Claude.
    pub(super) fn remember_acp_choice(&mut self, config: Option<&str>, value: &str) {
        let Some(cli) = self.current_agent() else {
            return;
        };
        let Some(draft) = self.acp_drafts.get_mut(cli).filter(|draft| !draft.is_empty()) else {
            return;
        };
        match config.map(str::to_string).or_else(|| draft.mode_config.clone()) {
            Some(id) => draft.set(&id, value),
            None if draft.modes.iter().any(|(id, _, _)| id == value) => draft.mode = Some(value.to_string()),
            None => return,
        }
        let wanted = draft.wanted();
        self.configs.set_acp_choices(cli, wanted);
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
            line.chosen = Some(cfg.mode.as_deref() == Some(id.as_str()));
            line.sublabel = (!hint.is_empty()).then_some(hint);
            let id = id.clone();
            line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.remember_acp_choice(None, &id);
                view.set_acp_mode(id.clone(), cx);
                // Elegir un modo termina «Permitir todo», como con Claude.
                if let Some(key) = view.active.clone() {
                    view.end_allow_all(Some(&key), cx);
                }
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
        // Los interruptores dicen qué hacen, en una línea con elipsis.
        let desc = match row.act {
            Some(Act::Ultracode) => Some(crate::i18n::t("pill.code.actions.descUltracode")),
            Some(Act::Thinking) => Some(crate::i18n::t("pill.code.actions.descThinking")),
            Some(Act::SwitchOnFlag) => Some(crate::i18n::t("pill.code.actions.descSwitchOnFlag")),
            Some(Act::FastMode) => Some(crate::i18n::t("pill.code.actions.descFastMode")),
            Some(Act::Sandbox) => Some(crate::i18n::t("pill.code.actions.descSandbox")),
            Some(Act::StatusLine) => Some(crate::i18n::t("pill.code.actions.descStatusLine")),
            Some(Act::Chrome) => Some(crate::i18n::t("pill.code.actions.descChrome")),
            _ => None,
        };
        if let Some(desc) = desc {
            line.sublabel = Some(desc.to_string());
            line.inline_sub = true;
        }
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
            if tab == 1 {
                return self.model_tab_entries(cx);
            }
            if let Some(banner) = self.acp_banner(tab) {
                out.push(banner);
            }
            let mut section: Option<&'static str> = None;
            for (index, row) in self.rows(tab).into_iter().enumerate() {
                let at = Self::row_section(tab, row.act);
                if at != section {
                    if let Some(title) = at {
                        out.push(Entry::Section(crate::i18n::t(title)));
                    }
                    section = at;
                }
                out.push(Entry::Line(self.line_of(tab * 100 + index, row, cx)));
            }
            return out;
        }
        // Con «/» delante se buscan solo comandos y skills, todos los que coincidan.
        let slash = query.starts_with('/');
        let commands: Vec<(usize, String, String)> = self
            .command_names()
            .into_iter()
            .enumerate()
            .filter(|(_, (name, _))| name.to_lowercase().contains(query.trim_start_matches('/')))
            .take(if slash { usize::MAX } else { 8 })
            .map(|(index, (name, description))| (index, name, description))
            .collect();
        if !commands.is_empty() {
            out.push(Entry::Section(crate::i18n::t(if slash { "pill.code.agentMenu.commandsSkills" } else { "pill.code.agentMenu.commands" })));
            for (index, name, description) in commands {
                let mut line = Line::new(("filter-command", index), format!("/{name}"));
                line.icon = Some("command");
                line.inline_sub = true;
                line.sublabel = Some(description);
                line.highlights = gpui_m3::match_ranges(&line.label, query.trim_start_matches('/'));
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
            // La pestaña de origen va en la sección: cada resultado dice de dónde viene.
            out.push(Entry::Section(section));
            for (index, row) in rows.into_iter().enumerate() {
                let mut line = self.line_of(tab * 100 + index, row, cx);
                line.highlights = gpui_m3::match_ranges(&line.label, query);
                out.push(Entry::Line(line));
            }
        }
        if out.is_empty() {
            out.push(Entry::Empty(crate::i18n::t("overlay.noResults")));
            // Vacío con acción: borra el filtro sin cerrar el menú.
            let mut clear = Line::new(("filter-clear", 0), crate::i18n::t("pill.code.actions.clearFilter").to_string());
            clear.icon = Some("x");
            clear.click = Some(boxed(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.menu_pick = 0;
                view.menu_filter.update(cx, |field, cx| field.set_text("", cx));
                cx.notify();
            })));
            out.push(Entry::Line(clear));
        }
        // Con filtro, una fila queda elegida para Enter: la primera, o la que se movió con ↑/↓.
        let pickable: Vec<usize> = pickable_lines(&out);
        if let Some(&at) = pickable.get(self.menu_pick % pickable.len().max(1)) {
            if let Entry::Line(line) = &mut out[at] {
                line.picked = true;
            }
        }
        out
    }

    /// ↑/↓ (o Tab/Shift+Tab) en el filtro del menú de acciones mueven la fila elegida.
    pub(super) fn menu_filter_key(&mut self, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let query = self.menu_filter.read(cx).text().trim().to_lowercase();
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
        let count = pickable_lines(&self.menu_entries(&query, self.menu_tab, cx)).len();
        if count == 0 {
            return;
        }
        let current = self.menu_pick % count;
        self.menu_pick = (current as isize + step).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    /// Enter en el filtro: hace lo mismo que un clic en la fila elegida. El clic corre
    /// después de esta actualización (sus manejadores vuelven a tomar la vista).
    pub(super) fn activate_menu_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.menu_filter.read(cx).text().trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let picked = self.menu_entries(&query, self.menu_tab, cx).into_iter().find_map(|entry| match entry {
            Entry::Line(line) if line.picked => line.click,
            _ => None,
        });
        if let Some(click) = picked {
            window.defer(cx, move |window, cx| click(&ClickEvent::default(), window, cx));
        }
    }

    /// La fila de Expressive para una línea.
    fn m3_item(&self, mut line: Line, cx: &mut Context<Self>) -> MenuItem {
        let muted = t().muted;
        let mut item = MenuItem::new(line.id, line.label).dense(true).highlighted(line.picked);
        if !line.highlights.is_empty() {
            item = item.label_highlights(std::mem::take(&mut line.highlights));
        }
        if let Some(icon) = line.icon {
            item = item.icon(icon);
        }
        if let Some(on) = line.radio {
            item = item.radio(on);
        }
        if let Some(on) = line.chosen {
            item = item.selected(on);
        }
        if let Some(level) = line.risk {
            item = item.risk(level);
        }
        if let Some(text) = line.tooltip {
            item = item.tooltip(text);
        }
        if let Some(label) = line.confirm {
            item = item.confirm(label);
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
        if let Some(name) = line.avatar {
            item = item.leading(Avatar::new(name).size(px(28.)));
        }
        item = match line.trailing {
            Trailing::None => item,
            Trailing::Value(value) => item.trailing(div().text_size(px(11.5)).text_color(muted).child(value)),
            Trailing::Kbd(kbd) => item.shortcut(kbd),
            Trailing::Switch(on) => item.trailing(Switch::new(("act-switch", line.id.1), on).compact(true)),
            Trailing::Effort => item.trailing(self.effort_slider(cx)),
            Trailing::Mcp { name, on, index, reconnect } => {
                let weak = cx.weak_entity();
                let mut trailing = div().flex().items_center().gap(px(6.)).min_h(px(40.));
                if reconnect {
                    let retry_name = name.clone();
                    trailing = trailing.child(gpui_m3::Chip::new(("mcp-reconnect", index), crate::i18n::t("pill.code.agentMenu.reconnect")).on_click(boxed(cx.listener(
                        move |view, _: &ClickEvent, _, cx| view.mcp_call("mcpReconnect", json!({ "name": retry_name }), cx),
                    ))));
                }
                trailing = trailing.child(Switch::new(("mcp-switch", index), on).compact(true).on_toggle(move |enabled, _, cx| {
                    let _ = weak.update(cx, |view, cx| view.mcp_call("mcpToggle", json!({ "name": name, "enabled": enabled }), cx));
                }));
                item.trailing(trailing)
            }
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
            Entry::Choice(picks) => menu.item(
                div()
                    .px(px(12.))
                    .py(px(4.))
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(picks.into_iter().map(|(pick, selected)| gpui_m3::Chip::new(pick.id, pick.label).filter(selected).on_click(pick.click))),
            ),
            // Una tarjeta de borrador: título con su hora, segunda línea, chips de
            // adjuntos por tipo y acciones debajo (Recuperar, Descartar).
            Entry::Block { title, subtitle, age, tags, picks } => menu.item(
                div()
                    .px(px(12.))
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
                            .children(age.map(|age| div().flex_none().text_size(px(11.)).text_color(t().muted).child(age))),
                    )
                    .children(subtitle.map(|subtitle| div().truncate().text_size(px(12.)).text_color(t().muted).child(subtitle)))
                    .when(!tags.is_empty(), |el| {
                        el.child(div().flex().flex_wrap().gap(px(4.)).children(tags.into_iter().map(|tag| {
                            let chip = gpui_m3::Chip::new(tag.id, tag.label).inert(true).size(gpui_m3::ChipSize::Small);
                            match tag.kind {
                                TagKind::File => chip.icon("file"),
                                TagKind::Image => chip.leading(svg().path("icons/image.svg").size(px(13.)).text_color(t().muted)),
                                TagKind::Pasted => chip.leading(svg().path("icons/clipboard.svg").size(px(13.)).text_color(t().muted)),
                                // La cita se reconoce por sus comillas, como en la caja.
                                TagKind::Quote => chip,
                            }
                        })))
                    })
                    .child(div().flex().gap(px(4.)).children(picks.into_iter().map(|pick| gpui_m3::Chip::new(pick.id, pick.label).on_click(pick.click)))),
            ),
        }
    }

    /// El menú de acciones (o, con `sub`, uno de sus submenús). `below`: abre bajo el cuadro de
    /// texto (el chat vacío lo tiene arriba).
    pub(super) fn agent_menu(&self, sub: Option<Sub>, below: bool, list_h: f32, cx: &mut Context<Self>) -> AnyElement {
        if let Some(sub) = sub {
            return self.sub_menu(sub, below, list_h, cx);
        }
        let query = self.menu_filter.read(cx).text().trim().to_lowercase();
        let filtering = !query.is_empty();
        let set_tab = cx.listener(|view, tab: &usize, _, cx| {
            view.menu_tab = *tab;
            cx.notify();
        });
        let tabs = TABS.iter().map(|(icon, label)| IconTab::new(*icon, crate::i18n::t(*label))).collect::<Vec<_>>();
        // Las pestañas y el buscador van del lado del cuadro de texto: el menú crece o se
        // achica por el otro, y ellos no se mueven al filtrar. Abriendo hacia arriba quedan
        // abajo, con el buscador al final; hacia abajo, arriba con el buscador primero.
        // Las pestañas quedan fijas también al filtrar (el icono con nombre solo en la
        // activa lo pone `IconTabs`); solo la lista se desplaza.
        let field = div().on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| view.menu_filter_key(event, cx))).child(self.menu_filter.clone());
        let tabs = div().when(below, |el| el.pt(px(8.)).pb(px(6.))).when(!below, |el| el.pt(px(6.)).pb(px(8.))).child(IconTabs::new("menu-tabs", tabs, self.menu_tab).on_change(move |tab, window, cx| set_tab(&tab, window, cx)));
        let fixed = if below { div().flex().flex_col().child(field).child(tabs) } else { div().flex().flex_col().child(tabs).child(field) };
        let menu = gpui_m3::Menu::new("agent-menu").width(MENU_W).max_h(px(list_h.min(MENU_H))).origin(menu_origin(below));
        let mut menu = if below { menu.header(fixed) } else { menu.footer(fixed) }
            .content_key(if filtering { 99 } else { self.menu_tab as u64 });
        // En la pestaña «Modelo», el esfuerzo va como bloque (nombre, barra
        // arrastrable y pista), no como fila con puntitos.
        let mut effort = (!filtering && self.menu_tab == 1).then(|| self.effort_block(cx)).flatten();
        for entry in self.menu_entries(&query, self.menu_tab, cx) {
            if effort.is_some() && matches!(&entry, Entry::Line(line) if matches!(line.trailing, Trailing::Effort)) {
                menu = menu.item(effort.take().unwrap());
                continue;
            }
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

    fn sub_menu(&self, sub: Sub, below: bool, list_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let back = Self::back_handler(cx);
        let mut menu = gpui_m3::Menu::new("agent-sub").width(MENU_W).max_h(px(list_h.min(MENU_H))).origin(menu_origin(below)).pin_to_anchor(true).back(sub.title(), back).content_key(100 + sub as u64);
        if sub == Sub::Model {
            let header = div().flex().flex_col().child(self.model_header(cx)).when(self.model_search_shown(), |el| el.child(div().px(px(8.)).pb(px(6.)).child(self.model_filter.clone())));
            menu = menu.header(header);
        } else if matches!(sub, Sub::Output | Sub::Agents | Sub::Commands) {
            // El filtro fijo: solo la lista se desplaza, como el buscador de modelos.
            menu = menu.header(div().px(px(8.)).pb(px(4.)).child(self.sub_filter.clone()));
        }
        for entry in self.sub_entries_with(sub, sub != Sub::Model, cx) {
            menu = self.m3_add(menu, entry, cx);
        }
        menu.into_any_element()
    }

    /// Lo que lista un submenú, para los tres estilos.
    pub(super) fn sub_entries(&self, sub: Sub, cx: &mut Context<Self>) -> Vec<Entry> {
        self.sub_entries_with(sub, true, cx)
    }

    /// Igual, pero `header: false` deja fuera el agente y el esfuerzo del submenú «Modelo»: en
    /// Expressive van fijos arriba (`model_header`) y solo la lista de modelos se desplaza.
    fn sub_entries_with(&self, sub: Sub, header: bool, cx: &mut Context<Self>) -> Vec<Entry> {
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
                if header && (!visible.is_empty() || current.is_some()) {
                    out.push(Entry::Section(crate::i18n::t("pill.code.agentMenu.agent")));
                    let fixed = self.agent_fixed();
                    let options = std::iter::once((None, "Claude Code"))
                        .chain(visible.iter().filter_map(|cli| super::acp::agent(cli)).map(|a| (Some(a.cli), a.name)));
                    let mut choices = Vec::new();
                    for (index, (cli, name)) in options.enumerate() {
                        if fixed {
                            if cli == current {
                                let mut line = Line::new(("agent", index), name);
                                line.radio = Some(true);
                                line.static_row = true;
                                line.sublabel = Some(crate::i18n::t("pill.code.agentMenu.fixedInChat").into());
                                out.push(Entry::Line(line));
                            }
                            continue;
                        }
                        let click = boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.choose_agent(cli, cx);
                            view.configs.set_last_agent(cli);
                        }));
                        choices.push((Pick { id: ("agent", index), label: name, click }, cli == current));
                    }
                    if !choices.is_empty() {
                        out.push(Entry::Choice(choices));
                    }
                    out.push(Entry::Separator);
                }
                // Un agente ACP: como Claude, el esfuerzo arriba y los modelos debajo (los
                // modos van en el menú de la píldora del modo). Antes de abrir la sesión el
                // agente todavía no dijo cuáles tiene.
                if current.is_some() {
                    self.acp_model_entries(&mut out, header, cx);
                    return out;
                }
                if header {
                    let mut effort = Line::new(("sub-effort", 0), crate::i18n::t("pill.code.agentMenu.effort"));
                    effort.icon = Some("gauge");
                    effort.static_row = true;
                    effort.inline_sub = true;
                    effort.sublabel = Some(config::effort_label(&self.chat_effort()).to_string());
                    effort.trailing = Trailing::Effort;
                    out.push(Entry::Line(effort));
                    out.push(Entry::Separator);
                }
                // El modelo de la conversación visible (puede venir como id de una
                // sesión retomada: se compara por nombre, como `isCurrent` de la referencia).
                let current = self.chat_model();
                let selected = self.current_model(&current);
                let (shown, more) = config::split_models(&self.models, selected.unwrap_or(&current));
                let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
                for index in visible {
                    let (id, name) = &self.models[index];
                    let mut line = Line::new(("model", index), name.clone());
                    line.chosen = Some(selected == Some(id.as_str()));
                    line.sublabel = if id.is_empty() { self.default_model_note() } else { self.model_info.get(id).cloned() };
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
                // De menos a más permisivo, cada uno con su descripción y su riesgo.
                for (index, (id, risk)) in config::MODE_ORDER.iter().enumerate() {
                    let mut line = Line::new(("permission", index), config::mode_name(id));
                    line.chosen = Some(config.permission_mode == *id);
                    line.sublabel = Some(config::mode_hint(id).to_string());
                    line.risk = Some(*risk);
                    let id = id.to_string();
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_config(|c| c.permission_mode = id.clone(), cx);
                        view.close_menus(cx);
                    })));
                    out.push(Entry::Line(line));
                }
            }
            Sub::Output => {
                let styles = if self.output_styles.is_empty() { vec!["default".to_string()] } else { self.output_styles.clone() };
                // Sin descripciones del sidecar: solo el nombre, con el elegido teñido.
                let query = self.sub_filter.read(cx).text().trim().to_lowercase();
                let mut shown = 0;
                for (index, style) in styles.into_iter().enumerate() {
                    let label = if style == "default" { crate::i18n::t("pill.code.agentMenu.default").to_string() } else { style.clone() };
                    if !query.is_empty() && !query.split_whitespace().all(|word| label.to_lowercase().contains(word)) {
                        continue;
                    }
                    shown += 1;
                    let mut line = Line::new(("output", index), label);
                    line.chosen = Some(config.output_style == style);
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        let style = style.clone();
                        view.set_config(|c| c.output_style = style, cx);
                        view.close_menus(cx);
                    })));
                    out.push(Entry::Line(line));
                }
                if shown == 0 {
                    out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noFilterMatch")));
                }
            }
            Sub::Agents => {
                // Sin origen en los datos (nombre y descripción): lista plana con
                // filtro; la inicial distingue las filas y la descripción va en una
                // línea con elipsis (el tooltip completo queda para gpui-m3).
                if self.agents.is_empty() {
                    out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noAgents")));
                } else {
                    let query = self.sub_filter.read(cx).text().trim().to_lowercase();
                    let mut shown = 0;
                    for (index, (name, description)) in self.agents.iter().enumerate() {
                        if !query.is_empty() {
                            let hay = format!("{name} {description}").to_lowercase();
                            if !query.split_whitespace().all(|word| hay.contains(word)) {
                                continue;
                            }
                        }
                        shown += 1;
                        let prompt = format!("Usa el subagente {name} para ");
                        let mut line = Line::new(("agent", index), name.clone());
                        line.avatar = Some(name.clone());
                        line.inline_sub = true;
                        line.sublabel = Some(description.clone());
                        line.tooltip = Some(description.clone());
                        line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            view.close_menus(cx);
                            view.insert(&prompt, window, cx);
                        })));
                        out.push(Entry::Line(line));
                    }
                    if shown == 0 {
                        out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noFilterMatch")));
                    }
                }
            }
            Sub::Commands => {
                // Solo se distingue la sesión de las skills locales (por nombre):
                // los argumentos y el resto de orígenes no llegan en los datos.
                let project = self.workspaces.active().and_then(|w| w.main().cloned());
                let skill_names: std::collections::HashSet<String> = local_skills(project.as_deref()).into_iter().map(|(name, _)| name).collect();
                let mut session = Vec::new();
                let mut skills = Vec::new();
                for (name, description) in self.command_names() {
                    if skill_names.contains(&name) {
                        skills.push((name, description));
                    } else {
                        session.push((name, description));
                    }
                }
                let query = self.sub_filter.read(cx).text();
                let trimmed = query.trim().to_lowercase();
                let keep = |name: &str, description: &str| {
                    if trimmed.is_empty() {
                        return true;
                    }
                    let hay = format!("{name} {description}").to_lowercase();
                    trimmed.split_whitespace().all(|word| hay.contains(word))
                };
                session.retain(|(name, description)| keep(name, description));
                skills.retain(|(name, description)| keep(name, description));
                if session.is_empty() && skills.is_empty() {
                    out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noFilterMatch")));
                } else {
                    let mut number = 0;
                    for (title, group) in [
                        (
                            match self.current_agent().and_then(super::acp::agent) {
                                Some(agent) => interned(agent.name),
                                None => crate::i18n::t("pill.code.agentMenu.sessionCmds"),
                            },
                            session,
                        ),
                        (crate::i18n::t("pill.code.agentMenu.localSkills"), skills),
                    ] {
                        if group.is_empty() {
                            continue;
                        }
                        out.push(Entry::Section(title));
                        for (name, description) in group {
                            let mut line = Line::new(("command", number), format!("/{name}"));
                            number += 1;
                            line.icon = Some("command");
                            line.inline_sub = true;
                            line.sublabel = Some(description);
                            if !trimmed.is_empty() {
                                line.highlights = gpui_m3::match_ranges(&line.label, trimmed.as_str());
                            }
                            line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.insert_command(&name, window, cx))));
                            out.push(Entry::Line(line));
                        }
                    }
                }
            }
            Sub::Mcp => self.mcp_entries(&mut out, cx),
            Sub::Multi => {
                out.push(Entry::Section(crate::i18n::t("pill.code.multi.hint")));
                // Los elegidos arriba, cada chip quita el suyo sin buscar su fila.
                if !self.multi_pick.is_empty() {
                    out.push(Entry::Section(tr("pill.code.multi.chosen")));
                    let mut unpicks = Vec::new();
                    for (order, (id, name)) in self.models.iter().filter(|item| self.multi_pick.contains(&item.0)).enumerate() {
                        let remove = id.clone();
                        unpicks.push((
                            Pick {
                                id: ("multi-unpick", order),
                                label: interned(name),
                                click: boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.multi_pick.remove(&remove);
                                    cx.notify();
                                })),
                            },
                            true,
                        ));
                    }
                    out.push(Entry::Choice(unpicks));
                }
                for (index, (id, name)) in self.models.iter().enumerate() {
                    let mut line = Line::new(("multi-model", index), name.clone());
                    // Casilla, no interruptor: la fila entera responde y la elegida se
                    // tiñe con su check, como la lista de modelos.
                    line.chosen = Some(self.multi_pick.contains(id));
                    line.sublabel = self.model_info.get(id).cloned();
                    let id = id.clone();
                    line.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        if !view.multi_pick.remove(&id) {
                            view.multi_pick.insert(id.clone());
                        }
                        cx.notify();
                    })));
                    out.push(Entry::Line(line));
                }
                out.push(Entry::Separator);
                let count = self.multi_pick.len();
                let mut send = Line::new(("multi-send", 0), tf("pill.code.multi.send", &[("n", &count)]));
                if count < 2 {
                    send.static_row = true;
                    send.sublabel = Some(crate::i18n::t("pill.code.multi.pickTwo").into());
                } else {
                    send.click = Some(boxed(cx.listener(|view, _: &ClickEvent, _, cx| view.send_multi(cx))));
                }
                out.push(Entry::Line(send));
            }
            Sub::Stash => {
                let total = self.stash.len();
                if total == 0 && !self.stash.has_undo() {
                    out.push(Entry::Empty(interned(&format!("{} — {}", tr("pill.code.stash.empty"), tr("pill.code.stash.emptyHint")))));
                } else {
                    let mut count = tf("pill.code.stash.count", &[("n", &total), ("max", &super::stash::MAX_ENTRIES)]);
                    if total >= super::stash::MAX_ENTRIES {
                        count = format!("{count} · {}", tr("pill.code.stash.full"));
                    }
                    out.push(Entry::Section(interned(&count)));
                    // Deshacer lo último descartado, mientras se pueda.
                    if self.stash.has_undo() {
                        let mut undo = Line::new(("stash-undo", 0), tr("pill.code.stash.undo"));
                        undo.click = Some(boxed(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.stash.restore_undo();
                            view.stash.save();
                            cx.notify();
                        })));
                        out.push(Entry::Line(undo));
                    }
                    let now = chrono::Utc::now().timestamp();
                    let mut tag_no = 0;
                    for (index, entry) in self.stash.entries().iter().enumerate() {
                        let id = entry.id;
                        let title = entry.snippet();
                        let subtitle = entry.second_line();
                        let mut tags: Vec<Tag> = Vec::new();
                        for file in &entry.files {
                            tags.push(Tag { id: ("stash-tag", tag_no), kind: TagKind::File, label: file.clone() });
                            tag_no += 1;
                        }
                        for image in &entry.images {
                            tags.push(Tag { id: ("stash-tag", tag_no), kind: TagKind::Image, label: image.name.clone() });
                            tag_no += 1;
                        }
                        for pasted in &entry.pasted {
                            if pasted.trim().is_empty() {
                                continue;
                            }
                            let lines = pasted.lines().count().max(1);
                            tags.push(Tag { id: ("stash-tag", tag_no), kind: TagKind::Pasted, label: tf("pill.code.view.pastedChip", &[("lines", &lines)]) });
                            tag_no += 1;
                        }
                        for quote in &entry.quotes {
                            let first = quote.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
                            let short: String = first.chars().take(40).collect();
                            if short.is_empty() {
                                continue;
                            }
                            tags.push(Tag { id: ("stash-tag", tag_no), kind: TagKind::Quote, label: format!("“{short}”") });
                            tag_no += 1;
                        }
                        let picks = vec![
                            Pick { id: ("stash-restore", index), label: tr("pill.code.stash.restore"), click: boxed(cx.listener(move |view, _: &ClickEvent, window, cx| view.restore_stash(id, window, cx))) },
                            Pick { id: ("stash-drop", index), label: tr("pill.code.stash.drop"), click: boxed(cx.listener(move |view, _: &ClickEvent, _, cx| view.drop_stash(id, cx))) },
                        ];
                        out.push(Entry::Block {
                            title: if title.is_empty() && !tags.is_empty() { tf("pill.code.stash.withAttachments", &[("count", &tags.len())]) } else { title },
                            subtitle: (!subtitle.is_empty()).then_some(subtitle),
                            age: stash_age(entry.age_secs(now)),
                            tags,
                            picks,
                        });
                    }
                }
            }
            Sub::Rewind => {
                use super::rewind::{rewind_scope, set_rewind_scope};
                let scope = rewind_scope();
                // Un alcance en vez de 3 chips por fila: elegir arriba, una vez.
                out.push(Entry::Section(tr("pill.code.rewind.scope")));
                let scopes = [
                    (Rewind::Code, "pill.code.agentMenu.code"),
                    (Rewind::Conversation, "pill.code.agentMenu.conversation"),
                    (Rewind::Both, "pill.code.agentMenu.both"),
                ];
                out.push(Entry::Choice(
                    scopes
                        .into_iter()
                        .map(|(what, key)| {
                            (
                                Pick {
                                    id: ("rewind-scope", what as usize),
                                    label: tr(key),
                                    click: boxed(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        set_rewind_scope(what);
                                        cx.notify();
                                    })),
                                },
                                scope == what,
                            )
                        })
                        .collect(),
                ));
                let turns = self.active_chat().map(|c| c.user_turns()).unwrap_or_default();
                if turns.is_empty() {
                    out.push(Entry::Empty(interned(&format!("{} — {}", tr("pill.code.agentMenu.noMessagesToRewind"), tr("pill.code.rewind.emptyHint")))));
                } else {
                    let count = turns.len();
                    let count = if count == 1 { tr("pill.code.rewind.oneMessage").to_string() } else { tf("pill.code.rewind.count", &[("n", &count)]) };
                    out.push(Entry::Section(interned(&count)));
                    // La hora de cada mensaje y cuántos archivos toca no existen como datos:
                    // la fila lleva solo el texto. Lo destructivo pide una segunda pulsación.
                    for (index, (uuid, text)) in turns.into_iter().enumerate() {
                        let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
                        let mut label: String = line.chars().take(70).collect();
                        if line.chars().count() > 70 {
                            label.push('…');
                        }
                        let mut row = Line::new(("rewind", index), label);
                        row.icon = Some("history");
                        if scope.destructive() {
                            let again = if scope == Rewind::Code { tr("pill.code.rewind.confirmCode") } else { tr("pill.code.rewind.confirmBoth") };
                            row.confirm = Some(again.to_string());
                        }
                        let (uuid, text) = (uuid, text.clone());
                        row.click = Some(boxed(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            view.rewind(uuid.clone(), text.clone(), rewind_scope(), window, cx);
                        })));
                        out.push(Entry::Line(row));
                    }
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
            // Sin botón en `Empty`: el camino (settings.json → mcpServers) va en el texto.
            out.push(Entry::Empty(crate::i18n::t("pill.code.agentMenu.noMcpHint")));
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
            // La causa del error y las herramientas no llegan en los datos
            // (`load_mcp` solo trae nombre y estado): punto más texto, sin inventar.
            let mut line = Line::new(("mcp", index), name.clone());
            line.static_row = true;
            line.dot = Some(color);
            line.sublabel = Some(hint.to_string());
            line.trailing = Trailing::Mcp {
                name: name.clone(),
                on: status != "disabled",
                index,
                reconnect: matches!(status.as_str(), "failed" | "needs-auth" | "pending"),
            };
            out.push(Entry::Line(line));
        }
    }

    /// Una acción sobre un servidor MCP; después se vuelve a pedir la lista.
    pub(super) fn mcp_call(&mut self, method: &str, mut params: Value, cx: &mut Context<Self>) {
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
            Act::StashDraft => self.stash_draft(window, cx),
            Act::SideChat => self.side_chat(window, cx),
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
        let agent = chat.acp.and_then(super::acp::agent).map_or("Claude", |agent| agent.name);
        let text = export_markdown(&chat.title, agent, &chat.items);
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
        self.show_toast_for(text, 3200, cx);
    }

    /// Un aviso que se queda `millis` milisegundos.
    pub(super) fn show_toast_for(&mut self, text: impl Into<SharedString>, millis: u64, cx: &mut Context<Self>) {
        self.toast_gen += 1;
        let generation = self.toast_gen;
        self.toast = Some(text.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(millis)).await;
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

/// La hora relativa de un borrador («hace 5 min»); `None` si se guardó antes de
/// anotar fecha.
fn stash_age(age_secs: Option<i64>) -> Option<String> {
    let minutes = age_secs? / 60;
    if minutes < 1 {
        Some(tr("pill.code.stash.ageNow").into())
    } else if minutes < 60 {
        Some(tf("pill.code.stash.ageMin", &[("n", &minutes)]))
    } else if minutes < 24 * 60 {
        Some(tf("pill.code.stash.ageHour", &[("n", &(minutes / 60))]))
    } else {
        Some(tf("pill.code.stash.ageDay", &[("n", &(minutes / 1440))]))
    }
}

/// Las filas que se pueden elegir con el teclado: las que hacen algo al hacer clic.
fn pickable_lines(entries: &[Entry]) -> Vec<usize> {
    entries.iter().enumerate().filter(|(_, entry)| matches!(entry, Entry::Line(line) if line.click.is_some() && !line.static_row)).map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lee_nombre_y_descripcion_de_una_skill() {
        let text = "---\nname: impeccable\ndescription: \"Mejora interfaces\"\n---\n# Cuerpo";
        assert_eq!(skill_front_matter(text), (Some("impeccable".into()), Some("Mejora interfaces".into())));
        assert_eq!(skill_front_matter("# Sin encabezado"), (None, None));
        let long = format!("---\ndescription: {}\n---", "a".repeat(200));
        assert!(skill_front_matter(&long).1.is_some_and(|d| d.ends_with('…') && d.chars().count() == 121));
    }

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
        let md = export_markdown("Mi chat", "Claude", &items);
        assert_eq!(
            md,
            "# Mi chat\n\n## Tú\n\nRevisa el README\n\n> **Read** `{\"file_path\":\"README.md\"}`\n\n> **Bash** `{\"command\":\"echo 'hola'\"}`\n\n## Claude\n\nListo.\n"
        );
        // El input se corta a 200 caracteres.
        assert!(export_markdown("Mi chat", "Codex", &items).contains("## Codex\n\nListo."));
        let long = export_markdown("t", "Claude", &[tool("Write", json!({ "content": "x".repeat(500) }))]);
        assert!(long.lines().nth(2).unwrap().chars().count() < 230);
    }
}
