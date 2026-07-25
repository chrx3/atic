//! Estado global de la aplicación y lógica de captura de grabaciones.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use atic_audio::{CaptureConfig, CaptureEvent, CaptureHandle, CaptureSession};
use atic_core::{AppDirs, Config, Db, Recording, RecordingStatus};
use atic_transcribe::LoadedModel;

use crate::live::LiveWorkerHandle;

/// Tras este idle sin uso, se liberan los modelos Whisper de RAM.
const WHISPER_IDLE_TTL: Duration = Duration::from_secs(10 * 60);
const WHISPER_IDLE_CHECK: Duration = Duration::from_secs(60);

/// Estado compartido, gestionado por Tauri (`app.manage`).
pub struct AppState {
    pub dirs: AppDirs,
    pub db: Mutex<Db>,
    pub config: Mutex<Config>,
    pub active: Mutex<Option<ActiveRecording>>,
    pub dictation: Mutex<Option<crate::dictation::ActiveDictation>>,
    pub audio_test_running: Mutex<bool>,
    /// Modelos Whisper residentes (dictado + reuniones pueden coexistir).
    pub whisper: Mutex<HashMap<PathBuf, Arc<LoadedModel>>>,
    /// Último uso del cache Whisper (carga o hit).
    pub whisper_last_used: Mutex<Option<Instant>>,
    /// Sesión de captura de pantalla activa (overlay de selección). Solo una.
    pub overlay_session: Mutex<Option<crate::capture_session::OverlaySession>>,
    /// Posición de la pill antes de invocar el clipboard en el cursor.
    /// Se restaura al cerrar/pegar; no se persiste en disco.
    pub pre_clipboard_position: Mutex<Option<(f64, f64)>>,
    /// Atajos que el SO rechazó registrar (ya los tiene otra app). Un conflicto
    /// es silencioso para el usuario si solo se loguea: esto lo hace visible.
    pub shortcut_failures: Mutex<Vec<String>>,
}

/// Grabación actualmente en curso.
pub struct ActiveRecording {
    pub recording: Recording,
    pub handle: CaptureHandle,
    /// Worker de STT en vivo (si `live_transcription` está activo).
    pub live: Option<LiveWorkerHandle>,
}

#[derive(Clone, Serialize)]
pub struct LevelsPayload {
    pub mic: f32,
    pub system: f32,
}

