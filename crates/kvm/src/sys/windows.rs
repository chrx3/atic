//! Windows: se escucha con ganchos de bajo nivel (`WH_MOUSE_LL` y
//! `WH_KEYBOARD_LL`), que pueden tragarse cada evento, y se inyecta con
//! `SendInput` (las teclas por código de escaneo: no dependen de la
//! distribución). El proceso se declara consciente de los DPI de cada
//! monitor, para que las coordenadas sean píxeles de verdad.
//!
//! Lo que Windows no deja tocar: la pantalla de inicio de sesión, Ctrl+Alt+Supr
//! y las ventanas que corren como administrador (si esto no lo es).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use windows_sys::Win32::Foundation::{BOOL, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateCursor, DispatchMessageW, PostThreadMessageW, SetSystemCursor, SetTimer, SystemParametersInfoW, UnhookWindowsHookEx, HHOOK, WM_APP,
    WM_TIMER, OCR_APPSTARTING, OCR_CROSS, OCR_HAND, OCR_HELP, OCR_IBEAM,
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

/// A quién le llega lo que capturan los ganchos. Se puede cambiar: una app que
/// apaga y vuelve a prender el servicio reusa los ganchos ya instalados.
static GRAB: Mutex<Option<Arc<Grab>>> = Mutex::new(None);
/// El último punto visto (para medir el movimiento) y dónde quedó estacionado el cursor.
static LAST: Mutex<(i32, i32)> = Mutex::new((0, 0));
static PARK: Mutex<(i32, i32)> = Mutex::new((0, 0));
/// El hilo de los ganchos (0: todavía no hay), que también esconde y muestra el cursor.
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// Si el cursor debería estar escondido; lo aplica el hilo de los ganchos.
static WANT_HIDDEN: AtomicBool = AtomicBool::new(false);
/// Aviso al hilo de los ganchos: aplicar `WANT_HIDDEN`.
const WM_CURSOR: u32 = WM_APP + 1;
/// Cada cuánto el hilo de los ganchos revisa que Windows no se los haya quitado.
const WATCH_MS: u32 = 500;
/// Con el cursor en el otro, lo que se puede alejar de donde quedó estacionado
/// antes de dar los ganchos por perdidos (con ellos, no se mueve nada).
const LOST_HOOK_DRIFT: i32 = 40;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn grab() -> Option<Arc<Grab>> {
    lock(&GRAB).clone()
}

