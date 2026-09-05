//! Historial local del portapapeles (texto + imágenes).
//!
//! Un hilo hace polling de `arboard`, persiste en `data/clipboard/` y expone
//! comandos para listar, pegar, fijar y borrar.

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arboard::{Clipboard, ImageData};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::AppState;
use atic_core::MutexExt;

fn capture_preview_label(label: &str) -> String {
    let word = crate::ui_lang::pick(crate::ui_lang::english(), "Captura", "Capture");
    if label.is_empty() {
        word.to_string()
    } else {
        format!("{word} {label}")
    }
}

fn image_preview_label(w: usize, h: usize) -> String {
    crate::ui_lang::msg(&format!("Imagen {w}×{h}"), &format!("Image {w}×{h}"))
}

fn item_missing() -> String {
    crate::ui_lang::msg("Ítem no encontrado", "Item not found")
}

fn item_empty_text() -> String {
    crate::ui_lang::msg("Ítem de texto vacío", "Text item is empty")
}

/// Ventana en primer plano antes de abrir el panel (para pegar ahí).
static PREV_FOREGROUND: AtomicIsize = AtomicIsize::new(0);

/// Cuándo se vio `PREV_FOREGROUND` por última vez. Solo para diagnóstico: un
/// destino de hace media hora y uno de hace 200 ms se ven igual sin este dato,
/// y esa diferencia es justo la que separa un objetivo legítimo de uno heredado
/// que manda el texto a la app equivocada.
static SAVED_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Hay un seguimiento de foco corriendo.
static TRACKING: AtomicBool = AtomicBool::new(false);

/// Cada cuánto se relee la ventana en foco durante el dictado.
const TRACK_MS: u64 = 200;

const MAX_ITEMS: usize = 100;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const POLL_MS: u64 = 450;
const HISTORY_FILE: &str = "history.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: String,
    pub kind: ClipboardKind,
    /// Preview de texto o etiqueta corta para imágenes.
    pub preview: String,
    /// Texto completo (solo text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Ruta absoluta al PNG (solo image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    pub created_at_ms: u64,
    #[serde(default)]
    pub pinned: bool,
    /// Huella para deduplicar.
    pub fingerprint: String,
    /// Origen: watcher | capture
    #[serde(default)]
    pub source: String,
}

/// Una captura recién copiada al portapapeles, esperando que el watcher la vea.
///
/// No se puede resolver esto con `suppress_until`: la imagen se queda en el
/// portapapeles indefinidamente, así que una ventana de tiempo solo retrasa el
/// duplicado hasta que expira. Y tampoco sirve precalcular el fingerprint de
/// contenido: el round-trip por el DIB de Windows no garantiza los mismos
/// bytes. Lo que sí sabemos con certeza son las dimensiones.
struct PendingCapture {
    /// `capture:<id>` — la identidad que ya quedó en el historial.
    fingerprint: String,
    width: usize,
    height: usize,
    at: SystemTime,
}

/// Cuánto vale la pena esperar a que el watcher vea nuestra propia captura.
const PENDING_CAPTURE_TTL: Duration = Duration::from_secs(10);

fn pending_is_fresh(pending: &PendingCapture) -> bool {
    pending
        .at
        .elapsed()
        .is_ok_and(|age| age < PENDING_CAPTURE_TTL)
}

/// ¿El watcher acaba de ver la imagen que nosotros mismos pusimos?
fn pending_capture_matches(pending: &PendingCapture, width: usize, height: usize) -> bool {
    pending_is_fresh(pending) && pending.width == width && pending.height == height
}

#[derive(Default)]
struct HistoryState {
    items: Vec<ClipboardItem>,
    last_fingerprint: Option<String>,
    /// Evita re-capturar lo que nosotros mismos pegamos.
    suppress_until: Option<SystemTime>,
    /// Ítems borrados a mano: el SO puede seguir teniendo el mismo contenido
    /// en el portapapeles; sin esto el watcher los re-ingiere al instante.
    deleted_fingerprints: HashSet<String>,
    /// La captura que acabamos de poner en el portapapeles, para que el watcher
    /// la reconozca como tal en vez de grabar una copia paralela.
    pending_capture: Option<PendingCapture>,
}

static HISTORY: Mutex<Option<Arc<Mutex<HistoryState>>>> = Mutex::new(None);

/// Un solo hilo a la vez puede tener el portapapeles de Windows abierto.
///
/// El watcher vive en un `thread::spawn` y las escrituras (copiar un dibujo,
/// una captura, pegar) corren en workers de Tauri. Sin este candado los dos
/// llaman `OpenClipboard` a la vez; arboard encodea el PNG *con el clipboard
/// ya abierto*, y `SetClipboardData` termina en `ERROR_CLIPBOARD_NOT_OPEN`
/// (1418, «El subproceso no tiene abierto un Portapapeles»).
static CLIPBOARD_GATE: Mutex<()> = Mutex::new(());

/// Toma el candado para una escritura. El watcher, si está leyendo, termina
/// esa vuelta y después se aparta: no se le deja `OpenClipboard` a mitad de
/// un `set_image`.
pub(crate) fn with_clipboard_write<R>(f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let _guard = CLIPBOARD_GATE.lock_or_recover();
    f()
}

/// Texto al portapapeles del sistema, por el mismo candado que las imágenes.
pub(crate) fn set_system_text(text: impl Into<String>) -> Result<(), String> {
    let text = text.into();
    with_clipboard_write(|| {
        let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text).map_err(|e| e.to_string())
    })
}

/// Intenta leer sin bloquear a quien está escribiendo. `None` = hay una
/// escritura en curso; esta vuelta del watcher se salta.
fn try_clipboard_read<R>(f: impl FnOnce() -> R) -> Option<R> {
    let _guard = match CLIPBOARD_GATE.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return None,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };
    Some(f())
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn fingerprint_text(text: &str) -> String {
    let mut h = DefaultHasher::new();
    "text".hash(&mut h);
    text.hash(&mut h);
    format!("{:x}", h.finish())
}

