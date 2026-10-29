//! ScreenCaptureKit → VideoToolbox: la pantalla en vivo a H.264 sin tocar los píxeles.
//!
//! SCK entrega cada cuadro como un `CVPixelBuffer` en NV12, ya escalado por la GPU y
//! solo cuando algo cambió (con la pantalla quieta no llega nada). Ese mismo búfer va
//! al codificador de hardware, que entrega H.264 en AVCC desde su propio hilo: aquí se
//! pasa a Annex-B, con SPS/PPS antes de cada keyframe.
//!
//! VideoToolbox y CoreMedia van por FFI directo (son pocas funciones de C); SCK, por
//! `objc2-screen-capture-kit`, como el audio del sistema en `atic-audio`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use block2::RcBlock;
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{available, define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput, SCStreamOutputType};

use super::{Output, Ready};
use crate::h264::{fit_even, out};

type Session = *mut c_void;
type SampleRef = *const c_void;

/// `kCMVideoCodecType_H264` ('avc1').
const CODEC_H264: u32 = u32::from_be_bytes(*b"avc1");
/// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` ('420v'): lo que mejor come el
/// codificador, sin convertir.
const NV12: u32 = u32::from_be_bytes(*b"420v");

type OutputCallback = extern "C" fn(refcon: *mut c_void, frame_refcon: *mut c_void, status: i32, flags: u32, sample: SampleRef);

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    fn VTCompressionSessionCreate(
        allocator: CFTypeRef,
        width: i32,
        height: i32,
        codec: u32,
        encoder_specification: CFDictionaryRef,
        source_attributes: CFDictionaryRef,
        compressed_allocator: CFTypeRef,
        callback: OutputCallback,
        refcon: *mut c_void,
        session: *mut Session,
    ) -> i32;
    fn VTSessionSetProperty(session: Session, key: CFStringRef, value: CFTypeRef) -> i32;
    fn VTCompressionSessionPrepareToEncodeFrames(session: Session) -> i32;
    fn VTCompressionSessionEncodeFrame(
        session: Session,
        image: *const c_void,
        pts: CMTime,
        duration: CMTime,
        properties: CFDictionaryRef,
        frame_refcon: *mut c_void,
        info: *mut u32,
    ) -> i32;
    fn VTCompressionSessionCompleteFrames(session: Session, until: CMTime) -> i32;
    fn VTCompressionSessionInvalidate(session: Session);

    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    static kVTProfileLevel_H264_Main_AutoLevel: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    static kVTCompressionPropertyKey_DataRateLimits: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration: CFStringRef;
    static kVTCompressionPropertyKey_ExpectedFrameRate: CFStringRef;
    static kVTEncodeFrameOptionKey_ForceKeyFrame: CFStringRef;
    static kVTVideoEncoderSpecification_EnableLowLatencyRateControl: CFStringRef;
    static kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: CFStringRef;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sample: SampleRef) -> *const c_void;
    fn CMSampleBufferGetDataBuffer(sample: SampleRef) -> *const c_void;
    fn CMSampleBufferGetFormatDescription(sample: SampleRef) -> *const c_void;
    fn CMSampleBufferGetSampleAttachmentsArray(sample: SampleRef, create: u8) -> CFArrayRef;
    fn CMBlockBufferGetDataLength(block: *const c_void) -> usize;
    fn CMBlockBufferCopyDataBytes(block: *const c_void, offset: usize, length: usize, destination: *mut c_void) -> i32;
    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        description: *const c_void,
        index: usize,
        set: *mut *const u8,
        size: *mut usize,
        count: *mut usize,
        header_length: *mut i32,
    ) -> i32;
    static kCMSampleAttachmentKey_NotSync: CFStringRef;
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

/// Un keyframe cada tanto aunque nadie lo pida (si algo se perdió sin aviso).
const KEYFRAME_SECS: i32 = 10;
/// Con la pantalla quieta SCK no entrega nada: un keyframe pedido sale con el último
/// cuadro si no llega otro en este rato.
const KEY_FROM_LAST: Duration = Duration::from_millis(100);

/// Un `CVPixelBuffer` retenido.
struct Pixels(*const c_void);
// SAFETY: un CVPixelBuffer retenido se puede soltar y leer desde cualquier hilo; aquí
// solo se pasa al codificador, que lo retiene por su cuenta.
unsafe impl Send for Pixels {}

impl Drop for Pixels {
    fn drop(&mut self) {
        // SAFETY: se retuvo al crearlo.
        unsafe { CFRelease(self.0) };
    }
}

