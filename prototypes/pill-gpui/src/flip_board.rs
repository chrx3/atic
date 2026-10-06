//! El tablero del reverso del flip: un papel continuo de páginas de 1200×900,
//! con textos, listas, imágenes y tinta que se mueven, se redimensionan y se
//! guardan solos.
//!
//! Lee y escribe el mismo `notes/atic-tablero/note.json` que usa Atic
//! (`notes.rs` del backend), con el mismo formato de bloques, así que lo que se
//! escribe aquí aparece allá y al revés. Solo toca la página sin título; el
//! resto del archivo queda como estaba.
//!
//! Se maneja como `FlipBoard.svelte`: zoom y desplazamiento, la tira de
//! miniaturas abajo, el dock de herramientas con sus teclas, selección por
//! recuadro y cajas de texto que se ajustan solas. Faltan, respecto de Atic,
//! elegir y escalar trazos sueltos, la edición de listas ítem por ítem, el
//! agarre y la ✕ de cada bloque, y la pestaña Reuniones del cajón.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    actions, canvas, div, img, point, prelude::*, px, rgb, svg, AnyElement, App, BoxShadow,
    ClipboardEntry, ContentMask, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    KeyBinding, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, PathBuilder, PathStyle, Pixels, Point, ScrollWheelEvent,
    SharedString, Subscription, Window,
};
use lyon::tessellation::{LineCap, LineJoin, StrokeOptions};
use serde::{Deserialize, Serialize};

use crate::anim::{ease_smooth_out, lerp, Tween};
use crate::hover::{tip, HoverExt};
use crate::text_area::TextArea;
use crate::text_input;

/// Página lógica, en píxeles de papel (`PAGINA_W` / `PAGINA_H`).
pub(crate) const PAGE_W: f32 = 1200.0;
pub(crate) const PAGE_H: f32 = 900.0;
/// Cota del papel (`TABLERO_MAX` = 12000): diez páginas.
const MAX_PAGES: usize = 10;
const MARGIN: f32 = 24.0;
const TEXT_W: f32 = 280.0;
const TEXT_H: f32 = 96.0;
const LIST_W: f32 = 260.0;
const LIST_H: f32 = 120.0;
const MIN_W: f32 = 80.0;
const MIN_H: f32 = 48.0;
const FONT_FAMILY: &str = "Segoe UI";
const FONT: f32 = 13.5;
/// Piso de legibilidad: por debajo no se encoge aunque no entre.
const FONT_MIN: f32 = 8.5;
const LINE: f32 = 1.5;
/// Relleno de la caja de texto (`18px 12px 10px`): sumas en cada eje.
const TEXT_PAD_X: f32 = 24.0;
const TEXT_PAD_Y: f32 = 28.0;
const TEXT_PAD_TOP: f32 = 18.0;
/// Ancho de una caja nueva o casi vacía, y hasta dónde se ensancha sola.
const TEXT_MIN_W: f32 = 160.0;
const TEXT_MAX_W: f32 = 560.0;
const TEXT_MIN_H: f32 = 48.0;
/// Aire para el cursor al final del renglón más largo.
const TEXT_CURSOR: f32 = 6.0;
const PLACEHOLDER: &str = "Escribe aquí. Se guarda solo y lo ves desde cualquier ventana.";
const BLOCK_RADIUS: f32 = 5.0;
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;
const ROW_H: f32 = 28.0;
const TOOLBAR_H: f32 = 40.0;
const PEN_WIDTH: f32 = 2.6;
const HIGHLIGHT_WIDTH: f32 = 12.0;
/// Distancia mínima entre puntos de un trazo, en papel.
const INK_MIN_STEP: f32 = 1.2;
/// Tamaños del borrador (radio en papel): chico, mediano y grande.
const ERASER_RADII: [f32; 3] = [8.0, 16.0, 32.0];
const UNDO_MAX: usize = 50;
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// Lo que hay que mover el cursor para que un clic sea arrastre.
const DRAG_MIN: f32 = 4.0;
/// Lado del agarre de la esquina para redimensionar, en pantalla.
const HANDLE: f32 = 16.0;
const ZOOM_MIN: f32 = 0.3;
const ZOOM_MAX: f32 = 2.4;
/// Los saltos de vista (tira, zoom, encuadre).
const VIEW_ANIM: Duration = Duration::from_millis(160);
/// El destello de una página nueva: dos pulsos de 1,2 s.
const FLASH: Duration = Duration::from_millis(2400);
const STRIP_PAD_TOP: f32 = 6.0;
const STRIP_PAD_BOTTOM: f32 = 10.0;
/// El dock: botones de 34, relleno de 4, separación de 2 y separadores de
/// 1 px con 4 de margen a cada lado.
const DOCK_BTN: f32 = 34.0;
const DOCK_PAD: f32 = 4.0;
const DOCK_GAP: f32 = 2.0;
const DOCK_SEP: f32 = 9.0;
/// El mismo tope que el historial de Atic para una imagen pegada.
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const PAGE_NAMES: [&str; MAX_PAGES] = [
    "Página 1", "Página 2", "Página 3", "Página 4", "Página 5", "Página 6", "Página 7",
    "Página 8", "Página 9", "Página 10",
];

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

/// Los íconos del tablero que no usa nadie más; los carga el `AssetSource` de
/// la app.
pub fn icon(path: &str) -> Option<&'static [u8]> {
    Some(match path {
        "icons/board/mouse-pointer-2.svg" => include_bytes!("../assets/icons/board/mouse-pointer-2.svg"),
        "icons/board/hand.svg" => include_bytes!("../assets/icons/board/hand.svg"),
        "icons/board/list-checks.svg" => include_bytes!("../assets/icons/board/list-checks.svg"),
        "icons/board/minus.svg" => include_bytes!("../assets/icons/board/minus.svg"),
        "icons/board/download.svg" => include_bytes!("../assets/icons/board/download.svg"),
        "icons/board/panel-right-open.svg" => include_bytes!("../assets/icons/board/panel-right-open.svg"),
        "icons/board/panel-right-close.svg" => include_bytes!("../assets/icons/board/panel-right-close.svg"),
        "icons/board/arrow-left.svg" => include_bytes!("../assets/icons/board/arrow-left.svg"),
        "icons/board/clipboard-paste.svg" => include_bytes!("../assets/icons/board/clipboard-paste.svg"),
        _ => return None,
    })
}

/// Los lápices de Atic (`LAPICES`): los trazos guardados allá se ven igual.
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

/// La página con la que abre el próximo Flip (`usize::MAX`: la primera).
pub(crate) static OPEN_PAGE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// Lo que se ve de una página del tablero, para el vistazo de Flip.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct PageGlance {
    /// La primera línea con texto, o vacía.
    pub(crate) summary: String,
    /// Los bloques de la página, en proporción a ella (0..1): para dibujar
    /// una miniatura. El `u8` dice qué es: 0 texto, 1 imagen, 2 lista.
    pub(crate) shapes: Vec<(f32, f32, f32, f32, u8)>,
    /// Los trazos a mano que caen en la página, en proporción (0..1), con su
    /// color.
    pub(crate) ink: Vec<(Vec<(f32, f32)>, (u8, u8, u8, u8))>,
}

/// Las páginas del tablero de Atic, leídas del disco.
pub(crate) fn board_glance() -> Vec<PageGlance> {
    let blocks = load_blocks();
    let count = pages_for(&blocks).max(1);
    (0..count)
        .map(|page| {
            let left = page as f32 * PAGE_W;
            let on_page: Vec<&Block> = blocks
                .iter()
                .filter(|b| b.x >= left && b.x < left + PAGE_W)
                .collect();
            let summary = on_page
                .iter()
                .find_map(|b| match &b.body {
                    Body::Text { body } => body.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string),
                    Body::Check { items } => items.iter().map(|i| i.text.trim()).find(|t| !t.is_empty()).map(str::to_string),
                    _ => None,
                })
                .unwrap_or_default();
            let shapes = on_page
                .iter()
                .filter(|b| b.has_frame() && !b.is_ink())
                .map(|b| {
                    let kind = match b.body {
                        Body::Text { .. } => 0,
                        Body::Image { .. } => 1,
                        _ => 2,
                    };
                    (
                        ((b.x - left) / PAGE_W).clamp(0.0, 1.0),
                        (b.y / PAGE_H).clamp(0.0, 1.0),
                        (b.w / PAGE_W).clamp(0.0, 1.0),
                        (b.h / PAGE_H).clamp(0.0, 1.0),
                        kind,
                    )
                })
                .collect();
            // Los trazos van en coordenadas del lienzo entero: los que tocan
            // esta página, corridos a ella.
            let ink = blocks
                .iter()
                .filter_map(|b| match &b.body {
                    Body::Ink { strokes, .. } => Some(strokes),
                    _ => None,
                })
                .flatten()
                .filter(|s| s.points.len() >= 2 && s.points.iter().any(|p| p[0] >= left && p[0] < left + PAGE_W))
                .map(|s| {
                    let points = s
                        .points
                        .iter()
                        .map(|p| ((p[0] - left) / PAGE_W, p[1] / PAGE_H))
                        .collect();
                    (points, parse_rgba(&s.color))
                })
                .collect();
            PageGlance { summary, shapes, ink }
        })
        .collect()
}

pub(crate) fn board_dir() -> Option<PathBuf> {
    notes_dir().map(|dir| dir.join("atic-tablero"))
}

