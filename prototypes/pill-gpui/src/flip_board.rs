//! El tablero del reverso del flip: una hoja de 1200×900 por página, con
//! textos, listas, imágenes y tinta que se mueven, se redimensionan y se
//! guardan solos.
//!
//! Lee y escribe el mismo `notes/atic-tablero/note.json` que usa Atic
//! (`notes.rs` del backend), con el mismo formato de bloques, así que lo que se
//! escribe aquí aparece allá y al revés. Solo toca la página sin título; el
//! resto del archivo queda como estaba.
//!
//! No está todo lo del original: faltan exportar, el cajón para insertar desde
//! el historial, las capturas y las reuniones, y el zoom libre (la hoja se
//! ajusta sola a la tarjeta).

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    actions, canvas, div, img, point, prelude::*, px, rgb, AnyElement, App, ClipboardEntry,
    ContentMask, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla, ImageFormat,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit,
    PathBuilder, PathStyle, Pixels, Point, SharedString, Subscription, Window,
};
use lyon::tessellation::{LineCap, LineJoin, StrokeOptions};
use serde::{Deserialize, Serialize};

use crate::text_area::TextArea;
use crate::text_input;

/// Página lógica, en píxeles de papel (`PAGINA_W` / `PAGINA_H`).
pub(crate) const PAGE_W: f32 = 1200.0;
pub(crate) const PAGE_H: f32 = 900.0;
const MARGIN: f32 = 24.0;
const TEXT_W: f32 = 280.0;
const TEXT_H: f32 = 96.0;
const LIST_W: f32 = 260.0;
const LIST_H: f32 = 120.0;
const MIN_W: f32 = 80.0;
const MIN_H: f32 = 40.0;
const FONT: f32 = 13.5;
const LINE: f32 = 1.5;
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;
const ROW_H: f32 = 28.0;
const TOOLBAR_H: f32 = 40.0;
const PEN_WIDTH: f32 = 2.6;
const HIGHLIGHT_WIDTH: f32 = 12.0;
const ERASER_RADIUS: f32 = 16.0;
const UNDO_MAX: usize = 60;
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// Lo que hay que mover el cursor para que un clic sea arrastre.
const DRAG_MIN: f32 = 4.0;
/// Lado del agarre de la esquina para redimensionar, en pantalla.
const HANDLE: f32 = 16.0;

/// Los lápices de Atic (`LAPICES`): los trazos guardados allá se ven igual.
/// Ancho del cajón de insertar.
const DRAWER_W: f32 = 190.0;
const MENU_W: f32 = 150.0;
const MENU_ROW: f32 = 28.0;
/// Cuánto esperar a que el overlay dibuje la página antes de capturarla.
const EXPORT_SETTLE: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, PartialEq)]
enum DrawerTab {
    Clip,
    Texts,
    Captures,
}

enum DrawerItem {
    Text { label: SharedString, body: String },
    Image { path: PathBuf },
}

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Png,
    Jpeg,
    Pdf,
    Docx,
    Pptx,
}

impl Format {
    const ALL: [Format; 5] = [Format::Png, Format::Jpeg, Format::Pdf, Format::Docx, Format::Pptx];

    /// El nombre que entiende `flip_export::export`.
    fn key(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Pptx => "pptx",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Format::Png => "Imagen PNG",
            Format::Jpeg => "Imagen JPEG",
            Format::Pdf => "PDF",
            Format::Docx => "Word",
            Format::Pptx => "PowerPoint",
        }
    }

    fn is_image(self) -> bool {
        matches!(self, Format::Png | Format::Jpeg)
    }
}

/// Lo que la vista necesita saber del monitor para capturar una página: el
/// overlay no sabe dónde está en el escritorio.
#[derive(Clone, Copy)]
pub struct Env {
    /// Tamaño lógico del monitor.
    pub screen: (f32, f32),
    /// Dónde cae el monitor en la ventana del overlay, en lógico.
    pub offset: (f32, f32),
    /// Escala de pantalla (1.0 = 100 %).
    pub dpi: f32,
    /// Esquina del monitor en el escritorio, en físico.
    pub origin_phys: (i32, i32),
}

impl Default for Env {
    fn default() -> Self {
        Self {
            screen: (1920.0, 1080.0),
            offset: (0.0, 0.0),
            dpi: 1.0,
            origin_phys: (0, 0),
        }
    }
}

const PALETTE: [u32; 10] = [
    0xe5483f, 0xd9622b, 0xd6b48a, 0x946718, 0x3f7355, 0x2f8f83, 0x526d83, 0x7a5ea8, 0xc14a7a,
    0x1c1917,
];
/// El resaltador lo guarda Atic como el color con alfa `66` (`conAlfa`).
const HIGHLIGHT_SUFFIX: &str = "66";

actions!(
    flip_paper,
    [Escape, DeleteSelected, Undo, Redo, PasteBlock, NewText, NewList]
);

const KEY_CONTEXT: &str = "FlipPaper";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Escape, context),
        KeyBinding::new("delete", DeleteSelected, context),
        KeyBinding::new("backspace", DeleteSelected, context),
        KeyBinding::new("ctrl-z", Undo, context),
        KeyBinding::new("ctrl-y", Redo, context),
        KeyBinding::new("ctrl-shift-z", Redo, context),
        KeyBinding::new("ctrl-v", PasteBlock, context),
    ]);
}

// --- Datos -----------------------------------------------------------------
//
// El mismo formato que `notes::Block` de Atic: `kind` en minúsculas y marco
// `x/y/w/h` en el tablero.

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) done: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Stroke {
    pub(crate) color: String,
    pub(crate) width: f32,
    pub(crate) points: Vec<[f32; 2]>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Body {
    Text { body: String },
    Image { asset: String, width: u32, height: u32 },
    Check { items: Vec<Item> },
    Ink { strokes: Vec<Stroke>, height: u32 },
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Block {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) x: f32,
    #[serde(default)]
    pub(crate) y: f32,
    #[serde(default)]
    pub(crate) w: f32,
    #[serde(default)]
    pub(crate) h: f32,
    #[serde(flatten)]
    pub(crate) body: Body,
}

impl Block {
    fn is_ink(&self) -> bool {
        matches!(self.body, Body::Ink { .. })
    }

    fn has_frame(&self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn new_id(counter: &mut u64) -> String {
    *counter += 1;
    format!("pill-{}-{}", now_ms(), counter)
}

pub(crate) fn notes_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ATIC_NOTES_DIR") {
        return Some(PathBuf::from(dir));
    }
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("ciat")
            .join("atic")
            .join("data")
            .join("notes"),
    )
}

pub(crate) fn board_dir() -> Option<PathBuf> {
    notes_dir().map(|dir| dir.join("atic-tablero"))
}

