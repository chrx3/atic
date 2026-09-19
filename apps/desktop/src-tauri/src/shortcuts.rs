//! Registro de atajos globales (grabación + dictado + pill + clipboard + fragmentos + agentes + captura + launcher).
//!
//! Teclado: `tauri-plugin-global-shortcut`.
//! Botones laterales del mouse: Raw Input (ver `mouse_bindings`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::mouse_bindings::{self, MouseAction, SideButton};
use crate::{clipboard_history, dictation, launcher, state};
use atic_core::MutexExt;

/// La UI está capturando un atajo (Ajustes): los globales quedan desregistrados
/// a propósito y ningún registro nuevo debe armarlos hasta terminar.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Primer `Pressed` de un chord. El auto-repeat de Windows reenvía Pressed
/// mientras se sostiene: sin esto, clipboard/launcher abren y se cierran solos.
fn take_key_press(held: &AtomicBool, state: ShortcutState) -> bool {
    match state {
        ShortcutState::Pressed => !held.swap(true, Ordering::SeqCst),
        ShortcutState::Released => {
            held.store(false, Ordering::Release);
            false
        }
    }
}

enum Binding {
    Key(Shortcut),
    Mouse(SideButton),
}

fn parse_binding(en: bool, name: &str, raw: &str) -> Result<Binding, String> {
    if let Some(btn) = mouse_bindings::parse_side_button(raw) {
        return Ok(Binding::Mouse(btn));
    }
    raw.parse::<Shortcut>().map(Binding::Key).map_err(|e| {
        if en {
            format!("Invalid {name} shortcut ({raw}): {e}")
        } else {
            format!("Atajo de {name} inválido ({raw}): {e}")
        }
    })
}

fn binding_dup_key(b: &Binding) -> String {
    match b {
        Binding::Key(sc) => format!("key:{sc:?}"),
        Binding::Mouse(SideButton::X1) => "mouse:x1".into(),
        Binding::Mouse(SideButton::X2) => "mouse:x2".into(),
    }
}

/// `None` si otro comando ya registra ese atajo. El SO admite un solo registro
/// por chord: el repetido queda inactivo y la UI lo marca compartido.
fn active<'a>(skipped: &HashSet<String>, key: &str, binding: &'a Binding) -> Option<&'a Binding> {
    if skipped.contains(key) {
        None
    } else {
        Some(binding)
    }
}

/// Grupos de claves de config que comparten el mismo atajo. Los usa la UI para
/// marcarlos en rojo; acá se registra solo el primero de cada grupo.
fn shared_groups(named: &[(&str, &Binding)]) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut group_of: HashMap<String, usize> = HashMap::new();
    for (key, binding) in named {
        match group_of.entry(binding_dup_key(binding)) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(groups.len());
                groups.push(vec![(*key).to_string()]);
            }
            std::collections::hash_map::Entry::Occupied(slot) => {
                groups[*slot.get()].push((*key).to_string());
            }
        }
    }
    groups.retain(|group| group.len() > 1);
    groups
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
    pub board: &'a str,
    pub color: &'a str,
    pub launcher: &'a str,
    pub window_flip: &'a str,
}

