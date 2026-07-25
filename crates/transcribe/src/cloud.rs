//! Transcripción en la nube (BYOK). Hoy: Groq Whisper.

use std::io::Cursor;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Result, TranscribeError};

const GROQ_TRANSCRIPTIONS_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

/// Default: mejor latencia/precio para dictado y live.
pub const GROQ_DICTATION_MODEL: &str = "whisper-large-v3-turbo";

/// Modelos STT oficiales de Groq (id, etiqueta UI).
pub const GROQ_WHISPER_MODELS: &[(&str, &str)] = &[
    ("whisper-large-v3-turbo", "Whisper Large v3 Turbo (rápido)"),
    ("whisper-large-v3", "Whisper Large v3 (más preciso)"),
];

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

/// Transcribe un WAV con Groq Whisper (multipart, OpenAI-compatible).
pub fn transcribe_groq(
    api_key: &str,
    wav_path: &Path,
    language: Option<&str>,
    model: &str,
) -> Result<String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(TranscribeError::MissingApiKey("groq".into()));
    }
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
    let key = api_key.trim();
    if key.is_empty() {
        return Err(TranscribeError::MissingApiKey("groq".into()));
    }
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

fn transcribe_groq_wav_bytes(
    api_key: &str,
    wav_bytes: Vec<u8>,
    file_name: &str,
    language: Option<&str>,
    model: &str,
) -> Result<String> {
    let model = normalize_groq_whisper_model(model);
    let part = reqwest::blocking::multipart::Part::bytes(wav_bytes)
        .file_name(file_name.to_string())
        .mime_str("audio/wav")
        .map_err(|e| TranscribeError::BadResponse(e.to_string()))?;

    let mut form = reqwest::blocking::multipart::Form::new()
        .text("model", model)
        .text("response_format", "json")
        .text("temperature", "0")
        .part("file", part);

    if let Some(lang) = language {
        if !lang.is_empty() && lang != "auto" {
            form = form.text("language", lang.to_string());
        }
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let resp = client
        .post(GROQ_TRANSCRIPTIONS_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        let snippet: String = body.chars().take(280).collect();
        return Err(TranscribeError::BadResponse(format!(
            "Groq STT {status}: {snippet}"
        )));
    }

    let parsed: GroqTranscriptResponse = serde_json::from_str(&body)
        .map_err(|e| TranscribeError::BadResponse(format!("JSON Groq inválido: {e}")))?;
    Ok(parsed.text.trim().to_string())
}