pub fn start_capture(grab: Arc<Grab>) -> Result<()> {
    {
        // Los ganchos ya están: lo que capturen va al `Grab` nuevo.
        let mut current = lock(&GRAB);
        if current.is_some() {
            *current = Some(grab);
            return Ok(());
        }
    }
    let (x, y) = cursor();
    *lock(&LAST) = (x as i32, y as i32);
    let (ready, started) = std::sync::mpsc::channel();
    // Los ganchos de bajo nivel se llaman en el hilo que los instaló, que tiene
    // que estar atendiendo mensajes.
    std::thread::Builder::new().name("atic-kvm-hooks".into()).spawn(move || {
        let Some(mut hooks) = install_hooks() else {
            let _ = ready.send(false);
            return;
        };
        *lock(&GRAB) = Some(grab);
        // SAFETY: sin precondiciones.
        HOOK_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        let _ = ready.send(true);
        let mut hidden = false;
        // SAFETY: un temporizador del hilo (sin ventana) y su cola de mensajes, que
        // este hilo atiende hasta el final del programa.
        unsafe {
            SetTimer(std::ptr::null_mut(), 0, WATCH_MS, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_CURSOR => apply_cursor(&mut hidden),
                    WM_TIMER if msg.hwnd.is_null() => watch(&mut hooks, &mut hidden),
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
    })?;
    if started.recv().unwrap_or(false) {
        drop_target::start();
        Ok(())
    } else {
        bail!("Windows no dejó instalar los ganchos del mouse y el teclado")
    }
}

/// Los dos ganchos, en el hilo que los va a atender.
fn install_hooks() -> Option<(HHOOK, HHOOK)> {
    // SAFETY: los procedimientos son `extern "system"` y viven todo el programa.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
        if m.is_null() || k.is_null() {
            uninstall_hooks((m, k));
            return None;
        }
        Some((m, k))
    }
}

fn uninstall_hooks((m, k): (HHOOK, HHOOK)) {
    // SAFETY: ganchos de este hilo; si Windows ya los quitó, la llamada solo falla.
    unsafe {
        if !m.is_null() {
            UnhookWindowsHookEx(m);
        }
        if !k.is_null() {
            UnhookWindowsHookEx(k);
        }
    }
}

/// Si Windows quitó los ganchos: con el cursor en el otro, el gancho se traga
/// todo movimiento y el cursor no se aleja de donde quedó estacionado.
fn hooks_lost(park: (i32, i32), now: (i32, i32)) -> bool {
    (now.0 - park.0).abs() > LOST_HOOK_DRIFT || (now.1 - park.1).abs() > LOST_HOOK_DRIFT
}

/// Revisión periódica en el hilo de los ganchos. Windows quita en silencio un
/// gancho de bajo nivel que tarda en responder (con el equipo cargado, por
/// ejemplo): el cursor quedaría escondido y Ctrl+Alt+Esc ya no llegaría. Se
/// reinstalan y el cursor vuelve.
fn watch(hooks: &mut (HHOOK, HHOOK), hidden: &mut bool) {
    let Some(grab) = grab() else { return };
    if grab.is_remote() {
        let (x, y) = cursor();
        if !hooks_lost(*lock(&PARK), (x as i32, y as i32)) {
            return;
        }
        tracing::warn!("Windows quitó los ganchos del mouse y el teclado: se reinstalan y el cursor vuelve");
        uninstall_hooks(*hooks);
        match install_hooks() {
            Some(h) => *hooks = h,
            None => {
                *hooks = (std::ptr::null_mut(), std::ptr::null_mut());
                tracing::error!("no se pudieron reinstalar los ganchos del mouse y el teclado");
            }
        }
        grab.recall();
    }
    // Pase lo que pase, aquí el cursor nunca queda escondido sin estar en el otro.
    if !grab.is_remote() {
        WANT_HIDDEN.store(false, Ordering::Release);
    }
    apply_cursor(hidden);
}

/// Esconde o muestra el cursor según `WANT_HIDDEN`. Va en el hilo de los
/// ganchos, pero fuera de ellos: cambiar los catorce cursores del sistema
/// tarda, y un gancho lento es un gancho que Windows quita.
fn apply_cursor(hidden: &mut bool) {
    let want = WANT_HIDDEN.load(Ordering::Acquire);
    if want != *hidden {
        *hidden = want;
        if want {
            hide_cursor();
        } else {
            show_cursor();
        }
    }
}

/// Pide al hilo de los ganchos esconder o mostrar el cursor.
fn request_cursor(hide: bool) {
    WANT_HIDDEN.store(hide, Ordering::Release);
    let thread = HOOK_THREAD.load(Ordering::Acquire);
    // SAFETY: un mensaje sin punteros a un hilo que atiende su cola.
    if thread == 0 || unsafe { PostThreadMessageW(thread, WM_CURSOR, 0, 0) } == 0 {
        // Sin el hilo (o con la cola llena), al menos que el cursor se vea.
        if !hide {
            show_cursor();
        }
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: con `code >= 0`, `lparam` apunta a un `MSLLHOOKSTRUCT` válido.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if info.flags & LLMHF_INJECTED == 0 {
            if let Some(grab) = grab() {
                if on_mouse(&grab, wparam as u32, info) == Verdict::Swallow {
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
            drop_target::on_move();
            grab.on_move(x as f64, y as f64, (x - lx) as f64, (y - ly) as f64, false)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            let down = message == WM_LBUTTONDOWN;
            let verdict = grab.on_button(Button::Left, down);
            if verdict == Verdict::Pass {
                drop_target::on_left_button(down);
            }
            verdict
        }
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
            if let (Some(grab), Some(hid)) = (grab(), keys::hid_from_windows(info.scanCode, info.flags & LLKHF_EXTENDED != 0, info.vkCode)) {
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
    request_cursor(true);
}

pub fn release_cursor(x: f64, y: f64) {
    let (x, y) = (x.round() as i32, y.round() as i32);
    *lock(&LAST) = (x, y);
    // SAFETY: sin precondiciones.
    unsafe { SetCursorPos(x, y) };
    request_cursor(false);
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

/// Los archivos que se están arrastrando ahora. En Windows no hay un
/// portapapeles de arrastre que se pueda leer desde fuera: los recibe una
/// ventana invisible en el borde hacia el otro ([`drop_target`]). Puede
/// esperar un poco: el gancho ve el cursor llegar al borde justo antes de que
/// el arrastre entre en esa ventana.
pub fn dragged_files() -> Vec<std::path::PathBuf> {
    drop_target::files()
}

/// Una ventana invisible (casi: transparente al 99 %) y siempre encima, de
/// dos píxeles, en cada borde hacia el otro equipo, registrada como
/// `IDropTarget`. Solo se muestra mientras se arrastra con el botón izquierdo,
/// para no tapar nunca un clic en el borde. Al entrar un arrastre, lee sus
/// archivos (`CF_HDROP`) y los guarda para [`dragged_files`]; al soltar,
/// acepta como copia. Tiene su propio hilo (OLE pide uno STA con su cola),
/// aparte del de los ganchos, que tiene que responder al tiro.
mod drop_target {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use windows::core::implement;
    use windows::Win32::Foundation::{HWND, POINTL};
    use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{
        OleInitialize, RegisterDragDrop, ReleaseStgMedium, IDropTarget, IDropTarget_Impl, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows_core::Ref;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostThreadMessageW, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos,
        ShowWindow, TranslateMessage, HWND_TOPMOST, LWA_ALPHA, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_APP, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    use super::lock;

    /// El grosor de la franja del borde.
    const THICK: f64 = 2.0;
    /// Cuánto espera [`files`] a que el arrastre entre en la franja.
    const WAIT: Duration = Duration::from_millis(600);
    const WM_SHOW: u32 = WM_APP + 2;
    const WM_HIDE: u32 = WM_APP + 3;

    static THREAD: AtomicU32 = AtomicU32::new(0);
    /// El botón izquierdo está apretado (lo ve el gancho del mouse).
    static HELD: AtomicBool = AtomicBool::new(false);
    /// Ya se pidió mostrar las franjas en este arrastre.
    static SHOWN: AtomicBool = AtomicBool::new(false);
    /// Lo que trae el arrastre actual: `None` hasta que entra en una franja.
    static DRAGGED: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);

    /// Arranca el hilo de las franjas (una vez, junto con los ganchos).
    pub fn start() {
        if THREAD.load(Ordering::Acquire) != 0 {
            return;
        }
        let spawned = std::thread::Builder::new().name("atic-kvm-drop".into()).spawn(run);
        if let Err(e) = spawned {
            tracing::warn!("no se pudo abrir la ventana para arrastrar al otro equipo: {e}");
        }
    }

    /// Desde el gancho del mouse (tiene que ser rápido: solo avisa al hilo).
    pub fn on_left_button(down: bool) {
        HELD.store(down, Ordering::Release);
        if down {
            SHOWN.store(false, Ordering::Release);
        } else if SHOWN.swap(false, Ordering::AcqRel) {
            post(WM_HIDE);
        }
    }

    /// Desde el gancho del mouse: el primer movimiento con el botón apretado
    /// es un arrastre que empieza.
    pub fn on_move() {
        if HELD.load(Ordering::Acquire) && !SHOWN.swap(true, Ordering::AcqRel) {
            post(WM_SHOW);
        }
    }

    fn post(message: u32) {
        let thread = THREAD.load(Ordering::Acquire);
        if thread != 0 {
            // SAFETY: un mensaje sin punteros a un hilo que atiende su cola.
            unsafe { PostThreadMessageW(thread, message, 0, 0) };
        }
    }

    pub fn files() -> Vec<PathBuf> {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(files) = lock(&DRAGGED).clone() {
                return files;
            }
            if Instant::now() >= until {
                return Vec::new();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn run() {
        // SAFETY: este hilo es STA para OLE, crea sus propias ventanas y atiende
        // su cola hasta el final del programa.
        unsafe {
            let _ = OleInitialize(None);
            let module = GetModuleHandleW(std::ptr::null());
            let class: Vec<u16> = "AticKvmDropEdge\0".encode_utf16().collect();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            let target: IDropTarget = DropTarget.into();
            let mut strips: Vec<windows_sys::Win32::Foundation::HWND> = Vec::new();
            THREAD.store(GetCurrentThreadId(), Ordering::Release);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_SHOW => show(&mut strips, &target, module, &class),
                    WM_HIDE => {
                        for &w in &strips {
                            ShowWindow(w, SW_HIDE);
                        }
                    }
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
    }

    /// Un arrastre empieza: las franjas van a los bordes hacia el otro (si se
    /// comparte con alguien) y lo del arrastre anterior se olvida.
    unsafe fn show(
        strips: &mut Vec<windows_sys::Win32::Foundation::HWND>,
        target: &IDropTarget,
        module: windows_sys::Win32::Foundation::HINSTANCE,
        class: &[u16],
    ) {
        *lock(&DRAGGED) = None;
        let Some(grab) = super::grab() else { return };
        if grab.is_remote() {
            return;
        }
        let Some((side, screens)) = grab.drop_edge() else { return };
        let rects = screens.exposed_strips(side, THICK);
        while strips.len() < rects.len() {
            // SAFETY: la clase se registró en este hilo; la ventana vive todo el programa.
            let w = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    module,
                    std::ptr::null(),
                )
            };
            if w.is_null() {
                tracing::warn!("no se pudo crear la ventana del borde para arrastrar");
                break;
            }
            // SAFETY: una ventana recién creada en este hilo, que ya inicializó OLE.
            unsafe {
                // Con alfa 0 Windows no la tomaría en cuenta para soltar; con 1 no se ve.
                SetLayeredWindowAttributes(w, 0, 1, LWA_ALPHA);
                if let Err(e) = RegisterDragDrop(HWND(w), target) {
                    tracing::warn!("no se pudo registrar la ventana del borde para arrastrar: {e}");
                }
            }
            strips.push(w);
        }
        for (i, &w) in strips.iter().enumerate() {
            // SAFETY: ventanas de este hilo.
            unsafe {
                match rects.get(i) {
                    Some(r) => {
                        SetWindowPos(
                            w,
                            HWND_TOPMOST,
                            r.x.round() as i32,
                            r.y.round() as i32,
                            r.w.round() as i32,
                            r.h.round() as i32,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        );
                    }
                    None => {
                        ShowWindow(w, SW_HIDE);
                    }
                }
            }
        }
    }

    #[implement(IDropTarget)]
    struct DropTarget;

    impl DropTarget {
        fn effect(effect: *mut DROPEFFECT) {
            let has = lock(&DRAGGED).as_ref().is_some_and(|f| !f.is_empty());
            if !effect.is_null() {
                // SAFETY: OLE pasa un puntero válido para la respuesta.
                unsafe { *effect = if has { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
            }
        }

        fn take(data: Ref<'_, IDataObject>) {
            let files = data.ok().map(read_hdrop).unwrap_or_default();
            if !files.is_empty() {
                tracing::debug!(n = files.len(), "archivos arrastrados al borde");
            }
            *lock(&DRAGGED) = Some(files);
        }
    }

    impl IDropTarget_Impl for DropTarget_Impl {
        fn DragEnter(&self, data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows_core::Result<()> {
            DropTarget::take(data);
            DropTarget::effect(effect);
            Ok(())
        }

        fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows_core::Result<()> {
            DropTarget::effect(effect);
            Ok(())
        }

        fn DragLeave(&self) -> windows_core::Result<()> {
            Ok(())
        }

        /// Soltar en la franja: los archivos ya van al otro (se mandaron al
        /// llegar al borde); aquí solo se acepta como copia.
        fn Drop(&self, data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows_core::Result<()> {
            if lock(&DRAGGED).as_ref().is_none_or(|f| f.is_empty()) {
                DropTarget::take(data);
            }
            DropTarget::effect(effect);
            Ok(())
        }
    }

    /// Las rutas de un `CF_HDROP` (lo que pone el Explorador al arrastrar archivos).
    fn read_hdrop(data: &IDataObject) -> Vec<PathBuf> {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        // SAFETY: un `IDataObject` vivo durante la llamada; el medio se libera abajo.
        let Ok(mut medium) = (unsafe { data.GetData(&format) }) else { return Vec::new() };
        let mut out = Vec::new();
        // SAFETY: con `TYMED_HGLOBAL` y `CF_HDROP`, `hGlobal` es un `HDROP`.
        unsafe {
            let drop = HDROP(medium.u.hGlobal.0);
            let n = DragQueryFileW(drop, u32::MAX, None);
            for i in 0..n {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                out.push(PathBuf::from(OsString::from_wide(&buf[..got.min(len)])));
            }
            ReleaseStgMedium(&mut medium);
        }
        out
    }
}
