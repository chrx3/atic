//! Bindings globales de botones laterales del mouse (Windows).
//!
//! `tauri-plugin-global-shortcut` usa `RegisterHotKey`, que no admite mouse.
//! Este módulo usa **Raw Input** (`WM_INPUT` + `RIDEV_INPUTSINK`) en una ventana
//! message-only oculta: los eventos son copias fuera del camino crítico del
//! input, así que **nunca pueden congelar el ratón del sistema**, aunque Atic
//! se cuelgue por completo.
//!
//! Limitación aceptada: Raw Input no puede consumir el evento. En algunas apps
//! (p. ej. navegadores) el botón lateral seguirá disparando su acción por
//! defecto (atrás/adelante) además de la de Atic. Es el tradeoff seguro frente
//! a `WH_MOUSE_LL`, que bloqueaba todo el input del SO.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::{clipboard_history, dictation, snippets, state};

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
    Snippets,
    Screenshot,
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    Down,
    Up,
}

/// 0 = sin binding. Resto = [`MouseAction`] + 1 (lock-free para el wndproc).
const ACT_NONE: u8 = 0;

#[derive(Debug, Clone, Copy)]
struct HookEvent {
    action: MouseAction,
    edge: Edge,
}

static BIND_X1: AtomicU8 = AtomicU8::new(ACT_NONE);
static BIND_X2: AtomicU8 = AtomicU8::new(ACT_NONE);
static RAW_STARTED: AtomicBool = AtomicBool::new(false);
/// UP de dictado perdido por canal lleno (evita PTT pegado).
static PENDING_DICTATION_UP: AtomicBool = AtomicBool::new(false);
static EVENT_TX: OnceLock<SyncSender<HookEvent>> = OnceLock::new();
static APP_HANDLE: OnceLock<Mutex<Option<AppHandle>>> = OnceLock::new();

fn action_to_u8(action: MouseAction) -> u8 {
    match action {
        MouseAction::Recording => 1,
        MouseAction::Dictation => 2,
        MouseAction::SummonPill => 3,
        MouseAction::Clipboard => 4,
        MouseAction::Snippets => 6,
        MouseAction::Screenshot => 5,
    }
}

fn u8_to_action(v: u8) -> Option<MouseAction> {
    match v {
        1 => Some(MouseAction::Recording),
        2 => Some(MouseAction::Dictation),
        3 => Some(MouseAction::SummonPill),
        4 => Some(MouseAction::Clipboard),
        5 => Some(MouseAction::Screenshot),
        6 => Some(MouseAction::Snippets),
        _ => None,
    }
}

fn app_slot() -> &'static Mutex<Option<AppHandle>> {
    APP_HANDLE.get_or_init(|| Mutex::new(None))
}

/// Interpreta `"MouseX1"` / `"MouseX2"`. Cualquier otro string → `None`.
pub fn parse_side_button(s: &str) -> Option<SideButton> {
    match s.trim() {
        "MouseX1" => Some(SideButton::X1),
        "MouseX2" => Some(SideButton::X2),
        _ => None,
    }
}

/// Arranca el hilo Raw Input (Windows). En otras plataformas es no-op.
pub fn init(app: &AppHandle) {
    if let Ok(mut slot) = app_slot().lock() {
        *slot = Some(app.clone());
    }

    #[cfg(windows)]
    ensure_rawinput_and_worker();

    #[cfg(not(windows))]
    tracing::info!("bindings de mouse lateral solo están disponibles en Windows");
}

