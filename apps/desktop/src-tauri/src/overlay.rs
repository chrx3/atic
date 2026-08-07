//! La ventana overlay: una sola superficie para todo lo que se funde.
//!
//! Existe por una razón concreta: un filtro SVG solo alcanza lo que está en el
//! mismo `document`. Mientras la pill y la burbuja de agentes fueron ventanas
//! distintas, ninguna cantidad de CSS podía unirlas — el cuello de la burbuja
//! era un puente pintado, no una fusión. Compartiendo ventana, sí lo es.
//!
//! El efecto secundario es más grande que la causa: casi toda la coreografía de
//! `floating.rs` y `pillStage.ts` existe porque dos escritores asíncronos —Rust
//! moviendo la ventana, el webview animando el contenido— comparten un
//! rectángulo. Acá el rectángulo del overlay es **constante** mientras se usa,
//! y las superficies se mueven con `transform`. Esa clase de carrera desaparece.
//!
//! ## Un solo régimen para los ex-styles: los maneja `tao`
//!
//! `tao` reescribe `GWL_EXSTYLE` **entero** desde su propio juego de banderas
//! cada vez que cambia una (`WindowFlags::apply_diff`). Así que un
//! `WS_EX_TRANSPARENT` puesto con `SetWindowLongPtrW` se borra solo en el
//! próximo `show()`, `set_always_on_top()` o `set_focusable()` — sin aviso.
//!
//! Ya pasó: la primera versión ponía el bit a mano y después llamaba a
//! `show()`. El overlay quedaba opaco al mouse y bloqueaba el escritorio entero.
//!
//! Por eso acá se usa `set_ignore_cursor_events()`, que es una bandera que
//! `tao` conoce y por lo tanto **sobrevive** a los demás cambios. Es más cara
//! —va por `PostMessage` y termina en un `SetWindowPos(SWP_FRAMECHANGED)`— y
//! puede que en el camino caliente del armado por rectángulos haya que
//! escribir el bit a mano. Si ese día llega, hay que pasar los DOS bits a mano
//! (el de cursor y el de foco) y re-aplicarlos tras cada cambio de bandera:
//! mezclarlos rompe en las dos direcciones.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager};

pub const LABEL: &str = "overlay";

/// Margen alrededor de los rectángulos para armar un poco antes de llegar.
///
/// Existe porque reaccionar al botón siempre llega tarde: Windows decide a qué
/// ventana va el clic al procesar la entrada, antes de entregarnos la copia
/// `RIDEV_INPUTSINK`. Hay que estar armado ANTES.
///
/// 6 px y no un margen escalado por velocidad: la banda armada pero vacía se
/// traga los clics que caen en ella, así que es una zona muerta. A 6 px es
/// imperceptible; escalarlo con la velocidad daría hasta 96 px de borde muerto
/// alrededor de cada superficie. Si se mide que se pierde el primer clic
/// moviendo rápido, la salida no es agrandar esto sino un subclass de
/// `WM_NCHITTEST` que devuelva `HTTRANSPARENT` fuera de los rects reales.
const ARM_MARGIN: f64 = 6.0;

/// Cada cuánto se consulta la posición del cursor, en milisegundos.
///
/// El hilo de Raw Input recibe un paquete por reporte del mouse: entre 125 y
/// 1000 por segundo. Sin este freno, un mouse gamer costaría mil `GetCursorPos`
/// por segundo para nada.
const SAMPLE_MS: i64 = 4;

/// Zonas que SÍ deben recibir el mouse, en físicos del escritorio virtual.
static HIT_RECTS: Mutex<Vec<PhysRect>> = Mutex::new(Vec::new());

/// ¿Debería el overlay estar recibiendo el mouse ahora mismo?
static ARMED: AtomicBool = AtomicBool::new(false);

