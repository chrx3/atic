//! Sesión de captura con overlay de selección (Fase 2).
//!
//! Flujo «congelar primero»: al abrir, se congela todo el escritorio virtual a
//! memoria y a un PNG temporal, y se crea UNA ventana overlay **opaca** que
//! cubre el escritorio virtual mostrando ese frame. El usuario selecciona una
//! ventana (clic), una región (arrastre) o un monitor (Espacio). La captura se
//! recorta del frame congelado (región/monitor) o se re-renderiza con
//! `PrintWindow` / `CGWindowListCreateImage` (ventana), de modo que el overlay
//! nunca aparece en el resultado.
//!
//! El overlay es opaco a propósito: las ventanas transparentes de WebView2
//! hacen crashear a wry en `WM_SETFOCUS` al recibir un clic.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use atic_core::MutexExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const OVERLAY_LABEL: &str = "capture-overlay";
/// Tras este idle sin uso, se destruye el webview precalentado para soltar RAM.
const CAPTURE_IDLE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Mismo umbral que `CAPTURE_IDLE_TTL`: captura e input deben quedar idle juntos.
const CAPTURE_INPUT_IDLE: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const CAPTURE_IDLE_CHECK: std::time::Duration = std::time::Duration::from_secs(60);
/// La creación en frío puede tardar varios segundos bajo carga; esto es red de
/// seguridad contra una creación trabada, no un timeout del path normal.
const CAPTURE_CREATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);
const CAPTURE_ACTIVITY_CHECK: std::time::Duration = std::time::Duration::from_secs(2);
const CAPTURE_ACTIVITY_RECENT: std::time::Duration = std::time::Duration::from_secs(2);

/// Último uso/precarga del overlay de captura, en milisegundos desde Unix epoch.
static CAPTURE_LAST_USED_MS: AtomicU64 = AtomicU64::new(0);

/// `start_impl` está congelando el escritorio (aún sin sesión activa).
/// Sin esto, un segundo atajo rápido abre otra captura en paralelo y un
/// `show()` tardío deja la ventana gris tapando el escritorio sin sesión.
static STARTING: AtomicBool = AtomicBool::new(false);
/// Sube en cada cancelación. El arranque en curso aborta si su token no coincide.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// La mira confirmó estar pintada y usable (ack del webview de captura).
///
/// Es el único hecho que distingue «la selección está en pantalla» de «la
/// ventana está visible pero el webview quedó en blanco». `is_visible()` no
/// sirve para eso: la pone en `true` este mismo módulo al llamar `show()`.
static REVEALED: AtomicBool = AtomicBool::new(false);
/// Ancla del shelf pendiente: la mira guarda el PNG y vuela; el shelf se
/// muestra al aterrizar.
static PENDING_SHELF: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn mark_capture_overlay_used() {
    CAPTURE_LAST_USED_MS.store(now_ms(), Ordering::SeqCst);
}

