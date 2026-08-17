//! Sesión de captura con overlay de selección (Fase 2).
//!
//! Flujo «congelar primero»: al abrir, se congela todo el escritorio virtual a
//! memoria y a un PNG temporal, y se crea UNA ventana overlay **opaca** que
//! cubre el escritorio virtual mostrando ese frame. El usuario selecciona una
//! ventana (clic), una región (arrastre) o un monitor (Espacio). La captura se
//! recorta del frame congelado (región/monitor) o se re-renderiza con
//! `PrintWindow` (ventana), de modo que el overlay nunca aparece en el
//! resultado.
//!
//! El overlay es opaco a propósito: las ventanas transparentes de WebView2
//! hacen crashear a wry en `WM_SETFOCUS` al recibir un clic.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use atic_core::MutexExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const OVERLAY_LABEL: &str = "capture-overlay";

/// `start_impl` está congelando el escritorio (aún sin sesión activa).
/// Sin esto, un segundo atajo rápido abre otra captura en paralelo y un
/// `show()` tardío deja la ventana gris tapando el escritorio sin sesión.
static STARTING: AtomicBool = AtomicBool::new(false);
/// Tick (ms) en que empezó el arranque actual. Sirve para no cancelar por un
/// segundo Pressed rebotado mientras el primero todavía congela.
static START_TICK_MS: AtomicU64 = AtomicU64::new(0);
/// Sube en cada cancelación. El arranque en curso aborta si su token no coincide.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// La mira confirmó estar pintada y usable (ack del webview de captura).
///
/// Es el único hecho que distingue «la selección está en pantalla» de «la
/// ventana está visible pero el webview quedó en blanco». `is_visible()` no
/// sirve para eso: la pone en `true` este mismo módulo al llamar `show()`.
static REVEALED: AtomicBool = AtomicBool::new(false);
/// Ventana en la que un segundo atajo durante `STARTING` solo reafirma input
/// en vez de cancelar (tap rápido / evento duplicado). Después sí cancela.
const START_GRACE_MS: u64 = 400;

#[cfg(windows)]
pub struct OverlaySession {
    /// Frame congelado de todo el escritorio virtual (coords físicas).
    frame: atic_capture::Frame,
    /// Ventanas candidatas (coords físicas globales), z-order topmost-first.
    candidates: Vec<atic_capture::windows::WindowCandidate>,
    /// Monitores, para la selección de monitor completo.
    monitors: Vec<atic_capture::monitors::MonitorInfo>,
    /// PNG temporal del frame congelado (se borra al terminar).
    frame_path: std::path::PathBuf,
}

#[cfg(not(windows))]
pub struct OverlaySession;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCandidate {
    /// `HWND` como entero (cabe en el rango seguro de JS).
    pub hwnd: i64,
    pub title: String,
    // Coordenadas LÓGICAS locales al overlay (px CSS).
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    /// Ruta absoluta del PNG congelado (el frontend la pasa por convertFileSrc).
    pub frame_path: String,
    /// Tamaño lógico del overlay (px CSS).
    pub width: f64,
    pub height: f64,
    pub candidates: Vec<OverlayCandidate>,
}

#[tauri::command]
pub fn start_capture_session(app: AppHandle) -> Result<(), String> {
    start_impl(&app)
}

/// Disparador para el atajo global y el tray (no pasa por `invoke`).
///
/// - Sesión ya abierta → cancela (toggle).
/// - Arranque en curso dentro de la gracia → no cancela: reafirma click-through.
///   Un tap muy corto no debe tumbar el freeze del primer Pressed.
/// - Arranque colgado pasado la gracia → cancela.
/// - Idle → arranca.
pub fn trigger(app: &AppHandle) -> Result<(), String> {
    if session_is_active(app) {
        end_session(app);
        return Ok(());
    }
    if STARTING.load(Ordering::SeqCst) {
        let started = START_TICK_MS.load(Ordering::SeqCst);
        let now = now_ms();
        if now.saturating_sub(started) < START_GRACE_MS {
            crate::overlay::reassert_capturing_input(app);
            return Ok(());
        }
        end_session(app);
        return Ok(());
    }
    start_impl(app)
}

