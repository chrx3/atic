//! El audio del exportado (Windows): se lee tramo por tramo y por bloques (nunca entero en
//! memoria), con un fundido corto en cada empalme para que los cortes no suenen a clic. Cada
//! tramo a 1× salta a su cuadro con `SetCurrentPosition` y copia el PCM que entrega Media
//! Foundation; los tramos a otra velocidad van en silencio.
//!
//! El mismo contrato que la versión de macOS (`export_audio_macos.rs`): el plan de cuadros, los
//! fundidos, los huecos por PTS y el cierre de cada tramo son los mismos, y un lector que falla
//! es `Err` (no fin de archivo). La diferencia está en la fuente: `IMFSourceReader`, al que se le
//! pide PCM de 16 bits estéreo a 48 kHz para que él remuestree y mezcle los canales.

use std::path::Path;

use windows::core::{GUID, HSTRING};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSample, IMFSourceReader, MFAudioFormat_PCM, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Audio, MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;

use super::info_win::{win, Session, TICKS_PER_SECOND};

/// PCM de 16 bits estéreo entrelazado a 48 kHz.
pub(super) const AUDIO_RATE: u32 = 48_000;
pub(super) const FRAME_BYTES: usize = 4;

/// El fundido de entrada y de salida de cada tramo (15 ms).
const FADE_FRAMES: usize = AUDIO_RATE as usize * 15 / 1000;
/// Cuánto se deja leer de más al final de cada tramo, por si el último trozo cae justo en el borde.
const TAIL_SECS: f64 = 0.02;

/// La primera pista de audio (el índice negativo que entiende Media Foundation).
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

/// El tiempo en unidades de 100 ns (un `GUID` nulo).
const NOW: GUID = GUID::from_u128(0);

/// Un tramo del audio del original. Con `rate != 1.0` va en silencio (dura
/// `(end - start) / rate` en el exportado).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AudioSeg {
    pub start: f64,
    pub end: f64,
    pub rate: f64,
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

/// Lo que suman los tramos del plan: los cuadros que tendrá el exportado (lo que devolverá
/// [`AudioStream::total_frames`]).
fn planned_total(plan: &[Planned]) -> u64 {
    plan.iter().map(|p| p.frames).sum()
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

// --- Relleno por PTS (puro, con fuente abstracta) ---------------------------------------

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

// --- Media Foundation -------------------------------------------------------------------

/// Un trozo de PCM del lector, ya copiado: el búfer de Media Foundation se suelta al leerlo.
struct PcmChunk {
    pos: i64,
    /// PCM de 16 bits estéreo entrelazado, en cuadros completos.
    bytes: Vec<u8>,
}

impl Chunk for PcmChunk {
    fn pos(&self) -> i64 {
        self.pos
    }
    fn frames(&self) -> usize {
        self.bytes.len() / FRAME_BYTES
    }
    fn copy(&self, from: usize, n: usize, dst: &mut [u8]) {
        let n = n.min(dst.len() / FRAME_BYTES).min(self.frames().saturating_sub(from));
        let start = from * FRAME_BYTES;
        if n == 0 || start >= self.bytes.len() {
            return;
        }
        dst[..n * FRAME_BYTES].copy_from_slice(&self.bytes[start..start + n * FRAME_BYTES]);
    }
}

/// Los bytes de `sample`, ya seguidos y copiados, en cuadros completos de 16 bits estéreo.
fn bytes_of(sample: &IMFSample) -> Option<Vec<u8>> {
    // SAFETY: el búfer se bloquea y se desbloquea aquí; los bytes se copian antes de soltarlo.
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer().ok()?;
        let mut data = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock(&mut data, None, Some(&mut len)).ok()?;
        let bytes = if data.is_null() || (len as usize) < FRAME_BYTES {
            Vec::new()
        } else {
            let mut bytes = std::slice::from_raw_parts(data, len as usize).to_vec();
            bytes.truncate(bytes.len() & !(FRAME_BYTES - 1));
            bytes
        };
        let _ = buffer.Unlock();
        Some(bytes)
    }
}

/// Un lector de una sola pasada sobre un tramo del audio. La copia del `IMFSourceReader`
/// (clonar una interfaz COM es un `AddRef`) la comparte con [`AudioStream`], que es quien busca
/// en el archivo.
struct TrackReader {
    reader: IMFSourceReader,
    /// El cuadro del original desde el cual lo que venga ya no es del tramo (con la cola).
    limit: i64,
}

