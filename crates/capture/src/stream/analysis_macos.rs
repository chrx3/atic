//! Mirar el audio de un video sin reproducirlo: la forma de onda y el WAV para Whisper.
//! AVAssetReader decodifica y reprocesa (frecuencia y canales) en el sistema; aquí solo se
//! lee por bloques, nunca el audio entero en memoria.

use std::ffi::c_void;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::msg_send;
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSError, NSString};

const LPCM: u32 = u32::from_be_bytes(*b"lpcm");
/// `AVAssetReaderStatusFailed`.
const READER_FAILED: isize = 3;
/// La forma de onda se saca a 16 kHz mono: sobra para ver picos por ventana.
const LEVELS_RATE: u32 = 16_000;
/// Lo que pide Whisper.
const WHISPER_RATE: u32 = 16_000;
/// Menos que esto, el audio es silencio (o ruido de fondo del códec).
const SILENCE: f32 = 1e-4;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: CFStringRef;
    static AVFormatIDKey: CFStringRef;
    static AVSampleRateKey: CFStringRef;
    static AVNumberOfChannelsKey: CFStringRef;
    static AVLinearPCMBitDepthKey: CFStringRef;
    static AVLinearPCMIsFloatKey: CFStringRef;
    static AVLinearPCMIsBigEndianKey: CFStringRef;
    static AVLinearPCMIsNonInterleaved: CFStringRef;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetPresentationTimeStamp(sample: *const CMSampleBuffer) -> CMTime;
    fn CMSampleBufferGetDataBuffer(sample: *const c_void) -> *const c_void;
    fn CMBlockBufferGetDataLength(block: *const c_void) -> usize;
    fn CMBlockBufferCopyDataBytes(block: *const c_void, offset: usize, length: usize, destination: *mut c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// Suelta un objeto de CoreFoundation retenido al salir del alcance.
struct Released(*const c_void);
impl Drop for Released {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: se creó o copió con +1 y se suelta una vez.
            unsafe { CFRelease(self.0) };
        }
    }
}

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("sin {}", name.to_string_lossy()))
}

fn key(key: CFStringRef) -> CFString {
    // SAFETY: constante de un framework.
    unsafe { CFString::wrap_under_get_rule(key) }
}

fn as_object<T: TCFType>(value: &T) -> &AnyObject {
    // SAFETY: los tipos de CoreFoundation usados aquí tienen puente con Foundation.
    unsafe { &*(value.as_CFTypeRef() as *const AnyObject) }
}

fn dict(pairs: &[(CFString, CFType)]) -> CFDictionary<CFString, CFType> {
    CFDictionary::from_CFType_pairs(pairs)
}

fn error_text(error: *mut AnyObject) -> String {
    // SAFETY: un `NSError` o nulo.
    unsafe { (error as *mut NSError).as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default()
}

fn seconds(time: CMTime) -> f64 {
    if time.timescale > 0 && time.flags.contains(CMTimeFlags::Valid) {
        time.value as f64 / time.timescale as f64
    } else {
        0.0
    }
}

fn open_asset(source: &Path) -> Result<Retained<AnyObject>, String> {
    // SAFETY: objetos de AVFoundation creados aquí.
    unsafe {
        let url: Retained<AnyObject> = msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&source.to_string_lossy())];
        let none: *const AnyObject = std::ptr::null();
        Ok(msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*url, options: none])
    }
}

