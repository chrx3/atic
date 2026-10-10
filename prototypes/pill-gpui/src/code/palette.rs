//! La paleta de comandos (Ctrl+K), como la de la referencia: acciones de la app, las
//! conversaciones del proyecto, el estilo y el modo de color.

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
    History,
    Settings,
    Changes,
    Files,
    Session(u64, SessionInfo),
    Style(Style),
    Mode(Mode),
}

pub(super) fn new_palette(cx: &mut Context<CodeView>) -> gpui::Entity<CommandPalette> {
    let palette = cx.new(|cx| CommandPalette::new(cx).placeholder("Busca un comando o una conversación…", cx).empty_text("Sin resultados"));
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
            (Command::new("plus", "Nueva conversación").hint("Ctrl+N"), PaletteAct::NewConversation),
            (Command::new("chat", "Nuevo chat sin proyecto"), PaletteAct::NewLoose),
            (Command::new("folder-plus", "Nuevo espacio…"), PaletteAct::NewSpace),
            (Command::new("folder", "Abrir carpeta…").hint("Ctrl+O"), PaletteAct::OpenFolder),
            (Command::new("history", "Historial de conversaciones"), PaletteAct::History),
            (Command::new("gear", "Configuración").hint("Ctrl+,"), PaletteAct::Settings),
            (Command::new("refresh", "Actualizar Claude Code"), PaletteAct::Settings),
        ];
        if let Some(workspace) = self.workspaces.active_id() {
            commands.push((Command::new("diff", "Ver cambios"), PaletteAct::Changes));
            commands.push((Command::new("files", "Ver archivos"), PaletteAct::Files));
            for info in self.history.get(&workspace).into_iter().flatten().take(30) {
                commands.push((Command::new("spark", info.title.clone()).hint("Conversación"), PaletteAct::Session(workspace, info.clone())));
            }
        }
        for (style, label) in Style::ALL {
            commands.push((Command::new("palette", format!("Estilo: {label}")), PaletteAct::Style(style)));
        }
        for (mode, icon, label) in [(Mode::Light, "sun", "Claro"), (Mode::Dark, "moon", "Oscuro"), (Mode::System, "monitor", "Sistema")] {
            commands.push((Command::new(icon, format!("Modo: {label}")), PaletteAct::Mode(mode)));
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
            PaletteAct::History => {
                self.history_page = true;
                self.load_history(cx);
            }
            PaletteAct::Settings => self.settings_open = true,
            PaletteAct::Changes => self.toggle_side(Side::Changes, cx),
            PaletteAct::Files => self.toggle_side(Side::Files, cx),
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
