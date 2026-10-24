//! Los atajos de Atic Code: una tabla de datos (acción, grupo, descripción, tecla por
//! defecto), de la que salen los `bind_keys` de la ventana, de la terminal y del editor y
//! la pestaña «Atajos» de la Configuración. Los cambios del usuario se guardan en
//! `code-claude.json` (`Configs::shortcuts`, id → tecla) y se aplican en vivo.
//!
//! GPUI no deja quitar un binding suelto (`clear_key_bindings` vaciaría también el
//! teclado de las demás ventanas de la pill), así que cada vez se vuelve a vincular todo:
//! primero con `NoAction` las teclas que quedaron sueltas y después las actuales. A igual
//! profundidad gana el último vinculado, y `NoAction` corta lo que tenga debajo.

use std::collections::HashMap;
use std::sync::Mutex;

use gpui::{div, prelude::*, px, AnyElement, App, ClickEvent, Context, KeyBinding, Keystroke, SharedString, Subscription, Window};
use gpui_m3::{Badge, Banner, Button, ButtonSize, IconButton, KeyCombo, KeyNames, ListGroup, SettingRow};

use super::style::t;
use crate::i18n::{t as tr, tf};
use super::{editor, terminal, CodeView};

/// Los contextos de teclas.
const GENERAL: &str = "AticCode";
const COMPOSER: &str = "CodeComposer > TextArea";
const SUGGESTING: &str = "CodeComposer > TextArea && suggesting";

/// El índice de la pestaña «Atajos» en la Configuración.
pub(super) const TAB: usize = 2;

/// Cómo se trata un atajo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Se vincula y el usuario puede cambiarlo.
    Edit,
    /// Se vincula, pero no se cambia (Esc, pegar, la lista de @).
    Fixed,
    /// Solo informa: lo resuelve otra parte (Esc Esc, el editor).
    Info,
}

pub struct Shortcut {
    pub id: &'static str,
    /// El grupo (su nombre estable, no el texto: para mostrarlo, `group_label`).
    pub group: &'static str,
    /// La clave del texto (se muestra con `desc()`).
    pub desc: &'static str,
    /// La tecla por defecto (`ctrl-shift-k`). Con espacios es una secuencia (solo `Info`).
    pub key: &'static str,
    pub ctx: &'static str,
    pub kind: Kind,
    /// Dentro de la terminal y del editor la tecla no actúa: pasa al shell o al editor.
    pub inner: bool,
}

impl Shortcut {
    /// La descripción en el idioma de ahora.
    pub fn desc(&self) -> &'static str {
        tr(self.desc)
    }
}

/// El título de un grupo en el idioma de ahora.
pub fn group_label(group: &str) -> &'static str {
    match group {
        "General" => tr("pill.code.shortcuts.groupGeneral"),
        "Conversación" => tr("pill.code.shortcuts.groupConversation"),
        "Composer" => tr("pill.code.shortcuts.groupComposer"),
        "Menciones" => tr("pill.code.shortcuts.groupMentions"),
        "Terminal" => tr("pill.code.shortcuts.groupTerminal"),
        "Editor" => tr("pill.code.shortcuts.groupEditor"),
        _ => "",
    }
}

const fn s(id: &'static str, group: &'static str, desc: &'static str, key: &'static str, ctx: &'static str, kind: Kind, inner: bool) -> Shortcut {
    Shortcut { id, group, desc, key, ctx, kind, inner }
}

/// Los grupos en el orden en que se muestran.
pub const GROUPS: [&str; 6] = ["General", "Conversación", "Composer", "Menciones", "Terminal", "Editor"];

use Kind::{Edit, Fixed, Info};

