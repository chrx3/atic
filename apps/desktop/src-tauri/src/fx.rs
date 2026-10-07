//! Tasas de cambio para el conversor del launcher. La descarga y la caché
//! viven en `atic_calc::fx` (las comparte la pill GPUI); acá se toman la ruta
//! y el interruptor del estado de la app.

use tauri::{AppHandle, Manager};

use crate::state::AppState;
use atic_core::MutexExt;

pub use atic_calc::fx::snapshot;

/// Arranque: carga la caché de disco y, si el opt-in está encendido y la tabla
/// está vieja, refresca en background.
pub fn init(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let enabled = state.config.lock_or_recover().launcher_currency;
    atic_calc::fx::init(&state.dirs.fx_rates_path(), enabled);
}

/// Cambió el interruptor: al encenderlo, trae tasas si la caché está vieja.
pub fn on_toggle(app: &AppHandle, enabled: bool) {
    if enabled {
        refresh_if_stale(app);
    }
}

/// Refresca si la tabla no existe o quedó vieja, sin bloquear a quien llama.
pub fn refresh_if_stale(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        atic_calc::fx::refresh_if_stale(state.dirs.fx_rates_path());
    }
}
