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

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter, Manager};

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

/// Zonas que SÍ deben recibir el mouse, en **CSS del overlay** (no físicos).
///
/// Se comparan contra el cursor pasado por `ScreenToClient` + escala: así el
/// hit-test usa el mismo espacio que el webview, también en el 2º monitor /
/// DPI mixto. Mapear CSS→físicos con `outer_position` fallaba cuando el origen
/// cliente no coincidía con el outer.
static HIT_RECTS: Mutex<Vec<HitCss>> = Mutex::new(Vec::new());

/// HWND del overlay (para `ScreenToClient` en el camino caliente).
static OVERLAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// HWND del overlay, o `None` si todavía no nació.
///
/// Lo usa la captura: `BitBlt` no ve ventanas layered/transparentes.
pub fn hwnd() -> Option<isize> {
    let h = OVERLAY_HWND.load(Ordering::Acquire);
    (h != 0).then_some(h)
}

/// Rasteriza pill/launcher/goo vía WebView2 y lo devuelve como frame físico.
#[cfg(windows)]
pub fn capture_layer(app: &AppHandle) -> Option<atic_capture::Frame> {
    let window = app.get_webview_window(LABEL)?;
    let png = match crate::webview_tweaks::capture_preview_png(&window) {
        Ok(png) => png,
        Err(err) => {
            tracing::warn!(target: "overlay", %err, "CapturePreview del overlay falló");
            return None;
        }
    };
    let (x, y) = window
        .hwnd()
        .ok()
        .and_then(|hwnd| atic_capture::windows::window_bounds(hwnd.0 as isize))
        .map(|r| (r.x, r.y))
        .unwrap_or((0, 0));
    match atic_capture::Frame::from_png(x, y, &png) {
        Ok(frame) => Some(frame),
        Err(err) => {
            tracing::warn!(target: "overlay", %err, "no se pudo decodificar el PNG del overlay");
            None
        }
    }
}

/// HWND de `main`. El overlay cubre el escritorio virtual y, si se arma encima
/// de esta ventana, se ve Atic pero el mouse se lo queda la lámina.
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Ya pedimos al frontend que suelte un drag pegado sobre `main`.
/// Evita un `YieldMain` por cada muestra del cursor.
#[cfg(windows)]
static YIELDED_MAIN: AtomicBool = AtomicBool::new(false);

/// Última escala conocida del overlay (`f64` en bits).
static OVERLAY_SCALE_BITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(f64::to_bits(1.0));

/// Viewport CSS real del webview (`window.innerWidth/Height`).
///
/// Con DPI mixto o un webview que no llenó la ventana, `client/scale_factor`
/// no coincide con el CSS: el fly-to se queda corto y los hit-rects no
/// contienen el cursor. 0 = todavía no avisó el frontend.
static CSS_VIEW_W_BITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CSS_VIEW_H_BITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Tamaño físico del overlay según `place()` (escritorio virtual).
///
/// `GetClientRect` a veces ya viene en DIP (igual que `innerWidth`). Si el
/// hit-test usa ese rect como si fueran físicos, al llegar el viewport CSS
/// el mapeo cambia y la pill se ve pero el mouse la atraviesa.
static OVERLAY_PHYS_W: AtomicU32 = AtomicU32::new(0);
static OVERLAY_PHYS_H: AtomicU32 = AtomicU32::new(0);

/// Topología aplicada al overlay. Si al login Windows aún no enumeró el
/// segundo monitor, esto queda en 1 pantalla y hay que reaplicar después.
#[derive(Clone, Copy, PartialEq, Eq)]
struct DisplayTopo {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    n: u32,
}

static APPLIED_TOPO: Mutex<Option<DisplayTopo>> = Mutex::new(None);

/// Avisos de `WM_DISPLAYCHANGE` (capacidad 1: varios seguidos son el mismo).
static DISPLAY_TX: OnceLock<SyncSender<()>> = OnceLock::new();

/// WndProc original de tao, para encadenar `WM_GETMINMAXINFO` / display.
#[cfg(windows)]
static OVERLAY_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// ¿Debería el overlay estar recibiendo el mouse ahora mismo?
static ARMED: AtomicBool = AtomicBool::new(false);

/// Arrastre OLE de un ítem (clipboard → otra app): forzar click-through total
/// para que la ventana always-on-top no se coma el drop en Cursor / Explorador.
static ITEM_DRAG_PASSTHROUGH: AtomicBool = AtomicBool::new(false);

/// Gesto de puntero en curso (pill / float). El overlay se queda armado aunque
/// el cursor salga del hit-rect o cruce `main`: si se desarma a mitad, Windows
/// no entrega el `pointerup` y el arrastre queda pegado para siempre.
static POINTER_GESTURE: AtomicBool = AtomicBool::new(false);

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
        // Ya estábamos en captura: igual reafirmar. Un segundo Pressed o un
        // reassert programado no debe ser no-op si el ex-style se pisó.
        if on {
            yield_to_capture(app);
        }
        return;
    }
    if on {
        // No emitir `overlay-dismiss`: ese evento es “clic afuera” y cierra
        // clipboard / textos / launcher (y manda la pill a casa). El atajo
        // de captura no es un clic afuera. La rueda ya ignora el dismiss
        // por `openDismissGrace`; sin gracia, el atajo dejaba las tools
        // cerradas. El mouse llega igual: `CAPTURING` pone click-through.
        // La rueda se cierra en `overlay-session-started`, no acá.
        // Aplicar YA: no basta con `send(Sync)`. El canal del worker descarta
        // cuando está lleno, y sin un movimiento de mouse el camino de reparación
        // de `reevaluate_arm` no corre. Resultado: con el cursor sobre pill/float
        // (overlay armado) la captura subía pero el mouse seguía en Atic hasta
        // un clic o un segundo atajo. Fuera de Atic el overlay ya era
        // click-through y el fallo no se notaba.
        yield_to_capture(app);
        // `set_ignore_cursor_events` / `set_focusable` van al hilo del event
        // loop. Si el atajo congela el escritorio en ese mismo hilo, el estilo
        // puede quedar en cola; y `set_focusable`/`show` posteriores lo pisan.
        // Hold “arreglaba” esto por tiempo. Los timers lo hacen sin depender
        // de key-repeat (`MOD_NOREPEAT` en global-hotkey).
        schedule_capturing_reassert(app);
    } else {
        set_topmost(app, crate::agents::bridge::overlay_should_be_topmost());
        send(Msg::Sync);
        // Sin esto, al cancelar con el cursor quieto sobre la pill `ARMED`
        // queda en falso hasta el próximo movimiento.
        #[cfg(windows)]
        reevaluate_arm();
    }
}