fn fingerprint_image(bytes: &[u8], w: usize, h: usize) -> String {
    let mut hasher = DefaultHasher::new();
    "image".hash(&mut hasher);
    w.hash(&mut hasher);
    h.hash(&mut hasher);
    bytes.len().hash(&mut hasher);
    let step = (bytes.len() / 64).max(1);
    for chunk in bytes.iter().step_by(step).take(64) {
        chunk.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

fn history_path(dir: &Path) -> PathBuf {
    dir.join(HISTORY_FILE)
}

fn load_history(dir: &Path) -> Vec<ClipboardItem> {
    let path = history_path(dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Atómica: el historial se reescribe entero con cada copia —cada 450 ms hay
/// una oportunidad de morir a mitad—, y `load_history` ante un JSON roto
/// devuelve una lista vacía, o sea que un truncado se vería como «se borró
/// todo» y no como un error.
fn save_history(dir: &Path, items: &[ClipboardItem]) {
    if let Ok(raw) = serde_json::to_string_pretty(items) {
        let _ = atic_core::write_atomic_str(&history_path(dir), &raw);
    }
}

fn dismissed_path(dir: &Path) -> PathBuf {
    dir.join("dismissed-captures.json")
}

/// Capturas que el usuario sacó del historial, entre arranques.
///
/// Va aparte de `history.json` porque no es una lista de ítems sino de
/// ausencias: el PNG sigue existiendo en la carpeta de capturas —es del gestor
/// de capturas, el clipboard no debe borrarlo— y el backfill lo encontraría de
/// nuevo en cada listado.
fn load_dismissed_captures(dir: &Path) -> HashSet<String> {
    std::fs::read_to_string(dismissed_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<HashSet<String>>(&raw).ok())
        .unwrap_or_default()
}

fn save_dismissed_captures(dir: &Path, fingerprints: &HashSet<String>) {
    let captures: HashSet<&String> = fingerprints
        .iter()
        .filter(|f| f.starts_with("capture:"))
        .collect();
    if let Ok(raw) = serde_json::to_string(&captures) {
        let _ = atic_core::write_atomic_str(&dismissed_path(dir), &raw);
    }
}

fn encode_png_rgba(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(
            rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn push_item(state: &mut HistoryState, dir: &Path, mut item: ClipboardItem) {
    if state.deleted_fingerprints.contains(&item.fingerprint) {
        return;
    }
    if state
        .last_fingerprint
        .as_ref()
        .is_some_and(|f| f == &item.fingerprint)
    {
        return;
    }
    // Contenido nuevo distinto al borrado: ya se puede volver a capturar
    // el mismo fingerprint si el usuario lo copia otra vez más adelante.
    //
    // Las capturas se salvan de la limpieza. El razonamiento de arriba es «el
    // usuario lo volvió a copiar a propósito», y para una captura eso no
    // aplica: nadie la vuelve a copiar, la relee `collect_clipboard_items` del
    // directorio. Sin esta excepción, borrar una captura duraba hasta la
    // siguiente copia de cualquier cosa y después reaparecía sola.
    state
        .deleted_fingerprints
        .retain(|f| f.starts_with("capture:"));
    if let Some(idx) = state
        .items
        .iter()
        .position(|existing| existing.fingerprint == item.fingerprint)
    {
        let mut existing = state.items.remove(idx);
        existing.created_at_ms = item.created_at_ms;
        existing.pinned = existing.pinned || item.pinned;
        state.last_fingerprint = Some(existing.fingerprint.clone());
        state.items.insert(0, existing);
        save_history(dir, &state.items);
        return;
    }

    state.last_fingerprint = Some(item.fingerprint.clone());
    if item.source.is_empty() {
        item.source = "watcher".into();
    }
    state.items.insert(0, item);
    prune(state, dir);
    save_history(dir, &state.items);
}

fn prune(state: &mut HistoryState, dir: &Path) {
    while state.items.len() > MAX_ITEMS {
        let remove_idx = state
            .items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| !item.pinned)
            .map(|(i, _)| i)
            .unwrap_or(state.items.len() - 1);
        let removed = state.items.remove(remove_idx);
        if let Some(path) = removed.image_path {
            let p = PathBuf::from(path);
            if p.starts_with(dir) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

/// ¿Lo que hay en el portapapeles pidió no quedar archivado?
///
/// Windows define dos formatos con los que quien copia declara que su contenido
/// es efímero. Los ponen los gestores de contraseñas (Bitwarden, 1Password,
/// KeePass) y algunos navegadores en campos de contraseña:
///
/// - `ExcludeClipboardContentFromMonitorProcessing` — su sola presencia
///   significa «ningún monitor debería tocar esto».
/// - `CanIncludeInClipboardHistory` — un DWORD; `0` es «no lo archives».
///
/// `arboard` no los mira: entrega el texto igual. Sin esta comprobación, una
/// contraseña copiada termina en `history.json` en claro y sobrevive al pegado,
/// que es exactamente lo que el gestor intentó evitar.
#[cfg(windows)]
fn clipboard_is_sensitive() -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    fn format_id(name: &str) -> u32 {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    }

    unsafe {
        let exclude = format_id("ExcludeClipboardContentFromMonitorProcessing");
        if exclude != 0 && IsClipboardFormatAvailable(exclude) != 0 {
            return true;
        }

        let can_include = format_id("CanIncludeInClipboardHistory");
        if can_include == 0 || IsClipboardFormatAvailable(can_include) == 0 {
            return false;
        }

        // El formato está: hay que leer el DWORD, porque un `1` es permiso
        // explícito y tratarlo como negativa perdería ítems legítimos.
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            // Otro proceso lo tiene abierto. Ante la duda no se guarda: perder
            // un ítem se nota y se rehace; archivar una contraseña, no.
            return true;
        }
        let handle = GetClipboardData(can_include);
        let mut opt_out = true;
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u32;
            if !ptr.is_null() {
                opt_out = std::ptr::read_unaligned(ptr) == 0;
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        opt_out
    }
}

/// Fuera de Windows no existen esos formatos, así que no hay nada que consultar.
#[cfg(not(windows))]
fn clipboard_is_sensitive() -> bool {
    false
}

/// Arranca el watcher (una sola vez) y carga el historial desde disco.
///
/// El hilo se levanta siempre, incluso con el historial apagado en Ajustes: la
/// decisión se consulta en cada vuelta, así que encenderlo y apagarlo surte
/// efecto sin reiniciar la app.
pub fn start_watcher(app: &AppHandle) {
    let state = app.state::<AppState>();
    let dir = state.dirs.clipboard_dir();
    let _ = std::fs::create_dir_all(&dir);

    let mut guard = HISTORY.lock_or_recover();
    if guard.is_some() {
        return;
    }
    let items = load_history(&dir);
    let last = items.first().map(|i| i.fingerprint.clone());
    let shared = Arc::new(Mutex::new(HistoryState {
        items,
        last_fingerprint: last,
        suppress_until: None,
        // Las capturas descartadas sí sobreviven al reinicio: el PNG sigue en
        // la carpeta de capturas, así que sin esto el backfill de
        // `collect_clipboard_items` las resucita en cada arranque.
        deleted_fingerprints: load_dismissed_captures(&dir),
        pending_capture: None,
    }));
    *guard = Some(shared.clone());
    drop(guard);

    let handle = app.clone();
    thread::spawn(move || {
        let Ok(mut clipboard) = Clipboard::new() else {
            tracing::warn!("clipboard watcher: no se pudo abrir el portapapeles");
            return;
        };
        loop {
            thread::sleep(Duration::from_millis(POLL_MS));
            let Some(app_state) = handle.try_state::<AppState>() else {
                continue;
            };
            // Se relee en cada vuelta y no una vez al arrancar: apagar el
            // historial en Ajustes tiene que dejar de guardar en el acto, no en
            // el próximo arranque.
            if !app_state.config.lock_or_recover().clipboard_history {
                continue;
            }
            let dir = app_state.dirs.clipboard_dir();

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
                        bytes: Cow::Owned(bytes),
                    };
                    match ingest_image(&shared, &dir, &img) {
                        Ok(changed) if changed => {
                            let _ = handle.emit("clipboard-history-changed", ());
                        }
                        Ok(_) => {}
                        Err(err) => tracing::debug!(%err, "clipboard image ingest"),
                    }
                }
                ClipPoll::Text(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let fp = fingerprint_text(trimmed);
                    let preview: String = trimmed.chars().take(120).collect();
                    let item = ClipboardItem {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: ClipboardKind::Text,
                        preview,
                        text: Some(trimmed.to_string()),
                        image_path: None,
                        created_at_ms: now_ms(),
                        pinned: false,
                        fingerprint: fp,
                        source: "watcher".into(),
                    };
                    let mut hist = shared.lock_or_recover();
                    let before_fp = hist.items.first().map(|i| i.fingerprint.clone());
                    push_item(&mut hist, &dir, item);
                    let after_fp = hist.items.first().map(|i| i.fingerprint.clone());
                    if before_fp != after_fp {
                        drop(hist);
                        let _ = handle.emit("clipboard-history-changed", ());
                    }
                }
            }
        }
    });
}

fn ingest_image(
    shared: &Arc<Mutex<HistoryState>>,
    dir: &Path,
    img: &ImageData<'_>,
) -> Result<bool, String> {
    let w = img.width;
    let h = img.height;
    let bytes = img.bytes.as_ref();
    if bytes.len() > MAX_IMAGE_BYTES * 4 {
        return Err(crate::ui_lang::msg(
            "Imagen demasiado grande",
            "Image is too large",
        ));
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
        // `copy_annotation` graba el ítem ANTES de escribir el portapapeles,
        // así que si esto suprime el ingest el dibujo ya está en la lista.
        if let Some(pending) = hist.pending_capture.take() {
            if pending_capture_matches(&pending, w, h) {
                tracing::debug!(
                    target: "clipboard",
                    fingerprint = %pending.fingerprint,
                    "imagen del portapapeles reconocida como captura propia"
                );
                hist.last_fingerprint = Some(fp);
                return Ok(false);
            }
            // Sigue vigente pero todavía no es esta imagen: devolverla.
            if pending_is_fresh(&pending) {
                hist.pending_capture = Some(pending);
            }
        }
    }
    let png = encode_png_rgba(bytes, w, h)?;
    if png.len() > MAX_IMAGE_BYTES {
        return Err(crate::ui_lang::msg(
            "PNG demasiado grande",
            "PNG is too large",
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let filename = format!("img-{id}.png");
    let path = dir.join(&filename);
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    let item = ClipboardItem {
        id,
        kind: ClipboardKind::Image,
        preview: image_preview_label(w, h),
        text: None,
        image_path: Some(path.to_string_lossy().into_owned()),
        created_at_ms: now_ms(),
        pinned: false,
        fingerprint: fp,
        source: "watcher".into(),
    };
    let mut hist = shared.lock_or_recover();
    let before = hist.items.first().map(|i| i.fingerprint.clone());
    push_item(&mut hist, dir, item);
    let after = hist.items.first().map(|i| i.fingerprint.clone());
    Ok(before != after)
}

fn shared_history() -> Result<Arc<Mutex<HistoryState>>, String> {
    HISTORY.lock_or_recover().as_ref().cloned().ok_or_else(|| {
        crate::ui_lang::msg(
            "El historial del portapapeles no está iniciado.",
            "Clipboard history is not started.",
        )
    })
}

fn find_item(state: &AppState, id: &str) -> Option<ClipboardItem> {
    if let Ok(shared) = shared_history() {
        if let Some(item) = shared.lock_or_recover().items.iter().find(|i| i.id == id) {
            return Some(item.clone());
        }
    }
    let captures = crate::capture::recent_captures_limited(&state.dirs.captures_dir(), 20);
    captures
        .into_iter()
        .find(|c| format!("capture-{}", c.id) == id)
        .map(|cap| ClipboardItem {
            id: id.to_string(),
            kind: ClipboardKind::Image,
            preview: capture_preview_label(&cap.label),
            text: None,
            image_path: Some(cap.path),
            created_at_ms: cap.created_at_ms,
            pinned: false,
            fingerprint: format!("capture:{}", cap.id),
            source: "capture".into(),
        })
}

#[tauri::command]
pub fn list_clipboard_history(state: State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    collect_clipboard_items(&state)
}

/// Payload para insertar un ítem del clipboard en agentes (composer o consola).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsComposerInsert {
    pub kind: ClipboardKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    /// Cursor en CSS del overlay al soltar; la consola elige el panel de debajo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

impl AgentsComposerInsert {
    fn at(
        kind: ClipboardKind,
        text: Option<String>,
        image_path: Option<String>,
        locate: bool,
    ) -> Self {
        let (x, y) = if locate {
            crate::overlay::cursor_css_point()
                .map(|(x, y)| (Some(x), Some(y)))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };
        Self {
            kind,
            text,
            image_path,
            x,
            y,
        }
    }

    fn text(text: String) -> Self {
        Self::at(ClipboardKind::Text, Some(text), None, false)
    }

    fn image(path: String) -> Self {
        Self::at(ClipboardKind::Image, None, Some(path), false)
    }

    fn text_drop(text: String) -> Self {
        Self::at(ClipboardKind::Text, Some(text), None, true)
    }

    fn image_drop(path: String) -> Self {
        Self::at(ClipboardKind::Image, None, Some(path), true)
    }
}

/// True si la burbuja de agentes está a la vista (crate-interno).
///
/// Lo contestaba `is_visible()` sobre su ventana. La consola vive dentro del
/// overlay, así que el estado lo lleva el propio puente.
pub(crate) fn agents_visible(_app: &AppHandle) -> bool {
    crate::agents::bridge::agents_open()
}

/// Inserta texto en el compositor de agentes y le devuelve el foco.
///
/// Evita Ctrl+V externo: ese camino saca el foco y tapa/oculta la burbuja.
pub(crate) fn insert_text_into_agents(app: &AppHandle, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err(crate::ui_lang::msg("Texto vacío", "Empty text"));
    }
    let _ = app.emit(
        "agents-composer-insert",
        AgentsComposerInsert::text(text.to_string()),
    );
    // Sin `set_focus`: la consola ya no es una ventana, y el foco del overlay lo
    // pide el propio campo al recibir el clic (`set_overlay_text_mode`).
    Ok(())
}

/// True si la burbuja de agentes está a la vista.
#[tauri::command]
pub fn agents_window_visible(app: AppHandle) -> bool {
    agents_visible(&app)
}

/// Ruta de archivo para arrastrar un ítem **imagen** (OLE / startDrag).
///
/// El texto ya no usa archivo: ver `start_clipboard_text_drag` (`CF_UNICODETEXT`).
/// Se mantiene para imágenes y para quien aún lea `.atic-drag-*.txt`.
#[tauri::command]
pub fn clipboard_drag_path(state: State<AppState>, id: String) -> Result<String, String> {
    let item = find_item(&state, &id).ok_or_else(item_missing)?;
    match item.kind {
        ClipboardKind::Image => item
            .image_path
            .ok_or_else(|| crate::ui_lang::msg("Imagen sin ruta", "Image has no path")),
        ClipboardKind::Text => {
            let text = item
                .text
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| item.preview.clone());
            if text.is_empty() {
                return Err(item_empty_text());
            }
            let path = state
                .dirs
                .clipboard_dir()
                .join(format!(".atic-drag-{id}.txt"));
            std::fs::write(&path, text.as_bytes())
                .map_err(|e| format!("no se pudo preparar el arrastre: {e}"))?;
            Ok(path.to_string_lossy().into_owned())
        }
    }
}

/// Arrastra texto como `CF_UNICODETEXT` (no como archivo).
///
/// En Windows, `tauri-plugin-drag` solo hace HDROP: soltar un `.txt` en Cursor
/// inserta la ruta. Este comando hace OLE de texto plano.
#[tauri::command]
pub async fn start_clipboard_text_drag(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let item = find_item(&state, &id).ok_or_else(item_missing)?;
    let text = match item.kind {
        ClipboardKind::Text => item
            .text
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| item.preview.clone()),
        ClipboardKind::Image => {
            return Err(crate::ui_lang::msg(
                "Usa arrastre de archivo para imágenes.",
                "Use file drag for images.",
            ))
        }
    };
    if text.is_empty() {
        return Err(item_empty_text());
    }

    #[cfg(windows)]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let drag_text = text.clone();
        app.run_on_main_thread(move || {
            let r = crate::ole_text_drag::drag_unicode_text(&drag_text);
            let _ = tx.send(r);
        })
        .map_err(|e| e.to_string())?;
        let outcome = rx.recv().map_err(|e| e.to_string())??;
        // CANCEL sobre agentes (QueryContinueDrag): insertar en composer/consola.
        if agents_visible(&app) && crate::overlay::cursor_over_hit_id("agents") {
            let _ = app.emit(
                "agents-composer-insert",
                AgentsComposerInsert::text_drop(text),
            );
            return Ok(());
        }
        // Soltado sobre algo que no acepta texto OLE: el drop vuelve con efecto
        // 0 y no pasa nada. Es lo que hacen las consolas y los terminales
        // Electron —donde vive Claude Code—, que solo registran drop-target de
        // archivos. Ahí caemos al mismo camino que el clic, que ahí sí anda.
        if outcome.dropped && outcome.effect == 0 {
            paste_into_window_under_cursor(&app, &text)?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, text);
        Err(crate::ui_lang::msg(
            "El arrastre de texto solo está en Windows.",
            "Text drag is only available on Windows.",
        ))
    }
}

/// Arrastra archivos como `CF_HDROP` con nuestro propio drag source.
///
/// Reemplaza a `tauri-plugin-drag` para imágenes y capturas. El plugin no deja
/// cancelar, y como el overlay queda click-through durante el arrastre, soltar
/// sobre la consola dejaba caer el archivo TAMBIÉN en la app de atrás. Nuestro
/// `QueryContinueDrag` cancela sobre agentes; el caller inserta después con
/// `try_clipboard_drop_on_agents`, igual que ya hacía.
#[tauri::command]
pub async fn start_file_drag(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Err(crate::ui_lang::msg(
            "Nada que arrastrar.",
            "Nothing to drag.",
        ));
    }

    #[cfg(windows)]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(crate::ole_text_drag::drag_files(&paths));
        })
        .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())??;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, paths);
        Err(crate::ui_lang::msg(
            "El arrastre de archivos solo está en Windows.",
            "File drag is only available on Windows.",
        ))
    }
}

