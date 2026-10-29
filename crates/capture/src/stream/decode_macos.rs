//! H.264 a `CVPixelBuffer` con VideoToolbox: la pantalla del celular en el PC, sin pasar
//! los píxeles por la CPU.
//!
//! Sale en NV12 de rango completo (`420f`) y respaldado por IOSurface, que es lo que
//! pinta GPUI directo en Metal (`gpui::surface`): sin convertir a BGRA ni subir una
//! imagen nueva por cuadro. El renderer de GPUI exige ese formato (si no, se cae): por
//! eso se revisa cada cuadro y uno distinto es un error, y quien lo usa vuelve a
//! OpenH264.
//!
//! El celular manda SPS/PPS una vez al empezar (el «csd» de MediaCodec) y los
//! keyframes pueden venir sin ellos: se guardan y la sesión se rehace solo si cambian.

use std::ffi::c_void;

use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_video::pixel_buffer::{CVPixelBuffer, CVPixelBufferRef};
use objc2_core_media::CMTime;

/// `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` ('420f'): lo único que pinta
/// `gpui::surface`.
const NV12_FULL: u32 = u32::from_be_bytes(*b"420f");

type Session = *mut c_void;
type Description = *const c_void;

type DecodedCallback = extern "C" fn(refcon: *mut c_void, frame_refcon: *mut c_void, status: i32, flags: u32, image: *const c_void, pts: CMTime, duration: CMTime);