/// La sesión del codificador. Codificar va siempre con el candado tomado (SCK y quien
/// pide un keyframe llegan de hilos distintos).
struct Encoder(Session);
// SAFETY: la sesión de VideoToolbox se puede usar desde cualquier hilo si las llamadas
// no se pisan: el `Mutex` de `Shared` lo asegura.
unsafe impl Send for Encoder {}

struct Shared {
    encoder: Mutex<Encoder>,
    /// El último cuadro que entregó SCK, para un keyframe con la pantalla quieta.
    last: Mutex<Option<Pixels>>,
    force_key: AtomicBool,
    /// Cuándo se pidió el keyframe que todavía no sale (ms desde `started`; 0: ninguno).
    key_asked_ms: AtomicU32,
    started: Instant,
    output: Output,
    ready: Ready,
    cap: u32,
}

impl Shared {
    fn now_ms(&self) -> u32 {
        (self.started.elapsed().as_millis() as u32).max(1)
    }

    /// Codifica un cuadro (con keyframe si se pidió).
    fn encode(&self, pixels: *const c_void) {
        let encoder = self.encoder.lock().unwrap_or_else(|e| e.into_inner());
        // Ya se soltó el stream (un último cuadro o keyframe que llega tarde).
        if encoder.0.is_null() {
            return;
        }
        let force = self.force_key.swap(false, Ordering::Relaxed);
        let properties = force.then(|| {
            // SAFETY: constante de VideoToolbox, válida mientras viva el proceso.
            let key = unsafe { CFString::wrap_under_get_rule(kVTEncodeFrameOptionKey_ForceKeyFrame) };
            CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())])
        });
        let pts = CMTime { value: self.started.elapsed().as_micros() as i64, timescale: 1_000_000, flags: CMTimeFlags::Valid, epoch: 0 };
        let duration = CMTime { value: 0, timescale: 0, flags: CMTimeFlags(0), epoch: 0 };
        // SAFETY: la sesión vive mientras viva `Shared`; el búfer lo retiene el
        // codificador mientras lo necesite.
        let status = unsafe {
            VTCompressionSessionEncodeFrame(
                encoder.0,
                pixels,
                pts,
                duration,
                properties.as_ref().map_or(std::ptr::null(), |p| p.as_concrete_TypeRef()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != 0 {
            tracing::debug!(status, "VideoToolbox no aceptó el cuadro");
            if force {
                self.force_key.store(true, Ordering::Relaxed);
            }
        } else if force {
            self.key_asked_ms.store(0, Ordering::Relaxed);
        }
    }

    /// Lo que entrega el codificador, ya en Annex-B.
    fn deliver(&self, sample: SampleRef) {
        // SAFETY: getters de CoreMedia sobre una muestra viva durante el callback.
        unsafe {
            let block = CMSampleBufferGetDataBuffer(sample);
            if block.is_null() {
                return;
            }
            let key = is_sync(sample);
            let mut data = Vec::with_capacity(CMBlockBufferGetDataLength(block) + 64);
            if key && !parameter_sets(CMSampleBufferGetFormatDescription(sample), &mut data) {
                tracing::debug!("keyframe sin SPS/PPS");
                return;
            }
            let length = CMBlockBufferGetDataLength(block);
            let mut avcc = vec![0u8; length];
            if CMBlockBufferCopyDataBytes(block, 0, length, avcc.as_mut_ptr().cast()) != 0 || !super::avcc_to_annex_b(&avcc, &mut data) {
                tracing::debug!("cuadro de VideoToolbox ilegible");
                return;
            }
            (self.output)(if key { out::KEY | out::CONFIG } else { 0 }, data);
        }
    }
}

/// `true` si la muestra es un keyframe (no trae `NotSync`).
unsafe fn is_sync(sample: SampleRef) -> bool {
    let attachments = unsafe { CMSampleBufferGetSampleAttachmentsArray(sample, 0) };
    if attachments.is_null() {
        return true;
    }
    // SAFETY: el arreglo es de la muestra (+0) y vive durante el callback.
    let attachments: CFArray<CFType> = unsafe { CFArray::wrap_under_get_rule(attachments) };
    let Some(first) = attachments.get(0) else { return true };
    let Some(first) = first.downcast::<CFDictionary>() else { return true };
    // SAFETY: constante de CoreMedia.
    let not_sync = unsafe { CFString::wrap_under_get_rule(kCMSampleAttachmentKey_NotSync) };
    match first.find(not_sync.as_concrete_TypeRef().cast()) {
        Some(value) => {
            // SAFETY: el valor es un CFBoolean de la muestra.
            let value = unsafe { CFBoolean::wrap_under_get_rule(*value as _) };
            !bool::from(value)
        }
        None => true,
    }
}

/// SPS y PPS de la descripción, en Annex-B, detrás de `out`.
unsafe fn parameter_sets(description: *const c_void, out: &mut Vec<u8>) -> bool {
    if description.is_null() {
        return false;
    }
    let mut count = 0usize;
    let mut index = 0;
    loop {
        let mut set: *const u8 = std::ptr::null();
        let mut size = 0usize;
        // SAFETY: lectura de la descripción viva; `set` apunta dentro de ella.
        let status = unsafe { CMVideoFormatDescriptionGetH264ParameterSetAtIndex(description, index, &mut set, &mut size, &mut count, std::ptr::null_mut()) };
        if status != 0 || set.is_null() {
            return index > 0;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unsafe { std::slice::from_raw_parts(set, size) });
        index += 1;
        if index >= count {
            return true;
        }
    }
}

extern "C" fn encoded(refcon: *mut c_void, _frame: *mut c_void, status: i32, _flags: u32, sample: SampleRef) {
    if status != 0 || sample.is_null() {
        if status != 0 {
            tracing::debug!(status, "VideoToolbox no codificó un cuadro");
        }
        return;
    }
    // SAFETY: `refcon` es el `Shared` del stream, que vive hasta después de invalidar
    // la sesión (y con ella, los callbacks).
    let shared = unsafe { &*(refcon as *const Shared) };
    shared.deliver(sample);
}

struct OutputIvars {
    shared: Arc<Shared>,
}

define_class!(
    // SAFETY: NSObject no impone requisitos extra y la clase no implementa Drop.
    #[unsafe(super(NSObject))]
    #[name = "AticScreenStreamOutput"]
    #[ivars = OutputIvars]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_did_output_sample_buffer_of_type(&self, _stream: &SCStream, sample_buffer: &CMSampleBuffer, r#type: SCStreamOutputType) {
            if r#type != SCStreamOutputType::Screen {
                return;
            }
            let shared = &self.ivars().shared;
            let sample = sample_buffer as *const CMSampleBuffer as SampleRef;
            // SAFETY: la muestra vive durante el callback; sin imagen es un cuadro
            // «quieto» de SCK (nada cambió).
            let pixels = unsafe { CMSampleBufferGetImageBuffer(sample) };
            if pixels.is_null() {
                return;
            }
            // SAFETY: se retiene para guardarlo más allá del callback.
            let kept = Pixels(unsafe { CFRetain(pixels) });
            *shared.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(kept);
            // Quien recibe va atrasado: este no se codifica (salvo un keyframe pedido).
            if !(shared.ready)() && !shared.force_key.load(Ordering::Relaxed) {
                return;
            }
            shared.encode(pixels);
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
            tracing::warn!(error = %error.localizedDescription(), "la captura de la pantalla se detuvo");
        }
    }
);

