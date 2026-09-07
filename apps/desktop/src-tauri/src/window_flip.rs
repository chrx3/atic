//! Prototipo: tapa la ventana activa, la “da vuelta” y muestra notas.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(windows)]
use tauri::{PhysicalPosition, PhysicalSize};

use crate::state::AppState;
use atic_core::MutexExt;

pub const LABEL: &str = "window-flip";

/// Atajo fijo del prototipo. No pasa por Ajustes todavía.
pub const SHORTCUT: &str = "CmdOrCtrl+Shift+B";

const NOTES_FILE: &str = "window_flip_notes.json";

static OPEN: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0);
static SESSION: Mutex<Option<FlipSession>> = Mutex::new(None);
/// HWND cloakeado. Si Atic se cierra mal hay que destaparlo igual.
static CLOAKED: AtomicIsize = AtomicIsize::new(0);

#[derive(Clone)]
struct FlipSession {
    key: String,
    title: String,
    exe: String,
    preview_path: PathBuf,
    note: String,
    target_hwnd: isize,
    card_left: f64,
    card_top: f64,
    card_width: f64,
    card_height: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowFlipView {
    pub key: String,
    pub title: String,
    pub exe: String,
    pub preview_path: String,
    pub note: String,
    pub card_left: f64,
    pub card_top: f64,
    pub card_width: f64,
    pub card_height: f64,
}

impl From<&FlipSession> for WindowFlipView {
    fn from(s: &FlipSession) -> Self {
        Self {
            key: s.key.clone(),
            title: s.title.clone(),
            exe: s.exe.clone(),
            preview_path: s.preview_path.to_string_lossy().into_owned(),
            note: s.note.clone(),
            card_left: s.card_left,
            card_top: s.card_top,
            card_width: s.card_width,
            card_height: s.card_height,
        }
    }
}

/// Abre o cierra la tapa sobre la ventana del frente.
pub fn toggle(app: &AppHandle) {
    if OPEN.load(Ordering::SeqCst) {
        request_close(app);
        return;
    }
    if let Err(err) = open(app) {
        tracing::warn!(target: "window_flip", %err, "no se pudo voltear la ventana");
    }
}

fn request_close(app: &AppHandle) {
    let gen = GEN.load(Ordering::SeqCst);
    let _ = app.emit("window-flip-request-close", ());
    let handle = app.clone();
    thread::spawn(move || {
        // Recaptura + giro. Si el front se queda mudo, esto saca la tapa
        // negra. No puede ser corto: si gana al giro, se ve un corte.
        thread::sleep(Duration::from_millis(2500));
        if OPEN.load(Ordering::SeqCst) && GEN.load(Ordering::SeqCst) == gen {
            hide(&handle);
        }
    });
}

/// El front ya animó el giro de vuelta: ocultar la tapa.
#[tauri::command]
pub fn window_flip_close(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
pub fn window_flip_state() -> Option<WindowFlipView> {
    SESSION.lock_or_recover().as_ref().map(WindowFlipView::from)
}

/// Foto fresca del HWND tapado, con PrintWindow (no ve nuestra tapa).
///
/// Si sale negro o falla, se deja la foto anterior: mejor un frame viejo
/// que un frente negro a mitad del giro.
#[tauri::command]
pub fn window_flip_refresh_preview(app: AppHandle) -> Result<WindowFlipView, String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err(crate::ui_lang::capture_windows_only());
    }
    #[cfg(windows)]
    {
        refresh_preview_windows(&app)
    }
}

#[tauri::command]
pub fn window_flip_save_note(state: State<AppState>, body: String) -> Result<(), String> {
    let mut guard = SESSION.lock_or_recover();
    let Some(session) = guard.as_mut() else {
        return Ok(());
    };
    session.note = body;
    save_note(&state.dirs.data_dir(), &session.key, &session.note);
    Ok(())
}

