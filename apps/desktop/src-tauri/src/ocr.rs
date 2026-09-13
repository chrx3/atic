//! OCR de capturas: Windows.Media.Ocr (WinRT) en Windows y Vision
//! (`VNRecognizeTextRequest`) en macOS.

use std::path::{Path, PathBuf};

use tauri::State;

use crate::capture;
use crate::state::AppState;

fn ocr_sidecar_path(capture_path: &Path) -> PathBuf {
    // Sidecar junto al PNG; quitar \\?\ para rutas normales en disco.
    let base = strip_verbatim_prefix(capture_path);
    PathBuf::from(format!("{}.ocr.txt", base.to_string_lossy()))
}

/// `canonicalize` en Windows añade `\\?\`; WinRT y algunos I/O lo rechazan.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn ensure_capture_path(state: &AppState, path: &str) -> Result<PathBuf, String> {
    capture::ensure_app_image(state, Path::new(path))
}

/// Windows.Media.Ocr falla en recortes de una línea o de pocos píxeles de
/// alto: el motor espera texto ~12 px y margen alrededor. Agranda y rellena
/// con el color del borde; si el decode falla, se manda el PNG original.
fn prepare_ocr_png(bytes: &[u8]) -> Vec<u8> {
    use image::imageops::{self, FilterType};
    use image::{DynamicImage, GenericImageView, ImageFormat, RgbImage};
    use std::io::Cursor;

    let Ok(img) = image::load_from_memory(bytes) else {
        return bytes.to_vec();
    };
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return bytes.to_vec();
    }

    const TARGET_MIN: u32 = 160;
    const TARGET_H: u32 = 72;
    const MAX_DIM: u32 = 2400;
    const PAD: u32 = 24;

    let mut scale = 1.0_f32;
    if w.min(h) < TARGET_MIN {
        scale = scale.max(TARGET_MIN as f32 / w.min(h) as f32);
    }
    if h < TARGET_H {
        scale = scale.max(TARGET_H as f32 / h as f32);
    }
    scale = scale.clamp(1.0, 4.0);

    let mut nw = ((w as f32) * scale).round().max(1.0) as u32;
    let mut nh = ((h as f32) * scale).round().max(1.0) as u32;
    let longest = nw.max(nh);
    if longest > MAX_DIM {
        let cap = MAX_DIM as f32 / longest as f32;
        nw = ((nw as f32) * cap).round().max(1.0) as u32;
        nh = ((nh as f32) * cap).round().max(1.0) as u32;
    }

    let scaled = if nw != w || nh != h {
        img.resize_exact(nw, nh, FilterType::Lanczos3)
    } else {
        img
    };

    let rgb = scaled.to_rgb8();
    let fill = *rgb.get_pixel(0, 0);
    let mut canvas: RgbImage =
        image::ImageBuffer::from_pixel(rgb.width() + PAD * 2, rgb.height() + PAD * 2, fill);
    imageops::replace(&mut canvas, &rgb, i64::from(PAD), i64::from(PAD));

    let mut out = Cursor::new(Vec::new());
    if DynamicImage::ImageRgb8(canvas)
        .write_to(&mut out, ImageFormat::Png)
        .is_err()
    {
        return bytes.to_vec();
    }
    out.into_inner()
}

#[cfg(windows)]
fn ocr_image_at(path: &Path) -> Result<String, String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    // No usar StorageFile::GetFileFromPathAsync: falla con rutas `\\?\…`
    // (canonicalize) y produce UNABLE_TO_MASK_PATH / 0x800700A1.
    let bytes = std::fs::read(path).map_err(|e| {
        let shown = strip_verbatim_prefix(path);
        crate::ui_lang::msg(
            &format!("No se pudo leer la captura ({}): {e}", shown.display()),
            &format!("Could not read the capture ({}): {e}", shown.display()),
        )
    })?;
    if bytes.is_empty() {
        return Err(crate::ui_lang::msg(
            "La captura está vacía.",
            "The capture is empty.",
        ));
    }
    let bytes = prepare_ocr_png(&bytes);

    let stream = InMemoryRandomAccessStream::new().map_err(|e| e.to_string())?;
    {
        let writer = DataWriter::CreateDataWriter(&stream).map_err(|e| e.to_string())?;
        writer.WriteBytes(&bytes).map_err(|e| e.to_string())?;
        writer
            .StoreAsync()
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        writer
            .FlushAsync()
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        let _ = writer.DetachStream();
    }
    stream.Seek(0).map_err(|e| e.to_string())?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| e.to_string())?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let text = result.Text().map_err(|e| e.to_string())?.to_string();
    Ok(text.trim().to_string())
}