/// Los errores de *sintaxis* de cualquier atajo abortan (se valida antes de
/// persistir en `set_config`). En cambio, un fallo al **registrar** un atajo
/// concreto (p. ej. conflicto con otra app) no impide registrar los demás: un
/// conflicto de captura no debe desactivar grabación, dictado ni pill.
///
/// Esos fallos se acumulan en [`AppState::shortcut_failures`] y se emiten como
/// `shortcuts-failed`, para que el usuario pueda reasignarlos: un atajo que el
/// SO rechazó es indistinguible de uno roto si solo queda en el log.
///
/// Un atajo **repetido** entre comandos ya no aborta: se registra el primero y
/// el resto se emite como `shortcuts-shared` para que la UI los marque. Así se
/// puede mover un atajo de un comando a otro sin desasignar y reasignar.
pub fn register_shortcuts(app: &AppHandle, bindings: ShortcutBindings<'_>) -> Result<(), String> {
    let en = crate::ui_lang::english();
    let n = |es, english| crate::ui_lang::pick(en, es, english);
    let recording = parse_binding(en, n("grabación", "recording"), bindings.recording)?;
    let dictation = parse_binding(en, n("dictado", "dictation"), bindings.dictation)?;
    let summon = parse_binding(en, n("traer pill", "bring pill"), bindings.summon_pill)?;
    let radial = parse_binding(
        en,
        n("rueda de la pill", "pill wheel"),
        bindings.pill_radial,
    )?;
    let clipboard = parse_binding(en, "clipboard", bindings.clipboard)?;
    let snippets = parse_binding(en, n("fragmentos", "snippets"), bindings.snippets)?;
    let agents = if crate::agents::UI_ENABLED {
        Some(parse_binding(en, n("agentes", "agents"), bindings.agents)?)
    } else {
        None
    };
    let screenshot = parse_binding(en, n("captura", "capture"), bindings.screenshot)?;
    let board = parse_binding(en, n("pizarra", "board"), bindings.board)?;
    let color = parse_binding(en, n("color", "color"), bindings.color)?;
    let launcher_bind = parse_binding(en, "launcher", bindings.launcher)?;
    let window_flip = parse_binding(
        en,
        n("voltear ventana", "flip window"),
        bindings.window_flip,
    )?;

    // Captura en curso: se valida igual (los parse de arriba ya rechazan un
    // atajo inválido antes de persistir), pero el registro espera. Al terminar,
    // la captura re-registra desde la config que ya quedó guardada.
    if CAPTURING.load(Ordering::SeqCst) {
        tracing::debug!("captura de atajo en curso: registro postergado");
        return Ok(());
    }

    // Claves de config por binding. La UI marca las filas con esto: los nombres
    // visibles dependen del idioma y no cruzan bien la frontera.
    let mut named: Vec<(&str, &Binding)> = vec![
        ("global_shortcut", &recording),
        ("dictation_shortcut", &dictation),
        ("summon_pill_shortcut", &summon),
        ("pill_radial_shortcut", &radial),
        ("clipboard_shortcut", &clipboard),
        ("snippets_shortcut", &snippets),
        ("screenshot_shortcut", &screenshot),
        ("board_shortcut", &board),
        ("color_shortcut", &color),
        ("launcher_shortcut", &launcher_bind),
        ("window_flip_shortcut", &window_flip),
    ];
    if let Some(ref agents) = agents {
        named.push(("agents_shortcut", agents));
    }

    // Un atajo repetido no aborta el guardado: se registra el primero y el
    // resto se marca compartido en la UI. Rechazarlo obligaba a desasignar y
    // reasignar para poder mover un atajo entre comandos: dos pasos de más.
    let shared = shared_groups(&named);
    // El primero de cada grupo queda activo. Al resto ni se intenta registrar:
    // el SO rechaza el segundo registro del mismo chord y no hay acción doble.
    let skipped: HashSet<String> = shared
        .iter()
        .flat_map(|group| group.iter().skip(1).cloned())
        .collect();

    let gs = app.global_shortcut();
    if let Err(err) = gs.unregister_all() {
        tracing::debug!(%err, "unregister_all (puede estar vacío)");
    }

    let mut mouse: Vec<(SideButton, MouseAction)> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    match active(&skipped, "global_shortcut", &recording) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    state::toggle_recording(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de grabación");
                failed.push("grabación".to_string());
            }
        }
        Some(Binding::Mouse(btn)) => mouse.push((*btn, MouseAction::Recording)),
        None => {}
    }

    match active(&skipped, "dictation_shortcut", &dictation) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |app, _sc, event| {
                let mode = app
                    .try_state::<state::AppState>()
                    .map(|s| s.config.lock_or_recover().dictation_mode.clone())
                    .unwrap_or_else(|| "push_to_talk".into());

                match (mode.as_str(), event.state()) {
                    ("push_to_talk", ShortcutState::Pressed) => {
                        if take_key_press(&held, ShortcutState::Pressed) {
                            dictation_ptt_down_via_slot(&handle);
                        }
                    }
                    ("push_to_talk", ShortcutState::Released) => {
                        let _ = take_key_press(&held, ShortcutState::Released);
                        dictation::dictation_key_up(&handle);
                    }
                    (_, ShortcutState::Pressed) => {
                        if take_key_press(&held, ShortcutState::Pressed) {
                            dictation_toggle_via_slot(&handle);
                        }
                    }
                    (_, ShortcutState::Released) => {
                        let _ = take_key_press(&held, ShortcutState::Released);
                    }
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de dictado");
                failed.push("dictado".to_string());
            }
        }
        Some(Binding::Mouse(btn)) => mouse.push((*btn, MouseAction::Dictation)),
        None => {}
    }

    match active(&skipped, "summon_pill_shortcut", &summon) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    state::summon_pill_to_cursor(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de traer pill");
                failed.push("traer pill".to_string());
            }
        }
        Some(Binding::Mouse(btn)) => mouse.push((*btn, MouseAction::SummonPill)),
        None => {}
    }

    match active(&skipped, "pill_radial_shortcut", &radial) {
        Some(Binding::Key(sc)) => {
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
        Some(Binding::Mouse(_)) => {
            tracing::warn!("la rueda de la pill solo admite atajo de teclado");
        }
        None => {}
    }

    match active(&skipped, "clipboard_shortcut", &clipboard) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    clipboard_history::remember_paste_target();
                    emit_tool_slot(&handle, "activate-tool-slot", "clipboard");
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de clipboard");
                failed.push("clipboard".to_string());
            }
        }
        Some(Binding::Mouse(btn)) => mouse.push((*btn, MouseAction::Clipboard)),
        None => {}
    }

    match active(&skipped, "snippets_shortcut", &snippets) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    clipboard_history::remember_paste_target();
                    emit_tool_slot(&handle, "activate-tool-slot", "snippets");
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de fragmentos");
                failed.push("fragmentos".to_string());
            }
        }
        Some(Binding::Mouse(btn)) => mouse.push((*btn, MouseAction::Snippets)),
        None => {}
    }

    if let Some(agents) = &agents {
        match active(&skipped, "agents_shortcut", agents) {
            Some(Binding::Key(sc)) => {
                let handle = app.clone();
                let held = AtomicBool::new(false);
                if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                    if take_key_press(&held, event.state()) {
                        emit_tool_slot(&handle, "activate-tool-slot", "agents");
                    }
                }) {
                    tracing::error!(%err, "no se pudo registrar el atajo de agentes");
                    failed.push("agentes".to_string());
                }
            }
            Some(Binding::Mouse(_)) => {
                tracing::warn!("la consola de agentes solo admite atajo de teclado");
            }
            None => {}
        }
    }

    match active(&skipped, "screenshot_shortcut", &screenshot) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    if let Err(error) = crate::capture_session::trigger(&handle) {
                        tracing::warn!(%error, "no se pudo abrir el overlay de captura");
                    }
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de captura");
                failed.push("captura".to_string());
            }
        }
        Some(Binding::Mouse(btn)) => mouse.push((*btn, MouseAction::Screenshot)),
        None => {}
    }

    match active(&skipped, "board_shortcut", &board) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    if let Err(error) = crate::annotate::toggle_board(&handle) {
                        tracing::warn!(%error, "no se pudo abrir la pizarra");
                    }
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de la pizarra");
                failed.push("pizarra".to_string());
            }
        }
        Some(Binding::Mouse(_)) => {
            tracing::warn!("la pizarra solo admite atajo de teclado");
        }
        None => {}
    }

    match active(&skipped, "color_shortcut", &color) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    if let Err(error) = crate::color_picker::trigger(&handle) {
                        tracing::warn!(%error, "no se pudo abrir el cuentagotas");
                    }
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de color");
                failed.push("color".to_string());
            }
        }
        Some(Binding::Mouse(_)) => {
            tracing::warn!("el cuentagotas solo admite atajo de teclado");
        }
        None => {}
    }

    match active(&skipped, "launcher_shortcut", &launcher_bind) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    launcher::toggle_via_slot(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo del launcher");
                failed.push("launcher".to_string());
            }
        }
        Some(Binding::Mouse(_)) => {
            tracing::warn!("el launcher solo admite atajo de teclado");
        }
        None => {}
    }

    match active(&skipped, "window_flip_shortcut", &window_flip) {
        Some(Binding::Key(sc)) => {
            let handle = app.clone();
            let held = AtomicBool::new(false);
            if let Err(err) = gs.on_shortcut(*sc, move |_app, _sc, event| {
                if take_key_press(&held, event.state()) {
                    crate::window_flip::toggle(&handle);
                }
            }) {
                tracing::error!(%err, "no se pudo registrar el atajo de voltear ventana");
                failed.push("voltear ventana".to_string());
            }
        }
        Some(Binding::Mouse(_)) => {
            tracing::warn!("voltear ventana solo admite atajo de teclado");
        }
        None => {}
    }

    mouse_bindings::set_bindings(app, mouse);

    if let Some(app_state) = app.try_state::<state::AppState>() {
        *app_state.shortcut_failures.lock_or_recover() = failed.clone();
        *app_state.shortcut_shared.lock_or_recover() = shared.clone();
    }
    // Siempre se emite, también vacío: así la UI puede limpiar un aviso previo
    // cuando el usuario reasigna el atajo en conflicto.
    let _ = app.emit("shortcuts-failed", failed);
    let _ = app.emit("shortcuts-shared", shared);

    Ok(())
}

