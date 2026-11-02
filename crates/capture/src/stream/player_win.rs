//! Reproducir un video para editarlo (Windows): el motor de medios de Media Foundation
//! (`IMFMediaEngine`, el de los navegadores) lleva el audio, el reloj, el salto y la
//! velocidad; cada cuadro se copia a una textura de la GPU (`HardwareFrame`), que el
//! compositor (`compose_win.rs`) dibuja sin pasar por la CPU.
//!
//! El mismo contrato que `player_macos.rs`: se usa desde el hilo de la ventana y quien
//! dibuja pide el cuadro del momento en cada pasada (`frame`).

use std::cell::Cell;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows::core::{implement, BSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MFCreateAttributes,
    CLSID_MFMediaEngineClassFactory, MF_MEDIA_ENGINE_CALLBACK, MF_MEDIA_ENGINE_DXGI_MANAGER, MF_MEDIA_ENGINE_EVENT, MF_MEDIA_ENGINE_EVENT_ERROR,
    MF_MEDIA_ENGINE_EVENT_LOADEDDATA, MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA, MF_MEDIA_ENGINE_EVENT_SEEKED, MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

use super::gfx_win::Gfx;
use super::info_win::{keep_media_foundation, win};

/// Lo más que se espera a que el archivo abra (metadatos y primer cuadro).
const OPEN_TIMEOUT: Duration = Duration::from_secs(6);

/// Un cuadro decodificado: la textura BGRA de la GPU que el reproductor llena. `serial` cambia
/// con cada cuadro nuevo (así quien guarda una salida compuesta sabe si sigue valiendo).
#[derive(Clone)]
pub struct HardwareFrame {
    pub(super) texture: ID3D11Texture2D,
    serial: u64,
    size: (u32, u32),
}

impl PartialEq for HardwareFrame {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial && self.texture == other.texture
    }
}

impl HardwareFrame {
    /// Un cuadro hecho por quien exporta (no por el reproductor): su textura y su número.
    pub(super) fn from_parts(texture: ID3D11Texture2D, serial: u64, size: (u32, u32)) -> Self {
        Self { texture, serial, size }
    }

    pub fn width(&self) -> u32 {
        self.size.0
    }

    pub fn height(&self) -> u32 {
        self.size.1
    }

    /// Los píxeles BGRA de este cuadro, arriba a la izquierda primero (para pruebas y para ver
    /// el video sin compositor).
    pub fn bgra(&self) -> Result<crate::Frame, String> {
        let gfx = Gfx::shared()?;         let (w, h, bytes) = gfx.read(&self.texture)?;
        Ok(crate::Frame::new(crate::Rect::new(0, 0, w, h), bytes))
    }
}

/// Lo que cuenta el motor de medios (desde sus hilos) a quien lo reproduce.
#[derive(Default)]
struct Events {
    metadata: bool,
    data: bool,
    /// Cuántas veces terminó un salto.
    seeks: u64,
    error: Option<String>,
}

struct Shared {
    events: Mutex<Events>,
    changed: Condvar,
}

#[implement(IMFMediaEngineNotify)]
struct Notify(Arc<Shared>);

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, _param2: u32) -> windows::core::Result<()> {
        let event = MF_MEDIA_ENGINE_EVENT(event as i32);
        if let Ok(mut events) = self.0.events.lock() {
            if event == MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA {
                events.metadata = true;
            } else if event == MF_MEDIA_ENGINE_EVENT_LOADEDDATA {
                events.data = true;
            } else if event == MF_MEDIA_ENGINE_EVENT_SEEKED {
                events.seeks += 1;
            } else if event == MF_MEDIA_ENGINE_EVENT_ERROR {
                events.error = Some(format!("el motor de medios falló (código {param1})"));
            }
        }
        self.0.changed.notify_all();
        Ok(())
    }
}

/// Un video abierto para reproducir.
pub struct VideoPlayer {
    engine: IMFMediaEngine,
    gfx: Gfx,
    shared: Arc<Shared>,
    texture: ID3D11Texture2D,
    duration: f64,
    size: (u32, u32),
    /// La marca de tiempo del último cuadro que entregó el motor.
    last_pts: i64,
    serial: u64,
    /// El próximo `frame` copia el cuadro aunque el motor no avise uno nuevo (tras abrir o saltar).
    refresh: bool,
    seeks_seen: u64,
    has_frame: bool,
    /// La velocidad de reproducción (1.0 normal); `play` la respeta.
    rate: Cell<f64>,
}

