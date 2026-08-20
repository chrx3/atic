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
use atic_core::MutexExt;

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

    if *state.audio_test_running.lock_or_recover() {
        return Err(crate::ui_lang::msg(
            "Hay una prueba de audio en curso.",
            "An audio test is already running.",
        ));
    }

    if state.dictation.lock_or_recover().is_some() {
        return Err(crate::ui_lang::msg(
            "Hay un dictado en curso. Termínalo antes de grabar.",
            "Dictation is in progress. Finish it before recording.",
        ));
    }

    let mut active = state.active.lock_or_recover();
    if active.is_some() {
        return Err(crate::ui_lang::msg(
            "Ya hay una grabación en curso.",
            "A recording is already in progress.",
        ));
    }

    let tracks = state
        .config
        .lock_or_recover()
        .effective_record_tracks()
        .to_string();
    let cfg_snapshot = state.config.lock_or_recover().clone();
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
            crate::ui_lang::english(),
        ) {
            if preflight.risk == "bluetooth_hands_free" {
                let message = preflight.message.unwrap_or_else(|| {
                    crate::ui_lang::msg(
                        "El micrófono Bluetooth activará el perfil Hands-Free.",
                        "The Bluetooth microphone will switch to the Hands-Free profile.",
                    )
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
            english: crate::ui_lang::english(),
        },
        tx,
    )
    .map_err(|e| e.to_ui(crate::ui_lang::english()))?;

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
    if state.config.lock_or_recover().beep_on_start {
        let (device_id, voice) = {
            let cfg = state.config.lock_or_recover();
            (
                cfg.output_device_id.clone(),
                cfg.sound_recording_start.clone(),
            )
        };
        crate::beep::play(crate::beep::SoundAction::RecordingStart, &voice, &device_id);
    }

    if let Some(advisory) = atic_audio::bluetooth_recording_advisory(
        capture_mic,
        capture_system,
        &mic_device_id,
        &output_device_id,
        crate::ui_lang::english(),
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

    let taken = state.active.lock_or_recover().take();
    let Some(active) = taken else {
        return Err(crate::ui_lang::msg(
            "No hay ninguna grabación en curso.",
            "There is no recording in progress.",
        ));
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
                    message: crate::ui_lang::msg(
                        "La pista del micrófono quedó prácticamente en silencio. Revisa el dispositivo y ejecuta la prueba de audio.",
                        "The microphone track was essentially silent. Check the device and run the audio test.",
                    ),
                },
            );
        }
        if summary.system_written && summary.system_peak_rms < 0.0015 {
            let _ = app.emit(
                "capture-warn",
                ErrorPayload {
                    message: crate::ui_lang::msg(
                        "La pista «Otros» quedó prácticamente en silencio. Revisa la salida seleccionada y ejecuta la prueba de audio.",
                        "The “Others” track was essentially silent. Check the selected output and run the audio test.",
                    ),
                },
            );
        }
    }
    rec.status = RecordingStatus::Recorded;

    state
        .db
        .lock_or_recover()
        .insert_recording(&rec)
        .map_err(|e| e.to_string())?;

    // Aviso audible de fin de grabación (configurable, mismo toggle que el de inicio).
    if state.config.lock_or_recover().beep_on_start {
        let (device_id, voice) = {
            let cfg = state.config.lock_or_recover();
            (
                cfg.output_device_id.clone(),
                cfg.sound_recording_stop.clone(),
            )
        };
        crate::beep::play(crate::beep::SoundAction::RecordingStop, &voice, &device_id);
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

    let auto_transcribe = state
        .config
        .lock_or_recover()
        .auto_transcribe_after_recording;
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
                    message: crate::ui_lang::msg(
                        &format!(
                            "La grabación quedó guardada, pero no pudo iniciar la transcripción automática: {err}"
                        ),
                        &format!(
                            "The recording was saved, but automatic transcription could not start: {err}"
                        ),
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
    if app
        .state::<AppState>()
        .dictation
        .lock_or_recover()
        .is_some()
    {
        let message = crate::ui_lang::msg(
            "Hay un dictado en curso. Termínalo antes de grabar.",
            "Dictation is in progress. Finish it before recording.",
        );
        tracing::warn!(%message, "atajo de grabación bloqueado");
        let _ = app.emit("capture-error", ErrorPayload { message });
        return;
    }
    let recording = app.state::<AppState>().active.lock_or_recover().is_some();
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
    crate::overlay::yield_to_main(app);
}

/// Aplica la visibilidad de la pill y persiste la preferencia.
pub fn set_pill_visible(app: &AppHandle, visible: bool) {
    let state = app.state::<AppState>();
    let changed = {
        let mut cfg = state.config.lock_or_recover();
        let changed = cfg.show_pill != visible;
        cfg.show_pill = visible;
        changed
    };
    // Persistir SOLO si cambió. `stash_pill_home` llama acá en cada apertura de
    // la rueda, así que sin esta guarda cada pulsación del atajo escribía
    // config.json a disco —sincrónico, en el hilo de UI— para no cambiar nada.
    if changed {
        let cfg = state.config.lock_or_recover().clone();
        let _ = cfg.save(&state.dirs.config_path());
    }

    if !visible {
        // Esconder la pill con un panel abierto dejaba el Escape global
        // registrado sin nadie que lo cerrara.
        crate::clipboard_history::unregister_clipboard_escape_close(app);
        *state.pre_clipboard_position.lock_or_recover() = None;
    }
    // La pill es un div del overlay: mostrarla u ocultarla es montarla o
    // desmontarla, no `show()`/`hide()` sobre una ventana.
    let _ = app.emit("pill-visibility", visible);
}

/// Alterna la visibilidad de la pill (usado por el menú del tray).
///
/// La verdad la tiene la config y ya no `is_visible()` de una ventana: el
/// overlay está siempre visible, así que preguntarle no diría nada sobre la
/// pill.
pub fn toggle_pill(app: &AppHandle) {
    let visible = app
        .try_state::<AppState>()
        .map(|s| s.config.lock_or_recover().show_pill)
        .unwrap_or(true);
    set_pill_visible(app, !visible);
}

/// Persiste la posición de la pill como su nuevo hogar.
fn remember_pill_home(app: &AppHandle, x: i32, y: i32) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut cfg = state.config.lock_or_recover();
    let next = Some((f64::from(x), f64::from(y)));
    // Mismo motivo que en `set_pill_visible`: volver al mismo hogar es el caso
    // normal, y reescribir el archivo idéntico en cada ciclo no aporta nada.
    if cfg.pill_position == next {
        return;
    }
    cfg.pill_position = next;
    let snapshot = cfg.clone();
    drop(cfg);
    let _ = snapshot.save(&state.dirs.config_path());
}