#[cfg(target_os = "macos")]
fn ocr_image_at(path: &Path) -> Result<String, String> {
    vision::recognize(path)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn ocr_image_at(_path: &Path) -> Result<String, String> {
    Err(crate::ui_lang::msg(
        "OCR no está disponible en esta plataforma.",
        "OCR is not available on this platform.",
    ))
}

fn write_sidecar(capture_path: &Path, text: &str) -> Result<(), String> {
    let sidecar = ocr_sidecar_path(capture_path);
    std::fs::write(&sidecar, text).map_err(|e| e.to_string())
}

/// Precalienta Vision para que la primera captura no pague la carga de assets.
///
/// La primera consulta al motor puede tardar decenas de segundos mientras el
/// sistema prepara los modelos; conviene pagarlo al arrancar, en segundo plano,
/// y no cuando el usuario pide el OCR de una captura.
#[cfg(target_os = "macos")]
pub fn warm_up() {
    use image::{ImageFormat, Rgb, RgbImage};

    // Imagen mínima con una mancha oscura: no importa el contenido, importa
    // que el pipeline completo (decode, request y modelo) quede cargado.
    let mut img = RgbImage::new(96, 32);
    for x in 8..88 {
        for y in 10..22 {
            img.put_pixel(x, y, Rgb([0, 0, 0]));
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    if image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, ImageFormat::Png)
        .is_err()
    {
        return;
    }
    match vision::recognize_bytes(out.get_ref()) {
        Ok(_) => tracing::info!("OCR: motor de Vision precalentado"),
        Err(error) => tracing::warn!(%error, "OCR: no se pudo precalentar Vision"),
    }
}

/// Corre el OCR fuera del hilo principal.
///
/// Vision tarda (y la primera vez descarga assets del sistema): bloquear el
/// event loop congelaba la app entera, incluida la ventana que abrió la tool.
async fn run_ocr(resolved: PathBuf) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || ocr_image_at(&resolved))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ocr_capture_text(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let resolved = ensure_capture_path(&state, &path)?;
    let text = run_ocr(resolved.clone()).await?;
    if !text.is_empty() {
        let _ = write_sidecar(&resolved, &text);
    }
    Ok(text)
}

#[tauri::command]
pub async fn ocr_capture_and_copy(
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let resolved = ensure_capture_path(&state, &path)?;
    let text = run_ocr(resolved.clone()).await?;
    if text.is_empty() {
        return Err(crate::ui_lang::msg(
            "No se detectó texto en la captura.",
            "No text was found in the capture.",
        ));
    }
    write_sidecar(&resolved, &text)?;
    crate::clipboard_history::set_system_text(text.clone())?;
    Ok(text)
}

#[tauri::command]
pub fn read_capture_ocr_cache(state: State<AppState>, path: String) -> Option<String> {
    let resolved = ensure_capture_path(&state, &path).ok()?;
    let sidecar = ocr_sidecar_path(&resolved);
    let raw = std::fs::read_to_string(&sidecar).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// OCR on-device con Vision (`VNRecognizeTextRequest`).
///
/// El framework no está envuelto por objc2, así que los mensajes van crudos.
/// El request es seguro fuera del hilo principal, pero el hilo de Tauri no
/// siempre tiene pool de autorelease: se abre uno por llamada para que los
/// objetos temporales de Foundation no se acumulen.
#[cfg(target_os = "macos")]
mod vision {
    use std::path::Path;

    use objc2::msg_send;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSLocale, NSRect, NSString};

    // El framework tiene que estar cargado para que `AnyClass::get` encuentre
    // `VNRecognizeTextRequest` y `VNImageRequestHandler`.
    #[link(name = "Vision", kind = "framework")]
    extern "C" {}

    /// `VNRequestTextRecognitionLevel.accurate`.
    const RECOGNITION_ACCURATE: isize = 0;

    pub(super) fn recognize(path: &Path) -> Result<String, String> {
        let bytes = std::fs::read(path).map_err(|e| {
            crate::ui_lang::msg(
                &format!("No se pudo leer la captura ({}): {e}", path.display()),
                &format!("Could not read the capture ({}): {e}", path.display()),
            )
        })?;
        if bytes.is_empty() {
            return Err(crate::ui_lang::msg(
                "La captura está vacía.",
                "The capture is empty.",
            ));
        }
        recognize_bytes(&bytes)
    }

    /// Igual que [`recognize`], con los bytes ya en memoria (el precalentado
    /// usa una imagen mínima generada acá).
    pub(super) fn recognize_bytes(bytes: &[u8]) -> Result<String, String> {
        let bytes = super::prepare_ocr_png(bytes);

        autoreleasepool(|_| {
            let Some(request_class) = AnyClass::get(c"VNRecognizeTextRequest") else {
                return Err(vision_unavailable());
            };
            // SAFETY: mensajes a clases cargadas del framework; los objetos
            // que devuelven +0 viven en este autoreleasepool.
            unsafe {
                let request: *mut AnyObject = msg_send![request_class, alloc];
                let request: *mut AnyObject = msg_send![request, init];
                if request.is_null() {
                    return Err(vision_unavailable());
                }
                let result = perform(request_class, request, &bytes);
                // SAFETY: `request` es +1 de `alloc`/`init`.
                drop(Retained::from_raw(request));
                result
            }
        })
    }

    /// Corre el request ya creado (prestado, +1 lo mantiene `recognize`).
    fn perform(
        request_class: &AnyClass,
        request: *mut AnyObject,
        bytes: &[u8],
    ) -> Result<String, String> {
        // SAFETY: los mensajes usan objetos vivos durante la llamada.
        unsafe {
            let _: () = msg_send![request, setRecognitionLevel: RECOGNITION_ACCURATE];
            let _: () = msg_send![request, setUsesLanguageCorrection: Bool::YES];
            if let Some(languages) = recognition_languages(request_class) {
                let refs: Vec<&NSString> = languages.iter().map(|s| &**s).collect();
                let array = NSArray::from_slice(&refs);
                let _: () = msg_send![request, setRecognitionLanguages: &*array];
            } else {
                // Sin lista, que Vision detecte el idioma solo (macOS 13+).
                let selector = objc2::sel!(setAutomaticallyDetectsLanguage:);
                let responds: Bool = msg_send![request, respondsToSelector: selector];
                if responds.as_bool() {
                    let _: () = msg_send![request, setAutomaticallyDetectsLanguage: Bool::YES];
                }
            }

            let handler_class =
                AnyClass::get(c"VNImageRequestHandler").ok_or_else(vision_unavailable)?;
            let data = NSData::with_bytes(bytes);
            let options = NSDictionary::<NSString, AnyObject>::new();
            let handler: *mut AnyObject = msg_send![handler_class, alloc];
            let handler: *mut AnyObject =
                msg_send![handler, initWithData: &*data, options: &*options];
            if handler.is_null() {
                return Err(vision_unavailable());
            }
            let result = (|| -> Result<String, String> {
                let requests = NSArray::from_slice(&[&*request]);
                let mut error: *mut AnyObject = std::ptr::null_mut();
                let ok: Bool = msg_send![handler, performRequests: &*requests, error: &mut error];
                if !ok.as_bool() {
                    return Err(error_message(error));
                }

                let results: *mut AnyObject = msg_send![request, results];
                if results.is_null() {
                    return Ok(String::new());
                }
                // SAFETY: `results` es un NSArray<VNRecognizedTextObservation*>
                // vivo en el autoreleasepool del llamador.
                let results: &NSArray<AnyObject> = &*results.cast();
                let mut lines: Vec<(f64, String)> = Vec::with_capacity(results.len());
                for observation in results.iter() {
                    let candidates: *mut AnyObject =
                        msg_send![&*observation, topCandidates: 1usize];
                    if candidates.is_null() {
                        continue;
                    }
                    // SAFETY: `candidates` es un NSArray<VNRecognizedText*> +0.
                    let candidates: &NSArray<AnyObject> = &*candidates.cast();
                    let Some(candidate) = candidates.firstObject() else {
                        continue;
                    };
                    let string: *mut AnyObject = msg_send![&*candidate, string];
                    if string.is_null() {
                        continue;
                    }
                    // SAFETY: `string` es un NSString +0.
                    let string: &NSString = &*string.cast();
                    let text = string.to_string();
                    if text.trim().is_empty() {
                        continue;
                    }
                    // Vision normaliza con origen abajo-izquierda: arriba =
                    // 1 - y - alto. Ordenar de arriba abajo mantiene el orden
                    // de lectura aunque la revisión no lo garantice.
                    let bbox: NSRect = msg_send![&*observation, boundingBox];
                    let top = 1.0 - (bbox.origin.y + bbox.size.height);
                    lines.push((top, text));
                }
                lines.sort_by(|a, b| a.0.total_cmp(&b.0));
                Ok(lines
                    .into_iter()
                    .map(|(_, text)| text)
                    .collect::<Vec<_>>()
                    .join("\n"))
            })();
            // SAFETY: `handler` es +1 de `alloc`/`init`.
            drop(Retained::from_raw(handler));
            result
        }
    }

    /// Idiomas preferidos del sistema que Vision soporta, en orden.
    ///
    /// El selector de la lista cambió con las versiones: primero el clásico
    /// `supportedRecognitionLanguagesAndReturnError:` y, si no está, el
    /// moderno `supportedRecognitionLanguagesForTextRecognitionLevel:revision:error:`.
    fn recognition_languages(request_class: &AnyClass) -> Option<Vec<Retained<NSString>>> {
        // SAFETY: los selectores devuelven un array de NSString +0; si fallan,
        // nil (el NSError opcional se descarta acá).
        let supported: *mut AnyObject = unsafe {
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let legacy = objc2::sel!(supportedRecognitionLanguagesAndReturnError:);
            let responds: Bool = msg_send![request_class, respondsToSelector: legacy];
            if responds.as_bool() {
                msg_send![request_class, supportedRecognitionLanguagesAndReturnError: &mut error]
            } else {
                let modern = objc2::sel!(
                    supportedRecognitionLanguagesForTextRecognitionLevel:revision:error:
                );
                let responds: Bool = msg_send![request_class, respondsToSelector: modern];
                if !responds.as_bool() {
                    return None;
                }
                msg_send![
                    request_class,
                    supportedRecognitionLanguagesForTextRecognitionLevel: RECOGNITION_ACCURATE,
                    revision: 0usize,
                    error: &mut error
                ]
            }
        };
        if supported.is_null() {
            return None;
        }
        // SAFETY: `supported` es un NSArray<NSString> vivo en el
        // autoreleasepool del llamador.
        let supported: &NSArray<NSString> = unsafe { &*supported.cast() };
        let supported: Vec<String> = supported.iter().map(|s| s.to_string()).collect();
        let preferred = NSLocale::preferredLanguages();
        let mut chosen: Vec<String> = Vec::new();
        for language in preferred.iter() {
            let tag = language.to_string();
            let base = tag.split('-').next().unwrap_or(&tag);
            // Coincidencia exacta y, si no hay (p. ej. es-CL), la misma
            // lengua en cualquier región soportada.
            let found = supported.iter().find(|known| **known == tag).or_else(|| {
                supported
                    .iter()
                    .find(|known| known.split('-').next() == Some(base))
            });
            if let Some(found) = found {
                if !chosen.iter().any(|chosen| chosen == found) {
                    chosen.push(found.clone());
                }
                if chosen.len() == 4 {
                    break;
                }
            }
        }
        if chosen.is_empty() {
            // Sin coincidencias, que Vision use su idioma o lo detecte.
            None
        } else {
            Some(chosen.iter().map(|tag| NSString::from_str(tag)).collect())
        }
    }

    fn error_message(error: *mut AnyObject) -> String {
        if error.is_null() {
            return crate::ui_lang::msg(
                "Vision no pudo reconocer texto en la captura.",
                "Vision could not recognize text in the capture.",
            );
        }
        // SAFETY: el NSError sigue vivo durante la llamada (lo posee el
        // autoreleasepool del llamador).
        let description: *mut AnyObject = unsafe { msg_send![error, localizedDescription] };
        if description.is_null() {
            return vision_unavailable();
        }
        // SAFETY: `description` es un NSString +0.
        let description: &NSString = unsafe { &*description.cast() };
        description.to_string()
    }

    fn vision_unavailable() -> String {
        crate::ui_lang::msg(
            "El reconocimiento de texto del sistema no está disponible.",
            "System text recognition is not available.",
        )
    }
}