/// Reafirma click-through del overlay de la pill mientras hay captura.
///
/// `show`/`set_always_on_top` del overlay de captura —y `set_focusable` al
/// salir del modo texto— reescriben el ex-style y pueden dejar la pill opaca
/// otra vez; sin esto, el mouse no llega a la selección hasta un clic
/// “afuera”. Fuerza el estilo aunque el atomic ya diga click-through.
pub fn reassert_capturing_input(app: &AppHandle) {
    if !CAPTURING.load(Ordering::Acquire) {
        return;
    }
    yield_to_capture(app);
}

/// Como [`reassert_capturing_input`], más reintentos a 0/16/50 ms.
///
/// Usar tras `show` del overlay de captura: el freeze suele durar más que los
/// timers del arranque, y `set_always_on_top`/`set_focusable` pueden pisar el
/// estilo justo al revelar la selección.
pub fn reassert_capturing_input_with_retry(app: &AppHandle) {
    reassert_capturing_input(app);
    if CAPTURING.load(Ordering::Acquire) {
        schedule_capturing_reassert(app);
    }
}

/// Reafirma click-through a 0 / 16 / 50 ms sin esperar key-repeat ni hold.
///
/// Cada disparo comprueba `CAPTURING`: si el usuario canceló, no vuelve a
/// apartar el overlay (así no rompe el modo texto de consola/composer).
fn schedule_capturing_reassert(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("atic-capture-yield".into())
        .spawn(move || {
            let start = std::time::Instant::now();
            for delay_ms in [0u64, 16, 50, 120, 250, 500] {
                let target = std::time::Duration::from_millis(delay_ms);
                if let Some(wait) = target.checked_sub(start.elapsed()) {
                    std::thread::sleep(wait);
                }
                if !CAPTURING.load(Ordering::Acquire) {
                    return;
                }
                reassert_capturing_input(&app);
            }
        });
}

/// Aparta el overlay de la pill del hit-testing para que gane el de captura.
///
/// Mismo proceso, dos always-on-top: Windows puede seguir entregando el hover a
/// la ventana que lo tenía aunque la de captura esté encima, hasta que la de
/// abajo sea click-through o haya un clic.
fn yield_to_capture(app: &AppHandle) {
    POINTER_GESTURE.store(false, Ordering::Release);
    // Su gemelo: también lo pone el front y también lo baja el front. Si el
    // webview se recarga (o muere) a mitad de un arrastre de ítem, nadie lo
    // bajaba nunca y el hit-testing quedaba en passthrough para siempre.
    ITEM_DRAG_PASSTHROUGH.store(false, Ordering::Release);
    ARMED.store(false, Ordering::Release);
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // Salir del modo texto y de la banda topmost ANTES del click-through:
    // `set_focusable` / `set_always_on_top` reescriben `GWL_EXSTYLE` entero y
    // borrarían `ignore_cursor_events` si fueran después. Sin bajar el
    // topmost, Windows sigue entregando el hover a esta ventana aunque la de
    // captura esté encima — hasta un clic “afuera”.
    #[cfg(windows)]
    {
        let _ = window.set_focusable(false);
        let _ = window.set_always_on_top(false);
    }
    set_click_through(&window, true);
    CLICK_THROUGH.store(true, Ordering::Release);
}

/// Click-through efectivo: durante captura el overlay nunca arma, aunque
/// `ARMED` o un gesto de puntero digan lo contrario.
fn desired_click_through(capturing: bool, armed: bool) -> bool {
    capturing || !armed
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
    /// El cursor está sobre `main` con un hit-rect de drag a pantalla completa.
    /// El frontend tiene que soltar el gesto; si no, el overlay sigue armado
    /// en todo el escritorio.
    YieldMain,
}

#[derive(Debug, Clone)]
struct HitCss {
    id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl HitCss {
    fn contains(&self, x: f64, y: f64, margin: f64) -> bool {
        x >= self.x - margin
            && y >= self.y - margin
            && x <= self.x + self.w + margin
            && y <= self.y + self.h + margin
    }
}

/// Floats del mismo overlay que pueden recibir un drop OLE (p. ej. composer).
fn is_ole_drop_target(id: &str) -> bool {
    id == "agents"
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
    // Tamaño real desde el create: WebView2 se queda con el `inner_size`
    // inicial aunque después se haga `set_size` al escritorio virtual. Con
    // 480×320 el CSS vive en un recuadro, el fly-to no llega al mouse y los
    // hit-rects no coinciden con el cursor.
    let vs = atic_capture::monitors::virtual_screen();
    let scale = atic_capture::monitors::enumerate()
        .iter()
        .map(|m| m.scale)
        .fold(1.0_f64, f64::max)
        .max(0.01);
    let lw = (f64::from(vs.width) / scale).max(1.0);
    let lh = (f64::from(vs.height) / scale).max(1.0);
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("overlay".into()))
            .title("Atic")
            .inner_size(lw, lh)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .focusable(false)
            .visible(false);
    // Perfil propio: main/launcher/captura y la app instalada comparten
    // `com.ciat.atic`. Cinco controladores WebView2 sobre el mismo user-data
    // es el HRESULT 0x8007139F (estado inválido) y un HWND sin Chromium.
    //
    // Consecuencia: localStorage no se comparte con main. El tema de esta
    // ventana sale de `get_config` / evento `ui-theme`, no del cache
    // `atic-theme`.
    if let Ok(dir) = app.path().app_local_data_dir() {
        builder = builder.data_directory(dir.join("overlay-webview"));
    }
    // Click-through + NOACTIVATE: Chromium cree la ventana ocluida y duerme
    // timers/rAF/IPC hasta el próximo clic. Los atajos sobre la rueda/floats
    // quedaban “pegados” hasta tocar otra cosa.
    #[cfg(windows)]
    {
        builder = builder.additional_browser_args(
            "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion",
        );
    }
    builder
        .build()
        .map_err(|err| tracing::error!(target: "overlay", ?err, "no se pudo crear la ventana"))
        .ok()
}