fn now_ms() -> u64 {
    #[cfg(windows)]
    {
        // SAFETY: GetTickCount64 no tiene precondiciones.
        unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
    }
    #[cfg(not(windows))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

fn session_is_active(app: &AppHandle) -> bool {
    app.try_state::<crate::state::AppState>()
        .is_some_and(|state| state.overlay_session.lock_or_recover().is_some())
}

fn abort_requested(token: u64) -> bool {
    GENERATION.load(Ordering::SeqCst) != token
}

#[tauri::command]
pub fn overlay_info(app: AppHandle) -> Result<OverlayInfo, String> {
    overlay_info_impl(&app)
}

#[tauri::command]
pub fn complete_window_capture(app: AppHandle, hwnd: i64) -> Result<String, String> {
    let (path, anchor) = window_capture_impl(&app, hwnd)?;
    finish(&app, &path, anchor);
    Ok(path)
}

#[tauri::command]
pub fn complete_region_capture(
    app: AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<String, String> {
    let (path, anchor) = region_capture_impl(&app, left, top, width, height)?;
    finish(&app, &path, anchor);
    Ok(path)
}

#[tauri::command]
pub fn complete_monitor_capture(app: AppHandle, x: f64, y: f64) -> Result<String, String> {
    let (path, anchor) = monitor_capture_impl(&app, x, y)?;
    finish(&app, &path, anchor);
    Ok(path)
}

#[tauri::command]
pub fn cancel_capture_session(app: AppHandle) {
    end_session(&app);
}

/// Cierra el overlay, copia al portapapeles, muestra el shelf y notifica.
fn finish(app: &AppHandle, path: &str, shelf_anchor: (i32, i32)) {
    end_session(app);
    crate::capture::notify_capture_ready(app, path, Some(shelf_anchor));
}

fn end_session(app: &AppHandle) {
    // Invalida cualquier `start_impl` en vuelo antes de ocultar/limpiar.
    GENERATION.fetch_add(1, Ordering::SeqCst);
    STARTING.store(false, Ordering::SeqCst);
    START_TICK_MS.store(0, Ordering::SeqCst);
    REVEALED.store(false, Ordering::SeqCst);

    // Ocultar (no cerrar): destruir la ventana provoca un crash de wry cuando
    // recibe WM_SETFOCUS durante su destrucción. Se reutiliza en la próxima
    // sesión.
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.hide();
    }
    // La main dejó de robar hover al mostrar el overlay; devolver hit-testing.
    restore_main_hit_testing(app);
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        let taken = state.overlay_session.lock_or_recover().take();
        end_session_cleanup(taken);
    }
    crate::overlay::set_capturing(app, false);
    let _ = app.emit("overlay-session-ended", ());
}

#[cfg(windows)]
fn end_session_cleanup(session: Option<OverlaySession>) {
    if let Some(session) = session {
        let _ = std::fs::remove_file(&session.frame_path);
    }
}

#[cfg(not(windows))]
fn end_session_cleanup(_session: Option<OverlaySession>) {}

// ---------------------------------------------------------------------------
// Implementación Windows
// ---------------------------------------------------------------------------

/// Suelta `STARTING`/`CAPTURING` si el arranque abortó sin llegar a sesión.
fn abandon_start(app: &AppHandle) {
    STARTING.store(false, Ordering::SeqCst);
    START_TICK_MS.store(0, Ordering::SeqCst);
    if !session_is_active(app) {
        crate::overlay::set_capturing(app, false);
    }
}

#[cfg(windows)]
fn start_impl(app: &AppHandle) -> Result<(), String> {
    // Solo una sesión: si ya hay overlay, cancelar (mismo criterio que el atajo).
    if session_is_active(app) {
        end_session(app);
        return Ok(());
    }
    // Otro arranque en curso: cancelar ese (toggle), no apilar otro.
    // (El atajo usa gracia en `trigger`; este camino es el comando IPC.)
    if STARTING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        end_session(app);
        return Ok(());
    }
    START_TICK_MS.store(now_ms(), Ordering::SeqCst);
    // Arranca sin ack: el de la sesión anterior no vale para esta.
    REVEALED.store(false, Ordering::SeqCst);

    let token = GENERATION.load(Ordering::SeqCst);

    // Click-through YA, en el hilo del atajo, ANTES del freeze. Si el freeze
    // corre en este mismo hilo, cualquier PostMessage/estilo en cola no aplica
    // hasta terminar — y un tap no espera. Hold solo daba tiempo de sobra.
    crate::overlay::set_capturing(app, true);

    let app_bg = app.clone();
    if std::thread::Builder::new()
        .name("atic-capture-start".into())
        .spawn(move || {
            if let Err(error) = start_freeze(&app_bg, token) {
                tracing::warn!(%error, "no se pudo abrir el overlay de captura");
                abandon_start(&app_bg);
            }
        })
        .is_err()
    {
        abandon_start(app);
        return Err("no se pudo iniciar la captura en segundo plano".into());
    }
    Ok(())
}

