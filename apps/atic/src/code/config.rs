//! La configuración de Claude de cada espacio de trabajo: el modelo, cómo pide
//! permisos, el esfuerzo y si muestra el razonamiento. Vale para las
//! conversaciones nuevas y se aplica en vivo a las abiertas. Se guarda en
//! `code-claude.json`, aparte de los espacios.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Los modos de permisos de Claude Code, con su nombre en la interfaz.
pub const MODES: [(&str, &str); 4] = [
    ("default", "Preguntar"),
    ("acceptEdits", "Editar automáticamente"),
    ("plan", "Plan"),
    ("bypassPermissions", "Omitir permisos"),
];

/// Los esfuerzos, de menos a más (los de la referencia). Sin elegir, decide Claude Code.
pub const EFFORTS: [(&str, &str); 5] = [("low", "Bajo"), ("medium", "Medio"), ("high", "Alto"), ("xhigh", "Muy alto"), ("max", "Máximo")];

/// El nombre de un esfuerzo en el idioma de ahora (`EFFORTS` guarda el de fábrica, en español).
pub fn effort_name(id: &str) -> &'static str {
    match id {
        "low" => crate::i18n::t("pill.code.config.effortLow"),
        "medium" => crate::i18n::t("pill.code.config.effortMedium"),
        "high" => crate::i18n::t("pill.code.config.effortHigh"),
        "xhigh" => crate::i18n::t("pill.code.config.effortXhigh"),
        "max" => crate::i18n::t("pill.code.config.effortMax"),
        _ => crate::i18n::t("pill.code.config.default"),
    }
}

pub fn effort_label(id: &str) -> &'static str {
    effort_name(id)
}

/// El nombre de un modo de permisos en el idioma de ahora (`MODES` guarda el de fábrica).
pub fn mode_name(id: &str) -> &'static str {
    match id {
        "acceptEdits" => crate::i18n::t("pill.code.config.modeAcceptEdits"),
        "plan" => crate::i18n::t("pill.code.config.modePlan"),
        "bypassPermissions" => crate::i18n::t("pill.code.config.modeBypass"),
        _ => crate::i18n::t("pill.code.config.modeAsk"),
    }
}

/// Lo que explica cada modo en su menú.
pub fn mode_hint(id: &str) -> &'static str {
    match id {
        "acceptEdits" => crate::i18n::t("pill.code.config.hintAcceptEdits"),
        "plan" => crate::i18n::t("pill.code.config.hintPlan"),
        "bypassPermissions" => crate::i18n::t("pill.code.config.hintBypass"),
        _ => crate::i18n::t("pill.code.config.hintAsk"),
    }
}

/// Los modelos mientras el sidecar no diga cuáles hay: los alias de Claude Code.
pub const MODELS: [(&str, &str); 4] = [("", "Predeterminado"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];

/// Los modelos a la vista y los demás (índices en `list`): de cada familia
/// («Opus 4.1», «Opus 4.5»…) solo el más nuevo, más el elegido y los que no
/// tienen versión (el predeterminado). Como `splitModels` de la referencia.
pub fn split_models(list: &[(String, String)], current: &str) -> (Vec<usize>, Vec<usize>) {
    let parse = |name: &str| -> Option<(String, f64)> {
        let mut parts = name.split_whitespace();
        let family = parts.next()?;
        let version = parts.next()?.parse().ok()?;
        parts.next().is_none().then(|| (family.to_string(), version))
    };
    let mut newest: HashMap<String, f64> = HashMap::new();
    for (_, name) in list {
        if let Some((family, version)) = parse(name) {
            let best = newest.entry(family).or_insert(version);
            *best = best.max(version);
        }
    }
    let (mut main, mut more) = (Vec::new(), Vec::new());
    for (index, (id, name)) in list.iter().enumerate() {
        let shown = match parse(name) {
            None => true,
            Some((family, version)) => newest.get(&family) == Some(&version) || id == current,
        };
        if shown { main.push(index) } else { more.push(index) }
    }
    (main, more)
}

/// El nombre de un modelo: el de la lista de Claude Code (`list`, con
/// «Default (recommended)» como «Predeterminado») o el del id
/// («claude-haiku-4-5-2025…» → «Haiku 4.5»). Como `modelName` de la referencia.
pub fn model_name(list: &[(String, String)], id: &str) -> String {
    if let Some((_, name)) = list.iter().find(|(model, _)| model == id) {
        return if name.eq_ignore_ascii_case("Default (recommended)") || name == "Predeterminado" { crate::i18n::t("pill.code.config.default").into() } else { name.clone() };
    }
    if let Some(name) = short_model(id) {
        return name;
    }
    if id == "default" {
        return crate::i18n::t("pill.code.config.default").into();
    }
    let mut chars = id.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// `claude-<familia>-<mayor>[-<menor>][-<fecha>]`, como la expresión de la referencia.
fn short_model(id: &str) -> Option<String> {
    let rest = id.strip_prefix("claude-")?;
    let mut parts = rest.split('-');
    let family = parts.next().filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_lowercase()))?;
    let major = parts.next().filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()))?;
    let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    let mut minor = None;
    let mut date = None;
    for part in parts {
        if !digits(part) {
            return None;
        }
        match (minor, date) {
            (None, None) if part.len() <= 2 => minor = Some(part),
            (_, None) if part.len() == 8 => date = Some(part),
            _ => return None,
        }
    }
    let mut name = family.to_string();
    name[..1].make_ascii_uppercase();
    Some(match minor {
        Some(minor) => format!("{name} {major}.{minor}"),
        None => format!("{name} {major}"),
    })
}