/// Lee un `.atic-drag-*.txt` del dir de clipboard (solo esas rutas).
#[tauri::command]
pub fn read_clipboard_drag_text(state: State<AppState>, path: String) -> Result<String, String> {
    let path = PathBuf::from(&path);
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !name.starts_with(".atic-drag-") || !name.ends_with(".txt") {
        return Err(crate::ui_lang::msg(
            "Ruta no permitida.",
            "Path is not allowed.",
        ));
    }
    let dir = state.dirs.clipboard_dir();
    let dir_ok = path
        .canonicalize()
        .ok()
        .zip(dir.canonicalize().ok())
        .map(|(p, d)| p.starts_with(d))
        .unwrap_or_else(|| path.starts_with(&dir));
    if !dir_ok {
        return Err(crate::ui_lang::msg(
            "Ruta fuera del historial.",
            "Path is outside clipboard history.",
        ));
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Deja el ítem en el portapapeles, y nada más.
///
/// `paste_clipboard_item` devuelve el foco a la app anterior y le manda
/// Ctrl+V. Eso es lo correcto desde la pill, que flota sobre la app en la que
/// se estaba escribiendo. Desde la ventana principal no lo es: ahí «la app
/// anterior» es cualquier cosa que estuviera abierta, y pegar le mete texto a
/// un tercero que nadie está mirando. Copiar es lo que esa ventana puede
/// prometer sin adivinar el destino.
#[tauri::command]
pub fn copy_clipboard_item(state: State<AppState>, id: String) -> Result<(), String> {
    let item = find_item(&state, &id).ok_or_else(item_missing)?;

    // El watcher ve cambiar el portapapeles y encolaría lo mismo arriba de
    // todo, duplicado. Se le avisa antes de escribir, como hace el pegado.
    if let Ok(shared) = shared_history() {
        let mut hist = shared.lock_or_recover();
        hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1200));
    }

    match item.kind {
        ClipboardKind::Text => {
            let text = item
                .text
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| item.preview.clone());
            if text.is_empty() {
                return Err(item_empty_text());
            }
            set_system_text(text)
        }
        ClipboardKind::Image => {
            let path = item
                .image_path
                .ok_or_else(|| crate::ui_lang::msg("Imagen sin ruta", "Image has no path"))?;
            crate::capture::copy_png_to_clipboard(Path::new(&path))
        }
    }
}

