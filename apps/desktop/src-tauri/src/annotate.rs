//! Editor de anotaciones: dibujar encima de una captura ya tomada.
//!
//! Ventana propia (`capture-annotate`), declarada en `tauri.conf.json` y
//! oculta al arrancar, igual que el estante y el lanzador. Se declara y no se
//! crea en caliente por el mismo motivo que el estante: crear ventanas WebView2
//! sobre la marcha hacía caer a wry.
//!
//! Rust hace tres cosas y ninguna es dibujar: da el tamaño de la ventana para
//! que la imagen entre a escala razonable, guarda el PNG que manda el lienzo, y
//! lo copia al portapapeles. El dibujo vive entero en el webview.

use std::path::Path;
use std::sync::Mutex;

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::MutexExt;

use crate::state::AppState;

pub const ANNOTATE_LABEL: &str = "capture-annotate";

/// Qué captura tiene que abrir el editor.
///
/// **El evento solo no alcanza.** Un WebView2 oculto está backgrounded —es por
/// lo que `capture_session.rs` le pasa `--disable-renderer-backgrounding` al
/// suyo— y un `emit` hacia un renderer dormido se pierde: la ventana aparecía
/// con la barra puesta y el lienzo vacío, sin error que mirar. Así que el
/// destino se deja acá y el webview lo **pide** al montar y cada vez que se
/// vuelve visible. El evento queda como empujón para el caso normal, igual que
/// `overlay-session-started` + `overlay_info()` en el overlay de captura.
static PENDING: Mutex<Option<AnnotateOpen>> = Mutex::new(None);

/// Alto de la barra de herramientas, en píxeles lógicos.
///
/// Está acá y no medido del DOM porque la ventana tiene que nacer con el
/// tamaño correcto: medir después obligaría a redimensionar con el editor ya
/// visible, que se ve como un salto.
const CHROME_H: f64 = 60.0;

/// Aire alrededor del lienzo, en píxeles lógicos.
const PADDING: f64 = 16.0;

/// Cuánto del área útil puede ocupar la ventana como mucho.
const MAX_FILL: f64 = 0.92;

/// Ventana mínima usable: por debajo, la barra no entra.
const MIN_W: f64 = 420.0;
const MIN_H: f64 = 260.0;

/// Tope al pasar la captura al lienzo. Un monitor 4K completo ronda los 5 MiB;
/// esto deja aire de sobra y sigue cortando un archivo que no tiene sentido.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Cómo se presenta el editor.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotateMode {
    /// Panel del tamaño de la captura, sobre lo que haya debajo.
    Panel,
    /// Pizarra: cubre el escritorio virtual sobre la pantalla congelada.
    Board,
}

/// Un rectángulo dentro de la imagen, en sus mismos píxeles.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Lo que necesita el editor para arrancar.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotateOpen {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub mode: AnnotateMode,
    /// Dónde poner los controles, en píxeles de la imagen.
    ///
    /// Con dos monitores, el centro del escritorio virtual es la costura entre
    /// las dos pantallas: una barra centrada ahí queda partida al medio. Esto
    /// es el área útil del monitor donde está el cursor, que es donde el
    /// usuario está mirando.
    pub focus: Option<FocusRect>,
}

/// Tamaño y posición de la ventana, en píxeles físicos.
#[derive(Debug, PartialEq)]
struct WindowRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

