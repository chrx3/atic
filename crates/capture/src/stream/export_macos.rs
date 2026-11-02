//! Exportar una grabación editada: el mismo video con la cámara (zoom y encuadre),
//! el cursor dibujado y los textos, sin los tramos cortados. Todo en la GPU: AVAssetReader
//! decodifica, Core Image recorta, escala y compone, y AVAssetWriter codifica por
//! hardware. El audio sigue los mismos cortes.
//!
//! Quien exporta decide qué se ve en cada cuadro (`Decor`): este módulo no sabe de
//! proyectos, zooms ni resortes, solo dibuja lo que le piden.
//!
//! También: [`thumbnails`] (las miniaturas de la línea de tiempo).

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_video::pixel_buffer::{CVPixelBuffer, CVPixelBufferRef};
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::msg_send;
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSError, NSString};

use super::export_audio_macos::{AudioSeg, AudioStream, AUDIO_RATE, FRAME_BYTES};
use super::export_text_macos::text_image;
use crate::{Frame, Rect};

use super::types::*;

const BGRA: u32 = u32::from_be_bytes(*b"BGRA");
const NV12_FULL: u32 = u32::from_be_bytes(*b"420f");
const AAC: u32 = u32::from_be_bytes(*b"aac ");
const READER_READING: isize = 1;
const READER_COMPLETED: isize = 2;
const READER_FAILED: isize = 3;
const WRITER_WRITING: isize = 1;
const WRITER_COMPLETED: isize = 2;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeVideo: CFStringRef;
    static AVMediaTypeAudio: CFStringRef;
    static AVFileTypeMPEG4: CFStringRef;
    static AVVideoCodecKey: CFStringRef;
    static AVVideoCodecTypeH264: CFStringRef;
    static AVVideoCodecTypeHEVC: CFStringRef;
    static AVVideoWidthKey: CFStringRef;
    static AVVideoHeightKey: CFStringRef;
    static AVVideoCompressionPropertiesKey: CFStringRef;
    static AVVideoAverageBitRateKey: CFStringRef;
    static AVVideoExpectedSourceFrameRateKey: CFStringRef;
    static AVVideoProfileLevelKey: CFStringRef;
    static AVVideoProfileLevelH264HighAutoLevel: CFStringRef;
    static AVFormatIDKey: CFStringRef;
    static AVSampleRateKey: CFStringRef;
    static AVNumberOfChannelsKey: CFStringRef;
    static AVEncoderBitRateKey: CFStringRef;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "CoreImage", kind = "framework")]
extern "C" {}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    static kCVPixelBufferWidthKey: CFStringRef;
    static kCVPixelBufferHeightKey: CFStringRef;
    static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    static kCVPixelBufferMetalCompatibilityKey: CFStringRef;
    static kCVPixelBufferPoolMinimumBufferCountKey: CFStringRef;
    static kCVImageBufferYCbCrMatrixKey: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_601_4: CFStringRef;
    fn CVBufferSetAttachment(buffer: *mut CVBuffer, key: CFStringRef, value: *const c_void, mode: u32);
    fn CVPixelBufferPoolCreate(allocator: *const c_void, pool_attributes: *const c_void, buffer_attributes: *const c_void, out: *mut *mut PixelPool) -> i32;
    fn CVPixelBufferCreate(allocator: *const c_void, width: usize, height: usize, format: u32, attributes: *const c_void, out: *mut *mut CVBuffer) -> i32;
    fn CVPixelBufferPoolCreatePixelBuffer(allocator: *const c_void, pool: *const PixelPool, out: *mut *mut CVBuffer) -> i32;
}

/// `kCVAttachmentMode_ShouldPropagate`.
const ATTACH_PROPAGATE: u32 = 1;

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sample: *const c_void) -> *const c_void;
    fn CMSampleBufferGetPresentationTimeStamp(sample: *const CMSampleBuffer) -> CMTime;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut ColorSpace;
    fn CGImageGetWidth(image: *const CGImageRaw) -> usize;
    fn CGImageGetHeight(image: *const CGImageRaw) -> usize;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        bytes_per_row: usize,
        space: *const c_void,
        info: u32,
    ) -> *mut c_void;
    fn CGContextDrawImage(context: *mut c_void, rect: CGRect, image: *const CGImageRaw);
}

/// Un `CGImage` en BGRA (filas de arriba abajo).
unsafe fn bgra_of(image: *const CGImageRaw) -> Option<(u32, u32, Vec<u8>)> {
    // `kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`.
    const INFO: u32 = 2 | (2 << 12);
    let (w, h) = unsafe { (CGImageGetWidth(image), CGImageGetHeight(image)) };
    if w == 0 || h == 0 {
        return None;
    }
    let mut buffer = vec![0u8; w * h * 4];
    let space = unsafe { CGColorSpaceCreateDeviceRGB() };
    let _space = Released(space as *const c_void);
    let context = unsafe { CGBitmapContextCreate(buffer.as_mut_ptr().cast(), w, h, 8, w * 4, space as *const c_void, INFO) };
    if context.is_null() {
        return None;
    }
    let _context = Released(context as *const c_void);
    unsafe { CGContextDrawImage(context, CGRect::new(0.0, 0.0, w as f64, h as f64), image) };
    Some((w as u32, h as u32, buffer))
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

// --- Tipos de C con su codificación para objc2 ---------------------------------------

macro_rules! opaque {
    ($name:ident, $c:literal) => {
        #[repr(C)]
        pub(crate) struct $name {
            _private: [u8; 0],
        }
        // SAFETY: la codificación del puntero de C (`^{nombre=}`).
        unsafe impl RefEncode for $name {
            const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct($c, &[]));
        }
    };
}
opaque!(CVBuffer, "__CVBuffer");
opaque!(ColorSpace, "CGColorSpace");
opaque!(CGImageRaw, "CGImage");
opaque!(PixelPool, "__CVPixelBufferPool");

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}
// SAFETY: las mismas disposiciones que CoreGraphics.
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}
unsafe impl RefEncode for CGRect {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}
unsafe impl Encode for Affine {
    const ENCODING: Encoding = Encoding::Struct(
        "CGAffineTransform",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
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

impl CGRect {
    fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { origin: CGPoint { x, y }, size: CGSize { width: w, height: h } }
    }
}

impl Affine {
    fn translate(tx: f64, ty: f64) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx, ty }
    }
    fn scale(s: f64) -> Self {
        Self { a: s, b: 0.0, c: 0.0, d: s, tx: 0.0, ty: 0.0 }
    }
    fn scale_xy(sx: f64, sy: f64) -> Self {
        Self { a: sx, b: 0.0, c: 0.0, d: sy, tx: 0.0, ty: 0.0 }
    }
}

// --- Ayudas -----------------------------------------------------------------------

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

fn dict(pairs: &[(CFString, CFType)]) -> CFDictionary<CFString, CFType> {
    CFDictionary::from_CFType_pairs(pairs)
}

fn error_text(error: *mut AnyObject) -> String {
    // SAFETY: un `NSError` o nulo.
    unsafe { (error as *mut NSError).as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default()
}

fn file_url(path: &Path) -> Result<Retained<AnyObject>, String> {
    let url: Retained<AnyObject> = unsafe { msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&path.to_string_lossy())] };
    Ok(url)
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

fn writer_error(writer: &AnyObject) -> String {
    let error: *mut AnyObject = unsafe { msg_send![writer, error] };
    error_text(error)
}

fn reader_error(reader: &AnyObject) -> String {
    let error: *mut AnyObject = unsafe { msg_send![reader, error] };
    error_text(error)
}

/// `finishWritingWithCompletionHandler:` y espera.
unsafe fn finish_writing(writer: &AnyObject) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let done = RcBlock::new(move || {
        let _ = tx.send(());
    });
    let _: () = unsafe { msg_send![writer, finishWritingWithCompletionHandler: &*done] };
    rx.recv_timeout(Duration::from_secs(300)).map_err(|_| "AVAssetWriter no terminó".to_string())?;
    let status: isize = unsafe { msg_send![writer, status] };
    if status != WRITER_COMPLETED {
        return Err(format!("AVAssetWriter terminó mal ({status}): {}", writer_error(writer)));
    }
    Ok(())
}

// --- Exportar ------------------------------------------------------------------------

/// El video de `source` y sus medidas en píxeles.
unsafe fn video_track(asset: &AnyObject) -> Result<(Retained<AnyObject>, (f64, f64)), String> {
    let tracks: Retained<NSArray<AnyObject>> = unsafe { msg_send![asset, tracksWithMediaType: ns(AVMediaTypeVideo)] };
    let track = tracks.firstObject().ok_or("el archivo no tiene video")?;
    let size: CGSize = unsafe { msg_send![&track, naturalSize] };
    Ok((track, (size.width, size.height)))
}

/// Los cuadros por segundo que declara la pista (0 si no los dice). Es la referencia
/// para saber si hay que descartar cuadros al exportar a otra cadencia.
unsafe fn track_fps(track: &AnyObject) -> f64 {
    let fps: f32 = unsafe { msg_send![track, nominalFrameRate] };
    fps.max(0.0) as f64
}

/// Lo que dura `source` (segundos) y sus medidas en píxeles.
pub fn video_info(source: &Path) -> Result<(f64, (f64, f64)), String> {
    objc2::rc::autoreleasepool(|_| unsafe {
        let none: *const AnyObject = std::ptr::null();
        let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*file_url(source)?, options: none];
        let duration = seconds(msg_send![&asset, duration]);
        let (_, size) = video_track(&asset)?;
        Ok((duration, size))
    })
}

/// Exporta `source` a `out` (MP4 con las opciones de `job.options`) según `job`.
pub fn export(source: &Path, out: &Path, job: Export) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| unsafe { export_inner(source, out, job) })
}

/// Los textos, dibujados cuando se piden y con un tope en memoria: unos subtítulos pueden
/// ser cientos.
struct TextCache {
    specs: Vec<TextSpec>,
    size: (f64, f64),
    images: HashMap<usize, Option<Retained<AnyObject>>>,
    order: VecDeque<usize>,
}

impl TextCache {
    /// Cuántos textos dibujados se guardan a la vez.
    const KEEP: usize = 24;

    fn new() -> Self {
        Self { specs: Vec::new(), size: (0.0, 0.0), images: HashMap::new(), order: VecDeque::new() }
    }

    /// Los textos y el cuadro nuevos. Se conservan los ya dibujados que siguen iguales
    /// (mismo índice, mismo texto y mismo cuadro): cambiar otra cosa de la escena no los
    /// rehace.
    fn set(&mut self, specs: &[TextSpec], size: (f64, f64)) {
        if size != self.size {
            self.images.clear();
            self.order.clear();
        } else {
            let old = &self.specs;
            self.images.retain(|index, _| old.get(*index).is_some_and(|spec| specs.get(*index) == Some(spec)));
            let images = &self.images;
            self.order.retain(|index| images.contains_key(index));
        }
        self.specs = specs.to_vec();
        self.size = size;
    }

    /// La imagen del texto `index`; `None` si no existe o no se pudo dibujar (se avisa una vez).
    unsafe fn get(&mut self, index: usize) -> Option<Retained<AnyObject>> {
        if let Some(image) = self.images.get(&index) {
            return image.clone();
        }
        let spec = self.specs.get(index)?;
        let image = match unsafe { text_image(spec, self.size.0, self.size.1) } {
            Ok(image) => Some(image),
            Err(error) => {
                tracing::warn!(index, %error, "exportar: no se pudo dibujar un texto");
                None
            }
        };
        if self.order.len() >= Self::KEEP {
            if let Some(old) = self.order.pop_front() {
                self.images.remove(&old);
            }
        }
        self.order.push_back(index);
        self.images.insert(index, image.clone());
        image
    }
}

