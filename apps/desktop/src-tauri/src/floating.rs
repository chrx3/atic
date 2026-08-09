//! Dueño único de la geometría de las ventanas flotantes (pill, shelf, overlay).
//!
//! Antes cada ventana resolvía lo suyo y el frontend clampeaba **otra vez** por
//! su cuenta: los dos clamps se pisaban y la pill se desplazaba un poco en cada
//! ciclo abrir/cerrar. Acá vive el único clamp, el único concepto de "monitor
//! correcto" y el único tween.
//!
//! Regla: el frontend declara *intención* ("al cursor", "a su hogar"); nunca
//! calcula coordenadas ni llama a `setPosition`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use atic_core::MutexExt;
use tauri::{AppHandle, Manager, PhysicalPosition};

/// Margen mínimo contra el borde del área útil.
const MARGIN: i32 = 0;

/// Duración del vuelo de la pill.
pub const GLIDE_MS: u64 = 190;
/// ~120fps: por encima, el compositor descarta frames igual.
const GLIDE_STEP: Duration = Duration::from_millis(8);

/// Invalida vuelos en curso. Sin esto, dos invocaciones seguidas del atajo
/// dejaban dos hilos animando la MISMA ventana a destinos distintos, y la pill
/// terminaba temblando entre ambos.
///
/// **Por ventana, no global.** Era un solo contador para toda la app, y con una
/// sola ventana flotante nadie lo notaba. Al sumar la burbuja de agentes se
/// volvió un cruce: la pill se reencuadra sola todo el tiempo —entra el timer,
/// se cierra la rueda— y cada uno de esos reencuadres MATABA el vuelo de la
/// burbuja a mitad de camino, dejándola congelada a medio crecer con el
/// contenido recortado. Cancelar un vuelo solo puede ser asunto de la ventana
/// que lo hace.
static GLIDE_GEN: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

/// Sube la generación de `label` y devuelve la nueva.
fn bump_gen(label: &str) -> u64 {
    let mut guard = GLIDE_GEN.lock_or_recover();
    let map = guard.get_or_insert_with(HashMap::new);
    let next = map.get(label).copied().unwrap_or(0) + 1;
    map.insert(label.to_string(), next);
    next
}

/// La generación viva de `label`.
fn gen_of(label: &str) -> u64 {
    GLIDE_GEN
        .lock_or_recover()
        .as_ref()
        .and_then(|m| m.get(label).copied())
        .unwrap_or(0)
}

/// Destino del tween en curso de cada ventana, con la generación que lo emitió.
///
/// Responde "¿dónde va a quedar esto cuando se aquiete?" mientras se mueve. La
/// generación es lo que lo hace confiable: si ya no coincide con la viva, el
/// tween murió y el dato es basura.
static TWEEN_DEST: Mutex<Option<HashMap<String, (u64, Rect)>>> = Mutex::new(None);

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