fn hide(app: &AppHandle) {
    OPEN.store(false, Ordering::SeqCst);
    // Primero la ventana viva, después la tapa: un frame sin las dos se ve
    // como un parpadeo negro.
    uncloak_current();
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    if let Some(session) = SESSION.lock_or_recover().take() {
        if !session.preview_path.as_os_str().is_empty() {
            olvidar_preview(&session.preview_path);
        }
    }
}

fn open(app: &AppHandle) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err(crate::ui_lang::capture_windows_only());
    }
    #[cfg(windows)]
    {
        open_windows(app)
    }
}

#[cfg(windows)]
fn open_windows(app: &AppHandle) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return Err("no hay ventana al frente".into());
    }
    if is_our_label(app, hwnd as isize, LABEL) || is_our_label(app, hwnd as isize, crate::overlay::LABEL)
    {
        hide(app);
        return Ok(());
    }
    if unsafe { IsIconic(hwnd) } != 0 {
        return Err("la ventana está minimizada".into());
    }

    let bounds = atic_capture::windows::window_bounds(hwnd as isize)
        .ok_or_else(|| "no pude leer el tamaño de la ventana".to_string())?;
    if bounds.width < 80 || bounds.height < 80 {
        return Err("la ventana es demasiado chica".into());
    }

    let title = unsafe { window_title(hwnd) };
    let exe = process_exe_name(hwnd).unwrap_or_else(|| "app".into());
    let key = note_key(&exe, &title);

    let dirs = app
        .try_state::<AppState>()
        .ok_or_else(|| "sin estado".to_string())?
        .dirs
        .clone();
    let gen = GEN.load(Ordering::SeqCst) + 1;
    let preview_path = dirs.overlay_frames_dir().join(format!("window-flip-{gen}.png"));
    let hay_foto = capture_preview(hwnd as isize, &preview_path, true);

    let overlay = overlay_for_flip(bounds);
    let (card_left, card_top, card_width, card_height) = card_layout(bounds, overlay);

    let note = load_note(&dirs.data_dir(), &key);
    let session = FlipSession {
        key,
        title,
        exe,
        preview_path: if hay_foto { preview_path } else { PathBuf::new() },
        note,
        target_hwnd: hwnd as isize,
        card_left,
        card_top,
        card_width,
        card_height,
    };

    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "falta la ventana window-flip".to_string())?;

    disable_dwm_transitions(&window);
    let _ = window.set_size(PhysicalSize::new(overlay.width, overlay.height));
    let _ = window.set_position(PhysicalPosition::new(overlay.x, overlay.y));

    *SESSION.lock_or_recover() = Some(session.clone());
    GEN.fetch_add(1, Ordering::SeqCst);
    OPEN.store(true, Ordering::SeqCst);

    cloak_target(hwnd as isize);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("window-flip-open", WindowFlipView::from(&session));
    Ok(())
}

#[cfg(windows)]
fn is_our_label(app: &AppHandle, hwnd: isize, label: &str) -> bool {
    app.get_webview_window(label)
        .and_then(|w| w.hwnd().ok())
        .is_some_and(|h| h.0 as isize == hwnd)
}

#[cfg(windows)]
fn refresh_preview_windows(app: &AppHandle) -> Result<WindowFlipView, String> {
    let dirs = app
        .try_state::<AppState>()
        .ok_or_else(|| "sin estado".to_string())?
        .dirs
        .clone();
    let mut guard = SESSION.lock_or_recover();
    let session = guard.as_mut().ok_or_else(|| "no hay tapa abierta".to_string())?;
    if session.target_hwnd == 0 {
        return Ok(WindowFlipView::from(&*session));
    }
    let gen = GEN.load(Ordering::SeqCst);
    let path = dirs.overlay_frames_dir().join(format!("window-flip-{gen}-back.png"));
    if !capture_preview(session.target_hwnd, &path, false) {
        return Ok(WindowFlipView::from(&*session));
    }
    if !session.preview_path.as_os_str().is_empty() && session.preview_path != path {
        olvidar_preview(&session.preview_path);
    }
    session.preview_path = path;
    Ok(WindowFlipView::from(&*session))
}