/// Lo último que se le APLICÓ de verdad a la ventana. `true` = deja pasar.
///
/// Separado de `ARMED` porque son dos cosas: uno es la intención, que se
/// calcula en la wndproc, y el otro el hecho, que aplica el worker. Compararlos
/// es lo único que detecta una desincronía — y una desincronía acá deja el
/// escritorio ENTERO sin recibir clics, porque el overlay lo cubre todo.
static CLICK_THROUGH: AtomicBool = AtomicBool::new(true);

/// Hay una sesión de captura en curso: el overlay se aparta.
///
/// La captura pone su propia ventana always-on-top encima de la pantalla
/// congelada para que elijas la región. Este overlay también es always-on-top y
/// se pone al frente cada vez que se arma, así que sobre la pill o sobre la
/// consola le robaba el mouse: ahí no se podía arrastrar la selección.
///
/// No se esconde: la captura ya se tomó, la consola SALE en la imagen, y lo
/// único que sobra es que siga recibiendo clics.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Entra o sale del modo captura.
pub fn set_capturing(app: &AppHandle, on: bool) {
    if CAPTURING.swap(on, Ordering::AcqRel) == on {
        return;
    }
    if on {
        ARMED.store(false, Ordering::Release);
    }
    send(Msg::Sync);
    let _ = app;
}

/// Reafirma click-through del overlay de la pill mientras hay captura.
///
/// `show`/`set_always_on_top` del overlay de captura pueden dejar el de la pill
/// otra vez encima y opaco; sin esto, el mouse sobre la ventana principal no
/// llega a la selección hasta un clic “afuera”.
pub fn reassert_capturing_input() {
    if !CAPTURING.load(Ordering::Acquire) {
        return;
    }
    ARMED.store(false, Ordering::Release);
    send(Msg::Sync);
}

/// Momento de la última muestra, para el freno de `SAMPLE_MS`.
static LAST_SAMPLE: AtomicI64 = AtomicI64::new(0);

/// Canal hacia el worker. La wndproc decide; el worker hace.
static TOGGLE_TX: OnceLock<SyncSender<Msg>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
enum Msg {
    /// Algo cambió en el armado. No lleva el valor a propósito: el worker lee
    /// `ARMED`, porque este canal descarta cuando está lleno y aplicar un valor
    /// viejo es exactamente cómo la ventana se quedaba opaca para siempre.
    Sync,
    /// Clic fuera de todas las zonas: para el frontend es «cerrá lo que tengas».
    Outside,
}

#[derive(Debug, Clone, Copy)]
struct PhysRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl PhysRect {
    fn contains(&self, x: f64, y: f64, margin: f64) -> bool {
        x >= self.x - margin
            && y >= self.y - margin
            && x <= self.x + self.w + margin
            && y <= self.y + self.h + margin
    }
}

/// Rectángulo en píxeles CSS, relativo a la esquina del overlay.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HitRect {
    /// Quién lo publica. Rust necesita distinguir la pill: la burbuja de
    /// agentes cuelga de ella y hasta ahora leía su rectángulo preguntándole a
    /// una ventana que ya no existe.
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Último rectángulo conocido de la pill, en físicos del escritorio.
static PILL_RECT: Mutex<Option<crate::floating::Rect>> = Mutex::new(None);

/// Dónde está la pill. Reemplaza a `floating::rect_of(app, "pill")`.
pub fn pill_rect() -> Option<crate::floating::Rect> {
    PILL_RECT.lock().ok().and_then(|g| *g)
}

/// Rectángulo del overlay, en píxeles físicos del escritorio virtual.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct OverlayRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Escala del monitor que cubre. El frontend la necesita para traducir
    /// medidas que Rust todavía razona en físicos.
    pub scale: f64,
}