pub const TABLE: [Shortcut; 31] = [
    s("palette", "General", "pill.code.shortcuts.palette", "ctrl-k", GENERAL, Edit, true),
    s("palette_alt", "General", "pill.code.shortcuts.paletteAlt", "ctrl-p", GENERAL, Edit, true),
    s("new_conversation", "General", "pill.code.shortcuts.newConversation", "ctrl-n", GENERAL, Edit, true),
    s("clear_conversation", "General", "pill.code.shortcuts.clearConversation", "ctrl-l", GENERAL, Edit, true),
    s("toggle_sidebar", "General", "pill.code.shortcuts.toggleSidebar", "ctrl-b", GENERAL, Edit, true),
    s("settings", "General", "pill.code.shortcuts.settings", "ctrl-,", GENERAL, Edit, false),
    s("show_files", "General", "pill.code.shortcuts.showFiles", "ctrl-e", GENERAL, Edit, true),
    s("show_changes", "General", "pill.code.shortcuts.showChanges", "ctrl-g", GENERAL, Edit, true),
    s("open_folder", "General", "pill.code.shortcuts.openFolder", "ctrl-o", GENERAL, Edit, true),
    s("close_menu", "General", "pill.code.shortcuts.closeMenu", "escape", GENERAL, Fixed, false),
    s("attach", "Composer", "pill.code.shortcuts.attach", "ctrl-u", GENERAL, Edit, true),
    s("send", "Composer", "pill.code.shortcuts.send", "enter", COMPOSER, Edit, false),
    s("newline", "Composer", "pill.code.shortcuts.newline", "shift-enter", COMPOSER, Edit, false),
    s("paste", "Composer", "pill.code.shortcuts.paste", "ctrl-v", COMPOSER, Fixed, false),
    s("submit_answers", "Conversación", "pill.code.shortcuts.submitAnswers", "ctrl-enter", GENERAL, Edit, true),
    s("rewind", "Conversación", "pill.code.shortcuts.rewind", "escape escape", "", Info, false),
    s("interrupt", "Conversación", "pill.code.shortcuts.interrupt", "escape", "", Info, false),
    s("mention_prev", "Menciones", "pill.code.shortcuts.mentionPrev", "up", SUGGESTING, Fixed, false),
    s("mention_next", "Menciones", "pill.code.shortcuts.mentionNext", "down", SUGGESTING, Fixed, false),
    s("mention_enter", "Menciones", "pill.code.shortcuts.mentionEnter", "enter", SUGGESTING, Fixed, false),
    s("mention_tab", "Menciones", "pill.code.shortcuts.mentionTab", "tab", SUGGESTING, Fixed, false),
    s("mention_close", "Menciones", "pill.code.shortcuts.mentionClose", "escape", SUGGESTING, Fixed, false),
    s("toggle_terminal", "Terminal", "pill.code.shortcuts.toggleTerminal", "ctrl-`", GENERAL, Edit, false),
    s("toggle_terminal_alt", "Terminal", "pill.code.shortcuts.toggleTerminalAlt", "ctrl-j", GENERAL, Edit, false),
    s("editor_save", "Editor", "pill.code.shortcuts.editorSave", "ctrl-s", "", Info, false),
    s("editor_undo", "Editor", "pill.code.shortcuts.editorUndo", "ctrl-z", "", Info, false),
    s("editor_indent", "Editor", "pill.code.shortcuts.editorIndent", "tab", "", Info, false),
    s("editor_close", "Editor", "pill.code.shortcuts.editorClose", "escape", "", Info, false),
    s("terminal_paste", "Terminal", "pill.code.shortcuts.terminalPaste", "ctrl-v", "", Info, false),
    s("terminal_interrupt", "Terminal", "pill.code.shortcuts.terminalInterrupt", "ctrl-c", "", Info, false),
    s("terminal_complete", "Terminal", "pill.code.shortcuts.terminalComplete", "tab", "", Info, false),
];

/// Teclas que no actúan en la terminal aunque no sean de la tabla: pasan al shell.
const SHELL_EXTRA: [&str; 5] = ["ctrl-j", "ctrl-v", "escape", "tab", "shift-tab"];
/// Y las del editor.
const EDITOR_EXTRA: [&str; 1] = ["ctrl-j"];

/// Teclas del sistema y de edición que no se pueden asignar.
const RESERVED: [&str; 7] = ["ctrl-c", "ctrl-x", "ctrl-v", "ctrl-a", "ctrl-z", "ctrl-y", "alt-f4"];

pub fn find(id: &str) -> Option<&'static Shortcut> {
    TABLE.iter().find(|row| row.id == id)
}