unsafe fn export_inner(source: &Path, out: &Path, job: Export) -> Result<(), String> {
    let none: *const AnyObject = std::ptr::null();
    let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*file_url(source)?, options: none];
    let (track, (src_w, src_h)) = video_track(&asset)?;
    let source_fps = track_fps(&track);
    let options = job.options.clone();
    let (out_w, out_h) = output_size((src_w as u32, src_h as u32), options.max_height, options.aspect);
    let segments: Vec<(f64, f64)> = job.segments.iter().copied().filter(|(a, b)| b > a).collect();
    let pieces = pieces(&segments, &job.speeds);
    if pieces.is_empty() {
        return Err("no queda nada del video".into());
    }
    // Lo que dura el exportado: el progreso y el cierre de la sesión cuentan en su tiempo.
    let total = out_length(&pieces);
    let fps = options.fps.clamp(1, 240);

    // El audio, por bloques: nunca entero en memoria.
    let audio_segs: Vec<AudioSeg> = pieces.iter().map(|p| AudioSeg { start: p.a, end: p.b, rate: p.rate }).collect();
    let mut audio = AudioStream::open(&asset, &audio_segs)?.filter(|a| a.total_frames() > 0);

    // El lector del video, en BGRA.
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let reader: Option<Retained<AnyObject>> = msg_send![class(c"AVAssetReader")?, assetReaderWithAsset: &*asset, error: &mut error];
    let reader = reader.ok_or_else(|| format!("AVAssetReader: {}", error_text(error)))?;
    // En YUV, como viene del codificador: Core Image lo convierte en la GPU al leerlo.
    let read_settings = dict(&[(key(kCVPixelBufferPixelFormatTypeKey), CFNumber::from(NV12_FULL as i32).as_CFType())]);
    let video_out: Retained<AnyObject> =
        msg_send![class(c"AVAssetReaderTrackOutput")?, assetReaderTrackOutputWithTrack: &*track, outputSettings: as_object(&read_settings)];
    let _: () = msg_send![&video_out, setAlwaysCopiesSampleData: Bool::NO];
    let _: () = msg_send![&reader, addOutput: &*video_out];

    // El escritor.
    let _ = std::fs::remove_file(out);
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let writer: Option<Retained<AnyObject>> =
        msg_send![class(c"AVAssetWriter")?, assetWriterWithURL: &*file_url(out)?, fileType: ns(AVFileTypeMPEG4), error: &mut error];
    let writer = writer.ok_or_else(|| format!("AVAssetWriter: {}", error_text(error)))?;
    let bitrate = bitrate(out_w, out_h, options.quality, options.codec);
    let mut compression = vec![
        (key(AVVideoAverageBitRateKey), CFNumber::from(bitrate).as_CFType()),
        (key(AVVideoExpectedSourceFrameRateKey), CFNumber::from(fps as i32).as_CFType()),
    ];
    // El perfil de H.264 no vale para HEVC.
    if options.codec == Codec::H264 {
        compression.push((key(AVVideoProfileLevelKey), key(AVVideoProfileLevelH264HighAutoLevel).as_CFType()));
    }
    let compression = dict(&compression);
    let codec = match options.codec {
        Codec::H264 => key(AVVideoCodecTypeH264),
        Codec::Hevc => key(AVVideoCodecTypeHEVC),
    };
    let video_settings = dict(&[
        (key(AVVideoCodecKey), codec.as_CFType()),
        (key(AVVideoWidthKey), CFNumber::from(out_w as i32).as_CFType()),
        (key(AVVideoHeightKey), CFNumber::from(out_h as i32).as_CFType()),
        (key(AVVideoCompressionPropertiesKey), compression.as_CFType()),
    ]);
    let video_in: Retained<AnyObject> =
        msg_send![class(c"AVAssetWriterInput")?, assetWriterInputWithMediaType: ns(AVMediaTypeVideo), outputSettings: as_object(&video_settings)];
    let empty: CFDictionary<CFString, CFType> = dict(&[]);
    let pool_attrs = dict(&[
        (key(kCVPixelBufferPixelFormatTypeKey), CFNumber::from(BGRA as i32).as_CFType()),
        (key(kCVPixelBufferWidthKey), CFNumber::from(out_w as i32).as_CFType()),
        (key(kCVPixelBufferHeightKey), CFNumber::from(out_h as i32).as_CFType()),
        (key(kCVPixelBufferIOSurfacePropertiesKey), empty.as_CFType()),
    ]);
    let adaptor: Retained<AnyObject> = msg_send![
        class(c"AVAssetWriterInputPixelBufferAdaptor")?,
        assetWriterInputPixelBufferAdaptorWithAssetWriterInput: &*video_in,
        sourcePixelBufferAttributes: as_object(&pool_attrs)
    ];
    let audio_in = audio
        .is_some()
        .then(|| -> Result<Retained<AnyObject>, String> {
            let aac = dict(&[
                (key(AVFormatIDKey), CFNumber::from(AAC as i32).as_CFType()),
                (key(AVSampleRateKey), CFNumber::from(AUDIO_RATE as f64).as_CFType()),
                (key(AVNumberOfChannelsKey), CFNumber::from(2i32).as_CFType()),
                (key(AVEncoderBitRateKey), CFNumber::from(160_000i32).as_CFType()),
            ]);
            let format = super::record_macos::pcm_format(AUDIO_RATE)?;
            let input: Retained<AnyObject> = msg_send![
                class(c"AVAssetWriterInput")?,
                assetWriterInputWithMediaType: ns(AVMediaTypeAudio),
                outputSettings: as_object(&aac),
                sourceFormatHint: format.as_ptr()
            ];
            Ok(input)
        })
        .transpose()?;
    for input in std::iter::once(&video_in).chain(audio_in.as_ref()) {
        let _: () = msg_send![&**input, setExpectsMediaDataInRealTime: Bool::NO];
        let can: Bool = msg_send![&writer, canAddInput: &**input];
        if !can.as_bool() {
            return Err("AVAssetWriter no acepta las pistas".into());
        }
        let _: () = msg_send![&writer, addInput: &**input];
    }

    // El mismo Composer que usa la vista previa: Core Image, el cursor, el marco y los
    // textos, armados una vez.
    let mut composer = Composer::new()?;
    composer.set_scene(&Scene {
        src: (src_w, src_h),
        out: (out_w, out_h),
        frame: job.frame.clone(),
        texts: job.texts.clone(),
        cursor_px: job.cursor_px,
        look: job.look,
    })?;

    let first = pieces[0].a;
    let last = pieces[pieces.len() - 1].b;
    let range = TimeRange { start: cmtime(first), duration: cmtime(last - first) };
    let _: () = msg_send![&reader, setTimeRange: range];
    let reading: Bool = msg_send![&reader, startReading];
    if !reading.as_bool() {
        let error: *mut AnyObject = msg_send![&reader, error];
        return Err(format!("AVAssetReader no empezó: {}", error_text(error)));
    }
    let started: Bool = msg_send![&writer, startWriting];
    if !started.as_bool() {
        return Err(format!("AVAssetWriter no empezó: {}", writer_error(&writer)));
    }
    let _: () = msg_send![&writer, startSessionAtSourceTime: cmtime(0.0)];

    let ready = |input: &AnyObject| -> bool {
        let ready: Bool = msg_send![input, isReadyForMoreMediaData];
        ready.as_bool()
    };
    let block = (AUDIO_RATE / 10) as usize;
    let mut audio_at = 0u64;
    let mut video_done = false;
    let mut audio_done = audio_in.is_none();
    let mut last_out = f64::NEG_INFINITY;
    let mut idle = Instant::now();
    let format = if audio_in.is_some() { Some(super::record_macos::pcm_format(AUDIO_RATE)?) } else { None };
    let result = (|| -> Result<(), String> {
        while !(video_done && audio_done) {
            if job.cancel.load(Ordering::Relaxed) {
                return Err("cancelada".into());
            }
            let mut progress = false;
            if !video_done && ready(&video_in) {
                progress = true;
                let status: isize = msg_send![&reader, status];
                let sample: *mut CMSampleBuffer = if status == READER_READING { msg_send![&video_out, copyNextSampleBuffer] } else { std::ptr::null_mut() };
                if sample.is_null() {
                    // Un lector que falló (archivo dañado, disco) no es fin de archivo:
                    // si no se distingue, el MP4 sale truncado y se publica como bueno.
                    // Y uno que sigue leyendo no terminó: el próximo intento decide (o
                    // el guardia de atasco de abajo).
                    let now: isize = msg_send![&reader, status];
                    if now == READER_FAILED {
                        return Err(format!("el lector del video falló: {}", reader_error(&reader)));
                    }
                    if now == READER_COMPLETED {
                        let _: () = msg_send![&video_in, markAsFinished];
                        video_done = true;
                    }
                } else {
                    let _sample = Released(sample as *const c_void);
                    let src = seconds(CMSampleBufferGetPresentationTimeStamp(sample));
                    let pixels = CMSampleBufferGetImageBuffer(sample as *const c_void) as *mut CVBuffer;
                    if let (Some(at), false) = (out_time(&pieces, src), pixels.is_null()) {
                        if keep_at(&pieces, src, last_out, at, fps as f64, source_fps) {
                            last_out = at;
                            let decor = (job.decor)(src, at);
                            let image = composer.compose_buffer(pixels, &decor)?;
                            let mut target: *mut CVBuffer = std::ptr::null_mut();
                            // El del grupo del adaptador; si no hay grupo, uno suelto.
                            // El grupo se pide cada vez: AVFoundation puede cambiarlo y uno
                            // guardado quedaba colgando (se caía al azar).
                            let pool: *const PixelPool = msg_send![&adaptor, pixelBufferPool];
                            let mut status = if pool.is_null() { -1 } else { CVPixelBufferPoolCreatePixelBuffer(std::ptr::null(), pool, &mut target) };
                            // El grupo puede quedarse corto mientras el codificador retiene
                            // cuadros: uno suelto, con las mismas medidas.
                            if status != 0 || target.is_null() {
                                target = std::ptr::null_mut();
                                status = CVPixelBufferCreate(std::ptr::null(), out_w as usize, out_h as usize, BGRA, pool_attrs.as_concrete_TypeRef().cast(), &mut target);
                            }
                            if status != 0 || target.is_null() {
                                return Err(format!("sin búfer para el cuadro exportado ({status}, grupo {})", !pool.is_null()));
                            }
                            let _target = Released(target as *const c_void);
                            composer.draw(&image, target);
                            let ok: Bool = msg_send![&adaptor, appendPixelBuffer: target, withPresentationTime: cmtime(at)];
                            if !ok.as_bool() {
                                return Err(format!("no se pudo escribir el cuadro: {}", writer_error(&writer)));
                            }
                            (job.progress)((at / total).clamp(0.0, 1.0));
                        }
                    }
                }
            }
            if let (false, Some(input), Some(format), Some(stream)) = (audio_done, audio_in.as_ref(), format.as_ref(), audio.as_mut()) {
                if ready(input) {
                    progress = true;
                    match stream.next_block(block)? {
                        None => {
                            let _: () = msg_send![&**input, markAsFinished];
                            audio_done = true;
                        }
                        Some(bytes) => {
                            let n = bytes.len() / FRAME_BYTES;
                            if n > 0 {
                                let pts = CMTime { value: audio_at as i64, timescale: AUDIO_RATE as i32, flags: CMTimeFlags::Valid, epoch: 0 };
                                let sample = super::record_macos::pcm_sample(&bytes[..n * FRAME_BYTES], n as u64, format.as_ptr(), pts)?;
                                let _released = Released(sample as *const c_void);
                                let ok: Bool = msg_send![&**input, appendSampleBuffer: &*sample];
                                if !ok.as_bool() {
                                    return Err(format!("no se pudo escribir el audio: {}", writer_error(&writer)));
                                }
                                audio_at += n as u64;
                            }
                        }
                    }
                }
            }
            if progress {
                idle = Instant::now();
                continue;
            }
            let status: isize = msg_send![&writer, status];
            if status != WRITER_WRITING || idle.elapsed() > Duration::from_secs(20) {
                return Err(format!("AVAssetWriter se trabó: {}", writer_error(&writer)));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _: () = msg_send![&reader, cancelReading];
        let _: () = msg_send![&writer, cancelWriting];
        let _ = std::fs::remove_file(out);
        return Err(error);
    }
    let _: () = msg_send![&writer, endSessionAtSourceTime: cmtime(total)];
    finish_writing(&writer)?;
    (job.progress)(1.0);
    Ok(())
}

/// Lo que se arma una sola vez y se pinta cuadro a cuadro: el mismo código para exportar y
/// para la vista previa del editor. Guarda el contexto de Core Image (Metal), los cursores,
/// los textos, el marco y el fondo; `set_scene` los rearma solo si la [`Scene`] cambió.
///
/// **Se usa desde un solo hilo** (el de la interfaz, o el de la exportación): guarda objetos
/// de Core Image y de Core Video que no son `Send`, y por eso él tampoco lo es.
///
/// La vista previa llama a [`render`](Self::render), que entrega el cuadro en NV12 de rango
/// completo (`420f`) para `gpui::surface`, de un grupo de buffers rotativos: GPUI puede seguir
/// leyendo el anterior mientras se escribe el siguiente. La exportación usa [`compose_buffer`]
/// y [`draw`] para escribir en BGRA directo en el grupo del escritor.
///
/// Color: GPUI decodifica el NV12 con la matriz BT.601 de rango completo (`ycbcrToRGBTransform`
/// de `shaders.metal`: R = Y + 1,402·Cr, B = Y + 1,772·Cb, con el Y y el CbCr de 0 a 255 sin
/// recortar). `CIContext::render:toCVPixelBuffer:` a un búfer `420f` escribe justo esa matriz
/// (comprobado con rojo, verde, azul, blanco, negro y gris: dentro de ±1 del valor exacto), así que
/// el NV12 sale directo, en una pasada, y se ve igual que el BGRA de la exportación. La ruta por
/// BGRA + `VTPixelTransferSession` se probó y se descartó: convertía con BT.709 (el rojo salía
/// con Y 54 en vez de 76) aunque el destino llevara la etiqueta BT.601.
pub struct Composer {
    context: Retained<AnyObject>,
    color_space: *mut ColorSpace,
    sprites: Option<Sprites>,
    scene: Option<Scene>,
    stage: Option<Stage>,
    texts: TextCache,
    /// La salida de la vista previa (NV12) y la de `render_bgra`.
    nv12: Option<OutPool>,
    bgra: Option<OutPool>,
}

/// Lo que depende de las medidas y del marco: dónde va el video, la máscara de las esquinas, el
/// fondo con su sombra y el negro de detrás de todo.
struct Stage {
    rect: [f64; 4],
    frame_look: Option<FrameParts>,
    background: Retained<AnyObject>,
}

/// Cuántos buffers de la vista previa se arman de entrada (y rotan).
const PREVIEW_BUFFERS: i32 = 3;

impl Composer {
    pub fn new() -> Result<Self, String> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let none: *const AnyObject = std::ptr::null();
            let context: Retained<AnyObject> = msg_send![class(c"CIContext")?, contextWithOptions: none];
            let color_space = CGColorSpaceCreateDeviceRGB();
            Ok(Self {
                context,
                color_space,
                sprites: Sprites::new().ok(),
                scene: None,
                stage: None,
                texts: TextCache::new(),
                nv12: None,
                bgra: None,
            })
        })
    }

    /// La escena que se pinta de ahora en adelante. Rearma el marco, el fondo y los textos solo si
    /// cambiaron (los textos ya dibujados que siguen iguales se conservan); con la misma escena no
    /// hace nada.
    pub fn set_scene(&mut self, scene: &Scene) -> Result<(), String> {
        if self.scene.as_ref() == Some(scene) {
            return Ok(());
        }
        let valid = |v: f64| v.is_finite() && v >= 1.0;
        if scene.out.0 < 2 || scene.out.1 < 2 || !valid(scene.src.0) || !valid(scene.src.1) {
            return Err(format!("escena inválida: video {:?}, cuadro {:?}", scene.src, scene.out));
        }
        let (out_w, out_h) = (scene.out.0 as f64, scene.out.1 as f64);
        let same_geometry = self.stage.is_some()
            && self.scene.as_ref().is_some_and(|old| old.src == scene.src && old.out == scene.out && old.frame == scene.frame);
        let resized = self.scene.as_ref().is_none_or(|old| old.out != scene.out);
        let stage = if same_geometry {
            None
        } else {
            Some(objc2::rc::autoreleasepool(|_| unsafe { build_stage(scene) })?)
        };
        if let Some(stage) = stage {
            self.stage = Some(stage);
        }
        self.texts.set(&scene.texts, (out_w, out_h));
        if resized {
            self.nv12 = None;
            self.bgra = None;
        }
        self.scene = Some(scene.clone());
        Ok(())
    }

    /// El cuadro `source` (lo que entrega `VideoPlayer::frame()`) compuesto según `decor`: NV12 de
    /// rango completo (`420f`), con IOSurface y compatible con Metal, de las medidas de
    /// `scene.out`, listo para `gpui::surface`. Sale de un grupo de buffers rotativos.
    pub fn render(&mut self, source: &CVPixelBuffer, decor: &Decor) -> Result<CVPixelBuffer, String> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let image = self.compose_buffer(source.as_concrete_TypeRef() as *mut CVBuffer, decor)?;
            self.render_nv12(&image)
        })
    }

    /// Lo mismo que [`render`](Self::render), en BGRA (lo que escribe la exportación): sirve para
    /// comparar y para sacar un cuadro como imagen.
    pub fn render_bgra(&mut self, source: &CVPixelBuffer, decor: &Decor) -> Result<CVPixelBuffer, String> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let image = self.compose_buffer(source.as_concrete_TypeRef() as *mut CVBuffer, decor)?;
            let size = self.scene.as_ref().ok_or("sin escena")?.out;
            if self.bgra.as_ref().is_none_or(|pool| pool.size != size) {
                self.bgra = Some(OutPool::new(size, BGRA, 1)?);
            }
            let target = self.bgra.as_ref().ok_or("sin grupo")?.get()?;
            self.draw(&image, target.as_concrete_TypeRef() as *mut CVBuffer);
            Ok(target)
        })
    }

    /// La imagen de Core Image del cuadro `pixels` ya compuesta (aún sin dibujar en ningún búfer).
    pub(crate) unsafe fn compose_buffer(&mut self, pixels: *mut CVBuffer, decor: &Decor) -> Result<Retained<AnyObject>, String> {
        let frame: Option<Retained<AnyObject>> = msg_send![class(c"CIImage")?, imageWithCVPixelBuffer: pixels];
        let frame = frame.ok_or("Core Image no leyó el cuadro")?;
        self.compose(&frame, decor)
    }

    /// Dibuja `image` (de [`compose_buffer`](Self::compose_buffer)) en `target`, un búfer de las
    /// medidas de la escena (BGRA o lo que Core Image sepa escribir).
    pub(crate) unsafe fn draw(&self, image: &AnyObject, target: *mut CVBuffer) {
        let Some(scene) = self.scene.as_ref() else { return };
        let _: () = msg_send![
            &self.context,
            render: image,
            toCVPixelBuffer: target,
            bounds: CGRect::new(0.0, 0.0, scene.out.0 as f64, scene.out.1 as f64),
            colorSpace: self.color_space
        ];
    }

    /// `image` en un búfer NV12 de rango completo del grupo de la vista previa.
    unsafe fn render_nv12(&mut self, image: &AnyObject) -> Result<CVPixelBuffer, String> {
        let size = self.scene.as_ref().ok_or("sin escena")?.out;
        if self.nv12.as_ref().is_none_or(|pool| pool.size != size) {
            self.nv12 = Some(OutPool::new(size, NV12_FULL, PREVIEW_BUFFERS)?);
        }
        let target = self.nv12.as_ref().ok_or("sin grupo")?.get()?;
        let target_ptr = target.as_concrete_TypeRef() as *mut CVBuffer;
        // Core Image escribe BT.601: se deja dicho en el búfer.
        tag_bt601(target_ptr);
        self.draw(image, target_ptr);
        Ok(target)
    }

    /// Un cuadro: las zonas tapadas sobre el original, la cámara sobre el video (en el
    /// rectángulo del video en el cuadro), el cursor encima y los textos al final (los textos no se
    /// acercan con el zoom; el cursor sí, como el video).
    unsafe fn compose(&mut self, frame: &AnyObject, decor: &Decor) -> Result<Retained<AnyObject>, String> {
        let scene = self.scene.as_ref().ok_or("sin escena")?;
        let stage = self.stage.as_ref().ok_or("sin escena")?;
        let (src_w, src_h) = scene.src;
        let (out_w, out_h) = (scene.out.0 as f64, scene.out.1 as f64);
        let look = scene.look;
        // El video se compone en su rectángulo (con la proporción del original) y al final se
        // pone en el cuadro.
        let [rx, ry, content_w, content_h] = stage.rect;

        // Zonas tapadas: sobre el cuadro original, antes de la cámara.
        let mut covered: Option<Retained<AnyObject>> = None;
        for r in &decor.redactions {
            if r.alpha <= 0.003 {
                continue;
            }
            let [x, y, w, h] = r.rect;
            let (x0, y0) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            let (x1, y1) = ((x + w).clamp(0.0, 1.0), (y + h).clamp(0.0, 1.0));
            if (x1 - x0) * src_w < 1.0 || (y1 - y0) * src_h < 1.0 {
                continue;
            }
            // Core Image cuenta desde abajo.
            let area = CGRect::new(x0 * src_w, (1.0 - y1) * src_h, (x1 - x0) * src_w, (y1 - y0) * src_h);
            let base: &AnyObject = covered.as_deref().unwrap_or(frame);
            let hidden = if r.pixelate {
                let block = (src_h * 0.012).max(6.0);
                unsafe {
                    filter(c"CIPixellate", &[("inputImage", base), ("inputCenter", &*vector_xy(area.origin.x, area.origin.y)?), ("inputScale", &*number(block)?)])?
                }
            } else {
                let radius = (src_h * 0.015).max(8.0);
                let clamped: Retained<AnyObject> = unsafe { msg_send![base, imageByClampingToExtent] };
                unsafe { filter(c"CIGaussianBlur", &[("inputImage", &*clamped), ("inputRadius", &*number(radius)?)])? }
            };
            let cropped: Retained<AnyObject> = unsafe { msg_send![&hidden, imageByCroppingToRect: area] };
            let faded = unsafe { fade(&cropped, r.alpha)? };
            covered = Some(unsafe { msg_send![&faded, imageByCompositingOverImage: base] });
        }
        let frame: &AnyObject = covered.as_deref().unwrap_or(frame);

        let [cx, cy, cw, ch] = decor.crop;
        let cw = cw.clamp(0.05, 1.0);
        let ch = ch.clamp(0.05, 1.0);
        // Core Image cuenta desde abajo a la izquierda.
        let left = cx * src_w;
        let bottom = (1.0 - cy - ch) * src_h;
        let (sx, sy) = (content_w / (cw * src_w), content_h / (ch * src_h));
        let place = |x: f64, y: f64| -> (f64, f64) { ((x - left) * sx, (y - bottom) * sy) };
        let moved: Retained<AnyObject> = unsafe { msg_send![frame, imageByApplyingTransform: Affine::translate(-left, -bottom)] };
        let mut image: Retained<AnyObject> = unsafe { msg_send![&moved, imageByApplyingTransform: Affine::scale_xy(sx, sy)] };

        // El desenfoque de movimiento: solo el video (el cursor va nítido, como en Screen
        // Studio), suave y solo si la cámara se movió lo bastante para notarse: que el
        // texto se siga leyendo en un paneo normal.
        if let Some(plan) = decor.blur.and_then(|blur| blur_plan(look.blur_amount, (content_w, content_h), blur)) {
            let full = CGRect::new(0.0, 0.0, content_w, content_h);
            let clamped: Retained<AnyObject> = unsafe { msg_send![&image, imageByClampingToExtent] };
            let mut blurred = clamped;
            if let Some((radius, angle)) = plan.pan {
                blurred = unsafe { filter(c"CIMotionBlur", &[("inputImage", &*blurred), ("inputRadius", &*number(radius)?), ("inputAngle", &*number(angle)?)])? };
            }
            if let Some(amount) = plan.zoom {
                blurred = unsafe {
                    filter(c"CIZoomBlur", &[("inputImage", &*blurred), ("inputCenter", &*vector_xy(content_w / 2.0, content_h / 2.0)?), ("inputAmount", &*number(amount)?)])?
                };
            }
            image = unsafe { msg_send![&blurred, imageByCroppingToRect: full] };
        }

        if let (Some((x, y, size)), Some(sprites)) = (decor.cursor, self.sprites.as_ref()) {
            if decor.cursor_alpha > 0.003 {
                let (sprite, (_, h), (hot_x, hot_y)) = sprites.pick(look.cursor_style, decor.cursor_shape);
                // El alto pedido (el de la flecha), en píxeles del original, y con el zoom como el resto.
                let k = scene.cursor_px * size / sprites.reference_height();
                let (px_, py_) = place(x * src_w, (1.0 - y) * src_h);
                let scale = k * sx;
                // La punta (el punto caliente, desde arriba a la izquierda) va en el cursor.
                let tx = px_ - hot_x * scale;
                let ty = py_ - (h - hot_y) * scale;
                let scaled: Retained<AnyObject> = unsafe { msg_send![&**sprite, imageByApplyingTransform: Affine::scale(scale)] };
                let placed: Retained<AnyObject> = unsafe { msg_send![&scaled, imageByApplyingTransform: Affine::translate(tx, ty)] };
                let placed = unsafe { fade(&placed, decor.cursor_alpha)? };
                image = unsafe { msg_send![&placed, imageByCompositingOverImage: &*image] };
            }
        }

        // Los clics, bajo el cursor no: encima del video, con el zoom.
        for &(x, y, p) in &decor.ripples {
            let (rx_, ry_) = place(x * src_w, (1.0 - y) * src_h);
            let Some(draw) = click_draw(look.click.kind, look.click.size, p, src_w * sx) else { continue };
            let effect = unsafe { click_image(rx_, ry_, draw, look.click.color)? };
            image = unsafe { msg_send![&effect, imageByCompositingOverImage: &*image] };
        }

        // El video entero en su rectángulo del cuadro; con marco, con esquinas redondeadas
        // sobre el fondo y su sombra.
        let content: Retained<AnyObject> = unsafe { msg_send![&image, imageByCroppingToRect: CGRect::new(0.0, 0.0, content_w, content_h)] };
        let placed: Retained<AnyObject> = unsafe { msg_send![&content, imageByApplyingTransform: Affine::translate(rx, ry)] };
        image = if let Some(parts) = stage.frame_look.as_ref() {
            let empty: Retained<AnyObject> = unsafe { msg_send![class(c"CIImage")?, emptyImage] };
            let rounded = unsafe {
                filter(c"CIBlendWithMask", &[("inputImage", &*placed), ("inputBackgroundImage", &*empty), ("inputMaskImage", &*parts.mask)])?
            };
            unsafe { msg_send![&rounded, imageByCompositingOverImage: &*parts.base] }
        } else {
            placed
        };

        for (index, alpha) in &decor.texts {
            let Some(text) = (unsafe { self.texts.get(*index) }) else { continue };
            let faded = unsafe { fade(&text, *alpha)? };
            image = unsafe { msg_send![&faded, imageByCompositingOverImage: &*image] };
        }
        let image: Retained<AnyObject> = unsafe { msg_send![&image, imageByCompositingOverImage: &*stage.background] };
        Ok(unsafe { msg_send![&image, imageByCroppingToRect: CGRect::new(0.0, 0.0, out_w, out_h)] })
    }
}

