//! Las repeticiones de la tecla del atajo de dictado mientras se mantiene apretada.
//!
//! El dictado se vigila sondeando el teclado (`dictation::Shortcut`), así que su atajo no
//! se consume: Windows repite la tecla (la X de Alt+X) todo el rato que se mantiene. Con
//! Alt abajo esas repeticiones no escriben nada, pero si se suelta Alt un instante antes
//! que la X, la siguiente repetición llega como una «x» normal a la app con el foco y el
//! texto dictado se pega detrás de ella.
//!
//! `guard` engancha el teclado (`WH_KEYBOARD_LL`) solo mientras dura eso: descarta las
//! pulsaciones de esa tecla hasta que se suelta. El soltar pasa siempre (si no,
//! `GetAsyncKeyState` la creería apretada para siempre), y también lo que no sea esa tecla
//! o lo que inyecta otra app.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_REMOVE, QS_ALLINPUT, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_SYSKEYDOWN,
};

/// La tecla que se está cuidando (código virtual), o 0.
static GUARDED: AtomicU32 = AtomicU32::new(0);

/// Lo más que se queda enganchado aunque la tecla no se suelte nunca (se perdió el soltar).
const MAX_HOLD: Duration = Duration::from_secs(120);

/// Descarta las pulsaciones repetidas de `vk` hasta que se suelte. Si ya se cuidaba una,
/// cambia a esta.
pub fn guard(vk: u32) {
    if vk == 0 || GUARDED.swap(vk, Ordering::SeqCst) != 0 {
        return;
    }
    std::thread::Builder::new()
        .name("atic-key-guard".into())
        .spawn(move || unsafe {
            let module = GetModuleHandleW(None).ok().map(Into::into);
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), module, 0) {
                Ok(hook) => hook,
                Err(error) => {
                    tracing::warn!(%error, "dictado: no se pudo cuidar la tecla del atajo");
                    GUARDED.store(0, Ordering::SeqCst);
                    return;
                }
            };
            let started = Instant::now();
            let mut msg = MSG::default();
            // El hook corre en este hilo: hay que bombear mensajes mientras dure.
            loop {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let vk = GUARDED.load(Ordering::SeqCst);
                if vk == 0 || GetAsyncKeyState(vk as i32) >= 0 || started.elapsed() > MAX_HOLD {
                    break;
                }
                MsgWaitForMultipleObjects(None, false, 30, QS_ALLINPUT);
            }
            let _ = UnhookWindowsHookEx(hook);
            GUARDED.store(0, Ordering::SeqCst);
        })
        .map_err(|error| {
            GUARDED.store(0, Ordering::SeqCst);
            tracing::warn!(%error, "dictado: sin hilo para cuidar la tecla del atajo");
        })
        .ok();
}

unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        let injected = event.flags.0 & LLKHF_INJECTED.0 != 0;
        if down && !injected && event.vkCode == GUARDED.load(Ordering::SeqCst) {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
