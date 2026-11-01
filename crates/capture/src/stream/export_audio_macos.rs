//! El audio del exportado: se lee tramo por tramo y por bloques (nunca entero en
//! memoria), con un fundido corto en cada empalme para que los cortes no suenen a
//! clic. Cada tramo a 1× abre su propio `AVAssetReader` con `timeRange` y copia los
//! bytes directo del búfer de CoreMedia al bloque de salida; los tramos a otra
//! velocidad van en silencio.

use std::ffi::c_void;

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSError};

/// PCM de 16 bits estéreo entrelazado a 48 kHz.
pub(super) const AUDIO_RATE: u32 = 48_000;
pub(super) const FRAME_BYTES: usize = 4;

/// El fundido de entrada y de salida de cada tramo (15 ms).
const FADE_FRAMES: usize = AUDIO_RATE as usize * 15 / 1000;
/// Cuánto se lee de más al final de cada tramo, por si el último búfer cae justo en el borde.
const TAIL_SECS: f64 = 0.02;
const LPCM: u32 = u32::from_be_bytes(*b"lpcm");
const READER_FAILED: isize = 3;

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

/// Un tramo del audio del original. Con `rate != 1.0` va en silencio (dura
/// `(end - start) / rate` en el exportado).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AudioSeg {
    pub start: f64,
    pub end: f64,
    pub rate: f64,
}

/// `CMTimeRange`.
#[repr(C)]
#[derive(Clone, Copy)]
struct TimeRange {
    start: CMTime,
    duration: CMTime,
}
// SAFETY: la misma disposición que `CMTimeRange` (`{?={?=qiIq}{?=qiIq}}`).
unsafe impl Encode for TimeRange {
    const ENCODING: Encoding = Encoding::Struct("?", &[CMTime::ENCODING, CMTime::ENCODING]);
}

// --- Plan de cuadros (puro) -------------------------------------------------------------

/// Un tramo ya medido en cuadros de salida.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Planned {
    /// Cuántos cuadros ocupa en el exportado.
    frames: u64,
    /// El cuadro del original donde empieza (solo cuenta si no es silencio).
    src_start: i64,
    silent: bool,
}

/// Mide cada tramo en cuadros de salida con redondeo acumulativo: el final de cada uno es
/// el redondeo del tiempo acumulado, así que el error nunca pasa de medio cuadro en total.
/// Los tramos vacíos o inválidos se saltan.
fn plan_frames(segs: &[AudioSeg]) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut secs = 0.0f64;
    let mut prev = 0u64;
    for seg in segs {
        let len = seg.end - seg.start;
        if !len.is_finite() || len <= 0.0 {
            continue;
        }
        let rate = if seg.rate.is_finite() && seg.rate > 0.0 { seg.rate } else { 1.0 };
        secs += len / rate;
        let end = ((secs * AUDIO_RATE as f64).round() as u64).max(prev);
        let frames = end - prev;
        prev = end;
        if frames == 0 {
            continue;
        }
        out.push(Planned { frames, src_start: (seg.start * AUDIO_RATE as f64).round() as i64, silent: (rate - 1.0).abs() > 1e-9 });
    }
    out
}

// --- Fundido (puro) ---------------------------------------------------------------------

/// La ganancia (0–1) del cuadro `i` de un tramo de `total` cuadros con un fundido de `fade`
/// cuadros en cada borde. El primero y el último quedan en 0.
fn fade_gain(i: usize, total: usize, fade: usize) -> f32 {
    let fade = fade.min(total / 2);
    if fade == 0 || i >= total {
        return 1.0;
    }
    let edge = i.min(total - 1 - i);
    if edge >= fade {
        1.0
    } else {
        edge as f32 / fade as f32
    }
}