// ---------- teclas: normalizar y mostrar ----------

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Mods {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

/// Separa los modificadores de la tecla (`ctrl--` es Ctrl y «-»).
fn split(key: &str) -> (Mods, &str) {
    let mut mods = Mods::default();
    let mut rest = key;
    'next: loop {
        for (prefix, apply) in [
            ("ctrl-", (|m: &mut Mods| m.ctrl = true) as fn(&mut Mods)),
            ("control-", |m| m.ctrl = true),
            ("alt-", |m| m.alt = true),
            ("shift-", |m| m.shift = true),
            ("cmd-", |m| m.win = true),
            ("win-", |m| m.win = true),
            ("super-", |m| m.win = true),
        ] {
            if let Some(after) = rest.strip_prefix(prefix) {
                if !after.is_empty() {
                    apply(&mut mods);
                    rest = after;
                    continue 'next;
                }
            }
        }
        return (mods, rest);
    }
}

/// La forma única de una tecla: modificadores en orden fijo y todo en minúsculas.
pub fn normalize(key: &str) -> String {
    let (mods, rest) = split(key.trim());
    let mut out = String::new();
    for (on, name) in [(mods.ctrl, "ctrl"), (mods.alt, "alt"), (mods.shift, "shift"), (mods.win, "win")] {
        if on {
            out.push_str(name);
            out.push('-');
        }
    }
    out.push_str(&rest.to_lowercase());
    out
}

fn key_name(key: &str) -> String {
    match key {
        "enter" => "Enter".into(),
        "escape" => "Esc".into(),
        "space" => "Espacio".into(),
        "backspace" => "Retroceso".into(),
        "delete" => "Supr".into(),
        "tab" => "Tab".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "pageup" => "Re Pág".into(),
        "pagedown" => "Av Pág".into(),
        "home" => "Inicio".into(),
        "end" => "Fin".into(),
        "insert" => "Ins".into(),
        other => other.to_uppercase(),
    }
}

/// Las teclas dibujadas como chips: `ctrl-shift-k` → `["Ctrl", "Mayús", "K"]`. Una
/// secuencia (`escape escape`) da los chips de cada paso, uno tras otro.
pub fn format_key(key: &str) -> Vec<String> {
    let mut chips = Vec::new();
    for step in key.split_whitespace() {
        let (mods, rest) = split(step);
        // En macOS, los símbolos de siempre y en su orden (⌃⌥⇧⌘).
        let names = if cfg!(target_os = "macos") { ["⌃", "⌥", "⇧", "⌘"] } else { ["Ctrl", "Alt", "Mayús", "Win"] };
        for (on, name) in [mods.ctrl, mods.alt, mods.shift, mods.win].into_iter().zip(names) {
            if on {
                chips.push(name.to_string());
            }
        }
        chips.push(key_name(rest));
    }
    chips
}

/// «Ctrl+Mayús+K», para los avisos.
pub fn label(key: &str) -> String {
    format_key(key).join("+")
}

// ---------- aplicar lo guardado sobre la tabla ----------

/// La tecla es una letra, un símbolo, una F o una tecla con nombre: `Keystroke::parse` acepta
/// casi cualquier texto, así que no basta.
fn valid_key(key: &str) -> bool {
    if Keystroke::parse(key).is_err() {
        return false;
    }
    let (_, rest) = split(key);
    let named = ["enter", "escape", "space", "backspace", "delete", "tab", "up", "down", "left", "right", "pageup", "pagedown", "home", "end", "insert"];
    let function = rest.len() > 1 && rest.starts_with('f') && rest[1..].chars().all(|c| c.is_ascii_digit());
    rest.chars().count() == 1 || function || named.contains(&rest)
}

/// Cada atajo con la tecla que vale ahora: la guardada si es válida, si no la de fábrica.
pub fn effective(saved: &HashMap<String, String>) -> Vec<(&'static Shortcut, String)> {
    TABLE
        .iter()
        .map(|row| {
            let key = match saved.get(row.id) {
                Some(key) if row.kind == Kind::Edit && valid_key(&normalize(key)) => normalize(key),
                // De fábrica, Ctrl es Comando en macOS.
                _ => normalize(&crate::keymap::primary(row.key)),
            };
            (row, key)
        })
        .collect()
}

