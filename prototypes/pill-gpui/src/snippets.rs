//! Textos como notch: la franja es el buscador y debajo va la lista de
//! fragmentos guardados. Pegar un fragmento expande sus variables
//! (`{fecha}`, `{hora}`, `{portapapeles}`…).
//!
//! Tiene tres pestañas, como Textos en Atic: los fragmentos, el **bloc** (una
//! sola nota que se guarda sola, en el mismo `scratchpad.json` que usa Atic) y
//! el **tablero** del flip (sus notas de texto y listas, para pegarlas; el giro
//! de la ventana no está en el prototipo).
//!
//! Lee `snippets.json` de Atic (`snippets.rs` del backend) y nunca lo escribe:
//! Atic lo tiene en memoria y lo reescribiría entero. Lo que se crea o borra
//! aquí va a `snippets-local.json`, igual que el Clipboard con `local.json`.
//! Sin ningún archivo usa fragmentos de prueba.

use std::path::PathBuf;
use std::time::SystemTime;

use chrono::Local as Now;
use gpui::{
    actions, div, list, prelude::*, px, rgb, svg, AnyElement, App, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, ListAlignment, ListState,
    SharedString, Subscription, Window,
};
use serde::{Deserialize, Serialize};

use crate::hover::{round_button, HoverExt};
use crate::clipboard::{fold, BAND_H, PANEL_H};
use crate::secrets;
use crate::text_area::TextArea;
use crate::text_input::{self, TextInput};

const MARK_GAP: f32 = 40.0;
const ROW_H: f32 = 36.0;
const FOOTER_H: f32 = 26.0;
const EMPTY_H: f32 = 90.0;
const SIDE_PAD: f32 = 8.0;
/// Largo del nombre derivado del cuerpo al crear desde el portapapeles.
const NAME_MAX: usize = 40;

actions!(
    snippets_panel,
    [
        SelectPrev,
        SelectNext,
        Confirm,
        Dismiss,
        NewFromClipboard,
        Remove,
        Quick1,
        Quick2,
        Quick3,
        Quick4,
        Quick5,
        Quick6,
        Quick7,
        Quick8,
        Quick9
    ]
);

const KEY_CONTEXT: &str = "SnippetsPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("ctrl-n", NewFromClipboard, context),
        KeyBinding::new("shift-delete", Remove, context),
        KeyBinding::new("ctrl-1", Quick1, context),
        KeyBinding::new("ctrl-2", Quick2, context),
        KeyBinding::new("ctrl-3", Quick3, context),
        KeyBinding::new("ctrl-4", Quick4, context),
        KeyBinding::new("ctrl-5", Quick5, context),
        KeyBinding::new("ctrl-6", Quick6, context),
        KeyBinding::new("ctrl-7", Quick7, context),
        KeyBinding::new("ctrl-8", Quick8, context),
        KeyBinding::new("ctrl-9", Quick9, context),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Textos,
    Bloc,
    Tablero,
}

/// Cuánto esperar tras la última tecla para guardar el bloc.
const PAD_SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

pub enum SnippetEvent {
    /// El texto ya con las variables expandidas.
    Paste(String),
    Close,
}

/// Un fragmento tal como lo guarda Atic.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    id: String,
    name: String,
    body: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    updated_at_ms: u64,
}

struct Snippet {
    stored: Stored,
    /// Una línea del cuerpo, con los secretos ocultos.
    preview: SharedString,
    secret: bool,
    has_vars: bool,
    folded_name: String,
    folded_aliases: Vec<String>,
    folded_body: String,
}

impl Snippet {
    fn new(stored: Stored) -> Self {
        let (shown, secret) = secrets::mask(&stored.body);
        let preview: String = shown
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(160)
            .collect();
        Self {
            has_vars: VARIABLES.iter().any(|(name, _)| stored.body.contains(name)),
            folded_name: fold(&stored.name),
            folded_aliases: stored.aliases.iter().map(|a| fold(a)).collect(),
            folded_body: fold(&stored.body),
            preview: preview.into(),
            secret,
            stored,
        }
    }

