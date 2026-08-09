//! Registro de atajos globales (grabación + dictado + pill + clipboard + fragmentos + agentes + captura + launcher).
//!
//! Teclado: `tauri-plugin-global-shortcut`.
//! Botones laterales del mouse: Raw Input (ver `mouse_bindings`).

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::mouse_bindings::{self, MouseAction, SideButton};
use crate::{clipboard_history, dictation, launcher, state};
use atic_core::MutexExt;

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

fn dictation_listening(app: &AppHandle) -> bool {
    app.try_state::<state::AppState>()
        .map(|s| s.dictation.lock_or_recover().is_some())
        .unwrap_or(false)
}

/// Asegura overlay visible y pide al front el pipeline de slot.
pub fn emit_tool_slot(app: &AppHandle, event: &str, tool: &str) {
    if let Some(pill) = app.get_webview_window(crate::overlay::LABEL) {
        let visible = app
            .try_state::<state::AppState>()
            .map(|s| s.config.lock_or_recover().show_pill)
            .unwrap_or(true);
        if visible {
            let _ = pill.set_always_on_top(true);
            let _ = pill.show();
        }
    }
    let _ = app.emit(event, tool);
}

/// Dictado toggle: al empezar, fly+activar vía overlay; al parar, stop directo.
pub fn dictation_toggle_via_slot(app: &AppHandle) {
    if dictation_listening(app) {
        dictation::toggle_dictation(app);
    } else {
        emit_tool_slot(app, "activate-tool-slot", "dictation");
    }
}

/// PTT down: vuela en paralelo y arranca ya (latencia del mic).
pub fn dictation_ptt_down_via_slot(app: &AppHandle) {
    if !dictation_listening(app) {
        emit_tool_slot(app, "fly-tool-slot", "dictation");
    }
    dictation::dictation_key_down(app);
}

/// Registra (o re-registra) los atajos globales.
///
/// Los atajos globales que registra la app.
///
/// Van agrupados y no como parámetros sueltos: eran varios `&str` del mismo
/// tipo en fila, así que intercambiar dos por error compilaba perfecto y el
/// bug recién aparecía al usar el atajo equivocado. Con campos nombrados,
/// eso no pasa.
pub struct ShortcutBindings<'a> {
    pub recording: &'a str,
    pub dictation: &'a str,
    pub summon_pill: &'a str,
    pub pill_radial: &'a str,
    pub clipboard: &'a str,
    pub snippets: &'a str,
    pub agents: &'a str,
    pub screenshot: &'a str,
    pub launcher: &'a str,
}

