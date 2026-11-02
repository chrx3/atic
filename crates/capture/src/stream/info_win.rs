//! Leer un video sin reproducirlo (Windows): lo que dura, sus medidas, sus miniaturas y sus
//! cuadros sueltos. Media Foundation decodifica y, con
//! `MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING`, convierte y escala a BGRA en el sistema.
//!
//! El mismo contrato que la versión de macOS (`export_macos.rs`): los cuadros que no se puedan
//! leer se saltan, y un archivo sin video es `Err`.

use std::path::Path;

use windows::core::{Error, GUID, HSTRING};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSample, IMFSourceReader, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Video, MFShutdown, MFStartup, MFVideoFormat_ARGB32, MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
    MFSTARTUP_FULL, MF_VERSION,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::{Frame, Rect};

/// Media Foundation cuenta el tiempo en unidades de 100 ns.
pub(super) const TICKS_PER_SECOND: i64 = 10_000_000;

/// La primera pista de video (el índice negativo que entiende Media Foundation).
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// El tiempo en unidades de 100 ns (un `GUID` nulo).
const NOW: GUID = GUID::from_u128(0);

/// El texto de un error de Windows.
pub(super) fn win(error: Error) -> String {
    error.message()
}

/// Media Foundation y COM en este hilo, con su vuelta atrás al salir del alcance.
///
/// `MFStartup` se cuenta por hilo: cada lectura lo pide y lo suelta.
pub(super) struct Session {
    /// Si el `CoInitializeEx` se pidió aquí (si el hilo ya estaba en COM, no se suelta).
    com: bool,
}

impl Session {
    pub(super) fn start() -> Result<Self, String> {
        // SAFETY: se empareja con `CoUninitialize` en `Drop` cuando se pidió aquí.
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            match MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                Ok(()) => Ok(Self { com }),
                Err(error) => {
                    if com {
                        CoUninitialize();
                    }
                    Err(format!("Media Foundation no arrancó: {}", win(error)))
                }
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: empareja el `MFStartup` de `start` y, si se pidió, el `CoInitializeEx`.
        unsafe {
            let _ = MFShutdown();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

/// Media Foundation arriba hasta que el proceso termine (el reproductor del editor): apagarlo
/// (`MFShutdown`) con un motor de medios recién cerrado se queda esperando, y la ventana del
/// editor vive mientras corre la app. `MFStartup` cuenta referencias: esta no se suelta.
pub(super) fn keep_media_foundation() -> Result<(), String> {
    static STARTED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    // COM de este hilo: si ya estaba en otro modo, vale igual.
    // SAFETY: no se empareja a propósito (el hilo sigue en COM mientras viva).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    STARTED
        .get_or_init(|| {
            // SAFETY: sin `MFShutdown` a propósito.
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|error| format!("Media Foundation no arrancó: {}", win(error)))
        })
        .clone()
}

/// Lo que dura `source` (segundos), leído del atributo de presentación (0 si no lo dice).
pub(super) fn duration(reader: &IMFSourceReader) -> f64 {
    // SAFETY: el lector está vivo.
    unsafe { reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION) }
        .ok()
        .and_then(|value| u64::try_from(&value).ok())
        .map_or(0.0, |ticks| ticks as f64 / TICKS_PER_SECOND as f64)
}

/// Un video abierto: el lector y lo que dicen el archivo de sí mismo.
pub(super) struct Source {
    pub(super) reader: IMFSourceReader,
    /// Lo que dura, en segundos.
    pub(super) duration: f64,
    /// Medidas del archivo, en píxeles.
    pub(super) size: (u32, u32),
}

impl Source {
    /// Abre `source` con el procesador de video habilitado (para convertir y escalar). La
    /// sesión de Media Foundation tiene que estar abierta.
    pub(super) fn open(source: &Path) -> Result<Self, String> {
        // SAFETY: objetos de Media Foundation creados y soltados aquí.
        unsafe {
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 2).map_err(win)?;
            let attributes = attributes.ok_or("Media Foundation no dio atributos")?;
            // El avanzado convierte YUV a RGB-32 y además escala. Los dos juntos no se
            // aceptan (`E_INVALIDARG`), así que va solo el avanzado.
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1).map_err(win)?;
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(source.as_os_str()), &attributes)
                .map_err(|error| format!("no pude abrir el video: {}", win(error)))?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(win)?;
            // Sin pista de video (o con una que el lector no conoce) no hay nada que leer.
            reader
                .SetStreamSelection(VIDEO, true)
                .map_err(|_| "el archivo no tiene video".to_string())?;
            let media_type = reader
                .GetNativeMediaType(VIDEO, 0)
                .map_err(|_| "el archivo no tiene video".to_string())?;
            let packed = media_type
                .GetUINT64(&MF_MT_FRAME_SIZE)
                .map_err(|_| "el video no dice sus medidas".to_string())?;
            let mut size = unpack(packed);
            // Un HEVC viene con el tamaño alineado a 16 (1376×736 para 1354×720): lo que se ve es la
            // apertura mínima de visualización.
            let mut area = [0u8; 16];
            if media_type.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None).is_ok() {
                let cx = i32::from_le_bytes([area[8], area[9], area[10], area[11]]);
                let cy = i32::from_le_bytes([area[12], area[13], area[14], area[15]]);
                if cx > 0 && cy > 0 && (cx as u32) <= size.0 && (cy as u32) <= size.1 {
                    size = (cx as u32, cy as u32);
                }
            }
            if size.0 == 0 || size.1 == 0 {
                return Err("el video no dice sus medidas".into());
            }
            let seconds = duration(&reader);
            Ok(Self { duration: seconds, reader, size })
        }
    }
}

