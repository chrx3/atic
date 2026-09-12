//! Prototipo: tapa la ventana activa, la “da vuelta” y muestra notas.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(windows)]
use tauri::{PhysicalPosition, PhysicalSize};

use crate::state::AppState;
use atic_core::MutexExt;

pub const LABEL: &str = "window-flip";

static OPEN: AtomicBool = AtomicBool::new(false);
static PRESENTED: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0);
static SESSION: Mutex<Option<FlipSession>> = Mutex::new(None);
/// Ventana de debajo y chrome de Atic (scratchpad, shelf, etc.).
/// Si Atic se cierra mal hay que devolverlas igual.
static CONCEAL: Mutex<Vec<Conceal>> = Mutex::new(Vec::new());

/// Cómo se sacó la ventana del escritorio. `DWMWA_CLOAK` es lo más limpio, pero
/// desde otro proceso casi nunca pega. Moverla “fuera” del escritorio virtual
/// tampoco sirve: con varios monitores Windows la reubica en otra pantalla.
/// El overlay es transparente a propósito (el aire del giro tiene que mostrar
/// el escritorio), así que si el cloak falla hay que esconderla de verdad,
/// sin cambiarle de monitor.
#[cfg(windows)]
struct Conceal {
    hwnd: isize,
    kind: ConcealKind,
    /// Floats de Atic (overlay, shelf…). Se restauran *después* de bajar
    /// la tapa; si no, el scratchpad tapa el escritorio un segundo.
    chrome: bool,
    /// `GetWindowPlacement` antes de ocultar. `SW_HIDE` de una maximizada
    /// la devuelve restaurada en el centro; esto la reabre como estaba.
    placement: Option<SavedPlacement>,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct SavedPlacement {
    show_cmd: u32,
    flags: u32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(windows)]
enum ConcealKind {
    Cloak,
    Hidden,
}

#[cfg(not(windows))]
struct Conceal {
    hwnd: isize,
    chrome: bool,
}

#[derive(Clone)]
struct FlipSession {
    key: String,
    title: String,
    exe: String,
    /// `data:image/png;base64,…` del .exe, o vacío si no se pudo leer.
    icon: String,
    preview_path: PathBuf,
    blocks: Vec<crate::notes::Block>,
    /// Carpeta de binarios de esta app. El front la necesita entera para
    /// armar la URL `asset://` de cada imagen.
    assets_dir: PathBuf,
    target_hwnd: isize,
    overlay_x: i32,
    overlay_y: i32,
    overlay_w: u32,
    overlay_h: u32,
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
    pub icon: String,
    pub preview_path: String,
    pub blocks: Vec<crate::notes::Block>,
    pub assets_dir: String,
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
            icon: s.icon.clone(),
            preview_path: s.preview_path.to_string_lossy().into_owned(),
            blocks: s.blocks.clone(),
            assets_dir: s.assets_dir.to_string_lossy().into_owned(),
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
        // Giro de vuelta. Si el front se queda mudo, esto saca la tapa.
        // Con margen sobre su propio respaldo (giro + 180 ms): que gane el
        // front, que sabe en qué frame quedó la foto.
        thread::sleep(Duration::from_millis(1200));
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

/// El frente ya está pintado: mostrar la tapa encima de la ventana viva.
#[tauri::command]
pub fn window_flip_present(app: AppHandle) -> Result<(), String> {
    if !OPEN.load(Ordering::SeqCst) {
        return Ok(());
    }
    show_cover(&app);
    Ok(())
}

/// La tapa ya cubre: ocultar la ventana viva. Separado de `present` para
/// no dejar un frame sin foto.
#[tauri::command]
pub fn window_flip_conceal(app: AppHandle) -> Result<(), String> {
    if !OPEN.load(Ordering::SeqCst) {
        return Ok(());
    }
    let hwnd = SESSION
        .lock_or_recover()
        .as_ref()
        .map(|s| s.target_hwnd)
        .unwrap_or(0);
    conceal_cover(&app, hwnd);
    Ok(())
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
pub fn window_flip_save_blocks(
    state: State<AppState>,
    blocks: Vec<crate::notes::Block>,
) -> Result<(), String> {
    let mut guard = SESSION.lock_or_recover();
    let Some(session) = guard.as_mut() else {
        return Ok(());
    };
    session.blocks = blocks;
    // La página se resuelve por título en cada guardado en vez de recordar su
    // id: la nota puede haber cambiado en disco (otra ventana de la misma app)
    // entre que se abrió la tapa y se escribió.
    let notes_dir = state.dirs.notes_dir();
    let mut note = crate::notes::load(&notes_dir, &session.exe);
    let page_id = note.page_for_title(&session.title);
    if let Some(page) = note.page_mut(&page_id) {
        page.set_blocks(session.blocks.clone());
    }
    crate::notes::save(&notes_dir, &mut note)?;
    crate::notes::collect_garbage(&notes_dir, &note);
    Ok(())
}

/// Guarda la imagen del portapapeles en la nota y devuelve con qué referirla.
///
/// El front no manda los bytes: los lee Rust del portapapeles del sistema, que
/// es el mismo contenido que el `paste` del webview acaba de ver.
#[tauri::command]
pub fn window_flip_paste_image(state: State<AppState>) -> Result<PastedImage, String> {
    let exe = SESSION
        .lock_or_recover()
        .as_ref()
        .map(|s| s.exe.clone())
        .ok_or_else(|| "no hay tapa abierta".to_string())?;
    let (asset, width, height) = crate::notes::add_clipboard_image(&state.dirs.notes_dir(), &exe)?;
    Ok(PastedImage {
        asset,
        width,
        height,
    })
}

/// Mete en la nota una imagen que ya está en disco (el historial del
/// portapapeles). Solo se aceptan rutas de las carpetas de datos de Atic: esto
/// lo llama el cajón con lo que el propio historial le dio, no el usuario.
#[tauri::command]
pub fn window_flip_import_image(
    state: State<AppState>,
    path: String,
) -> Result<PastedImage, String> {
    let origen = PathBuf::from(&path);
    let origen = origen
        .canonicalize()
        .map_err(|_| "esa imagen no es del historial".to_string())?;
    let permitido = [state.dirs.clipboard_dir(), state.dirs.captures_dir()]
        .iter()
        .any(|raiz| {
            raiz.canonicalize()
                .ok()
                .map(|raiz| origen.starts_with(raiz))
                .unwrap_or(false)
        });
    if !permitido {
        return Err("esa imagen no es del historial".into());
    }
    let exe = SESSION
        .lock_or_recover()
        .as_ref()
        .map(|s| s.exe.clone())
        .ok_or_else(|| "no hay tapa abierta".to_string())?;
    let (asset, width, height) =
        crate::notes::import_image(&state.dirs.notes_dir(), &exe, &origen)?;
    Ok(PastedImage {
        asset,
        width,
        height,
    })
}

/// Data URL de un binario de la nota abierta.
///
/// El protocolo de assets es otro origen: dibujarlo en un canvas contamina
/// `toDataURL` y el export del tablero falla. Un data URL es del mismo origen.
#[tauri::command]
pub fn window_flip_asset_data(state: State<AppState>, asset: String) -> Result<String, String> {
    let nombre = Path::new(&asset)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .ok_or_else(|| "imagen inválida".to_string())?;
    let exe = SESSION
        .lock_or_recover()
        .as_ref()
        .map(|s| s.exe.clone())
        .ok_or_else(|| "no hay tapa abierta".to_string())?;
    let dir = crate::notes::assets_dir(&state.dirs.notes_dir(), &exe);
    let destino = dir.join(nombre);
    let dir = dir
        .canonicalize()
        .map_err(|_| "no se encontró la imagen".to_string())?;
    let destino = destino
        .canonicalize()
        .map_err(|_| "no se encontró la imagen".to_string())?;
    if !destino.starts_with(&dir) {
        return Err("esa imagen no es de la nota".into());
    }
    let bytes = std::fs::read(&destino).map_err(|e| e.to_string())?;
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/png"
    };
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{data}"))
}

/// ¿El foco se fue a una ventana que no es de Atic?
///
/// El reverso se cierra al perder el foco para no quedar pegado arriba de
/// todo, pero abrir el portapapeles o cualquier otra flotante de Atic también
/// es perder el foco, y ahí cerrar es exactamente lo contrario de lo que el
/// usuario quiso.
#[tauri::command]
pub fn window_flip_focus_is_foreign(app: AppHandle) -> bool {
    #[cfg(not(windows))]
    {
        let _ = app;
        true
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetForegroundWindow, GA_ROOT,
        };
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return false;
        }
        let raiz = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let hwnd = if raiz.is_null() { hwnd } else { raiz } as isize;
        !app.webview_windows()
            .values()
            .filter_map(|w| w.hwnd().ok())
            .any(|h| h.0 as isize == hwnd)
    }
}

