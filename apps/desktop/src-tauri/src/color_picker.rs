//! Cuentagotas en vivo: no congela la pantalla.
//!
//! Una lupa pequeña (ventana opaca, always-on-top, sin foco) sigue al cursor.
//! El píxel se lee con `BitBlt` (Windows) o Core Graphics (macOS) en las coords
//! globales del cursor, así que coincide con lo que hay bajo el puntero también
//! con DPI ≠ 100% y varios monitores. El clic se come con un `WH_MOUSE_LL` de
//! sesión corta en Windows; en macOS con un `CGEventTap` activo, que también
//! traga Esc/Enter/R mientras dura la sesión.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::overlay;
use atic_core::MutexExt;

#[cfg(target_os = "macos")]
use core_foundation::base::TCFType;
#[cfg(target_os = "macos")]
use core_foundation::mach_port::CFMachPort;
#[cfg(target_os = "macos")]
use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};

const LABEL: &str = "color-loupe";
const GRID: u32 = 13;
const OFFSET: i32 = 28;
/// Aire alrededor del contenido. La piel líquida proyecta su sombra ahí; sin
/// margen el `drop-shadow` se recorta contra el borde de la ventana.
const PAD: f64 = 24.0;
const COMPACT: (f64, f64) = (264.0 + PAD * 2.0, 112.0 + PAD * 2.0);
const ROSE: (f64, f64) = (296.0 + PAD * 2.0, 540.0 + PAD * 2.0);
/// Lo que tarda la piel en encogerse al cerrar la rosa. La ventana espera esa
/// animación antes de achicarse: si achica primero, el contenido se recorta.
const SHRINK_DELAY: Duration = Duration::from_millis(260);
/// Igual, para la salida completa. La lupa se despide antes de desaparecer.
const CLOSE_DELAY: Duration = Duration::from_millis(200);

static RUNNING: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static ROSE_OPEN: AtomicBool = AtomicBool::new(false);
static WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
static COMMIT_PENDING: AtomicBool = AtomicBool::new(false);
static PICK_POSITION: AtomicU64 = AtomicU64::new(0);
static LOUPE_HWND: AtomicIsize = AtomicIsize::new(0);
/// `PAD` en píxeles físicos del monitor donde está la lupa.
///
/// La ventana es más grande que la gota: el resto es aire transparente para la
/// sombra. Todo lo que pregunta «¿el cursor está sobre la lupa?» tiene que
/// medir contra la gota, no contra la ventana, o el cuentagotas se queda con
/// un anillo muerto donde ni lee color ni recibe clics.
static LOUPE_PAD_PX: AtomicI32 = AtomicI32::new(0);
/// Gota de la lupa en puntos globales (macOS), para el hit-test del event tap.
#[cfg(target_os = "macos")]
static LOUPE_X: AtomicI32 = AtomicI32::new(0);
#[cfg(target_os = "macos")]
static LOUPE_Y: AtomicI32 = AtomicI32::new(0);
#[cfg(target_os = "macos")]
static LOUPE_W: AtomicI32 = AtomicI32::new(0);
#[cfg(target_os = "macos")]
static LOUPE_H: AtomicI32 = AtomicI32::new(0);
/// La rosa tiene el foco de teclado: los atajos se dejan pasar (ahí se tipea).
#[cfg(target_os = "macos")]
static LOUPE_FOCUSED: AtomicBool = AtomicBool::new(false);
/// Ventana que tenía el primer plano antes de que la rosa lo tomara.
static PREVIOUS_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
static WANT_PICK: AtomicBool = AtomicBool::new(false);
static WANT_CANCEL: AtomicBool = AtomicBool::new(false);
/// Enter tragado por el hook: no reusa `WANT_PICK`, que lleva la coords del clic.
static WANT_ENTER_COMMIT: AtomicBool = AtomicBool::new(false);
static WANT_TOGGLE_ROSE: AtomicBool = AtomicBool::new(false);
static EAT_LEFT_UP: AtomicBool = AtomicBool::new(false);
static EAT_RIGHT_UP: AtomicBool = AtomicBool::new(false);
static EAT_ESC_UP: AtomicBool = AtomicBool::new(false);
static EAT_ENTER_UP: AtomicBool = AtomicBool::new(false);
static EAT_R_UP: AtomicBool = AtomicBool::new(false);
static LAST_PATCH: Mutex<Option<OverlayPatch>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayPatch {
    pub session: u64,
    pub hex: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub size: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct CommitRequest {
    session: u64,
    patch: Option<OverlayPatch>,
}

#[derive(Clone, Serialize)]
struct PickerError {
    session: u64,
    message: String,
}

fn report_error(app: &AppHandle, session: u64, message: String) {
    tracing::warn!(%message, session, "color picker");
    let _ = app.emit_to(
        LABEL,
        "color-picker-error",
        PickerError { session, message },
    );
}

#[tauri::command]
pub async fn start_color_picker(app: AppHandle) -> Result<(), String> {
    trigger(&app)
}

#[tauri::command]
pub async fn stop_color_picker(app: AppHandle, session: u64) {
    if !abort(session) {
        stop(&app);
    }
}

#[tauri::command]
pub async fn complete_color_pick(
    app: AppHandle,
    hex: String,
    session: u64,
) -> Result<String, String> {
    if abort(session) {
        return Err("La sesión de color ya terminó".into());
    }
    let result = finish(&app, &hex);
    // A failed clipboard write leaves the loop alive and allows retry/cancel.
    COMMIT_PENDING.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub async fn color_picker_set_rose(app: AppHandle, open: bool, session: u64) -> Result<(), String> {
    if abort(session) {
        return Err("La sesión de color ya terminó".into());
    }
    let previous = ROSE_OPEN.swap(open, Ordering::SeqCst);
    // Crecer va primero y encoger va último, la misma regla que la pill: la
    // ventana es la unión de los dos tamaños mientras dura la animación, así
    // que el contenido nunca se recorta contra el borde.
    if open {
        if let Err(error) = resize_loupe(&app, true) {
            ROSE_OPEN.store(previous, Ordering::SeqCst);
            return Err(error);
        }
    } else {
        shrink_loupe_soon(&app, session);
    }
    Ok(())
}

/// Achica la ventana cuando la piel ya se encogió. Se cancela sola si la rosa
/// vuelve a abrirse o si la sesión termina mientras espera.
fn shrink_loupe_soon(app: &AppHandle, session: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHRINK_DELAY);
        if abort(session) || ROSE_OPEN.load(Ordering::SeqCst) {
            return;
        }
        let _ = resize_loupe(&app, false);
    });
}

