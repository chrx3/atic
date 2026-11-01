//! Grabar una zona de la pantalla a MP4 como lo hace el sistema (⌘⇧5): ScreenCaptureKit
//! entrega cada cuadro como un `CVPixelBuffer` en NV12 (IOSurface, sin pasar por la CPU)
//! y `AVAssetWriter` lo codifica en H.264 con el codificador de hardware.
//!
//! Pausar no corta nada: quien graba decide con `Stamp` en qué momento del video va
//! cada cuadro (`None` en pausa: ese no va), así el archivo sale de una sola pieza.
//!
//! El audio va aparte (`atic-audio`, la mezcla de micrófono y sistema): al terminar,
//! [`mux_pcm`] lo junta con el video sin volver a codificar la imagen.
//!
//! AVFoundation va por `msg_send` (es poco); los tipos de CoreMedia y CoreVideo
//! llevan su codificación para que objc2 los verifique en debug.

use std::ffi::c_void;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use block2::RcBlock;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use dispatch2::DispatchQueue;
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ProtocolObject};
use objc2::{available, define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_screen_capture_kit::{SCContentFilter, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput, SCStreamOutputType};

use crate::Rect;

/// En qué momento del video va un cuadro que llega ahora; `None`: no va (en pausa o
/// antes de empezar). Se llama desde el hilo de ScreenCaptureKit.
pub type Stamp = Box<dyn Fn() -> Option<Duration> + Send + Sync>;

const NV12: u32 = u32::from_be_bytes(*b"420v");
const FPS: i32 = 60;
/// `kAudioFormatMPEG4AAC` y `kAudioFormatLinearPCM`.
const AAC: u32 = u32::from_be_bytes(*b"aac ");
const LPCM: u32 = u32::from_be_bytes(*b"lpcm");
/// `kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked`.
const PCM_FLAGS: u32 = 4 | 8;
/// `AVAssetWriterStatusCompleted`.
const WRITER_COMPLETED: isize = 2;
/// `AVAssetReaderStatusReading`.
const READER_READING: isize = 1;
const READER_FAILED: isize = 3;
/// Cuánto espera [`mux_pcm`] sin que el escritor acepte nada (ni video ni audio) antes
/// de darlo por trabado. Con 10 s, un equipo cargado o con la memoria al límite perdía
/// el audio de grabaciones que habrían terminado.
const MUX_IDLE: Duration = Duration::from_secs(30);

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
    static AVVideoMaxKeyFrameIntervalDurationKey: CFStringRef;
    static AVVideoProfileLevelKey: CFStringRef;
    static AVVideoProfileLevelH264HighAutoLevel: CFStringRef;
    static AVVideoAllowFrameReorderingKey: CFStringRef;
    static AVFormatIDKey: CFStringRef;
    static AVSampleRateKey: CFStringRef;
    static AVNumberOfChannelsKey: CFStringRef;
    static AVEncoderBitRateKey: CFStringRef;
}

#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sample: *const c_void) -> *const c_void;
    fn CMSampleBufferGetPresentationTimeStamp(sample: *const CMSampleBuffer) -> CMTime;
    fn CMAudioFormatDescriptionCreate(
        allocator: *const c_void,
        asbd: *const AudioStreamBasicDescription,
        layout_size: usize,
        layout: *const c_void,
        cookie_size: usize,
        cookie: *const c_void,
        extensions: *const c_void,
        out: *mut *const FormatDescription,
    ) -> i32;
    fn CMBlockBufferCreateWithMemoryBlock(
        allocator: *const c_void,
        memory: *mut c_void,
        block_length: usize,
        block_allocator: *const c_void,
        custom_source: *const c_void,
        offset: usize,
        data_length: usize,
        flags: u32,
        out: *mut *const c_void,
    ) -> i32;
    fn CMBlockBufferReplaceDataBytes(source: *const c_void, block: *const c_void, offset: usize, length: usize) -> i32;
    fn CMAudioSampleBufferCreateReadyWithPacketDescriptions(
        allocator: *const c_void,
        data: *const c_void,
        format: *const FormatDescription,
        samples: isize,
        pts: CMTime,
        packets: *const c_void,
        out: *mut *mut CMSampleBuffer,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRetain(cf: *const c_void) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> u8;
}

/// `CVPixelBufferRef`, con su codificación para objc2.
#[repr(C)]
struct CVBuffer {
    _private: [u8; 0],
}
// SAFETY: la codificación de `CVBufferRef` (`^{__CVBuffer=}`).
unsafe impl RefEncode for CVBuffer {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CVBuffer", &[]));
}

/// `CMFormatDescriptionRef`, con su codificación para objc2.
#[repr(C)]
pub(super) struct FormatDescription {
    _private: [u8; 0],
}
// SAFETY: la codificación de `CMFormatDescriptionRef` (`^{opaqueCMFormatDescription=}`).
unsafe impl RefEncode for FormatDescription {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("opaqueCMFormatDescription", &[]));
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
// SAFETY: la misma disposición que `CGRect`.
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct(
        "CGRect",
        &[Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]), Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING])],
    );
}

