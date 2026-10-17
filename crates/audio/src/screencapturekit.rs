//! Audio del sistema en macOS con ScreenCaptureKit.
//!
//! SCK no tiene un stream "solo audio": se arma un `SCStream` con
//! `capturesAudio` sobre un display y sólo se consume la salida de audio
//! (`SCStreamOutputType::Audio`). Requiere Grabación de pantalla (el mismo TCC
//! que las capturas) y macOS 13+.
//!
//! SCK entrega PCM float32 y sólo llama cuando hay audio. Cada bloque se
//! ubica en la línea de tiempo de la sesión por su presentation timestamp
//! (host time, el mismo reloj que el `mHostTime` del micrófono): el escritor
//! común rellena los huecos y el arranque tardío con silencio, para que la
//! pista no se desalinee del micrófono.

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{available, define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, AudioBuffer, AudioBufferList,
};
use objc2_core_media::{
    CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType,
};

use crate::{AudioError, AudioTapChunk, CaptureEvent, CaptureTrack, Clock, LevelMeter, TimedBuffer};

/// `kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment`.
const ASSURE_ALIGNMENT: u32 = 1 << 0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> u8;
    fn CGRequestScreenCaptureAccess() -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// `CMTime` de CoreMedia. Se declara acá para no sumar la feature `CMTime`
/// de `objc2-core-media` (cambiaría el `Cargo.lock`).
#[repr(C)]
#[derive(Clone, Copy)]
struct CmTime {
    value: i64,
    timescale: i32,
    flags: u32,
    /// Solo para el layout: no se lee.
    #[allow(dead_code)]
    epoch: i64,
}

/// `kCMTimeFlags_Valid`.
const CM_TIME_VALID: u32 = 1 << 0;

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetPresentationTimeStamp(sbuf: *const std::ffi::c_void) -> CmTime;
}

#[repr(C)]
struct MachTimebase {
    numer: u32,
    denom: u32,
}

extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebase) -> i32;
}

/// La hora del host (el reloj de `CMClockGetHostTimeClock`), en segundos.
fn host_now_secs() -> f64 {
    static TIMEBASE: OnceLock<(u32, u32)> = OnceLock::new();
    let (numer, denom) = *TIMEBASE.get_or_init(|| {
        let mut info = MachTimebase { numer: 0, denom: 0 };
        // SAFETY: llena la estructura que se le pasa.
        let status = unsafe { mach_timebase_info(&mut info) };
        if status != 0 || info.denom == 0 {
            (1, 1)
        } else {
            (info.numer, info.denom)
        }
    });
    // SAFETY: lectura del reloj monotónico del sistema.
    let ticks = unsafe { mach_absolute_time() };
    ticks as f64 * f64::from(numer) / f64::from(denom) / 1e9
}

/// Presentation timestamp del bloque en segundos, si es válido.
fn presentation_secs(sample: &CMSampleBuffer) -> Option<f64> {
    // SAFETY: getter puro sobre un sample buffer vivo durante el callback.
    let pts = unsafe {
        CMSampleBufferGetPresentationTimeStamp(sample as *const CMSampleBuffer as *const _)
    };
    if pts.flags & CM_TIME_VALID == 0 || pts.timescale <= 0 {
        return None;
    }
    let secs = pts.value as f64 / f64::from(pts.timescale);
    secs.is_finite().then_some(secs)
}

/// Lo más viejo que puede venir un bloque según su timestamp. Más que esto
/// es que el PTS no está en host time: se usa la hora de llegada.
const MAX_DELIVERY_DELAY: f64 = 1.0;

/// Dónde cae en la sesión un bloque de `frames` cuadros que llegó en
/// `arrival` (desde el origen). `delay`: cuánto antes de ahora empezó según
/// su PTS, si se pudo leer.
fn system_estimate(arrival: Duration, delay: Option<f64>, frames: usize, rate: u32) -> Duration {
    match delay.filter(|d| (0.0..=MAX_DELIVERY_DELAY).contains(d)) {
        Some(delay) => arrival.saturating_sub(Duration::from_secs_f64(delay)),
        None => arrival.saturating_sub(Duration::from_secs_f64(
            frames as f64 / f64::from(rate.max(1)),
        )),
    }
}