    /// Qué tan bien responde a la búsqueda; `None` si no coincide. Alias y
    /// nombre pesan más que el cuerpo, y las letras salteadas (`rvs` →
    /// «reunión de revisión») cuentan solo sobre el nombre.
    fn score(&self, query: &str) -> Option<u32> {
        if query.is_empty() {
            return Some(0);
        }
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let mut total = 0;
        for token in &tokens {
            let best = self
                .folded_aliases
                .iter()
                .filter_map(|alias| {
                    if alias == token {
                        Some(100)
                    } else if alias.starts_with(token) {
                        Some(80)
                    } else {
                        None
                    }
                })
                .max()
                .or_else(|| {
                    if self.folded_name.starts_with(token) {
                        Some(70)
                    } else if self
                        .folded_name
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|word| word.starts_with(token))
                    {
                        Some(60)
                    } else if self.folded_name.contains(token) {
                        Some(45)
                    } else if self.folded_body.contains(token) {
                        Some(30)
                    } else if is_subsequence(token, &self.folded_name) {
                        Some(15)
                    } else {
                        None
                    }
                })?;
            total += best;
        }
        Some(total)
    }
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut hay = haystack.chars();
    needle.chars().all(|n| hay.any(|h| h == n))
}

/// Variables que se expanden al pegar, con lo que se muestra en el pie.
const VARIABLES: [(&str, &str); 4] = [
    ("{fecha}", "fecha de hoy"),
    ("{hora}", "hora actual"),
    ("{fecha_hora}", "fecha y hora"),
    ("{portapapeles}", "lo que hay copiado"),
];

/// Reemplaza las variables; lo que no se reconoce queda tal cual.
pub(crate) fn expand(body: &str, clipboard: &str) -> String {
    let now = Now::now();
    body.replace("{fecha_hora}", &now.format("%d-%m-%Y %H:%M").to_string())
        .replace("{fecha}", &now.format("%d-%m-%Y").to_string())
        .replace("{hora}", &now.format("%H:%M").to_string())
        .replace("{portapapeles}", clipboard)
}

#[derive(Default, Serialize, Deserialize)]
struct Local {
    #[serde(default)]
    added: Vec<Stored>,
    #[serde(default)]
    hidden: Vec<String>,
}

fn local_file() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(base)
            .join("atic-gpui")
            .join("snippets-local.json"),
    )
}

impl Local {
    fn load() -> Self {
        local_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let Some(path) = local_file() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, raw);
        }
    }
}

/// `%APPDATA%\ciat\atic\data\snippets\snippets.json`, o la carpeta de
/// `ATIC_SNIPPETS_DIR`.
fn atic_file() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ATIC_SNIPPETS_DIR") {
        return Some(PathBuf::from(dir).join("snippets.json"));
    }
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("ciat")
            .join("atic")
            .join("data")
            .join("snippets")
            .join("snippets.json"),
    )
}

fn atic_data_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("ciat")
            .join("atic")
            .join("data"),
    )
}

/// `scratchpad.json` junto a `snippets.json`: `{ body, updatedAtMs }`.
fn pad_file() -> Option<PathBuf> {
    atic_file().map(|file| file.with_file_name("scratchpad.json"))
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pad {
    #[serde(default)]
    body: String,
    #[serde(default)]
    updated_at_ms: u64,
}

fn load_pad() -> String {
    pad_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Pad>(&raw).ok())
        .map(|pad| pad.body)
        .unwrap_or_default()
}