/// Lee los bloques de la página sin título; `[]` si no hay nota todavía.
fn load_blocks() -> Vec<Block> {
    let Some(file) = board_dir().map(|dir| dir.join("note.json")) else {
        return Vec::new();
    };
    let Some(note) = std::fs::read_to_string(file)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    else {
        return Vec::new();
    };
    let pages = note["pages"].as_array().cloned().unwrap_or_default();
    let page = pages
        .iter()
        .find(|page| page["title"].as_str().is_some_and(str::is_empty))
        .or(pages.first());
    page.and_then(|page| page["blocks"].as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| serde_json::from_value::<Block>(block.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Escribe los bloques en la página sin título, dejando lo demás del archivo.
/// A un lado y renombrar: un corte a medias no deja la nota rota.
fn save_blocks(blocks: &[Block]) -> bool {
    let Some(dir) = board_dir() else {
        return false;
    };
    let file = dir.join("note.json");
    let secs = (now_ms() / 1000) as i64;
    let mut note: serde_json::Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| {
            serde_json::json!({
                "id": format!("pill-{}", now_ms()),
                "app": "atic-tablero",
                "pages": [],
                "updated_at": secs,
            })
        });
    let Ok(blocks) = serde_json::to_value(blocks) else {
        return false;
    };
    if !note["pages"].is_array() {
        note["pages"] = serde_json::json!([]);
    }
    let pages = note["pages"].as_array_mut().expect("pages es un arreglo");
    match pages
        .iter_mut()
        .find(|page| page["title"].as_str().is_some_and(str::is_empty))
    {
        Some(page) => {
            page["blocks"] = blocks;
            page["updated_at"] = secs.into();
        }
        None => pages.push(serde_json::json!({
            "id": format!("pill-page-{}", now_ms()),
            "title": "",
            "blocks": blocks,
            "updated_at": secs,
        })),
    }
    note["updated_at"] = secs.into();
    let Ok(raw) = serde_json::to_string_pretty(&note) else {
        return false;
    };
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join("note.json.tmp");
    std::fs::write(&tmp, raw).is_ok() && std::fs::rename(&tmp, &file).is_ok()
}

/// Notas viejas (todo en 0) se apilan; las que ya tienen marco se dejan.
fn place_if_needed(blocks: &mut [Block]) {
    let mut y = MARGIN;
    for block in blocks.iter_mut().filter(|b| !b.is_ink() && !b.has_frame()) {
        let (w, h) = match &block.body {
            Body::Image { width, height, .. } => image_size(*width, *height),
            Body::Check { .. } => (LIST_W, LIST_H),
            _ => (TEXT_W, TEXT_H),
        };
        (block.x, block.y, block.w, block.h) = (MARGIN, y, w, h);
        y += h + 16.0;
    }
}

fn image_size(width: u32, height: u32) -> (f32, f32) {
    let max = PAGE_W - MARGIN * 2.0;
    let w = (width as f32).clamp(80.0, max);
    let ratio = if width == 0 {
        1.0
    } else {
        height as f32 / width as f32
    };
    (w, (w * ratio).max(48.0))
}

/// Cuántas páginas hacen falta para que nada quede fuera de una celda.
pub(crate) fn pages_for(blocks: &[Block]) -> usize {
    let max_x = blocks
        .iter()
        .filter(|b| !b.is_ink())
        .fold(PAGE_W, |max, b| max.max(b.x + b.w + MARGIN));
    ((max_x / PAGE_W).ceil() as usize).clamp(1, 10)
}

/// `#rrggbb` o `#rrggbbaa`.
fn demo_blocks() -> Vec<Block> {
    let frame = |id: &str, x, y, w, h, body| Block {
        id: id.into(),
        x,
        y,
        w,
        h,
        body,
    };
    let item = |id: &str, text: &str, done| Item {
        id: id.into(),
        text: text.into(),
        done,
    };
    let wave: Vec<[f32; 2]> = (0..60)
        .map(|i| {
            let t = i as f32 / 59.0;
            [560.0 + t * 520.0, 520.0 + (t * 14.0).sin() * 40.0]
        })
        .collect();
    vec![
        frame(
            "demo-ink",
            0.0,
            0.0,
            PAGE_W,
            PAGE_H,
            Body::Ink {
                strokes: vec![
                    Stroke {
                        color: "#e5483f".into(),
                        width: PEN_WIDTH,
                        points: wave.clone(),
                    },
                    Stroke {
                        color: "#d6b48a66".into(),
                        width: HIGHLIGHT_WIDTH,
                        points: wave.iter().map(|p| [p[0], p[1] + 70.0]).collect(),
                    },
                ],
                height: 0,
            },
        ),
        frame(
            "demo-text",
            40.0,
            40.0,
            420.0,
            170.0,
            Body::Text {
                body: "Reunión del jueves\n\nRevisar el flip, el tablero y los bordes.\nLlevar las notas a Atic.".into(),
            },
        ),
        frame(
            "demo-list",
            500.0,
            40.0,
            320.0,
            190.0,
            Body::Check {
                items: vec![
                    item("a", "Redondear las esquinas", true),
                    item("b", "Probar el tablero", false),
                    item("c", "Pegar una imagen", false),
                ],
            },
        ),
        frame(
            "demo-sel",
            40.0,
            260.0,
            340.0,
            120.0,
            Body::Text {
                body: "Una nota más, que se puede mover y estirar desde la esquina.".into(),
            },
        ),
    ]
}

/// `#rrggbb` o `#rrggbbaa` como bytes RGBA.
pub(crate) fn parse_rgba(hex: &str) -> (u8, u8, u8, u8) {
    let digits = hex.trim_start_matches('#');
    let value = u32::from_str_radix(digits.get(..6).unwrap_or("ece8dd"), 16).unwrap_or(0xece8dd);
    let alpha = digits
        .get(6..8)
        .and_then(|a| u8::from_str_radix(a, 16).ok())
        .unwrap_or(255);
    ((value >> 16) as u8, (value >> 8) as u8, value as u8, alpha)
}

fn parse_color(hex: &str) -> Hsla {
    let digits = hex.trim_start_matches('#');
    let value = u32::from_str_radix(digits.get(..6).unwrap_or("ece8dd"), 16).unwrap_or(0xece8dd);
    let mut color: Hsla = rgb(value).into();
    if let Some(alpha) = digits.get(6..8).and_then(|a| u8::from_str_radix(a, 16).ok()) {
        color.a = alpha as f32 / 255.0;
    }
    color
}

fn color_hex(value: u32) -> String {
    format!("#{value:06x}")
}

// --- La vista --------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Pen,
    Highlight,
    Eraser,
}

enum Drag {
    /// Mover o redimensionar un bloque: su marco al empezar y dónde se apretó.
    Move { id: String, from: (f32, f32), start: Point<Pixels> },
    Resize { id: String, from: (f32, f32), start: Point<Pixels> },
    /// Apretado sobre un bloque sin moverse todavía: si no hay arrastre, es un
    /// clic.
    Press { id: String, start: Point<Pixels>, was_selected: bool, click: usize },
    Ink,
    Erase,
}

pub enum PaperEvent {
    Close,
}

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    sheet: Hsla,
    card: Hsla,
    line: Hsla,
    accent: Hsla,
}

pub struct PaperView {
    blocks: Vec<Block>,
    page: usize,
    extra_pages: usize,
    selected: Option<String>,
    editing: Option<String>,
    editor: Entity<TextArea>,
    tool: Tool,
    color: usize,
    undo: Vec<Vec<Block>>,
    redo: Vec<Vec<Block>>,
    drag: Option<Drag>,
    live_stroke: Option<Vec<[f32; 2]>>,
    /// Dónde está la vista en la ventana y cuánto mide, que pone quien la
    /// aloja.
    origin: (f32, f32),
    size: (f32, f32),
    /// Esquinas de la tarjeta: GPUI no recorta a una forma redondeada, así que
    /// el fondo de la vista es el que las redondea.
    radius: f32,
    env: Env,
    drawer: bool,
    drawer_tab: DrawerTab,
    drawer_items: Vec<DrawerItem>,
    export_menu: bool,
    /// Mientras se exporta una imagen: la página que se está dibujando a tamaño
    /// real para capturarla.
    export_page: Option<usize>,
    exporting: bool,
    /// Último aviso de exportar y, si salió bien, el archivo.
    notice: Option<(String, Option<PathBuf>)>,
    dirty: bool,
    edits: u64,
    counter: u64,
    saved: Option<bool>,
    focus: FocusHandle,
    colors: Colors,
    /// Colores de la hoja al exportar: papel claro, como en Atic.
    light: Colors,
    _editor_changed: Subscription,
}

impl EventEmitter<PaperEvent> for PaperView {}

impl Focusable for PaperView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for PaperView {
    /// Si se cierra antes de que venza el retardo, lo pendiente no se pierde.
    fn drop(&mut self) {
        if self.dirty {
            save_blocks(&self.blocks);
        }
    }
}

/// La hoja dentro de la vista: dónde cae y a qué escala, en coordenadas de la
/// vista.
struct Layout {
    x: f32,
    y: f32,
    scale: f32,
}