/// Pone el ítem en el clipboard y envía Ctrl+V a la app que tenía el foco.
///
/// Si la burbuja de agentes está abierta, inserta ahí (evento interno) en vez
/// de pegar en una app externa: el Ctrl+V externo sacaba el foco y dejaba la
/// burbuja tapada justo cuando se quería usarla.
#[tauri::command]
pub fn paste_clipboard_item(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    let item = find_item(&state, &id).ok_or_else(item_missing)?;

    if let Ok(shared) = shared_history() {
        let mut hist = shared.lock_or_recover();
        hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1600));
    }

    if agents_visible(&app) {
        let payload = match item.kind {
            ClipboardKind::Text => {
                let text = item
                    .text
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| item.preview.clone());
                if text.is_empty() {
                    return Err(item_empty_text());
                }
                AgentsComposerInsert::text(text)
            }
            ClipboardKind::Image => {
                let path = item
                    .image_path
                    .ok_or_else(|| crate::ui_lang::msg("Imagen sin ruta", "Image has no path"))?;
                AgentsComposerInsert::image(path)
            }
        };
        let _ = app.emit("agents-composer-insert", payload);
        hide_clipboard_window(app.clone());
        return Ok(());
    }

    // Devolver el foco a la app anterior; si no, Ctrl+V se lo traga la webview.
    //
    // Antes esto además escondía la pill, que tenía ventana propia y sí se
    // quedaba el foco al hacer clic. Dentro del overlay no lo toma nunca.
    restore_foreground_hwnd();
    // WebView2/Electron necesitan un poco más para asentar el foco del hijo Chromium.
    thread::sleep(Duration::from_millis(220));

    #[cfg(windows)]
    let target = resolve_paste_target_hwnd();
    #[cfg(not(windows))]
    let target = None;

    let result = match item.kind {
        ClipboardKind::Text => {
            let text = item
                .text
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| item.preview.clone());
            if text.is_empty() {
                return Err(item_empty_text());
            }
            set_system_text(text)?;
            thread::sleep(Duration::from_millis(80));
            paste_text_hotkey_for(&app, target)
        }
        ClipboardKind::Image => {
            let path = item
                .image_path
                .ok_or_else(|| crate::ui_lang::msg("Imagen sin ruta", "Image has no path"))?;
            crate::capture::copy_png_to_clipboard(Path::new(&path))?;
            thread::sleep(Duration::from_millis(80));
            paste_text_hotkey_for(&app, target)
        }
    };

    hide_clipboard_window(app.clone());
    result
}

/// Inserta un ítem del historial en el composer de agentes (sin ocultar clipboard).
#[tauri::command]
pub fn insert_clipboard_into_agents(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    if !agents_visible(&app) {
        return Err("agentes no está abierto".into());
    }
    let item = find_item(&state, &id).ok_or_else(item_missing)?;
    let payload = match item.kind {
        ClipboardKind::Text => {
            let text = item
                .text
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| item.preview.clone());
            if text.is_empty() {
                return Err(item_empty_text());
            }
            AgentsComposerInsert::text_drop(text)
        }
        ClipboardKind::Image => {
            let path = item
                .image_path
                .ok_or_else(|| crate::ui_lang::msg("Imagen sin ruta", "Image has no path"))?;
            AgentsComposerInsert::image_drop(path)
        }
    };
    let _ = app.emit("agents-composer-insert", payload);
    Ok(())
}

/// Tras OLE de imagen: si el cursor quedó sobre agentes, insertar en composer/consola
/// (misma webview: HDROP a menudo no dispara el HTML5 drop).
#[tauri::command]
pub fn try_clipboard_drop_on_agents(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<bool, String> {
    if !agents_visible(&app) || !crate::overlay::cursor_over_hit_id("agents") {
        return Ok(false);
    }
    insert_clipboard_into_agents(app, state, id)?;
    Ok(true)
}

#[tauri::command]
pub fn pin_clipboard_item(state: State<AppState>, id: String, pinned: bool) -> Result<(), String> {
    let dir = state.dirs.clipboard_dir();
    let shared = shared_history()?;

    {
        let mut hist = shared.lock_or_recover();
        if let Some(item) = hist.items.iter_mut().find(|i| i.id == id) {
            item.pinned = pinned;
            save_history(&dir, &hist.items);
            return Ok(());
        }
        // Captura ya promovida: el listado sigue pudiendo pedir pin con id capture-*
        if let Some(cap_id) = id.strip_prefix("capture-") {
            let fp = format!("capture:{cap_id}");
            if let Some(item) = hist.items.iter_mut().find(|i| i.fingerprint == fp) {
                item.pinned = pinned;
                save_history(&dir, &hist.items);
                return Ok(());
            }
        }
    }

    if !pinned {
        return Err(item_missing());
    }

    // Favoritar captura: copiar PNG al dir de clipboard y persistir en history.json
    let virtual_item = find_item(&state, &id).ok_or_else(item_missing)?;
    let src_path = virtual_item
        .image_path
        .as_deref()
        .ok_or_else(|| crate::ui_lang::msg("Imagen sin ruta", "Image has no path"))?;
    let src = Path::new(src_path);
    if !src.is_file() {
        return Err(crate::ui_lang::msg(
            "Archivo de imagen no encontrado.",
            "Image file was not found.",
        ));
    }

    let new_id = uuid::Uuid::new_v4().to_string();
    let dest = dir.join(format!("img-{new_id}.png"));
    let _ = std::fs::create_dir_all(&dir);
    if src != dest.as_path() {
        std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    }

    let item = ClipboardItem {
        id: new_id,
        kind: ClipboardKind::Image,
        preview: virtual_item.preview,
        text: None,
        image_path: Some(dest.to_string_lossy().into_owned()),
        created_at_ms: virtual_item.created_at_ms,
        pinned: true,
        fingerprint: virtual_item.fingerprint,
        source: if virtual_item.source.is_empty() {
            "capture".into()
        } else {
            virtual_item.source
        },
    };

    let mut hist = shared.lock_or_recover();
    if let Some(existing) = hist
        .items
        .iter_mut()
        .find(|i| i.fingerprint == item.fingerprint)
    {
        existing.pinned = true;
        save_history(&dir, &hist.items);
        return Ok(());
    }
    hist.items.insert(0, item);
    prune(&mut hist, &dir);
    save_history(&dir, &hist.items);
    Ok(())
}

#[tauri::command]
pub fn delete_clipboard_item(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    let dir = state.dirs.clipboard_dir();
    let shared = shared_history()?;
    let mut hist = shared.lock_or_recover();
    let Some(idx) = hist.items.iter().position(|i| i.id == id) else {
        // Captura que todavía viene del backfill del directorio: no está en
        // `items`, así que no hay nada que remover — pero sí hay que anotar el
        // fingerprint, o `collect_clipboard_items` la vuelve a inyectar en el
        // próximo listado. `set_pinned` ya tenía esta rama; el borrado no, y
        // por eso la X de una captura devolvía «Ítem no encontrado».
        if let Some(cap_id) = id.strip_prefix("capture-") {
            hist.deleted_fingerprints
                .insert(format!("capture:{cap_id}"));
            hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1200));
            save_dismissed_captures(&dir, &hist.deleted_fingerprints);
            drop(hist);
            let _ = app.emit("clipboard-history-changed", ());
            return Ok(());
        }
        return Err(item_missing());
    };
    let removed = hist.items.remove(idx);
    hist.deleted_fingerprints
        .insert(removed.fingerprint.clone());
    if removed.fingerprint.starts_with("capture:") {
        save_dismissed_captures(&dir, &hist.deleted_fingerprints);
    }
    // Evita carrera con el poll inmediato del watcher.
    hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1200));
    if let Some(path) = removed.image_path {
        let p = PathBuf::from(path);
        if p.starts_with(&dir) {
            let _ = std::fs::remove_file(p);
        }
    }
    if hist.last_fingerprint.as_deref() == Some(&removed.fingerprint) {
        hist.last_fingerprint = hist.items.first().map(|i| i.fingerprint.clone());
    }
    save_history(&dir, &hist.items);
    drop(hist);
    let _ = app.emit("clipboard-history-changed", ());
    Ok(())
}

/// Saca una captura del historial sin tocar el PNG.
///
/// El archivo es del gestor de capturas; si se borra desde allá, el historial
/// tiene que olvidarlo o deja un ítem con ruta muerta.
pub(crate) fn dismiss_capture(app: &AppHandle, cap_id: &str) {
    let Ok(shared) = shared_history() else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let dir = state.dirs.clipboard_dir();
    let id = format!("capture-{cap_id}");
    let fp = format!("capture:{cap_id}");
    let mut hist = shared.lock_or_recover();
    hist.items
        .retain(|item| item.id != id && item.fingerprint != fp);
    if hist.last_fingerprint.as_deref() == Some(fp.as_str()) {
        hist.last_fingerprint = hist.items.first().map(|i| i.fingerprint.clone());
    }
    hist.deleted_fingerprints.insert(fp);
    save_dismissed_captures(&dir, &hist.deleted_fingerprints);
    save_history(&dir, &hist.items);
    drop(hist);
    let _ = app.emit("clipboard-history-changed", ());
}

