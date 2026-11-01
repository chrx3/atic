//! De un MP4 ya exportado a GIF: AVAssetImageGenerator saca un cuadro a la vez, a tiempos
//! fijos, y ImageIO (`com.compuserve.gif`) los va sumando al archivo. El cuantizador de
//! color es el del sistema (paleta propia por cuadro).

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::CFURL;
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::msg_send;
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_foundation::{NSArray, NSError, NSString};

/// Más de esto no se gana nada en un GIF (y los navegadores lo recortan).
const MAX_FPS: u32 = 30;
/// ImageIO guarda los cuadros ya codificados hasta cerrar el archivo: este tope acota la
/// memoria. Si el video es largo, baja el ritmo en vez de pasarse.
const MAX_FRAMES: usize = 1500;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeVideo: CFStringRef;
}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    static kCGImagePropertyGIFDictionary: CFStringRef;
    static kCGImagePropertyGIFLoopCount: CFStringRef;
    static kCGImagePropertyGIFDelayTime: CFStringRef;
    static kCGImagePropertyGIFUnclampedDelayTime: CFStringRef;
    fn CGImageDestinationCreateWithURL(url: *const c_void, kind: *const c_void, count: usize, options: *const c_void) -> *mut c_void;
    fn CGImageDestinationSetProperties(destination: *mut c_void, properties: *const c_void);
    fn CGImageDestinationAddImage(destination: *mut c_void, image: *const c_void, properties: *const c_void);
    fn CGImageDestinationFinalize(destination: *mut c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// `CGImageRef`, con su codificación para objc2.
#[repr(C)]
struct CGImageRaw {
    _private: [u8; 0],
}
// SAFETY: la codificación del puntero de C (`^{CGImage=}`).
unsafe impl RefEncode for CGImageRaw {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
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

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("sin {}", name.to_string_lossy()))
}

fn key(key: CFStringRef) -> CFString {
    // SAFETY: constante de un framework.
    unsafe { CFString::wrap_under_get_rule(key) }
}

fn dict(pairs: &[(CFString, CFType)]) -> CFDictionary<CFString, CFType> {
    CFDictionary::from_CFType_pairs(pairs)
}

