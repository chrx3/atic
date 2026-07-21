//! Bindings globales de botones laterales del mouse (Windows).
//!
//! `tauri-plugin-global-shortcut` usa `RegisterHotKey`, que no admite mouse.
//! Este módulo añade un hook `WH_MOUSE_LL` solo para `MouseX1` / `MouseX2`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use tauri::{AppHandle, Manager};

use crate::{clipboard_history, dictation, state};

/// Botón lateral del mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideButton {
    /// Botón "atrás" (XBUTTON1).
    X1,
    /// Botón "adelante" (XBUTTON2).
    X2,
}

/// Acción disparada por un botón lateral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Recording,
    Dictation,
    SummonPill,
    Clipboard,
    Screenshot,
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    Down,
    Up,
}

struct Registry {
    app: AppHandle,
    map: HashMap<SideButton, MouseAction>,
}

static REGISTRY: OnceLock<Mutex<Option<Registry>>> = OnceLock::new();
static HOOK_STARTED: AtomicBool = AtomicBool::new(false);

fn registry() -> &'static Mutex<Option<Registry>> {
    REGISTRY.get_or_init(|| Mutex::new(None))
}

/// Interpreta `"MouseX1"` / `"MouseX2"`. Cualquier otro string → `None`.
pub fn parse_side_button(s: &str) -> Option<SideButton> {
    match s.trim() {
        "MouseX1" => Some(SideButton::X1),
        "MouseX2" => Some(SideButton::X2),
        _ => None,
    }
}

/// Sustituye el mapa de bindings laterales. Vacío = no consume clics.
///
/// En Windows arranca el hilo del hook la primera vez que hay bindings.
#[cfg(windows)]
pub fn set_bindings(app: &AppHandle, bindings: Vec<(SideButton, MouseAction)>) {
    let count = bindings.len();
    {
        let mut map = HashMap::with_capacity(count);
        for (btn, action) in bindings {
            map.insert(btn, action);
        }
        *registry().lock().unwrap() = Some(Registry {
            app: app.clone(),
            map,
        });
    }
    if count > 0 {
        ensure_hook_thread();
    }
    tracing::info!(count, "bindings de mouse lateral actualizados");
}

#[cfg(not(windows))]
pub fn set_bindings(_app: &AppHandle, bindings: Vec<(SideButton, MouseAction)>) {
    if !bindings.is_empty() {
        tracing::warn!("bindings de mouse lateral solo están disponibles en Windows");
    }
}

#[cfg(windows)]
fn ensure_hook_thread() {
    if HOOK_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let result = thread::Builder::new()
        .name("atic-mouse-hook".into())
        .spawn(hook_thread_main);
    if let Err(err) = result {
        HOOK_STARTED.store(false, Ordering::SeqCst);
        tracing::error!(%err, "no se pudo arrancar el hilo del hook de mouse");
    }
}

#[cfg(windows)]
fn hook_thread_main() {
    use windows_sys::Win32::Foundation::HINSTANCE;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        MSG, WH_MOUSE_LL,
    };

    // SAFETY: WH_MOUSE_LL con hMod nulo y threadId 0 instala un hook de proceso.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_proc),
            std::ptr::null_mut::<()>() as HINSTANCE,
            0,
        )
    };
    if hook.is_null() {
        HOOK_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("SetWindowsHookExW(WH_MOUSE_LL) falló");
        return;
    }
    tracing::info!("hook WH_MOUSE_LL instalado (botones laterales)");

    // SAFETY: message pump estándar; el hook vive hasta que el proceso termina.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

#[cfg(windows)]
unsafe extern "system" fn mouse_proc(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
    };

    if code >= 0 {
        let msg = wparam as u32;
        if msg == WM_XBUTTONDOWN || msg == WM_XBUTTONUP {
            // SAFETY: lparam apunta a MSLLHOOKSTRUCT válido durante el callback.
            let info = &*(lparam as *const MSLLHOOKSTRUCT);
            let xbtn = ((info.mouseData >> 16) & 0xffff) as u16;
            let button = if xbtn == XBUTTON1 as u16 {
                Some(SideButton::X1)
            } else if xbtn == XBUTTON2 as u16 {
                Some(SideButton::X2)
            } else {
                None
            };

            if let Some(btn) = button {
                if let Some((app, action)) = lookup(btn) {
                    let edge = if msg == WM_XBUTTONDOWN {
                        Edge::Down
                    } else {
                        Edge::Up
                    };
                    // Nunca trabajo pesado en el callback del hook.
                    thread::spawn(move || dispatch(&app, action, edge));
                    return 1;
                }
            }
        }
    }

    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn lookup(btn: SideButton) -> Option<(AppHandle, MouseAction)> {
    let guard = registry().lock().ok()?;
    let reg = guard.as_ref()?;
    let action = *reg.map.get(&btn)?;
    Some((reg.app.clone(), action))
}

fn dispatch(app: &AppHandle, action: MouseAction, edge: Edge) {
    match (action, edge) {
        (MouseAction::Dictation, edge) => {
            let mode = app
                .try_state::<state::AppState>()
                .map(|s| s.config.lock().unwrap().dictation_mode.clone())
                .unwrap_or_else(|| "push_to_talk".into());
            match (mode.as_str(), edge) {
                ("push_to_talk", Edge::Down) => dictation::dictation_key_down(app),
                ("push_to_talk", Edge::Up) => dictation::dictation_key_up(app),
                (_, Edge::Down) => dictation::toggle_dictation(app),
                _ => {}
            }
        }
        (MouseAction::Recording, Edge::Down) => state::toggle_recording(app),
        (MouseAction::SummonPill, Edge::Down) => state::summon_pill_to_cursor(app),
        (MouseAction::Clipboard, Edge::Down) => clipboard_history::summon_clipboard_panel(app),
        (MouseAction::Screenshot, Edge::Down) => {
            if let Err(error) = crate::capture_session::trigger(app) {
                tracing::warn!(%error, "no se pudo abrir el overlay de captura (mouse)");
            }
        }
        (_, Edge::Up) => {}
    }
}