#[repr(C)]
struct CallbackRecord {
    callback: DecodedCallback,
    refcon: *mut c_void,
}

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    fn VTDecompressionSessionCreate(
        allocator: CFTypeRef,
        description: Description,
        decoder_specification: CFDictionaryRef,
        destination_attributes: CFDictionaryRef,
        callback: *const CallbackRecord,
        session: *mut Session,
    ) -> i32;
    fn VTDecompressionSessionDecodeFrame(session: Session, sample: *const c_void, flags: u32, frame_refcon: *mut c_void, info: *mut u32) -> i32;
    fn VTDecompressionSessionCanAcceptFormatDescription(session: Session, description: Description) -> u8;
    fn VTDecompressionSessionInvalidate(session: Session);
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
        allocator: CFTypeRef,
        count: usize,
        sets: *const *const u8,
        sizes: *const usize,
        header_length: i32,
        description: *mut Description,
    ) -> i32;
    fn CMBlockBufferCreateWithMemoryBlock(
        allocator: CFTypeRef,
        memory: *mut c_void,
        length: usize,
        block_allocator: CFTypeRef,
        custom_source: *const c_void,
        offset: usize,
        data_length: usize,
        flags: u32,
        block: *mut *const c_void,
    ) -> i32;
    fn CMBlockBufferReplaceDataBytes(source: *const c_void, block: *const c_void, offset: usize, length: usize) -> i32;
    fn CMSampleBufferCreateReady(
        allocator: CFTypeRef,
        data: *const c_void,
        description: Description,
        samples: isize,
        timing_entries: isize,
        timing: *const c_void,
        size_entries: isize,
        sizes: *const usize,
        sample: *mut *const c_void,
    ) -> i32;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    static kCVPixelBufferMetalCompatibilityKey: CFStringRef;
    fn CVPixelBufferRetain(buffer: *const c_void) -> *const c_void;
    fn CVPixelBufferRelease(buffer: *const c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// `kCMBlockBufferAssureMemoryNowFlag`: la memoria del bloque se reserva al crearlo.
const ASSURE_MEMORY_NOW: u32 = 1;

/// Un cuadro decodificado, listo para `gpui::surface`.
pub struct HardwareFrame(pub CVPixelBuffer);

// SAFETY: un `CVPixelBuffer` es un objeto de CoreFoundation con conteo de referencias
// atómico; el decodificador ya no lo toca después de entregarlo.
unsafe impl Send for HardwareFrame {}

impl HardwareFrame {
    pub fn width(&self) -> u32 {
        self.0.get_width() as u32
    }

    pub fn height(&self) -> u32 {
        self.0.get_height() as u32
    }
}

/// El decodificador de hardware. La sesión se crea con el primer SPS/PPS.
pub struct HardwareDecoder {
    sps: Vec<u8>,
    pps: Vec<u8>,
    description: Description,
    session: Session,
}

// SAFETY: la sesión y la descripción solo las usa quien tiene el decodificador
// (`&mut self`); VideoToolbox no exige un hilo en particular.
unsafe impl Send for HardwareDecoder {}

impl Default for HardwareDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareDecoder {
    pub fn new() -> Self {
        Self { sps: Vec::new(), pps: Vec::new(), description: std::ptr::null(), session: std::ptr::null_mut() }
    }

    /// Un cuadro del canal (Annex-B). `Ok(None)` mientras no hay imagen (solo SPS/PPS, o
    /// todavía no llegaron). `Err` si VideoToolbox no puede: quien lo usa cambia a
    /// OpenH264.
    pub fn decode(&mut self, annex_b: &[u8]) -> Result<Option<HardwareFrame>, String> {
        let mut avcc = Vec::with_capacity(annex_b.len() + 16);
        let (mut sps, mut pps) = (None, None);
        for nal in nal_payloads(annex_b) {
            match nal[0] & 0x1f {
                7 => sps = Some(nal),
                8 => pps = Some(nal),
                // Cortes de imagen (con y sin keyframe): van a la muestra, en AVCC.
                1 | 5 => {
                    avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                    avcc.extend_from_slice(nal);
                }
                _ => {}
            }
        }
        // Pueden llegar juntos o en cuadros distintos: la sesión se rehace cuando están
        // los dos y alguno cambió.
        let mut changed = false;
        if let Some(sps) = sps.filter(|sps| *sps != self.sps.as_slice()) {
            self.sps = sps.to_vec();
            changed = true;
        }
        if let Some(pps) = pps.filter(|pps| *pps != self.pps.as_slice()) {
            self.pps = pps.to_vec();
            changed = true;
        }
        if changed && !self.sps.is_empty() && !self.pps.is_empty() {
            self.reconfigure()?;
        }
        if avcc.is_empty() || self.session.is_null() {
            return Ok(None);
        }
        let mut decoded: *const c_void = std::ptr::null();
        // SAFETY: bloque y muestra se crean, se usan en una decodificación sincrónica
        // (sin `EnableAsynchronousDecompression`: el callback corre antes de volver) y se
        // sueltan; `decoded` recibe un búfer retenido (+1).
        let status = unsafe {
            let mut block: *const c_void = std::ptr::null();
            let status = CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null(),
                std::ptr::null_mut(),
                avcc.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                avcc.len(),
                ASSURE_MEMORY_NOW,
                &mut block,
            );
            if status != 0 {
                return Err(format!("CMBlockBuffer ({status})"));
            }
            let status = CMBlockBufferReplaceDataBytes(avcc.as_ptr().cast(), block, 0, avcc.len());
            let mut sample: *const c_void = std::ptr::null();
            let size = avcc.len();
            let status = if status == 0 {
                CMSampleBufferCreateReady(std::ptr::null(), block, self.description, 1, 0, std::ptr::null(), 1, &size, &mut sample)
            } else {
                status
            };
            CFRelease(block);
            if status != 0 {
                return Err(format!("CMSampleBuffer ({status})"));
            }
            let status = VTDecompressionSessionDecodeFrame(self.session, sample, 0, (&mut decoded as *mut *const c_void).cast(), std::ptr::null_mut());
            CFRelease(sample);
            status
        };
        if status != 0 {
            if !decoded.is_null() {
                // SAFETY: retenido por el callback.
                unsafe { CVPixelBufferRelease(decoded) };
            }
            return Err(format!("VideoToolbox no decodificó el cuadro ({status})"));
        }
        if decoded.is_null() {
            return Ok(None);
        }
        // SAFETY: el callback lo retuvo (+1): pasa a ser nuestro.
        let buffer = unsafe { CVPixelBuffer::wrap_under_create_rule(decoded as CVPixelBufferRef) };
        if buffer.get_pixel_format() != NV12_FULL {
            return Err(format!("VideoToolbox entregó otro formato: {:#x}", buffer.get_pixel_format()));
        }
        Ok(Some(HardwareFrame(buffer)))
    }

    /// SPS/PPS nuevos: otra descripción y, si la sesión no la acepta, otra sesión.
    fn reconfigure(&mut self) -> Result<(), String> {
        let sets = [self.sps.as_ptr(), self.pps.as_ptr()];
        let sizes = [self.sps.len(), self.pps.len()];
        let mut description: Description = std::ptr::null();
        // SAFETY: los punteros y largos son de los vectores vivos; la descripción sale +1.
        let status = unsafe { CMVideoFormatDescriptionCreateFromH264ParameterSets(std::ptr::null(), 2, sets.as_ptr(), sizes.as_ptr(), 4, &mut description) };
        if status != 0 || description.is_null() {
            return Err(format!("SPS/PPS ilegibles ({status})"));
        }
        // SAFETY: sesión viva; la descripción vieja se suelta después de reemplazarla.
        unsafe {
            if !self.session.is_null() && VTDecompressionSessionCanAcceptFormatDescription(self.session, description) == 0 {
                self.close_session();
            }
            if !self.description.is_null() {
                CFRelease(self.description);
            }
        }
        self.description = description;
        if self.session.is_null() {
            self.session = create_session(description)?;
        }
        Ok(())
    }

    fn close_session(&mut self) {
        if !self.session.is_null() {
            // SAFETY: después de invalidar no hay más callbacks.
            unsafe {
                VTDecompressionSessionInvalidate(self.session);
                CFRelease(self.session);
            }
            self.session = std::ptr::null_mut();
        }
    }
}

impl Drop for HardwareDecoder {
    fn drop(&mut self) {
        self.close_session();
        if !self.description.is_null() {
            // SAFETY: la descripción es nuestra (+1).
            unsafe { CFRelease(self.description) };
        }
    }
}

