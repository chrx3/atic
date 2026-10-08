//! La sección Atajos de la ventana de Ajustes: los atajos globales de
//! `config.json`, con un editor que graba la combinación apretada.
//!
//! Mientras se graba, los atajos de la pill se sueltan
//! (`hotkeys::set_paused`): si no, apretar uno ya tomado dispararía su
//! herramienta en vez de grabarse. Al guardar, el hilo de atajos los vuelve a
//! registrar en la próxima revisión de la config.

use std::path::PathBuf;

use atic_core::{AppDirs, Config};
use gpui::{
    div, prelude::*, px, App, ClickEvent, Context, Entity, FocusHandle, Focusable, KeyDownEvent, SharedString,
    Window,
};

use crate::dictation_settings::shortcut_label;
use crate::i18n::t;
use crate::meetings::settings::{button, card, row, save_config};
use crate::platform::hotkeys;

/// Cada atajo: el campo de `config.json`, su título y su ayuda.
struct Entry {
    title: &'static str,
    hint: &'static str,
    get: fn(&Config) -> &str,
    set: fn(&mut Config, String),
}

macro_rules! entry {
    ($field:ident, $title:literal, $hint:literal) => {
        Entry {
            title: $title,
            hint: $hint,
            get: |cfg| cfg.$field.as_str(),
            set: |cfg, value| cfg.$field = value,
        }
    };
}

const ENTRIES: [Entry; 12] = [
    entry!(global_shortcut, "settings.shortcuts.record", "settings.shortcuts.recordHint"),
    entry!(dictation_shortcut, "settings.shortcuts.dictate", "settings.shortcuts.dictateHint"),
    entry!(summon_pill_shortcut, "settings.shortcuts.summon", "settings.shortcuts.summonHint"),
    entry!(pill_radial_shortcut, "settings.shortcuts.wheel", "settings.shortcuts.wheelHint"),
    entry!(clipboard_shortcut, "settings.shortcuts.clipboard", ""),
    entry!(snippets_shortcut, "settings.shortcuts.snippets", ""),
    entry!(agents_shortcut, "settings.shortcuts.agents", "settings.shortcuts.agentsHint"),
    entry!(screenshot_shortcut, "settings.shortcuts.screenshot", ""),
    entry!(board_shortcut, "settings.shortcuts.board", "settings.shortcuts.boardHint"),
    entry!(color_shortcut, "settings.shortcuts.color", "settings.shortcuts.colorHint"),
    entry!(launcher_shortcut, "settings.shortcuts.launcher", "settings.shortcuts.launcherHint"),
    entry!(window_flip_shortcut, "settings.shortcuts.flip", "settings.shortcuts.flipHint"),
];

pub struct ShortcutsPane {
    path: PathBuf,
    cfg: Config,
    /// El atajo que se está grabando (índice en `ENTRIES`).
    recording: Option<usize>,
    focus: FocusHandle,
}

pub fn shortcuts_pane(cx: &mut App) -> Option<Entity<ShortcutsPane>> {
    let path = AppDirs::new().ok()?.config_path();
    Some(cx.new(|cx| ShortcutsPane { cfg: Config::load(&path), path, recording: None, focus: cx.focus_handle() }))
}

impl Focusable for ShortcutsPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for ShortcutsPane {
    fn drop(&mut self) {
        if self.recording.is_some() {
            hotkeys::set_paused(false);
        }
    }
}

/// La combinación como la escribe Atic (`Ctrl+Shift+V`, `Alt+F5`), o `None`
/// si la tecla no sirve para un atajo (una sola tecla sin modificadores, o
/// una que `RegisterHotKey` no entiende).
pub(crate) fn chord_text(control: bool, alt: bool, shift: bool, win: bool, key: &str) -> Option<String> {
    let key = match key {
        "space" => "Space".to_string(),
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => k.to_ascii_uppercase(),
        k if k.starts_with('f') && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
            k.to_ascii_uppercase()
        }
        _ => return None,
    };
    let is_function = key.starts_with('F') && key.len() > 1;
    if !(control || alt || win || is_function) {
        return None;
    }
    let mut parts = Vec::new();
    if control {
        parts.push("Ctrl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    if win {
        parts.push("Super".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
}

impl ShortcutsPane {
    fn start(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = Some(index);
        hotkeys::set_paused(true);
        window.focus(&self.focus);
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        hotkeys::set_paused(false);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.recording else {
            return;
        };
        let stroke = &event.keystroke;
        if stroke.key == "escape" {
            return self.stop(cx);
        }
        let mods = &stroke.modifiers;
        let Some(text) = chord_text(mods.control, mods.alt, mods.shift, mods.platform, &stroke.key) else {
            // Un modificador solo, o una tecla que no sirve: se sigue esperando.
            return;
        };
        let set = ENTRIES[index].set;
        self.cfg = save_config(&self.path, |cfg| set(cfg, text));
        cx.stop_propagation();
        self.stop(cx);
    }
}

impl Render for ShortcutsPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = card();
        for (index, entry) in ENTRIES.iter().enumerate() {
            let recording = self.recording == Some(index);
            let current = (entry.get)(&self.cfg);
            let value: SharedString = if recording {
                t("pill.settings.pressShortcut").into()
            } else if current.is_empty() {
                "—".into()
            } else {
                shortcut_label(current).into()
            };
            let hint = if entry.hint.is_empty() { "" } else { t(entry.hint) };
            list = list.child(row(
                t(entry.title),
                hint,
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(12.)).text_color(crate::settings::hsla(crate::settings::MUTED)).child(value))
                    .child(button(
                        SharedString::from(format!("shortcut-{index}")),
                        if recording { t("pill.settings.cancel") } else { t("pill.settings.change") },
                        cx.listener(move |pane, _: &ClickEvent, window, cx| {
                            if pane.recording == Some(index) {
                                pane.stop(cx);
                            } else {
                                pane.start(index, window, cx);
                            }
                        }),
                    )),
            ));
        }
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().px(px(4.)).text_size(px(12.)).text_color(crate::settings::hsla(crate::settings::MUTED)).child(t("settings.shortcuts.hint")))
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graba_como_escribe_atic() {
        assert_eq!(chord_text(true, false, true, false, "v").as_deref(), Some("Ctrl+Shift+V"));
        assert_eq!(chord_text(false, true, false, false, "space").as_deref(), Some("Alt+Space"));
        assert_eq!(chord_text(false, false, false, false, "f5").as_deref(), Some("F5"));
        assert_eq!(chord_text(false, false, true, false, "a"), None);
        assert_eq!(chord_text(true, false, false, false, "escape"), None);
        assert_eq!(chord_text(true, false, false, false, "control"), None);
    }

    #[test]
    fn lo_grabado_lo_entiende_el_hilo_de_atajos() {
        let text = chord_text(true, true, false, false, "4").unwrap();
        assert!(crate::dictation::Shortcut::parse(&text).is_some());
    }
}