#[derive(Serialize)]
pub struct PastedImage {
    pub asset: String,
    pub width: u32,
    pub height: u32,
}

fn hide(app: &AppHandle) {
    OPEN.store(false, Ordering::SeqCst);
    PRESENTED.store(false, Ordering::SeqCst);
    let target = SESSION
        .lock_or_recover()
        .as_ref()
        .map(|s| s.target_hwnd)
        .unwrap_or(0);
    // Ventana viva debajo de la foto, se espera a que se pinte, y recién
    // ahí se baja la tapa. Si se oculta antes, un frame de escritorio
    // parpadea.
    reveal_matching(|c| !c.chrome);
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::DwmFlush;
        use windows_sys::Win32::Graphics::Gdi::{
            RedrawWindow, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW,
        };
        if target != 0 {
            unsafe {
                let _ = RedrawWindow(
                    target as _,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
                );
            }
        }
        // Un `DwmFlush` solo espera al frame en curso, que todavía es el de
        // antes de destapar. El segundo es el que ya la tiene en pantalla.
        let _ = unsafe { DwmFlush() };
        let _ = unsafe { DwmFlush() };
    }
    park(app);
    reveal_matching(|c| c.chrome);
    focus_target(app, target);
    if let Some(session) = SESSION.lock_or_recover().take() {
        if !session.preview_path.as_os_str().is_empty() {
            olvidar_preview(&session.preview_path);
        }
    }
}