/// Atajos que el SO rechazó en el último registro (para la UI).
#[tauri::command]
pub fn failed_shortcuts(state: tauri::State<state::AppState>) -> Vec<String> {
    state.shortcut_failures.lock_or_recover().clone()
}

/// Grupos de claves de config cuyo atajo comparten dos o más comandos.
#[tauri::command]
pub fn shared_shortcuts(state: tauri::State<state::AppState>) -> Vec<Vec<String>> {
    state.shortcut_shared.lock_or_recover().clone()
}

/// Desregistra todos los globales mientras la UI captura un atajo nuevo.
///
/// Sin esto, apretar el atajo deseado dispara la herramienta que ya lo tenía:
/// el registro en el SO no distingue «estoy configurando» de «quiero usarlo».
#[tauri::command]
pub fn suspend_shortcuts(app: AppHandle) {
    CAPTURING.store(true, Ordering::SeqCst);
    if let Err(err) = app.global_shortcut().unregister_all() {
        tracing::debug!(%err, "unregister_all al capturar (puede estar vacío)");
    }
    // Los laterales del mouse se observan siempre: vaciar los bindings evita
    // que el mismo pulsado capture y además dispare la acción ya asignada.
    mouse_bindings::set_bindings(&app, Vec::new());
}

/// Re-registra los globales al terminar la captura. La config pudo cambiar
/// mientras se capturaba, así que se relee en vez de reusar bindings.
#[tauri::command]
pub fn resume_shortcuts(app: AppHandle) -> Result<(), String> {
    CAPTURING.store(false, Ordering::SeqCst);
    reregister_from_config(&app)
}

