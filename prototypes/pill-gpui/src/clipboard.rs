//! El Clipboard como notch: la franja del notch es el buscador, debajo va una
//! tira con las imágenes y colores, y después los textos en una línea,
//! agrupados por día. El alto se ajusta a lo que hay que mostrar.
//!
//! Lee el historial real de Atic (`history.rs`); sin él, usa entradas de
//! prueba. Con la pill nativa, favoritos y borrados van a `history.json`
//! (`clipboard_owner`); si no, a un archivo propio (`local.json`), porque el
//! historial es de la app de Tauri.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    actions, div, img, list, prelude::*, px, rgb, svg, AnyElement, App, ClickEvent, Context,
    Entity, EventEmitter, FocusHandle, Focusable, Hsla, Image, ImageFormat, KeyBinding,
    ListAlignment, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    RenderImage, ScrollHandle, SharedString, StyledImage, Subscription, Window,
};
use serde::{Deserialize, Serialize};

use crate::hover::{round_button, HoverExt};
use crate::history;
use crate::secrets;
use crate::text_input::{self, TextInput};

/// Ancho del notch abierto: con 312 px el texto se cortaba a los 30 caracteres.
pub const PANEL_W: f32 = 440.0;
/// Alto máximo; con menos entradas el notch crece menos.
pub const PANEL_H: f32 = 460.0;
/// La franja del notch, que con el panel abierto es el buscador.
pub const BAND_H: f32 = 40.0;
/// Hueco a la izquierda del buscador donde el notch pinta su marca.
pub(crate) const MARK_GAP: f32 = 40.0;
const STRIP_H: f32 = 52.0;
const STRIP_PAD: f32 = 10.0;
/// Un texto más largo que esto merece vista previa: en la fila no cabe.
const PREVIEW_MIN_CHARS: usize = 56;
const THUMB_W: f32 = 78.0;
const SWATCH_W: f32 = 52.0;
const HEADER_H: f32 = 22.0;
const ROW_H: f32 = 30.0;
const FOOTER_H: f32 = 26.0;
const EMPTY_H: f32 = 90.0;
const SIDE_PAD: f32 = 8.0;
/// Lo que hay que mover el cursor con el botón apretado para que sea arrastre
/// y no clic (`ClipboardHistoryList.svelte`).
const DRAG_THRESHOLD: f32 = 6.0;
/// Cada cuánto se mira si Atic reescribió `history.json` (su watcher corre a
/// 450 ms).
const RELOAD_EVERY: Duration = Duration::from_millis(1000);

