//! Comandos invocables desde el frontend.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::{Config, Recording};

use crate::state::{self, AppState};
use atic_core::MutexExt;

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    allow_bluetooth_hands_free: Option<bool>,
) -> Result<Recording, String> {
    state::start_capture(&app, allow_bluetooth_hands_free.unwrap_or(false))
}

#[tauri::command]
pub fn stop_recording(app: AppHandle) -> Result<Recording, String> {
    state::stop_capture(&app)
}

#[tauri::command]
pub fn is_recording(state: State<AppState>) -> bool {
    state.active.lock_or_recover().is_some()
}

#[tauri::command]
pub fn list_recordings(state: State<AppState>) -> Result<Vec<Recording>, String> {
    state
        .db
        .lock_or_recover()
        .list_recordings()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_recording(state: State<AppState>, id: String) -> Result<(), String> {
    // Resolver primero contra SQLite: nunca derivar una ruta borrable desde un
    // parámetro IPC sin comprobar que corresponde a una grabación conocida.
    let recording = state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Grabación no encontrada.".to_string())?;
    let dir = state.dirs.recording_dir(&recording.id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    state
        .db
        .lock_or_recover()
        .delete_recording(&id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn rename_recording(state: State<AppState>, id: String, title: String) -> Result<(), String> {
    state
        .db
        .lock_or_recover()
        .update_title(&id, &title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_config(state: State<AppState>) -> Config {
    state.config.lock_or_recover().clone()
}

#[tauri::command]
pub fn set_config(app: AppHandle, state: State<AppState>, config: Config) -> Result<(), String> {
    let show_pill = config.show_pill;
    let ui_theme = config.ui_theme.clone();
    let want_autostart = config.autostart;
    let shortcut = config.global_shortcut.clone();
    let dictation_shortcut = config.dictation_shortcut.clone();
    let summon_pill_shortcut = config.summon_pill_shortcut.clone();
    let pill_radial_shortcut = config.pill_radial_shortcut.clone();
    let clipboard_shortcut = config.clipboard_shortcut.clone();
    let snippets_shortcut = config.snippets_shortcut.clone();
    let agents_shortcut = config.agents_shortcut.clone();
    let screenshot_shortcut = config.screenshot_shortcut.clone();
    let board_shortcut = config.board_shortcut.clone();
    let launcher_shortcut = config.launcher_shortcut.clone();
    let prev = state.config.lock_or_recover().clone();

    // Si algún atajo cambió, validar+registrar PRIMERO: aborta si es inválido
    // antes de persistir un valor que dejaría el hotkey muerto.
    if shortcut != prev.global_shortcut
        || dictation_shortcut != prev.dictation_shortcut
        || summon_pill_shortcut != prev.summon_pill_shortcut
        || pill_radial_shortcut != prev.pill_radial_shortcut
        || clipboard_shortcut != prev.clipboard_shortcut
        || snippets_shortcut != prev.snippets_shortcut
        || agents_shortcut != prev.agents_shortcut
        || screenshot_shortcut != prev.screenshot_shortcut
        || board_shortcut != prev.board_shortcut
        || launcher_shortcut != prev.launcher_shortcut
    {
        crate::shortcuts::register_shortcuts(
            &app,
            crate::shortcuts::ShortcutBindings {
                recording: &shortcut,
                dictation: &dictation_shortcut,
                summon_pill: &summon_pill_shortcut,
                pill_radial: &pill_radial_shortcut,
                clipboard: &clipboard_shortcut,
                snippets: &snippets_shortcut,
                agents: &agents_shortcut,
                screenshot: &screenshot_shortcut,
                board: &board_shortcut,
                launcher: &launcher_shortcut,
            },
        )?;
    }

    let models_changed = config.whisper_model != prev.whisper_model
        || config.dictation_whisper_model != prev.dictation_whisper_model
        || config.live_whisper_model != prev.live_whisper_model;
    {
        *state.config.lock_or_recover() = config.clone();
    }
    config
        .save(&state.dirs.config_path())
        .map_err(|e| e.to_string())?;
    if models_changed {
        state::preload_whisper_async(&app);
    }
    state::set_pill_visible(&app, show_pill);
    sync_autostart(&app, want_autostart);
    // El overlay tiene `data_directory` propio: localStorage no cruza.
    let _ = app.emit("ui-theme", ui_theme);
    if config.onboarding_done != prev.onboarding_done
        || config.onboarding_practice_done != prev.onboarding_practice_done
    {
        let _ = app.emit("onboarding-practice", ());
    }

    Ok(())
}

fn sync_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(err) = result {
        tracing::warn!(%err, enabled, "no se pudo actualizar autostart");
    }
}

#[tauri::command]
pub fn set_pill_visible(app: AppHandle, visible: bool) {
    state::set_pill_visible(&app, visible);
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    state::show_main(&app);
}

/// Devuelve la ruta absoluta de una pista ("mic" | "system") de una grabación,
/// para que el frontend la convierta con `convertFileSrc` y la reproduzca.
#[tauri::command]
pub fn recording_track_path(
    state: State<AppState>,
    id: String,
    track: String,
) -> Result<String, String> {
    let file = match track.as_str() {
        "mic" => "mic.wav",
        "system" => "system.wav",
        _ => return Err("Pista inválida.".into()),
    };
    let recording = state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Grabación no encontrada.".to_string())?;
    let track_exists = match track.as_str() {
        "mic" => recording.mic_path.is_some(),
        "system" => recording.system_path.is_some(),
        _ => false,
    };
    if !track_exists {
        return Err("El archivo de audio no existe.".into());
    }
    let path = state.dirs.recording_dir(&recording.id).join(file);
    if !path.exists() {
        return Err("El archivo de audio no existe.".into());
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn toggle_dictation(app: AppHandle) {
    crate::dictation::toggle_dictation(&app);
}

#[tauri::command]
pub fn dictation_phase(state: State<AppState>) -> crate::dictation::DictationPhase {
    if state.dictation.lock_or_recover().is_some() {
        crate::dictation::DictationPhase::Listening
    } else {
        crate::dictation::DictationPhase::Idle
    }
}

#[tauri::command]
pub fn list_input_devices() -> Result<Vec<atic_audio::InputDeviceInfo>, String> {
    atic_audio::list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_output_devices() -> Result<Vec<atic_audio::InputDeviceInfo>, String> {
    atic_audio::list_output_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn debug_list_audio_devices() -> Result<String, String> {
    atic_audio::debug_list_audio_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn audio_preflight(state: State<AppState>) -> Result<atic_audio::AudioPreflight, String> {
    let config = state.config.lock_or_recover().clone();
    let tracks = config.effective_record_tracks();
    atic_audio::audio_preflight(
        tracks == "both" || tracks == "mic",
        tracks == "both" || tracks == "system",
        &config.mic_device_id,
        &config.output_device_id,
    )
    .map_err(|error| error.to_string())
}

#[derive(Serialize)]
pub struct AudioTestResult {
    pub mic: Option<atic_audio::WavAnalysis>,
    pub system: Option<atic_audio::WavAnalysis>,
    pub preflight: atic_audio::AudioPreflight,
}

/// Abre durante cinco segundos las mismas pistas que usaría una grabación real.
#[tauri::command]
pub async fn test_audio(app: AppHandle, config: Config) -> Result<AudioTestResult, String> {
    {
        let state = app.state::<AppState>();
        let mut running = state.audio_test_running.lock_or_recover();
        if *running {
            return Err("Ya hay una prueba de audio en curso.".into());
        }
        if state.active.lock_or_recover().is_some() {
            return Err("Detén la grabación antes de probar el audio.".into());
        }
        if state.dictation.lock_or_recover().is_some() {
            return Err("Termina el dictado antes de probar el audio.".into());
        }
        *running = true;
    }

    let joined = tauri::async_runtime::spawn_blocking(move || {
        let tracks = config.effective_record_tracks();
        let capture_mic = tracks == "both" || tracks == "mic";
        let capture_system = tracks == "both" || tracks == "system";
        let preflight = atic_audio::audio_preflight(
            capture_mic,
            capture_system,
            &config.mic_device_id,
            &config.output_device_id,
        )
        .map_err(|error| error.to_string())?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("atic-audio-test-{nonce}"));
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let mic_path = dir.join("mic.wav");
        let system_path = dir.join("system.wav");
        let (events, _receiver) = std::sync::mpsc::channel();
        let handle = atic_audio::CaptureSession::start(
            atic_audio::CaptureConfig {
                mic_wav: mic_path.clone(),
                system_wav: system_path.clone(),
                capture_mic,
                capture_system,
                noise_suppression: config.noise_suppression,
                mic_device_id: config.mic_device_id,
                output_device_id: config.output_device_id,
                stt_tap: None,
            },
            events,
        )
        .map_err(|error| error.to_string())?;
        std::thread::sleep(std::time::Duration::from_secs(5));
        let summary = handle.stop();
        let mic = summary
            .mic_written
            .then(|| atic_audio::analyze_wav(&mic_path))
            .transpose()
            .map_err(|error| error.to_string())?;
        let system = summary
            .system_written
            .then(|| atic_audio::analyze_wav(&system_path))
            .transpose()
            .map_err(|error| error.to_string())?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok(AudioTestResult {
            mic,
            system,
            preflight,
        })
    })
    .await;

    *app.state::<AppState>().audio_test_running.lock_or_recover() = false;
    joined.map_err(|error| error.to_string())?
}

/// Abre en el explorador una carpeta de datos de Atic.
///
/// `kind`: `recordings` | `clipboard` | `snippets` | `captures` | `logs` | `data`.
pub(crate) fn open_data_dir_kind(
    app: &AppHandle,
    dirs: &atic_core::AppDirs,
    kind: &str,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let dir = match kind.trim().to_ascii_lowercase().as_str() {
        "recordings" => dirs.recordings_dir(),
        "clipboard" => dirs.clipboard_dir(),
        "snippets" => dirs.snippets_dir(),
        "captures" => dirs.captures_dir(),
        "logs" => dirs.logs_dir(),
        "data" => dirs.data_dir(),
        other => {
            return Err(format!("Carpeta de datos desconocida: {other}"));
        }
    };
    let _ = std::fs::create_dir_all(&dir);
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<AppState>, kind: String) -> Result<(), String> {
    open_data_dir_kind(&app, &state.dirs, &kind)
}

/// Abre en el explorador los archivos de una grabación (WAV y JSON).
///
/// No abre el listado de ids: eso no se puede reconocer a ojo.
#[tauri::command]
pub fn open_recording_dir(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let rec = state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Grabación no encontrada.".to_string())?;
    let dir = state.dirs.recording_dir(&id);
    if !dir.is_dir() {
        return Err("Esta grabación no tiene archivos en disco.".into());
    }
    let _ = std::fs::write(dir.join("titulo.txt"), rec.title.as_bytes());
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}