#[cfg(windows)]
/// Deja la foto de la ventana en `path`. `false` = no hay foto usable.
///
/// Siempre recorta al marco visible. `permitir_pantalla` solo entra si
/// PrintWindow sale negro: un BitBlt del escritorio, y solo al abrir, cuando
/// la tapa todavía no cubre nada.
fn capture_preview(hwnd: isize, path: &Path, permitir_pantalla: bool) -> bool {
    let frame = match atic_capture::engine::capture_window_visual(hwnd) {
        Ok(frame) => Some(frame),
        Err(err) => {
            tracing::warn!(target: "window_flip", %err, "no se pudo recortar la foto al marco visible");
            None
        }
    };
    let Some(frame) = frame else {
        if permitir_pantalla {
            olvidar_preview(path);
        }
        return false;
    };
    let png = match frame.to_png() {
        Ok(png) => png,
        Err(err) => {
            tracing::warn!(target: "window_flip", %err, "no se pudo codificar la miniatura");
            return false;
        }
    };
    if let Err(err) = std::fs::write(path, png) {
        tracing::warn!(target: "window_flip", %err, "no se pudo guardar la miniatura");
        return false;
    }
    true
}

/// Overlay más grande que la ventana: al girar en 3D la tarjeta se estrecha y
/// el aire alrededor tiene que ser transparente, no un rectángulo opaco.
fn overlay_for_flip(visual: atic_capture::Rect) -> atic_capture::Rect {
    let pad_x = (visual.width / 3).max(96);
    let pad_y = (visual.height / 10).max(48);
    atic_capture::Rect::new(
        visual.x.saturating_sub(pad_x as i32),
        visual.y.saturating_sub(pad_y as i32),
        visual.width + pad_x * 2,
        visual.height + pad_y * 2,
    )
}

fn card_layout(visual: atic_capture::Rect, overlay: atic_capture::Rect) -> (f64, f64, f64, f64) {
    let w = overlay.width.max(1) as f64;
    let h = overlay.height.max(1) as f64;
    (
        (visual.x - overlay.x) as f64 / w,
        (visual.y - overlay.y) as f64 / h,
        visual.width as f64 / w,
        visual.height as f64 / h,
    )
}

#[cfg(windows)]
fn cloak_target(hwnd: isize) {
    uncloak_current();
    if hwnd == 0 {
        return;
    }
    set_cloaked(hwnd, true);
    CLOAKED.store(hwnd, Ordering::SeqCst);
}

fn uncloak_current() {
    let hwnd = CLOAKED.swap(0, Ordering::SeqCst);
    if hwnd != 0 {
        #[cfg(windows)]
        set_cloaked(hwnd, false);
    }
}

/// Si Atic se cierra con una tapa abierta, la ventana de debajo no puede
/// quedar invisible.
pub(crate) fn uncloak_on_exit() {
    uncloak_current();
}

#[cfg(windows)]
fn set_cloaked(hwnd: isize, on: bool) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // DWMWA_CLOAK = 13. No está en todos los bindings de windows-sys 0.59.
    const DWMWA_CLOAK: u32 = 13;
    let value: i32 = if on { 1 } else { 0 };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd as _,
            DWMWA_CLOAK,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Borra la foto de la ventana.
///
/// Se llama al fallar la captura y al cerrar la tapa. Lo segundo es a
/// propósito: eso es una foto entera de la ventana que el usuario volteó —un
/// banco, un gestor de contraseñas, lo que fuera— y no tiene por qué quedarse
/// en el disco hasta el próximo volteo.
fn olvidar_preview(path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::debug!(target: "window_flip", %err, "no se pudo borrar la miniatura");
        }
    }
}

