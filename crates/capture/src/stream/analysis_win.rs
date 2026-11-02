//! Mirar el audio de un video sin reproducirlo (Windows): la forma de onda y el WAV para
//! Whisper. Media Foundation decodifica y reprocesa (frecuencia y canales); aquí solo se lee
//! por bloques, nunca el audio entero en memoria.
//!
//! El mismo contrato que la versión de macOS (`analysis_macos.rs`): el nivel de cada ventana es
//! su pico sobre el más alto de todo el audio, y un video sin audio no es un error para la onda.

use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSourceReader, MFAudioFormat_PCM, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
};

use super::info_win::{duration, win, Session, TICKS_PER_SECOND};

/// La forma de onda se saca a 16 kHz mono: sobra para ver picos por ventana.
const LEVELS_RATE: u32 = 16_000;
/// Lo que pide Whisper.
const WHISPER_RATE: u32 = 16_000;
/// Menos que esto, el audio es silencio (o ruido de fondo del códec).
const SILENCE: f32 = 1e-4;

/// La primera pista de audio (el índice negativo que entiende Media Foundation).
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

/// El audio de un video, ya reprocesado a PCM de 16 bits mono.
struct Audio {
    reader: IMFSourceReader,
    /// Lo que dura el video, en segundos.
    duration: f64,
}

impl Audio {
    /// Abre el audio de `source` a `rate`. `Ok(None)` si el video no tiene pista de audio. La
    /// sesión de Media Foundation tiene que estar abierta.
    fn open(source: &Path, rate: u32) -> Result<Option<Self>, String> {
        // SAFETY: objetos de Media Foundation creados y soltados aquí.
        unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(source.as_os_str()), None::<&IMFAttributes>)
                .map_err(|error| format!("no pude abrir el video: {}", win(error)))?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(win)?;
            if reader.SetStreamSelection(AUDIO, true).is_err() || reader.GetNativeMediaType(AUDIO, 0).is_err() {
                return Ok(None);
            }
            let seconds = duration(&reader);
            let media_type = MFCreateMediaType().map_err(win)?;
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(win)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(win)?;
            media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(win)?;
            // Mono y a `rate`: el lector inserta el remuestreador y mezcla los canales.
            media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate).map_err(win)?;
            media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1).map_err(win)?;
            // Una muestra de 16 bits: dos bytes por muestra.
            media_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2).map_err(win)?;
            media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 2).map_err(win)?;
            reader
                .SetCurrentMediaType(AUDIO, None, &media_type)
                .map_err(|error| format!("no pude preparar el audio: {}", win(error)))?;
            Ok(Some(Self { reader, duration: seconds }))
        }
    }

    /// Todos los bloques, en orden: `on_block` recibe el segundo en que empieza cada uno y sus
    /// bytes (PCM de 16 bits little endian, mono).
    fn blocks(&self, on_block: &mut dyn FnMut(f64, &[u8]) -> Result<(), String>) -> Result<(), String> {
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            // SAFETY: el lector vive en `self`.
            unsafe { self.reader.ReadSample(AUDIO, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample)) }
                .map_err(win)?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(());
            }
            let Some(sample) = sample else { continue };
            let at = time as f64 / TICKS_PER_SECOND as f64;
            // SAFETY: el buffer se bloquea y se desbloquea aquí; `on_block` no se guarda la
            // rebanada.
            unsafe {
                let buffer = match sample.ConvertToContiguousBuffer() {
                    Ok(buffer) => buffer,
                    Err(_) => continue,
                };
                let mut data = std::ptr::null_mut();
                let mut len = 0u32;
                if buffer.Lock(&mut data, None, Some(&mut len)).is_err() {
                    continue;
                }
                let result = if data.is_null() || len == 0 {
                    Ok(())
                } else {
                    on_block(at, std::slice::from_raw_parts(data, len as usize))
                };
                let _ = buffer.Unlock();
                result?;
            }
        }
    }
}

/// Los picos del audio por ventanas de `1 / per_second` segundos. Los límites de las ventanas
/// salen de la cuenta exacta de muestras (no de un largo redondeado), así que no se desfasan en
/// grabaciones de horas.
struct Windows {
    rate: u64,
    per_second: u64,
    /// Muestras vistas hasta ahora.
    pos: u64,
    /// El pico de la ventana en curso.
    current: f32,
    /// Los picos de las ventanas ya cerradas.
    peaks: Vec<f32>,
}