/// El mismo modelo aunque uno venga como alias del selector («opus») y otro
/// como id («claude-opus-4-5»). Como `sameModel` de la referencia.
pub fn same_model(list: &[(String, String)], a: &str, b: &str) -> bool {
    a == b || model_name(list, a) == model_name(list, b)
}

/// El modelo de `list` que corresponde a `current`: el mismo valor o, si
/// `current` no está en la lista (el id de una sesión retomada), el del mismo
/// nombre. Como `isCurrent` del submenú de modelos de la referencia.
pub fn current_model<'a>(list: &'a [(String, String)], current: &str) -> Option<&'a str> {
    list.iter()
        .find(|(id, _)| id == current)
        .or_else(|| list.iter().find(|(id, _)| same_model(list, current, id)))
        .map(|(id, _)| id.as_str())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    /// Vacío: el de Claude Code.
    pub model: String,
    pub permission_mode: String,
    pub effort: String,
    pub thinking: bool,
    pub ultracode: bool,
    pub fast_mode: bool,
    /// Cambiar de modelo cuando Claude Code marca un mensaje.
    pub switch_model_on_flag: bool,
    /// «default» o el nombre de un estilo de salida.
    pub output_style: String,
    pub sandbox: bool,
    /// Claude in Chrome: se fija al abrir la sesión.
    pub chrome: bool,
    pub remote_control: bool,
    /// Compactar una conversación larga que quedó inactiva justo antes de que
    /// venza el caché de prompts (beta): retomarla sale más barato.
    pub compact_idle: bool,
    /// Con un turno en curso, Enter manda el mensaje dentro de ese turno (dirigirlo) en
    /// vez de dejarlo en la cola. Alt+Enter hace lo contrario.
    pub follow_up_steer: bool,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            permission_mode: "default".into(),
            effort: String::new(),
            thinking: false,
            ultracode: false,
            fast_mode: false,
            switch_model_on_flag: true,
            output_style: "default".into(),
            sandbox: false,
            chrome: false,
            remote_control: false,
            compact_idle: false,
            follow_up_steer: true,
        }
    }
}