/// Vacía el historial conservando los pines.
#[tauri::command]
pub fn clear_clipboard_history(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let dir = state.dirs.clipboard_dir();
    let shared = shared_history()?;
    let mut hist = shared.lock_or_recover();
    let (pinned, rest): (Vec<_>, Vec<_>) = hist.items.drain(..).partition(|i| i.pinned);
    for item in &rest {
        hist.deleted_fingerprints.insert(item.fingerprint.clone());
    }
    save_dismissed_captures(&dir, &hist.deleted_fingerprints);
    hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1200));
    for item in rest {
        if let Some(path) = item.image_path {
            let p = PathBuf::from(path);
            if p.starts_with(&dir) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    hist.items = pinned;
    hist.last_fingerprint = hist.items.first().map(|i| i.fingerprint.clone());
    save_history(&dir, &hist.items);
    drop(hist);
    let _ = app.emit("clipboard-history-changed", ());
    Ok(())
}

/// Pone texto en el portapapeles y lo pega en la ventana en primer plano.
///
/// Default: Ctrl+V. Ctrl+Shift+V solo en terminales Electron/WebView2
/// (Terax, Hyper…), donde Chromium intercepta Ctrl+V.
pub(crate) fn paste_text(app: &AppHandle, text: &str) -> Result<(), String> {
    set_system_text(text.to_string())?;
    thread::sleep(Duration::from_millis(80));
    #[cfg(windows)]
    let target = resolve_paste_target_hwnd();
    #[cfg(not(windows))]
    let target = None;
    paste_text_hotkey_for(app, target)
}

/// True si hay un HWND externo guardado (aunque Atic tenga el foco ahora).
pub(crate) fn has_saved_external_paste_target() -> bool {
    #[cfg(windows)]
    {
        saved_paste_target_hwnd().is_some_and(|h| !is_own_app_hwnd(h))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// True solo si la ventana en primer plano es externa (lista para SendInput).
///
/// Usar en el poller de la cola: un HWND guardado no implica que Ctrl+V
/// llegue ahí mientras Atic sigue en primer plano.
pub(crate) fn has_live_external_foreground() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};
        unsafe {
            let fg = GetForegroundWindow();
            !fg.is_null() && IsWindow(fg) != 0 && !is_own_app_hwnd(fg)
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Graba una captura recién tomada como ítem REAL del historial.
///
/// Antes las capturas no existían en `items`: `collect_clipboard_items` las
/// sintetizaba en cada listado leyendo el directorio. De ahí salían tres cosas:
///
/// 1. Duplicado garantizado. `notify_capture_ready` copia el PNG al
///    portapapeles y el watcher lo graba como «Imagen 488×540», con su propio
///    archivo en el dir de clipboard y un fingerprint de contenido. El dedup
///    del merge compara contra `capture:<id>` y contra la ruta del dir de
///    capturas: ninguna de las dos claves puede empatar con eso, nunca.
/// 2. Imposible de borrar. `delete_clipboard_item` busca por id dentro de
///    `items`, y el ítem virtual no estaba ahí.
/// 3. El PNG quedaba guardado dos veces en disco.
///
/// Se llama ANTES de copiar al portapapeles y suprime al watcher, para que la
/// identidad que quede sea esta y no la que él improvisaría.
pub(crate) fn record_capture(app: &AppHandle, cap: &crate::capture::CaptureItem) {
    push_capture_item(app, cap, true);
}

/// Igual que [`record_capture`], pero sin anunciar `pending_capture`.
///
/// Guardar un dibujo escribe un PNG en capturas y NO lo pone en el
/// portapapeles. Si se anunciara por dimensiones, el watcher podría tragar
/// la captura original (mismo tamaño, todavía en el clipboard) como si
/// fuera esta versión anotada.
pub(crate) fn record_saved_capture(app: &AppHandle, cap: &crate::capture::CaptureItem) {
    push_capture_item(app, cap, false);
}

fn push_capture_item(app: &AppHandle, cap: &crate::capture::CaptureItem, announce_pending: bool) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(shared) = shared_history() else {
        return;
    };
    let dir = state.dirs.clipboard_dir();
    let item = ClipboardItem {
        id: format!("capture-{}", cap.id),
        kind: ClipboardKind::Image,
        preview: capture_preview_label(&cap.label),
        text: None,
        image_path: Some(cap.path.clone()),
        created_at_ms: cap.created_at_ms,
        pinned: false,
        fingerprint: format!("capture:{}", cap.id),
        source: "capture".into(),
    };
    {
        let mut hist = shared.lock_or_recover();
        if announce_pending {
            // El PNG está por entrar al portapapeles. Se anuncia por
            // dimensiones —lo único que sobrevive seguro al round-trip por
            // el DIB— para que `ingest_image` la reconozca en vez de grabar
            // su propia copia.
            hist.pending_capture = Some(PendingCapture {
                fingerprint: item.fingerprint.clone(),
                width: cap.width as usize,
                height: cap.height as usize,
                at: SystemTime::now(),
            });
        }
        push_item(&mut hist, &dir, item);
    }
    let _ = app.emit("clipboard-history-changed", ());
}

/// Archiva un PNG que el usuario copió a propósito (dibujo del editor).
///
/// Hay que grabarlo ANTES de escribir el portapapeles: si no, `pending_capture`
/// de una captura reciente del mismo tamaño hace que el watcher trague el
/// dibujo y no aparezca en el historial.
pub(crate) fn record_copied_png(app: &AppHandle, png: &[u8], width: u32, height: u32) {
    const MAX_COPIED_PNG: usize = 32 * 1024 * 1024;
    if png.is_empty() || png.len() > MAX_COPIED_PNG {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(shared) = shared_history() else {
        return;
    };
    let dir = state.dirs.clipboard_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%err, "no se pudo crear la carpeta del historial");
        return;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let filename = format!("img-{id}.png");
    let path = dir.join(&filename);
    if let Err(err) = std::fs::write(&path, png) {
        tracing::warn!(%err, "no se pudo archivar el dibujo copiado");
        return;
    }
    let item = ClipboardItem {
        id: id.clone(),
        kind: ClipboardKind::Image,
        preview: image_preview_label(width as usize, height as usize),
        text: None,
        image_path: Some(path.to_string_lossy().into_owned()),
        created_at_ms: now_ms(),
        pinned: false,
        fingerprint: format!("annotate:{id}"),
        source: "annotate".into(),
    };
    {
        let mut hist = shared.lock_or_recover();
        hist.pending_capture = Some(PendingCapture {
            fingerprint: item.fingerprint.clone(),
            width: width as usize,
            height: height as usize,
            at: SystemTime::now(),
        });
        push_item(&mut hist, &dir, item);
    }
    let _ = app.emit("clipboard-history-changed", ());
}

/// Ítems del historial para búsqueda u otros agregadores.
///
/// El merge del directorio de capturas quedó solo como **backfill**: capturas
/// viejas, o tomadas antes de que existiera [`record_capture`]. Las nuevas ya
/// llegan como ítems reales.
pub(crate) fn collect_clipboard_items(state: &AppState) -> Result<Vec<ClipboardItem>, String> {
    let shared = shared_history()?;
    let (mut items, deleted) = {
        let hist = shared.lock_or_recover();
        (hist.items.clone(), hist.deleted_fingerprints.clone())
    };

    let captures = crate::capture::recent_captures_limited(&state.dirs.captures_dir(), 20);
    for cap in captures {
        let fp = format!("capture:{}", cap.id);
        // Sin esto el backfill resucita lo borrado en cada listado: `push_item`
        // y `record_image` respetan `deleted_fingerprints`, este camino no lo
        // miraba.
        if deleted.contains(&fp) {
            continue;
        }
        if items
            .iter()
            .any(|i| i.fingerprint == fp || i.image_path.as_deref() == Some(&cap.path))
        {
            continue;
        }
        items.push(ClipboardItem {
            id: format!("capture-{}", cap.id),
            kind: ClipboardKind::Image,
            preview: capture_preview_label(&cap.label),
            text: None,
            image_path: Some(cap.path),
            created_at_ms: cap.created_at_ms,
            pinned: false,
            fingerprint: fp,
            source: "capture".into(),
        });
    }
    items.sort_by(|a, b| {
        a.pinned
            .cmp(&b.pinned)
            .then(b.created_at_ms.cmp(&a.created_at_ms))
    });
    Ok(items)
}

/// HWND destino explícito (p. ej. el guardado al abrir el historial). Evita
/// depender de GetForegroundWindow tras restaurar el foco.
fn paste_text_hotkey_for(
    app: &AppHandle,
    #[cfg(windows)] target: Option<windows_sys::Win32::Foundation::HWND>,
    #[cfg(not(windows))] _target: Option<()>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = target.or_else(resolve_paste_target_hwnd);
        let use_shift = hwnd.is_some_and(hwnd_needs_ctrl_shift_v);
        let exe = hwnd.and_then(process_exe_name).unwrap_or_default();
        // La edad del destino separa un objetivo legítimo (segundos) de uno
        // heredado de hace rato, que es como el texto termina en otra app.
        tracing::info!(
            use_shift,
            %exe,
            visto_hace = %saved_age_label(),
            "pegado: chord {}",
            if use_shift { "Ctrl+Shift+V" } else { "Ctrl+V" }
        );
        if use_shift {
            paste_ctrl_shift_v(app)
        } else {
            paste_ctrl_v()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, _target);
        Err(crate::ui_lang::msg(
            "Pega con Ctrl/Cmd+V; el contenido ya está en el portapapeles.",
            "Paste with Ctrl/Cmd+V; the content is already on the clipboard.",
        ))
    }
}

#[cfg(windows)]
fn saved_paste_target_hwnd() -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

    let raw = PREV_FOREGROUND.load(Ordering::SeqCst);
    if raw == 0 {
        return None;
    }
    unsafe {
        let hwnd = raw as HWND;
        if IsWindow(hwnd) == 0 {
            None
        } else {
            Some(hwnd)
        }
    }
}

