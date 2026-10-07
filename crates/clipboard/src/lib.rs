//! El historial del portapapeles de Atic, sin UI: el modelo de `history.json`,
//! cómo se guarda, el watcher que lo llena y el candado para escribir en el
//! portapapeles del sistema.
//!
//! Lo usan la app de Tauri y la pill GPUI. Solo uno de los dos procesos corre
//! el watcher a la vez (con `native_pill`, la pill): dos dueños de
//! `history.json` se pisarían.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use image::ImageEncoder;
use serde::{Deserialize, Serialize};

mod gate;
mod queue;
mod sensitive;
mod source;
mod store;
mod watcher;

pub use queue::{PasteQueue, PasteQueueItem, MAX_QUEUED};
pub use gate::{set_system_text, try_clipboard_read, with_clipboard_write};
pub use sensitive::clipboard_is_sensitive;
pub use source::clipboard_source_app;
#[cfg(windows)]
pub use source::process_exe_path;
pub use store::{
    load_dismissed_captures, load_history, save_dismissed_captures, save_history, History,
    PendingCapture,
};
pub use watcher::{ingest_image, spawn_watcher, Change, Hooks};

pub const MAX_ITEMS: usize = 100;
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const HISTORY_FILE: &str = "history.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: String,
    pub kind: ClipboardKind,
    /// Preview de texto o etiqueta corta para imágenes.
    pub preview: String,
    /// Texto completo (solo text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Ruta absoluta al PNG (solo image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    pub created_at_ms: u64,
    #[serde(default)]
    pub pinned: bool,
    /// Huella para deduplicar.
    pub fingerprint: String,
    /// Origen: watcher | capture
    #[serde(default)]
    pub source: String,
    /// Ruta del ejecutable que copió: el dueño del portapapeles o, si no lo
    /// declara, la ventana activa. Solo lo llena el watcher; las entradas
    /// anteriores no lo tienen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn fingerprint_text(text: &str) -> String {
    let mut h = DefaultHasher::new();
    "text".hash(&mut h);
    text.hash(&mut h);
    format!("{:x}", h.finish())
}

pub fn fingerprint_image(bytes: &[u8], w: usize, h: usize) -> String {
    let mut hasher = DefaultHasher::new();
    "image".hash(&mut hasher);
    w.hash(&mut hasher);
    h.hash(&mut hasher);
    bytes.len().hash(&mut hasher);
    let step = (bytes.len() / 64).max(1);
    for chunk in bytes.iter().step_by(step).take(64) {
        chunk.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

pub fn encode_png_rgba(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(
            rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(out)
}