/// Registra todos los globales con la config vigente.
pub fn reregister_from_config(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<state::AppState>() else {
        return Ok(());
    };
    let cfg = state.config.lock_or_recover().clone();
    register_shortcuts(
        app,
        ShortcutBindings {
            recording: &cfg.global_shortcut,
            dictation: &cfg.dictation_shortcut,
            summon_pill: &cfg.summon_pill_shortcut,
            pill_radial: &cfg.pill_radial_shortcut,
            clipboard: &cfg.clipboard_shortcut,
            snippets: &cfg.snippets_shortcut,
            agents: &cfg.agents_shortcut,
            screenshot: &cfg.screenshot_shortcut,
            board: &cfg.board_shortcut,
            color: &cfg.color_shortcut,
            launcher: &cfg.launcher_shortcut,
            window_flip: &cfg.window_flip_shortcut,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_key_press_ignores_repeat_until_release() {
        let held = AtomicBool::new(false);
        assert!(take_key_press(&held, ShortcutState::Pressed));
        assert!(!take_key_press(&held, ShortcutState::Pressed));
        assert!(!take_key_press(&held, ShortcutState::Pressed));
        assert!(!take_key_press(&held, ShortcutState::Released));
        assert!(take_key_press(&held, ShortcutState::Pressed));
    }

    #[test]
    fn shared_groups_junta_solo_los_repetidos() {
        let alt_q: Binding = Binding::Key("Alt+Q".parse().unwrap());
        let alt_q_again: Binding = Binding::Key("Alt+Q".parse().unwrap());
        let alt_z: Binding = Binding::Key("Alt+Z".parse().unwrap());
        let named = vec![
            ("dictation_shortcut", &alt_q),
            ("clipboard_shortcut", &alt_z),
            ("snippets_shortcut", &alt_q_again),
        ];
        assert_eq!(
            shared_groups(&named),
            vec![vec![
                "dictation_shortcut".to_string(),
                "snippets_shortcut".to_string()
            ]]
        );
    }

    #[test]
    fn shared_groups_tambien_mira_los_botones_del_mouse() {
        let key: Binding = Binding::Key("Alt+Z".parse().unwrap());
        let x1: Binding = Binding::Mouse(SideButton::X1);
        let x1_again: Binding = Binding::Mouse(SideButton::X1);
        let named = vec![
            ("dictation_shortcut", &x1),
            ("clipboard_shortcut", &key),
            ("snippets_shortcut", &x1_again),
        ];
        assert_eq!(
            shared_groups(&named),
            vec![vec![
                "dictation_shortcut".to_string(),
                "snippets_shortcut".to_string()
            ]]
        );
    }
}