/// Encaja `(x, y)` dentro del monitor (o del escritorio virtual).
///
/// Usa **`bounds`** (pantalla completa), no `work_area`: la pill y los floats
/// pueden solapar la barra de tareas y pegarse al borde — hace falta para un
/// modo tipo Dynamic Island. El monitor se elige por el **centro**; si el
/// centro cae en el hueco entre pantallas, se clampea al escritorio virtual.
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

        let rect = monitors
            .iter()
            .find(|m| m.bounds.contains(cx, cy))
            .map(|m| m.bounds)
            .unwrap_or_else(atic_capture::monitors::virtual_screen);

        // max() contra el mínimo: en un monitor más chico que la ventana, el
        // clamp invertido la mandaba fuera de pantalla.
        let max_x = (rect.right() - ww - MARGIN).max(rect.x + MARGIN);
        let max_y = (rect.bottom() - hh - MARGIN).max(rect.y + MARGIN);
        (
            x.clamp(rect.x + MARGIN, max_x),
            y.clamp(rect.y + MARGIN, max_y),
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

/// Traza de geometría: cada línea es UNA escritura de posición, y leídas en
/// orden reconstruyen el recorrido completo de la ventana.
///
/// Va en `debug` para no ensuciar el log normal, pero se queda en el código: es
/// la única forma práctica de depurar una ventana flotante —dónde está y quién
/// la movió no se puede inferir mirando el código, porque hay varios escritores
/// (el reconciliador, los tweens, el clamp del monitor). Se enciende con:
///
/// ```text
/// RUST_LOG=info,pill_geo=debug pnpm tauri dev
/// ```
macro_rules! geo {
    ($($arg:tt)*) => {
        tracing::debug!(target: "pill_geo", $($arg)*)
    };
}

/// Posición y tamaño actuales, para las trazas.
fn snapshot(app: &AppHandle, label: &str) -> ((i32, i32), (i32, i32)) {
    let Some(w) = app.get_webview_window(label) else {
        return ((0, 0), (0, 0));
    };
    let pos = w.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    let size = w
        .outer_size()
        .map(|s| (s.width as i32, s.height as i32))
        .unwrap_or((0, 0));
    (pos, size)
}

/// Aplica posición y tamaño en UNA sola llamada al SO.
///
/// `set_size` + `set_position` son dos `SetWindowPos`, y DWM puede componer un
/// frame entre medio: la ventana ya del tamaño nuevo, todavía anclada en la
/// esquina vieja. Eso es lo que se veía como una "tercera posición" al abrir la
/// rueda. Un único `SetWindowPos` con ambos valores no deja ese hueco.
///
/// Devuelve `false` si no se pudo aplicar; el llamador cae al camino portable.
#[cfg(windows)]
fn set_bounds(window: &tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    // SAFETY: el HWND es válido mientras viva la ventana y `SetWindowPos` no
    // retiene el puntero. Las banderas evitan tocar foco y orden Z.
    let ok = unsafe {
        SetWindowPos(
            hwnd.0 as _,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
        )
    };
    ok != 0
}

/// Coloca la ventana de inmediato. Cancela cualquier vuelo en curso.
pub fn place(app: &AppHandle, label: &str, anchor: Anchor) -> Option<(i32, i32)> {
    let target = resolve(app, label, anchor)?;
    let generation = bump_gen(label);
    let (from, size) = snapshot(app, label);
    let window = app.get_webview_window(label)?;
    let _ = window.set_position(PhysicalPosition::new(target.0, target.1));
    geo!(
        "PLACE      {label} anchor={anchor:?} size={size:?} {from:?} -> {target:?} gen={generation}"
    );
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
    /// Esquina inferior izquierda. Es el pivote del colapso de un panel que
    /// había abierto hacia arriba: ahí la barra vive en el borde de abajo de la
    /// ventana, así que preservar la esquina superior la haría saltar.
    BottomLeft,
    /// No es un pivote relativo: centra la ventana **ya redimensionada** en el
    /// cursor. Existe para que crecer e ir al cursor sean UNA escritura de
    /// posición. Cuando eran dos (`resize` + `place`), entre medio se pintaba
    /// un frame con la rueda crecida sobre la pill vieja, 115 px arriba y a la
    /// izquierda: la "tercera posición" en la que aparecía antes de la rueda.
    Cursor,
}

impl Pivot {
    fn parse(raw: &str) -> Self {
        match raw {
            "center" => Pivot::Center,
            "panel" => Pivot::Panel,
            "bottomLeft" => Pivot::BottomLeft,
            "cursor" => Pivot::Cursor,
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
    animate: bool,
) -> Result<ResizeResult, String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("no existe la ventana {label}"))?;

    let pivot = Pivot::parse(&anchor);

    // Un vuelo en curso también escribe la posición, desde otro hilo y con las
    // coordenadas calculadas para el tamaño VIEJO. Si no se cancela acá, los
    // dos escritores se pisan durante ~190 ms y la ventana termina donde la
    // dejó el que perdió la carrera. Redimensionar manda: invalida el vuelo.
    let cancelled = bump_gen(&label);

    let scale = window.scale_factor().unwrap_or(1.0);
    let prev_pos = window.outer_position().ok();
    let prev_size = window.outer_size().ok();

    // Sin geometría previa no hay pivote posible: redimensionar y salir.
    let (Some(prev_pos), Some(prev_size)) = (prev_pos, prev_size) else {
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())?;
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
        Pivot::BottomLeft => {
            up = true;
            (prev_pos.x, prev_pos.y + prev_h - next_h)
        }
        // Centrado con el tamaño NUEVO. Ese era el punto de todo: hacerlo con
        // el viejo dejaba la ventana descentrada respecto del puntero.
        Pivot::Cursor => match cursor_position() {
            Some((cx, cy)) => (cx - next_w / 2, cy - next_h / 2),
            // Sin cursor (o fuera de Windows), no mover: mejor quieta que en
            // una coordenada inventada.
            None => (prev_pos.x, prev_pos.y),
        },
    };

    let (cx, cy) = clamp(x, y, next_w, next_h);

    // `cursor` siempre anima: ir al puntero y crecer son el MISMO movimiento.
    // Los pivotes relativos saltan salvo que el llamador pida lo contrario —lo
    // pide para los cambios de estado de la barra compacta (disco ↔ dictado ↔
    // grabación), donde el salto se leía como un parpadeo.
    if pivot == Pivot::Cursor || animate {
        tween(
            &app,
            &label,
            Rect {
                x: cx,
                y: cy,
                w: next_w,
                h: next_h,
            },
        );
        return Ok(ResizeResult { up });
    }

    // Camino atómico primero; el portable queda de reserva.
    #[cfg(windows)]
    let atomic = set_bounds(&window, cx, cy, next_w, next_h);
    #[cfg(not(windows))]
    let atomic = false;

    if !atomic {
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())?;
        let _ = window.set_position(PhysicalPosition::new(cx, cy));
    }
    // `pre` vs `post` separa el pivote del clamp: si difieren, la ventana la
    // corrió el borde del monitor, no el pivote elegido.
    geo!(
        "RESIZE     {label} pivot={pivot:?} size=({prev_w},{prev_h})->({next_w},{next_h}) \
         pos=({},{})->pre=({x},{y}) post=({cx},{cy}) up={up} cancel_gen={cancelled} \
         atomico={atomic}",
        prev_pos.x,
        prev_pos.y
    );
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