pub fn place(app: &AppHandle) -> Option<OverlayRect> {
    #[cfg(windows)]
    {
        let window = create(app)?;
        install_overlay_hooks(&window);

        let monitors = atic_capture::monitors::enumerate();
        let mixed = monitors.first().is_some_and(|first| {
            monitors
                .iter()
                .any(|m| (m.scale - first.scale).abs() > f64::EPSILON)
        });
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

        let _ = window.set_always_on_top(true);
        let _ = window.show();
        // WebView2 a veces se queda con el `inner_size` del create (480×320)
        // y el overlay cubre la pantalla pero el CSS sigue en un recuadro:
        // la pill vuela "a medio camino" y los floats nacen corridos.
        let mut rect = apply_overlay_geometry(&window);
        // El webview termina de nacer después de `show`: repetir o se queda
        // con el recuadro del create (fly-to corto, pill sin clics).
        //
        // Tras un login, el segundo monitor a veces aparece segundos después:
        // hay que volver a medir el escritorio virtual, no solo corregir el
        // desfase del cliente.
        {
            let boot = app.clone();
            std::thread::spawn(move || {
                let marks = [16u64, 50, 200, 500, 1000, 2000, 5000, 15000, 30000];
                let start = std::time::Instant::now();
                for mark in marks {
                    let target = std::time::Duration::from_millis(mark);
                    if let Some(left) = target.checked_sub(start.elapsed()) {
                        std::thread::sleep(left);
                    }
                    let again = boot.clone();
                    let _ = boot.run_on_main_thread(move || {
                        sync_overlay_to_displays(&again);
                    });
                }
            });
        }
        if let Ok(hwnd) = window.hwnd() {
            OVERLAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        }
        let scale = window.scale_factor().unwrap_or(1.0).max(0.01);
        rect.scale = scale;
        OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
        // Al final y no antes: `show()` y `set_always_on_top()` son cambios de
        // bandera, y cada uno reescribe el ex-style entero. El orden solo es
        // seguro porque este bit también es de `tao` — si algún día se escribe
        // a mano, hay que re-aplicarlo después de cada uno de ellos.
        set_click_through(&window, true);
        keep_non_occluding(&window);

        tracing::info!(
            target: "overlay",
            monitores = monitors.len(),
            escalas_mixtas = mixed,
            scale,
            "overlay en {},{} {}x{}", rect.x, rect.y, rect.w, rect.h
        );
        let _ = app.emit("overlay-ready", ());
        #[cfg(windows)]
        reevaluate_arm();
        Some(rect)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

#[cfg(windows)]
fn display_topo() -> DisplayTopo {
    let vs = atic_capture::monitors::virtual_screen();
    DisplayTopo {
        x: vs.x,
        y: vs.y,
        w: vs.width,
        h: vs.height,
        n: atic_capture::monitors::enumerate().len() as u32,
    }
}

/// Encaja el overlay al escritorio virtual actual y anota la topología.
#[cfg(windows)]
fn apply_overlay_geometry(window: &tauri::WebviewWindow) -> OverlayRect {
    let vs = atic_capture::monitors::virtual_screen();
    let rect = OverlayRect {
        x: vs.x,
        y: vs.y,
        w: vs.width as i32,
        h: vs.height as i32,
        scale: 1.0,
    };
    let _ = window.set_max_size(Some(tauri::Size::Physical(tauri::PhysicalSize::new(
        16384, 16384,
    ))));
    let _ = window.set_position(tauri::PhysicalPosition::new(rect.x, rect.y));
    let _ = window.set_size(tauri::PhysicalSize::new(rect.w as u32, rect.h as u32));
    cover_virtual_screen(window);
    crate::webview_tweaks::sync_controller_bounds(window);
    OVERLAY_PHYS_W.store(rect.w.max(0) as u32, Ordering::SeqCst);
    OVERLAY_PHYS_H.store(rect.h.max(0) as u32, Ordering::SeqCst);
    if let Ok(mut g) = APPLIED_TOPO.lock() {
        *g = Some(display_topo());
    }
    rect
}

/// Si Windows acaba de sumar/quitar un monitor, reajusta el overlay.
#[cfg(windows)]
fn sync_overlay_to_displays(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    install_overlay_hooks(&window);
    let topo = display_topo();
    let same = APPLIED_TOPO
        .lock()
        .ok()
        .and_then(|g| *g)
        .is_some_and(|prev| prev == topo);
    if same {
        cover_virtual_screen(&window);
        crate::webview_tweaks::sync_controller_bounds(&window);
        return;
    }
    tracing::info!(
        target: "overlay",
        monitores = topo.n,
        "escritorio virtual cambió; overlay a {},{} {}x{}",
        topo.x,
        topo.y,
        topo.w,
        topo.h
    );
    let _ = apply_overlay_geometry(&window);
    let scale = window.scale_factor().unwrap_or(1.0).max(0.01);
    OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
    keep_non_occluding(&window);
    let _ = app.emit("overlay-ready", ());
    reevaluate_arm();
}

#[cfg(windows)]
fn orig_overlay_wndproc() -> Option<unsafe extern "system" fn(
    windows_sys::Win32::Foundation::HWND,
    u32,
    windows_sys::Win32::Foundation::WPARAM,
    windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT> {
    type WndProcFn = unsafe extern "system" fn(
        windows_sys::Win32::Foundation::HWND,
        u32,
        windows_sys::Win32::Foundation::WPARAM,
        windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT;
    let orig = OVERLAY_WNDPROC.load(Ordering::SeqCst);
    if orig == 0 {
        None
    } else {
        // SAFETY: el valor lo guardó `SetWindowLongPtrW` como WndProc de tao.
        Some(unsafe { std::mem::transmute::<isize, WndProcFn>(orig) })
    }
}

/// Windows recorta `SetWindowPos` al monitor actual vía `WM_GETMINMAXINFO`.
/// Sin esto, el overlay del login (cuando aún hay una sola pantalla) no puede
/// crecer al aparecer la segunda. `WM_DISPLAYCHANGE` avisa en caliente.
#[cfg(windows)]
unsafe extern "system" fn overlay_wndproc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, MINMAXINFO, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_GETMINMAXINFO,
    };

    if msg == WM_GETMINMAXINFO && lparam != 0 {
        if let Some(orig_fn) = orig_overlay_wndproc() {
            let _ = CallWindowProcW(Some(orig_fn), hwnd, msg, wparam, lparam);
        }
        let mmi = lparam as *mut MINMAXINFO;
        let vs = atic_capture::monitors::virtual_screen();
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

    if msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED {
        if let Some(tx) = DISPLAY_TX.get() {
            let _ = tx.try_send(());
        }
    }

    match orig_overlay_wndproc() {
        Some(orig_fn) => CallWindowProcW(Some(orig_fn), hwnd, msg, wparam, lparam),
        None => 0,
    }
}

#[cfg(windows)]
fn install_overlay_hooks(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWLP_WNDPROC,
    };

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    let ours = overlay_wndproc as *const () as isize;
    // SAFETY: HWND de Tauri vivo; Get/SetWindowLongPtr solo leen/escriben el
    // puntero a WndProc. Encadenamos la de tao.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if current == ours {
            return;
        }
        let saved = OVERLAY_WNDPROC.load(Ordering::SeqCst);
        if saved != 0 && saved != current && current != ours {
            tracing::warn!(
                target: "overlay",
                "el overlay tiene un WndProc distinto al guardado; no se reengancha"
            );
            return;
        }
        if saved == 0 {
            OVERLAY_WNDPROC.store(current, Ordering::SeqCst);
        }
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours);
    }
}

fn start_display_watch(app: AppHandle) {
    let (tx, rx) = sync_channel::<()>(1);
    if DISPLAY_TX.set(tx).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("atic-display-sync".into())
        .spawn(move || {
            while let Ok(()) = rx.recv() {
                std::thread::sleep(std::time::Duration::from_millis(400));
                while rx.try_recv().is_ok() {}
                let again = app.clone();
                let _ = app.run_on_main_thread(move || {
                    #[cfg(windows)]
                    sync_overlay_to_displays(&again);
                    #[cfg(not(windows))]
                    let _ = again;
                });
            }
        })
        .ok();
}

