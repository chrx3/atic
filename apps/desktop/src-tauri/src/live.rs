//! Worker de transcripción en vivo durante una grabación de reunión.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use atic_audio::{AudioTapChunk, CaptureTrack};
use atic_core::{secrets, SecretKind, Segment, Speaker};
use atic_transcribe::{LiveEngine, LivePcmChunk, LiveSttBackend, LiveUpdate};

use crate::state::{get_or_load_whisper, AppState};
use atic_core::MutexExt;

const GROQ_LIVE_KEY_REQUIRED_MSG: &str =
    "Configura tu API key de Groq en Ajustes para usar el motor en la nube.";

#[derive(Clone, Serialize)]
pub struct LiveSegmentPayload {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Speaker,
    pub text: String,
}

#[derive(Clone, Serialize)]
pub struct LiveErrorPayload {
    pub message: String,
}

impl From<&Segment> for LiveSegmentPayload {
    fn from(s: &Segment) -> Self {
        Self {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            speaker: s.speaker,
            text: s.text.clone(),
        }
    }
}

/// Handle para unir el worker al detener la captura.
pub struct LiveWorkerHandle {
    join: JoinHandle<()>,
    /// Si es true, el worker deja de pedir ventanas nuevas / omite el flush.
    cancel: Arc<AtomicBool>,
}

impl LiveWorkerHandle {
    /// Cancela la vista previa y espera su salida en otro hilo antes de continuar.
    /// Esto libera el modelo local sin bloquear el stop ni la interfaz.
    pub fn stop_preview_in_background<F>(self, on_stopped: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.cancel.store(true, Ordering::Relaxed);
        let join = self.join;
        let callback = Arc::new(Mutex::new(Some(on_stopped)));
        let callback_bg = Arc::clone(&callback);
        if let Err(err) = thread::Builder::new()
            .name("live-stt-stop".into())
            .spawn(move || {
                if join.join().is_err() {
                    tracing::warn!("el worker de vista previa terminó inesperadamente");
                }
                if let Some(callback) = callback_bg.lock_or_recover().take() {
                    callback();
                }
            })
        {
            tracing::warn!(%err, "no se pudo crear el hilo de cierre de vista previa");
            if let Some(callback) = callback.lock_or_recover().take() {
                callback();
            }
        }
    }
}

enum LiveWorkerMode {
    Local(std::sync::Arc<atic_transcribe::LoadedModel>),
    Groq { api_key: String, model: String },
}

fn resolve_groq_api_key() -> Option<String> {
    if let Ok(Some(key)) = secrets::get_secret(SecretKind::GroqApiKey) {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Arranca el worker que consume el tap PCM y emite eventos a la UI.
///
/// `live_engine`: `local` | `groq`. Si es `groq` sin API key, cae a Whisper
/// local cuando el modelo está disponible; si no, falla con mensaje claro.
pub fn spawn_live_worker(
    app: AppHandle,
    tap_rx: Receiver<AudioTapChunk>,
    language: Option<String>,
    model_id: &str,
    live_engine: &str,
    groq_model: &str,
) -> Result<LiveWorkerHandle, String> {
    let want_groq = live_engine.eq_ignore_ascii_case("groq");
    let groq_key = if want_groq {
        resolve_groq_api_key()
    } else {
        None
    };

    let mode = if let Some(api_key) = groq_key {
        LiveWorkerMode::Groq {
            api_key,
            model: atic_transcribe::normalize_groq_whisper_model(groq_model).to_string(),
        }
    } else {
        if want_groq {
            let _ = app.emit(
                "live-transcript-error",
                LiveErrorPayload {
                    message: GROQ_LIVE_KEY_REQUIRED_MSG.into(),
                },
            );
        }
        let state = app.state::<AppState>();
        let model_path =
            atic_transcribe::models::require_downloaded(&state.dirs.models_dir(), model_id)
                .map_err(|e| {
                    if want_groq {
                        format!("{GROQ_LIVE_KEY_REQUIRED_MSG} (fallback local no disponible: {e})")
                    } else {
                        format!("Modelo live «{model_id}» no disponible: {e}")
                    }
                })?;
        let loaded = get_or_load_whisper(&state, &model_path)?;
        LiveWorkerMode::Local(loaded)
    };

    let lang = language;
    let app2 = app.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    let join = thread::Builder::new()
        .name("live-stt".into())
        .spawn(move || run_live_worker(app2, tap_rx, mode, lang, cancel_worker))
        .map_err(|e| e.to_string())?;

    Ok(LiveWorkerHandle { join, cancel })
}

fn run_live_worker(
    app: AppHandle,
    tap_rx: Receiver<AudioTapChunk>,
    mode: LiveWorkerMode,
    language: Option<String>,
    cancel: Arc<AtomicBool>,
) {
    let mut engine = LiveEngine::new(language);

    while let Ok(chunk) = tap_rx.recv() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let pcm = to_live_chunk(chunk);
        let backend = backend_for(&mode);
        match engine.push(backend, pcm) {
            Ok(updates) => apply_updates(&app, updates),
            Err(err) => {
                tracing::error!(%err, "error en live STT");
                let _ = app.emit(
                    "live-transcript-error",
                    LiveErrorPayload {
                        message: err.to_string(),
                    },
                );
            }
        }
    }

    // Si el tap termina por sí solo, completa la última ventana de la vista previa.
    // Al detener manualmente se cancela para dar prioridad al transcript final.
    if !cancel.load(Ordering::Relaxed) {
        let backend = backend_for(&mode);
        match engine.flush(backend) {
            Ok(updates) => apply_updates(&app, updates),
            Err(err) => {
                tracing::error!(%err, "error en flush live STT");
                let _ = app.emit(
                    "live-transcript-error",
                    LiveErrorPayload {
                        message: err.to_string(),
                    },
                );
            }
        }
    }
}

fn backend_for(mode: &LiveWorkerMode) -> LiveSttBackend<'_> {
    match mode {
        LiveWorkerMode::Local(loaded) => LiveSttBackend::Local(&loaded.model),
        LiveWorkerMode::Groq { api_key, model } => LiveSttBackend::Groq { api_key, model },
    }
}

fn to_live_chunk(chunk: AudioTapChunk) -> LivePcmChunk {
    LivePcmChunk {
        speaker: match chunk.track {
            CaptureTrack::Mic => Speaker::Me,
            CaptureTrack::System => Speaker::Others,
        },
        start_ms: chunk.start_ms,
        sample_rate: chunk.sample_rate,
        channels: chunk.channels,
        samples: chunk.samples,
    }
}

fn apply_updates(app: &AppHandle, updates: Vec<LiveUpdate>) {
    for update in updates {
        match update {
            LiveUpdate::Partial(seg) => {
                let _ = app.emit("live-transcript-partial", LiveSegmentPayload::from(&seg));
            }
            LiveUpdate::Final(seg) => {
                let _ = app.emit("live-transcript-final", LiveSegmentPayload::from(&seg));
            }
        }
    }
}