/// La página del tablero es la de título vacío, como `page_for_title("")` en
/// Atic: se lee y se escribe la misma, nunca otra.
fn is_board_page(page: &serde_json::Value) -> bool {
    page["title"].as_str().is_some_and(str::is_empty)
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
    note["pages"]
        .as_array()
        .and_then(|pages| pages.iter().find(|page| is_board_page(page)))
        .and_then(|page| page["blocks"].as_array())
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
    let raw = std::fs::read_to_string(&file).ok();
    let parsed = raw.as_deref().and_then(|raw| serde_json::from_str(raw).ok());
    if raw.is_some() && parsed.is_none() {
        // Ilegible: antes que escribir encima, se deja al lado como hace
        // Atic. Es texto del usuario que no tiene en otro lado.
        let _ = std::fs::rename(&file, dir.join("note.roto.json"));
    }
    let mut note: serde_json::Value = parsed.unwrap_or_else(|| {
        serde_json::json!({
            "id": format!("pill-{}", now_ms()),
            "app": "atic-tablero",
            "pages": [],
            "updated_at": secs,
        })
    });
    let Ok(serde_json::Value::Array(mut blocks)) = serde_json::to_value(blocks) else {
        return false;
    };
    if !note["pages"].is_array() {
        note["pages"] = serde_json::json!([]);
    }
    let pages = note["pages"].as_array_mut().expect("pages es un arreglo");
    match pages.iter_mut().find(|page| is_board_page(page)) {
        Some(page) => {
            // Los bloques que esta versión no sabe leer (de una versión más
            // nueva de Atic) se conservan tal cual.
            if let Some(old) = page["blocks"].as_array() {
                blocks.extend(
                    old.iter()
                        .filter(|block| serde_json::from_value::<Block>((*block).clone()).is_err())
                        .cloned(),
                );
            }
            page["blocks"] = blocks.into();
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

/// Borra de `assets/` las imágenes que ya no menciona ninguna página, como
/// `notes::collect_garbage` en Atic. Solo al cerrar el tablero: mientras está
/// abierto, deshacer puede devolver una imagen que se acaba de quitar.
fn collect_garbage() {
    let Some(dir) = board_dir() else {
        return;
    };
    let Some(note) = std::fs::read_to_string(dir.join("note.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    else {
        // Sin una nota legible no se sabe qué está en uso: no se borra nada.
        return;
    };
    let alive: std::collections::HashSet<&str> = note["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|page| page["blocks"].as_array())
        .flatten()
        .filter(|block| block["kind"] == "image")
        .filter_map(|block| block["asset"].as_str())
        .collect();
    let Ok(entries) = std::fs::read_dir(dir.join("assets")) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| !alive.contains(name)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
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

// --- Medir texto ------------------------------------------------------------
//
// Las mismas cuentas que `flipLayout.ts` de Atic (`medidaTexto`,
// `altoParaAncho`, `ajustarFuente`): la caja se ajusta a lo escrito y, si no
// entra, la letra se achica. Miden con una función para poder probarlas sin
// ventana.

/// Ancho en px de una cadena a un tamaño de letra.
type Measure<'a> = &'a dyn Fn(&str, f32) -> f32;

/// Envuelve por palabras al ancho dado (`envolver`): cuántos renglones salen.
fn wrapped_lines(text: &str, max_w: f32, size: f32, measure: Measure) -> usize {
    let space = measure(" ", size);
    // Una palabra más ancha que la caja la parte GPUI: ocupa varios renglones.
    let rows_of = |w: f32| (w / max_w.max(1.0)).ceil().max(1.0) as usize;
    let mut lines = 0;
    let mut current: Option<f32> = None;
    for word in text.split_whitespace() {
        let w = measure(word, size);
        match current {
            Some(width) if width + space + w <= max_w => current = Some(width + space + w),
            _ => {
                lines += rows_of(w);
                current = Some(if w > max_w { w % max_w } else { w });
            }
        }
    }
    lines.max(1)
}

/// Alto que pide el cuerpo a un ancho y tamaño dados (`altoEnvolvente`).
fn wrapped_height(body: &str, width: f32, size: f32, measure: Measure) -> f32 {
    let usable = (width - TEXT_PAD_X).max(8.0);
    let lines: usize = body
        .split('\n')
        .map(|line| wrapped_lines(line, usable, size, measure))
        .sum();
    TEXT_PAD_Y + lines as f32 * size * LINE
}

/// Alto a un ancho dado, sin bajar del mínimo de una caja (`altoParaAncho`).
fn height_for_width(body: &str, width: f32, measure: Measure) -> f32 {
    TEXT_MIN_H.max(wrapped_height(body, width, FONT, measure).ceil())
}

/// La medida que pide un texto para verse entero (`medidaTexto`): el ancho
/// del renglón más largo, entre el mínimo y el tope; pasado el tope envuelve.
fn text_size(body: &str, measure: Measure) -> (f32, f32) {
    let longest = body
        .split('\n')
        .map(|line| measure(line, FONT))
        .fold(0.0, f32::max);
    let w = (longest + TEXT_PAD_X + TEXT_CURSOR)
        .clamp(TEXT_MIN_W, TEXT_MAX_W)
        .ceil();
    (w, height_for_width(body, w, measure))
}

/// Vacía, el ancho de siempre con el placeholder envuelto (`medidaAuto`).
fn auto_size(body: &str, measure: Measure) -> (f32, f32) {
    if body.is_empty() {
        return (TEXT_W, height_for_width(PLACEHOLDER, TEXT_W, measure));
    }
    text_size(body, measure)
}

/// La letra más grande con la que el cuerpo entra en la caja (`ajustarFuente`):
/// nunca sube del tamaño base ni baja del mínimo.
fn fit_font(body: &str, w: f32, h: f32, measure: Measure) -> f32 {
    if body.trim().is_empty() || w <= 0.0 || h <= 0.0 {
        return FONT;
    }
    let fits = |size: f32| wrapped_height(body, w, size, measure) <= h;
    if fits(FONT) {
        return FONT;
    }
    let (mut small, mut big) = (FONT_MIN, FONT);
    for _ in 0..12 {
        let mid = (small + big) / 2.0;
        if fits(mid) {
            small = mid;
        } else {
            big = mid;
        }
    }
    FONT_MIN.max((small * 10.0).round() / 10.0)
}

/// Mide con la letra del tablero, como el `canvas` que usa Atic.
struct TextMeasure {
    system: std::sync::Arc<gpui::TextSystem>,
    font: gpui::FontId,
}

impl TextMeasure {
    fn new(cx: &App) -> Self {
        let system = cx.text_system().clone();
        let font = system.resolve_font(&gpui::font(FONT_FAMILY));
        Self { system, font }
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|ch| {
                self.system
                    .advance(self.font, px(size), ch)
                    .map(|s| f32::from(s.width))
                    .unwrap_or(size * 0.5)
            })
            .sum()
    }
}

// --- Páginas, grupos y trazos ------------------------------------------------

/// Caja de un trazo contando su grosor (`cajaTrazo`).
fn stroke_box(stroke: &Stroke) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &stroke.points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    if x0 > x1 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let pad = stroke.width / 2.0;
    (x0 - pad, y0 - pad, x1 - x0 + pad * 2.0, y1 - y0 + pad * 2.0)
}

/// La página de algo: la de su centro, así eliminar una página nunca parte
/// a la mitad lo que cruza el borde (`paginaDe`).
fn page_of(x: f32, w: f32) -> usize {
    ((x + w / 2.0) / PAGE_W).floor().max(0.0) as usize
}

/// Cuánto hay en una página: lo que se perdería al eliminarla.
fn page_content(blocks: &[Block], index: usize) -> (usize, usize) {
    let mut objects = 0;
    let mut strokes = 0;
    for block in blocks {
        match &block.body {
            Body::Ink { strokes: list, .. } => {
                strokes += list
                    .iter()
                    .filter(|s| {
                        let (x, _, w, _) = stroke_box(s);
                        page_of(x, w) == index
                    })
                    .count();
            }
            _ if page_of(block.x, block.w) == index => objects += 1,
            _ => {}
        }
    }
    (objects, strokes)
}

/// El tablero sin la página `index`: se va lo que tenía y lo de la derecha se
/// corre una página a la izquierda (`quitarPaginaDe`).
fn without_page(blocks: &[Block], index: usize) -> Vec<Block> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        if let Body::Ink { strokes, height } = &block.body {
            let strokes = strokes
                .iter()
                .filter_map(|s| {
                    let (x, _, w, _) = stroke_box(s);
                    match page_of(x, w) {
                        page if page == index => None,
                        page if page > index => Some(Stroke {
                            points: s.points.iter().map(|p| [p[0] - PAGE_W, p[1]]).collect(),
                            ..s.clone()
                        }),
                        _ => Some(s.clone()),
                    }
                })
                .collect();
            out.push(Block {
                body: Body::Ink {
                    strokes,
                    height: *height,
                },
                ..block.clone()
            });
            continue;
        }
        match page_of(block.x, block.w) {
            page if page == index => {}
            page if page > index => out.push(Block {
                x: block.x - PAGE_W,
                ..block.clone()
            }),
            _ => out.push(block.clone()),
        }
    }
    out
}

/// Varios elementos elegidos a la vez: bloques por id y trazos (del bloque de
/// tinta) por índice.
#[derive(Clone, Default, PartialEq)]
struct Group {
    ids: Vec<String>,
    strokes: Vec<usize>,
}

impl Group {
    fn len(&self) -> usize {
        self.ids.len() + self.strokes.len()
    }
}

/// Lo que toca un recuadro (`enRecuadro`): un bloque si su marco se cruza, un
/// trazo si alguno de sus puntos cae adentro.
fn in_marquee(blocks: &[Block], (x, y, w, h): (f32, f32, f32, f32)) -> Group {
    let (x1, y1) = (x + w, y + h);
    let mut group = Group::default();
    // Los índices de trazo son del primer bloque de tinta, el que se dibuja
    // y se edita.
    let mut ink_seen = false;
    for block in blocks {
        match &block.body {
            Body::Ink { strokes, .. } => {
                if !std::mem::replace(&mut ink_seen, true) {
                    for (i, stroke) in strokes.iter().enumerate() {
                        if stroke.points.iter().any(|p| p[0] >= x && p[0] <= x1 && p[1] >= y && p[1] <= y1) {
                            group.strokes.push(i);
                        }
                    }
                }
            }
            _ => {
                if block.x < x1 && block.x + block.w > x && block.y < y1 && block.y + block.h > y {
                    group.ids.push(block.id.clone());
                }
            }
        }
    }
    group
}

/// Todo el tablero: lo que elige Ctrl+A.
fn whole_board(blocks: &[Block]) -> Group {
    let mut group = Group::default();
    let mut ink_seen = false;
    for block in blocks {
        match &block.body {
            Body::Ink { strokes, .. } => {
                if !std::mem::replace(&mut ink_seen, true) {
                    group.strokes = (0..strokes.len()).collect();
                }
            }
            _ => group.ids.push(block.id.clone()),
        }
    }
    group
}

/// El trazo entero más cercano al punto, si está a `radius` o menos
/// (`borrarLineaCercana`): el clic limpio del borrador.
fn nearest_stroke(strokes: &[Stroke], (x, y): (f32, f32), radius: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, stroke) in strokes.iter().enumerate() {
        for p in &stroke.points {
            let d = (p[0] - x).hypot(p[1] - y);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
    }
    best.filter(|(_, d)| *d <= radius).map(|(i, _)| i)
}

/// Una miniatura de página: lo mínimo para dibujarla (`miniaturasTablero`).
struct Mini {
    /// En proporción a la página (0..1).
    pieces: Vec<MiniPiece>,
    ink: Vec<(Vec<(f32, f32)>, f32, Hsla)>,
}

struct MiniPiece {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Imagen: su archivo; lista: filas; texto: renglones.
    kind: MiniKind,
}

enum MiniKind {
    Image(PathBuf),
    List(usize),
    Text(usize),
}

fn minis(blocks: &[Block], pages: usize) -> Vec<Mini> {
    let assets = board_dir().map(|dir| dir.join("assets"));
    (0..pages)
        .map(|page| {
            let left = page as f32 * PAGE_W;
            let pieces = blocks
                .iter()
                .filter(|b| !b.is_ink() && b.x < left + PAGE_W && b.x + b.w > left && b.has_frame())
                .filter_map(|b| {
                    let kind = match &b.body {
                        Body::Image { asset, .. } => MiniKind::Image(assets.as_ref()?.join(asset)),
                        Body::Check { items } => MiniKind::List(items.len().min(4)),
                        _ => MiniKind::Text(((b.h / 24.0).round() as usize).clamp(1, 4)),
                    };
                    Some(MiniPiece {
                        x: (b.x - left) / PAGE_W,
                        y: b.y / PAGE_H,
                        w: b.w / PAGE_W,
                        h: b.h / PAGE_H,
                        kind,
                    })
                })
                .collect();
            let ink = blocks
                .iter()
                .filter_map(|b| match &b.body {
                    Body::Ink { strokes, .. } => Some(strokes),
                    _ => None,
                })
                .flatten()
                .filter(|s| s.points.iter().any(|p| p[0] >= left && p[0] < left + PAGE_W))
                .map(|s| {
                    let mut color = parse_color(&s.color);
                    color.a = 1.0;
                    (
                        s.points.iter().map(|p| ((p[0] - left) / PAGE_W, p[1] / PAGE_H)).collect(),
                        s.width * 2.0,
                        color,
                    )
                })
                .collect();
            Mini { pieces, ink }
        })
        .collect()
}

// --- La vista --------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Hand,
    Pen,
    Highlight,
    Eraser,
    Text,
    List,
}

impl Tool {
    /// El dock, en orden: ícono y nombre con su tecla (`herramientas`).
    const DOCK: [(Tool, &'static str, &'static str); 7] = [
        (Tool::Select, "icons/board/mouse-pointer-2.svg", "Mover (V)"),
        (Tool::Hand, "icons/board/hand.svg", "Mano (M · Espacio)"),
        (Tool::Pen, "icons/pencil.svg", "Lápiz (P)"),
        (Tool::Highlight, "icons/highlighter.svg", "Resaltador (H)"),
        (Tool::Eraser, "icons/eraser.svg", "Borrador (E)"),
        (Tool::Text, "icons/type.svg", "Texto (T)"),
        (Tool::List, "icons/board/list-checks.svg", "Lista (L)"),
    ];

    fn inks(self) -> bool {
        matches!(self, Tool::Pen | Tool::Highlight)
    }

    /// Lápices y borrador tienen paleta.
    fn has_palette(self) -> bool {
        matches!(self, Tool::Pen | Tool::Highlight | Tool::Eraser)
    }

    fn index(self) -> usize {
        Self::DOCK.iter().position(|(tool, _, _)| *tool == self).unwrap_or(0)
    }
}

/// Dónde cae el botón `i` del dock, desde su borde izquierdo: botones de 34,
/// separaciones de 2 y un separador antes del lápiz y del texto.
fn dock_x(i: usize) -> f32 {
    let seps = (i >= 2) as usize + (i >= 5) as usize;
    DOCK_PAD + i as f32 * (DOCK_BTN + DOCK_GAP) + seps as f32 * (DOCK_SEP + DOCK_GAP)
}

enum Drag {
    /// Mover o redimensionar un bloque: su marco al empezar y dónde se apretó.
    Move { id: String, from: (f32, f32), start: Point<Pixels> },
    /// `registered`: ya se guardó el paso de deshacer (al primer movimiento).
    Resize { id: String, from: (f32, f32), start: Point<Pixels>, registered: bool },
    /// Apretado sobre un bloque sin moverse todavía: si no hay arrastre, es un
    /// clic.
    Press { id: String, start: Point<Pixels>, was_selected: bool, click: usize },
    Ink,
    /// El borrador: sin moverse de verdad es un clic, que borra la línea
    /// entera más cercana al soltar.
    Erase { start: Point<Pixels>, last: (f32, f32), moved: bool, registered: bool },
    /// Mover la vista: con la mano, Espacio o el botón del medio.
    Pan { start: Point<Pixels>, from: (f32, f32) },
    /// Recuadro de selección desde el papel vacío.
    Marquee { start: Point<Pixels>, from: (f32, f32), moved: bool },
}

pub enum PaperEvent {
    Close,
}

struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    /// Fondo de la vista, detrás del papel.
    bg: Hsla,
    sheet: Hsla,
    /// Superficie de lo que flota (dock, menús, cajón).
    card: Hsla,
    line: Hsla,
    accent: Hsla,
    ok: Hsla,
    danger: Hsla,
}

/// Pedido de confirmación en curso.
#[derive(Clone, Copy)]
enum Confirm {
    /// Borrar lo elegido: cuántos y si es todo el tablero.
    Group { count: usize, all: bool },
    /// Eliminar (o vaciar, si es la única) una página con contenido.
    Page { index: usize, objects: usize, strokes: usize },
}

/// Un salto de la vista animado: de dónde parte.
struct ViewAnim {
    zoom: f32,
    pan: (f32, f32),
    start: Instant,
}

pub struct PaperView {
    blocks: Vec<Block>,
    /// Páginas del papel, aunque estén vacías: lo que se guarda es el
    /// contenido, las páginas vacías viven en la sesión.
    paper_pages: usize,
    selected: Option<String>,
    group: Option<Group>,
    marquee: Option<(f32, f32, f32, f32)>,
    editing: Option<String>,
    /// El tablero antes de entrar a escribir (`sesionAntes`): al salir, si
    /// cambió, queda como un solo paso de deshacer.
    edit_before: Option<Vec<Block>>,
    /// La caja que se edita se acaba de crear: su paso de deshacer es el de
    /// crearla.
    edit_fresh: bool,
    /// La letra ajustada de cada caja, por su texto y medida: medir cada
    /// cuadro saldría caro con textos largos.
    font_cache: std::cell::RefCell<std::collections::HashMap<String, (String, f32, f32, f32)>>,
    editor: Entity<TextArea>,
    tool: Tool,
    palette: bool,
    color: usize,
    eraser_radius: f32,
    /// Dónde está el borrador, para dibujar su alcance.
    eraser_at: Option<(f32, f32)>,
    /// Espacio apretado: mano mientras dure.
    space: bool,
    undo: Vec<Vec<Block>>,
    redo: Vec<Vec<Block>>,
    drag: Option<Drag>,
    live_stroke: Option<Vec<[f32; 2]>>,
    /// La vista: zoom y corrimiento en pantalla. Mientras nadie la toque se
    /// encuadra sola con el tamaño de la tarjeta (`vistaTocada`).
    zoom: f32,
    pan: (f32, f32),
    view_touched: bool,
    view_anim: Option<ViewAnim>,
    /// La página a la que hay que ir en cuanto se sepa el tamaño de la vista.
    pending_page: Option<usize>,
    /// Destello en el borde de una página recién agregada o quitada.
    flash: Option<(f32, Instant)>,
    /// La píldora del dock que se desliza a la herramienta activa.
    dock_pill: Tween,
    hovered_mini: Option<usize>,
    confirm: Option<Confirm>,
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
        if !self.dirty || save_blocks(&self.blocks) {
            collect_garbage();
        }
    }
}