#[cfg(windows)]
fn start_freeze(app: &AppHandle, token: u64) -> Result<(), String> {
    use atic_capture::{engine, monitors, windows as capwin};

    let state = app.state::<crate::state::AppState>();
    let include_cursor = state.config.lock_or_recover().capture_include_cursor;

    // Si un intento anterior dejó el overlay visible (telón #111), BitBlt lo
    // congela como “escritorio” gris. Ocultar y dar un frame a DWM.
    ensure_overlay_hidden(app);
    std::thread::sleep(std::time::Duration::from_millis(32));
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    if let Err(err) = ensure_capture_overlay(app) {
        abandon_start(app);
        return Err(err);
    }
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    let virtual_screen = monitors::virtual_screen();
    let mut frame = match engine::capture_rect(virtual_screen, include_cursor) {
        Ok(frame) => frame,
        Err(error) => {
            abandon_start(app);
            return Err(error.to_string());
        }
    };
    // BitBlt no ve el overlay layered (pill, launcher). Recortes de Windows sí
    // porque usa Graphics Capture. Pedirle el bitmap a WebView2 y blendear.
    let mut composed = false;
    if let Some(mut overlay) = crate::overlay::capture_layer(app) {
        if overlay.prepare_overlay_layer() {
            frame.blend_over(&overlay);
            composed = true;
            tracing::info!(target: "overlay", "pill/launcher compuestos en la captura");
        }
    }
    if !composed {
        if let Some(hwnd) = crate::overlay::hwnd() {
            match engine::print_window_tree(hwnd) {
                Ok(Some(mut overlay)) => {
                    if overlay.prepare_overlay_layer() {
                        frame.blend_over(&overlay);
                        tracing::info!(target: "overlay", "overlay compuesto vía PrintWindow");
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        target: "overlay",
                        "la captura no incluye la pill: WebView2 y PrintWindow salieron vacíos"
                    );
                }
                Err(err) => {
                    tracing::warn!(%err, "no se pudo imprimir el overlay en la captura");
                }
            }
        }
    }
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    let png = match frame.to_png() {
        Ok(png) => png,
        Err(error) => {
            abandon_start(app);
            return Err(error.to_string());
        }
    };
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    let frame_path = state.dirs.overlay_frames_dir().join("overlay.png");
    if let Err(error) = std::fs::write(&frame_path, &png) {
        abandon_start(app);
        return Err(error.to_string());
    }

    let monitors = monitors::enumerate();
    let candidates = capwin::enumerate_candidates(std::process::id(), &monitors);

    {
        let mut guard = state.overlay_session.lock_or_recover();
        if abort_requested(token) {
            abandon_start(app);
            drop(guard);
            let _ = std::fs::remove_file(&frame_path);
            return Ok(());
        }
        *guard = Some(OverlaySession {
            frame,
            candidates,
            monitors,
            frame_path,
        });
        STARTING.store(false, Ordering::SeqCst);
        START_TICK_MS.store(0, Ordering::SeqCst);
    }

    if abort_requested(token) {
        end_session(app);
        return Ok(());
    }

    // CAPTURING ya está activo desde el primer Pressed; reafirmar por si el
    // freeze/blur pisó el ex-style mientras tanto.
    crate::overlay::reassert_capturing_input(app);
    // Mostrar YA: el webview de captura nace oculto y Chromium se duerme.
    // Si esperamos a que el frontend reciba el evento y llame a `show`,
    // a veces no llega nunca: la mira no aparece y la pill queda click-through.
    let app_show = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(show_overlay_window(&app_show));
    })
    .map_err(|err| err.to_string())?;
    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            tracing::warn!(%err, "no se pudo mostrar el overlay de captura");
            end_session(app);
            return Err(err);
        }
        Err(_) => {
            end_session(app);
            return Err("timeout mostrando capture-overlay".into());
        }
    }
    let _ = app.emit("overlay-session-started", ());
    schedule_show_watchdog(app.clone(), token);
    Ok(())
}