impl PaperView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let colors = Colors {
            text: rgb(0xf0f0ea).into(),
            muted: rgb(0x9a9a90).into(),
            faint: rgb(0x6e6e66).into(),
            sheet: rgb(0x242421).into(),
            card: rgb(0x2e2e2a).into(),
            line: rgb(0x3a3a35).into(),
            accent: rgb(0x7aa2ff).into(),
        };
        let editor = cx.new(|cx| {
            TextArea::new("Escribe…", colors.text, colors.faint, colors.text, cx)
        });
        let editor_changed = cx.subscribe(&editor, |paper, _, _: &text_input::Changed, cx| {
            paper.apply_edit(cx);
        });
        let mut blocks = load_blocks();
        place_if_needed(&mut blocks);
        let mut paper = Self {
            blocks,
            page: 0,
            extra_pages: 1,
            selected: None,
            editing: None,
            editor,
            tool: Tool::Select,
            color: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            live_stroke: None,
            origin: (0.0, 0.0),
            size: (900.0, 600.0),
            radius: 0.0,
            env: Env::default(),
            drawer: false,
            drawer_tab: DrawerTab::Clip,
            drawer_items: Vec::new(),
            export_menu: false,
            export_page: None,
            exporting: false,
            notice: None,
            dirty: false,
            edits: 0,
            counter: 0,
            saved: None,
            focus: cx.focus_handle(),
            colors,
            light: Colors {
                text: rgb(0x1c1917).into(),
                muted: rgb(0x6b665c).into(),
                faint: rgb(0x9a948a).into(),
                sheet: rgb(0xf4f1ea).into(),
                card: rgb(0xfffdf8).into(),
                line: rgb(0xd9d4c8).into(),
                accent: rgb(0x3f6fd9).into(),
            },
            _editor_changed: editor_changed,
        };
        // `PILL_PAPER_DEMO=1`: llena la hoja con ejemplos para revisar el
        // dibujo; no marca nada como cambiado, así que no se guarda.
        if std::env::var_os("PILL_PAPER_DEMO").is_some() {
            paper.blocks = demo_blocks();
        }
        // `PILL_PAPER_DRAWER=clip|texts|caps`: abre el cajón en esa fuente.
        if let Ok(tab) = std::env::var("PILL_PAPER_DRAWER") {
            paper.drawer = true;
            paper.drawer_tab = match tab.as_str() {
                "texts" => DrawerTab::Texts,
                "caps" => DrawerTab::Captures,
                _ => DrawerTab::Clip,
            };
            paper.load_drawer();
        }
        paper.extra_pages = paper.pages();
        paper
    }

    /// Lo pone quien aloja la vista, cada cuadro.
    pub fn sync(&mut self, origin: (f32, f32), size: (f32, f32), radius: f32, env: Env) {
        self.origin = origin;
        self.size = size;
        self.radius = radius;
        self.env = env;
    }

    fn pages(&self) -> usize {
        pages_for(&self.blocks).max(self.extra_pages)
    }

    /// A qué escala se dibuja una página al exportarla: a tamaño real si cabe
    /// en el monitor, y si no, lo más grande que quepa.
    fn export_scale(&self) -> f32 {
        (self.env.screen.0 / PAGE_W)
            .min(self.env.screen.1 / PAGE_H)
            .min(1.0)
    }

    fn drawer_width(&self) -> f32 {
        if self.drawer {
            DRAWER_W
        } else {
            0.0
        }
    }

    fn layout(&self) -> Layout {
        if self.export_page.is_some() {
            // Centrada en el monitor, sin barra ni cajón, en coordenadas de la
            // vista.
            let scale = self.export_scale();
            return Layout {
                x: self.env.offset.0 + (self.env.screen.0 - PAGE_W * scale) / 2.0 - self.origin.0,
                y: self.env.offset.1 + (self.env.screen.1 - PAGE_H * scale) / 2.0 - self.origin.1,
                scale,
            };
        }
        let (w, h) = self.size;
        let drawer = self.drawer_width();
        let avail_w = (w - 24.0 - drawer).max(100.0);
        let avail_h = (h - TOOLBAR_H - 24.0).max(100.0);
        let scale = (avail_w / PAGE_W).min(avail_h / PAGE_H);
        Layout {
            x: (w - drawer - PAGE_W * scale) / 2.0,
            y: TOOLBAR_H + 12.0 + (avail_h - PAGE_H * scale) / 2.0,
            scale,
        }
    }

    /// Posición de la ventana → papel (coordenadas del tablero entero).
    fn to_paper(&self, p: Point<Pixels>) -> (f32, f32) {
        let layout = self.layout();
        (
            (f32::from(p.x) - self.origin.0 - layout.x) / layout.scale + self.page as f32 * PAGE_W,
            (f32::from(p.y) - self.origin.1 - layout.y) / layout.scale,
        )
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// El bloque que está bajo un punto de papel; el de más arriba gana.
    fn hit(&self, (x, y): (f32, f32)) -> Option<usize> {
        self.blocks
            .iter()
            .enumerate()
            .rev()
            .find(|(_, b)| !b.is_ink() && x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h)
            .map(|(i, _)| i)
    }

    // --- Historial y guardado ---------------------------------------------

    /// Guarda el estado antes de un cambio, para deshacerlo.
    fn remember(&mut self) {
        self.undo.push(self.blocks.clone());
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.blocks, previous));
            self.selected = None;
            self.changed(cx);
        }
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.blocks, next));
            self.selected = None;
            self.changed(cx);
        }
    }

    /// Algo cambió: guardar pronto, sin esperar a que se cierre.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        self.saved = None;
        self.edits += 1;
        let edits = self.edits;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |paper, cx| {
                if paper.edits == edits {
                    paper.save_now(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        let ok = save_blocks(&self.blocks);
        if ok {
            self.dirty = false;
        }
        self.saved = Some(ok);
        cx.notify();
    }

    // --- Edición de texto -------------------------------------------------

    fn start_editing(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let text = match &self.blocks[index].body {
            Body::Text { body } => body.clone(),
            // Una lista se edita como líneas: una línea, un ítem.
            Body::Check { items } => items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => return,
        };
        self.remember();
        self.editing = Some(id.to_string());
        self.selected = Some(id.to_string());
        self.editor.update(cx, |editor, cx| editor.set_text(&text, cx));
        window.focus(&self.editor.focus_handle(cx));
        cx.notify();
    }

    fn stop_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing.take() else {
            return;
        };
        // Un texto que quedó vacío no deja una caja huérfana.
        if let Some(index) = self.index_of(&id) {
            let empty = match &self.blocks[index].body {
                Body::Text { body } => body.trim().is_empty(),
                Body::Check { items } => items.iter().all(|item| item.text.trim().is_empty()),
                _ => false,
            };
            if empty {
                self.blocks.remove(index);
                self.selected = None;
                self.changed(cx);
            }
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// El editor cambió: el bloque toma su texto.
    fn apply_edit(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editing.clone() else {
            return;
        };
        let Some(index) = self.index_of(&id) else {
            return;
        };
        let text = self.editor.read(cx).text().to_string();
        let mut counter = self.counter;
        let block = &mut self.blocks[index];
        match &mut block.body {
            Body::Text { body } => {
                *body = text;
                // La caja crece con el texto para que nada quede cortado.
                let chars_per_line = ((block.w - PAD_X * 2.0) / (FONT * 0.5)).max(8.0);
                let lines: f32 = body
                    .split('\n')
                    .map(|line| (line.chars().count() as f32 / chars_per_line).ceil().max(1.0))
                    .sum();
                block.h = block.h.max(lines * FONT * LINE + PAD_Y * 2.0);
            }
            Body::Check { items } => {
                let lines: Vec<&str> = text.split('\n').collect();
                let mut next = Vec::with_capacity(lines.len());
                for (i, line) in lines.iter().enumerate() {
                    // Cada línea conserva el ítem de su posición (y su marca).
                    let (id, done) = items
                        .get(i)
                        .map(|item| (item.id.clone(), item.done))
                        .unwrap_or_else(|| (new_id(&mut counter), false));
                    next.push(Item {
                        id,
                        text: (*line).to_string(),
                        done,
                    });
                }
                *items = next;
                block.h = block.h.max(items.len() as f32 * ROW_H + PAD_Y * 2.0);
            }
            _ => {}
        }
        self.counter = counter;
        self.changed(cx);
    }

    // --- Altas y bajas ----------------------------------------------------

    /// El centro de lo que se ve de la página, para poner lo nuevo.
    fn page_center(&self) -> (f32, f32) {
        (self.page as f32 * PAGE_W + PAGE_W / 2.0, PAGE_H / 2.0)
    }

    fn add_block(&mut self, body: Body, w: f32, h: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        self.remember();
        let (cx0, cy0) = self.page_center();
        // Cada bloque nuevo, un poco corrido: no se apilan exactos.
        let nudge = (self.blocks.len() % 6) as f32 * 24.0;
        let id = new_id(&mut self.counter);
        let editable = matches!(body, Body::Text { .. } | Body::Check { .. });
        self.blocks.push(Block {
            id: id.clone(),
            x: cx0 - w / 2.0 + nudge,
            y: cy0 - h / 2.0 + nudge,
            w,
            h,
            body,
        });
        self.selected = Some(id.clone());
        self.tool = Tool::Select;
        self.changed(cx);
        if editable {
            // `start_editing` guarda otro punto de deshacer: se descarta.
            self.undo.pop();
            self.start_editing(&id, window, cx);
        }
    }

    fn new_text(&mut self, _: &NewText, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        self.add_block(Body::Text { body: String::new() }, TEXT_W, TEXT_H, window, cx);
    }

    fn new_list(&mut self, _: &NewList, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let id = new_id(&mut self.counter);
        self.add_block(
            Body::Check {
                items: vec![Item {
                    id,
                    text: String::new(),
                    done: false,
                }],
            },
            LIST_W,
            LIST_H,
            window,
            cx,
        );
    }

    /// Pega lo copiado: texto como caja, imagen como imagen.
    fn paste_block(&mut self, _: &PasteBlock, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(image) if image.format == ImageFormat::Png => {
                    let Some((width, height)) = png_size(&image.bytes) else {
                        continue;
                    };
                    let Some(dir) = board_dir().map(|dir| dir.join("assets")) else {
                        continue;
                    };
                    let name = format!("pill-{}.png", now_ms());
                    let _ = std::fs::create_dir_all(&dir);
                    if std::fs::write(dir.join(&name), &image.bytes).is_err() {
                        continue;
                    }
                    let (w, h) = image_size(width, height);
                    self.add_block(
                        Body::Image {
                            asset: name,
                            width,
                            height,
                        },
                        w,
                        h,
                        window,
                        cx,
                    );
                    return;
                }
                _ => {}
            }
        }
        if let Some(text) = item.text().filter(|t| !t.trim().is_empty()) {
            self.add_text_block(text, window, cx);
        }
    }

    /// Una caja con este texto, ya armada: no entra a editar, queda elegida.
    fn add_text_block(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let lines = text.lines().count().max(1) as f32;
        let h = (lines * FONT * LINE + PAD_Y * 2.0).clamp(TEXT_H, 500.0);
        self.add_block(Body::Text { body: text }, TEXT_W * 1.4, h, window, cx);
        self.stop_editing(window, cx);
    }

    /// Una imagen de un archivo: se copia a los `assets` de la nota, que es de
    /// donde las lee Atic.
    fn add_image_file(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = board_dir().map(|dir| dir.join("assets")) else {
            return;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let name = format!("pill-{}.{ext}", now_ms());
        let _ = std::fs::create_dir_all(&dir);
        if std::fs::copy(path, dir.join(&name)).is_err() {
            return;
        }
        let (width, height) = image::image_dimensions(path).unwrap_or((640, 400));
        let (w, h) = image_size(width, height);
        self.add_block(
            Body::Image {
                asset: name,
                width,
                height,
            },
            w,
            h,
            window,
            cx,
        );
    }

    fn delete_selected(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let Some(id) = self.selected.take() else {
            return;
        };
        if let Some(index) = self.index_of(&id) {
            self.remember();
            self.blocks.remove(index);
            self.changed(cx);
        }
        window.focus(&self.focus);
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            self.stop_editing(window, cx);
        } else if self.selected.is_some() {
            self.selected = None;
            cx.notify();
        } else {
            cx.emit(PaperEvent::Close);
        }
    }

    // --- Ratón ------------------------------------------------------------

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let local = (
            f32::from(event.position.x) - self.origin.0,
            f32::from(event.position.y) - self.origin.1,
        );
        // Mientras se captura una página no hay nada que tocar.
        if self.export_page.is_some() {
            return;
        }
        // La barra de herramientas atiende sus propios clics.
        if local.1 < TOOLBAR_H {
            if self.export_menu {
                self.export_menu = false;
                cx.notify();
            }
            return;
        }
        // El menú de exportar: un clic dentro lo atiende su fila; fuera lo cierra.
        if self.export_menu {
            let (w, _) = self.size;
            let inside = local.0 >= w - 40.0 - MENU_W
                && local.0 <= w - 40.0
                && local.1 <= TOOLBAR_H + Format::ALL.len() as f32 * MENU_ROW + 10.0;
            if !inside {
                self.export_menu = false;
                cx.notify();
            }
            return;
        }
        // El cajón.
        if self.drawer && local.0 >= self.size.0 - DRAWER_W {
            return;
        }
        // La paleta flotante del lápiz.
        if matches!(self.tool, Tool::Pen | Tool::Highlight)
            && local.1 <= TOOLBAR_H + 34.0
            && local.0 <= 8.0 + PALETTE.len() as f32 * 20.0 + 16.0
        {
            return;
        }
        let paper = self.to_paper(event.position);
        match self.tool {
            Tool::Pen | Tool::Highlight => {
                self.stop_editing(window, cx);
                self.remember();
                self.live_stroke = Some(vec![[paper.0, paper.1]]);
                self.drag = Some(Drag::Ink);
                cx.notify();
            }
            Tool::Eraser => {
                self.stop_editing(window, cx);
                self.remember();
                self.erase_at(paper, cx);
                self.drag = Some(Drag::Erase);
            }
            Tool::Select => self.press_select(event, paper, window, cx),
        }
    }

    fn press_select(
        &mut self,
        event: &MouseDownEvent,
        paper: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scale = self.layout().scale;
        // Con un bloque elegido, su esquina inferior derecha lo redimensiona.
        if let Some(index) = self.selected.as_deref().and_then(|id| self.index_of(id)) {
            let b = &self.blocks[index];
            let (id, size_now) = (b.id.clone(), (b.w, b.h));
            let (rx, ry) = (b.x + b.w, b.y + b.h);
            let size = HANDLE / scale;
            if self.editing.is_none()
                && paper.0 >= rx - size
                && paper.0 <= rx + size / 2.0
                && paper.1 >= ry - size
                && paper.1 <= ry + size / 2.0
            {
                self.remember();
                self.drag = Some(Drag::Resize {
                    id,
                    from: size_now,
                    start: event.position,
                });
                return;
            }
        }
        match self.hit(paper) {
            Some(index) => {
                let id = self.blocks[index].id.clone();
                // Clic dentro del bloque que ya se está editando: el cursor
                // lo maneja el editor.
                if self.editing.as_deref() == Some(id.as_str()) {
                    return;
                }
                self.stop_editing(window, cx);
                // La casilla de un ítem se marca sin más.
                if self.toggle_checkbox(index, paper, cx) {
                    self.selected = Some(id);
                    return;
                }
                let was_selected = self.selected.as_deref() == Some(id.as_str());
                // Se sube al frente.
                let block = self.blocks.remove(index);
                self.blocks.push(block);
                self.selected = Some(id.clone());
                self.drag = Some(Drag::Press {
                    id,
                    start: event.position,
                    was_selected,
                    click: event.click_count,
                });
                cx.notify();
            }
            None => {
                self.stop_editing(window, cx);
                self.selected = None;
                window.focus(&self.focus);
                cx.notify();
            }
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let scale = self.layout().scale;
        let paper = self.to_paper(event.position);
        match self.drag.take() {
            Some(Drag::Ink) => {
                if let Some(stroke) = self.live_stroke.as_mut() {
                    let last = stroke.last().copied().unwrap_or([paper.0, paper.1]);
                    // Un punto cada par de píxeles basta.
                    if (paper.0 - last[0]).hypot(paper.1 - last[1]) * scale >= 2.0 {
                        stroke.push([paper.0, paper.1]);
                    }
                }
                self.drag = Some(Drag::Ink);
                cx.notify();
            }
            Some(Drag::Erase) => {
                self.erase_at(paper, cx);
                self.drag = Some(Drag::Erase);
            }
            Some(Drag::Press { id, start, was_selected, click }) => {
                let moved = (f32::from(event.position.x) - f32::from(start.x))
                    .hypot(f32::from(event.position.y) - f32::from(start.y));
                if moved < DRAG_MIN {
                    self.drag = Some(Drag::Press { id, start, was_selected, click });
                    return;
                }
                let Some(index) = self.index_of(&id) else {
                    return;
                };
                self.remember();
                let from = (self.blocks[index].x, self.blocks[index].y);
                self.drag = Some(Drag::Move { id, from, start });
                self.move_to(event.position, cx);
            }
            Some(Drag::Move { id, from, start }) => {
                self.drag = Some(Drag::Move { id, from, start });
                self.move_to(event.position, cx);
            }
            Some(Drag::Resize { id, from, start }) => {
                if let Some(index) = self.index_of(&id) {
                    let dx = (f32::from(event.position.x) - f32::from(start.x)) / scale;
                    let dy = (f32::from(event.position.y) - f32::from(start.y)) / scale;
                    let block = &mut self.blocks[index];
                    block.w = (from.0 + dx).max(MIN_W);
                    block.h = (from.1 + dy).max(MIN_H);
                }
                self.drag = Some(Drag::Resize { id, from, start });
                self.changed(cx);
            }
            None => {}
        }
    }

    fn move_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let scale = self.layout().scale;
        let Some(Drag::Move { id, from, start }) = &self.drag else {
            return;
        };
        let (id, from, start) = (id.clone(), *from, *start);
        let Some(index) = self.index_of(&id) else {
            return;
        };
        let dx = (f32::from(position.x) - f32::from(start.x)) / scale;
        let dy = (f32::from(position.y) - f32::from(start.y)) / scale;
        let max_x = self.pages() as f32 * PAGE_W;
        let block = &mut self.blocks[index];
        block.x = (from.0 + dx).clamp(0.0, (max_x - block.w).max(0.0));
        block.y = (from.1 + dy).clamp(0.0, (PAGE_H - block.h).max(0.0));
        self.changed(cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Ink) => self.finish_stroke(cx),
            Some(Drag::Press { id, was_selected, click, .. }) => {
                // Sin arrastre fue un clic: con doble clic, o sobre un bloque
                // que ya estaba elegido, se edita.
                if click >= 2 || was_selected {
                    self.start_editing(&id, window, cx);
                }
            }
            Some(Drag::Move { .. }) | Some(Drag::Resize { .. }) | Some(Drag::Erase) | None => {}
        }
    }

    /// Guarda el trazo que se acaba de dibujar en el bloque de tinta.
    fn finish_stroke(&mut self, cx: &mut Context<Self>) {
        let Some(points) = self.live_stroke.take() else {
            return;
        };
        // Un clic suelto deja un punto: se duplica para que se dibuje.
        let mut points = points;
        if points.len() == 1 {
            points.push(points[0]);
        }
        let highlight = self.tool == Tool::Highlight;
        let stroke = Stroke {
            color: if highlight {
                format!("{}{HIGHLIGHT_SUFFIX}", color_hex(PALETTE[self.color]))
            } else {
                color_hex(PALETTE[self.color])
            },
            width: if highlight { HIGHLIGHT_WIDTH } else { PEN_WIDTH },
            points,
        };
        let ink = self.ink_index();
        if let Body::Ink { strokes, .. } = &mut self.blocks[ink].body {
            strokes.push(stroke);
        }
        self.changed(cx);
    }

    /// El bloque de tinta, creado si falta.
    fn ink_index(&mut self) -> usize {
        if let Some(index) = self.blocks.iter().position(Block::is_ink) {
            return index;
        }
        let id = new_id(&mut self.counter);
        self.blocks.insert(
            0,
            Block {
                id,
                x: 0.0,
                y: 0.0,
                w: PAGE_W,
                h: PAGE_H,
                body: Body::Ink {
                    strokes: Vec::new(),
                    height: 0,
                },
            },
        );
        0
    }

    /// El borrador quita los trozos de trazo que toca y parte el resto.
    fn erase_at(&mut self, at: (f32, f32), cx: &mut Context<Self>) {
        let mut changed = false;
        for block in &mut self.blocks {
            if let Body::Ink { strokes, .. } = &mut block.body {
                changed |= erase_strokes(strokes, at, ERASER_RADIUS);
            }
        }
        if changed {
            self.changed(cx);
        }
    }

    /// Si el punto cae en la casilla de un ítem de lista, alterna su marca.
    fn toggle_checkbox(&mut self, index: usize, at: (f32, f32), cx: &mut Context<Self>) -> bool {
        let block = &self.blocks[index];
        let Body::Check { items } = &block.body else {
            return false;
        };
        let row = ((at.1 - block.y - PAD_Y) / ROW_H).floor();
        let across = at.0 - block.x;
        if row < 0.0 || row as usize >= items.len() || across < PAD_X - 6.0 || across > PAD_X + 22.0 {
            return false;
        }
        let row = row as usize;
        self.remember();
        if let Body::Check { items } = &mut self.blocks[index].body {
            items[row].done = !items[row].done;
        }
        self.changed(cx);
        true
    }

    fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        self.tool = tool;
        if tool != Tool::Select {
            self.selected = None;
        }
        cx.notify();
    }

    fn go_page(&mut self, page: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        self.page = page.min(self.pages() - 1);
        self.selected = None;
        cx.notify();
    }

    fn add_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        if self.pages() >= 10 {
            return;
        }
        self.extra_pages = self.pages() + 1;
        self.page = self.pages() - 1;
        cx.notify();
    }
}