impl StreamOutput {
    fn new(ivars: OutputIvars) -> Retained<Self> {
        let output = Self::alloc().set_ivars(ivars);
        // SAFETY: `init` de NSObject sobre una instancia recién allocada.
        unsafe { msg_send![super(output), init] }
    }
}

pub(super) struct Stream {
    stream: Retained<SCStream>,
    output: Retained<StreamOutput>,
    _queue: dispatch2::DispatchRetained<DispatchQueue>,
    shared: Arc<Shared>,
    size: (u32, u32),
    /// Saca keyframes del último cuadro con la pantalla quieta; termina al soltarse.
    _keeper: std::sync::mpsc::Sender<()>,
}

// SAFETY: SCK no marca `SCStream` como MainThreadOnly y parar o dar de baja el output
// se puede llamar desde cualquier hilo; el output solo lo toca SCK (desde su cola) y
// `Drop`. Lo demás (`Shared`) ya es `Send + Sync`.
unsafe impl Send for Stream {}

impl Stream {
    pub(super) fn start(monitor: &crate::MonitorInfo, max_width: u32, bitrate: u32, fps: u32, output: Output, ready: Ready) -> Result<Self, String> {
        if !available!(macos = 13.0) {
            return Err("ScreenCaptureKit pide macOS 13".into());
        }
        // SAFETY: consulta pura de TCC.
        if unsafe { CGPreflightScreenCaptureAccess() } == 0 {
            return Err("falta el permiso de Grabación de pantalla".into());
        }
        let content = shareable_content()?;
        // SAFETY: getters de SCK sobre contenido retenido.
        let display = unsafe {
            let target = display_id_at(monitor.bounds.x, monitor.bounds.y);
            let displays = content.displays();
            (0..displays.count()).map(|i| displays.objectAtIndex(i)).find(|d| Some(d.displayID()) == target).or_else(|| displays.firstObject())
        }
        .ok_or("no hay pantallas para capturar")?;
        // Píxeles de verdad (Retina: el doble de los puntos), con el tope de ancho.
        let (points_w, points_h) = unsafe { (display.width() as u32, display.height() as u32) };
        let scale = monitor.scale.max(1.);
        let (width, height) = fit_even((points_w as f64 * scale) as u32, (points_h as f64 * scale) as u32, max_width);

        let cap = bitrate.max(250_000);
        let shared = Arc::new(Shared {
            encoder: Mutex::new(Encoder(std::ptr::null_mut())),
            last: Mutex::new(None),
            force_key: AtomicBool::new(true),
            key_asked_ms: AtomicU32::new(0),
            started: Instant::now(),
            output,
            ready,
            cap,
        });
        // El codificador avisa con un puntero a `shared` (que lo sobrevive: se invalida
        // al soltar el stream, antes que `shared`).
        let session = create_session(width, height, cap, fps, Arc::as_ptr(&shared) as *mut c_void)?;
        shared.encoder.lock().unwrap_or_else(|e| e.into_inner()).0 = session;

        (shared.output)(out::FORMAT, format!(r#"{{"width":{width},"height":{height},"rotation":0}}"#).into_bytes());

        let excluded = NSArray::new();
        // SAFETY: SCK retiene lo que necesita de la pantalla y la config.
        let filter = unsafe { SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &excluded) };
        let config = unsafe { SCStreamConfiguration::new() };
        // SAFETY: setters de una config recién creada.
        unsafe {
            config.setWidth(width as usize);
            config.setHeight(height as usize);
            config.setPixelFormat(NV12);
            config.setMinimumFrameInterval(CMTime { value: 1, timescale: fps.max(1) as i32, flags: CMTimeFlags::Valid, epoch: 0 });
            config.setShowsCursor(true);
            config.setQueueDepth(4);
        }
        let queue = DispatchQueue::new("com.ciat.atic.screen-stream", None);
        let handler = StreamOutput::new(OutputIvars { shared: shared.clone() });
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
        match started_rx.recv_timeout(Duration::from_secs(6)) {
            Ok(None) => {}
            Ok(Some(error)) => return Err(format!("ScreenCaptureKit no empezó: {error}")),
            Err(_) => {
                unsafe { stream.stopCaptureWithCompletionHandler(None) };
                return Err("ScreenCaptureKit no respondió".into());
            }
        }

        let (keeper, stop) = mpsc::channel();
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("atic-pantalla-quieta".into())
            .spawn(move || keep_keyframes(weak, stop))
            .map_err(|e| e.to_string())?;
        tracing::info!(ancho = width, alto = height, fps, "pantalla en vivo con ScreenCaptureKit y VideoToolbox");
        Ok(Self { stream, output: handler, _queue: queue, shared, size: (width, height), _keeper: keeper })
    }

    pub(super) fn request_keyframe(&self) {
        self.shared.force_key.store(true, Ordering::Relaxed);
        let _ = self.shared.key_asked_ms.compare_exchange(0, self.shared.now_ms(), Ordering::Relaxed, Ordering::Relaxed);
    }

    pub(super) fn set_bitrate(&self, bps: u32) {
        let encoder = self.shared.encoder.lock().unwrap_or_else(|e| e.into_inner());
        if encoder.0.is_null() {
            return;
        }
        set_rate(encoder.0, bps.clamp(250_000, self.shared.cap));
    }

    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
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
        let mut encoder = self.shared.encoder.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: después de invalidar no hay más callbacks con el refcon; la sesión queda
        // en nulo para quien llegue tarde (`Shared::encode`).
        unsafe {
            VTCompressionSessionCompleteFrames(encoder.0, CMTime { value: 0, timescale: 0, flags: CMTimeFlags(0), epoch: 0 });
            VTCompressionSessionInvalidate(encoder.0);
            CFRelease(encoder.0);
        }
        encoder.0 = std::ptr::null_mut();
    }
}