/// Si la mira no confirma que está en pantalla, soltar el mouse.
///
/// Antes esto preguntaba `window.is_visible()`, que no puede fallar que sí:
/// `start_freeze` llama `show()` sobre esa misma ventana unas líneas más
/// arriba, así que siempre daba `true` y el watchdog no cancelaba nunca. El
/// caso a atrapar es justamente el otro —ventana visible con el webview en
/// blanco—: la mira no aparece, nadie completa ni cancela, la sesión queda
/// abierta y la pill se queda click-through porque `CAPTURING` sigue puesto.
/// El síntoma es «la captura no hace nada y la pill deja de responder hasta
/// apretar el atajo de nuevo» (el segundo disparo entra por el toggle de
/// `start_impl` y cierra la sesión).
///
/// Corre 1 s DESPUÉS del watchdog del propio overlay (`WATCHDOG_MS` = 5 s en
/// `CaptureOverlaySurface`): si ese webview está vivo se cancela solo y con
/// mejor contexto. Este es el backstop para cuando ni siquiera llegó a
/// escuchar `overlay-session-started`, que es precisamente cuando el de allá
/// no existe.
const SHOW_WATCHDOG_SECS: u64 = 6;

fn schedule_show_watchdog(app: AppHandle, token: u64) {
    std::thread::Builder::new()
        .name("atic-capture-watch".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(SHOW_WATCHDOG_SECS));
            if abort_requested(token) || !session_is_active(&app) {
                return;
            }
            if REVEALED.load(Ordering::SeqCst) {
                return;
            }
            tracing::warn!(
                "captura: la mira nunca confirmó estar visible; cancelando para \
                 devolver el mouse a la pill"
            );
            end_session(&app);
        })
        .ok();
}

/// Muestra el overlay solo cuando el frontend ya tiene el frame listo.
#[tauri::command]
pub fn show_capture_overlay(app: AppHandle) -> Result<(), String> {
    show_overlay_window(&app)
}

/// El webview de captura avisa que la mira ya está visible y usable.
///
/// Sin este ack, Rust no tiene forma de saber que la selección arrancó: es lo
/// que apaga el watchdog de [`schedule_show_watchdog`].
#[tauri::command]
pub fn capture_overlay_revealed() {
    REVEALED.store(true, Ordering::SeqCst);
}

fn ensure_overlay_hidden(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.hide();
    }
}