impl Drop for Composer {
    fn drop(&mut self) {
        // SAFETY: se creó con +1 en `new` y se suelta una vez.
        unsafe { CFRelease(self.color_space as *const c_void) };
    }
}

/// Dónde va el video y su marco, para una escena.
unsafe fn build_stage(scene: &Scene) -> Result<Stage, String> {
    let (out_w, out_h) = (scene.out.0 as f64, scene.out.1 as f64);
    // Dónde va el video en el cuadro: sin marco, sin margen.
    let margin = scene.frame.as_ref().map_or(0.0, |f| frame_margin(f.padding, out_w, out_h));
    let rect = content_rect((out_w, out_h), scene.src, margin);
    let frame_look = scene.frame.as_ref().map(|spec| frame_parts(spec, out_w, out_h, rect)).transpose()?;
    let background: Retained<AnyObject> = {
        let black: Retained<AnyObject> = unsafe { msg_send![class(c"CIColor")?, colorWithRed: 0.0f64, green: 0.0f64, blue: 0.0f64, alpha: 1.0f64] };
        let image: Retained<AnyObject> = unsafe { msg_send![class(c"CIImage")?, imageWithColor: &*black] };
        unsafe { msg_send![&image, imageByCroppingToRect: CGRect::new(0.0, 0.0, out_w, out_h)] }
    };
    Ok(Stage { rect, frame_look, background })
}

/// Un grupo de buffers de salida con IOSurface y compatibles con Metal, de un formato y
/// medidas fijos. Un buffer vuelve al grupo cuando nadie más lo retiene (ni GPUI ni la GPU), así
/// que nunca se escribe sobre uno que todavía se está leyendo.
struct OutPool {
    pool: *mut PixelPool,
    size: (u32, u32),
}

impl OutPool {
    /// `keep`: cuántos buffers se arman de entrada.
    unsafe fn new(size: (u32, u32), format: u32, keep: i32) -> Result<Self, String> {
        let empty: CFDictionary<CFString, CFType> = dict(&[]);
        let attrs = dict(&[
            (key(kCVPixelBufferPixelFormatTypeKey), CFNumber::from(format as i32).as_CFType()),
            (key(kCVPixelBufferWidthKey), CFNumber::from(size.0 as i32).as_CFType()),
            (key(kCVPixelBufferHeightKey), CFNumber::from(size.1 as i32).as_CFType()),
            (key(kCVPixelBufferIOSurfacePropertiesKey), empty.as_CFType()),
            (key(kCVPixelBufferMetalCompatibilityKey), CFBoolean::true_value().as_CFType()),
        ]);
        let pool_attrs = dict(&[(key(kCVPixelBufferPoolMinimumBufferCountKey), CFNumber::from(keep).as_CFType())]);
        let mut pool: *mut PixelPool = std::ptr::null_mut();
        let status = unsafe { CVPixelBufferPoolCreate(std::ptr::null(), pool_attrs.as_concrete_TypeRef().cast(), attrs.as_concrete_TypeRef().cast(), &mut pool) };
        if status != 0 || pool.is_null() {
            return Err(format!("sin grupo de buffers ({status})"));
        }
        Ok(Self { pool, size })
    }

    /// Un buffer libre.
    unsafe fn get(&self) -> Result<CVPixelBuffer, String> {
        let mut buffer: *mut CVBuffer = std::ptr::null_mut();
        let status = unsafe { CVPixelBufferPoolCreatePixelBuffer(std::ptr::null(), self.pool, &mut buffer) };
        if status != 0 || buffer.is_null() {
            return Err(format!("sin buffer en el grupo ({status})"));
        }
        // SAFETY: llega con +1; el `CVPixelBuffer` lo suelta.
        Ok(unsafe { CVPixelBuffer::wrap_under_create_rule(buffer as *mut c_void as CVPixelBufferRef) })
    }
}

impl Drop for OutPool {
    fn drop(&mut self) {
        // SAFETY: el grupo se creó con +1 y se suelta una vez; los buffers ya entregados siguen vivos.
        unsafe { CFRelease(self.pool as *const c_void) };
    }
}

/// Marca el búfer como BT.601 (la matriz que usa GPUI y la que escribe Core Image).
unsafe fn tag_bt601(buffer: *mut CVBuffer) {
    unsafe {
        CVBufferSetAttachment(buffer, kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_601_4 as *const c_void, ATTACH_PROPAGATE);
    }
}

/// Lo fijo del marco, armado una vez: la máscara de las esquinas del video y el fondo
/// con la sombra.
struct FrameParts {
    mask: Retained<AnyObject>,
    base: Retained<AnyObject>,
}

fn color(rgb: Rgb, alpha: f64) -> Result<Retained<AnyObject>, String> {
    Ok(unsafe { msg_send![class(c"CIColor")?, colorWithRed: rgb[0], green: rgb[1], blue: rgb[2], alpha: alpha] })
}

/// Un filtro de Core Image con sus entradas; devuelve su imagen.
unsafe fn filter(name: &std::ffi::CStr, inputs: &[(&str, &AnyObject)]) -> Result<Retained<AnyObject>, String> {
    let name = name.to_string_lossy();
    let f: Option<Retained<AnyObject>> = unsafe { msg_send![class(c"CIFilter")?, filterWithName: &*NSString::from_str(&name)] };
    let f = f.ok_or_else(|| format!("sin el filtro {name}"))?;
    for (key, value) in inputs {
        let _: () = unsafe { msg_send![&f, setValue: *value, forKey: &*NSString::from_str(key)] };
    }
    let out: Option<Retained<AnyObject>> = unsafe { msg_send![&f, outputImage] };
    out.ok_or_else(|| format!("{name} no dio imagen"))
}

fn number(value: f64) -> Result<Retained<AnyObject>, String> {
    Ok(unsafe { msg_send![class(c"NSNumber")?, numberWithDouble: value] })
}

fn vector_rect(rect: CGRect) -> Result<Retained<AnyObject>, String> {
    Ok(unsafe { msg_send![class(c"CIVector")?, vectorWithCGRect: rect] })
}

fn vector_xy(x: f64, y: f64) -> Result<Retained<AnyObject>, String> {
    Ok(unsafe { msg_send![class(c"CIVector")?, vectorWithX: x, Y: y] })
}

/// Un rectángulo redondeado de `color`.
unsafe fn rounded_rect(rect: CGRect, radius: f64, color: &AnyObject) -> Result<Retained<AnyObject>, String> {
    unsafe { filter(c"CIRoundedRectangleGenerator", &[("inputExtent", &*vector_rect(rect)?), ("inputRadius", &*number(radius)?), ("inputColor", color)]) }
}

/// El fondo de imagen: llena el cuadro (recortada a su proporción, centrada). Si el archivo
/// falta o no se lee, un gris oscuro.
unsafe fn backdrop_image(path: &Path, out_w: f64, out_h: f64) -> Result<Retained<AnyObject>, String> {
    let loaded: Option<Retained<AnyObject>> = match file_url(path) {
        Ok(url) if path.is_file() => unsafe { msg_send![class(c"CIImage")?, imageWithContentsOfURL: &*url] },
        _ => None,
    };
    let extent: CGRect = match &loaded {
        Some(image) => unsafe { msg_send![&**image, extent] },
        None => CGRect::default(),
    };
    let (w, h) = (extent.size.width, extent.size.height);
    match loaded {
        Some(image) if w >= 1.0 && h >= 1.0 && w.is_finite() && h.is_finite() => {
            let scale = (out_w / w).max(out_h / h);
            let moved: Retained<AnyObject> = unsafe { msg_send![&image, imageByApplyingTransform: Affine::translate(-extent.origin.x, -extent.origin.y)] };
            let scaled: Retained<AnyObject> = unsafe { msg_send![&moved, imageByApplyingTransform: Affine::scale(scale)] };
            Ok(unsafe { msg_send![&scaled, imageByApplyingTransform: Affine::translate((out_w - w * scale) / 2.0, (out_h - h * scale) / 2.0)] })
        }
        _ => {
            tracing::warn!(path = %path.display(), "exportar: no se pudo leer la imagen de fondo, va un gris oscuro");
            Ok(unsafe { msg_send![class(c"CIImage")?, imageWithColor: &*color([0.1, 0.1, 0.11], 1.0)?] })
        }
    }
}