fn time(at: Duration) -> CMTime {
    CMTime { value: at.as_micros() as i64, timescale: 1_000_000, flags: CMTimeFlags::Valid, epoch: 0 }
}

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("sin {}", name.to_string_lossy()))
}

/// Una constante de AVFoundation como objeto (es un `NSString`).
fn ns(key: CFStringRef) -> &'static AnyObject {
    // SAFETY: constantes del framework, vivas mientras viva el proceso.
    unsafe { &*(key as *const AnyObject) }
}

fn key(key: CFStringRef) -> CFString {
    // SAFETY: constante del framework.
    unsafe { CFString::wrap_under_get_rule(key) }
}

/// Un `CFDictionary` como `NSDictionary` (puente sin costo).
fn dict(pairs: &[(CFString, CFType)]) -> CFDictionary<CFString, CFType> {
    CFDictionary::from_CFType_pairs(pairs)
}

fn as_object<T: TCFType>(value: &T) -> &AnyObject {
    // SAFETY: los tipos de CoreFoundation usados aquí tienen puente con Foundation.
    unsafe { &*(value.as_CFTypeRef() as *const AnyObject) }
}

fn file_url(path: &Path) -> Result<Retained<AnyObject>, String> {
    // SAFETY: `fileURLWithPath:` devuelve una URL autoliberada que se retiene.
    let url: Retained<AnyObject> = unsafe { msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&path.to_string_lossy())] };
    Ok(url)
}

fn error_text(error: *mut AnyObject) -> String {
    // SAFETY: un `NSError` de AVFoundation o nulo.
    unsafe { (error as *mut NSError).as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default()
}

/// Un objeto de AVFoundation que se usa con el candado tomado.
struct Obj(Retained<AnyObject>);
// SAFETY: AVAssetWriter y sus entradas se pueden usar desde cualquier hilo si las
// llamadas no se pisan; aquí van siempre con el `Mutex` de `Writer` tomado.
unsafe impl Send for Obj {}

struct Writer {
    writer: Obj,
    input: Obj,
    adaptor: Obj,
    last: Option<Duration>,
    /// El último cuadro: al terminar se repite al final, así el video dura lo grabado
    /// aunque la pantalla haya quedado quieta (SCK no manda cuadros sin cambios).
    last_pixels: Option<Released>,
    frames: u64,
    /// Ya no se agregan cuadros (terminando o descartada).
    closed: bool,
}

struct Shared {
    writer: Mutex<Writer>,
    stamp: Stamp,
}

impl Shared {
    fn push(&self, pixels: *const CVBuffer) {
        let Some(at) = (self.stamp)() else { return };
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        // Las marcas tienen que crecer: dos cuadros en el mismo microsegundo, el segundo sobra.
        if w.closed || w.last.is_some_and(|last| at <= last) {
            return;
        }
        // SAFETY: entrada y adaptador vivos; el búfer lo retiene AVFoundation si lo necesita.
        unsafe {
            let ready: Bool = msg_send![&*w.input.0, isReadyForMoreMediaData];
            if !ready.as_bool() {
                return;
            }
            let ok: Bool = msg_send![&*w.adaptor.0, appendPixelBuffer: pixels, withPresentationTime: time(at)];
            if !ok.as_bool() {
                let error: *mut AnyObject = msg_send![&*w.writer.0, error];
                tracing::warn!(error = error_text(error), "grabar: AVAssetWriter no aceptó el cuadro");
                w.closed = true;
                return;
            }
        }
        w.last = Some(at);
        // SAFETY: se retiene para repetirlo al final; `Released` lo suelta.
        w.last_pixels = Some(Released(unsafe { CFRetain(pixels.cast()) }));
        w.frames += 1;
    }
}

struct OutputIvars {
    shared: Arc<Shared>,
}

define_class!(
    // SAFETY: NSObject no impone requisitos extra y la clase no implementa Drop.
    #[unsafe(super(NSObject))]
    #[name = "AticScreenRecorderOutput"]
    #[ivars = OutputIvars]
    struct RecorderOutput;

    unsafe impl NSObjectProtocol for RecorderOutput {}

    unsafe impl SCStreamOutput for RecorderOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_did_output_sample_buffer_of_type(&self, _stream: &SCStream, sample_buffer: &CMSampleBuffer, r#type: SCStreamOutputType) {
            if r#type != SCStreamOutputType::Screen {
                return;
            }
            // SAFETY: la muestra vive durante el callback; sin imagen es un cuadro
            // «quieto» de SCK (nada cambió) y no se graba: el anterior sigue.
            let pixels = unsafe { CMSampleBufferGetImageBuffer(sample_buffer as *const CMSampleBuffer as *const c_void) } as *const CVBuffer;
            if !pixels.is_null() {
                self.ivars().shared.push(pixels);
            }
        }
    }

    unsafe impl SCStreamDelegate for RecorderOutput {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
            tracing::warn!(error = %error.localizedDescription(), "grabar: la captura de la pantalla se detuvo");
        }
    }
);