#[cfg(windows)]
fn input_idle_ms() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` apunta a memoria válida y `cbSize` tiene el tamaño requerido.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // `dwTime` es un tick de 32 bits. Se compara contra los 32 bits bajos del
    // tick actual con resta modular: un wrap cuenta el elapsed real, no "idle
    // para siempre".
    let now = unsafe { GetTickCount64() as u32 };
    Some(now.wrapping_sub(info.dwTime) as u64)
}

#[cfg(target_os = "macos")]
fn input_idle_ms() -> Option<u64> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    // CombinedSessionState = 0, kCGAnyInputEventType = 0xFFFFFFFF.
    let secs = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    if secs.is_finite() && secs >= 0.0 {
        Some((secs * 1000.0) as u64)
    } else {
        None
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn input_idle_ms() -> Option<u64> {
    // Sin probe nativo se conserva el comportamiento seguro: usuario activo.
    Some(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayKind {
    Capture,
}

#[cfg(any(windows, target_os = "macos"))]
pub struct OverlaySession {
    /// Windows: frame congelado del escritorio virtual (coords físicas), que
    /// también es la fuente de los recortes.
    #[cfg(windows)]
    frame: atic_capture::Frame,
    /// macOS: frames **nativos** por monitor, en el mismo orden que
    /// `monitors`. Un Retina y un 1x no comparten grilla de píxeles, así que
    /// los recortes finales salen de acá y conservan la resolución de cada
    /// pantalla.
    #[cfg(target_os = "macos")]
    natives: Vec<atic_capture::Frame>,
    /// Rect del escritorio virtual en el espacio del preview (píxeles del
    /// JPEG que ve el frontend).
    preview_bounds: atic_capture::Rect,
    /// Píxeles de preview por punto del escritorio (1.0 en Windows).
    preview_scale: f64,
    /// Ventanas candidatas (coords globales), z-order topmost-first.
    candidates: Vec<atic_capture::windows::WindowCandidate>,
    /// Monitores, para la selección de monitor completo.
    monitors: Vec<atic_capture::monitors::MonitorInfo>,
    /// JPEG temporal del frame congelado (se borra al terminar).
    frame_path: std::path::PathBuf,
    kind: OverlayKind,
}

#[cfg(any(windows, target_os = "macos"))]
impl OverlaySession {
    /// Punto global (puntos en Mac, físicos en Windows) → píxeles del preview.
    fn point_to_preview(&self, x: i32, y: i32) -> (f64, f64) {
        (
            f64::from(x) * self.preview_scale - f64::from(self.preview_bounds.x),
            f64::from(y) * self.preview_scale - f64::from(self.preview_bounds.y),
        )
    }

    /// Píxeles del preview (relativos a su esquina) → punto global.
    fn preview_to_point(&self, right: f64, down: f64) -> (i32, i32) {
        (
            ((f64::from(self.preview_bounds.x) + right) / self.preview_scale).round() as i32,
            ((f64::from(self.preview_bounds.y) + down) / self.preview_scale).round() as i32,
        )
    }

    /// Recorta una región del escritorio congelado (coords en puntos) a la
    /// mejor resolución disponible.
    ///
    /// Windows: del frame único. macOS: de los frames nativos; si la región
    /// toca un solo monitor sale nativa, si cruza monitores de escalas
    /// distintas se compone en la mayor.
    fn crop_points(&self, rect: atic_capture::Rect) -> Option<atic_capture::Frame> {
        #[cfg(windows)]
        {
            self.frame.crop(rect)
        }
        #[cfg(target_os = "macos")]
        {
            crop_natives(&self.monitors, &self.natives, rect)
        }
    }
}

/// Escala un rect en el espacio global a píxeles de una escala dada.
#[cfg(any(windows, target_os = "macos"))]
fn scaled_rect(rect: atic_capture::Rect, scale: f64) -> atic_capture::Rect {
    atic_capture::Rect::new(
        (f64::from(rect.x) * scale).round() as i32,
        (f64::from(rect.y) * scale).round() as i32,
        (f64::from(rect.width) * scale).round().max(1.0) as u32,
        (f64::from(rect.height) * scale).round().max(1.0) as u32,
    )
}

/// Compone el recorte de una región en puntos usando los frames nativos.
#[cfg(target_os = "macos")]
fn crop_natives(
    monitors: &[atic_capture::monitors::MonitorInfo],
    natives: &[atic_capture::Frame],
    rect: atic_capture::Rect,
) -> Option<atic_capture::Frame> {
    let mut out_scale = 1.0_f64;
    let mut found = false;
    for monitor in monitors.iter().take(natives.len()) {
        if monitor.bounds.intersection(&rect).is_some() {
            out_scale = out_scale.max(monitor.scale.max(0.01));
            found = true;
        }
    }
    if !found {
        return None;
    }
    let out_bounds = scaled_rect(rect, out_scale);
    let mut canvas = atic_capture::Frame::new(
        out_bounds,
        vec![0u8; out_bounds.width as usize * out_bounds.height as usize * 4],
    );
    let mut any = false;
    for (monitor, native) in monitors.iter().zip(natives) {
        let Some(inter) = monitor.bounds.intersection(&rect) else {
            continue;
        };
        let Some(cropped) = native.crop(scaled_rect(inter, monitor.scale.max(0.01))) else {
            continue;
        };
        canvas.blend_over_scaled(&cropped, scaled_rect(inter, out_scale));
        any = true;
    }
    any.then_some(canvas)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub struct OverlaySession;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCandidate {
    /// `HWND` como entero (cabe en el rango seguro de JS).
    pub hwnd: i64,
    pub title: String,
    /// Coordenadas en píxeles del PNG congelado (origen = esquina del
    /// escritorio virtual). No son CSS: el frontend las mapea al recuadro
    /// real de la imagen para no desfasar con DPI o varios monitores.
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    /// Ruta absoluta del JPEG de la mira (el frontend la pasa por convertFileSrc).
    pub frame_path: String,
    /// Tamaño del frame congelado en píxeles físicos.
    pub width: f64,
    pub height: f64,
    pub candidates: Vec<OverlayCandidate>,
    pub monitors: Vec<OverlayMonitor>,
    pub kind: OverlayKind,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayMonitor {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LandingRect {
    /// Píxeles del frame congelado, el mismo espacio que `OverlayCandidate`.
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub fn start_capture_session(app: AppHandle) -> Result<(), String> {
    trigger(&app)
}

/// Disparador para el atajo, el tray y el botón de la UI.
///
/// - Sesión ya abierta → cancela (toggle).
/// - Arranque en curso (congelando) → se ignora. El freeze de un escritorio
///   grande / la 1ª creación del webview tarda más que un segundo tap; si
///   canceláramos, el usuario tiene que apretar tres veces para ver la mira.
///   Un arranque colgado lo corta el watchdog, no el segundo clic.
/// - Idle → arranca.
pub fn trigger(app: &AppHandle) -> Result<(), String> {
    // Sin despedida: lo próximo que pasa acá es congelar la pantalla.
    crate::color_picker::stop_now(app);
    trigger_kind(app, OverlayKind::Capture)
}

fn trigger_kind(app: &AppHandle, kind: OverlayKind) -> Result<(), String> {
    if session_is_active(app) {
        let same = session_kind(app) == Some(kind);
        end_session(app);
        if same {
            return Ok(());
        }
    }
    if STARTING.load(Ordering::SeqCst) {
        crate::overlay::reassert_capturing_input(app);
        return Ok(());
    }
    start_impl(app, kind)
}

fn session_kind(app: &AppHandle) -> Option<OverlayKind> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        app.try_state::<crate::state::AppState>().and_then(|state| {
            state
                .overlay_session
                .lock_or_recover()
                .as_ref()
                .map(|s| s.kind)
        })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
        None
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

/// Copia al portapapeles y deja el shelf para el vuelo. El overlay sigue
/// arriba hasta `complete_capture_fly` / `end_session`.
fn finish(app: &AppHandle, path: &str, shelf_anchor: (i32, i32)) {
    *PENDING_SHELF
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(shelf_anchor);
    crate::capture::notify_capture_ready_ex(app, path, Some(shelf_anchor), false);
}

fn take_pending_shelf() -> Option<(i32, i32)> {
    PENDING_SHELF
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take()
}

/// La mira ya aterrizó: esconder el overlay y revelar el shelf.
#[tauri::command]
pub fn complete_capture_fly(app: AppHandle) {
    end_session(&app);
}

/// Rectángulo del thumb del shelf, en píxeles del frame congelado.
#[tauri::command]
pub fn capture_shelf_landing(
    app: AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<LandingRect, String> {
    capture_shelf_landing_impl(&app, left, top, width, height)
}

fn end_session(app: &AppHandle) {
    // Invalida cualquier `start_impl` en vuelo antes de ocultar/limpiar.
    GENERATION.fetch_add(1, Ordering::SeqCst);
    STARTING.store(false, Ordering::SeqCst);
    REVEALED.store(false, Ordering::SeqCst);

    // El shelf primero: si se oculta el overlay antes, hay un frame de
    // escritorio vivo. Si había un vuelo pendiente (o se canceló a mitad),
    // la captura ya está en disco y tiene que aparecer.
    let pending = take_pending_shelf();
    if let Some(anchor) = pending {
        let _ = crate::capture_shelf::show_shelf(app, Some(anchor));
    }

    // Ocultar (no cerrar): destruir la ventana provoca un crash de wry cuando
    // recibe WM_SETFOCUS durante su destrucción. Se reutiliza en la próxima
    // sesión.
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        #[cfg(windows)]
        disable_dwm_transitions(&window);
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

#[cfg(any(windows, target_os = "macos"))]
fn end_session_cleanup(session: Option<OverlaySession>) {
    if let Some(session) = session {
        let _ = std::fs::remove_file(&session.frame_path);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn end_session_cleanup(_session: Option<OverlaySession>) {}

// ---------------------------------------------------------------------------
// Implementación Windows
// ---------------------------------------------------------------------------

/// Suelta `STARTING`/`CAPTURING` si el arranque abortó sin llegar a sesión.
fn abandon_start(app: &AppHandle) {
    STARTING.store(false, Ordering::SeqCst);
    if !session_is_active(app) {
        crate::overlay::set_capturing(app, false);
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn start_impl(app: &AppHandle, kind: OverlayKind) -> Result<(), String> {
    mark_capture_overlay_used();
    // Solo una sesión: si ya hay overlay, cancelar (mismo criterio que el atajo).
    if session_is_active(app) {
        end_session(app);
        return Ok(());
    }
    // Otro arranque en curso: no cancelar (el freeze sigue). Carrera rara
    // entre atajo y botón; `trigger` ya filtró el caso habitual.
    if STARTING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }
    // Arranca sin ack: el de la sesión anterior no vale para esta.
    REVEALED.store(false, Ordering::SeqCst);

    let token = GENERATION.load(Ordering::SeqCst);

    // Click-through de la pill. El freeze corre en este hilo, como la pizarra:
    // el estilo llega cuando Windows bombea mensajes, y para entonces la mira
    // ya cubre el escritorio.
    crate::overlay::set_capturing(app, true);

    // Misma forma que la pizarra: congelar y mostrar en este hilo. Un spawn
    // extra dejaba la mira esperando el scheduler y un hop al hilo principal
    // después de que los píxeles ya estaban listos.
    if let Err(error) = start_freeze(app, token, kind) {
        tracing::warn!(%error, "no se pudo abrir el overlay de captura");
        abandon_start(app);
        return Err(error);
    }
    Ok(())
}

/// Dibuja la pill y el lanzador encima del frame recién congelado.
///
/// BitBlt no ve el overlay layered; los recortes de Windows sí, porque usan
/// Graphics Capture. Se le pide el bitmap a WebView2 y, si no lo da, se cae a
/// `PrintWindow`. Sin esto la pill desaparece de lo congelado.
///
/// Está aparte porque lo usan la selección de captura y la pizarra: las dos
/// congelan la misma pantalla y las dos tienen que incluir el overlay.
#[cfg(windows)]
pub(crate) fn compose_overlay(app: &AppHandle, frame: &mut atic_capture::Frame) {
    let mut composed = false;
    if let Some(mut overlay) = crate::overlay::capture_layer(app) {
        if overlay.prepare_overlay_layer() {
            frame.blend_over(&overlay);
            composed = true;
            tracing::info!(target: "overlay", "pill/launcher compuestos en la captura");
        }
    }
    if composed {
        return;
    }
    let Some(hwnd) = crate::overlay::hwnd() else {
        return;
    };
    match atic_capture::engine::print_window_tree(hwnd) {
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

/// Congela el escritorio para la mira.
///
/// Devuelve el frame del preview (el que se muestra), su escala en píxeles por
/// punto y —en macOS— los frames nativos por monitor, que son la fuente de los
/// recortes finales. En Windows el preview ya es el frame físico único y
/// `natives` va vacío.
#[cfg(windows)]
fn freeze_desktop(
    include_cursor: bool,
) -> Result<(atic_capture::Frame, f64, Vec<atic_capture::Frame>), String> {
    let vs = atic_capture::monitors::virtual_screen();
    let frame = atic_capture::engine::capture_rect(vs, include_cursor)
        .map_err(crate::ui_lang::map_capture_error)?;
    Ok((frame, 1.0, Vec::new()))
}

#[cfg(target_os = "macos")]
fn freeze_desktop(
    include_cursor: bool,
) -> Result<(atic_capture::Frame, f64, Vec<atic_capture::Frame>), String> {
    use atic_capture::{engine, monitors, Frame, Rect};

    let monitors = monitors::enumerate();
    let vs = monitors::virtual_screen();
    // El preview se arma en la escala **mayor** presente: es la que AppKit le
    // da al backing de la ventana que cruza pantallas, así que el Retina queda
    // 1:1 y los 1x se downsamplean (nítidos igual). Con la escala menor el
    // text del escritorio congelado salía borroso en el Retina. Es solo el
    // telón de la selección; cada captura final sale nativa de su monitor.
    let preview_scale = monitors.iter().map(|m| m.scale).fold(0.0_f64, f64::max);
    let preview_scale = if preview_scale.is_finite() && preview_scale > 0.1 {
        preview_scale
    } else {
        1.0
    };

    let mut natives = Vec::with_capacity(monitors.len());
    let mut first_error: Option<String> = None;
    let mut captured = 0usize;
    for monitor in &monitors {
        match engine::capture_rect(monitor.bounds, include_cursor) {
            Ok(frame) => {
                natives.push(frame);
                captured += 1;
            }
            Err(error) => {
                tracing::warn!(%error, id = %monitor.id, "no se pudo congelar el monitor");
                first_error.get_or_insert_with(|| crate::ui_lang::map_capture_error(error));
                natives.push(Frame::new(Rect::new(0, 0, 1, 1), vec![0u8; 4]));
            }
        }
    }
    if captured == 0 {
        return Err(first_error.unwrap_or_else(|| {
            crate::ui_lang::msg(
                "No se pudo congelar ninguna pantalla.",
                "Could not freeze any screen.",
            )
        }));
    }

    let preview_bounds = scaled_rect(vs, preview_scale);
    let mut preview = Frame::new(
        preview_bounds,
        vec![0u8; preview_bounds.width as usize * preview_bounds.height as usize * 4],
    );
    for (monitor, native) in monitors.iter().zip(natives.iter()) {
        if native.bounds.width <= 1 {
            continue;
        }
        preview.blend_over_scaled(native, scaled_rect(monitor.bounds, preview_scale));
    }
    tracing::debug!(
        target: "captura",
        w = preview.width(),
        h = preview.height(),
        non_black = preview
            .bgra
            .chunks_exact(4)
            .any(|px| px[0] > 8 || px[1] > 8 || px[2] > 8),
        "preview armado"
    );
    Ok((preview, preview_scale, natives))
}

#[cfg(any(windows, target_os = "macos"))]
fn start_freeze(app: &AppHandle, token: u64, kind: OverlayKind) -> Result<(), String> {
    use atic_capture::{monitors, windows as capwin};

    let state = app.state::<crate::state::AppState>();
    let include_cursor = state.config.lock_or_recover().capture_include_cursor;

    hide_overlay_if_visible(app);
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    // Congelar YA. Crear el webview o encodear el preview no puede ir antes:
    // el escritorio seguiría cambiando mientras esperamos.
    let (frame, preview_scale, natives) = match freeze_desktop(include_cursor) {
        Ok(frozen) => frozen,
        Err(error) => {
            abandon_start(app);
            return Err(error);
        }
    };
    // En Windows el frame único ya es la fuente de recortes; `natives` solo lo
    // usa macOS.
    #[cfg(windows)]
    let _ = &natives;
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    let pid = std::process::id();
    let windows_ready = std::thread::Builder::new()
        .name("atic-capture-windows".into())
        .spawn(move || {
            let monitors = monitors::enumerate();
            let candidates = capwin::enumerate_candidates(pid, &monitors);
            (monitors, candidates)
        })
        .ok();

    let overlay_ready = overlay_is_ready(app);
    let app_overlay = app.clone();
    let overlay_wait = if overlay_ready {
        None
    } else {
        std::thread::Builder::new()
            .name("atic-capture-overlay".into())
            .spawn(move || ensure_capture_overlay(&app_overlay))
            .ok()
    };

    // JPEG de la mira: el PNG lossless es la captura final, no este telón.
    // En un dual 4K el PNG se come cientos de ms.
    let jpeg = match frame.to_jpeg(80) {
        Ok(jpeg) => jpeg,
        Err(error) => {
            abandon_start(app);
            return Err(error.to_string());
        }
    };
    if abort_requested(token) {
        abandon_start(app);
        return Ok(());
    }

    let frames_dir = state.dirs.overlay_frames_dir();
    let frame_path = frames_dir.join("overlay.jpg");
    let _ = std::fs::remove_file(frames_dir.join("overlay.png"));
    if let Err(error) = std::fs::write(&frame_path, &jpeg) {
        abandon_start(app);
        return Err(error.to_string());
    }

    let (monitors, candidates) = match windows_ready {
        Some(handle) => handle.join().unwrap_or_else(|_| {
            let monitors = monitors::enumerate();
            let candidates = capwin::enumerate_candidates(pid, &monitors);
            (monitors, candidates)
        }),
        None => {
            let monitors = monitors::enumerate();
            let candidates = capwin::enumerate_candidates(pid, &monitors);
            (monitors, candidates)
        }
    };

    match overlay_wait {
        Some(handle) => match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                abandon_start(app);
                let _ = std::fs::remove_file(&frame_path);
                return Err(err);
            }
            Err(_) => {
                if let Err(err) = ensure_capture_overlay(app) {
                    abandon_start(app);
                    let _ = std::fs::remove_file(&frame_path);
                    return Err(err);
                }
            }
        },
        None if overlay_ready => {}
        None => {
            if let Err(err) = ensure_capture_overlay(app) {
                abandon_start(app);
                let _ = std::fs::remove_file(&frame_path);
                return Err(err);
            }
        }
    }
    if abort_requested(token) {
        abandon_start(app);
        let _ = std::fs::remove_file(&frame_path);
        return Ok(());
    }

    {
        let mut guard = state.overlay_session.lock_or_recover();
        if abort_requested(token) {
            abandon_start(app);
            drop(guard);
            let _ = std::fs::remove_file(&frame_path);
            return Ok(());
        }
        let preview_bounds = frame.bounds;
        #[cfg(windows)]
        let session = OverlaySession {
            frame,
            preview_bounds,
            preview_scale,
            candidates,
            monitors,
            frame_path,
            kind,
        };
        #[cfg(target_os = "macos")]
        let session = OverlaySession {
            natives,
            preview_bounds,
            preview_scale,
            candidates,
            monitors,
            frame_path,
            kind,
        };
        *guard = Some(session);
        STARTING.store(false, Ordering::SeqCst);
    }

    if abort_requested(token) {
        end_session(app);
        return Ok(());
    }

    // CAPTURING ya está activo desde el primer Pressed; reafirmar por si el
    // freeze/blur pisó el ex-style mientras tanto.
    crate::overlay::reassert_capturing_input(app);
    // No mostrar todavía: la ventana oculta carga el JPEG. Si nace visible,
    // el usuario ve #111 un instante —el negro entre el atajo y el congelado.
    // `eval` despierta Chromium sin el telón.
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.eval("void 0");
    }
    let info = match overlay_info_impl(app) {
        Ok(info) => info,
        Err(err) => {
            end_session(app);
            return Err(err);
        }
    };
    let _ = app.emit("overlay-session-started", info);
    tracing::debug!(target: "captura", overlay_ready, "sesión de captura emitida");
    // En Mac un WKWebView oculto no carga la página hasta que la ventana se
    // muestra: sin este empujón el webview precalentado nunca escucha el
    // evento y la mira queda en negro hasta que el watchdog cancela. Se
    // muestra transparente y click-through; el frontend la revela cuando el
    // frame está pintado.
    #[cfg(target_os = "macos")]
    wake_capture_overlay(app);
    schedule_show_watchdog(app.clone(), token);
    // La pill encima del frame, sin bloquear la mira. Si el recorte llega
    // antes, la captura sale sin la pill: es el caso raro.
    #[cfg(windows)]
    compose_overlay_into_session(app);
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
fn hide_overlay_if_visible(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    ensure_overlay_hidden(app);
    // Un frame a DWM: si BitBlt corre ahora, congela el telón #111.
    std::thread::sleep(std::time::Duration::from_millis(16));
}

#[cfg(windows)]
fn compose_overlay_into_session(app: &AppHandle) {
    let state = app.state::<crate::state::AppState>();
    let mut guard = state.overlay_session.lock_or_recover();
    if let Some(session) = guard.as_mut() {
        compose_overlay(app, &mut session.frame);
    }
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
#[cfg(windows)]
const SHOW_WATCHDOG_SECS: u64 = 6;
/// En Mac la ventana recién se muestra al empezar la sesión y la página se
/// carga ahí: el primer render en dev (Vite transformando la ruta) puede
/// pasar de 6 s. En release revela en menos de un segundo; el watchdog solo
/// cubre que la creación/el render se traben.
#[cfg(target_os = "macos")]
const SHOW_WATCHDOG_SECS: u64 = 15;
#[cfg(not(any(windows, target_os = "macos")))]
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
    tracing::debug!(target: "captura", "mira revelada");
    REVEALED.store(true, Ordering::SeqCst);
}

fn ensure_overlay_hidden(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        #[cfg(windows)]
        disable_dwm_transitions(&window);
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
#[cfg(any(windows, target_os = "macos"))]
fn virtual_screen_logical() -> (f64, f64, f64, f64) {
    let vs = atic_capture::monitors::virtual_screen();
    // En Mac `virtual_screen()` ya está en puntos (unidad lógica de AppKit).
    #[cfg(target_os = "macos")]
    let scale = 1.0_f64;
    #[cfg(not(target_os = "macos"))]
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
        // El proc original se guarda en UN solo static, y lo comparten las dos
        // ventanas que cubren el escritorio virtual (la mira de captura y la
        // pizarra). Vale porque todas las ventanas de tao son de la misma
        // clase y traen el mismo proc; si algún día no lo fueran, encadenar al
        // de otra ventana rompería la que llegó primero, así que en ese caso se
        // deja sin enganchar: lo peor es que no pueda abarcar dos monitores.
        let saved = CAPTURE_OVERLAY_WNDPROC.load(Ordering::SeqCst);
        if saved != 0 && saved != current {
            tracing::warn!(
                target: "overlay",
                "otra ventana tiene un WndProc distinto; no se limita al escritorio virtual"
            );
            return;
        }
        CAPTURE_OVERLAY_WNDPROC.store(current, Ordering::SeqCst);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours);
    }
}

/// Coloca el cliente del overlay sobre TODO el escritorio virtual.
#[cfg(windows)]
pub(crate) fn cover_virtual_desktop(window: &tauri::WebviewWindow) {
    cover_rect(window, atic_capture::monitors::virtual_screen());
}

/// Coloca el cliente del overlay sobre `vs`, en píxeles físicos.
///
/// Un `set_position` + `set_size` son dos `SetWindowPos`: DWM puede recortar
/// el tamaño a un monitor y recentrar la ventana entre medio. Acá va posición
/// y tamaño juntos, y después se corrige el desfase de no-cliente (borde DWM)
/// para que el (0,0) del CSS coincida con el del PNG congelado.
///
/// Toma el rectángulo y no lo mide adentro porque la pizarra cubre un solo
/// monitor —el que se congeló— y la mira de captura el escritorio entero.
#[cfg(windows)]
pub(crate) fn cover_rect(window: &tauri::WebviewWindow, vs: atic_capture::Rect) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };

    install_virtual_screen_limit(window);

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

/// Por encima de la pill (nivel 25) y de la barra de menú (24).
///
/// `always_on_top` de Tauri usa `NSFloatingWindowLevel` (3). La mira tiene que
/// tapar el escritorio entero, incluido el notch de la pill.
#[cfg(target_os = "macos")]
fn macos_capture_overlay_chrome(window: &tauri::WebviewWindow) {
    let Ok(ptr) = window.ns_window() else {
        return;
    };
    let ns = ptr as *mut objc2::runtime::AnyObject;
    if ns.is_null() {
        return;
    }
    unsafe {
        // NSPopUpMenuWindowLevel = 101.
        let _: () = objc2::msg_send![ns, setLevel: 101isize];
        let behavior: usize = 1 | (1 << 4) | (1 << 8);
        let _: () = objc2::msg_send![ns, setCollectionBehavior: behavior];
        let _: () = objc2::msg_send![ns, setHidesOnDeactivate: false];
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn cover_virtual_desktop(window: &tauri::WebviewWindow) {
    cover_rect(window, atic_capture::monitors::virtual_screen());
}

#[cfg(target_os = "macos")]
pub(crate) fn cover_rect(window: &tauri::WebviewWindow, vs: atic_capture::Rect) {
    // `vs` está en puntos y AppKit coloca ventanas en puntos: lógico directo.
    // `Physical` se dividiría por el backing scale de la ventana y con
    // monitores de escalas distintas no hay un factor único.
    let _ = window.set_max_size(Some(tauri::Size::Logical(tauri::LogicalSize::new(
        16384.0, 16384.0,
    ))));
    let _ = window.set_position(tauri::LogicalPosition::new(
        f64::from(vs.x),
        f64::from(vs.y),
    ));
    let _ = window.set_size(tauri::LogicalSize::new(
        f64::from(vs.width).max(1.0),
        f64::from(vs.height).max(1.0),
    ));
    macos_capture_overlay_chrome(window);
}

/// Sin esto DWM anima el `show`/`hide` como si naciera una ventana nueva.
/// La mira tiene que aparecer como el mismo escritorio, un frame después.
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

fn create_capture_overlay(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    #[cfg(any(windows, target_os = "macos"))]
    let (lx, ly, lw, lh) = virtual_screen_logical();
    #[cfg(not(any(windows, target_os = "macos")))]
    let (lx, ly, lw, lh) = (0.0, 0.0, 800.0, 600.0);

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        tauri::WebviewUrl::App("capture-overlay".into()),
    )
    .title(crate::ui_lang::pick(
        crate::ui_lang::english(),
        "Seleccionar captura",
        "Select capture",
    ))
    .inner_size(lw, lh)
    .max_inner_size(16384.0, 16384.0)
    .position(lx, ly)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false);
    #[cfg(target_os = "macos")]
    {
        // Transparente para poder mostrarla temprano (despierta el webview
        // oculto, que en WKWebView no carga hasta verse) sin que aparezca un
        // fondo negro antes de que el frame esté pintado.
        builder = builder
            .accept_first_mouse(true)
            .transparent(true)
            .visible_on_all_workspaces(true);
    }
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
        disable_dwm_transitions(&window);
    }
    #[cfg(target_os = "macos")]
    {
        cover_virtual_desktop(&window);
        macos_capture_overlay_chrome(&window);
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
/// captura paga el costo de crearla. `prewarm_capture_overlay` adelanta ese
/// costo al arranque, como la ventana de la pizarra.
///
/// Precarga el webview de captura en background. Sin esto, la primera mira
/// paga crear Chromium *después* del freeze y se siente más lenta que la pizarra.
pub(crate) fn prewarm_capture_overlay(app: &AppHandle) {
    ensure_capture_overlay_idle_reaper(app);
    ensure_capture_overlay_activity_rewarmer(app);
    #[cfg(any(windows, target_os = "macos"))]
    {
        // El arranque y el rewarmer pueden pedirlo a la vez (la creación tarda
        // segundos y el rewarmer la ve "faltante"): una sola creación en vuelo.
        static PREWARM_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
        if PREWARM_IN_FLIGHT.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        let _ = std::thread::Builder::new()
            .name("atic-capture-prewarm".into())
            .spawn(move || {
                if let Err(err) = ensure_capture_overlay(&app) {
                    tracing::warn!(%err, "no se pudo precargar el overlay de captura");
                } else {
                    mark_capture_overlay_used();
                }
                PREWARM_IN_FLIGHT.store(false, Ordering::SeqCst);
            });
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
    }
}

/// Hilo único: si el overlay precalentado no se usa durante `CAPTURE_IDLE_TTL`,
/// destruye la ventana oculta y deja que la próxima captura la recree on-demand.
///
/// Invariante anti-churn: para destruir hacen falta DOS idles a la vez, sin
/// capturas por `CAPTURE_IDLE_TTL` y sin input por `CAPTURE_INPUT_IDLE`. Si solo
/// miráramos capturas, un usuario activo dispararía destroy a los 10 minutos y
/// el monitor de actividad lo recrearía enseguida, ciclando WebView2 y ~350 MB.
fn ensure_capture_overlay_idle_reaper(app: &AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    let spawn_result = std::thread::Builder::new()
        .name("atic-capture-idle".into())
        .spawn(move || loop {
            std::thread::sleep(CAPTURE_IDLE_CHECK);
            let starting = STARTING.load(Ordering::SeqCst);
            let revealed = REVEALED.load(Ordering::SeqCst);
            let active = session_is_active(&app2);
            let last = CAPTURE_LAST_USED_MS.load(Ordering::SeqCst);
            let now = now_ms();
            let elapsed_ms = now.saturating_sub(last);
            let input_idle = input_idle_ms();
            let window = app2.get_webview_window(OVERLAY_LABEL);
            let window_exists = window.is_some();
            let visible_result = window.as_ref().map(|window| window.is_visible());
            tracing::debug!(
                starting,
                revealed,
                active,
                last_used_ms = last,
                elapsed_ms,
                input_idle_ms = ?input_idle,
                window_exists,
                visible = ?visible_result,
                "capture idle tick"
            );
            if starting || revealed {
                continue;
            }
            if active {
                continue;
            }
            if last == 0 {
                continue;
            }
            if elapsed_ms < CAPTURE_IDLE_TTL.as_millis() as u64 {
                continue;
            }

            let app_destroy = app2.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            if app2
                .run_on_main_thread(move || {
                    let starting = STARTING.load(Ordering::SeqCst);
                    let revealed = REVEALED.load(Ordering::SeqCst);
                    let active = session_is_active(&app_destroy);
                    let mut visible_result = None;
                    let mut input_idle = None;
                    let (destroyed, branch) = if starting || revealed || active {
                        (false, "busy")
                    } else if let Some(window) = app_destroy.get_webview_window(OVERLAY_LABEL) {
                        let visible = window.is_visible();
                        let visible_blocks_destroy =
                            visible.as_ref().map_or(true, |visible| *visible);
                        visible_result = Some(visible);
                        if visible_blocks_destroy {
                            (false, "visible")
                        } else {
                            input_idle = input_idle_ms();
                            if input_idle.is_none_or(|elapsed| {
                                elapsed < CAPTURE_INPUT_IDLE.as_millis() as u64
                            }) {
                                (false, "input-active")
                            } else {
                                let _ = window.destroy();
                                (true, "destroy")
                            }
                        }
                    } else {
                        (false, "missing-window")
                    };
                    tracing::debug!(
                        starting,
                        revealed,
                        active,
                        branch,
                        destroyed,
                        input_idle_ms = ?input_idle,
                        visible = ?visible_result,
                        "capture idle main-thread decision"
                    );
                    let _ = tx.send(destroyed);
                })
                .is_err()
            {
                continue;
            }
            if rx
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap_or(false)
            {
                std::thread::sleep(std::time::Duration::from_millis(400));
                CAPTURE_LAST_USED_MS.store(0, Ordering::SeqCst);
                tracing::info!(
                    idle_secs = CAPTURE_IDLE_TTL.as_secs(),
                    "overlay de captura liberado por idle"
                );
            }
        });
    tracing::debug!(
        spawn = if spawn_result.is_ok() { "Ok" } else { "Err" },
        error = ?spawn_result.as_ref().err(),
        "capture idle reaper spawn"
    );
}

/// Hilo único: cuando vuelve el input tras liberar el overlay, lo vuelve a
/// precalentar sin mezclar esa decisión con el reaper de memoria.
fn ensure_capture_overlay_activity_rewarmer(app: &AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    static WARMING: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    let spawn_result = std::thread::Builder::new()
        .name("atic-capture-activity".into())
        .spawn(move || loop {
            std::thread::sleep(CAPTURE_ACTIVITY_CHECK);
            let starting = STARTING.load(Ordering::SeqCst);
            let active = session_is_active(&app2);
            let window_exists = app2.get_webview_window(OVERLAY_LABEL).is_some();
            let input_idle = input_idle_ms();
            let warming = WARMING.load(Ordering::SeqCst);
            tracing::debug!(
                starting,
                active,
                window_exists,
                input_idle_ms = ?input_idle,
                warming,
                "capture activity tick"
            );
            if window_exists {
                if WARMING.swap(false, Ordering::SeqCst) {
                    tracing::info!("overlay de captura re-precargado por actividad");
                }
                continue;
            }
            if starting || active || warming {
                continue;
            }
            if input_idle.is_none_or(|elapsed| elapsed > CAPTURE_ACTIVITY_RECENT.as_millis() as u64)
            {
                continue;
            }
            if WARMING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                continue;
            }
            tracing::debug!(
                input_idle_ms = ?input_idle,
                "capture activity rewarm decision"
            );
            prewarm_capture_overlay(&app2);
            // Si la creación falla, soltar `WARMING`: sin esta red de seguridad
            // un intento fallido trabaría el monitor para siempre.
            std::thread::spawn(|| {
                std::thread::sleep(CAPTURE_CREATE_TIMEOUT + std::time::Duration::from_secs(5));
                WARMING.store(false, Ordering::SeqCst);
            });
        });
    tracing::debug!(
        spawn = if spawn_result.is_ok() { "Ok" } else { "Err" },
        error = ?spawn_result.as_ref().err(),
        "capture activity rewarmer spawn"
    );
}

fn overlay_is_ready(app: &AppHandle) -> bool {
    app.get_webview_window(OVERLAY_LABEL)
        .is_some_and(|window| capture_webview_alive(&window))
}

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
    rx.recv_timeout(CAPTURE_CREATE_TIMEOUT).map_err(|_| {
        crate::ui_lang::msg(
            "Se agotó el tiempo al crear la captura.",
            "Timed out creating the capture overlay.",
        )
    })?
}

fn restore_main_hit_testing(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_ignore_cursor_events(false);
    }
}

#[cfg(any(windows, target_os = "macos"))]
/// Despierta la mira sin revelarla: la muestra transparente y click-through.
///
/// `WKWebView` no carga la página de una ventana oculta; mostrarla al empezar
/// la sesión es lo que hace que el webview monte, pida `overlayInfo()` y
/// cargue el frame. El frontend llama después a `showCaptureOverlay`, que la
/// vuelve interactiva y la deja con el frame ya pintado.
#[cfg(target_os = "macos")]
fn wake_capture_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    // Click-through: todavía no hay frame que seleccionar y no debe robarle
    // el mouse al escritorio mientras carga.
    let _ = window.set_ignore_cursor_events(true);
    cover_virtual_desktop(&window);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    tracing::debug!(target: "captura", "mira despertada");
}

fn show_overlay_window(app: &AppHandle) -> Result<(), String> {
    tracing::debug!(target: "captura", "show_overlay_window");
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
    #[cfg(windows)]
    disable_dwm_transitions(&window);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    mark_capture_overlay_used();
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
    #[cfg(windows)]
    raise_capture_overlay(&window);
    #[cfg(target_os = "macos")]
    macos_capture_overlay_chrome(&window);
    cover_virtual_desktop(&window);
    schedule_cover_retries(app);

    // No usar `set_focus()`: si `SetForegroundWindow` falla, tao inyecta Alt y
    // esta app también usa SendInput para pegar. `force_foreground` activa sin
    // teclas fantasma — hace falta para Esc y para que WebView2 reciba el mouse
    // sin un clic previo “fuera” de la main.
    #[cfg(windows)]
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
    #[cfg(target_os = "macos")]
    {
        let _ = window.set_focus();
    }
    Ok(())
}

/// WebView2 termina de nacer después del `show`: sin repetir, se queda en el
/// recuadro del create (una pantalla, centrada).
#[cfg(any(windows, target_os = "macos"))]
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

#[cfg(not(any(windows, target_os = "macos")))]
fn show_overlay_window(_app: &AppHandle) -> Result<(), String> {
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(any(windows, target_os = "macos"))]
fn overlay_info_impl(app: &AppHandle) -> Result<OverlayInfo, String> {
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    let bounds = session.preview_bounds;

    let to_preview = |x: i32, y: i32| session.point_to_preview(x, y);

    let candidates = session
        .candidates
        .iter()
        .map(|candidate| {
            let visual = candidate.visual_bounds;
            let (left, top) = to_preview(visual.x, visual.y);
            OverlayCandidate {
                hwnd: candidate.hwnd as i64,
                title: candidate.title.clone(),
                left,
                top,
                width: f64::from(visual.width) * session.preview_scale,
                height: f64::from(visual.height) * session.preview_scale,
            }
        })
        .collect();

    let monitors = session
        .monitors
        .iter()
        .map(|monitor| {
            let visual = monitor.bounds;
            let (left, top) = to_preview(visual.x, visual.y);
            OverlayMonitor {
                left,
                top,
                width: f64::from(visual.width) * session.preview_scale,
                height: f64::from(visual.height) * session.preview_scale,
            }
        })
        .collect();

    Ok(OverlayInfo {
        frame_path: session.frame_path.to_string_lossy().into_owned(),
        width: f64::from(bounds.width),
        height: f64::from(bounds.height),
        candidates,
        monitors,
        kind: session.kind,
    })
}

/// Padding y thumb del shelf: tienen que coincidir con `ShelfSurface.svelte`.
const SHELF_PAD: f64 = 8.0;
const SHELF_THUMB_W: f64 = 192.0;
const SHELF_THUMB_H: f64 = 120.0;

#[cfg(any(windows, target_os = "macos"))]
fn capture_shelf_landing_impl(
    app: &AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<LandingRect, String> {
    let state = app.state::<crate::state::AppState>();
    let (near, bounds, preview_scale) = {
        let guard = state.overlay_session.lock_or_recover();
        let session = guard.as_ref().ok_or("sin sesión de captura activa")?;
        // `left/top/width/height` llegan en píxeles del preview, relativos a la
        // esquina del frame: se traducen a un punto global para elegir monitor.
        let (nx, ny) = session.preview_to_point(left + width * 0.5, top + height * 0.5);
        ((nx, ny), session.preview_bounds, session.preview_scale)
    };

    let left_side = state
        .config
        .lock_or_recover()
        .capture_shelf_side
        .eq_ignore_ascii_case("left");

    // Colocar (oculto) para leer la posición real, no una estimación.
    crate::floating::place(
        app,
        "capture-shelf",
        crate::floating::Anchor::BottomCorner {
            near: Some(near),
            left_side,
        },
    )
    .ok_or("no se pudo ubicar el shelf")?;

    let shelf = app.get_webview_window("capture-shelf").ok_or("sin shelf")?;
    let pos = shelf.outer_position().map_err(|e| e.to_string())?;
    // En Mac la posición de Tauri es física pero el global son puntos.
    let (pos_x, pos_y) = (
        crate::floating::to_global(&shelf, f64::from(pos.x)),
        crate::floating::to_global(&shelf, f64::from(pos.y)),
    );
    // Un `CSS px` equivale a la escala DPI de Windows o a 1 punto en Mac; pasar
    // a píxeles del preview multiplica además por su escala.
    #[cfg(windows)]
    let units_per_css = shelf.scale_factor().unwrap_or(1.0).max(0.01);
    #[cfg(target_os = "macos")]
    let units_per_css = 1.0_f64;
    let thumb_x = (pos_x + SHELF_PAD * units_per_css) * preview_scale;
    let thumb_y = (pos_y + SHELF_PAD * units_per_css) * preview_scale;

    Ok(LandingRect {
        left: thumb_x - f64::from(bounds.x),
        top: thumb_y - f64::from(bounds.y),
        width: SHELF_THUMB_W * units_per_css * preview_scale,
        height: SHELF_THUMB_H * units_per_css * preview_scale,
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn capture_shelf_landing_impl(
    _app: &AppHandle,
    _left: f64,
    _top: f64,
    _width: f64,
    _height: f64,
) -> Result<LandingRect, String> {
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(any(windows, target_os = "macos"))]
fn window_capture_impl(app: &AppHandle, hwnd: i64) -> Result<(String, (i32, i32)), String> {
    use atic_capture::{engine, windows as capwin};

    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    // PrintWindow renderiza solo la ventana; si falla/negro, recorta del frame
    // congelado (nunca de la pantalla, para no capturar el overlay).
    let window_bounds = capwin::window_bounds(hwnd as isize);
    let frame =
        match engine::print_window(hwnd as isize).map_err(crate::ui_lang::map_capture_error)? {
            Some(frame) => frame,
            None => {
                let win_bounds = window_bounds.ok_or("ventana sin límites")?;
                session
                    .crop_points(win_bounds)
                    .ok_or("la ventana quedó fuera del área capturada")?
            }
        };
    // El ancla va en coords globales (puntos en Mac): la ventana nativa no
    // sirve para ubicar el shelf si el monitor no es 1x.
    let anchor = window_bounds
        .map(rect_center)
        .unwrap_or_else(|| rect_center(frame.bounds));
    drop(guard);
    save_capture(app, &frame, anchor)
}

#[cfg(any(windows, target_os = "macos"))]
fn region_capture_impl(
    app: &AppHandle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<(String, (i32, i32)), String> {
    use atic_capture::Rect;

    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    // La mira manda píxeles del preview; el recorte se pide en coords globales
    // para que cada monitor aporte su resolución nativa.
    let (px, py) = session.preview_to_point(left, top);
    let region = Rect::new(
        px,
        py,
        (width / session.preview_scale).round().max(1.0) as u32,
        (height / session.preview_scale).round().max(1.0) as u32,
    );
    let frame = session
        .crop_points(region)
        .ok_or("la región quedó fuera del área capturada")?;
    let anchor = rect_center(region);
    drop(guard);
    save_capture(app, &frame, anchor)
}

#[cfg(any(windows, target_os = "macos"))]
fn monitor_capture_impl(app: &AppHandle, x: f64, y: f64) -> Result<(String, (i32, i32)), String> {
    let state = app.state::<crate::state::AppState>();
    let guard = state.overlay_session.lock_or_recover();
    let session = guard.as_ref().ok_or("sin sesión de captura activa")?;

    let (point_x, point_y) = session.preview_to_point(x, y);
    let monitor = session
        .monitors
        .iter()
        .find(|monitor| monitor.bounds.contains(point_x, point_y))
        .or_else(|| session.monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| session.monitors.first())
        .ok_or("no se encontró el monitor")?;
    let frame = session
        .crop_points(monitor.bounds)
        .ok_or("el monitor quedó fuera del área capturada")?;
    let anchor = rect_center(monitor.bounds);
    drop(guard);
    save_capture(app, &frame, anchor)
}

/// Centro de un rect en coords globales, para el vuelo del shelf.
#[cfg(any(windows, target_os = "macos"))]
fn rect_center(rect: atic_capture::Rect) -> (i32, i32) {
    (
        rect.x + rect.width as i32 / 2,
        rect.y + rect.height as i32 / 2,
    )
}

#[cfg(any(windows, target_os = "macos"))]
fn save_capture(
    app: &AppHandle,
    frame: &atic_capture::Frame,
    anchor: (i32, i32),
) -> Result<(String, (i32, i32)), String> {
    use atic_capture::naming;
    let state = app.state::<crate::state::AppState>();
    let png = frame.to_png().map_err(|e| e.to_string())?;
    let dir = state.dirs.captures_dir();
    let path = dir.join(naming::unique_capture_filename(&dir));
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    Ok((path.to_string_lossy().into_owned(), anchor))
}

// ---------------------------------------------------------------------------
// Stubs para plataformas sin motor de captura.
// ---------------------------------------------------------------------------

#[cfg(not(any(windows, target_os = "macos")))]
fn start_impl(_app: &AppHandle, _kind: OverlayKind) -> Result<(), String> {
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn overlay_info_impl(_app: &AppHandle) -> Result<OverlayInfo, String> {
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn window_capture_impl(_app: &AppHandle, _hwnd: i64) -> Result<(String, (i32, i32)), String> {
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn region_capture_impl(
    _app: &AppHandle,
    _left: f64,
    _top: f64,
    _width: f64,
    _height: f64,
) -> Result<(String, (i32, i32)), String> {
    Err(crate::ui_lang::capture_windows_only())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn monitor_capture_impl(
    _app: &AppHandle,
    _x: f64,
    _y: f64,
) -> Result<(String, (i32, i32)), String> {
    Err(crate::ui_lang::capture_windows_only())
}
