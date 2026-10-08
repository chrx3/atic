//! Lo que la pill aplica de `config.json` cuando cambia, lo cambie quien lo
//! cambie (sus Ajustes o la ventana de Tauri): el idioma y, con la pill
//! nativa, el inicio con Windows. Mira la fecha del archivo cada 2 s.

use std::time::{Duration, SystemTime};

use atic_core::{AppDirs, Config};

const EVERY: Duration = Duration::from_secs(2);

pub fn spawn() {
    let Ok(dirs) = AppDirs::new() else {
        return;
    };
    let path = dirs.config_path();
    let started = std::thread::Builder::new()
        .name("config-json".into())
        .spawn(move || {
            let mut seen: Option<SystemTime> = None;
            loop {
                let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
                if modified.is_some() && modified != seen {
                    seen = modified;
                    apply(&Config::load(&path));
                }
                std::thread::sleep(EVERY);
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no arrancó el seguimiento de config.json");
    }
}

fn apply(cfg: &Config) {
    crate::i18n::set_language(&cfg.resolved_ui_language());
    // En desarrollo no: apuntaría el inicio con Windows al exe de `target/`.
    if cfg.native_pill && !cfg!(debug_assertions) {
        crate::platform::autostart::sync(cfg.autostart);
    }
}
