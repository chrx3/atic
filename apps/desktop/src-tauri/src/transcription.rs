//! Comandos y eventos de transcripción (gestión de modelos + Whisper local).

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::{secrets, RecordingStatus, SecretKind, Speaker, Transcript};
use atic_transcribe::{self as transcribe, TrackInput};

use crate::state::AppState;
use atic_core::MutexExt;

#[derive(Clone, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub display_name: String,
    pub approx_size_bytes: u64,
    pub downloaded: bool,
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
struct IdPayload {
    id: String,
}

#[derive(Clone, Serialize)]
struct ErrorIdPayload {
    id: String,
    message: String,
}

#[derive(Clone, Serialize)]
struct TranscribeProgress {
    id: String,
    progress: f32,
}

/// Lista el catálogo de modelos con su estado de descarga.
#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelStatus> {
    let dir = state.dirs.models_dir();
    transcribe::CATALOG
        .iter()
        .map(|m| ModelStatus {
            id: m.id.to_string(),
            display_name: m.display_name.to_string(),
            approx_size_bytes: m.approx_size_bytes,
            downloaded: transcribe::models::is_downloaded(&dir, m),
        })
        .collect()
}

/// ¿Hay lo necesario para transcribir reuniones y dictar?
#[tauri::command]
pub fn current_model_ready(state: State<AppState>) -> bool {
    let cfg = state.config.lock_or_recover().clone();
    let dir = state.dirs.models_dir();
    let meeting_ok = if cfg.meeting_backend == "groq" {
        true
    } else {
        transcribe::models::require_downloaded(&dir, &cfg.whisper_model).is_ok()
    };
    // Groq: key del llavero; no bloquea el arranque.
    let dictation_ok = if cfg.dictation_backend == "groq" {
        true
    } else {
        transcribe::models::require_downloaded(&dir, &cfg.dictation_whisper_model).is_ok()
    };
    let live_ok = if !cfg.live_transcription {
        true
    } else if cfg.live_engine == "groq" {
        let has_groq_key = atic_core::secrets::get_secret(atic_core::SecretKind::GroqApiKey)
            .ok()
            .flatten()
            .is_some_and(|k| !k.trim().is_empty());
        has_groq_key
            || transcribe::models::require_downloaded(&dir, &cfg.live_whisper_model).is_ok()
    } else {
        transcribe::models::require_downloaded(&dir, &cfg.live_whisper_model).is_ok()
    };
    meeting_ok && dictation_ok && live_ok
}

/// Descarga un modelo en segundo plano, emitiendo progreso.
#[tauri::command]
pub fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let info = *transcribe::models::find(&id).ok_or_else(|| {
        crate::ui_lang::msg(
            &format!("Modelo desconocido: {id}"),
            &format!("Unknown model: {id}"),
        )
    })?;
    let models_dir = state.dirs.models_dir();

    if transcribe::models::is_downloaded(&models_dir, &info) {
        let _ = app.emit("model-download-done", IdPayload { id });
        return Ok(());
    }

    let app2 = app.clone();
    std::thread::spawn(move || {
        let last = AtomicU64::new(0);
        let result = transcribe::models::download(&models_dir, &info, |downloaded, total| {
            let prev = last.load(Ordering::Relaxed);
            // Throttle: emitir cada ~2 MB o al terminar.
            if downloaded.saturating_sub(prev) >= 2_000_000 || downloaded == total {
                last.store(downloaded, Ordering::Relaxed);
                let _ = app2.emit(
                    "model-download-progress",
                    DownloadProgress {
                        id: info.id.to_string(),
                        downloaded,
                        total,
                    },
                );
            }
        });
        match result {
            Ok(()) => {
                let _ = app2.emit(
                    "model-download-done",
                    IdPayload {
                        id: info.id.to_string(),
                    },
                );
                // Si es un modelo activo (reuniones o dictado), precargarlo.
                let state = app2.state::<AppState>();
                let cfg = state.config.lock_or_recover().clone();
                if cfg.whisper_model == info.id
                    || cfg.dictation_whisper_model == info.id
                    || cfg.live_whisper_model == info.id
                {
                    crate::state::preload_whisper_async(&app2);
                }
            }
            Err(err) => {
                let _ = app2.emit(
                    "model-download-error",
                    ErrorIdPayload {
                        id: info.id.to_string(),
                        message: err.to_ui(crate::ui_lang::english()),
                    },
                );
            }
        }
    });
    Ok(())
}