fn show_cover(app: &AppHandle) {
    if !OPEN.load(Ordering::SeqCst) {
        return;
    }
    let geom = SESSION
        .lock_or_recover()
        .as_ref()
        .map(|s| (s.overlay_x, s.overlay_y, s.overlay_w, s.overlay_h));
    if let Some(window) = app.get_webview_window(LABEL) {
        #[cfg(windows)]
        disable_dwm_transitions(&window);
        #[cfg(windows)]
        if let Some((x, y, w, h)) = geom {
            let _ = window.set_size(PhysicalSize::new(w, h));
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
        let _ = window.set_always_on_top(true);
        let _ = window.show();
    }
}

fn conceal_cover(app: &AppHandle, hwnd: isize) {
    if !OPEN.load(Ordering::SeqCst) {
        return;
    }
    if PRESENTED.swap(true, Ordering::SeqCst) {
        return;
    }
    show_cover(app);
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::DwmFlush;
        let _ = unsafe { DwmFlush() };
        conceal_hwnd(hwnd, false, true);
        conceal_atic_chrome(app, hwnd);
        let _ = unsafe { DwmFlush() };
    }
    #[cfg(not(windows))]
    {
        conceal_hwnd(hwnd, false, true);
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_focus();
    }
    // La pill vive en el overlay a pantalla completa: si la tapa queda
    // encima, la tapa transparente no deja verla. Subir el overlay (click
    // through salvo la pill) la deja visible sin comerse el teclado. Una
    // sola vez: cada restack es un SetWindowPos justo antes del giro.
    crate::overlay::raise(app);
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetForegroundWindow, IsIconic, GA_ROOT,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return Err("no hay ventana al frente".into());
    }
    let hwnd = {
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if root.is_null() {
            hwnd
        } else {
            root
        }
    };
    if is_our_label(app, hwnd as isize, LABEL)
        || is_our_label(app, hwnd as isize, crate::overlay::LABEL)
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
    let exe_path = process_exe_path(hwnd);
    let exe = exe_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "app".into());
    let icon = exe_path
        .as_ref()
        .and_then(|p| crate::launcher_icons::icon_data_url(p))
        .unwrap_or_default();
    let key = note_key(&exe, &title);

    let dirs = app
        .try_state::<AppState>()
        .ok_or_else(|| "sin estado".to_string())?
        .dirs
        .clone();
    let overlay = overlay_for_flip(bounds);
    let (card_left, card_top, card_width, card_height) = card_layout(bounds, overlay);

    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "falta la ventana window-flip".to_string())?;

    disable_dwm_transitions(&window);
    // Tamaño ya, posición no: si se mueve a pantalla ahora, la tapa vacía
    // pestañea. Se coloca en `show_cover` con la foto lista.
    let _ = window.set_size(PhysicalSize::new(overlay.width, overlay.height));

    let gen = GEN.load(Ordering::SeqCst) + 1;
    let preview_path = dirs
        .overlay_frames_dir()
        .join(format!("window-flip-{gen}.png"));
    // No hace falta tapar la pill: la foto va sin las ventanas layered, así
    // que el overlay no entra. Esconderla acá era el pestañeo de arranque.
    let hay_foto = capture_preview(hwnd as isize, &preview_path, true);

    let mut doc = crate::notes::load(&dirs.notes_dir(), &exe);
    let page_id = doc.page_for_title(&title);
    let blocks = doc
        .page_mut(&page_id)
        .map(|page| page.blocks.clone())
        .unwrap_or_default();
    let assets_dir = crate::notes::assets_dir(&dirs.notes_dir(), &exe);
    let session = FlipSession {
        key,
        title,
        exe,
        icon,
        preview_path: if hay_foto {
            preview_path
        } else {
            PathBuf::new()
        },
        blocks,
        assets_dir,
        target_hwnd: hwnd as isize,
        overlay_x: overlay.x,
        overlay_y: overlay.y,
        overlay_w: overlay.width,
        overlay_h: overlay.height,
        card_left,
        card_top,
        card_width,
        card_height,
    };

    *SESSION.lock_or_recover() = Some(session.clone());
    GEN.fetch_add(1, Ordering::SeqCst);
    OPEN.store(true, Ordering::SeqCst);
    PRESENTED.store(false, Ordering::SeqCst);

    // No mostrar todavía: el webview vacío pestañea. El front llama
    // `present` cuando la foto ya está en el frente.
    let _ = window.emit("window-flip-open", WindowFlipView::from(&session));

    let handle = app.clone();
    let gen = GEN.load(Ordering::SeqCst);
    thread::spawn(move || {
        // Holgado respecto de lo que el front espera por la foto: si esto
        // gana la carrera, tapa sin frente pintado y eso sí se ve.
        thread::sleep(Duration::from_millis(1800));
        if OPEN.load(Ordering::SeqCst)
            && GEN.load(Ordering::SeqCst) == gen
            && !PRESENTED.load(Ordering::SeqCst)
        {
            // El front nunca contestó. Esconder la ventana viva bajo una
            // tapa vacía la haría desaparecer: mejor que el atajo no haga
            // nada y se pueda volver a intentar.
            tracing::warn!(target: "window_flip", "el front no montó la tapa; se cancela el volteo");
            hide(&handle);
        }
    });
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
    let session = guard
        .as_mut()
        .ok_or_else(|| "no hay tapa abierta".to_string())?;
    if session.target_hwnd == 0 {
        return Ok(WindowFlipView::from(&*session));
    }
    let gen = GEN.load(Ordering::SeqCst);
    let path = dirs
        .overlay_frames_dir()
        .join(format!("window-flip-{gen}-back.png"));
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
/// Siempre recorta al marco visible. `permitir_pantalla` usa BitBlt del
/// escritorio (píxeles de DWM, misma nitidez que la ventana viva). PrintWindow
/// de un WebView se ve más blando y con las esquinas cuadradas.
///
/// Sin las layered: con `CAPTUREBLT` la pill del overlay quedaba pegada en el
/// frente de la tarjeta cuando caía sobre la ventana que se voltea.
fn capture_preview(hwnd: isize, path: &Path, permitir_pantalla: bool) -> bool {
    let frame = if permitir_pantalla {
        match atic_capture::windows::window_bounds(hwnd)
            .ok_or_else(|| "ventana sin límites".to_string())
            .and_then(|bounds| {
                atic_capture::engine::capture_rect_without_layered(bounds)
                    .map_err(|err| err.to_string())
            }) {
            Ok(frame) => Some(frame),
            Err(err) => {
                tracing::warn!(target: "window_flip", %err, "BitBlt de la ventana falló; pruebo PrintWindow");
                match atic_capture::engine::capture_window_visual(hwnd) {
                    Ok(frame) => Some(frame),
                    Err(err) => {
                        tracing::warn!(target: "window_flip", %err, "no se pudo recortar la foto al marco visible");
                        None
                    }
                }
            }
        }
    } else {
        match atic_capture::engine::capture_window_visual(hwnd) {
            Ok(frame) => Some(frame),
            Err(err) => {
                tracing::warn!(target: "window_flip", %err, "no se pudo recortar la foto al marco visible");
                None
            }
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
///
/// El aire alcanza con que cubra la sombra y el par de píxeles que la
/// perspectiva agrega en los ángulos intermedios: el giro hunde la tarjeta en
/// Z medio ancho (`HUNDIDO` en `WindowFlipSurface.svelte`), y con eso el borde
/// que se acerca queda en z = 0 y nunca se proyecta más grande que su marco.
/// Si eso cambia, este pad se queda corto y la tarjeta se corta al girar.
///
/// Y tiene que ser chico: la tapa no es click-through, así que se come los
/// clics de todo lo que cubre. Con el pad viejo (un tercio del ancho) una
/// ventana grande dejaba la pantalla entera sin recibir clics.
///
/// Si la ventana está pegada al borde de la pantalla, se corre el overlay
/// (sin recortar la tarjeta) para que Windows no lo clampee mal.
fn overlay_for_flip(visual: atic_capture::Rect) -> atic_capture::Rect {
    let pad_x = (visual.width / 16).max(88);
    let pad_y = (visual.height / 16).max(88);
    let desired = atic_capture::Rect::new(
        visual.x - pad_x as i32,
        visual.y - pad_y as i32,
        visual.width + pad_x * 2,
        visual.height + pad_y * 2,
    );
    fit_overlay_to_screen(desired, visual)
}

fn fit_overlay_to_screen(
    mut overlay: atic_capture::Rect,
    _visual: atic_capture::Rect,
) -> atic_capture::Rect {
    let screen = atic_capture::monitors::virtual_screen();
    if screen.is_empty() {
        return overlay;
    }
    if overlay.width > screen.width {
        overlay.width = screen.width;
        overlay.x = screen.x;
    }
    if overlay.height > screen.height {
        overlay.height = screen.height;
        overlay.y = screen.y;
    }
    if overlay.x < screen.x {
        overlay.x = screen.x;
    }
    if overlay.y < screen.y {
        overlay.y = screen.y;
    }
    if overlay.right() > screen.right() {
        overlay.x = screen.right() - overlay.width as i32;
    }
    if overlay.bottom() > screen.bottom() {
        overlay.y = screen.bottom() - overlay.height as i32;
    }
    overlay
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

fn conceal_hwnd(hwnd: isize, chrome: bool, allow_hide: bool) {
    if hwnd == 0 {
        return;
    }
    {
        let guard = CONCEAL.lock_or_recover();
        if guard.iter().any(|c| c.hwnd == hwnd) {
            return;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, chrome, allow_hide);
    }
    #[cfg(windows)]
    {
        if !window_is_visible(hwnd) {
            return;
        }
        disable_dwm_transitions_hwnd(hwnd);
        let placement = read_placement(hwnd);
        let kind = if try_cloak(hwnd) {
            ConcealKind::Cloak
        } else if allow_hide && try_hide(hwnd) {
            ConcealKind::Hidden
        } else {
            tracing::warn!(
                target: "window_flip",
                allow_hide,
                "no se pudo ocultar la ventana: se va a ver detrás del giro"
            );
            return;
        };
        CONCEAL.lock_or_recover().push(Conceal {
            hwnd,
            kind,
            chrome,
            placement,
        });
    }
}

#[cfg(windows)]
fn conceal_atic_chrome(app: &AppHandle, skip: isize) {
    // El overlay (pill) no se oculta: es chrome del escritorio, no de la
    // ventana que gira. Shelf / loupe sí tapan la tarjeta.
    const LABELS: &[&str] = &[
        crate::capture_shelf::LABEL,
        crate::annotate::ANNOTATE_LABEL,
        "color-loupe",
    ];
    for label in LABELS {
        if let Some(hwnd) = hwnd_of(app, label) {
            if hwnd != skip {
                conceal_hwnd(hwnd, true, true);
            }
        }
    }
}

#[cfg(windows)]
fn hwnd_of(app: &AppHandle, label: &str) -> Option<isize> {
    app.get_webview_window(label)
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
}

fn reveal_matching(pred: impl Fn(&Conceal) -> bool) {
    let mut guard = CONCEAL.lock_or_recover();
    let mut keep = Vec::new();
    let mut restore = Vec::new();
    for item in guard.drain(..) {
        if pred(&item) {
            restore.push(item);
        } else {
            keep.push(item);
        }
    }
    *guard = keep;
    drop(guard);
    #[cfg(not(windows))]
    {
        let _ = restore;
    }
    #[cfg(windows)]
    {
        for item in restore {
            restore_concealed(item);
        }
    }
}

fn reveal_current() {
    reveal_matching(|_| true);
}

/// Si Atic se cierra con una tapa abierta, la ventana de debajo no puede
/// quedar invisible.
pub(crate) fn uncloak_on_exit() {
    reveal_current();
}

#[cfg(windows)]
fn restore_concealed(conceal: Conceal) {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

    if unsafe { IsWindow(conceal.hwnd as _) } == 0 {
        return;
    }
    match conceal.kind {
        ConcealKind::Cloak => {
            set_cloaked(conceal.hwnd, false);
        }
        ConcealKind::Hidden => {
            restore_placement(conceal.hwnd, conceal.placement);
        }
    }
}

/// Estaciona la tapa fuera de pantalla, visible para WebView2.
/// El primer `show` en frío pestañea; dejarla caliente evita ese frame.
pub fn park(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    #[cfg(windows)]
    {
        disable_dwm_transitions(&window);
        let _ = window.set_size(PhysicalSize::new(1280, 800));
        let _ = window.set_position(PhysicalPosition::new(-32000, -32000));
        let _ = window.show();
    }
    #[cfg(not(windows))]
    {
        let _ = window.hide();
    }
}

#[cfg(windows)]
fn read_placement(hwnd: isize) -> Option<SavedPlacement> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};

    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    let ok = unsafe { GetWindowPlacement(hwnd as _, &mut placement) };
    if ok == 0 {
        return None;
    }
    Some(SavedPlacement {
        show_cmd: placement.showCmd,
        flags: placement.flags,
        left: placement.rcNormalPosition.left,
        top: placement.rcNormalPosition.top,
        right: placement.rcNormalPosition.right,
        bottom: placement.rcNormalPosition.bottom,
    })
}

#[cfg(windows)]
fn restore_placement(hwnd: isize, saved: Option<SavedPlacement>) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPlacement, ShowWindow, SW_SHOWNOACTIVATE, WINDOWPLACEMENT,
    };

    let Some(saved) = saved else {
        unsafe {
            let _ = ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
        }
        return;
    };
    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    placement.flags = saved.flags;
    placement.showCmd = saved.show_cmd;
    placement.rcNormalPosition = RECT {
        left: saved.left,
        top: saved.top,
        right: saved.right,
        bottom: saved.bottom,
    };
    let ok = unsafe { SetWindowPlacement(hwnd as _, &placement) };
    if ok == 0 {
        unsafe {
            let _ = ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
        }
    }
}