impl RecorderOutput {
    fn new(ivars: OutputIvars) -> Retained<Self> {
        let output = Self::alloc().set_ivars(ivars);
        // SAFETY: `init` de NSObject sobre una instancia recién allocada.
        unsafe { msg_send![super(output), init] }
    }
}

/// Una grabación en curso. Soltarla sin [`ScreenRecorder::finish`] la descarta.
pub struct ScreenRecorder {
    stream: Retained<SCStream>,
    output: Retained<RecorderOutput>,
    _queue: dispatch2::DispatchRetained<DispatchQueue>,
    shared: Arc<Shared>,
    size: (u32, u32),
    stopped: bool,
}

// SAFETY: como `stream::macos::Stream`: SCStream no es MainThreadOnly y parar se puede
// llamar desde cualquier hilo; lo compartido va con candado.
unsafe impl Send for ScreenRecorder {}

impl ScreenRecorder {
    /// Empieza a grabar `region` (en puntos, coordenadas globales con el origen arriba a
    /// la izquierda, como CoreGraphics) a `path` (MP4). Con `exclude_self`, las ventanas
    /// de esta app no salen en el video.
    pub fn start(region: Rect, path: &Path, exclude_self: bool, show_cursor: bool, stamp: Stamp) -> Result<Self, String> {
        if !available!(macos = 13.0) {
            return Err("ScreenCaptureKit pide macOS 13".into());
        }
        // SAFETY: consulta pura de TCC.
        if unsafe { CGPreflightScreenCaptureAccess() } == 0 {
            return Err("falta el permiso de Grabación de pantalla".into());
        }
        let (display_id, display_bounds, scale) = display_for(region).ok_or("la zona no está en ninguna pantalla")?;
        let content = super::macos::shareable_content()?;
        // SAFETY: getters de SCK sobre contenido retenido.
        let display = unsafe {
            let displays = content.displays();
            (0..displays.count()).map(|i| displays.objectAtIndex(i)).find(|d| d.displayID() == display_id)
        }
        .ok_or("ScreenCaptureKit no ve la pantalla de la zona")?;

        // La zona dentro de su pantalla, recortada a ella.
        let x0 = (region.x as f64).max(display_bounds.origin.x);
        let y0 = (region.y as f64).max(display_bounds.origin.y);
        let x1 = (region.right() as f64).min(display_bounds.origin.x + display_bounds.size.width);
        let y1 = (region.bottom() as f64).min(display_bounds.origin.y + display_bounds.size.height);
        if x1 - x0 < 2.0 || y1 - y0 < 2.0 {
            return Err("la zona es muy chica".into());
        }
        let source = CGRect {
            origin: CGPoint { x: x0 - display_bounds.origin.x, y: y0 - display_bounds.origin.y },
            size: CGSize { width: x1 - x0, height: y1 - y0 },
        };
        let width = (((x1 - x0) * scale).round() as u32).max(2) & !1;
        let height = (((y1 - y0) * scale).round() as u32).max(2) & !1;

        // HEVC por hardware en Apple Silicon: la mitad de bits a igual calidad para el
        // máster que después se edita. En Intel se sigue con H.264 (no todos tienen
        // codificador HEVC, y quedarse sin grabación es peor que un archivo más grande).
        // Si el escritor no acepta HEVC, se cae a H.264 sin tocar la toma.
        let hevc = cfg!(target_arch = "aarch64");
        let writer = match create_writer(path, width, height, hevc) {
            Ok(writer) => writer,
            Err(error) if hevc => {
                tracing::warn!(%error, "grabar: sin HEVC, se graba en H.264");
                let _ = std::fs::remove_file(path);
                create_writer(path, width, height, false)?
            }
            Err(error) => return Err(error),
        };
        let shared = Arc::new(Shared { writer: Mutex::new(writer), stamp });

        // SAFETY: SCK retiene lo que necesita de la pantalla, las apps y la config.
        let filter = unsafe {
            let apps = content.applications();
            let own = std::process::id() as i32;
            let mine: Vec<_> = if exclude_self {
                (0..apps.count())
                    .map(|i| apps.objectAtIndex(i))
                    .filter(|app| {
                        let pid: i32 = msg_send![&**app, processID];
                        pid == own
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let refs: Vec<&_> = mine.iter().map(|app| &**app).collect();
            let apps = NSArray::from_slice(&refs);
            SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), &display, &apps, &NSArray::new())
        };
        let config = unsafe { SCStreamConfiguration::new() };
        // SAFETY: setters de una config recién creada.
        unsafe {
            let _: () = msg_send![&config, setSourceRect: source];
            config.setWidth(width as usize);
            config.setHeight(height as usize);
            config.setPixelFormat(NV12);
            config.setMinimumFrameInterval(CMTime { value: 1, timescale: FPS, flags: CMTimeFlags::Valid, epoch: 0 });
            config.setShowsCursor(show_cursor);
            config.setQueueDepth(5);
        }
        let queue = DispatchQueue::new("com.ciat.atic.screen-record", None);
        let handler = RecorderOutput::new(OutputIvars { shared: shared.clone() });
        let stream = unsafe { SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, Some(ProtocolObject::from_ref(&*handler))) };
        // SAFETY: el output queda retenido por el stream y por `handler`.
        unsafe {
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(ProtocolObject::from_ref(&*handler), SCStreamOutputType::Screen, Some(&queue))
                .map_err(|e| e.localizedDescription().to_string())?;
        }
        let (started_tx, started_rx) = mpsc::channel::<Option<String>>();
        let started = RcBlock::new(move |error: *mut NSError| {
            let _ = started_tx.send(unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string()));
        });
        // SAFETY: el bloque vive hasta que SCK lo copia.
        unsafe { stream.startCaptureWithCompletionHandler(Some(&started)) };
        let recorder = Self { stream, output: handler, _queue: queue, shared, size: (width, height), stopped: false };
        match started_rx.recv_timeout(Duration::from_secs(6)) {
            Ok(None) => {}
            Ok(Some(error)) => return Err(format!("ScreenCaptureKit no empezó: {error}")),
            Err(_) => return Err("ScreenCaptureKit no respondió".into()),
        }
        tracing::info!(ancho = width, alto = height, "grabar: ScreenCaptureKit y AVAssetWriter");
        Ok(recorder)
    }

    /// Las medidas del video.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    fn stop_capture(&mut self) {
        if std::mem::replace(&mut self.stopped, true) {
            return;
        }
        // SAFETY: parar y dar de baja el output; después ya no llegan cuadros.
        unsafe {
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let done = RcBlock::new(move |_: *mut NSError| {
                let _ = done_tx.send(());
            });
            self.stream.stopCaptureWithCompletionHandler(Some(&done));
            let _ = done_rx.recv_timeout(Duration::from_secs(2));
            let _ = self.stream.removeStreamOutput_type_error(ProtocolObject::from_ref(&*self.output), SCStreamOutputType::Screen);
        }
    }

    /// Para y cierra el MP4, que dura `duration` (el último cuadro se estira hasta ahí).
    /// `Ok(false)` si no alcanzó a llegar ningún cuadro (no queda archivo).
    pub fn finish(mut self, duration: Duration) -> Result<bool, String> {
        self.stop_capture();
        let mut w = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        w.closed = true;
        if w.frames == 0 {
            // SAFETY: cancelar un escritor vivo.
            unsafe {
                let _: () = msg_send![&*w.writer.0, cancelWriting];
            }
            return Ok(false);
        }
        let end = duration.max(w.last.unwrap_or_default() + Duration::from_millis(1));
        // SAFETY: el último cuadro sigue retenido; cerrar la entrada y el escritor y
        // esperar a que termine.
        unsafe {
            // Repetido justo antes del final: el último tramo quieto queda en el video.
            let tail = end.saturating_sub(Duration::from_millis(1));
            if let Some(pixels) = w.last_pixels.take().filter(|_| w.last.is_some_and(|last| tail > last + Duration::from_millis(20))) {
                let ready: Bool = msg_send![&*w.input.0, isReadyForMoreMediaData];
                if ready.as_bool() {
                    let _: Bool = msg_send![&*w.adaptor.0, appendPixelBuffer: pixels.0 as *const CVBuffer, withPresentationTime: time(tail)];
                }
            }
            let _: () = msg_send![&*w.input.0, markAsFinished];
            let _: () = msg_send![&*w.writer.0, endSessionAtSourceTime: time(end)];
            finish_writing(&w.writer.0)?;
        }
        Ok(true)
    }
}

