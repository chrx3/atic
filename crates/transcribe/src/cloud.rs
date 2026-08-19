//! Transcripción en la nube (BYOK). Hoy: Groq Whisper.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use atic_core::{Segment, Speaker, Transcript};
use serde::Deserialize;

use crate::decode::{self, WHISPER_RATE};
use crate::error::{Result, TranscribeError};
use crate::TrackInput;

const GROQ_TRANSCRIPTIONS_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

/// Default: mejor latencia/precio para dictado y live.
pub const GROQ_DICTATION_MODEL: &str = "whisper-large-v3-turbo";

/// Modelos STT oficiales de Groq (id, etiqueta UI).
pub const GROQ_WHISPER_MODELS: &[(&str, &str)] = &[
    ("whisper-large-v3-turbo", "Whisper Large v3 Turbo (rápido)"),
    ("whisper-large-v3", "Whisper Large v3 (más preciso)"),
];

/// Trozo enviado a Groq: 10 min @ 16 kHz. 16-bit mono ≈ 19 MB (límite free: 25 MB).
const GROQ_CHUNK_SAMPLES: usize = 10 * 60 * WHISPER_RATE as usize;
/// Receta oficial de Groq: ~10 s de solape para no cortar palabras.
const GROQ_OVERLAP_SAMPLES: usize = 10 * WHISPER_RATE as usize;
const GROQ_OVERLAP_MS: i64 = (GROQ_OVERLAP_SAMPLES as i64 * 1000) / WHISPER_RATE as i64;

const GROQ_DICTATION_TIMEOUT: Duration = Duration::from_secs(60);
const GROQ_MEETING_TIMEOUT: Duration = Duration::from_secs(180);

/// Devuelve un id válido del catálogo; desconocidos → turbo.
pub fn normalize_groq_whisper_model(model: &str) -> &'static str {
    let trimmed = model.trim();
    for (id, _) in GROQ_WHISPER_MODELS {
        if trimmed.eq_ignore_ascii_case(id) {
            return id;
        }
    }
    GROQ_DICTATION_MODEL
}

#[derive(Debug, Deserialize)]
struct GroqTranscriptResponse {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GroqVerboseResponse {
    text: Option<String>,
    language: Option<String>,
    #[serde(default)]
    segments: Vec<GroqVerboseSegment>,
}

#[derive(Debug, Deserialize)]
struct GroqVerboseSegment {
    start: f64,
    end: f64,
    text: String,
    #[serde(default)]
    no_speech_prob: Option<f64>,
}

/// Transcribe un WAV con Groq Whisper (multipart, OpenAI-compatible).
pub fn transcribe_groq(
    api_key: &str,
    wav_path: &Path,
    language: Option<&str>,
    model: &str,
) -> Result<String> {
    let key = require_key(api_key)?;
    if !wav_path.exists() {
        return Err(TranscribeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "archivo de audio no encontrado",
        )));
    }

    let bytes = std::fs::read(wav_path)?;
    let file_name = wav_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mic.wav")
        .to_string();

    transcribe_groq_wav_bytes(key, bytes, &file_name, language, model)
}

/// Transcribe PCM mono `f32` (p. ej. 16 kHz) enviando un WAV en memoria a Groq.
///
/// Pensado para ventanas cortas del live: no escribe archivos temporales.
pub fn transcribe_groq_pcm(
    api_key: &str,
    samples: &[f32],
    sample_rate: u32,
    language: Option<&str>,
    model: &str,
) -> Result<String> {
    let key = require_key(api_key)?;
    if samples.is_empty() {
        return Ok(String::new());
    }
    if sample_rate == 0 {
        return Err(TranscribeError::BadResponse(
            "sample_rate inválido para Groq PCM".into(),
        ));
    }

    let bytes = pcm_f32_mono_to_wav_bytes(samples, sample_rate)?;
    transcribe_groq_wav_bytes(key, bytes, "live.wav", language, model)
}

