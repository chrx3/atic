//! Registro de atajos globales (grabación + dictado + pill + clipboard + fragmentos + captura).
//!
//! Teclado: `tauri-plugin-global-shortcut`.
//! Botones laterales del mouse: Raw Input (ver `mouse_bindings`).

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::mouse_bindings::{self, MouseAction, SideButton};
use crate::{clipboard_history, dictation, snippets, state};

enum Binding {
    Key(Shortcut),
    Mouse(SideButton),
}

fn parse_binding(name: &str, raw: &str) -> Result<Binding, String> {
    if let Some(btn) = mouse_bindings::parse_side_button(raw) {
        return Ok(Binding::Mouse(btn));
    }
    raw.parse::<Shortcut>()
        .map(Binding::Key)
        .map_err(|e| format!("Atajo de {name} inválido ({raw}): {e}"))
}

fn binding_dup_key(b: &Binding) -> String {
    match b {
        Binding::Key(sc) => format!("key:{sc:?}"),
        Binding::Mouse(SideButton::X1) => "mouse:x1".into(),
        Binding::Mouse(SideButton::X2) => "mouse:x2".into(),
    }
}

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
    clipboard_shortcut: &str,
    snippets_shortcut: &str,
    screenshot_shortcut: &str,
) -> Result<(), String> {
    let recording = parse_binding("grabación", recording_shortcut)?;
    let dictation = parse_binding("dictado", dictation_shortcut)?;
    let summon = parse_binding("traer pill", summon_pill_shortcut)?;
    let clipboard = parse_binding("clipboard", clipboard_shortcut)?;
    let snippets = parse_binding("fragmentos", snippets_shortcut)?;
    let screenshot = parse_binding("captura", screenshot_shortcut)?;

    let named = [
        ("grabación", &recording),
        ("dictado", &dictation),
        ("traer pill", &summon),
        ("clipboard", &clipboard),
        ("fragmentos", &snippets),
        ("captura", &screenshot),
    ];
    for i in 0..named.len() {
        for j in (i + 1)..named.len() {
            if binding_dup_key(named[i].1) == binding_dup_key(named[j].1) {
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

    let mut mouse: Vec<(SideButton, MouseAction)> = Vec::new();

    match &recording {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    state::toggle_recording(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de grabación");
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Recording)),
    }

    match &dictation {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |app, _sc, event| {
                let mode = app
                    .try_state::<state::AppState>()
                    .map(|s| s.config.lock().unwrap().dictation_mode.clone())
                    .unwrap_or_else(|| "push_to_talk".into());

                match (mode.as_str(), event.state()) {
                    ("push_to_talk", ShortcutState::Pressed) => {
                        dictation::dictation_key_down(&handle)
                    }
                    ("push_to_talk", ShortcutState::Released) => {
                        dictation::dictation_key_up(&handle)
                    }
                    (_, ShortcutState::Pressed) => dictation::toggle_dictation(&handle),
                    _ => {}
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de dictado");
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Dictation)),
    }

    match &summon {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    state::summon_pill_to_cursor(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de traer pill");
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::SummonPill)),
    }

    match &clipboard {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    clipboard_history::summon_clipboard_panel(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de clipboard");
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Clipboard)),
    }

    match &snippets {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    snippets::summon_snippets_panel(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de fragmentos");
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Snippets)),
    }

    match &screenshot {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    if let Err(error) = crate::capture_session::trigger(&handle) {
                        tracing::warn!(%error, "no se pudo abrir el overlay de captura");
                    }
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de captura");
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Screenshot)),
    }

    mouse_bindings::set_bindings(app, mouse);

    // unregister_all() arriba quita Esc temporal del clipboard; reponerlo si sigue abierto.
    if app
        .try_state::<state::AppState>()
        .is_some_and(|s| s.pre_clipboard_position.lock().unwrap().is_some())
    {
        clipboard_history::register_clipboard_escape_close(app);
    }

    Ok(())
}