/// Desplaza el marco para que el (0,0) del cliente coincida con el escritorio
/// virtual. El tamaño interior ya lo puso `set_size`; acá solo se corrige el
/// desfase de no-cliente (caption residual, borde DWM).
#[cfg(windows)]
fn cover_virtual_screen(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as _;
    let vs = atic_capture::monitors::virtual_screen();

    let mut origin = POINT { x: 0, y: 0 };
    let mut outer = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: HWND de Tauri vivo; ClientToScreen / GetWindowRect solo leen.
    let ok =
        unsafe { ClientToScreen(hwnd, &mut origin) != 0 && GetWindowRect(hwnd, &mut outer) != 0 };
    if !ok {
        return;
    }
    let dx = vs.x - origin.x;
    let dy = vs.y - origin.y;
    if dx == 0 && dy == 0 {
        return;
    }

    tracing::info!(
        target: "overlay",
        dx,
        dy,
        origin_x = origin.x,
        origin_y = origin.y,
        vs_x = vs.x,
        vs_y = vs.y,
        "cliente del overlay desfasado del escritorio virtual; se corrige"
    );

    // SAFETY: mismo HWND; SWP_NOSIZE conserva el inner_size ya aplicado.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            outer.left + dx,
            outer.top + dy,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
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
    // `set_always_on_top` reescribe el ex-style entero y puede llevarse puesto
    // el alfa uniforme; sin reponerlo el overlay vuelve a tapar al de atrás
    // (ver `keep_non_occluding`).
    keep_non_occluding(&window);
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

/// Alfa uniforme del overlay: 254, un punto por debajo del máximo.
const OVERLAY_ALPHA: u8 = 254;

/// Que el overlay nunca cuente como ventana *tapadora* para el resto del SO.
///
/// Al armar hay que sacarle `WS_EX_TRANSPARENT`, y en ese instante el overlay
/// pasa a ser, para cualquier otra app, una ventana visible, topmost y del
/// tamaño del escritorio entero. El detector de oclusión de Chromium
/// (`NativeWindowOcclusionTrackerWin`) marca ocluida a la ventana de atrás y
/// esa deja de componer: **el video se congela apenas el mouse toca la pill, y
/// el audio sigue**, porque van por caminos separados.
///
/// Es el mismo mecanismo que ya nos pega al revés —por eso el overlay nace con
/// `--disable-features=CalculateNativeWinOcclusion`—, pero esa bandera solo
/// vale para nuestro webview; el navegador del usuario tiene el suyo activo.
///
/// El detector descarta como oclusor toda ventana `WS_EX_LAYERED` cuyo alfa
/// uniforme no sea exactamente 255. Medido en vivo, esta ventana ya es layered
/// con `LWA_ALPHA` y alfa 255: alcanza con bajarlo a 254. A ojo es indistinguible
/// (0,4 %) y no toca el alfa por píxel del webview, que es de donde sale la
/// transparencia real.
///
/// Hay que reponerlo después de CADA cambio de banderas: `set_ignore_cursor_events`
/// reescribe `GWL_EXSTYLE` entero, y si en el camino quita y repone
/// `WS_EX_LAYERED` el alfa vuelve al default.
pub fn keep_non_occluding(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE,
            LWA_ALPHA, WS_EX_LAYERED,
        };
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let hwnd = hwnd.0 as _;
        // SAFETY: el HWND lo da Tauri y vive mientras viva la ventana. Solo se
        // AÑADE `WS_EX_LAYERED` si falta —nunca se toca `WS_EX_TRANSPARENT`,
        // que es de `CLICK_THROUGH` y desincronizarlo deja el escritorio entero
        // sin recibir clics.
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if ex & (WS_EX_LAYERED as isize) == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
            }
            SetLayeredWindowAttributes(hwnd, 0, OVERLAY_ALPHA, LWA_ALPHA);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

/// Arranque: colocar, mostrar y dejar el worker escuchando.
///
/// Las otras ventanas WebView2 se crean en el mismo instante (main, shelf,
/// captura, launcher). Si el overlay entra en esa carrera, wry a veces
/// devuelve un HWND sin Chromium: la pill no pinta y nunca llega
/// `viewport CSS del overlay`. Esperamos a que esa carrera termine, y si
/// a los 2 s el frontend no avisó, destruimos y volvemos a crear.
pub fn setup(app: &AppHandle) {
    remember_main(app);
    start_toggle_worker(app.clone());
    start_display_watch(app.clone());
    let handle = app.clone();
    std::thread::Builder::new()
        .name("atic-overlay-boot".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let boot = handle.clone();
            let (placed_tx, placed_rx) = std::sync::mpsc::channel();
            let _ = handle.run_on_main_thread(move || {
                if place(&boot).is_none() {
                    tracing::warn!(target: "overlay", "no se pudo colocar el overlay");
                }
                crate::webview_tweaks::apply_to_all_windows(&boot);
                let _ = placed_tx.send(());
            });
            let _ = placed_rx.recv_timeout(std::time::Duration::from_secs(5));
            for _ in 0..80 {
                let css_w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
                if css_w > 1.0 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            tracing::warn!(
                target: "overlay",
                "el overlay no reportó viewport CSS; recreando la ventana"
            );
            let drop = handle.clone();
            let _ = handle.run_on_main_thread(move || drop_overlay(&drop));
            std::thread::sleep(std::time::Duration::from_millis(400));
            let retry = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if retry.get_webview_window(LABEL).is_some() {
                    drop_overlay(&retry);
                }
                if place(&retry).is_none() {
                    tracing::error!(target: "overlay", "no se pudo recrear el overlay");
                }
                crate::webview_tweaks::apply_to_all_windows(&retry);
            });
        })
        .ok();
}

fn drop_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
    OVERLAY_HWND.store(0, Ordering::Release);
    OVERLAY_PHYS_W.store(0, Ordering::Release);
    OVERLAY_PHYS_H.store(0, Ordering::Release);
    CSS_VIEW_W_BITS.store(0, Ordering::Release);
    CSS_VIEW_H_BITS.store(0, Ordering::Release);
    ARMED.store(false, Ordering::Release);
    CLICK_THROUGH.store(true, Ordering::Release);
    #[cfg(windows)]
    OVERLAY_WNDPROC.store(0, Ordering::Release);
    if let Ok(mut g) = APPLIED_TOPO.lock() {
        *g = None;
    }
}

/// Guarda el HWND de `main` para el hit-test del camino caliente.
pub fn remember_main(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    if let Ok(hwnd) = main.hwnd() {
        MAIN_HWND.store(hwnd.0 as isize, Ordering::Release);
    }
}