/// Destino de un vuelo y cuánto va a durar.
///
/// La duración se devuelve para que el frontend pueda **esperar** a que la
/// ventana aterrice antes de redimensionarla. Sin ese dato solo le quedaba
/// adivinar, y redimensionar a mitad del vuelo es justo lo que hacía que el
/// panel creciera en un punto intermedio en vez de en el destino.
#[derive(Debug, Clone, Copy)]
pub struct Flight {
    pub x: i32,
    pub y: i32,
    /// Milisegundos de vuelo; 0 si ya estaba en el destino.
    pub ms: u64,
}

/// Rectángulo de ventana en píxeles físicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Anima la ventana desde su rectángulo actual hasta `to`, y devuelve el
/// destino **al instante**.
///
/// Interpola posición Y tamaño a la vez. Eso es lo que convierte abrir la rueda
/// en UN movimiento en vez de "viajar" y después "crecer": no existe ningún
/// instante en que la ventana esté quieta en un tamaño intermedio, así que no
/// hay una tercera posición que percibir. Cada frame aplica el rectángulo
/// completo de una sola llamada, así que tampoco queda un frame con el tamaño
/// nuevo sobre la posición vieja.
///
/// No bloquea: el llamador (un comando de Tauri) no debe quedarse esperando.
pub fn tween(app: &AppHandle, label: &str, to: Rect) -> Option<Flight> {
    let window = app.get_webview_window(label)?;
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let from = Rect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    };

    let generation = bump_gen(label);

    if from == to {
        geo!("TWEEN.SKIP {label} ya estaba en {to:?} gen={generation}");
        return Some(Flight {
            x: to.x,
            y: to.y,
            ms: 0,
        });
    }

    geo!("TWEEN.INIT {label} {from:?} -> {to:?} gen={generation}");

    // Publicar el destino ANTES de arrancar: quien pregunte dónde va a quedar
    // la ventana tiene que poder saberlo desde el primer frame.
    if let Ok(mut dest) = TWEEN_DEST.lock() {
        dest.get_or_insert_with(HashMap::new)
            .insert(label.to_string(), (generation, to));
    }

    let handle = app.clone();
    let label = label.to_string();

    std::thread::spawn(move || {
        let began = Instant::now();
        let total = Duration::from_millis(GLIDE_MS);
        let mut frames = 0u32;
        loop {
            // Otro tween (o un place) tomó el control: abandonar sin tocar nada.
            let live = gen_of(&label);
            if live != generation {
                let (at, sz) = snapshot(&handle, &label);
                geo!(
                    "TWEEN.KILL {label} gen={generation} lo_mato={live} at={at:?} \
                     size={sz:?} destino={to:?} frames={frames}"
                );
                return;
            }
            let elapsed = began.elapsed();
            // Progreso por reloj, no por número de frame: así una pausa del SO
            // no alarga la animación, solo le saca frames.
            let t = (elapsed.as_secs_f64() / total.as_secs_f64()).min(1.0);
            // Ease-out cúbico: sale rápido y frena contra el destino.
            let e = 1.0 - (1.0 - t).powi(3);
            let lerp = |a: i32, b: i32| (f64::from(a) + f64::from(b - a) * e).round() as i32;
            let now = Rect {
                x: lerp(from.x, to.x),
                y: lerp(from.y, to.y),
                w: lerp(from.w, to.w),
                h: lerp(from.h, to.h),
            };

            let Some(win) = handle.get_webview_window(&label) else {
                return;
            };
            #[cfg(windows)]
            {
                set_bounds(&win, now.x, now.y, now.w, now.h);
            }
            #[cfg(not(windows))]
            {
                let _ = win.set_position(PhysicalPosition::new(now.x, now.y));
                let _ = win.set_size(tauri::PhysicalSize::new(now.w as u32, now.h as u32));
            }
            frames += 1;

            if t >= 1.0 {
                geo!(
                    "TWEEN.DONE {label} gen={generation} at={to:?} frames={frames} ms={}",
                    began.elapsed().as_millis()
                );
                return;
            }
            std::thread::sleep(GLIDE_STEP);
        }
    });

    Some(Flight {
        x: to.x,
        y: to.y,
        ms: GLIDE_MS,
    })
}