/// Con la pantalla quieta SCK no entrega cuadros: un keyframe pedido sale con el
/// último que hubo.
fn keep_keyframes(shared: std::sync::Weak<Shared>, stop: mpsc::Receiver<()>) {
    while let Err(mpsc::RecvTimeoutError::Timeout) = stop.recv_timeout(Duration::from_millis(50)) {
        let Some(shared) = shared.upgrade() else { return };
        let asked = shared.key_asked_ms.load(Ordering::Relaxed);
        if asked == 0 || shared.now_ms().saturating_sub(asked) < KEY_FROM_LAST.as_millis() as u32 {
            continue;
        }
        // Se retiene con el candado tomado: SCK puede reemplazar (soltar) el último.
        // SAFETY: el búfer guardado está retenido mientras está en `last`.
        let last = shared.last.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|p| Pixels(unsafe { CFRetain(p.0) }));
        if let Some(kept) = last {
            shared.force_key.store(true, Ordering::Relaxed);
            shared.encode(kept.0);
        }
    }
}

fn create_session(width: u32, height: u32, bitrate: u32, fps: u32, refcon: *mut c_void) -> Result<Session, String> {
    // SAFETY: constantes de VideoToolbox; diccionarios armados con core-foundation.
    unsafe {
        let low_latency = CFString::wrap_under_get_rule(kVTVideoEncoderSpecification_EnableLowLatencyRateControl);
        let hardware = CFString::wrap_under_get_rule(kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder);
        let specification = CFDictionary::from_CFType_pairs(&[(low_latency, CFBoolean::true_value()), (hardware, CFBoolean::true_value())]);
        let mut session: Session = std::ptr::null_mut();
        let status = VTCompressionSessionCreate(
            std::ptr::null(),
            width as i32,
            height as i32,
            CODEC_H264,
            specification.as_concrete_TypeRef(),
            std::ptr::null(),
            std::ptr::null(),
            encoded,
            refcon,
            &mut session,
        );
        if status != 0 || session.is_null() {
            return Err(format!("VideoToolbox no creó el codificador ({status})"));
        }
        let set = |key: CFStringRef, value: CFTypeRef| VTSessionSetProperty(session, key, value);
        set(kVTCompressionPropertyKey_RealTime, CFBoolean::true_value().as_CFTypeRef());
        set(kVTCompressionPropertyKey_ProfileLevel, kVTProfileLevel_H264_Main_AutoLevel.cast());
        set(kVTCompressionPropertyKey_AllowFrameReordering, CFBoolean::false_value().as_CFTypeRef());
        set(kVTCompressionPropertyKey_ExpectedFrameRate, CFNumber::from(fps as i32).as_CFTypeRef());
        set(kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration, CFNumber::from(KEYFRAME_SECS).as_CFTypeRef());
        set_rate(session, bitrate);
        VTCompressionSessionPrepareToEncodeFrames(session);
        Ok(session)
    }
}