/// Captura de sistema viva. Al soltarlo se detiene el stream.
pub(crate) struct SystemAudioGuard {
    stream: Retained<SCStream>,
    /// El stream retiene el output; guardamos el handle para darlo de baja.
    output: Retained<SystemAudioOutput>,
    _queue: dispatch2::DispatchRetained<DispatchQueue>,
}

impl Drop for SystemAudioGuard {
    fn drop(&mut self) {
        unsafe {
            self.stream.stopCaptureWithCompletionHandler(None);
            let _ = self.stream.removeStreamOutput_type_error(
                ProtocolObject::from_ref(&*self.output),
                SCStreamOutputType::Audio,
            );
        }
    }
}

struct OutputIvars {
    tx: Sender<TimedBuffer>,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    sample_rate: u32,
    channels: u16,
    /// El cero de la sesión, común con el micrófono.
    origin: Instant,
    /// Los callbacks llegan en una cola serial: el candado nunca compite.
    clock: Mutex<Clock>,
    english: bool,
    warned_format: AtomicU8,
}

define_class!(
    // SAFETY: NSObject no impone requisitos extra y la clase no implementa Drop.
    #[unsafe(super(NSObject))]
    #[name = "AticSystemAudioOutput"]
    #[ivars = OutputIvars]
    struct SystemAudioOutput;

    unsafe impl NSObjectProtocol for SystemAudioOutput {}

    unsafe impl SCStreamOutput for SystemAudioOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_did_output_sample_buffer_of_type(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            r#type: SCStreamOutputType,
        ) {
            if r#type != SCStreamOutputType::Audio {
                return;
            }
            let ivars = self.ivars();
            // Las dos horas, juntas y antes de cualquier trabajo.
            let arrival = ivars.origin.elapsed();
            let host_now = host_now_secs();
            let Some(samples) = samples_from(sample_buffer, ivars.channels) else {
                if ivars.warned_format.swap(1, Ordering::Relaxed) == 0 {
                    tracing::warn!(
                        target: "audio",
                        "ScreenCaptureKit entregó un formato que no es float32; se ignora"
                    );
                }
                return;
            };
            if samples.is_empty() {
                return;
            }
            let sum_sq: f64 = samples
                .iter()
                .map(|sample| f64::from(*sample) * f64::from(*sample))
                .sum();
            ivars
                .meter
                .observe((sum_sq / samples.len() as f64).sqrt() as f32);

            let pts = presentation_secs(sample_buffer);
            let frames = samples.len() / usize::from(ivars.channels.max(1));
            let estimate = system_estimate(
                arrival,
                pts.map(|pts| host_now - pts),
                frames,
                ivars.sample_rate,
            );
            let hw = pts.and_then(|pts| Duration::try_from_secs_f64(pts).ok());
            let at = match ivars.clock.lock() {
                Ok(mut clock) => clock.stamp(hw, estimate),
                Err(_) => estimate,
            };

            if let Some(tap) = &ivars.stt_tap {
                let _ = tap.try_send(AudioTapChunk {
                    track: CaptureTrack::System,
                    start_ms: at.as_millis() as i64,
                    sample_rate: ivars.sample_rate,
                    channels: ivars.channels,
                    samples: samples.clone(),
                });
            }
            let _ = ivars.tx.send(TimedBuffer { at, samples });
        }
    }

    unsafe impl SCStreamDelegate for SystemAudioOutput {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
            let ivars = self.ivars();
            let message = error.localizedDescription().to_string();
            let text = if ivars.english {
                format!("System audio stopped: {message}")
            } else {
                format!("El audio del sistema se detuvo: {message}")
            };
            tracing::warn!(target: "audio", %message, "SCStream se detuvo");
            let _ = ivars.events.send(CaptureEvent::Error(text));
        }
    }
);

