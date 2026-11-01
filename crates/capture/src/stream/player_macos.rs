//! Reproducir un video para editarlo: AVPlayer (con su audio) y cada cuadro como
//! `CVPixelBuffer` en NV12 de rango completo respaldado por IOSurface, que
//! `gpui::surface` dibuja sin pasar por la CPU (como [`super::HardwareFrame`] del
//! celular).
//!
//! Se usa desde el hilo de la ventana: AVPlayer no necesita otro, y quien dibuja
//! pide el cuadro del momento en cada pasada (`frame`).

use std::cell::Cell;
use std::ffi::c_void;
use std::path::Path;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_video::pixel_buffer::{CVPixelBuffer, CVPixelBufferRef};
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::msg_send;
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSString};

use super::HardwareFrame;

/// `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` ('420f'): lo que pinta `gpui::surface`.
const NV12_FULL: u32 = u32::from_be_bytes(*b"420f");

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeVideo: CFStringRef;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    static kCVPixelBufferMetalCompatibilityKey: CFStringRef;
}

#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    fn CACurrentMediaTime() -> f64;
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}
// SAFETY: la misma disposición que `CGSize`.
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("sin {}", name.to_string_lossy()))
}

fn seconds(time: CMTime) -> f64 {
    if time.timescale > 0 && time.flags.contains(CMTimeFlags::Valid) {
        time.value as f64 / time.timescale as f64
    } else {
        0.0
    }
}

fn time(secs: f64) -> CMTime {
    CMTime { value: (secs.max(0.0) * 600.0).round() as i64, timescale: 600, flags: CMTimeFlags::Valid, epoch: 0 }
}

const ZERO: CMTime = CMTime { value: 0, timescale: 1, flags: CMTimeFlags::Valid, epoch: 0 };

/// Un video abierto para reproducir.
pub struct VideoPlayer {
    player: Retained<AnyObject>,
    output: Retained<AnyObject>,
    duration: f64,
    size: (u32, u32),
    /// El último cuadro: se sigue mostrando mientras no llegue otro.
    last: Option<CVPixelBuffer>,
    /// Cuándo se volvió a pedir el cuadro de ahora, esperando el primero.
    primed: Option<std::time::Instant>,
    /// La velocidad de reproducción (1.0 normal); `play` la respeta.
    rate: Cell<f64>,
}

impl VideoPlayer {
    pub fn open(path: &Path) -> Result<Self, String> {
        objc2::rc::autoreleasepool(|_| unsafe { Self::open_inner(path) })
    }