/// Dónde va a quedar la ventana cuando todo se aquiete.
///
/// Devuelve el destino del tween en curso, o la posición actual si no hay
/// ninguno. Existe para guardar el "hogar" de la pill: leer la posición viva
/// solo es correcto con la ventana quieta. Spameando el atajo, cada apertura
/// capturaba un punto intermedio del cierre anterior y el hogar se iba
/// caminando por la pantalla — la misma deriva de antes, por otra puerta.
pub fn resting_position(app: &AppHandle, label: &str) -> Option<(i32, i32)> {
    if let Ok(dest) = TWEEN_DEST.lock() {
        if let Some((generation, rect)) = dest.as_ref().and_then(|m| m.get(label)) {
            // La generación viva es la prueba de que el tween sigue corriendo:
            // si otro lo canceló, su destino ya no dice nada.
            if gen_of(label) == *generation {
                return Some((rect.x, rect.y));
            }
        }
    }
    let window = app.get_webview_window(label)?;
    let pos = window.outer_position().ok()?;
    Some((pos.x, pos.y))
}

/// Vuela hasta `anchor` conservando el tamaño. Caso particular de [`tween`].
pub fn glide(app: &AppHandle, label: &str, anchor: Anchor) -> Option<Flight> {
    let (x, y) = resolve(app, label, anchor)?;
    let window = app.get_webview_window(label)?;
    let size = window.outer_size().ok()?;
    tween(
        app,
        label,
        Rect {
            x,
            y,
            w: size.width as i32,
            h: size.height as i32,
        },
    )
}