/// Aplica el fundido a `pcm` (16 bits estéreo entrelazado) que son los cuadros
/// `first_frame..` de un tramo de `total` cuadros. Se puede llamar por bloques.
fn apply_fade(pcm: &mut [u8], first_frame: usize, total: usize, fade: usize) {
    let frames = pcm.len() / FRAME_BYTES;
    let fade = fade.min(total / 2);
    // Un bloque del todo dentro de la parte plana no se toca.
    if fade == 0 || (first_frame >= fade && first_frame + frames + fade <= total) {
        return;
    }
    for (j, frame) in pcm.chunks_exact_mut(FRAME_BYTES).enumerate() {
        let gain = fade_gain(first_frame + j, total, fade);
        if gain >= 1.0 {
            continue;
        }
        for sample in frame.chunks_exact_mut(2) {
            let value = i16::from_le_bytes([sample[0], sample[1]]);
            sample.copy_from_slice(&((value as f32 * gain) as i16).to_le_bytes());
        }
    }
}

// --- Relleno por PTS (puro, con fuente abstracta) -----------------------------------------

/// Un trozo de PCM del lector, ubicado por su PTS.
trait Chunk {
    /// El cuadro del original (a `AUDIO_RATE`) donde empieza.
    fn pos(&self) -> i64;
    fn frames(&self) -> usize;
    /// Copia `n` cuadros desde el cuadro `from` del trozo a `dst`.
    fn copy(&self, from: usize, n: usize, dst: &mut [u8]);
}

trait Source {
    type C: Chunk;
    fn next_chunk(&mut self) -> Result<Option<Self::C>, String>;
}

fn push_silence(out: &mut Vec<u8>, frames: usize) {
    out.resize(out.len() + frames * FRAME_BYTES, 0);
}

/// Entrega cuadros consecutivos del original desde `cursor`: ubica cada trozo por su PTS,
/// rellena con silencio los huecos y lo que falte al final.
struct Fill<C> {
    cursor: i64,
    chunk: Option<C>,
    eof: bool,
}

impl<C: Chunk> Fill<C> {
    fn new(cursor: i64) -> Self {
        Self { cursor, chunk: None, eof: false }
    }

    /// Agrega a `out` exactamente `want` cuadros.
    fn read<S: Source<C = C>>(&mut self, source: &mut S, out: &mut Vec<u8>, mut want: usize) -> Result<(), String> {
        while want > 0 {
            if self.chunk.is_none() && !self.eof {
                match source.next_chunk()? {
                    Some(chunk) => self.chunk = Some(chunk),
                    None => self.eof = true,
                }
            }
            let Some(chunk) = &self.chunk else {
                push_silence(out, want);
                self.cursor += want as i64;
                return Ok(());
            };
            let (pos, frames) = (chunk.pos(), chunk.frames());
            if self.cursor >= pos + frames as i64 {
                // Ya pasamos este trozo (o se solapaba con el anterior).
                self.chunk = None;
                continue;
            }
            if self.cursor < pos {
                let gap = ((pos - self.cursor) as u64).min(want as u64) as usize;
                push_silence(out, gap);
                self.cursor += gap as i64;
                want -= gap;
                continue;
            }
            let from = (self.cursor - pos) as usize;
            let n = (frames - from).min(want);
            let at = out.len();
            push_silence(out, n);
            chunk.copy(from, n, &mut out[at..]);
            self.cursor += n as i64;
            want -= n;
        }
        Ok(())
    }
}

// --- AVFoundation ------------------------------------------------------------------------

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

fn ns(key: CFStringRef) -> &'static AnyObject {
    // SAFETY: constantes de los frameworks, vivas mientras viva el proceso.
    unsafe { &*(key as *const AnyObject) }
}

fn key(key: CFStringRef) -> CFString {
    // SAFETY: constante de un framework.
    unsafe { CFString::wrap_under_get_rule(key) }
}

