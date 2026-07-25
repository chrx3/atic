//! Dueño único de la geometría de las ventanas flotantes (pill, shelf, overlay).
//!
//! Antes cada ventana resolvía lo suyo y el frontend clampeaba **otra vez** por
//! su cuenta: los dos clamps se pisaban y la pill se desplazaba un poco en cada
//! ciclo abrir/cerrar. Acá vive el único clamp, el único concepto de "monitor
//! correcto" y el único tween.
//!
//! Regla: el frontend declara *intención* ("al cursor", "a su hogar"); nunca
//! calcula coordenadas ni llama a `setPosition`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition};

/// Margen mínimo contra el borde del área útil.
const MARGIN: i32 = 8;

/// Duración del vuelo de la pill.
const GLIDE_MS: u64 = 190;
/// ~120fps: por encima, el compositor descarta frames igual.
const GLIDE_STEP: Duration = Duration::from_millis(8);

/// Invalida vuelos en curso. Sin esto, dos invocaciones seguidas del atajo
/// dejaban dos hilos animando la MISMA ventana a destinos distintos, y la pill
/// terminaba temblando entre ambos.
static GLIDE_GEN: AtomicU64 = AtomicU64::new(0);

/// Dónde debe quedar una ventana flotante.
#[derive(Debug, Clone, Copy)]
pub enum Anchor {
    /// Centrada en el cursor (invocación por atajo).
    Cursor,
    /// Esquina superior izquierda exacta, en coordenadas del escritorio virtual.
    Point(i32, i32),
    /// Esquina inferior del monitor que contiene el punto (shelf de capturas).
    BottomCorner {
        near: Option<(i32, i32)>,
        left_side: bool,
    },
}

/// Posición del cursor en coordenadas del escritorio virtual.
#[cfg(windows)]
pub fn cursor_position() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    // SAFETY: POINT es POD; GetCursorPos escribe coordenadas de pantalla.
    let ok = unsafe { GetCursorPos(&mut pt) };
    if ok != 0 {
        Some((pt.x, pt.y))
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn cursor_position() -> Option<(i32, i32)> {
    None
}

/// Encaja `(x, y)` dentro del área útil del monitor que corresponde.
///
/// El monitor se elige por el **centro** de la ventana, no por su esquina: con
/// la esquina, una ventana a caballo entre dos pantallas saltaba a la otra.
pub fn clamp(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    #[cfg(windows)]
    {
        let monitors = atic_capture::monitors::enumerate();
        if monitors.is_empty() {
            return (x, y);
        }
        let ww = w.max(1);
        let hh = h.max(1);
        let (cx, cy) = (x + ww / 2, y + hh / 2);

        let target = monitors
            .iter()
            .find(|m| m.work_area.contains(cx, cy))
            .or_else(|| monitors.iter().find(|m| m.is_primary))
            .or_else(|| monitors.first());

        let Some(m) = target else {
            return (x, y);
        };
        let work = m.work_area;
        // max() contra el mínimo: en un monitor más chico que la ventana, el
        // clamp invertido la mandaba fuera de pantalla.
        let max_x = (work.right() - ww - MARGIN).max(work.x + MARGIN);
        let max_y = (work.bottom() - hh - MARGIN).max(work.y + MARGIN);
        (
            x.clamp(work.x + MARGIN, max_x),
            y.clamp(work.y + MARGIN, max_y),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (w, h);
        (x, y)
    }
}

/// Resuelve un [`Anchor`] a la esquina superior izquierda ya clampeada.
pub fn resolve(app: &AppHandle, label: &str, anchor: Anchor) -> Option<(i32, i32)> {
    let window = app.get_webview_window(label)?;
    let size = window.outer_size().ok()?;
    let (w, h) = (size.width as i32, size.height as i32);

    let (x, y) = match anchor {
        Anchor::Point(px, py) => (px, py),
        Anchor::Cursor => {
            let (cx, cy) = cursor_position()?;
            (cx - w / 2, cy - h / 2)
        }
        Anchor::BottomCorner { near, left_side } => {
            #[cfg(windows)]
            {
                let monitors = atic_capture::monitors::enumerate();
                let target = near
                    .and_then(|(px, py)| monitors.iter().find(|m| m.bounds.contains(px, py)))
                    .or_else(|| monitors.iter().find(|m| m.is_primary))
                    .or_else(|| monitors.first())?;
                let work = target.work_area;
                const CORNER_MARGIN: i32 = 16;
                let x = if left_side {
                    work.x + CORNER_MARGIN
                } else {
                    work.right() - w - CORNER_MARGIN
                };
                (x, work.bottom() - h - CORNER_MARGIN)
            }
            #[cfg(not(windows))]
            {
                let _ = (near, left_side);
                return None;
            }
        }
    };

    Some(clamp(x, y, w, h))
}

/// Coloca la ventana de inmediato. Cancela cualquier vuelo en curso.
pub fn place(app: &AppHandle, label: &str, anchor: Anchor) -> Option<(i32, i32)> {
    let target = resolve(app, label, anchor)?;
    GLIDE_GEN.fetch_add(1, Ordering::SeqCst);
    let window = app.get_webview_window(label)?;
    let _ = window.set_position(PhysicalPosition::new(target.0, target.1));
    Some(target)
}

/// Punto que se conserva al redimensionar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pivot {
    /// Esquina superior izquierda (crecimiento normal, hacia abajo-derecha).
    TopLeft,
    /// Centro: la rueda crece desde la marca en vez de desplegarse.
    Center,
    /// Barra fija: el panel crece hacia abajo, o hacia arriba si no entra.
    Panel,
}

impl Pivot {
    fn parse(raw: &str) -> Self {
        match raw {
            "center" => Pivot::Center,
            "panel" => Pivot::Panel,
            _ => Pivot::TopLeft,
        }
    }
}

/// Resultado del reencuadre: la UI necesita saber si el panel abrió hacia
/// arriba para invertir el orden visual y el redondeo de esquinas.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ResizeResult {
    pub up: bool,
}