/// Las dos medidas de un `UINT64` de Media Foundation (el ancho arriba).
pub(super) fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// Las dos medidas en un `UINT64` como lo pide Media Foundation (el ancho arriba).
pub(super) fn pack(size: (u32, u32)) -> u64 {
    ((size.0 as u64) << 32) | size.1 as u64
}

/// El tamaño de los cuadros que se piden de un video de `size` a `height` de alto: nunca
/// agranda (como el generador de macOS) y deja el ancho par.
fn scaled(size: (u32, u32), height: u32) -> (u32, u32) {
    let (w, h) = size;
    let out_h = height.max(1).min(h.max(1));
    let out_w = (w as f64 * out_h as f64 / h.max(1) as f64).round().max(2.0) as u32;
    ((out_w & !1).max(2), out_h)
}

/// Un video listo para sacar cuadros a un tamaño fijo.
struct Video {
    source: Source,
    /// Las medidas de lo que entrega Media Foundation (ya escalado).
    out: (u32, u32),
    /// Bytes de una fila; negativo si el cuadro viene de abajo hacia arriba.
    stride: i32,
}

impl Video {
    fn open(source: &Path, height: u32) -> Result<Self, String> {
        let source = Source::open(source)?;
        let (out, stride) = configure(&source.reader, scaled(source.size, height))?;
        Ok(Self { source, out, stride })
    }

    /// El cuadro que se ve en el segundo `at`: el último con marca de tiempo anterior o igual
    /// (el primero, si el video empieza después). `None` si no se pudo leer.
    fn frame(&mut self, at: f64) -> Option<Frame> {
        let target = (at.max(0.0) * TICKS_PER_SECOND as f64).round() as i64;
        // SAFETY: el lector se creó aquí y se usa desde este hilo.
        unsafe { self.source.reader.SetCurrentPosition(&NOW, &PROPVARIANT::from(target)) }.ok()?;
        let (out, stride) = (self.out, self.stride);
        // El último cuadro antes del objetivo. Se guarda sin convertir (solo el elegido se pasa
        // a BGRA).
        let mut before: Option<IMFSample> = None;
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            // SAFETY: el lector vive en `self`.
            unsafe {
                self.source.reader.ReadSample(VIDEO, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample))
            }
            .ok()?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                // Se acabó el video: vale el último cuadro anterior al objetivo.
                return before.and_then(|sample| locked(&sample, |bytes| to_frame(bytes, out, stride)).flatten());
            }
            let Some(sample) = sample else { continue };
            // El lector arranca en el keyframe anterior: lo que sobra se descarta.
            if time <= target {
                before = Some(sample);
                continue;
            }
            // Este ya pasa el objetivo: el que se ve en `at` es el anterior (o este, si el
            // video empieza después).
            if let Some(previous) = before {
                if let Some(frame) = locked(&previous, |bytes| to_frame(bytes, out, stride)).flatten() {
                    return Some(frame);
                }
            }
            return locked(&sample, |bytes| to_frame(bytes, out, stride)).flatten();
        }
    }
}