fn save_pad(body: &str) -> bool {
    let Some(path) = pad_file() else {
        return false;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let pad = Pad {
        body: body.to_string(),
        updated_at_ms: now_ms(),
    };
    let Ok(raw) = serde_json::to_string_pretty(&pad) else {
        return false;
    };
    // Escribir a un lado y renombrar: un corte a medias no deja el bloc roto.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, raw).is_ok() && std::fs::rename(&tmp, &path).is_ok()
}

/// Los bloques de texto y de listas del tablero (`notes/atic-tablero`).
/// Imágenes y trazos no se pegan como texto y se omiten.
fn load_board() -> Vec<Stored> {
    let dir = std::env::var_os("ATIC_NOTES_DIR")
        .map(PathBuf::from)
        .or_else(|| atic_data_dir().map(|dir| dir.join("notes")));
    let Some(file) = dir.map(|dir| dir.join("atic-tablero").join("note.json")) else {
        return Vec::new();
    };
    let Some(note) = std::fs::read_to_string(file)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    else {
        return Vec::new();
    };
    let pages = note["pages"].as_array().cloned().unwrap_or_default();
    // El tablero es la página sin título.
    let page = pages
        .iter()
        .find(|page| page["title"].as_str().is_some_and(str::is_empty))
        .or(pages.first());
    let Some(blocks) = page.and_then(|page| page["blocks"].as_array()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let (label, body) = match block["kind"].as_str()? {
                "text" => ("Nota", block["body"].as_str()?.to_string()),
                "check" => {
                    let lines: Vec<String> = block["items"]
                        .as_array()?
                        .iter()
                        .map(|item| {
                            let done = item["done"].as_bool().unwrap_or(false);
                            format!(
                                "- [{}] {}",
                                if done { "x" } else { " " },
                                item["text"].as_str().unwrap_or("")
                            )
                        })
                        .collect();
                    ("Lista", lines.join("\n"))
                }
                _ => return None,
            };
            if body.trim().is_empty() {
                return None;
            }
            let first: String = body
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(NAME_MAX)
                .collect();
            Some(Stored {
                id: format!("board-{index}"),
                name: format!("{label} · {first}"),
                body,
                aliases: Vec::new(),
                updated_at_ms: 0,
            })
        })
        .collect()
}

/// Los textos guardados como (nombre, cuerpo), del más reciente al más viejo:
/// lo mismo que muestra Textos, para el cajón del tablero.
pub(crate) fn texts() -> Vec<(String, String)> {
    let local = Local::load();
    let mut stored = load_atic().unwrap_or_else(|| {
        if local.added.is_empty() {
            mock_snippets()
        } else {
            Vec::new()
        }
    });
    stored.extend(local.added.iter().cloned());
    stored.retain(|s| !local.hidden.contains(&s.id));
    stored.sort_by_key(|s| std::cmp::Reverse(s.updated_at_ms));
    stored.into_iter().map(|s| (s.name, s.body)).collect()
}