/// `main` está al frente: soltar un drag pegado (hit-rect fullscreen).
pub fn yield_to_main(app: &AppHandle) {
    remember_main(app);
    POINTER_GESTURE.store(false, Ordering::Release);
    // El drag de ítem muere con el gesto: si `main` pasó al frente, ya no hay
    // arrastre que sostener y el front podría no llegar a bajarlo.
    ITEM_DRAG_PASSTHROUGH.store(false, Ordering::Release);
    #[cfg(windows)]
    reevaluate_arm();
    let _ = app.emit("overlay-yield-main", ());
}

/// ¿Armar el overlay?
///
/// La pill y los floats tienen que recibir el mouse aunque se solapen con
/// `main`. Lo que no puede armarse encima de `main` es el hit-rect a pantalla
/// completa de un drag ya muerto: esa lámina deja Atic pintada y sin input.
/// Durante un gesto de puntero hay que seguir armado: si se vuelve
/// click-through a mitad, se pierde el `pointerup`.
fn should_arm(
    capturing: bool,
    over_main: bool,
    over_hit: bool,
    has_drag: bool,
    gesture: bool,
) -> bool {
    if capturing {
        return false;
    }
    if gesture {
        return true;
    }
    if over_main && has_drag {
        return false;
    }
    over_hit
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
                        let capturing = CAPTURING.load(Ordering::Acquire);
                        let armed = ARMED.load(Ordering::Acquire);
                        let through = desired_click_through(capturing, armed);
                        if CLICK_THROUGH.load(Ordering::Acquire) == through {
                            break;
                        }
                        // Durante captura nunca reclamar el frente: le roba el
                        // mouse al overlay de selección.
                        if armed && !capturing {
                            raise(&app);
                        }
                        let Some(window) = app.get_webview_window(LABEL) else {
                            break;
                        };
                        if capturing {
                            ARMED.store(false, Ordering::Release);
                            POINTER_GESTURE.store(false, Ordering::Release);
                        }
                        set_click_through(&window, through);
                        // Inmediatamente después del cambio de banderas: es el
                        // único punto que aplica el click-through, así que es
                        // el único donde el alfa se puede haber perdido.
                        keep_non_occluding(&window);
                        CLICK_THROUGH.store(through, Ordering::Release);
                    },
                    Msg::Outside => {
                        let _ = app.emit("overlay-dismiss", ());
                    }
                    Msg::YieldMain => {
                        let _ = app.emit("overlay-yield-main", ());
                    }
                }
            }
        })
        .ok();
}

/// Publica las zonas que deben recibir el mouse.
///
/// Las manda el frontend en píxeles CSS relativos al overlay y se guardan así:
/// el armado compara contra el cursor en el mismo espacio (`ScreenToClient`).
///
/// Mientras se arrastra una superficie, el frontend publica un rectángulo que
/// cubre todo: así el puntero puede salirse de la forma sin que el overlay se
/// desarme a mitad del arrastre. No hace falta una API aparte para eso.
#[tauri::command]
pub fn set_overlay_hit_rects(app: AppHandle, rects: Vec<HitRect>) {
    // Guardar las zonas aunque el HWND todavía no exista: el overlay arranca
    // con retraso y, si se descartan, el frontend no vuelve a mandarlas
    // (`#sent` ya las dio por publicadas) y la pill queda inalcanzable.
    if let Some(window) = app.get_webview_window(LABEL) {
        let scale = window.scale_factor().unwrap_or(1.0);
        OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::Release);
        if let Ok(hwnd) = window.hwnd() {
            OVERLAY_HWND.store(hwnd.0 as isize, Ordering::Release);
        }
        // Origen del cliente (no outer): alinea PILL_RECT con el cursor.
        let origin = client_origin_physical().unwrap_or_else(|| {
            window
                .outer_position()
                .map(|p| (f64::from(p.x), f64::from(p.y)))
                .unwrap_or((0.0, 0.0))
        });

        // Guardar aparte el de la pill en físicos: bubble_rect / panel_float.
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
            let p = &rects[i];
            if let Ok(mut guard) = PILL_RECT.lock() {
                let (px, py) = css_to_physical_client(p.x, p.y);
                let (pw, ph) = css_to_physical_client(p.w, p.h);
                *guard = Some(crate::floating::Rect {
                    x: (origin.0 + px).round() as i32,
                    y: (origin.1 + py).round() as i32,
                    w: pw.round() as i32,
                    h: ph.round() as i32,
                });
            }
        }
    }

    // Durante OLE out-drag: solo drop-targets del overlay (agentes). El resto
    // sigue en click-through para soltar en Cursor/Explorador.
    let mapped: Vec<HitCss> = rects
        .iter()
        .filter(|r| !ITEM_DRAG_PASSTHROUGH.load(Ordering::Acquire) || is_ole_drop_target(&r.id))
        .map(|r| HitCss {
            id: r.id.clone(),
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        })
        .collect();

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

/// Gesto de puntero: armar YA, sin esperar el hit-rect fullscreen por IPC.
///
/// El camino de `set_overlay_hit_rects` llega un rAF más tarde. En ese hueco
/// el cursor ya salió de la pill, el overlay se desarma y el `pointerup` se
/// lo queda otra ventana.
#[tauri::command]
pub fn set_overlay_pointer_gesture(_app: AppHandle, on: bool) {
    if on && CAPTURING.load(Ordering::Acquire) {
        return;
    }
    POINTER_GESTURE.store(on, Ordering::SeqCst);
    if on {
        ARMED.store(true, Ordering::SeqCst);
        send(Msg::Sync);
    } else {
        #[cfg(windows)]
        reevaluate_arm();
    }
}

/// Durante un arrastre OLE hacia otra app: click-through fuera de los
/// drop-targets del overlay. Sin vaciar `agents`, el drop al composer caía
/// atrás (misma webview + WS_EX_TRANSPARENT).
#[tauri::command]
pub fn set_overlay_item_drag(app: AppHandle, on: bool) {
    ITEM_DRAG_PASSTHROUGH.store(on, Ordering::SeqCst);
    if on {
        if let Ok(mut guard) = HIT_RECTS.lock() {
            guard.retain(|r| is_ole_drop_target(&r.id));
        }
        ARMED.store(false, Ordering::SeqCst);
        if let Some(window) = app.get_webview_window(LABEL) {
            set_click_through(&window, true);
            CLICK_THROUGH.store(true, Ordering::SeqCst);
        }
        #[cfg(windows)]
        reevaluate_arm();
    } else {
        #[cfg(windows)]
        reevaluate_arm();
    }
}

/// Relee el cursor y rearma agentes durante el loop modal de `DoDragDrop`.
pub fn nudge_item_drag_arm() {
    if !ITEM_DRAG_PASSTHROUGH.load(Ordering::Acquire) {
        return;
    }
    #[cfg(windows)]
    reevaluate_arm();
}

/// ¿El cursor CSS del overlay está sobre el hit-rect `id`?
#[tauri::command]
pub fn overlay_cursor_over_hit(id: String) -> bool {
    cursor_over_hit_id(&id)
}

