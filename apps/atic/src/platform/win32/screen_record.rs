//! Grabar una zona de la pantalla a MP4.
//!
//! Windows.Graphics.Capture entrega los cuadros del monitor ya en la GPU; se
//! copia solo la zona a una textura que la CPU puede leer, se pasa a NV12 y
//! Media Foundation la codifica en H.264 (con el codificador de la tarjeta si
//! hay uno). Todo corre en un hilo propio, a `FPS` cuadros por segundo como
//! máximo: Windows solo manda un cuadro cuando algo cambió en la pantalla, así
//! que el video sale con cuadros de duración variable.
//!
//! Las ventanas excluidas de las capturas (`WDA_EXCLUDEFROMCAPTURE`, la pill)
//! no salen en el video.
//!
//! Con audio (`screen_audio`), el video se escribe primero a un temporal y al
//! terminar se junta con la mezcla en el MP4 final: el video pasa tal cual y
//! el audio se codifica en AAC (`mux`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use atic_capture::{Frame, Rect as PhysRect};
use std::fs::File;
use std::io::BufReader;
use windows::core::{Interface, HSTRING};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFByteStream, IMFSample, IMFSinkWriter, IMFSourceReader, MFAudioFormat_AAC, MFAudioFormat_PCM,
    MFCreateAttributes, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL,
    MFCreateSourceReaderFromURL, MFMediaType_Audio, MFMediaType_Video, MFNominalRange_16_235,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MFShutdown, MFStartup, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFVideoTransferMatrix_BT709, MFSTARTUP_FULL, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_MT_VIDEO_NOMINAL_RANGE,
    MF_MT_YUV_MATRIX, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::Shell::{FOLDERID_Videos, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};

use crate::screen_audio::{read_up_to, RecordClock, ScreenAudio, Tracks, FRAME_BYTES, RATE};

const FPS: u32 = 30;
/// Unidades de Media Foundation: 100 ns.
const TICKS_PER_SECOND: i64 = 10_000_000;
/// Lo más chico que vale la pena grabar, en píxeles físicos.
const MIN_SIDE: u32 = 16;

/// Lo que queda al terminar: el archivo y su primer cuadro, para mostrarlo.
pub struct Saved {
    pub path: PathBuf,
    pub thumb: Option<Frame>,
}

/// Una grabación en curso. `stop` la termina (o `discard`, sin guardar); el
/// archivo queda listo cuando `finished` lo devuelve.
pub struct Recording {
    stop: Arc<AtomicBool>,
    discard: Arc<AtomicBool>,
    clock: Arc<RecordClock>,
    done: Receiver<Result<Option<Saved>, String>>,
}

impl Recording {
    /// Lo grabado, sin las pausas.
    pub fn elapsed(&self) -> Duration {
        self.clock.recorded()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Termina sin guardar nada.
    pub fn discard(&self) {
        self.discard.store(true, Ordering::Relaxed);
        self.stop();
    }

    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn paused(&self) -> bool {
        self.clock.paused()
    }

    pub fn set_paused(&self, paused: bool) {
        self.clock.set_paused(paused);
    }

    /// El resultado, cuando el hilo ya cerró el archivo: `None` si se descartó.
    pub fn finished(&self) -> Option<Result<Option<Saved>, String>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("la grabación se cortó".into())),
        }
    }
}

/// Empieza a grabar `region` (físico, del escritorio virtual) en `path`,
/// con las pistas de audio pedidas. Vuelve cuando la captura y el codificador
/// están listos, o con el error. Si el audio no abre, graba sin él.
pub fn start(region: PhysRect, path: PathBuf, audio: Option<Tracks>) -> Result<Recording, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let discard = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(RecordClock::default());
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (done_tx, done) = mpsc::channel();
    let flags = Flags {
        stop: stop.clone(),
        discard: discard.clone(),
        clock: clock.clone(),
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let temp = std::env::temp_dir().join(format!("atic-grabar-{stamp}"));
    std::thread::Builder::new()
        .name("grabar pantalla".into())
        .spawn(move || {
            let result = with_media_foundation(|| record(region, &path, &temp, audio, &flags, &ready_tx))
                .map(|kept| kept.map(|thumb| Saved { path, thumb }));
            let _ = std::fs::remove_dir_all(&temp);
            let _ = done_tx.send(result);
        })
        .map_err(|error| error.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(Recording {
            stop,
            discard,
            clock,
            done,
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            Err("la captura no arrancó a tiempo".into())
        }
    }
}

/// `Videos\Atic`, donde quedan las grabaciones.
pub fn videos_dir() -> Option<PathBuf> {
    let path = unsafe { SHGetKnownFolderPath(&FOLDERID_Videos, KNOWN_FOLDER_FLAG(0), None) }.ok()?;
    let text = unsafe { path.to_string() };
    unsafe { CoTaskMemFree(Some(path.0 as *const _)) };
    Some(PathBuf::from(text.ok()?).join("Atic"))
}

fn with_media_foundation<T>(run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = match MFStartup(MF_VERSION, MFSTARTUP_FULL) {
            Ok(()) => {
                let result = run();
                let _ = MFShutdown();
                result
            }
            Err(error) => Err(format!("Media Foundation: {error}")),
        };
        if com {
            CoUninitialize();
        }
        result
    }
}