/// Encaja la imagen en el área útil, dejando sitio para la barra.
///
/// Función aparte —y con tests— porque es la única cuenta del módulo que puede
/// salir mal de forma silenciosa: una ventana más grande que la pantalla se ve
/// como «el editor no tiene botones», que es un síntoma que no señala su causa.
///
/// `work` es el área útil en píxeles físicos; `scale` la del monitor, para
/// convertir el cromo (que se piensa en píxeles lógicos) a físicos.
fn fit_window(image: (u32, u32), work: (i32, i32, u32, u32), scale: f64) -> WindowRect {
    let (wx, wy, ww, wh) = work;
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let chrome = (CHROME_H + PADDING * 2.0) * scale;
    let side_pad = PADDING * 2.0 * scale;

    let max_w = (f64::from(ww) * MAX_FILL - side_pad).max(1.0);
    let max_h = (f64::from(wh) * MAX_FILL - chrome).max(1.0);

    let img_w = f64::from(image.0.max(1));
    let img_h = f64::from(image.1.max(1));
    // Nunca se agranda: una captura chica se anota a tamaño real, que es donde
    // el trazo cae exactamente donde el ojo lo puso.
    let fit = (max_w / img_w).min(max_h / img_h).min(1.0);

    let w = (img_w * fit + side_pad).max(MIN_W * scale);
    let h = (img_h * fit + chrome).max(MIN_H * scale);
    let w = w.min(f64::from(ww)).round();
    let h = h.min(f64::from(wh)).round();

    WindowRect {
        x: wx + ((f64::from(ww) - w) / 2.0).round() as i32,
        y: wy + ((f64::from(wh) - h) / 2.0).round() as i32,
        w: w as u32,
        h: h as u32,
    }
}

/// Área útil y escala del monitor donde está el cursor (o el primario).
#[cfg(windows)]
fn target_work_area() -> ((i32, i32, u32, u32), f64) {
    let monitors = atic_capture::monitors::enumerate();
    let cursor = crate::floating::cursor_position();
    let target = cursor
        .and_then(|(x, y)| monitors.iter().find(|m| m.bounds.contains(x, y)))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first());
    match target {
        Some(m) => (
            (m.work_area.x, m.work_area.y, m.work_area.width, m.work_area.height),
            m.scale,
        ),
        None => ((0, 0, 1280, 720), 1.0),
    }
}

#[cfg(not(windows))]
fn target_work_area() -> ((i32, i32, u32, u32), f64) {
    ((0, 0, 1280, 720), 1.0)
}

/// Abre el editor sobre una captura del directorio de capturas.
#[tauri::command]
pub fn open_annotator(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    open_annotator_path(&app, &state.dirs.captures_dir(), &path)
}

/// La misma apertura, para quien ya tiene el `AppHandle` y el directorio.
///
/// La usa `activate_capture` cuando el clic en la miniatura está configurado
/// para dibujar: pasar por el comando obligaría a resolver un `State` que el
/// llamador ya tiene en la mano.
pub fn open_annotator_path(
    app: &AppHandle,
    captures_dir: &Path,
    path: &str,
) -> Result<(), String> {
    let target = crate::capture::ensure_capture_in_dir(captures_dir, Path::new(path))?;
    let (width, height) = png_size(&target)?;

    let Some(window) = app.get_webview_window(ANNOTATE_LABEL) else {
        return Err("la ventana del editor no existe".into());
    };

    let (work, scale) = target_work_area();
    let rect = fit_window((width, height), work, scale);
    let _ = window.unminimize();
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_size(tauri::PhysicalSize::new(rect.w, rect.h));
    let _ = window.set_position(tauri::PhysicalPosition::new(rect.x, rect.y));

    let pending = AnnotateOpen {
        path: target.to_string_lossy().into_owned(),
        width,
        height,
        mode: AnnotateMode::Panel,
        focus: None,
    };
    // Primero el destino, después mostrar, y el evento al final —con el webview
    // ya despierto—, que es el mismo orden que usa el estante en
    // `notify_capture_ready`. Al revés, el evento le llega a un renderer
    // dormido y no llega nunca.
    *PENDING.lock_or_recover() = Some(pending.clone());

    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    let _ = app.emit("annotate-open", pending);
    Ok(())
}

/// Qué captura tiene que dibujar el editor, si hay alguna.
///
/// La pide el lienzo al montar y cada vez que la ventana se vuelve visible. Es
/// la vía fiable; el evento es solo el atajo.
#[tauri::command]
pub fn pending_annotation() -> Option<AnnotateOpen> {
    PENDING.lock_or_recover().clone()
}