/// Lleva la pill al cursor sin persistir el hogar (camino del clipboard).
///
/// Devuelve el vuelo (destino + duración) ya clampeado. El vuelo corre aparte:
/// este comando no debe quedarse bloqueado esperándolo, pero sí le informa al
/// frontend cuánto dura para que no redimensione a mitad de camino.
pub fn animate_pill_to_cursor(app: &AppHandle) -> Option<crate::floating::Flight> {
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
    if app.get_webview_window(crate::overlay::LABEL).is_none() {
        return;
    }
    set_pill_visible(app, true);

    // Traer pill cancela cualquier hogar temporal del clipboard. Soltar el
    // Escape global es parte de cancelarlo: si el panel estaba abierto, nadie
    // más lo desregistraría y la tecla quedaría secuestrada en todo el SO.
    crate::clipboard_history::unregister_clipboard_escape_close(app);
    if let Some(state) = app.try_state::<AppState>() {
        *state.pre_clipboard_position.lock_or_recover() = None;
    }

    // Cierra el panel y devuelve la pill a su forma compacta ANTES de medir:
    // el ancla depende del tamaño, y un panel abierto la centraría mal.
    //
    // El vuelo lo dispara el frontend con `summon_pill_here`, no esta función.
    // Volar acá mismo era una carrera contra el colapso: el ancla se resolvía
    // con los 312×380 del panel (la pill quedaba ~130 px arriba del cursor) y
    // el reencuadre que llegaba después cancelaba el vuelo a mitad de camino.
    // Solo el frontend sabe cuándo terminó de encoger.
    let _ = app.emit("pill-reset", ());
}

/// Vuela la pill al cursor y persiste ese punto como su hogar.
///
/// Lo invoca el frontend al terminar de colapsar, en respuesta a `pill-reset`.
#[tauri::command]
pub fn summon_pill_here(app: AppHandle) -> Result<(), String> {
    tracing::info!(target: "pill_geo", "CMD        summon_pill_here");
    let flight = crate::floating::glide(&app, "pill", crate::floating::Anchor::Cursor)
        .ok_or_else(|| {
            crate::ui_lang::msg(
                "No se pudo colocar la pill en el cursor.",
                "Could not move the pill to the cursor.",
            )
        })?;
    remember_pill_home(&app, flight.x, flight.y);
    Ok(())
}

/// Traza del frontend en el mismo flujo que la de Rust.
///
/// `console.log` del webview no sale por la terminal, así que sin esto el
/// recorrido queda contado a medias: se ven las escrituras de posición pero no
/// la intención que las pidió ni en qué orden las emitió el frontend.
#[tauri::command]
pub fn pill_trace(msg: String) {
    tracing::debug!(target: "pill_geo", "UI         {msg}");
}

/// Devuelve el modelo Whisper en memoria, cargándolo si hace falta.
pub fn get_or_load_whisper(
    state: &AppState,
    model_path: &Path,
) -> Result<Arc<LoadedModel>, String> {
    *state.whisper_last_used.lock_or_recover() = Some(Instant::now());

    let mut guard = state.whisper.lock_or_recover();
    if let Some(loaded) = guard.get(model_path) {
        return Ok(Arc::clone(loaded));
    }

    // Carga con el mutex retenido: evita dos hilos leyendo el mismo GGML a la vez.
    tracing::info!(path = %model_path.display(), "cargando modelo Whisper en memoria");
    let loaded = Arc::new(
        LoadedModel::load(model_path).map_err(|e| e.to_ui(crate::ui_lang::english()))?,
    );
    guard.insert(model_path.to_path_buf(), Arc::clone(&loaded));
    Ok(loaded)
}

/// Quita del cache rutas que ya no estan en la config (libera RAM).
pub fn prune_whisper_cache(state: &AppState, keep: &[PathBuf]) {
    let mut guard = state.whisper.lock_or_recover();
    guard.retain(|path, _| keep.iter().any(|k| k == path));
}

fn whisper_in_use(state: &AppState) -> bool {
    state.active.lock_or_recover().is_some() || state.dictation.lock_or_recover().is_some()
}

/// Libera todos los modelos Whisper residentes si no hay trabajo activo.
pub fn unload_whisper_cache(state: &AppState) -> usize {
    if whisper_in_use(state) {
        return 0;
    }
    let mut guard = state.whisper.lock_or_recover();
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
        let cfg = state.config.lock_or_recover().clone();
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
        let last = *state.whisper_last_used.lock_or_recover();
        let Some(last) = last else {
            continue;
        };
        if last.elapsed() < WHISPER_IDLE_TTL {
            continue;
        }
        let cached = state.whisper.lock_or_recover().len();
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
            *state.whisper_last_used.lock_or_recover() = None;
        }
    });
}
