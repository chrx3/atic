//! Panel de sistema: recursos, audio, pantallas y cierre de apps.
//!
//! El overlay hospeda la UI. Acá vive el SO: snapshot, volumen, brillo y
//! lock/sleep/mute/trash vía [`crate::system_actions`].

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use atic_core::MutexExt;

use crate::state::AppState;

// Solo Windows: resolver la ruta del `.exe` y pedirle el ícono al shell es
// Win32 + COM. En macOS la pestaña de recursos no muestra íconos.
#[cfg(windows)]
mod app_icons;
mod apps;
mod audio;
mod awake;
mod display;
mod snapshot;
mod watch;

pub use snapshot::can_control_stem;
pub use watch::start as start_watch;

/// Al cerrar la app: soltar el café si quedó puesto.
pub fn shutdown() {
    awake::release();
}

static SYSTEM_OPEN: AtomicBool = AtomicBool::new(false);
static SYSTEM_ALWAYS_ON_TOP: AtomicBool = AtomicBool::new(false);

const SYS_ANCHOR: &str = "system-bubble-anchor";
const SYS_DISMISS: &str = "system-bubble-dismiss";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemApp {
    pub id: String,
    pub name: String,
    /// Ícono de la app como data URL. Falta cuando no se puede sacar del
    /// ejecutable —o cuando todavía no le tocó el turno al presupuesto por
    /// vuelta—: la fila reserva el mismo hueco igual.
    pub icon: Option<String>,
    pub pid: u32,
    pub cpu: f32,
    pub ram_bytes: u64,
    pub can_close: bool,
    pub can_force: bool,
    /// Se puede traer al frente: tiene ventana. Un demonio no.
    pub can_focus: bool,
    /// Sin ícono ni ventana (`node`, `cargo`, helpers). La vista los esconde
    /// detrás de un interruptor: son ruido hasta que los buscas.
    pub background: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemSnapshot {
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub apps: Vec<SystemApp>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioSession {
    pub id: String,
    pub name: String,
    /// Ícono de la app como data URL. Falta cuando no se puede sacar del
    /// ejecutable: la fila queda igual, con el hueco reservado.
    pub icon: Option<String>,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemAudio {
    pub volume: f32,
    pub muted: bool,
    pub per_app: bool,
    pub sessions: Vec<AudioSession>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemDisplay {
    pub id: String,
    pub name: String,
    pub primary: bool,
    pub brightness: Option<f32>,
}

#[allow(dead_code)]
pub fn float_open() -> bool {
    SYSTEM_OPEN.load(Ordering::Relaxed)
}

pub fn float_always_on_top() -> bool {
    SYSTEM_ALWAYS_ON_TOP.load(Ordering::Relaxed)
}

pub fn init_always_on_top(on: bool) {
    SYSTEM_ALWAYS_ON_TOP.store(on, Ordering::Relaxed);
}

/// Forma del float de sistema.
///
/// Más alto que el panel genérico (clipboard, textos): acá arriba de la lista
/// viven la fila rápida, las pestañas, dos medidores, el orden y el filtro. Con
/// los 372 px del panel común, la lista de apps quedaba en dos filas.
const SYS_SHAPE: crate::floating::BubbleShape = crate::floating::BubbleShape {
    w: 320,
    h: 470,
    gap: 10,
    corner: 18,
};

pub fn summon_system_panel(app: &AppHandle) {
    tracing::info!(target: "overlay", "show system float");
    crate::panel_float::show(app, &SYSTEM_OPEN, SYS_SHAPE, SYS_ANCHOR);
    crate::overlay::set_topmost(app, crate::agents::bridge::overlay_should_be_topmost());
}

#[tauri::command]
pub fn show_system_window(app: AppHandle) {
    summon_system_panel(&app);
}

#[tauri::command]
pub fn hide_system_window(app: AppHandle) {
    crate::panel_float::hide(&app, &SYSTEM_OPEN, SYS_DISMISS);
    crate::overlay::set_topmost(&app, crate::agents::bridge::overlay_should_be_topmost());
}

#[tauri::command]
pub fn system_always_on_top() -> bool {
    float_always_on_top()
}

#[tauri::command]
pub fn set_system_always_on_top(app: AppHandle, on: bool) {
    SYSTEM_ALWAYS_ON_TOP.store(on, Ordering::Relaxed);
    if let Some(state) = app.try_state::<AppState>() {
        let snapshot = {
            let Ok(mut cfg) = state.config.lock() else {
                crate::overlay::set_topmost(
                    &app,
                    crate::agents::bridge::overlay_should_be_topmost(),
                );
                return;
            };
            cfg.system_always_on_top = on;
            cfg.clone()
        };
        let _ = snapshot.save(&state.dirs.config_path());
    }
    crate::overlay::set_topmost(&app, crate::agents::bridge::overlay_should_be_topmost());
}

#[tauri::command]
pub fn system_snapshot() -> Result<SystemSnapshot, String> {
    snapshot::read()
}

#[tauri::command]
pub fn system_audio() -> Result<SystemAudio, String> {
    audio::read()
}

#[tauri::command]
pub fn system_set_volume(volume: f32) -> Result<(), String> {
    audio::set_master(volume.clamp(0.0, 1.0))
}

#[tauri::command]
pub fn system_set_muted(muted: bool) -> Result<(), String> {
    audio::set_muted(muted)
}

#[tauri::command]
pub fn system_set_session_volume(id: String, volume: f32) -> Result<(), String> {
    audio::set_session(&id, volume.clamp(0.0, 1.0))
}

#[tauri::command]
pub fn system_displays() -> Result<Vec<SystemDisplay>, String> {
    display::list()
}

#[tauri::command]
pub fn system_set_brightness(id: String, brightness: f32) -> Result<(), String> {
    display::set_brightness(&id, brightness.clamp(0.0, 1.0))
}

#[tauri::command]
pub fn system_close_app(id: String) -> Result<u32, String> {
    apps::close(&id)
}

#[tauri::command]
pub fn system_force_app(id: String) -> Result<(), String> {
    apps::force(&id)
}

/// Trae la app al frente. Es lo que uno quiere hacer con una lista de apps
/// nueve de cada diez veces; cerrarla es la excepción.
#[tauri::command]
pub fn system_focus_app(id: String) -> Result<(), String> {
    apps::focus(&id)
}

/// ¿El equipo está retenido despierto?
#[tauri::command]
pub fn system_awake() -> bool {
    awake::is_on()
}

/// Café: mientras esté puesto, el equipo no se duerme ni apaga la pantalla.
#[tauri::command]
pub fn set_system_awake(on: bool) -> Result<(), String> {
    awake::set(on)
}

/// Avisos encendidos ahora mismo (la pill los pide al montarse).
#[tauri::command]
pub fn system_alerts() -> Vec<watch::SystemAlert> {
    watch::active()
}

/// Umbrales de los avisos, tal como están guardados.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSettings {
    pub enabled: bool,
    pub cpu: u8,
    pub ram: u8,
    pub seconds: u16,
}

#[tauri::command]
pub fn system_alert_settings(app: AppHandle) -> Result<AlertSettings, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "sin estado".to_string())?;
    let cfg = state.config.lock_or_recover();
    Ok(AlertSettings {
        enabled: cfg.system_alerts,
        cpu: cfg.system_alert_cpu,
        ram: cfg.system_alert_ram,
        seconds: cfg.system_alert_seconds,
    })
}

/// Guarda los umbrales. El vigilante los relee en el siguiente latido.
#[tauri::command]
pub fn set_system_alert_settings(app: AppHandle, settings: AlertSettings) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "sin estado".to_string())?;
    let snapshot = {
        let mut cfg = state.config.lock_or_recover();
        cfg.system_alerts = settings.enabled;
        // 0 = no vigilar esa métrica; el resto se acota a algo que pueda pasar.
        cfg.system_alert_cpu = if settings.cpu == 0 {
            0
        } else {
            settings.cpu.clamp(30, 100)
        };
        cfg.system_alert_ram = if settings.ram == 0 {
            0
        } else {
            settings.ram.clamp(30, 100)
        };
        cfg.system_alert_seconds = settings.seconds.clamp(5, 3600);
        cfg.clone()
    };
    snapshot
        .save(&state.dirs.config_path())
        .map_err(|e| format!("no se pudo guardar: {e}"))
}

#[tauri::command]
pub fn system_action(id: String) -> Result<(), String> {
    match id.as_str() {
        "lock" => crate::system_actions::lock_screen(),
        "sleep" => crate::system_actions::sleep(),
        "mute" => crate::system_actions::toggle_mute(),
        "trash" => crate::system_actions::empty_trash(),
        other => Err(format!("acción desconocida: {other}")),
    }
}