/// Abre la pizarra: dibujar sobre la pantalla, ahí donde está.
///
/// **Sobre la pantalla congelada, no sobre la viva.** Congelar es lo que hace
/// que el trazo caiga donde el ojo lo puso: con el escritorio en movimiento
/// —un video, un cursor que parpadea— lo de abajo se corre y la marca deja de
/// señalar lo que señalaba. Dibujar sobre lo vivo es otra herramienta y otra
/// discusión.
#[tauri::command]
pub fn start_board(app: AppHandle) -> Result<(), String> {
    start_board_impl(&app)
}

/// El atajo global abre y cierra: pulsarlo con la pizarra puesta la saca.
///
/// Es la misma regla que la mira de captura (`capture_session::trigger`), y no
/// es cosmética: una ventana que cubre el escritorio entero tiene que poder
/// irse por donde vino, sin buscar el botón.
pub fn toggle_board(app: &AppHandle) -> Result<(), String> {
    let showing = app
        .get_webview_window(ANNOTATE_LABEL)
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    let is_board = PENDING
        .lock_or_recover()
        .as_ref()
        .is_some_and(|open| open.mode == AnnotateMode::Board);
    if showing && is_board {
        close_annotator(app.clone());
        return Ok(());
    }
    start_board_impl(app)
}

fn start_board_impl(app: &AppHandle) -> Result<(), String> {
    // Esconder ANTES de congelar, y darle un frame a DWM. Es la misma lección
    // que dejó escrita `start_freeze`: si el editor quedó visible de una vuelta
    // anterior, BitBlt lo congela como si fuera el escritorio y la pizarra
    // nace con una foto de sí misma.
    if let Some(window) = app.get_webview_window(ANNOTATE_LABEL) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            std::thread::sleep(std::time::Duration::from_millis(32));
        }
    }

    let state = app.state::<AppState>();
    let (path, width, height) = freeze_screen(app, &state)?;

    let Some(window) = app.get_webview_window(ANNOTATE_LABEL) else {
        return Err("la ventana del editor no existe".into());
    };

    let pending = AnnotateOpen {
        path: path.to_string_lossy().into_owned(),
        width,
        height,
        mode: AnnotateMode::Board,
        focus: focus_monitor(),
    };
    *PENDING.lock_or_recover() = Some(pending.clone());

    let _ = window.unminimize();
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    #[cfg(windows)]
    crate::capture_session::cover_virtual_desktop(&window);

    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    let _ = app.emit("annotate-open", pending);
    Ok(())
}

/// Área útil del monitor donde está el cursor, en píxeles de la imagen.
///
/// El origen de la imagen es la esquina del escritorio virtual, que puede tener
/// coordenadas negativas: por eso se le resta, y no se usa el rect tal cual.
#[cfg(windows)]
fn focus_monitor() -> Option<FocusRect> {
    let vs = atic_capture::monitors::virtual_screen();
    let monitors = atic_capture::monitors::enumerate();
    let cursor = crate::floating::cursor_position();
    let target = cursor
        .and_then(|(x, y)| monitors.iter().find(|m| m.bounds.contains(x, y)))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first())?;
    // El área útil y no los bounds: los controles no deben caer bajo la barra
    // de tareas.
    let area = target.work_area;
    Some(FocusRect {
        x: area.x - vs.x,
        y: area.y - vs.y,
        width: area.width,
        height: area.height,
    })
}

#[cfg(not(windows))]
fn focus_monitor() -> Option<FocusRect> {
    None
}