/// Coloca el overlay sobre TODO el escritorio virtual y lo muestra.
///
/// El plan original lo ponía sobre un solo monitor y lo movía cuando la pill
/// cruzaba a otro. Se descartó al probarlo: arrastrar una superficie hacia el
/// borde la recorta contra el canto del overlay durante todo el trecho que va
/// desde que asoma hasta que su centro cruza —medio ancho de la forma— y al
/// llegar al otro monitor hay que reubicarla bajo el cursor sin romper la
/// captura del puntero. Mucha maquinaria y un artefacto visible, para resolver
/// un caso que abarcando todo simplemente no existe.
///
/// El precio es el DPI: una ventana tiene UNA escala, la del monitor donde
/// Windows la considere. Con monitores de escalas distintas, las superficies se
/// dibujan con la escala equivocada en todos menos uno. Por eso abajo se avisa
/// si las escalas no coinciden — es la condición que hace falsa esta decisión.
/// Crea la ventana. Se hace acá y no en `tauri.conf.json` a propósito.
///
/// Las ventanas declaradas en la config nacen ANTES de que corra `setup()`, y
/// sus webviews empiezan a cargar enseguida — o sea que pueden invocar comandos
/// mientras `setup()` todavía está abriendo la base y leyendo la config, antes
/// del `manage()`. Esa carrera ya existía; sumar un webview más la volcó, y la
/// app arrancaba con «state not managed for field `state`».
///
/// Creándola desde su propio módulo, después del `manage()`, el overlay deja de
/// competir en el arranque. (La carrera de fondo sigue latente para las otras
/// ventanas: la salida de verdad es mover el `manage()` a la cadena del
/// `Builder`, antes de `setup()`.)
fn create(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        return Some(existing);
    }
    tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("overlay".into()))
        .title("Atic")
        .inner_size(480.0, 320.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focusable(false)
        .visible(false)
        .build()
        .map_err(|err| tracing::error!(target: "overlay", ?err, "no se pudo crear la ventana"))
        .ok()
}