impl SystemAudioOutput {
    fn new(ivars: OutputIvars) -> Retained<Self> {
        let handler = Self::alloc().set_ivars(ivars);
        // SAFETY: `init` de NSObject sobre una instancia recién allocada.
        unsafe { msg_send![super(handler), init] }
    }
}

/// Arranca la captura de audio del sistema y devuelve el guard + el writer.
///
/// Bloquea hasta que `startCapture` confirma, igual que `CaptureSession::start`
/// con el micrófono: los errores de arranque se propagan de inmediato.
pub(crate) fn start(
    path: &Path,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    english: bool,
    origin: Instant,
) -> Result<(SystemAudioGuard, crate::TrackWriter), AudioError> {
    if !available!(macos = 13.0) {
        return Err(AudioError::SystemAudioUnsupported(
            "macOS 13 o superior (ScreenCaptureKit)".into(),
        ));
    }
    if !screen_capture_allowed() {
        let _ = request_screen_capture();
        return Err(AudioError::SystemAudioPermission);
    }

    let content = shareable_content()?;
    let displays = unsafe { content.displays() };
    let display = displays.firstObject().ok_or_else(|| {
        AudioError::Config("no hay pantallas para capturar el audio del sistema".into())
    })?;
    let excluded = NSArray::new();
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &excluded,
        )
    };

    let config = unsafe { SCStreamConfiguration::new() };
    // SAFETY: setters de una config recién creada; no retienen objetos.
    unsafe {
        config.setCapturesAudio(true);
        config.setSampleRate(48_000);
        config.setChannelCount(2);
        // Beeps y sonidos propios no entran en la pista de "otros".
        config.setExcludesCurrentProcessAudio(true);
        // No consumimos video: el mínimo válido.
        config.setWidth(2);
        config.setHeight(2);
    }

    let queue = DispatchQueue::new("com.ciat.atic.system-audio", None);
    let (tx, rx) = mpsc::channel::<TimedBuffer>();
    let handler = SystemAudioOutput::new(OutputIvars {
        tx,
        meter,
        events,
        stt_tap,
        sample_rate: 48_000,
        channels: 2,
        origin,
        clock: Mutex::new(Clock::new()),
        english,
        warned_format: AtomicU8::new(0),
    });

    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            Some(ProtocolObject::from_ref(&*handler)),
        )
    };
    // SAFETY: el output queda retenido por el stream y por `handler`.
    unsafe {
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*handler),
                SCStreamOutputType::Audio,
                Some(&queue),
            )
            .map_err(|error| AudioError::Config(error.localizedDescription().to_string()))?;
    }

    let (started_tx, started_rx) = mpsc::channel::<bool>();
    let started = RcBlock::new(move |error: *mut NSError| {
        let _ = started_tx.send(error.is_null());
    });
    // SAFETY: el bloque vive hasta que SCK lo copia; el completion avisa por canal.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&started)) };
    match started_rx.recv_timeout(Duration::from_secs(6)) {
        Ok(true) => {}
        Ok(false) => {
            return Err(AudioError::Config(
                "ScreenCaptureKit no pudo iniciar el audio del sistema".into(),
            ));
        }
        Err(_) => {
            unsafe { stream.stopCaptureWithCompletionHandler(None) };
            return Err(AudioError::Config(
                "ScreenCaptureKit no respondió al iniciar el audio del sistema".into(),
            ));
        }
    }

    // Lo que SCK haya entregado mientras tanto espera en el canal con su
    // instante: el escritor lo ubica igual.
    let writer = crate::spawn_writer(
        path.to_path_buf(),
        wav_spec(),
        48_000,
        2,
        crate::Stage::Plain,
        rx,
        CaptureTrack::System,
    );
    Ok((
        SystemAudioGuard {
            stream,
            output: handler,
            _queue: queue,
        },
        writer,
    ))
}

fn screen_capture_allowed() -> bool {
    // SAFETY: consulta pura de TCC.
    unsafe { CGPreflightScreenCaptureAccess() != 0 }
}

