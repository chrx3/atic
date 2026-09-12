//! Audio del sistema en macOS con ScreenCaptureKit.
//!
//! SCK no tiene un stream "solo audio": se arma un `SCStream` con
//! `capturesAudio` sobre un display y sólo se consume la salida de audio
//! (`SCStreamOutputType::Audio`). Requiere Grabación de pantalla (el mismo TCC
//! que las capturas) y macOS 13+.
//!
//! SCK entrega PCM float32 y sólo llama cuando hay audio: el WAV se escribe
//! contra la línea de tiempo real (rellena silencio en los huecos) para que no
//! se desalinee del micrófono.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::thread::{self, JoinHandle};
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

use crate::{AudioError, AudioTapChunk, CaptureEvent, CaptureTrack, LevelMeter};

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
    tx: Sender<Vec<f32>>,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    sample_rate: u32,
    channels: u16,
    frames_seen: AtomicU64,
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
            let Some(samples) = samples_from(sample_buffer) else {
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

            if let Some(tap) = &ivars.stt_tap {
                let channels = u64::from(ivars.channels.max(1));
                let frames = samples.len() as u64 / channels;
                let start_frames = ivars.frames_seen.fetch_add(frames, Ordering::Relaxed);
                let start_ms = ((start_frames * 1000) / u64::from(ivars.sample_rate.max(1))) as i64;
                let _ = tap.try_send(AudioTapChunk {
                    track: CaptureTrack::System,
                    start_ms,
                    sample_rate: ivars.sample_rate,
                    channels: ivars.channels,
                    samples: samples.clone(),
                });
            }
            let _ = ivars.tx.send(samples);
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
) -> Result<(SystemAudioGuard, JoinHandle<Result<u64, AudioError>>), AudioError> {
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
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let handler = SystemAudioOutput::new(OutputIvars {
        tx,
        meter,
        events,
        stt_tap,
        sample_rate: 48_000,
        channels: 2,
        frames_seen: AtomicU64::new(0),
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

    let writer = spawn_timeline_writer(path.to_path_buf(), wav_spec(), rx);
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

/// Convierte el `CMSampleBuffer` de SCK a f32 interleaved.
///
/// Devuelve `None` si el formato no es float32 (SCK debería garantizarlo).
fn samples_from(sample: &CMSampleBuffer) -> Option<Vec<f32>> {
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
    result
}

unsafe fn list_to_f32(list: *const AudioBufferList, sample: &CMSampleBuffer) -> Option<Vec<f32>> {
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
    if non_interleaved {
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
        let count = buffer.mDataByteSize as usize / 4;
        out.extend_from_slice(unsafe {
            std::slice::from_raw_parts(buffer.mData as *const f32, count)
        });
    }
    Some(out)
}

fn wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

/// Writer con línea de tiempo: si SCK deja de llamar (silencio), rellena con
/// ceros hasta el tiempo transcurrido para no encoger la pista de "otros".
fn spawn_timeline_writer(
    path: PathBuf,
    spec: hound::WavSpec,
    rx: Receiver<Vec<f32>>,
) -> JoinHandle<Result<u64, AudioError>> {
    thread::spawn(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = hound::WavWriter::create(&path, spec)?;
        let channels = u64::from(spec.channels.max(1));
        let rate = u64::from(spec.sample_rate.max(1));
        let started = Instant::now();
        let mut frames: u64 = 0;
        let mut written: u64 = 0;
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(buffer) => {
                    for sample in buffer {
                        writer.write_sample(sample)?;
                        written += 1;
                    }
                    frames = written / channels;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let target = started.elapsed().as_millis() as u64 * rate / 1000;
                    let missing = target.saturating_sub(frames);
                    if missing > rate / 20 {
                        for _ in 0..missing * channels {
                            writer.write_sample(0.0f32)?;
                            written += 1;
                        }
                        frames += missing;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        writer.finalize()?;
        Ok(written)
    })
}