impl Windows {
    fn new(rate: u32, per_second: u32) -> Self {
        Self { rate: rate.max(1) as u64, per_second: per_second.max(1) as u64, pos: 0, current: 0.0, peaks: Vec::new() }
    }

    /// La primera muestra de la ventana `index`.
    fn start_of(&self, index: u64) -> u64 {
        (index * self.rate).div_ceil(self.per_second)
    }

    fn push(&mut self, pcm: &[i16]) {
        let mut rest = pcm;
        loop {
            // Cierra las ventanas que ya terminaron.
            while self.pos >= self.start_of(self.peaks.len() as u64 + 1) {
                self.peaks.push(self.current);
                self.current = 0.0;
            }
            if rest.is_empty() {
                break;
            }
            let room = (self.start_of(self.peaks.len() as u64 + 1) - self.pos) as usize;
            let (now, later) = rest.split_at(room.min(rest.len()));
            for &s in now {
                self.current = self.current.max((s as f32 / 32768.0).abs());
            }
            self.pos += now.len() as u64;
            rest = later;
        }
    }

    /// Silencio hasta la muestra `sample` (el audio que empieza después del cero).
    fn skip_to(&mut self, sample: u64) {
        let zeros = [0i16; 1024];
        while self.pos < sample {
            let n = (sample - self.pos).min(zeros.len() as u64) as usize;
            self.push(&zeros[..n]);
        }
    }

    /// Cierra la ventana a medias y deja `len` niveles (si `len` es 0, los que haya),
    /// normalizados al pico más alto.
    fn finish(mut self, len: usize) -> Vec<f32> {
        if self.pos > self.start_of(self.peaks.len() as u64) {
            self.peaks.push(self.current);
        }
        if len > 0 {
            self.peaks.resize(len, 0.0);
        }
        normalize(self.peaks)
    }
}

/// Cada nivel sobre el más alto (0–1); si todo es casi silencio, ceros.
fn normalize(mut peaks: Vec<f32>) -> Vec<f32> {
    let max = peaks.iter().copied().fold(0.0f32, f32::max);
    if max < SILENCE {
        peaks.iter_mut().for_each(|p| *p = 0.0);
    } else {
        peaks.iter_mut().for_each(|p| *p = (*p / max).min(1.0));
    }
    peaks
}

