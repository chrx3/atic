//! La paleta de comandos (Ctrl+K), como la de la referencia: acciones de la app, las
//! conversaciones del proyecto, el estilo y el modo de color.

use crate::i18n::{t, tf};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use gpui::{AppContext, Context, Window};
use gpui_m3::{Command, CommandPalette, CommandPaletteEvent};

use super::shortcuts;
use super::style::{Mode, Style};
use super::{CodeView, SessionInfo, Side};

/// Lo que ejecuta cada comando de la paleta.
#[derive(Clone)]
pub enum PaletteAct {
    NewConversation,
    NewLoose,
    NewSpace,
    OpenFolder,
    ImportVscode,
    History,
    Settings,
    Changes,
    Files,
    /// Una pestaña de archivo del editor que ya está abierta.
    Tab(PathBuf),
    Session(u64, SessionInfo),
    Style(Style),
    Mode(Mode),
    /// Entrar o salir de pantalla completa (macOS: si no, el semáforo verde es la única salida).
    FullScreen,
    /// Una conversación nueva con la guía para investigar un problema de Atic.
    Diagnose,
    /// Una rama de la conversación de ahora, en un panel aparte.
    SideChat,
    /// Ajustes abiertos en una pestaña (`settings_search::*_TAB`).
    SettingsTab(usize),
    /// Mostrar u ocultar la terminal (Ctrl+T).
    Terminal,
}

/// Las 3 últimas elecciones de la paleta, en memoria: con la caja vacía salen
/// primero para reentrar sin escribir (punto 3 de la propuesta; la cabecera
/// «Recientes» pide secciones en gpui-m3, ver el informe).
static RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());
const MAX_RECENT: usize = 3;

/// La clave estable de una elección (las pestañas por su ruta, las sesiones
/// por proyecto e id: los títulos se repiten).
fn recent_key(act: &PaletteAct) -> String {
    match act {
        PaletteAct::NewConversation => "new-conversation".into(),
        PaletteAct::NewLoose => "new-loose".into(),
        PaletteAct::NewSpace => "new-space".into(),
        PaletteAct::OpenFolder => "open-folder".into(),
        PaletteAct::ImportVscode => "import-vscode".into(),
        PaletteAct::History => "history".into(),
        PaletteAct::Settings => "settings".into(),
        PaletteAct::Changes => "changes".into(),
        PaletteAct::Files => "files".into(),
        PaletteAct::Tab(path) => format!("tab:{}", path.display()),
        PaletteAct::Session(workspace, info) => format!("session:{workspace}:{}", info.session_id),
        PaletteAct::Style(style) => format!("style:{style:?}"),
        PaletteAct::Mode(mode) => format!("mode:{mode:?}"),
        PaletteAct::FullScreen => "fullscreen".into(),
        PaletteAct::Diagnose => "diagnose".into(),
        PaletteAct::SideChat => "side-chat".into(),
        PaletteAct::SettingsTab(tab) => format!("settings-tab:{tab}"),
        PaletteAct::Terminal => "terminal".into(),
    }
}

/// Guarda una elección al frente de los recientes (sin repetidos).
fn remember(act: &PaletteAct) {
    let key = recent_key(act);
    let Ok(mut recent) = RECENT.lock() else {
        return;
    };
    recent.retain(|entry| *entry != key);
    recent.insert(0, key);
    recent.truncate(MAX_RECENT);
}

/// El comando con el atajo vigente del usuario a la derecha (punto 2 de la
/// propuesta: respeta lo cambiado en Ajustes › Atajos, y ⌘ en macOS). Las
/// filas que no son acciones llevan la insignia de su clase («Pestaña»,
/// «Conversación») en vez de atajo, como pide la propuesta.
fn with_shortcut(command: Command, keys: &HashMap<&str, String>, id: &str) -> Command {
    match keys.get(id) {
        Some(key) => command.hint(shortcuts::label(key)),
        None => command,
    }
}

pub(super) fn new_palette(cx: &mut Context<CodeView>) -> gpui::Entity<CommandPalette> {
    let palette = cx.new(|cx| CommandPalette::new(cx)
            .placeholder(t("pill.code.palette.placeholder"), cx)
            .empty_text(t("overlay.noResults"))
            .clear_label(t("pill.code.project.clearFilter"))
            .key_hints([("↑↓".into(), t("pill.code.palette.hintMove").into()), ("Enter".into(), t("pill.code.palette.hintRun").into()), ("Esc".into(), t("pill.code.palette.hintClose").into())]),
    );
    cx.subscribe(&palette, |view: &mut CodeView, _, event: &CommandPaletteEvent, cx| {
        view.palette_open = false;
        if let CommandPaletteEvent::Run(index) = event {
            if let Some(act) = view.palette_acts.get(*index).cloned() {
                view.pending_palette = Some(act);
            }
        }
        cx.notify();
    })
    .detach();
    palette
}