/// Congela el escritorio virtual a un PNG y devuelve ruta y tamaño físico.
#[cfg(windows)]
fn freeze_screen(
    app: &AppHandle,
    state: &State<AppState>,
) -> Result<(std::path::PathBuf, u32, u32), String> {
    use atic_capture::{engine, monitors};

    let vs = monitors::virtual_screen();
    // Sin cursor, al revés que en una captura: en la pizarra el puntero es la
    // herramienta, y dejarlo dibujado sería un puntero de más en la pantalla.
    let mut frame = engine::capture_rect(vs, false).map_err(|e| e.to_string())?;
    crate::capture_session::compose_overlay(app, &mut frame);
    let png = frame.to_png().map_err(|e| e.to_string())?;

    let dir = state.dirs.overlay_frames_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear la carpeta: {e}"))?;
    // Archivo propio y no el `overlay.png` de la mira de captura: esa sesión lo
    // borra al terminar, y borrarle el suyo a la pizarra la dejaría en blanco.
    let path = dir.join("board.png");
    std::fs::write(&path, &png).map_err(|e| format!("no se pudo congelar: {e}"))?;
    Ok((path, vs.width, vs.height))
}

#[cfg(not(windows))]
fn freeze_screen(
    _app: &AppHandle,
    _state: &State<AppState>,
) -> Result<(std::path::PathBuf, u32, u32), String> {
    Err("la pizarra todavía no existe en esta plataforma".into())
}

/// La captura como data URL, para el lienzo.
///
/// **No se sirve por el protocolo de assets** aunque el estante lo haga. Un
/// `<img>` de otro origen *contamina* el canvas, y un canvas contaminado no
/// deja llamar a `toDataURL`: se dibujaría bien y fallaría justo al copiar o
/// guardar, que es lo único que importa. Un data URL es del mismo origen por
/// definición, y el costo es una copia en base64 al abrir.
#[tauri::command]
pub fn annotation_image(state: State<AppState>, path: String) -> Result<String, String> {
    let target = resolve_source(&state, &path)?;
    let meta = std::fs::metadata(&target).map_err(|e| format!("no se pudo leer: {e}"))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "la captura pesa más de {} MiB",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&target).map_err(|e| format!("no se pudo leer: {e}"))?;
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:image/png;base64,{data}"))
}

/// Deja pasar solo las dos carpetas de las que el editor puede leer: las
/// capturas del usuario y los frames congelados de la pizarra.
fn resolve_source(state: &State<AppState>, path: &str) -> Result<std::path::PathBuf, String> {
    let target = Path::new(path);
    if let Ok(ok) = crate::capture::ensure_capture_in_dir(&state.dirs.captures_dir(), target) {
        return Ok(ok);
    }
    crate::capture::ensure_capture_in_dir(&state.dirs.overlay_frames_dir(), target)
        .map_err(|_| "Ruta fuera de las carpetas del editor.".to_string())
}

#[tauri::command]
pub fn close_annotator(app: AppHandle) {
    // Se limpia el destino al cerrar, no al leerlo: si se consumiera en la
    // lectura, la ventana que se pide dos veces (evento + al hacerse visible)
    // se quedaría sin imagen en la segunda.
    let previous = PENDING.lock_or_recover().take();
    // El congelado de la pizarra es una pantalla entera en disco y no le sirve
    // a nadie después. Lo guardado, si el usuario guardó, ya es otro archivo.
    if let Some(open) = previous {
        if open.mode == AnnotateMode::Board {
            let _ = std::fs::remove_file(&open.path);
        }
    }
    if let Some(window) = app.get_webview_window(ANNOTATE_LABEL) {
        let _ = window.hide();
    }
}

/// Guarda lo anotado como una captura más. Devuelve la ruta nueva.
///
/// **No sobrescribe el original.** Anotar es una versión, no una edición: el
/// original es lo que de verdad se vio en pantalla, y perderlo por dibujar una
/// flecha encima no tiene vuelta atrás.
#[tauri::command]
pub fn save_annotation(
    app: AppHandle,
    state: State<AppState>,
    data: String,
) -> Result<String, String> {
    let bytes = decode_png(&data)?;
    let dir = state.dirs.captures_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear la carpeta: {e}"))?;
    let name = atic_capture::naming::unique_capture_filename(&dir);
    let path = dir.join(name);
    std::fs::write(&path, &bytes).map_err(|e| format!("no se pudo guardar: {e}"))?;

    // Lo anotado sale por la misma puerta que cualquier captura: el estante.
    // Antes esto solo refrescaba listas, y el archivo quedaba guardado en un
    // sitio que había que ir a adivinar. El estante dice dónde quedó, lo deja
    // arrastrar y abre la carpeta, que es justo lo que se necesita recién
    // guardado.
    let item = crate::capture::capture_item(&path);
    let _ = crate::capture_shelf::show_shelf(&app, None);
    match item {
        Some(item) => {
            let _ = app.emit("screenshot-created", item);
        }
        None => {
            let _ = app.emit("screenshot-shelf-updated", ());
        }
    }
    Ok(path.to_string_lossy().into_owned())
}