#[cfg(test)]
pub fn key_of(saved: &HashMap<String, String>, id: &str) -> String {
    effective(saved).into_iter().find(|(row, _)| row.id == id).map(|(_, key)| key).unwrap_or_default()
}

/// Otro atajo que ya usa esa tecla. La lista de @ y los `Info` no cuentan: viven en
/// contextos más hondos o los resuelve otra parte.
pub fn conflict(saved: &HashMap<String, String>, id: &str, key: &str) -> Option<&'static Shortcut> {
    let key = normalize(key);
    effective(saved).into_iter().find(|(row, k)| row.id != id && row.kind != Kind::Info && row.ctx != SUGGESTING && *k == key).map(|(row, _)| row)
}

/// Revisa una tecla capturada para un atajo: devuelve la tecla normalizada o por qué no.
pub fn check(saved: &HashMap<String, String>, id: &str, key: &str) -> Result<String, String> {
    let row = find(id).filter(|row| row.kind == Kind::Edit).ok_or_else(|| tr("pill.code.shortcuts.cannotChange").to_string())?;
    let key = normalize(key);
    if !valid_key(&key) {
        return Err(tr("pill.code.shortcuts.invalidCombo").into());
    }
    let (mods, rest) = split(&key);
    let function = rest.len() > 1 && rest.starts_with('f') && rest[1..].chars().all(|c| c.is_ascii_digit());
    // Los de la caja de texto pueden ser Enter solo; el resto necesita Ctrl o Alt (si no,
    // la tecla dejaría de escribirse).
    if !(mods.ctrl || mods.alt || mods.win || function || (row.ctx != GENERAL && rest == "enter")) {
        return Err(if cfg!(target_os = "macos") { tr("pill.code.shortcuts.needModifierMac") } else { tr("pill.code.shortcuts.needModifier") }.into());
    }
    if RESERVED.iter().any(|reserved| normalize(&crate::keymap::primary(reserved)) == key) {
        return Err(tf("pill.code.shortcuts.reserved", &[("key", &label(&key))]));
    }
    if let Some(other) = conflict(saved, id, &key) {
        return Err(tf("pill.code.shortcuts.taken", &[("key", &label(&key)), ("desc", &other.desc())]));
    }
    Ok(key)
}

/// Lo guardado con un cambio: si la tecla es la de fábrica, se borra la entrada.
pub fn with_change(saved: &HashMap<String, String>, id: &str, key: &str) -> HashMap<String, String> {
    let mut next = saved.clone();
    match find(id) {
        Some(row) if normalize(key) != normalize(&crate::keymap::primary(row.key)) => {
            next.insert(id.to_string(), normalize(key));
        }
        _ => {
            next.remove(id);
        }
    }
    next
}

/// Las teclas que no actúan dentro de la terminal.
pub fn terminal_keys(saved: &HashMap<String, String>) -> Vec<String> {
    inner_keys(saved, &SHELL_EXTRA)
}

/// Y dentro del editor.
pub fn editor_keys(saved: &HashMap<String, String>) -> Vec<String> {
    inner_keys(saved, &EDITOR_EXTRA)
}

fn inner_keys(saved: &HashMap<String, String>, extra: &[&str]) -> Vec<String> {
    let mut keys: Vec<String> = effective(saved).into_iter().filter(|(row, _)| row.inner).map(|(_, key)| key).collect();
    for key in extra {
        if !keys.iter().any(|k| k == key) {
            keys.push((*key).to_string());
        }
    }
    keys
}

// ---------- vincular ----------