/// Crate-interno: cursor sobre un hit-rect publicado.
pub fn cursor_over_hit_id(id: &str) -> bool {
    #[cfg(windows)]
    {
        let Some((x, y)) = cursor_overlay_css() else {
            return false;
        };
        let Ok(rects) = HIT_RECTS.lock() else {
            return false;
        };
        rects
            .iter()
            .any(|r| r.id == id && r.contains(x, y, ARM_MARGIN))
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        false
    }
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

/// Cursor en CSS del overlay.
///
/// Misma conversión que `pill_home`: `GetCursorPos` menos el origen del
/// cliente, luego físicos → CSS. `ScreenToClient` a veces no comparte unidad
/// con `GetClientRect` (físico vs DIP) y el hit-test se iba a un lado de la
/// pill pintada.
#[cfg(windows)]
fn cursor_overlay_css() -> Option<(f64, f64)> {
    let hwnd = OVERLAY_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return None;
    }
    let (cx, cy) = crate::floating::cursor_position()?;
    let (ox, oy) = client_origin_physical()?;
    Some(physical_client_to_css(
        f64::from(cx) - ox,
        f64::from(cy) - oy,
    ))
}

/// Esquina (0,0) del cliente del overlay en físicos de pantalla.
#[cfg(windows)]
fn client_origin_physical() -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;

    let hwnd = OVERLAY_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return None;
    }
    let mut pt = POINT { x: 0, y: 0 };
    // SAFETY: HWND del overlay; ClientToScreen escribe el POINT.
    let ok = unsafe { ClientToScreen(hwnd as _, &mut pt) };
    if ok == 0 {
        return None;
    }
    Some((f64::from(pt.x), f64::from(pt.y)))
}

#[cfg(not(windows))]
fn client_origin_physical() -> Option<(f64, f64)> {
    None
}

/// Mapeo lineal cliente-físico → CSS del webview.
///
/// Si el frontend ya mandó `innerWidth/Height`, se usa eso. Si no, `client/scale`.
fn map_client_to_css(
    x: f64,
    y: f64,
    client_w: f64,
    client_h: f64,
    css_w: f64,
    css_h: f64,
) -> (f64, f64) {
    (x * css_w / client_w.max(1.0), y * css_h / client_h.max(1.0))
}

fn map_css_to_client(
    x: f64,
    y: f64,
    client_w: f64,
    client_h: f64,
    css_w: f64,
    css_h: f64,
) -> (f64, f64) {
    (x * client_w / css_w.max(1.0), y * client_h / css_h.max(1.0))
}

#[cfg(windows)]
fn client_size_physical() -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

    let hwnd = OVERLAY_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return None;
    }
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: HWND del overlay vivo; GetClientRect solo escribe el RECT.
    let ok = unsafe { GetClientRect(hwnd as _, &mut rc) };
    if ok == 0 || rc.right <= 0 || rc.bottom <= 0 {
        return None;
    }
    Some((f64::from(rc.right), f64::from(rc.bottom)))
}

fn css_viewport_size(phys_w: f64, phys_h: f64) -> (f64, f64) {
    let w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
    let h = f64::from_bits(CSS_VIEW_H_BITS.load(Ordering::Acquire));
    if w > 1.0 && h > 1.0 {
        return (w, h);
    }
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire)).max(0.01);
    (phys_w / scale, phys_h / scale)
}

/// Extensión física del overlay para mapear cursor / hogar / monitores.
///
/// Prefiere el tamaño que `place()` guardó. Si `GetClientRect` ya coincide
/// con el CSS, no es físico: se reconstruye con la escala.
fn resolve_physical_extent(
    stored_w: f64,
    stored_h: f64,
    client_w: f64,
    client_h: f64,
    css_w: f64,
    css_h: f64,
    scale: f64,
) -> (f64, f64) {
    if stored_w > 1.0 && stored_h > 1.0 {
        return (stored_w, stored_h);
    }
    if css_w > 1.0 && (client_w - css_w).abs() < 4.0 && (client_h - css_h).abs() < 4.0 {
        let s = scale.max(0.01);
        return (client_w * s, client_h * s);
    }
    (client_w, client_h)
}

#[cfg(windows)]
fn physical_extent() -> (f64, f64) {
    let stored_w = f64::from(OVERLAY_PHYS_W.load(Ordering::Acquire));
    let stored_h = f64::from(OVERLAY_PHYS_H.load(Ordering::Acquire));
    let (cw, ch) = client_size_physical().unwrap_or((1.0, 1.0));
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire)).max(0.01);
    let css_w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
    let css_h = f64::from_bits(CSS_VIEW_H_BITS.load(Ordering::Acquire));
    resolve_physical_extent(stored_w, stored_h, cw, ch, css_w, css_h, scale)
}

#[cfg(windows)]
fn physical_client_to_css(x: f64, y: f64) -> (f64, f64) {
    let (pw, ph) = physical_extent();
    let (vw, vh) = css_viewport_size(pw, ph);
    map_client_to_css(x, y, pw, ph, vw, vh)
}

#[cfg(windows)]
fn css_to_physical_client(x: f64, y: f64) -> (f64, f64) {
    let (pw, ph) = physical_extent();
    let (vw, vh) = css_viewport_size(pw, ph);
    map_css_to_client(x, y, pw, ph, vw, vh)
}

#[cfg(not(windows))]
fn physical_client_to_css(x: f64, y: f64) -> (f64, f64) {
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire)).max(0.01);
    (x / scale, y / scale)
}

#[cfg(not(windows))]
fn css_to_physical_client(x: f64, y: f64) -> (f64, f64) {
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire)).max(0.01);
    (x * scale, y * scale)
}

/// El frontend avisa su `innerWidth/Height` real.
///
/// Con DPI mixto, `client/scale_factor` no coincide con lo que pinta WebView2.
/// Hit-test y fly-to tienen que usar el mismo espacio que `getBoundingClientRect`.
#[tauri::command]
pub fn set_overlay_css_viewport(w: f64, h: f64) {
    if w <= 1.0 || h <= 1.0 {
        return;
    }
    let prev_w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
    let prev_h = f64::from_bits(CSS_VIEW_H_BITS.load(Ordering::Acquire));
    if (prev_w - w).abs() < 0.5 && (prev_h - h).abs() < 0.5 {
        return;
    }
    CSS_VIEW_W_BITS.store(w.to_bits(), Ordering::Release);
    CSS_VIEW_H_BITS.store(h.to_bits(), Ordering::Release);
    tracing::info!(
        target: "overlay",
        css_w = w,
        css_h = h,
        phys_w = OVERLAY_PHYS_W.load(Ordering::Acquire),
        phys_h = OVERLAY_PHYS_H.load(Ordering::Acquire),
        "viewport CSS del overlay"
    );
    #[cfg(windows)]
    reevaluate_arm();
}