fn load_atic() -> Option<Vec<Stored>> {
    let raw = std::fs::read_to_string(atic_file()?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
}

pub struct SnippetsPanel {
    all: Vec<Snippet>,
    /// Índices en `all`, en el orden en que se muestran.
    shown: Vec<usize>,
    selected: usize,
    list: ListState,
    search: Entity<TextInput>,
    pub pinned: bool,
    /// El notch pinta su marca a la izquierda del buscador.
    pub mark_gap: bool,
    /// El tope de alto del panel: en un costado, casi toda la pantalla (lo
    /// pone la pill); si no, el de siempre.
    pub max_height: Option<f32>,
    from_atic: bool,
    tab: Tab,
    area: Entity<TextArea>,
    /// El bloc tiene cambios sin guardar, y cuántas ediciones van (para que
    /// solo guarde la última tras una ráfaga).
    pad_dirty: bool,
    pad_edits: u64,
    pad_saved: bool,
    _area_changed: Subscription,
    colors: Colors,
    local: Local,
    _search_changed: Subscription,
}

impl EventEmitter<SnippetEvent> for SnippetsPanel {}

impl Focusable for SnippetsPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl SnippetsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let colors = Colors {
            text: rgb(0xf0f0ea).into(),
            muted: rgb(0x9a9a90).into(),
            faint: rgb(0x6e6e66).into(),
        };
        let search = cx.new(|cx| {
            TextInput::new("Buscar textos…", colors.text, colors.muted, colors.text, cx)
        });
        let search_changed = cx.subscribe(&search, |panel, _, _: &text_input::Changed, cx| {
            panel.refilter(cx);
        });
        let area = cx.new(|cx| {
            TextArea::new(
                "Escribe lo que quieras recordar…",
                colors.text,
                colors.faint,
                colors.text,
                cx,
            )
        });
        let area_changed = cx.subscribe(&area, |panel, _, _: &text_input::Changed, cx| {
            panel.pad_changed(cx);
        });
        let mut panel = Self {
            all: Vec::new(),
            shown: Vec::new(),
            selected: 0,
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            search,
            pinned: false,
            mark_gap: true,
            max_height: None,
            from_atic: false,
            tab: Tab::Textos,
            area,
            pad_dirty: false,
            pad_edits: 0,
            pad_saved: false,
            _area_changed: area_changed,
            colors,
            local: Local::load(),
            _search_changed: search_changed,
        };
        panel.reload(cx);
        panel
    }

    /// Al abrir: buscador vacío, y se relee lo que Atic haya guardado.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.tab = Tab::Textos;
        self.search.update(cx, |search, cx| search.clear(cx));
        self.reload(cx);
    }

    /// El bloc se relee al abrir, salvo que haya algo sin guardar.
    fn load_pad_into_area(&mut self, cx: &mut Context<Self>) {
        if self.pad_dirty {
            return;
        }
        let body = load_pad();
        self.area.update(cx, |area, cx| area.set_text(&body, cx));
        self.pad_saved = false;
    }

    fn pad_changed(&mut self, cx: &mut Context<Self>) {
        self.pad_dirty = true;
        self.pad_saved = false;
        self.pad_edits += 1;
        let edits = self.pad_edits;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PAD_SAVE_DELAY).await;
            let _ = this.update(cx, |panel, cx| {
                if panel.pad_edits == edits {
                    panel.save_pad_now(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn save_pad_now(&mut self, cx: &mut Context<Self>) {
        if !self.pad_dirty {
            return;
        }
        let body = self.area.read(cx).text().to_string();
        if save_pad(&body) {
            self.pad_dirty = false;
            self.pad_saved = true;
        }
        cx.notify();
    }

    fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        if self.tab == Tab::Bloc {
            self.save_pad_now(cx);
        }
        self.tab = tab;
        self.search.update(cx, |search, cx| search.clear(cx));
        match tab {
            Tab::Bloc => {
                self.load_pad_into_area(cx);
                window.focus(&self.area.focus_handle(cx));
            }
            _ => {
                self.reload(cx);
                window.focus(&self.search.focus_handle(cx));
            }
        }
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if self.tab == Tab::Bloc {
            self.load_pad_into_area(cx);
            cx.notify();
            return;
        }
        if self.tab == Tab::Tablero {
            self.all = load_board().into_iter().map(Snippet::new).collect();
            self.refilter(cx);
            return;
        }
        let atic = load_atic();
        self.from_atic = atic.is_some();
        let mut stored = atic.unwrap_or_else(|| {
            if self.local.added.is_empty() {
                mock_snippets()
            } else {
                Vec::new()
            }
        });
        stored.extend(self.local.added.iter().cloned());
        stored.retain(|s| !self.local.hidden.contains(&s.id));
        // Lo último que se tocó, primero.
        stored.sort_by_key(|s| std::cmp::Reverse(s.updated_at_ms));
        self.all = stored.into_iter().map(Snippet::new).collect();
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = fold(self.search.read(cx).text());
        let query = query.trim();
        let mut scored: Vec<(u32, usize)> = self
            .all
            .iter()
            .enumerate()
            .filter_map(|(index, snippet)| snippet.score(query).map(|score| (score, index)))
            .collect();
        // Estable: con el mismo puntaje manda el orden por reciente.
        scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
        self.shown = scored.into_iter().map(|(_, index)| index).collect();
        self.list.reset(self.shown.len());
        self.selected = 0;
        self.list.scroll_to_reveal_item(0);
        cx.notify();
    }

    /// El alto que necesita el notch, con la franja.
    pub fn desired_height(&self) -> f32 {
        if self.tab == Tab::Bloc {
            return PANEL_H;
        }
        let body = if self.shown.is_empty() {
            EMPTY_H
        } else {
            self.shown.len() as f32 * ROW_H
        };
        (BAND_H + body + FOOTER_H + SIDE_PAD).min(self.max_height.unwrap_or(PANEL_H))
    }

    fn select(&mut self, index: usize, reveal: bool, cx: &mut Context<Self>) {
        let index = index.min(self.shown.len().saturating_sub(1));
        if index == self.selected {
            return;
        }
        self.selected = index;
        if reveal {
            self.list.scroll_to_reveal_item(index);
        }
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.saturating_sub(1), true, cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected + 1, true, cx);
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.paste(self.selected, cx);
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        if !self.pinned {
            cx.emit(SnippetEvent::Close);
        }
    }

    /// Ctrl+1..9 pega la N-ésima fila visible.
    fn paste(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.tab == Tab::Bloc {
            return;
        }
        let Some(&index) = self.shown.get(row) else {
            return;
        };
        let clipboard = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        let text = expand(&self.all[index].stored.body, &clipboard);
        cx.emit(SnippetEvent::Paste(text));
    }

    /// Guarda lo copiado como fragmento nuevo, con el nombre sacado de su
    /// primera línea (como las notas de Atic: no se pide nombre antes de
    /// escribir).
    fn new_from_clipboard(&mut self, _: &NewFromClipboard, _: &mut Window, cx: &mut Context<Self>) {
        self.create_from_clipboard(cx);
    }

    fn create_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.tab != Tab::Textos {
            return;
        }
        let Some(body) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if body.trim().is_empty() {
            return;
        }
        let name: String = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(NAME_MAX)
            .collect();
        self.local.added.push(Stored {
            id: format!("local-{}", now_ms()),
            name,
            body,
            aliases: Vec::new(),
            updated_at_ms: now_ms(),
        });
        self.local.save();
        self.reload(cx);
    }

    fn remove(&mut self, row: usize, cx: &mut Context<Self>) {
        // El tablero es de Atic y el bloc no es una lista.
        if self.tab != Tab::Textos {
            return;
        }
        let Some(&index) = self.shown.get(row) else {
            return;
        };
        let id = self.all[index].stored.id.clone();
        self.local.added.retain(|s| s.id != id);
        if !self.local.hidden.contains(&id) {
            self.local.hidden.push(id);
        }
        self.local.save();
        let keep = self.selected;
        self.reload(cx);
        self.select(keep, true, cx);
    }

    fn remove_selected(&mut self, _: &Remove, _: &mut Window, cx: &mut Context<Self>) {
        self.remove(self.selected, cx);
    }

    fn icon_button(
        &self,
        id: &'static str,
        icon: &'static str,
        tip: &'static str,
        active: bool,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (text, faint) = (self.colors.text, self.colors.faint);
        round_button(
            id,
            icon,
            tip,
            active,
            text,
            faint,
            cx.listener(move |panel, _: &ClickEvent, _, cx| on_click(panel, cx)),
        )
    }

    fn tab_button(
        &self,
        id: &'static str,
        icon: &'static str,
        tab: Tab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (text, faint) = (self.colors.text, self.colors.faint);
        let active = self.tab == tab;
        let tip = match tab {
            Tab::Textos => "Textos",
            Tab::Bloc => "Bloc",
            Tab::Tablero => "Notas del tablero",
        };
        round_button(
            id,
            icon,
            tip,
            active,
            text,
            faint,
            cx.listener(move |panel, _: &ClickEvent, window, cx| panel.set_tab(tab, window, cx)),
        )
    }

    fn render_band(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(BAND_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .pr(px(SIDE_PAD))
            .when(self.mark_gap, |el| {
                el.child(
                    div()
                        .id("snip-mark")
                        .w(px(MARK_GAP))
                        .h_full()
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(SnippetEvent::Close)
                        })),
                )
            })
            .when(!self.mark_gap, |el| {
                el.pl(px(14.)).child(
                    svg()
                        .path("icons/search.svg")
                        .size(px(13.))
                        .flex_none()
                        .mr(px(6.))
                        .text_color(self.colors.faint),
                )
            })
            .child(if self.tab == Tab::Bloc {
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .text_color(self.colors.muted)
                    .child("Bloc")
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child(self.search.clone())
                    .into_any_element()
            })
            .when(self.tab == Tab::Textos, |el| {
                el.child(self.icon_button(
                    "snip-new",
                    "icons/plus.svg",
                    "Guardar lo copiado (Ctrl+N)",
                    false,
                    |panel, cx| panel.create_from_clipboard(cx),
                    cx,
                ))
            })
            .child(self.tab_button("snip-tab-textos", "icons/text-align-start.svg", Tab::Textos, cx))
            .child(self.tab_button("snip-tab-bloc", "icons/pencil.svg", Tab::Bloc, cx))
            .child(self.tab_button("snip-tab-tablero", "icons/layers.svg", Tab::Tablero, cx))
            .child(self.icon_button(
                "snip-pin",
                "icons/pin.svg",
                if self.pinned { "Soltar: se cierra al salir" } else { "Fijar: queda abierto" },
                self.pinned,
                |panel, cx| {
                    panel.pinned = !panel.pinned;
                    cx.notify();
                },
                cx,
            ))
    }

    fn render_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let (text, muted, faint) = (self.colors.text, self.colors.muted, self.colors.faint);
        let snippet = &self.all[self.shown[row]];
        let selected = row == self.selected;
        let aliases = snippet.stored.aliases.first().cloned();

        let remove = div()
            .id(("snip-remove", row))
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(11.))
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                panel.remove(row, cx)
            }))
            .child(svg().path("icons/x.svg").size(px(12.)).text_color(muted))
            .hover_bg(("snip-remove-fx", row), text.opacity(0.0), text.opacity(0.1));

        let row_el = div()
            .id(("snip-row", row))
            .w_full()
            .h(px(ROW_H))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(10.))
            .cursor_pointer()
            .on_hover(cx.listener(move |panel, hovered: &bool, _, cx| {
                if *hovered {
                    panel.select(row, false, cx);
                }
            }))
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.paste(row, cx)))
            .child(
                div()
                    .w(px(10.))
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(faint)
                    .children((row < 9).then(|| (row + 1).to_string())),
            )
            .child(
                div()
                    .max_w(px(150.))
                    .flex_none()
                    .truncate()
                    .text_size(px(12.))
                    .child(snippet.stored.name.clone()),
            )
            .when(snippet.secret, |el| {
                el.child(
                    svg()
                        .path("icons/lock.svg")
                        .size(px(11.))
                        .flex_none()
                        .text_color(muted),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(faint)
                    .child(snippet.preview.clone()),
            )
            .when(snippet.has_vars, |el| {
                el.child(div().flex_none().text_size(px(11.)).text_color(muted).child("{ }"))
            })
            .children(aliases.map(|alias| {
                div()
                    .flex_none()
                    .px(px(6.))
                    .rounded(px(6.))
                    .bg(text.opacity(0.08))
                    .text_size(px(10.))
                    .text_color(muted)
                    .child(alias)
            }))
            // La fila elegida (con el cursor o las flechas) se realza con
            // transición; la × aparece fundiéndose solo con el cursor encima.
            .fx(("snip-row-fx", row), move |el, h| {
                el.bg(h.mix(text.opacity(0.0), text.opacity(0.08))).child(
                    div()
                        .flex_none()
                        .opacity(h.over)
                        .when(h.over < 0.05, |el| el.invisible())
                        .child(remove),
                )
            })
            .lit(selected);
        div()
            .w_full()
            .px(px(SIDE_PAD))
            .child(row_el)
            .into_any_element()
    }

    fn empty_message(&self) -> &'static str {
        if self.tab == Tab::Tablero && self.all.is_empty() {
            "El tablero no tiene notas de texto."
        } else if self.all.is_empty() {
            "Sin textos. Copia algo y pulsa Ctrl+N para guardarlo."
        } else {
            "Nada coincide"
        }
    }

    /// El pie cambia según lo elegido: si tiene variables, dice cuáles.
    fn footer_text(&self, cx: &App) -> String {
        if self.tab == Tab::Bloc {
            let chars = self.area.read(cx).text().chars().count();
            let state = if self.pad_dirty {
                "Guardando…"
            } else if self.pad_saved {
                "Guardado"
            } else {
                "Se guarda solo"
            };
            return format!("{state} · {chars} caracteres");
        }
        if self.tab == Tab::Tablero {
            return "↵ pegar la nota · tablero de Atic, solo lectura".to_string();
        }
        let vars = self
            .shown
            .get(self.selected)
            .map(|&index| &self.all[index])
            .filter(|snippet| snippet.has_vars)
            .map(|snippet| {
                VARIABLES
                    .iter()
                    .filter(|(name, _)| snippet.stored.body.contains(name))
                    .map(|(name, what)| format!("{name} = {what}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            });
        match vars {
            Some(vars) => vars,
            None => {
                let origin = if self.from_atic { "" } else { " · datos de prueba" };
                format!("↵ pegar · Ctrl+1–9 directo · Ctrl+N guardar lo copiado · Mayús+Supr quitar{origin}")
            }
        }
    }
}

impl Render for SnippetsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, faint, muted) = (self.colors.text, self.colors.faint, self.colors.muted);

        let body = if self.tab == Tab::Bloc {
            div()
                .flex_1()
                .min_h_0()
                .px(px(SIDE_PAD + 8.))
                .py(px(4.))
                .text_size(px(12.))
                .line_height(px(18.))
                .child(self.area.clone())
                .into_any_element()
        } else if self.shown.is_empty() {
            div()
                .h(px(EMPTY_H))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(muted)
                .child(self.empty_message())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    list(
                        self.list.clone(),
                        cx.processor(|panel, row: usize, _, cx| panel.render_row(row, cx)),
                    )
                    .w_full()
                    .flex_1(),
                )
                .into_any_element()
        };

        let footer = div()
            .h(px(FOOTER_H))
            .flex_none()
            .flex()
            .items_center()
            .px(px(SIDE_PAD + 8.))
            .text_size(px(10.))
            .text_color(faint)
            .truncate()
            .child(self.footer_text(cx));

        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::new_from_clipboard))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(|panel, _: &Quick1, _, cx| panel.paste(0, cx)))
            .on_action(cx.listener(|panel, _: &Quick2, _, cx| panel.paste(1, cx)))
            .on_action(cx.listener(|panel, _: &Quick3, _, cx| panel.paste(2, cx)))
            .on_action(cx.listener(|panel, _: &Quick4, _, cx| panel.paste(3, cx)))
            .on_action(cx.listener(|panel, _: &Quick5, _, cx| panel.paste(4, cx)))
            .on_action(cx.listener(|panel, _: &Quick6, _, cx| panel.paste(5, cx)))
            .on_action(cx.listener(|panel, _: &Quick7, _, cx| panel.paste(6, cx)))
            .on_action(cx.listener(|panel, _: &Quick8, _, cx| panel.paste(7, cx)))
            .on_action(cx.listener(|panel, _: &Quick9, _, cx| panel.paste(8, cx)))
            .size_full()
            .flex()
            .flex_col()
            .pb(px(SIDE_PAD))
            .font_family("Segoe UI")
            .text_color(text)
            .child(self.render_band(cx))
            .child(body)
            .child(footer)
    }
}