actions!(
    clipboard_panel,
    [
        SelectPrev,
        SelectNext,
        Confirm,
        Dismiss,
        ToggleFavorite,
        Remove,
        DrawImage,
        OpenImage,
        ReadText,
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

const KEY_CONTEXT: &str = "ClipboardPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("ctrl-d", ToggleFavorite, context),
        KeyBinding::new("shift-delete", Remove, context),
        KeyBinding::new("ctrl-e", DrawImage, context),
        KeyBinding::new("ctrl-o", OpenImage, context),
        KeyBinding::new("ctrl-t", ReadText, context),
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

#[derive(Clone)]
pub enum Content {
    Text(SharedString),
    Color(SharedString, Hsla),
    Image(Picture),
}

/// Una imagen del historial. Las de Atic se leen del disco recién al pintarlas
/// o pegarlas: son hasta 100 PNG de hasta 8 MB.
#[derive(Clone)]
pub enum Picture {
    Embedded(Arc<Image>),
    File(Arc<Path>),
}

impl Picture {
    pub fn load(&self) -> std::io::Result<Arc<Image>> {
        match self {
            Picture::Embedded(image) => Ok(image.clone()),
            Picture::File(path) => Ok(Arc::new(Image::from_bytes(
                ImageFormat::Png,
                std::fs::read(path)?,
            ))),
        }
    }

    pub fn view(&self) -> gpui::Img {
        match self {
            Picture::Embedded(image) => img(image.clone()),
            Picture::File(path) => img(path.clone()),
        }
    }
}

#[derive(Clone)]
pub struct Entry {
    pub id: usize,
    /// El `id` de Atic: sobrevive a las recargas.
    pub key: SharedString,
    pub content: Content,
    /// Una línea para la fila; con los secretos ocultos.
    pub preview: SharedString,
    /// El texto completo para la vista previa; con los secretos ocultos.
    pub shown: SharedString,
    /// Tiene algo que parece una clave.
    pub secret: bool,
    /// Lo que se ve, plegado para buscar: un secreto no se encuentra por su
    /// valor.
    pub folded: String,
    pub created_ms: u64,
    pub pinned: bool,
    /// Ícono de la app que lo copió.
    pub source_icon: Option<Arc<RenderImage>>,
}

/// Cuánto texto entra en la vista previa.
const SHOWN_CHARS: usize = 1200;

impl Entry {
    /// Imágenes y colores siempre; un texto, si no cabe en su fila.
    pub fn worth_preview(&self) -> bool {
        match self.content {
            Content::Image(_) | Content::Color(..) => true,
            Content::Text(_) => {
                self.shown.chars().count() > PREVIEW_MIN_CHARS || self.shown.contains('\n')
            }
        }
    }

    /// Comandos, código y JSON se leen mejor en monoespaciada.
    pub fn looks_like_code(&self) -> bool {
        self.preview.contains(['{', ';', '$', '\\'])
            || ["cargo ", "pnpm ", "git ", "npm "]
                .iter()
                .any(|prefix| self.preview.starts_with(prefix))
    }

    pub fn new(id: usize, key: SharedString, content: Content, text: &str) -> Self {
        let (shown, secret) = secrets::mask(text);
        let shown: String = shown.chars().take(SHOWN_CHARS).collect();
        Self {
            id,
            key,
            content,
            preview: history::one_line(&shown),
            folded: fold(&shown),
            shown: shown.into(),
            secret,
            created_ms: 0,
            pinned: false,
            source_icon: None,
        }
    }

    /// Imágenes y colores van en la tira de arriba; el resto, en la lista.
    fn in_strip(&self) -> bool {
        matches!(self.content, Content::Image(_) | Content::Color(..))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Text,
    Images,
}

pub enum PanelEvent {
    Paste(Entry),
    Drag(Entry),
    /// Dibujar sobre una imagen en la pizarra.
    Draw(Entry),
    Close,
}

/// Una fila de la lista: separador de grupo o entrada.
#[derive(Clone, Copy)]
enum Row {
    Header(&'static str),
    /// Índice en `entries` y su atajo Ctrl+1..9.
    Item(usize, Option<usize>),
}

/// Lo que se puede elegir con las flechas, en orden: primero la tira, después
/// la lista.
#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Strip(usize),
    /// Índice en `rows`.
    Row(usize),
}

/// Favoritos y borrados hechos en el prototipo, entre arranques.
#[derive(Default, Serialize, Deserialize)]
struct Local {
    #[serde(default)]
    pins: HashMap<String, bool>,
    #[serde(default)]
    hidden: Vec<String>,
}

fn local_file() -> Option<PathBuf> {
    crate::paths::file("local.json")
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

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
}

pub struct ClipboardPanel {
    entries: Vec<Entry>,
    /// Índices en `entries` de la tira.
    strip: Vec<usize>,
    rows: Vec<Row>,
    picks: Vec<Pick>,
    selected: usize,
    list: ListState,
    strip_scroll: ScrollHandle,
    search: Entity<TextInput>,
    filter: Filter,
    favorites_only: bool,
    pub pinned: bool,
    /// El notch pinta su marca a la izquierda del buscador.
    pub mark_gap: bool,
    /// El tope de alto del panel: en un costado, casi toda la pantalla (lo
    /// pone la pill); si no, el de siempre.
    pub max_height: Option<f32>,
    colors: Colors,
    /// Entrada apretada y dónde, hasta que se suelta o empieza el arrastre.
    press: Option<(usize, Point<Pixels>)>,
    /// El último gesto terminó en arrastre: el clic que le sigue no pega.
    dragged: bool,
    /// Un aviso en el pie («Texto copiado»), con cuándo apareció.
    note: Option<(SharedString, std::time::Instant)>,
    /// Leyendo el texto de una imagen: no se lanza otra lectura.
    reading: bool,
    local: Local,
    _search_changed: Subscription,
}

impl EventEmitter<PanelEvent> for ClipboardPanel {}

impl Focusable for ClipboardPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl ClipboardPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let colors = Colors {
            text: rgb(0xf0f0ea).into(),
            muted: rgb(0x9a9a90).into(),
            faint: rgb(0x6e6e66).into(),
        };
        let search = cx.new(|cx| {
            TextInput::new(
                "Buscar en el portapapeles…",
                colors.text,
                colors.muted,
                colors.text,
                cx,
            )
        });
        let search_changed = cx.subscribe(&search, |panel, _, _: &text_input::Changed, cx| {
            panel.refilter(cx);
        });
        let dir = history::dir();
        let loaded = dir
            .as_deref()
            .and_then(|dir| history::load_if_changed(dir, None));
        let seen = loaded.as_ref().map(|(stamp, _)| *stamp);
        let entries = match loaded {
            Some((_, entries)) => {
                println!("clipboard: {} entradas de Atic", entries.len());
                entries
            }
            None => {
                println!("clipboard: sin history.json de Atic, datos de prueba");
                mock_entries()
            }
        };
        if let Some(dir) = dir {
            Self::watch(dir, seen, cx);
        }
        let mut panel = Self {
            entries: Vec::new(),
            strip: Vec::new(),
            rows: Vec::new(),
            picks: Vec::new(),
            selected: 0,
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            strip_scroll: ScrollHandle::new(),
            search,
            filter: Filter::All,
            favorites_only: false,
            pinned: false,
            mark_gap: true,
            max_height: None,
            colors,
            press: None,
            dragged: false,
            note: None,
            reading: false,
            local: Local::load(),
            _search_changed: search_changed,
        };
        panel.set_entries(entries, cx);
        panel
    }

    /// Al abrir: buscador vacío y la lista arriba, como en Atic.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.clear(cx));
        self.refilter(cx);
    }

    /// Recarga cuando Atic reescribe el historial. Leer y parsear va en otro
    /// hilo; aquí solo se cambian las entradas.
    fn watch(dir: PathBuf, mut seen: Option<history::Stamp>, cx: &mut Context<Self>) {
        let dir: Arc<Path> = dir.into();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(RELOAD_EVERY).await;
            let dir = dir.clone();
            let loaded = cx
                .background_spawn(async move { history::load_if_changed(&dir, seen) })
                .await;
            let alive = match loaded {
                Some((stamp, entries)) => {
                    seen = Some(stamp);
                    this.update(cx, |panel, cx| {
                        println!("clipboard: recargado, {} entradas", entries.len());
                        panel.set_entries(entries, cx)
                    })
                    .is_ok()
                }
                None => this.upgrade().is_some(),
            };
            if !alive {
                break;
            }
        })
        .detach();
    }

    /// Cambia las entradas sin perder la elegida.
    fn set_entries(&mut self, mut entries: Vec<Entry>, cx: &mut Context<Self>) {
        let selected_key = self.selected_entry().map(|index| self.entries[index].key.clone());
        entries.retain(|entry| !self.local.hidden.iter().any(|key| key.as_str() == entry.key.as_ref()));
        for entry in &mut entries {
            if let Some(&pinned) = self.local.pins.get(entry.key.as_ref()) {
                entry.pinned = pinned;
            }
        }
        self.entries = entries;
        self.rebuild(cx);
        self.selected = selected_key
            .and_then(|key| {
                (0..self.picks.len()).find(|&pick| {
                    self.entry_of(pick)
                        .is_some_and(|index| self.entries[index].key == key)
                })
            })
            .unwrap_or_else(|| self.first_pick());
        cx.notify();
    }

    /// Filtro nuevo: la selección vuelve al primer texto (o a la primera
    /// imagen si solo hay imágenes). Así la vista previa de imagen no empuja la
    /// lista apenas se abre.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.rebuild(cx);
        self.selected = self.first_pick();
        self.list.scroll_to_reveal_item(0);
        self.strip_scroll.scroll_to_item(0);
        cx.notify();
    }

    fn first_pick(&self) -> usize {
        if self.filter == Filter::Images {
            return 0;
        }
        self.picks
            .iter()
            .position(|pick| matches!(pick, Pick::Row(_)))
            .unwrap_or(0)
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let query = fold(self.search.read(cx).text());
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let now = chrono::Local::now();
        let matches = |entry: &Entry| {
            (!self.favorites_only || entry.pinned)
                && (entry.folded.contains(query.trim())
                    || tokens.iter().all(|token| entry.folded.contains(token)))
        };

        self.strip = if self.filter == Filter::Text {
            Vec::new()
        } else {
            (0..self.entries.len())
                .filter(|&index| self.entries[index].in_strip() && matches(&self.entries[index]))
                .collect()
        };

        let texts: Vec<usize> = if self.filter == Filter::Images {
            Vec::new()
        } else {
            (0..self.entries.len())
                .filter(|&index| !self.entries[index].in_strip() && matches(&self.entries[index]))
                .collect()
        };
        // Favoritos arriba y fijos; el resto por día. Atic ya los guarda del
        // más nuevo al más viejo.
        let mut rows = Vec::new();
        let mut quick = 0;
        let mut push = |rows: &mut Vec<Row>, index: usize| {
            quick += 1;
            rows.push(Row::Item(index, (quick <= 9).then_some(quick)));
        };
        let favorites: Vec<usize> = texts
            .iter()
            .copied()
            .filter(|&index| self.entries[index].pinned)
            .collect();
        if !favorites.is_empty() && !self.favorites_only {
            rows.push(Row::Header("Favoritos"));
        }
        for &index in &favorites {
            push(&mut rows, index);
        }
        let mut day = None;
        for &index in &texts {
            if self.entries[index].pinned {
                continue;
            }
            let this_day = history::day_of(self.entries[index].created_ms, now);
            if day != Some(this_day) {
                rows.push(Row::Header(this_day.label()));
                day = Some(this_day);
            }
            push(&mut rows, index);
        }
        self.rows = rows;
        self.list.reset(self.rows.len());

        self.picks = (0..self.strip.len())
            .map(Pick::Strip)
            .chain(
                self.rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| matches!(row, Row::Item(..)))
                    .map(|(index, _)| Pick::Row(index)),
            )
            .collect();
    }

    /// Lo que la vista previa flotante muestra: la entrada elegida, si vale la
    /// pena verla en grande.
    pub fn preview(&self) -> Option<&Entry> {
        let entry = &self.entries[self.selected_entry()?];
        entry.worth_preview().then_some(entry)
    }

    /// Las últimas copiadas, para el vistazo.
    pub fn recent(&self, count: usize) -> Vec<Entry> {
        self.entries.iter().take(count).cloned().collect()
    }

    fn row_height(&self, row: usize) -> f32 {
        match self.rows[row] {
            Row::Header(_) => HEADER_H,
            Row::Item(..) => ROW_H,
        }
    }

    /// El alto que necesita el notch para mostrar lo que hay, con la franja.
    pub fn desired_height(&self) -> f32 {
        let strip = if self.strip.is_empty() {
            0.0
        } else {
            STRIP_H + STRIP_PAD
        };
        let list: f32 = (0..self.rows.len()).map(|row| self.row_height(row)).sum();
        let body = if self.picks.is_empty() {
            EMPTY_H
        } else {
            strip + list
        };
        (BAND_H + body + FOOTER_H + SIDE_PAD).min(self.max_height.unwrap_or(PANEL_H))
    }

    fn entry_of(&self, pick: usize) -> Option<usize> {
        match *self.picks.get(pick)? {
            Pick::Strip(slot) => self.strip.get(slot).copied(),
            Pick::Row(row) => match self.rows[row] {
                Row::Item(index, _) => Some(index),
                Row::Header(_) => None,
            },
        }
    }

    fn selected_entry(&self) -> Option<usize> {
        self.entry_of(self.selected)
    }

    fn select(&mut self, pick: usize, cx: &mut Context<Self>) {
        self.select_with(pick, true, cx);
    }

    /// Con el cursor no se desplaza la lista: lo que está bajo el cursor ya se
    /// ve, y moverla haría saltar la fila.
    fn select_with(&mut self, pick: usize, reveal: bool, cx: &mut Context<Self>) {
        let pick = pick.min(self.picks.len().saturating_sub(1));
        if pick == self.selected {
            return;
        }
        self.selected = pick;
        if reveal {
            match self.picks.get(pick) {
                Some(Pick::Row(row)) => self.list.scroll_to_reveal_item(*row),
                Some(Pick::Strip(slot)) => self.strip_scroll.scroll_to_item(*slot),
                None => {}
            }
        }
        cx.notify();
    }

    fn hover_pick(&mut self, target: Pick, cx: &mut Context<Self>) {
        if let Some(pick) = self.picks.iter().position(|p| *p == target) {
            self.select_with(pick, false, cx);
        }
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.saturating_sub(1), cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected + 1, cx);
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected_entry() {
            self.paste(index, cx);
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        if !self.pinned {
            cx.emit(PanelEvent::Close);
        }
    }

    fn toggle_selected(&mut self, _: &ToggleFavorite, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected_entry() {
            self.toggle_favorite(index, cx);
        }
    }

    /// Ctrl+E: la imagen elegida, a la pizarra.
    fn draw_selected(&mut self, _: &DrawImage, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected_entry() {
            self.draw(index, cx);
        }
    }

    /// Ctrl+O: la imagen elegida, en el visor de imágenes del sistema.
    fn open_selected(&mut self, _: &OpenImage, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected_entry() {
            self.open_image(index, cx);
        }
    }

    fn open_image(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Content::Image(Picture::File(path))) = self.entries.get(index).map(|e| &e.content) else {
            return;
        };
        // `explorer <archivo>` lo abre con la app predeterminada.
        if let Err(error) = std::process::Command::new("explorer").arg(path.as_ref()).spawn() {
            eprintln!("portapapeles: no se pudo abrir la imagen: {error}");
            self.show_note("No se pudo abrir la imagen", cx);
        }
    }

    /// Ctrl+T: el texto de la imagen elegida, al portapapeles.
    fn read_selected(&mut self, _: &ReadText, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected_entry() {
            self.read_text(index, cx);
        }
    }

    fn read_text(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Content::Image(picture)) = self.entries.get(index).map(|e| e.content.clone()) else {
            return;
        };
        if self.reading {
            return;
        }
        self.reading = true;
        self.show_note("Leyendo el texto…", cx);
        cx.spawn(async move |this, cx| {
            let text = cx
                .background_spawn(async move {
                    let image = picture.load().map_err(|error| error.to_string())?;
                    let mut bgra = image::load_from_memory(&image.bytes)
                        .map_err(|error| error.to_string())?
                        .to_rgba8();
                    let (width, height) = bgra.dimensions();
                    for pixel in bgra.chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    crate::ocr::recognize(width, height, &bgra)
                })
                .await;
            let _ = this.update(cx, |panel, cx| {
                panel.reading = false;
                match text {
                    Ok(text) if !text.trim().is_empty() => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        panel.show_note("Texto copiado", cx);
                    }
                    Ok(_) => panel.show_note("No encontré texto en la imagen", cx),
                    Err(error) => {
                        eprintln!("portapapeles: OCR: {error}");
                        panel.show_note("No se pudo leer el texto", cx);
                    }
                }
            });
        })
        .detach();
    }

    /// Un aviso en el pie por unos segundos.
    fn show_note(&mut self, text: &'static str, cx: &mut Context<Self>) {
        const NOTE_FOR: std::time::Duration = std::time::Duration::from_secs(3);
        self.note = Some((text.into(), std::time::Instant::now()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_FOR).await;
            let _ = this.update(cx, |panel, cx| {
                if panel.note.as_ref().is_some_and(|(_, at)| at.elapsed() >= NOTE_FOR) {
                    panel.note = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn draw(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get(index).filter(|e| matches!(e.content, Content::Image(_))) {
            cx.emit(PanelEvent::Draw(entry.clone()));
        }
    }

    fn remove_selected(&mut self, _: &Remove, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected_entry() {
            let keep = self.selected;
            self.remove(index, cx);
            self.select(keep, cx);
        }
    }

    /// Ctrl+N pega el N-ésimo texto de la lista.
    fn quick(&mut self, number: usize, cx: &mut Context<Self>) {
        let index = self.rows.iter().find_map(|row| match *row {
            Row::Item(index, Some(n)) if n == number => Some(index),
            _ => None,
        });
        if let Some(index) = index {
            self.paste(index, cx);
        }
    }

    fn paste(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get(index) {
            cx.emit(PanelEvent::Paste(entry.clone()));
        }
    }

    fn maybe_start_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((index, origin)) = self.press else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.press = None;
            return;
        }
        let delta = event.position - origin;
        let distance = f32::from(delta.x).hypot(f32::from(delta.y));
        if distance < DRAG_THRESHOLD {
            return;
        }
        self.press = None;
        self.dragged = true;
        if let Some(entry) = self.entries.get(index) {
            cx.emit(PanelEvent::Drag(entry.clone()));
        }
    }

    fn toggle_favorite(&mut self, index: usize, cx: &mut Context<Self>) {
        let selected_key = self.entries[index].key.clone();
        let entry = &mut self.entries[index];
        entry.pinned = !entry.pinned;
        if !crate::clipboard_owner::set_pinned(&entry.key, entry.pinned) {
            self.local
                .pins
                .insert(entry.key.to_string(), entry.pinned);
            self.local.save();
        }
        // La entrada cambia de grupo: la selección la sigue.
        self.rebuild(cx);
        if let Some(pick) = (0..self.picks.len()).find(|&pick| {
            self.entry_of(pick)
                .is_some_and(|i| self.entries[i].key == selected_key)
        }) {
            self.select(pick, cx);
        }
        cx.notify();
    }

    /// Cuántas entradas se borraron aquí (siguen en `history.json`).
    pub fn hidden_count(&self) -> usize {
        self.local.hidden.len()
    }

    /// Vuelve a mostrar lo borrado aquí, releyendo el historial de Atic.
    pub fn unhide_all(&mut self, cx: &mut Context<Self>) {
        self.local.hidden.clear();
        self.local.save();
        let Some(dir) = history::dir() else {
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            let loaded = cx.background_spawn(async move { history::load_if_changed(&dir, None) }).await;
            if let Some((_, entries)) = loaded {
                this.update(cx, |panel, cx| panel.set_entries(entries, cx)).ok();
            }
        })
        .detach();
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        let entry = self.entries.remove(index);
        if !crate::clipboard_owner::delete(&entry.key) {
            self.local.hidden.push(entry.key.to_string());
            self.local.save();
        }
        self.rebuild(cx);
        cx.notify();
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        // Un segundo clic en el filtro activo lo apaga.
        self.filter = if self.filter == filter {
            Filter::All
        } else {
            filter
        };
        self.refilter(cx);
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
        let colors = &self.colors;
        let (text, faint) = (colors.text, colors.faint);
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

    /// Mouse sobre una entrada: apretar arma el arrastre, soltar sin moverse
    /// pega.
    fn pressable<E: InteractiveElement + StatefulInteractiveElement>(
        &self,
        element: E,
        index: usize,
        cx: &mut Context<Self>,
    ) -> E {
        element
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |panel, event: &MouseDownEvent, _, _| {
                    panel.press = Some((index, event.position));
                    panel.dragged = false;
                }),
            )
            .on_mouse_move(cx.listener(|panel, event: &MouseMoveEvent, _, cx| {
                panel.maybe_start_drag(event, cx)
            }))
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                panel.press = None;
                if !panel.dragged {
                    panel.paste(index, cx)
                }
            }))
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
                // El hueco de la marca también cierra, como clic en la marca.
                el.child(
                    div()
                        .id("clip-mark")
                        .w(px(MARK_GAP))
                        .h_full()
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(PanelEvent::Close)
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
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child(self.search.clone()),
            )
            .child(self.icon_button(
                "clip-text",
                "icons/type.svg",
                "Solo texto",
                self.filter == Filter::Text,
                |panel, cx| panel.set_filter(Filter::Text, cx),
                cx,
            ))
            .child(self.icon_button(
                "clip-images",
                "icons/image.svg",
                "Solo imágenes",
                self.filter == Filter::Images,
                |panel, cx| panel.set_filter(Filter::Images, cx),
                cx,
            ))
            .child(self.icon_button(
                "clip-favorites",
                "icons/star.svg",
                "Favoritos",
                self.favorites_only,
                |panel, cx| {
                    panel.favorites_only = !panel.favorites_only;
                    panel.refilter(cx);
                },
                cx,
            ))
            .child(self.icon_button(
                "clip-pin",
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

    fn render_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.strip.is_empty() {
            return None;
        }
        let colors = &self.colors;
        let (text, faint) = (colors.text, colors.faint);
        let selected = self.picks.get(self.selected).copied();
        let tiles = self
            .strip
            .iter()
            .enumerate()
            .map(|(slot, &index)| {
                let entry = &self.entries[index];
                let is_selected = selected == Some(Pick::Strip(slot));
                let tile = div()
                    .id(("clip-tile", entry.id))
                    .h(px(STRIP_H))
                    .flex_none()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .border_2()
                    // Pasar el cursor la elige: la vista previa la sigue.
                    .on_hover(cx.listener(move |panel, hovered: &bool, _, cx| {
                        if *hovered {
                            panel.hover_pick(Pick::Strip(slot), cx);
                        }
                    }));
                let is_image = matches!(entry.content, Content::Image(_));
                let tile = match &entry.content {
                    Content::Image(picture) => tile.w(px(THUMB_W)).bg(text.opacity(0.06)).child(
                        picture
                            .view()
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            // Un PNG grande tarda en decodificarse; uno borrado
                            // no llega nunca.
                            .with_loading(move || div().size_full().into_any_element())
                            .with_fallback(move || {
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        svg().path("icons/image.svg").size(px(14.)).text_color(faint),
                                    )
                                    .into_any_element()
                            }),
                    ),
                    Content::Color(_, color) => tile.w(px(SWATCH_W)).bg(*color),
                    Content::Text(_) => tile,
                };
                // Con el cursor encima: abrir en el visor (Ctrl+O), leer el
                // texto (Ctrl+T) y dibujar en la pizarra (Ctrl+E).
                let button = |id: &'static str, icon: &'static str, action: fn(&mut Self, usize, &mut Context<Self>)| {
                    div()
                        .id((id, entry.id))
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(11.))
                        .bg(gpui::black().opacity(0.6))
                        .hover(|style| style.bg(gpui::black().opacity(0.85)))
                        .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            action(panel, index, cx)
                        }))
                        .child(svg().path(icon).size(px(12.)).text_color(gpui::white()))
                };
                let buttons = is_image.then(|| {
                    div()
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .flex()
                        .gap(px(3.))
                        .opacity(0.0)
                        .group_hover("clip-tile", |style| style.opacity(1.0))
                        .child(button("clip-open", "icons/external-link.svg", Self::open_image))
                        .child(button("clip-text", "icons/scan-text.svg", Self::read_text))
                        .child(button("clip-draw", "icons/pencil.svg", Self::draw))
                });
                let tile = tile.relative().group("clip-tile").children(buttons);
                self.pressable(tile, index, cx)
                    .fx(("clip-tile-fx", entry.id), move |el, h| {
                        // Elegida, borde lleno; con el cursor encima, a medias.
                        el.border_color(text.opacity(if is_selected { 0.9 } else { 0.35 * h.over }))
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        // La rueda del mouse avanza la tira de lado: GPUI la pasa al eje x
        // cuando solo ese tiene scroll.
        Some(
            div()
                .id("clip-strip")
                .flex_none()
                .flex()
                .gap(px(6.))
                .px(px(SIDE_PAD))
                .mb(px(STRIP_PAD))
                .overflow_x_scroll()
                .track_scroll(&self.strip_scroll)
                .children(tiles),
        )
    }

    fn render_row(&self, row: usize, now: chrono::DateTime<chrono::Local>, cx: &mut Context<Self>) -> AnyElement {
        let colors = &self.colors;
        let (text, muted, faint) = (colors.text, colors.muted, colors.faint);
        let (index, quick) = match self.rows[row] {
            Row::Header(label) => {
                return div()
                    .w_full()
                    .h(px(HEADER_H))
                    .px(px(SIDE_PAD + 8.))
                    .flex()
                    .items_end()
                    .pb(px(3.))
                    .text_size(px(10.))
                    .text_color(faint)
                    .child(label)
                    .into_any_element()
            }
            Row::Item(index, quick) => (index, quick),
        };
        let entry = &self.entries[index];
        let selected = self.picks.get(self.selected) == Some(&Pick::Row(row));
        let secret = entry.secret;
        let pinned = entry.pinned;
        let looks_like_code = entry.looks_like_code();

        let actions = div()
            .absolute()
            .right(px(0.))
            .top(px(0.))
            .h_full()
            .flex()
            .items_center()
            .child(
                div()
                    .id(("clip-star", entry.id))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(11.))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        panel.toggle_favorite(index, cx)
                    }))
                    .child(
                        svg()
                            .path("icons/star.svg")
                            .size(px(12.))
                            .text_color(if pinned { text } else { muted }),
                    )
                    .hover_bg(("clip-star-fx", entry.id), text.opacity(0.0), text.opacity(0.1)),
            )
            .child(
                div()
                    .id(("clip-remove", entry.id))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(11.))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        panel.remove(index, cx)
                    }))
                    .child(svg().path("icons/x.svg").size(px(12.)).text_color(muted))
                    .hover_bg(("clip-remove-fx", entry.id), text.opacity(0.0), text.opacity(0.1)),
            );

        let row_el = div()
            .id(("clip-row", entry.id))
            .w_full()
            .h(px(ROW_H))
            .px(px(8.))
            .flex()
            .items_center()
            .on_hover(cx.listener(move |panel, hovered: &bool, _, cx| {
                if *hovered {
                    panel.hover_pick(Pick::Row(row), cx);
                }
            }))
            .gap(px(8.))
            .rounded(px(10.))
            .cursor_pointer()
            .child(
                div()
                    .w(px(10.))
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(faint)
                    .children(quick.map(|n| n.to_string())),
            )
            // De dónde vino: el ícono de la app, o el de texto si Atic no lo
            // registró (entradas anteriores a `sourceApp`).
            .child(match entry.source_icon.clone() {
                Some(icon) => img(icon).size(px(14.)).flex_none().into_any_element(),
                None => svg()
                    .path("icons/type.svg")
                    .size(px(12.))
                    .mx(px(1.))
                    .flex_none()
                    .text_color(faint)
                    .into_any_element(),
            })
            .when(secret, |el| {
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
                    .text_size(px(12.))
                    .when(looks_like_code, |el| {
                        el.font_family("Cascadia Mono").text_size(px(11.))
                    })
                    .truncate()
                    .child(entry.preview.clone()),
            );
        let when = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(10.))
            .text_color(faint)
            .when(pinned, |el| el.child(svg().path("icons/star.svg").size(px(10.)).text_color(muted)))
            .child(history::short_when(entry.created_ms, now));
        // `list` mide cada fila por su contenido: sin `w_full` la fila queda
        // del ancho de su texto y la hora flota al medio.
        div()
            .w_full()
            .px(px(SIDE_PAD))
            .child(
                self.pressable(row_el, index, cx)
                    .fx(("clip-row-fx", entry.id), move |el, h| {
                        // La hora se va y las acciones llegan cruzándose,
                        // como `.clip-quick` en la web.
                        el.bg(h.mix(text.opacity(0.0), text.opacity(0.08))).child(
                            div()
                                .relative()
                                .w(px(48.))
                                .h_full()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_end()
                                .child(when.opacity(1.0 - h.over))
                                .child(
                                    actions
                                        .opacity(h.over)
                                        .when(h.over < 0.05, |el| el.invisible()),
                                ),
                        )
                    })
                    .lit(selected),
            )
            .into_any_element()
    }

    fn empty_message(&self) -> &'static str {
        if self.entries.is_empty() {
            "El historial está vacío. Copia algo y aparece aquí."
        } else if self.favorites_only && !self.entries.iter().any(|entry| entry.pinned) {
            "No hay favoritos. Márcalos con la estrella o Ctrl+D."
        } else {
            "Nada coincide"
        }
    }
}