/// Pide cuadros en BGRA del tamaño `out` (RGB32 y, si no se deja, ARGB32): Media Foundation
/// mete su procesador de video para convertir y escalar. Devuelve el tamaño real y los bytes
/// de una fila (negativo si el cuadro viene de abajo hacia arriba).
pub(super) fn configure(reader: &IMFSourceReader, out: (u32, u32)) -> Result<((u32, u32), i32), String> {
    let mut last = String::new();
    for subtype in [MFVideoFormat_RGB32, MFVideoFormat_ARGB32] {
        match configure_as(reader, out, &subtype) {
            Ok(done) => return Ok(done),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// [`configure`] con un subtipo concreto de video (los dos son BGRA en memoria).
fn configure_as(reader: &IMFSourceReader, out: (u32, u32), subtype: &GUID) -> Result<((u32, u32), i32), String> {
    // SAFETY: el lector está vivo.
    unsafe {
        let media_type = MFCreateMediaType().map_err(win)?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(win)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, subtype).map_err(win)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(win)?;
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack(out)).map_err(win)?;
        reader
            .SetCurrentMediaType(VIDEO, None, &media_type)
            .map_err(|error| format!("no pude preparar los cuadros: {}", win(error)))?;
        let current = reader.GetCurrentMediaType(VIDEO).map_err(win)?;
        let real = unpack(current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(win)?);
        if real.0 == 0 || real.1 == 0 {
            return Err("Media Foundation no dio las medidas del cuadro".into());
        }
        // El paso lo decide el decodificador: en RGB32 suele venir de abajo hacia arriba.
        let stride = match current.GetUINT32(&MF_MT_DEFAULT_STRIDE) {
            Ok(0) | Err(_) => real.0 as i32 * 4,
            Ok(value) => value as i32,
        };
        Ok((real, stride))
    }
}

/// Corre `read` con los bytes de `sample`, ya seguidos.
pub(super) fn locked<T>(sample: &IMFSample, read: impl FnOnce(&[u8]) -> T) -> Option<T> {
    // SAFETY: el buffer se bloquea y se desbloquea aquí; `read` no se guarda la rebanada.
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer().ok()?;
        let mut data = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock(&mut data, None, Some(&mut len)).ok()?;
        let out = if data.is_null() || len == 0 {
            None
        } else {
            Some(read(std::slice::from_raw_parts(data, len as usize)))
        };
        let _ = buffer.Unlock();
        out
    }
}

/// El cuadro BGRA de un buffer de Media Foundation: filas de `size.0 * 4` bytes con `stride`
/// bytes de paso, de abajo hacia arriba si es negativo. El alfa queda opaco (en RGB32 no
/// significa nada y suele venir en cero).
pub(super) fn to_frame(bytes: &[u8], size: (u32, u32), stride: i32) -> Option<Frame> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let row = w * 4;
    let step = stride.unsigned_abs() as usize;
    if w == 0 || h == 0 || step < row || bytes.len() < (h - 1) * step + row {
        return None;
    }
    let mut bgra = vec![0u8; row * h];
    for y in 0..h {
        let from = if stride < 0 { h - 1 - y } else { y };
        bgra[y * row..(y + 1) * row].copy_from_slice(&bytes[from * step..from * step + row]);
    }
    for pixel in bgra.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    Some(Frame::new(Rect::new(0, 0, size.0, size.1), bgra))
}

/// Lo que dura `source` (segundos) y sus medidas en píxeles.
pub fn video_info(source: &Path) -> Result<(f64, (f64, f64)), String> {
    let _session = Session::start()?;
    let source = Source::open(source)?;
    Ok((source.duration, (source.size.0 as f64, source.size.1 as f64)))
}

/// `count` miniaturas repartidas en el video, de `height` píxeles de alto (para la línea de
/// tiempo). Las que no se puedan leer se saltan, como en macOS.
pub fn thumbnails(source: &Path, count: usize, height: u32) -> Result<Vec<Frame>, String> {
    let _session = Session::start()?;
    let mut video = Video::open(source, height)?;
    let mut frames = Vec::new();
    for i in 0..count {
        // En mitad de cada tramo, para no caer siempre en el borde (como en macOS).
        let at = video.source.duration * (i as f64 + 0.5) / count.max(1) as f64;
        if let Some(frame) = video.frame(at) {
            frames.push(frame);
        }
    }
    Ok(frames)
}

