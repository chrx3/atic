//! El texto de las imágenes del portapapeles, para encontrarlas buscando lo
//! que dicen.
//!
//! Cada imagen se lee una sola vez, en un hilo propio de baja prioridad (el
//! OCR del sistema: Vision en macOS, `Windows.Media.Ocr` en Windows), y el
//! resultado queda en `clipboard-ocr.json` de la carpeta de la pill, por la
//! huella de la imagen. Va aparte de `history.json` porque ese archivo lo
//! reescribe el watcher con cada copia (y en otros tiempos, la app de Tauri).
//!
//! Nada de esto corre en el hilo de la UI ni en el del watcher: `history.rs`
//! pide la lectura al cargar el historial (en segundo plano) y el panel
//! recoge lo nuevo cuando cambia `generation`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const FILE: &str = "clipboard-ocr.json";
/// Las imágenes más grandes que esto ni se abren.
const MAX_FILE_BYTES: u64 = 24 * 1024 * 1024;
const MAX_PIXELS: u64 = 60_000_000;
/// Lado máximo que se le pasa al OCR: una captura de un monitor 5K se achica
/// un poco; las de una pantalla Retina normal, no.
const MAX_SIDE: u32 = 4096;
/// Cuánto texto se guarda por imagen: alcanza para buscar.
const MAX_TEXT_CHARS: usize = 4000;
/// Al arrancar, esperar a que la pill termine de abrir antes de leer el
/// historial viejo.
const START_DELAY: Duration = Duration::from_secs(20);
/// Respiro entre una imagen y la siguiente.
const PAUSE: Duration = Duration::from_millis(300);
/// Cada cuántas lecturas se guarda aunque queden pendientes.
const SAVE_EVERY: usize = 8;

/// El archivo en disco.
#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    texts: HashMap<String, String>,
}

#[derive(Default)]
struct State {
    loaded: bool,
    /// Huella → texto leído («» si la imagen no tiene texto).
    texts: HashMap<String, String>,
    /// En la cola del hilo.
    queued: HashSet<String>,
    /// Falló en esta sesión: no se reintenta hasta el próximo arranque.
    failed: HashSet<String>,
}

struct Job {
    key: String,
    path: PathBuf,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static WORKER: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
/// Sube con cada texto nuevo: el panel lo mira para recoger lo leído.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn state() -> MutexGuard<'static, State> {
    let mut state = STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !state.loaded {
        state.loaded = true;
        state.texts = load();
    }
    state
}

fn file() -> Option<PathBuf> {
    crate::paths::file(FILE)
}

fn load() -> HashMap<String, String> {
    file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Stored>(&raw).ok())
        .map(|stored| stored.texts)
        .unwrap_or_default()
}

fn save(texts: HashMap<String, String>) {
    let Some(path) = file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string(&Stored { texts }) {
        if let Err(error) = atic_core::write_atomic_str(&path, &raw) {
            tracing::debug!(%error, "portapapeles: no se pudo guardar el texto de las imágenes");
        }
    }
}

/// La llave de una imagen: su huella de `history.json` (el contenido, o
/// `capture:<archivo>` para las capturas) o, en entradas viejas sin huella,
/// el nombre del archivo y su tamaño.
pub fn key(fingerprint: &str, file_name: &str, size: u64) -> String {
    if fingerprint.trim().is_empty() {
        format!("file:{file_name}:{size}")
    } else {
        fingerprint.to_string()
    }
}

/// El texto ya leído de una imagen. `Some("")`: se leyó y no tiene texto.
pub fn text(key: &str) -> Option<String> {
    state().texts.get(key).cloned()
}

/// Cuenta lo leído: cambia cada vez que llega un texto nuevo.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Pide leer una imagen si todavía no se leyó ni está en la cola.
pub fn request(key: &str, path: &Path) {
    {
        let mut state = state();
        if state.texts.contains_key(key) || state.queued.contains(key) || state.failed.contains(key) {
            return;
        }
        state.queued.insert(key.to_string());
    }
    let sender = WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("portapapeles-ocr".into())
            .spawn(move || work(receiver));
        if let Err(error) = spawned {
            tracing::warn!(%error, "portapapeles: no se pudo abrir el hilo del OCR");
        }
        Mutex::new(sender)
    });
    let job = Job { key: key.to_string(), path: path.to_path_buf() };
    let sent = sender.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).send(job);
    if sent.is_err() {
        let mut state = state();
        state.queued.remove(key);
        state.failed.insert(key.to_string());
    }
}

/// Guarda un texto leído a pedido (Ctrl+T sobre una imagen sin leer).
pub fn store(key: &str, text: &str) {
    let texts = {
        let mut state = state();
        state.texts.insert(key.to_string(), clean(text));
        state.texts.clone()
    };
    GENERATION.fetch_add(1, Ordering::AcqRel);
    save(texts);
}

