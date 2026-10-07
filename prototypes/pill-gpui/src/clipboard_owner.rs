//! La pill es dueña del historial del portapapeles: corre el watcher de
//! `atic-clipboard`, que guarda en `history.json` de Atic. El panel lo sigue
//! leyendo de ahí (`history.rs`), igual que cuando lo escribía la app de Tauri.
//!
//! Solo con `native_pill`: sin ella, el dueño es Tauri y aquí no se captura.
//! Se mira `config.json` en cada vuelta del watcher (con caché por fecha de
//! modificación), así que apagar el historial en Ajustes surte efecto en el
//! acto.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use atic_clipboard::{History, Hooks};
use atic_core::{AppDirs, Config};

/// Arranca el watcher. No hace nada si no se encuentra la carpeta de Atic.
pub fn start() {
    let Ok(dirs) = AppDirs::new() else {
        tracing::warn!("portapapeles: sin carpeta de datos de Atic");
        return;
    };
    let dir = dirs.clipboard_dir();
    let shared = Arc::new(Mutex::new(History::load(&dir)));
    let mut owner = Owner::new(dirs.config_path());
    atic_clipboard::spawn_watcher(
        shared,
        dir,
        Hooks {
            enabled: Box::new(move || owner.capturing()),
            image_label,
            on_change: Box::new(|_| {}),
        },
    );
}

fn image_label(width: usize, height: usize) -> String {
    crate::i18n::tf("pill.clipboard.imageLabel", &[("w", &width), ("h", &height)])
}

/// Si la pill debe capturar, releyendo `config.json` solo cuando cambia.
struct Owner {
    path: PathBuf,
    modified: Option<SystemTime>,
    capturing: bool,
}

impl Owner {
    fn new(path: PathBuf) -> Self {
        Self { path, modified: None, capturing: false }
    }

    fn capturing(&mut self) -> bool {
        let modified = std::fs::metadata(&self.path).and_then(|meta| meta.modified()).ok();
        if modified != self.modified || modified.is_none() {
            self.modified = modified;
            self.capturing = should_capture(&Config::load(&self.path));
        }
        self.capturing
    }
}

fn should_capture(cfg: &Config) -> bool {
    cfg.native_pill && cfg.clipboard_history
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captura_solo_con_la_pill_nativa_y_el_historial_encendido() {
        let mut cfg = Config::default();
        cfg.native_pill = true;
        cfg.clipboard_history = true;
        assert!(should_capture(&cfg));
        cfg.native_pill = false;
        assert!(!should_capture(&cfg));
        cfg.native_pill = true;
        cfg.clipboard_history = false;
        assert!(!should_capture(&cfg));
    }
}