/// La acción de cada id, ya vinculada a su tecla.
fn binding(id: &str, key: &str, context: Option<&'static str>) -> Option<KeyBinding> {
    use super::*;
    Some(match id {
        "palette" | "palette_alt" => KeyBinding::new(key, OpenPalette, context),
        "new_conversation" => KeyBinding::new(key, NewConversation, context),
        "clear_conversation" => KeyBinding::new(key, ClearConversation, context),
        "toggle_sidebar" => KeyBinding::new(key, ToggleSidebar, context),
        "settings" => KeyBinding::new(key, ToggleSettings, context),
        "show_files" => KeyBinding::new(key, ShowFiles, context),
        "show_changes" => KeyBinding::new(key, ShowChanges, context),
        "open_folder" => KeyBinding::new(key, OpenFolder, context),
        "close_menu" => KeyBinding::new(key, CloseMenu, context),
        "attach" => KeyBinding::new(key, Attach, context),
        "send" => KeyBinding::new(key, super::Send, context),
        "newline" => KeyBinding::new(key, crate::text_area::Newline, context),
        "paste" => KeyBinding::new(key, PasteAttach, context),
        "submit_answers" => KeyBinding::new(key, SubmitAnswers, context),
        "mention_prev" => KeyBinding::new(key, gpui_m3::SuggestionPrevious, context),
        "mention_next" => KeyBinding::new(key, gpui_m3::SuggestionNext, context),
        "mention_enter" | "mention_tab" => KeyBinding::new(key, gpui_m3::SuggestionAccept, context),
        "mention_close" => KeyBinding::new(key, gpui_m3::SuggestionDismiss, context),
        "toggle_terminal" | "toggle_terminal_alt" => KeyBinding::new(key, terminal::ToggleTerminal, context),
        _ => return None,
    })
}

/// Lo vinculado la última vez (contexto y tecla): lo que ya no se use se anula.
static APPLIED: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());