impl ClaudeConfig {
    pub fn mode_label(&self) -> &'static str {
        mode_name(&self.permission_mode)
    }

    /// Lo que se suma a `start` para abrir una sesión con esta configuración.
    pub fn start_params(&self, params: &mut Value) {
        if !self.model.is_empty() {
            params["model"] = json!(self.model);
        }
        if !self.effort.is_empty() {
            params["effort"] = json!(self.effort);
        }
        params["permissionMode"] = json!(self.permission_mode);
        params["thinking"] = json!(self.thinking);
        params["chrome"] = json!(self.chrome);
        params["remoteControl"] = json!(self.remote_control);
        params["flags"] = self.flags(None);
    }

    /// Los ajustes de Claude Code que van por `applyFlags`, como en la referencia: todos
    /// (al abrir una sesión) o solo los que cambiaron respecto de `before`. El
    /// esfuerzo no: es de cada conversación (`sync_chat_settings`).
    pub fn flags(&self, before: Option<&ClaudeConfig>) -> Value {
        let mut flags = serde_json::Map::new();
        let changed = |pick: fn(&ClaudeConfig) -> Value| before.is_none_or(|b| pick(b) != pick(self));
        if changed(|c| json!(c.thinking)) {
            flags.insert("alwaysThinkingEnabled".into(), json!(self.thinking));
        }
        if changed(|c| json!(c.ultracode)) {
            flags.insert("ultracode".into(), json!(self.ultracode));
        }
        if changed(|c| json!(c.fast_mode)) {
            flags.insert("fastMode".into(), json!(self.fast_mode));
        }
        if changed(|c| json!(c.switch_model_on_flag)) {
            flags.insert("switchModelsOnFlag".into(), json!(self.switch_model_on_flag));
        }
        if changed(|c| json!(c.output_style)) {
            flags.insert("outputStyle".into(), json!(self.output_style));
        }
        if changed(|c| json!(c.sandbox)) && (before.is_some() || self.sandbox) {
            flags.insert("sandbox".into(), json!({ "enabled": self.sandbox }));
        }
        Value::Object(flags)
    }
}

/// El ancho de lectura por defecto de Atic Code (el del composer de siempre).
fn default_chat_width() -> f32 {
    super::view::THREAD_W
}

/// Los anchos que ofrece Apariencia (0 = todo el ancho, para el ajuste «Ancho del chat»).
pub const CHAT_WIDTHS: [f32; 5] = [0., 680., 860., 1040., 1220.];