/// Transcribe una grabación completa por Groq: cada pista a 16 kHz, troceada,
/// con `verbose_json` para conservar marcas de tiempo (Yo / Otros).
pub fn transcribe_groq_recording(
    api_key: &str,
    tracks: &[TrackInput<'_>],
    language: Option<&str>,
    model: &str,
    progress: impl Fn(f32) + Send + Sync + 'static,
) -> Result<Transcript> {
    let key = require_key(api_key)?;
    let progress = Arc::new(progress);
    let model = normalize_groq_whisper_model(model);

    let mut transcript = Transcript {
        language: language.map(|s| s.to_string()),
        segments: Vec::new(),
    };

    let track_count = tracks.len().max(1) as f32;
    for (idx, track) in tracks.iter().enumerate() {
        let samples = decode::load_wav_mono_16k(track.wav)?;
        if samples.is_empty() {
            continue;
        }
        let base = idx as f32 / track_count;
        let prog = progress.clone();
        let (segs, detected) =
            transcribe_groq_samples(key, &samples, track.speaker, language, model, move |p| {
                prog(base + p / track_count)
            })?;
        if transcript.language.is_none() {
            transcript.language = detected;
        }
        transcript.segments.extend(segs);
    }

    transcript.sort();
    progress(1.0);
    Ok(transcript)
}

fn transcribe_groq_samples(
    api_key: &str,
    samples: &[f32],
    speaker: Speaker,
    language: Option<&str>,
    model: &str,
    on_progress: impl Fn(f32) + Send + Sync + 'static,
) -> Result<(Vec<Segment>, Option<String>)> {
    if samples.is_empty() {
        return Ok((Vec::new(), None));
    }

    let starts = chunk_starts(samples.len(), GROQ_CHUNK_SAMPLES, GROQ_OVERLAP_SAMPLES);
    let chunk_count = starts.len().max(1) as f32;
    let mut all = Vec::new();
    let mut detected_lang = None;

    for (idx, &chunk_start) in starts.iter().enumerate() {
        let chunk_end = (chunk_start + GROQ_CHUNK_SAMPLES).min(samples.len());
        let chunk = &samples[chunk_start..chunk_end];
        let offset_ms = samples_to_ms(chunk_start);
        let duration_ms = samples_to_ms(chunk.len()).max(1);
        on_progress(idx as f32 / chunk_count);

        if !decode::has_speech_activity(chunk) {
            tracing::debug!(
                samples = chunk.len(),
                offset_ms,
                "trozo Groq sin actividad de voz, se omite"
            );
            continue;
        }

        let bytes = pcm_f32_mono_to_wav_bytes(chunk, WHISPER_RATE)?;
        let body = post_groq_transcription(
            api_key,
            bytes,
            "chunk.wav",
            language,
            model,
            GroqFormat::VerboseJson,
            GROQ_MEETING_TIMEOUT,
        )?;
        let (segs, lang) = segments_from_verbose_json(&body, speaker, offset_ms, duration_ms)?;
        if detected_lang.is_none() {
            detected_lang = lang;
        }
        append_chunk_segments(&mut all, segs, idx, offset_ms, GROQ_OVERLAP_MS);
    }

    on_progress(1.0);
    Ok((all, detected_lang))
}

fn transcribe_groq_wav_bytes(
    api_key: &str,
    wav_bytes: Vec<u8>,
    file_name: &str,
    language: Option<&str>,
    model: &str,
) -> Result<String> {
    let body = post_groq_transcription(
        api_key,
        wav_bytes,
        file_name,
        language,
        model,
        GroqFormat::Json,
        GROQ_DICTATION_TIMEOUT,
    )?;
    let parsed: GroqTranscriptResponse = serde_json::from_str(&body)
        .map_err(|e| TranscribeError::BadResponse(format!("JSON Groq inválido: {e}")))?;
    Ok(parsed.text.trim().to_string())
}

#[derive(Clone, Copy)]
enum GroqFormat {
    Json,
    VerboseJson,
}

fn post_groq_transcription(
    api_key: &str,
    wav_bytes: Vec<u8>,
    file_name: &str,
    language: Option<&str>,
    model: &str,
    format: GroqFormat,
    timeout: Duration,
) -> Result<String> {
    let model = normalize_groq_whisper_model(model).to_string();
    let file_name = file_name.to_string();
    let language = language
        .map(str::trim)
        .filter(|lang| !lang.is_empty() && *lang != "auto")
        .map(str::to_string);

    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()?;

    let mut last_err = TranscribeError::BadResponse("Groq STT: sin respuesta".into());
    for attempt in 0..3 {
        let form = groq_form(
            wav_bytes.clone(),
            &file_name,
            &model,
            format,
            language.as_deref(),
        )?;
        let resp = client
            .post(GROQ_TRANSCRIPTIONS_URL)
            .bearer_auth(api_key)
            .multipart(form)
            .send()?;
        let status = resp.status();
        let body = resp.text()?;
        if status.as_u16() == 429 && attempt < 2 {
            tracing::warn!(attempt, "Groq STT 429, reintento");
            std::thread::sleep(Duration::from_secs(2 * (attempt as u64 + 1)));
            continue;
        }
        if !status.is_success() {
            let snippet: String = body.chars().take(280).collect();
            last_err = TranscribeError::BadResponse(format!("Groq STT {status}: {snippet}"));
            if status.is_server_error() && attempt < 2 {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            return Err(last_err);
        }
        return Ok(body);
    }
    Err(last_err)
}

fn groq_form(
    wav_bytes: Vec<u8>,
    file_name: &str,
    model: &str,
    format: GroqFormat,
    language: Option<&str>,
) -> Result<reqwest::blocking::multipart::Form> {
    let part = reqwest::blocking::multipart::Part::bytes(wav_bytes)
        .file_name(file_name.to_string())
        .mime_str("audio/wav")
        .map_err(|e| TranscribeError::BadResponse(e.to_string()))?;

    let response_format = match format {
        GroqFormat::Json => "json",
        GroqFormat::VerboseJson => "verbose_json",
    };

    let mut form = reqwest::blocking::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", response_format)
        .text("temperature", "0")
        .part("file", part);

    if matches!(format, GroqFormat::VerboseJson) {
        form = form.text("timestamp_granularities[]", "segment");
    }
    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    Ok(form)
}

fn pcm_f32_mono_to_wav_bytes(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &s in samples {
            let clipped = s.clamp(-1.0, 1.0);
            let i = (clipped * i16::MAX as f32) as i16;
            writer.write_sample(i)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

fn require_key(api_key: &str) -> Result<&str> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(TranscribeError::MissingApiKey("groq".into()));
    }
    Ok(key)
}

fn samples_to_ms(samples: usize) -> i64 {
    (samples as i64 * 1000) / WHISPER_RATE as i64
}

fn chunk_starts(len: usize, chunk: usize, overlap: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    if len <= chunk {
        return vec![0];
    }
    let step = chunk.saturating_sub(overlap).max(1);
    let mut starts = Vec::new();
    let mut start = 0usize;
    while start < len {
        starts.push(start);
        if start + chunk >= len {
            break;
        }
        start += step;
    }
    starts
}

fn append_chunk_segments(
    out: &mut Vec<Segment>,
    segs: Vec<Segment>,
    chunk_index: usize,
    offset_ms: i64,
    overlap_ms: i64,
) {
    if chunk_index == 0 {
        out.extend(segs);
        return;
    }
    for seg in segs {
        if seg.start_ms - offset_ms < overlap_ms {
            continue;
        }
        out.push(seg);
    }
}

fn segments_from_verbose_json(
    body: &str,
    speaker: Speaker,
    offset_ms: i64,
    duration_ms: i64,
) -> Result<(Vec<Segment>, Option<String>)> {
    let parsed: GroqVerboseResponse = serde_json::from_str(body)
        .map_err(|e| TranscribeError::BadResponse(format!("JSON Groq inválido: {e}")))?;
    let language = parsed
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut segments = Vec::new();
    for raw in parsed.segments {
        if raw.no_speech_prob.unwrap_or(0.0) > 0.6 {
            continue;
        }
        let text = raw.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let mut start_ms = (raw.start * 1000.0).round() as i64 + offset_ms;
        let mut end_ms = (raw.end * 1000.0).round() as i64 + offset_ms;
        if end_ms <= start_ms {
            end_ms = start_ms + 1;
        }
        if start_ms < 0 {
            start_ms = 0;
        }
        segments.push(Segment {
            start_ms,
            end_ms,
            speaker,
            speaker_name: None,
            text,
        });
    }

    if segments.is_empty() {
        if let Some(text) = parsed
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            segments.push(Segment {
                start_ms: offset_ms,
                end_ms: offset_ms + duration_ms.max(1),
                speaker,
                speaker_name: None,
                text: text.to_string(),
            });
        }
    }

    Ok((segments, language))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_verbose_json_segments_with_offset() {
        let body = r#"{
            "language": "es",
            "text": "Hola mundo",
            "segments": [
                {"start": 0.0, "end": 1.5, "text": " Hola", "no_speech_prob": 0.1},
                {"start": 1.5, "end": 2.4, "text": " mundo", "no_speech_prob": 0.05}
            ]
        }"#;
        let (segs, lang) = segments_from_verbose_json(body, Speaker::Me, 60_000, 2_400).unwrap();
        assert_eq!(lang.as_deref(), Some("es"));
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].start_ms, 60_000);
        assert_eq!(segs[0].end_ms, 61_500);
        assert_eq!(segs[0].text, "Hola");
        assert_eq!(segs[1].start_ms, 61_500);
        assert_eq!(segs[1].speaker, Speaker::Me);
    }

    #[test]
    fn skips_no_speech_and_falls_back_to_full_text() {
        let body = r#"{
            "text": "Hola",
            "segments": [
                {"start": 0.0, "end": 1.0, "text": " ", "no_speech_prob": 0.9}
            ]
        }"#;
        let (segs, _) = segments_from_verbose_json(body, Speaker::Others, 0, 1000).unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "Hola");
        assert_eq!(segs[0].speaker, Speaker::Others);
        assert_eq!(segs[0].end_ms, 1000);
    }

    #[test]
    fn drops_overlap_from_later_chunks() {
        let mut out = vec![Segment {
            start_ms: 0,
            end_ms: 9_500,
            speaker: Speaker::Me,
            speaker_name: None,
            text: "antes".into(),
        }];
        let next = vec![
            Segment {
                start_ms: 600_000,
                end_ms: 605_000,
                speaker: Speaker::Me,
                speaker_name: None,
                text: "solape".into(),
            },
            Segment {
                start_ms: 612_000,
                end_ms: 614_000,
                speaker: Speaker::Me,
                speaker_name: None,
                text: "después".into(),
            },
        ];
        append_chunk_segments(&mut out, next, 1, 600_000, GROQ_OVERLAP_MS);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].text, "después");
    }

    #[test]
    fn chunks_long_audio_with_overlap() {
        let starts = chunk_starts(
            GROQ_CHUNK_SAMPLES * 2,
            GROQ_CHUNK_SAMPLES,
            GROQ_OVERLAP_SAMPLES,
        );
        assert_eq!(starts.len(), 3);
        assert_eq!(starts[0], 0);
        assert_eq!(starts[1], GROQ_CHUNK_SAMPLES - GROQ_OVERLAP_SAMPLES);
    }

    #[test]
    fn wav_bytes_stay_under_the_free_upload_cap() {
        // 10 min 16 kHz 16-bit mono + header ≪ 25 MB.
        let samples = vec![0.0f32; GROQ_CHUNK_SAMPLES];
        let bytes = pcm_f32_mono_to_wav_bytes(&samples, WHISPER_RATE).unwrap();
        assert!(bytes.len() < 25 * 1024 * 1024);
        assert!(bytes.len() > 44);
    }
}