/// Vincula todos los atajos de Atic Code con lo guardado. Se llama al abrir la app y
/// cada vez que cambia un atajo: las teclas viejas quedan en `NoAction` y las vigentes
/// se vuelven a vincular al final, que es donde ganan a igual profundidad.
pub fn bind_keys(cx: &mut App, saved: &HashMap<String, String>) {
    let rows = effective(saved);
    // La lista de @ va al final de todo: comparte profundidad con la caja (Enter).
    let (mentions, rest): (Vec<_>, Vec<_>) = rows.iter().filter(|(row, _)| row.kind != Kind::Info).partition(|(row, _)| row.ctx == SUGGESTING);
    let current: Vec<(&'static str, String)> = rows.iter().filter(|(row, _)| row.kind != Kind::Info).map(|(row, key)| (row.ctx, key.clone())).collect();
    let mut applied = APPLIED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let stale: Vec<KeyBinding> =
        applied.iter().filter(|entry| !current.contains(entry)).map(|(ctx, key)| KeyBinding::new(key, gpui::NoAction, Some(*ctx))).collect();
    cx.bind_keys(stale);
    cx.bind_keys(rest.iter().filter_map(|(row, key)| binding(row.id, key, Some(row.ctx))));
    terminal::bind_keys(cx, &terminal_keys(saved));
    editor::bind_keys(cx, &editor_keys(saved));
    cx.bind_keys(mentions.iter().filter_map(|(row, key)| binding(row.id, key, Some(row.ctx))));
    *applied = current;
}

// ---------- la pestaña ----------

/// Lo que la pestaña recuerda: qué atajo espera tecla y el último aviso.
#[derive(Default)]
pub struct Ui {
    capturing: Option<&'static str>,
    notice: Option<String>,
    /// El espía de teclas, mientras se captura.
    sub: Option<Subscription>,
}

impl CodeView {
    /// Empieza a esperar la nueva combinación de un atajo.
    fn start_capture(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcuts_ui.capturing = Some(id);
        self.shortcuts_ui.notice = None;
        let view = cx.entity().downgrade();
        let handle = window.window_handle();
        // Antes de que la tecla llegue a ninguna acción: así Ctrl+K no abre la paleta.
        self.shortcuts_ui.sub = Some(cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != handle {
                return;
            }
            let key = event.keystroke.unparse();
            let taken = view.update(cx, |this, cx| this.captured(&key, cx)).unwrap_or(false);
            if taken {
                cx.stop_propagation();
            }
        }));
        cx.notify();
    }

    fn stop_capture(&mut self) {
        self.shortcuts_ui.capturing = None;
        self.shortcuts_ui.sub = None;
    }

    /// Una tecla llegó mientras se captura; `true` si la consumimos.
    fn captured(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.shortcuts_ui.capturing else {
            self.stop_capture();
            return false;
        };
        // Si se cerró la configuración o se cambió de pestaña, la captura ya no vale.
        if !self.settings_open || !(self.settings_tab == TAB || !self.settings_query(cx).is_empty()) {
            self.stop_capture();
            return false;
        }
        if normalize(key) == "escape" {
            self.stop_capture();
            self.shortcuts_ui.notice = None;
        } else {
            match check(&self.configs.shortcuts, id, key) {
                Ok(key) => {
                    let next = with_change(&self.configs.shortcuts, id, &key);
                    self.set_shortcuts(next, cx);
                    self.shortcuts_ui.notice = None;
                    self.stop_capture();
                }
                // Se sigue esperando: el aviso dice por qué no sirvió.
                Err(why) => self.shortcuts_ui.notice = Some(why),
            }
        }
        cx.notify();
        true
    }

    /// Guarda los atajos y los aplica en vivo.
    fn set_shortcuts(&mut self, next: HashMap<String, String>, cx: &mut Context<Self>) {
        self.configs.shortcuts = next;
        self.configs.save();
        bind_keys(cx, &self.configs.shortcuts);
    }

    fn reset_shortcut(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut next = self.configs.shortcuts.clone();
        next.remove(id);
        self.shortcuts_ui.notice = None;
        self.set_shortcuts(next, cx);
        cx.notify();
    }

    fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.stop_capture();
        self.shortcuts_ui.notice = None;
        self.set_shortcuts(HashMap::new(), cx);
        cx.notify();
    }

    /// El aviso de la captura (tecla inválida o en uso), si hay.
    pub(super) fn shortcut_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let why = self.shortcuts_ui.notice.clone()?;
        Some(
            Banner::new("shortcut-notice")
                .warning()
                .icon("x")
                .title(why)
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.shortcuts_ui.notice = None;
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    /// Una fila de la lista: la descripción a la izquierda y, alineada a la derecha, la
    /// combinación (clic para cambiarla) con «Restablecer» al lado, que aparece al pasar el
    /// cursor o cuando la tecla no es la de fábrica. Las filas fijas y las informativas no
    /// tienen acción y se ven más tenues.
    pub(super) fn shortcut_row(&self, row: &'static Shortcut, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let waiting = self.shortcuts_ui.capturing == Some(id);
        let custom = self.configs.shortcuts.contains_key(id);
        let editable = row.kind == Kind::Edit;
        let mut combo = KeyCombo::new(SharedString::from(format!("shortcut-key-{id}")), key.to_string()).names(KeyNames::spanish());
        if editable {
            combo = combo
                .capturing(waiting)
                .prompt(tr("pill.code.shortcuts.prompt"))
                .tooltip(tr("pill.code.shortcuts.changeTooltip"))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.start_capture(id, window, cx)));
        } else {
            combo = combo.muted(true);
        }
        let description = if waiting {
            Some(tr("pill.code.shortcuts.escCancels").to_string())
        } else if custom {
            Some(tf("pill.code.shortcuts.factory", &[("key", &label(row.key))]))
        } else {
            None
        };
        let mut control = div().flex().items_center().gap(px(6.));
        match row.kind {
            Kind::Edit => {}
            Kind::Fixed => control = control.child(Badge::new(tr("pill.code.shortcuts.fixed"))),
            Kind::Info => control = control.child(Badge::new(tr("pill.code.shortcuts.info"))),
        }
        control = control.child(combo);
        if editable {
            // El hueco del botón es fijo: las combinaciones quedan alineadas con o sin él.
            let group = SharedString::from(format!("shortcut-group-{id}"));
            control = control.child(
                div().w(px(32.)).flex_none().child(
                    div()
                        .opacity(if custom && !waiting { 1. } else { 0. })
                        .group_hover(group, |style| style.opacity(1.))
                        .child(
                            IconButton::new(SharedString::from(format!("shortcut-reset-{id}")), "refresh")
                                .size(px(28.))
                                .tooltip(tr("pill.code.shortcuts.reset"))
                                .disabled(!custom || waiting)
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.reset_shortcut(id, cx))),
                        ),
                ),
            );
        } else {
            control = control.child(div().w(px(32.)).flex_none());
        }
        let mut item = SettingRow::new(SharedString::from(format!("shortcut-row-{id}")), row.desc()).muted(!editable).control(control);
        if let Some(description) = description {
            item = item.description(description);
        }
        div().id(SharedString::from(format!("shortcut-wrap-{id}"))).group(SharedString::from(format!("shortcut-group-{id}"))).child(item).into_any_element()
    }

    /// Un grupo de atajos: su título y una fila por atajo. En la pestaña, «Terminal» y
    /// «Editor» cierran con las teclas que no actúan dentro de ellos.
    fn shortcut_group(&self, group: &'static str, cx: &mut Context<Self>) -> ListGroup {
        let saved = &self.configs.shortcuts;
        let mut list = ListGroup::new().title(group_label(group));
        for (row, key) in effective(saved).into_iter().filter(|(row, _)| row.group == group) {
            list = list.custom_row(self.shortcut_row(row, &key, cx));
        }
        // Lo que pasa al shell o al editor no es de la tabla: se muestra como nota.
        let inner = match group {
            "Terminal" => Some((tr("pill.code.shortcuts.innerTerminal"), terminal_keys(saved))),
            "Editor" => Some((tr("pill.code.shortcuts.innerEditor"), editor_keys(saved))),
            _ => None,
        };
        if let Some((text, keys)) = inner {
            let t = t();
            let mut wrap = div().flex().flex_wrap().gap(px(8.));
            for (index, key) in keys.into_iter().enumerate() {
                wrap = wrap.child(KeyCombo::new(SharedString::from(format!("inner-{group}-{index}")), key).names(KeyNames::spanish()).small(true).muted(true));
            }
            list = list.custom_row(div().flex().flex_col().gap(px(6.)).child(div().text_size(px(12.5)).text_color(t.muted).child(text)).child(wrap));
        }
        list
    }

    /// La pestaña «Atajos»: una lista por grupo, y «Restablecer todos» arriba.
    pub(super) fn shortcuts_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let changed = !self.configs.shortcuts.is_empty();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(div().flex_1().text_size(px(13.)).text_color(t.muted).child(tr("pill.code.shortcuts.hint")))
            .child(
                Button::new("shortcuts-reset-all", tr("pill.code.shortcuts.resetAll"))
                    .tonal()
                    .icon("refresh")
                    .size(ButtonSize::Small)
                    .disabled(!changed)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.reset_all_shortcuts(cx))),
            );
        let mut page = div().flex().flex_col().gap(px(14.)).child(header);
        if let Some(notice) = self.shortcut_notice(cx) {
            page = page.child(notice);
        }
        for group in GROUPS {
            page = page.child(self.shortcut_group(group, cx));
        }
        page.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(id, key)| (id.to_string(), key.to_string())).collect()
    }

    /// La tecla de fábrica como queda en este sistema (Ctrl es ⌘ en macOS).
    fn p(key: &str) -> String {
        normalize(&crate::keymap::primary(key))
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn formats_keys_as_mac_symbols() {
        assert_eq!(format_key("cmd-shift-k"), ["⇧", "⌘", "K"]);
        assert_eq!(format_key("ctrl-alt-x"), ["⌃", "⌥", "X"]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn formats_keys_as_chips() {
        assert_eq!(format_key("ctrl-shift-k"), ["Ctrl", "Mayús", "K"]);
        assert_eq!(format_key("shift-ctrl-k"), ["Ctrl", "Mayús", "K"]);
        assert_eq!(format_key("ctrl-,"), ["Ctrl", ","]);
        assert_eq!(format_key("ctrl--"), ["Ctrl", "-"]);
        assert_eq!(format_key("alt-enter"), ["Alt", "Enter"]);
        assert_eq!(format_key("escape escape"), ["Esc", "Esc"]);
        assert_eq!(format_key("up"), ["↑"]);
        assert_eq!(label("ctrl-alt-f5"), "Ctrl+Alt+F5");
    }

    #[test]
    fn normalizes_modifier_order_and_case() {
        assert_eq!(normalize("shift-ctrl-K"), "ctrl-shift-k");
        assert_eq!(normalize("cmd-alt-x"), "alt-win-x");
        assert_eq!(normalize("ctrl--"), "ctrl--");
    }

    #[test]
    fn table_ids_are_unique_and_keys_valid() {
        for (i, row) in TABLE.iter().enumerate() {
            assert!(TABLE.iter().skip(i + 1).all(|other| other.id != row.id), "id repetido: {}", row.id);
            assert!(GROUPS.contains(&row.group), "grupo desconocido: {}", row.group);
            if row.kind != Kind::Info {
                assert!(Keystroke::parse(row.key).is_ok(), "tecla inválida: {}", row.key);
                assert!(binding(row.id, row.key, Some(row.ctx)).is_some(), "sin acción: {}", row.id);
            }
        }
    }

    #[test]
    fn defaults_have_no_conflicts() {
        let none = HashMap::new();
        for (row, key) in effective(&none) {
            if row.kind == Kind::Edit {
                assert!(conflict(&none, row.id, &key).is_none(), "{} choca por defecto", row.id);
            }
        }
    }

    #[test]
    fn saved_changes_apply_over_defaults() {
        let map = saved(&[("palette", "Alt-Shift-P"), ("close_menu", "ctrl-q"), ("settings", "no válida-")]);
        assert_eq!(key_of(&map, "palette"), "alt-shift-p");
        // Los fijos no cambian aunque el archivo lo diga.
        assert_eq!(key_of(&map, "close_menu"), "escape");
        // Una entrada inválida cae a la de fábrica.
        assert_eq!(key_of(&map, "settings"), p("ctrl-,"));
        assert_eq!(key_of(&map, "attach"), p("ctrl-u"));
        assert_eq!(key_of(&map, "nada"), "");
    }

    #[test]
    fn detects_conflicts() {
        let none = HashMap::new();
        assert_eq!(conflict(&none, "attach", &p("ctrl-k")).map(|row| row.id), Some("palette"));
        assert!(conflict(&none, "palette", &p("ctrl-k")).is_none());
        assert!(conflict(&none, "attach", "ctrl-shift-u").is_none());
        // Con un cambio guardado, la tecla vieja queda libre y la nueva ocupada.
        let map = saved(&[("palette", "ctrl-m")]);
        assert!(conflict(&map, "attach", &p("ctrl-k")).is_none());
        assert_eq!(conflict(&map, "attach", "Ctrl-M").map(|row| row.id), Some("palette"));
    }

    #[test]
    fn check_rejects_unusable_keys() {
        let none = HashMap::new();
        assert_eq!(check(&none, "attach", "ctrl-shift-u"), Ok("ctrl-shift-u".into()));
        assert!(check(&none, "attach", "u").is_err(), "una letra sola rompería la escritura");
        assert!(check(&none, "attach", &p("ctrl-c")).is_err());
        assert!(check(&none, "attach", &p("ctrl-k")).unwrap_err().contains("paleta"));
        assert!(check(&none, "close_menu", "ctrl-q").is_err());
        assert_eq!(check(&none, "send", "enter"), Ok("enter".into()));
        assert!(check(&none, "palette", "enter").is_err());
        assert_eq!(check(&none, "settings", "f2"), Ok("f2".into()));
    }

    #[test]
    fn with_change_drops_default_entries() {
        let none = HashMap::new();
        let map = with_change(&none, "palette", "ctrl-m");
        assert_eq!(map.get("palette").map(String::as_str), Some("ctrl-m"));
        let back = with_change(&map, "palette", &p("ctrl-k"));
        assert!(back.is_empty());
    }

    #[test]
    fn inner_keys_follow_changes() {
        let none = HashMap::new();
        let shell = terminal_keys(&none);
        for key in ["ctrl-k", "ctrl-p", "ctrl-n", "ctrl-b", "ctrl-u", "ctrl-e", "ctrl-g", "ctrl-o", "ctrl-l", "ctrl-j", "ctrl-v", "ctrl-enter", "escape", "tab", "shift-tab"] {
            // De fábrica Ctrl es ⌘ en macOS; los extras del shell siguen con Ctrl.
            assert!(shell.iter().any(|k| *k == key || *k == p(key)), "{key} falta en la terminal");
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(shell.len(), 15);
            assert_eq!(editor_keys(&none).len(), 11);
        }
        let map = saved(&[("palette", "ctrl-m")]);
        assert!(terminal_keys(&map).iter().any(|k| k == "ctrl-m"));
        assert!(!terminal_keys(&map).iter().any(|k| *k == p("ctrl-k")));
    }
}