#[cfg(windows)]
fn disable_dwm_transitions(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let disable: i32 = 1;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd.0 as _,
            DWMWA_TRANSITIONS_FORCEDISABLED as u32,
            std::ptr::from_ref(&disable).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(windows)]
unsafe fn window_title(hwnd: windows_sys::Win32::Foundation::HWND) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if copied <= 0 {
        return String::new();
    }
    buffer.truncate(copied as usize);
    String::from_utf16_lossy(&buffer)
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

fn note_key(exe: &str, title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        exe.to_string()
    } else {
        format!("{exe}|{title}")
    }
}

fn notes_path(data_dir: &Path) -> PathBuf {
    data_dir.join(NOTES_FILE)
}

fn load_all(data_dir: &Path) -> HashMap<String, String> {
    let Ok(raw) = std::fs::read_to_string(notes_path(data_dir)) else {
        return HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn load_note(data_dir: &Path, key: &str) -> String {
    load_all(data_dir).get(key).cloned().unwrap_or_default()
}

fn save_note(data_dir: &Path, key: &str, body: &str) {
    let mut all = load_all(data_dir);
    if body.is_empty() {
        all.remove(key);
    } else {
        all.insert(key.to_string(), body.to_string());
    }
    if let Ok(raw) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(notes_path(data_dir), raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_clave_junta_exe_y_titulo() {
        assert_eq!(note_key("teams.exe", "Standup"), "teams.exe|Standup");
        assert_eq!(note_key("notepad.exe", "  "), "notepad.exe");
    }

    #[test]
    fn olvidar_una_foto_que_no_existe_no_es_un_error() {
        // Se llama al fallar la captura, y ahí lo normal es que no haya nada.
        olvidar_preview(&std::env::temp_dir().join("atic-flip-no-existe.png"));
    }

    #[test]
    fn la_foto_se_borra_de_verdad() {
        let p = std::env::temp_dir().join(format!("atic-flip-{}.png", std::process::id()));
        std::fs::write(&p, b"png").unwrap();
        olvidar_preview(&p);
        assert!(!p.exists(), "la foto de la ventana no puede quedarse en disco");
    }

    #[test]
    fn una_nota_vacia_borra_la_entrada_en_vez_de_guardar_vacio() {
        let dir = std::env::temp_dir().join(format!("atic-flip-notas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        save_note(&dir, "teams.exe|Standup", "acordarse del deploy");
        assert_eq!(load_note(&dir, "teams.exe|Standup"), "acordarse del deploy");

        // Borrar el texto tiene que dejar el archivo limpio: si no, el JSON
        // acumula una entrada vacía por cada ventana que alguien volteó.
        save_note(&dir, "teams.exe|Standup", "");
        assert_eq!(load_note(&dir, "teams.exe|Standup"), "");
        assert!(load_all(&dir).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn las_notas_de_dos_ventanas_no_se_pisan() {
        let dir = std::env::temp_dir().join(format!("atic-flip-dos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        save_note(&dir, note_key("teams.exe", "Standup").as_str(), "una");
        save_note(&dir, note_key("code.exe", "main.rs").as_str(), "otra");
        assert_eq!(load_note(&dir, "teams.exe|Standup"), "una");
        assert_eq!(load_note(&dir, "code.exe|main.rs"), "otra");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_aire_del_giro_deja_la_tarjeta_en_el_marco_visual() {
        let visual = atic_capture::Rect::new(200, 100, 400, 300);
        let overlay = overlay_for_flip(visual);
        assert!(overlay.width > visual.width);
        assert!(overlay.height > visual.height);
        let (left, top, width, height) = card_layout(visual, overlay);
        assert!((left + width + left - 1.0).abs() < 1e-9);
        assert!(top > 0.0 && top < 0.5);
        assert!((width - visual.width as f64 / overlay.width as f64).abs() < 1e-9);
        assert!((height - visual.height as f64 / overlay.height as f64).abs() < 1e-9);
    }
}