/// HWND externo para pegar: guardado al abrir historial/dictado, o primer plano
/// actual si no es una ventana de Atic (la pill roba foco al interactuar).
#[cfg(windows)]
pub(crate) fn last_external_hwnd() -> Option<windows_sys::Win32::Foundation::HWND> {
    resolve_paste_target_hwnd()
}

/// Plan B del arrastre de texto: pegar en la ventana que está bajo el cursor.
///
/// Hay targets que no registran drop-target de TEXTO: las consolas clásicas y
/// los terminales Electron (Terax, Hyper, Tabby…) solo aceptan archivos, así
/// que el drop se rechaza en silencio y el usuario ve que "no pasa nada".
/// Pero apuntó a una ventana bien clara, así que se le da lo que pidió por el
/// camino que sí funciona en todas partes: el mismo del clic —portapapeles más
/// el chord correcto, que `paste_text_hotkey_for` ya elige por exe.
///
/// Solo se llama tras un drop REAL: un Esc no llega acá (ver `DragOutcome`).
#[cfg(windows)]
fn paste_into_window_under_cursor(app: &AppHandle, text: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, IsWindow, WindowFromPoint, GA_ROOT,
    };

    let hwnd = unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return Ok(());
        }
        let under = WindowFromPoint(pt);
        if under.is_null() {
            return Ok(());
        }
        // La ventana RAÍZ, no el control bajo el puntero: tanto el foco como la
        // elección del chord razonan sobre el top-level.
        GetAncestor(under, GA_ROOT)
    };
    // Soltar sobre el escritorio —o sobre nosotros mismos— no pega nada.
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 || is_own_app_hwnd(hwnd) {
        return Ok(());
    }

    // Nuestro propio pegado no debe volver a entrar al historial.
    if let Ok(shared) = shared_history() {
        let mut hist = shared.lock_or_recover();
        hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1600));
    }

    force_foreground(hwnd);
    // Mismo respiro que el clic: WebView2/Electron tardan en asentar el foco del
    // hijo Chromium, y sin esto el chord se pierde.
    thread::sleep(Duration::from_millis(220));
    set_system_text(text.to_string())?;
    thread::sleep(Duration::from_millis(80));
    paste_text_hotkey_for(app, Some(hwnd))
}

#[cfg(windows)]
fn resolve_paste_target_hwnd() -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};

    if let Some(hwnd) = saved_paste_target_hwnd().filter(|h| !is_own_app_hwnd(*h)) {
        return Some(hwnd);
    }
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() || IsWindow(fg) == 0 || is_own_app_hwnd(fg) {
            None
        } else {
            Some(fg)
        }
    }
}

#[cfg(windows)]
pub fn is_own_app_hwnd(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    // PID del proceso actual: cubre atic-desktop.exe (dev) y Atic.exe (release).
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != 0 && pid == GetCurrentProcessId() {
            return true;
        }
    }
    process_exe_name(hwnd)
        .is_some_and(|exe| matches!(exe.as_str(), "atic-desktop.exe" | "atic.exe"))
}

/// True solo si el destino necesita Ctrl+Shift+V para pegar.
///
/// Consolas nativas (Windows Terminal, cmd, Cascadia) ya aceptan Ctrl+V.
/// El caso difícil son terminales Electron/WebView2 (Terax…): ahí Ctrl+V
/// lo come Chromium y el paste del xterm es Ctrl+Shift+V.
///
/// No matchear por título ni por `claude.exe` (chat ≠ terminal).
#[cfg(windows)]
fn hwnd_needs_ctrl_shift_v(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    let Some(exe) = process_exe_name(hwnd) else {
        return false;
    };
    exe_needs_ctrl_shift_v(&exe)
}

/// Terminales Electron/WebView2 donde el paste es Ctrl+Shift+V.
#[cfg(windows)]
fn exe_needs_ctrl_shift_v(exe: &str) -> bool {
    matches!(
        exe,
        "terax.exe" | "hyper.exe" | "tabby.exe" | "terminus.exe" | "electerm.exe"
    )
}

#[cfg(windows)]
fn process_exe_name(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut path_buf = [0u16; 1024];
        let mut path_len = path_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, path_buf.as_mut_ptr(), &mut path_len);
        CloseHandle(handle);
        if ok == 0 || path_len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
        Some(
            path.rsplit(['\\', '/'])
                .next()
                .unwrap_or(&path)
                .to_ascii_lowercase(),
        )
    }
}

fn paste_ctrl_v() -> Result<(), String> {
    #[cfg(windows)]
    {
        send_paste_chord(false)
    }
    #[cfg(not(windows))]
    {
        Err(crate::ui_lang::msg(
            "Pega con Ctrl/Cmd+V; el contenido ya está en el portapapeles.",
            "Paste with Ctrl/Cmd+V; the content is already on the clipboard.",
        ))
    }
}

/// Ctrl+Shift+V: el atajo global del historial es el mismo chord por defecto.
/// Hay que suspenderlo o RegisterHotKey se come el SendInput y no llega a Terax.
#[cfg(windows)]
fn paste_ctrl_shift_v(app: &AppHandle) -> Result<(), String> {
    let clipboard_sc = app
        .try_state::<AppState>()
        .map(|s| s.config.lock_or_recover().clipboard_shortcut.clone());

    let suspended = clipboard_sc
        .as_deref()
        .and_then(|raw| suspend_key_shortcut(app, raw));

    let result = send_paste_chord(true);

    if suspended.is_some() {
        // Pequeña pausa para que el chord termine de procesarse antes de
        // volver a registrar el hotkey global.
        thread::sleep(Duration::from_millis(50));
        reregister_shortcuts_from_config(app);
    }

    result
}

/// Quita un atajo de teclado global si es parseable (no mouse).
#[cfg(windows)]
fn suspend_key_shortcut(app: &AppHandle, raw: &str) -> Option<()> {
    if crate::mouse_bindings::parse_side_button(raw).is_some() {
        return None;
    }
    let Ok(sc) = raw.parse::<Shortcut>() else {
        return None;
    };
    match app.global_shortcut().unregister(sc) {
        Ok(()) => {
            tracing::debug!(%raw, "atajo global suspendido para pegado");
            Some(())
        }
        Err(err) => {
            tracing::debug!(%raw, %err, "no se pudo suspender atajo (puede no estar registrado)");
            // Igual intentamos el paste; a veces no estaba registrado.
            Some(())
        }
    }
}

#[cfg(windows)]
fn reregister_shortcuts_from_config(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let cfg = state.config.lock_or_recover().clone();
    if let Err(err) = crate::shortcuts::register_shortcuts(
        app,
        crate::shortcuts::ShortcutBindings {
            recording: &cfg.global_shortcut,
            dictation: &cfg.dictation_shortcut,
            summon_pill: &cfg.summon_pill_shortcut,
            pill_radial: &cfg.pill_radial_shortcut,
            clipboard: &cfg.clipboard_shortcut,
            snippets: &cfg.snippets_shortcut,
            agents: &cfg.agents_shortcut,
            screenshot: &cfg.screenshot_shortcut,
            board: &cfg.board_shortcut,
            color: &cfg.color_shortcut,
            launcher: &cfg.launcher_shortcut,
        },
    ) {
        tracing::warn!(%err, "no se pudieron re-registrar atajos tras pegado");
    }
}

#[cfg(windows)]
fn send_paste_chord(with_shift: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RMENU, VK_RSHIFT, VK_RWIN,
        VK_SHIFT, VK_V,
    };

    unsafe fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    unsafe {
        let ctrl = VK_CONTROL as VIRTUAL_KEY;
        let shift = VK_SHIFT as VIRTUAL_KEY;
        let v = VK_V as VIRTUAL_KEY;

        // Soltar los modificadores que estén FÍSICAMENTE hundidos y no formen
        // parte del chord.
        //
        // El dictado es mantener-para-hablar, así que al inyectar el pegado es
        // normal que todavía haya un Alt o un Win apretado del propio atajo.
        // Ahí nuestro Ctrl+Shift+V le llega a la app como Ctrl+Shift+Alt+V: no
        // lo reconoce como pegar y Windows contesta con el "ding". Eso es lo
        // que hacía que pegara a veces sí y a veces no —dependía de cuánto
        // tardaras en soltar la tecla— y por qué hacer clic primero lo
        // arreglaba: para cuando clickeabas, ya no quedaba nada hundido.
        let mut stray: Vec<VIRTUAL_KEY> = vec![VK_LWIN, VK_RWIN, VK_LMENU, VK_RMENU];
        if !with_shift {
            stray.push(VK_LSHIFT);
            stray.push(VK_RSHIFT);
        }
        let held: Vec<VIRTUAL_KEY> = stray
            .into_iter()
            .filter(|k| GetAsyncKeyState(*k as i32) < 0)
            .collect();
        if !held.is_empty() {
            tracing::debug!(
                target: "paste_geo",
                "MODIF      {} modificador(es) hundidos, los suelto antes del chord",
                held.len()
            );
        }
        let mut inputs: Vec<INPUT> = held.into_iter().map(|k| key(k, true)).collect();

        inputs.extend(if with_shift {
            vec![
                key(ctrl, false),
                key(shift, false),
                key(v, false),
                key(v, true),
                key(shift, true),
                key(ctrl, true),
            ]
        } else {
            vec![
                key(ctrl, false),
                key(v, false),
                key(v, true),
                key(ctrl, true),
            ]
        });
        let sent = SendInput(
            inputs.len() as u32,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if sent == 0 {
            return Err(crate::ui_lang::msg(
                "No se pudo pegar. El contenido quedó en el portapapeles (Ctrl+V / Ctrl+Shift+V).",
                "Could not paste. The content stayed on the clipboard (Ctrl+V / Ctrl+Shift+V).",
            ));
        }
    }
    Ok(())
}

