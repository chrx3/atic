//! Transcripción en vivo por ventanas.
//!
//! Acumula PCM por pista, aplica un gate RMS simple, transcribe ventanas de
//! ~6 s con solape de ~1 s y deduplica el texto del solape.
//! Backend: Whisper local (greedy) o Groq (PCM en memoria).

use atic_core::{Segment, Speaker, Transcript};

use crate::cloud;
use crate::decode::{pcm_to_mono_16k, WHISPER_RATE};
use crate::error::Result;
use crate::whisper::{TranscribeMode, WhisperModel};

/// Backend de STT para cada ventana del motor live.
#[derive(Clone, Copy)]
pub enum LiveSttBackend<'a> {
    Local(&'a WhisperModel),
    /// API key de Groq del usuario (BYOK).
    Groq {
        api_key: &'a str,
    },
}

impl LiveSttBackend<'_> {
    fn transcribe_window(
        &self,
        pcm: &[f32],
        language: Option<&str>,
        speaker: Speaker,
    ) -> Result<String> {
        match self {
            LiveSttBackend::Local(model) => {
                let segs = model.transcribe_track(
                    pcm,
                    speaker,
                    language,
                    TranscribeMode::Dictation,
                    |_| {},
                )?;
                Ok(segs
                    .iter()
                    .map(|s| s.text.trim())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            LiveSttBackend::Groq { api_key } => {
                cloud::transcribe_groq_pcm(api_key, pcm, WHISPER_RATE, language)
            }
        }
    }
}

/// Ventana de análisis (~6 s @ 16 kHz).
const WINDOW_SAMPLES: usize = 6 * WHISPER_RATE as usize;
/// Solape entre ventanas consecutivas (~1 s).
const OVERLAP_SAMPLES: usize = WHISPER_RATE as usize;
const STEP_SAMPLES: usize = WINDOW_SAMPLES - OVERLAP_SAMPLES;
/// Umbral RMS alineado con el gate `high` de `atic-audio::noise`.
const ENERGY_GATE_RMS: f32 = 0.008;

/// Bloque PCM crudo (cualquier rate/canales) hacia el motor live.
#[derive(Debug, Clone)]
pub struct LivePcmChunk {
    pub speaker: Speaker,
    pub start_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

/// Actualización emitida por el motor (parcial = borrador; final = consolidado).
#[derive(Debug, Clone)]
pub enum LiveUpdate {
    Partial(Segment),
    Final(Segment),
}

/// Motor de live: un buffer por hablante.
pub struct LiveEngine {
    language: Option<String>,
    me: TrackState,
    others: TrackState,
}

struct TrackState {
    speaker: Speaker,
    pcm: Vec<f32>,
    /// Tiempo absoluto (ms) correspondiente a `pcm[0]`.
    origin_ms: i64,
    /// Si ya recibimos el primer chunk (para anclar `origin_ms`).
    started: bool,
    last_text: String,
}

impl LiveEngine {
    pub fn new(language: Option<String>) -> Self {
        Self {
            language,
            me: TrackState::new(Speaker::Me),
            others: TrackState::new(Speaker::Others),
        }
    }

    /// Incorpora un chunk y, si hay ventanas listas, las transcribe.
    pub fn push(
        &mut self,
        backend: LiveSttBackend<'_>,
        chunk: LivePcmChunk,
    ) -> Result<Vec<LiveUpdate>> {
        let mono = pcm_to_mono_16k(&chunk.samples, chunk.channels, chunk.sample_rate);
        if mono.is_empty() {
            return Ok(Vec::new());
        }
        let track = self.track_mut(chunk.speaker);
        track.append(chunk.start_ms, &mono);
        self.drain_windows(backend, chunk.speaker)
    }

    /// Procesa el audio restante al detener la grabación.
    pub fn flush(&mut self, backend: LiveSttBackend<'_>) -> Result<Vec<LiveUpdate>> {
        let mut out = Vec::new();
        for speaker in [Speaker::Me, Speaker::Others] {
            out.extend(self.flush_track(backend, speaker)?);
        }
        Ok(out)
    }

    /// Segmentos finales acumulados (solo los `Final` que el caller haya guardado).
    pub fn into_transcript(segments: Vec<Segment>, language: Option<String>) -> Transcript {
        let mut transcript = Transcript { language, segments };
        transcript.sort();
        transcript
    }

    fn track_mut(&mut self, speaker: Speaker) -> &mut TrackState {
        match speaker {
            Speaker::Me => &mut self.me,
            Speaker::Others => &mut self.others,
        }
    }

    fn drain_windows(
        &mut self,
        backend: LiveSttBackend<'_>,
        speaker: Speaker,
    ) -> Result<Vec<LiveUpdate>> {
        let mut updates = Vec::new();
        loop {
            let ready = {
                let track = self.track_mut(speaker);
                track.pcm.len() >= WINDOW_SAMPLES
            };
            if !ready {
                break;
            }
            updates.extend(self.process_window(backend, speaker, false)?);
        }
        Ok(updates)
    }

    fn flush_track(
        &mut self,
        backend: LiveSttBackend<'_>,
        speaker: Speaker,
    ) -> Result<Vec<LiveUpdate>> {
        let mut updates = Vec::new();
        // Vaciar ventanas completas pendientes.
        updates.extend(self.drain_windows(backend, speaker)?);
        // Último remanente (mínimo ~1.5 s para no mandar ruido corto).
        let min_tail = WHISPER_RATE as usize + WHISPER_RATE as usize / 2;
        let has_tail = {
            let track = self.track_mut(speaker);
            track.pcm.len() >= min_tail
        };
        if has_tail {
            updates.extend(self.process_window(backend, speaker, true)?);
        }
        Ok(updates)
    }

    fn process_window(
        &mut self,
        backend: LiveSttBackend<'_>,
        speaker: Speaker,
        is_tail: bool,
    ) -> Result<Vec<LiveUpdate>> {
        let language = self.language.clone();
        let (window, origin_ms, prev_text, take) = {
            let track = self.track_mut(speaker);
            let take = if is_tail {
                track.pcm.len()
            } else {
                WINDOW_SAMPLES
            };
            let window = track.pcm[..take].to_vec();
            let origin_ms = track.origin_ms;
            let prev_text = track.last_text.clone();
            (window, origin_ms, prev_text, take)
        };

        let rms = window_rms(&window);
        if rms < ENERGY_GATE_RMS {
            self.advance_track(speaker, take, is_tail);
            return Ok(Vec::new());
        }

        let end_ms = origin_ms + samples_to_ms(take as i64);
        let raw_text = backend.transcribe_window(&window, language.as_deref(), speaker)?;
        if raw_text.is_empty() {
            self.advance_track(speaker, take, is_tail);
            return Ok(Vec::new());
        }

        let partial = Segment {
            start_ms: origin_ms,
            end_ms,
            speaker,
            speaker_name: None,
            text: raw_text.clone(),
        };

        let deduped = strip_overlap_prefix(&prev_text, &raw_text);
        self.advance_track(speaker, take, is_tail);

        let mut updates = vec![LiveUpdate::Partial(partial)];
        if !deduped.is_empty() {
            let start_ms = if prev_text.is_empty() {
                origin_ms
            } else {
                // Tras el primer segmento, el solape (~1 s) ya se emitió.
                origin_ms + samples_to_ms(OVERLAP_SAMPLES as i64)
            };
            let final_seg = Segment {
                start_ms: start_ms.min(end_ms.saturating_sub(200)),
                end_ms,
                speaker,
                speaker_name: None,
                text: deduped.clone(),
            };
            self.track_mut(speaker).last_text = if prev_text.is_empty() {
                deduped
            } else {
                format!("{prev_text} {deduped}")
            };
            // Mantener last_text acotado (últimas ~40 palabras) para dedupe.
            {
                let track = self.track_mut(speaker);
                let words: Vec<&str> = track.last_text.split_whitespace().collect();
                if words.len() > 40 {
                    track.last_text = words[words.len() - 40..].join(" ");
                }
            }
            updates.push(LiveUpdate::Final(final_seg));
        }
        Ok(updates)
    }

    fn advance_track(&mut self, speaker: Speaker, take: usize, is_tail: bool) {
        let track = self.track_mut(speaker);
        if is_tail {
            track.pcm.clear();
            return;
        }
        let drain = take.min(STEP_SAMPLES).min(track.pcm.len());
        if drain == 0 {
            return;
        }
        track.pcm.drain(..drain);
        track.origin_ms += samples_to_ms(drain as i64);
    }
}

impl TrackState {
    fn new(speaker: Speaker) -> Self {
        Self {
            speaker,
            pcm: Vec::new(),
            origin_ms: 0,
            started: false,
            last_text: String::new(),
        }
    }

    fn append(&mut self, chunk_start_ms: i64, mono_16k: &[f32]) {
        if !self.started {
            self.origin_ms = chunk_start_ms;
            self.started = true;
            self.pcm.extend_from_slice(mono_16k);
            return;
        }
        // Si hubo drops en el tap, rellenar silencio para no desfasar tiempos.
        let expected_ms = self.origin_ms + samples_to_ms(self.pcm.len() as i64);
        let gap_ms = chunk_start_ms - expected_ms;
        if gap_ms > 40 {
            let pad = (gap_ms * WHISPER_RATE as i64 / 1000).max(0) as usize;
            self.pcm.resize(self.pcm.len() + pad, 0.0);
        }
        self.pcm.extend_from_slice(mono_16k);
        let _ = self.speaker;
    }
}

fn samples_to_ms(samples: i64) -> i64 {
    (samples * 1000) / WHISPER_RATE as i64
}

fn window_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// Quita el prefijo de `next` que coincide con el sufijo de `previous` (solape).
fn strip_overlap_prefix(previous: &str, next: &str) -> String {
    let prev: Vec<&str> = previous.split_whitespace().collect();
    let next_w: Vec<&str> = next.split_whitespace().collect();
    if next_w.is_empty() {
        return String::new();
    }
    if prev.is_empty() {
        return next_w.join(" ");
    }
    let max = prev.len().min(next_w.len()).min(24);
    for n in (1..=max).rev() {
        let suffix = &prev[prev.len() - n..];
        let prefix = &next_w[..n];
        if suffix
            .iter()
            .zip(prefix.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return next_w[n..].join(" ");
        }
    }
    next_w.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_strips_shared_prefix() {
        let out = strip_overlap_prefix("hola mundo cruel", "mundo cruel día");
        assert_eq!(out, "día");
    }

    #[test]
    fn dedupe_keeps_all_when_no_overlap() {
        let out = strip_overlap_prefix("alpha", "beta gamma");
        assert_eq!(out, "beta gamma");
    }

    #[test]
    fn silence_rms_is_below_gate() {
        let silence = vec![0.0f32; WINDOW_SAMPLES];
        assert!(window_rms(&silence) < ENERGY_GATE_RMS);
    }

    #[test]
    fn window_constants_are_sane() {
        assert_eq!(WINDOW_SAMPLES - OVERLAP_SAMPLES, STEP_SAMPLES);
        assert_eq!(STEP_SAMPLES, 5 * WHISPER_RATE as usize);
    }
}