/// Quita de `strokes` los puntos a menos de `radius` de `at` y parte los
/// trazos por donde los cortó. Un trozo de un solo punto desaparece. Devuelve
/// si cambió algo.
fn erase_strokes(strokes: &mut Vec<Stroke>, at: (f32, f32), radius: f32) -> bool {
    let hit = |p: &[f32; 2]| (p[0] - at.0).hypot(p[1] - at.1) <= radius;
    let mut changed = false;
    let mut next = Vec::with_capacity(strokes.len());
    for stroke in strokes.drain(..) {
        if !stroke.points.iter().any(hit) {
            next.push(stroke);
            continue;
        }
        changed = true;
        let mut run: Vec<[f32; 2]> = Vec::new();
        for point in &stroke.points {
            if hit(point) {
                if run.len() >= 2 {
                    next.push(Stroke {
                        points: std::mem::take(&mut run),
                        ..stroke.clone()
                    });
                }
                run.clear();
            } else {
                run.push(*point);
            }
        }
        if run.len() >= 2 {
            next.push(Stroke {
                points: run,
                ..stroke.clone()
            });
        }
    }
    *strokes = next;
    changed
}

/// Ancho y alto de un PNG, leídos de su cabecera.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let read = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some((read(16), read(20)))
}

// --- Dibujo ----------------------------------------------------------------