/// Float de clipboard abierto (independiente de la pill).
static CLIPBOARD_OPEN: AtomicBool = AtomicBool::new(false);

/// Preferencia de pin: float fijado arriba mientras está abierto.
static CLIPBOARD_ALWAYS_ON_TOP: AtomicBool = AtomicBool::new(false);

const CLIP_ANCHOR: &str = "clipboard-bubble-anchor";
const CLIP_DISMISS: &str = "clipboard-bubble-dismiss";

/// API simétrica a `agents_open`. El stacking del overlay ya no la consulta.
#[allow(dead_code)]
pub fn float_open() -> bool {
    CLIPBOARD_OPEN.load(Ordering::Relaxed)
}

pub fn float_always_on_top() -> bool {
    CLIPBOARD_ALWAYS_ON_TOP.load(Ordering::Relaxed)
}

pub fn init_always_on_top(on: bool) {
    CLIPBOARD_ALWAYS_ON_TOP.store(on, Ordering::Relaxed);
}

/// Abrir el float de clipboard (idempotente). El cierre lo decide el overlay.
pub fn summon_clipboard_panel(app: &AppHandle) {
    save_foreground_hwnd();
    tracing::info!(target: "overlay", "show clipboard float");
    crate::panel_float::show(
        app,
        &CLIPBOARD_OPEN,
        crate::panel_float::PANEL_SHAPE,
        CLIP_ANCHOR,
    );
    crate::overlay::set_topmost(app, crate::agents::bridge::overlay_should_be_topmost());
}

#[tauri::command]
pub fn show_clipboard_window(app: AppHandle) {
    summon_clipboard_panel(&app);
}

#[tauri::command]
pub fn hide_clipboard_window(app: AppHandle) {
    crate::panel_float::hide(&app, &CLIPBOARD_OPEN, CLIP_DISMISS);
    crate::overlay::set_topmost(&app, crate::agents::bridge::overlay_should_be_topmost());
}

#[tauri::command]
pub fn clipboard_always_on_top() -> bool {
    float_always_on_top()
}

#[tauri::command]
pub fn set_clipboard_always_on_top(app: AppHandle, on: bool) {
    CLIPBOARD_ALWAYS_ON_TOP.store(on, Ordering::Relaxed);
    if let Some(state) = app.try_state::<AppState>() {
        let snapshot = {
            let Ok(mut cfg) = state.config.lock() else {
                crate::overlay::set_topmost(
                    &app,
                    crate::agents::bridge::overlay_should_be_topmost(),
                );
                return;
            };
            cfg.clipboard_always_on_top = on;
            cfg.clone()
        };
        let _ = snapshot.save(&state.dirs.config_path());
    }
    crate::overlay::set_topmost(&app, crate::agents::bridge::overlay_should_be_topmost());
}

/// Prepara la pill para abrir un panel (historial o fragmentos).
///
/// Guarda la posición actual —el «hogar»— una sola vez por sesión, para
/// restaurarla al cerrar o pegar. Con `fly`, además la lleva al cursor (camino
/// del atajo global); sin él, el panel se expande donde la pill ya está.
///
/// Ya no registra Escape como atajo global. Ese hook secuestraba la tecla en
/// TODAS las apps del sistema (`RegisterHotKey` la consume) y era redundante:
/// si la pill tiene el foco, el handler local la cierra; y si lo pierde, el
/// cierre por blur ya se encarga.
/// Devuelve los milisegundos que dura el vuelo hasta el cursor (0 si no vuela).
/// El frontend espera ese tiempo antes de expandir el panel: crecer a mitad del
/// vuelo dejaba el panel abierto en un punto intermedio del recorrido.
#[tauri::command]
pub fn prepare_clipboard_pill(app: AppHandle, fly: bool) -> Result<u64, String> {
    tracing::info!(target: "pill_geo", "CMD        prepare_pill fly={fly}");
    stash_pre_clipboard_position(&app);
    if !fly {
        return Ok(0);
    }
    let flight = crate::state::animate_pill_to_cursor(&app).ok_or_else(|| {
        crate::ui_lang::msg(
            "No se pudo colocar la pill en el cursor.",
            "Could not move the pill to the cursor.",
        )
    })?;
    Ok(flight.ms)
}

/// Guarda el hogar de la pill **sin moverla** y la hace visible.
///
/// Tiene que correr ANTES de cualquier reencuadre. Antes esto vivía dentro de
/// `snap_pill_to_cursor`, que primero dejaba que el frontend redimensionara y
/// recién después guardaba: para entonces el `resize` con pivote al centro ya
/// había corrido la ventana 115 px, y ese punto desplazado quedaba grabado
/// como "hogar". Cada ciclo de rueda perdía 115 px arriba y a la izquierda —
/// la deriva era exactamente medio crecimiento de la rueda, por ciclo.
#[tauri::command]
pub fn stash_pill_home(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("pill").ok_or_else(|| {
        crate::ui_lang::msg(
            "No existe la ventana pill.",
            "The pill window does not exist.",
        )
    })?;
    stash_pre_clipboard_position(&app);
    tracing::info!(target: "pill_geo", "CMD        stash_pill_home");
    crate::state::set_pill_visible(&app, true);
    Ok(())
}

/// Devuelve la pill al hogar guardado. `true` si había uno y se aplicó.
#[tauri::command]
pub fn restore_pill_position(app: AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    let home = *state.pre_clipboard_position.lock_or_recover();
    tracing::info!(target: "pill_geo", "CMD        restore_pill_position home={home:?}");
    let Some((x, y)) = state.pre_clipboard_position.lock_or_recover().take() else {
        return Ok(false);
    };

    let target_x = x.round() as i32;
    let target_y = y.round() as i32;
    crate::state::animate_pill_to(&app, target_x, target_y);

    {
        let mut cfg = state.config.lock_or_recover();
        cfg.pill_position = Some((x, y));
        let snapshot = cfg.clone();
        drop(cfg);
        let _ = snapshot.save(&state.dirs.config_path());
    }
    Ok(true)
}

/// Encoge y vuelve al hogar en UN solo movimiento (cierre de la rueda).
///
/// El camino viejo eran dos: `resize` con pivote al centro —que corría la
/// ventana +115 px al implosionar— y recién después el vuelo al hogar desde ese
/// punto. Interpolando rectángulo completo no hay punto intermedio: la rueda se
/// achica mientras viaja.
///
/// Devuelve `false` si no había hogar guardado; ahí el llamador encoge en el
/// lugar por el camino normal.
#[tauri::command]
pub fn morph_pill_home(app: AppHandle, width: f64, height: f64) -> Result<bool, String> {
    let window = app.get_webview_window("pill").ok_or_else(|| {
        crate::ui_lang::msg(
            "No existe la ventana pill.",
            "The pill window does not exist.",
        )
    })?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let w = (width * scale).round() as i32;
    let h = (height * scale).round() as i32;

    let Some(state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    let Some((hx, hy)) = state.pre_clipboard_position.lock_or_recover().take() else {
        tracing::info!(target: "pill_geo", "CMD        morph_pill_home SIN HOGAR");
        return Ok(false);
    };

    // El clamp usa el tamaño FINAL, no el actual: es el error que hacía que el
    // destino se calculara contra el borde con las medidas de la rueda.
    let (x, y) = crate::floating::clamp(hx.round() as i32, hy.round() as i32, w, h);
    tracing::info!(
        target: "pill_geo",
        "CMD        morph_pill_home hogar=({hx},{hy}) size=({w},{h}) -> ({x},{y})"
    );
    crate::floating::tween(&app, "pill", crate::floating::Rect { x, y, w, h });

    let mut cfg = state.config.lock_or_recover();
    let next = Some((f64::from(x), f64::from(y)));
    // Cerrar la rueda devuelve la pill al MISMO hogar casi siempre: sin esta
    // guarda, spamear el atajo reescribía config.json en cada ciclo.
    if cfg.pill_position != next {
        cfg.pill_position = next;
        let snapshot = cfg.clone();
        drop(cfg);
        let _ = snapshot.save(&state.dirs.config_path());
    }
    Ok(true)
}

/// Suelta el hogar temporal sin mover la pill (la trajo un summon permanente).
pub(crate) fn unregister_clipboard_escape_close(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.pre_clipboard_position.lock_or_recover() = None;
    }
}