impl TrackReader {
    /// Deja el lector en `from` (cuadro del original) y corta en `limit` (con la cola).
    fn start(reader: &IMFSourceReader, from: i64, limit: i64) -> Result<Self, String> {
        let ticks = (from.max(0) as f64 * TICKS_PER_SECOND as f64 / AUDIO_RATE as f64).round() as i64;
        // SAFETY: el lector vive en `AudioStream` mientras esta copia lo tenga referenciado.
        unsafe { reader.SetCurrentPosition(&NOW, &PROPVARIANT::from(ticks)) }
            .map_err(|error| format!("no pude saltar en el audio: {}", win(error)))?;
        Ok(Self { reader: reader.clone(), limit })
    }
}

impl Source for TrackReader {
    type C = PcmChunk;

    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, String> {
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            // SAFETY: el lector vive en `self`.
            unsafe { self.reader.ReadSample(AUDIO, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample)) }
                .map_err(|error| format!("el lector de audio falló: {}", win(error)))?;
            // Un lector que falló no es fin de archivo: si no se distingue, el MP4 sale
            // truncado y se publica como bueno.
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err("el lector de audio falló".into());
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            // Los que no traen muestra (un `STREAMTICK`) no aportan audio.
            let Some(sample) = sample else { continue };
            let pos = (time as f64 * AUDIO_RATE as f64 / TICKS_PER_SECOND as f64).round() as i64;
            // Lo que empieza pasado el final del tramo (con la cola) sobra.
            if pos >= self.limit {
                return Ok(None);
            }
            let Some(bytes) = bytes_of(&sample) else { continue };
            if bytes.is_empty() {
                continue;
            }
            return Ok(Some(PcmChunk { pos, bytes }));
        }
    }
}

/// El audio del exportado, por bloques. Se usa desde el mismo hilo que lo abrió: Media
/// Foundation y COM son de ese hilo.
pub(super) struct AudioStream {
    reader: IMFSourceReader,
    plan: Vec<Planned>,
    total: u64,
    /// Cuántos cuadros se han entregado.
    delivered: u64,
    /// El tramo en curso (índice en `plan`) y cuántos de sus cuadros ya salieron.
    index: usize,
    done: u64,
    /// El lector del tramo en curso, solo si no es silencio.
    current: Option<(TrackReader, Fill<PcmChunk>)>,
    /// La sesión de Media Foundation, viva mientras el lector lea (se suelta al final, cuando
    /// ya no queda ningún objeto de Media Foundation).
    _session: Session,
}