/// Los errores de *sintaxis* de cualquier atajo abortan (se valida antes de
/// persistir en `set_config`). En cambio, un fallo al **registrar** un atajo
/// concreto (p. ej. conflicto con otra app) no impide registrar los demás: un
/// conflicto de captura no debe desactivar grabación, dictado ni pill.
///
/// Esos fallos se acumulan en [`AppState::shortcut_failures`] y se emiten como
/// `shortcuts-failed`, para que el usuario pueda reasignarlos: un atajo que el
/// SO rechazó es indistinguible de uno roto si solo queda en el log.
pub fn register_shortcuts(app: &AppHandle, bindings: ShortcutBindings<'_>) -> Result<(), String> {
    let recording = parse_binding("grabación", bindings.recording)?;
    let dictation = parse_binding("dictado", bindings.dictation)?;
    let summon = parse_binding("traer pill", bindings.summon_pill)?;
    let radial = parse_binding("rueda de la pill", bindings.pill_radial)?;
    let clipboard = parse_binding("clipboard", bindings.clipboard)?;
    let snippets = parse_binding("fragmentos", bindings.snippets)?;
    let agents = parse_binding("agentes", bindings.agents)?;
    let screenshot = parse_binding("captura", bindings.screenshot)?;
    let launcher_bind = parse_binding("launcher", bindings.launcher)?;

    let named = [
        ("grabación", &recording),
        ("dictado", &dictation),
        ("traer pill", &summon),
        ("rueda de la pill", &radial),
        ("clipboard", &clipboard),
        ("fragmentos", &snippets),
        ("agentes", &agents),
        ("captura", &screenshot),
        ("launcher", &launcher_bind),
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
    let mut failed: Vec<String> = Vec::new();

    match &recording {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    state::toggle_recording(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de grabación");
                failed.push("grabación".to_string());
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
                    .map(|s| s.config.lock_or_recover().dictation_mode.clone())
                    .unwrap_or_else(|| "push_to_talk".into());

                match (mode.as_str(), event.state()) {
                    ("push_to_talk", ShortcutState::Pressed) => {
                        dictation_ptt_down_via_slot(&handle)
                    }
                    ("push_to_talk", ShortcutState::Released) => {
                        dictation::dictation_key_up(&handle)
                    }
                    (_, ShortcutState::Pressed) => dictation_toggle_via_slot(&handle),
                    _ => {}
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de dictado");
                failed.push("dictado".to_string());
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
                failed.push("traer pill".to_string());
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::SummonPill)),
    }

    match &radial {
        Binding::Key(sc) => {
            let handle = app.clone();
            // Mantener-para-abrir: Pressed abre, Released activa y cierra. El
            // auto-repeat del SO reenvía Pressed mientras se sostiene, pero el
            // front ignora las repeticiones (openRadial es idempotente).
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                // La pill vive dentro del overlay: los eventos van ahí.
                let Some(pill) = handle.get_webview_window(crate::overlay::LABEL) else {
                    return;
                };
                match event.state() {
                    ShortcutState::Pressed => {
                        // La rueda ES la pill: si el usuario la ocultó, no
                        // resucitarla a la fuerza (nada volvía a esconderla y
                        // la config quedaba mintiendo).
                        let visible = handle
                            .try_state::<state::AppState>()
                            .map(|s| s.config.lock_or_recover().show_pill)
                            .unwrap_or(true);
                        if !visible {
                            return;
                        }
                        // Guardar el destino de pegado ANTES del set_focus del
                        // front: si no, clipboard/fragmentos abiertos desde la
                        // rueda pegan en la ventana equivocada (o encolan).
                        clipboard_history::remember_paste_target();
                        let _ = pill.set_always_on_top(true);
                        let _ = pill.show();
                        let _ = pill.emit("pill-radial-press", ());
                    }
                    ShortcutState::Released => {
                        let _ = pill.emit("pill-radial-release", ());
                    }
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de la rueda");
                failed.push("rueda de herramientas".to_string());
            }
        }
        Binding::Mouse(_) => {
            tracing::warn!("la rueda de la pill solo admite atajo de teclado");
        }
    }

    match &clipboard {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    // Pill → cursor → float (mismo pipeline que dictado/launcher).
                    clipboard_history::remember_paste_target();
                    emit_tool_slot(&handle, "activate-tool-slot", "clipboard");
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de clipboard");
                failed.push("clipboard".to_string());
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Clipboard)),
    }

    match &snippets {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    clipboard_history::remember_paste_target();
                    emit_tool_slot(&handle, "activate-tool-slot", "snippets");
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de fragmentos");
                failed.push("fragmentos".to_string());
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Snippets)),
    }

    match &agents {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    // `show_agents_window` ya es toggle (panel_float).
                    crate::agents::bridge::show_agents_window(handle.clone());
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de agentes");
                failed.push("agentes".to_string());
            }
        }
        Binding::Mouse(_) => {
            tracing::warn!("la consola de agentes solo admite atajo de teclado");
        }
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
                failed.push("captura".to_string());
            }
        }
        Binding::Mouse(btn) => mouse.push((*btn, MouseAction::Screenshot)),
    }

    match &launcher_bind {
        Binding::Key(sc) => {
            let handle = app.clone();
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if matches!(event.state(), ShortcutState::Pressed) {
                    launcher::toggle_via_slot(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo del launcher");
                failed.push("launcher".to_string());
            }
        }
        Binding::Mouse(_) => {
            tracing::warn!("el launcher solo admite atajo de teclado");
        }
    }

    mouse_bindings::set_bindings(app, mouse);

    if let Some(app_state) = app.try_state::<state::AppState>() {
        *app_state.shortcut_failures.lock_or_recover() = failed.clone();
    }
    // Siempre se emite, también vacío: así la UI puede limpiar un aviso previo
    // cuando el usuario reasigna el atajo en conflicto.
    let _ = app.emit("shortcuts-failed", failed);

    Ok(())
}

/// Atajos que el SO rechazó en el último registro (para la UI).
#[tauri::command]
pub fn failed_shortcuts(state: tauri::State<state::AppState>) -> Vec<String> {
    state.shortcut_failures.lock_or_recover().clone()
}
