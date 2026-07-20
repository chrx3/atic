//! Historial local del portapapeles (texto + imágenes).
//!
//! Un hilo hace polling de `arboard`, persiste en `data/clipboard/` y expone
//! comandos para listar, pegar, fijar y borrar.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arboard::{Clipboard, ImageData};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

/// Ventana en primer plano antes de abrir el panel (para pegar ahí).
static PREV_FOREGROUND: AtomicIsize = AtomicIsize::new(0);

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

#[derive(Default)]
struct HistoryState {
    items: Vec<ClipboardItem>,
    last_fingerprint: Option<String>,
    /// Evita re-capturar lo que nosotros mismos pegamos.
    suppress_until: Option<SystemTime>,
}

static HISTORY: Mutex<Option<Arc<Mutex<HistoryState>>>> = Mutex::new(None);

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

fn save_history(dir: &Path, items: &[ClipboardItem]) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(raw) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(history_path(dir), raw);
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
    if state
        .last_fingerprint
        .as_ref()
        .is_some_and(|f| f == &item.fingerprint)
    {
        return;
    }
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

/// Arranca el watcher (una sola vez) y carga el historial desde disco.
pub fn start_watcher(app: &AppHandle) {
    let state = app.state::<AppState>();
    let dir = state.dirs.clipboard_dir();
    let _ = std::fs::create_dir_all(&dir);

    let mut guard = HISTORY.lock().unwrap();
    if guard.is_some() {
        return;
    }
    let items = load_history(&dir);
    let last = items.first().map(|i| i.fingerprint.clone());
    let shared = Arc::new(Mutex::new(HistoryState {
        items,
        last_fingerprint: last,
        suppress_until: None,
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
            let dir = app_state.dirs.clipboard_dir();

            {
                let mut hist = shared.lock().unwrap();
                if let Some(until) = hist.suppress_until {
                    if SystemTime::now() < until {
                        continue;
                    }
                    hist.suppress_until = None;
                }
            }

            if let Ok(img) = clipboard.get_image() {
                match ingest_image(&shared, &dir, &img) {
                    Ok(changed) if changed => {
                        let _ = handle.emit("clipboard-history-changed", ());
                    }
                    Ok(_) => {}
                    Err(err) => tracing::debug!(%err, "clipboard image ingest"),
                }
                continue;
            }
            if let Ok(text) = clipboard.get_text() {
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
                let mut hist = shared.lock().unwrap();
                let before_fp = hist.items.first().map(|i| i.fingerprint.clone());
                push_item(&mut hist, &dir, item);
                let after_fp = hist.items.first().map(|i| i.fingerprint.clone());
                if before_fp != after_fp {
                    drop(hist);
                    let _ = handle.emit("clipboard-history-changed", ());
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
        return Err("imagen demasiado grande".into());
    }
    let fp = fingerprint_image(bytes, w, h);
    {
        let hist = shared.lock().unwrap();
        if hist.last_fingerprint.as_ref() == Some(&fp) {
            return Ok(false);
        }
    }
    let png = encode_png_rgba(bytes, w, h)?;
    if png.len() > MAX_IMAGE_BYTES {
        return Err("png demasiado grande".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let filename = format!("img-{id}.png");
    let path = dir.join(&filename);
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    let item = ClipboardItem {
        id,
        kind: ClipboardKind::Image,
        preview: format!("Imagen {w}×{h}"),
        text: None,
        image_path: Some(path.to_string_lossy().into_owned()),
        created_at_ms: now_ms(),
        pinned: false,
        fingerprint: fp,
        source: "watcher".into(),
    };
    let mut hist = shared.lock().unwrap();
    let before = hist.items.first().map(|i| i.fingerprint.clone());
    push_item(&mut hist, dir, item);
    let after = hist.items.first().map(|i| i.fingerprint.clone());
    Ok(before != after)
}

fn shared_history() -> Result<Arc<Mutex<HistoryState>>, String> {
    HISTORY
        .lock()
        .unwrap()
        .as_ref()
        .cloned()
        .ok_or_else(|| "historial de clipboard no iniciado".into())
}

fn find_item(state: &AppState, id: &str) -> Option<ClipboardItem> {
    if let Ok(shared) = shared_history() {
        if let Some(item) = shared.lock().unwrap().items.iter().find(|i| i.id == id) {
            return Some(item.clone());
        }
    }
    let captures = crate::capture::recent_captures_limited(&state.dirs.captures_dir(), 20);
    captures.into_iter().find(|c| format!("capture-{}", c.id) == id).map(|cap| {
        ClipboardItem {
            id: id.to_string(),
            kind: ClipboardKind::Image,
            preview: if cap.label.is_empty() {
                "Captura".into()
            } else {
                format!("Captura {}", cap.label)
            },
            text: None,
            image_path: Some(cap.path),
            created_at_ms: cap.created_at_ms,
            pinned: false,
            fingerprint: format!("capture:{}", cap.id),
            source: "capture".into(),
        }
    })
}

/// Lista el historial (más reciente primero), fusionando capturas de Atic.
#[tauri::command]
pub fn list_clipboard_history(state: State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    let shared = shared_history()?;
    let mut items = shared.lock().unwrap().items.clone();

    let captures = crate::capture::recent_captures_limited(&state.dirs.captures_dir(), 20);
    for cap in captures {
        let fp = format!("capture:{}", cap.id);
        if items.iter().any(|i| i.fingerprint == fp || i.image_path.as_deref() == Some(&cap.path))
        {
            continue;
        }
        items.push(ClipboardItem {
            id: format!("capture-{}", cap.id),
            kind: ClipboardKind::Image,
            preview: if cap.label.is_empty() {
                "Captura".into()
            } else {
                format!("Captura {}", cap.label)
            },
            text: None,
            image_path: Some(cap.path),
            created_at_ms: cap.created_at_ms,
            pinned: false,
            fingerprint: fp,
            source: "capture".into(),
        });
    }
    items.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.created_at_ms.cmp(&a.created_at_ms))
    });
    Ok(items)
}

/// Pone el ítem en el clipboard y envía Ctrl+V a la app que tenía el foco.
#[tauri::command]
pub fn paste_clipboard_item(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let item = find_item(&state, &id).ok_or_else(|| "Ítem no encontrado".to_string())?;

    if let Ok(shared) = shared_history() {
        let mut hist = shared.lock().unwrap();
        hist.suppress_until = Some(SystemTime::now() + Duration::from_millis(1600));
    }

    // La pill tiene el foco al hacer clic: hay que ocultarla y devolver el foco
    // a la app anterior; si no, Ctrl+V se traga la propia webview.
    let pill = app.get_webview_window("pill");
    if let Some(ref win) = pill {
        let _ = win.hide();
    }
    restore_foreground_hwnd();
    // WebView2/Electron necesitan un poco más para asentar el foco del hijo Chromium.
    thread::sleep(Duration::from_millis(220));

    let result = match item.kind {
        ClipboardKind::Text => {
            let text = item.text.unwrap_or_default();
            paste_text(&text)
        }
        ClipboardKind::Image => {
            let path = item
                .image_path
                .ok_or_else(|| "imagen sin ruta".to_string())?;
            crate::capture::copy_png_to_clipboard(Path::new(&path))?;
            thread::sleep(Duration::from_millis(80));
            // Mismo criterio que texto: consolas suelen querer Ctrl+Shift+V.
            paste_text_hotkey()
        }
    };

    let _ = app.emit("pill-clipboard-close", ());
    if let Some(win) = pill {
        let _ = win.show();
    }
    result
}

#[tauri::command]
pub fn pin_clipboard_item(state: State<AppState>, id: String, pinned: bool) -> Result<(), String> {
    let dir = state.dirs.clipboard_dir();
    let shared = shared_history()?;

    {
        let mut hist = shared.lock().unwrap();
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
        return Err("Ítem no encontrado".into());
    }

    // Favoritar captura: copiar PNG al dir de clipboard y persistir en history.json
    let virtual_item =
        find_item(&state, &id).ok_or_else(|| "Ítem no encontrado".to_string())?;
    let src_path = virtual_item
        .image_path
        .as_deref()
        .ok_or_else(|| "imagen sin ruta".to_string())?;
    let src = Path::new(src_path);
    if !src.is_file() {
        return Err("archivo de imagen no encontrado".into());
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

    let mut hist = shared.lock().unwrap();
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
pub fn delete_clipboard_item(state: State<AppState>, id: String) -> Result<(), String> {
    let dir = state.dirs.clipboard_dir();
    let shared = shared_history()?;
    let mut hist = shared.lock().unwrap();
    let Some(idx) = hist.items.iter().position(|i| i.id == id) else {
        return Err("Ítem no encontrado".into());
    };
    let removed = hist.items.remove(idx);
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
    Ok(())
}

/// Vacía el historial conservando los pines.
#[tauri::command]
pub fn clear_clipboard_history(state: State<AppState>) -> Result<(), String> {
    let dir = state.dirs.clipboard_dir();
    let shared = shared_history()?;
    let mut hist = shared.lock().unwrap();
    let (pinned, rest): (Vec<_>, Vec<_>) = hist.items.drain(..).partition(|i| i.pinned);
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
    Ok(())
}

/// Pone texto en el portapapeles y lo pega en la ventana en primer plano.
///
/// En consolas / Electron (Claude Code, Windows Terminal, etc.) usa
/// Ctrl+Shift+V; en el resto, Ctrl+V. Compartido con dictado.
pub(crate) fn paste_text(text: &str) -> Result<(), String> {
    {
        let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
        clipboard
            .set_text(text.to_string())
            .map_err(|e| e.to_string())?;
    }
    thread::sleep(Duration::from_millis(80));
    paste_text_hotkey()
}

/// Elige el atajo de pegado según la clase de la ventana en foco.
fn paste_text_hotkey() -> Result<(), String> {
    #[cfg(windows)]
    {
        // Terminales / Electron suelen ignorar Ctrl+V (clipboard history) y
        // requieren Ctrl+Shift+V. Notepad/Word siguen con Ctrl+V.
        if foreground_prefers_ctrl_shift_v() {
            paste_ctrl_shift_v()
        } else {
            paste_ctrl_v()
        }
    }
    #[cfg(not(windows))]
    {
        Err("Pega con Ctrl/Cmd+V; el contenido ya está en el portapapeles.".into())
    }
}

/// Consolas / terminales (y Claude Code / Terax). No usar `Chrome_WidgetWin`
/// a ciegas: VS Code también es Electron y ahí Ctrl+Shift+V abre preview.
#[cfg(windows)]
fn foreground_prefers_ctrl_shift_v() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        hwnd_prefers_ctrl_shift_v(hwnd)
    }
}

#[cfg(windows)]
fn hwnd_prefers_ctrl_shift_v(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    unsafe {
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let class = String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase();
            if class.contains("console") || class.contains("cascadia") {
                return true;
            }
        }

        let Some(exe) = process_exe_name(hwnd) else {
            return false;
        };
        exe_prefers_ctrl_shift_v(&exe)
    }
}

/// Exes de consola / terminales embebidos (Tauri, Electron, nativos).
/// Evitar editores (code, cursor, devenv): ahí Ctrl+Shift+V no es pegar.
#[cfg(windows)]
fn exe_prefers_ctrl_shift_v(exe: &str) -> bool {
    matches!(
        exe,
        "claude.exe"
            | "windowsterminal.exe"
            | "wt.exe"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "conhost.exe"
            | "alacritty.exe"
            | "wezterm-gui.exe"
            | "wezterm.exe"
            | "warp.exe"
            | "hyper.exe"
            | "tabby.exe"
            | "kitty.exe"
            | "fluent-terminal.exe"
            | "conemu64.exe"
            | "conemu.exe"
            | "terax.exe"
            | "terminus.exe"
            | "rio.exe"
            | "ghostty.exe"
            | "electerm.exe"
            | "mintty.exe"
            | "putty.exe"
            | "mobaxterm.exe"
            | "xshell.exe"
            | "securecrt.exe"
            | "cmder.exe"
            | "firecmd.exe"
            | "console.exe"
            | "console2.exe"
            | "qterminal.exe"
            | "tilix.exe"
            | "gnome-terminal.exe"
            | "iterm2.exe"
            | "wave.exe"
    ) || (
        // Heurística: *terminal*.exe / *term*.exe, sin editores.
        (exe.contains("terminal") || exe.ends_with("term.exe") || exe.contains("-term."))
            && !exe.contains("code")
            && !exe.contains("cursor")
            && !exe.contains("devenv")
            && !exe.contains("notepad")
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
        Err("Pega con Ctrl/Cmd+V; el contenido ya está en el portapapeles.".into())
    }
}

#[cfg(windows)]
fn paste_ctrl_shift_v() -> Result<(), String> {
    send_paste_chord(true)
}

#[cfg(windows)]
fn send_paste_chord(with_shift: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_SHIFT, VK_V,
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
        let mut inputs = if with_shift {
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
        };
        let sent = SendInput(
            inputs.len() as u32,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if sent == 0 {
            return Err(
                "No se pudo pegar. El contenido quedó en el portapapeles (Ctrl+V / Ctrl+Shift+V)."
                    .into(),
            );
        }
    }
    Ok(())
}

/// Atajo del clipboard: el frontend hace toggle (cerrar si ya está abierto).
///
/// No reposiciona aquí: si el panel ya está expandido, centrar con ese alto
/// deja la ventana mal anclada. El frontend, al abrir, pide
/// [`prepare_clipboard_pill`] (compacta + cursor) y luego expande.
pub fn summon_clipboard_panel(app: &AppHandle) {
    // Guardar el foco ANTES de que la pill lo robe (solo importa al abrir).
    save_foreground_hwnd();
    let _ = app.emit("pill-clipboard-toggle", ());
}

/// Compacta la pill y la anima hasta el cursor (antes de expandir el historial).
///
/// Guarda la posición actual una sola vez por sesión de clipboard para
/// restaurarla al cerrar o pegar. Bloquea hasta terminar el fly-to para que
/// el frontend expanda el panel recién al llegar.
#[tauri::command]
pub fn prepare_clipboard_pill(app: AppHandle) -> Result<(), String> {
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = pill.set_size(tauri::LogicalSize::new(112.0, 48.0));
        stash_pre_clipboard_position(&app, &pill);
    }
    crate::state::animate_pill_to_cursor(&app)
        .ok_or_else(|| "No se pudo colocar la pill en el cursor".to_string())?;
    Ok(())
}

/// Restaura la pill a la posición previa al summon del clipboard (si existe).
///
/// Compacta y anima (fly-to) de vuelta al home. Devuelve `true` si hubo
/// posición guardada y se aplicó.
#[tauri::command]
pub fn restore_pill_position(app: AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    let Some((x, y)) = state.pre_clipboard_position.lock().unwrap().take() else {
        return Ok(false);
    };

    let target_x = x.round() as i32;
    let target_y = y.round() as i32;

    if let Some(pill) = app.get_webview_window("pill") {
        // Compacto primero: la home se guardó con tamaño idle.
        let _ = pill.set_size(tauri::LogicalSize::new(112.0, 48.0));
        crate::state::animate_pill_to(&app, target_x, target_y);
    }

    {
        let mut cfg = state.config.lock().unwrap();
        cfg.pill_position = Some((x, y));
        let snapshot = cfg.clone();
        drop(cfg);
        let _ = snapshot.save(&state.dirs.config_path());
    }
    Ok(true)
}

/// Guarda la posición home solo la primera vez de la sesión (reabrir en el
/// cursor no debe pisar el home original).
fn stash_pre_clipboard_position(app: &AppHandle, pill: &tauri::WebviewWindow) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut pre = state.pre_clipboard_position.lock().unwrap();
    if pre.is_some() {
        return;
    }
    *pre = pill
        .outer_position()
        .ok()
        .map(|p| (p.x as f64, p.y as f64))
        .or_else(|| state.config.lock().unwrap().pill_position);
}