impl Drop for ScreenRecorder {
    fn drop(&mut self) {
        self.stop_capture();
        let mut w = self.shared.writer.lock().unwrap_or_else(|e| e.into_inner());
        if !w.closed {
            w.closed = true;
            // SAFETY: cancelar un escritor vivo (borra el archivo a medias).
            unsafe {
                let _: () = msg_send![&*w.writer.0, cancelWriting];
            }
        }
    }
}

/// La pantalla que contiene el centro de `region`: su id, sus límites en puntos y su
/// escala (píxeles por punto).
fn display_for(region: Rect) -> Option<(u32, CGRect, f64)> {
    use core_graphics::display::CGDisplay;
    let cx = region.x as f64 + region.width as f64 / 2.0;
    let cy = region.y as f64 + region.height as f64 / 2.0;
    let ids = CGDisplay::active_displays().ok()?;
    let pick = |id: &u32| {
        let b = CGDisplay::new(*id).bounds();
        cx >= b.origin.x && cx < b.origin.x + b.size.width && cy >= b.origin.y && cy < b.origin.y + b.size.height
    };
    let id = ids.iter().find(|id| pick(id)).or_else(|| ids.first()).copied()?;
    let display = CGDisplay::new(id);
    let b = display.bounds();
    let scale = match display.display_mode() {
        Some(mode) if b.size.width > 0.0 => mode.pixel_width() as f64 / b.size.width,
        _ => 1.0,
    };
    let bounds = CGRect { origin: CGPoint { x: b.origin.x, y: b.origin.y }, size: CGSize { width: b.size.width, height: b.size.height } };
    Some((id, bounds, scale.max(1.0)))
}