fn request_screen_capture() -> bool {
    // SAFETY: muestra el diálogo del sistema si falta el permiso.
    unsafe { CGRequestScreenCaptureAccess() != 0 }
}

/// `SCShareableContent` no implementa `Send`, pero el traspaso por el canal
/// sincroniza el callback con el hilo que espera.
struct SendableContent(Retained<SCShareableContent>);
// SAFETY: SCK no marca la clase como MainThreadOnly y el valor se transfiere
// una sola vez, después de que el bloque terminó de escribirlo.
unsafe impl Send for SendableContent {}

fn shareable_content() -> Result<Retained<SCShareableContent>, AudioError> {
    let (tx, rx) = mpsc::channel::<Result<SendableContent, String>>();
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = if content.is_null() {
                let message = unsafe { error.as_ref() }
                    .map(|error| error.localizedDescription().to_string())
                    .unwrap_or_else(|| "ScreenCaptureKit no devolvió contenido".into());
                Err(message)
            } else {
                // SAFETY: la API entrega una referencia +0; se retiene para
                // conservarla fuera del callback.
                unsafe { Retained::retain(content) }
                    .map(SendableContent)
                    .ok_or_else(|| "no se pudo retener SCShareableContent".to_string())
            };
            let _ = tx.send(result);
        },
    );
    // SAFETY: el bloque vive hasta que SCK lo copia.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    match rx.recv_timeout(Duration::from_secs(6)) {
        Ok(Ok(SendableContent(content))) => Ok(content),
        Ok(Err(message)) => Err(AudioError::Config(message)),
        Err(_) => Err(AudioError::Config(
            "ScreenCaptureKit no respondió al enumerar pantallas".into(),
        )),
    }
}

/// Convierte el `CMSampleBuffer` de SCK a f32 interleaved con `channels`
/// canales (los del WAV): si SCK entregara otra cantidad, el WAV no puede
/// recibir más ni menos muestras por cuadro o la pista cambia de velocidad y
/// se desalinea.
///
/// Devuelve `None` si el formato no es float32 (SCK debería garantizarlo).
fn samples_from(sample: &CMSampleBuffer, channels: u16) -> Option<Vec<f32>> {
    // SAFETY: se pide el tamaño necesario con un buffer nulo; no escribe.
    let mut needed: usize = 0;
    let status = unsafe {
        sample.audio_buffer_list_with_retained_block_buffer(
            &mut needed,
            std::ptr::null_mut(),
            0,
            None,
            None,
            ASSURE_ALIGNMENT,
            std::ptr::null_mut(),
        )
    };
    if needed == 0 {
        if status != 0 {
            tracing::trace!(target: "audio", status, "sin AudioBufferList en el sample");
        }
        return None;
    }

    // La lista exige alineación de 16: `Vec<u128>` la garantiza.
    let mut storage = vec![0u128; needed.div_ceil(16)];
    let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
    // SAFETY: `storage` tiene al menos `needed` bytes y el block buffer de
    // salida se libera más abajo.
    let status = unsafe {
        sample.audio_buffer_list_with_retained_block_buffer(
            std::ptr::null_mut(),
            storage.as_mut_ptr() as *mut AudioBufferList,
            needed,
            None,
            None,
            ASSURE_ALIGNMENT,
            &mut block,
        )
    };
    if status != 0 || block.is_null() {
        if !block.is_null() {
            unsafe { CFRelease(block.cast()) };
        }
        tracing::warn!(target: "audio", status, "no se pudo leer el AudioBufferList");
        return None;
    }

    let result = unsafe { list_to_f32(storage.as_ptr() as *const AudioBufferList, sample) };
    // El block buffer viene con referencia +1: se libera acá.
    unsafe { CFRelease(block.cast()) };
    result.map(|(samples, from)| remap_channels(samples, from, usize::from(channels.max(1))))
}