fn save_foreground_hwnd() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        unsafe {
            PREV_FOREGROUND.store(GetForegroundWindow() as isize, Ordering::SeqCst);
        }
    }
}

fn restore_foreground_hwnd() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

        let raw = PREV_FOREGROUND.load(Ordering::SeqCst);
        if raw == 0 {
            return;
        }
        unsafe {
            let hwnd = raw as HWND;
            if IsWindow(hwnd) == 0 {
                return;
            }
            force_foreground_for_paste(hwnd);
        }
    }
}

/// Devuelve el foco a la app destino de forma fiable y, en terminales
/// WebView2/Electron (Terax, Hyper, Tabby…), al hijo Chromium que recibe teclas.
#[cfg(windows)]
fn force_foreground_for_paste(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::ptr;
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, EnumChildWindows, GetClassNameW,
        GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
        ASFW_ANY,
    };

    /// DFS: el último `Chrome_*` visible suele ser el widget que recibe input.
    unsafe extern "system" fn find_chrome_child(child: HWND, lparam: LPARAM) -> BOOL {
        let best = &mut *(lparam as *mut HWND);
        if IsWindowVisible(child) != 0 {
            let mut buf = [0u16; 256];
            let len = GetClassNameW(child, buf.as_mut_ptr(), buf.len() as i32);
            if len > 0 {
                let class = String::from_utf16_lossy(&buf[..len as usize]);
                if class.starts_with("Chrome_WidgetWin")
                    || class.starts_with("Chrome_RenderWidget")
                {
                    *best = child;
                }
            }
        }
        EnumChildWindows(child, Some(find_chrome_child), lparam);
        1
    }

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

        let input_hwnd = if hwnd_prefers_ctrl_shift_v(hwnd) {
            let mut chrome: HWND = ptr::null_mut();
            EnumChildWindows(hwnd, Some(find_chrome_child), &mut chrome as *mut _ as LPARAM);
            if !chrome.is_null() {
                chrome
            } else {
                hwnd
            }
        } else {
            hwnd
        };
        let _ = SetFocus(input_hwnd);

        if attached_tgt {
            let _ = AttachThreadInput(cur_tid, target_tid, 0);
        }
        if attached_fg {
            let _ = AttachThreadInput(cur_tid, fg_tid, 0);
        }
    }
}