/// Lo que el hilo de la grabación comparte con la UI.
struct Flags {
    stop: Arc<AtomicBool>,
    discard: Arc<AtomicBool>,
    clock: Arc<RecordClock>,
}

/// Graba hasta que se pida parar. Devuelve el primer cuadro, o `None` si se
/// descartó (no quedó archivo).
fn record(
    region: PhysRect,
    path: &Path,
    temp: &Path,
    tracks: Option<Tracks>,
    flags: &Flags,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<Option<Option<Frame>>, String> {
    let clock = &flags.clock;
    // El audio abre antes que el video; lo que llegue antes de empezar se
    // descarta.
    let audio = tracks.and_then(|tracks| match ScreenAudio::start(tracks, temp.to_path_buf(), clock.clone()) {
        Ok(audio) => Some(audio),
        Err(error) => {
            eprintln!("grabar: sin audio ({error})");
            None
        }
    });
    let video = temp.join("video.mp4");
    let setup = Capture::open(region).and_then(|capture| {
        let encoder = Encoder::create(&video, capture.width, capture.height)?;
        Ok((capture, encoder))
    });
    let (mut capture, mut encoder) = match setup {
        Ok(pair) => {
            let _ = ready.send(Ok(()));
            pair
        }
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            if let Some(audio) = audio {
                let _ = audio.finish(Duration::ZERO);
            }
            return Err(error);
        }
    };
    let started = Instant::now();
    clock.begin(started);
    let tick = Duration::from_secs(1) / FPS;
    let mut next = started;
    let recorded = (|| {
        while !flags.stop.load(Ordering::Relaxed) {
            // En pausa los cuadros se leen igual (para no juntarlos) pero no van.
            let at = clock.now();
            if capture.grab()? {
                if let Some(at) = at {
                    encoder.push(&capture.nv12, at)?;
                }
            }
            next += tick;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                // Atrasado (el codificador tardó): no se intenta recuperar.
                next = now;
            }
        }
        let duration = clock.recorded();
        if flags.discard.load(Ordering::Relaxed) {
            return Ok(duration);
        }
        encoder.finish(duration).map(|()| duration)
    })();
    let thumb = capture.thumb.take();
    drop(capture);
    // El audio se cierra siempre: si no, el micrófono queda abierto.
    let duration = recorded.as_ref().copied().unwrap_or_default();
    let mixed = audio.map(|audio| audio.finish(duration));
    recorded?;
    if flags.discard.load(Ordering::Relaxed) {
        // La carpeta temporal se borra entera al salir.
        return Ok(None);
    }
    match mixed {
        Some(Ok(Some(mixed))) => match mux(&video, &mixed.path, mixed.frames, path) {
            Ok(()) => return Ok(Some(thumb)),
            Err(error) => {
                eprintln!("grabar: no se pudo juntar el audio ({error}); queda sin él");
                let _ = std::fs::remove_file(path);
            }
        },
        Some(Err(error)) => eprintln!("grabar: el audio falló ({error}); queda sin él"),
        Some(Ok(None)) | None => {}
    }
    move_file(&video, path).map(|()| Some(thumb))
}

/// Mueve el temporal a su lugar; si están en discos distintos, lo copia.
fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

fn win(error: windows::core::Error) -> String {
    error.message()
}

// --- La captura ---------------------------------------------------------------

struct Capture {
    context: ID3D11DeviceContext,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    staging: ID3D11Texture2D,
    /// La zona dentro de la textura del monitor.
    crop: D3D11_BOX,
    width: u32,
    height: u32,
    nv12: Vec<u8>,
    /// El primer cuadro en BGRA: la foto del estante.
    thumb: Option<Frame>,
}