/// Esconde la lupa cuando terminó su animación de salida.
///
/// Una sesión nueva mueve `GENERATION`, así que el escondite pendiente de la
/// anterior se cancela y no se lleva por delante a la que acaba de abrir.
fn hide_loupe_soon(app: &AppHandle) {
    let token = GENERATION.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(CLOSE_DELAY);
        if GENERATION.load(Ordering::SeqCst) != token || RUNNING.load(Ordering::SeqCst) {
            return;
        }
        hide_loupe(&app);
    });
}

#[derive(Clone, Serialize)]
pub struct PickerState {
    session: u64,
    active: bool,
    open: bool,
    patch: Option<OverlayPatch>,
}

#[tauri::command]
pub async fn color_picker_state() -> PickerState {
    PickerState {
        session: GENERATION.load(Ordering::SeqCst),
        active: RUNNING.load(Ordering::SeqCst),
        open: ROSE_OPEN.load(Ordering::SeqCst),
        patch: LAST_PATCH.lock_or_recover().clone(),
    }
}

pub fn trigger(app: &AppHandle) -> Result<(), String> {
    if RUNNING.load(Ordering::SeqCst) {
        stop(app);
        return Ok(());
    }
    if WORKER_ACTIVE.load(Ordering::SeqCst) {
        return Err("Suelta el botón del ratón antes de volver a elegir un color".into());
    }
    crate::capture_session::cancel_capture_session(app.clone());
    start(app)
}

/// Cierra la sesión con despedida: la piel corre su salida y la ventana se
/// esconde después.
pub fn stop(app: &AppHandle) {
    stop_inner(app, true);
}

/// Cierra la sesión y esconde la lupa **ya**.
///
/// Lo usa quien va a congelar la pantalla enseguida: una despedida de 200 ms
/// terminaría dibujada dentro de la captura.
pub fn stop_now(app: &AppHandle) {
    stop_inner(app, false);
}

fn stop_inner(app: &AppHandle, farewell: bool) {
    if !RUNNING.swap(false, Ordering::SeqCst) {
        return;
    }
    let session = GENERATION.fetch_add(1, Ordering::SeqCst);
    ROSE_OPEN.store(false, Ordering::SeqCst);
    WANT_PICK.store(false, Ordering::SeqCst);
    WANT_CANCEL.store(false, Ordering::SeqCst);
    WANT_ENTER_COMMIT.store(false, Ordering::SeqCst);
    WANT_TOGGLE_ROSE.store(false, Ordering::SeqCst);
    COMMIT_PENDING.store(false, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    LOUPE_FOCUSED.store(false, Ordering::SeqCst);
    overlay::set_capturing(app, false);
    // El evento primero: la piel corre su animación de salida y recién
    // entonces la ventana se esconde.
    let _ = app.emit_to(LABEL, "color-picker-ended", session);
    if farewell {
        hide_loupe_soon(app);
    } else {
        hide_loupe(app);
    }
}

fn start(app: &AppHandle) -> Result<(), String> {
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
        return Err(crate::ui_lang::capture_windows_only());
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        ensure_loupe(app)?;
        if WORKER_ACTIVE.swap(true, Ordering::SeqCst) {
            return Err("Suelta el botón del ratón antes de volver a elegir un color".into());
        }
        RUNNING.store(true, Ordering::SeqCst);
        ROSE_OPEN.store(false, Ordering::SeqCst);
        COMMIT_PENDING.store(false, Ordering::SeqCst);
        *LAST_PATCH.lock_or_recover() = None;
        let token = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        // La pill NO se esconde. Es click-through mientras dura la sesión, así
        // que no se come el clic, y quedarse a la vista es lo que hace que el
        // cuentagotas se lea como una tool de Atic y no como una ventana suelta.
        // Solo se pierde el color del píxel que ella misma tapa.
        overlay::set_capturing(app, true);
        if let Err(error) = show_loupe(app) {
            stop(app);
            WORKER_ACTIVE.store(false, Ordering::SeqCst);
            return Err(error);
        }
        let app_bg = app.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("atic-color-pick".into())
            .spawn(move || run_loop(app_bg, token))
        {
            stop(app);
            WORKER_ACTIVE.store(false, Ordering::SeqCst);
            return Err(error.to_string());
        }
        Ok(())
    }
}

/// Constantes de teclado por plataforma: VK de Windows / keycode de macOS.
#[cfg(windows)]
const KEY_ESC: i32 = 0x1B;
#[cfg(windows)]
const KEY_ENTER: i32 = 0x0D;
#[cfg(windows)]
const KEY_R: i32 = 0x52;
#[cfg(target_os = "macos")]
const KEY_ESC: i32 = 53;
#[cfg(target_os = "macos")]
const KEY_ENTER: i32 = 36;
#[cfg(target_os = "macos")]
const KEY_R: i32 = 15;

/// ¿Sigue apretado el botón que abrió la tool?
#[cfg(windows)]
fn primary_down() -> bool {
    key_down(0x01)
}

#[cfg(target_os = "macos")]
fn primary_down() -> bool {
    crate::floating::primary_button_down()
}

