//! Idioma de la UI (`config.ui_language`) para copy nativo y errores IPC.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

static UI_EN: AtomicBool = AtomicBool::new(false);

pub fn set_english(en: bool) {
    UI_EN.store(en, Ordering::Relaxed);
}

pub fn english() -> bool {
    UI_EN.load(Ordering::Relaxed)
}

pub fn pick<'a>(en: bool, es: &'a str, english: &'a str) -> &'a str {
    if en {
        english
    } else {
        es
    }
}

pub fn msg(es: &str, en: &str) -> String {
    pick(english(), es, en).to_string()
}

pub fn rec_missing() -> String {
    msg("Grabación no encontrada.", "Recording not found.")
}

#[allow(dead_code)]
pub fn capture_windows_only() -> String {
    msg(
        "La captura de pantalla solo está disponible en Windows.",
        "Screen capture is only available on Windows.",
    )
}

pub fn apply_window_titles(app: &AppHandle) {
    let en = english();
    let titles = [
        ("capture-shelf", pick(en, "Captura", "Capture")),
        (
            "capture-annotate",
            pick(en, "Dibujar sobre la captura", "Draw on capture"),
        ),
        ("launcher", pick(en, "Buscar", "Search")),
    ];
    for (label, title) in titles {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_title(title);
        }
    }
}