/// ¿Esta ventana tiene un WebView2 vivo detrás?
///
/// `eval` no sirve como sondeo: encola el script y devuelve `Ok` aunque la
/// creación del webview haya fallado, así que decía «vivo» sobre una ventana
/// hueca. Por eso el camino de recrear de abajo nunca corría: en los logs
/// `"sin Chromium"` aparecía 0 veces mientras la captura fallaba en TODOS los
/// arranques.
///
/// Se piden dos hechos en vez de uno: que Tauri tenga el webview registrado
/// (`with_webview` falla si no) y que exista el hijo `WRY_WEBVIEW`, que es el
/// HWND que hospeda al controller. Un falso «muerto» solo cuesta recrear la
/// ventana, y queda escrito en el log de quien llama.
fn capture_webview_alive(window: &tauri::WebviewWindow) -> bool {
    if window.with_webview(|_| {}).is_err() {
        return false;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW;
        let Ok(hwnd) = window.hwnd() else {
            return false;
        };
        let mut class: Vec<u16> = "WRY_WEBVIEW".encode_utf16().collect();
        class.push(0);
        // SAFETY: el HWND lo da Tauri y vive mientras viva la ventana;
        // FindWindowEx solo consulta la jerarquía.
        let child = unsafe {
            FindWindowExW(
                hwnd.0 as _,
                std::ptr::null_mut(),
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if child.is_null() {
            return false;
        }
    }
    true
}

/// Tamaño/origen lógicos del escritorio virtual.
///
/// Misma cuenta que el overlay de la pill: WebView2 se queda con el
/// `inner_size` del create. Si nace a 800×600, en dos monitores la mira queda
/// como una sola pantalla, a menudo centrada entre las dos.
#[cfg(windows)]
fn virtual_screen_logical() -> (f64, f64, f64, f64) {
    let vs = atic_capture::monitors::virtual_screen();
    let scale = atic_capture::monitors::enumerate()
        .iter()
        .map(|m| m.scale)
        .fold(1.0_f64, f64::max)
        .max(0.01);
    (
        f64::from(vs.x) / scale,
        f64::from(vs.y) / scale,
        (f64::from(vs.width) / scale).max(1.0),
        (f64::from(vs.height) / scale).max(1.0),
    )
}

/// WndProc anterior del overlay de captura (la de tao). Encadenamos para no
/// comerse el resto de mensajes.
#[cfg(windows)]
static CAPTURE_OVERLAY_WNDPROC: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
type WndProcFn = unsafe extern "system" fn(
    windows_sys::Win32::Foundation::HWND,
    u32,
    windows_sys::Win32::Foundation::WPARAM,
    windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT;

#[cfg(windows)]
fn orig_capture_wndproc() -> Option<WndProcFn> {
    let orig = CAPTURE_OVERLAY_WNDPROC.load(Ordering::SeqCst);
    if orig == 0 {
        None
    } else {
        // SAFETY: el valor lo guardó `SetWindowLongPtrW` como WndProc de tao.
        Some(unsafe { std::mem::transmute::<isize, WndProcFn>(orig) })
    }
}

/// Windows limita `SetWindowPos` al monitor actual vía `WM_GETMINMAXINFO`.
/// Sin esto, un overlay que pretende cubrir el escritorio virtual se recorta
/// a una sola pantalla y DWM lo recentra entre los dos monitores.
#[cfg(windows)]
unsafe extern "system" fn capture_overlay_wndproc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, MINMAXINFO, WM_GETMINMAXINFO,
    };

    if msg == WM_GETMINMAXINFO && lparam != 0 {
        if let Some(orig_fn) = orig_capture_wndproc() {
            let _ = CallWindowProcW(Some(orig_fn), hwnd, msg, wparam, lparam);
        }
        let mmi = lparam as *mut MINMAXINFO;
        let vs = atic_capture::monitors::virtual_screen();
        // Margen para el no-cliente (borde DWM) al hacer coincidir el cliente
        // con el escritorio virtual.
        let max_w = vs.width as i32 + 64;
        let max_h = vs.height as i32 + 64;
        (*mmi).ptMaxSize.x = max_w;
        (*mmi).ptMaxSize.y = max_h;
        (*mmi).ptMaxPosition.x = vs.x;
        (*mmi).ptMaxPosition.y = vs.y;
        (*mmi).ptMaxTrackSize.x = max_w;
        (*mmi).ptMaxTrackSize.y = max_h;
        return 0;
    }

    match orig_capture_wndproc() {
        Some(orig_fn) => CallWindowProcW(Some(orig_fn), hwnd, msg, wparam, lparam),
        None => 0,
    }
}

#[cfg(windows)]
fn install_virtual_screen_limit(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWLP_WNDPROC,
    };

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    let ours = capture_overlay_wndproc as *const () as isize;
    // SAFETY: HWND de Tauri vivo; Get/SetWindowLongPtr solo leen/escriben el
    // puntero a WndProc. Encadenamos la de tao para no romper el resto.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if current == ours {
            return;
        }
        CAPTURE_OVERLAY_WNDPROC.store(current, Ordering::SeqCst);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours);
    }
}