impl CodeView {
    /// Abre la paleta con la lista de ahora, agrupada como la propuesta
    /// (`target/menus/paleta.html`, punto 1): acciones, conversaciones
    /// (pestañas y sesiones) y apariencia. `CommandPalette` (gpui-m3) no
    /// dibuja cabeceras de sección (ver el informe): el orden es lo que agrupa.
    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // El atajo vigente de cada acción, una sola vez por apertura.
        let keys: HashMap<&str, String> = shortcuts::effective(&self.configs.shortcuts)
            .into_iter()
            .map(|(row, key)| (row.id, key))
            .collect();
        // Acciones de la app.
        let mut commands = vec![
            (with_shortcut(Command::new("plus", t("pill.code.palette.newConversation")), &keys, "new_conversation"), PaletteAct::NewConversation),
            (Command::new("chat", t("pill.code.palette.newLoose")), PaletteAct::NewLoose),
            (Command::new("folder-plus", t("pill.code.palette.newSpace")), PaletteAct::NewSpace),
            (with_shortcut(Command::new("folder", t("pill.code.palette.openFolder")), &keys, "open_folder"), PaletteAct::OpenFolder),
            (Command::new("import", t("pill.code.palette.importVscode")), PaletteAct::ImportVscode),
            (Command::new("history", t("pill.code.palette.history")), PaletteAct::History),
            (with_shortcut(Command::new("gear", t("pill.code.palette.settings")), &keys, "settings"), PaletteAct::Settings),
            (Command::new("refresh", t("pill.code.palette.updateClaude")), PaletteAct::SettingsTab(super::settings_search::CLAUDE_TAB)),
            (with_shortcut(Command::new("command", t("pill.code.palette.shortcuts")), &keys, "show_shortcuts"), PaletteAct::SettingsTab(super::settings_search::SHORTCUTS_TAB)),
            (with_shortcut(Command::new("terminal", t("pill.code.palette.terminal")), &keys, "toggle_terminal_t"), PaletteAct::Terminal),
            (Command::new("smartphone", t("pill.code.palette.phone")), PaletteAct::SettingsTab(super::settings_search::PHONE_TAB)),
            (Command::new("gauge", t("pill.code.palette.diagnose")), PaletteAct::Diagnose),
            (Command::new("chat", t("pill.code.side.action")), PaletteAct::SideChat),
        ];
        if let Some(workspace) = self.workspaces.active_id() {
            commands.push((with_shortcut(Command::new("diff", t("pill.code.palette.viewChanges")), &keys, "show_changes"), PaletteAct::Changes));
            commands.push((with_shortcut(Command::new("files", t("pill.code.palette.viewFiles")), &keys, "show_files"), PaletteAct::Files));
            if cfg!(target_os = "macos") {
                let label = if window.is_fullscreen() { t("pill.code.palette.exitFullscreen") } else { t("pill.code.palette.enterFullscreen") };
                commands.push((Command::new("monitor", label).hint("⌃⌘F"), PaletteAct::FullScreen));
            }
            // Las pestañas abiertas del editor, como en la referencia (`Overlays.tsx:513`).
            for tab in &self.tabs.files {
                let name = tab.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| tab.path.display().to_string());
                commands.push((Command::new("file", tf("pill.code.palette.openFile", &[("name", &name)])).hint(t("pill.code.palette.tab")), PaletteAct::Tab(tab.path.clone())));
            }
            for info in self.history.get(&workspace).into_iter().flatten().take(30) {
                commands.push((Command::new("spark", info.title.clone()).hint(t("pill.code.palette.conversation")), PaletteAct::Session(workspace, info.clone())));
            }
        } else if cfg!(target_os = "macos") {
            let label = if window.is_fullscreen() { t("pill.code.palette.exitFullscreen") } else { t("pill.code.palette.enterFullscreen") };
            commands.push((Command::new("monitor", label).hint("⌃⌘F"), PaletteAct::FullScreen));
        }
        for (style, label) in Style::ALL.into_iter().filter(|_| !crate::appearance::ONLY_EXPRESSIVE) {
            commands.push((Command::new("palette", tf("pill.code.palette.style", &[("label", &label)])), PaletteAct::Style(style)));
        }
        for (mode, icon, label) in [(Mode::Light, "sun", t("settings.appearance.light")), (Mode::Dark, "moon", t("settings.appearance.dark")), (Mode::System, "monitor", t("settings.appearance.system"))] {
            commands.push((Command::new(icon, tf("pill.code.palette.mode", &[("label", &label)])), PaletteAct::Mode(mode)));
        }
        // Recientes primero: con la caja vacía la paleta muestra todo en este
        // orden, así se reentra sin escribir (`sort_by_key` es estable: el
        // resto conserva su grupo).
        let order: Vec<String> = RECENT.lock().map(|recent| recent.clone()).unwrap_or_default();
        if !order.is_empty() {
            commands.sort_by_key(|(_, act)| order.iter().position(|key| *key == recent_key(act)).unwrap_or(usize::MAX));
        }
        // Cabeceras: los recientes arriba y el resto en su grupo.
        let recent_count = order.len().min(MAX_RECENT);
        let commands: Vec<(Command, PaletteAct)> = commands
            .into_iter()
            .enumerate()
            .map(|(position, (command, act))| {
                let section = if position < recent_count && order.iter().any(|key| *key == recent_key(&act)) {
                    t("pill.code.menus.recents")
                } else {
                    match act {
                        PaletteAct::Tab(_) | PaletteAct::Session(..) => t("pill.code.palette.sectionConversations"),
                        PaletteAct::Style(_) | PaletteAct::Mode(_) => t("pill.code.palette.sectionAppearance"),
                        _ => t("pill.code.palette.sectionActions"),
                    }
                };
                (command.section(section), act)
            })
            .collect();
        let (list, acts): (Vec<Command>, Vec<PaletteAct>) = commands.into_iter().unzip();
        self.palette_acts = acts;
        self.palette.update(cx, |palette, cx| {
            palette.set_commands(list, cx);
            palette.reset(cx);
        });
        self.palette.read(cx).focus(window, cx);
        self.palette_open = true;
        cx.notify();
    }

    /// «Diagnosticar un problema»: abre una conversación suelta con la guía ya escrita en la
    /// caja (versión, sistema, dónde están los logs y la configuración), sin enviarla: tú
    /// cuentas qué pasó al final y la mandas. La guía pide al agente preguntar antes de
    /// arreglar o publicar nada.
    fn start_diagnose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_loose_chat(window, cx);
        let path_text = |path: Option<PathBuf>| path.map(|p| p.display().to_string()).unwrap_or_default();
        let logs = path_text(crate::paths::logs_dir());
        let data = path_text(crate::paths::atic_data_dir());
        let text = tf("pill.code.diagnose.prompt", &[("version", &env!("CARGO_PKG_VERSION")), ("os", &std::env::consts::OS), ("logs", &logs), ("data", &data)]);
        self.composer.update(cx, |area, cx| area.set_text_at(&text, text.len(), cx));
        self.focus_composer(window, cx);
    }

    /// Ejecuta el comando elegido (después de cerrar la paleta, con la ventana a mano).
    pub(super) fn run_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(act) = self.pending_palette.take() else {
            return;
        };
        // Guarda la elección para los recientes de la próxima apertura.
        remember(&act);
        match act {
            PaletteAct::NewConversation => self.new_conversation(window, cx),
            PaletteAct::NewLoose => self.new_loose_chat(window, cx),
            PaletteAct::NewSpace => self.open_new_space(window, cx),
            PaletteAct::OpenFolder => self.pick_folders(None, cx),
            PaletteAct::ImportVscode => self.open_new_space(window, cx),
            PaletteAct::History => {
                self.history_page = true;
                self.load_history(cx);
            }
            PaletteAct::Settings => self.settings_open = true,
            PaletteAct::SettingsTab(tab) => {
                self.settings_tab = tab;
                self.settings_open = true;
            }
            PaletteAct::FullScreen => window.toggle_fullscreen(),
            PaletteAct::Diagnose => self.start_diagnose(window, cx),
            PaletteAct::SideChat => self.side_chat(window, cx),
            PaletteAct::Terminal => self.toggle_terminal(window, cx),
            PaletteAct::Changes => self.toggle_side(Side::Changes, cx),
            PaletteAct::Files => self.toggle_side(Side::Files, cx),
            PaletteAct::Tab(path) => self.open_file(path, cx),
            PaletteAct::Session(workspace, info) => self.pick_chat(None, Some(info), workspace, window, cx),
            PaletteAct::Style(style) => self.set_appearance(style, self.configs.mode, window, cx),
            PaletteAct::Mode(mode) => self.set_appearance(self.configs.style, mode, window, cx),
        }
        cx.notify();
    }
}