/// El papel dentro de la vista: dónde cae su esquina y a qué escala, en
/// coordenadas de la vista.
struct Layout {
    x: f32,
    y: f32,
    scale: f32,
}

impl PaperView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let text: Hsla = rgb(0xf0f0ea).into();
        let colors = Colors {
            text,
            muted: rgb(0x9a9a90).into(),
            faint: rgb(0x6e6e66).into(),
            bg: rgb(0x1a1a18).into(),
            // `--hoja` del tema oscuro de Atic: la superficie elevada con un
            // 30 % del texto.
            sheet: rgb(0x656561).into(),
            card: rgb(0x2a2a26).into(),
            line: text.opacity(0.12),
            accent: text,
            ok: rgb(0x6faf88).into(),
            danger: rgb(0xe5483f).into(),
        };
        let editor = cx.new(|cx| {
            TextArea::new(PLACEHOLDER, colors.text, colors.muted, colors.text, cx)
        });
        let editor_changed = cx.subscribe(&editor, |paper, _, _: &text_input::Changed, cx| {
            paper.apply_edit(cx);
        });
        let mut blocks = load_blocks();
        place_if_needed(&mut blocks);
        // El vistazo de Flip puede pedir una página.
        let page = OPEN_PAGE.swap(usize::MAX, std::sync::atomic::Ordering::Relaxed);
        let pending_page = (page != usize::MAX).then(|| page.min(pages_for(&blocks).max(1) - 1));
        let mut paper = Self {
            blocks,
            paper_pages: 1,
            selected: None,
            group: None,
            marquee: None,
            editing: None,
            edit_before: None,
            edit_fresh: false,
            font_cache: Default::default(),
            editor,
            tool: Tool::Select,
            palette: false,
            color: 0,
            eraser_radius: ERASER_RADII[1],
            eraser_at: None,
            space: false,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            live_stroke: None,
            zoom: 1.0,
            pan: (0.0, 0.0),
            view_touched: false,
            view_anim: None,
            pending_page,
            flash: None,
            dock_pill: Tween::new(dock_x(0), Duration::from_millis(250), ease_smooth_out),
            hovered_mini: None,
            confirm: None,
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
                bg: rgb(0xf4f1ea).into(),
                sheet: rgb(0xf4f1ea).into(),
                card: rgb(0xfffdf8).into(),
                line: rgb(0xd9d4c8).into(),
                accent: rgb(0x1c1917).into(),
                ok: rgb(0x3f7355).into(),
                danger: rgb(0xc0392b).into(),
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
        paper.paper_pages = paper.pages();
        paper
    }

    /// Lo pone quien aloja la vista, cada cuadro.
    pub fn sync(&mut self, origin: (f32, f32), size: (f32, f32), radius: f32, env: Env) {
        self.origin = origin;
        self.size = size;
        self.radius = radius;
        self.env = env;
        // Recién ahora se sabe cuánto mide la vista: el salto pedido al abrir
        // va sin animación.
        if let Some(page) = self.pending_page.take() {
            self.show_page(page);
            self.view_anim = None;
        }
    }

    fn pages(&self) -> usize {
        pages_for(&self.blocks).max(self.paper_pages).clamp(1, MAX_PAGES)
    }

    fn paper_w(&self) -> f32 {
        self.pages() as f32 * PAGE_W
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

    /// Ancho de una miniatura: `clamp(52px, 7vw, 84px)` como en Atic.
    fn mini_w(&self) -> f32 {
        (self.size.0 * 0.07).clamp(52.0, 84.0)
    }

    fn strip_h(&self) -> f32 {
        self.mini_w() * 0.75 + STRIP_PAD_TOP + STRIP_PAD_BOTTOM
    }

    /// El rectángulo de la vista del papel: entre la barra y la tira, sin el
    /// cajón.
    fn view_box(&self) -> (f32, f32, f32, f32) {
        let (w, h) = self.size;
        (
            0.0,
            TOOLBAR_H,
            (w - self.drawer_width()).max(100.0),
            (h - TOOLBAR_H - self.strip_h()).max(100.0),
        )
    }

    /// El zoom que encuadra el papel entero sin pasarse del 100 % (`encuadrar`).
    fn fit_zoom(&self) -> f32 {
        let (_, _, vw, vh) = self.view_box();
        ((vw - 24.0) / self.paper_w())
            .min((vh - 24.0) / PAGE_H)
            .min(1.0)
            .max(0.05)
    }

    /// Zoom y corrimiento a los que va la vista.
    fn view_target(&self) -> (f32, (f32, f32)) {
        if self.view_touched {
            (self.zoom, self.pan)
        } else {
            (self.fit_zoom(), (0.0, 0.0))
        }
    }

    /// Zoom y corrimiento de este cuadro, con el salto animado si lo hay.
    fn view(&self) -> (f32, (f32, f32)) {
        let (zoom, pan) = self.view_target();
        let Some(anim) = &self.view_anim else {
            return (zoom, pan);
        };
        let t = (anim.start.elapsed().as_secs_f32() / VIEW_ANIM.as_secs_f32()).clamp(0.0, 1.0);
        let e = ease_smooth_out(t);
        (
            lerp(anim.zoom, zoom, e),
            (lerp(anim.pan.0, pan.0, e), lerp(anim.pan.1, pan.1, e)),
        )
    }

    /// El usuario toma la vista: desde acá manda lo que haga y el cambio de
    /// tamaño de la tarjeta ya no la encuadra.
    fn touch_view(&mut self) {
        if !self.view_touched {
            let (zoom, pan) = self.view_target();
            self.zoom = zoom;
            self.pan = pan;
            self.view_touched = true;
        }
    }

    /// Mueve la vista con transición, como `moverVista` (arrastre y rueda no
    /// pasan por acá: ahí el movimiento es continuo).
    fn animate_view(&mut self, change: impl FnOnce(&mut Self)) {
        let (zoom, pan) = self.view();
        change(self);
        self.view_anim = Some(ViewAnim {
            zoom,
            pan,
            start: Instant::now(),
        });
    }

    fn layout(&self) -> Layout {
        if let Some(page) = self.export_page {
            // Centrada en el monitor, sin barra ni cajón, en coordenadas de la
            // vista; la esquina es la de esa página.
            let scale = self.export_scale();
            return Layout {
                x: self.env.offset.0 + (self.env.screen.0 - PAGE_W * scale) / 2.0
                    - self.origin.0
                    - page as f32 * PAGE_W * scale,
                y: self.env.offset.1 + (self.env.screen.1 - PAGE_H * scale) / 2.0 - self.origin.1,
                scale,
            };
        }
        let (zoom, pan) = self.view();
        let (vx, vy, vw, vh) = self.view_box();
        Layout {
            x: vx + vw / 2.0 + pan.0 - self.paper_w() * zoom / 2.0,
            y: vy + vh / 2.0 + pan.1 - PAGE_H * zoom / 2.0,
            scale: zoom,
        }
    }

    fn local(&self, p: Point<Pixels>) -> (f32, f32) {
        (f32::from(p.x) - self.origin.0, f32::from(p.y) - self.origin.1)
    }

    /// Posición de la ventana → papel.
    fn to_paper(&self, p: Point<Pixels>) -> (f32, f32) {
        let layout = self.layout();
        let (x, y) = self.local(p);
        ((x - layout.x) / layout.scale, (y - layout.y) / layout.scale)
    }

    /// El punto del papel al centro de la vista: donde cae lo pegado.
    fn visible_center(&self) -> (f32, f32) {
        let layout = self.layout();
        let (vx, vy, vw, vh) = self.view_box();
        (
            ((vx + vw / 2.0 - layout.x) / layout.scale).clamp(0.0, self.paper_w()),
            ((vy + vh / 2.0 - layout.y) / layout.scale).clamp(0.0, PAGE_H),
        )
    }

    /// La página que mira el centro de la vista.
    fn current_page(&self) -> usize {
        let (zoom, pan) = self.view();
        let x = self.paper_w() / 2.0 - pan.0 / zoom;
        ((x / PAGE_W).floor().max(0.0) as usize).min(self.pages() - 1)
    }

    /// Trae una página al medio, sin alejar más de lo que ya está (`irAPagina`).
    fn show_page(&mut self, index: usize) {
        let (_, _, vw, vh) = self.view_box();
        let fits = ((vw - 24.0) / PAGE_W).min((vh - 24.0) / PAGE_H);
        let paper_w = self.paper_w();
        self.animate_view(|paper| {
            paper.touch_view();
            if fits > 0.0 {
                paper.zoom = paper.zoom.min(fits).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            paper.pan = (paper.zoom * (paper_w / 2.0 - (index as f32 + 0.5) * PAGE_W), 0.0);
        });
    }

    /// Zoom con centro en un punto de la vista: lo que está bajo el cursor no
    /// se mueve.
    fn zoom_at(&mut self, at: (f32, f32), factor: f32) {
        self.touch_view();
        self.view_anim = None;
        let (vx, vy, vw, vh) = self.view_box();
        let rel = (at.0 - vx - vw / 2.0, at.1 - vy - vh / 2.0);
        let q = ((rel.0 - self.pan.0) / self.zoom, (rel.1 - self.pan.1) / self.zoom);
        let zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        self.pan = (rel.0 - q.0 * zoom, rel.1 - q.1 * zoom);
        self.zoom = zoom;
    }

    /// Los botones − y +: ±15 % con centro en la vista, animado.
    fn zoom_step(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.animate_view(|paper| {
            paper.touch_view();
            let zoom = (paper.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
            let k = zoom / paper.zoom;
            paper.pan = (paper.pan.0 * k, paper.pan.1 * k);
            paper.zoom = zoom;
        });
        cx.notify();
    }

    /// El porcentaje: vuelve a encuadrar todo.
    fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        self.animate_view(|paper| paper.view_touched = false);
        cx.notify();
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

    /// Recorta un marco al papel: lo arrastrado no se pierde por el borde.
    fn clamp_frame(&self, x: f32, y: f32, w: f32, h: f32) -> (f32, f32) {
        (
            x.min((self.paper_w() - w).max(0.0)).max(0.0),
            y.min((PAGE_H - h).max(0.0)).max(0.0),
        )
    }

    /// Dónde cae algo de `w × h` centrado en un punto, dentro del papel
    /// (`ubicar`).
    fn place(&self, w: f32, h: f32, (x, y): (f32, f32)) -> (f32, f32) {
        (
            (x - w / 2.0).min(self.paper_w() - w - 8.0).max(8.0),
            (y - h / 2.0).min(PAGE_H - 24.0).max(8.0),
        )
    }

    fn deselect(&mut self) {
        self.selected = None;
        self.group = None;
    }

    // --- Historial y guardado ---------------------------------------------

    /// Guarda el estado antes de un cambio, para deshacerlo.
    fn remember(&mut self) {
        self.push_undo(self.blocks.clone());
    }

    fn push_undo(&mut self, state: Vec<Block>) {
        self.undo.push(state);
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        self.stop_editing(window, cx);
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.blocks, previous));
            self.deselect();
            self.changed(cx);
        }
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        self.stop_editing(window, cx);
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.blocks, next));
            self.deselect();
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
        self.edit_before = Some(self.blocks.clone());
        self.edit_fresh = false;
        self.editing = Some(id.to_string());
        self.selected = Some(id.to_string());
        self.group = None;
        self.editor.update(cx, |editor, cx| editor.set_text(&text, cx));
        window.focus(&self.editor.focus_handle(cx));
        cx.notify();
    }

    fn stop_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing.take() else {
            return;
        };
        let before = self.edit_before.take();
        let fresh = std::mem::take(&mut self.edit_fresh);
        // Un texto que quedó vacío no deja una caja huérfana.
        let mut removed = false;
        if let Some(index) = self.index_of(&id) {
            let empty = match &self.blocks[index].body {
                Body::Text { body } => body.trim().is_empty(),
                Body::Check { items } => items.iter().all(|item| item.text.trim().is_empty()),
                _ => false,
            };
            if empty {
                self.blocks.remove(index);
                self.selected = None;
                removed = true;
                self.changed(cx);
            }
        }
        if fresh {
            // Se creó y quedó vacía: como si nunca hubiera estado. Si tiene
            // algo, el paso de crearla ya lo deshace entero.
            if removed {
                self.undo.pop();
            }
        } else if let Some(before) = before {
            let same = serde_json::to_string(&before).ok() == serde_json::to_string(&self.blocks).ok();
            if !same {
                self.push_undo(before);
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
        let measure = TextMeasure::new(cx);
        let measure = |s: &str, size: f32| measure.width(s, size);
        let paper_w = self.paper_w();
        let mut counter = self.counter;
        let block = &mut self.blocks[index];
        match &mut block.body {
            Body::Text { body } => {
                // La caja se ajusta a lo escrito (`ajustarTexto`): si el ancho
                // es el que el ajuste le habría dado al texto de antes, es
                // automático y sigue al texto; si no, lo pusiste tú y se
                // respeta. El alto siempre se ajusta. Todo dentro del papel.
                let free_w = (paper_w - block.x).max(80.0);
                let free_h = (PAGE_H - block.y).max(TEXT_MIN_H);
                let auto = auto_size(body, &measure).0.min(free_w);
                let manual = (block.w - auto).abs() > 1.0;
                *body = text;
                if !manual {
                    block.w = auto_size(body, &measure).0.min(free_w);
                }
                let shown = if body.is_empty() { PLACEHOLDER } else { body.as_str() };
                block.h = height_for_width(shown, block.w, &measure).min(free_h);
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
                block.h = block
                    .h
                    .max(items.len() as f32 * ROW_H + PAD_Y * 2.0)
                    .min((PAGE_H - block.y).max(block.h));
            }
            _ => {}
        }
        self.counter = counter;
        self.changed(cx);
    }

    // --- Altas y bajas ----------------------------------------------------

    fn add_block_at(
        &mut self,
        body: Body,
        (x, y): (f32, f32),
        w: f32,
        h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_editing(window, cx);
        self.remember();
        let id = new_id(&mut self.counter);
        let editable = matches!(body, Body::Text { .. } | Body::Check { .. });
        self.blocks.push(Block {
            id: id.clone(),
            x,
            y,
            w,
            h,
            body,
        });
        self.selected = Some(id.clone());
        self.group = None;
        self.set_tool_quiet(Tool::Select);
        self.changed(cx);
        // Al crear se escribe de inmediato, como en Atic.
        if editable {
            self.start_editing(&id, window, cx);
            self.edit_fresh = true;
        }
    }

    /// Una caja de texto centrada en un punto, con la medida de lo que trae
    /// (`insertarTexto`).
    fn insert_text(&mut self, text: String, at: (f32, f32), window: &mut Window, cx: &mut Context<Self>) {
        let measure = TextMeasure::new(cx);
        let measure = |s: &str, size: f32| measure.width(s, size);
        let w = auto_size(&text, &measure).0.min(self.paper_w() - 16.0);
        let shown = if text.is_empty() { PLACEHOLDER } else { text.as_str() };
        let h = height_for_width(shown, w, &measure).min(PAGE_H - 32.0);
        let at = self.place(w, h, at);
        self.add_block_at(Body::Text { body: text }, at, w, h, window, cx);
    }

    fn insert_list(&mut self, at: (f32, f32), window: &mut Window, cx: &mut Context<Self>) {
        let id = new_id(&mut self.counter);
        let at = self.place(LIST_W, LIST_H, at);
        self.add_block_at(
            Body::Check {
                items: vec![Item {
                    id,
                    text: String::new(),
                    done: false,
                }],
            },
            at,
            LIST_W,
            LIST_H,
            window,
            cx,
        );
    }

    fn insert_image(&mut self, asset: String, width: u32, height: u32, window: &mut Window, cx: &mut Context<Self>) {
        let (w, h) = image_size(width, height);
        let at = self.place(w, h, self.visible_center());
        self.add_block_at(Body::Image { asset, width, height }, at, w, h, window, cx);
    }

    fn new_text(&mut self, _: &NewText, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        if self.editing.is_none() {
            self.insert_text(String::new(), self.visible_center(), window, cx);
        }
    }

    fn new_list(&mut self, _: &NewList, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        if self.editing.is_none() {
            self.insert_list(self.visible_center(), window, cx);
        }
    }

    /// Pega lo copiado: imagen como imagen, texto como caja.
    fn paste_block(&mut self, _: &PasteBlock, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        if self.editing.is_some() {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for entry in item.entries() {
            if let ClipboardEntry::Image(image) = entry {
                match store_image(&image.bytes) {
                    Some((asset, width, height)) => self.insert_image(asset, width, height, window, cx),
                    None => self.image_failed(cx),
                }
                return;
            }
        }
        if let Some(text) = item.text().filter(|t| !t.trim().is_empty()) {
            self.insert_text(text.trim().to_string(), self.visible_center(), window, cx);
        }
    }

    /// Una imagen de un archivo, copiada a los `assets` de la nota como PNG,
    /// que es de donde las lee Atic.
    fn add_image_file(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        match store_image(&bytes) {
            Some((asset, width, height)) => self.insert_image(asset, width, height, window, cx),
            None => self.image_failed(cx),
        }
    }

    /// Una imagen que no se pudo leer o que pesa más de lo que guarda Atic.
    fn image_failed(&mut self, cx: &mut Context<Self>) {
        self.notice = Some(("No se pudo insertar la imagen (formato o tamaño).".into(), None));
        cx.notify();
    }

    fn delete_selected(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        if self.editing.is_some() {
            return;
        }
        if self.group.is_some() {
            self.ask_delete_group(cx);
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

    /// Pide confirmar el borrado de lo elegido: dice cuánto y si es todo.
    fn ask_delete_group(&mut self, cx: &mut Context<Self>) {
        let Some(group) = &self.group else {
            return;
        };
        let count = group.len();
        if count == 0 {
            return;
        }
        let all = count == whole_board(&self.blocks).len();
        self.confirm = Some(Confirm::Group { count, all });
        cx.notify();
    }

    fn delete_group(&mut self, cx: &mut Context<Self>) {
        let Some(group) = self.group.take() else {
            return;
        };
        self.remember();
        self.blocks.retain(|b| !group.ids.contains(&b.id));
        if let Some(Body::Ink { strokes, .. }) = self.blocks.iter_mut().find(|b| b.is_ink()).map(|b| &mut b.body) {
            let mut i = 0;
            strokes.retain(|_| {
                let keep = !group.strokes.contains(&i);
                i += 1;
                keep
            });
        }
        self.selected = None;
        // Si no quedó nada, el tablero vuelve a una página.
        if whole_board(&self.blocks).len() == 0 && self.paper_pages > 1 {
            self.paper_pages = 1;
            self.animate_view(|paper| paper.view_touched = false);
        }
        self.changed(cx);
    }

    /// La ✕ de una miniatura. Con contenido pregunta antes.
    fn ask_remove_page(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        let (objects, strokes) = page_content(&self.blocks, index);
        if objects + strokes > 0 {
            self.confirm = Some(Confirm::Page { index, objects, strokes });
            cx.notify();
            return;
        }
        self.remove_page(index, cx);
    }

    /// Quita la página y lo que tiene; lo de la derecha se corre. Si es la
    /// única, queda vacía. Se deshace con Ctrl+Z.
    fn remove_page(&mut self, index: usize, cx: &mut Context<Self>) {
        let pages = self.pages();
        self.remember();
        self.blocks = without_page(&self.blocks, index);
        self.deselect();
        if pages > 1 {
            self.touch_view();
            self.paper_pages = pages - 1;
            // El papel se achica centrado: se compensa para que lo que se
            // miraba no salte.
            self.pan.0 -= PAGE_W / 2.0 * self.zoom;
            self.flash = Some((index.min(pages - 2) as f32 * PAGE_W, Instant::now()));
        }
        self.changed(cx);
    }

    /// Agrega una página a la derecha y lleva la vista a ella (`agregarYVer`).
    fn add_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        let pages = self.pages();
        if pages >= MAX_PAGES {
            return;
        }
        self.touch_view();
        self.view_anim = None;
        self.paper_pages = pages + 1;
        // El papel crece centrado: lo que se miraba queda donde estaba.
        self.pan.0 += PAGE_W / 2.0 * self.zoom;
        self.flash = Some((pages as f32 * PAGE_W, Instant::now()));
        self.show_page(pages);
        cx.notify();
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() {
            return;
        }
        // Esc cierra lo que esté encima antes de voltear la tarjeta.
        if self.confirm.take().is_some() || std::mem::take(&mut self.export_menu) || std::mem::take(&mut self.palette) {
            cx.notify();
        } else if self.editing.is_some() {
            self.stop_editing(window, cx);
        } else if self.selected.is_some() || self.group.is_some() || self.tool != Tool::Select {
            self.deselect();
            self.set_tool_quiet(Tool::Select);
            cx.notify();
        } else {
            cx.emit(PaperEvent::Close);
        }
    }

    // --- Teclado ----------------------------------------------------------

    /// Las teclas de una letra (`V M P H E T L`), Espacio, Ctrl+A, las flechas
    /// y Enter. Mientras se escribe en una caja no se tocan.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() || self.confirm.is_some() || self.export_page.is_some() {
            return;
        }
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        let mods = &keystroke.modifiers;
        if key == "space" && !mods.control && !mods.alt {
            if !self.space {
                self.space = true;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if mods.control && key == "a" {
            if whole_board(&self.blocks).len() > 0 {
                self.set_tool_quiet(Tool::Select);
                self.selected = None;
                self.group = Some(whole_board(&self.blocks));
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if mods.control || mods.alt || mods.platform {
            return;
        }
        let arrow = match key {
            "left" => Some((-1.0, 0.0)),
            "right" => Some((1.0, 0.0)),
            "up" => Some((0.0, -1.0)),
            "down" => Some((0.0, 1.0)),
            _ => None,
        };
        if let Some((dx, dy)) = arrow {
            self.nudge(dx, dy, mods.shift, cx);
            cx.stop_propagation();
            return;
        }
        match key {
            "v" => self.set_tool(Tool::Select, window, cx),
            "m" => self.set_tool(Tool::Hand, window, cx),
            // Con la tecla, los lápices abren su paleta y el borrador no.
            "p" | "d" => {
                self.set_tool(Tool::Pen, window, cx);
                self.palette = true;
            }
            "h" => {
                self.set_tool(Tool::Highlight, window, cx);
                self.palette = true;
            }
            "e" => {
                self.set_tool(Tool::Eraser, window, cx);
                self.palette = false;
            }
            "t" => self.set_tool(Tool::Text, window, cx),
            "l" => self.set_tool(Tool::List, window, cx),
            "[" | "]" if self.tool == Tool::Eraser => {
                self.step_eraser(if key == "]" { 1 } else { -1 }, cx)
            }
            "enter" => match self.selected.clone() {
                Some(id) => self.start_editing(&id, window, cx),
                None => return,
            },
            _ => return,
        }
        cx.stop_propagation();
    }

    fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" && self.space {
            self.space = false;
            cx.notify();
        }
    }

    /// Las flechas mueven lo elegido 8 px; con Shift, 1.
    fn nudge(&mut self, dx: f32, dy: f32, fine: bool, cx: &mut Context<Self>) {
        let Some(index) = self.selected.as_deref().and_then(|id| self.index_of(id)) else {
            return;
        };
        let step = if fine { 1.0 } else { 8.0 };
        let b = &self.blocks[index];
        let (x, y) = self.clamp_frame(b.x + dx * step, b.y + dy * step, b.w, b.h);
        self.remember();
        let b = &mut self.blocks[index];
        (b.x, b.y) = (x, y);
        self.changed(cx);
    }

    fn step_eraser(&mut self, step: i32, cx: &mut Context<Self>) {
        let i = ERASER_RADII.iter().position(|r| *r == self.eraser_radius).unwrap_or(1) as i32;
        self.eraser_radius = ERASER_RADII[(i + step).clamp(0, 2) as usize];
        cx.notify();
    }

    // --- Ratón ------------------------------------------------------------

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_some() || self.confirm.is_some() {
            return;
        }
        let local = self.local(event.position);
        let (vx, vy, vw, vh) = self.view_box();
        if local.0 < vx || local.0 > vx + vw || local.1 < vy || local.1 > vy + vh {
            return;
        }
        let delta = event.delta.pixel_delta(px(24.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control {
            // Rueda: 8 % por muesca, como Atic; el pellizco del touchpad
            // llega en píxeles y va proporcional.
            let factor = if event.delta.precise() {
                (1.0 + dy * 0.004).clamp(0.8, 1.25)
            } else if dy > 0.0 {
                1.08
            } else if dy < 0.0 {
                0.92
            } else {
                return;
            };
            self.zoom_at(local, factor);
        } else {
            self.touch_view();
            self.view_anim = None;
            let (dx, dy) = if event.modifiers.shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
            self.pan.0 += dx;
            self.pan.1 += dy;
        }
        cx.notify();
    }

    fn start_pan(&mut self, start: Point<Pixels>) {
        self.touch_view();
        self.view_anim = None;
        self.drag = Some(Drag::Pan {
            start,
            from: self.pan,
        });
    }

    fn on_middle_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.export_page.is_none() && self.confirm.is_none() && self.drag.is_none() {
            self.start_pan(event.position);
            cx.notify();
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        // Mientras se captura una página o se pregunta algo, no hay nada que tocar.
        if self.export_page.is_some() || self.confirm.is_some() {
            return;
        }
        // La barra de arriba atiende sus propios clics (el botón Exportar
        // abre y cierra su menú).
        if local.1 < TOOLBAR_H {
            return;
        }
        // El menú de exportar: un clic dentro lo atiende su fila; fuera lo cierra.
        if self.export_menu {
            self.export_menu = false;
            cx.notify();
            return;
        }
        // El cajón y la tira de páginas.
        if self.drawer && local.0 >= self.size.0 - DRAWER_W {
            return;
        }
        if local.1 >= self.size.1 - self.strip_h() {
            return;
        }
        // Un clic fuera de la paleta la cierra.
        self.palette = false;
        let paper = self.to_paper(event.position);
        if self.space || self.tool == Tool::Hand {
            self.start_pan(event.position);
            cx.notify();
            return;
        }
        match self.tool {
            Tool::Pen | Tool::Highlight => {
                self.stop_editing(window, cx);
                self.deselect();
                let p = self.clamp_point(paper);
                self.live_stroke = Some(vec![[p.0, p.1]]);
                self.drag = Some(Drag::Ink);
                cx.notify();
            }
            Tool::Eraser => {
                self.stop_editing(window, cx);
                self.drag = Some(Drag::Erase {
                    start: event.position,
                    last: paper,
                    moved: false,
                    registered: false,
                });
                // Redibujar ya: el seguimiento del arrastre se arma al dibujar.
                cx.notify();
            }
            // Texto y lista se ponen donde se hace clic, en el papel vacío.
            Tool::Text => {
                if self.hit(paper).is_none() {
                    self.insert_text(String::new(), paper, window, cx);
                }
            }
            Tool::List => {
                if self.hit(paper).is_none() {
                    self.insert_list(paper, window, cx);
                }
            }
            Tool::Select | Tool::Hand => self.press_select(event, paper, window, cx),
        }
    }

    fn clamp_point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (x.clamp(0.0, self.paper_w()), y.clamp(0.0, PAGE_H))
    }

    fn press_select(
        &mut self,
        event: &MouseDownEvent,
        paper: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scale = self.layout().scale;
        self.group = None;
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
                self.drag = Some(Drag::Resize {
                    id,
                    from: size_now,
                    start: event.position,
                    registered: false,
                });
                cx.notify();
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
                // Dejar de editar puede quitar una caja vacía y correr los
                // índices: se vuelve a buscar por id.
                self.stop_editing(window, cx);
                let Some(index) = self.index_of(&id) else {
                    return;
                };
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
                // Papel vacío: arrastrar dibuja un recuadro que elige lo que
                // toca; un clic limpio suelta lo elegido.
                self.stop_editing(window, cx);
                window.focus(&self.focus);
                self.drag = Some(Drag::Marquee {
                    start: event.position,
                    from: paper,
                    moved: false,
                });
                cx.notify();
            }
        }
    }

    /// El cursor sobre la vista sin nada apretado: el alcance del borrador.
    /// Los arrastres van por `drag_move`, que los sigue en cualquier parte.
    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.tool == Tool::Eraser && self.export_page.is_none() {
            self.eraser_at = Some(self.to_paper(event.position));
            cx.notify();
        }
    }

    /// Un arrastre en curso. Se escucha en toda la ventana mientras dura (como
    /// `setPointerCapture` en Atic): pasar sobre el dock, la tira o fuera de
    /// la tarjeta no lo corta.
    fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        perf::moved();
        let paper = self.to_paper(event.position);
        if self.tool == Tool::Eraser {
            self.eraser_at = Some(paper);
        }
        if !matches!(event.pressed_button, Some(MouseButton::Left) | Some(MouseButton::Middle)) {
            return;
        }
        let scale = self.layout().scale;
        let moved_from = |start: Point<Pixels>| {
            (f32::from(event.position.x) - f32::from(start.x))
                .hypot(f32::from(event.position.y) - f32::from(start.y))
        };
        match self.drag.take() {
            Some(Drag::Pan { start, from }) => {
                self.pan = (
                    from.0 + f32::from(event.position.x) - f32::from(start.x),
                    from.1 + f32::from(event.position.y) - f32::from(start.y),
                );
                self.drag = Some(Drag::Pan { start, from });
                cx.notify();
            }
            Some(Drag::Marquee { start, from, moved }) => {
                let moved = moved || moved_from(start) > DRAG_MIN;
                if moved {
                    let to = self.clamp_point(paper);
                    self.marquee = Some((
                        from.0.min(to.0),
                        from.1.min(to.1),
                        (to.0 - from.0).abs(),
                        (to.1 - from.1).abs(),
                    ));
                    cx.notify();
                }
                self.drag = Some(Drag::Marquee { start, from, moved });
            }
            Some(Drag::Ink) => {
                let p = self.clamp_point(paper);
                if let Some(stroke) = self.live_stroke.as_mut() {
                    let last = stroke.last().copied().unwrap_or([p.0, p.1]);
                    if (p.0 - last[0]).hypot(p.1 - last[1]) >= INK_MIN_STEP {
                        stroke.push([p.0, p.1]);
                    }
                }
                self.drag = Some(Drag::Ink);
                cx.notify();
            }
            Some(Drag::Erase { start, last, moved, registered }) => {
                // Sin moverse de verdad es un clic: el temblor no parte líneas.
                if !moved && moved_from(start) <= 5.0 {
                    self.drag = Some(Drag::Erase { start, last, moved, registered });
                    return;
                }
                // El paso de deshacer, en el primer borrado de verdad.
                let before = (!registered).then(|| self.blocks.clone());
                let erased = self.erase_along(last, paper, cx);
                if let (true, Some(before)) = (erased, before) {
                    self.push_undo(before);
                }
                self.drag = Some(Drag::Erase {
                    start,
                    last: paper,
                    moved: true,
                    registered: registered || erased,
                });
            }
            Some(Drag::Press { id, start, was_selected, click }) => {
                if moved_from(start) < DRAG_MIN {
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
            Some(Drag::Resize { id, from, start, registered }) => {
                if !registered {
                    self.remember();
                }
                if let Some(index) = self.index_of(&id) {
                    let dx = (f32::from(event.position.x) - f32::from(start.x)) / scale;
                    let dy = (f32::from(event.position.y) - f32::from(start.y)) / scale;
                    let paper_w = self.paper_w();
                    let block = &mut self.blocks[index];
                    // Tampoco se escapa del papel; una imagen guarda su forma.
                    let max_w = (paper_w - block.x).max(MIN_W);
                    let max_h = (PAGE_H - block.y).max(MIN_H);
                    block.w = (from.0 + dx).clamp(MIN_W, max_w);
                    block.h = if matches!(block.body, Body::Image { .. }) {
                        (block.w * from.1 / from.0.max(1.0)).clamp(MIN_H, max_h)
                    } else {
                        (from.1 + dy).clamp(MIN_H, max_h)
                    };
                }
                self.drag = Some(Drag::Resize { id, from, start, registered: true });
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
        let b = &self.blocks[index];
        let (x, y) = self.clamp_frame(from.0 + dx, from.1 + dy, b.w, b.h);
        let block = &mut self.blocks[index];
        (block.x, block.y) = (x, y);
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
            Some(Drag::Marquee { moved, .. }) => {
                let rect = self.marquee.take();
                self.deselect();
                if let (true, Some(rect)) = (moved, rect) {
                    let group = in_marquee(&self.blocks, rect);
                    // Uno solo se elige como siempre, con su asa.
                    if group.ids.len() == 1 && group.strokes.is_empty() {
                        self.selected = group.ids.first().cloned();
                    } else if group.len() > 0 {
                        self.group = Some(group);
                    }
                }
                cx.notify();
            }
            Some(Drag::Erase { last, moved: false, .. }) => {
                // Clic limpio: se borra la línea entera más cercana.
                let radius = self.eraser_radius;
                let ink = self.blocks.iter().position(Block::is_ink);
                let nearest = ink.and_then(|i| match &self.blocks[i].body {
                    Body::Ink { strokes, .. } => nearest_stroke(strokes, last, radius).map(|s| (i, s)),
                    _ => None,
                });
                if let Some((i, s)) = nearest {
                    self.remember();
                    if let Body::Ink { strokes, .. } = &mut self.blocks[i].body {
                        strokes.remove(s);
                    }
                    self.changed(cx);
                }
            }
            Some(Drag::Pan { .. })
            | Some(Drag::Erase { .. })
            | Some(Drag::Move { .. })
            | Some(Drag::Resize { .. })
            | None => {}
        }
        cx.notify();
    }

    fn on_middle_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.drag, Some(Drag::Pan { .. })) {
            self.drag = None;
            cx.notify();
        }
    }

    /// Guarda el trazo que se acaba de dibujar en el bloque de tinta. Un clic
    /// sin arrastre no deja nada.
    fn finish_stroke(&mut self, cx: &mut Context<Self>) {
        let Some(points) = self.live_stroke.take() else {
            return;
        };
        if points.len() < 2 {
            cx.notify();
            return;
        }
        self.remember();
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
        self.blocks.push(Block {
            id,
            x: 0.0,
            y: 0.0,
            w: self.paper_w(),
            h: PAGE_H,
            body: Body::Ink {
                strokes: Vec::new(),
                height: 0,
            },
        });
        self.blocks.len() - 1
    }

    /// Borra por el tramo recorrido desde el último punto, en pasos de medio
    /// radio: un movimiento rápido no deja huecos sin borrar.
    fn erase_along(&mut self, from: (f32, f32), to: (f32, f32), cx: &mut Context<Self>) -> bool {
        let radius = self.eraser_radius;
        let steps = ((to.0 - from.0).hypot(to.1 - from.1) / (radius / 2.0)).ceil().max(1.0) as usize;
        let mut changed = false;
        for block in &mut self.blocks {
            if let Body::Ink { strokes, .. } = &mut block.body {
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let at = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
                    changed |= erase_strokes(strokes, at, radius);
                }
            }
        }
        if changed {
            self.changed(cx);
        }
        changed
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

    /// Cambia de herramienta sin tocar el foco ni la paleta.
    fn set_tool_quiet(&mut self, tool: Tool) {
        self.tool = tool;
        self.dock_pill.set(dock_x(tool.index()), Instant::now());
        if tool != Tool::Eraser {
            self.eraser_at = None;
        }
    }

    /// Elegir una herramienta del dock o con su tecla. Elegir el lápiz abre
    /// sus colores; elegirlo de nuevo los cierra.
    fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(window, cx);
        self.palette = tool.has_palette() && !(self.tool == tool && self.palette);
        self.set_tool_quiet(tool);
        if tool != Tool::Select {
            self.deselect();
        }
        cx.notify();
    }
}