impl Capture {
    fn open(region: PhysRect) -> Result<Self, String> {
        // El monitor del centro de la zona; lo que se salga de él se recorta.
        let center = POINT {
            x: region.x + region.width as i32 / 2,
            y: region.y + region.height as i32 / 2,
        };
        let monitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return Err("no se encontró el monitor de la zona".into());
        }
        let m = info.rcMonitor;
        let area = region
            .intersection(&PhysRect::from_ltrb(m.left, m.top, m.right, m.bottom))
            .ok_or("la zona quedó fuera de la pantalla")?;
        // NV12 necesita lados pares.
        let (width, height) = (area.width & !1, area.height & !1);
        if width < MIN_SIDE || height < MIN_SIDE {
            return Err("la zona es muy chica para grabar".into());
        }
        let (left, top) = ((area.x - m.left) as u32, (area.y - m.top) as u32);
        let crop = D3D11_BOX {
            left,
            top,
            front: 0,
            right: left + width,
            bottom: top + height,
            back: 1,
        };

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(win)?;
        let (device, context) = device.zip(context).ok_or("sin dispositivo de Direct3D")?;
        let dxgi: IDXGIDevice = device.cast().map_err(win)?;
        let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .and_then(|inspectable| inspectable.cast())
            .map_err(win)?;

        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(win)?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor) }.map_err(win)?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            item.Size().map_err(win)?,
        )
        .map_err(win)?;
        let session = pool.CreateCaptureSession(&item).map_err(win)?;
        let _ = session.SetIsCursorCaptureEnabled(true);
        // Sin el borde amarillo de Windows 11; donde no se pueda, queda.
        let _ = session.SetIsBorderRequired(false);

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(win)?;
        let staging = staging.ok_or("sin textura para leer los cuadros")?;
        session.StartCapture().map_err(win)?;
        Ok(Self {
            context,
            pool,
            session,
            staging,
            crop,
            width,
            height,
            nv12: Vec::new(),
            thumb: None,
        })
    }

    /// Toma el último cuadro que llegó, si llegó alguno, y lo deja en `nv12`.
    fn grab(&mut self) -> Result<bool, String> {
        let mut latest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            if let Some(old) = latest.replace(frame) {
                let _ = old.Close();
            }
        }
        let Some(frame) = latest else {
            return Ok(false);
        };
        let texture: ID3D11Texture2D = frame
            .Surface()
            .and_then(|surface| surface.cast::<IDirect3DDxgiInterfaceAccess>())
            .and_then(|access| unsafe { access.GetInterface() })
            .map_err(win)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .CopySubresourceRegion(&self.staging, 0, 0, 0, 0, &texture, 0, Some(&self.crop));
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(win)?;
            let pitch = mapped.RowPitch as usize;
            let bgra = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * self.height as usize);
            bgra_to_nv12(bgra, pitch, self.width as usize, self.height as usize, &mut self.nv12);
            if self.thumb.is_none() {
                let row = self.width as usize * 4;
                let pixels = bgra.chunks(pitch).flat_map(|line| &line[..row]).copied().collect();
                self.thumb = Some(Frame::new(PhysRect::new(0, 0, self.width, self.height), pixels));
            }
            self.context.Unmap(&self.staging, 0);
        }
        let _ = frame.Close();
        Ok(true)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

// --- El codificador -------------------------------------------------------------

struct Encoder {
    writer: IMFSinkWriter,
    stream: u32,
    width: u32,
    height: u32,
    /// El último cuadro, que se escribe cuando llega el siguiente: recién ahí
    /// se sabe cuánto dura.
    pending: Option<(IMFSample, i64)>,
    written: u32,
}

