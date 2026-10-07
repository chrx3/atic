//! La pill es dueña del historial del portapapeles: corre el watcher de
//! `atic-clipboard`, que guarda en `history.json` de Atic. El panel lo sigue
//! leyendo de ahí (`history.rs`), igual que cuando lo escribía la app de Tauri.
//!
//! Solo con `native_pill`: sin ella, el dueño es Tauri y aquí no se captura.
//! Se mira `config.json` en cada vuelta del watcher (con caché por fecha de
//! modificación), así que apagar el historial en Ajustes surte efecto en el
//! acto.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use atic_clipboard::{History, Hooks};
use atic_core::{AppDirs, Config, MutexExt};

/// El historial en memoria y su carpeta, para fijar y borrar desde el panel.
static SHARED: OnceLock<(Arc<Mutex<History>>, PathBuf)> = OnceLock::new();
/// `native_pill` según la última lectura de `config.json`.
static OWNER: AtomicBool = AtomicBool::new(false);

/// Arranca el watcher. No hace nada si no se encuentra la carpeta de Atic.
pub fn start() {
    let Ok(dirs) = AppDirs::new() else {
        tracing::warn!("portapapeles: sin carpeta de datos de Atic");
        return;
    };
    let dir = dirs.clipboard_dir();
    let shared = Arc::new(Mutex::new(History::load(&dir)));
    let _ = SHARED.set((shared.clone(), dir.clone()));
    let mut owner = Owner::new(dirs.config_path());
    owner.capturing();
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

/// La pill es la dueña de `history.json` (`native_pill`): fijar y borrar van
/// ahí y no a su `local.json`.
pub fn owns() -> bool {
    SHARED.get().is_some() && OWNER.load(Ordering::Relaxed)
}

/// Fija o suelta un ítem en `history.json`. `false` si la pill no es la dueña
/// o el ítem no está.
pub fn set_pinned(id: &str, pinned: bool) -> bool {
    let Some((shared, dir)) = SHARED.get().filter(|_| owns()) else {
        return false;
    };
    shared.lock_or_recover().set_pinned(dir, id, pinned)
}

/// Borra un ítem de `history.json` (no vuelve aunque se copie otra vez).
/// `false` si la pill no es la dueña o el ítem no está.
pub fn delete(id: &str) -> bool {
    let Some((shared, dir)) = SHARED.get().filter(|_| owns()) else {
        return false;
    };
    shared.lock_or_recover().delete(dir, id).is_some()
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
            let cfg = Config::load(&self.path);
            OWNER.store(cfg.native_pill, Ordering::Relaxed);
            self.capturing = should_capture(&cfg);
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