#[cfg(any(windows, target_os = "macos"))]
fn run_loop(app: AppHandle, token: u64) {
    struct WorkerGuard(AppHandle, u64);
    impl Drop for WorkerGuard {
        fn drop(&mut self) {
            if !abort(self.1) {
                stop(&self.0);
            }
            WORKER_ACTIVE.store(false, Ordering::SeqCst);
        }
    }
    let _worker = WorkerGuard(app.clone(), token);
    WANT_PICK.store(false, Ordering::SeqCst);
    WANT_CANCEL.store(false, Ordering::SeqCst);
    WANT_ENTER_COMMIT.store(false, Ordering::SeqCst);
    WANT_TOGGLE_ROSE.store(false, Ordering::SeqCst);
    EAT_LEFT_UP.store(false, Ordering::SeqCst);
    EAT_RIGHT_UP.store(false, Ordering::SeqCst);
    EAT_ESC_UP.store(false, Ordering::SeqCst);
    EAT_ENTER_UP.store(false, Ordering::SeqCst);
    EAT_R_UP.store(false, Ordering::SeqCst);

    // El clic que abrió la tool no debe copiar un color.
    while primary_down() {
        if abort(token) {
            return;
        }
        pump_mouse();
        std::thread::sleep(Duration::from_millis(16));
    }

    let input = match start_input_capture(token) {
        Ok(input) => input,
        Err(error) => {
            report_error(&app, token, error);
            stop(&app);
            return;
        }
    };

    let mut was_r = key_down(KEY_R);
    let mut was_enter = key_down(0x0D);
    let mut pending_since = Instant::now();
    let mut sample_failed = false;
    let mut sampled_at = Instant::now() - Duration::from_secs(1);
    let mut monitors = atic_capture::monitors::enumerate();
    let mut monitors_at = Instant::now();
    let mut previous_position = None;
    loop {
        let iteration = Instant::now();
        if abort(token) {
            break;
        }

        if WANT_CANCEL.swap(false, Ordering::SeqCst) || key_down(KEY_ESC) {
            stop(&app);
            break;
        }
        let rose = ROSE_OPEN.load(Ordering::SeqCst);
        let enter = key_down(KEY_ENTER);
        let clicked = WANT_PICK.swap(false, Ordering::SeqCst);
        let focused = loupe_is_foreground();
        // El hook traga Enter/R: `GetAsyncKeyState` no las ve. El flanco
        // queda en el atomic; el poll queda de respaldo si el hook no instaló.
        let pick = clicked
            || WANT_ENTER_COMMIT.swap(false, Ordering::SeqCst)
            || (enter && !was_enter && !focused);
        was_enter = enter;
        let r_key = key_down(KEY_R);
        let toggle_rose =
            WANT_TOGGLE_ROSE.swap(false, Ordering::SeqCst) || (r_key && !was_r && !focused);
        if toggle_rose && !COMMIT_PENDING.load(Ordering::SeqCst) {
            // The frontend owns the editing state and acknowledges the resize.
            let _ = app.emit_to(LABEL, "color-toggle-rose", token);
        }
        was_r = r_key;
        if COMMIT_PENDING.load(Ordering::SeqCst) {
            if pending_since.elapsed() > Duration::from_secs(5) {
                COMMIT_PENDING.store(false, Ordering::SeqCst);
                report_error(
                    &app,
                    token,
                    "La copia no respondió. Vuelve a intentarlo.".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(16));
            continue;
        }
        let position = if clicked {
            Some(unpack_position(PICK_POSITION.load(Ordering::SeqCst)))
        } else {
            crate::floating::cursor_position()
        };
        let mut selected = None;
        if !rose {
            if let Some((cx, cy)) = position {
                if cursor_over_loupe(cx, cy) {
                    std::thread::sleep(Duration::from_millis(16));
                    continue;
                }
                if monitors_at.elapsed() >= Duration::from_secs(2) {
                    monitors = atic_capture::monitors::enumerate();
                    monitors_at = Instant::now();
                }
                // Native positioning is asynchronous and independent of JS.
                if previous_position != position {
                    place_loupe_cached(&app, cx, cy, false, &monitors);
                    previous_position = position;
                }
                if pick || sampled_at.elapsed() >= Duration::from_millis(33) {
                    match sample_live(cx, cy, GRID, &monitors) {
                        Ok(mut patch) => {
                            patch.session = token;
                            *LAST_PATCH.lock_or_recover() = Some(patch.clone());
                            sample_failed = false;
                            if pick {
                                selected = Some(patch);
                            } else {
                                let _ = app.emit_to(LABEL, "color-patch", &patch);
                            }
                        }
                        Err(error) => {
                            if !sample_failed || pick {
                                report_error(&app, token, error);
                            }
                            sample_failed = true;
                        }
                    }
                    sampled_at = Instant::now();
                }
            }
        }
        if pick && (rose || selected.is_some()) {
            COMMIT_PENDING.store(true, Ordering::SeqCst);
            pending_since = Instant::now();
            if let Err(error) = app.emit_to(
                LABEL,
                "color-request-commit",
                CommitRequest {
                    session: token,
                    patch: selected,
                },
            ) {
                COMMIT_PENDING.store(false, Ordering::SeqCst);
                report_error(&app, token, error.to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(16).saturating_sub(iteration.elapsed()));
    }
    // Even after cancellation, consume the release paired with a swallowed down.
    // New sessions wait for this worker, so an old hook cannot eat their clicks.
    let _ = input.join();
}

fn pack_position(x: i32, y: i32) -> u64 {
    ((x as u32 as u64) << 32) | y as u32 as u64
}

fn unpack_position(value: u64) -> (i32, i32) {
    ((value >> 32) as u32 as i32, value as u32 as i32)
}

// --- Windows: hooks de sesión corta -----------------------------------------

/// Escucha global de Windows: hooks de sesión corta en un hilo propio.
#[cfg(windows)]
fn start_input_capture(token: u64) -> Result<std::thread::JoinHandle<()>, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let input = std::thread::Builder::new()
        .name("atic-color-input".into())
        .spawn(move || {
            let mouse = install_click_hook();
            let keys = install_key_hook();
            let _ = tx.send(!mouse.is_null());
            if mouse.is_null() {
                uninstall_key_hook(keys);
                return;
            }
            while !abort(token) || eat_pending() {
                pump_mouse();
                if abort(token) {
                    clear_eat_if_key_up();
                }
                std::thread::sleep(Duration::from_millis(4));
            }
            uninstall_key_hook(keys);
            uninstall_click_hook(mouse);
        })
        .map_err(|error| format!("No se pudo iniciar el cuentagotas: {error}"))?;
    if rx.recv_timeout(Duration::from_secs(2)).ok() != Some(true) {
        let _ = input.join();
        return Err("No se pudo activar la captura del ratón".into());
    }
    Ok(input)
}

// --- macOS: event tap activo ------------------------------------------------

#[cfg(target_os = "macos")]
type CGEventRef = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFMachPortRef = core_foundation::mach_port::CFMachPortRef;

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            u32,
            CGEventRef,
            *mut std::ffi::c_void,
        ) -> CGEventRef,
        user_info: *mut std::ffi::c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetLocation(event: CGEventRef) -> core_graphics::geometry::CGPoint;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
}