#[cfg(windows)]
fn try_cloak(hwnd: isize) -> bool {
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;

    if !set_cloaked(hwnd, true) {
        return false;
    }
    let _ = unsafe { DwmFlush() };
    if window_is_cloaked(hwnd) {
        return true;
    }
    set_cloaked(hwnd, false);
    false
}

#[cfg(windows)]
fn focus_target(app: &AppHandle, hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, SetForegroundWindow,
    };

    if hwnd == 0 {
        return;
    }
    if is_our_label(app, hwnd, "main") {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
        return;
    }
    unsafe {
        let _ = AllowSetForegroundWindow(u32::MAX);
        let _ = SetForegroundWindow(hwnd as _);
    }
}

#[cfg(not(windows))]
fn focus_target(_app: &AppHandle, _hwnd: isize) {}

#[cfg(windows)]
fn window_is_visible(hwnd: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, IsWindowVisible};
    unsafe { IsWindow(hwnd as _) != 0 && IsWindowVisible(hwnd as _) != 0 }
}

#[cfg(windows)]
fn try_hide(hwnd: isize) -> bool {
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindow, IsWindowVisible, ShowWindow, SW_HIDE,
    };

    unsafe {
        if IsWindow(hwnd as _) == 0 {
            return false;
        }
        let _ = ShowWindow(hwnd as _, SW_HIDE);
        let _ = DwmFlush();
        IsWindowVisible(hwnd as _) == 0
    }
}