fn as_object<T: TCFType>(value: &T) -> &AnyObject {
    // SAFETY: los tipos de CoreFoundation usados aquí tienen puente con Foundation.
    unsafe { &*(value.as_CFTypeRef() as *const AnyObject) }
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

fn cmtime(secs: f64) -> CMTime {
    CMTime { value: (secs.max(0.0) * 1_000_000.0).round() as i64, timescale: 1_000_000, flags: CMTimeFlags::Valid, epoch: 0 }
}

/// Un búfer de muestras del lector, sin copiarlo: los bytes se copian directo al destino.
struct SampleChunk {
    _sample: Released,
    block: *const c_void,
    pos: i64,
    frames: usize,
}

impl Chunk for SampleChunk {
    fn pos(&self) -> i64 {
        self.pos
    }
    fn frames(&self) -> usize {
        self.frames
    }
    fn copy(&self, from: usize, n: usize, dst: &mut [u8]) {
        let n = n.min(dst.len() / FRAME_BYTES).min(self.frames.saturating_sub(from));
        // SAFETY: `block` vive con `_sample`; el rango cae dentro de sus bytes y `dst` los cabe.
        unsafe { CMBlockBufferCopyDataBytes(self.block, from * FRAME_BYTES, n * FRAME_BYTES, dst.as_mut_ptr().cast()) };
    }
}

/// Un lector de una sola pasada sobre un tramo del audio.
struct TrackReader {
    reader: Retained<AnyObject>,
    output: Retained<AnyObject>,
}

impl TrackReader {
    /// Lee `from..to` (segundos del original) como PCM de 16 bits estéreo a 48 kHz.
    unsafe fn start(asset: &AnyObject, track: &AnyObject, from: f64, to: f64) -> Result<Self, String> {
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let reader: Option<Retained<AnyObject>> = unsafe { msg_send![class(c"AVAssetReader")?, assetReaderWithAsset: asset, error: &mut error] };
        let reader = reader.ok_or_else(|| format!("no se pudo leer el audio (AVAssetReader): {}", error_text(error)))?;
        let settings = CFDictionary::from_CFType_pairs(&[
            (key(unsafe { AVFormatIDKey }), CFNumber::from(LPCM as i32).as_CFType()),
            (key(unsafe { AVSampleRateKey }), CFNumber::from(AUDIO_RATE as f64).as_CFType()),
            (key(unsafe { AVNumberOfChannelsKey }), CFNumber::from(2i32).as_CFType()),
            (key(unsafe { AVLinearPCMBitDepthKey }), CFNumber::from(16i32).as_CFType()),
            (key(unsafe { AVLinearPCMIsFloatKey }), CFBoolean::false_value().as_CFType()),
            (key(unsafe { AVLinearPCMIsBigEndianKey }), CFBoolean::false_value().as_CFType()),
            (key(unsafe { AVLinearPCMIsNonInterleaved }), CFBoolean::false_value().as_CFType()),
        ] as &[(CFString, CFType)]);
        let output: Retained<AnyObject> =
            unsafe { msg_send![class(c"AVAssetReaderTrackOutput")?, assetReaderTrackOutputWithTrack: track, outputSettings: as_object(&settings)] };
        let can_add: Bool = unsafe { msg_send![&reader, canAddOutput: &*output] };
        if !can_add.as_bool() {
            return Err("el lector de audio no acepta la salida PCM".to_string());
        }
        let _: () = unsafe { msg_send![&reader, addOutput: &*output] };
        let range = TimeRange { start: cmtime(from), duration: cmtime(to - from + TAIL_SECS) };
        let _: () = unsafe { msg_send![&reader, setTimeRange: range] };
        let started: Bool = unsafe { msg_send![&reader, startReading] };
        if !started.as_bool() {
            let error: *mut AnyObject = unsafe { msg_send![&reader, error] };
            return Err(format!("el lector de audio no arrancó: {}", error_text(error)));
        }
        Ok(Self { reader, output })
    }
}

impl Source for TrackReader {
    type C = SampleChunk;

    fn next_chunk(&mut self) -> Result<Option<SampleChunk>, String> {
        loop {
            let sample: *mut CMSampleBuffer = unsafe { msg_send![&self.output, copyNextSampleBuffer] };
            if sample.is_null() {
                let status: isize = unsafe { msg_send![&self.reader, status] };
                if status == READER_FAILED {
                    let error: *mut AnyObject = unsafe { msg_send![&self.reader, error] };
                    return Err(format!("el lector de audio falló: {}", error_text(error)));
                }
                return Ok(None);
            }
            let sample = Released(sample as *const c_void);
            let block = unsafe { CMSampleBufferGetDataBuffer(sample.0) };
            if block.is_null() {
                continue;
            }
            let frames = unsafe { CMBlockBufferGetDataLength(block) } / FRAME_BYTES;
            if frames == 0 {
                continue;
            }
            let pts = seconds(unsafe { CMSampleBufferGetPresentationTimeStamp(sample.0 as *const CMSampleBuffer) });
            let pos = (pts * AUDIO_RATE as f64).round() as i64;
            return Ok(Some(SampleChunk { _sample: sample, block, pos, frames }));
        }
    }
}

impl Drop for TrackReader {
    fn drop(&mut self) {
        // Si se suelta a medias (se canceló el export), el lector deja de decodificar.
        let _: () = unsafe { msg_send![&self.reader, cancelReading] };
    }
}

pub(super) struct AudioStream {
    asset: Retained<AnyObject>,
    track: Retained<AnyObject>,
    plan: Vec<Planned>,
    total: u64,
    /// Cuántos cuadros se han entregado.
    delivered: u64,
    /// El tramo en curso (índice en `plan`) y cuántos de sus cuadros ya salieron.
    index: usize,
    done: u64,
    /// El lector del tramo en curso, solo si no es silencio.
    current: Option<(TrackReader, Fill<SampleChunk>)>,
}

impl AudioStream {
    /// `None` si el video no tiene audio.
    pub(super) unsafe fn open(asset: &AnyObject, segs: &[AudioSeg]) -> Result<Option<Self>, String> {
        let tracks: Retained<NSArray<AnyObject>> = unsafe { msg_send![asset, tracksWithMediaType: ns(AVMediaTypeAudio)] };
        let Some(track) = tracks.firstObject() else { return Ok(None) };
        let asset = unsafe { Retained::retain(asset as *const AnyObject as *mut AnyObject) }.ok_or("no se pudo retener el video")?;
        let plan = plan_frames(segs);
        let total = plan.iter().map(|p| p.frames).sum();
        Ok(Some(Self { asset, track, plan, total, delivered: 0, index: 0, done: 0, current: None }))
    }

    /// Cuántos cuadros de audio (a `AUDIO_RATE`) tendrá el exportado.
    pub(super) fn total_frames(&self) -> u64 {
        self.total
    }

    /// El próximo bloque (hasta `max_frames` cuadros; bytes de PCM) o `None` al final.
    pub(super) fn next_block(&mut self, max_frames: usize) -> Result<Option<Vec<u8>>, String> {
        objc2::rc::autoreleasepool(|_| unsafe { self.next_block_inner(max_frames.max(1)) })
    }

    unsafe fn next_block_inner(&mut self, max_frames: usize) -> Result<Option<Vec<u8>>, String> {
        let remaining = self.total - self.delivered;
        if remaining == 0 {
            return Ok(None);
        }
        let want = (max_frames as u64).min(remaining) as usize;
        let mut out: Vec<u8> = Vec::with_capacity(want * FRAME_BYTES);
        while out.len() < want * FRAME_BYTES {
            let Some(seg) = self.plan.get(self.index).copied() else { break };
            let left = seg.frames - self.done;
            let n = ((want - out.len() / FRAME_BYTES) as u64).min(left) as usize;
            let at = out.len();
            if seg.silent {
                push_silence(&mut out, n);
            } else {
                if self.current.is_none() {
                    let from = seg.src_start as f64 / AUDIO_RATE as f64;
                    let reader = unsafe { TrackReader::start(&self.asset, &self.track, from, from + seg.frames as f64 / AUDIO_RATE as f64) }?;
                    self.current = Some((reader, Fill::new(seg.src_start)));
                }
                if let Some((reader, fill)) = &mut self.current {
                    fill.read(reader, &mut out, n)?;
                }
                apply_fade(&mut out[at..], self.done as usize, seg.frames as usize, FADE_FRAMES);
            }
            self.done += n as u64;
            if self.done >= seg.frames {
                self.index += 1;
                self.done = 0;
                self.current = None;
            }
        }
        self.delivered += (out.len() / FRAME_BYTES) as u64;
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, rate: f64) -> AudioSeg {
        AudioSeg { start, end, rate }
    }

    #[test]
    fn plan_de_cortes_y_silencios() {
        let plan = plan_frames(&[seg(0.0, 1.0, 1.0), seg(2.0, 2.5, 1.0), seg(3.0, 13.0, 2.0)]);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0], Planned { frames: 48_000, src_start: 0, silent: false });
        assert_eq!(plan[1], Planned { frames: 24_000, src_start: 96_000, silent: false });
        // 10 s a 2×: 5 s de silencio.
        assert_eq!(plan[2], Planned { frames: 240_000, src_start: 144_000, silent: true });
        assert_eq!(plan.iter().map(|p| p.frames).sum::<u64>(), 312_000);
    }

    #[test]
    fn plan_a_media_velocidad_dura_el_doble() {
        let plan = plan_frames(&[seg(0.0, 1.0, 0.5)]);
        assert_eq!(plan, vec![Planned { frames: 96_000, src_start: 0, silent: true }]);
    }

    #[test]
    fn plan_redondea_sin_deriva() {
        // Cada tramo mide 10,4 cuadros: 10, 11, 10... y el total es el redondeo del acumulado.
        let len = 10.4 / AUDIO_RATE as f64;
        let segs: Vec<AudioSeg> = (0..5).map(|i| seg(i as f64, i as f64 + len, 1.0)).collect();
        let plan = plan_frames(&segs);
        let frames: Vec<u64> = plan.iter().map(|p| p.frames).collect();
        assert_eq!(frames, vec![10, 11, 10, 11, 10]);
        assert_eq!(frames.iter().sum::<u64>(), 52);
        // Muchos tramos chicos: nunca se aleja más de medio cuadro del tiempo real.
        let segs: Vec<AudioSeg> = (0..1000).map(|i| seg(i as f64, i as f64 + 0.0123456, 1.0)).collect();
        let total: u64 = plan_frames(&segs).iter().map(|p| p.frames).sum();
        assert_eq!(total, (1000.0f64 * 0.0123456 * 48_000.0).round() as u64);
    }

    #[test]
    fn plan_salta_tramos_vacios_o_invalidos() {
        let plan = plan_frames(&[seg(1.0, 1.0, 1.0), seg(3.0, 2.0, 1.0), seg(0.0, f64::NAN, 1.0), seg(0.0, 0.5, 1.0)]);
        assert_eq!(plan, vec![Planned { frames: 24_000, src_start: 0, silent: false }]);
        assert!(plan_frames(&[]).is_empty());
    }

    #[test]
    fn el_fundido_deja_los_bordes_en_cero_y_el_medio_intacto() {
        assert_eq!(fade_gain(0, 2000, 720), 0.0);
        assert_eq!(fade_gain(1999, 2000, 720), 0.0);
        assert_eq!(fade_gain(720, 2000, 720), 1.0);
        assert_eq!(fade_gain(1279, 2000, 720), 1.0);
        assert!((fade_gain(360, 2000, 720) - 0.5).abs() < 1e-6);
        // Simétrico.
        assert_eq!(fade_gain(100, 2000, 720), fade_gain(1899, 2000, 720));
        // Un tramo más corto que dos fundidos baja el fundido a su mitad.
        assert_eq!(fade_gain(0, 10, 720), 0.0);
        assert_eq!(fade_gain(5, 10, 720), fade_gain(4, 10, 720));
        assert_eq!(fade_gain(0, 1, 720), 1.0);
    }

    fn pcm_const(frames: usize, value: i16) -> Vec<u8> {
        let mut pcm = Vec::new();
        for _ in 0..frames {
            pcm.extend_from_slice(&value.to_le_bytes());
            pcm.extend_from_slice(&value.to_le_bytes());
        }
        pcm
    }

    fn sample(pcm: &[u8], frame: usize, channel: usize) -> i16 {
        let i = frame * FRAME_BYTES + channel * 2;
        i16::from_le_bytes([pcm[i], pcm[i + 1]])
    }

    #[test]
    fn apply_fade_da_lo_mismo_por_bloques_que_de_una_vez() {
        let total = 5000;
        let mut whole = pcm_const(total, 10_000);
        apply_fade(&mut whole, 0, total, FADE_FRAMES);
        let mut parts = pcm_const(total, 10_000);
        let mut first = 0;
        for size in [1000, 1, 1799, 1200, 1000] {
            apply_fade(&mut parts[first * FRAME_BYTES..(first + size) * FRAME_BYTES], first, total, FADE_FRAMES);
            first += size;
        }
        assert_eq!(first, total);
        assert_eq!(whole, parts);
        assert_eq!(sample(&whole, 0, 0), 0);
        assert_eq!(sample(&whole, 0, 1), 0);
        assert_eq!(sample(&whole, total - 1, 0), 0);
        assert_eq!(sample(&whole, 2500, 0), 10_000);
        assert_eq!(sample(&whole, 360, 1), 5_000);
        // Las muestras negativas bajan igual.
        let mut neg = pcm_const(total, -10_000);
        apply_fade(&mut neg, 0, total, FADE_FRAMES);
        assert_eq!(sample(&neg, 360, 0), -5_000);
    }

    /// El valor de cada cuadro es su índice en el original (módulo i16).
    struct TestChunk {
        pos: i64,
        frames: usize,
    }

    impl Chunk for TestChunk {
        fn pos(&self) -> i64 {
            self.pos
        }
        fn frames(&self) -> usize {
            self.frames
        }
        fn copy(&self, from: usize, n: usize, dst: &mut [u8]) {
            for k in 0..n {
                let v = (self.pos + (from + k) as i64) as i16;
                dst[k * 4..k * 4 + 2].copy_from_slice(&v.to_le_bytes());
                dst[k * 4 + 2..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }

    struct TestSource(std::vec::IntoIter<TestChunk>);

    impl Source for TestSource {
        type C = TestChunk;
        fn next_chunk(&mut self) -> Result<Option<TestChunk>, String> {
            Ok(self.0.next())
        }
    }

    fn source(chunks: &[(i64, usize)]) -> TestSource {
        TestSource(chunks.iter().map(|&(pos, frames)| TestChunk { pos, frames }).collect::<Vec<_>>().into_iter())
    }

    #[test]
    fn fill_ubica_por_pts_rellena_huecos_y_el_final() {
        let mut src = source(&[(0, 100), (150, 100)]);
        let mut fill = Fill::new(90);
        let mut out = Vec::new();
        fill.read(&mut src, &mut out, 80).unwrap();
        assert_eq!(out.len(), 80 * FRAME_BYTES);
        for k in 0..80 {
            let want = match 90 + k as i64 {
                i if i < 100 => i as i16,
                i if i < 150 => 0,
                i => i as i16,
            };
            assert_eq!(sample(&out, k, 0), want, "cuadro {k}");
            assert_eq!(sample(&out, k, 1), want, "cuadro {k}");
        }
        // Lo que sigue: datos hasta 249 y después silencio, hasta completar lo pedido.
        let mut out = Vec::new();
        fill.read(&mut src, &mut out, 200).unwrap();
        assert_eq!(out.len(), 200 * FRAME_BYTES);
        assert_eq!(sample(&out, 0, 0), 170);
        assert_eq!(sample(&out, 79, 0), 249);
        assert_eq!(sample(&out, 80, 0), 0);
        assert_eq!(sample(&out, 199, 0), 0);
    }

    #[test]
    fn fill_con_trozos_solapados_o_anteriores_al_tramo() {
        // El primer trozo empieza antes del tramo; el segundo se solapa con el primero.
        let mut src = source(&[(0, 100), (50, 100)]);
        let mut fill = Fill::new(40);
        let mut out = Vec::new();
        fill.read(&mut src, &mut out, 110).unwrap();
        for k in 0..110 {
            assert_eq!(sample(&out, k, 0), (40 + k) as i16, "cuadro {k}");
        }
    }

    #[test]
    fn fill_sin_audio_es_todo_silencio() {
        let mut src = source(&[]);
        let mut fill = Fill::new(1_000);
        let mut out = Vec::new();
        fill.read(&mut src, &mut out, 300).unwrap();
        assert_eq!(out, vec![0u8; 300 * FRAME_BYTES]);
    }

    #[test]
    fn fill_con_audio_que_empieza_despues_del_tramo() {
        let mut src = source(&[(30, 20)]);
        let mut fill = Fill::new(0);
        let mut out = Vec::new();
        fill.read(&mut src, &mut out, 60).unwrap();
        assert_eq!(sample(&out, 29, 0), 0);
        assert_eq!(sample(&out, 30, 0), 30);
        assert_eq!(sample(&out, 49, 0), 49);
        assert_eq!(sample(&out, 50, 0), 0);
        assert_eq!(out.len(), 60 * FRAME_BYTES);
    }
}