impl AudioStream {
    /// `Ok(None)` si el video no tiene pista de audio.
    pub(super) fn open(source: &Path, segs: &[AudioSeg]) -> Result<Option<Self>, String> {
        let session = Session::start()?;
        // SAFETY: los objetos de Media Foundation se crean y se sueltan aquí, y el lector vive
        // mientras viva `AudioStream`.
        unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(source.as_os_str()), None::<&IMFAttributes>)
                .map_err(|error| format!("no pude abrir el video: {}", win(error)))?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(win)?;
            if reader.SetStreamSelection(AUDIO, true).is_err() || reader.GetNativeMediaType(AUDIO, 0).is_err() {
                return Ok(None);
            }
            let media_type = MFCreateMediaType().map_err(win)?;
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(win)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(win)?;
            media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(win)?;
            // Estéreo a `AUDIO_RATE`: el lector inserta el remuestreador y mezcla los canales.
            media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).map_err(win)?;
            media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(win)?;
            // Un cuadro son dos muestras de 16 bits: cuatro bytes.
            media_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, FRAME_BYTES as u32).map_err(win)?;
            media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_RATE * FRAME_BYTES as u32).map_err(win)?;
            reader
                .SetCurrentMediaType(AUDIO, None, &media_type)
                .map_err(|error| format!("no pude preparar el audio: {}", win(error)))?;
            let plan = plan_frames(segs);
            let total = planned_total(&plan);
            Ok(Some(Self { reader, plan, total, delivered: 0, index: 0, done: 0, current: None, _session: session }))
        }
    }

    /// Cuántos cuadros de audio (a `AUDIO_RATE`) tendrá el exportado.
    pub(super) fn total_frames(&self) -> u64 {
        self.total
    }

    /// El próximo bloque (hasta `frames` cuadros; bytes de PCM) o `None` al final.
    pub(super) fn next_block(&mut self, frames: usize) -> Result<Option<Vec<u8>>, String> {
        let remaining = self.total - self.delivered;
        if remaining == 0 {
            return Ok(None);
        }
        let want = (frames.max(1) as u64).min(remaining) as usize;
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
                    let tail = (TAIL_SECS * AUDIO_RATE as f64).round() as i64;
                    let reader = TrackReader::start(&self.reader, seg.src_start, seg.src_start + seg.frames as i64 + tail)?;
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
        assert_eq!(planned_total(&plan), 312_000);
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
        assert_eq!(planned_total(&plan), 52);
        // Muchos tramos chicos: nunca se aleja más de medio cuadro del tiempo real.
        let segs: Vec<AudioSeg> = (0..1000).map(|i| seg(i as f64, i as f64 + 0.0123456, 1.0)).collect();
        let total = planned_total(&plan_frames(&segs));
        assert_eq!(total, (1000.0f64 * 0.0123456 * 48_000.0).round() as u64);
    }

    #[test]
    fn plan_salta_tramos_vacios_o_invalidos() {
        let plan = plan_frames(&[seg(1.0, 1.0, 1.0), seg(3.0, 2.0, 1.0), seg(0.0, f64::NAN, 1.0), seg(0.0, 0.5, 1.0)]);
        assert_eq!(plan, vec![Planned { frames: 24_000, src_start: 0, silent: false }]);
        assert!(plan_frames(&[]).is_empty());
    }

    #[test]
    fn total_frames_es_la_suma_del_plan() {
        assert_eq!(planned_total(&plan_frames(&[])), 0);
        assert_eq!(planned_total(&plan_frames(&[seg(0.0, 1.0, 1.0)])), 48_000);
        // A 2× dura la mitad: 2 s del original son 1 s de audio.
        assert_eq!(planned_total(&plan_frames(&[seg(0.0, 2.0, 2.0)])), 48_000);
        // Un tramo inválido no suma.
        assert_eq!(planned_total(&plan_frames(&[seg(2.0, 2.0, 1.0)])), 0);
    }

    #[test]
    fn el_silencio_son_ceros_del_largo_pedido() {
        let mut out = vec![7u8; FRAME_BYTES];
        push_silence(&mut out, 3);
        assert_eq!(out.len(), 4 * FRAME_BYTES);
        assert!(out[FRAME_BYTES..].iter().all(|&byte| byte == 0));
        push_silence(&mut out, 0);
        assert_eq!(out.len(), 4 * FRAME_BYTES);
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
    fn un_trozo_solo_copia_los_cuadros_que_le_tocan() {
        let chunk = PcmChunk { pos: 10, bytes: pcm_const(5, 1234) };
        assert_eq!(chunk.frames(), 5);
        let mut dst = vec![0u8; 2 * FRAME_BYTES];
        // Del cuadro 3 del trozo salen solo dos.
        chunk.copy(3, 4, &mut dst);
        assert_eq!(sample(&dst, 0, 0), 1234);
        assert_eq!(sample(&dst, 1, 1), 1234);
        // Pedir más de lo que queda no se pasa del final ni del destino.
        let mut dst = vec![0u8; 4 * FRAME_BYTES];
        chunk.copy(4, 4, &mut dst);
        assert_eq!(sample(&dst, 0, 0), 1234);
        assert_eq!(sample(&dst, 1, 0), 0);
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

    /// Un video de verdad, a mano: `ATIC_TEST_VIDEO=<video con audio>`. Abre el audio, pide un
    /// tramo a 1× y otro a 2× (este en silencio) y comprueba que lo entregado suma
    /// `total_frames()`.
    #[test]
    #[ignore]
    fn lee_el_audio_de_un_video_de_verdad() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let segs = [seg(0.0, 1.0, 1.0), seg(1.0, 3.0, 2.0)];
        let mut audio = AudioStream::open(Path::new(&source), &segs)
            .expect("abrir el audio")
            .expect("el video tiene audio");
        let total = audio.total_frames();
        // 1 s a 1× más 2 s a 2×.
        assert_eq!(total, 96_000);
        let mut got = 0u64;
        while let Some(bytes) = audio.next_block(4_800).expect("leer el audio") {
            assert!(!bytes.is_empty());
            assert_eq!(bytes.len() % FRAME_BYTES, 0);
            let frames = (bytes.len() / FRAME_BYTES) as u64;
            got += frames;
            assert!(got <= total, "se entregó más de lo que dice total_frames");
        }
        assert_eq!(got, total);
        // El lector se puede volver a usar: lo mismo que pidió el exportador, otro bloque.
        assert!(audio.next_block(4_800).expect("releer").is_none());
    }
}