/// Coloca el cliente del overlay sobre TODO el escritorio virtual.
///
/// Un `set_position` + `set_size` son dos `SetWindowPos`: DWM puede recortar
/// el tamaño a un monitor y recentrar la ventana entre medio. Acá va posición
/// y tamaño juntos, y después se corrige el desfase de no-cliente (borde DWM)
/// para que el (0,0) del CSS coincida con el del PNG congelado.
#[cfg(windows)]
fn cover_virtual_desktop(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };

    install_virtual_screen_limit(window);

    let vs = atic_capture::monitors::virtual_screen();
    // Más que cualquier escritorio virtual razonable. Si se iguala al monitor
    // actual, Windows vuelve a recortar la ventana a una sola pantalla.
    let _ = window.set_max_size(Some(tauri::Size::Physical(tauri::PhysicalSize::new(
        16384, 16384,
    ))));

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;

    // SAFETY: HWND de Tauri vivo; SetWindowPos/GetWindowRect/ClientToScreen
    // no retienen el handle.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            vs.x,
            vs.y,
            vs.width as i32,
            vs.height as i32,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
        );

        let mut origin = POINT { x: 0, y: 0 };
        let mut client = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut outer = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if ClientToScreen(hwnd, &mut origin) == 0
            || GetClientRect(hwnd, &mut client) == 0
            || GetWindowRect(hwnd, &mut outer) == 0
        {
            crate::webview_tweaks::sync_controller_bounds(window);
            return;
        }

        let client_w = client.right - client.left;
        let client_h = client.bottom - client.top;
        let dx = vs.x - origin.x;
        let dy = vs.y - origin.y;
        let dw = vs.width as i32 - client_w;
        let dh = vs.height as i32 - client_h;
        if dx != 0 || dy != 0 || dw != 0 || dh != 0 {
            tracing::info!(
                target: "overlay",
                dx,
                dy,
                dw,
                dh,
                vs_x = vs.x,
                vs_y = vs.y,
                vs_w = vs.width,
                vs_h = vs.height,
                "capture-overlay desfasado del escritorio virtual; se corrige"
            );
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                outer.left + dx,
                outer.top + dy,
                (outer.right - outer.left) + dw,
                (outer.bottom - outer.top) + dh,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
            );
        }
    }

    crate::webview_tweaks::sync_controller_bounds(window);
}

fn create_capture_overlay(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    #[cfg(windows)]
    let (lx, ly, lw, lh) = virtual_screen_logical();
    #[cfg(not(windows))]
    let (lx, ly, lw, lh) = (0.0, 0.0, 800.0, 600.0);

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        tauri::WebviewUrl::App("capture-overlay".into()),
    )
    .title("Seleccionar captura")
    .inner_size(lw, lh)
    .max_inner_size(16384.0, 16384.0)
    .position(lx, ly)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false);
    if let Ok(dir) = app.path().app_local_data_dir() {
        builder = builder.data_directory(dir.join("capture-overlay-webview"));
    }
    #[cfg(windows)]
    {
        builder = builder.additional_browser_args(
            "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion",
        );
    }
    let window = builder
        .build()
        .map_err(|err| format!("no se pudo crear capture-overlay: {err}"))?;
    #[cfg(windows)]
    {
        install_virtual_screen_limit(&window);
        crate::webview_tweaks::disable_browser_accelerator_keys(&window);
        cover_virtual_desktop(&window);
    }
    Ok(window)
}

/// Garantiza una ventana de captura con webview usable, creándola si hace falta.
///
/// **Por qué no se declara en `tauri.conf.json`.** Estaba ahí, y era la única
/// de las cuatro ventanas con `additionalBrowserArgs`. WebView2 tiene un solo
/// environment por carpeta de user-data y sus opciones las fija quien lo crea
/// primero —`main`, sin esos flags—, así que al llegarle el turno rechazaba las
/// opciones distintas:
///
/// ```text
/// ERROR tauri_runtime_wry: failed to create webview: HRESULT(0x8007139F)
/// "The group or resource is not in the correct state to perform the requested operation."
/// ```
///
/// Pasaba en el 100% de los arranques. La ventana existía pero hueca: sin JS no
/// escuchaba `overlay-session-started`, la mira no aparecía, nadie completaba ni
/// cancelaba, y como `CAPTURING` seguía puesto la pill se quedaba
/// click-through hasta apretar el atajo otra vez (que entra por el toggle de
/// `start_impl` y recién ahí cierra la sesión).
///
/// Creándola acá, `create_capture_overlay` le da su propio `data_directory`:
/// environment aparte, los flags anti-throttling sí se aplican, y la primera
/// captura paga el costo de crearla.
fn ensure_capture_overlay(app: &AppHandle) -> Result<(), String> {
    let app_probe = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let verdict = match app_probe.get_webview_window(OVERLAY_LABEL) {
            Some(window) if capture_webview_alive(&window) => 0u8,
            Some(window) => {
                tracing::warn!(
                    target: "overlay",
                    "capture-overlay sin Chromium; recreando"
                );
                let _ = window.destroy();
                1u8
            }
            None => 2u8,
        };
        let _ = tx.send(verdict);
    })
    .map_err(|err| err.to_string())?;
    let verdict = rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap_or(2);
    if verdict == 0 {
        return Ok(());
    }
    if verdict == 1 {
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    let app_create = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let result = if app_create.get_webview_window(OVERLAY_LABEL).is_some() {
            Ok(())
        } else {
            create_capture_overlay(&app_create).map(|_| ())
        };
        let _ = tx.send(result);
    })
    .map_err(|err| err.to_string())?;
    rx.recv_timeout(std::time::Duration::from_secs(8))
        .map_err(|_| "timeout creando capture-overlay".to_string())?
}

