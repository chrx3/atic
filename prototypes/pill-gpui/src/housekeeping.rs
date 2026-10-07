//! Lo que Atic hace al arrancar y, con `native_pill`, le toca a la pill:
//! reparar grabaciones que quedaron a medio transcribir o resumir, y borrar
//! las grabaciones y capturas viejas según Ajustes. En un hilo aparte: toca
//! el disco y la base.

use atic_core::{housekeeping, AppDirs, Config, Db};

pub fn spawn() {
    let started = std::thread::Builder::new()
        .name("orden-al-arrancar".into())
        .spawn(run);
    if let Err(error) = started {
        tracing::warn!(%error, "no arrancó el orden al iniciar");
    }
}

fn run() {
    let Ok(dirs) = AppDirs::new() else {
        return;
    };
    let config = Config::load(&dirs.config_path());
    // Sin la pill nativa, el dueño de las grabaciones es la app de Tauri.
    if !config.native_pill {
        return;
    }
    match Db::open(&dirs.db_path()) {
        Ok(db) => {
            housekeeping::recover_orphaned_statuses(&db, &dirs);
            housekeeping::run_auto_cleanup(&db, &dirs, &config);
        }
        Err(error) => tracing::warn!(%error, "no se abrió la base para el orden al iniciar"),
    }
    let result = atic_capture::retention::cleanup_captures(
        &dirs.captures_dir(),
        config.capture_retention_hours,
        std::time::SystemTime::now(),
    );
    if result.deleted > 0 || !result.errors.is_empty() {
        tracing::info!(
            deleted = result.deleted,
            errors = result.errors.len(),
            "limpieza automática de capturas"
        );
    }
}