fn pack(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

fn ticks(duration: Duration) -> i64 {
    (duration.as_nanos() / 100) as i64
}

/// Bits por segundo: alcanza para texto nítido sin archivos enormes.
fn bitrate(width: u32, height: u32) -> u32 {
    let bits = width as f64 * height as f64 * FPS as f64 * 0.12;
    bits.clamp(2_000_000.0, 40_000_000.0) as u32
}

impl Encoder {
    fn create(path: &Path, width: u32, height: u32) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        // Con el codificador de la tarjeta si se puede; si no, el de Windows.
        match Self::with_hardware(path, width, height, true) {
            Ok(encoder) => Ok(encoder),
            Err(error) => {
                eprintln!("grabar: sin codificador de la tarjeta ({error}), uso el de Windows");
                Self::with_hardware(path, width, height, false)
            }
        }
    }

    fn with_hardware(path: &Path, width: u32, height: u32, hardware: bool) -> Result<Self, String> {
        unsafe {
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 1).map_err(win)?;
            let attributes = attributes.ok_or("sin atributos")?;
            attributes
                .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, hardware as u32)
                .map_err(win)?;
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None::<&IMFByteStream>, &attributes)
                .map_err(win)?;

            let output = MFCreateMediaType().map_err(win)?;
            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(win)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).map_err(win)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate(width, height)).map_err(win)?;
            output
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(win)?;
            output.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height)).map_err(win)?;
            output.SetUINT64(&MF_MT_FRAME_RATE, pack(FPS, 1)).map_err(win)?;
            output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).map_err(win)?;
            let stream = writer.AddStream(&output).map_err(win)?;

            let input = MFCreateMediaType().map_err(win)?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(win)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).map_err(win)?;
            input
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(win)?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height)).map_err(win)?;
            input.SetUINT64(&MF_MT_FRAME_RATE, pack(FPS, 1)).map_err(win)?;
            input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).map_err(win)?;
            input.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32).map_err(win)?;
            input
                .SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
                .map_err(win)?;
            writer.SetInputMediaType(stream, &input, None::<&IMFAttributes>).map_err(win)?;
            writer.BeginWriting().map_err(win)?;
            Ok(Self {
                writer,
                stream,
                width,
                height,
                pending: None,
                written: 0,
            })
        }
    }

    fn push(&mut self, nv12: &[u8], at: Duration) -> Result<(), String> {
        let time = ticks(at);
        let sample = memory_sample(nv12)?;
        unsafe { sample.SetSampleTime(time) }.map_err(win)?;
        if let Some((previous, started)) = self.pending.take() {
            self.write(previous, started, time)?;
        }
        self.pending = Some((sample, time));
        Ok(())
    }

    fn write(&mut self, sample: IMFSample, start: i64, end: i64) -> Result<(), String> {
        unsafe {
            sample.SetSampleDuration((end - start).max(1)).map_err(win)?;
            self.writer.WriteSample(self.stream, &sample).map_err(win)?;
        }
        self.written += 1;
        Ok(())
    }

    /// El último cuadro dura hasta que se detuvo la grabación.
    fn finish(mut self, at: Duration) -> Result<(), String> {
        if let Some((sample, start)) = self.pending.take() {
            let end = ticks(at).max(start + TICKS_PER_SECOND / FPS as i64);
            self.write(sample, start, end)?;
        }
        if self.written == 0 {
            return Err("no llegó ningún cuadro de la pantalla".into());
        }
        println!("grabar: {} cuadros de {}×{}", self.written, self.width, self.height);
        unsafe { self.writer.Finalize() }.map_err(win)
    }
}

// --- Juntar el audio ----------------------------------------------------------

/// Bits por segundo del AAC: 192 kbps estéreo (el codificador de Windows
/// acepta 96, 128, 160 y 192).
const AAC_BYTES_PER_SECOND: u32 = 24_000;
/// El audio se escribe en bloques de 100 ms.
const AUDIO_BLOCK: u64 = RATE as u64 / 10;