/// Puerto vivo del tap, para re-habilitarlo si el SO lo desactiva por timeout.
#[cfg(target_os = "macos")]
static TAP_PORT: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Tipos de `CGEventType` que interesan (CGEventTypes.h).
#[cfg(target_os = "macos")]
const EV_LEFT_DOWN: u32 = 1;
#[cfg(target_os = "macos")]
const EV_LEFT_UP: u32 = 2;
#[cfg(target_os = "macos")]
const EV_RIGHT_DOWN: u32 = 3;
#[cfg(target_os = "macos")]
const EV_RIGHT_UP: u32 = 4;
#[cfg(target_os = "macos")]
const EV_KEY_DOWN: u32 = 10;
#[cfg(target_os = "macos")]
const EV_KEY_UP: u32 = 11;
#[cfg(target_os = "macos")]
const EV_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
#[cfg(target_os = "macos")]
const EV_TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
/// `kCGKeyboardEventKeycode`.
#[cfg(target_os = "macos")]
const KEYCODE_FIELD: u32 = 9;
/// `kCGSessionEventTap`, `kCGEventTapPlacementHeadInsert`, `Default` (activo).
#[cfg(target_os = "macos")]
const SESSION_TAP: u32 = 1;
#[cfg(target_os = "macos")]
const TAP_HEAD_INSERT: u32 = 0;
#[cfg(target_os = "macos")]
const TAP_ACTIVE: u32 = 0;

/// Callback del tap: sólo atomics y lecturas de CG, como el wndproc de
/// Windows. Devolver null se come el evento.
#[cfg(target_os = "macos")]
unsafe extern "C" fn tap_callback(
    _proxy: *mut std::ffi::c_void,
    etype: u32,
    event: CGEventRef,
    _user_info: *mut std::ffi::c_void,
) -> CGEventRef {
    if etype == EV_TAP_DISABLED_BY_TIMEOUT || etype == EV_TAP_DISABLED_BY_USER {
        let port = TAP_PORT.load(Ordering::Acquire);
        if !port.is_null() {
            unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
        }
        return event;
    }
    // SAFETY: `event` llega vivo del SO durante el callback.
    let location = unsafe { CGEventGetLocation(event) };
    let (x, y) = (location.x.round() as i32, location.y.round() as i32);
    let over_loupe = cursor_over_loupe(x, y);
    let running = RUNNING.load(Ordering::SeqCst);
    let consume = match etype {
        EV_LEFT_DOWN => {
            if running && !over_loupe {
                if !COMMIT_PENDING.load(Ordering::SeqCst) && !WANT_PICK.load(Ordering::SeqCst) {
                    PICK_POSITION.store(pack_position(x, y), Ordering::SeqCst);
                    WANT_PICK.store(true, Ordering::SeqCst);
                }
                EAT_LEFT_UP.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        }
        EV_LEFT_UP => EAT_LEFT_UP.swap(false, Ordering::SeqCst),
        EV_RIGHT_DOWN => {
            if running && !over_loupe {
                WANT_CANCEL.store(true, Ordering::SeqCst);
                EAT_RIGHT_UP.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        }
        EV_RIGHT_UP => EAT_RIGHT_UP.swap(false, Ordering::SeqCst),
        EV_KEY_DOWN => {
            if !running || loupe_is_foreground() {
                false
            } else {
                // SAFETY: `event` es un evento de teclado vivo.
                let key = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) } as i32;
                match key {
                    KEY_ESC => {
                        if !EAT_ESC_UP.swap(true, Ordering::SeqCst) {
                            WANT_CANCEL.store(true, Ordering::SeqCst);
                        }
                        true
                    }
                    KEY_ENTER => {
                        if !EAT_ENTER_UP.swap(true, Ordering::SeqCst) {
                            WANT_ENTER_COMMIT.store(true, Ordering::SeqCst);
                        }
                        true
                    }
                    KEY_R => {
                        if !EAT_R_UP.swap(true, Ordering::SeqCst) {
                            WANT_TOGGLE_ROSE.store(true, Ordering::SeqCst);
                        }
                        true
                    }
                    _ => false,
                }
            }
        }
        EV_KEY_UP => {
            // SAFETY: `event` es un evento de teclado vivo.
            let key = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) } as i32;
            match key {
                KEY_ESC => EAT_ESC_UP.swap(false, Ordering::SeqCst),
                KEY_ENTER => EAT_ENTER_UP.swap(false, Ordering::SeqCst),
                KEY_R => EAT_R_UP.swap(false, Ordering::SeqCst),
                _ => false,
            }
        }
        _ => false,
    };
    if consume {
        std::ptr::null_mut()
    } else {
        event
    }
}

/// Crea el tap activo. Sin Accesibilidad macOS no lo permite: se pide y se
/// devuelve un mensaje para la UI.
#[cfg(target_os = "macos")]
fn create_event_tap() -> Result<CFMachPort, String> {
    if !crate::macos_notes::accessibility_trusted() {
        let _ = crate::macos_notes::request_accessibility();
        return Err(crate::ui_lang::msg(
            "Atic necesita permiso de Accesibilidad para el cuentagotas. Actívalo en Ajustes → Privacidad y seguridad → Accesibilidad y reinicia Atic.",
            "Atic needs Accessibility permission for the eyedropper. Enable it in Settings → Privacy & Security → Accessibility, then restart Atic.",
        ));
    }
    let mask = (1u64 << EV_LEFT_DOWN)
        | (1u64 << EV_LEFT_UP)
        | (1u64 << EV_RIGHT_DOWN)
        | (1u64 << EV_RIGHT_UP)
        | (1u64 << EV_KEY_DOWN)
        | (1u64 << EV_KEY_UP);
    // SAFETY: callback estático y sin user_info; sólo crea el tap.
    let port = unsafe {
        CGEventTapCreate(
            SESSION_TAP,
            TAP_HEAD_INSERT,
            TAP_ACTIVE,
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };
    if port.is_null() {
        return Err(crate::ui_lang::msg(
            "No se pudo escuchar el ratón. Revisa el permiso de Accesibilidad de Atic.",
            "Could not listen to the mouse. Check Atic's Accessibility permission.",
        ));
    }
    TAP_PORT.store(port.cast::<std::ffi::c_void>(), Ordering::Release);
    // SAFETY: la regla de creación transfiere la referencia del tap a CFMachPort.
    Ok(unsafe { CFMachPort::wrap_under_create_rule(port) })
}

#[cfg(target_os = "macos")]
fn attach_event_tap(tap: &CFMachPort) -> Result<(), String> {
    let source = tap
        .create_runloop_source(0)
        .map_err(|_| "No se pudo crear la fuente del run loop".to_string())?;
    let current = CFRunLoop::get_current();
    current.add_source(&source, unsafe { kCFRunLoopCommonModes });
    // SAFETY: el tap vive hasta que el hilo termina.
    unsafe { CGEventTapEnable(tap.as_concrete_TypeRef(), true) };
    Ok(())
}

#[cfg(target_os = "macos")]
fn detach_event_tap() {
    let port = TAP_PORT.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !port.is_null() {
        // SAFETY: puerto creado por este módulo y todavía vivo.
        unsafe { CGEventTapEnable(port as CFMachPortRef, false) };
    }
}

/// Escucha global de macOS: un `CGEventTap` activo en un hilo con run loop.
#[cfg(target_os = "macos")]
fn start_input_capture(token: u64) -> Result<std::thread::JoinHandle<()>, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let input = std::thread::Builder::new()
        .name("atic-color-input".into())
        .spawn(move || {
            let tap = match create_event_tap() {
                Ok(tap) => tap,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            if let Err(error) = attach_event_tap(&tap) {
                let _ = tx.send(Err(error));
                return;
            }
            let _ = tx.send(Ok(()));
            while !abort(token) {
                CFRunLoop::run_in_mode(
                    unsafe { kCFRunLoopDefaultMode },
                    Duration::from_millis(50),
                    false,
                );
            }
            detach_event_tap();
            // el tap se libera al salir del hilo
        })
        .map_err(|error| format!("No se pudo iniciar el cuentagotas: {error}"))?;
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(input),
        Ok(Err(error)) => {
            let _ = input.join();
            Err(error)
        }
        _ => {
            let _ = input.join();
            Err("No se pudo activar la captura del ratón".into())
        }
    }
}