/// Bitrate promedio y tope por segundo (1,5 veces: deja respirar a los keyframes).
fn set_rate(session: Session, bps: u32) {
    // SAFETY: propiedades de una sesión viva.
    unsafe {
        VTSessionSetProperty(session, kVTCompressionPropertyKey_AverageBitRate, CFNumber::from(bps as i32).as_CFTypeRef());
        let bytes = CFNumber::from((bps as f64 * 1.5 / 8.) as i64);
        let seconds = CFNumber::from(1.0f64);
        let limits = CFArray::from_CFTypes(&[bytes, seconds]);
        VTSessionSetProperty(session, kVTCompressionPropertyKey_DataRateLimits, limits.as_CFTypeRef());
    }
}

/// El id de la pantalla cuyo origen (en puntos) es `(x, y)`.
fn display_id_at(x: i32, y: i32) -> Option<u32> {
    core_graphics::display::CGDisplay::active_displays().ok()?.into_iter().find(|&id| {
        let bounds = core_graphics::display::CGDisplay::new(id).bounds();
        bounds.origin.x.round() as i32 == x && bounds.origin.y.round() as i32 == y
    })
}

/// `SCShareableContent` no implementa `Send`; se pasa una sola vez por el canal.
struct SendableContent(Retained<SCShareableContent>);
// SAFETY: SCK no la marca como MainThreadOnly y se transfiere una sola vez.
unsafe impl Send for SendableContent {}

fn shareable_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel::<Result<SendableContent, String>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let result = if content.is_null() {
            Err(unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "ScreenCaptureKit no devolvió pantallas".into()))
        } else {
            // SAFETY: referencia +0: se retiene para usarla fuera del callback.
            unsafe { Retained::retain(content) }.map(SendableContent).ok_or_else(|| "no se pudo retener SCShareableContent".to_string())
        };
        let _ = tx.send(result);
    });
    // SAFETY: el bloque vive hasta que SCK lo copia.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    match rx.recv_timeout(Duration::from_secs(6)) {
        Ok(result) => result.map(|c| c.0),
        Err(_) => Err("ScreenCaptureKit no respondió al buscar pantallas".into()),
    }
}