impl PaperView {
    fn button(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (text, faint) = (self.colors.text, self.colors.faint);
        div()
            .id(id)
            .h(px(26.))
            .px(px(9.))
            .flex()
            .flex_none()
            .items_center()
            .rounded(px(13.))
            .text_size(px(11.))
            .text_color(if active { text } else { self.colors.muted })
            .when(active, |el| el.bg(text.opacity(0.14)))
            .hover(|el| el.bg(text.opacity(0.08)).text_color(text))
            .cursor_pointer()
            .on_click(cx.listener(move |paper, _, window, cx| on_click(paper, window, cx)))
            .child(label)
            .when(false, |el| el.text_color(faint))
    }

    fn swatch(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let color: Hsla = rgb(PALETTE[index]).into();
        let active = self.color == index;
        div()
            .id(("swatch", index))
            .size(px(16.))
            .flex_none()
            .rounded(px(8.))
            .bg(color)
            .border_2()
            .border_color(if active {
                self.colors.text
            } else {
                self.colors.text.opacity(0.0)
            })
            .cursor_pointer()
            .on_click(cx.listener(move |paper, _, _, cx| {
                paper.color = index;
                cx.notify();
            }))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let pages = self.pages();
        let status = match (&self.notice, self.saved) {
            (Some((text, _)), _) => text.clone(),
            (None, Some(true)) => "Guardado".to_string(),
            (None, Some(false)) => "No se pudo guardar".to_string(),
            (None, None) if self.dirty => "Guardando…".to_string(),
            _ => String::new(),
        };
        let shown_file = self.notice.as_ref().and_then(|(_, file)| file.clone());
        let separator = || div().w(px(1.)).h(px(16.)).mx(px(5.)).bg(colors.line);
        div()
            .h(px(TOOLBAR_H))
            .w_full()
            .flex_none()
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(1.))
            .child(self.button("tool-select", "Elegir", self.tool == Tool::Select,
                |paper, window, cx| paper.set_tool(Tool::Select, window, cx), cx))
            .child(self.button("tool-pen", "Lápiz", self.tool == Tool::Pen,
                |paper, window, cx| paper.set_tool(Tool::Pen, window, cx), cx))
            .child(self.button("tool-highlight", "Resaltador", self.tool == Tool::Highlight,
                |paper, window, cx| paper.set_tool(Tool::Highlight, window, cx), cx))
            .child(self.button("tool-eraser", "Borrador", self.tool == Tool::Eraser,
                |paper, window, cx| paper.set_tool(Tool::Eraser, window, cx), cx))
            .child(separator())
            .child(self.button("add-text", "Texto", false,
                |paper, window, cx| paper.new_text(&NewText, window, cx), cx))
            .child(self.button("add-list", "Lista", false,
                |paper, window, cx| paper.new_list(&NewList, window, cx), cx))
            .child(self.button("add-paste", "Pegar", false,
                |paper, window, cx| paper.paste_block(&PasteBlock, window, cx), cx))
            .child(separator())
            .child(self.button("undo", "↶", false, |paper, window, cx| paper.undo(&Undo, window, cx), cx))
            .child(self.button("redo", "↷", false, |paper, window, cx| paper.redo(&Redo, window, cx), cx))
            .child(div().flex_1().min_w_0())
            .child(
                div()
                    .id("status")
                    .max_w(px(190.))
                    .truncate()
                    .text_size(px(10.))
                    .text_color(colors.faint)
                    .mr(px(6.))
                    .when_some(shown_file, |el, file| {
                        el.cursor_pointer()
                            .hover(|el| el.text_color(colors.text))
                            .on_click(cx.listener(move |_, _, _, _| show_in_folder(&file)))
                    })
                    .child(status),
            )
            .child(self.button("page-prev", "‹", false,
                |paper, window, cx| paper.go_page(paper.page.saturating_sub(1), window, cx), cx))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted)
                    .child(format!("{} / {}", self.page + 1, pages)),
            )
            .child(self.button("page-next", "›", false,
                |paper, window, cx| paper.go_page(paper.page + 1, window, cx), cx))
            .child(self.button("page-add", "+", false,
                |paper, window, cx| paper.add_page(window, cx), cx))
            .child(separator())
            .child(self.button("drawer", "Cajón", self.drawer,
                |paper, _, cx| paper.toggle_drawer(cx), cx))
            .child(self.button("export", "Exportar", self.export_menu,
                |paper, _, cx| {
                    if !paper.exporting {
                        paper.export_menu = !paper.export_menu;
                        cx.notify();
                    }
                }, cx))
            .child(self.button("close", "✕", false, |_, _, cx| cx.emit(PaperEvent::Close), cx))
    }

    /// Los colores del lápiz, flotando bajo la barra (como la paleta de Atic).
    fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .occlude()
            .left(px(8.))
            .top(px(TOOLBAR_H))
            .h(px(30.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(15.))
            .bg(self.colors.card)
            .border_1()
            .border_color(self.colors.line)
            .children((0..PALETTE.len()).map(|i| self.swatch(i, cx)))
    }

    fn render_export_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        div()
            .absolute()
            // Los clics del menú no deben llegar a lo que hay debajo (el cajón).
            .occlude()
            .right(px(40.))
            .top(px(TOOLBAR_H))
            .w(px(MENU_W))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(colors.card)
            .border_1()
            .border_color(colors.line)
            .children(Format::ALL.iter().map(|&format| {
                div()
                    .id(("export", format as usize))
                    .h(px(MENU_ROW))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(7.))
                    .text_size(px(12.))
                    .text_color(colors.text)
                    .cursor_pointer()
                    .hover(|el| el.bg(colors.text.opacity(0.08)))
                    .on_click(cx.listener(move |paper, _, window, cx| {
                        paper.start_export(format, window, cx)
                    }))
                    .child(format.label())
            }))
    }

    // --- Cajón ------------------------------------------------------------

    fn toggle_drawer(&mut self, cx: &mut Context<Self>) {
        self.drawer = !self.drawer;
        if self.drawer {
            self.load_drawer();
        }
        cx.notify();
    }

    fn set_drawer_tab(&mut self, tab: DrawerTab, cx: &mut Context<Self>) {
        self.drawer_tab = tab;
        self.load_drawer();
        cx.notify();
    }

    /// Relee la fuente elegida: son archivos de Atic que cambian solos.
    fn load_drawer(&mut self) {
        self.drawer_items = match self.drawer_tab {
            DrawerTab::Clip => crate::history::dir()
                .and_then(|dir| crate::history::load_if_changed(&dir, None))
                .map(|(_, entries)| entries)
                .unwrap_or_default()
                .iter()
                .take(40)
                .filter_map(|entry| match &entry.content {
                    crate::clipboard::Content::Text(text) => Some(DrawerItem::Text {
                        label: entry.preview.clone(),
                        body: text.to_string(),
                    }),
                    crate::clipboard::Content::Color(label, _) => Some(DrawerItem::Text {
                        label: label.clone(),
                        body: label.to_string(),
                    }),
                    crate::clipboard::Content::Image(crate::clipboard::Picture::File(path)) => {
                        Some(DrawerItem::Image {
                            path: path.to_path_buf(),
                        })
                    }
                    crate::clipboard::Content::Image(_) => None,
                })
                .collect(),
            DrawerTab::Texts => crate::snippets::texts()
                .into_iter()
                .map(|(name, body)| DrawerItem::Text {
                    label: name.into(),
                    body,
                })
                .collect(),
            DrawerTab::Captures => recent_captures()
                .into_iter()
                .map(|path| DrawerItem::Image { path })
                .collect(),
        };
    }

    fn insert_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.drawer_items.get(index) {
            Some(DrawerItem::Text { body, .. }) => {
                let body = body.clone();
                self.add_text_block(body, window, cx);
            }
            Some(DrawerItem::Image { path }) => {
                let path = path.clone();
                self.add_image_file(&path, window, cx);
            }
            None => {}
        }
    }

    fn render_drawer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let (text, muted, faint) = (colors.text, colors.muted, colors.faint);
        let tab = |id: &'static str, label: &'static str, which: DrawerTab, cx: &mut Context<Self>| {
            let active = self.drawer_tab == which;
            div()
                .id(id)
                .h(px(24.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(12.))
                .text_size(px(10.5))
                .text_color(if active { text } else { muted })
                .when(active, |el| el.bg(text.opacity(0.14)))
                .hover(|el| el.bg(text.opacity(0.08)))
                .cursor_pointer()
                .on_click(cx.listener(move |paper, _, _, cx| paper.set_drawer_tab(which, cx)))
                .child(label)
        };
        let empty = match self.drawer_tab {
            DrawerTab::Clip => "El historial está vacío.",
            DrawerTab::Texts => "No hay textos guardados.",
            DrawerTab::Captures => "Aún no hay capturas.",
        };
        let items: Vec<AnyElement> = self.drawer_items.iter().enumerate().map(|(i, item)| {
            let row = div()
                .id(("drawer-item", i))
                .w_full()
                .flex_none()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|el| el.bg(text.opacity(0.08)))
                .on_click(cx.listener(move |paper, _, window, cx| {
                    paper.insert_item(i, window, cx)
                }));
            match item {
                DrawerItem::Text { label, .. } => row
                    .h(px(46.))
                    .px(px(8.))
                    .py(px(6.))
                    .overflow_hidden()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(text)
                    .child(label.clone())
                    .into_any_element(),
                DrawerItem::Image { path } => row
                    .p(px(4.))
                    .child(
                        img(path.clone())
                            .w_full()
                            .h(px(84.))
                            .rounded(px(6.))
                            .object_fit(ObjectFit::Cover)
                            .with_fallback(move || {
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .text_color(faint)
                                    .child("sin vista previa")
                                    .into_any_element()
                            }),
                    )
                    .into_any_element(),
            }
        }).collect();
        div()
            .absolute()
            .occlude()
            .right(px(8.))
            .top(px(TOOLBAR_H))
            .bottom(px(8.))
            .w(px(DRAWER_W - 12.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(colors.card)
            .border_1()
            .border_color(colors.line)
            .child(
                div()
                    .flex_none()
                    .p(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(tab("drawer-clip", "Portapapeles", DrawerTab::Clip, cx))
                    .child(tab("drawer-texts", "Textos", DrawerTab::Texts, cx))
                    .child(tab("drawer-caps", "Capturas", DrawerTab::Captures, cx)),
            )
            .child(
                div()
                    .id("drawer-list")
                    .flex_1()
                    .min_h_0()
                    .px(px(6.))
                    .pb(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .overflow_y_scroll()
                    .children(items)
                    .when(self.drawer_items.is_empty(), |el| {
                        el.child(
                            div()
                                .p(px(10.))
                                .text_size(px(11.))
                                .text_color(faint)
                                .child(empty),
                        )
                    }),
            )
    }

    // --- Exportar ---------------------------------------------------------

    /// Captura la página que se está dibujando a tamaño real en el monitor.
    fn capture_page(&self) -> Option<atic_capture::Frame> {
        let scale = self.export_scale();
        let dpi = self.env.dpi;
        let left = (self.env.screen.0 - PAGE_W * scale) / 2.0;
        let top = (self.env.screen.1 - PAGE_H * scale) / 2.0;
        let rect = atic_capture::Rect::new(
            self.env.origin_phys.0 + (left * dpi).round() as i32,
            self.env.origin_phys.1 + (top * dpi).round() as i32,
            (PAGE_W * scale * dpi).round() as u32,
            (PAGE_H * scale * dpi).round() as u32,
        );
        atic_capture::engine::capture_rect(rect, false).ok()
    }

    fn start_export(&mut self, format: Format, window: &mut Window, cx: &mut Context<Self>) {
        if self.exporting {
            return;
        }
        self.stop_editing(window, cx);
        self.selected = None;
        self.export_menu = false;
        // Lo pendiente a disco antes: el archivo exportado sale de la memoria,
        // pero así Atic y el tablero no se contradicen.
        self.save_now(cx);
        self.exporting = true;
        self.notice = Some(("Exportando…".into(), None));
        let blocks = self.blocks.clone();
        let count = self.pages();
        let Some(assets) = board_dir().map(|dir| dir.join("assets")) else {
            self.finish_export(Err("No se encontró la carpeta de Atic.".into()), cx);
            return;
        };
        let path = export_destination(format, count);
        let (page_w, page_h) = (PAGE_W as f64, PAGE_H as f64);
        let was_page = self.page;
        if format.is_image() {
            cx.spawn(async move |this, cx| {
                let mut frames: Vec<(u32, u32, Vec<u8>)> = Vec::new();
                for page in 0..count {
                    let shown = this.update(cx, |paper, cx| {
                        paper.export_page = Some(page);
                        paper.page = page;
                        cx.notify();
                    });
                    if shown.is_err() {
                        return;
                    }
                    cx.background_executor().timer(EXPORT_SETTLE).await;
                    let frame = this.update(cx, |paper, _| paper.capture_page()).ok().flatten();
                    if let Some(frame) = frame {
                        frames.push((frame.width(), frame.height(), frame.bgra));
                    }
                }
                let _ = this.update(cx, |paper, cx| {
                    paper.export_page = None;
                    paper.page = was_page;
                    cx.notify();
                });
                let jpeg = format == Format::Jpeg;
                let result = cx
                    .background_spawn(async move {
                        if frames.len() != count {
                            return Err("No se pudo capturar alguna página.".to_string());
                        }
                        let pages = crate::flip_pages::image_pages(&frames, jpeg)?;
                        crate::flip_export::export(format.key(), &path, &pages, page_w, page_h)
                            .map(|path| (path, 0))
                    })
                    .await;
                let _ = this.update(cx, |paper, cx| paper.finish_export(result, cx));
            })
            .detach();
        } else {
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_spawn(async move {
                        let (pages, failed) =
                            crate::flip_pages::native_pages(&blocks, count, &assets);
                        crate::flip_export::export(format.key(), &path, &pages, page_w, page_h)
                            .map(|path| (path, failed.len()))
                    })
                    .await;
                let _ = this.update(cx, |paper, cx| paper.finish_export(result, cx));
            })
            .detach();
        }
    }

    fn finish_export(&mut self, result: Result<(PathBuf, usize), String>, cx: &mut Context<Self>) {
        self.exporting = false;
        self.notice = Some(match result {
            Ok((path, missing)) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let tail = if missing > 0 {
                    format!(" · {missing} imágenes no se leyeron")
                } else {
                    String::new()
                };
                (format!("Exportado: {name}{tail} · mostrar"), Some(path))
            }
            Err(error) => (format!("No se pudo exportar: {error}"), None),
        });
        cx.notify();
    }

    /// Un bloque de texto, lista o imagen, con su marco sobre la hoja.
    fn render_block(&self, block: &Block, layout: &Layout, colors: &Colors) -> Option<AnyElement> {
        let s = layout.scale;
        let page_x = self.page as f32 * PAGE_W;
        // Solo lo que cae en esta página.
        if block.x + block.w <= page_x || block.x >= page_x + PAGE_W {
            return None;
        }
        let selected = self.selected.as_deref() == Some(block.id.as_str());
        let editing = self.editing.as_deref() == Some(block.id.as_str());
        let frame = div()
            .absolute()
            .left(px((block.x - page_x) * s))
            .top(px(block.y * s))
            .w(px(block.w * s))
            .h(px(block.h * s));

        let font = (FONT * s).max(7.0);
        let line = font * LINE;
        let content: AnyElement = match &block.body {
            Body::Text { body } => {
                let inner = div()
                    .size_full()
                    .px(px(PAD_X * s))
                    .py(px(PAD_Y * s))
                    .text_size(px(font))
                    .line_height(px(line));
                if editing {
                    inner.child(self.editor.clone()).into_any_element()
                } else if body.is_empty() {
                    inner
                        .text_color(colors.faint)
                        .child("Escribe…")
                        .into_any_element()
                } else {
                    inner
                        .text_color(colors.text)
                        .child(SharedString::from(body.clone()))
                        .into_any_element()
                }
            }
            Body::Check { items } => {
                let inner = div()
                    .size_full()
                    .px(px(PAD_X * s))
                    .py(px(PAD_Y * s))
                    .text_size(px(font))
                    .line_height(px(ROW_H * s));
                if editing {
                    inner.child(self.editor.clone()).into_any_element()
                } else {
                    let accent = colors.accent;
                    let (text, faint, line_color) = (colors.text, colors.faint, colors.line);
                    inner
                        .flex()
                        .flex_col()
                        .children(items.iter().map(|item| {
                            let boxed = div()
                                .size(px(14.0 * s))
                                .flex_none()
                                .mr(px(8.0 * s))
                                .rounded(px(4.0 * s))
                                .border_1()
                                .border_color(if item.done { accent } else { line_color })
                                .when(item.done, |el| el.bg(accent));
                            div()
                                .h(px(ROW_H * s))
                                .flex()
                                .items_center()
                                .child(boxed)
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(if item.done { faint } else { text })
                                        .when(item.done, |el| el.line_through())
                                        .child(SharedString::from(item.text.clone())),
                                )
                        }))
                        .into_any_element()
                }
            }
            Body::Image { asset, .. } => {
                let path = board_dir().map(|dir| dir.join("assets").join(asset));
                match path {
                    Some(path) => img(path)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element(),
                    None => div().size_full().into_any_element(),
                }
            }
            Body::Ink { .. } => return None,
        };

        let is_image = matches!(block.body, Body::Image { .. });
        let surface = div()
            .size_full()
            .overflow_hidden()
            .rounded(px(10.0 * s))
            .when(!is_image, |el| el.bg(colors.card))
            .border_1()
            .border_color(if selected { colors.accent } else { colors.line })
            .child(content);
        Some(
            frame
                .child(surface)
                .when(selected && !editing, |el| {
                    el.child(
                        div()
                            .absolute()
                            .right(px(-5.))
                            .bottom(px(-5.))
                            .size(px(10.))
                            .rounded(px(3.))
                            .bg(colors.accent),
                    )
                })
                .into_any_element(),
        )
    }

    /// La tinta de todos los bloques de tinta, y el trazo que se está dibujando.
    fn ink_canvas(&self, layout: &Layout) -> impl IntoElement {
        let scale = layout.scale;
        let page_x = self.page as f32 * PAGE_W;
        let mut strokes: Vec<(Vec<(f32, f32)>, f32, Hsla)> = Vec::new();
        for block in &self.blocks {
            if let Body::Ink { strokes: list, .. } = &block.body {
                for stroke in list {
                    let points: Vec<(f32, f32)> =
                        stroke.points.iter().map(|p| (p[0], p[1])).collect();
                    strokes.push((points, stroke.width, parse_color(&stroke.color)));
                }
            }
        }
        if let Some(points) = &self.live_stroke {
            let highlight = self.tool == Tool::Highlight;
            let mut color: Hsla = rgb(PALETTE[self.color]).into();
            if highlight {
                color.a = 0x66 as f32 / 255.0;
            }
            strokes.push((
                points.iter().map(|p| (p[0], p[1])).collect(),
                if highlight { HIGHLIGHT_WIDTH } else { PEN_WIDTH },
                color,
            ));
        }
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for (points, width, color) in &strokes {
                        let smooth = crate::board::smooth(points);
                        let screen: Vec<Point<Pixels>> = smooth
                            .iter()
                            .map(|(x, y)| {
                                point(
                                    bounds.origin.x + px((x - page_x) * scale),
                                    bounds.origin.y + px(y * scale),
                                )
                            })
                            .collect();
                        let line_width = (width * scale).max(1.0);
                        let options = StrokeOptions::default()
                            .with_line_width(line_width)
                            .with_line_cap(LineCap::Round)
                            .with_line_join(LineJoin::Round);
                        let mut builder = PathBuilder::stroke(px(line_width))
                            .with_style(PathStyle::Stroke(options));
                        builder.add_polygon(&screen, false);
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, *color);
                        }
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }
}