#[derive(Clone, Serialize)]
pub struct ErrorPayload {
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct StatusPayload {
    pub active: bool,
    pub recording: Option<Recording>,
}

/// Prefijo estable para que la UI pueda pedir confirmación sin depender del texto.
pub const BLUETOOTH_CONFIRM_REQUIRED: &str = "BLUETOOTH_CONFIRM_REQUIRED:";

/// Inicia la captura y registra la grabación como activa.
pub fn start_capture(
    app: &AppHandle,
    allow_bluetooth_hands_free: bool,
) -> Result<Recording, String> {
    let state = app.state::<AppState>();

    if *state.audio_test_running.lock().unwrap() {
        return Err("Hay una prueba de audio en curso.".into());
    }

    if state.dictation.lock().unwrap().is_some() {
        return Err("Hay un dictado en curso. Termínalo antes de grabar.".into());
    }

    let mut active = state.active.lock().unwrap();
    if active.is_some() {
        return Err("Ya hay una grabación en curso.".into());
    }

    let tracks = state
        .config
        .lock()
        .unwrap()
        .effective_record_tracks()
        .to_string();
    let cfg_snapshot = state.config.lock().unwrap().clone();
    let noise_suppression = cfg_snapshot.noise_suppression.clone();
    let mic_device_id = cfg_snapshot.mic_device_id.clone();
    let output_device_id = cfg_snapshot.output_device_id.clone();
    let capture_mic = tracks == "both" || tracks == "mic";
    let capture_system = tracks == "both" || tracks == "system";

    if !allow_bluetooth_hands_free {
        if let Ok(preflight) = atic_audio::audio_preflight(
            capture_mic,
            capture_system,
            &mic_device_id,
            &output_device_id,
        ) {
            if preflight.risk == "bluetooth_hands_free" {
                let message = preflight.message.unwrap_or_else(|| {
                    "El micrófono Bluetooth activará el perfil Hands-Free.".into()
                });
                let _ = app.emit(
                    "capture-warn",
                    ErrorPayload {
                        message: message.clone(),
                    },
                );
                return Err(format!("{BLUETOOTH_CONFIRM_REQUIRED}{message}"));
            }
        }
    }

    let mut rec = Recording::new(Utc::now());
    let dir = state.dirs.recording_dir(&rec.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mic_wav = dir.join("mic.wav");
    let system_wav = dir.join("system.wav");

    // Live local: tap no bloqueante. Capacidad pequeña → dropea si STT se atrasa.
    let want_live = cfg_snapshot.live_transcription;
    let (stt_tap, live_rx) = if want_live {
        let (tx, rx) = mpsc::sync_channel(512);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let (tx, rx) = mpsc::channel::<CaptureEvent>();
    let handle = CaptureSession::start(
        CaptureConfig {
            mic_wav,
            system_wav,
            capture_mic,
            capture_system,
            noise_suppression,
            mic_device_id: mic_device_id.clone(),
            output_device_id: output_device_id.clone(),
            stt_tap,
        },
        tx,
    )
    .map_err(|e| e.to_string())?;

    // Reenvía los eventos de captura a la UI hasta que el canal se cierra.
    let app_fwd = app.clone();
    std::thread::spawn(move || {
        for event in rx {
            match event {
                CaptureEvent::Levels { mic, system } => {
                    let _ = app_fwd.emit("audio-levels", LevelsPayload { mic, system });
                }
                CaptureEvent::Error(message) => {
                    tracing::warn!(%message, "aviso de captura");
                    let _ = app_fwd.emit("capture-error", ErrorPayload { message });
                }
            }
        }
    });

    let mut live = None;
    if let Some(tap_rx) = live_rx {
        let language = if cfg_snapshot.language == "auto" {
            None
        } else {
            Some(cfg_snapshot.language.clone())
        };
        match crate::live::spawn_live_worker(
            app.clone(),
            tap_rx,
            language,
            &cfg_snapshot.live_whisper_model,
            &cfg_snapshot.live_engine,
            &cfg_snapshot.live_groq_model,
        ) {
            Ok(worker) => live = Some(worker),
            Err(message) => {
                tracing::warn!(%message, "live STT no arrancó; la grabación continúa sin live");
                let _ = app.emit(
                    "live-transcript-error",
                    crate::live::LiveErrorPayload { message },
                );
            }
        }
    }

    rec.mic_path = if capture_mic {
        Some("mic.wav".into())
    } else {
        None
    };
    rec.system_path = if capture_system {
        Some("system.wav".into())
    } else {
        None
    };
    *active = Some(ActiveRecording {
        recording: rec.clone(),
        handle,
        live,
    });
    drop(active);

    // Aviso audible de consentimiento (configurable).
    if state.config.lock().unwrap().beep_on_start {
        let device_id = state.config.lock().unwrap().output_device_id.clone();
        crate::beep::play_start_beep(&device_id);
    }

    if let Some(advisory) = atic_audio::bluetooth_recording_advisory(
        capture_mic,
        capture_system,
        &mic_device_id,
        &output_device_id,
    ) {
        let message = match &advisory.suggestion {
            Some(suggestion) => format!("{} {}", advisory.message, suggestion),
            None => advisory.message,
        };
        let _ = app.emit("capture-warn", ErrorPayload { message });
    }

    let _ = app.emit(
        "recording-status",
        StatusPayload {
            active: true,
            recording: Some(rec.clone()),
        },
    );
    Ok(rec)
}

/// Detiene la captura activa, persiste la grabación y notifica a la UI.
///
/// El stop del WAV no espera al worker live (Whisper/Groq): la UI pasa a
/// "no grabando" en cuanto el audio se detiene. La vista previa se descarta y,
/// si está habilitado, comienza la transcripción completa en segundo plano.
pub fn stop_capture(app: &AppHandle) -> Result<Recording, String> {
    let state = app.state::<AppState>();

    let taken = state.active.lock().unwrap().take();
    let Some(active) = taken else {
        return Err("No hay ninguna grabación en curso.".into());
    };

    // 1) Detener solo la captura de audio (no el STT live).
    let summary = active.handle.stop();

    let mut rec = active.recording;
    rec.duration_secs = summary.duration_secs.round() as i64;
    if !summary.mic_written {
        rec.mic_path = None;
    }
    if !summary.system_written {
        rec.system_path = None;
    }
    if summary.duration_secs >= 2.0 {
        if summary.mic_written && summary.mic_peak_rms < 0.0015 {
            let _ = app.emit(
                "capture-warn",
                ErrorPayload {
                    message: "La pista del micrófono quedó prácticamente en silencio. Revisa el dispositivo y ejecuta la prueba de audio.".into(),
                },
            );
        }
        if summary.system_written && summary.system_peak_rms < 0.0015 {
            let _ = app.emit(
                "capture-warn",
                ErrorPayload {
                    message: "La pista «Otros» quedó prácticamente en silencio. Revisa la salida seleccionada y ejecuta la prueba de audio.".into(),
                },
            );
        }
    }
    rec.status = RecordingStatus::Recorded;

    state
        .db
        .lock()
        .unwrap()
        .insert_recording(&rec)
        .map_err(|e| e.to_string())?;

    // Aviso audible de fin de grabación (configurable, mismo toggle que el de inicio).
    if state.config.lock().unwrap().beep_on_start {
        let device_id = state.config.lock().unwrap().output_device_id.clone();
        crate::beep::play_stop_beep(&device_id);
    }

    // 2) UI responsive: "no grabando" antes de cualquier flush live.
    let _ = app.emit(
        "recording-status",
        StatusPayload {
            active: false,
            recording: None,
        },
    );
    let _ = app.emit("recordings-changed", ());

    let auto_transcribe = state.config.lock().unwrap().auto_transcribe_after_recording;
    let app_bg = app.clone();
    let rec_id = rec.id.clone();
    let start_batch = move || {
        if !auto_transcribe {
            return;
        }
        if let Err(err) = crate::transcription::transcribe_recording(app_bg.clone(), rec_id.clone())
        {
            tracing::warn!(%err, id = %rec_id, "no se pudo iniciar la transcripción automática");
            let _ = app_bg.emit(
                "capture-warn",
                ErrorPayload {
                    message: format!(
                        "La grabación quedó guardada, pero no pudo iniciar la transcripción automática: {err}"
                    ),
                },
            );
        }
    };

    // 3) La vista previa nunca se guarda. Al liberarla, arranca el proceso final
    // sobre los WAV completos; sin vista previa, puede comenzar inmediatamente.
    if let Some(live) = active.live {
        live.stop_preview_in_background(start_batch);
    } else {
        start_batch();
    }

    Ok(rec)
}

/// Alterna grabación (usado por el atajo global).
pub fn toggle_recording(app: &AppHandle) {
    if app.state::<AppState>().dictation.lock().unwrap().is_some() {
        let message = "Hay un dictado en curso. Termínalo antes de grabar.".to_string();
        tracing::warn!(%message, "atajo de grabación bloqueado");
        let _ = app.emit("capture-error", ErrorPayload { message });
        return;
    }
    let recording = app.state::<AppState>().active.lock().unwrap().is_some();
    let result = if recording {
        stop_capture(app)
    } else {
        start_capture(app, false)
    };
    if let Err(message) = result {
        tracing::error!(%message, "el atajo de grabación falló");
        let _ = app.emit("capture-error", ErrorPayload { message });
    }
}

/// Muestra y enfoca la ventana principal.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Aplica la visibilidad de la pill y persiste la preferencia.
pub fn set_pill_visible(app: &AppHandle, visible: bool) {
    let state = app.state::<AppState>();
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.show_pill = visible;
    }
    let cfg = state.config.lock().unwrap().clone();
    let _ = cfg.save(&state.dirs.config_path());

    if let Some(window) = app.get_webview_window("pill") {
        if visible {
            let _ = window.set_always_on_top(true);
            let _ = window.show();
        } else {
            // Esconder la pill con un panel abierto dejaba el Escape global
            // registrado sin nadie que lo cerrara.
            crate::clipboard_history::unregister_clipboard_escape_close(app);
            *state.pre_clipboard_position.lock().unwrap() = None;
            let _ = window.hide();
        }
    }
}

/// Alterna la visibilidad de la pill (usado por el menú del tray).
pub fn toggle_pill(app: &AppHandle) {
    let visible = app
        .get_webview_window("pill")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    set_pill_visible(app, !visible);
}

/// Persiste la posición de la pill como su nuevo hogar.
fn remember_pill_home(app: &AppHandle, x: i32, y: i32) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut cfg = state.config.lock().unwrap();
    cfg.pill_position = Some((f64::from(x), f64::from(y)));
    let snapshot = cfg.clone();
    drop(cfg);
    let _ = snapshot.save(&state.dirs.config_path());
}

/// Lleva la pill al cursor sin persistir el hogar (camino del clipboard).
///
/// Devuelve el destino ya clampeado. El vuelo corre aparte: este comando no
/// debe quedarse bloqueado esperándolo.
pub fn animate_pill_to_cursor(app: &AppHandle) -> Option<(i32, i32)> {
    set_pill_visible(app, true);
    crate::floating::glide(app, "pill", crate::floating::Anchor::Cursor)
}

/// Lleva la pill a un punto exacto (volver al hogar tras cerrar un panel).
pub fn animate_pill_to(app: &AppHandle, target_x: i32, target_y: i32) {
    crate::floating::glide(
        app,
        "pill",
        crate::floating::Anchor::Point(target_x, target_y),
    );
}

/// Muestra la pill y la trae al cursor. Traslado permanente: persiste el hogar.
pub fn summon_pill_to_cursor(app: &AppHandle) {
    if app.get_webview_window("pill").is_none() {
        return;
    }
    set_pill_visible(app, true);

    // Traer pill cancela cualquier hogar temporal del clipboard. Soltar el
    // Escape global es parte de cancelarlo: si el panel estaba abierto, nadie
    // más lo desregistraría y la tecla quedaría secuestrada en todo el SO.
    crate::clipboard_history::unregister_clipboard_escape_close(app);
    if let Some(state) = app.try_state::<AppState>() {
        *state.pre_clipboard_position.lock().unwrap() = None;
    }

    // Cierra el panel y devuelve la pill a su forma compacta ANTES de medir:
    // el ancla depende del tamaño, y un panel abierto la centraría mal.
    let _ = app.emit("pill-reset", ());

    if let Some((x, y)) = crate::floating::glide(app, "pill", crate::floating::Anchor::Cursor)
    {
        remember_pill_home(app, x, y);
    } else {
        tracing::warn!("no se pudo colocar la pill en el cursor");
    }
}

/// Devuelve el modelo Whisper en memoria, cargándolo si hace falta.
pub fn get_or_load_whisper(
    state: &AppState,
    model_path: &Path,
) -> Result<Arc<LoadedModel>, String> {
    *state.whisper_last_used.lock().unwrap() = Some(Instant::now());

    let mut guard = state.whisper.lock().unwrap();
    if let Some(loaded) = guard.get(model_path) {
        return Ok(Arc::clone(loaded));
    }

    // Carga con el mutex retenido: evita dos hilos leyendo el mismo GGML a la vez.
    tracing::info!(path = %model_path.display(), "cargando modelo Whisper en memoria");
    let loaded = Arc::new(LoadedModel::load(model_path).map_err(|e| e.to_string())?);
    guard.insert(model_path.to_path_buf(), Arc::clone(&loaded));
    Ok(loaded)
}

/// Quita del cache rutas que ya no estan en la config (libera RAM).
pub fn prune_whisper_cache(state: &AppState, keep: &[PathBuf]) {
    let mut guard = state.whisper.lock().unwrap();
    guard.retain(|path, _| keep.iter().any(|k| k == path));
}

fn whisper_in_use(state: &AppState) -> bool {
    state.active.lock().unwrap().is_some() || state.dictation.lock().unwrap().is_some()
}

/// Libera todos los modelos Whisper residentes si no hay trabajo activo.
pub fn unload_whisper_cache(state: &AppState) -> usize {
    if whisper_in_use(state) {
        return 0;
    }
    let mut guard = state.whisper.lock().unwrap();
    let n = guard.len();
    guard.clear();
    n
}

/// Precarga los modelos configurados si estan en disco.
///
/// Cuando dictado y reuniones comparten modelo (el default), se carga una sola
/// vez: reduce el arranque, el uso de RAM y la primera descarga a ~148 MB.
/// El modelo de live solo se precarga si live local está activo.
pub fn preload_whisper_async(app: &AppHandle) {
    let app2 = app.clone();
    thread::spawn(move || {
        let state = app2.state::<AppState>();
        let cfg = state.config.lock().unwrap().clone();
        let models_dir = state.dirs.models_dir();
        // Dictado primero: es el camino sensible a latencia.
        let mut ids: Vec<&str> = vec![
            cfg.dictation_whisper_model.as_str(),
            cfg.whisper_model.as_str(),
        ];
        if cfg.live_transcription && cfg.live_engine == "local" {
            ids.push(cfg.live_whisper_model.as_str());
        }
        let mut keep = Vec::new();
        for model_id in ids {
            match atic_transcribe::models::require_downloaded(&models_dir, model_id) {
                Ok(path) => {
                    // No cargar dos veces el mismo modelo cuando dictado y
                    // reuniones comparten el perfil rápido predeterminado.
                    if keep.contains(&path) {
                        continue;
                    }
                    keep.push(path.clone());
                    match get_or_load_whisper(&state, &path) {
                        Ok(_) => tracing::info!(%model_id, "modelo Whisper precargado"),
                        Err(err) => {
                            tracing::warn!(%err, %model_id, "no se pudo precargar Whisper")
                        }
                    }
                }
                Err(_) => {
                    tracing::debug!(%model_id, "modelo no descargado; se omitió la precarga");
                }
            }
        }
        prune_whisper_cache(&state, &keep);
    });
    ensure_whisper_idle_unloader(app);
}

/// Hilo único: si Whisper no se usa durante `WHISPER_IDLE_TTL`, libera la RAM.
fn ensure_whisper_idle_unloader(app: &AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    thread::spawn(move || loop {
        thread::sleep(WHISPER_IDLE_CHECK);
        let Some(state) = app2.try_state::<AppState>() else {
            continue;
        };
        if whisper_in_use(&state) {
            continue;
        }
        let last = *state.whisper_last_used.lock().unwrap();
        let Some(last) = last else {
            continue;
        };
        if last.elapsed() < WHISPER_IDLE_TTL {
            continue;
        }
        let cached = state.whisper.lock().unwrap().len();
        if cached == 0 {
            continue;
        }
        let n = unload_whisper_cache(&state);
        if n > 0 {
            tracing::info!(
                models = n,
                idle_secs = WHISPER_IDLE_TTL.as_secs(),
                "modelos Whisper liberados por idle"
            );
            *state.whisper_last_used.lock().unwrap() = None;
        }
    });
}