/// Transcribe una grabación en segundo plano.
#[tauri::command]
pub fn transcribe_recording(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();

    let rec = state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|e| e.to_ui(crate::ui_lang::english()))?
        .ok_or_else(crate::ui_lang::rec_missing)?;

    let cfg = state.config.lock_or_recover().clone();
    let engine = if cfg.meeting_backend == "groq" {
        let api_key = resolve_groq_api_key().ok_or_else(|| {
            crate::ui_lang::msg(
                "Configurá tu API key de Groq en Ajustes, o pasá la transcripción a Local.",
                "Set your Groq API key in Settings, or switch transcription to Local.",
            )
        })?;
        MeetingEngine::Groq {
            api_key,
            model: cfg.meeting_groq_model.clone(),
        }
    } else {
        let model_path =
            transcribe::models::require_downloaded(&state.dirs.models_dir(), &cfg.whisper_model)
                .map_err(|e| e.to_ui(crate::ui_lang::english()))?;
        MeetingEngine::Local(model_path)
    };

    let want = cfg.effective_transcribe_tracks();
    let want_mic = want == "both" || want == "mic";
    let want_system = want == "both" || want == "system";

    let dir = state.dirs.recording_dir(&id);
    let transcript_path = state.dirs.transcript_path(&id);
    let mic = if want_mic {
        rec.mic_path.as_ref().map(|_| dir.join("mic.wav"))
    } else {
        None
    };
    let system = if want_system {
        rec.system_path.as_ref().map(|_| dir.join("system.wav"))
    } else {
        None
    };

    if mic.is_none() && system.is_none() {
        return Err(crate::ui_lang::msg(
            "No hay pistas para transcribir con la configuración actual (yo / otros).",
            "There are no tracks to transcribe with the current settings (me / others).",
        ));
    }

    let language = if cfg.language == "auto" {
        None
    } else {
        Some(cfg.language.clone())
    };

    state
        .db
        .lock_or_recover()
        .update_status(&id, RecordingStatus::Transcribing)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("recordings-changed", ());

    let app2 = app.clone();
    std::thread::spawn(move || {
        run_transcription(app2, id, engine, mic, system, language, transcript_path);
    });
    Ok(())
}

