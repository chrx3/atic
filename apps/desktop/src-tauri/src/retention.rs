//! Política local de conservación de grabaciones. La lógica vive en
//! `atic_core::housekeeping` (la comparte la pill GPUI).

use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::housekeeping::{self, RetentionCleanupResult, RetentionPreview};

use crate::state::AppState;
use atic_core::MutexExt;

fn days_or_config(state: &AppState, days: Option<u32>) -> u32 {
    days.unwrap_or_else(|| state.config.lock_or_recover().retention_days)
}

#[tauri::command]
pub fn retention_preview(
    state: State<AppState>,
    days: Option<u32>,
) -> Result<RetentionPreview, String> {
    let days = days_or_config(&state, days);
    housekeeping::retention_preview(&state.db.lock_or_recover(), &state.dirs, days)
}

#[tauri::command]
pub fn cleanup_retention(
    app: AppHandle,
    state: State<AppState>,
    confirm: bool,
    days: Option<u32>,
) -> Result<RetentionCleanupResult, String> {
    if !confirm {
        return Err(crate::ui_lang::msg(
            "La limpieza requiere confirmación explícita.",
            "Cleanup requires explicit confirmation.",
        ));
    }
    if state.active.lock_or_recover().is_some() || state.dictation.lock_or_recover().is_some() {
        return Err(crate::ui_lang::msg(
            "Termina la grabación o el dictado antes de limpiar datos.",
            "Finish recording or dictation before cleaning data.",
        ));
    }
    let days = days_or_config(&state, days);
    let result = housekeeping::retention_cleanup(&state.db.lock_or_recover(), &state.dirs, days)?;
    let _ = app.emit("recordings-changed", ());
    Ok(result)
}

pub fn run_auto_cleanup(app: &AppHandle) {
    let state = app.state::<AppState>();
    let config = state.config.lock_or_recover().clone();
    housekeeping::run_auto_cleanup(&state.db.lock_or_recover(), &state.dirs, &config);
}