impl VideoPlayer {
    pub fn open(path: &Path) -> Result<Self, String> {
        keep_media_foundation()?;
        let gfx = Gfx::shared()?;
        let shared = Arc::new(Shared { events: Mutex::new(Events::default()), changed: Condvar::new() });
        // SAFETY: objetos de Media Foundation y Direct3D creados aquí; el motor se apaga en `Drop`.
        unsafe {
            let notify: IMFMediaEngineNotify = Notify(shared.clone()).into();
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 3).map_err(win)?;
            let attributes = attributes.ok_or("Media Foundation no dio atributos")?;
            attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify).map_err(win)?;
            attributes.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &gfx.manager).map_err(win)?;
            attributes.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32).map_err(win)?;
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("sin motor de medios: {}", win(e)))?;
            let engine = factory.CreateInstance(0, &attributes).map_err(|e| format!("no se pudo crear el motor de medios: {}", win(e)))?;
            engine.SetAutoPlay(false).map_err(win)?;
            let source = BSTR::from(path.to_string_lossy().as_ref());
            engine.SetSource(&source).map_err(win)?;
            engine.Load().map_err(win)?;

            // Metadatos y primer cuadro.
            let deadline = Instant::now() + OPEN_TIMEOUT;
            {
                let mut events = shared.events.lock().map_err(|_| "el motor de medios se trabó")?;
                while !(events.metadata && events.data) {
                    if let Some(error) = events.error.take() {
                        let _ = engine.Shutdown();
                        return Err(error);
                    }
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        let _ = engine.Shutdown();
                        return Err("el video no abrió a tiempo".into());
                    }
                    events = shared.changed.wait_timeout(events, left).map_err(|_| "el motor de medios se trabó")?.0;
                }
            }
            if !engine.HasVideo().as_bool() {
                let _ = engine.Shutdown();
                return Err("el archivo no tiene video".into());
            }
            let (mut w, mut h) = (0u32, 0u32);
            engine.GetNativeVideoSize(Some(&mut w), Some(&mut h)).map_err(win)?;
            if w == 0 || h == 0 {
                let _ = engine.Shutdown();
                return Err("el video no dice sus medidas".into());
            }
            let duration = engine.GetDuration();
            let desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut texture: Option<ID3D11Texture2D> = None;
            gfx.device.CreateTexture2D(&desc, None, Some(&mut texture)).map_err(win)?;
            let texture = texture.ok_or("Direct3D no creó la textura del cuadro")?;
            Ok(Self {
                engine,
                gfx,
                shared,
                texture,
                duration: if duration.is_finite() { duration.max(0.0) } else { 0.0 },
                size: (w, h),
                last_pts: i64::MIN,
                serial: 0,
                refresh: true,
                seeks_seen: 0,
                has_frame: false,
                rate: Cell::new(1.0),
            })
        }
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Las medidas del video, en píxeles.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn playing(&self) -> bool {
        // SAFETY: getters de un motor vivo.
        unsafe { !self.engine.IsPaused().as_bool() && !self.engine.IsEnded().as_bool() }
    }

    pub fn play(&self) {
        // Terminado: vuelve a empezar.
        if self.time() >= self.duration - 0.05 {
            self.seek(0.0);
        }
        // SAFETY: motor vivo.
        unsafe {
            let _ = self.engine.SetPlaybackRate(self.rate.get());
            let _ = self.engine.Play();
        }
    }

    /// La velocidad de reproducción (1.0 normal). Se aplica en el acto si está sonando y
    /// `play` la respeta después.
    pub fn set_rate(&self, rate: f64) {
        let rate = if rate.is_finite() { rate.clamp(0.1, 16.0) } else { 1.0 };
        self.rate.set(rate);
        // SAFETY: motor vivo.
        unsafe {
            let _ = self.engine.SetDefaultPlaybackRate(rate);
            if self.playing() {
                let _ = self.engine.SetPlaybackRate(rate);
            }
        }
    }

    /// El volumen, de 0.0 (mudo) a 1.0.
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        // SAFETY: motor vivo.
        unsafe {
            let _ = self.engine.SetVolume(volume as f64);
        }
    }

    pub fn pause(&self) {
        // SAFETY: motor vivo.
        unsafe {
            let _ = self.engine.Pause();
        }
    }

    /// Dónde va, en segundos.
    pub fn time(&self) -> f64 {
        // SAFETY: getter de un motor vivo.
        let now = unsafe { self.engine.GetCurrentTime() };
        if now.is_finite() {
            now.clamp(0.0, self.duration.max(0.0))
        } else {
            0.0
        }
    }

    /// Salta justo a `secs` (el cuadro exacto al arrastrar el cabezal).
    pub fn seek(&self, secs: f64) {
        // SAFETY: motor vivo.
        unsafe {
            let _ = self.engine.SetCurrentTime(secs.clamp(0.0, self.duration.max(0.0)));
        }
    }

    /// Ya llegó algún cuadro.
    pub fn has_frame(&self) -> bool {
        self.has_frame
    }

    /// El cuadro de ahora; si no hay uno nuevo, el último.
    pub fn frame(&mut self) -> Option<HardwareFrame> {
        // Un salto terminado: el motor ya tiene el cuadro nuevo aunque no lo "anuncie".
        if let Ok(events) = self.shared.events.lock() {
            if events.seeks != self.seeks_seen {
                self.seeks_seen = events.seeks;
                self.refresh = true;
            }
        }
        // SAFETY: motor y textura vivos; el contexto del dispositivo es seguro entre hilos.
        unsafe {
            // `OnVideoStreamTick` da la marca del cuadro nuevo; sin cuadro nuevo devuelve S_FALSE
            // (que la API deja como `Ok` con la marca en 0): una marca igual a la anterior, o 0,
            // es "nada nuevo".
            let pts = self.engine.OnVideoStreamTick().unwrap_or(0);
            let fresh = self.refresh || (pts != 0 && pts != self.last_pts);
            if fresh {
                let rect = RECT { left: 0, top: 0, right: self.size.0 as i32, bottom: self.size.1 as i32 };
                if self.engine.TransferVideoFrame(&self.texture, None, &rect, None).is_ok() {
                    self.serial += 1;
                    self.has_frame = true;
                    self.refresh = false;
                    if pts != 0 {
                        self.last_pts = pts;
                    }
                }
            }
        }
        self.has_frame.then(|| HardwareFrame { texture: self.texture.clone(), serial: self.serial, size: self.size })
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        // SAFETY: apaga el motor antes de soltar la sesión de Media Foundation.
        unsafe {
            let _ = self.engine.Pause();
            let _ = self.engine.Shutdown();         }
        let _ = &self.gfx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un video de verdad, a mano: `ATIC_TEST_VIDEO=<video>`. Abre, salta y guarda el cuadro como PNG.
    #[test]
    #[ignore]
    fn abre_salta_y_entrega_un_cuadro() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let mut player = VideoPlayer::open(Path::new(&source)).expect("abrir");
        println!("{} s, {:?}", player.duration(), player.size());
        assert!(player.duration() > 0.0);
        let at = (player.duration() / 2.0).min(2.0);
        player.seek(at);
        let started = Instant::now();
        let mut frame = None;
        while started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(30));
            frame = player.frame();
            if frame.is_some() && (player.time() - at).abs() < 0.2 && started.elapsed() > Duration::from_millis(300) {
                break;
            }
        }
        let frame = frame.expect("un cuadro");
        println!("cuadro {}x{} en {:.2} s (pidió {at:.2})", frame.width(), frame.height(), player.time());
        let image = frame.bgra().expect("leer la textura");
        let png = image.to_png().expect("png");
        let out = std::env::temp_dir().join("atic-player-test.png");
        std::fs::write(&out, png).expect("escribir");
        println!("guardado en {}", out.display());
        // Suena un rato y el reloj avanza.
        player.play();         std::thread::sleep(Duration::from_millis(600));
        assert!(player.playing());
        let moved = player.time();
        player.pause();         assert!(moved > at, "el reloj no avanzó: {moved} <= {at}");
    }
}