    /// SAFETY: objetos de AVFoundation creados y configurados aquí.
    unsafe fn open_inner(path: &Path) -> Result<Self, String> {
        let url: Retained<AnyObject> = msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&path.to_string_lossy())];
        let none: *const AnyObject = std::ptr::null();
        let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*url, options: none];
        let duration = seconds(msg_send![&asset, duration]);
        let tracks: Retained<NSArray<AnyObject>> = msg_send![&asset, tracksWithMediaType: &*(AVMediaTypeVideo as *const AnyObject)];
        let track = tracks.firstObject().ok_or("el archivo no tiene video")?;
        let natural: CGSize = msg_send![&track, naturalSize];
        let size = (natural.width.round() as u32, natural.height.round() as u32);

        let attributes = {
            let format = CFString::wrap_under_get_rule(kCVPixelBufferPixelFormatTypeKey);
            let iosurface = CFString::wrap_under_get_rule(kCVPixelBufferIOSurfacePropertiesKey);
            let metal = CFString::wrap_under_get_rule(kCVPixelBufferMetalCompatibilityKey);
            let empty: CFDictionary<CFString, CFNumber> = CFDictionary::from_CFType_pairs(&[]);
            CFDictionary::from_CFType_pairs(&[
                (format, CFNumber::from(NV12_FULL as i32).as_CFType()),
                (iosurface, empty.as_CFType()),
                (metal, CFBoolean::true_value().as_CFType()),
            ])
        };
        let output: Allocated<AnyObject> = msg_send![class(c"AVPlayerItemVideoOutput")?, alloc];
        let output: Option<Retained<AnyObject>> =
            msg_send![output, initWithPixelBufferAttributes: &*(attributes.as_concrete_TypeRef() as *const AnyObject)];
        let output = output.ok_or("AVFoundation no armó la salida de video")?;
        let item: Retained<AnyObject> = msg_send![class(c"AVPlayerItem")?, playerItemWithAsset: &*asset];
        let _: () = msg_send![&item, addOutput: &*output];
        let player: Retained<AnyObject> = msg_send![class(c"AVPlayer")?, playerWithPlayerItem: &*item];
        Ok(Self { player, output, duration, size, last: None, primed: None, rate: Cell::new(1.0) })
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Las medidas del video, en píxeles.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn playing(&self) -> bool {
        // SAFETY: getter de un AVPlayer vivo.
        let rate: f32 = unsafe { msg_send![&self.player, rate] };
        rate != 0.0
    }

    pub fn play(&self) {
        // Terminado: vuelve a empezar.
        if self.time() >= self.duration - 0.05 {
            self.seek(0.0);
        }
        // `setRate:` con algo distinto de 0 también empieza a reproducir (como `play`, pero
        // a la velocidad guardada).
        // SAFETY: AVPlayer vivo.
        unsafe {
            let _: () = msg_send![&self.player, setRate: self.rate.get() as f32];
        }
    }

    /// La velocidad de reproducción (1.0 normal). Se aplica en el acto si está sonando y
    /// `play` la respeta después.
    pub fn set_rate(&self, rate: f64) {
        let rate = if rate.is_finite() { rate.clamp(0.1, 16.0) } else { 1.0 };
        self.rate.set(rate);
        if self.playing() {
            // SAFETY: AVPlayer vivo.
            unsafe {
                let _: () = msg_send![&self.player, setRate: rate as f32];
            }
        }
    }

    /// El volumen, de 0.0 (mudo) a 1.0.
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        // SAFETY: AVPlayer vivo.
        unsafe {
            let _: () = msg_send![&self.player, setVolume: volume];
        }
    }

    pub fn pause(&self) {
        // SAFETY: AVPlayer vivo.
        unsafe {
            let _: () = msg_send![&self.player, pause];
        }
    }

    /// Dónde va, en segundos.
    pub fn time(&self) -> f64 {
        // SAFETY: getter de un AVPlayer vivo.
        seconds(unsafe { msg_send![&self.player, currentTime] })
    }

    /// Salta justo a `secs` (sin tolerancia: al arrastrar el cabezal se ve el cuadro exacto).
    pub fn seek(&self, secs: f64) {
        let to = time(secs.clamp(0.0, self.duration.max(0.0)));
        // SAFETY: AVPlayer vivo.
        unsafe {
            let _: () = msg_send![&self.player, seekToTime: to, toleranceBefore: ZERO, toleranceAfter: ZERO];
        }
    }

    /// Ya llegó algún cuadro.
    pub fn has_frame(&self) -> bool {
        self.last.is_some()
    }

    /// El cuadro de ahora, para `gpui::surface`; si no hay uno nuevo, el último.
    pub fn frame(&mut self) -> Option<HardwareFrame> {
        // Sin ningún cuadro todavía: un salto pedido antes de que el video estuviera
        // listo se pierde; se repite cada tanto hasta que llegue el primero.
        if self.last.is_none() && !self.playing() && self.primed.is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(250)) {
            self.primed = Some(std::time::Instant::now());
            // Saltar al mismo punto no entrega cuadro: un milisegundo más allá sí.
            self.seek(self.time() + 0.001);
        }
        // SAFETY: la salida está viva; el búfer copiado llega con +1 y queda en `last`.
        unsafe {
            let at: CMTime = msg_send![&self.output, itemTimeForHostTime: CACurrentMediaTime()];
            let fresh: Bool = msg_send![&self.output, hasNewPixelBufferForItemTime: at];
            if fresh.as_bool() {
                let shown: *mut CMTime = std::ptr::null_mut();
                let pixels: *mut CVBuffer = msg_send![&self.output, copyPixelBufferForItemTime: at, itemTimeForDisplay: shown];
                if !pixels.is_null() {
                    self.last = Some(CVPixelBuffer::wrap_under_create_rule(pixels as *mut c_void as CVPixelBufferRef));
                }
            }
        }
        self.last.clone().map(HardwareFrame)
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        self.pause();
    }
}