fn bitrate(width: u32, height: u32, hevc: bool) -> i64 {
    // Como el de Windows (`win32/screen_record.rs`), a 30 cuadros de referencia: la
    // pantalla casi siempre está quieta y el códec casi no gasta en lo que no cambia.
    // HEVC lleva ~60 % de los bits para la misma calidad (igual que al exportar).
    let codec = if hevc { 0.6 } else { 1.0 };
    (width as f64 * height as f64 * 30.0 * 0.12 * codec).clamp(2_000_000.0, 40_000_000.0) as i64
}

fn create_writer(path: &Path, width: u32, height: u32, hevc: bool) -> Result<Writer, String> {
    let url = file_url(path)?;
    let mut compression = vec![
        (key(unsafe { AVVideoAverageBitRateKey }), CFNumber::from(bitrate(width, height, hevc)).as_CFType()),
        (key(unsafe { AVVideoExpectedSourceFrameRateKey }), CFNumber::from(FPS).as_CFType()),
        (key(unsafe { AVVideoMaxKeyFrameIntervalDurationKey }), CFNumber::from(2.0f64).as_CFType()),
        (key(unsafe { AVVideoAllowFrameReorderingKey }), CFBoolean::false_value().as_CFType()),
    ];
    // El perfil de H.264 no vale para HEVC: allá el codificador elige.
    if !hevc {
        compression.push((key(unsafe { AVVideoProfileLevelKey }), key(unsafe { AVVideoProfileLevelH264HighAutoLevel }).as_CFType()));
    }
    let compression = dict(&compression);
    let codec = if hevc { key(unsafe { AVVideoCodecTypeHEVC }) } else { key(unsafe { AVVideoCodecTypeH264 }) };
    let settings = dict(&[
        (key(unsafe { AVVideoCodecKey }), codec.as_CFType()),
        (key(unsafe { AVVideoWidthKey }), CFNumber::from(width as i32).as_CFType()),
        (key(unsafe { AVVideoHeightKey }), CFNumber::from(height as i32).as_CFType()),
        (key(unsafe { AVVideoCompressionPropertiesKey }), compression.as_CFType()),
    ]);
    // SAFETY: objetos de AVFoundation creados y configurados aquí.
    unsafe {
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let writer: Option<Retained<AnyObject>> =
            msg_send![class(c"AVAssetWriter")?, assetWriterWithURL: &*url, fileType: ns(AVFileTypeMPEG4), error: &mut error];
        let writer = writer.ok_or_else(|| format!("AVAssetWriter: {}", error_text(error)))?;
        let input: Retained<AnyObject> =
            msg_send![class(c"AVAssetWriterInput")?, assetWriterInputWithMediaType: ns(AVMediaTypeVideo), outputSettings: as_object(&settings)];
        let _: () = msg_send![&input, setExpectsMediaDataInRealTime: Bool::YES];
        let can: Bool = msg_send![&writer, canAddInput: &*input];
        if !can.as_bool() {
            return Err("AVAssetWriter no acepta el video".into());
        }
        let _: () = msg_send![&writer, addInput: &*input];
        let none: *const AnyObject = std::ptr::null();
        let adaptor: Retained<AnyObject> = msg_send![
            class(c"AVAssetWriterInputPixelBufferAdaptor")?,
            assetWriterInputPixelBufferAdaptorWithAssetWriterInput: &*input,
            sourcePixelBufferAttributes: none
        ];
        let started: Bool = msg_send![&writer, startWriting];
        if !started.as_bool() {
            let error: *mut AnyObject = msg_send![&writer, error];
            return Err(format!("AVAssetWriter no empezó: {}", error_text(error)));
        }
        let zero = CMTime { value: 0, timescale: 1_000_000, flags: CMTimeFlags::Valid, epoch: 0 };
        let _: () = msg_send![&writer, startSessionAtSourceTime: zero];
        Ok(Writer { writer: Obj(writer), input: Obj(input), adaptor: Obj(adaptor), last: None, last_pixels: None, frames: 0, closed: false })
    }
}

/// `finishWritingWithCompletionHandler:` y espera.
unsafe fn finish_writing(writer: &AnyObject) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<()>();
    let done = RcBlock::new(move || {
        let _ = tx.send(());
    });
    // SAFETY: el bloque vive hasta que AVFoundation lo copia.
    let _: () = unsafe { msg_send![writer, finishWritingWithCompletionHandler: &*done] };
    rx.recv_timeout(Duration::from_secs(120)).map_err(|_| "AVAssetWriter no terminó".to_string())?;
    let status: isize = unsafe { msg_send![writer, status] };
    if status != WRITER_COMPLETED {
        let error: *mut AnyObject = unsafe { msg_send![writer, error] };
        return Err(format!("AVAssetWriter terminó mal ({status}): {}", error_text(error)));
    }
    Ok(())
}