/// El nivel (0–1) del audio en ventanas de `1 / per_second` segundos: el pico de cada una
/// sobre el más alto de todo el audio. Vacío si el video no tiene audio.
pub fn audio_levels(source: &Path, per_second: u32) -> Result<Vec<f32>, String> {
    let per_second = per_second.max(1);
    let _session = Session::start()?;
    let Some(audio) = Audio::open(source, LEVELS_RATE)? else { return Ok(Vec::new()) };
    let expected = (audio.duration * per_second as f64).ceil().max(0.0) as usize;
    // Lo último que puede pedir un audio que empieza tarde (sin pasarse del video).
    let limit = (audio.duration.max(0.0) * LEVELS_RATE as f64) as u64;
    let mut windows = Windows::new(LEVELS_RATE, per_second);
    let mut scratch: Vec<i16> = Vec::new();
    let mut first = true;
    audio.blocks(&mut |at, bytes| {
        // Un audio que no empieza en cero deja silencio antes.
        if first {
            first = false;
            let start = (at.max(0.0) * LEVELS_RATE as f64) as u64;
            windows.skip_to(start.min(limit));
        }
        scratch.clear();
        scratch.extend(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
        windows.push(&scratch);
        Ok(())
    })?;
    Ok(windows.finish(expected))
}

// --- El WAV para Whisper -----------------------------------------------------------

/// La cabecera RIFF/WAVE de 44 bytes de PCM mono de 16 kHz y 16 bits con `data_len` bytes.
fn wav_header(data_len: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&data_len.saturating_add(36).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM entero
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&WHISPER_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(WHISPER_RATE * 2).to_le_bytes()); // bytes por segundo
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // bytes por muestra
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// El audio de `source` como WAV mono de 16 kHz y 16 bits (lo que pide Whisper).
/// `Err` si no hay audio.
pub fn extract_wav_16k(source: &Path, out: &Path) -> Result<(), String> {
    let result = write_wav(source, out);
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn write_wav(source: &Path, out: &Path) -> Result<(), String> {
    let _session = Session::start()?;
    // Sin pista de audio no hay WAV: el mensaje lo busca quien pide los subtítulos.
    let Some(audio) = Audio::open(source, WHISPER_RATE)? else {
        return Err("el video no tiene audio".into());
    };
    let file = std::fs::File::create(out).map_err(|e| format!("no pude crear el WAV: {e}"))?;
    let mut file = BufWriter::new(file);
    // Los tamaños se escriben al final, cuando se sabe cuánto audio hubo.
    file.write_all(&wav_header(0)).map_err(|e| e.to_string())?;
    let mut data_len: u64 = 0;
    audio.blocks(&mut |_, bytes| {
        let even = &bytes[..bytes.len() & !1];
        file.write_all(even).map_err(|e| format!("no pude escribir el WAV: {e}"))?;
        data_len += even.len() as u64;
        Ok(())
    })?;
    if data_len == 0 {
        return Err("el video no tiene audio".into());
    }
    let mut file = file.into_inner().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.write_all(&wav_header(data_len.min(u32::MAX as u64 - 36) as u32)).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_pico_de_cada_ventana_se_normaliza_al_mas_alto() {
        // 4 muestras/s y 2 ventanas/s → 2 muestras por ventana.
        let mut w = Windows::new(4, 2);
        w.push(&[16384, 0, 8192, 0]);
        // Picos 0,5 y 0,25 → 1,0 y 0,5.
        assert_eq!(w.finish(2), vec![1.0, 0.5]);
    }

    #[test]
    fn el_ultimo_bloque_a_medias_cuenta_y_el_largo_se_completa() {
        let mut w = Windows::new(4, 2);
        w.push(&[16384, 0, -32768, 0, 8192]);
        // Tres ventanas (la última con una sola muestra) y dos de relleno en silencio.
        assert_eq!(w.finish(5), vec![0.5, 1.0, 0.25, 0.0, 0.0]);
    }

    #[test]
    fn las_ventanas_no_se_desfasan_con_una_frecuencia_que_no_divide() {
        // 3 muestras/s y 2 ventanas/s: límites en 0, 2, 3, 5 (no 1,5 redondeado).
        let mut w = Windows::new(3, 2);
        w.push(&[0, 16384, 8192, 0, -32768]);
        assert_eq!(w.finish(3), vec![0.5, 0.25, 1.0]);
    }

    #[test]
    fn da_igual_en_cuantos_bloques_llegue() {
        let pcm = [100i16, -2000, 300, 4000, -500, 600, 700, -8000, 900];
        let mut whole = Windows::new(6, 4);
        whole.push(&pcm);
        let mut parts = Windows::new(6, 4);
        for chunk in pcm.chunks(2) {
            parts.push(chunk);
        }
        assert_eq!(whole.finish(0), parts.finish(0));
    }

    #[test]
    fn el_silencio_da_ceros_del_largo_correcto() {
        let mut w = Windows::new(4, 2);
        w.push(&[0, 1, 0, 0]);
        // Pico de 1/32768 ≈ 3e-5 < 1e-4: silencio.
        assert_eq!(w.finish(4), vec![0.0; 4]);
        assert_eq!(normalize(vec![0.0, 0.00005]), vec![0.0, 0.0]);
    }

    #[test]
    fn el_audio_que_empieza_tarde_deja_silencio_antes() {
        let mut w = Windows::new(4, 2);
        w.skip_to(4);
        w.push(&[-32768, 0]);
        assert_eq!(w.finish(3), vec![0.0, 0.0, 1.0]);
    }

    #[test]
    fn la_cabecera_wav_es_la_de_pcm_mono_16k_de_16_bits() {
        let h = wav_header(1000);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 1036);
        assert_eq!(&h[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(h[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 32_000);
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(h[34..36].try_into().unwrap()), 16);
        assert_eq!(&h[36..40], b"data");
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 1000);
    }

    /// Un video de verdad, a mano: `ATIC_TEST_VIDEO=<video>` con la onda y el WAV.
    #[test]
    #[ignore]
    fn saca_la_onda_y_el_wav_de_un_video_de_verdad() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let levels = audio_levels(Path::new(&source), 20).expect("niveles");
        println!("{} ventanas de audio", levels.len());
        assert!(levels.iter().all(|&level| (0.0..=1.0).contains(&level)));
        assert!(levels.iter().any(|&level| level > 0.0));
        let out = std::env::temp_dir().join("atic-prueba-16k.wav");
        extract_wav_16k(Path::new(&source), &out).expect("wav");
        let wav = std::fs::read(&out).expect("leer el wav");
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
        assert!(wav.len() > 44);
        let _ = std::fs::remove_file(&out);
    }
}