/// Guarda la posición home solo la primera vez de la sesión (reabrir en el
/// cursor no debe pisar el home original).
fn stash_pre_clipboard_position(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut pre = state.pre_clipboard_position.lock_or_recover();
    if pre.is_some() {
        return;
    }
    // Posición en REPOSO, no la viva. Spameando el atajo, una apertura puede
    // caer en medio del cierre anterior; leer la posición del momento grabaría
    // como hogar un punto a mitad de camino y la pill se iría caminando.
    *pre = crate::floating::resting_position(app, "pill")
        .map(|(x, y)| (f64::from(x), f64::from(y)))
        .or_else(|| state.config.lock_or_recover().pill_position);
}

/// Guarda la ventana en foco para pegar después (dictado / clipboard).
pub(crate) fn remember_paste_target() {
    save_foreground_hwnd();
}

/// Devuelve el foco a la ventana guardada con [`remember_paste_target`].
pub(crate) fn focus_paste_target() {
    restore_foreground_hwnd();
}

fn save_foreground_hwnd() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        unsafe {
            let hwnd = GetForegroundWindow();
            // Foco en Atic (o nulo): conservar el destino previo. La pill roba
            // el foco al abrirse, así que el bueno es el de antes.
            if hwnd.is_null() || is_own_app_hwnd(hwnd) {
                return;
            }
            let raw = hwnd as isize;
            let changed = PREV_FOREGROUND.swap(raw, Ordering::SeqCst) != raw;
            // Se sella SIEMPRE, cambie o no: el dato útil es "hace cuánto vi
            // este destino por última vez", no "hace cuánto cambió". Sellando
            // solo al cambiar, un destino que se estuvo confirmando cada 200 ms
            // se reportaba con minutos de antigüedad y parecía basura heredada.
            *SAVED_AT.lock_or_recover() = Some(std::time::Instant::now());
            // El log sí es solo al cambiar: esto corre cada 200 ms durante el
            // dictado y no queremos una línea por tick.
            if !changed {
                return;
            }
            let exe = process_exe_name(hwnd).unwrap_or_else(|| "?".into());
            tracing::debug!(target: "paste_geo", "DESTINO    {exe}");
        }
    }
}

/// Sigue la ventana externa en foco mientras dura un dictado.
///
/// Un solo snapshot al arrancar no alcanza: entre que empiezas a hablar y que el
/// texto está listo pasan segundos, y en ese rato es normal hacer clic en el
/// input donde realmente lo quieres. El destino tiene que ser la última ventana
/// que tocaste, no la que estaba cuando apretaste el atajo.
pub(crate) fn start_foreground_tracking() {
    if TRACKING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| {
        while TRACKING.load(Ordering::SeqCst) {
            save_foreground_hwnd();
            thread::sleep(Duration::from_millis(TRACK_MS));
        }
    });
}

/// Congela el destino. A partir de acá el foco lo movemos nosotros, así que
/// seguir mirándolo solo agregaría ruido.
pub(crate) fn stop_foreground_tracking() {
    TRACKING.store(false, Ordering::SeqCst);
}

/// Hace cuánto se guardó el destino de pegado, para las trazas.
fn saved_age_label() -> String {
    match *SAVED_AT.lock_or_recover() {
        Some(at) => format!("{:?}", at.elapsed()),
        None => "nunca".to_string(),
    }
}

fn restore_foreground_hwnd() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

        let raw = PREV_FOREGROUND.load(Ordering::SeqCst);
        if raw == 0 {
            tracing::debug!(target: "paste_geo", "RESTORE    sin destino guardado");
            return;
        }
        unsafe {
            let hwnd = raw as HWND;
            if IsWindow(hwnd) == 0 {
                tracing::debug!(target: "paste_geo", "RESTORE    el destino guardado ya no existe");
                return;
            }
            force_foreground(hwnd);
            // Verificar que el SO nos hizo caso. `SetForegroundWindow` puede
            // fallar en silencio: Windows se lo niega a procesos sin foco ni
            // input reciente, y ahí las teclas se van a otro lado.
            let now = GetForegroundWindow();
            let exe = process_exe_name(hwnd).unwrap_or_else(|| "?".into());
            if now == hwnd {
                tracing::debug!(target: "paste_geo", "RESTORE    ok destino={exe}");
            } else {
                let actual = if now.is_null() {
                    "ninguna".to_string()
                } else {
                    process_exe_name(now).unwrap_or_else(|| "?".into())
                };
                tracing::debug!(
                    target: "paste_geo",
                    "RESTORE    FALLO queria={exe} pero el frente es={actual}"
                );
            }
        }
    }
}

/// Registra qué control tiene el foco de teclado en el primer plano actual.
///
/// Es el dato que faltaba: mandar el chord a una ventana sin campo editable se
/// ve EXACTAMENTE igual que un pegado exitoso —`SendInput` no informa si
/// alguien atendió la tecla—, así que "pegado, queued=false" no prueba nada.
/// Con la clase del control enfocado se distingue un fallo de foco de un fallo
/// de tecla sin tener que adivinar.
pub(crate) fn log_focus_state() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
        };
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() {
                return;
            }
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(fg, &mut pid);
            let cur = GetCurrentThreadId();
            // GetFocus habla de la cola del hilo llamador: sin acoplar, siempre
            // contestaría por Atic en vez de por la app destino.
            let attached = tid != 0 && tid != cur && AttachThreadInput(cur, tid, 1) != 0;
            let focused = GetFocus();
            let class = if focused.is_null() {
                "NINGUNO".to_string()
            } else {
                let mut buf = [0u16; 256];
                let len = GetClassNameW(focused, buf.as_mut_ptr(), buf.len() as i32);
                String::from_utf16_lossy(&buf[..len.max(0) as usize])
            };
            if attached {
                let _ = AttachThreadInput(cur, tid, 0);
            }
            tracing::debug!(target: "paste_geo", "FOCO       control={class}");
        }
    }
}

/// Trae una ventana al primer plano y deja su input listo para escribir.
///
/// Nació para el pegado —traer la app destino antes del Ctrl+V— y la usa
/// también el modo texto del overlay: es la única forma de activar una ventana
/// sin que `tao` inyecte `VK_LMENU` cuando `SetForegroundWindow` le falla, y
/// esta app manda teclas de verdad por el mismo canal.
#[cfg(windows)]
pub fn force_foreground(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        SetForegroundWindow, ASFW_ANY,
    };

    unsafe {
        let mut target_pid = 0u32;
        let target_tid = GetWindowThreadProcessId(hwnd, &mut target_pid);
        if target_pid != 0 {
            let _ = AllowSetForegroundWindow(target_pid);
        } else {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }

        let fg = GetForegroundWindow();
        let cur_tid = GetCurrentThreadId();
        let mut fg_pid = 0u32;
        let fg_tid = if !fg.is_null() {
            GetWindowThreadProcessId(fg, &mut fg_pid)
        } else {
            0
        };

        let attached_fg =
            fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, 1) != 0;
        let attached_tgt = target_tid != 0
            && target_tid != cur_tid
            && target_tid != fg_tid
            && AttachThreadInput(cur_tid, target_tid, 1) != 0;

        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);

        // NO se fuerza el foco a un hijo concreto.
        //
        // Acá había un `SetFocus` sobre "el último hijo `Chrome_*` visible",
        // una adivinanza: en Electron/WebView2 no hay forma de saber cuál es el
        // campo real, y en una app con varios paneles esa adivinanza cae en el
        // widget equivocado y MUEVE el cursor fuera del input. Al activar la
        // ventana, Chromium restaura solo el foco interno que tenía —el cursor
        // donde el usuario lo dejó—, que es mejor que cualquier suposición
        // nuestra. Quitar esto mismo del camino del dictado fue lo que lo hizo
        // funcionar.
        if attached_tgt {
            let _ = AttachThreadInput(cur_tid, target_tid, 0);
        }
        if attached_fg {
            let _ = AttachThreadInput(cur_tid, fg_tid, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(width: usize, height: usize, age: Duration) -> PendingCapture {
        PendingCapture {
            fingerprint: "capture:test.png".into(),
            width,
            height,
            at: SystemTime::now() - age,
        }
    }

    #[test]
    fn pending_fresco_del_mismo_tamano_se_reconoce() {
        let p = pending(805, 38, Duration::from_millis(200));
        assert!(pending_capture_matches(&p, 805, 38));
    }

    #[test]
    fn pending_no_traga_otro_tamano() {
        let p = pending(805, 38, Duration::from_millis(200));
        assert!(!pending_capture_matches(&p, 1920, 1080));
    }

    #[test]
    fn pending_caduca_a_los_diez_segundos() {
        let p = pending(805, 38, Duration::from_secs(11));
        assert!(!pending_is_fresh(&p));
        assert!(!pending_capture_matches(&p, 805, 38));
    }
}