/// Fragmentos de prueba para cuando Atic no tiene `snippets.json`.
fn mock_snippets() -> Vec<Stored> {
    let items: [(&str, &str, &[&str]); 9] = [
        (
            "Saludo formal",
            "Estimado equipo:\n\nJunto con saludar, les escribo para...\n\nSaludos cordiales,\nCarlos",
            &["hola"],
        ),
        (
            "Confirmar reunión",
            "Confirmo nuestra reunión del {fecha} a las {hora}. Quedo atento a cualquier cambio.",
            &["conf"],
        ),
        (
            "Seguimiento",
            "Hola, te escribo para saber si pudiste revisar lo que te envié. Cualquier duda me avisas.",
            &["seg"],
        ),
        (
            "Respuesta con cita",
            "Sobre lo que comentas:\n> {portapapeles}\n\nLo reviso y te respondo hoy.",
            &["cita"],
        ),
        ("Firma", "Carlos Alcántara\nTSG Enviro · Santiago de Chile", &["firma"]),
        (
            "Consulta de clientes",
            "SELECT id, nombre, db_name FROM mantenedor.cliente WHERE activo = 1;",
            &["sql"],
        ),
        (
            "Registro del día",
            "## {fecha_hora}\n- Hecho:\n- Pendiente:\n- Bloqueos:",
            &["log"],
        ),
        (
            "Correo de la oficina",
            "contacto@example.com",
            &["mail"],
        ),
        (
            "Token de prueba",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            &[],
        ),
    ];
    let now = now_ms();
    items
        .iter()
        .enumerate()
        .map(|(i, (name, body, aliases))| Stored {
            id: format!("mock-{i}"),
            name: (*name).into(),
            body: (*body).into(),
            aliases: aliases.iter().map(|a| (*a).into()).collect(),
            updated_at_ms: now - i as u64 * 3_600_000,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(name: &str, body: &str, aliases: &[&str]) -> Snippet {
        Snippet::new(Stored {
            id: name.into(),
            name: name.into(),
            body: body.into(),
            aliases: aliases.iter().map(|a| (*a).into()).collect(),
            updated_at_ms: 0,
        })
    }

    #[test]
    fn alias_beats_name_beats_body() {
        let s = snippet("Confirmar reunión", "nos vemos mañana", &["conf"]);
        let alias = s.score("conf").unwrap();
        let name = s.score("reunion").unwrap();
        let body = s.score("manana").unwrap();
        assert!(alias > name && name > body);
    }

    #[test]
    fn letters_skipped_only_match_the_name() {
        let s = snippet("Reunión de revisión", "texto", &[]);
        assert!(s.score("rvs").is_some());
        assert!(snippet("Firma", "rvs", &[]).score("rvs").is_some()); // por el cuerpo
        assert!(snippet("Firma", "otra cosa", &[]).score("rvs").is_none());
    }

    #[test]
    fn every_token_must_match() {
        let s = snippet("Saludo formal", "estimado equipo", &[]);
        assert!(s.score("saludo equipo").is_some());
        assert!(s.score("saludo zzz").is_none());
    }

    #[test]
    fn variables_expand_and_unknown_stay() {
        let out = expand("{portapapeles} {nada} {fecha}", "hola");
        assert!(out.starts_with("hola {nada} "));
        assert!(!out.contains("{fecha}"));
    }
}