/// Interleaved de `from` canales a `to`: mono se duplica, los que sobran se
/// descartan y los que faltan repiten el último.
fn remap_channels(samples: Vec<f32>, from: usize, to: usize) -> Vec<f32> {
    if from == to || from == 0 {
        return samples;
    }
    let frames = samples.len() / from;
    let mut out = Vec::with_capacity(frames * to);
    for frame in samples.chunks_exact(from) {
        for channel in 0..to {
            out.push(frame[channel.min(from - 1)]);
        }
    }
    out
}

/// Las muestras intercaladas y cuántos canales trae cada cuadro.
unsafe fn list_to_f32(
    list: *const AudioBufferList,
    sample: &CMSampleBuffer,
) -> Option<(Vec<f32>, usize)> {
    // SAFETY: la descripción es válida durante la llamada; sólo se copia el ASBD.
    let asbd = {
        let description = unsafe { sample.format_description() }?;
        let ptr = unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&description) };
        if ptr.is_null() {
            return None;
        }
        unsafe { *ptr }
    };
    if asbd.mFormatFlags & kAudioFormatFlagIsFloat == 0 || asbd.mBitsPerChannel != 32 {
        return None;
    }
    // El WAV se etiqueta a 48 kHz: otra tasa lo haría correr a otra velocidad.
    if (asbd.mSampleRate - 48_000.0).abs() > 1.0 {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                target: "audio",
                rate = asbd.mSampleRate,
                "ScreenCaptureKit entrega otra tasa que la pedida (48 kHz)"
            );
        }
    }
    // SAFETY: `list` apunta a la lista que llenó CoreMedia.
    let buffers = unsafe { (*list).mNumberBuffers } as usize;
    if buffers == 0 {
        return None;
    }
    let first = unsafe { std::ptr::addr_of!((*list).mBuffers) as *const AudioBuffer };
    let buffer_at = |index: usize| unsafe { &*first.add(index) };

    let non_interleaved =
        buffers > 1 || asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved as u32 != 0;
    let mut out = Vec::new();
    let channels;
    if non_interleaved {
        channels = buffers;
        let frames = (0..buffers)
            .map(|index| buffer_at(index).mDataByteSize as usize / 4)
            .min()
            .unwrap_or(0);
        out.reserve(frames * buffers);
        for frame in 0..frames {
            for index in 0..buffers {
                let data = buffer_at(index).mData as *const f32;
                if data.is_null() {
                    return None;
                }
                out.push(unsafe { *data.add(frame) });
            }
        }
    } else {
        let buffer = buffer_at(0);
        if buffer.mData.is_null() {
            return None;
        }
        channels = (buffer.mNumberChannels as usize).max(1);
        let count = buffer.mDataByteSize as usize / 4;
        out.extend_from_slice(unsafe {
            std::slice::from_raw_parts(buffer.mData as *const f32, count)
        });
    }
    Some((out, channels))
}

fn wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canales_al_formato_del_wav() {
        assert_eq!(remap_channels(vec![0.1, 0.2], 1, 2), vec![0.1, 0.1, 0.2, 0.2]);
        assert_eq!(remap_channels(vec![0.1, 0.2, 0.3, 0.4], 2, 2), vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(
            remap_channels(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 3, 2),
            vec![0.1, 0.2, 0.4, 0.5]
        );
    }

    /// El PTS manda si es plausible; si no (no viene en host time), la
    /// llegada menos lo que dura el bloque.
    #[test]
    fn instante_del_bloque_de_sistema() {
        // Valores exactos en binario: 62,5 ms y 125 ms.
        let arrival = Duration::from_millis(1_000);
        let ms = |micros: u64| Duration::from_micros(micros);
        assert_eq!(system_estimate(arrival, Some(0.0625), 480, 48_000), ms(937_500));
        assert_eq!(system_estimate(arrival, Some(-5.0), 3_000, 48_000), ms(937_500));
        assert_eq!(system_estimate(arrival, Some(7_200.0), 6_000, 48_000), ms(875_000));
        assert_eq!(system_estimate(arrival, None, 6_000, 48_000), ms(875_000));
        assert_eq!(system_estimate(Duration::ZERO, Some(0.5), 480, 48_000), Duration::ZERO);
    }
}