#[cfg(windows)]
fn window_is_cloaked(hwnd: isize) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};

    let mut cloaked: u32 = 0;
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd as _,
            DWMWA_CLOAKED as u32,
            std::ptr::from_mut(&mut cloaked).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    hr == 0 && cloaked != 0
}

#[cfg(windows)]
fn set_cloaked(hwnd: isize, on: bool) -> bool {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // DWMWA_CLOAK = 13. No está en todos los bindings de windows-sys 0.59.
    const DWMWA_CLOAK: u32 = 13;
    let value: i32 = if on { 1 } else { 0 };
    unsafe {
        DwmSetWindowAttribute(
            hwnd as _,
            DWMWA_CLOAK,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of::<i32>() as u32,
        ) == 0
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
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    disable_dwm_transitions_hwnd(hwnd.0 as isize);
}

#[cfg(windows)]
fn disable_dwm_transitions_hwnd(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
    };
    let disable: i32 = 1;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd as _,
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
fn process_exe_path(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<PathBuf> {
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
        Some(PathBuf::from(String::from_utf16_lossy(
            &path_buf[..path_len as usize],
        )))
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
        assert!(
            !p.exists(),
            "la foto de la ventana no puede quedarse en disco"
        );
    }

    /// Guarda como lo hace `window_flip_save_note`, sin la sesión de por medio.
    fn escribir(notes_dir: &Path, exe: &str, titulo: &str, cuerpo: &str) {
        let mut note = crate::notes::load(notes_dir, exe);
        let page_id = note.page_for_title(titulo);
        if let Some(page) = note.page_mut(&page_id) {
            page.set_plain_text(cuerpo);
        }
        crate::notes::save(notes_dir, &mut note).unwrap();
    }

    fn leer(notes_dir: &Path, exe: &str, titulo: &str) -> String {
        let mut note = crate::notes::load(notes_dir, exe);
        let page_id = note.page_for_title(titulo);
        note.page_mut(&page_id)
            .map(|p| p.plain_text())
            .unwrap_or_default()
    }

    #[test]
    fn la_nota_sobrevive_al_cambio_de_titulo() {
        let dir = std::env::temp_dir().join(format!("atic-notas-titulo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        escribir(&dir, "teams.exe", "Standup", "acordarse del deploy");
        // La misma app con otro título: página nueva, pero la anterior sigue.
        escribir(&dir, "teams.exe", "Retro", "traer los números");
        assert_eq!(leer(&dir, "teams.exe", "Standup"), "acordarse del deploy");
        assert_eq!(leer(&dir, "teams.exe", "Retro"), "traer los números");
        assert_eq!(crate::notes::load(&dir, "teams.exe").pages.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn las_notas_de_dos_apps_no_se_pisan() {
        let dir = std::env::temp_dir().join(format!("atic-notas-dos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        escribir(&dir, "teams.exe", "Standup", "una");
        escribir(&dir, "code.exe", "main.rs", "otra");
        assert_eq!(leer(&dir, "teams.exe", "Standup"), "una");
        assert_eq!(leer(&dir, "code.exe", "main.rs"), "otra");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_json_viejo_se_migra_a_paginas() {
        let dir = std::env::temp_dir().join(format!("atic-notas-migra-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let notes = dir.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(
            dir.join("window_flip_notes.json"),
            r#"{"teams.exe|Standup":"una","teams.exe|Retro":"otra","code.exe":"suelta"}"#,
        )
        .unwrap();

        crate::notes::migrate_legacy(&dir, &notes);

        assert_eq!(leer(&notes, "teams.exe", "Standup"), "una");
        assert_eq!(leer(&notes, "teams.exe", "Retro"), "otra");
        assert_eq!(leer(&notes, "code.exe", ""), "suelta");
        // El original se conserva renombrado, no se borra.
        assert!(!dir.join("window_flip_notes.json").exists());
        assert!(dir.join("window_flip_notes.migrado.json").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_aire_del_giro_deja_la_tarjeta_en_el_marco_visual() {
        let visual = atic_capture::Rect::new(200, 100, 400, 300);
        let overlay = overlay_for_flip(visual);
        assert!(overlay.width > visual.width);
        assert!(overlay.height > visual.height);
        let (_left, top, width, height) = card_layout(visual, overlay);
        assert!(top >= 0.0 && top + height <= 1.0 + 1e-9);
        assert!((width - visual.width as f64 / overlay.width as f64).abs() < 1e-9);
        assert!((height - visual.height as f64 / overlay.height as f64).abs() < 1e-9);
    }

    #[test]
    fn pegada_arriba_la_tarjeta_no_se_sale_del_overlay() {
        let visual = atic_capture::Rect::new(80, 0, 400, 300);
        let overlay = overlay_for_flip(visual);
        assert!(overlay.y <= visual.y);
        assert!(overlay.bottom() >= visual.bottom());
        let (left, top, width, height) = card_layout(visual, overlay);
        assert!(left >= 0.0 && left + width <= 1.0 + 1e-9);
        assert!(top >= 0.0 && top + height <= 1.0 + 1e-9);
    }
}