/// Bloques de audio de 100 ms.
fn audio_block(rate: u32) -> u64 {
    rate as u64 / 10
}

/// `video` (el MP4 de [`ScreenRecorder`]) y `pcm` (`frames` cuadros de 16 bits estéreo
/// intercalados a `rate`, sin cabecera) en `out`, con el audio en AAC. La imagen se copia
/// tal cual. Las dos pistas se escriben intercaladas por tiempo, como pide AVAssetWriter.
///
/// Si el escritor se traba, se reintenta una vez desde cero (un tropiezo del
/// equipo suele pasar a la segunda); los demás errores no mejoran repitiendo y salen tal cual.
pub fn mux_pcm(video: &Path, pcm: &Path, frames: u64, rate: u32, out: &Path) -> Result<(), String> {
    let attempt = || objc2::rc::autoreleasepool(|_| unsafe { mux_inner(video, pcm, frames, rate, out) });
    match attempt() {
        Err(first) if is_stall(&first) => {
            tracing::warn!(error = %first, "grabar: el escritor se trabó al juntar el audio; se reintenta");
            std::thread::sleep(Duration::from_secs(1));
            let _ = std::fs::remove_file(out);
            attempt().map_err(|second| format!("{second} (primer intento: {first})"))
        }
        result => result,
    }
}

/// El error de `mux_inner` es un atasco del escritor (vale reintentar), no un problema
/// del video o del audio.
fn is_stall(error: &str) -> bool {
    error.starts_with("AVAssetWriter se trabó")
}