/// Olvida lo leído de imágenes que ya no están en el historial.
pub fn retain(keys: &HashSet<String>) {
    let texts = {
        let mut state = state();
        let before = state.texts.len();
        state.texts.retain(|key, _| keys.contains(key));
        if state.texts.len() == before {
            return;
        }
        state.texts.clone()
    };
    save(texts);
}

/// Lo que se guarda: sin espacios sobrantes y con tope.
fn clean(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.char_indices().nth(MAX_TEXT_CHARS) {
        Some((cut, _)) => trimmed[..cut].to_string(),
        None => trimmed.to_string(),
    }
}

fn work(jobs: Receiver<Job>) {
    lower_priority();
    std::thread::sleep(START_DELAY);
    let mut unsaved = 0;
    while let Ok(job) = jobs.recv() {
        let read = load_bgra(&job.path).and_then(|(width, height, bgra)| {
            crate::ocr::recognize_in_background(width, height, &bgra)
        });
        let mut state = state();
        state.queued.remove(&job.key);
        match read {
            Ok(text) => {
                state.texts.insert(job.key, clean(&text));
                unsaved += 1;
                GENERATION.fetch_add(1, Ordering::AcqRel);
            }
            Err(error) => {
                tracing::debug!(%error, path = %job.path.display(), "portapapeles: OCR de la imagen");
                state.failed.insert(job.key);
            }
        }
        let flush = unsaved > 0 && (state.queued.is_empty() || unsaved >= SAVE_EVERY);
        let texts = flush.then(|| state.texts.clone());
        drop(state);
        if let Some(texts) = texts {
            unsaved = 0;
            save(texts);
        }
        std::thread::sleep(PAUSE);
    }
}

/// La imagen en BGRA, achicada si hace falta.
fn load_bgra(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let size = std::fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err(format!("la imagen pesa {size} bytes"));
    }
    let (width, height) = image::image_dimensions(path).map_err(|error| error.to_string())?;
    if width as u64 * height as u64 > MAX_PIXELS || width == 0 || height == 0 {
        return Err(format!("la imagen mide {width}×{height}"));
    }
    let mut picture = image::open(path).map_err(|error| error.to_string())?;
    let (width, height) = fit(width, height);
    if (width, height) != (picture.width(), picture.height()) {
        picture = picture.resize_exact(width, height, image::imageops::FilterType::Triangle);
    }
    let mut bgra = picture.to_rgba8().into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok((width, height, bgra))
}

/// El tamaño que se le pasa al OCR: el mismo o, si un lado pasa de
/// `MAX_SIDE`, achicado sin deformar.
fn fit(width: u32, height: u32) -> (u32, u32) {
    let side = width.max(height);
    if side <= MAX_SIDE {
        return (width, height);
    }
    let scale = MAX_SIDE as f64 / side as f64;
    let shrink = |n: u32| ((n as f64 * scale).round() as u32).max(1);
    (shrink(width), shrink(height))
}

/// Que el OCR no le quite tiempo a lo que el usuario está haciendo.
#[cfg(target_os = "macos")]
fn lower_priority() {
    extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    /// `QOS_CLASS_UTILITY` de `<sys/qos.h>`.
    const QOS_CLASS_UTILITY: u32 = 0x11;
    // SAFETY: solo cambia la prioridad del hilo actual.
    unsafe {
        pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(windows)]
fn lower_priority() {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    // SAFETY: solo cambia la prioridad del hilo actual.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_llave_es_la_huella_y_sin_ella_archivo_y_tamano() {
        assert_eq!(key("a1b2c3", "img-1.png", 10), "a1b2c3");
        assert_eq!(key("capture:Captura.png", "Captura.png", 10), "capture:Captura.png");
        assert_eq!(key("", "img-1.png", 10), "file:img-1.png:10");
        // Otro tamaño es otra imagen aunque se llame igual.
        assert_ne!(key("", "img-1.png", 10), key("", "img-1.png", 11));
    }

    #[test]
    fn se_achica_sin_deformar_solo_lo_grande() {
        assert_eq!(fit(2880, 1800), (2880, 1800));
        assert_eq!(fit(5120, 2880), (4096, 2304));
        assert_eq!(fit(100, 9000), (46, 4096));
    }

    #[test]
    fn el_texto_guardado_va_recortado_y_con_tope() {
        assert_eq!(clean("  hola\n"), "hola");
        let long = "ñ".repeat(MAX_TEXT_CHARS + 10);
        assert_eq!(clean(&long).chars().count(), MAX_TEXT_CHARS);
    }
}