fn error_text(error: *mut AnyObject) -> String {
    // SAFETY: un `NSError` o nulo.
    unsafe { (error as *mut NSError).as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default()
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

/// Cada cuánto va un cuadro, en centésimas de segundo (lo único que sabe un GIF), y cuántos
/// salen: `fps` como mucho (con tope en [`MAX_FPS`]) y nunca más de [`MAX_FRAMES`].
fn plan(duration: f64, fps: u32) -> (u32, usize) {
    let fps = fps.clamp(1, MAX_FPS);
    let mut step = ((100.0 / fps as f64).round() as u32).max(1);
    let total = duration.max(0.0) * 100.0;
    if (total / step as f64).ceil() > MAX_FRAMES as f64 {
        step = (total / MAX_FRAMES as f64).ceil() as u32;
    }
    let frames = ((total / step as f64).ceil() as usize).max(1);
    (step, frames)
}

/// `max_width` de ancho como mucho (con su proporción) y `fps` cuadros por segundo.
/// `progress` recibe de 0 a 1; si `cancel` se activa, borra lo escrito y devuelve `Err`.
pub fn export_gif(source: &Path, out: &Path, max_width: u32, fps: u32, progress: &dyn Fn(f64), cancel: &AtomicBool) -> Result<(), String> {
    let _ = std::fs::remove_file(out);
    let result = objc2::rc::autoreleasepool(|_| gif_inner(source, out, max_width, fps, progress, cancel));
    if result.is_err() {
        // Ni un archivo a medias ni uno viejo que se confunda con el nuevo.
        let _ = std::fs::remove_file(out);
        remove_partials(out);
    }
    result
}

/// ImageIO escribe a un temporal oculto junto al destino (`.<nombre>-<azar>`) y lo renombra
/// al cerrar el archivo: si se cancela o falla antes, ese temporal queda ahí.
fn remove_partials(out: &Path) {
    let (Some(dir), Some(name)) = (out.parent(), out.file_name()) else { return };
    let prefix = format!(".{}-", name.to_string_lossy());
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn gif_inner(source: &Path, out: &Path, max_width: u32, fps: u32, progress: &dyn Fn(f64), cancel: &AtomicBool) -> Result<(), String> {
    // SAFETY: objetos de AVFoundation e ImageIO creados y usados aquí; los selectores son
    // los de AVAssetImageGenerator y los retornos se comprueban.
    unsafe {
        let url: Retained<AnyObject> = msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&source.to_string_lossy())];
        let none: *const AnyObject = std::ptr::null();
        let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*url, options: none];
        let duration = seconds(msg_send![&asset, duration]);
        let tracks: Retained<NSArray<AnyObject>> = msg_send![&asset, tracksWithMediaType: &*(AVMediaTypeVideo as *const AnyObject)];
        let track = tracks.firstObject().ok_or("el archivo no tiene video")?;
        let natural: CGSize = msg_send![&track, naturalSize];
        if duration <= 0.0 || natural.width < 1.0 || natural.height < 1.0 {
            return Err("el video no tiene duración".into());
        }

        let (step, count) = plan(duration, fps);
        let step_secs = step as f64 / 100.0;

        let generator: Retained<AnyObject> = msg_send![class(c"AVAssetImageGenerator")?, assetImageGeneratorWithAsset: &*asset];
        let _: () = msg_send![&generator, setAppliesPreferredTrackTransform: Bool::YES];
        if natural.width > max_width as f64 && max_width > 0 {
            let to = CGSize { width: max_width as f64, height: (natural.height * max_width as f64 / natural.width).round().max(1.0) };
            let _: () = msg_send![&generator, setMaximumSize: to];
        }
        // Media separación entre cuadros: da el cuadro justo cuando cabe y no obliga a
        // decodificar desde el último cuadro clave cuando no.
        let tolerance = cmtime(step_secs / 2.0);
        let _: () = msg_send![&generator, setRequestedTimeToleranceBefore: tolerance];
        let _: () = msg_send![&generator, setRequestedTimeToleranceAfter: tolerance];

        let target = CFURL::from_path(out, false).ok_or("ruta de salida inválida")?;
        let gif = CFString::from_static_string("com.compuserve.gif");
        let destination = CGImageDestinationCreateWithURL(target.as_concrete_TypeRef() as *const c_void, gif.as_concrete_TypeRef() as *const c_void, count, std::ptr::null());
        if destination.is_null() {
            return Err("ImageIO no pudo crear el GIF".into());
        }
        let _destination = Released(destination as *const c_void);

        // Bucle infinito (0) en el archivo; el retardo, en cada cuadro.
        let file_gif = dict(&[(key(kCGImagePropertyGIFLoopCount), CFNumber::from(0i32).as_CFType())]);
        let file_props = dict(&[(key(kCGImagePropertyGIFDictionary), file_gif.as_CFType())]);
        CGImageDestinationSetProperties(destination, file_props.as_concrete_TypeRef() as *const c_void);
        let delay = CFNumber::from(step_secs);
        let frame_gif = dict(&[
            (key(kCGImagePropertyGIFDelayTime), delay.as_CFType()),
            (key(kCGImagePropertyGIFUnclampedDelayTime), delay.as_CFType()),
        ]);
        let frame_props = dict(&[(key(kCGImagePropertyGIFDictionary), frame_gif.as_CFType())]);

        // El cuadro anterior se guarda por si uno falla: se repite en vez de desajustar la
        // cuenta de cuadros que ya se le dijo a ImageIO.
        let mut last: Option<Released> = None;
        for i in 0..count {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelada".into());
            }
            objc2::rc::autoreleasepool(|_| -> Result<(), String> {
                let at = cmtime((i as f64 * step_secs).min((duration - 0.001).max(0.0)));
                let actual: *mut CMTime = std::ptr::null_mut();
                let mut error: *mut AnyObject = std::ptr::null_mut();
                let image: *mut CGImageRaw = msg_send![&generator, copyCGImageAtTime: at, actualTime: actual, error: &mut error];
                if !image.is_null() {
                    last = Some(Released(image as *const c_void));
                }
                let Some(frame) = &last else {
                    return Err(format!("no pude leer el video: {}", error_text(error)));
                };
                CGImageDestinationAddImage(destination, frame.0, frame_props.as_concrete_TypeRef() as *const c_void);
                Ok(())
            })?;
            progress(((i + 1) as f64 / count as f64) * 0.98);
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelada".into());
        }
        // Aquí se escribe el archivo.
        if !CGImageDestinationFinalize(destination) {
            return Err("ImageIO no pudo escribir el GIF".into());
        }
        progress(1.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_ritmo_se_redondea_a_centesimas() {
        // 15 fps → 6,67 cs → 7 cs; 10 s → ceil(1000 / 7) = 143 cuadros.
        assert_eq!(plan(10.0, 15), (7, 143));
        // 10 fps → 10 cs exactos.
        assert_eq!(plan(2.0, 10), (10, 20));
    }

    #[test]
    fn el_ritmo_tiene_tope_de_30_fps() {
        // 60 fps pedidos se tratan como 30 → 3 cs.
        assert_eq!(plan(1.0, 60).0, 3);
        assert_eq!(plan(1.0, 0).0, 100);
    }

    #[test]
    fn un_video_largo_baja_el_ritmo_para_no_pasar_de_1500_cuadros() {
        // 300 s a 30 fps serían 10000 cuadros: el paso sube a 20 cs → 1500 cuadros.
        assert_eq!(plan(300.0, 30), (20, 1500));
        let (step, frames) = plan(3600.0, 15);
        assert!(frames <= MAX_FRAMES && step > 7);
    }

    #[test]
    fn un_video_cortisimo_da_al_menos_un_cuadro() {
        assert_eq!(plan(0.0, 15).1, 1);
        assert_eq!(plan(0.01, 15).1, 1);
    }
}
