//! Decodificación de WAV al formato que espera Whisper: mono, f32, 16 kHz.

use std::path::Path;

use crate::error::Result;

/// Frecuencia de muestreo requerida por Whisper.
pub const WHISPER_RATE: u32 = 16_000;

/// Ventana corta para detectar actividad en dictados (20 ms a 16 kHz).
const DICTATION_FRAME_SAMPLES: usize = (WHISPER_RATE as usize) / 50;
/// Contexto acústico que se conserva al inicio/fin de la voz para no cortar
/// consonantes. La transcripción de reuniones no aplica este recorte.
const DICTATION_PADDING_SAMPLES: usize = (WHISPER_RATE as usize * 400) / 1_000;

/// Carga un WAV (cualquier formato/canales/rate) como mono f32 a 16 kHz.
pub fn load_wav_mono_16k(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    // Muestras interleaved como f32 en [-1, 1], sea cual sea el formato origen.
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / scale)
                .collect()
        }
    };

    // Downmix a mono promediando los canales de cada frame.
    let mono: Vec<f32> = if channels <= 1 {
        interleaved
    } else {
        interleaved
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    };

    Ok(resample_linear(&mono, spec.sample_rate, WHISPER_RATE))
}

/// Downmix interleaved → mono y resample a 16 kHz (formato Whisper).
pub fn pcm_to_mono_16k(interleaved: &[f32], channels: u16, sample_rate: u32) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = if ch <= 1 {
        interleaved.to_vec()
    } else {
        interleaved
            .chunks(ch)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    };
    resample_linear(&mono, sample_rate.max(1), WHISPER_RATE)
}

/// Resampleo por interpolación lineal. Suficiente para voz + Whisper; se puede
/// sustituir por un resampler de mayor calidad más adelante.
pub fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if input.is_empty() || in_rate == out_rate {
        return input.to_vec();
    }
    let ratio = out_rate as f64 / in_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let last = input.len() - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input[idx.min(last)];
        let b = input[(idx + 1).min(last)];
        out.push(a + (b - a) * frac);
    }
    out
}

/// Quita el silencio / ruido estacionario largo antes y después de un dictado.
///
/// Pensado para notebooks: ventiladores y zumbido elevan el piso de ruido.
/// Tras RNNoise, el umbral relativo + piso absoluto evita mandar a Whisper
/// segundos de soplido. Las reuniones no usan este recorte.
pub(crate) fn trim_dictation_silence(samples: &[f32]) -> Vec<f32> {
    if samples.len() <= DICTATION_FRAME_SAMPLES {
        return samples.to_vec();
    }

    let levels: Vec<f32> = samples
        .chunks(DICTATION_FRAME_SAMPLES)
        .map(|frame| {
            let energy = frame.iter().map(|sample| sample * sample).sum::<f32>();
            (energy / frame.len() as f32).sqrt()
        })
        .collect();
    if levels.is_empty() {
        return Vec::new();
    }

    // Percentil 25 ≈ ruido de fondo (ventilador). Multiplicador alto: la voz
    // debe destacar; si no, mejor no inventar texto sobre soplido.
    let mut sorted = levels.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let noise_floor = sorted[sorted.len() / 4];
    let threshold = (noise_floor * 3.0).max(0.004);

    let Some(first_active) = levels.iter().position(|level| *level >= threshold) else {
        return Vec::new();
    };
    let last_active = levels
        .iter()
        .rposition(|level| *level >= threshold)
        .unwrap_or(first_active);

    // Exige un tramo mínimo de voz (~120 ms) para no disparar por un click.
    let active_frames = last_active.saturating_sub(first_active) + 1;
    if active_frames < 6 {
        return Vec::new();
    }

    let start = first_active
        .saturating_mul(DICTATION_FRAME_SAMPLES)
        .saturating_sub(DICTATION_PADDING_SAMPLES);
    let end = ((last_active + 1) * DICTATION_FRAME_SAMPLES + DICTATION_PADDING_SAMPLES)
        .min(samples.len());
    samples[start..end].to_vec()
}

/// ¿Hay picos de voz, o el audio es silencio / estática plana?
///
/// Whisper inventa frases en bucle sobre ruido estacionario. Si todos los
/// frames de 20 ms tienen casi el mismo nivel, no vale la pena transcribir.
pub(crate) fn has_speech_activity(samples: &[f32]) -> bool {
    const FRAME: usize = (WHISPER_RATE as usize) / 50;
    if samples.len() < FRAME * 10 {
        return samples.iter().any(|sample| sample.abs() >= 0.008);
    }

    let mut levels: Vec<f32> = samples
        .chunks(FRAME)
        .map(|frame| {
            let energy = frame.iter().map(|sample| sample * sample).sum::<f32>();
            (energy / frame.len() as f32).sqrt()
        })
        .collect();
    if levels.is_empty() {
        return false;
    }
    levels.sort_by(|a, b| a.total_cmp(b));
    let p10 = levels[levels.len() / 10];
    let p50 = levels[levels.len() / 2];
    let peak = levels[levels.len() - 1];
    let dynamic = peak / (p50 + 1e-6);
    let spread = p50 / (p10 + 1e-6);
    dynamic >= 2.4 || spread >= 3.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_identity() {
        let x = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&x, 16_000, 16_000), x);
    }

    #[test]
    fn resample_downsamples_length() {
        let x = vec![0.0f32; 48_000];
        let y = resample_linear(&x, 48_000, 16_000);
        assert!((y.len() as i64 - 16_000).abs() <= 1);
    }

    #[test]
    fn dictation_trim_keeps_voice_and_context() {
        let silence = vec![0.0; WHISPER_RATE as usize];
        let voice = vec![0.05; WHISPER_RATE as usize];
        let mut samples = silence.clone();
        samples.extend_from_slice(&voice);
        samples.extend_from_slice(&silence);

        let trimmed = trim_dictation_silence(&samples);
        // 1 s de voz + hasta 400 ms de contexto por lado, no los 3 s enteros.
        assert!(trimmed.len() >= WHISPER_RATE as usize);
        assert!(trimmed.len() <= (WHISPER_RATE as usize * 2));
        assert!(trimmed.contains(&0.05));
    }

    #[test]
    fn dictation_trim_discards_silence_only() {
        let samples = vec![0.0; WHISPER_RATE as usize * 3];
        assert!(trim_dictation_silence(&samples).is_empty());
    }

    #[test]
    fn dictation_trim_discards_steady_fan_noise() {
        // Soplido constante (ventilador) sin picos de voz.
        let samples = vec![0.003; WHISPER_RATE as usize * 2];
        assert!(trim_dictation_silence(&samples).is_empty());
    }

    #[test]
    fn speech_activity_rejects_silence_and_static() {
        let silence = vec![0.0; WHISPER_RATE as usize];
        assert!(!has_speech_activity(&silence));
        let stationary = vec![0.04; WHISPER_RATE as usize];
        assert!(!has_speech_activity(&stationary));
    }

    #[test]
    fn speech_activity_keeps_a_voice_burst() {
        let mut samples = vec![0.0; WHISPER_RATE as usize];
        samples.extend(vec![0.08; WHISPER_RATE as usize / 2]);
        samples.extend(vec![0.0; WHISPER_RATE as usize / 2]);
        assert!(has_speech_activity(&samples));
    }
}