/// El marco para el video que va en `rect` (`[x, y, ancho, alto]` del cuadro de salida).
fn frame_parts(spec: &FrameSpec, out_w: f64, out_h: f64, rect: [f64; 4]) -> Result<FrameParts, String> {
    let rect = CGRect::new(rect[0], rect[1], rect[2], rect[3]);
    let radius = spec.radius.clamp(0.0, 0.1) * out_h;
    unsafe {
        let white = color([1.0, 1.0, 1.0], 1.0)?;
        let mask = rounded_rect(rect, radius, &*white)?;
        let full = CGRect::new(0.0, 0.0, out_w, out_h);
        let backdrop: Retained<AnyObject> = match &spec.backdrop {
            Backdrop::Solid(rgb) => {
                let image: Retained<AnyObject> = msg_send![class(c"CIImage")?, imageWithColor: &*color(*rgb, 1.0)?];
                image
            }
            Backdrop::Gradient(a, b) => filter(
                c"CILinearGradient",
                &[
                    ("inputPoint0", &*vector_xy(0.0, out_h)?),
                    ("inputPoint1", &*vector_xy(out_w, 0.0)?),
                    ("inputColor0", &*color(*a, 1.0)?),
                    ("inputColor1", &*color(*b, 1.0)?),
                ],
            )?,
            Backdrop::Image(path) => backdrop_image(path, out_w, out_h)?,
        };
        let mut base: Retained<AnyObject> = msg_send![&backdrop, imageByCroppingToRect: full];
        if let Some((alpha, blur, drop)) = shadow_params(spec.shadow, out_h) {
            let shade = rounded_rect(rect, radius, &*color([0.0, 0.0, 0.0], alpha)?)?;
            let blurred = filter(c"CIGaussianBlur", &[("inputImage", &*shade), ("inputRadius", &*number(blur)?)])?;
            let lowered: Retained<AnyObject> = msg_send![&blurred, imageByApplyingTransform: Affine::translate(0.0, -drop)];
            let composed: Retained<AnyObject> = msg_send![&lowered, imageByCompositingOverImage: &*base];
            base = msg_send![&composed, imageByCroppingToRect: full];
        }
        Ok(FrameParts { mask, base })
    }
}

/// El efecto de un clic, de `rgb`, centrado en `(x, y)` del cuadro de salida.
unsafe fn click_image(x: f64, y: f64, draw: ClickDraw, rgb: Rgb) -> Result<Retained<AnyObject>, String> {
    // Un disco de `color` opaco hasta `solid` que se desvanece hasta `radius`.
    let disk = |solid: f64, radius: f64, alpha: f64| unsafe {
        filter(
            c"CIRadialGradient",
            &[
                ("inputCenter", &*vector_xy(x, y)?),
                ("inputRadius0", &*number(solid)?),
                ("inputRadius1", &*number(radius)?),
                ("inputColor0", &*color(rgb, alpha)?),
                ("inputColor1", &*color(rgb, 0.0)?),
            ],
        )
    };
    match draw {
        ClickDraw::Disk { radius, solid, alpha } => disk(solid, radius, alpha),
        ClickDraw::Ring { radius, thickness, alpha } => {
            // El borde se difumina 1,5 px: un anillo fino sin dientes.
            const SOFT: f64 = 1.5;
            let outer = disk(radius, radius + SOFT, alpha)?;
            let inner = unsafe {
                filter(
                    c"CIRadialGradient",
                    &[
                        ("inputCenter", &*vector_xy(x, y)?),
                        ("inputRadius0", &*number((radius - thickness).max(0.0))?),
                        ("inputRadius1", &*number((radius - thickness).max(0.0) + SOFT)?),
                        ("inputColor0", &*color([0.0; 3], 1.0)?),
                        ("inputColor1", &*color([0.0; 3], 0.0)?),
                    ],
                )?
            };
            let ring = unsafe { filter(c"CISourceOutCompositing", &[("inputImage", &*outer), ("inputBackgroundImage", &*inner)])? };
            let reach = radius + SOFT + 2.0;
            Ok(unsafe { msg_send![&ring, imageByCroppingToRect: CGRect::new(x - reach, y - reach, reach * 2.0, reach * 2.0)] })
        }
    }
}

/// `image` con opacidad `alpha`.
unsafe fn fade(image: &AnyObject, alpha: f64) -> Result<Retained<AnyObject>, String> {
    if alpha >= 0.999 {
        return Ok(unsafe { Retained::retain(image as *const AnyObject as *mut AnyObject) }.ok_or("sin imagen")?);
    }
    let vector: Retained<AnyObject> = unsafe { msg_send![class(c"CIVector")?, vectorWithX: 0.0f64, Y: 0.0f64, Z: 0.0f64, W: alpha.clamp(0.0, 1.0)] };
    let name = NSString::from_str("inputAVector");
    let params: Retained<AnyObject> = unsafe { msg_send![class(c"NSDictionary")?, dictionaryWithObject: &*vector, forKey: &*name] };
    Ok(unsafe { msg_send![image, imageByApplyingFilter: &*NSString::from_str("CIColorMatrix"), withInputParameters: &*params] })
}

/// La flecha del sistema (`NSCursor.arrowCursor`): su imagen, medidas en puntos y punto
/// caliente (desde arriba a la izquierda).
unsafe fn cursor_image() -> Result<Sprite, String> {
    let cursor: Option<Retained<AnyObject>> = unsafe { msg_send![class(c"NSCursor")?, arrowCursor] };
    // Sin AppKit andando (o sin imagen): la flecha propia.
    let Some(cursor) = cursor else { return drawn_arrow(Tone::Light) };
    let image: Option<Retained<AnyObject>> = unsafe { msg_send![&cursor, image] };
    let Some(image) = image else { return drawn_arrow(Tone::Light) };
    let size: CGSize = unsafe { msg_send![&image, size] };
    let hot: CGPoint = unsafe { msg_send![&cursor, hotSpot] };
    let rect: *mut CGRect = std::ptr::null_mut();
    let none: *const AnyObject = std::ptr::null();
    let cg: *mut CGImageRaw = unsafe { msg_send![&image, CGImageForProposedRect: rect, context: none, hints: none] };
    if cg.is_null() {
        return drawn_arrow(Tone::Light);
    }
    let ci: Retained<AnyObject> = unsafe { msg_send![class(c"CIImage")?, imageWithCGImage: cg] };
    // La imagen viene a la escala de la pantalla: se lleva a puntos.
    let extent: CGRect = unsafe { msg_send![&ci, extent] };
    let k = size.height / extent.size.height.max(1.0);
    let ci: Retained<AnyObject> = unsafe { msg_send![&ci, imageByApplyingTransform: Affine::scale(k)] };
    Ok((ci, (size.width, size.height), (hot.x, hot.y)))
}

/// Un cursor listo para dibujar: su imagen, medidas en puntos y punto caliente (desde
/// arriba a la izquierda).
type Sprite = (Retained<AnyObject>, (f64, f64), (f64, f64));

/// De qué color se dibuja un cursor a mano: blanco con borde negro, o negro con borde blanco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Light,
    Dark,
}

impl Tone {
    /// El tono (0–255) del relleno y del borde.
    fn values(self) -> (u32, u32) {
        match self {
            Tone::Light => (255, 0),
            Tone::Dark => (0, 255),
        }
    }
}

/// Flecha, mano, I-beam y cruz de un mismo tono.
struct Shapes {
    arrow: Sprite,
    hand: Sprite,
    ibeam: Sprite,
    cross: Sprite,
}

impl Shapes {
    fn pick(&self, shape: CursorShape) -> &Sprite {
        match shape {
            CursorShape::Arrow => &self.arrow,
            CursorShape::Hand => &self.hand,
            CursorShape::IBeam => &self.ibeam,
            CursorShape::Cross => &self.cross,
        }
    }
}

/// Los cursores de cada estilo y forma, armados una vez. En `System` la flecha es la del
/// sistema; todo lo demás se dibuja aquí.
struct Sprites {
    system: Shapes,
    dark: Shapes,
    dot: Sprite,
    ring: Sprite,
}

impl Sprites {
    unsafe fn new() -> Result<Self, String> {
        let arrow = unsafe { cursor_image()? };
        // Si alguno no se dibuja, va la flecha.
        let system = Shapes {
            hand: unsafe { drawn_hand(Tone::Light) }.unwrap_or_else(|_| arrow.clone()),
            ibeam: unsafe { drawn_ibeam(Tone::Light) }.unwrap_or_else(|_| arrow.clone()),
            cross: unsafe { drawn_cross(Tone::Light) }.unwrap_or_else(|_| arrow.clone()),
            arrow,
        };
        // La flecha negra se dibuja aquí, al alto de la del sistema.
        let dark_arrow = unsafe { drawn_arrow(Tone::Dark) }.map(|sprite| rescaled(sprite, system.arrow.1 .1 / DRAWN_ARROW_H));
        let dark_arrow = dark_arrow.unwrap_or_else(|_| system.arrow.clone());
        let dark = Shapes {
            hand: unsafe { drawn_hand(Tone::Dark) }.unwrap_or_else(|_| dark_arrow.clone()),
            ibeam: unsafe { drawn_ibeam(Tone::Dark) }.unwrap_or_else(|_| dark_arrow.clone()),
            cross: unsafe { drawn_cross(Tone::Dark) }.unwrap_or_else(|_| dark_arrow.clone()),
            arrow: dark_arrow,
        };
        let dot = unsafe { drawn_dot() }.unwrap_or_else(|_| system.arrow.clone());
        let ring = unsafe { drawn_ring() }.unwrap_or_else(|_| system.arrow.clone());
        Ok(Self { system, dark, dot, ring })
    }

    /// El cursor de `style` para `shape`. `Dot` y `Ring` valen para toda forma menos el I-beam, que
    /// conserva la suya.
    fn pick(&self, style: CursorStyle, shape: CursorShape) -> &Sprite {
        match (style, shape) {
            (CursorStyle::System, _) => self.system.pick(shape),
            (CursorStyle::Dark, _) => self.dark.pick(shape),
            (CursorStyle::Dot | CursorStyle::Ring, CursorShape::IBeam) => &self.system.ibeam,
            (CursorStyle::Dot, _) => &self.dot,
            (CursorStyle::Ring, _) => &self.ring,
            (CursorStyle::Arrow, _) => &self.system.arrow,
        }
    }

    /// El alto en puntos al que se mide `cursor_px`: el de la flecha del sistema.
    fn reference_height(&self) -> f64 {
        self.system.arrow.1 .1.max(1.0)
    }
}

/// Un cursor ya hecho, a otra escala (la imagen, sus medidas y su punto caliente).
fn rescaled((image, (w, h), (hx, hy)): Sprite, k: f64) -> Sprite {
    let scaled: Retained<AnyObject> = unsafe { msg_send![&image, imageByApplyingTransform: Affine::scale(k)] };
    (scaled, (w * k, h * k), (hx * k, hy * k))
}

/// El alto en puntos de la flecha dibujada aquí.
const DRAWN_ARROW_H: f64 = 21.0;

/// Un cursor dibujado a mano de `w × h` puntos, a 4×: `fill(x, y)` dice si el punto
/// (en puntos, desde arriba a la izquierda) va en el color de relleno y `border(x, y)` si va en
/// el del borde alrededor (según el `tone`). `hot` es su punta.
unsafe fn raster_sprite(w: usize, h: usize, hot: (f64, f64), tone: Tone, fill: &dyn Fn(f64, f64) -> bool, border: &dyn Fn(f64, f64) -> bool) -> Result<Sprite, String> {
    const K: f64 = 4.0;
    let (fill_tone, border_tone) = tone.values();
    let (pw, ph) = (w * 4, h * 4);
    let mut rgba = vec![0u8; pw * ph * 4];
    for py in 0..ph {
        for px in 0..pw {
            // Muestras 2×2 por píxel: bordes suaves.
            let (mut inner, mut outer) = (0u32, 0u32);
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let (x, y) = ((px as f64 + sx) / K, (py as f64 + sy) / K);
                if fill(x, y) {
                    inner += 1;
                } else if border(x, y) {
                    outer += 1;
                }
            }
            let a = (inner + outer) * 255 / 4;
            let v = (inner * fill_tone + outer * border_tone) / 4;
            let i = (py * pw + px) * 4;
            // RGBA premultiplicado.
            rgba[i..i + 4].copy_from_slice(&[v as u8, v as u8, v as u8, a as u8]);
        }
    }
    let data: Retained<AnyObject> = unsafe { msg_send![class(c"NSData")?, dataWithBytes: rgba.as_ptr() as *const c_void, length: rgba.len()] };
    let space = unsafe { CGColorSpaceCreateDeviceRGB() };
    let _space = Released(space as *const c_void);
    // `kCIFormatRGBA8`.
    let format: i32 = unsafe { RGBA8 };
    let image: Retained<AnyObject> = unsafe {
        msg_send![
            class(c"CIImage")?,
            imageWithBitmapData: &*data,
            bytesPerRow: pw * 4,
            size: CGSize { width: pw as f64, height: ph as f64 },
            format: format,
            colorSpace: space
        ]
    };
    // `imageWithBitmapData` ya pone la primera fila arriba: solo se lleva a puntos.
    let scaled: Retained<AnyObject> = unsafe { msg_send![&image, imageByApplyingTransform: Affine::scale(1.0 / K)] };
    Ok((scaled, (w as f64, h as f64), hot))
}

/// La distancia del punto `(x, y)` al segmento `a`–`b`.
fn segment_distance((x, y): (f64, f64), (ax, ay): (f64, f64), (bx, by): (f64, f64)) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 0.0 { 0.0 } else { (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0) };
    ((x - (ax + t * dx)).powi(2) + (y - (ay + t * dy)).powi(2)).sqrt()
}

/// Si `(x, y)` cae en alguna de las cápsulas `(desde, hasta, radio)` agrandadas en `grow`.
fn in_capsules(capsules: &[((f64, f64), (f64, f64), f64)], x: f64, y: f64, grow: f64) -> bool {
    capsules.iter().any(|&(a, b, r)| segment_distance((x, y), a, b) <= r + grow)
}

/// La flecha de siempre, dibujada aquí: blanca con borde negro, a 4× (medidas en
/// puntos: 12 × 19, con la punta arriba a la izquierda).
unsafe fn drawn_arrow(tone: Tone) -> Result<Sprite, String> {
    // El contorno en puntos, desde la punta.
    let outline: [(f64, f64); 7] = [(1.0, 1.0), (1.0, 17.0), (5.0, 13.2), (7.8, 19.5), (10.2, 18.4), (7.5, 12.3), (12.6, 12.3)];
    let inside = |poly: &[(f64, f64)], x: f64, y: f64| {
        let mut odd = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                odd = !odd;
            }
            j = i;
        }
        odd
    };
    // El borde: el mismo contorno, un poco más grande alrededor de su centro.
    let grow = |(x, y): (f64, f64), by: f64| -> (f64, f64) {
        let (cx, cy) = (5.5, 11.0);
        let (dx, dy) = (x - cx, y - cy);
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        (x + dx / len * by, y + dy / len * by)
    };
    let border: Vec<(f64, f64)> = outline.iter().map(|p| grow(*p, 1.1)).collect();
    unsafe { raster_sprite(14, DRAWN_ARROW_H as usize, (1.0, 1.0), tone, &|x, y| inside(&outline, x, y), &|x, y| inside(&border, x, y)) }
}