/// Lee el audio de `asset` como PCM de 16 bits enteros con `rate` y `channels` (el lector
/// reprocesa), bloque a bloque: `on_block` recibe el segundo del bloque y sus bytes
/// (little endian, intercalados). `Ok(false)` si no hay pista de audio.
fn read_pcm(asset: &AnyObject, rate: u32, channels: i32, on_block: &mut dyn FnMut(f64, &[u8]) -> Result<(), String>) -> Result<bool, String> {
    // SAFETY: lector de AVFoundation creado y usado aquí; las muestras llegan con +1 y se
    // sueltan con `Released`.
    unsafe {
        let tracks: Retained<NSArray<AnyObject>> = msg_send![asset, tracksWithMediaType: &*(AVMediaTypeAudio as *const AnyObject)];
        let Some(track) = tracks.firstObject() else { return Ok(false) };
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let reader: Option<Retained<AnyObject>> = msg_send![class(c"AVAssetReader")?, assetReaderWithAsset: asset, error: &mut error];
        let reader = reader.ok_or_else(|| format!("AVAssetReader: {}", error_text(error)))?;
        let settings = dict(&[
            (key(AVFormatIDKey), CFNumber::from(LPCM as i32).as_CFType()),
            (key(AVSampleRateKey), CFNumber::from(rate as f64).as_CFType()),
            (key(AVNumberOfChannelsKey), CFNumber::from(channels).as_CFType()),
            (key(AVLinearPCMBitDepthKey), CFNumber::from(16i32).as_CFType()),
            (key(AVLinearPCMIsFloatKey), CFBoolean::false_value().as_CFType()),
            (key(AVLinearPCMIsBigEndianKey), CFBoolean::false_value().as_CFType()),
            (key(AVLinearPCMIsNonInterleaved), CFBoolean::false_value().as_CFType()),
        ]);
        let output: Retained<AnyObject> =
            msg_send![class(c"AVAssetReaderTrackOutput")?, assetReaderTrackOutputWithTrack: &*track, outputSettings: as_object(&settings)];
        let _: () = msg_send![&reader, addOutput: &*output];
        let started: Bool = msg_send![&reader, startReading];
        if !started.as_bool() {
            let error: *mut AnyObject = msg_send![&reader, error];
            return Err(format!("AVAssetReader no empezó: {}", error_text(error)));
        }
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let more = objc2::rc::autoreleasepool(|_| -> Result<bool, String> {
                let sample: *mut CMSampleBuffer = msg_send![&output, copyNextSampleBuffer];
                if sample.is_null() {
                    return Ok(false);
                }
                let _sample = Released(sample as *const c_void);
                let block = CMSampleBufferGetDataBuffer(sample as *const c_void);
                if block.is_null() {
                    return Ok(true);
                }
                let len = CMBlockBufferGetDataLength(block);
                bytes.resize(len, 0);
                if CMBlockBufferCopyDataBytes(block, 0, len, bytes.as_mut_ptr().cast()) != 0 {
                    return Ok(true);
                }
                on_block(seconds(CMSampleBufferGetPresentationTimeStamp(sample)), &bytes)?;
                Ok(true)
            });
            match more {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    let _: () = msg_send![&reader, cancelReading];
                    return Err(e);
                }
            }
        }
        let status: isize = msg_send![&reader, status];
        if status == READER_FAILED {
            let error: *mut AnyObject = msg_send![&reader, error];
            return Err(format!("AVAssetReader falló: {}", error_text(error)));
        }
        Ok(true)
    }
}

// --- La forma de onda --------------------------------------------------------------

/// Los picos del audio por ventanas de `1 / per_second` segundos. Los límites de las
/// ventanas salen de la cuenta exacta de muestras (no de un largo redondeado), así que no
/// se desfasan en grabaciones de horas.
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
    objc2::rc::autoreleasepool(|_| {
        let asset = open_asset(source)?;
        // SAFETY: getter de un AVURLAsset vivo.
        let duration = seconds(unsafe { msg_send![&asset, duration] });
        let expected = (duration * per_second as f64).ceil().max(0.0) as usize;
        let mut windows = Windows::new(LEVELS_RATE, per_second);
        let mut scratch: Vec<i16> = Vec::new();
        let mut first = true;
        let has_audio = read_pcm(&asset, LEVELS_RATE, 1, &mut |at, bytes| {
            // Un audio que no empieza en cero deja silencio antes (sin pasarse del video).
            if first {
                first = false;
                let start = (at.max(0.0) * LEVELS_RATE as f64) as u64;
                windows.skip_to(start.min((duration.max(0.0) * LEVELS_RATE as f64) as u64));
            }
            scratch.clear();
            scratch.extend(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
            windows.push(&scratch);
            Ok(())
        })?;
        if !has_audio {
            return Ok(Vec::new());
        }
        Ok(windows.finish(expected))
    })
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
    let result = objc2::rc::autoreleasepool(|_| write_wav(source, out));
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn write_wav(source: &Path, out: &Path) -> Result<(), String> {
    let asset = open_asset(source)?;
    let file = std::fs::File::create(out).map_err(|e| format!("no pude crear el WAV: {e}"))?;
    let mut file = BufWriter::new(file);
    // Los tamaños se escriben al final, cuando se sabe cuánto audio hubo.
    file.write_all(&wav_header(0)).map_err(|e| e.to_string())?;
    let mut data_len: u64 = 0;
    let has_audio = read_pcm(&asset, WHISPER_RATE, 1, &mut |_, bytes| {
        let even = &bytes[..bytes.len() & !1];
        file.write_all(even).map_err(|e| format!("no pude escribir el WAV: {e}"))?;
        data_len += even.len() as u64;
        Ok(())
    })?;
    if !has_audio || data_len == 0 {
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
        // 3 muestras/s y 2 ventanas/s: limites en 0, 2, 3, 5 (no 1,5 redondeado).
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
        assert_eq!(u32::from_le_bytes(h[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(h[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 32_000);
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(h[34..36].try_into().unwrap()), 16);
        assert_eq!(&h[36..40], b"data");
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 1000);
    }
}
