//! «Texto» del estante y tapar datos en macOS: Vision
//! (`VNRecognizeTextRequest`), en los idiomas preferidos del sistema. Sin
//! modelos aparte ni permisos.
//!
//! El modo preciso compila sus modelos de Core ML la primera vez (puede
//! tardar sin caché): mientras se precalienta en otro hilo, el OCR usa el
//! rápido, que responde en menos de un segundo.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, sel};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSLocale, NSRange, NSRect, NSString};

// Para que `AnyClass::get` encuentre las clases de Vision.
#[link(name = "Vision", kind = "framework")]
extern "C" {}

/// Una palabra leída y su caja, en píxeles de la imagen.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Lee el texto de una imagen BGRA, de arriba abajo. Corre en un hilo de fondo.
pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, String> {
    let lines = read(width, height, bgra, false)?;
    Ok(lines.into_iter().map(|line| line.text).collect::<Vec<_>>().join("\n"))
}

/// Las líneas leídas, cada una con sus palabras y dónde están (para tapar
/// justo lo que hay que tapar). Corre en un hilo de fondo.
pub fn recognize_lines(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<Vec<Word>>, String> {
    let lines = read(width, height, bgra, true)?;
    Ok(lines.into_iter().map(|line| line.words).collect())
}

struct Line {
    top: f64,
    text: String,
    words: Vec<Word>,
}

/// `VNRequestTextRecognitionLevel`.
#[derive(Clone, Copy)]
enum Level {
    Accurate = 0,
    Fast = 1,
}

static ACCURATE_READY: AtomicBool = AtomicBool::new(false);

fn level() -> Level {
    static WARM_UP: Once = Once::new();
    WARM_UP.call_once(|| {
        let _ = std::thread::Builder::new().name("ocr-precalentar".into()).spawn(|| {
            let mut img = vec![255u8; 96 * 32 * 4];
            for y in 10..22 {
                for x in 8..88 {
                    let i = (y * 96 + x) * 4;
                    img[i..i + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
            if let Ok(png) = encode_png(96, 32, &img) {
                let _ = run(&png, 96, 32, Level::Accurate, false);
            }
            ACCURATE_READY.store(true, Ordering::Release);
        });
    });
    if ACCURATE_READY.load(Ordering::Acquire) {
        Level::Accurate
    } else {
        Level::Fast
    }
}

fn read(width: u32, height: u32, bgra: &[u8], words: bool) -> Result<Vec<Line>, String> {
    let png = encode_png(width, height, bgra)?;
    run(&png, width, height, level(), words)
}

fn encode_png(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = bgra.to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(width, height, rgba).ok_or("la captura no tiene el tamaño esperado")?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn unavailable() -> String {
    "El reconocimiento de texto del sistema no está disponible.".into()
}

fn run(png: &[u8], width: u32, height: u32, level: Level, want_words: bool) -> Result<Vec<Line>, String> {
    autoreleasepool(|_| {
        let request_class = AnyClass::get(c"VNRecognizeTextRequest").ok_or_else(unavailable)?;
        let handler_class = AnyClass::get(c"VNImageRequestHandler").ok_or_else(unavailable)?;
        // SAFETY: mensajes a clases de Vision ya cargadas; lo que vuelve +0
        // vive en este autoreleasepool.
        unsafe {
            let request: Retained<AnyObject> = msg_send![request_class, new];
            let _: () = msg_send![&request, setRecognitionLevel: level as isize];
            let _: () = msg_send![&request, setUsesLanguageCorrection: Bool::YES];
            match languages(&request) {
                Some(list) => {
                    let refs: Vec<&NSString> = list.iter().map(|s| &**s).collect();
                    let array = NSArray::from_slice(&refs);
                    let _: () = msg_send![&request, setRecognitionLanguages: &*array];
                }
                None => {
                    let responds: Bool = msg_send![&request, respondsToSelector: sel!(setAutomaticallyDetectsLanguage:)];
                    if responds.as_bool() {
                        let _: () = msg_send![&request, setAutomaticallyDetectsLanguage: Bool::YES];
                    }
                }
            }

            let data = NSData::with_bytes(png);
            let options = NSDictionary::<NSString, AnyObject>::new();
            let handler: Allocated<AnyObject> = msg_send![handler_class, alloc];
            let handler: Option<Retained<AnyObject>> = msg_send![handler, initWithData: &*data, options: &*options];
            let handler = handler.ok_or_else(unavailable)?;
            let requests = NSArray::from_slice(&[&*request]);
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let ok: Bool = msg_send![&handler, performRequests: &*requests, error: &mut error];
            if !ok.as_bool() {
                return Err(error_message(error));
            }
            let results: Option<Retained<NSArray<AnyObject>>> = msg_send![&request, results];
            let Some(results) = results else {
                return Ok(Vec::new());
            };
            let (w, h) = (width as f64, height as f64);
            let mut lines = Vec::with_capacity(results.len());
            for observation in results.iter() {
                let candidates: Option<Retained<NSArray<AnyObject>>> = msg_send![&*observation, topCandidates: 1usize];
                let Some(candidate) = candidates.and_then(|c| c.firstObject()) else {
                    continue;
                };
                let string: Option<Retained<NSString>> = msg_send![&*candidate, string];
                let Some(string) = string else {
                    continue;
                };
                let text = string.to_string();
                if text.trim().is_empty() {
                    continue;
                }
                // Vision normaliza con el origen abajo a la izquierda.
                let bbox: NSRect = msg_send![&*observation, boundingBox];
                let top = 1.0 - (bbox.origin.y + bbox.size.height);
                let words = if want_words { words_of(&candidate, &text, w, h) } else { Vec::new() };
                lines.push(Line { top, text, words });
            }
            // De arriba abajo: el orden de lectura.
            lines.sort_by(|a, b| a.top.total_cmp(&b.top));
            Ok(lines)
        }
    })
}

/// Las palabras de una línea con su caja, en píxeles (origen arriba).
///
/// SAFETY: `candidate` es un `VNRecognizedText` vivo.
unsafe fn words_of(candidate: &AnyObject, text: &str, w: f64, h: f64) -> Vec<Word> {
    let mut out = Vec::new();
    // Los rangos de `NSString` van en unidades UTF-16.
    let mut offset = 0usize;
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end();
        let len16: usize = word.encode_utf16().count();
        let range = NSRange::new(offset, len16);
        offset += piece.encode_utf16().count();
        if word.is_empty() {
            continue;
        }
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let rect: Option<Retained<AnyObject>> = msg_send![candidate, boundingBoxForRange: range, error: &mut error];
        let Some(rect) = rect else {
            continue;
        };
        let bbox: NSRect = msg_send![&*rect, boundingBox];
        out.push(Word {
            text: word.to_string(),
            x: (bbox.origin.x * w) as f32,
            y: ((1.0 - bbox.origin.y - bbox.size.height) * h) as f32,
            w: (bbox.size.width * w) as f32,
            h: (bbox.size.height * h) as f32,
        });
    }
    out
}

/// Los idiomas preferidos del sistema que Vision sabe leer, en orden (hasta
/// cuatro). `None`: que Vision lo detecte.
///
/// SAFETY: `request` es un `VNRecognizeTextRequest` vivo.
unsafe fn languages(request: &AnyObject) -> Option<Vec<Retained<NSString>>> {
    let responds: Bool = msg_send![request, respondsToSelector: sel!(supportedRecognitionLanguagesAndReturnError:)];
    if !responds.as_bool() {
        return None;
    }
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let supported: Option<Retained<NSArray<NSString>>> =
        msg_send![request, supportedRecognitionLanguagesAndReturnError: &mut error];
    let supported: Vec<String> = supported?.iter().map(|s| s.to_string()).collect();
    let mut chosen: Vec<String> = Vec::new();
    for language in NSLocale::preferredLanguages().iter() {
        let tag = language.to_string();
        let base = tag.split('-').next().unwrap_or(&tag).to_string();
        // Exacto y, si no hay (es-CL), la misma lengua en otra región.
        let found = supported
            .iter()
            .find(|known| **known == tag)
            .or_else(|| supported.iter().find(|known| known.split('-').next() == Some(base.as_str())));
        if let Some(found) = found {
            if !chosen.contains(found) {
                chosen.push(found.clone());
            }
            if chosen.len() == 4 {
                break;
            }
        }
    }
    (!chosen.is_empty()).then(|| chosen.iter().map(|tag| NSString::from_str(tag)).collect())
}

fn error_message(error: *mut AnyObject) -> String {
    if error.is_null() {
        return "Vision no pudo reconocer texto en la captura.".into();
    }
    // SAFETY: el `NSError` vive en el autoreleasepool del llamador.
    let description: Option<Retained<NSString>> = unsafe { msg_send![error, localizedDescription] };
    description.map(|d| d.to_string()).unwrap_or_else(unavailable)
}