/// ¿El cursor está sobre la ventana principal visible (no minimizada)?
///
/// `GetWindowRect` + `GetCursorPos` son aptos para el hilo de Raw Input:
/// nada de APIs de Tauri acá.
#[cfg(windows)]
fn cursor_over_visible_main() -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic, IsWindowVisible};

    let hwnd = MAIN_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return false;
    }
    let hwnd = hwnd as _;
    // SAFETY: HWND de `main` vivo mientras la app corre; estas APIs solo leen.
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return false;
        }
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rc) == 0 {
            return false;
        }
        let Some((cx, cy)) = crate::floating::cursor_position() else {
            return false;
        };
        cx >= rc.left && cx < rc.right && cy >= rc.top && cy < rc.bottom
    }
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
    let Some((x, y)) = cursor_overlay_css() else {
        return;
    };

    let Ok(rects) = HIT_RECTS.try_lock() else {
        return;
    };
    let over_hit = rects.iter().any(|r| r.contains(x, y, ARM_MARGIN));
    let has_drag = rects.iter().any(|r| r.id == "drag");
    drop(rects);

    let over_main = cursor_over_visible_main();
    let gesture = POINTER_GESTURE.load(Ordering::Acquire);
    if over_main && has_drag && !gesture {
        if !YIELDED_MAIN.swap(true, Ordering::AcqRel) {
            send(Msg::YieldMain);
        }
    } else if !over_main {
        YIELDED_MAIN.store(false, Ordering::Release);
    }

    let over = should_arm(
        CAPTURING.load(Ordering::Acquire),
        over_main,
        over_hit,
        has_drag,
        gesture,
    );

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
pub struct OverlayRectCss {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Un monitor en píxeles CSS relativos al overlay.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct OverlayArea {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Área útil del monitor: la pantalla menos lo que el SO ya reservó.
    ///
    /// No es lo mismo que los bounds y la diferencia solo importa **en los
    /// bordes**, que es justo donde antes no se usaba nada: acoplar la pill al
    /// canto de abajo con los bounds la mete debajo de la barra de tareas. En
    /// Windows sale de `rcWork`, que `MonitorInfo` ya traía calculada; en macOS
    /// será la pantalla menos la barra de menú y el Dock.
    ///
    /// Es también lo que vuelve innecesario el `BOTTOM_SLOT_INSET` a ojo del
    /// frontend, que existía exactamente porque acá se devolvían bounds.
    pub work: OverlayRectCss,
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
///
/// Devuelve un rectángulo pelado y no un `OverlayArea`: esto traduce una caja
/// cualquiera, no un monitor, y no hay ningún área útil que informar.
pub fn to_local(app: &AppHandle, r: crate::floating::Rect) -> Option<OverlayRectCss> {
    #[cfg(windows)]
    {
        let (ox, oy, _scale) = frame(app)?;
        let (x, y) = physical_client_to_css(f64::from(r.x) - ox, f64::from(r.y) - oy);
        let (w, h) = physical_client_to_css(f64::from(r.w), f64::from(r.h));
        Some(OverlayRectCss { x, y, w, h })
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
        // `set_focusable` reescribe el ex-style y pisa `ignore_cursor_events`.
        // Si hay captura en curso, no reclamar foco (le robaría la selección) y
        // reponer el click-through que acabamos de borrar.
        if CAPTURING.load(Ordering::Acquire) {
            ARMED.store(false, Ordering::Release);
            set_click_through(&window, true);
            CLICK_THROUGH.store(true, Ordering::Release);
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
    let scale = window.scale_factor().unwrap_or(1.0);
    OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::Release);
    if let Ok(hwnd) = window.hwnd() {
        OVERLAY_HWND.store(hwnd.0 as isize, Ordering::Release);
    }
    let (ox, oy) = client_origin_physical().or_else(|| {
        window
            .outer_position()
            .ok()
            .map(|p| (f64::from(p.x), f64::from(p.y)))
    })?;
    Some((ox, oy, scale))
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
        // Misma conversión que el hit-test y `pill_home` (físicos − origen).
        let _ = frame(&app)?;
        let (x, y) = cursor_overlay_css()?;
        Some(OverlayPoint { x, y })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Monitor en el que abrir el launcher: mouse, o el de la ventana con foco.
///
/// Atajo global mientras escribes → pantalla de esa app. Clic en la pill →
/// pantalla del cursor, aunque el foco siga en el otro monitor.
#[tauri::command]
pub fn overlay_active_anchor(app: AppHandle) -> Option<OverlayPoint> {
    #[cfg(windows)]
    {
        let _ = frame(&app)?;
        let (sx, sy) = active_desktop_point()?;
        let (ox, oy) = client_origin_physical()?;
        let (x, y) = physical_client_to_css(f64::from(sx) - ox, f64::from(sy) - oy);
        Some(OverlayPoint { x, y })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

#[cfg(windows)]
fn hwnd_is_ours(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    if hwnd.is_null() {
        return false;
    }
    let overlay = OVERLAY_HWND.load(Ordering::Acquire);
    let main = MAIN_HWND.load(Ordering::Acquire);
    let raw = hwnd as isize;
    if (overlay != 0 && raw == overlay) || (main != 0 && raw == main) {
        return true;
    }
    let mut pid = 0u32;
    // SAFETY: HWND vivo; GetWindowThreadProcessId solo escribe el pid.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid == std::process::id()
}

#[cfg(windows)]
fn foreground_point() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, IsIconic,
    };

    // SAFETY: GetForegroundWindow no tiene precondiciones.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() || hwnd_is_ours(hwnd) {
        return None;
    }
    // SAFETY: HWND del primer plano; IsIconic / GetWindowRect solo leen.
    unsafe {
        if IsIconic(hwnd) != 0 {
            return None;
        }
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rc) == 0 {
            return None;
        }
        Some(((rc.left + rc.right) / 2, (rc.top + rc.bottom) / 2))
    }
}

#[cfg(windows)]
fn cursor_over_overlay_chrome() -> bool {
    let Some((x, y)) = cursor_overlay_css() else {
        return false;
    };
    let Ok(rects) = HIT_RECTS.try_lock() else {
        return false;
    };
    rects.iter().any(|r| r.contains(x, y, ARM_MARGIN))
}

#[cfg(windows)]
fn active_desktop_point() -> Option<(i32, i32)> {
    let cursor = crate::floating::cursor_position();
    // Clic en la pill / chrome de Atic: esa pantalla, no la del foco ajeno.
    if cursor.is_some() && cursor_over_overlay_chrome() {
        return cursor;
    }
    let focus = foreground_point();
    match (cursor, focus) {
        (Some(c), Some(f)) => {
            let cm = atic_capture::monitors::from_point(c.0, c.1);
            let fm = atic_capture::monitors::from_point(f.0, f.1);
            if cm.as_ref().map(|m| m.id.as_str()) != fm.as_ref().map(|m| m.id.as_str()) {
                Some(f)
            } else {
                Some(c)
            }
        }
        (Some(c), None) => Some(c),
        (None, Some(f)) => Some(f),
        (None, None) => None,
    }
}

/// Áreas de cada monitor en coordenadas del overlay.
///
/// El overlay abarca el escritorio virtual entero, así que su propio rectángulo
/// no sirve para decidir si algo entra: una superficie pegada al borde derecho
/// del monitor izquierdo tiene "espacio a la derecha" que en realidad es otra
/// pantalla. Quien decide hacia dónde abrir un panel necesita los monitores, no
/// el overlay.
///
/// Se usa **`bounds`** (pantalla completa), no `work_area`: la pill puede
/// solapar la barra de tareas y el borde — prerrequisito de un modo tipo
/// Dynamic Island. El shelf de capturas sigue anclándose a `work_area` en
/// `floating::Anchor::BottomCorner`.
#[tauri::command]
pub fn overlay_work_areas(app: AppHandle) -> Vec<OverlayArea> {
    #[cfg(windows)]
    {
        let Some((ox, oy, _scale)) = frame(&app) else {
            return Vec::new();
        };
        atic_capture::monitors::enumerate()
            .iter()
            .map(|m| {
                let to_css = |r: &atic_capture::Rect| {
                    let (x, y) = physical_client_to_css(f64::from(r.x) - ox, f64::from(r.y) - oy);
                    let (w, h) = physical_client_to_css(f64::from(r.width), f64::from(r.height));
                    OverlayRectCss { x, y, w, h }
                };
                let bounds = to_css(&m.bounds);
                OverlayArea {
                    x: bounds.x,
                    y: bounds.y,
                    w: bounds.w,
                    h: bounds.h,
                    work: to_css(&m.work_area),
                }
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Vec::new()
    }
}

/// ¿El botón principal del mouse está apretado ahora mismo?
///
/// El arrastre de la pill termina con `pointerup`, y ese evento **se pierde**
/// cuando el puntero se va a una ventana de otro proceso. La barra de tareas es
/// el caso claro: al soltar ahí, el gesto quedaba colgado para siempre, con el
/// hit-rect a pantalla completa puesto y el overlay armado sobre todo el
/// escritorio.
///
/// El movimiento ya se le pregunta a Win32 (`overlay_cursor`) exactamente por
/// este motivo —el comentario de `beginDrag` lo dice—, pero el final del gesto
/// seguía dependiendo del DOM. Esto cierra el otro extremo por la misma vía.
#[tauri::command]
pub fn overlay_primary_down() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
        // SAFETY: ninguna de las dos tiene precondiciones ni toca memoria nuestra.
        unsafe {
            // Con los botones invertidos el "principal" es el físico derecho.
            let vk = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
                0x02 // VK_RBUTTON
            } else {
                0x01 // VK_LBUTTON
            };
            (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
        }
    }
    #[cfg(not(windows))]
    {
        // `true` = "seguí como estabas". Devolver `false` cortaría todos los
        // arrastres al primer cuadro donde el overlay no está implementado.
        true
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

        let Some((ox, oy, _scale)) = frame(&app) else {
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
        let (px, py) = css_to_physical_client(x, y);
        state.config.lock_or_recover().pill_position = Some((ox + px, oy + py));
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

        let (ox, oy, _scale) = frame(&app)?;
        let state = app.try_state::<crate::state::AppState>()?;
        let (px, py) = state.config.lock_or_recover().pill_position?;
        let (x, y) = physical_client_to_css(px - ox, py - oy);
        Some(OverlayPoint { x, y })
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

#[cfg(test)]
mod tests {
    use super::{
        desired_click_through, map_client_to_css, map_css_to_client, resolve_physical_extent,
        should_arm,
    };

    #[test]
    fn client_to_css_follows_inner_width() {
        // DPI 1.25: cliente 1920×1080, CSS 1536×864. Centro → 768,432.
        let (x, y) = map_client_to_css(960.0, 540.0, 1920.0, 1080.0, 1536.0, 864.0);
        assert!((x - 768.0).abs() < 0.01);
        assert!((y - 432.0).abs() < 0.01);
        let (bx, by) = map_css_to_client(x, y, 1920.0, 1080.0, 1536.0, 864.0);
        assert!((bx - 960.0).abs() < 0.01);
        assert!((by - 540.0).abs() < 0.01);
    }

    #[test]
    fn physical_extent_prefers_placed_size_when_client_is_dip() {
        // Escritorio 3840×1080, GetClientRect ya en DIP 3072×864, escala 1.25.
        let (pw, ph) = resolve_physical_extent(3840.0, 1080.0, 3072.0, 864.0, 3072.0, 864.0, 1.25);
        assert!((pw - 3840.0).abs() < 0.01);
        assert!((ph - 1080.0).abs() < 0.01);
        // Cursor físico sobre la pill (193,292) en el monitor primario:
        // cliente 2113 → CSS 1690, estable antes y después del viewport.
        let (x, y) = map_client_to_css(2113.0, 292.0, pw, ph, 3072.0, 864.0);
        assert!((x - 1690.4).abs() < 0.1);
        assert!((y - 233.6).abs() < 0.1);
        let fallback_css_w = pw / 1.25;
        let fallback_css_h = ph / 1.25;
        let (x0, y0) = map_client_to_css(2113.0, 292.0, pw, ph, fallback_css_w, fallback_css_h);
        assert!((x - x0).abs() < 0.1);
        assert!((y - y0).abs() < 0.1);
    }

    #[test]
    fn physical_extent_reconstructs_when_client_matches_css() {
        let (pw, ph) = resolve_physical_extent(0.0, 0.0, 3072.0, 864.0, 3072.0, 864.0, 1.25);
        assert!((pw - 3840.0).abs() < 0.01);
        assert!((ph - 1080.0).abs() < 0.01);
    }

    #[test]
    fn overlay_arms_pill_even_over_main() {
        assert!(!should_arm(false, false, false, false, false));
        assert!(should_arm(false, false, true, false, false));
        // Pill / float encima de `main`: tiene que recibir el mouse.
        assert!(should_arm(false, true, true, false, false));
        // Hit-rect fullscreen muerto encima de `main`: ceder el input.
        assert!(!should_arm(false, true, true, true, false));
        assert!(!should_arm(true, false, true, false, false));
        assert!(!should_arm(true, true, true, false, false));
    }

    #[test]
    fn overlay_stays_armed_during_pointer_gesture() {
        assert!(should_arm(false, true, false, true, true));
        assert!(should_arm(false, true, true, true, true));
        assert!(!should_arm(true, true, true, true, true));
    }

    #[test]
    fn capture_forces_click_through_even_if_armed() {
        assert!(desired_click_through(true, true));
        assert!(desired_click_through(true, false));
        assert!(!desired_click_through(false, true));
        assert!(desired_click_through(false, false));
    }
}
