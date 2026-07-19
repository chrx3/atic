//! Importación de audio externo (WAV/MP3/M4A) a `mic.wav` mono 16 kHz.

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::decode::{pcm_to_mono_16k, WHISPER_RATE};
use crate::error::{Result, TranscribeError};

/// Decodifica un archivo soportado a PCM mono f32 16 kHz y lo escribe como WAV.
///
/// Devuelve la duración en segundos (redondeada) del audio normalizado.
pub fn import_audio_to_wav(src: &Path, dest_wav: &Path) -> Result<i64> {
    let samples = decode_file_mono_16k(src)?;
    if samples.is_empty() {
        return Err(TranscribeError::AudioDecode(
            "el archivo no contiene audio decodificable".into(),
        ));
    }
    write_wav_mono_16k(dest_wav, &samples)?;
    Ok((samples.len() as f64 / f64::from(WHISPER_RATE)).round() as i64)
}

/// Decodifica WAV/MP3/M4A (u otros formatos habilitados) a mono f32 @ 16 kHz.
pub fn decode_file_mono_16k(path: &Path) -> Result<Vec<f32>> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| TranscribeError::AudioDecode(e.to_string()))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| TranscribeError::AudioDecode("no se encontró pista de audio".into()))?
        .clone();

    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| TranscribeError::AudioDecode("parámetros de audio no disponibles".into()))?
        .clone();

    let sample_rate = audio_params.sample_rate.unwrap_or(0);
    let channels_hint = audio_params
        .channels
        .as_ref()
        .map(|c| c.count() as u16)
        .unwrap_or(0);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .map_err(|e| TranscribeError::AudioDecode(e.to_string()))?;

    let track_id = track.id;
    let mut interleaved = Vec::new();
    let mut frame = Vec::new();
    let mut sample_rate = sample_rate;
    let mut channels = channels_hint;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(err) => return Err(TranscribeError::AudioDecode(err.to_string())),
        };

        if packet.track_id != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                sample_rate = spec.rate();
                channels = spec.channels().count() as u16;
                frame.clear();
                decoded.copy_to_vec_interleaved(&mut frame);
                interleaved.extend_from_slice(&frame);
            }
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => continue,
            Err(err) => return Err(TranscribeError::AudioDecode(err.to_string())),
        }
    }

    if sample_rate == 0 || channels == 0 {
        return Err(TranscribeError::AudioDecode(
            "no se pudo determinar sample rate o canales del audio".into(),
        ));
    }

    Ok(pcm_to_mono_16k(&interleaved, channels, sample_rate))
}

/// Escribe PCM mono f32 como WAV float 32-bit a 16 kHz (formato de captura).
pub fn write_wav_mono_16k(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s.clamp(-1.0, 1.0))?;
    }
    writer.finalize()?;
    Ok(())
}