/// `video` (H.264 en MP4) y `audio` (crudo de `screen_audio`) en `out`. El
/// video se copia sin volver a codificar. Las dos pistas se escriben
/// intercaladas por tiempo: el escritor frena la que se adelanta mucho.
fn mux(video: &Path, audio: &Path, frames: u64, out: &Path) -> Result<(), String> {
    let video_stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let mut raw = BufReader::new(File::open(audio).map_err(|error| error.to_string())?);
    unsafe {
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(video.as_os_str()), None::<&IMFAttributes>)
            .map_err(win)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(win)?;
        reader.SetStreamSelection(video_stream, true).map_err(win)?;
        let video_type = reader.GetNativeMediaType(video_stream, 0).map_err(win)?;

        let writer =
            MFCreateSinkWriterFromURL(&HSTRING::from(out.as_os_str()), None::<&IMFByteStream>, None::<&IMFAttributes>)
                .map_err(win)?;
        let video_out = writer.AddStream(&video_type).map_err(win)?;

        let aac = MFCreateMediaType().map_err(win)?;
        aac.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(win)?;
        aac.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).map_err(win)?;
        aac.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(win)?;
        aac.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE).map_err(win)?;
        aac.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(win)?;
        aac.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AAC_BYTES_PER_SECOND).map_err(win)?;
        let audio_out = writer.AddStream(&aac).map_err(win)?;

        let pcm = MFCreateMediaType().map_err(win)?;
        pcm.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(win)?;
        pcm.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(win)?;
        pcm.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(win)?;
        pcm.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE).map_err(win)?;
        pcm.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(win)?;
        pcm.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, FRAME_BYTES as u32).map_err(win)?;
        pcm.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE * FRAME_BYTES as u32).map_err(win)?;
        writer.SetInputMediaType(audio_out, &pcm, None::<&IMFAttributes>).map_err(win)?;
        writer.BeginWriting().map_err(win)?;

        let mut next_video = read_video(&reader, video_stream)?;
        let mut written = 0u64;
        let mut bytes = vec![0u8; AUDIO_BLOCK as usize * FRAME_BYTES];
        loop {
            let audio_time = written as i64 * TICKS_PER_SECOND / RATE as i64;
            let audio_left = written < frames;
            match next_video.take() {
                Some((sample, time)) if !audio_left || time <= audio_time => {
                    writer.WriteSample(video_out, &sample).map_err(win)?;
                    next_video = read_video(&reader, video_stream)?;
                }
                pending if audio_left => {
                    next_video = pending;
                    let n = (frames - written).min(AUDIO_BLOCK);
                    let block = &mut bytes[..n as usize * FRAME_BYTES];
                    let got = read_up_to(&mut raw, block).map_err(|error| error.to_string())?;
                    block[got..].fill(0);
                    let sample = memory_sample(block)?;
                    sample.SetSampleTime(audio_time).map_err(win)?;
                    sample
                        .SetSampleDuration(n as i64 * TICKS_PER_SECOND / RATE as i64)
                        .map_err(win)?;
                    writer.WriteSample(audio_out, &sample).map_err(win)?;
                    written += n;
                }
                _ => break,
            }
        }
        writer.Finalize().map_err(win)
    }
}

/// El próximo cuadro comprimido del video, o `None` al final.
unsafe fn read_video(reader: &IMFSourceReader, stream: u32) -> Result<Option<(IMFSample, i64)>, String> {
    loop {
        let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
        unsafe { reader.ReadSample(stream, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample)) }
            .map_err(win)?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Ok(None);
        }
        if let Some(sample) = sample {
            return Ok(Some((sample, time)));
        }
    }
}

/// Una muestra de Media Foundation con una copia de `data`.
fn memory_sample(data: &[u8]) -> Result<IMFSample, String> {
    unsafe {
        let len = data.len() as u32;
        let buffer = MFCreateMemoryBuffer(len).map_err(win)?;
        let mut ptr = std::ptr::null_mut();
        buffer.Lock(&mut ptr, None, None).map_err(win)?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        buffer.Unlock().map_err(win)?;
        buffer.SetCurrentLength(len).map_err(win)?;
        let sample = MFCreateSample().map_err(win)?;
        sample.AddBuffer(&buffer).map_err(win)?;
        Ok(sample)
    }
}

// --- Color ------------------------------------------------------------------------