/// SAFETY: objetos de AVFoundation y CoreMedia creados y usados aquí; cada muestra
/// copiada del lector se suelta después de escribirla.
unsafe fn mux_inner(video: &Path, pcm: &Path, frames: u64, rate: u32, out: &Path) -> Result<(), String> {
    const FRAME_BYTES: usize = 4;
    let mut raw = BufReader::new(File::open(pcm).map_err(|e| e.to_string())?);
    let none: *const AnyObject = std::ptr::null();

    // El video, tal cual.
    let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*file_url(video)?, options: none];
    let tracks: Retained<NSArray<AnyObject>> = msg_send![&asset, tracksWithMediaType: ns(AVMediaTypeVideo)];
    let track = tracks.firstObject().ok_or("el video no tiene imagen")?;
    let formats: Retained<NSArray<AnyObject>> = msg_send![&track, formatDescriptions];
    let format = formats.firstObject().ok_or("el video no tiene formato")?;
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let reader: Option<Retained<AnyObject>> = msg_send![class(c"AVAssetReader")?, assetReaderWithAsset: &*asset, error: &mut error];
    let reader = reader.ok_or_else(|| format!("AVAssetReader: {}", error_text(error)))?;
    let reader_out: Retained<AnyObject> = msg_send![class(c"AVAssetReaderTrackOutput")?, assetReaderTrackOutputWithTrack: &*track, outputSettings: none];
    let _: () = msg_send![&reader_out, setAlwaysCopiesSampleData: Bool::NO];
    let _: () = msg_send![&reader, addOutput: &*reader_out];

    let _ = std::fs::remove_file(out);
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let writer: Option<Retained<AnyObject>> =
        msg_send![class(c"AVAssetWriter")?, assetWriterWithURL: &*file_url(out)?, fileType: ns(AVFileTypeMPEG4), error: &mut error];
    let writer = writer.ok_or_else(|| format!("AVAssetWriter: {}", error_text(error)))?;
    let format_hint = Retained::as_ptr(&format) as *const FormatDescription;
    let video_in: Retained<AnyObject> = msg_send![
        class(c"AVAssetWriterInput")?,
        assetWriterInputWithMediaType: ns(AVMediaTypeVideo),
        outputSettings: none,
        sourceFormatHint: format_hint
    ];

    // El audio: entra PCM y sale AAC.
    let format_guard = pcm_format(rate)?;
    let pcm_format = format_guard.as_ptr();
    let aac = dict(&[
        (key(AVFormatIDKey), CFNumber::from(AAC as i32).as_CFType()),
        (key(AVSampleRateKey), CFNumber::from(rate as f64).as_CFType()),
        (key(AVNumberOfChannelsKey), CFNumber::from(2i32).as_CFType()),
        (key(AVEncoderBitRateKey), CFNumber::from(160_000i32).as_CFType()),
    ]);
    let audio_in: Retained<AnyObject> = msg_send![
        class(c"AVAssetWriterInput")?,
        assetWriterInputWithMediaType: ns(AVMediaTypeAudio),
        outputSettings: as_object(&aac),
        sourceFormatHint: pcm_format
    ];
    for input in [&video_in, &audio_in] {
        let _: () = msg_send![&**input, setExpectsMediaDataInRealTime: Bool::NO];
        let can: Bool = msg_send![&writer, canAddInput: &**input];
        if !can.as_bool() {
            return Err("AVAssetWriter no acepta las pistas".into());
        }
        let _: () = msg_send![&writer, addInput: &**input];
    }

    let reading: Bool = msg_send![&reader, startReading];
    if !reading.as_bool() {
        let error: *mut AnyObject = msg_send![&reader, error];
        return Err(format!("AVAssetReader no empezó: {}", error_text(error)));
    }
    let started: Bool = msg_send![&writer, startWriting];
    if !started.as_bool() {
        let error: *mut AnyObject = msg_send![&writer, error];
        return Err(format!("AVAssetWriter no empezó: {}", error_text(error)));
    }
    let zero = CMTime { value: 0, timescale: rate as i32, flags: CMTimeFlags::Valid, epoch: 0 };
    let _: () = msg_send![&writer, startSessionAtSourceTime: zero];

    // Un lector que falló no es fin de archivo: si se confunden, el MP4 sale
    // truncado y se guarda como una grabación completa.
    let reader_failed = || -> String {
        let error: *mut AnyObject = msg_send![&reader, error];
        format!("el lector del video falló: {}", error_text(error))
    };
    let next_video = || -> Result<Option<(*mut CMSampleBuffer, f64)>, String> {
        // Un `null` con el lector todavía leyendo no siempre es el final (el estado tarda
        // un instante en cambiar): se espera un poco antes de darlo por terminado. Uno
        // que falló sí es un error: si no, el MP4 sale truncado y se guarda como bueno.
        for _ in 0..100 {
            let status: isize = msg_send![&reader, status];
            if status == READER_FAILED {
                return Err(reader_failed());
            }
            if status != READER_READING {
                return Ok(None);
            }
            let sample: *mut CMSampleBuffer = msg_send![&reader_out, copyNextSampleBuffer];
            if !sample.is_null() {
                let pts = CMSampleBufferGetPresentationTimeStamp(sample);
                let secs = if pts.timescale > 0 { pts.value as f64 / pts.timescale as f64 } else { 0.0 };
                return Ok(Some((sample, secs)));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err("el lector del video no terminó".into())
    };
    let writer_error = || -> String {
        let error: *mut AnyObject = msg_send![&writer, error];
        error_text(error)
    };
    let ready = |input: &AnyObject| -> bool {
        let ready: Bool = msg_send![input, isReadyForMoreMediaData];
        ready.as_bool()
    };

    let block = audio_block(rate);
    let mut bytes = vec![0u8; block as usize * FRAME_BYTES];
    let mut written = 0u64;
    let mut pending = next_video()?;
    let (mut video_done, mut audio_done) = (false, false);
    // Se escribe lo que el escritor pida: él intercala y frena la pista que se
    // adelanta. Esperar una pista en particular lo trababa.
    let result = (|| -> Result<(), String> {
        let mut idle = Instant::now();
        while !(video_done && audio_done) {
            let mut progress = false;
            if !video_done && ready(&video_in) {
                match pending.take() {
                    Some((sample, _)) => {
                        let _released = Released(sample as *const c_void);
                        let ok: Bool = msg_send![&video_in, appendSampleBuffer: &*sample];
                        if !ok.as_bool() {
                            return Err(format!("no se pudo copiar el video: {}", writer_error()));
                        }
                        pending = next_video()?;
                    }
                    None => {
                        let _: () = msg_send![&video_in, markAsFinished];
                        video_done = true;
                    }
                }
                progress = true;
            }
            if !audio_done && ready(&audio_in) {
                if written < frames {
                    let n = (frames - written).min(block);
                    let chunk = &mut bytes[..n as usize * FRAME_BYTES];
                    let got = read_up_to(&mut raw, chunk).map_err(|e| e.to_string())?;
                    chunk[got..].fill(0);
                    let pts = CMTime { value: written as i64, timescale: rate as i32, flags: CMTimeFlags::Valid, epoch: 0 };
                    let sample = pcm_sample(chunk, n, pcm_format, pts)?;
                    let _released = Released(sample as *const c_void);
                    let ok: Bool = msg_send![&audio_in, appendSampleBuffer: &*sample];
                    if !ok.as_bool() {
                        return Err(format!("no se pudo escribir el audio: {}", writer_error()));
                    }
                    written += n;
                } else {
                    let _: () = msg_send![&audio_in, markAsFinished];
                    audio_done = true;
                }
                progress = true;
            }
            if progress {
                idle = Instant::now();
                continue;
            }
            let status: isize = msg_send![&writer, status];
            if status != 1 || idle.elapsed() > MUX_IDLE {
                // Con el texto del error vacío (lo normal en un atasco) esto es lo único
                // que dice dónde quedó.
                return Err(format!(
                    "AVAssetWriter se trabó (estado {status}, sin avance {} s, video {}, audio {} de {frames} cuadros): {}",
                    idle.elapsed().as_secs(),
                    if video_done { "listo" } else { "pendiente" },
                    written,
                    writer_error()
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    if let Some((sample, _)) = pending {
        CFRelease(sample as *const c_void);
    }
    if let Err(error) = result {
        let _: () = msg_send![&reader, cancelReading];
        let _: () = msg_send![&writer, cancelWriting];
        return Err(error);
    }
    finish_writing(&writer)
}

/// La descripción de PCM de 16 bits estéreo intercalado a `rate` (la que entra al
/// codificador de AAC).
pub(super) struct PcmFormat(*const FormatDescription);

impl PcmFormat {
    pub(super) fn as_ptr(&self) -> *const FormatDescription {
        self.0
    }
}

impl Drop for PcmFormat {
    fn drop(&mut self) {
        // SAFETY: se creó con +1 y se suelta una vez.
        unsafe { CFRelease(self.0.cast()) };
    }
}

pub(super) fn pcm_format(rate: u32) -> Result<PcmFormat, String> {
    const FRAME_BYTES: u32 = 4;
    let asbd = AudioStreamBasicDescription {
        sample_rate: rate as f64,
        format_id: LPCM,
        format_flags: PCM_FLAGS,
        bytes_per_packet: FRAME_BYTES,
        frames_per_packet: 1,
        bytes_per_frame: FRAME_BYTES,
        channels_per_frame: 2,
        bits_per_channel: 16,
        reserved: 0,
    };
    let mut format: *const FormatDescription = std::ptr::null();
    // SAFETY: describe un formato de PCM; la descripción sale con +1.
    let status = unsafe {
        CMAudioFormatDescriptionCreate(std::ptr::null(), &asbd, 0, std::ptr::null(), 0, std::ptr::null(), std::ptr::null(), &mut format)
    };
    if status != 0 || format.is_null() {
        return Err(format!("CoreMedia no describió el audio ({status})"));
    }
    Ok(PcmFormat(format))
}

/// Una muestra de audio con una copia de `data` (`n` cuadros) en `pts`.
pub(super) unsafe fn pcm_sample(data: &[u8], n: u64, format: *const FormatDescription, pts: CMTime) -> Result<*mut CMSampleBuffer, String> {
    let mut block: *const c_void = std::ptr::null();
    let status = CMBlockBufferCreateWithMemoryBlock(
        std::ptr::null(),
        std::ptr::null_mut(),
        data.len(),
        std::ptr::null(),
        std::ptr::null(),
        0,
        data.len(),
        0,
        &mut block,
    );
    if status != 0 || block.is_null() {
        return Err(format!("CoreMedia no armó el bloque de audio ({status})"));
    }
    let _block = Released(block);
    let status = CMBlockBufferReplaceDataBytes(data.as_ptr().cast(), block, 0, data.len());
    if status != 0 {
        return Err(format!("CoreMedia no copió el audio ({status})"));
    }
    let mut sample: *mut CMSampleBuffer = std::ptr::null_mut();
    let status = CMAudioSampleBufferCreateReadyWithPacketDescriptions(std::ptr::null(), block, format, n as isize, pts, std::ptr::null(), &mut sample);
    if status != 0 || sample.is_null() {
        return Err(format!("CoreMedia no armó la muestra de audio ({status})"));
    }
    Ok(sample)
}

/// Suelta un objeto de CoreFoundation retenido al salir del alcance.
struct Released(*const c_void);
// SAFETY: soltar un objeto de CoreFoundation retenido vale desde cualquier hilo.
unsafe impl Send for Released {}

impl Drop for Released {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: se creó o copió con +1 y se suelta una vez.
            unsafe { CFRelease(self.0) };
        }
    }
}

/// Lee hasta llenar `buf` o llegar al final; devuelve cuánto leyó.
fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
#[test]
fn solo_un_atasco_del_escritor_se_reintenta() {
    assert!(is_stall("AVAssetWriter se trabó (estado 1, sin avance 30 s, video listo, audio 10 de 20 cuadros): "));
    assert!(!is_stall("AVAssetWriter no terminó"));
    assert!(!is_stall("el video no tiene imagen"));
    assert!(!is_stall("no se pudo escribir el audio: disco lleno"));
    assert!(!is_stall("AVAssetWriter terminó mal (3): x"));
}

/// `ATIC_TEST_RECORD=1`: graba 2 s de verdad (pide el permiso de Grabación de pantalla
/// para la terminal) y le junta un tono de 440 Hz.
#[cfg(test)]
#[test]
fn graba_y_junta_de_verdad() {
    if std::env::var("ATIC_TEST_RECORD").is_err() {
        return;
    }
    let dir = std::env::temp_dir().join("atic-grabar-prueba");
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("video.mp4");
    let started = Instant::now();
    let recorder = ScreenRecorder::start(Rect::new(0, 0, 800, 500), &video, true, true, Box::new(move || Some(started.elapsed()))).expect("empieza");
    std::thread::sleep(Duration::from_secs(6));
    assert!(recorder.finish(started.elapsed()).expect("cierra"));
    let rate = 48_000u32;
    let frames = rate as u64 * 6;
    let mut pcm = Vec::new();
    for i in 0..frames {
        let v = ((i as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin() * 8000.0) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let raw = dir.join("audio.raw");
    std::fs::write(&raw, pcm).unwrap();
    let out = dir.join("final.mp4");
    mux_pcm(&video, &raw, frames, rate, &out).expect("junta");
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
    eprintln!("{}", out.display());
}