/// Dónde quedó la punta de una burbuja y a qué distancia de su esquina.
///
/// La punta es la que convierte una ventana en un globo de diálogo: dice de
/// dónde salió. Si la burbuja se corre para no salirse de la pantalla, la punta
/// tiene que quedarse apuntando al origen igual, así que su posición se calcula
/// aparte y no se deduce del centro.
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BubbleAnchor {
    /// Lado de la burbuja donde va la punta: `top`, `bottom`, `left`, `right`.
    /// Es el lado que mira al origen.
    pub side: &'static str,
    /// Píxeles desde la esquina superior izquierda de la burbuja hasta el
    /// centro de la punta, a lo largo del lado.
    pub offset: i32,
}

/// Las medidas de diseño de una burbuja, en píxeles **lógicos**.
///
/// Van juntas porque se leen juntas: son las gemelas de las del CSS, y la
/// conversión a físicos pasa una sola vez, dentro de [`bubble_rect`].
///
/// Ya no hay `inset`. Existía porque una ventana recorta su propia sombra, así
/// que el globo vivía dentro de un marco transparente de 62 px y toda la
/// geometría tenía que descontarlo. Dentro del overlay no hay ventana que
/// recortar: el rectángulo que sale de acá es el del globo y nada más.
#[derive(Clone, Copy)]
pub struct BubbleShape {
    /// Tamaño del globo.
    pub w: i32,
    pub h: i32,
    /// Separación entre el origen y el globo.
    pub gap: i32,
    /// Radio de las esquinas del globo: la punta no puede caer sobre ellas.
    pub corner: i32,
}