impl Render for PaperView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.layout();
        let exporting_page = self.export_page.is_some();
        let theme = if exporting_page { &self.light } else { &self.colors };
        let sheet_w = PAGE_W * layout.scale;
        let sheet_h = PAGE_H * layout.scale;
        let blocks: Vec<AnyElement> = self
            .blocks
            .iter()
            .filter_map(|block| self.render_block(block, &layout, theme))
            .collect();
        let sheet = div()
            .absolute()
            .left(px(layout.x))
            .top(px(layout.y))
            .w(px(sheet_w))
            .h(px(sheet_h))
            .bg(theme.sheet)
            .overflow_hidden()
            // Al exportar, la hoja va lisa: sin borde ni esquinas, que saldrían
            // en la imagen.
            .when(!exporting_page, |el| {
                el.rounded(px(8.)).border_1().border_color(theme.line)
            })
            .child(self.ink_canvas(&layout))
            .children(blocks);
        if exporting_page {
            return div().size_full().relative().child(sheet).into_any_element();
        }
        let tool = self.tool;
        let inking = matches!(tool, Tool::Pen | Tool::Highlight);
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::delete_selected))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::paste_block))
            .on_action(cx.listener(Self::new_text))
            .on_action(cx.listener(Self::new_list))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .size_full()
            .relative()
            .font_family("Segoe UI")
            .text_color(theme.text)
            .bg(rgb(0x1b1b19))
            .rounded(px(self.radius))
            .cursor(match tool {
                Tool::Select => gpui::CursorStyle::Arrow,
                _ => gpui::CursorStyle::Crosshair,
            })
            .child(self.render_toolbar(cx))
            .child(sheet)
            .when(self.drawer, |el| el.child(self.render_drawer(cx)))
            .when(inking, |el| el.child(self.render_palette(cx)))
            .when(self.export_menu, |el| el.child(self.render_export_menu(cx)))
            .into_any_element()
    }
}