#[cfg(target_os = "macos")]
fn cursor_over_loupe(x: i32, y: i32) -> bool {
    let w = LOUPE_W.load(Ordering::SeqCst);
    let h = LOUPE_H.load(Ordering::SeqCst);
    if w <= 0 || h <= 0 {
        return false;
    }
    let pad = LOUPE_PAD_PX.load(Ordering::SeqCst);
    let left = LOUPE_X.load(Ordering::SeqCst) + pad;
    let top = LOUPE_Y.load(Ordering::SeqCst) + pad;
    x >= left && x < left + (w - pad * 2).max(0) && y >= top && y < top + (h - pad * 2).max(0)
}

#[cfg(target_os = "macos")]
fn key_down(keycode: i32) -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceKeyState(state_id: i32, key: u16) -> u8;
    }
    // kCGEventSourceStateCombinedSessionState = 0.
    unsafe { CGEventSourceKeyState(0, keycode as u16) != 0 }
}

#[cfg(target_os = "macos")]
fn pump_mouse() {}

/// En Mac los atajos se tragan salvo con la rosa abierta y Atic al frente.
#[cfg(target_os = "macos")]
fn loupe_is_foreground() -> bool {
    LOUPE_FOCUSED.load(Ordering::SeqCst)
}

#[cfg(windows)]
fn cursor_over_loupe(x: i32, y: i32) -> bool {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::PtInRect;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let hwnd = LOUPE_HWND.load(Ordering::SeqCst);
    if hwnd == 0 {
        return false;
    }
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd as _, &mut rc) } == 0 {
        return false;
    }
    // Encoger hasta la gota: el aire de la sombra no es la lupa.
    let pad = LOUPE_PAD_PX.load(Ordering::SeqCst);
    rc.left += pad;
    rc.top += pad;
    rc.right -= pad;
    rc.bottom -= pad;
    unsafe { PtInRect(&rc, POINT { x, y }) != 0 }
}

#[cfg(windows)]
fn key_down(vk: i32) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

#[cfg(windows)]
fn pump_mouse() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    unsafe {
        let mut msg = std::mem::zeroed::<MSG>();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(windows)]
fn install_click_hook() -> windows_sys::Win32::UI::WindowsAndMessaging::HHOOK {
    use windows_sys::Win32::Foundation::HINSTANCE;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_MOUSE_LL};

    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        SetWindowsHookExW(WH_MOUSE_LL, Some(click_hook), module as HINSTANCE, 0)
    }
}

#[cfg(windows)]
fn uninstall_click_hook(hook: windows_sys::Win32::UI::WindowsAndMessaging::HHOOK) {
    use windows_sys::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
    if !hook.is_null() {
        unsafe {
            UnhookWindowsHookEx(hook);
        }
    }
}

#[cfg(windows)]
fn eat_pending() -> bool {
    EAT_LEFT_UP.load(Ordering::SeqCst)
        || EAT_RIGHT_UP.load(Ordering::SeqCst)
        || EAT_ESC_UP.load(Ordering::SeqCst)
        || EAT_ENTER_UP.load(Ordering::SeqCst)
        || EAT_R_UP.load(Ordering::SeqCst)
}

#[cfg(windows)]
fn clear_eat_if_key_up() {
    if !key_down(0x01) {
        EAT_LEFT_UP.store(false, Ordering::SeqCst);
    }
    if !key_down(0x02) {
        EAT_RIGHT_UP.store(false, Ordering::SeqCst);
    }
    if !key_down(0x1B) {
        EAT_ESC_UP.store(false, Ordering::SeqCst);
    }
    if !key_down(0x0D) {
        EAT_ENTER_UP.store(false, Ordering::SeqCst);
    }
    if !key_down(0x52) {
        EAT_R_UP.store(false, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn loupe_is_foreground() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = LOUPE_HWND.load(Ordering::SeqCst);
    hwnd != 0 && unsafe { GetForegroundWindow() as isize == hwnd }
}

#[cfg(windows)]
fn install_key_hook() -> windows_sys::Win32::UI::WindowsAndMessaging::HHOOK {
    use windows_sys::Win32::Foundation::HINSTANCE;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_KEYBOARD_LL};

    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), module as HINSTANCE, 0)
    }
}

#[cfg(windows)]
fn uninstall_key_hook(hook: windows_sys::Win32::UI::WindowsAndMessaging::HHOOK) {
    use windows_sys::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
    if !hook.is_null() {
        unsafe {
            UnhookWindowsHookEx(hook);
        }
    }
}

