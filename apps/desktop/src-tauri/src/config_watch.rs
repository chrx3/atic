//! Relee `config.json` cuando otra app lo cambia.
//!
//! La pill nativa (GPUI) guarda sus Ajustes en el mismo archivo. Atic lo tiene
//! en memoria y lo reescribe entero al guardar: sin releerlo seguiría con los
//! valores viejos y, al guardar cualquier otra cosa, pisaría lo de la pill.
//!
//! Basta con mirar la fecha del archivo cada segundo: es un `stat`, y los
//! guardados de Atic también la cambian pero dejan el archivo igual a lo que
//! ya tiene en memoria, así que no hacen nada.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use atic_core::{Config, MutexExt};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

const EVERY: Duration = Duration::from_secs(1);
/// Cuánto tiempo tras arrancar se reintentan los atajos que fallaron, y cada
/// cuántas vueltas de `EVERY`.
const SHORTCUT_RETRY_FOR: Duration = Duration::from_secs(30);
const SHORTCUT_RETRY_EVERY: u32 = 3;

/// Si la pill es la nativa, tal como estaba al arrancar. Cambiarlo exige
/// reiniciar: el overlay se crea (o no) una sola vez.
static NATIVE_PILL: AtomicBool = AtomicBool::new(false);

pub fn set_native_pill(on: bool) {
    NATIVE_PILL.store(on, Ordering::Relaxed);
}

pub fn native_pill() -> bool {
    NATIVE_PILL.load(Ordering::Relaxed)
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    let path = app.state::<AppState>().dirs.config_path();
    let spawned = std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let mut seen = modified(&path);
            let started = Instant::now();
            let mut ticks: u32 = 0;
            loop {
                std::thread::sleep(EVERY);
                ticks += 1;
                if started.elapsed() < SHORTCUT_RETRY_FOR && ticks % SHORTCUT_RETRY_EVERY == 0 {
                    retry_failed_shortcuts(&app);
                }
                let now = modified(&path);
                if now == seen {
                    continue;
                }
                seen = now;
                let Some(on_disk) = load(&path) else {
                    continue;
                };
                let current = app.state::<AppState>().config.lock_or_recover().clone();
                if same(&on_disk, &current) {
                    continue;
                }
                tracing::info!("config.json cambió fuera de Atic: se aplica");
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(err) = crate::commands::apply_config(&handle, on_disk, false) {
                        tracing::warn!(%err, "no se pudo aplicar la config releída");
                    }
                });
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "no se pudo vigilar config.json");
    }
}

/// La pill nativa, si estaba abierta antes que Atic, tiene los atajos hasta
/// que nota que Atic arrancó (unos segundos): los que fallaron al iniciar se
/// reintentan un rato en vez de quedar sin dueño.
fn retry_failed_shortcuts(app: &AppHandle) {
    if native_pill() || app.state::<AppState>().shortcut_failures.lock_or_recover().is_empty() {
        return;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = crate::shortcuts::reregister_from_config(&handle) {
            tracing::warn!(%err, "no se pudieron reintentar los atajos");
        }
    });
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `Config::load` devuelve los valores por defecto si el archivo falta o no se
/// entiende. Aquí eso significaría borrar la config del usuario: se ignora.
fn load(path: &Path) -> Option<Config> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?;
    Some(Config::load(path))
}

fn same(a: &Config, b: &Config) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_or_missing_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("atic-config-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        assert!(load(&path).is_none());
        std::fs::write(&path, "{ a medio escribir").unwrap();
        assert!(load(&path).is_none());
        std::fs::write(&path, r#"{"native_pill": true}"#).unwrap();
        assert!(load(&path).is_some_and(|cfg| cfg.native_pill));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_detects_any_change() {
        let a = Config::default();
        let mut b = a.clone();
        assert!(same(&a, &b));
        b.dictation_mode = "toggle".into();
        assert!(!same(&a, &b));
    }
}