/// Espacio apretado ahora mismo, aunque la ventana no tenga el foco.
fn space_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SPACE};
    unsafe { GetAsyncKeyState(VK_SPACE.0 as i32) < 0 }
}

/// Guarda una imagen (de cualquier formato que lea `image`) en los `assets`
/// del tablero como PNG, igual que Atic: un solo formato y el mismo tope de
/// tamaño. Devuelve el nombre y su medida.
fn store_image(bytes: &[u8]) -> Option<(String, u32, u32)> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    if png.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let dir = board_dir()?.join("assets");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("img-{}.png", uuid_like());
    std::fs::write(dir.join(&name), png).ok()?;
    Some((name, width, height))
}

/// Un nombre único sin traer `uuid`: tiempo en nanosegundos y un contador.
fn uuid_like() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:x}-{n:x}")
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
#[cfg(test)]
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let read = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some((read(16), read(20)))
}

// --- Dibujo ----------------------------------------------------------------

impl PaperView {
    /// Botón de ícono de la barra de arriba (`rb-btn-ghost ico`).
    fn icon_button(
        &self,
        id: &'static str,
        icon: &'static str,
        label: Option<&'static str>,
        tip_text: &'static str,
        enabled: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (text, muted) = (self.colors.text, self.colors.muted);
        div()
            .id(id)
            .h(px(30.))
            .min_w(px(30.))
            .px(px(if label.is_some() { 10. } else { 0. }))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded(px(15.))
            .text_size(px(12.))
            .tooltip(tip(tip_text))
            .when(enabled, |el| {
                el.cursor_pointer()
                    .on_click(cx.listener(move |paper, _, window, cx| on_click(paper, window, cx)))
            })
            .when(!enabled, |el| el.opacity(0.35))
            .child(svg().path(icon).size(px(15.)).text_color(muted))
            .when_some(label, |el, label| el.child(label))
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                el.bg(h.mix(text.opacity(0.0), text.opacity(0.08 + 0.05 * h.press)))
                    .text_color(h.mix(muted, text))
            })
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let status = match (&self.notice, self.saved) {
            (Some((text, _)), _) => text.clone(),
            (None, Some(true)) => "Guardado".to_string(),
            (None, Some(false)) => "No se pudo guardar".to_string(),
            (None, None) if self.dirty => "Guardando…".to_string(),
            _ => String::new(),
        };
        let shown_file = self.notice.as_ref().and_then(|(_, file)| file.clone());
        let separator = || div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(colors.line);
        let percent = format!("{}%", (self.view().0 * 100.0).round());
        let drawer_icon = if self.drawer {
            "icons/board/panel-right-close.svg"
        } else {
            "icons/board/panel-right-open.svg"
        };
        div()
            .h(px(TOOLBAR_H))
            .w_full()
            .flex_none()
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                div()
                    .id("status")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .pl(px(4.))
                    .text_size(px(11.))
                    .text_color(colors.faint)
                    .when_some(shown_file, |el, file| {
                        el.cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, _| show_in_folder(&file)))
                    })
                    .child(status)
                    .fx("status-fx", {
                        let (faint, text) = (colors.faint, colors.text);
                        move |el, h| el.text_color(h.mix(faint, text))
                    }),
            )
            .child(self.icon_button("undo", "icons/undo.svg", None, "Deshacer (Ctrl+Z)",
                !self.undo.is_empty(), |paper, window, cx| paper.undo(&Undo, window, cx), cx))
            .child(self.icon_button("redo", "icons/redo.svg", None, "Rehacer (Ctrl+Shift+Z)",
                !self.redo.is_empty(), |paper, window, cx| paper.redo(&Redo, window, cx), cx))
            .child(separator())
            .child(self.icon_button("zoom-out", "icons/board/minus.svg", None, "Alejar",
                true, |paper, _, cx| paper.zoom_step(1.0 / 1.15, cx), cx))
            .child(
                div()
                    .id("zoom-reset")
                    .h(px(30.))
                    .min_w(px(52.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(15.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .tooltip(tip("Restablecer vista"))
                    .on_click(cx.listener(|paper, _, _, cx| paper.zoom_reset(cx)))
                    .child(percent)
                    .fx("zoom-reset-fx", {
                        let (text, muted) = (colors.text, colors.muted);
                        move |el, h| {
                            el.bg(h.mix(text.opacity(0.0), text.opacity(0.08)))
                                .text_color(h.mix(muted, text))
                        }
                    }),
            )
            .child(self.icon_button("zoom-in", "icons/plus.svg", None, "Acercar",
                true, |paper, _, cx| paper.zoom_step(1.15, cx), cx))
            .child(separator())
            .child(self.icon_button("paste", "icons/board/clipboard-paste.svg", None, "Pegar (Ctrl+V)",
                true, |paper, window, cx| paper.paste_block(&PasteBlock, window, cx), cx))
            .child(self.icon_button("export", "icons/board/download.svg", Some("Exportar"), "Exportar",
                !self.exporting, |paper, _, cx| {
                    paper.export_menu = !paper.export_menu;
                    cx.notify();
                }, cx))
            .child(self.icon_button("drawer", drawer_icon, Some("Insertar"), "Insertar",
                true, |paper, _, cx| paper.toggle_drawer(cx), cx))
            .child(self.icon_button("close", "icons/board/arrow-left.svg", Some("Volver"), "Volver (Esc)",
                true, |_, _, cx| cx.emit(PaperEvent::Close), cx))
    }

    /// El dock: herramientas en una píldora flotante abajo al centro, con la
    /// activa marcada por una píldora que se desliza (`.dock`, `.pastilla`).
    fn render_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let (text, muted, card) = (colors.text, colors.muted, colors.card);
        let now = Instant::now();
        let ink: Hsla = rgb(PALETTE[self.color]).into();
        let sep = || div().flex_none().w(px(1.)).h(px(20.)).mx(px(4.)).bg(colors.line);
        let mut dock = div()
            .id("dock")
            .occlude()
            .relative()
            .flex()
            .items_center()
            .gap(px(DOCK_GAP))
            .p(px(DOCK_PAD))
            .rounded(px(999.))
            .bg(card.opacity(0.92))
            .border_1()
            .border_color(colors.line)
            .shadow(vec![
                shadow(0.0, 8.0, 24.0, 0.22),
                shadow(0.0, 1.0, 2.0, 0.12),
            ])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .absolute()
                    .top(px(DOCK_PAD))
                    .left(px(self.dock_pill.value(now)))
                    .size(px(DOCK_BTN))
                    .rounded(px(999.))
                    .bg(text.opacity(0.10)),
            );
        for (i, (tool, icon, label)) in Tool::DOCK.iter().copied().enumerate() {
            if i == 2 || i == 5 {
                dock = dock.child(sep());
            }
            let active = self.tool == tool;
            dock = dock.child(
                div()
                    .id(("dock-tool", i))
                    .relative()
                    .size(px(DOCK_BTN))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(999.))
                    .cursor_pointer()
                    .tooltip(tip(label))
                    .on_click(cx.listener(move |paper, _, window, cx| paper.set_tool(tool, window, cx)))
                    .child(
                        div()
                            .relative()
                            .child(svg().path(icon).size(px(17.)).text_color(if active { text } else { muted }))
                            // El color del lápiz, en un punto sobre el ícono.
                            .when(tool.inks(), |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .right(px(-3.))
                                        .bottom(px(-3.))
                                        .size(px(7.))
                                        .rounded(px(4.))
                                        .bg(ink)
                                        .border_1()
                                        .border_color(card),
                                )
                            }),
                    )
                    .fx(("dock-tool-fx", i), move |el, h| {
                        el.bg(h.mix(text.opacity(0.0), text.opacity(if active { 0.0 } else { 0.06 })))
                    }),
            );
        }
        let has_selection = self.selected.is_some() || self.group.is_some();
        if has_selection {
            let danger = colors.danger;
            dock = dock.child(sep()).child(
                div()
                    .id("dock-delete")
                    .size(px(DOCK_BTN))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(999.))
                    .cursor_pointer()
                    .tooltip(tip(if self.group.is_some() { "Borrar lo elegido" } else { "Quitar" }))
                    .on_click(cx.listener(|paper, _, window, cx| paper.delete_selected(&DeleteSelected, window, cx)))
                    .child(svg().path("icons/trash.svg").size(px(17.)).text_color(danger))
                    .fx("dock-delete-fx", move |el, h| el.bg(h.mix(danger.opacity(0.0), danger.opacity(0.12)))),
            );
        }
        // La paleta abre hacia arriba, sobre la herramienta.
        let palette = (self.palette && self.tool.has_palette()).then(|| {
            let left = dock_x(self.tool.index());
            let content: AnyElement = if self.tool == Tool::Eraser {
                div()
                    .flex()
                    .gap(px(4.))
                    .children(ERASER_RADII.iter().enumerate().map(|(i, &radius)| {
                        let chosen = self.eraser_radius == radius;
                        let d = 6.0 + i as f32 * 6.0;
                        div()
                            .id(("eraser-size", i))
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(14.))
                            .cursor_pointer()
                            .when(chosen, |el| el.bg(text.opacity(0.14)))
                            .tooltip(tip(["Borrador chico ([ ])", "Borrador mediano ([ ])", "Borrador grande ([ ])"][i]))
                            .on_click(cx.listener(move |paper, _, _, cx| {
                                paper.eraser_radius = radius;
                                paper.palette = false;
                                cx.notify();
                            }))
                            .child(div().size(px(d)).rounded(px(d / 2.0)).bg(if chosen { text } else { muted }))
                    }))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .max_w(px(232. - 16.))
                    .children((0..PALETTE.len()).map(|i| self.swatch(i, cx)))
                    .into_any_element()
            };
            div()
                .id("palette")
                .occlude()
                .absolute()
                .bottom(px(DOCK_BTN + DOCK_PAD * 2.0 + 12.0))
                .left(px(left - 8.0))
                .p(px(8.))
                .rounded(px(14.))
                .bg(card)
                .border_1()
                .border_color(colors.line)
                .shadow(vec![shadow(0.0, 8.0, 24.0, 0.22)])
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(content)
        });
        let (_, vy, vw, vh) = self.view_box();
        div()
            .absolute()
            .left(px(0.))
            .w(px(vw))
            .top(px(vy + vh - DOCK_BTN - DOCK_PAD * 2.0 - 2.0 - 14.0))
            .flex()
            .justify_center()
            .child(div().relative().child(dock).children(palette))
    }

    fn swatch(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let color: Hsla = rgb(PALETTE[index]).into();
        let chosen = self.color == index;
        let (text, card) = (self.colors.text, self.colors.card);
        // Anillo doble al elegir: uno del color de la tarjeta y otro del texto.
        div()
            .id(("swatch", index))
            .size(px(24.))
            .flex_none()
            .rounded(px(12.))
            .p(px(2.))
            .border_2()
            .border_color(if chosen { text } else { text.opacity(0.0) })
            .cursor_pointer()
            .on_click(cx.listener(move |paper, _, _, cx| {
                paper.color = index;
                paper.palette = false;
                cx.notify();
            }))
            .child(div().size_full().rounded(px(10.)).bg(color))
            .fx(("swatch-fx", index), move |el, h| {
                if chosen {
                    el
                } else {
                    el.border_color(text.opacity(0.35 * h.t)).bg(card)
                }
            })
    }

    /// La tira de páginas: una miniatura por página con su contenido y el «+»
    /// al final (`.tira`).
    fn render_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let (text, muted, line, accent, card) = (colors.text, colors.muted, colors.line, colors.accent, colors.card);
        let mini_w = self.mini_w();
        let mini_h = mini_w * 0.75;
        let pages = self.pages();
        let current = self.current_page();
        let several = pages > 1 || whole_board(&self.blocks).len() > 0;
        let items = minis(&self.blocks, pages).into_iter().enumerate().map(|(i, mini)| {
            let active = i == current;
            let show_x = several && self.hovered_mini == Some(i);
            let ink = mini.ink;
            let sheet = colors.sheet;
            let thumb = div()
                .id(("mini", i))
                .relative()
                .w(px(mini_w))
                .h(px(mini_h))
                .overflow_hidden()
                .rounded(px(4.))
                .bg(sheet)
                .border_1()
                .border_color(if active { accent } else { line })
                .cursor_pointer()
                .tooltip(tip(PAGE_NAMES[i.min(PAGE_NAMES.len() - 1)]))
                .on_click(cx.listener(move |paper, _, _, cx| {
                    paper.show_page(i);
                    cx.notify();
                }))
                .children(mini.pieces.into_iter().map(move |piece| {
                    let frame = div()
                        .absolute()
                        .left(px(piece.x * mini_w))
                        .top(px(piece.y * mini_h))
                        .w(px((piece.w * mini_w).max(2.0)))
                        .h(px((piece.h * mini_h).max(2.0)));
                    match piece.kind {
                        MiniKind::Image(path) => frame
                            .child(img(path).size_full().object_fit(ObjectFit::Fill))
                            .into_any_element(),
                        MiniKind::Text(rows) | MiniKind::List(rows) => frame
                            .p(px(2.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .overflow_hidden()
                            .children((0..rows).map(move |r| {
                                div()
                                    .flex_none()
                                    .h(px(2.))
                                    .rounded(px(1.))
                                    .bg(text.opacity(0.38))
                                    .when(r + 1 == rows && rows > 1, |el| el.w(gpui::relative(0.55)))
                            }))
                            .into_any_element(),
                    }
                }))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            for (points, width, color) in &ink {
                                let screen: Vec<Point<Pixels>> = points
                                    .iter()
                                    .map(|(x, y)| {
                                        point(
                                            bounds.origin.x + px(x * mini_w),
                                            bounds.origin.y + px(y * mini_h),
                                        )
                                    })
                                    .collect();
                                paint_polyline(window, &screen, (width * mini_w / PAGE_W).max(0.6), *color);
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(3.))
                        .bottom(px(1.))
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child(format!("{}", i + 1)),
                )
                .on_hover(cx.listener(move |paper, hovered: &bool, _, cx| {
                    if *hovered {
                        paper.hovered_mini = Some(i);
                    } else if paper.hovered_mini == Some(i) {
                        paper.hovered_mini = None;
                    }
                    cx.notify();
                }))
                .fx(("mini-fx", i), move |el, h| el.mt(px(-2.0 * h.t)).mb(px(2.0 * h.t)));
            div().relative().flex_none().child(thumb).when(show_x, |el| {
                el.child(
                    div()
                        .id(("mini-x", i))
                        .absolute()
                        .top(px(-5.))
                        .right(px(-5.))
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .bg(card)
                        .border_1()
                        .border_color(line)
                        .cursor_pointer()
                        .tooltip(tip(if pages > 1 { "Eliminar página" } else { "Vaciar página" }))
                        .on_hover(cx.listener(move |paper, hovered: &bool, _, cx| {
                            if *hovered {
                                paper.hovered_mini = Some(i);
                            } else if paper.hovered_mini == Some(i) {
                                paper.hovered_mini = None;
                            }
                            cx.notify();
                        }))
                        .on_click(cx.listener(move |paper, _, window, cx| paper.ask_remove_page(i, window, cx)))
                        .child(svg().path("icons/x.svg").size(px(10.)).text_color(muted)),
                )
            })
        }).collect::<Vec<_>>();
        div()
            .id("strip")
            .occlude()
            .absolute()
            .left(px(0.))
            .w(px(self.size.0 - self.drawer_width()))
            .bottom(px(0.))
            .h(px(self.strip_h()))
            .bg(colors.bg)
            .pt(px(STRIP_PAD_TOP))
            .pb(px(STRIP_PAD_BOTTOM))
            .flex()
            .justify_center()
            .items_center()
            .gap(px(6.))
            .children(items)
            .when(pages < MAX_PAGES, |el| {
                el.child(
                    div()
                        .id("mini-add")
                        .w(px(mini_w))
                        .h(px(mini_h))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_dashed()
                        .cursor_pointer()
                        .tooltip(tip("Añadir página"))
                        .on_click(cx.listener(|paper, _, window, cx| paper.add_page(window, cx)))
                        .child(svg().path("icons/plus.svg").size(px(15.)).text_color(muted))
                        .fx("mini-add-fx", move |el, h| el.border_color(h.mix(line, text))),
                )
            })
    }

    /// El «¿seguro?» de borrar lo elegido o una página (`ConfirmDialog`).
    fn render_confirm(&self, confirm: Confirm, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        let pages = self.pages();
        let (title, body, action) = match confirm {
            Confirm::Group { all: true, .. } => (
                "¿Borrar todo el tablero?".to_string(),
                "Se va todo: textos, listas, imágenes y dibujos de todas las páginas. Puedes deshacerlo con Ctrl+Z mientras el tablero siga abierto.".to_string(),
                "Borrar todo",
            ),
            Confirm::Group { count, .. } => (
                format!("¿Borrar {count} elementos?"),
                "Se van los objetos y trazos elegidos. Puedes deshacerlo con Ctrl+Z mientras el tablero siga abierto.".to_string(),
                "Borrar lo elegido",
            ),
            Confirm::Page { index, objects, strokes } => (
                if pages == 1 {
                    "¿Vaciar la página?".to_string()
                } else {
                    format!("¿Eliminar la página {}?", index + 1)
                },
                format!(
                    "Se va todo lo que tiene: {objects} objetos y {strokes} trazos. Las páginas de la derecha se corren. Puedes deshacerlo con Ctrl+Z mientras el tablero siga abierto."
                ),
                if pages == 1 { "Vaciar página" } else { "Eliminar página" },
            ),
        };
        let (text, muted, danger) = (colors.text, colors.muted, colors.danger);
        let button = |id: &'static str, label: &'static str, primary: bool, on_click: Box<dyn Fn(&mut Self, &mut Context<Self>)>, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(30.))
                .px(px(14.))
                .flex()
                .items_center()
                .rounded(px(15.))
                .text_size(px(12.))
                .cursor_pointer()
                .on_click(cx.listener(move |paper, _, _, cx| on_click(paper, cx)))
                .child(label)
                .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                    if primary {
                        el.bg(h.mix(danger.opacity(0.85), danger)).text_color(rgb(0xffffff))
                    } else {
                        el.bg(h.mix(text.opacity(0.06), text.opacity(0.12))).text_color(text)
                    }
                })
        };
        let cancel = button(
            "confirm-cancel",
            "Cancelar",
            false,
            Box::new(|paper, cx| {
                paper.confirm = None;
                cx.notify();
            }),
            cx,
        );
        let ok = button(
            "confirm-ok",
            action,
            true,
            Box::new(move |paper, cx| {
                paper.confirm = None;
                match confirm {
                    Confirm::Group { .. } => paper.delete_group(cx),
                    Confirm::Page { index, .. } => paper.remove_page(index, cx),
                }
            }),
            cx,
        );
        div()
            .id("confirm")
            .occlude()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.35))
            .on_mouse_down(MouseButton::Left, cx.listener(|paper, _, _, cx| {
                paper.confirm = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("confirm-card")
                    .w(px(340.))
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .rounded(px(14.))
                    .bg(colors.card)
                    .border_1()
                    .border_color(colors.line)
                    .shadow(vec![shadow(0.0, 12.0, 32.0, 0.35)])
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().text_size(px(14.)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(text).child(title))
                    .child(div().text_size(px(12.)).line_height(px(17.)).text_color(muted).child(body))
                    .child(
                        div()
                            .pt(px(4.))
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(cancel)
                            .child(ok),
                    ),
            )
    }

    fn render_export_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.colors;
        div()
            .absolute()
            // Los clics del menú no deben llegar a lo que hay debajo (el cajón).
            .occlude()
            // Bajo «Exportar», que queda antes de «Insertar» y «Volver».
            .right(px(178.))
            .top(px(TOOLBAR_H - 2.0))
            .min_w(px(MENU_W))
            .p(px(6.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(colors.card)
            .border_1()
            .border_color(colors.line)
            .shadow(vec![shadow(0.0, 8.0, 24.0, 0.22)])
            .children(Format::ALL.iter().map(|&format| {
                div()
                    .id(("export", format as usize))
                    .h(px(MENU_ROW))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(7.))
                    .text_size(px(12.5))
                    .text_color(colors.text)
                    .cursor_pointer()
                    .on_click(cx.listener(move |paper, _, window, cx| {
                        paper.start_export(format, window, cx)
                    }))
                    .child(format.label())
                    .hover_bg(("export-fx", format as usize), colors.text.opacity(0.0), colors.text.opacity(0.08))
            }))
            .child(
                div()
                    .px(px(10.))
                    .pt(px(4.))
                    .pb(px(2.))
                    .max_w(px(180.))
                    .text_size(px(10.5))
                    .text_color(colors.faint)
                    .child("Una página por hoja; las imágenes salen en .zip si hay varias."),
            )
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
                self.insert_text(body, self.visible_center(), window, cx);
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
                .cursor_pointer()
                .on_click(cx.listener(move |paper, _, _, cx| paper.set_drawer_tab(which, cx)))
                .child(label)
                .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                    let (rest, over) = if active { (0.14, 0.16) } else { (0.0, 0.08) };
                    el.bg(h.mix(text.opacity(rest), text.opacity(over)))
                        .text_color(if active { text } else { h.mix(muted, text) })
                })
        };
        let empty = match self.drawer_tab {
            DrawerTab::Clip => "Todavía no copiaste nada",
            DrawerTab::Texts => "No tienes textos guardados",
            DrawerTab::Captures => "Todavía no hay capturas",
        };
        let items: Vec<AnyElement> = self.drawer_items.iter().enumerate().map(|(i, item)| {
            let row = div()
                .id(("drawer-item", i))
                .w_full()
                .flex_none()
                .rounded(px(8.))
                .cursor_pointer()
                .on_click(cx.listener(move |paper, _, window, cx| {
                    paper.insert_item(i, window, cx)
                }));
            let fx = ("drawer-item-fx", i);
            let (rest, over) = (text.opacity(0.0), text.opacity(0.08));
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
                    .hover_bg(fx, rest, over)
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
                    .hover_bg(fx, rest, over)
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
                    .child(tab("drawer-clip", "Clip", DrawerTab::Clip, cx))
                    .child(tab("drawer-texts", "Textos", DrawerTab::Texts, cx))
                    .child(tab("drawer-caps", "Fotos", DrawerTab::Captures, cx)),
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
        self.deselect();
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
        if format.is_image() {
            cx.spawn(async move |this, cx| {
                let mut frames: Vec<(u32, u32, Vec<u8>)> = Vec::new();
                for page in 0..count {
                    let shown = this.update(cx, |paper, cx| {
                        paper.export_page = Some(page);
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

    /// Un bloque de texto, lista o imagen, con su marco sobre el papel.
    fn render_block(
        &self,
        block: &Block,
        layout: &Layout,
        colors: &Colors,
        measure: Measure,
    ) -> Option<AnyElement> {
        let s = layout.scale;
        let selected = self.selected.as_deref() == Some(block.id.as_str());
        let in_group = self.group.as_ref().is_some_and(|g| g.ids.contains(&block.id));
        let editing = self.editing.as_deref() == Some(block.id.as_str());
        let exporting = self.export_page.is_some();
        let frame = div()
            .absolute()
            .left(px(block.x * s))
            .top(px(block.y * s))
            .w(px(block.w * s))
            .h(px(block.h * s));

        let content: AnyElement = match &block.body {
            Body::Text { body } => {
                // Si no entra, la letra se achica (`ajustarFuente`).
                let font = self.cached_font(block, body, measure) * s;
                let inner = div()
                    .size_full()
                    .pt(px(TEXT_PAD_TOP * s))
                    .pb(px((TEXT_PAD_Y - TEXT_PAD_TOP) * s))
                    .px(px(TEXT_PAD_X / 2.0 * s))
                    .text_size(px(font))
                    .line_height(px(font * LINE));
                if editing {
                    inner.child(self.editor.clone()).into_any_element()
                } else if body.is_empty() {
                    inner
                        .italic()
                        .text_color(colors.muted)
                        .child(PLACEHOLDER)
                        .into_any_element()
                } else {
                    inner
                        .text_color(colors.text)
                        .child(SharedString::from(body.clone()))
                        .into_any_element()
                }
            }
            Body::Check { items } => {
                let font = FONT * s;
                let inner = div()
                    .size_full()
                    .px(px(PAD_X * s))
                    .py(px(PAD_Y * s))
                    .text_size(px(font))
                    .line_height(px(ROW_H * s));
                if editing {
                    inner.child(self.editor.clone()).into_any_element()
                } else {
                    let ok = colors.ok;
                    let (text, muted) = (colors.text, colors.muted);
                    inner
                        .flex()
                        .flex_col()
                        .children(items.iter().map(|item| {
                            let boxed = div()
                                .size(px(14.0 * s))
                                .flex_none()
                                .mr(px(8.0 * s))
                                .rounded(px(3.0 * s))
                                .border_1()
                                .border_color(if item.done { ok } else { muted })
                                .when(item.done, |el| el.bg(ok));
                            let label: SharedString = if item.text.is_empty() {
                                "Ítem".into()
                            } else {
                                item.text.clone().into()
                            };
                            div()
                                .h(px(ROW_H * s))
                                .flex()
                                .items_center()
                                .child(boxed)
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(if item.done || item.text.is_empty() { muted } else { text })
                                        .when(item.done, |el| el.line_through())
                                        .when(item.text.is_empty(), |el| el.italic())
                                        .child(label),
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
                        .object_fit(ObjectFit::Fill)
                        .into_any_element(),
                    None => div().size_full().into_any_element(),
                }
            }
            Body::Ink { .. } => return None,
        };

        let is_image = matches!(block.body, Body::Image { .. });
        let text = colors.text;
        // Sin fondo de tarjeta, como en Atic: un contorno leve al pasar, más
        // marcado al elegir.
        let outline = if exporting {
            None
        } else if selected || in_group {
            Some(text.opacity(0.45))
        } else if is_image {
            Some(gpui::black().opacity(0.10))
        } else {
            None
        };
        let id = SharedString::from(format!("block-{}", block.id));
        let surface = div()
            .id(id.clone())
            .size_full()
            .overflow_hidden()
            .rounded(px(BLOCK_RADIUS * s))
            .border_1()
            .border_color(outline.unwrap_or(text.opacity(0.0)))
            .child(content);
        let surface: AnyElement = if exporting || selected || in_group {
            surface.into_any_element()
        } else {
            surface
                .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                    el.border_color(h.mix(outline.unwrap_or(text.opacity(0.0)), text.opacity(0.22)))
                })
                .into_any_element()
        };
        Some(
            frame
                .child(surface)
                .when(selected && !editing && !exporting, |el| {
                    el.child(
                        div()
                            .absolute()
                            .right(px(-8.))
                            .bottom(px(-8.))
                            .size(px(16.))
                            .rounded(px(3.))
                            .border_2()
                            .border_color(text)
                            .bg(colors.card),
                    )
                })
                .into_any_element(),
        )
    }

    /// `fit_font` guardado por caja mientras su texto y su medida no cambien.
    fn cached_font(&self, block: &Block, body: &str, measure: Measure) -> f32 {
        let mut cache = self.font_cache.borrow_mut();
        if let Some((cached, w, h, font)) = cache.get(&block.id) {
            if cached == body && *w == block.w && *h == block.h {
                return *font;
            }
        }
        let font = fit_font(body, block.w, block.h, measure);
        cache.insert(block.id.clone(), (body.to_string(), block.w, block.h, font));
        font
    }

    /// La tinta de todos los bloques de tinta, y el trazo que se está dibujando.
    fn ink_canvas(&self, layout: &Layout) -> impl IntoElement {
        let scale = layout.scale;
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
                            .map(|(x, y)| point(bounds.origin.x + px(x * scale), bounds.origin.y + px(y * scale)))
                            .collect();
                        paint_polyline(window, &screen, (width * scale).max(1.0), *color);
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }

    /// La cuadrícula de puntos del papel (`.papel`): uno cada 24 px. Si a este
    /// zoom quedarían muy juntos, no se dibuja.
    fn dots_canvas(&self, layout: &Layout, color: Hsla) -> impl IntoElement {
        let scale = layout.scale;
        let step = 24.0 * scale;
        let (cols, rows) = ((self.paper_w() / 24.0) as usize, (PAGE_H / 24.0) as usize);
        let (vx, vy, vw, vh) = self.view_box();
        let (lx, ly) = (layout.x, layout.y);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                if step < 7.0 {
                    return;
                }
                let d = (2.0 * scale).clamp(1.0, 3.0);
                // Solo lo que se ve.
                let c0 = (((vx - lx) / step).floor().max(0.0)) as usize;
                let c1 = ((((vx + vw - lx) / step).ceil()) as usize).min(cols);
                let r0 = (((vy - ly) / step).floor().max(0.0)) as usize;
                let r1 = ((((vy + vh - ly) / step).ceil()) as usize).min(rows);
                for c in c0..c1 {
                    for r in r0..r1 {
                        let x = bounds.origin.x + px(12.0 * scale + c as f32 * step - d / 2.0);
                        let y = bounds.origin.y + px(12.0 * scale + r as f32 * step - d / 2.0);
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(point(x, y), gpui::size(px(d), px(d))),
                            color,
                        ).corner_radii(px(d / 2.0)));
                    }
                }
            },
        )
        .absolute()
        .size_full()
    }
}

/// Un trazo con puntas y uniones redondas.
fn paint_polyline(window: &mut Window, points: &[Point<Pixels>], width: f32, color: Hsla) {
    let options = StrokeOptions::default()
        .with_line_width(width)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round);
    let mut builder = PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(options));
    builder.add_polygon(points, false);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn shadow(x: f32, y: f32, blur: f32, alpha: f32) -> BoxShadow {
    BoxShadow {
        color: gpui::black().opacity(alpha),
        offset: point(px(x), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
    }
}

impl Render for PaperView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        perf::frame(now);
        // Lo que todavía anima pide otro cuadro.
        if self.view_anim.as_ref().is_some_and(|a| a.start.elapsed() >= VIEW_ANIM) {
            self.view_anim = None;
        }
        if self.flash.is_some_and(|(_, at)| at.elapsed() >= FLASH) {
            self.flash = None;
        }
        if self.view_anim.is_some() || self.flash.is_some() || self.dock_pill.is_running(now) {
            window.request_animation_frame();
        }
        // Soltar Espacio con la ventana sin foco no llega: se mira la tecla.
        if self.space && !space_held() {
            self.space = false;
        }
        let drag_capture = self.drag.is_some().then(|| cx.entity().downgrade());
        let layout = self.layout();
        let export_page = self.export_page;
        let theme = if export_page.is_some() { &self.light } else { &self.colors };
        let measure = TextMeasure::new(cx);
        let measure = |s: &str, size: f32| measure.width(s, size);
        // Solo los bloques que caen en lo que se ve (o en la página que se
        // exporta).
        let (vx, _, vw, _) = self.view_box();
        let (visible_l, visible_r) = match export_page {
            Some(page) => (page as f32 * PAGE_W, (page + 1) as f32 * PAGE_W),
            None => ((vx - layout.x) / layout.scale, (vx + vw - layout.x) / layout.scale),
        };
        let blocks: Vec<AnyElement> = self
            .blocks
            .iter()
            .filter(|b| {
                (b.x + b.w >= visible_l && b.x <= visible_r)
                    || self.editing.as_deref() == Some(b.id.as_str())
            })
            .filter_map(|block| self.render_block(block, &layout, theme, &measure))
            .collect();
        let s = layout.scale;
        let paper_w = self.paper_w();
        let (text, accent) = (theme.text, theme.accent);
        let empty = export_page.is_none()
            && whole_board(&self.blocks).len() == 0
            && self.live_stroke.is_none();
        let sheet = div()
            .absolute()
            .left(px(layout.x))
            .top(px(layout.y))
            .w(px(paper_w * s))
            .h(px(PAGE_H * s))
            .bg(theme.sheet)
            .overflow_hidden()
            // Al exportar, la hoja va lisa: sin puntos, borde ni esquinas, que
            // saldrían en la imagen.
            .when(export_page.is_none(), |el| {
                el.rounded(px(8.))
                    .shadow(vec![shadow(0.0, 2.0, 8.0, 0.28)])
                    .child(self.dots_canvas(&layout, text.opacity(0.22)))
                    // Las líneas entre páginas.
                    .children((1..self.pages()).map(|p| {
                        div()
                            .absolute()
                            .left(px(p as f32 * PAGE_W * s))
                            .top(px(0.))
                            .bottom(px(0.))
                            .w(px(1.))
                            .bg(text.opacity(0.12 * 0.8))
                    }))
                    .when_some(self.flash, |el, (x, at)| {
                        // Dos destellos de 1,2 s en el borde de la página nueva.
                        let t = (at.elapsed().as_secs_f32() / 1.2).fract();
                        let alpha = 1.0 - (t * 2.0 - 1.0).abs();
                        el.child(
                            div()
                                .absolute()
                                .left(px(x * s - 1.0))
                                .top(px(0.))
                                .bottom(px(0.))
                                .w(px(2.))
                                .bg(accent.opacity(alpha)),
                        )
                    })
            })
            // La tinta va encima de los bloques, como en Atic: un subrayado o
            // una flecha sobre una caja tiene que verse.
            .children(blocks)
            .child(self.ink_canvas(&layout))
            .when_some(self.marquee, |el, (x, y, w, h)| {
                el.child(
                    div()
                        .absolute()
                        .left(px(x * s))
                        .top(px(y * s))
                        .w(px(w * s))
                        .h(px(h * s))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(text.opacity(0.7))
                        .bg(text.opacity(0.10)),
                )
            })
            .when_some(
                self.eraser_at.filter(|_| self.tool == Tool::Eraser && export_page.is_none()),
                |el, (x, y)| {
                    // El alcance real del borrador.
                    let r = self.eraser_radius * s;
                    el.child(
                        div()
                            .absolute()
                            .left(px(x * s - r))
                            .top(px(y * s - r))
                            .size(px(r * 2.0))
                            .rounded(px(r))
                            .border_1()
                            .border_color(text.opacity(0.65))
                            .bg(text.opacity(0.08)),
                    )
                },
            );
        if let Some(page) = export_page {
            // Solo la página que se captura: el resto del papel queda fuera.
            let left = page as f32 * PAGE_W * s;
            return div()
                .size_full()
                .relative()
                .child(
                    div()
                        .absolute()
                        .left(px(layout.x + left))
                        .top(px(layout.y))
                        .w(px(PAGE_W * s))
                        .h(px(PAGE_H * s))
                        .overflow_hidden()
                        .child(sheet.left(px(-left)).top(px(0.))),
                )
                .into_any_element();
        }
        // La caja de lo elegido en grupo, con su «Borrar N».
        let group_box = self.group.as_ref().and_then(|group| {
            let mut boxes: Vec<(f32, f32, f32, f32)> = self
                .blocks
                .iter()
                .filter(|b| group.ids.contains(&b.id))
                .map(|b| (b.x, b.y, b.w, b.h))
                .collect();
            if let Some(Body::Ink { strokes, .. }) = self.blocks.iter().find(|b| b.is_ink()).map(|b| &b.body) {
                boxes.extend(group.strokes.iter().filter_map(|&i| strokes.get(i)).map(stroke_box));
            }
            let x0 = boxes.iter().map(|b| b.0).fold(f32::MAX, f32::min);
            let y0 = boxes.iter().map(|b| b.1).fold(f32::MAX, f32::min);
            let x1 = boxes.iter().map(|b| b.0 + b.2).fold(f32::MIN, f32::max);
            let y1 = boxes.iter().map(|b| b.1 + b.3).fold(f32::MIN, f32::max);
            (x0 <= x1).then(|| {
                let count = group.len();
                let (card, line) = (self.colors.card, self.colors.line);
                div()
                    .absolute()
                    .left(px(layout.x + (x0 - 8.0) * s))
                    .top(px(layout.y + (y0 - 8.0) * s))
                    .w(px((x1 - x0 + 16.0) * s))
                    .h(px((y1 - y0 + 16.0) * s))
                    .rounded(px(4.))
                    .border_1()
                    .border_dashed()
                    .border_color(text.opacity(0.55))
                    .child(
                        div()
                            .id("group-delete")
                            .occlude()
                            .absolute()
                            .top(px(6.))
                            .left(px(6.))
                            .h(px(26.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(13.))
                            .bg(card)
                            .border_1()
                            .border_color(line)
                            .text_size(px(12.))
                            .text_color(text)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|paper, _, _, cx| paper.ask_delete_group(cx)))
                            .child(svg().path("icons/trash.svg").size(px(13.)).text_color(text))
                            .child(format!("Borrar {count}")),
                    )
            })
        });
        let tool = self.tool;
        let panning = self.space || tool == Tool::Hand;
        let empty_hint = empty.then(|| {
            let (_, vy, vw, vh) = self.view_box();
            let inking = matches!(tool, Tool::Pen | Tool::Highlight | Tool::Eraser);
            div()
                .absolute()
                .left(px(0.))
                .w(px(vw))
                .top(px(vy + vh * 0.26))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .max_w(px(280.))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(self.colors.text)
                        .text_center()
                        .child("Elige el Lápiz para esbozar, o empieza con una nota."),
                )
                .when(!inking, |el| {
                    el.child(
                        div()
                            .id("empty-add-text")
                            .occlude()
                            .h(px(30.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .rounded(px(15.))
                            .text_size(px(12.))
                            .text_color(self.colors.text)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|paper, _, window, cx| {
                                let at = (PAGE_W / 2.0, PAGE_H * 0.42);
                                paper.insert_text(String::new(), at, window, cx);
                            }))
                            .child("Añadir texto")
                            .hover_bg("empty-add-text-fx", self.colors.text.opacity(0.08), self.colors.text.opacity(0.14)),
                    )
                })
        });
        let cursor = match (panning, &self.drag, tool) {
            (_, Some(Drag::Pan { .. }), _) => gpui::CursorStyle::ClosedHand,
            (true, _, _) => gpui::CursorStyle::OpenHand,
            (_, _, Tool::Pen | Tool::Highlight | Tool::Eraser) => gpui::CursorStyle::Crosshair,
            (_, _, Tool::Text) => gpui::CursorStyle::IBeam,
            _ => gpui::CursorStyle::Arrow,
        };
        let root = div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::delete_selected))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::paste_block))
            .on_action(cx.listener(Self::new_text))
            .on_action(cx.listener(Self::new_list))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_middle_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_middle_up))
            .size_full()
            .relative()
            .overflow_hidden()
            .font_family(FONT_FAMILY)
            .text_color(self.colors.text)
            .bg(self.colors.bg)
            .rounded(px(self.radius))
            .cursor(cursor)
            .child(perf::marker(0))
            .child(sheet)
            .child(perf::marker(1))
            .children(drag_capture.map(|this| {
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == gpui::DispatchPhase::Bubble {
                                let _ = this.update(cx, |paper, cx| paper.drag_move(event, cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_full()
            }))
            .children(group_box)
            .children(empty_hint)
            .child(self.render_dock(cx))
            .child(perf::marker(2))
            .child(self.render_strip(cx))
            .child(perf::marker(3))
            .child(div().absolute().top(px(0.)).left(px(0.)).w_full().bg(self.colors.bg).rounded_t(px(self.radius)).child(self.render_toolbar(cx)))
            .child(perf::marker(4))
            .when(self.drawer, |el| el.child(self.render_drawer(cx)))
            .when(self.export_menu, |el| el.child(self.render_export_menu(cx)))
            .when_some(self.confirm, |el, confirm| el.child(self.render_confirm(confirm, cx)))
            .child(canvas(|_, _, _| {}, |_, _, _, _| perf::painted()).absolute().size(px(0.)))
            .into_any_element();
        perf::built(now);
        root
    }
}

/// Medición temporal (`PILL_BOARD_PERF=1`): cuadros, eventos de arrastre y
/// cuánto tarda armar y pintar el tablero, una línea por segundo.
mod perf {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct Stats {
        since: Option<Instant>,
        frames: u32,
        moves: u32,
        build: Duration,
        build_max: Duration,
        frame_start: Option<Instant>,
        paint: Duration,
        paint_max: Duration,
        /// Tiempo desde el inicio del cuadro hasta cada marca, sumado.
        marks: [Duration; 5],
    }

    static STATS: Mutex<Option<Stats>> = Mutex::new(None);

    fn on() -> bool {
        std::env::var_os("PILL_BOARD_PERF").is_some()
    }

    fn with(f: impl FnOnce(&mut Stats)) {
        if !on() {
            return;
        }
        let mut guard = STATS.lock().unwrap();
        let stats = guard.get_or_insert_with(Stats::default);
        f(stats);
        let since = *stats.since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(1) && stats.frames > 0 {
            eprintln!(
                "[tablero] {} cuadros/s · {} movimientos/s · armar {:.2} ms (máx {:.2}) · armar+pintar {:.2} ms (máx {:.2})",
                stats.frames,
                stats.moves,
                stats.build.as_secs_f64() * 1000.0 / stats.frames as f64,
                stats.build_max.as_secs_f64() * 1000.0,
                stats.paint.as_secs_f64() * 1000.0 / stats.frames as f64,
                stats.paint_max.as_secs_f64() * 1000.0,
            );
            let ms = |d: Duration| d.as_secs_f64() * 1000.0 / stats.frames as f64;
            let m = stats.marks;
            eprintln!(
                "[tablero]   hasta pintar {:.2} · papel {:.2} · dock {:.2} · tira {:.2} · barra {:.2}",
                ms(m[0]),
                ms(m[1].saturating_sub(m[0])),
                ms(m[2].saturating_sub(m[1])),
                ms(m[3].saturating_sub(m[2])),
                ms(m[4].saturating_sub(m[3])),
            );
            *stats = Stats::default();
        }
    }

    pub fn frame(now: Instant) {
        with(|s| {
            s.frames += 1;
            s.frame_start = Some(now);
        });
    }

    pub fn built(start: Instant) {
        with(|s| {
            let d = start.elapsed();
            s.build += d;
            s.build_max = s.build_max.max(d);
        });
    }

    pub fn painted() {
        with(|s| {
            if let Some(start) = s.frame_start.take() {
                let d = start.elapsed();
                s.paint += d;
                s.paint_max = s.paint_max.max(d);
            }
        });
    }

    /// Un elemento vacío que anota cuándo se pinta: lo que se pintó antes de
    /// él ya está.
    pub fn marker(i: usize) -> impl gpui::IntoElement {
        use gpui::Styled;
        gpui::canvas(|_, _, _| {}, move |_, _, _, _| {
            with(|s| {
                if let Some(start) = s.frame_start {
                    s.marks[i] += start.elapsed();
                }
            })
        })
        .absolute()
        .size(gpui::px(0.))
    }

    pub fn moved() {
        with(|s| s.moves += 1);
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
pub(crate) fn recent_captures() -> Vec<PathBuf> {
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

    /// Una letra de 7 px por carácter a 13,5 (proporcional al tamaño).
    fn fake_measure(s: &str, size: f32) -> f32 {
        s.chars().count() as f32 * 7.0 * size / FONT
    }

    #[test]
    fn a_text_box_grows_with_its_longest_line_up_to_the_cap() {
        // Corto: el ancho mínimo y una línea (28 de relleno + 13,5 × 1,5).
        assert_eq!(text_size("hola", &fake_measure), (TEXT_MIN_W, 49.0));
        // Una línea de 40 caracteres: 280 + relleno + cursor.
        let line = "a".repeat(40);
        assert_eq!(text_size(&line, &fake_measure).0, 280.0 + TEXT_PAD_X + TEXT_CURSOR);
        // Muy larga: llega al tope y baja de renglón.
        let long = "palabra ".repeat(40);
        let (w, h) = text_size(&long, &fake_measure);
        assert_eq!(w, TEXT_MAX_W);
        assert!(h > TEXT_MIN_H);
    }

    #[test]
    fn the_font_shrinks_only_when_the_text_does_not_fit() {
        let body = "palabra ".repeat(30);
        assert_eq!(fit_font(&body, 560.0, 400.0, &fake_measure), FONT);
        let small = fit_font(&body, 200.0, 80.0, &fake_measure);
        assert!(small < FONT && small >= FONT_MIN);
        // Nunca baja del mínimo aunque no entre.
        assert_eq!(fit_font(&"x ".repeat(500), 100.0, 50.0, &fake_measure), FONT_MIN);
    }

    fn text_block(id: &str, x: f32) -> Block {
        Block {
            id: id.into(),
            x,
            y: 100.0,
            w: 200.0,
            h: 80.0,
            body: Body::Text { body: id.into() },
        }
    }

    fn ink_block(strokes: Vec<Stroke>) -> Block {
        Block {
            id: "ink".into(),
            x: 0.0,
            y: 0.0,
            w: PAGE_W,
            h: PAGE_H,
            body: Body::Ink { strokes, height: 0 },
        }
    }

    #[test]
    fn removing_a_page_shifts_what_is_to_its_right() {
        let blocks = vec![
            text_block("a", 100.0),
            text_block("b", PAGE_W + 100.0),
            text_block("c", PAGE_W * 2.0 + 100.0),
            ink_block(vec![
                stroke(&[[PAGE_W + 10.0, 10.0], [PAGE_W + 50.0, 10.0]]),
                stroke(&[[PAGE_W * 2.0 + 10.0, 10.0], [PAGE_W * 2.0 + 50.0, 10.0]]),
            ]),
        ];
        assert_eq!(page_content(&blocks, 1), (1, 1));
        let left = without_page(&blocks, 1);
        let ids: Vec<&str> = left.iter().filter(|b| !b.is_ink()).map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        // Lo de la tercera página quedó en la segunda.
        assert_eq!(left.iter().find(|b| b.id == "c").unwrap().x, PAGE_W + 100.0);
        let Body::Ink { strokes, .. } = &left.iter().find(|b| b.is_ink()).unwrap().body else {
            unreachable!()
        };
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].points[0][0], PAGE_W + 10.0);
    }

    #[test]
    fn the_marquee_takes_what_it_touches() {
        let blocks = vec![
            text_block("a", 100.0),
            text_block("b", 600.0),
            ink_block(vec![stroke(&[[120.0, 300.0], [140.0, 300.0]]), stroke(&[[900.0, 800.0], [950.0, 800.0]])]),
        ];
        // Toca en parte a «a» y el primer trazo, no a «b».
        let group = in_marquee(&blocks, (50.0, 150.0, 200.0, 200.0));
        assert_eq!(group.ids, ["a"]);
        assert_eq!(group.strokes, [0]);
        assert_eq!(whole_board(&blocks).len(), 4);
    }

    #[test]
    fn a_clean_eraser_click_takes_the_nearest_whole_line() {
        // Como en Atic, se mide contra los puntos del trazo.
        let line = |y: f32| stroke(&(0..=10).map(|i| [i as f32 * 10.0, y]).collect::<Vec<_>>());
        let strokes = vec![line(0.0), line(50.0)];
        assert_eq!(nearest_stroke(&strokes, (50.0, 45.0), 16.0), Some(1));
        assert_eq!(nearest_stroke(&strokes, (50.0, 25.0), 16.0), None);
    }

    #[test]
    fn the_dock_pill_lands_on_each_tool() {
        // Mover, Mano │ Lápiz, Resaltador, Borrador │ Texto, Lista.
        assert_eq!(dock_x(0), DOCK_PAD);
        assert_eq!(dock_x(1), DOCK_PAD + DOCK_BTN + DOCK_GAP);
        assert_eq!(dock_x(2), DOCK_PAD + 2.0 * (DOCK_BTN + DOCK_GAP) + DOCK_SEP + DOCK_GAP);
        assert_eq!(dock_x(5) - dock_x(4), DOCK_BTN + DOCK_GAP + DOCK_SEP + DOCK_GAP);
    }

    #[test]
    fn saving_keeps_the_rest_of_the_note() {
        let dir = std::env::temp_dir().join(format!("pill-board-test-{}", now_ms()));
        let board = dir.join("atic-tablero");
        std::fs::create_dir_all(&board).unwrap();
        std::fs::write(
            board.join("note.json"),
            r#"{"id":"n","app":"atic-tablero","pages":[
                {"id":"p0","title":"otra","blocks":[{"kind":"image","id":"i","asset":"usada.png","width":1,"height":1}],"updated_at":1},
                {"id":"p1","title":"","blocks":[{"kind":"sticker","id":"s","emoji":"x"}],"updated_at":1}],"updated_at":1}"#,
        )
        .unwrap();
        let assets = board.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("usada.png"), b"x").unwrap();
        std::fs::write(assets.join("huerfana.png"), b"x").unwrap();
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
        // Un bloque que esta versión no conoce no se pierde al guardar.
        assert_eq!(pages[1]["blocks"][1]["kind"], "sticker");
        assert_eq!(load_blocks().len(), 1);

        // Al cerrar se van las imágenes que ninguna página usa.
        collect_garbage();
        assert!(assets.join("usada.png").exists());
        assert!(!assets.join("huerfana.png").exists());

        // Una nota ilegible queda al lado en vez de perderse.
        std::fs::write(board.join("note.json"), "{roto").unwrap();
        assert!(save_blocks(&blocks));
        assert_eq!(std::fs::read_to_string(board.join("note.roto.json")).unwrap(), "{roto");
        assert_eq!(load_blocks().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