pub fn place(app: &AppHandle) -> Option<OverlayRect> {
    #[cfg(windows)]
    {
        let window = create(app)?;

        let monitors = atic_capture::monitors::enumerate();
        let mixed = monitors
            .iter()
            .any(|m| (m.scale - monitors[0].scale).abs() > f64::EPSILON);
        if mixed {
            let escalas: Vec<String> = monitors
                .iter()
                .map(|m| format!("{}={}", m.id, m.scale))
                .collect();
            tracing::warn!(
                target: "overlay",
                "monitores con escalas distintas ({}): las superficies van a \
                 dibujarse con la escala de uno solo",
                escalas.join(" ")
            );
        }

        let vs = atic_capture::monitors::virtual_screen();
        let rect = OverlayRect {
            x: vs.x,
            y: vs.y,
            w: vs.width as i32,
            h: vs.height as i32,
            scale: monitors.first().map(|m| m.scale).unwrap_or(1.0),
        };

        let _ = window.set_position(tauri::PhysicalPosition::new(rect.x, rect.y));
        let _ = window.set_size(tauri::PhysicalSize::new(rect.w as u32, rect.h as u32));
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        // Al final y no antes: `show()` y `set_always_on_top()` son cambios de
        // bandera, y cada uno reescribe el ex-style entero. El orden solo es
        // seguro porque este bit también es de `tao` — si algún día se escribe
        // a mano, hay que re-aplicarlo después de cada uno de ellos.
        set_click_through(&window, true);

        tracing::info!(
            target: "overlay",
            monitores = monitors.len(),
            escalas_mixtas = mixed,
            "overlay en {},{} {}x{}", rect.x, rect.y, rect.w, rect.h
        );
        Some(rect)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Vuelve a poner el overlay al frente, sin activarlo.
///
/// Solo Z-order (`SetWindowPos`). No llama a `set_always_on_top`: tao
/// reescribe `GWL_EXSTYLE` entero en cada cambio de bandera, y si eso corre
/// en el camino caliente del armado puede dejar `WS_EX_TRANSPARENT` pegado
/// mientras el atomic ya dice lo contrario — la pill se ve encima pero el
/// mouse atraviesa hacia las apps de abajo.
///
/// `SWP_NOACTIVATE` es lo importante: subir la ventana no debe robarle el foco
/// a la app en la que estés escribiendo.
///
/// El overlay (pill + floats) queda siempre topmost: la pill no puede hundirse
/// al desfijar un float porque comparten ventana. El pin solo afecta dismiss.
pub fn raise(app: &AppHandle) {
    if CAPTURING.load(Ordering::Acquire) {
        return;
    }
    let on = crate::agents::bridge::overlay_should_be_topmost();
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        let Some(window) = app.get_webview_window(LABEL) else {
            return;
        };
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let insert_after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: el HWND lo da Tauri y vive mientras viva la ventana.
        unsafe {
            SetWindowPos(
                hwnd.0 as _,
                insert_after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, on);
    }
}

/// Aplica o quita always-on-top del overlay.
///
/// No pelea con el overlay de captura: ahí `CAPTURING` manda y tocar el
/// stacking deja la selección sin mouse.
///
/// Tras `set_always_on_top` hay que reponer el click-through: ese cambio
/// reescribe los ex-styles y puede pisar `ignore_cursor_events`.
pub fn set_topmost(app: &AppHandle, on: bool) {
    if CAPTURING.load(Ordering::Acquire) {
        return;
    }
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let _ = window.set_always_on_top(on);
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        if let Ok(hwnd) = window.hwnd() {
            let insert_after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
            // SAFETY: el HWND lo da Tauri y vive mientras viva la ventana.
            unsafe {
                SetWindowPos(
                    hwnd.0 as _,
                    insert_after,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
    // Intención actual del worker de armado, no el último valor aplicado:
    // si hubo carrera, el atomic de estilo puede mentir.
    let through = !ARMED.load(Ordering::Acquire);
    set_click_through(&window, through);
    CLICK_THROUGH.store(through, Ordering::Release);
}

/// Deja pasar el mouse a lo que haya debajo, o lo intercepta.
///
/// `true` = el overlay no existe para el hit-testing de Windows.
pub fn set_click_through(window: &tauri::WebviewWindow, on: bool) {
    if let Err(err) = window.set_ignore_cursor_events(on) {
        tracing::warn!(target: "overlay", ?err, "no se pudo cambiar el click-through");
    }
}

/// Arranque: colocar, mostrar y dejar el worker escuchando.
pub fn setup(app: &AppHandle) {
    if place(app).is_none() {
        tracing::warn!(target: "overlay", "no se pudo colocar el overlay");
    }
    start_toggle_worker(app.clone());
}

/// Hilo que aplica el click-through.
///
/// Aparte del de Raw Input porque `set_ignore_cursor_events` no es apto para
/// una wndproc: va por `PostMessage` y termina en `SetWindowPos`. La wndproc
/// solo decide y despacha; acá se hace el trabajo.
///
/// Canal de capacidad 1 con `try_send`: si el worker está ocupado, el aviso se
/// descarta. No se pierde nada — el siguiente movimiento del mouse vuelve a
/// evaluar, y lo que importa es el estado final, no la secuencia.
fn start_toggle_worker(app: AppHandle) {
    use tauri::Emitter;

    // Capacidad 2 y no 1: un clic afuera manda `Outside` y, si además el cursor
    // venía de salir de una zona, `Sync`. Con capacidad 1 uno de los dos se
    // perdía, y el que se perdía era siempre el segundo.
    //
    // Que siga descartando cuando se llena está bien: `Sync` no lleva datos, y
    // el worker converge al estado actual. Lo único que cuesta un aviso perdido
    // es esperar al siguiente movimiento del mouse.
    let (tx, rx) = sync_channel::<Msg>(2);
    if TOGGLE_TX.set(tx).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("atic-overlay-hit".into())
        .spawn(move || {
            while let Ok(msg) = rx.recv() {
                match msg {
                    // El mensaje solo DESPIERTA; lo que se aplica es `ARMED`.
                    //
                    // Reproducir el mensaje sería replicar una cola que descarta
                    // cuando está llena, y aplicar un valor viejo es justo cómo
                    // la ventana terminaba opaca para siempre. Convergiendo al
                    // estado actual, un aviso perdido no cuesta nada: el
                    // siguiente pone las cosas en su sitio.
                    Msg::Sync => loop {
                        let armed = ARMED.load(Ordering::Acquire);
                        if CLICK_THROUGH.load(Ordering::Acquire) != armed {
                            break;
                        }
                        // Volver a reclamar el frente antes de armar.
                        //
                        // Entre las ventanas always-on-top el orden lo decide la
                        // activación, y esta nunca se activa (`focusable: false`,
                        // o sea `WS_EX_NOACTIVATE`): nunca sube sola. Alcanzaba
                        // con abrir la ventana principal una vez para que el
                        // overlay quedara debajo y los clics sobre la pill se los
                        // llevara la de atrás.
                        if armed {
                            raise(&app);
                        }
                        let Some(window) = app.get_webview_window(LABEL) else {
                            break;
                        };
                        // `armed` = el cursor está sobre una zona viva, así que
                        // el click-through se APAGA.
                        set_click_through(&window, !armed);
                        CLICK_THROUGH.store(!armed, Ordering::Release);
                        // Y otra vuelta: pudo cambiar mientras se aplicaba.
                    },
                    Msg::Outside => {
                        let _ = app.emit("overlay-dismiss", ());
                    }
                }
            }
        })
        .ok();
}

/// Publica las zonas que deben recibir el mouse.
///
/// Las manda el frontend en píxeles CSS relativos al overlay, y se guardan ya
/// convertidas a físicos del escritorio: el camino caliente tiene que ser una
/// comparación y nada más.
///
/// Mientras se arrastra una superficie, el frontend publica un rectángulo que
/// cubre todo: así el puntero puede salirse de la forma sin que el overlay se
/// desarme a mitad del arrastre. No hace falta una API aparte para eso.
#[tauri::command]
pub fn set_overlay_hit_rects(app: AppHandle, rects: Vec<HitRect>) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let origin = window
        .outer_position()
        .map(|p| (f64::from(p.x), f64::from(p.y)))
        .unwrap_or((0.0, 0.0));

    let mapped: Vec<PhysRect> = rects
        .iter()
        .map(|r| PhysRect {
            x: origin.0 + r.x * scale,
            y: origin.1 + r.y * scale,
            w: r.w * scale,
            h: r.h * scale,
        })
        .collect();

    // Guardar aparte el de la pill: es el origen del que cuelga la burbuja.
    // Durante un arrastre se publica un rectángulo de pantalla completa sin id,
    // así que solo se pisa cuando viene el de verdad.
    //
    // Y es la SILUETA, no la zona viva: la zona incluye el respiro que la pill
    // deja alrededor para la rueda, así que anclando ahí el globo quedaría a
    // `gap` + ese respiro de distancia y el cuello no llegaría a cruzarlo.
    if let Some(i) = rects
        .iter()
        .position(|r| r.id == "pill-skin")
        .or_else(|| rects.iter().position(|r| r.id == "pill"))
    {
        let p = &mapped[i];
        if let Ok(mut guard) = PILL_RECT.lock() {
            *guard = Some(crate::floating::Rect {
                x: p.x.round() as i32,
                y: p.y.round() as i32,
                w: p.w.round() as i32,
                h: p.h.round() as i32,
            });
        }
    }

    if let Ok(mut guard) = HIT_RECTS.lock() {
        *guard = mapped;
    }

    // Y replantearse el armado, porque las zonas acaban de cambiar.
    //
    // Sin esto, una zona que aparece o crece BAJO UN CURSOR QUIETO no arma
    // nada: `ARMED` solo se recalculaba con el movimiento del mouse. El caso
    // que lo destapó es la barra de la pill al empezar a grabar — crece para
    // meter el cuadradito de detener justo donde estaba el puntero, y el botón
    // quedaba visible pero muerto hasta mover el mouse. Le pasa igual a la
    // burbuja de agentes cuando se abre debajo del cursor.
    #[cfg(windows)]
    reevaluate_arm();
}

/// Llamado desde el hilo de Raw Input en cada paquete de movimiento.
///
/// Contrato de esa wndproc: solo atomics y `try_send`. Nada acá bloquea — el
/// `try_lock` cae al estado anterior si hubiera contención, que con escrituras
/// de unas pocas por segundo es prácticamente imposible.
#[cfg(windows)]
pub fn on_cursor_sample() {
    let now = now_ms();
    let last = LAST_SAMPLE.load(Ordering::Relaxed);
    if now - last < SAMPLE_MS {
        return;
    }
    LAST_SAMPLE.store(now, Ordering::Relaxed);
    reevaluate_arm();
}

/// Decide si el overlay tiene que estar armado, ya, contra el cursor de ahora.
///
/// Separado del muestreo porque hay dos motivos para replantearse la respuesta
/// y solo uno es el movimiento del mouse: el otro es que cambien las zonas.
/// Este no lleva freno a propósito — el del muestreo existe para no gastar mil
/// `GetCursorPos` por segundo con un mouse rápido, y aplicárselo también a los
/// cambios de geometría dejaría el arreglo funcionando de forma intermitente,
/// que es peor que no tenerlo.
#[cfg(windows)]
fn reevaluate_arm() {
    let Some((cx, cy)) = crate::floating::cursor_position() else {
        return;
    };
    let (x, y) = (f64::from(cx), f64::from(cy));

    let Ok(rects) = HIT_RECTS.try_lock() else {
        return;
    };
    let over =
        !CAPTURING.load(Ordering::Acquire) && rects.iter().any(|r| r.contains(x, y, ARM_MARGIN));
    drop(rects);

    // Solo en los flancos: aplicar el estilo en cada muestra sería cientos de
    // `SetWindowPos` por segundo para no cambiar nada.
    if ARMED.swap(over, Ordering::AcqRel) == over {
        // Sin flanco. Pero si lo aplicado no es lo opuesto a la intención, la
        // ventana quedó desincronizada y hay que repararla.
        //
        // Esto es la salida de emergencia. El aviso viaja por un canal que
        // DESCARTA cuando está lleno, y el worker es lento —cada cambio termina
        // en un `SetWindowPos(SWP_FRAMECHANGED)`—, así que basta con que la
        // geometría revolotee (abrir la rueda sobre la consola, por ejemplo)
        // para que se pierda un «salí». Perdido ese, `ARMED` decía «afuera» y
        // la ventana seguía opaca: el escritorio entero dejaba de recibir
        // clics, y sin flanco nuevo no había forma de volver.
        //
        // Además explicaba el otro síntoma: con la ventana opaca pero `ARMED`
        // en falso, cada clic llegaba al webview Y se contaba como «clic
        // afuera», así que abrir la rueda la cerraba en el mismo gesto.
        if CLICK_THROUGH.load(Ordering::Acquire) == over {
            send(Msg::Sync);
        }
        return;
    }
    send(Msg::Sync);
}

#[cfg(not(windows))]
pub fn on_cursor_sample() {}

/// Un botón principal bajó. Si fue fuera de las zonas vivas, es «cerrá».
///
/// Se mira `ARMED` y no se vuelve a testear la posición: ese atomic ya lo
/// mantiene al día el muestreo de movimiento, y leerlo es más barato y más
/// consistente que rehacer la cuenta con un cursor que pudo moverse entre el
/// paquete y su procesamiento.
#[cfg(windows)]
pub fn on_button_down() {
    if !ARMED.load(Ordering::Acquire) {
        send(Msg::Outside);
    }
}

#[cfg(not(windows))]
pub fn on_button_down() {}

/// `try_send`: si el worker está ocupado se descarta. Lo que importa es el
/// estado final, no la secuencia.
fn send(msg: Msg) {
    if let Some(tx) = TOGGLE_TX.get() {
        match tx.try_send(msg) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

#[cfg(windows)]
fn now_ms() -> i64 {
    // SAFETY: GetTickCount64 no tiene precondiciones.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() as i64 }
}

/// Punto en píxeles CSS relativos al overlay.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct OverlayPoint {
    pub x: f64,
    pub y: f64,
}

/// Rectángulo en píxeles CSS relativos al overlay.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct OverlayArea {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Escala del overlay. Es el puente entre los físicos de Win32 y los CSS.
pub fn scale(app: &AppHandle) -> f64 {
    app.get_webview_window(LABEL)
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0)
}

/// Traduce un rectángulo físico del escritorio a coordenadas del overlay.
///
/// Lo usa la burbuja de agentes: su sitio se calcula contra los monitores, que
/// Win32 informa en físicos, y se dibuja con CSS, que trabaja en lógicos.
pub fn to_local(app: &AppHandle, r: crate::floating::Rect) -> Option<OverlayArea> {
    #[cfg(windows)]
    {
        let (ox, oy, scale) = frame(app)?;
        Some(OverlayArea {
            x: (f64::from(r.x) - ox) / scale,
            y: (f64::from(r.y) - oy) / scale,
            w: f64::from(r.w) / scale,
            h: f64::from(r.h) / scale,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (app, r);
        None
    }
}

/// Deja que el overlay reciba el teclado, o se lo vuelve a quitar.
///
/// El overlay nace `focusable: false` —o sea `WS_EX_NOACTIVATE`— para no robarle
/// el foco a la app en la que estés escribiendo. El precio es que **tampoco
/// puede recibir teclas**, y adentro hay campos de texto: el compositor de
/// agentes, la búsqueda del historial, el bloc de notas.
///
/// Así que el foco se pide justo al entrar a un campo y se devuelve al salir.
/// No se usa `set_focus()`: cuando `SetForegroundWindow` le falla, `tao` inyecta
/// `VK_LMENU` con `SendInput`, y esta app también usa `SendInput` para el
/// Ctrl+V del pegado. Con `force_foreground` no se inyecta ninguna tecla.
///
/// El bit de click-through no hace falta reponerlo: `set_focusable` reescribe
/// el ex-style entero desde las banderas de `tao`, y el de cursor es una de
/// ellas.
#[tauri::command]
pub fn set_overlay_text_mode(app: AppHandle, on: bool) {
    #[cfg(windows)]
    {
        let Some(window) = app.get_webview_window(LABEL) else {
            return;
        };
        if let Err(err) = window.set_focusable(on) {
            tracing::warn!(target: "overlay", ?err, "no se pudo cambiar el modo texto");
            return;
        }
        if on {
            // El agarre del primer plano va en un HILO APARTE.
            //
            // `force_foreground` funde la cola de entrada de quien lo llama con
            // la de la app que esté al frente (`AttachThreadInput`), y mientras
            // dure eso las dos comparten estado de entrada: si la otra app está
            // ocupada, el que se cuelga es el hilo que llamó. En el pegado esto
            // siempre corrió en un hilo suelto; traerlo al hilo de la UI —por
            // ser un comando— dejaba la interfaz entera a merced de un tercero.
            // Visto: escribir en el compositor con el navegador al frente
            // congelaba el overlay.
            if let Ok(hwnd) = window.hwnd() {
                let raw = hwnd.0 as isize;
                std::thread::spawn(move || {
                    crate::clipboard_history::force_foreground(raw as _);
                });
            }
        } else {
            // Volver a ser inactivable pierde el sitio en la pila de topmost.
            raise(&app);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, on);
    }
}

/// Origen y escala del overlay, para traducir físicos a CSS.
#[cfg(windows)]
fn frame(app: &AppHandle) -> Option<(f64, f64, f64)> {
    let window = app.get_webview_window(LABEL)?;
    let pos = window.outer_position().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    Some((f64::from(pos.x), f64::from(pos.y), scale))
}

/// Dónde está el cursor, en coordenadas del overlay.
///
/// Hace falta porque la rueda sale EN EL CURSOR, y el webview no puede
/// preguntarlo: mientras el overlay está en click-through no recibe ningún
/// evento de puntero, así que la última posición que conoce el DOM es vieja o
/// no existe.
#[tauri::command]
pub fn overlay_cursor(app: AppHandle) -> Option<OverlayPoint> {
    #[cfg(windows)]
    {
        let (ox, oy, scale) = frame(&app)?;
        let (cx, cy) = crate::floating::cursor_position()?;
        Some(OverlayPoint {
            x: (f64::from(cx) - ox) / scale,
            y: (f64::from(cy) - oy) / scale,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Áreas útiles de cada monitor, en coordenadas del overlay.
///
/// El overlay abarca el escritorio virtual entero, así que su propio rectángulo
/// no sirve para decidir si algo entra: una superficie pegada al borde derecho
/// del monitor izquierdo tiene "espacio a la derecha" que en realidad es otra
/// pantalla. Quien decide hacia dónde abrir un panel necesita los monitores, no
/// el overlay.
///
/// Es `work_area` y no `bounds` porque un panel no debe quedar debajo de la
/// barra de tareas.
#[tauri::command]
pub fn overlay_work_areas(app: AppHandle) -> Vec<OverlayArea> {
    #[cfg(windows)]
    {
        let Some((ox, oy, scale)) = frame(&app) else {
            return Vec::new();
        };
        atic_capture::monitors::enumerate()
            .iter()
            .map(|m| OverlayArea {
                x: (f64::from(m.work_area.x) - ox) / scale,
                y: (f64::from(m.work_area.y) - oy) / scale,
                w: f64::from(m.work_area.width) / scale,
                h: f64::from(m.work_area.height) / scale,
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Vec::new()
    }
}

/// Guarda dónde quedó la pill tras arrastrarla.
///
/// Antes esto lo hacía `WindowEvent::Moved`: al mover la ventana, Tauri avisaba
/// y se persistía la posición. Dentro del overlay la pill no es una ventana, así
/// que nadie avisa — la escritura tiene que ser explícita, al soltar.
///
/// Se guarda en físicos del escritorio, que es la unidad en la que la config ya
/// venía guardando `pill_position` y la que sobrevive a cambios de escala.
#[tauri::command]
pub fn save_pill_home(app: AppHandle, x: f64, y: f64) {
    #[cfg(windows)]
    {
        use atic_core::sync::MutexExt;

        let Some((ox, oy, scale)) = frame(&app) else {
            return;
        };
        let Some(state) = app.try_state::<crate::state::AppState>() else {
            return;
        };
        // Durante el clipboard en el cursor la pill está de paseo: guardar ahí
        // pisaría el hogar de verdad con una posición prestada.
        if state.pre_clipboard_position.lock_or_recover().is_some() {
            return;
        }
        state.config.lock_or_recover().pill_position = Some((ox + x * scale, oy + y * scale));
    }
    #[cfg(not(windows))]
    {
        let _ = (app, x, y);
    }
}

/// Dónde tiene que arrancar la pill, en coordenadas del overlay.
///
/// La config guarda `pill_position` en físicos del escritorio —es lo que
/// sobrevive a cambios de escala y de resolución—, así que la conversión vive
/// acá y no repartida en el frontend. Simétrico con `save_pill_home`.
#[tauri::command]
pub fn pill_home(app: AppHandle) -> Option<OverlayPoint> {
    #[cfg(windows)]
    {
        use atic_core::sync::MutexExt;

        let (ox, oy, scale) = frame(&app)?;
        let state = app.try_state::<crate::state::AppState>()?;
        let (px, py) = state.config.lock_or_recover().pill_position?;
        Some(OverlayPoint {
            x: (px - ox) / scale,
            y: (py - oy) / scale,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Rectángulo actual, para que el frontend sepa en qué coordenadas trabaja.
#[tauri::command]
pub fn overlay_rect(app: AppHandle) -> Option<OverlayRect> {
    #[cfg(windows)]
    {
        let window = app.get_webview_window(LABEL)?;
        let pos = window.outer_position().ok()?;
        let size = window.outer_size().ok()?;
        let scale = window.scale_factor().unwrap_or(1.0);
        Some(OverlayRect {
            x: pos.x,
            y: pos.y,
            w: size.width as i32,
            h: size.height as i32,
            scale,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}