/// BGRA (filas de `pitch` bytes) a NV12 con BT.709 en rango limitado: el plano
/// Y entero y luego U y V intercalados, uno por cada 2×2 píxeles. `width` y
/// `height` son pares.
pub(crate) fn bgra_to_nv12(bgra: &[u8], pitch: usize, width: usize, height: usize, out: &mut Vec<u8>) {
    out.resize(width * height * 3 / 2, 0);
    let (luma, chroma) = out.split_at_mut(width * height);
    for (y, row) in luma.chunks_exact_mut(width).enumerate() {
        let src = &bgra[y * pitch..y * pitch + width * 4];
        for (dst, px) in row.iter_mut().zip(src.chunks_exact(4)) {
            let (b, g, r) = (px[0] as i32, px[1] as i32, px[2] as i32);
            *dst = (16 + ((47 * r + 157 * g + 16 * b + 128) >> 8)) as u8;
        }
    }
    for (y, row) in chroma.chunks_exact_mut(width).enumerate() {
        let top = &bgra[2 * y * pitch..];
        let bottom = &bgra[(2 * y + 1) * pitch..];
        for (x, uv) in row.chunks_exact_mut(2).enumerate() {
            let (mut b, mut g, mut r) = (0, 0, 0);
            for line in [top, bottom] {
                for px in line[x * 8..x * 8 + 8].chunks_exact(4) {
                    b += px[0] as i32;
                    g += px[1] as i32;
                    r += px[2] as i32;
                }
            }
            let (b, g, r) = (b / 4, g / 4, r / 4);
            uv[0] = (128 + ((-26 * r - 87 * g + 112 * b + 128) >> 8)).clamp(0, 255) as u8;
            uv[1] = (128 + ((112 * r - 102 * g - 10 * b + 128) >> 8)).clamp(0, 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, pitch: usize, [b, g, r]: [u8; 3]) -> Vec<u8> {
        let mut bgra = vec![0u8; pitch * height];
        for y in 0..height {
            for x in 0..width {
                bgra[y * pitch + x * 4..y * pitch + x * 4 + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        bgra
    }

    /// Graba de verdad la esquina del monitor principal. A mano:
    /// `ATIC_REC_OUT=<archivo.mp4> cargo test graba_la_pantalla -- --ignored`;
    /// con `ATIC_REC_AUDIO=1` (o `mic`, `system`), también el audio.
    #[test]
    #[ignore]
    fn graba_la_pantalla() {
        let path = PathBuf::from(std::env::var("ATIC_REC_OUT").expect("ATIC_REC_OUT"));
        let audio = std::env::var("ATIC_REC_AUDIO").ok().map(|which| Tracks {
            mic: which != "system",
            system: which != "mic",
        });
        let recording = start(PhysRect::new(0, 0, 801, 601), path.clone(), audio).expect("arrancar");
        // `ATIC_REC_TONE=<archivo.wav>`: suena al segundo exacto, para medir
        // en el audio grabado cuánto se corre.
        if let Ok(tone) = std::env::var("ATIC_REC_TONE") {
            use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME};
            std::thread::sleep(Duration::from_secs(1));
            let played = unsafe { PlaySoundW(&HSTRING::from(tone), None, SND_FILENAME | SND_ASYNC) };
            assert!(played.as_bool());
            std::thread::sleep(Duration::from_secs(2));
        } else if std::env::var("ATIC_REC_PAUSE").is_ok() {
            // 1 s, pausa de 1,5 s, 1 s más: el video debe durar 2 s.
            std::thread::sleep(Duration::from_secs(1));
            recording.set_paused(true);
            std::thread::sleep(Duration::from_millis(1500));
            recording.set_paused(false);
            std::thread::sleep(Duration::from_secs(1));
        } else {
            std::thread::sleep(Duration::from_secs(2));
        }
        recording.stop();
        let result = loop {
            if let Some(result) = recording.finished() {
                break result;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let saved = result.expect("terminar").expect("guardada");
        assert_eq!(saved.path, path);
        assert!(saved.thumb.is_some());
        assert!(std::fs::metadata(&path).expect("archivo").len() > 1000);
    }

    #[test]
    fn blanco_y_negro_en_rango_limitado() {
        let mut out = Vec::new();
        bgra_to_nv12(&solid(4, 2, 16, [255, 255, 255]), 16, 4, 2, &mut out);
        assert_eq!(out.len(), 12);
        assert!(out[..8].iter().all(|&y| y == 235), "{out:?}");
        assert!(out[8..].iter().all(|&c| (127..=129).contains(&c)), "{out:?}");

        bgra_to_nv12(&solid(4, 2, 16, [0, 0, 0]), 16, 4, 2, &mut out);
        assert!(out[..8].iter().all(|&y| y == 16));
        assert!(out[8..].iter().all(|&c| c == 128));
    }

    #[test]
    fn el_rojo_va_en_v_y_se_ignora_el_relleno_de_la_fila() {
        let mut out = Vec::new();
        // Filas de 24 bytes para 4 píxeles: los 8 de relleno no cuentan.
        let mut bgra = solid(4, 2, 24, [0, 0, 255]);
        for y in 0..2 {
            bgra[y * 24 + 16..y * 24 + 24].fill(200);
        }
        bgra_to_nv12(&bgra, 24, 4, 2, &mut out);
        assert!(out[..8].iter().all(|&y| y == 63), "{out:?}");
        let (u, v) = (out[8], out[9]);
        assert!(u < 110 && v > 230, "u {u}, v {v}");
        assert_eq!(&out[8..10], &out[10..12]);
    }
}