/// Calcula dónde va la burbuja y dónde cae su punta. **No mueve nada.**
///
/// Separado de aplicarlo porque el destino hace falta ANTES: la ventana sale
/// del tamaño de la pill y crece hasta acá, así que quien la abre necesita el
/// rectángulo final para poder interpolar hacia él.
///
/// Prueba abajo, arriba, derecha e izquierda en ese orden y se queda con el
/// primer lado donde entra entera. El orden no es arbitrario: la pill suele
/// vivir en la mitad superior y arrastrarse poco, así que abajo acierta casi
/// siempre; probar primero el lado con más espacio haría que la burbuja
/// cambiara de lado por unos pocos píxeles de diferencia entre aperturas.
///
/// En el lado elegido se ancla por **esquina**, no al centro: el panel no debe
/// quedar debajo/al lado centrado tapando la pill, sino salir de ella y
/// tocarse solo por el cuello en una esquina.
#[cfg(windows)]
/// El origen llega como RECTÁNGULO y ya no como etiqueta de ventana.
///
/// La pill dejó de ser una ventana —vive dentro del overlay—, así que
/// `get_webview_window("pill")` devolvía `None` y la burbuja no se mostraba.
/// Quien llama sabe dónde está la pill (`overlay::pill_rect()`); acá solo se
/// hace la geometría.
pub fn bubble_rect(
    app: &AppHandle,
    origin_rect: Rect,
    shape: BubbleShape,
) -> Option<(Rect, BubbleAnchor)> {
    // La forma llega en píxeles LÓGICOS y todo lo demás acá es físico. A escala
    // 100% son el mismo número y la diferencia no existe; a 125% la burbuja
    // quedaba pegada a la pill y la punta caía sobre la esquina redondeada.
    let scale = crate::overlay::scale(app);
    let fisico = |v: i32| (v as f64 * scale).round() as i32;
    let (gap, corner) = (fisico(shape.gap), fisico(shape.corner));

    // El tamaño lo dice quien llama, y NO se mide nada: al cerrarse, la burbuja
    // se repliega sobre la pill. Midiéndola, la segunda apertura crecía hasta
    // el tamaño de la pill y no había pulsación que la recuperara. El destino
    // es un dato de diseño, no el estado que quedó.
    let (bw, bh) = (fisico(shape.w), fisico(shape.h));

    let (ax, ay, aw, ah) = (origin_rect.x, origin_rect.y, origin_rect.w, origin_rect.h);
    let (acx, acy) = (ax + aw / 2, ay + ah / 2);

    let monitors = atic_capture::monitors::enumerate();
    let work = monitors
        .iter()
        .find(|m| m.work_area.contains(acx, acy))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first())
        .map(|m| m.work_area)?;

    let fits_below = ay + ah + gap + bh + MARGIN <= work.bottom();
    let fits_above = ay - gap - bh - MARGIN >= work.y;
    let fits_right = ax + aw + gap + bw + MARGIN <= work.right();

    // Ancla por el BORDE de la pill, no por su centro: el panel crece hacia
    // un lado y solo se tocan en una esquina (cuello líquido). Centrar hacía
    // que la pill quedara encima del panel.
    // `near` = pill en la esquina “de inicio” del lado; `far` = espejo.
    let along_axis = |near: i32, far: i32, size: i32, lo: i32, hi: i32| -> i32 {
        if near >= lo + MARGIN && near + size + MARGIN <= hi {
            near
        } else if far >= lo + MARGIN && far + size + MARGIN <= hi {
            far
        } else {
            near
        }
    };

    // (x, y del globo, lado del globo que mira al origen)
    let (mut x, mut y, side) = if fits_below {
        // Pill arriba-izquierda del panel (crece a la derecha); si no, arriba-derecha.
        let x = along_axis(
            ax + aw - corner,
            ax - bw + corner,
            bw,
            work.x,
            work.right(),
        );
        (x, ay + ah + gap, "top")
    } else if fits_above {
        let x = along_axis(
            ax + aw - corner,
            ax - bw + corner,
            bw,
            work.x,
            work.right(),
        );
        (x, ay - gap - bh, "bottom")
    } else if fits_right {
        // Pill izquierda-arriba del panel (crece hacia abajo).
        let y = along_axis(
            ay + ah - corner,
            ay - bh + corner,
            bh,
            work.y,
            work.bottom(),
        );
        (ax + aw + gap, y, "left")
    } else {
        let y = along_axis(
            ay + ah - corner,
            ay - bh + corner,
            bh,
            work.y,
            work.bottom(),
        );
        (ax - gap - bw, y, "right")
    };

    // Clamp solo en el eje cruzado: si también corriéramos el eje del cuello,
    // el panel se alejaba metros de la pill o se le montaba encima.
    let (cx, cy) = clamp(x, y, bw, bh);
    match side {
        "top" | "bottom" => x = cx,
        "left" | "right" => y = cy,
        _ => {
            x = cx;
            y = cy;
        }
    }

    // La punta no puede caer sobre una esquina redondeada: se vería despegada
    // del borde. `corner` es el radio, y se le suma el ancho de la propia punta.
    let along = if side == "top" || side == "bottom" {
        (acx - x).clamp(corner, (bw - corner).max(corner))
    } else {
        (acy - y).clamp(corner, (bh - corner).max(corner))
    };

    geo!("BUBBLE     agentes <- pill lado={side} off={along} en {x},{y} tam {bw}x{bh}");
    Some((
        Rect { x, y, w: bw, h: bh },
        BubbleAnchor {
            side,
            offset: along,
        },
    ))
}

#[cfg(not(windows))]
pub fn bubble_rect(
    _app: &AppHandle,
    _origin_rect: Rect,
    _shape: BubbleShape,
) -> Option<(Rect, BubbleAnchor)> {
    None
}
