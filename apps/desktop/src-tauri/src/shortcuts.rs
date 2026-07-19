//! Registro de atajos globales (grabación + dictado + traer pill + captura).

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::{dictation, state};

/// Registra (o re-registra) los atajos globales.
///
/// Los errores de *sintaxis* de cualquier atajo abortan (se valida antes de
/// persistir en `set_config`). En cambio, un fallo al **registrar** un atajo
/// concreto (p. ej. conflicto con otra app) solo se registra en el log y no
/// impide registrar los demás: un conflicto de captura no debe desactivar
/// grabación, dictado ni pill.
pub fn register_shortcuts(
    app: &AppHandle,
    recording_shortcut: &str,
    dictation_shortcut: &str,
    summon_pill_shortcut: &str,
    screenshot_shortcut: &str,
) -> Result<(), String> {
    let recording: Shortcut = recording_shortcut
        .parse()
        .map_err(|e| format!("Atajo de grabación inválido ({recording_shortcut}): {e}"))?;
    let dictation: Shortcut = dictation_shortcut
        .parse()
        .map_err(|e| format!("Atajo de dictado inválido ({dictation_shortcut}): {e}"))?;
    let summon: Shortcut = summon_pill_shortcut
        .parse()
        .map_err(|e| format!("Atajo de pill inválido ({summon_pill_shortcut}): {e}"))?;
    let screenshot: Shortcut = screenshot_shortcut
        .parse()
        .map_err(|e| format!("Atajo de captura inválido ({screenshot_shortcut}): {e}"))?;

    // Ningún par puede coincidir.
    let named = [
        ("grabación", recording),
        ("dictado", dictation),
        ("traer pill", summon),
        ("captura", screenshot),
    ];
    for i in 0..named.len() {
        for j in (i + 1)..named.len() {
            if named[i].1 == named[j].1 {
                return Err(format!(
                    "Los atajos de {} y {} no pueden coincidir.",
                    named[i].0, named[j].0
                ));
            }
        }
    }

    let gs = app.global_shortcut();
    if let Err(err) = gs.unregister_all() {
        tracing::debug!(%err, "unregister_all (puede estar vacío)");
    }

    let handle = app.clone();
    if let Err(err) = gs.on_shortcut(recording, move |_app, _sc, event| {
        if matches!(event.state(), ShortcutState::Pressed) {
            state::toggle_recording(&handle);
        }
    }) {
        tracing::error!(%err, "no se pudo registrar el atajo de grabación");
    }

    let handle = app.clone();
    if let Err(err) = gs.on_shortcut(dictation, move |app, _sc, event| {
        let mode = app
            .try_state::<state::AppState>()
            .map(|s| s.config.lock().unwrap().dictation_mode.clone())
            .unwrap_or_else(|| "push_to_talk".into());

        match (mode.as_str(), event.state()) {
            ("push_to_talk", ShortcutState::Pressed) => dictation::dictation_key_down(&handle),
            ("push_to_talk", ShortcutState::Released) => dictation::dictation_key_up(&handle),
            (_, ShortcutState::Pressed) => dictation::toggle_dictation(&handle),
            _ => {}
        }
    }) {
        tracing::error!(%err, "no se pudo registrar el atajo de dictado");
    }

    let handle = app.clone();
    if let Err(err) = gs.on_shortcut(summon, move |_app, _sc, event| {
        if matches!(event.state(), ShortcutState::Pressed) {
            state::summon_pill_to_cursor(&handle);
        }
    }) {
        tracing::error!(%err, "no se pudo registrar el atajo de traer pill");
    }

    let handle = app.clone();
    if let Err(err) = gs.on_shortcut(screenshot, move |_app, _sc, event| {
        if matches!(event.state(), ShortcutState::Pressed) {
            if let Err(error) = crate::capture_session::trigger(&handle) {
                tracing::warn!(%error, "no se pudo abrir el overlay de captura");
            }
        }
    }) {
        tracing::error!(%err, "no se pudo registrar el atajo de captura");
    }

    Ok(())
}