/// Copia lo anotado al portapapeles como imagen.
#[tauri::command]
pub fn copy_annotation(data: String) -> Result<(), String> {
    let bytes = decode_png(&data)?;
    crate::capture::copy_png_bytes(&bytes)
}

/// Acepta tanto base64 pelado como un data URL (`data:image/png;base64,…`),
/// que es lo que devuelve `canvas.toDataURL`.
fn decode_png(data: &str) -> Result<Vec<u8>, String> {
    let payload = match data.split_once("base64,") {
        Some((_, rest)) => rest,
        None => data,
    };
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|e| format!("imagen inválida: {e}"))
}

fn png_size(path: &Path) -> Result<(u32, u32), String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    // El IHDR va al principio: no hace falta leer el archivo entero.
    let mut header = vec![0u8; 1024];
    let read = file.read(&mut header).map_err(|e| e.to_string())?;
    header.truncate(read);
    atic_capture::encoding::png_dimensions(&header).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: (i32, i32, u32, u32) = (0, 0, 1920, 1040);

    #[test]
    fn una_captura_chica_se_anota_a_tamano_real() {
        let rect = fit_window((640, 400), WORK, 1.0);
        assert_eq!(rect.w, 640 + (PADDING * 2.0) as u32);
        assert_eq!(rect.h, 400 + (CHROME_H + PADDING * 2.0) as u32);
    }

    #[test]
    fn una_captura_enorme_entra_en_el_area_util() {
        let rect = fit_window((3840, 2160), WORK, 1.0);
        assert!(rect.w <= WORK.2, "ancho {} > {}", rect.w, WORK.2);
        assert!(rect.h <= WORK.3, "alto {} > {}", rect.h, WORK.3);
    }

    #[test]
    fn la_ventana_queda_centrada_en_su_monitor() {
        let work = (-1920, 100, 1920, 1040);
        let rect = fit_window((800, 600), work, 1.0);
        let center = rect.x + rect.w as i32 / 2;
        assert!((center - (-1920 + 960)).abs() <= 1, "centro en {center}");
    }

    #[test]
    fn nunca_baja_del_minimo_usable() {
        let rect = fit_window((16, 16), WORK, 1.0);
        assert!(rect.w >= MIN_W as u32);
        assert!(rect.h >= MIN_H as u32);
    }

    #[test]
    fn una_imagen_degenerada_no_divide_por_cero() {
        let rect = fit_window((0, 0), WORK, 1.0);
        assert!(rect.w > 0 && rect.h > 0);
    }

    #[test]
    fn el_cromo_escala_con_el_monitor() {
        // A 200%, la barra ocupa el doble de píxeles físicos.
        let uno = fit_window((400, 400), WORK, 1.0);
        let dos = fit_window((400, 400), WORK, 2.0);
        assert!(dos.h > uno.h);
    }

    #[test]
    fn acepta_data_url_y_base64_pelado() {
        // "hola" en base64.
        let plain = decode_png("aG9sYQ==").expect("base64 pelado");
        let url = decode_png("data:image/png;base64,aG9sYQ==").expect("data url");
        assert_eq!(plain, b"hola".to_vec());
        assert_eq!(plain, url);
    }

    #[test]
    fn una_imagen_invalida_no_pasa() {
        assert!(decode_png("no soy base64 %%%").is_err());
    }
}