/// Sustituye los bindings laterales. Vacío = no dispara acciones.
pub fn set_bindings(app: &AppHandle, bindings: Vec<(SideButton, MouseAction)>) {
    #[cfg(not(windows))]
    {
        let _ = app;
        if !bindings.is_empty() {
            tracing::warn!("bindings de mouse lateral solo están disponibles en Windows");
        }
        return;
    }

    #[cfg(windows)]
    {
        let count = bindings.len();

        let mut x1 = ACT_NONE;
        let mut x2 = ACT_NONE;
        for (btn, action) in bindings {
            let code = action_to_u8(action);
            match btn {
                SideButton::X1 => x1 = code,
                SideButton::X2 => x2 = code,
            }
        }
        BIND_X1.store(x1, Ordering::Release);
        BIND_X2.store(x2, Ordering::Release);

        if let Ok(mut slot) = app_slot().lock() {
            *slot = Some(app.clone());
        }

        ensure_rawinput_and_worker();
        tracing::info!(count, x1, x2, "bindings de mouse lateral actualizados");
    }
}

#[cfg(windows)]
fn ensure_rawinput_and_worker() {
    if RAW_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let (tx, rx) = mpsc::sync_channel::<HookEvent>(256);
    if EVENT_TX.set(tx).is_err() {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("EVENT_TX de mouse ya inicializado");
        return;
    }

    if let Err(err) = thread::Builder::new()
        .name("atic-mouse-worker".into())
        .spawn(move || {
            loop {
                let ev = match rx.recv_timeout(Duration::from_millis(40)) {
                    Ok(ev) => Some(ev),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                let app = app_slot().lock().ok().and_then(|g| g.clone());
                let Some(app) = app else {
                    continue;
                };
                if let Some(ev) = ev {
                    dispatch(&app, ev.action, ev.edge);
                }
                // Recuperar UP de dictado si el wndproc lo marcó con canal lleno.
                if PENDING_DICTATION_UP.swap(false, Ordering::AcqRel) {
                    dispatch(&app, MouseAction::Dictation, Edge::Up);
                }
            }
        })
    {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!(%err, "no se pudo arrancar el worker de mouse");
        return;
    }

    let result = thread::Builder::new()
        .name("atic-mouse-rawinput".into())
        .spawn(rawinput_thread_main);
    if let Err(err) = result {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!(%err, "no se pudo arrancar el hilo Raw Input de mouse");
    }
}

/// Encola evento desde el wndproc. Si el canal está lleno, no pierde UP de dictado.
#[cfg(windows)]
fn enqueue_hook_event(ev: HookEvent) {
    let Some(tx) = EVENT_TX.get() else {
        return;
    };
    match tx.try_send(ev) {
        Ok(()) => {}
        Err(TrySendError::Full(ev)) => {
            if matches!((ev.action, ev.edge), (MouseAction::Dictation, Edge::Up)) {
                PENDING_DICTATION_UP.store(true, Ordering::Release);
            } else {
                tracing::trace!(
                    action = ?ev.action,
                    edge = ?ev.edge,
                    "evento mouse descartado (canal lleno)"
                );
            }
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

#[cfg(windows)]
fn rawinput_thread_main() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage,
        CS_HREDRAW, CS_VREDRAW, HWND_MESSAGE, MSG, WNDCLASSW,
    };

    const CLASS_NAME: &[u16] = &[
        b'A' as u16,
        b't' as u16,
        b'i' as u16,
        b'c' as u16,
        b'R' as u16,
        b'a' as u16,
        b'w' as u16,
        b'I' as u16,
        b'n' as u16,
        b'p' as u16,
        b'u' as u16,
        b't' as u16,
        0,
    ];

    // SAFETY: hilo dedicado con message pump; GetModuleHandleW(null) = módulo del proceso.
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if hinstance.is_null() {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("GetModuleHandleW falló (Raw Input mouse)");
        return;
    }

    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(rawinput_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: CLASS_NAME.as_ptr(),
    };

    // SAFETY: registro de clase local al proceso.
    let atom = unsafe { RegisterClassW(&wc) };
    if atom == 0 {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("RegisterClassW(AticRawInput) falló");
        return;
    }

    // SAFETY: ventana message-only (HWND_MESSAGE); no visible, solo recibe WM_INPUT.
    let hwnd: HWND = unsafe {
        CreateWindowExW(
            0,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("CreateWindowExW(HWND_MESSAGE) falló (Raw Input mouse)");
        return;
    }

    // Generic Desktop Controls / Mouse, sink even when unfocused.
    let devices = [RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    }];

    // SAFETY: hwnd válido en este hilo; RIDEV_INPUTSINK entrega copias pasivas.
    let registered = unsafe {
        RegisterRawInputDevices(
            devices.as_ptr(),
            devices.len() as u32,
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    };
    if registered == 0 {
        RAW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("RegisterRawInputDevices falló (mouse lateral)");
        return;
    }

    tracing::info!("Raw Input registrado (botones laterales MouseX1/MouseX2)");

    // SAFETY: message pump estándar; vive hasta que el proceso termina.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Wndproc: solo atomics + try_send en WM_INPUT. Sin Mutex, sin spawn, sin I/O.
#[cfg(windows)]
unsafe extern "system" fn rawinput_wnd_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, HRAWINPUT, RAWINPUT, RID_INPUT, RIM_TYPEMOUSE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP, RI_MOUSE_BUTTON_5_DOWN,
        RI_MOUSE_BUTTON_5_UP, WM_INPUT,
    };

    if msg == WM_INPUT {
        let hraw = lparam as HRAWINPUT;
        let header_size =
            std::mem::size_of::<windows_sys::Win32::UI::Input::RAWINPUTHEADER>() as u32;

        // Tamaño necesario.
        let mut size: u32 = 0;
        let probe = unsafe {
            GetRawInputData(
                hraw,
                RID_INPUT,
                std::ptr::null_mut(),
                &mut size,
                header_size,
            )
        };
        if probe == u32::MAX || size == 0 {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }

        // Buffer alineado para RAWINPUT (puede ser más grande que sizeof por HID).
        let mut buf = vec![0u8; size as usize];
        let got = unsafe {
            GetRawInputData(
                hraw,
                RID_INPUT,
                buf.as_mut_ptr().cast(),
                &mut size,
                header_size,
            )
        };
        if got == u32::MAX || (got as usize) < std::mem::size_of::<RAWINPUT>() {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }

        // SAFETY: GetRawInputData escribió un RAWINPUT válido en buf.
        let raw = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
        if raw.header.dwType == RIM_TYPEMOUSE {
            // SAFETY: unión RAWMOUSE_0 → Anonymous.usButtonFlags.
            let flags = unsafe { raw.data.mouse.Anonymous.Anonymous.usButtonFlags } as u32;

            let edge_btn: Option<(SideButton, Edge)> = if flags & RI_MOUSE_BUTTON_4_DOWN != 0 {
                Some((SideButton::X1, Edge::Down))
            } else if flags & RI_MOUSE_BUTTON_4_UP != 0 {
                Some((SideButton::X1, Edge::Up))
            } else if flags & RI_MOUSE_BUTTON_5_DOWN != 0 {
                Some((SideButton::X2, Edge::Down))
            } else if flags & RI_MOUSE_BUTTON_5_UP != 0 {
                Some((SideButton::X2, Edge::Up))
            } else {
                None
            };

            if let Some((btn, edge)) = edge_btn {
                let bound = match btn {
                    SideButton::X1 => BIND_X1.load(Ordering::Acquire),
                    SideButton::X2 => BIND_X2.load(Ordering::Acquire),
                };
                if let Some(action) = u8_to_action(bound) {
                    enqueue_hook_event(HookEvent { action, edge });
                }
            }
        }

        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
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
        (MouseAction::Snippets, Edge::Down) => snippets::summon_snippets_panel(app),
        (MouseAction::Screenshot, Edge::Down) => {
            if let Err(error) = crate::capture_session::trigger(app) {
                tracing::warn!(%error, "no se pudo abrir el overlay de captura (mouse)");
            }
        }
        (_, Edge::Up) => {}
    }
}