/// Dónde se guarda lo exportado: `Documentos\Tableros de Atic`.
fn export_destination(format: Format, pages: usize) -> PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Documents"))
        .unwrap_or_else(std::env::temp_dir)
        // No `Atic`: en Windows `Documentos\Atic` y `Documentostic` son la
        // misma carpeta, y puede ser el repositorio.
        .join("Tableros de Atic");
    let ext = match format {
        Format::Png | Format::Jpeg if pages > 1 => "zip",
        Format::Png => "png",
        Format::Jpeg => "jpg",
        Format::Pdf => "pdf",
        Format::Docx => "docx",
        Format::Pptx => "pptx",
    };
    base.join(format!(
        "tablero-{}.{ext}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ))
}

/// Abre el Explorador con el archivo elegido.
fn show_in_folder(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

/// Las capturas más recientes: las de Atic y las del prototipo.
fn recent_captures() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(data) = notes_dir().and_then(|notes| notes.parent().map(|p| p.to_path_buf())) {
        folders.push(data.join("captures"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        folders.push(PathBuf::from(local).join("atic-gpui").join("captures"));
    }
    let mut files: Vec<(SystemTime, PathBuf)> = folders
        .iter()
        .filter_map(|folder| std::fs::read_dir(folder).ok())
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp").then_some(())?;
            Some((entry.metadata().ok()?.modified().ok()?, path))
        })
        .collect();
    files.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    files.into_iter().take(30).map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: &[[f32; 2]]) -> Stroke {
        Stroke {
            color: "#e5483f".into(),
            width: 2.6,
            points: points.to_vec(),
        }
    }

    #[test]
    fn the_eraser_splits_a_stroke_where_it_cuts() {
        let line: Vec<[f32; 2]> = (0..11).map(|i| [i as f32 * 10.0, 0.0]).collect();
        let mut strokes = vec![stroke(&line)];
        // Corta por el medio (x = 50) con radio 15: caen los puntos 40, 50 y 60.
        assert!(erase_strokes(&mut strokes, (50.0, 0.0), 15.0));
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[0].points.last().unwrap()[0], 30.0);
        assert_eq!(strokes[1].points.first().unwrap()[0], 70.0);
    }

    #[test]
    fn the_eraser_leaves_distant_strokes_alone() {
        let mut strokes = vec![stroke(&[[0.0, 0.0], [10.0, 0.0]])];
        assert!(!erase_strokes(&mut strokes, (500.0, 500.0), 16.0));
        assert_eq!(strokes.len(), 1);
    }

    #[test]
    fn a_leftover_single_point_disappears() {
        let mut strokes = vec![stroke(&[[0.0, 0.0], [20.0, 0.0], [40.0, 0.0]])];
        // Borra los dos últimos: queda un solo punto, que no es un trazo.
        assert!(erase_strokes(&mut strokes, (30.0, 0.0), 12.0));
        assert!(strokes.is_empty());
    }

    #[test]
    fn highlight_colors_carry_their_alpha() {
        let solid = parse_color("#e5483f");
        let marker = parse_color("#e5483f66");
        assert!((solid.a - 1.0).abs() < 1e-6);
        assert!((marker.a - 0.4).abs() < 0.01);
    }

    #[test]
    fn blocks_roundtrip_in_atics_format() {
        let json = r##"[
            {"kind":"text","id":"t","body":"hola","x":1.0,"y":2.0,"w":3.0,"h":4.0},
            {"kind":"check","id":"c","items":[{"id":"i","text":"a","done":true}],"x":0.0,"y":0.0,"w":260.0,"h":120.0},
            {"kind":"image","id":"m","asset":"a.png","width":10,"height":20,"x":0.0,"y":0.0,"w":10.0,"h":20.0},
            {"kind":"ink","id":"k","strokes":[{"color":"#e5483f","width":2.6,"points":[[1.0,2.0],[3.0,4.0]]}],"height":0,"x":0.0,"y":0.0,"w":1200.0,"h":900.0}
        ]"##;
        let blocks: Vec<Block> = serde_json::from_str(json).expect("lee el formato de Atic");
        assert_eq!(blocks.len(), 4);
        // Escribir y volver a leer no cambia nada (el ancho 2.6 es f32, como en
        // Atic, y se guarda como 2.5999999).
        let once = serde_json::to_string(&blocks).unwrap();
        let again: Vec<Block> = serde_json::from_str(&once).unwrap();
        assert_eq!(serde_json::to_string(&again).unwrap(), once);
        let value: serde_json::Value = serde_json::from_str(&once).unwrap();
        let kinds: Vec<&str> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["text", "check", "image", "ink"]);
        assert_eq!(value[1]["items"][0]["done"], true);
    }

    #[test]
    fn pages_grow_with_content_to_the_right() {
        let mut block = Block {
            id: "t".into(),
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            body: Body::Text { body: String::new() },
        };
        assert_eq!(pages_for(&[block.clone()]), 1);
        block.x = PAGE_W + 10.0;
        assert_eq!(pages_for(&[block]), 2);
    }

    #[test]
    fn png_size_reads_the_header() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((640, 480)));
        assert_eq!(png_size(b"nope"), None);
    }

    #[test]
    fn saving_keeps_the_rest_of_the_note() {
        let dir = std::env::temp_dir().join(format!("pill-board-test-{}", now_ms()));
        let board = dir.join("atic-tablero");
        std::fs::create_dir_all(&board).unwrap();
        std::fs::write(
            board.join("note.json"),
            r#"{"id":"n","app":"atic-tablero","pages":[
                {"id":"p0","title":"otra","blocks":[],"updated_at":1},
                {"id":"p1","title":"","blocks":[],"updated_at":1}],"updated_at":1}"#,
        )
        .unwrap();
        std::env::set_var("ATIC_NOTES_DIR", &dir);
        let blocks = vec![Block {
            id: "t".into(),
            x: 1.0,
            y: 1.0,
            w: 100.0,
            h: 50.0,
            body: Body::Text { body: "hola".into() },
        }];
        assert!(save_blocks(&blocks));
        let note: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(board.join("note.json")).unwrap())
                .unwrap();
        let pages = note["pages"].as_array().unwrap();
        // La otra página queda intacta; la sin título tiene el bloque.
        assert_eq!(pages[0]["title"], "otra");
        assert_eq!(pages[1]["blocks"][0]["body"], "hola");
        assert_eq!(load_blocks().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
