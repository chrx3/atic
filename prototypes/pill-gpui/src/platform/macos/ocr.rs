//! «Texto» del estante, tapar datos y buscar imágenes del portapapeles en
//! macOS: Vision (`VNRecognizeTextRequest`), en los idiomas preferidos del
//! sistema más español e inglés. Sin modelos aparte ni permisos.
//!
//! Las líneas que entrega Vision se ordenan y se unen en párrafos en
//! `layout.rs`.
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

mod layout;

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

/// Lee el texto de una imagen BGRA en orden de lectura, con los párrafos
/// unidos. Corre en un hilo de fondo.
pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, String> {
    let lines = read(width, height, bgra, false)?;
    Ok(layout_text(lines))
}

/// Como `recognize`, pero para lo que se guarda (el texto de las imágenes del
/// portapapeles): espera al modelo preciso en vez de usar el rápido mientras
/// se precalienta. Puede tardar; solo en un hilo de fondo de baja prioridad.
pub fn recognize_in_background(width: u32, height: u32, bgra: &[u8]) -> Result<String, String> {
    let _ = level();
    // El precalentamiento compila los modelos una vez; si tarda demasiado,
    // el modo preciso los compila igual al usarlo.
    for _ in 0..120 {
        if ACCURATE_READY.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let png = encode_png(width, height, bgra)?;
    let lines = run(&png, width, height, Level::Accurate, false)?;
    Ok(layout_text(lines))
}

fn layout_text(lines: Vec<Line>) -> String {
    layout::text(lines.into_iter().map(|line| line.piece).collect())
}

/// Las líneas leídas, cada una con sus palabras y dónde están (para tapar
/// justo lo que hay que tapar). Corre en un hilo de fondo.
pub fn recognize_lines(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<Vec<Word>>, String> {
    let lines = read(width, height, bgra, true)?;
    Ok(lines.into_iter().map(|line| line.words).collect())
}

struct Line {
    /// El texto y su caja, en píxeles con el origen arriba.
    piece: layout::Piece,
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
            // Con 0 (lo de fábrica) Vision procesa a resolución completa;
            // letra de menos de 6 px no se lee igual, y así puede achicar
            // las capturas grandes.
            let min_height = (6.0 / height.max(1) as f32).min(0.05);
            let _: () = msg_send![&request, setMinimumTextHeight: min_height];
            // Los idiomas dependen del nivel: se piden después de fijarlo.
            let chosen = languages(&request);
            if !chosen.list.is_empty() {
                let list: Vec<Retained<NSString>> = chosen.list.iter().map(|tag| NSString::from_str(tag)).collect();
                let refs: Vec<&NSString> = list.iter().map(|s| &**s).collect();
                let array = NSArray::from_slice(&refs);
                let _: () = msg_send![&request, setRecognitionLanguages: &*array];
            }
            // Apple aconseja fijar los idiomas cuando se saben: la detección
            // automática solo si falta alguno de los preferidos (otra
            // escritura, por ejemplo) o no hay lista.
            if chosen.list.is_empty() || chosen.missing {
                let responds: Bool = msg_send![&request, respondsToSelector: sel!(setAutomaticallyDetectsLanguage:)];
                if responds.as_bool() {
                    let _: () = msg_send![&request, setAutomaticallyDetectsLanguage: Bool::YES];
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
                let words = if want_words { words_of(&candidate, &text, w, h) } else { Vec::new() };
                let piece = layout::Piece {
                    text,
                    x: bbox.origin.x * w,
                    y: (1.0 - bbox.origin.y - bbox.size.height) * h,
                    w: bbox.size.width * w,
                    h: bbox.size.height * h,
                };
                lines.push(Line { piece, words });
            }
            // De arriba abajo (para tapar datos); `layout` arma el orden de
            // lectura fino del texto.
            lines.sort_by(|a, b| a.piece.y.total_cmp(&b.piece.y));
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

/// Los idiomas para Vision y si quedó fuera alguno de los preferidos.
struct Languages {
    list: Vec<String>,
    missing: bool,
}

/// Los idiomas preferidos del sistema que Vision sabe leer en el nivel ya
/// fijado, más español e inglés.
///
/// SAFETY: `request` es un `VNRecognizeTextRequest` vivo.
unsafe fn languages(request: &AnyObject) -> Languages {
    let none = Languages { list: Vec::new(), missing: true };
    let responds: Bool = msg_send![request, respondsToSelector: sel!(supportedRecognitionLanguagesAndReturnError:)];
    if !responds.as_bool() {
        return none;
    }
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let supported: Option<Retained<NSArray<NSString>>> =
        msg_send![request, supportedRecognitionLanguagesAndReturnError: &mut error];
    let Some(supported) = supported else {
        return none;
    };
    let supported: Vec<String> = supported.iter().map(|s| s.to_string()).collect();
    let preferred: Vec<String> = NSLocale::preferredLanguages().iter().map(|s| s.to_string()).collect();
    choose_languages(&preferred, &supported)
}

/// Cuántos idiomas se le pasan a Vision: con más, corrige peor.
const MAX_LANGUAGES: usize = 4;
/// Siempre van: el usuario escribe en español y casi toda la interfaz que
/// captura tiene inglés.
const ALWAYS: [&str; 2] = ["es", "en"];

/// Lo que Vision sabe leer de `preferred` (etiquetas BCP 47 como `es-CL` o
/// `zh-Hans-CL`), en orden y sin repetir, más español e inglés. Una etiqueta
/// sin igual exacto toma la misma lengua (y escritura, si la trae) en otra
/// región: `es-CL` → `es-ES`.
fn choose_languages(preferred: &[String], supported: &[String]) -> Languages {
    let lookup = |tag: &str| -> Option<String> {
        let tag = tag.replace('_', "-");
        if let Some(exact) = supported.iter().find(|known| known.eq_ignore_ascii_case(&tag)) {
            return Some(exact.clone());
        }
        let mut parts = tag.split('-');
        let base = parts.next()?.to_ascii_lowercase();
        let script = parts.next().filter(|part| part.len() == 4).map(str::to_ascii_lowercase);
        let same_base = |known: &&String| known.split('-').next().is_some_and(|b| b.eq_ignore_ascii_case(&base));
        let with_script = script.as_ref().and_then(|script| {
            supported
                .iter()
                .filter(same_base)
                .find(|known| known.split('-').nth(1).is_some_and(|s| s.eq_ignore_ascii_case(script)))
        });
        with_script.or_else(|| supported.iter().find(same_base)).cloned()
    };
    let mut list: Vec<String> = Vec::new();
    let mut missing = false;
    for tag in preferred {
        match lookup(tag.as_str()) {
            Some(found) if !list.contains(&found) => list.push(found),
            Some(_) => {}
            None => missing = true,
        }
    }
    // Español e inglés caben siempre: si hace falta, a costa de los últimos
    // preferidos.
    let always: Vec<String> = ALWAYS.iter().filter_map(|tag| lookup(*tag)).filter(|tag| !list.contains(tag)).collect();
    let room = MAX_LANGUAGES.saturating_sub(always.len());
    if list.len() > room {
        list.truncate(room);
        missing = true;
    }
    list.extend(always);
    Languages { list, missing }
}

fn error_message(error: *mut AnyObject) -> String {
    if error.is_null() {
        return "Vision no pudo reconocer texto en la captura.".into();
    }
    // SAFETY: el `NSError` vive en el autoreleasepool del llamador.
    let description: Option<Retained<NSString>> = unsafe { msg_send![error, localizedDescription] };
    description.map(|d| d.to_string()).unwrap_or_else(unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|tag| tag.to_string()).collect()
    }

    const SUPPORTED: &[&str] = &["en-US", "fr-FR", "it-IT", "de-DE", "es-ES", "pt-BR", "zh-Hans", "zh-Hant", "ja-JP"];

    #[test]
    fn espanol_de_chile_toma_el_de_espana_y_suma_ingles() {
        let chosen = choose_languages(&tags(&["es-CL", "es-419"]), &tags(SUPPORTED));
        assert_eq!(chosen.list, ["es-ES", "en-US"]);
        assert!(!chosen.missing);
    }

    #[test]
    fn el_orden_de_preferencia_se_respeta() {
        let chosen = choose_languages(&tags(&["en-CL", "es-CL"]), &tags(SUPPORTED));
        assert_eq!(chosen.list, ["en-US", "es-ES"]);
    }

    #[test]
    fn la_escritura_cuenta_y_lo_desconocido_avisa() {
        let chosen = choose_languages(&tags(&["zh-Hant-TW", "eo"]), &tags(SUPPORTED));
        assert_eq!(chosen.list, ["zh-Hant", "es-ES", "en-US"]);
        assert!(chosen.missing);
    }

    #[test]
    fn espanol_e_ingles_caben_aunque_haya_muchos_preferidos() {
        let chosen = choose_languages(&tags(&["fr-FR", "de-DE", "it-IT", "ja-JP", "pt-BR"]), &tags(SUPPORTED));
        assert_eq!(chosen.list, ["fr-FR", "de-DE", "es-ES", "en-US"]);
        assert!(chosen.missing);
    }

    #[test]
    fn sin_espanol_en_vision_igual_hay_lista() {
        let chosen = choose_languages(&tags(&["es-CL"]), &tags(&["en-US"]));
        assert_eq!(chosen.list, ["en-US"]);
        assert!(chosen.missing);
    }
}