/// El ancho de la lista que corresponde al guardado: el igual o, si no, el más cercano.
pub fn chat_width_index(current: f32) -> usize {
    if current <= 0. {
        return 0;
    }
    CHAT_WIDTHS
        .iter()
        .enumerate()
        .skip(1)
        .min_by(|(_, a), (_, b)| (current - **a).abs().partial_cmp(&(current - **b).abs()).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// La letra de Atic Code (Apariencia): Inter por defecto, como t3code; Roboto, la de
/// Material 3; o la del sistema (Segoe UI en Windows, San Francisco en macOS).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeFont {
    #[default]
    Inter,
    Roboto,
    System,
}

impl CodeFont {
    pub const ALL: [CodeFont; 3] = [CodeFont::Inter, CodeFont::Roboto, CodeFont::System];

    pub fn family(self) -> &'static str {
        match self {
            CodeFont::Inter => gpui_m3::theme::FONT_FAMILY,
            CodeFont::Roboto => gpui_m3::theme::ROBOTO_FONT_FAMILY,
            CodeFont::System => crate::fonts::UI_TEXT,
        }
    }

    /// Su nombre en la Apariencia (Inter y Roboto se escriben igual en todo idioma).
    pub fn label(self) -> String {
        match self {
            CodeFont::Inter => "Inter".into(),
            CodeFont::Roboto => "Roboto".into(),
            CodeFont::System => crate::i18n::t("pill.code.config.fontSystem").into(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Configs {
    by_workspace: HashMap<u64, ClaudeConfig>,
    /// El último modo de permisos elegido: sigue al usuario entre proyectos, así una
    /// conversación nueva en otro proyecto no vuelve a «Plan» si venía en «Omitir permisos».
    #[serde(default)]
    permission_mode: Option<String>,
    /// La letra de toda la ventana (`CodeFont`).
    #[serde(default)]
    pub font: CodeFont,
    /// El estilo y el modo de la ventana: de la app, no de cada proyecto.
    #[serde(default)]
    pub style: super::style::Style,
    #[serde(default)]
    pub mode: super::style::Mode,
    /// La opacidad de los fondos en Liquid Glass (0–100); sin valor, la de fábrica.
    #[serde(default)]
    pub glass_opacity: Option<u8>,
    /// La línea de estado bajo la caja de texto (modelo y último turno).
    #[serde(default)]
    pub status_line: bool,
    /// Los chips de la fila bajo la caja que el usuario escondió (`STATUS_CHIPS`); sin
    /// entrada, se ven todos.
    #[serde(default)]
    pub status_hidden: Vec<String>,
    /// Los espacios favoritos: van primero en la barra.
    #[serde(default)]
    pub favorites: Vec<u64>,
    /// El orden que dejó el usuario al arrastrar los espacios en la barra. Va
    /// aquí y no en `space-workspaces.json` (la lista de proyectos, con sus carpetas).
    #[serde(default)]
    pub order: Vec<u64>,
    /// Los espacios que ya se abrieron alguna vez: al abrir uno por primera
    /// vez se despliega en la barra (`expandOnOpen` de la referencia).
    #[serde(default)]
    pub opened: Vec<u64>,
    /// El color de acento de cada espacio, por estilo (`#rrggbb`), como los
    /// acentos de proyecto de la referencia. Sin entrada, el del estilo.
    #[serde(default)]
    pub accents: HashMap<u64, HashMap<String, String>>,
    /// El alto de la terminal integrada, como lo dejó el usuario al arrastrar.
    #[serde(default)]
    pub terminal_height: Option<f32>,
    /// Su ancho cuando va a la derecha.
    #[serde(default)]
    pub terminal_width: Option<f32>,
    /// El ancho del panel derecho (Cambios y Archivos) y el del visor de un archivo,
    /// como los dejó el usuario al arrastrar su borde.
    #[serde(default)]
    pub panel_width: Option<f32>,
    #[serde(default)]
    pub doc_width: Option<f32>,
    /// El ancho de la barra lateral, igual en los tres estilos.
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    /// La barra como una sola lista de conversaciones (cada una con su proyecto arriba) en
    /// vez de carpetas por proyecto.
    #[serde(default)]
    pub sidebar_list: bool,
    /// Los agentes que el usuario ocultó (por su CLI): no se ofrecen en el chat ni en la
    /// terminal.
    #[serde(default)]
    pub hidden_agents: Vec<String>,
    /// Si va bajo el chat o a su derecha.
    #[serde(default)]
    pub terminal_dock: super::terminal::Dock,
    /// El tamaño de su letra (el zoom); sin valor, el de fábrica.
    #[serde(default)]
    pub terminal_font: Option<f32>,
    /// El ancho máximo del hilo y la caja de texto (0 = todo el ancho). Sin valor
    /// guardado, el de siempre (`view::THREAD_W`).
    #[serde(default = "default_chat_width")]
    pub chat_width: f32,
    /// Los atajos que el usuario cambió (id → tecla); sin entrada, el de fábrica
    /// (`shortcuts::TABLE`).
    #[serde(default)]
    pub shortcuts: HashMap<String, String>,
    /// Cómo avisa una conversación que terminó, falló o pide algo mientras no se mira.
    #[serde(default)]
    pub notify: NotifyMode,
    /// Esconde del chat las herramientas que terminaron bien (quedan los errores, los
    /// subagentes y lo que pide algo).
    #[serde(default)]
    pub hide_ok_tools: bool,
    /// Quien delega decide por sus subagentes: se abren en un modo que no pregunta y lo que
    /// igual pregunten lo contesta él y no el usuario. Para dejar a los agentes trabajando
    /// solos; apagado, los permisos de un subagente son del usuario.
    #[serde(default)]
    pub orchestrator_decides: bool,
    /// El agente de la última conversación que se empezó (`claude` o el CLI de uno ACP): una
    /// nueva parte con él, y no siempre con Claude.
    #[serde(default)]
    pub last_agent: Option<String>,
    /// Lo último elegido en cada agente ACP (`AcpConfig::wanted`: modelo, esfuerzo, modo),
    /// para que una conversación nueva con él lo herede como hereda lo suyo Claude.
    #[serde(default)]
    pub acp_choices: HashMap<String, Value>,
    /// Qué pasa al elegir otra conversación con la vista dividida (`split.rs`). Una
    /// configuración de antes, sin el campo, queda con lo de fábrica: abrirla sola.
    #[serde(default)]
    pub split_pick: SplitPick,
}

/// Al elegir una conversación con la vista dividida (Configuración → General).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitPick {
    /// Se abre sola y los paneles quedan guardados como un grupo en la barra.
    #[default]
    Alone,
    /// Va al panel enfocado (como antes); al arrastrar vuelve la zona de reemplazo.
    Replace,
}

impl SplitPick {
    pub const ALL: [SplitPick; 2] = [SplitPick::Alone, SplitPick::Replace];
}

/// Los chips de la fila bajo la caja que se pueden esconder (Apariencia), con su nombre.
pub const STATUS_CHIPS: [(&str, &str); 5] = [
    ("agents", "pill.code.settingsM3.chipAgents"),
    ("remote", "pill.code.settingsM3.chipRemote"),
    ("usage", "pill.code.settingsM3.chipUsage"),
    ("branch", "pill.code.settingsM3.chipBranch"),
    ("duration", "pill.code.settingsM3.chipDuration"),
];

/// Cómo avisa Atic Code (`notify.rs`) cuando no se está mirando la conversación.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyMode {
    /// El aviso del sistema, como siempre.
    #[default]
    System,
    /// Solo un sonido.
    Sound,
    /// El aviso y el sonido.
    Both,
    /// Nada.
    Off,
}

impl NotifyMode {
    pub const ALL: [NotifyMode; 4] = [NotifyMode::System, NotifyMode::Sound, NotifyMode::Both, NotifyMode::Off];
}

/// Los espacios en el orden de la barra: los favoritos primero y, dentro de
/// cada grupo, el que dejó el usuario al arrastrar (`order`); los que no
/// están en `order` (nuevos) van al final del grupo, en el orden de la lista
/// (como `sidebarProjects` de la referencia).
pub fn sidebar_order(ids: &[u64], favorites: &[u64], order: &[u64]) -> Vec<u64> {
    let mut sorted = ids.to_vec();
    // El sort es estable: los que no tienen lugar guardado conservan el orden de la lista.
    sorted.sort_by_key(|id| order.iter().position(|o| o == id).unwrap_or(usize::MAX));
    let (mut first, rest): (Vec<u64>, Vec<u64>) = sorted.into_iter().partition(|id| favorites.contains(id));
    first.extend(rest);
    first
}

impl Configs {
    pub fn load() -> Self {
        let mut configs: Self = crate::paths::file("code-claude.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| {
                // Instalación nueva: el ancho de lectura de fábrica, no el `Default` de
                // la struct (que deja 0 = «Completo»).
                Self { chat_width: default_chat_width(), ..Self::default() }
            });
        // Solo Material 3 Expressive (`appearance::ONLY_EXPRESSIVE`).
        if crate::appearance::ONLY_EXPRESSIVE {
            configs.style = super::style::Style::Expressive;
        }
        // El modo claro u oscuro es el de «Tema de las ventanas» (`theme::code_mode`).
        if let Some(mode) = crate::theme::code_mode() {
            configs.mode = mode;
        }
        // Para revisar con capturas (`CODE_OPEN` en mod.rs): otro estilo o modo sin tocar
        // lo guardado.
        match std::env::var("CODE_STYLE").as_deref() {
            Ok("formal") => configs.style = super::style::Style::Formal,
            Ok("expressive") => configs.style = super::style::Style::Expressive,
            Ok("glass") => configs.style = super::style::Style::Glass,
            _ => {}
        }
        match std::env::var("CODE_MODE").as_deref() {
            Ok("light") => configs.mode = super::style::Mode::Light,
            Ok("dark") => configs.mode = super::style::Mode::Dark,
            _ => {}
        }
        // Para capturas: otra letra sin tocar lo guardado (como `CODE_STYLE`).
        match std::env::var("CODE_FONT").as_deref() {
            Ok("inter") => configs.font = CodeFont::Inter,
            Ok("roboto") => configs.font = CodeFont::Roboto,
            Ok("system") => configs.font = CodeFont::System,
            _ => {}
        }
        // Los componentes de gpui-m3 dibujan con la letra elegida desde el primer cuadro.
        gpui_m3::theme::set_font_family(configs.font.family());
        configs
    }

    pub fn save(&self) {
        // Una ventana abierta para capturas no guarda nada (ni el estilo de prueba).
        if std::env::var_os("CODE_OPEN").is_some() {
            return;
        }
        let Some(path) = crate::paths::file("code-claude.json") else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "atic code: no se guardó la configuración de Claude");
                }
            }
            Err(error) => tracing::warn!(%error, "atic code: configuración sin serializar"),
        }
    }

    pub fn get(&self, workspace: u64) -> ClaudeConfig {
        let mut config = self.by_workspace.get(&workspace).cloned().unwrap_or_default();
        if let Some(mode) = &self.permission_mode {
            config.permission_mode = mode.clone();
        }
        config
    }

    /// El acento propio del espacio en este estilo.
    pub fn accent(&self, workspace: u64, style: super::style::Style) -> Option<u32> {
        self.accents.get(&workspace)?.get(style.key()).and_then(|hex| super::style::parse_hex(hex))
    }

    /// El acento que se ve: el del espacio en el estilo de ahora.
    pub fn active_accent(&self, workspace: Option<u64>) -> Option<u32> {
        self.accent(workspace?, self.style)
    }

    /// Fija (o con `None` quita) el acento del espacio en este estilo.
    pub fn set_accent(&mut self, workspace: u64, style: super::style::Style, color: Option<u32>) {
        match color {
            Some(rgb) => {
                self.accents.entry(workspace).or_default().insert(style.key().to_string(), super::style::to_hex(rgb));
            }
            None => {
                if let Some(map) = self.accents.get_mut(&workspace) {
                    map.remove(style.key());
                    if map.is_empty() {
                        self.accents.remove(&workspace);
                    }
                }
            }
        }
        self.save();
    }

    pub fn toggle_favorite(&mut self, workspace: u64) {
        match self.favorites.iter().position(|id| *id == workspace) {
            Some(index) => {
                self.favorites.remove(index);
            }
            None => self.favorites.push(workspace),
        }
        self.save();
    }

    /// Cambia la barra entre carpetas por proyecto y una sola lista.
    pub fn set_font(&mut self, font: CodeFont) {
        self.font = font;
        gpui_m3::theme::set_font_family(font.family());
        self.save();
    }

    /// ¿Se ve el chip `id` de la fila bajo la caja?
    pub fn status_shown(&self, id: &str) -> bool {
        !self.status_hidden.iter().any(|h| h == id)
    }

    /// Muestra o esconde el chip `id` de la fila bajo la caja.
    pub fn set_status_shown(&mut self, id: &str, shown: bool) {
        self.status_hidden.retain(|h| h != id);
        if !shown {
            self.status_hidden.push(id.to_string());
        }
    }

    pub fn set_sidebar_list(&mut self, list: bool) {
        self.sidebar_list = list;
        self.save();
    }

    pub fn set_terminal_height(&mut self, height: f32) {
        self.terminal_height = Some(height.round());
        self.save();
    }

    /// Muestra u oculta un agente en el chat y la terminal.
    pub fn set_agent_hidden(&mut self, cli: &str, hidden: bool) {
        self.hidden_agents.retain(|c| c != cli);
        if hidden {
            self.hidden_agents.push(cli.to_string());
        }
        self.save();
    }

    /// El agente elegido para empezar (`None` es Claude).
    pub fn set_last_agent(&mut self, cli: Option<&str>) {
        let cli = cli.unwrap_or("claude");
        if self.last_agent.as_deref() != Some(cli) {
            self.last_agent = Some(cli.to_string());
            self.save();
        }
    }

    /// Guarda lo elegido en un agente ACP (su `wanted`).
    pub fn set_acp_choices(&mut self, cli: &str, wanted: Value) {
        if self.acp_choices.get(cli) != Some(&wanted) {
            self.acp_choices.insert(cli.to_string(), wanted);
            self.save();
        }
    }

    pub fn agent_hidden(&self, cli: &str) -> bool {
        self.hidden_agents.iter().any(|c| c == cli)
    }

    pub fn set_terminal_width(&mut self, width: f32) {
        self.terminal_width = Some(width.round());
        self.save();
    }

    pub fn set_terminal_dock(&mut self, dock: super::terminal::Dock) {
        self.terminal_dock = dock;
        self.save();
    }

    pub fn set_terminal_font(&mut self, size: f32) {
        self.terminal_font = Some(size);
        self.save();
    }

    /// Guarda el orden de la barra tras arrastrar un espacio.
    pub fn set_order(&mut self, order: Vec<u64>) {
        self.order = order;
        self.save();
    }

    /// La primera vez que se abre un espacio devuelve `true` y lo anota.
    pub fn first_open(&mut self, workspace: u64) -> bool {
        if self.opened.contains(&workspace) {
            return false;
        }
        self.opened.push(workspace);
        self.save();
        true
    }

    pub fn set(&mut self, workspace: u64, config: ClaudeConfig) {
        self.permission_mode = Some(config.permission_mode.clone());
        self.by_workspace.insert(workspace, config);
        self.save();
    }
}

