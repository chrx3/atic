//! Dictado MVP: grabar mic → transcribir local → pegar en el campo activo.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use atic_audio::{CaptureConfig, CaptureEvent, CaptureHandle, CaptureSession};
use atic_core::{secrets, SecretKind, Speaker};
use atic_transcribe::{self as transcribe, TrackInput};

use crate::state::{AppState, ErrorPayload, LevelsPayload};
use atic_core::MutexExt;

fn resolve_groq_api_key() -> Option<String> {
    if let Ok(Some(key)) = secrets::get_secret(SecretKind::GroqApiKey) {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

const GROQ_KEY_REQUIRED_MSG: &str =
    "Configura tu API key de Groq en Ajustes para usar el dictado en la nube.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationPhase {
    Idle,
    Listening,
    Transcribing,
    Pasted,
    Error,
}

#[derive(Clone, Serialize)]
pub struct DictationStatusPayload {
    pub phase: DictationPhase,
    pub message: Option<String>,
}

pub struct ActiveDictation {
    pub wav_path: PathBuf,
    pub temp_dir: PathBuf,
    pub handle: CaptureHandle,
}

fn emit_status(app: &AppHandle, phase: DictationPhase, message: Option<String>) {
    let _ = app.emit(
        "dictation-status",
        DictationStatusPayload { phase, message },
    );
}

/// Alterna dictado (modo toggle): start → stop+transcribe+paste.
pub fn toggle_dictation(app: &AppHandle) {
    let state = app.state::<AppState>();
    let listening = state.dictation.lock_or_recover().is_some();
    if listening {
        stop_and_paste(app);
    } else if let Err(message) = start_dictation(app) {
        tracing::error!(%message, "no se pudo iniciar dictado");
        emit_status(app, DictationPhase::Error, Some(message.clone()));
        let _ = app.emit("capture-error", ErrorPayload { message });
    }
}

/// Push-to-talk: mantener = escuchar, soltar = transcribir y pegar.
pub fn dictation_key_down(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.dictation.lock_or_recover().is_some() {
        return;
    }
    if let Err(message) = start_dictation(app) {
        tracing::error!(%message, "no se pudo iniciar dictado (PTT)");
        emit_status(app, DictationPhase::Error, Some(message.clone()));
        let _ = app.emit("capture-error", ErrorPayload { message });
    }
}

/// Push-to-talk: al soltar la tecla.
pub fn dictation_key_up(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.dictation.lock_or_recover().is_some() {
        stop_and_paste(app);
    }
}

fn start_dictation(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    if state.active.lock_or_recover().is_some() {
        return Err("Hay una grabación de reunión en curso. Deténla antes de dictar.".into());
    }
    if state.dictation.lock_or_recover().is_some() {
        return Err("Ya estás dictando.".into());
    }

    // Guardar destino de pegado YA: antes de que la pill/UI robe el foco.
    crate::clipboard_history::remember_paste_target();

    // Validar backend de dictado antes de abrir el mic.
    // Sin key de Groq se cae a Whisper local: el modelo local debe estar listo.
    let cfg = state.config.lock_or_recover().clone();
    let use_groq = cfg.dictation_backend == "groq" && resolve_groq_api_key().is_some();
    if !use_groq {
        if cfg.dictation_backend == "groq" {
            tracing::warn!(
                "dictado Groq sin API key; se usará Whisper local. {}",
                GROQ_KEY_REQUIRED_MSG
            );
        }
        let _ = transcribe::models::require_downloaded(
            &state.dirs.models_dir(),
            &cfg.dictation_whisper_model,
        )
        .map_err(|e| {
            if cfg.dictation_backend == "groq" {
                format!("{GROQ_KEY_REQUIRED_MSG} Mientras tanto, descarga un modelo local: {e}")
            } else {
                e.to_string()
            }
        })?;
    }

    let temp_dir = state
        .dirs
        .data_dir()
        .join("dictation")
        .join(format!("{}", Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let wav_path = temp_dir.join("mic.wav");
    let cfg = state.config.lock_or_recover().clone();
    // Notebooks: ventiladores suelen tapar la voz. El dictado siempre aplica
    // al menos RNNoise medium (aunque en reuniones esté en off/low).
    let noise_suppression = effective_dictation_noise(&cfg.noise_suppression);
    let mic_device_id = if cfg.dictation_mic_device_id.trim().is_empty() {
        cfg.mic_device_id.clone()
    } else {
        cfg.dictation_mic_device_id.clone()
    };
    let output_device_id = cfg.output_device_id.clone();

    let (tx, rx) = mpsc::channel::<CaptureEvent>();
    let handle = CaptureSession::start(
        CaptureConfig {
            mic_wav: wav_path.clone(),
            system_wav: temp_dir.join("system.wav"),
            capture_mic: true,
            capture_system: false,
            noise_suppression,
            mic_device_id,
            output_device_id,
            stt_tap: None,
        },
        tx,
    )
    .map_err(|e| e.to_string())?;

    let app_fwd = app.clone();
    thread::spawn(move || {
        for event in rx {
            match event {
                CaptureEvent::Levels { mic, system } => {
                    let _ = app_fwd.emit("audio-levels", LevelsPayload { mic, system });
                }
                CaptureEvent::Error(message) => {
                    tracing::warn!(%message, "aviso de dictado");
                    let _ = app_fwd.emit("capture-error", ErrorPayload { message });
                }
            }
        }
    });

    *state.dictation.lock_or_recover() = Some(ActiveDictation {
        wav_path,
        temp_dir,
        handle,
    });

    let (ui_sounds, out, voice) = {
        let cfg = state.config.lock_or_recover();
        (
            cfg.ui_sounds,
            cfg.output_device_id.clone(),
            cfg.sound_dictation_start.clone(),
        )
    };
    if ui_sounds {
        crate::beep::play(crate::beep::SoundAction::DictationStart, &voice, &out);
    }

    // Seguir el foco mientras dictás: si hacés clic en otro input antes de que
    // el texto esté listo, ese pasa a ser el destino.
    crate::clipboard_history::start_foreground_tracking();

    emit_status(app, DictationPhase::Listening, None);
    Ok(())
}

fn stop_and_paste(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(active) = state.dictation.lock_or_recover().take() else {
        return;
    };

    emit_status(app, DictationPhase::Transcribing, None);

    let app2 = app.clone();
    thread::spawn(move || {
        let total_started = Instant::now();
        let summary = active.handle.stop();
        let cleanup = active.temp_dir.clone();

        let result = (|| -> Result<(String, crate::paste_queue::PasteOutcome), String> {
            if !summary.mic_written || !active.wav_path.exists() {
                return Err("No se capturó audio. Intenta de nuevo.".into());
            }
            if summary.duration_secs < 0.4 {
                return Err("Dictado demasiado corto.".into());
            }

            let state = app2.state::<AppState>();
            let cfg = state.config.lock_or_recover().clone();
            let language = if cfg.language == "auto" {
                None
            } else {
                Some(cfg.language.as_str())
            };

            let whisper_started = Instant::now();
            let groq_key = if cfg.dictation_backend == "groq" {
                resolve_groq_api_key()
            } else {
                None
            };
            let text = if let Some(api_key) = groq_key {
                let text = transcribe::transcribe_groq(
                    &api_key,
                    &active.wav_path,
                    language,
                    &cfg.dictation_groq_model,
                )
                .map_err(|e| e.to_string())?;
                tracing::info!(
                    audio_secs = summary.duration_secs,
                    whisper_ms = whisper_started.elapsed().as_millis(),
                    backend = "groq",
                    model = %cfg.dictation_groq_model,
                    "dictado transcrito"
                );
                text
            } else {
                if cfg.dictation_backend == "groq" {
                    tracing::warn!(
                        "dictado Groq sin API key; fallback a Whisper local. {}",
                        GROQ_KEY_REQUIRED_MSG
                    );
                }
                let model_path = transcribe::models::require_downloaded(
                    &state.dirs.models_dir(),
                    &cfg.dictation_whisper_model,
                )
                .map_err(|e| {
                    if cfg.dictation_backend == "groq" {
                        format!(
                            "{GROQ_KEY_REQUIRED_MSG} Mientras tanto, descarga un modelo local: {e}"
                        )
                    } else {
                        e.to_string()
                    }
                })?;
                let loaded = crate::state::get_or_load_whisper(&state, &model_path)?;
                let tracks = [TrackInput {
                    wav: &active.wav_path,
                    speaker: Speaker::Me,
                }];
                let transcript = transcribe::transcribe_with_model(
                    &loaded.model,
                    &tracks,
                    language,
                    transcribe::TranscribeMode::Dictation,
                    |_| {},
                )
                .map_err(|e| e.to_string())?;
                tracing::info!(
                    audio_secs = summary.duration_secs,
                    whisper_ms = whisper_started.elapsed().as_millis(),
                    backend = "local",
                    "dictado transcrito"
                );
                transcript
                    .segments
                    .iter()
                    .map(|s| s.text.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string()
            };

            if text.is_empty() {
                return Err("No se detectó habla. Prueba otra vez.".into());
            }

            // Congelar el destino: de acá en más el foco lo movemos nosotros.
            crate::clipboard_history::stop_foreground_tracking();

            // Si ya estás en una ventana externa, ESA es la que querés: no
            // tocar el foco. Si no, restaurar el destino guardado al arrancar
            // (o el último clic externo durante el dictado). Agentes abierto
            // no corta este camino: try_paste_or_enqueue pega afuera primero
            // y solo cae al compositor de agentes si no hay destino externo.
            if crate::clipboard_history::has_live_external_foreground() {
                crate::clipboard_history::log_focus_state();
            } else if crate::clipboard_history::has_saved_external_paste_target() {
                crate::clipboard_history::focus_paste_target();
                thread::sleep(Duration::from_millis(220));
                crate::clipboard_history::log_focus_state();
            }
            let outcome = crate::paste_queue::try_paste_or_enqueue(&app2, &text)?;
            Ok((text, outcome))
        })();

        // Red de seguridad: el camino feliz ya lo cortó antes de pegar, pero si
        // la transcripción falló se sale por arriba y el hilo quedaría vivo.
        crate::clipboard_history::stop_foreground_tracking();
        let _ = std::fs::remove_dir_all(cleanup);

        match result {
            Ok((text, outcome)) => {
                let elapsed_ms = total_started.elapsed().as_millis();
                let queued = outcome == crate::paste_queue::PasteOutcome::Queued;
                tracing::info!(
                    len = text.len(),
                    elapsed_ms,
                    queued,
                    "dictado pegado o encolado"
                );
                let (ui_sounds, out, voice) = {
                    let state = app2.state::<AppState>();
                    let cfg = state.config.lock_or_recover();
                    (
                        cfg.ui_sounds,
                        cfg.output_device_id.clone(),
                        cfg.sound_dictation_done.clone(),
                    )
                };
                if ui_sounds {
                    crate::beep::play(crate::beep::SoundAction::DictationDone, &voice, &out);
                }
                let message = if queued {
                    format!("En cola para pegar ({} caracteres)", text.chars().count())
                } else {
                    format!(
                        "Pegado ({} caracteres · {:.1} s)",
                        text.chars().count(),
                        elapsed_ms as f64 / 1_000.0
                    )
                };
                emit_status(&app2, DictationPhase::Pasted, Some(message));
                // Vuelve a idle tras un momento para no dejar la pill en "Pegado".
                thread::sleep(Duration::from_millis(1600));
                emit_status(&app2, DictationPhase::Idle, None);
            }
            Err(message) => {
                tracing::warn!(%message, "dictado falló");
                emit_status(&app2, DictationPhase::Error, Some(message.clone()));
                let _ = app2.emit("capture-error", ErrorPayload { message });
                thread::sleep(Duration::from_millis(2200));
                emit_status(&app2, DictationPhase::Idle, None);
            }
        }
    });
}

/// Nivel mínimo de NS para dictado: `off`/`low` → `medium`; respeta `high`.
fn effective_dictation_noise(configured: &str) -> String {
    match configured {
        "high" => "high".into(),
        "medium" => "medium".into(),
        _ => "medium".into(),
    }
}
