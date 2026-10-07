use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use arboard::{Clipboard, ImageData};
use atic_core::MutexExt;

use crate::{
    clipboard_is_sensitive, clipboard_source_app, encode_png_rgba, fingerprint_image,
    fingerprint_text, now_ms, try_clipboard_read, ClipboardItem, ClipboardKind, History,
    MAX_IMAGE_BYTES,
};

const POLL_MS: u64 = 450;

/// Qué entró al historial.
pub enum Change<'a> {
    Text(&'a str),
    Image,
}

/// Lo que el watcher le pregunta y le avisa a la app que lo corre.
pub struct Hooks {
    /// Se consulta en cada vuelta: apagar el historial en Ajustes tiene que
    /// dejar de guardar en el acto, no en el próximo arranque.
    pub enabled: Box<dyn Fn() -> bool + Send>,
    /// La etiqueta de una imagen («Imagen 640×480»), en el idioma de la UI.
    pub image_label: fn(usize, usize) -> String,
    /// Algo nuevo quedó arriba del historial (y ya está en `history.json`).
    pub on_change: Box<dyn Fn(Change<'_>) + Send>,
}

enum ClipPoll {
    Sensitive,
    Image {
        width: usize,
        height: usize,
        bytes: Vec<u8>,
    },
    Text(String),
    Empty,
}

/// Arranca el hilo que mira el portapapeles y llena `shared`, guardando en
/// `dir`. El hilo vive lo que el proceso.
pub fn spawn_watcher(shared: Arc<Mutex<History>>, dir: PathBuf, hooks: Hooks) {
    thread::spawn(move || {
        let Ok(mut clipboard) = Clipboard::new() else {
            tracing::warn!("clipboard watcher: no se pudo abrir el portapapeles");
            return;
        };
        loop {
            thread::sleep(Duration::from_millis(POLL_MS));
            if !(hooks.enabled)() {
                continue;
            }

            // suppress_until no necesita el clipboard: se mira primero, para
            // no abrir el portapapeles justo cuando nosotros mismos estamos
            // escribiendo un dibujo o una captura.
            {
                let mut hist = shared.lock_or_recover();
                if let Some(until) = hist.suppress_until {
                    if SystemTime::now() < until {
                        continue;
                    }
                    hist.suppress_until = None;
                }
            }

            // Antes de leer: mientras leemos, el portapapeles es nuestro.
            let source_app = clipboard_source_app();
            let Some(poll) = try_clipboard_read(|| {
                // Lo que el dueño del contenido pidió no archivar no se
                // archiva. Es la única señal que existe: los gestores de
                // contraseñas la ponen justamente para que su copia no
                // sobreviva al pegado.
                if clipboard_is_sensitive() {
                    return ClipPoll::Sensitive;
                }
                if let Ok(img) = clipboard.get_image() {
                    return ClipPoll::Image {
                        width: img.width,
                        height: img.height,
                        bytes: img.bytes.into_owned(),
                    };
                }
                if let Ok(text) = clipboard.get_text() {
                    return ClipPoll::Text(text);
                }
                ClipPoll::Empty
            }) else {
                continue;
            };

            match poll {
                ClipPoll::Sensitive | ClipPoll::Empty => continue,
                ClipPoll::Image {
                    width,
                    height,
                    bytes,
                } => {
                    let img = ImageData {
                        width,
                        height,
                        bytes: bytes.into(),
                    };
                    match ingest_image(&shared, &dir, &img, source_app, hooks.image_label) {
                        Ok(true) => (hooks.on_change)(Change::Image),
                        Ok(false) => {}
                        Err(err) => tracing::debug!(%err, "clipboard image ingest"),
                    }
                }
                ClipPoll::Text(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let item = ClipboardItem {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: ClipboardKind::Text,
                        preview: trimmed.chars().take(120).collect(),
                        text: Some(trimmed.to_string()),
                        image_path: None,
                        created_at_ms: now_ms(),
                        pinned: false,
                        fingerprint: fingerprint_text(trimmed),
                        source: "watcher".into(),
                        source_app,
                    };
                    let mut hist = shared.lock_or_recover();
                    let before_fp = hist.items.first().map(|i| i.fingerprint.clone());
                    hist.push_item(&dir, item);
                    let after_fp = hist.items.first().map(|i| i.fingerprint.clone());
                    if before_fp != after_fp {
                        drop(hist);
                        // Lo sensible ya quedó afuera arriba: solo avisa lo que
                        // entra al historial.
                        (hooks.on_change)(Change::Text(trimmed));
                    }
                }
            }
        }
    });
}

/// Guarda una imagen del portapapeles como PNG en `dir` y la suma al
/// historial. `Ok(true)` si quedó arriba como algo nuevo.
pub fn ingest_image(
    shared: &Arc<Mutex<History>>,
    dir: &Path,
    img: &ImageData<'_>,
    source_app: Option<String>,
    image_label: fn(usize, usize) -> String,
) -> Result<bool, String> {
    let w = img.width;
    let h = img.height;
    let bytes = img.bytes.as_ref();
    if bytes.len() > MAX_IMAGE_BYTES * 4 {
        return Err("imagen demasiado grande".into());
    }
    let fp = fingerprint_image(bytes, w, h);
    {
        let mut hist = shared.lock_or_recover();
        if hist.deleted_fingerprints.contains(&fp) {
            return Ok(false);
        }
        if hist.last_fingerprint.as_ref() == Some(&fp) {
            return Ok(false);
        }
        // ¿Es la captura/dibujo que acabamos de copiar nosotros? Ya está en
        // el historial con su identidad; grabarla acá otra vez es el
        // duplicado «Imagen 631×638» + «Captura 15:36». Se sella su
        // fingerprint de contenido en `last_fingerprint` para que no vuelva
        // a entrar mientras siga en el portapapeles.
        //
        // Solo dimensiones: el round-trip por el DIB no conserva los bytes.
        // El riesgo es tragar un dibujo del mismo tamaño copiado enseguida;
        // quien copia un dibujo graba el ítem ANTES de escribir el
        // portapapeles, así que si esto suprime el ingest el dibujo ya está en
        // la lista.
        if let Some(pending) = hist.pending_capture.take() {
            if pending.matches(w, h) {
                tracing::debug!(
                    target: "clipboard",
                    fingerprint = %pending.fingerprint,
                    "imagen del portapapeles reconocida como captura propia"
                );
                hist.last_fingerprint = Some(fp);
                return Ok(false);
            }
            // Sigue vigente pero todavía no es esta imagen: devolverla.
            if pending.is_fresh() {
                hist.pending_capture = Some(pending);
            }
        }
    }
    let png = encode_png_rgba(bytes, w, h)?;
    if png.len() > MAX_IMAGE_BYTES {
        return Err("PNG demasiado grande".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("img-{id}.png"));
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    let item = ClipboardItem {
        id,
        kind: ClipboardKind::Image,
        preview: image_label(w, h),
        text: None,
        image_path: Some(path.to_string_lossy().into_owned()),
        created_at_ms: now_ms(),
        pinned: false,
        fingerprint: fp,
        source: "watcher".into(),
        source_app,
    };
    let mut hist = shared.lock_or_recover();
    let before = hist.items.first().map(|i| i.fingerprint.clone());
    hist.push_item(dir, item);
    let after = hist.items.first().map(|i| i.fingerprint.clone());
    Ok(before != after)
}