/// Los cuadros del video en los segundos `times` (ya dentro de `0..duración`), de `height`
/// píxeles de alto y con el tiempo exacto pedido (sin la tolerancia de las miniaturas). Para
/// que un agente «vea» el video. Los que no se pudieron leer se saltan.
pub fn frames_at(source: &Path, times: &[f64], height: u32) -> Result<Vec<(f64, Frame)>, String> {
    let _session = Session::start()?;
    let mut video = Video::open(source, height)?;
    let last = (video.source.duration - 0.05).max(0.0);
    let mut frames = Vec::new();
    for &t in times {
        if let Some(frame) = video.frame(t.clamp(0.0, last)) {
            frames.push((t, frame));
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_tamano_respeta_la_proporcion_y_deja_el_ancho_par() {
        // 170,6 → 171 → 170 (par).
        assert_eq!(scaled((1920, 1080), 96), (170, 96));
        assert_eq!(scaled((1080, 1920), 96), (54, 96));
        assert_eq!(scaled((1000, 1000), 96), (96, 96));
        // Un ancho que redondea a 5 baja a 4 (el par de abajo).
        assert_eq!(scaled((52, 1000), 96), (4, 96));
        // Un cuadro casi sin ancho deja el mínimo.
        assert_eq!(scaled((10, 1000), 96), (2, 96));
    }

    #[test]
    fn nunca_agranda_el_cuadro() {
        // Pedir más alto que el video devuelve el tamaño del video.
        assert_eq!(scaled((640, 480), 1080), (640, 480));
        assert_eq!(scaled((640, 480), 0), (2, 1));
    }

    #[test]
    fn las_medidas_van_y_vuelven_del_u64() {
        assert_eq!(unpack(pack((1920, 1080))), (1920, 1080));
        assert_eq!(pack((1920, 1080)), 1920u64 << 32 | 1080);
    }

    #[test]
    fn un_cuadro_de_abajo_hacia_arriba_se_voltea() {
        let ar = [1u8, 2, 3, 0];
        let ab = [4u8, 5, 6, 0];
        let br = [7u8, 8, 9, 0];
        let bb = [10u8, 11, 12, 0];
        // El buffer trae primero la fila de abajo.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&br);
        bytes.extend_from_slice(&bb);
        bytes.extend_from_slice(&ar);
        bytes.extend_from_slice(&ab);
        let frame = to_frame(&bytes, (2, 2), -8).expect("un cuadro");
        assert_eq!((frame.width(), frame.height()), (2, 2));
        let mut esperado = Vec::new();
        for p in [ar, ab, br, bb] {
            esperado.extend_from_slice(&p);
        }
        // El alfa queda opaco en los cuatro.
        for pixel in esperado.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        assert_eq!(frame.bgra, esperado);
    }

    #[test]
    fn una_fila_con_relleno_se_recorta() {
        let (a, b, c, d) = ([1u8, 2, 3, 0], [4u8, 5, 6, 0], [7u8, 8, 9, 0], [10u8, 11, 12, 0]);
        // Filas de 12 bytes para 8 de píxeles: los últimos 4 no se usan.
        let mut bytes = Vec::new();
        for fila in [[a, b], [c, d]] {
            for p in fila {
                bytes.extend_from_slice(&p);
            }
            bytes.extend_from_slice(&[200, 200, 200, 200]);
        }
        let frame = to_frame(&bytes, (2, 2), 12).expect("un cuadro");
        let mut esperado = Vec::new();
        for p in [a, b, c, d] {
            esperado.extend_from_slice(&p);
        }
        for pixel in esperado.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        assert_eq!(frame.bgra, esperado);
    }

    #[test]
    fn una_rebanada_corta_no_da_cuadro() {
        // Falta la última fila.
        assert!(to_frame(&[0u8; 16], (2, 3), 8).is_none());
        // Un paso menor que la fila no cuadra.
        assert!(to_frame(&[0u8; 32], (4, 2), 8).is_none());
        // Sin medidas no hay nada.
        assert!(to_frame(&[0u8; 32], (0, 2), 8).is_none());
    }

    /// Un MP4 de verdad, a mano: `ATIC_TEST_VIDEO=<video>` con `thumbnails` a 2 y las medidas.
    #[test]
    #[ignore]
    fn lee_un_video_de_verdad() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let (duration, (w, h)) = video_info(Path::new(&source)).expect("medidas");
        println!("{source}: {duration} s, {w}×{h}");
        assert!(duration > 0.0 && w > 0.0 && h > 0.0);
        let thumbs = thumbnails(Path::new(&source), 4, 60).expect("miniaturas");
        assert!(!thumbs.is_empty());
        for thumb in &thumbs {
            // Nunca agranda: un video más bajo que 60 queda con su propio alto.
            assert!(thumb.height() > 0 && thumb.height() <= 60);
            assert_eq!(thumb.bgra.len(), thumb.width() as usize * thumb.height() as usize * 4);
        }
        let frames = frames_at(Path::new(&source), &[duration / 4.0], 120).expect("cuadros");
        assert!(!frames.is_empty());
    }
}