fn create_session(description: Description) -> Result<Session, String> {
    // SAFETY: constantes de CoreVideo; diccionarios armados con core-foundation.
    let attributes = unsafe {
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
    let record = CallbackRecord { callback: decoded, refcon: std::ptr::null_mut() };
    let mut session: Session = std::ptr::null_mut();
    // SAFETY: la descripción está viva; VideoToolbox copia el registro del callback.
    let status = unsafe { VTDecompressionSessionCreate(std::ptr::null(), description, std::ptr::null(), attributes.as_concrete_TypeRef(), &record, &mut session) };
    if status != 0 || session.is_null() {
        return Err(format!("VideoToolbox no creó el decodificador ({status})"));
    }
    Ok(session)
}

/// Deja el cuadro decodificado (retenido) donde apunta `frame_refcon`.
extern "C" fn decoded(_refcon: *mut c_void, frame_refcon: *mut c_void, status: i32, _flags: u32, image: *const c_void, _pts: CMTime, _duration: CMTime) {
    if status != 0 || image.is_null() || frame_refcon.is_null() {
        return;
    }
    let slot = frame_refcon as *mut *const c_void;
    // SAFETY: `frame_refcon` apunta al `decoded` de `HardwareDecoder::decode`, vivo
    // durante la decodificación sincrónica; si ya había uno (varios cortes), se suelta.
    unsafe {
        if !(*slot).is_null() {
            CVPixelBufferRelease(*slot);
        }
        *slot = CVPixelBufferRetain(image);
    }
}

/// Las unidades NAL de un tramo Annex-B, sin su código de inicio.
fn nal_payloads(annex_b: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= annex_b.len() {
        if annex_b[i] == 0 && annex_b[i + 1] == 0 && annex_b[i + 2] == 1 {
            starts.push((i, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut nals = Vec::with_capacity(starts.len());
    for (k, &(_, from)) in starts.iter().enumerate() {
        let mut to = starts.get(k + 1).map_or(annex_b.len(), |&(code, _)| code);
        // El cero de más de un código de 4 bytes (`00 00 00 01`) no es del NAL anterior.
        while to > from && annex_b[to - 1] == 0 && starts.get(k + 1).is_some() {
            to -= 1;
        }
        if to > from {
            nals.push(&annex_b[from..to]);
        }
    }
    nals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_los_nal_de_annex_b() {
        let bytes = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 0];
        let nals = nal_payloads(&bytes);
        assert_eq!(nals, vec![&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 0][..]]);
    }

    /// Un H.264 de verdad (`ATIC_TEST_H264`, Annex-B; p. ej. de ffmpeg con
    /// `-f h264`), de a una unidad NAL como lo manda la prueba de la pill: a mano.
    #[test]
    #[ignore]
    fn decodifica_un_video_annex_b_por_hardware() {
        let bytes = std::fs::read(std::env::var("ATIC_TEST_H264").expect("ATIC_TEST_H264")).unwrap();
        let mut decoder = HardwareDecoder::new();
        let started = std::time::Instant::now();
        let mut shown = 0;
        for nal in nal_payloads(&bytes) {
            let mut annex_b = vec![0, 0, 0, 1];
            annex_b.extend_from_slice(nal);
            if let Some(frame) = decoder.decode(&annex_b).unwrap() {
                assert_eq!(frame.0.get_pixel_format(), NV12_FULL);
                shown += 1;
            }
        }
        println!("{shown} cuadros, {:.2} ms cada uno", started.elapsed().as_secs_f32() * 1000. / shown.max(1) as f32);
        assert!(shown >= 150, "{shown} cuadros");
    }

    /// Lo que sale de VideoToolbox (la pantalla codificada por `ScreenStream`) se vuelve
    /// a leer por hardware, en el formato que pinta GPUI. A mano: pide el permiso de
    /// grabar pantalla.
    #[test]
    #[ignore]
    fn la_pantalla_codificada_se_decodifica_por_hardware() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        let monitor = crate::monitors::enumerate().into_iter().find(|m| m.is_primary).unwrap();
        let frames: Arc<Mutex<Vec<(u8, Vec<u8>)>>> = Arc::default();
        let sink = frames.clone();
        let stream = super::super::ScreenStream::start(&monitor, 1280, 4_000_000, 30, Box::new(move |f, d| sink.lock().unwrap().push((f, d))), Box::new(|| true)).expect("empieza");
        std::thread::sleep(Duration::from_millis(1500));
        stream.request_keyframe();
        std::thread::sleep(Duration::from_millis(300));
        drop(stream);
        let frames = std::mem::take(&mut *frames.lock().unwrap());
        let mut decoder = HardwareDecoder::new();
        let (mut shown, started) = (0, Instant::now());
        for (flags, data) in &frames[1..] {
            if let Some(frame) = decoder.decode(data).unwrap_or_else(|e| panic!("cuadro {flags}: {e}")) {
                assert_eq!((frame.width(), frame.height()), (1280, 800));
                shown += 1;
            }
        }
        println!("{shown} de {} cuadros, {:.2} ms cada uno", frames.len() - 1, started.elapsed().as_secs_f32() * 1000. / shown.max(1) as f32);
        assert!(shown > 0);
    }
}