impl Render for ClipboardPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let (text, faint) = (colors.text, colors.faint);

        let body = if self.picks.is_empty() {
            div()
                .h(px(EMPTY_H))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(colors.muted)
                .child(self.empty_message())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .children(self.render_strip(cx))
                .when(!self.rows.is_empty(), |el| {
                    el.child(
                        list(
                            self.list.clone(),
                            cx.processor(|panel, row: usize, _, cx| {
                                panel.render_row(row, chrono::Local::now(), cx)
                            }),
                        )
                        .w_full()
                        .flex_1(),
                    )
                })
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
            .child(match &self.note {
                Some((note, _)) => note.clone(),
                None if self
                    .selected_entry()
                    .is_some_and(|index| matches!(self.entries[index].content, Content::Image(_))) =>
                {
                    "↵ pegar · Ctrl+O abrir · Ctrl+T texto · Ctrl+E dibujar · arrastra a otra app".into()
                }
                None => "↵ pegar · Ctrl+1–9 directo · Ctrl+D favorito · Mayús+Supr quitar · arrastra a otra app".into(),
            });

        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::toggle_selected))
            .on_action(cx.listener(Self::draw_selected))
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::read_selected))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(|panel, _: &Quick1, _, cx| panel.quick(1, cx)))
            .on_action(cx.listener(|panel, _: &Quick2, _, cx| panel.quick(2, cx)))
            .on_action(cx.listener(|panel, _: &Quick3, _, cx| panel.quick(3, cx)))
            .on_action(cx.listener(|panel, _: &Quick4, _, cx| panel.quick(4, cx)))
            .on_action(cx.listener(|panel, _: &Quick5, _, cx| panel.quick(5, cx)))
            .on_action(cx.listener(|panel, _: &Quick6, _, cx| panel.quick(6, cx)))
            .on_action(cx.listener(|panel, _: &Quick7, _, cx| panel.quick(7, cx)))
            .on_action(cx.listener(|panel, _: &Quick8, _, cx| panel.quick(8, cx)))
            .on_action(cx.listener(|panel, _: &Quick9, _, cx| panel.quick(9, cx)))
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

