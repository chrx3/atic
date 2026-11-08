//! Windows: se escucha con ganchos de bajo nivel (`WH_MOUSE_LL` y
//! `WH_KEYBOARD_LL`), que pueden tragarse cada evento, y se inyecta con
//! `SendInput` (las teclas por código de escaneo: no dependen de la
//! distribución). El proceso se declara consciente de los DPI de cada
//! monitor, para que las coordenadas sean píxeles de verdad.
//!
//! Lo que Windows no deja tocar: la pantalla de inicio de sesión, Ctrl+Alt+Supr
//! y las ventanas que corren como administrador (si esto no lo es).

use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{bail, Result};
use windows_sys::Win32::Foundation::{BOOL, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateCursor, DispatchMessageW, SetSystemCursor, SystemParametersInfoW, OCR_APPSTARTING, OCR_CROSS, OCR_HAND, OCR_HELP, OCR_IBEAM,
    OCR_NO, OCR_NORMAL, OCR_SIZEALL, OCR_SIZENESW, OCR_SIZENS, OCR_SIZENWSE, OCR_SIZEWE, OCR_UP, OCR_WAIT, SPI_SETCURSORS, GetCursorPos, GetMessageW, GetSystemMetrics, SetCursorPos, SetWindowsHookExW, TranslateMessage, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::keys;
use crate::layout::{Rect, Screens};
use crate::proto::Button;
use crate::source::{Grab, Verdict};

/// Mientras el cursor está en el otro, aquí queda estacionado tantos píxeles
/// hacia dentro del borde: el gancho mide el movimiento desde ahí, y contra el
/// borde mismo Windows recortaría el empuje hacia fuera.
pub const PARK_INSET: f64 = 200.0;

pub fn prepare() {
    // SAFETY: sin precondiciones; falla si ya se fijó (da lo mismo).
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // Si una sesión anterior se cerró de golpe con el cursor escondido.
    show_cursor();
}

// ---------------------------------------------------------------- recibir

pub fn screens() -> Screens {
    unsafe extern "system" fn each(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` es el `Vec` de `screens`, vivo durante `EnumDisplayMonitors`.
        let out = unsafe { &mut *(data as *mut Vec<Rect>) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        // SAFETY: un monitor que acaba de dar el sistema y una estructura con su tamaño.
        if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
            let r = info.rcMonitor;
            out.push(Rect { x: r.left as f64, y: r.top as f64, w: (r.right - r.left) as f64, h: (r.bottom - r.top) as f64 });
        }
        1
    }
    let mut out: Vec<Rect> = Vec::new();
    // SAFETY: `out` vive durante la llamada, que es síncrona.
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut out as *mut Vec<Rect> as LPARAM) };
    Screens(out)
}

pub fn cursor() -> (f64, f64) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: `p` es un `POINT` válido.
    unsafe { GetCursorPos(&mut p) };
    (p.x as f64, p.y as f64)
}

/// El cursor a `(x, y)` del escritorio virtual. Absoluto: un movimiento
/// relativo pasaría otra vez por la aceleración del mouse.
pub fn move_to(x: f64, y: f64, _dx: f64, _dy: f64) {
    // SAFETY: `GetSystemMetrics` no tiene precondiciones.
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_YVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2) as f64,
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2) as f64,
        )
    };
    let nx = ((x - vx) / (vw - 1.0) * 65535.0).round().clamp(0.0, 65535.0) as i32;
    let ny = ((y - vy) / (vh - 1.0) * 65535.0).round().clamp(0.0, 65535.0) as i32;
    send(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0)]);
}

pub fn button(button: Button, down: bool) {
    let (flags, data) = match (button, down) {
        (Button::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (Button::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (Button::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (Button::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (Button::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (Button::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        // XBUTTON1 es atrás; XBUTTON2, adelante.
        (Button::Back, true) => (MOUSEEVENTF_XDOWN, 1),
        (Button::Back, false) => (MOUSEEVENTF_XUP, 1),
        (Button::Forward, true) => (MOUSEEVENTF_XDOWN, 2),
        (Button::Forward, false) => (MOUSEEVENTF_XUP, 2),
    };
    send(&[mouse(flags, 0, 0, data)]);
}

/// Un salto de rueda son 120 unidades; Windows acepta fracciones (rueda suave).
pub fn wheel(dx: f32, dy: f32) {
    let wy = (dy * 120.0).round() as i32;
    let wx = (dx * 120.0).round() as i32;
    let mut inputs = Vec::new();
    if wy != 0 {
        inputs.push(mouse(MOUSEEVENTF_WHEEL, 0, 0, wy));
    }
    if wx != 0 {
        inputs.push(mouse(MOUSEEVENTF_HWHEEL, 0, 0, wx));
    }
    send(&inputs);
}

pub fn key(hid: u16, down: bool, _repeat: bool) {
    let Some((scan, extended)) = keys::windows_from_hid(hid) else { return };
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    send(&[INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }]);
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    // SAFETY: `inputs` vive durante la llamada y su largo es el que se pasa.
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

// ---------------------------------------------------------------- mandar

static GRAB: OnceLock<Arc<Grab>> = OnceLock::new();
/// El último punto visto (para medir el movimiento) y dónde quedó estacionado el cursor.
static LAST: Mutex<(i32, i32)> = Mutex::new((0, 0));
static PARK: Mutex<(i32, i32)> = Mutex::new((0, 0));

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn start_capture(grab: Arc<Grab>) -> Result<()> {
    if GRAB.get().is_some() {
        bail!("ya se está escuchando el mouse y el teclado");
    }
    let (x, y) = cursor();
    *lock(&LAST) = (x as i32, y as i32);
    let (ready, started) = std::sync::mpsc::channel();
    // Los ganchos de bajo nivel se llaman en el hilo que los instaló, que tiene
    // que estar atendiendo mensajes.
    std::thread::Builder::new().name("atic-kvm-hooks".into()).spawn(move || {
        // SAFETY: los ganchos viven lo que dura el hilo (todo el programa).
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
            let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
            if m.is_null() || k.is_null() {
                let _ = ready.send(false);
                return;
            }
            let _ = GRAB.set(grab);
            let _ = ready.send(true);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    })?;
    if started.recv().unwrap_or(false) {
        Ok(())
    } else {
        bail!("Windows no dejó instalar los ganchos del mouse y el teclado")
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: con `code >= 0`, `lparam` apunta a un `MSLLHOOKSTRUCT` válido.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if info.flags & LLMHF_INJECTED == 0 {
            if let Some(grab) = GRAB.get() {
                if on_mouse(grab, wparam as u32, info) == Verdict::Swallow {
                    return 1;
                }
            }
        }
    }
    // SAFETY: se le pasa lo mismo que llegó.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn on_mouse(grab: &Grab, message: u32, info: &MSLLHOOKSTRUCT) -> Verdict {
    let high = (info.mouseData >> 16) as u16;
    match message {
        WM_MOUSEMOVE => {
            let (x, y) = (info.pt.x, info.pt.y);
            if grab.is_remote() {
                // El cursor no se mueve (el gancho se traga el evento): el movimiento es desde donde quedó.
                let (px, py) = *lock(&PARK);
                return grab.on_move(x as f64, y as f64, (x - px) as f64, (y - py) as f64, false);
            }
            let (lx, ly) = std::mem::replace(&mut *lock(&LAST), (x, y));
            grab.on_move(x as f64, y as f64, (x - lx) as f64, (y - ly) as f64, false)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => grab.on_button(Button::Left, message == WM_LBUTTONDOWN),
        WM_RBUTTONDOWN | WM_RBUTTONUP => grab.on_button(Button::Right, message == WM_RBUTTONDOWN),
        WM_MBUTTONDOWN | WM_MBUTTONUP => grab.on_button(Button::Middle, message == WM_MBUTTONDOWN),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let button = if high == 2 { Button::Forward } else { Button::Back };
            grab.on_button(button, message == WM_XBUTTONDOWN)
        }
        WM_MOUSEWHEEL => grab.on_wheel(0.0, high as i16 as f32 / 120.0),
        WM_MOUSEHWHEEL => grab.on_wheel(high as i16 as f32 / 120.0, 0.0),
        _ => Verdict::Pass,
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: con `code >= 0`, `lparam` apunta a un `KBDLLHOOKSTRUCT` válido.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if info.flags & LLKHF_INJECTED == 0 {
            if let (Some(grab), Some(hid)) = (GRAB.get(), keys::hid_from_windows(info.scanCode, info.flags & LLKHF_EXTENDED != 0, info.vkCode)) {
                let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                if grab.on_key(hid, down) == Verdict::Swallow {
                    return 1;
                }
            }
        }
    }
    // SAFETY: se le pasa lo mismo que llegó.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

pub fn hold_cursor(x: f64, y: f64) {
    let (x, y) = (x.round() as i32, y.round() as i32);
    *lock(&PARK) = (x, y);
    // SAFETY: sin precondiciones.
    unsafe { SetCursorPos(x, y) };
    hide_cursor();
}

pub fn release_cursor(x: f64, y: f64) {
    let (x, y) = (x.round() as i32, y.round() as i32);
    *lock(&LAST) = (x, y);
    // SAFETY: sin precondiciones.
    unsafe { SetCursorPos(x, y) };
    show_cursor();
}

/// Los cursores del sistema que se cambian por uno transparente.
const CURSORS: [u32; 14] = [
    OCR_NORMAL, OCR_IBEAM, OCR_WAIT, OCR_CROSS, OCR_UP, OCR_SIZENWSE, OCR_SIZENESW, OCR_SIZEWE, OCR_SIZENS, OCR_SIZEALL, OCR_NO, OCR_HAND,
    OCR_APPSTARTING, OCR_HELP,
];

/// Mientras el cursor está en el otro equipo, aquí no se ve: cada cursor del
/// sistema pasa a ser uno transparente. Es lo que hacen otros programas de
/// este tipo; una ventana propia no sirve, porque el gancho se traga el
/// movimiento y Windows nunca le pregunta qué cursor mostrar.
fn hide_cursor() {
    // Máscara AND en 1 y XOR en 0: el píxel deja ver lo de abajo.
    let and = [0xFFu8; 32 * 32 / 8];
    let xor = [0u8; 32 * 32 / 8];
    for id in CURSORS {
        // SAFETY: las máscaras viven durante la llamada; `SetSystemCursor` se queda con el cursor y lo destruye.
        unsafe {
            let blank = CreateCursor(GetModuleHandleW(std::ptr::null()), 0, 0, 32, 32, and.as_ptr().cast(), xor.as_ptr().cast());
            if !blank.is_null() {
                SetSystemCursor(blank, id);
            }
        }
    }
}

/// Vuelven los cursores del usuario (los del esquema que tenga elegido).
fn show_cursor() {
    // SAFETY: `SPI_SETCURSORS` no usa el puntero.
    unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), 0) };
}