fn restore_main_hit_testing(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_ignore_cursor_events(false);
    }
}

#[cfg(windows)]
fn show_overlay_window(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::state::AppState>();
    {
        let guard = state.overlay_session.lock_or_recover();
        if guard.as_ref().is_none() {
            // Cancelaron mientras el PNG cargaba: no mostrar el telón.
            return Ok(());
        }
    }

    let window = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("la ventana del overlay no existe")?;
    let _ = window.set_decorations(false);
    // Por si una sesión anterior dejó el overlay como click-through.
    let _ = window.set_ignore_cursor_events(false);
    // Tamaño/posición ANTES del show, en un solo SetWindowPos: si no, Windows
    // recorta a un monitor y DWM centra la ventana entre las dos pantallas.
    cover_virtual_desktop(&window);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    cover_virtual_desktop(&window);

    // Misma app, mismo proceso: con el cursor sobre `main`, Windows puede
    // seguir entregándole el hover aunque el overlay sea topmost. Mientras
    // dura la selección, `main` no debe participar del hit-testing.
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_ignore_cursor_events(true);
    }

    // La pill también es always-on-top: reafirmar click-through y subir el
    // overlay de captura al frente de la banda topmost. Con retry: el freeze
    // ya consumió los timers del primer Pressed.
    crate::overlay::reassert_capturing_input_with_retry(app);
    raise_capture_overlay(&window);
    cover_virtual_desktop(&window);
    schedule_cover_retries(app);

    // No usar `set_focus()`: si `SetForegroundWindow` falla, tao inyecta Alt y
    // esta app también usa SendInput para pegar. `force_foreground` activa sin
    // teclas fantasma — hace falta para Esc y para que WebView2 reciba el mouse
    // sin un clic previo “fuera” de la main.
    if let Ok(hwnd) = window.hwnd() {
        let raw = hwnd.0 as isize;
        let app_bg = app.clone();
        std::thread::spawn(move || {
            crate::clipboard_history::force_foreground(raw as _);
            // El blur del overlay (salida del modo texto) llega DESPUÉS y
            // `set_focusable(false)` puede haber pisado el click-through otra
            // vez; reafirmar cuando el primer plano ya cambió.
            crate::overlay::reassert_capturing_input(&app_bg);
        });
    }
    Ok(())
}

/// WebView2 termina de nacer después del `show`: sin repetir, se queda en el
/// recuadro del create (una pantalla, centrada).
#[cfg(windows)]
fn schedule_cover_retries(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("atic-capture-cover".into())
        .spawn(move || {
            for ms in [16u64, 50, 200, 500] {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                if !session_is_active(&app) {
                    return;
                }
                let app_cover = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if !session_is_active(&app_cover) {
                        return;
                    }
                    if let Some(window) = app_cover.get_webview_window(OVERLAY_LABEL) {
                        cover_virtual_desktop(&window);
                    }
                });
            }
        })
        .ok();
}

