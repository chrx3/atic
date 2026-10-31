//! La paleta de comandos (Ctrl+K), como la de la referencia: acciones de la app, las
//! conversaciones del proyecto, el estilo y el modo de color.

use crate::i18n::{t, tf};
use std::path::PathBuf;

use gpui::{AppContext, Context, Window};
use gpui_m3::{Command, CommandPalette, CommandPaletteEvent};

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
}

pub(super) fn new_palette(cx: &mut Context<CodeView>) -> gpui::Entity<CommandPalette> {
    let palette = cx.new(|cx| CommandPalette::new(cx).placeholder(t("pill.code.palette.placeholder"), cx).empty_text(t("overlay.noResults")));
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
    /// Abre la paleta con la lista de ahora.
    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut commands = vec![
            (Command::new("plus", t("pill.code.palette.newConversation")).hint(crate::keymap::hint("Ctrl+N")), PaletteAct::NewConversation),
            (Command::new("chat", t("pill.code.palette.newLoose")), PaletteAct::NewLoose),
            (Command::new("folder-plus", t("pill.code.palette.newSpace")), PaletteAct::NewSpace),
            (Command::new("folder", t("pill.code.palette.openFolder")).hint(crate::keymap::hint("Ctrl+O")), PaletteAct::OpenFolder),
            (Command::new("import", t("pill.code.palette.importVscode")), PaletteAct::ImportVscode),
            (Command::new("history", t("pill.code.palette.history")), PaletteAct::History),
            (Command::new("gear", t("pill.code.palette.settings")).hint(crate::keymap::hint("Ctrl+,")), PaletteAct::Settings),
            (Command::new("refresh", t("pill.code.palette.updateClaude")), PaletteAct::Settings),
        ];
        if let Some(workspace) = self.workspaces.active_id() {
            commands.push((Command::new("diff", t("pill.code.palette.viewChanges")).hint(crate::keymap::hint("Ctrl+G")), PaletteAct::Changes));
            commands.push((Command::new("files", t("pill.code.palette.viewFiles")).hint(crate::keymap::hint("Ctrl+E")), PaletteAct::Files));
            // Las pestañas abiertas del editor, como en la referencia (`Overlays.tsx:513`).
            for tab in &self.tabs.files {
                let name = tab.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| tab.path.display().to_string());
                commands.push((Command::new("file", tf("pill.code.palette.openFile", &[("name", &name)])).hint(t("pill.code.palette.tab")), PaletteAct::Tab(tab.path.clone())));
            }
            for info in self.history.get(&workspace).into_iter().flatten().take(30) {
                commands.push((Command::new("spark", info.title.clone()).hint(t("pill.code.palette.conversation")), PaletteAct::Session(workspace, info.clone())));
            }
        }
        if cfg!(target_os = "macos") {
            let label = if window.is_fullscreen() { t("pill.code.palette.exitFullscreen") } else { t("pill.code.palette.enterFullscreen") };
            commands.push((Command::new("monitor", label).hint("⌃⌘F"), PaletteAct::FullScreen));
        }
        for (style, label) in Style::ALL {
            commands.push((Command::new("palette", tf("pill.code.palette.style", &[("label", &label)])), PaletteAct::Style(style)));
        }
        for (mode, icon, label) in [(Mode::Light, "sun", t("settings.appearance.light")), (Mode::Dark, "moon", t("settings.appearance.dark")), (Mode::System, "monitor", t("settings.appearance.system"))] {
            commands.push((Command::new(icon, tf("pill.code.palette.mode", &[("label", &label)])), PaletteAct::Mode(mode)));
        }
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

    /// Ejecuta el comando elegido (después de cerrar la paleta, con la ventana a mano).
    pub(super) fn run_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(act) = self.pending_palette.take() else {
            return;
        };
        match act {
            PaletteAct::NewConversation => self.new_conversation(window, cx),
            PaletteAct::NewLoose => self.new_loose_chat(window, cx),
            PaletteAct::NewSpace => self.open_new_space(window, cx),
            PaletteAct::OpenFolder => self.pick_folders(None, cx),
            PaletteAct::ImportVscode => self.import_vscode(window, cx),
            PaletteAct::History => {
                self.history_page = true;
                self.load_history(cx);
            }
            PaletteAct::Settings => self.settings_open = true,
            PaletteAct::FullScreen => window.toggle_fullscreen(),
            PaletteAct::Changes => self.toggle_side(Side::Changes, cx),
            PaletteAct::Files => self.toggle_side(Side::Files, cx),
            PaletteAct::Tab(path) => self.open_file(path, cx),
            PaletteAct::Session(workspace, info) => {
                self.history_page = false;
                self.open_session(workspace, info, window, cx);
            }
            PaletteAct::Style(style) => self.set_appearance(style, self.configs.mode, window, cx),
            PaletteAct::Mode(mode) => self.set_appearance(self.configs.style, mode, window, cx),
        }
        cx.notify();
    }
}