/// Come Esc / Enter / R para que no se escriban en la app de abajo.
///
/// El muestreo no toma foco (`SW_SHOWNOACTIVATE`): sin esto, Enter manda el
/// chat y R se escribe en el editor. Con la rosa abierta la lupa SÍ tiene
/// foco y hay que dejar pasar — el hex se tipea ahí.
///
/// Mismas reglas que el hook del mouse: volver al toque, no I/O, desinstalar
/// al terminar y tragar el key-up pareado aunque la sesión ya cerró.
#[cfg(windows)]
unsafe extern "system" fn key_hook(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let msg = wparam as u32;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    if !is_up && !is_down {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    if lparam == 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let vk = unsafe { (*(lparam as *const KBDLLHOOKSTRUCT)).vkCode };
    if is_up {
        let eaten = match vk {
            0x1B => EAT_ESC_UP.swap(false, Ordering::SeqCst),
            0x0D => EAT_ENTER_UP.swap(false, Ordering::SeqCst),
            0x52 => EAT_R_UP.swap(false, Ordering::SeqCst),
            _ => false,
        };
        if eaten {
            return 1;
        }
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    if !RUNNING.load(Ordering::SeqCst) || loupe_is_foreground() {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    match vk {
        0x1B => {
            // `swap` ignora el repeat: el primer down marca el up pendiente.
            if !EAT_ESC_UP.swap(true, Ordering::SeqCst) {
                WANT_CANCEL.store(true, Ordering::SeqCst);
            }
            return 1;
        }
        0x0D => {
            if !EAT_ENTER_UP.swap(true, Ordering::SeqCst) {
                WANT_ENTER_COMMIT.store(true, Ordering::SeqCst);
            }
            return 1;
        }
        0x52 => {
            if !EAT_R_UP.swap(true, Ordering::SeqCst) {
                WANT_TOGGLE_ROSE.store(true, Ordering::SeqCst);
            }
            return 1;
        }
        _ => {}
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Come el clic de pantalla para no pulsar lo de debajo. Sobre la lupa deja pasar.
///
/// El proc vuelve enseguida: no espera locks ni I/O. Un `WH_MOUSE_LL` lento
/// congela el mouse de todo el sistema; por eso solo vive durante la sesión.
#[cfg(windows)]
unsafe extern "system" fn click_hook(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_RBUTTONDOWN, WM_RBUTTONUP,
    };

    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let msg = wparam as u32;
    if msg == WM_LBUTTONUP && EAT_LEFT_UP.swap(false, Ordering::SeqCst) {
        return 1;
    }
    if msg == WM_RBUTTONUP && EAT_RIGHT_UP.swap(false, Ordering::SeqCst) {
        return 1;
    }
    if !RUNNING.load(Ordering::SeqCst) {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    // Movement packets need no window lookup: only button-downs are intercepted.
    if !matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN) {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
    let over_loupe = cursor_over_loupe(info.pt.x, info.pt.y);
    match msg {
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK if !over_loupe => {
            if !COMMIT_PENDING.load(Ordering::SeqCst) && !WANT_PICK.load(Ordering::SeqCst) {
                PICK_POSITION.store(pack_position(info.pt.x, info.pt.y), Ordering::SeqCst);
                WANT_PICK.store(true, Ordering::SeqCst);
            }
            EAT_LEFT_UP.store(true, Ordering::SeqCst);
            return 1;
        }
        WM_RBUTTONDOWN if !over_loupe => {
            WANT_CANCEL.store(true, Ordering::SeqCst);
            EAT_RIGHT_UP.store(true, Ordering::SeqCst);
            return 1;
        }
        _ => {}
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn abort(token: u64) -> bool {
    !RUNNING.load(Ordering::SeqCst) || GENERATION.load(Ordering::SeqCst) != token
}

#[cfg(any(windows, target_os = "macos"))]
fn sample_live(
    cx: i32,
    cy: i32,
    size: u32,
    monitors: &[atic_capture::monitors::MonitorInfo],
) -> Result<OverlayPatch, String> {
    use atic_capture::{engine, Rect};

    let size = (size.clamp(1, 31) | 1).max(1);
    let half = size as i32 / 2;
    let origin_x = cx - half;
    let origin_y = cy - half;
    let vs =
        Rect::union_all(monitors.iter().map(|m| m.bounds)).ok_or("No hay pantallas disponibles")?;
    let capture = Rect::from_ltrb(
        origin_x.max(vs.x),
        origin_y.max(vs.y),
        (origin_x + size as i32).min(vs.right()),
        (origin_y + size as i32).min(vs.bottom()),
    );
    if capture.is_empty() {
        return Err("fuera de pantalla".into());
    }
    let frame = engine::capture_rect(capture, false).map_err(crate::ui_lang::map_capture_error)?;
    // En Mac el frame sale a resolución nativa (puntos × escala): el punto
    // global se traduce a píxeles del PNG. En Windows ya coinciden.
    let scale = if capture.width > 0 {
        frame.width() as f64 / f64::from(capture.width)
    } else {
        1.0
    };
    let to_pixel = |x: i32, y: i32| {
        (
            ((f64::from(x - capture.x)) * scale).round() as i32,
            ((f64::from(y - capture.y)) * scale).round() as i32,
        )
    };
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for py in 0..size as i32 {
        for px in 0..size as i32 {
            let (fx, fy) = to_pixel(origin_x + px, origin_y + py);
            let i = ((py * size as i32 + px) * 4) as usize;
            if let Some([r, g, b, a]) = frame.pixel_rgba(fx, fy) {
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
                rgba[i + 3] = a;
            }
        }
    }
    let [r, g, b, _] = {
        let (fx, fy) = to_pixel(cx, cy);
        frame.pixel_rgba(fx, fy).unwrap_or([0, 0, 0, 255])
    };
    Ok(OverlayPatch {
        session: 0,
        hex: format!("#{r:02X}{g:02X}{b:02X}"),
        r,
        g,
        b,
        size,
        rgba,
    })
}

fn finish(app: &AppHandle, hex: &str) -> Result<String, String> {
    let hex = normalize_color_value(hex)?;
    crate::clipboard_history::set_system_text(&hex)?;
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        let (ui_sounds, output_device_id, sound_voice) = {
            let cfg = state.config.lock_or_recover();
            (
                cfg.ui_sounds,
                cfg.output_device_id.clone(),
                cfg.sound_capture.clone(),
            )
        };
        if ui_sounds {
            crate::beep::play(
                crate::beep::SoundAction::Capture,
                &sound_voice,
                &output_device_id,
            );
        }
    }
    stop(app);
    let _ = app.emit("color-picked", &hex);
    Ok(hex)
}

fn normalize_color_value(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(format!("#{}", digits.to_ascii_uppercase()));
    }
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.len() <= 64 {
        let rgb = lower.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')'));
        let hsl = lower.strip_prefix("hsl(").and_then(|s| s.strip_suffix(')'));
        if let Some(body) = rgb.or(hsl) {
            let fields: Vec<_> = body.split(',').map(str::trim).collect();
            if fields.len() == 3
                && fields.iter().enumerate().all(|(i, field)| {
                    let is_percent = hsl.is_some() && i > 0;
                    let raw = if is_percent {
                        field.strip_suffix('%')
                    } else {
                        Some(*field)
                    };
                    let max = if rgb.is_some() {
                        255.0
                    } else if i == 0 {
                        360.0
                    } else {
                        100.0
                    };
                    raw.and_then(|s| s.parse::<f64>().ok())
                        .is_some_and(|n| n.is_finite() && n >= 0.0 && n <= max)
                })
            {
                return Ok(trimmed.to_string());
            }
        }
    }
    Err("color inválido".into())
}

fn ensure_loupe(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    Err("la ventana del cuentagotas no existe".into())
}

fn show_loupe(app: &AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Err("la ventana del cuentagotas no existe".into());
    };
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_always_on_top(true);
    resize_loupe(app, false)?;
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        if let Ok(hwnd) = window.hwnd() {
            LOUPE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            unsafe {
                ShowWindow(hwnd.0 as _, SW_SHOWNOACTIVATE);
            }
        } else {
            let _ = window.show();
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = window.show();
        // Por encima del menú, como la pill: la lupa puede quedar cerca del
        // techo y no debe meterse debajo de la barra.
        crate::overlay::macos_set_status_level(&window);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = window.show();
    }
    Ok(())
}

fn hide_loupe(app: &AppHandle) {
    LOUPE_HWND.store(0, Ordering::SeqCst);
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // `show_loupe` shows natively (`SW_SHOWNOACTIVATE`), so tao never flags the
    // window as visible and its `hide()` is a no-op: the loupe stayed on screen
    // after the session ended. Hide through the same native path.
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, IsWindow, IsWindowVisible, ShowWindow, SW_HIDE,
        };
        if let Ok(hwnd) = window.hwnd() {
            let previous = PREVIOUS_FOREGROUND.swap(0, Ordering::SeqCst);
            unsafe {
                // Hiding a focused loupe leaves it as the foreground window and
                // keys go to a hidden webview: hand the focus back first, while
                // this process still owns it.
                if previous != 0
                    && GetForegroundWindow() as isize == hwnd.0 as isize
                    && IsWindow(previous as _) != 0
                    && IsWindowVisible(previous as _) != 0
                {
                    crate::clipboard_history::force_foreground(previous as _);
                }
                ShowWindow(hwnd.0 as _, SW_HIDE);
            }
            return;
        }
    }
    let _ = window.hide();
}