/// Sube el overlay de captura al frente de las always-on-top (pill, launcher…).
#[cfg(windows)]
fn raise_capture_overlay(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // SAFETY: HWND lo da Tauri y vive mientras viva la ventana.
    unsafe {
        SetWindowPos(
            hwnd.0 as _,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

#[cfg(not(windows))]
fn show_overlay_window(_app: &AppHandle) -> Result<(), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(windows)]
fn overlay_info_impl(app: &AppHandle) -> Result<OverlayInfo, String> {
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    let scale = overlay_scale(app);
    let bounds = session.frame.bounds;

    let candidates = session
        .candidates
        .iter()
        .map(|candidate| {
            let visual = candidate.visual_bounds;
            OverlayCandidate {
                hwnd: candidate.hwnd as i64,
                title: candidate.title.clone(),
                left: f64::from(visual.x - bounds.x) / scale,
                top: f64::from(visual.y - bounds.y) / scale,
                width: f64::from(visual.width) / scale,
                height: f64::from(visual.height) / scale,
            }
        })
        .collect();

    Ok(OverlayInfo {
        frame_path: session.frame_path.to_string_lossy().into_owned(),
        width: f64::from(bounds.width) / scale,
        height: f64::from(bounds.height) / scale,
        candidates,
    })
}

#[cfg(windows)]
fn window_capture_impl(app: &AppHandle, hwnd: i64) -> Result<(String, (i32, i32)), String> {
    use atic_capture::{engine, windows as capwin};

    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    // PrintWindow renderiza solo la ventana; si falla/negro, recorta del frame
    // congelado (nunca de la pantalla, para no capturar el overlay).
    let frame = match engine::print_window(hwnd as isize).map_err(|e| e.to_string())? {
        Some(frame) => frame,
        None => {
            let win_bounds = capwin::window_bounds(hwnd as isize).ok_or("ventana sin límites")?;
            session
                .frame
                .crop(win_bounds)
                .ok_or("la ventana quedó fuera del área capturada")?
        }
    };
    drop(guard);
    save_capture(app, &frame)
}

#[cfg(windows)]
fn region_capture_impl(
    app: &AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<(String, (i32, i32)), String> {
    use atic_capture::Rect;

    let scale = overlay_scale(app);
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;
    let bounds = session.frame.bounds;

    let region = Rect::new(
        bounds.x + (left * scale).round() as i32,
        bounds.y + (top * scale).round() as i32,
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    let frame = session
        .frame
        .crop(region)
        .ok_or("la región quedó fuera del área capturada")?;
    drop(guard);
    save_capture(app, &frame)
}

#[cfg(windows)]
fn monitor_capture_impl(app: &AppHandle, x: f64, y: f64) -> Result<(String, (i32, i32)), String> {
    let scale = overlay_scale(app);
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;
    let bounds = session.frame.bounds;

    let point_x = bounds.x + (x * scale).round() as i32;
    let point_y = bounds.y + (y * scale).round() as i32;
    let monitor = session
        .monitors
        .iter()
        .find(|monitor| monitor.bounds.contains(point_x, point_y))
        .or_else(|| session.monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| session.monitors.first())
        .ok_or("no se encontró el monitor")?;
    let frame = session
        .frame
        .crop(monitor.bounds)
        .ok_or("el monitor quedó fuera del área capturada")?;
    drop(guard);
    save_capture(app, &frame)
}

#[cfg(windows)]
fn save_capture(
    app: &AppHandle,
    frame: &atic_capture::Frame,
) -> Result<(String, (i32, i32)), String> {
    use atic_capture::naming;
    let state = app.state::<crate::state::AppState>();
    let png = frame.to_png().map_err(|e| e.to_string())?;
    let dir = state.dirs.captures_dir();
    let path = dir.join(naming::unique_capture_filename(&dir));
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    let anchor = (
        frame.bounds.x + frame.bounds.width as i32 / 2,
        frame.bounds.y + frame.bounds.height as i32 / 2,
    );
    Ok((path.to_string_lossy().into_owned(), anchor))
}

#[cfg(windows)]
fn overlay_scale(app: &AppHandle) -> f64 {
    app.get_webview_window(OVERLAY_LABEL)
        .and_then(|window| window.scale_factor().ok())
        .unwrap_or(1.0)
}

// ---------------------------------------------------------------------------
// Stubs para plataformas no-Windows (la captura solo existe en Windows).
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn start_impl(_app: &AppHandle) -> Result<(), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn overlay_info_impl(_app: &AppHandle) -> Result<OverlayInfo, String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn window_capture_impl(_app: &AppHandle, _hwnd: i64) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn region_capture_impl(
    _app: &AppHandle,
    _left: f64,
    _top: f64,
    _width: f64,
    _height: f64,
) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}

#[cfg(not(windows))]
fn monitor_capture_impl(
    _app: &AppHandle,
    _x: f64,
    _y: f64,
) -> Result<(String, (i32, i32)), String> {
    Err("La captura de pantalla solo está disponible en Windows.".into())
}