/// La mano de los vínculos, dibujada aquí (24 × 23 puntos): el índice arriba, los otros
/// dedos y la palma; la punta del índice es su punto caliente.
unsafe fn drawn_hand(tone: Tone) -> Result<Sprite, String> {
    // Cápsulas (desde, hasta, radio): índice, medio, anular, meñique, palma y pulgar.
    let shape: [((f64, f64), (f64, f64), f64); 6] = [
        ((8.0, 3.8), (8.0, 13.0), 1.9),
        ((11.8, 8.0), (11.8, 13.0), 1.9),
        ((15.6, 9.0), (15.6, 13.0), 1.9),
        ((19.4, 10.5), (19.4, 13.0), 1.9),
        ((9.0, 16.5), (17.5, 16.5), 4.5),
        ((6.0, 15.5), (3.2, 12.0), 1.8),
    ];
    unsafe { raster_sprite(24, 23, (8.0, 2.0), tone, &|x, y| in_capsules(&shape, x, y, 0.0), &|x, y| in_capsules(&shape, x, y, 1.0)) }
}

/// El cursor de texto, dibujado aquí (12 × 22 puntos): una barra con remates arriba y
/// abajo; el punto caliente es su centro.
unsafe fn drawn_ibeam(tone: Tone) -> Result<Sprite, String> {
    // Cápsulas finas: la barra y los dos remates.
    let shape: [((f64, f64), (f64, f64), f64); 3] = [((6.0, 3.0), (6.0, 19.0), 1.0), ((3.2, 2.6), (8.8, 2.6), 0.9), ((3.2, 19.4), (8.8, 19.4), 0.9)];
    unsafe { raster_sprite(12, 22, (6.0, 11.0), tone, &|x, y| in_capsules(&shape, x, y, 0.0), &|x, y| in_capsules(&shape, x, y, 1.0)) }
}

/// La cruz de precisión, dibujada aquí (22 × 22 puntos); el punto caliente es su centro.
unsafe fn drawn_cross(tone: Tone) -> Result<Sprite, String> {
    let shape: [((f64, f64), (f64, f64), f64); 2] = [((3.0, 11.0), (19.0, 11.0), 1.0), ((11.0, 3.0), (11.0, 19.0), 1.0)];
    unsafe { raster_sprite(22, 22, (11.0, 11.0), tone, &|x, y| in_capsules(&shape, x, y, 0.0), &|x, y| in_capsules(&shape, x, y, 1.0)) }
}

/// Si `(x, y)` está a una distancia de `(cx, cy)` entre `from` y `to`.
fn in_annulus((x, y): (f64, f64), (cx, cy): (f64, f64), from: f64, to: f64) -> bool {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    d >= from && d <= to
}

/// El punto: un disco blanco con borde oscuro (14 × 14 puntos); el punto caliente es su centro.
unsafe fn drawn_dot() -> Result<Sprite, String> {
    let c = (7.0, 7.0);
    unsafe { raster_sprite(14, 14, c, Tone::Light, &|x, y| in_annulus((x, y), c, 0.0, 5.0), &|x, y| in_annulus((x, y), c, 0.0, 6.2)) }
}

/// El anillo: blanco, de centro vacío, con un borde oscuro fino por dentro y por fuera (20 × 20
/// puntos); el punto caliente es su centro.
unsafe fn drawn_ring() -> Result<Sprite, String> {
    let c = (10.0, 10.0);
    unsafe { raster_sprite(20, 20, c, Tone::Light, &|x, y| in_annulus((x, y), c, 5.6, 8.0), &|x, y| in_annulus((x, y), c, 4.8, 8.9)) }
}

#[link(name = "CoreImage", kind = "framework")]
extern "C" {
    #[link_name = "kCIFormatRGBA8"]
    static RGBA8: i32;
}

/// `count` miniaturas repartidas en el video, de `height` píxeles de alto (para la línea
/// de tiempo).
pub fn thumbnails(source: &Path, count: usize, height: u32) -> Result<Vec<Frame>, String> {
    objc2::rc::autoreleasepool(|_| unsafe {
        let none: *const AnyObject = std::ptr::null();
        let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*file_url(source)?, options: none];
        let duration = seconds(msg_send![&asset, duration]);
        let (_, (w, h)) = video_track(&asset)?;
        let generator: Retained<AnyObject> = msg_send![class(c"AVAssetImageGenerator")?, assetImageGeneratorWithAsset: &*asset];
        let _: () = msg_send![&generator, setAppliesPreferredTrackTransform: Bool::YES];
        let max = CGSize { width: (w / h.max(1.0) * height as f64).round(), height: height as f64 };
        let _: () = msg_send![&generator, setMaximumSize: max];
        let tolerance = cmtime(duration / count.max(1) as f64 / 2.0);
        let _: () = msg_send![&generator, setRequestedTimeToleranceBefore: tolerance];
        let _: () = msg_send![&generator, setRequestedTimeToleranceAfter: tolerance];
        let mut frames = Vec::new();
        for i in 0..count {
            let at = cmtime(duration * (i as f64 + 0.5) / count as f64);
            let actual: *mut CMTime = std::ptr::null_mut();
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let image: *mut CGImageRaw = msg_send![&generator, copyCGImageAtTime: at, actualTime: actual, error: &mut error];
            if image.is_null() {
                continue;
            }
            let _image = Released(image as *const c_void);
            if let Some((iw, ih, bgra)) = bgra_of(image) {
                frames.push(Frame::new(Rect::new(0, 0, iw, ih), bgra));
            }
        }
        Ok(frames)
    })
}

/// Los cuadros del video en los segundos `times` (ya dentro de `0..duración`), de `height` píxeles de
/// alto y con el tiempo exacto pedido (sin la tolerancia de las miniaturas). Para que un agente
/// «vea» el video. Los que no se pudieron leer se saltan.
pub fn frames_at(source: &Path, times: &[f64], height: u32) -> Result<Vec<(f64, Frame)>, String> {
    objc2::rc::autoreleasepool(|_| unsafe {
        let none: *const AnyObject = std::ptr::null();
        let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*file_url(source)?, options: none];
        let duration = seconds(msg_send![&asset, duration]);
        let (_, (w, h)) = video_track(&asset)?;
        let generator: Retained<AnyObject> = msg_send![class(c"AVAssetImageGenerator")?, assetImageGeneratorWithAsset: &*asset];
        let _: () = msg_send![&generator, setAppliesPreferredTrackTransform: Bool::YES];
        let max = CGSize { width: (w / h.max(1.0) * height as f64).round(), height: height as f64 };
        let _: () = msg_send![&generator, setMaximumSize: max];
        let exact = cmtime(0.0);
        let _: () = msg_send![&generator, setRequestedTimeToleranceBefore: exact];
        let _: () = msg_send![&generator, setRequestedTimeToleranceAfter: exact];
        let mut frames = Vec::new();
        for &t in times {
            let at = cmtime(t.clamp(0.0, (duration - 0.05).max(0.0)));
            let actual: *mut CMTime = std::ptr::null_mut();
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let image: *mut CGImageRaw = msg_send![&generator, copyCGImageAtTime: at, actualTime: actual, error: &mut error];
            if image.is_null() {
                continue;
            }
            let _image = Released(image as *const c_void);
            if let Some((iw, ih, bgra)) = bgra_of(image) {
                frames.push((t, Frame::new(Rect::new(0, 0, iw, ih), bgra)));
            }
        }
        Ok(frames)
    })
}

