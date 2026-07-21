//! Transcripción de audio a texto, 100% local (whisper.cpp vía whisper-rs).
//!
//! Fase 2: gestor de modelos (descarga bajo demanda), decodificación de WAV a
//! mono 16 kHz y transcripción por pista con fusión de segmentos.

pub mod cloud;
pub mod decode;
pub mod error;
pub mod import;
pub mod live;
pub mod models;
pub mod whisper;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atic_core::{Speaker, Transcript};

pub use cloud::{
    normalize_groq_whisper_model, transcribe_groq, transcribe_groq_pcm, GROQ_DICTATION_MODEL,
    GROQ_WHISPER_MODELS,
};
pub use error::{Result, TranscribeError};
pub use import::import_audio_to_wav;
pub use live::{LiveEngine, LivePcmChunk, LiveSttBackend, LiveUpdate};
pub use models::{ModelInfo, CATALOG};
pub use whisper::{TranscribeMode, WhisperModel};

/// Una pista a transcribir con su hablante asociado.
pub struct TrackInput<'a> {
    pub wav: &'a Path,
    pub speaker: Speaker,
}

/// Modelo cargado en memoria, reutilizable entre dictados/transcripciones.
///
/// Cargar GGML desde disco (~100–500+ MB) es el coste fijo dominante; mantener
/// esta instancia viva evita ese hit en cada dictado.
pub struct LoadedModel {
    pub path: PathBuf,
    pub model: WhisperModel,
}

impl LoadedModel {
    pub fn load(model_path: &Path) -> Result<Self> {
        Ok(Self {
            path: model_path.to_path_buf(),
            model: WhisperModel::load(model_path)?,
        })
    }

    pub fn matches_path(&self, model_path: &Path) -> bool {
        self.path == model_path
    }
}

/// Transcribe una grabación completa: decodifica cada pista, la pasa por
/// Whisper y fusiona todos los segmentos ordenados por tiempo.
///
/// Carga el modelo en cada llamada. Preferir [`transcribe_with_model`] cuando
/// el modelo ya está en memoria (dictado / sesiones repetidas).
///
/// `language` en `None` activa autodetección. `progress` recibe el avance
/// global 0.0..1.0 (se comparte con el callback interno de Whisper, por eso es
/// `Fn + Send + Sync + 'static`).
pub fn transcribe_recording(
    model_path: &Path,
    tracks: &[TrackInput<'_>],
    language: Option<&str>,
    progress: impl Fn(f32) + Send + Sync + 'static,
) -> Result<Transcript> {
    let loaded = LoadedModel::load(model_path)?;
    transcribe_with_model(
        &loaded.model,
        tracks,
        language,
        TranscribeMode::Meeting,
        progress,
    )
}

/// Como [`transcribe_recording`], pero reutiliza un [`WhisperModel`] ya cargado
/// y permite elegir el perfil de decode ([`TranscribeMode`]).
pub fn transcribe_with_model(
    model: &WhisperModel,
    tracks: &[TrackInput<'_>],
    language: Option<&str>,
    mode: TranscribeMode,
    progress: impl Fn(f32) + Send + Sync + 'static,
) -> Result<Transcript> {
    let progress = Arc::new(progress);

    let mut transcript = Transcript {
        language: language.map(|s| s.to_string()),
        segments: Vec::new(),
    };

    let track_count = tracks.len().max(1) as f32;
    for (idx, track) in tracks.iter().enumerate() {
        let samples = decode::load_wav_mono_16k(track.wav)?;
        let samples = if mode == TranscribeMode::Dictation {
            let trimmed = decode::trim_dictation_silence(&samples);
            tracing::debug!(
                original_samples = samples.len(),
                dictation_samples = trimmed.len(),
                "audio de dictado preparado para Whisper"
            );
            trimmed
        } else {
            samples
        };
        if samples.is_empty() {
            continue;
        }
        let base = idx as f32 / track_count;
        let prog = progress.clone();
        let segs = model.transcribe_track(&samples, track.speaker, language, mode, move |p| {
            prog(base + (p as f32 / 100.0) / track_count);
        })?;
        transcript.segments.extend(segs);
    }

    transcript.sort();
    progress(1.0);
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_base() {
        assert!(models::find("base").is_some());
        assert!(models::find("inexistente").is_none());
    }
}