/// Redimensiona y reubica en una sola operación.
///
/// El frontend hacía `setSize` y `setPosition` como dos IPC separados: entre
/// ambos había un frame con la ventana ya del tamaño nuevo pero todavía anclada
/// en la esquina vieja. Ese parpadeo es lo que obligaba a ocultar todo el
/// chrome durante el reencuadre. Acá ocurre todo sin ceder el hilo, así que no
/// queda frame intermedio que tapar.
#[tauri::command]
pub fn resize_floating(
    app: AppHandle,
    label: String,
    width: f64,
    height: f64,
    anchor: String,
) -> Result<ResizeResult, String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("no existe la ventana {label}"))?;

    let pivot = Pivot::parse(&anchor);
    let scale = window.scale_factor().unwrap_or(1.0);
    let prev_pos = window.outer_position().ok();
    let prev_size = window.outer_size().ok();

    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;

    let (Some(prev_pos), Some(prev_size)) = (prev_pos, prev_size) else {
        return Ok(ResizeResult { up: false });
    };

    // El tamaño físico se calcula, no se relee: `outer_size()` justo después de
    // `set_size` puede devolver el valor viejo (el resize del SO es asíncrono).
    let next_w = (width * scale).round() as i32;
    let next_h = (height * scale).round() as i32;
    let prev_w = prev_size.width as i32;
    let prev_h = prev_size.height as i32;

    let mut up = false;
    let (x, y) = match pivot {
        Pivot::TopLeft => (prev_pos.x, prev_pos.y),
        Pivot::Center => (
            prev_pos.x + (prev_w - next_w) / 2,
            prev_pos.y + (prev_h - next_h) / 2,
        ),
        Pivot::Panel => {
            // Abrir hacia abajo si el panel entra en el área útil; si no, dejar
            // la barra donde está y crecer hacia arriba.
            if fits_below(prev_pos.x, prev_pos.y, next_w, next_h) {
                (prev_pos.x, prev_pos.y)
            } else {
                up = true;
                (prev_pos.x, prev_pos.y + prev_h - next_h)
            }
        }
    };

    let (x, y) = clamp(x, y, next_w, next_h);
    let _ = window.set_position(PhysicalPosition::new(x, y));
    Ok(ResizeResult { up })
}

/// ¿Una ventana de alto `h` anclada en `(x, y)` entra en el área útil?
///
/// El monitor se busca por la esquina de la barra, que es el punto que no se
/// mueve: es el que decide si el panel cabe hacia abajo.
fn fits_below(x: i32, y: i32, w: i32, h: i32) -> bool {
    #[cfg(windows)]
    {
        let monitors = atic_capture::monitors::enumerate();
        let center_x = x + w / 2;
        let target = monitors
            .iter()
            .find(|m| m.work_area.contains(center_x, y))
            .or_else(|| monitors.iter().find(|m| m.is_primary))
            .or_else(|| monitors.first());
        let Some(m) = target else {
            return true;
        };
        y + h <= m.work_area.bottom() - MARGIN
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y, w, h);
        true
    }
}

/// Anima la ventana hasta `anchor` y devuelve el destino **al instante**.
///
/// No bloquea: el llamador (un comando de Tauri) no debe quedarse esperando el
/// vuelo. Antes `restore_pill_position` animaba en línea y dejaba el IPC
/// tomado ~190 ms en cada cierre de panel.
pub fn glide(app: &AppHandle, label: &str, anchor: Anchor) -> Option<(i32, i32)> {
    let target = resolve(app, label, anchor)?;
    let window = app.get_webview_window(label)?;
    let start = window.outer_position().ok()?;
    let (sx, sy) = (start.x, start.y);

    if (sx, sy) == target {
        return Some(target);
    }

    let generation = GLIDE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = app.clone();
    let label = label.to_string();

    std::thread::spawn(move || {
        let began = Instant::now();
        let total = Duration::from_millis(GLIDE_MS);
        loop {
            // Otro vuelo (o un place) tomó el control: abandonar sin tocar nada.
            if GLIDE_GEN.load(Ordering::SeqCst) != generation {
                return;
            }
            let elapsed = began.elapsed();
            // Progreso por reloj, no por número de frame: así una pausa del SO
            // no alarga el vuelo, solo le saca frames.
            let t = (elapsed.as_secs_f64() / total.as_secs_f64()).min(1.0);
            // Ease-out cúbico: sale rápido y frena contra el destino.
            let e = 1.0 - (1.0 - t).powi(3);
            let x = sx as f64 + f64::from(target.0 - sx) * e;
            let y = sy as f64 + f64::from(target.1 - sy) * e;

            let Some(win) = handle.get_webview_window(&label) else {
                return;
            };
            let _ = win.set_position(PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));

            if t >= 1.0 {
                return;
            }
            std::thread::sleep(GLIDE_STEP);
        }
    });

    Some(target)
}