/// Tapa a medias los correos de `text`: se ven las dos primeras letras y el dominio
/// (`an•••@example.com`), lo justo para reconocer la cuenta en una captura.
pub fn mask_emails(text: &str) -> String {
    text.split(' ')
        .map(|word| match word.find('@') {
            Some(at) if at > 0 && word[at + 1..].contains('.') => {
                let shown: String = word[..at].chars().take(2).collect();
                format!("{shown}•••{}", &word[at..])
            }
            _ => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_modo_de_permisos_sigue_en_otro_proyecto() {
        let mut configs = Configs::default();
        configs.by_workspace.insert(1, ClaudeConfig { permission_mode: "plan".into(), ..Default::default() });
        assert_eq!(configs.get(1).permission_mode, "plan");
        configs.permission_mode = Some("bypassPermissions".into());
        assert_eq!(configs.get(1).permission_mode, "bypassPermissions");
        assert_eq!(configs.get(7).permission_mode, "bypassPermissions");
    }

    #[test]
    fn la_vista_dividida_abre_sola_salvo_que_se_elija_otra_cosa() {
        // Una configuración de antes, sin el campo: abrir sola.
        let old: Configs = serde_json::from_str(r#"{ "by_workspace": {}, "sidebar_list": true }"#).unwrap();
        assert_eq!(old.split_pick, SplitPick::Alone);
        assert!(old.sidebar_list);
        let replace: Configs = serde_json::from_str(r#"{ "by_workspace": {}, "split_pick": "replace" }"#).unwrap();
        assert_eq!(replace.split_pick, SplitPick::Replace);
        // Y se guarda con el nombre que se lee.
        let saved = serde_json::to_value(&replace).unwrap();
        assert_eq!(saved["split_pick"], "replace");
        assert_eq!(Configs::default().split_pick, SplitPick::Alone);
    }

    #[test]
    fn el_indice_del_ancho_toma_el_mas_cercano() {
        assert_eq!(chat_width_index(0.), 0);
        assert_eq!(chat_width_index(-5.), 0);
        assert_eq!(chat_width_index(700.), 1);
        assert_eq!(chat_width_index(840.), 2);
        assert_eq!(chat_width_index(860.), 2);
        assert_eq!(chat_width_index(2000.), 4);
    }

    #[test]
    fn lo_predeterminado_no_fuerza_modelo_ni_esfuerzo() {
        let mut params = json!({ "key": "c1" });
        ClaudeConfig::default().start_params(&mut params);
        assert_eq!(params["permissionMode"], "default");
        assert!(params.get("model").is_none() && params.get("effort").is_none());

        let config = ClaudeConfig { model: "opus".into(), effort: "high".into(), permission_mode: "plan".into(), thinking: true, ..Default::default() };
        let mut params = json!({});
        config.start_params(&mut params);
        assert_eq!((&params["model"], &params["effort"], &params["thinking"]), (&json!("opus"), &json!("high"), &json!(true)));
        assert_eq!(config.mode_label(), "Plan");
    }

    #[test]
    fn en_vivo_solo_van_los_ajustes_que_cambiaron() {
        let before = ClaudeConfig::default();
        let after = ClaudeConfig { effort: "max".into(), sandbox: true, ..before.clone() };
        // El esfuerzo no se difunde: va a la conversación visible.
        assert_eq!(after.flags(Some(&before)), json!({ "sandbox": { "enabled": true } }));
        // Al abrir, todos menos el sandbox apagado y el esfuerzo (que va aparte).
        let all = before.flags(None);
        assert!(all.get("sandbox").is_none() && all.get("effortLevel").is_none());
        assert_eq!(all["outputStyle"], "default");
    }

    #[test]
    fn cambiar_de_modelo_al_marcar_llega_a_claude_code() {
        // Va al abrir la sesión y, si cambia, en vivo (`switchModelsOnFlag`).
        let before = ClaudeConfig::default();
        assert_eq!(before.flags(None)["switchModelsOnFlag"], true);
        let after = ClaudeConfig { switch_model_on_flag: false, ..before.clone() };
        assert_eq!(after.flags(Some(&before)), json!({ "switchModelsOnFlag": false }));
    }

    #[test]
    fn los_correos_se_tapan_a_medias() {
        assert_eq!(mask_emails("ana@example.com"), "an•••@example.com");
        assert_eq!(mask_emails("ana@example.com's Organization"), "an•••@example.com's Organization");
        assert_eq!(mask_emails("Claude Max"), "Claude Max");
        assert_eq!(mask_emails("a@b"), "a@b");
    }

    #[test]
    fn los_favoritos_van_primero_sin_perder_el_orden() {
        assert_eq!(sidebar_order(&[1, 2, 3, 4], &[], &[]), vec![1, 2, 3, 4]);
        assert_eq!(sidebar_order(&[1, 2, 3, 4], &[3], &[]), vec![3, 1, 2, 4]);
        // Entre favoritos manda el orden de la lista, no el de marcarlos.
        assert_eq!(sidebar_order(&[1, 2, 3, 4], &[4, 2], &[]), vec![2, 4, 1, 3]);
        // Un favorito que ya no existe no aparece.
        assert_eq!(sidebar_order(&[1, 2], &[9, 2], &[]), vec![2, 1]);
    }

    #[test]
    fn el_orden_guardado_manda_dentro_de_cada_grupo() {
        // Sin favoritos, el orden del usuario.
        assert_eq!(sidebar_order(&[1, 2, 3, 4], &[], &[4, 3, 2, 1]), vec![4, 3, 2, 1]);
        // Los favoritos siguen primero y entre ellos manda el orden guardado.
        assert_eq!(sidebar_order(&[1, 2, 3, 4], &[1, 3], &[4, 3, 2, 1]), vec![3, 1, 4, 2]);
        // Un espacio nuevo (sin lugar guardado) va al final de su grupo; uno que ya no existe se ignora.
        assert_eq!(sidebar_order(&[1, 2, 3, 5], &[], &[9, 3, 2, 1]), vec![3, 2, 1, 5]);
        assert_eq!(sidebar_order(&[1, 2, 3, 5, 6], &[6], &[3, 2, 1]), vec![6, 3, 2, 1, 5]);
    }

    #[test]
    fn de_cada_familia_se_ve_el_modelo_mas_nuevo() {
        let list: Vec<(String, String)> = [("", "Predeterminado"), ("opus-41", "Opus 4.1"), ("opus-45", "Opus 4.5"), ("sonnet-45", "Sonnet 4.5"), ("sonnet-4", "Sonnet 4")]
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        assert_eq!(split_models(&list, ""), (vec![0, 2, 3], vec![1, 4]));
        // El elegido se ve aunque sea viejo.
        assert_eq!(split_models(&list, "opus-41"), (vec![0, 1, 2, 3], vec![4]));
    }

    fn models(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
    }

    #[test]
    fn el_nombre_del_modelo_sale_de_la_lista_o_del_id() {
        let list = models(&[("", "Default (recommended)"), ("opus", "Opus 4.5")]);
        assert_eq!(model_name(&list, ""), "Predeterminado");
        assert_eq!(model_name(&list, "opus"), "Opus 4.5");
        assert_eq!(model_name(&[], "claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(model_name(&[], "claude-opus-4-20250514"), "Opus 4");
        assert_eq!(model_name(&[], "claude-sonnet-4-5"), "Sonnet 4.5");
        assert_eq!(model_name(&[], "default"), "Predeterminado");
        assert_eq!(model_name(&[], "sonnet"), "Sonnet");
        // Lo que no es un id de Claude se deja como viene, con mayúscula.
        assert_eq!(model_name(&[], "claude-opus-4-5[1m]"), "Claude-opus-4-5[1m]");
    }

    #[test]
    fn el_submenu_marca_el_alias_del_modelo_retomado() {
        let list = models(&[("", "Predeterminado"), ("opus", "Opus 4.5"), ("sonnet", "Sonnet 4.5")]);
        assert_eq!(current_model(&list, "sonnet"), Some("sonnet"));
        assert_eq!(current_model(&list, ""), Some(""));
        assert_eq!(current_model(&list, "claude-opus-4-5-20251101"), Some("opus"));
        assert_eq!(current_model(&list, "claude-haiku-4-5"), None);
    }

    #[test]
    fn el_alias_y_el_id_son_el_mismo_modelo() {
        let list = models(&[("", "Predeterminado"), ("opus", "Opus 4.5"), ("sonnet", "Sonnet 4.5")]);
        assert!(same_model(&list, "opus", "claude-opus-4-5-20251101"));
        assert!(same_model(&list, "opus", "opus"));
        assert!(!same_model(&list, "sonnet", "claude-opus-4-5"));
        assert!(!same_model(&list, "", "opus"));
        // Sin lista, el alias no tiene versión y no se confunde con un id.
        assert!(!same_model(&[], "opus", "claude-opus-4-5"));
    }
}