/// Minúsculas y sin tildes, como `clipboardSearch.ts`.
pub fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// 100 entradas (el máximo de Atic) para probar sin el historial real.
fn mock_entries() -> Vec<Entry> {
    let texts = [
        "Reunión con el equipo de plataforma el jueves a las 10:30",
        "https://github.com/zed-industries/zed/tree/main/crates/gpui",
        "SELECT id, nombre, db_name FROM mantenedor.cliente WHERE activo = 1;",
        "pnpm --dir apps/desktop exec vitest run src/lib/clipboardSearch.test.ts",
        "Gracias por el envío, lo reviso mañana temprano y te confirmo.",
        "cargo test --locked -p atic-core historial",
        "Dirección: Av. Providencia 1234, oficina 56, Santiago",
        "fn main() { println!(\"hola desde la pill\"); }",
        "El informe trimestral quedó en la carpeta compartida de Operaciones",
        "calcantara@ejemplo.cl",
        "https://claude.ai/code",
        "Pendiente: revisar la animación de la rueda en monitores 150 %",
        "TODO(pill): mover la isla cuando hay notch",
        "Contraseña del wifi de invitados: pídela en recepción",
        "{ \"tema\": \"atic\", \"modo\": \"oscuro\", \"rueda\": 9 }",
        "Número de seguimiento: 7AB3-55Q2-910Z",
    ];
    let colors = ["#e85a52", "#6faf88", "rgb(212, 168, 75)", "#8fa9b8", "#1a1a18"];
    let images = [
        (
            "Captura · gráfico de ventas",
            history::embedded(include_bytes!("../assets/mock/grafico.png")),
        ),
        (
            "Captura · ventana de la app",
            history::embedded(include_bytes!("../assets/mock/ventana.png")),
        ),
        (
            "atardecer.png",
            history::embedded(include_bytes!("../assets/mock/atardecer.png")),
        ),
        (
            "Ícono de Atic",
            history::embedded(include_bytes!("../assets/mock/icono.png")),
        ),
    ];

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    (0..100)
        .map(|id| {
            let (content, preview): (Content, SharedString) = match id % 9 {
                4 => {
                    let label = colors[id % colors.len()];
                    let color = history::parse_color(label).unwrap_or_default();
                    (Content::Color(label.into(), color), label.into())
                }
                7 => {
                    let (name, picture) = &images[id % images.len()];
                    (Content::Image(picture.clone()), (*name).into())
                }
                _ => {
                    let text: SharedString = texts[id % texts.len()].into();
                    (Content::Text(text.clone()), text)
                }
            };
            Entry {
                // Cada entrada, unos 37 minutos antes que la anterior.
                created_ms: now_ms.saturating_sub(id as u64 * 37 * 60_000),
                pinned: id % 13 == 2,
                ..Entry::new(id, format!("mock-{id}").into(), content, &preview)
            }
        })
        .collect()
}