fn resize_loupe(app: &AppHandle, rose: bool) -> Result<(), String> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or("La ventana del cuentagotas no existe")?;
    #[cfg(any(windows, target_os = "macos"))]
    {
        #[cfg(windows)]
        let hwnd = {
            let hwnd = window.hwnd().map_err(|e| e.to_string())?;
            LOUPE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            hwnd
        };
        if let Some((cx, cy)) = crate::floating::cursor_position() {
            place_loupe_cached(app, cx, cy, rose, &atic_capture::monitors::enumerate());
        }
        // No synchronous geometry getters in an IPC callback: these can wait for
        // the same window thread that is currently servicing the callback.
        // tao's `set_focus` only activates windows it showed itself, and this
        // one is shown natively, so it never gave the rose keyboard focus.
        if rose {
            #[cfg(windows)]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
                let raw = hwnd.0 as isize;
                let previous = unsafe { GetForegroundWindow() } as isize;
                if previous != 0 && previous != raw {
                    PREVIOUS_FOREGROUND.store(previous, Ordering::SeqCst);
                }
                std::thread::spawn(move || crate::clipboard_history::force_foreground(raw as _));
            }
            #[cfg(target_os = "macos")]
            {
                let _ = window.set_focus();
                LOUPE_FOCUSED.store(true, Ordering::SeqCst);
            }
        } else {
            #[cfg(target_os = "macos")]
            LOUPE_FOCUSED.store(false, Ordering::SeqCst);
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if rose {
            window.set_focus().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn place_loupe_cached(
    _app: &AppHandle,
    cx: i32,
    cy: i32,
    rose: bool,
    monitors: &[atic_capture::monitors::MonitorInfo],
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOZORDER,
    };
    let hwnd = LOUPE_HWND.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let Some(mon) = monitors
        .iter()
        .find(|m| m.bounds.contains(cx, cy))
        .or_else(|| monitors.first())
    else {
        return;
    };
    let (lw, lh) = if rose { ROSE } else { COMPACT };
    let work = mon.work_area;
    let w = ((lw * mon.scale).round() as i32).min(work.width as i32);
    let h = ((lh * mon.scale).round() as i32).min(work.height as i32);
    let pad = (PAD * mon.scale).round() as i32;
    LOUPE_PAD_PX.store(pad, Ordering::SeqCst);
    // `OFFSET` es la distancia del cursor a la GOTA. El aire de la sombra va
    // por dentro de esa cuenta: si no, la lupa se separaría del puntero justo
    // el ancho del margen invisible.
    let offset = ((f64::from(OFFSET) * mon.scale).round() as i32 - pad).max(0);
    let x = if cx + offset + w <= work.right() {
        cx + offset
    } else {
        cx - offset - w
    };
    let y = if cy + offset + h <= work.bottom() {
        cy + offset
    } else {
        cy - offset - h
    };
    let x = x.clamp(work.x, (work.right() - w).max(work.x));
    let y = y.clamp(work.y, (work.bottom() - h).max(work.y));
    unsafe {
        SetWindowPos(
            hwnd as _,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// En Mac todo el espacio global son puntos: la posición y el tamaño van
/// lógicos y la gota se recorta para el hit-test del tap.
#[cfg(target_os = "macos")]
fn place_loupe_cached(
    app: &AppHandle,
    cx: i32,
    cy: i32,
    rose: bool,
    monitors: &[atic_capture::monitors::MonitorInfo],
) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let Some(mon) = monitors
        .iter()
        .find(|m| m.bounds.contains(cx, cy))
        .or_else(|| monitors.first())
    else {
        return;
    };
    let (lw, lh) = if rose { ROSE } else { COMPACT };
    let work = mon.work_area;
    let w = (lw.round() as i32).min(work.width as i32);
    let h = (lh.round() as i32).min(work.height as i32);
    let pad = PAD.round() as i32;
    LOUPE_PAD_PX.store(pad, Ordering::SeqCst);
    let offset = (OFFSET - pad).max(0);
    let x = if cx + offset + w <= work.right() {
        cx + offset
    } else {
        cx - offset - w
    };
    let y = if cy + offset + h <= work.bottom() {
        cy + offset
    } else {
        cy - offset - h
    };
    let x = x.clamp(work.x, (work.right() - w).max(work.x));
    let y = y.clamp(work.y, (work.bottom() - h).max(work.y));
    // Mover y redimensionar son dos escrituras nativas: el tamaño sólo cambia
    // al abrir/cerrar la rosa. Mandarlo en cada movimiento forzaba un resize
    // del webview a 60 Hz y la lupa se sentía pegajosa.
    let size_changed = LOUPE_W.load(Ordering::SeqCst) != w || LOUPE_H.load(Ordering::SeqCst) != h;
    LOUPE_X.store(x, Ordering::SeqCst);
    LOUPE_Y.store(y, Ordering::SeqCst);
    LOUPE_W.store(w, Ordering::SeqCst);
    LOUPE_H.store(h, Ordering::SeqCst);
    if size_changed {
        crate::floating::apply_global_bounds(&window, x, y, w, h);
    } else {
        crate::floating::set_global_position(&window, x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_coordinates_preserve_negative_monitor_origins() {
        for (x, y) in [(0, 0), (-3840, -2160), (1234, -200), (i32::MIN, i32::MAX)] {
            assert_eq!(unpack_position(pack_position(x, y)), (x, y));
        }
    }

    #[test]
    fn accepts_only_valid_color_values_for_the_clipboard() {
        assert_eq!(normalize_color_value(" ff00Aa ").unwrap(), "#FF00AA");
        for valid in ["rgb(0, 128, 255)", "hsl(360, 100%, 50%)"] {
            assert_eq!(normalize_color_value(valid).unwrap(), valid);
        }
        for invalid in [
            "",
            "#fff",
            "rgb(nope)",
            "rgb(256,0,0)",
            "rgb(NaN,0,0)",
            "hsl(20,50,50)",
            "hsl(0,101%,0%)",
        ] {
            assert!(normalize_color_value(invalid).is_err(), "{invalid}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn consumes_both_button_releases_even_after_session_ended() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WM_LBUTTONUP, WM_RBUTTONUP};
        RUNNING.store(false, Ordering::SeqCst);
        EAT_LEFT_UP.store(true, Ordering::SeqCst);
        EAT_RIGHT_UP.store(true, Ordering::SeqCst);
        // No active session/window or hook is needed: paired releases must be
        // handled before the early return and before dereferencing mouse data.
        assert_eq!(unsafe { click_hook(0, WM_LBUTTONUP as usize, 0) }, 1);
        assert_eq!(unsafe { click_hook(0, WM_RBUTTONUP as usize, 0) }, 1);
        assert!(!EAT_LEFT_UP.load(Ordering::SeqCst));
        assert!(!EAT_RIGHT_UP.load(Ordering::SeqCst));
    }

    #[cfg(windows)]
    #[test]
    fn consumes_shortcut_releases_even_after_session_ended() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, WM_KEYUP};
        RUNNING.store(false, Ordering::SeqCst);
        EAT_ESC_UP.store(true, Ordering::SeqCst);
        EAT_ENTER_UP.store(true, Ordering::SeqCst);
        EAT_R_UP.store(true, Ordering::SeqCst);
        let eat_up = |vk: u32| {
            let info = KBDLLHOOKSTRUCT {
                vkCode: vk,
                scanCode: 0,
                flags: 0,
                time: 0,
                dwExtraInfo: 0,
            };
            unsafe { key_hook(0, WM_KEYUP as usize, &info as *const _ as isize) }
        };
        assert_eq!(eat_up(0x1B), 1);
        assert_eq!(eat_up(0x0D), 1);
        assert_eq!(eat_up(0x52), 1);
        assert!(!EAT_ESC_UP.load(Ordering::SeqCst));
        assert!(!EAT_ENTER_UP.load(Ordering::SeqCst));
        assert!(!EAT_R_UP.load(Ordering::SeqCst));
    }

    #[cfg(windows)]
    #[test]
    fn swallowed_shortcuts_signal_the_picker_once() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, WM_KEYDOWN};
        RUNNING.store(true, Ordering::SeqCst);
        LOUPE_HWND.store(0, Ordering::SeqCst);
        WANT_CANCEL.store(false, Ordering::SeqCst);
        WANT_ENTER_COMMIT.store(false, Ordering::SeqCst);
        WANT_TOGGLE_ROSE.store(false, Ordering::SeqCst);
        EAT_ESC_UP.store(false, Ordering::SeqCst);
        EAT_ENTER_UP.store(false, Ordering::SeqCst);
        EAT_R_UP.store(false, Ordering::SeqCst);
        let eat_down = |vk: u32| {
            let info = KBDLLHOOKSTRUCT {
                vkCode: vk,
                scanCode: 0,
                flags: 0,
                time: 0,
                dwExtraInfo: 0,
            };
            unsafe { key_hook(0, WM_KEYDOWN as usize, &info as *const _ as isize) }
        };
        assert_eq!(eat_down(0x1B), 1);
        assert!(WANT_CANCEL.swap(false, Ordering::SeqCst));
        assert_eq!(eat_down(0x1B), 1);
        assert!(!WANT_CANCEL.load(Ordering::SeqCst));
        assert_eq!(eat_down(0x0D), 1);
        assert!(WANT_ENTER_COMMIT.swap(false, Ordering::SeqCst));
        assert_eq!(eat_down(0x0D), 1);
        assert!(!WANT_ENTER_COMMIT.load(Ordering::SeqCst));
        assert_eq!(eat_down(0x52), 1);
        assert!(WANT_TOGGLE_ROSE.swap(false, Ordering::SeqCst));
        assert_eq!(eat_down(0x52), 1);
        assert!(!WANT_TOGGLE_ROSE.load(Ordering::SeqCst));
        RUNNING.store(false, Ordering::SeqCst);
        WANT_CANCEL.store(false, Ordering::SeqCst);
        WANT_ENTER_COMMIT.store(false, Ordering::SeqCst);
        WANT_TOGGLE_ROSE.store(false, Ordering::SeqCst);
        EAT_ESC_UP.store(false, Ordering::SeqCst);
        EAT_ENTER_UP.store(false, Ordering::SeqCst);
        EAT_R_UP.store(false, Ordering::SeqCst);
    }
}