fn resolve_groq_api_key() -> Option<String> {
    secrets::get_secret(SecretKind::GroqApiKey)
        .ok()
        .flatten()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

enum MeetingEngine {
    Local(PathBuf),
    Groq { api_key: String, model: String },
}

#[allow(clippy::too_many_arguments)]
fn run_transcription(
    app: AppHandle,
    id: String,
    engine: MeetingEngine,
    mic: Option<PathBuf>,
    system: Option<PathBuf>,
    language: Option<String>,
    transcript_path: PathBuf,
) {
    let mut tracks = Vec::new();
    if let Some(p) = &mic {
        tracks.push(TrackInput {
            wav: p,
            speaker: Speaker::Me,
        });
    }
    if let Some(p) = &system {
        tracks.push(TrackInput {
            wav: p,
            speaker: Speaker::Others,
        });
    }

    let last = Arc::new(AtomicI32::new(-1));
    let app_prog = app.clone();
    let id_prog = id.clone();
    let on_progress = move |progress: f32| {
        let pct = (progress * 100.0) as i32;
        if pct != last.load(Ordering::Relaxed) {
            last.store(pct, Ordering::Relaxed);
            let _ = app_prog.emit(
                "transcribe-progress",
                TranscribeProgress {
                    id: id_prog.clone(),
                    progress,
                },
            );
        }
    };

    let result = match engine {
        MeetingEngine::Local(model_path) => {
            let state = app.state::<AppState>();
            match crate::state::get_or_load_whisper(&state, &model_path) {
                Ok(loaded) => transcribe::transcribe_with_model(
                    &loaded.model,
                    &tracks,
                    language.as_deref(),
                    transcribe::TranscribeMode::Meeting,
                    on_progress,
                ),
                Err(message) => {
                    let _ = state
                        .db
                        .lock_or_recover()
                        .update_status(&id, RecordingStatus::Error);
                    let _ = app.emit(
                        "transcribe-error",
                        ErrorIdPayload {
                            id: id.clone(),
                            message,
                        },
                    );
                    let _ = app.emit("recordings-changed", ());
                    return;
                }
            }
        }
        MeetingEngine::Groq { api_key, model } => transcribe::transcribe_groq_recording(
            &api_key,
            &tracks,
            language.as_deref(),
            &model,
            on_progress,
        ),
    };

    let state = app.state::<AppState>();
    match result {
        Ok(transcript) if transcript.segments.is_empty() => {
            tracing::warn!(%id, "la transcripción terminó sin segmentos de texto");
            let _ = state
                .db
                .lock_or_recover()
                .update_status(&id, RecordingStatus::Error);
            let _ = app.emit(
                "transcribe-error",
                ErrorIdPayload {
                    id: id.clone(),
                    message: crate::ui_lang::msg(
                        "No se produjo texto; prueba re-transcribir o revisá la pista y el idioma.",
                        "No text was produced; try transcribing again or check the track and language.",
                    ),
                },
            );
        }
        Ok(transcript) => {
            if let Err(err) = transcript.save(&transcript_path) {
                tracing::error!(%err, "no se pudo guardar la transcripción");
                let _ = state
                    .db
                    .lock_or_recover()
                    .update_status(&id, RecordingStatus::Error);
                let _ = app.emit(
                    "transcribe-error",
                    ErrorIdPayload {
                        id: id.clone(),
                        message: crate::ui_lang::msg(
                            &format!("No se pudo guardar la transcripción: {err}"),
                            &format!("Could not save the transcript: {err}"),
                        ),
                    },
                );
            } else {
                let _ = state
                    .db
                    .lock_or_recover()
                    .update_status(&id, RecordingStatus::Transcribed);
                let _ = app.emit("transcript-ready", IdPayload { id: id.clone() });
            }
        }
        Err(err) => {
            let _ = state
                .db
                .lock_or_recover()
                .update_status(&id, RecordingStatus::Error);
            let _ = app.emit(
                "transcribe-error",
                ErrorIdPayload {
                    id: id.clone(),
                    message: err.to_ui(crate::ui_lang::english()),
                },
            );
        }
    }
    let _ = app.emit("recordings-changed", ());
}

/// Devuelve la transcripción guardada de una grabación, si existe.
#[tauri::command]
pub fn get_transcript(state: State<AppState>, id: String) -> Result<Option<Transcript>, String> {
    Transcript::load(&state.dirs.transcript_path(&id)).map_err(|e| e.to_string())
}

/// Guarda correcciones humanas y deja cualquier resumen anterior como pendiente.
#[tauri::command]
pub fn save_transcript(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    mut transcript: Transcript,
) -> Result<(), String> {
    state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(crate::ui_lang::rec_missing)?;

    if transcript.segments.len() > 50_000 {
        return Err(crate::ui_lang::msg(
            "La transcripción supera el máximo de 50.000 fragmentos.",
            "The transcript exceeds the 50,000-segment limit.",
        ));
    }
    if transcript
        .language
        .as_ref()
        .is_some_and(|value| value.len() > 16)
    {
        return Err(crate::ui_lang::msg(
            "El código de idioma es demasiado largo.",
            "The language code is too long.",
        ));
    }

    let mut total_chars = 0usize;
    transcript.segments.retain_mut(|segment| {
        segment.text = segment.text.trim().to_string();
        if segment.text.is_empty() {
            return false;
        }
        total_chars = total_chars.saturating_add(segment.text.chars().count());
        segment.start_ms = segment.start_ms.max(0);
        segment.end_ms = segment.end_ms.max(segment.start_ms + 1);
        segment.speaker_name = segment
            .speaker_name
            .take()
            .map(|name| name.trim().chars().take(80).collect::<String>())
            .filter(|name| !name.is_empty());
        true
    });
    if total_chars > 5_000_000 {
        return Err(crate::ui_lang::msg(
            "La transcripción supera el máximo de cinco millones de caracteres.",
            "The transcript exceeds the five-million-character limit.",
        ));
    }
    transcript.sort();
    transcript
        .save(&state.dirs.transcript_path(&id))
        .map_err(|error| error.to_string())?;

    let summary_path = state.dirs.summary_path(&id);
    if summary_path.exists() {
        std::fs::remove_file(&summary_path).map_err(|error| error.to_string())?;
    }
    state
        .db
        .lock_or_recover()
        .update_status(&id, RecordingStatus::Transcribed)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("transcript-ready", IdPayload { id: id.clone() });
    let _ = app.emit("recordings-changed", ());
    Ok(())
}