/// `ATIC_TEST_EXPORT=<video>`: exporta de verdad con zoom, cursor, un texto y un corte.
#[cfg(test)]
#[test]
fn exporta_de_verdad() {
    let Ok(source) = std::env::var("ATIC_TEST_EXPORT") else {
        return;
    };
    let out = std::env::temp_dir().join("atic-exporta-prueba.mp4");
    let started = Instant::now();
    let job = Export {
        segments: vec![(0.0, 1.5), (2.5, 4.0)],
        speeds: vec![SpeedSpan { start: 3.0, end: 4.0, rate: 2.0 }],
        texts: vec![TextSpec::plain("Hola desde Atic", TextAt::Top)],
        cursor_px: 40.0,
        frame: Some(FrameSpec { backdrop: Backdrop::Gradient([0.55, 0.18, 0.89], [0.29, 0.0, 0.88]), padding: 0.08, radius: 0.02, shadow: 0.5 }),
        look: LookSpec::default(),
        options: ExportOptions { aspect: Some((9, 16)), ..ExportOptions::default() },
        decor: Box::new(|src, out| {
            let z = if src > 1.0 { 0.5 } else { 1.0 };
            Decor {
                crop: [0.25 * (1.0 - z) * 2.0, 0.25 * (1.0 - z) * 2.0, z, z],
                cursor: Some((0.5, 0.5, 1.0)),
                cursor_shape: if src > 1.0 { CursorShape::Hand } else { CursorShape::Arrow },
                texts: if out < 1.0 { vec![(0, 1.0 - out)] } else { vec![] },
                ripples: vec![(0.5, 0.5, (src % 0.5) * 2.0)],
                blur: Some((0.01, 0.0, 1.0)),
                redactions: vec![Redaction { rect: [0.1, 0.1, 0.3, 0.2], pixelate: src > 1.0, alpha: 1.0 }],
                ..Decor::default()
            }
        }),
        progress: Box::new(|_| {}),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    export(Path::new(&source), &out, job).expect("exporta");
    eprintln!("{} en {:?}", out.display(), started.elapsed());
    let thumbs = thumbnails(Path::new(&source), 4, 60).expect("miniaturas");
    assert_eq!(thumbs.len(), 4);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }


    // --- El Composer con imágenes de verdad (necesita Core Image y la GPU) ---------------

    /// Un `CVPixelBuffer` BGRA de `w × h` con IOSurface y Metal, pintado con `pixel(x, y)` (RGB 0–255).
    fn bgra_buffer(w: usize, h: usize, pixel: impl Fn(usize, usize) -> [u8; 3]) -> CVPixelBuffer {
        unsafe {
            let empty: CFDictionary<CFString, CFType> = dict(&[]);
            let attrs = dict(&[
                (key(kCVPixelBufferIOSurfacePropertiesKey), empty.as_CFType()),
                (key(kCVPixelBufferMetalCompatibilityKey), CFBoolean::true_value().as_CFType()),
            ]);
            let mut buffer: *mut CVBuffer = std::ptr::null_mut();
            let status = CVPixelBufferCreate(std::ptr::null(), w, h, BGRA, attrs.as_concrete_TypeRef().cast(), &mut buffer);
            assert_eq!(status, 0);
            let buffer = CVPixelBuffer::wrap_under_create_rule(buffer as *mut c_void as CVPixelBufferRef);
            buffer.lock_base_address(0);
            let base = core_video::pixel_buffer::CVPixelBufferGetBaseAddress(buffer.as_concrete_TypeRef()) as *mut u8;
            let stride = buffer.get_bytes_per_row();
            for y in 0..h {
                for x in 0..w {
                    let [r, g, b] = pixel(x, y);
                    base.add(y * stride + x * 4).copy_from_nonoverlapping([b, g, r, 255].as_ptr(), 4);
                }
            }
            buffer.unlock_base_address(0);
            buffer
        }
    }

    /// Los planos de un NV12: Y (`w × h`) y CbCr (`w/2 × h/2`, de a dos bytes).
    fn read_nv12(buffer: &CVPixelBuffer) -> (usize, usize, Vec<u8>, Vec<[u8; 2]>) {
        assert_eq!(buffer.get_pixel_format(), NV12_FULL);
        let (w, h) = (buffer.get_width(), buffer.get_height());
        buffer.lock_base_address(1);
        let mut luma = Vec::with_capacity(w * h);
        let mut chroma = Vec::with_capacity(w * h / 4);
        unsafe {
            let y_base = core_video::pixel_buffer::CVPixelBufferGetBaseAddressOfPlane(buffer.as_concrete_TypeRef(), 0) as *const u8;
            let y_stride = buffer.get_bytes_per_row_of_plane(0);
            for y in 0..h {
                luma.extend_from_slice(std::slice::from_raw_parts(y_base.add(y * y_stride), w));
            }
            let c_base = core_video::pixel_buffer::CVPixelBufferGetBaseAddressOfPlane(buffer.as_concrete_TypeRef(), 1) as *const u8;
            let c_stride = buffer.get_bytes_per_row_of_plane(1);
            for y in 0..h / 2 {
                for x in 0..w / 2 {
                    chroma.push([*c_base.add(y * c_stride + x * 2), *c_base.add(y * c_stride + x * 2 + 1)]);
                }
            }
        }
        buffer.unlock_base_address(1);
        (w, h, luma, chroma)
    }

    /// Un BGRA leído de vuelta como RGB (de arriba abajo).
    fn read_rgb(buffer: &CVPixelBuffer) -> (usize, usize, Vec<[u8; 3]>) {
        assert_eq!(buffer.get_pixel_format(), BGRA);
        let (w, h) = (buffer.get_width(), buffer.get_height());
        buffer.lock_base_address(1);
        let mut out = Vec::with_capacity(w * h);
        unsafe {
            let base = core_video::pixel_buffer::CVPixelBufferGetBaseAddress(buffer.as_concrete_TypeRef()) as *const u8;
            let stride = buffer.get_bytes_per_row();
            for y in 0..h {
                for x in 0..w {
                    let p = base.add(y * stride + x * 4);
                    out.push([*p.add(2), *p.add(1), *p]);
                }
            }
        }
        buffer.unlock_base_address(1);
        (w, h, out)
    }

    /// BT.601 de rango completo (lo que decodifica GPUI): Y, Cb y Cr de un RGB 0–255.
    fn bt601_full([r, g, b]: [u8; 3]) -> [f64; 3] {
        let (r, g, b) = (r as f64, g as f64, b as f64);
        [0.299 * r + 0.587 * g + 0.114 * b, 128.0 - 0.168736 * r - 0.331264 * g + 0.5 * b, 128.0 + 0.5 * r - 0.418688 * g - 0.081312 * b]
    }

    const STRIPES: [(&str, [u8; 3]); 6] = [
        ("rojo", [255, 0, 0]),
        ("verde", [0, 255, 0]),
        ("azul", [0, 0, 255]),
        ("blanco", [255, 255, 255]),
        ("negro", [0, 0, 0]),
        ("gris", [128, 128, 128]),
    ];

    /// La IOSurface del búfer (0 si no tiene); sin retener, solo para comparar.
    fn io_surface_of(buffer: &CVPixelBuffer) -> usize {
        unsafe { core_video::pixel_buffer_io_surface::CVPixelBufferGetIOSurface(buffer.as_concrete_TypeRef()) as usize }
    }

    fn stripes_source() -> CVPixelBuffer {
        bgra_buffer(192, 64, |x, _| STRIPES[x / 32].1)
    }

    fn plain_scene() -> Scene {
        Scene { src: (192.0, 64.0), out: (192, 64), frame: None, texts: vec![], cursor_px: 40.0, look: LookSpec::default() }
    }


    fn gray_source(w: usize, h: usize, v: u8) -> CVPixelBuffer {
        bgra_buffer(w, h, |_, _| [v, v, v])
    }

    fn scene_of(w: u32, h: u32) -> Scene {
        Scene { src: (w as f64, h as f64), out: (w, h), frame: None, texts: vec![], cursor_px: 40.0, look: LookSpec::default() }
    }

    /// Un cuadro BGRA de `source` con `scene` y `decor`, como RGB de arriba abajo.
    fn rgb_of(scene: &Scene, source: &CVPixelBuffer, decor: &Decor) -> Vec<[u8; 3]> {
        let mut composer = Composer::new().unwrap();
        composer.set_scene(scene).unwrap();
        read_rgb(&composer.render_bgra(source, decor).unwrap()).2
    }

    fn changed(a: &[[u8; 3]], b: &[[u8; 3]]) -> Vec<usize> {
        (0..a.len()).filter(|&i| a[i] != b[i]).collect()
    }

    /// Las cajas (x0, y0, x1, y1) de los píxeles en `idx` de una imagen de ancho `w`.
    fn bounds(idx: &[usize], w: usize) -> (usize, usize, usize, usize) {
        let xs = idx.iter().map(|i| i % w);
        let ys = idx.iter().map(|i| i / w);
        (xs.clone().min().unwrap(), ys.clone().min().unwrap(), xs.max().unwrap(), ys.max().unwrap())
    }

    #[test]
    fn el_nv12_de_la_vista_previa_calza_con_bt601_de_rango_completo() {
        let source = stripes_source();
        let mut composer = Composer::new().unwrap();
        composer.set_scene(&plain_scene()).unwrap();
        let nv12 = composer.render(&source, &Decor::default()).unwrap();
        // Lo que lee GPUI: 420f, con IOSurface y del tamaño de la escena.
        assert_eq!((nv12.get_width(), nv12.get_height()), (192, 64));
        assert_eq!(nv12.get_pixel_format(), NV12_FULL);
        assert!(io_surface_of(&nv12) != 0, "sin IOSurface");
        let (w, _, luma, chroma) = read_nv12(&nv12);
        // El BGRA de la exportación es el mismo cuadro: los colores se ven igual.
        let (bw, _, rgb) = read_rgb(&composer.render_bgra(&source, &Decor::default()).unwrap());
        for (i, (label, want)) in STRIPES.iter().enumerate() {
            let (x, y) = (i * 32 + 16, 32);
            let got = [luma[y * w + x] as f64, chroma[(y / 2) * (w / 2) + x / 2][0] as f64, chroma[(y / 2) * (w / 2) + x / 2][1] as f64];
            let expected = bt601_full(*want);
            for c in 0..3 {
                assert!((got[c] - expected[c]).abs() <= 3.0, "{label}: plano {c} da {got:?}, esperado {expected:?}");
            }
            // Y desde el BGRA que salió de la misma escena.
            let from_bgra = bt601_full(rgb[y * bw + x]);
            for c in 0..3 {
                assert!((got[c] - from_bgra[c]).abs() <= 3.0, "{label}: NV12 {got:?} y BGRA {from_bgra:?} no coinciden");
            }
        }
        // Sin corrimiento de brillo en conjunto: el Y medio es el esperado.
        let mean: f64 = luma.iter().map(|&v| v as f64).sum::<f64>() / luma.len() as f64;
        let want: f64 = STRIPES.iter().map(|(_, c)| bt601_full(*c)[0]).sum::<f64>() / 6.0;
        assert!((mean - want).abs() < 1.5, "Y medio {mean}, esperado {want}");
    }

    #[test]
    fn el_nv12_conserva_colores_intermedios() {
        // Un naranja, un verde azulado y un violeta apagado.
        let colors = [[255u8, 128, 0], [0, 128, 128], [150, 90, 200]];
        let source = bgra_buffer(96, 64, |x, _| colors[x / 32]);
        let mut composer = Composer::new().unwrap();
        composer.set_scene(&Scene { src: (96.0, 64.0), out: (96, 64), ..plain_scene() }).unwrap();
        let (w, _, luma, chroma) = read_nv12(&composer.render(&source, &Decor::default()).unwrap());
        for (i, want) in colors.iter().enumerate() {
            let (x, y) = (i * 32 + 16, 32);
            let got = [luma[y * w + x] as f64, chroma[(y / 2) * (w / 2) + x / 2][0] as f64, chroma[(y / 2) * (w / 2) + x / 2][1] as f64];
            let expected = bt601_full(*want);
            for c in 0..3 {
                assert!((got[c] - expected[c]).abs() <= 3.0, "{want:?}: plano {c} da {got:?}, esperado {expected:?}");
            }
        }
    }

    #[test]
    fn la_salida_rota_entre_buffers_y_sigue_a_la_escena() {
        let source = stripes_source();
        let mut composer = Composer::new().unwrap();
        composer.set_scene(&plain_scene()).unwrap();
        // Mientras alguien retiene los anteriores (GPUI leyéndolos) no se escribe sobre ellos.
        let held: Vec<CVPixelBuffer> = (0..5).map(|_| composer.render(&source, &Decor::default()).unwrap()).collect();
        let surfaces: std::collections::HashSet<usize> = held.iter().map(io_surface_of).collect();
        assert_eq!(surfaces.len(), held.len(), "se reusó un búfer que seguía retenido");
        // Otras medidas de salida: el grupo se rearma.
        composer.set_scene(&Scene { out: (96, 32), ..plain_scene() }).unwrap();
        let small = composer.render(&source, &Decor::default()).unwrap();
        assert_eq!((small.get_width(), small.get_height()), (96, 32));
    }

    #[test]
    fn set_scene_rearma_solo_lo_que_cambio() {
        let texts = vec![TextSpec::plain("Uno", TextAt::Top), TextSpec::plain("Dos", TextAt::Bottom)];
        let framed = Scene {
            frame: Some(FrameSpec { backdrop: Backdrop::Solid([0.2, 0.3, 0.4]), padding: 0.08, radius: 0.02, shadow: 0.5 }),
            texts,
            ..plain_scene()
        };
        let source = stripes_source();
        let mut composer = Composer::new().unwrap();
        composer.set_scene(&framed).unwrap();
        let decor = Decor { texts: vec![(0, 1.0), (1, 1.0)], ..Decor::default() };
        composer.render_bgra(&source, &decor).unwrap();
        let stage = |c: &Composer| Retained::as_ptr(&c.stage.as_ref().unwrap().background) as usize;
        let (stage_id, cached) = (stage(&composer), composer.texts.images.len());
        assert_eq!(cached, 2);
        // La misma escena: nada cambia.
        composer.set_scene(&framed.clone()).unwrap();
        assert_eq!(stage(&composer), stage_id);
        // Solo el aspecto: ni el marco ni los textos ya dibujados se rehacen.
        let other_look = Scene { look: LookSpec { blur_amount: 0.3, ..LookSpec::default() }, ..framed.clone() };
        composer.set_scene(&other_look).unwrap();
        assert_eq!(stage(&composer), stage_id);
        assert_eq!(composer.texts.images.len(), 2);
        // Un texto distinto: solo se tira ese.
        let mut edited = other_look.clone();
        edited.texts[1] = TextSpec::plain("Tres", TextAt::Bottom);
        composer.set_scene(&edited).unwrap();
        assert_eq!(stage(&composer), stage_id);
        assert_eq!(composer.texts.images.len(), 1);
        assert!(composer.texts.images.contains_key(&0));
        // Otro marco: se rearma.
        let mut reframed = edited.clone();
        reframed.frame.as_mut().unwrap().padding = 0.2;
        composer.set_scene(&reframed).unwrap();
        assert_ne!(stage(&composer), stage_id);
        // Otras medidas: también los textos.
        composer.set_scene(&Scene { out: (96, 32), ..reframed }).unwrap();
        assert_eq!(composer.texts.images.len(), 0);
        // Una escena que no sirve se rechaza y no rompe la anterior.
        assert!(composer.set_scene(&Scene { out: (0, 32), ..plain_scene() }).is_err());
        assert!(composer.render_bgra(&source, &Decor::default()).is_ok());
    }

    #[test]
    fn dos_escenas_iguales_se_comparan_iguales() {
        let a = plain_scene();
        assert_eq!(a, a.clone());
        let changes: Vec<Scene> = vec![
            Scene { src: (193.0, 64.0), ..a.clone() },
            Scene { out: (190, 64), ..a.clone() },
            Scene { frame: Some(FrameSpec { backdrop: Backdrop::Solid([0.0; 3]), padding: 0.0, radius: 0.0, shadow: 0.0 }), ..a.clone() },
            Scene { texts: vec![TextSpec::plain("x", TextAt::Top)], ..a.clone() },
            Scene { cursor_px: 41.0, ..a.clone() },
            Scene { look: LookSpec { cursor_style: CursorStyle::Dot, ..a.look }, ..a.clone() },
            Scene { look: LookSpec { blur_amount: 1.5, ..a.look }, ..a.clone() },
            Scene { look: LookSpec { click: ClickSpec { size: 2.0, ..a.look.click }, ..a.look }, ..a.clone() },
        ];
        for other in &changes {
            assert_ne!(&a, other);
        }
        let shadow_a = FrameSpec { backdrop: Backdrop::Image("/a.png".into()), padding: 0.1, radius: 0.0, shadow: 0.5 };
        assert_eq!(shadow_a, shadow_a.clone());
        assert_ne!(shadow_a, FrameSpec { shadow: 0.6, ..shadow_a.clone() });
        assert_eq!(LookSpec::default(), LookSpec { cursor_style: CursorStyle::System, click: ClickSpec { kind: ClickKind::Ripple, size: 1.0, color: [1.0; 3] }, blur_amount: 1.0 });
    }

    #[test]
    fn el_desenfoque_se_multiplica_y_con_cero_se_apaga() {
        let content = (1920.0, 1080.0);
        // Un paneo chico: 1% del ancho por cuadro = 19,2 px, un 20 % = 3,84 de radio.
        let normal = blur_plan(1.0, content, (0.01, 0.0, 1.0)).unwrap();
        let (radius, angle) = normal.pan.unwrap();
        assert!(near(radius, 3.84) && angle.abs() < 1e-12 && normal.zoom.is_none());
        let double = blur_plan(2.0, content, (0.01, 0.0, 1.0)).unwrap().pan.unwrap().0;
        assert!(near(double, 7.68));
        // Más de 2 vale 2, y lo que no es un número, 1.
        assert_eq!(blur_plan(9.0, content, (0.01, 0.0, 1.0)), blur_plan(2.0, content, (0.01, 0.0, 1.0)));
        assert_eq!(blur_plan(f64::NAN, content, (0.01, 0.0, 1.0)), Some(normal));
        // Con 0 no hay filtro, haya el movimiento que haya.
        assert_eq!(blur_plan(0.0, content, (0.2, 0.2, 2.0)), None);
        assert_eq!(blur_plan(-1.0, content, (0.2, 0.2, 2.0)), None);
        // Los topes de hoy (7 y 3 px en 1080p) se escalan con el multiplicador.
        let big = |amount| blur_plan(amount, content, (0.5, 0.0, 1.5)).unwrap();
        assert!(near(big(1.0).pan.unwrap().0, 7.0) && near(big(1.0).zoom.unwrap(), 3.0));
        assert!(near(big(2.0).pan.unwrap().0, 14.0) && near(big(2.0).zoom.unwrap(), 6.0));
        assert!(near(big(0.5).pan.unwrap().0, 3.5) && near(big(0.5).zoom.unwrap(), 1.5));
        // Un movimiento que no se nota (bajo el umbral) no lleva filtro; el zoom solo, tampoco el paneo.
        assert_eq!(blur_plan(1.0, content, (0.0002, 0.0, 1.0)), None);
        let only_zoom = blur_plan(1.0, content, (0.0, 0.0, 1.05)).unwrap();
        assert!(only_zoom.pan.is_none() && only_zoom.zoom.is_some());
        // El ángulo va hacia arriba en Core Image: un movimiento hacia abajo de la pantalla sale negativo.
        assert!(blur_plan(1.0, content, (0.0, 0.05, 1.0)).unwrap().pan.unwrap().1 < 0.0);
    }

    #[test]
    fn el_clic_normaliza_su_tamano_y_cada_tipo_se_dibuja_distinto() {
        let unit = 1920.0;
        assert_eq!(click_draw(ClickKind::None, 1.0, 0.2, unit), None);
        // El de siempre: radio (0,012 + 0,03 p) del ancho, 0,32 de opacidad que cae al cuadrado.
        let Some(ClickDraw::Disk { radius, solid, alpha }) = click_draw(ClickKind::Ripple, 1.0, 0.0, unit) else { panic!() };
        assert!(near(radius, 23.04) && near(solid, 23.04 * 0.55) && near(alpha, 0.32));
        let Some(ClickDraw::Disk { radius, alpha, .. }) = click_draw(ClickKind::Ripple, 1.0, 0.5, unit) else { panic!() };
        assert!(near(radius, 0.027 * 1920.0) && near(alpha, 0.08));
        // El tamaño multiplica el radio, entre 0,5 y 2,5.
        let r = |size| match click_draw(ClickKind::Ripple, size, 0.0, unit) {
            Some(ClickDraw::Disk { radius, .. }) => radius,
            _ => panic!(),
        };
        assert!(near(r(2.0), 2.0 * r(1.0)) && near(r(0.1), 0.5 * r(1.0)) && near(r(9.0), 2.5 * r(1.0)) && near(r(f64::NAN), r(1.0)));
        // Al terminar no queda nada.
        for kind in [ClickKind::Ripple, ClickKind::Ring, ClickKind::Pulse] {
            assert_eq!(click_draw(kind, 1.0, 1.0, unit), None);
            assert!(click_draw(kind, 1.0, 0.1, unit).is_some());
        }
        // El anillo es un anillo fino; el pulso llega más lejos que la onda.
        let Some(ClickDraw::Ring { radius, thickness, .. }) = click_draw(ClickKind::Ring, 1.0, 0.5, unit) else { panic!() };
        assert!(thickness < radius * 0.35 && thickness >= 1.5);
        let reach = |kind| match click_draw(kind, 1.0, 0.8, unit) {
            Some(ClickDraw::Disk { radius, .. } | ClickDraw::Ring { radius, .. }) => radius,
            None => panic!(),
        };
        assert!(reach(ClickKind::Pulse) > reach(ClickKind::Ripple));
        // Un video chico en el cuadro: el anillo no baja de 1,5 px de grosor.
        let Some(ClickDraw::Ring { thickness, .. }) = click_draw(ClickKind::Ring, 0.5, 0.2, 100.0) else { panic!() };
        assert!(near(thickness, 1.5));
    }

    #[test]
    fn la_sombra_escala_su_opacidad_y_su_difuminado() {
        assert_eq!(shadow_params(0.0, 1080.0), None);
        assert_eq!(shadow_params(-1.0, 1080.0), None);
        assert_eq!(shadow_params(f64::NAN, 1080.0), None);
        // 0,5 es la de siempre: opacidad 0,5, radio 2,2 % del alto, baja 1,2 %.
        let (alpha, blur, drop) = shadow_params(0.5, 1000.0).unwrap();
        assert!(near(alpha, 0.5) && near(blur, 22.0) && near(drop, 12.0));
        let (alpha, blur, _) = shadow_params(1.0, 1000.0).unwrap();
        assert!(near(alpha, 1.0) && near(blur, 27.5));
        // Pasar de 1 es 1.
        assert_eq!(shadow_params(3.0, 1000.0), shadow_params(1.0, 1000.0));
        assert!(shadow_params(0.25, 1000.0).unwrap().1 < 22.0);
    }

    #[test]
    fn el_cursor_elige_su_forma_segun_el_estilo() {
        let sprites = unsafe { Sprites::new() }.unwrap();
        let same = |a: &Sprite, b: &Sprite| std::ptr::eq(a, b);
        for shape in [CursorShape::Arrow, CursorShape::Hand, CursorShape::IBeam, CursorShape::Cross] {
            assert!(same(sprites.pick(CursorStyle::System, shape), sprites.system.pick(shape)));
            assert!(same(sprites.pick(CursorStyle::Dark, shape), sprites.dark.pick(shape)));
        }
        // El punto y el anillo valen para toda forma menos el I-beam, que conserva el suyo.
        for shape in [CursorShape::Arrow, CursorShape::Hand, CursorShape::Cross] {
            assert!(same(sprites.pick(CursorStyle::Dot, shape), &sprites.dot));
            assert!(same(sprites.pick(CursorStyle::Ring, shape), &sprites.ring));
        }
        assert!(same(sprites.pick(CursorStyle::Dot, CursorShape::IBeam), &sprites.system.ibeam));
        assert!(same(sprites.pick(CursorStyle::Ring, CursorShape::IBeam), &sprites.system.ibeam));
        // «Flecha» dibuja la flecha del sistema para toda forma, también el I-beam.
        for shape in [CursorShape::Arrow, CursorShape::Hand, CursorShape::IBeam, CursorShape::Cross] {
            assert!(same(sprites.pick(CursorStyle::Arrow, shape), &sprites.system.arrow));
        }
        // El punto caliente del punto, del anillo y de las formas centradas cae en su centro.
        for sprite in [&sprites.dot, &sprites.ring, &sprites.system.cross, &sprites.dark.cross] {
            let (_, (w, h), (hx, hy)) = sprite;
            assert!(near(*hx, w / 2.0) && near(*hy, h / 2.0));
        }
        // La flecha negra mide lo mismo que la del sistema.
        assert!(near(sprites.dark.arrow.1 .1, sprites.system.arrow.1 .1));
        // El cursor se mide con la flecha del sistema.
        assert!(near(sprites.reference_height(), sprites.system.arrow.1 .1.max(1.0)));
    }

    /// Dónde cae el cursor: en un video de 640 × 360 gris, `style` y `shape` con el cursor en el
    /// centro. Devuelve el cuadro sin cursor y con él.
    fn with_cursor(style: CursorStyle, shape: CursorShape, size: f64) -> (Vec<[u8; 3]>, Vec<[u8; 3]>) {
        let source = gray_source(640, 360, 110);
        let mut scene = scene_of(640, 360);
        scene.look.cursor_style = style;
        let base = rgb_of(&scene, &source, &Decor::default());
        let decor = Decor { cursor: Some((0.5, 0.5, size)), cursor_shape: shape, ..Decor::default() };
        (base, rgb_of(&scene, &source, &decor))
    }

    #[test]
    fn cada_estilo_de_cursor_se_dibuja_en_su_punto_caliente() {
        let (cx, cy) = (320usize, 180usize);
        // El punto: un disco centrado en el cursor, blanco al medio.
        let (base, shot) = with_cursor(CursorStyle::Dot, CursorShape::Arrow, 1.0);
        let diff = changed(&base, &shot);
        assert!(diff.len() > 150, "el punto casi no se dibuja ({} píxeles)", diff.len());
        let (x0, y0, x1, y1) = bounds(&diff, 640);
        let (mid_x, mid_y) = ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0);
        assert!((mid_x - cx as f64).abs() <= 1.5 && (mid_y - cy as f64).abs() <= 1.5, "el punto queda en ({mid_x}, {mid_y})");
        assert!(shot[cy * 640 + cx][0] > 240, "el centro del punto es blanco");
        // El anillo: el centro queda vacío (el video) y a su radio hay blanco.
        let (base, shot) = with_cursor(CursorStyle::Ring, CursorShape::Arrow, 1.0);
        assert_eq!(shot[cy * 640 + cx], base[cy * 640 + cx], "el anillo tiene el centro vacío");
        let diff = changed(&base, &shot);
        let (x0, y0, x1, y1) = bounds(&diff, 640);
        assert!(((x0 + x1) as f64 / 2.0 - cx as f64).abs() <= 1.5 && ((y0 + y1) as f64 / 2.0 - cy as f64).abs() <= 1.5);
        assert!((0..640).any(|x| shot[cy * 640 + x][0] > 240), "el anillo tiene su parte blanca");
        // Las flechas: la punta (esquina de arriba a la izquierda) va en el cursor.
        for style in [CursorStyle::System, CursorStyle::Dark] {
            let (base, shot) = with_cursor(style, CursorShape::Arrow, 1.0);
            let (x0, y0, x1, y1) = bounds(&changed(&base, &shot), 640);
            assert!((x0 as i64 - cx as i64).abs() <= 3 && (y0 as i64 - cy as i64).abs() <= 3, "{style:?}: la punta queda en ({x0}, {y0})");
            // Mide lo pedido: 40 px de alto, más o menos.
            assert!((30..=48).contains(&(y1 - y0 + 1)), "{style:?}: mide {} px de alto", y1 - y0 + 1);
            assert!(x1 > x0);
        }
        // La mano y la cruz: la oscura es negra con borde blanco y la normal al revés.
        for shape in [CursorShape::Hand, CursorShape::Cross, CursorShape::IBeam] {
            let (base, light) = with_cursor(CursorStyle::System, shape, 1.0);
            let (_, dark) = with_cursor(CursorStyle::Dark, shape, 1.0);
            let region = |shot: &[[u8; 3]]| -> Vec<u8> { changed(&base, shot).iter().map(|&i| shot[i][0]).collect() };
            let (light, dark) = (region(&light), region(&dark));
            assert!(light.iter().any(|&v| v > 240) && light.iter().any(|&v| v < 25), "{shape:?} clara con relleno blanco y borde negro");
            assert!(dark.iter().any(|&v| v > 240) && dark.iter().any(|&v| v < 25), "{shape:?} oscura con relleno negro y borde blanco");
            // El centro de la cruz y del I-beam es su relleno: blanco en uno, negro en el otro.
            if shape != CursorShape::Hand {
                let center = |shot: &[[u8; 3]]| shot[180 * 640 + 320][0];
                let (light_full, dark_full) = (with_cursor(CursorStyle::System, shape, 1.0).1, with_cursor(CursorStyle::Dark, shape, 1.0).1);
                assert!(center(&light_full) > 200 && center(&dark_full) < 50, "{shape:?}: relleno {} y {}", center(&light_full), center(&dark_full));
            }
        }
        // El tamaño escala con `size`: al doble, el doble de alto.
        let (base, one) = with_cursor(CursorStyle::Dark, CursorShape::Arrow, 1.0);
        let (_, two) = with_cursor(CursorStyle::Dark, CursorShape::Arrow, 2.0);
        let h = |shot: &[[u8; 3]]| {
            let (_, y0, _, y1) = bounds(&changed(&base, shot), 640);
            (y1 - y0 + 1) as f64
        };
        assert!((h(&two) / h(&one) - 2.0).abs() < 0.15);
    }

    /// El cuadro de 640 × 360 gris con un clic en el centro (progreso `p`) y el `click` pedido.
    fn with_click(click: ClickSpec, p: f64) -> (Vec<[u8; 3]>, Vec<[u8; 3]>) {
        let source = gray_source(640, 360, 90);
        let mut scene = scene_of(640, 360);
        scene.look.click = click;
        let base = rgb_of(&scene, &source, &Decor::default());
        let decor = Decor { ripples: vec![(0.5, 0.5, p)], ..Decor::default() };
        (base, rgb_of(&scene, &source, &decor))
    }

    #[test]
    fn cada_clic_se_dibuja_a_su_manera() {
        let (cx, cy) = (320usize, 180usize);
        let spec = |kind, size| ClickSpec { kind, size, color: [1.0; 3] };
        // Sin efecto: el cuadro queda idéntico.
        let (base, shot) = with_click(spec(ClickKind::None, 1.0), 0.1);
        assert!(changed(&base, &shot).is_empty());
        // La onda: un disco más claro, con el centro afectado.
        let (base, ripple) = with_click(spec(ClickKind::Ripple, 1.0), 0.1);
        assert!(ripple[cy * 640 + cx][0] > base[cy * 640 + cx][0]);
        // El anillo: el centro queda igual y en su radio hay más luz.
        let (base, ring) = with_click(spec(ClickKind::Ring, 1.0), 0.2);
        assert_eq!(ring[cy * 640 + cx], base[cy * 640 + cx], "el anillo no toca el centro");
        let diff = changed(&base, &ring);
        assert!(diff.len() > 100, "el anillo casi no se dibuja ({} píxeles)", diff.len());
        let (x0, _, x1, _) = bounds(&diff, 640);
        let radius = (x1 - x0) as f64 / 2.0;
        let on_ring = (0..640).filter(|&x| ring[cy * 640 + x][0] > base[cy * 640 + x][0] + 40).count();
        assert!((2..=14).contains(&on_ring), "el anillo es fino ({on_ring} px de grosor en la fila del centro, x2 por los dos lados)");
        // El pulso llega más lejos que la onda en el mismo avance.
        let (base_p, pulse) = with_click(spec(ClickKind::Pulse, 1.0), 0.6);
        let (_, ripple_late) = with_click(spec(ClickKind::Ripple, 1.0), 0.6);
        let reach = |a: &[[u8; 3]], b: &[[u8; 3]]| {
            let (x0, _, x1, _) = bounds(&changed(a, b), 640);
            x1 - x0
        };
        assert!(reach(&base_p, &pulse) > reach(&base_p, &ripple_late));
        // `size` multiplica el radio.
        let (base2, ring2) = with_click(spec(ClickKind::Ring, 2.0), 0.2);
        let (x0b, _, x1b, _) = bounds(&changed(&base2, &ring2), 640);
        let ratio = (x1b - x0b) as f64 / 2.0 / radius;
        assert!((ratio - 2.0).abs() < 0.15, "el doble de tamaño, el doble de radio (salió {ratio})");
        // El color es el pedido.
        let (base, red) = with_click(ClickSpec { kind: ClickKind::Pulse, size: 1.0, color: [1.0, 0.1, 0.1] }, 0.1);
        let p = red[cy * 640 + cx];
        assert!(p[0] > base[cy * 640 + cx][0] + 20 && (p[1] as i32) < (p[0] as i32) - 20, "el pulso rojo es rojo: {p:?}");
    }

    #[test]
    fn el_desenfoque_de_movimiento_se_nota_y_sigue_a_blur_amount() {
        // Rayas finas verticales: el desenfoque horizontal les baja el contraste.
        let source = bgra_buffer(640, 360, |x, _| if (x / 4) % 2 == 0 { [235, 235, 235] } else { [20, 20, 20] });
        let sharpness = |amount: f64, blur: Option<(f64, f64, f64)>| {
            let mut scene = scene_of(640, 360);
            scene.look.blur_amount = amount;
            let rgb = rgb_of(&scene, &source, &Decor { blur, ..Decor::default() });
            (100..260).map(|y| (100..540).map(|x| (rgb[y * 640 + x][0] as i32 - rgb[y * 640 + x + 1][0] as i32).abs() as i64).sum::<i64>()).sum::<i64>()
        };
        let moving = Some((0.03, 0.0, 1.0));
        let still = sharpness(1.0, None);
        let (zero, one, two) = (sharpness(0.0, moving), sharpness(1.0, moving), sharpness(2.0, moving));
        // Con 0 sale igual que sin movimiento.
        assert_eq!(zero, still);
        assert!(one < still * 9 / 10, "con 1 el cuadro se desenfoca ({one} contra {still})");
        assert!(two < one * 9 / 10, "con 2 se desenfoca más que con 1 ({two} contra {one})");
        // Con la mitad queda entre medio.
        let half = sharpness(0.5, moving);
        assert!(half < still && half > one, "{still} > {half} > {one}");
    }

    #[test]
    fn la_sombra_del_marco_oscurece_alrededor_del_video() {
        let source = gray_source(640, 360, 128);
        let at = |shadow: f64| {
            let mut scene = scene_of(640, 360);
            scene.frame = Some(FrameSpec { backdrop: Backdrop::Solid([1.0; 3]), padding: 0.12, radius: 0.02, shadow });
            rgb_of(&scene, &source, &Decor::default())
        };
        let (none, normal, strong) = (at(0.0), at(0.5), at(1.0));
        // El video llena 554 × 311 con margen de 43 px: su borde de abajo cae en y = 336 (de
        // arriba abajo). Se mira 4 px más abajo, donde solo hay fondo y sombra.
        assert_eq!(content_rect((640.0, 360.0), (640.0, 360.0), frame_margin(0.12, 640.0, 360.0)), [43.0, 24.0, 554.0, 311.0]);
        let probe = |img: &[[u8; 3]]| img[340 * 640 + 320][0] as i32;
        // Sin sombra el fondo sigue blanco; con ella oscurece, y más con más.
        assert_eq!(probe(&none), 255);
        assert!(probe(&normal) < 235, "la sombra de 0,5 oscurece ({})", probe(&normal));
        assert!(probe(&strong) < probe(&normal) - 15, "la de 1 oscurece más ({} contra {})", probe(&strong), probe(&normal));
        // Y en conjunto también.
        let area = |img: &[[u8; 3]]| img.iter().map(|p| 255 - p[0] as i64).sum::<i64>();
        assert!(area(&normal) > area(&none) && area(&strong) > area(&normal));
    }

    #[test]
    fn el_aspecto_por_defecto_es_el_de_siempre() {
        let look = LookSpec::default();
        assert_eq!(look.cursor_style, CursorStyle::System);
        assert_eq!(look.click, ClickSpec { kind: ClickKind::Ripple, size: 1.0, color: [1.0, 1.0, 1.0] });
        assert_eq!(look.blur_amount, 1.0);
    }


    /// El cuadro de `source` en `at` segundos, en NV12 de rango completo (como lo entrega el
    /// reproductor).
    fn decoded_frame(source: &Path, at: f64) -> CVPixelBuffer {
        objc2::rc::autoreleasepool(|_| unsafe {
            let none: *const AnyObject = std::ptr::null();
            let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset").unwrap(), URLAssetWithURL: &*file_url(source).unwrap(), options: none];
            let (track, _) = video_track(&asset).unwrap();
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let reader: Option<Retained<AnyObject>> = msg_send![class(c"AVAssetReader").unwrap(), assetReaderWithAsset: &*asset, error: &mut error];
            let reader = reader.expect("lector");
            let settings = dict(&[(key(kCVPixelBufferPixelFormatTypeKey), CFNumber::from(NV12_FULL as i32).as_CFType())]);
            let output: Retained<AnyObject> =
                msg_send![class(c"AVAssetReaderTrackOutput").unwrap(), assetReaderTrackOutputWithTrack: &*track, outputSettings: as_object(&settings)];
            let _: () = msg_send![&reader, addOutput: &*output];
            let range = TimeRange { start: cmtime(at), duration: cmtime(1.0) };
            let _: () = msg_send![&reader, setTimeRange: range];
            let started: Bool = msg_send![&reader, startReading];
            assert!(started.as_bool(), "el lector no empezó");
            let sample: *mut CMSampleBuffer = msg_send![&output, copyNextSampleBuffer];
            assert!(!sample.is_null(), "sin cuadros en {at} s");
            let _sample = Released(sample as *const c_void);
            let pixels = CMSampleBufferGetImageBuffer(sample as *const c_void);
            // Se retiene: el cuadro sobrevive a la muestra.
            CVPixelBuffer::wrap_under_get_rule(pixels as CVPixelBufferRef)
        })
    }

    /// Un NV12 como lo muestra GPUI: la matriz BT.601 de rango completo de su shader, a RGB.
    fn gpui_view_of(nv12: &CVPixelBuffer) -> (usize, usize, Vec<[u8; 3]>) {
        let (w, h, luma, chroma) = read_nv12(nv12);
        let mut out = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let yy = luma[y * w + x] as f64 / 255.0;
                let [cb, cr] = chroma[(y / 2) * (w / 2) + x / 2];
                let (cb, cr) = (cb as f64 / 255.0 - 0.5, cr as f64 / 255.0 - 0.5);
                let to = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                out.push([to(yy + 1.402 * cr), to(yy - 0.3441 * cb - 0.7141 * cr), to(yy + 1.772 * cb)]);
            }
        }
        (w, h, out)
    }

    fn save_png(path: &str, w: usize, h: usize, rgb: &[[u8; 3]]) {
        let file = std::io::BufWriter::new(std::fs::File::create(path).expect("png"));
        let mut encoder = png::Encoder::new(file, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let flat: Vec<u8> = rgb.iter().flatten().copied().collect();
        encoder.write_header().unwrap().write_image_data(&flat).unwrap();
    }

    /// `ATIC_TEST_COMPOSER=<video>`: un cuadro de ese video por el Composer en vertical 9:16, con
    /// marco, texto, zoom, cursor de mano y clic en anillo. Guarda el NV12 (como lo vería GPUI) en
    /// `/tmp/atic-composer-prueba.png` y el BGRA de la exportación en
    /// `/tmp/atic-composer-prueba-bgra.png`, para compararlos.
    #[test]
    fn prueba_del_composer() {
        let Ok(source) = std::env::var("ATIC_TEST_COMPOSER") else {
            return;
        };
        let (_, (src_w, src_h)) = video_info(Path::new(&source)).expect("video");
        let frame = decoded_frame(Path::new(&source), 1.0);
        let out = output_size((src_w as u32, src_h as u32), Some(1280), Some((9, 16)));
        let scene = Scene {
            src: (src_w, src_h),
            out,
            frame: Some(FrameSpec { backdrop: Backdrop::Gradient([0.55, 0.18, 0.89], [0.29, 0.0, 0.88]), padding: 0.06, radius: 0.025, shadow: 0.7 }),
            texts: vec![TextSpec::plain("Hola desde Atic", TextAt::Top)],
            cursor_px: 44.0,
            look: LookSpec { cursor_style: CursorStyle::System, click: ClickSpec { kind: ClickKind::Ring, size: 1.5, color: [1.0, 0.85, 0.3] }, blur_amount: 1.0 },
        };
        let decor = Decor {
            crop: [0.2, 0.15, 0.6, 0.6],
            cursor: Some((0.45, 0.4, 1.0)),
            cursor_shape: CursorShape::Hand,
            texts: vec![(0, 1.0)],
            ripples: vec![(0.45, 0.4, 0.3)],
            blur: Some((0.01, 0.004, 1.02)),
            ..Decor::default()
        };
        let mut composer = Composer::new().expect("composer");
        composer.set_scene(&scene).expect("escena");
        let started = Instant::now();
        let nv12 = composer.render(&frame, &decor).expect("render");
        eprintln!("NV12 {}×{} en {:?}", nv12.get_width(), nv12.get_height(), started.elapsed());
        let bgra = composer.render_bgra(&frame, &decor).expect("render bgra");
        let (w, h, view) = gpui_view_of(&nv12);
        let (_, _, reference) = read_rgb(&bgra);
        save_png("/tmp/atic-composer-prueba.png", w, h, &view);
        save_png("/tmp/atic-composer-prueba-bgra.png", w, h, &reference);
        let (mut sum, mut max) = (0u64, 0i32);
        for (a, b) in view.iter().zip(&reference) {
            for c in 0..3 {
                let d = (a[c] as i32 - b[c] as i32).abs();
                sum += d as u64;
                max = max.max(d);
            }
        }
        eprintln!("diferencia NV12 contra BGRA: media {:.2}, máxima {max} (de 255)", sum as f64 / (w * h * 3) as f64);
        eprintln!("/tmp/atic-composer-prueba.png y /tmp/atic-composer-prueba-bgra.png");

        // Los estilos de cursor (flecha, mano y I-beam de cada uno) y los tipos de clic, recortados
        // alrededor del cursor, en una tira: `/tmp/atic-composer-estilos.png`.
        const TILE: usize = 140;
        let plain = Scene { src: (src_w, src_h), out: (src_w as u32 & !1, src_h as u32 & !1), frame: None, texts: vec![], cursor_px: 44.0, look: LookSpec::default() };
        let mut tiles: Vec<Vec<[u8; 3]>> = Vec::new();
        let mut shoot = |look: LookSpec, shape: CursorShape, ripple: Option<f64>| {
            let mut composer = Composer::new().unwrap();
            composer.set_scene(&Scene { look, ..plain.clone() }).unwrap();
            let decor = Decor { cursor: Some((0.5, 0.5, 1.0)), cursor_shape: shape, ripples: ripple.map(|p| (0.5, 0.5, p)).into_iter().collect(), ..Decor::default() };
            let (w, _, rgb) = read_rgb(&composer.render_bgra(&frame, &decor).unwrap());
            let (cx, cy) = (src_w as usize / 2, src_h as usize / 2);
            let mut tile = Vec::with_capacity(TILE * TILE);
            for y in cy - TILE / 2..cy + TILE / 2 {
                tile.extend_from_slice(&rgb[y * w + cx - TILE / 2..y * w + cx + TILE / 2]);
            }
            tiles.push(tile);
        };
        for style in [CursorStyle::System, CursorStyle::Dot, CursorStyle::Ring, CursorStyle::Dark] {
            for shape in [CursorShape::Arrow, CursorShape::Hand, CursorShape::IBeam] {
                shoot(LookSpec { cursor_style: style, ..LookSpec::default() }, shape, None);
            }
        }
        for kind in [ClickKind::Ripple, ClickKind::Ring, ClickKind::Pulse] {
            for p in [0.15, 0.5] {
                shoot(LookSpec { click: ClickSpec { kind, size: 1.0, color: [1.0, 0.85, 0.3] }, ..LookSpec::default() }, CursorShape::Arrow, Some(p));
            }
        }
        let (columns, rows) = (6usize, tiles.len().div_ceil(6));
        let mut sheet = vec![[0u8; 3]; columns * TILE * rows * TILE];
        for (n, tile) in tiles.iter().enumerate() {
            let (col, row) = (n % columns, n / columns);
            for y in 0..TILE {
                let at = (row * TILE + y) * columns * TILE + col * TILE;
                sheet[at..at + TILE].copy_from_slice(&tile[y * TILE..(y + 1) * TILE]);
            }
        }
        save_png("/tmp/atic-composer-estilos.png", columns * TILE, rows * TILE, &sheet);
        eprintln!("/tmp/atic-composer-estilos.png (cursores: sistema, punto, anillo y oscuro × flecha, mano e I-beam; clics: onda, anillo y pulso × 2 avances)");
    }

    #[test]
    fn el_bitrate_sigue_la_tabla_del_contrato() {
        // 1920 × 1080 × 30 = 62 208 000 bits por segundo de referencia.
        assert_eq!(bitrate(1920, 1080, Quality::Standard, Codec::H264), 7_464_960);
        assert_eq!(bitrate(1920, 1080, Quality::Low, Codec::H264), 4_354_560);
        assert_eq!(bitrate(1920, 1080, Quality::High, Codec::H264), 12_441_600);
        assert_eq!(bitrate(1920, 1080, Quality::Max, Codec::H264), 21_772_800);
        // HEVC lleva el 60 %.
        assert_eq!(bitrate(1920, 1080, Quality::Standard, Codec::Hevc), 4_478_976);
    }

    #[test]
    fn el_bitrate_tiene_piso_y_tope() {
        assert_eq!(bitrate(320, 180, Quality::Low, Codec::H264), 2_000_000);
        // 4K al máximo pasaría de 60 Mbps.
        assert_eq!(bitrate(3840, 2160, Quality::Max, Codec::H264), 60_000_000);
        // Con HEVC cabe: 8 294 400 × 30 × 0,35 × 0,6.
        assert_eq!(bitrate(3840, 2160, Quality::Max, Codec::Hevc), 52_254_720);
    }

    #[test]
    fn el_cuadro_de_salida_es_par_y_nunca_agranda() {
        assert_eq!(output_size((1920, 1080), None, None), (1920, 1080));
        assert_eq!(output_size((1920, 1080), Some(720), None), (1280, 720));
        assert_eq!(output_size((1920, 1080), Some(540), None), (960, 540));
        // Un tope mayor que el original no agranda.
        assert_eq!(output_size((1920, 1080), Some(2160), None), (1920, 1080));
        // Medidas impares del original: pares en la salida.
        assert_eq!(output_size((1919, 1081), None, None), (1918, 1080));
    }

    #[test]
    fn el_cuadro_de_salida_sigue_la_proporcion_pedida() {
        assert_eq!(output_size((1920, 1080), None, Some((16, 9))), (1920, 1080));
        assert_eq!(output_size((1920, 1080), None, Some((1, 1))), (1080, 1080));
        assert_eq!(output_size((1920, 1080), None, Some((4, 5))), (864, 1080));
        // 1080 × 9 / 16 = 607,5: se corta y queda par.
        assert_eq!(output_size((1920, 1080), None, Some((9, 16))), (606, 1080));
        assert_eq!(output_size((1920, 1080), Some(720), Some((9, 16))), (404, 720));
    }

    #[test]
    fn el_video_entra_entero_y_centrado() {
        // Sin margen y misma proporción: llena el cuadro.
        assert_eq!(content_rect((1920.0, 1080.0), (1920.0, 1080.0), 0.0), [0.0, 0.0, 1920.0, 1080.0]);
        // Cuadrado: el ancho manda y sobra arriba y abajo.
        assert_eq!(content_rect((1600.0, 1600.0), (1920.0, 1080.0), 0.0), [0.0, 350.0, 1600.0, 900.0]);
        // Con margen: se resta de los dos lados.
        assert_eq!(content_rect((1600.0, 1600.0), (1920.0, 1080.0), 160.0), [160.0, 440.0, 1280.0, 720.0]);
        // Vertical: 606 de ancho, 340,875 de alto (341) y centrado (369,5 se corta a 369).
        assert_eq!(content_rect((606.0, 1080.0), (1920.0, 1080.0), 0.0), [0.0, 369.0, 606.0, 341.0]);
        // Un video vertical en un cuadro ancho: manda el alto.
        assert_eq!(content_rect((1920.0, 1080.0), (1080.0, 1920.0), 0.0), [656.0, 0.0, 608.0, 1080.0]);
    }

    #[test]
    fn el_marco_con_la_misma_proporcion_conserva_su_margen() {
        let pad = frame_margin(0.08, 1920.0, 1080.0);
        assert!(near(pad, 86.4));
        // Ancho 1920 − 172,8 = 1747,2 y alto 982,8, redondeados.
        assert_eq!(content_rect((1920.0, 1080.0), (1920.0, 1080.0), pad), [86.0, 48.0, 1747.0, 983.0]);
        // El margen tiene tope (30 % del lado corto).
        assert!(near(frame_margin(0.9, 1920.0, 1080.0), 324.0));
    }

    /// Cortes en `(0, 10)` y `(12, 20)`; a 2× de 2 a 4 y a 0,5× de 15 a 18.
    fn con_velocidades() -> Vec<Piece> {
        let speeds = [SpeedSpan { start: 2.0, end: 4.0, rate: 2.0 }, SpeedSpan { start: 15.0, end: 18.0, rate: 0.5 }];
        pieces(&[(0.0, 10.0), (12.0, 20.0)], &speeds)
    }

    #[test]
    fn los_tramos_se_parten_donde_cambia_la_velocidad() {
        let p = con_velocidades();
        let rates: Vec<f64> = p.iter().map(|p| p.rate).collect();
        assert_eq!(rates, vec![1.0, 2.0, 1.0, 1.0, 0.5, 1.0]);
        let starts: Vec<f64> = p.iter().map(|p| p.out0).collect();
        assert_eq!(starts, vec![0.0, 2.0, 3.0, 9.0, 12.0, 18.0]);
        assert!(near(out_length(&p), 20.0));
    }

    #[test]
    fn el_tiempo_de_salida_cuenta_cortes_y_velocidades() {
        let p = con_velocidades();
        assert!(near(out_time(&p, 1.0).unwrap(), 1.0));
        assert!(near(out_time(&p, 3.0).unwrap(), 2.5));
        assert!(near(out_time(&p, 4.0).unwrap(), 3.0));
        assert!(near(out_time(&p, 9.5).unwrap(), 8.5));
        assert!(near(out_time(&p, 12.0).unwrap(), 9.0));
        assert!(near(out_time(&p, 16.0).unwrap(), 14.0));
        assert!(near(out_time(&p, 19.0).unwrap(), 19.0));
        // Lo cortado y lo que pasa del final no queda.
        assert_eq!(out_time(&p, 10.0), None);
        assert_eq!(out_time(&p, 11.0), None);
        assert_eq!(out_time(&p, 20.0), None);
    }

    #[test]
    fn una_velocidad_que_cruza_un_corte_se_reparte() {
        let p = pieces(&[(0.0, 4.0), (6.0, 10.0)], &[SpeedSpan { start: 2.0, end: 8.0, rate: 2.0 }]);
        assert_eq!(p.len(), 4);
        assert!(near(out_length(&p), 6.0));
        // 6 → 7 va a 2×: 3 s de lo anterior más medio segundo.
        assert!(near(out_time(&p, 7.0).unwrap(), 3.5));
        assert!(near(out_time(&p, 9.0).unwrap(), 5.0));
    }

    #[test]
    fn los_tramos_contiguos_a_la_misma_velocidad_se_juntan() {
        // Sin empalme falso en el audio.
        let p = pieces(&[(0.0, 2.0), (2.0, 4.0)], &[SpeedSpan { start: 5.0, end: 8.0, rate: 2.0 }]);
        assert_eq!(p, vec![Piece { a: 0.0, b: 4.0, rate: 1.0, out0: 0.0 }]);
        // Sin velocidades ni cortes, el tiempo no cambia.
        let p = pieces(&[(0.0, 10.0)], &[]);
        assert!(near(out_time(&p, 7.25).unwrap(), 7.25));
        assert!(pieces(&[(3.0, 3.0)], &[]).is_empty());
    }

    #[test]
    fn a_velocidad_normal_no_se_descarta_ningun_cuadro() {
        // Dos cuadros a 12 ms (un jitter normal de la captura) entran los dos a 1×…
        let p = pieces(&[(0.0, 10.0)], &[SpeedSpan { start: 5.0, end: 8.0, rate: 4.0 }]);
        assert!(keep_at(&p, 1.0, 1.0, 1.012, 60.0, 60.0));
        // …pero a 4× el segundo no cabe, y uno repetido (mismo tiempo) tampoco entra nunca.
        assert!(!keep_at(&p, 5.012, 2.25, 2.253, 60.0, 60.0));
        assert!(!keep_at(&p, 1.0, 1.0, 1.0, 60.0, 60.0));
        assert_eq!(rate_at(&p, 6.0), 4.0);
        assert_eq!(rate_at(&p, 9.0), 1.0);
    }

    #[test]
    fn exportar_a_menos_cuadros_por_segundo_los_descarta() {
        let p = pieces(&[(0.0, 10.0)], &[]);
        // Una fuente de 60 a 30: el segundo cuadro (12 ms) ya no cabe…
        assert!(!keep_at(&p, 1.012, 1.0, 1.012, 30.0, 60.0));
        // …y uno a 34 ms sí.
        assert!(keep_at(&p, 1.034, 1.0, 1.034, 30.0, 60.0));
        // A la misma cadencia no se toca nada, y una fuente que no declara fps tampoco.
        assert!(keep_at(&p, 1.012, 1.0, 1.012, 60.0, 60.0));
        assert!(keep_at(&p, 1.012, 1.0, 1.012, 30.0, 0.0));
    }

    #[test]
    fn los_cuadros_muy_juntos_se_descartan() {
        assert!(keep_frame(f64::NEG_INFINITY, 0.0, 60.0));
        // 0,9 / 60 = 0,015 s.
        assert!(!keep_frame(0.0, 0.010, 60.0));
        assert!(keep_frame(0.0, 1.0 / 60.0, 60.0));
        // A 4× de un video a 60 cuadros llega uno cada 1/240 s: queda uno de cada cuatro.
        let mut last = f64::NEG_INFINITY;
        let mut kept = 0;
        for i in 0..240 {
            let at = i as f64 / 240.0;
            if keep_frame(last, at, 60.0) {
                last = at;
                kept += 1;
            }
        }
        assert_eq!(kept, 60);
    }
}
