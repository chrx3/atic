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

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, AtomicU32, AtomicU64, Ordering};
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

/// Los hit-rects que poda un arrastre de ítem, para devolverlos al terminar.
static ITEM_DRAG_SAVED_RECTS: Mutex<Vec<HitCss>> = Mutex::new(Vec::new());

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

/// El último apply no llegó a cubrir el escritorio: hay que reintentar.
static COVER_RETRY: AtomicBool = AtomicBool::new(false);
static COVER_TRIES: AtomicU32 = AtomicU32::new(0);

/// Avisos de `WM_DISPLAYCHANGE` (capacidad 1: varios seguidos son el mismo).
static DISPLAY_TX: OnceLock<SyncSender<()>> = OnceLock::new();

/// Hasta cuándo ignorar `WM_DISPLAYCHANGE` (ms Unix): el propio `SetWindowPos`
/// lo dispara, y resetear `COVER_TRIES` ahí reencuadraba en loop sobre `main`.
#[cfg(windows)]
static IGNORE_DISPLAY_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
/// Último `apply_overlay_geometry`, para reintentar un HWND recortado
/// (p.ej. al colgar una llamada) sin spamear cada 2 s.
#[cfg(windows)]
static LAST_APPLY_MS: AtomicU64 = AtomicU64::new(0);

/// Handle de la app para la wndproc: no llega por parámetro (es un callback
/// crudo de Win32), así que se guarda acá una sola vez en `setup()`.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

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
    /// Clic fuera de todas las zonas: para el frontend es «cierra lo que tengas».
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
        // Dev: puerto CDP propio. El perfil compartido (main/launcher) toma
        // el de WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS; este webview usa otro
        // user-data-dir y sin puerto propio queda indepurable.
        #[cfg(debug_assertions)]
        let args = "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion --remote-debugging-port=9223";
        #[cfg(not(debug_assertions))]
        let args = "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion";
        builder = builder.additional_browser_args(args);
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
        let rect = apply_overlay_geometry(&window);
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
            scale = rect.scale,
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
    let scale = crate::webview_tweaks::sync_controller_bounds(window)
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0).max(0.01));
    OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
    OVERLAY_PHYS_W.store(rect.w.max(0) as u32, Ordering::SeqCst);
    OVERLAY_PHYS_H.store(rect.h.max(0) as u32, Ordering::SeqCst);
    note_overlay_apply();
    if let Ok(mut g) = APPLIED_TOPO.lock() {
        *g = Some(display_topo());
    }
    OverlayRect { scale, ..rect }
}

/// Reaplica el tamaño de Ajustes sin reencuadrar el HWND.
pub fn refresh_overlay_scale(app: &AppHandle) {
    #[cfg(windows)]
    {
        CSS_VIEW_W_BITS.store(0, Ordering::Release);
        CSS_VIEW_H_BITS.store(0, Ordering::Release);
        if let Some(window) = app.get_webview_window(LABEL) {
            if let Some(scale) = crate::webview_tweaks::sync_controller_bounds(&window) {
                OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
            }
        }
        if let Some(window) = app.get_webview_window("capture-overlay") {
            let _ = crate::webview_tweaks::sync_controller_bounds(&window);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// El `SetWindowPos` del apply dispara `WM_DISPLAYCHANGE`: no hay que
/// tratarlo como un cambio de escritorio real.
#[cfg(windows)]
fn note_overlay_apply() {
    let t = unix_ms();
    LAST_APPLY_MS.store(t, Ordering::Relaxed);
    IGNORE_DISPLAY_UNTIL_MS.store(t.saturating_add(800), Ordering::Relaxed);
}

#[cfg(windows)]
fn display_event_is_echo() -> bool {
    unix_ms() < IGNORE_DISPLAY_UNTIL_MS.load(Ordering::Relaxed)
}

/// Tras una suspensión larga con cambio de topología, Windows puede dejar el
/// overlay minimizado: `GetWindowRect` devuelve la posición icónica
/// (-32000,-32000) y ni `set_position` ni `SetWindowPos` lo sacan de ahí, así
/// que `overlay_covers_topo` no se cumple nunca. Restaurar sin activar para no
/// robarle el foco a la ventana de adelante.
#[cfg(windows)]
fn restore_overlay_if_minimized(window: &tauri::WebviewWindow) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, ShowWindow, SW_SHOWNOACTIVATE};

    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    // SAFETY: HWND de Tauri vivo; `IsIconic` solo lee y `ShowWindow` cambia el
    // estado de esa ventana.
    unsafe {
        if IsIconic(hwnd.0 as _) == 0 {
            return false;
        }
        ShowWindow(hwnd.0 as _, SW_SHOWNOACTIVATE);
    }
    tracing::info!(
        target: "overlay",
        "overlay minimizado por Windows; restaurado sin activar"
    );
    true
}

/// ¿El overlay quedó icónico? `IsIconic` sobre el HWND guardado: apto para el
/// hilo de poll, que no puede tocar APIs de Tauri fuera del hilo principal.
#[cfg(windows)]
fn overlay_is_iconic() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsIconic;

    let hwnd = OVERLAY_HWND.load(Ordering::Acquire);
    // SAFETY: HWND del overlay vivo mientras la app corre; `IsIconic` solo lee.
    hwnd != 0 && unsafe { IsIconic(hwnd as _) != 0 }
}

/// Si Windows acaba de sumar/quitar un monitor, reajusta el overlay.
#[cfg(windows)]
fn sync_overlay_to_displays(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    install_overlay_hooks(&window);
    if restore_overlay_if_minimized(&window) {
        // Los reintentos gastados mientras estaba icónico no cuentan: sin
        // presupuesto nuevo, la rama de rendición deja la pill invisible.
        COVER_TRIES.store(0, Ordering::Relaxed);
    }
    let topo = display_topo();
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire))
        .max(window.scale_factor().unwrap_or(1.0))
        .max(0.01);
    let same = APPLIED_TOPO
        .lock()
        .ok()
        .and_then(|g| *g)
        .is_some_and(|prev| prev == topo);
    let covers = overlay_covers_topo(&window, &topo, scale);
    if same && covers {
        COVER_RETRY.store(false, Ordering::Relaxed);
        COVER_TRIES.store(0, Ordering::Relaxed);
        cover_virtual_screen(&window);
        if let Some(scale) = crate::webview_tweaks::sync_controller_bounds(&window) {
            OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
        }
        return;
    }
    // DPI mixto: `GetWindowRect` en DIP nunca iguala `topo` físico con
    // `scale_factor=1`. Cada `SetWindowPos` dispara `WM_DISPLAYCHANGE` y el
    // overlay se reencuadra encima de `main` hasta dejar el escritorio muerto.
    if same && COVER_TRIES.load(Ordering::Relaxed) >= 8 {
        COVER_RETRY.store(false, Ordering::Relaxed);
        cover_virtual_screen(&window);
        if let Some(scale) = crate::webview_tweaks::sync_controller_bounds(&window) {
            OVERLAY_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
        }
        return;
    }
    if !same {
        COVER_TRIES.store(0, Ordering::Relaxed);
        // El innerWidth viejo aplasta los dos monitores en el recuadro de uno.
        CSS_VIEW_W_BITS.store(0, Ordering::Release);
        CSS_VIEW_H_BITS.store(0, Ordering::Release);
    }
    let outer = window
        .outer_position()
        .ok()
        .zip(window.outer_size().ok())
        .map(|(p, s)| format!("{},{} {}x{}", p.x, p.y, s.width, s.height))
        .unwrap_or_else(|| "?".into());
    tracing::info!(
        target: "overlay",
        monitores = topo.n,
        same,
        covers,
        scale,
        hwnd_rect = %outer,
        "escritorio virtual cambió; overlay a {},{} {}x{}",
        topo.x,
        topo.y,
        topo.w,
        topo.h
    );
    let rect = apply_overlay_geometry(&window);
    let scale = rect.scale.max(0.01);
    let covers_now = overlay_covers_topo(&window, &topo, scale);
    if covers_now {
        COVER_RETRY.store(false, Ordering::Relaxed);
        COVER_TRIES.store(0, Ordering::Relaxed);
    } else {
        let tries = COVER_TRIES.fetch_add(1, Ordering::Relaxed) + 1;
        COVER_RETRY.store(tries < 8, Ordering::Relaxed);
    }
    // Reaplicar un HWND recortado no debe emitir `overlay-ready` ni subir el
    // overlay: en DPI mixto eso dejaba la lámina encima de `main` cada 2 s.
    if !same || covers_now {
        keep_non_occluding(&window);
        let _ = app.emit("overlay-ready", ());
        reevaluate_arm();
    }
}

/// `SetWindowRect` a veces viene en DIP y `topo` en físicos. Aceptar ambos.
fn extent_matches(got: i32, want: i32, scale: f64) -> bool {
    const SLACK: i32 = 32;
    if (got - want).abs() <= SLACK {
        return true;
    }
    if scale <= 1.01 {
        return false;
    }
    let scaled = (f64::from(got) * scale).round() as i32;
    let unscaled = (f64::from(got) / scale).round() as i32;
    (scaled - want).abs() <= SLACK || (unscaled - want).abs() <= SLACK
}

/// `SetWindowPos` a veces se recorta al monitor actual aunque la topología
/// ya sea de dos pantallas. Hay que comparar el HWND, no el último apply.
#[cfg(windows)]
fn overlay_covers_topo(window: &tauri::WebviewWindow, topo: &DisplayTopo, scale: f64) -> bool {
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    let Some(outer) = hwnd_outer_rect(hwnd.0 as isize) else {
        return false;
    };
    hwnd_rect_covers_topo(outer, topo, scale)
}

#[cfg(windows)]
fn hwnd_outer_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

    if hwnd == 0 {
        return None;
    }
    let mut outer = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: HWND del overlay vivo; GetWindowRect solo lee.
    if unsafe { GetWindowRect(hwnd as _, &mut outer) } == 0 {
        return None;
    }
    Some((
        outer.left,
        outer.top,
        outer.right - outer.left,
        outer.bottom - outer.top,
    ))
}

#[cfg(windows)]
fn hwnd_rect_covers_topo(outer: (i32, i32, i32, i32), topo: &DisplayTopo, scale: f64) -> bool {
    let (x, y, w, h) = outer;
    if rect_matches_topo(x, y, w, h, topo, scale) {
        return true;
    }
    // `scale_factor` del HWND que cubre dos monitores suele ser 1.0, y
    // `GetWindowRect` viene en DIP del monitor al 125%. Probar el factor
    // real (físico/CSS) y la escala más alta de los monitores.
    let css_w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
    let phys_w = f64::from(OVERLAY_PHYS_W.load(Ordering::Acquire));
    if css_w > 1.0 && phys_w > 1.0 {
        let inferred = phys_w / css_w;
        if rect_matches_topo(x, y, w, h, topo, inferred) {
            return true;
        }
    }
    let max_mon = atic_capture::monitors::enumerate()
        .into_iter()
        .map(|m| m.scale)
        .fold(1.0_f64, f64::max);
    if (max_mon - scale).abs() > 0.02 {
        return rect_matches_topo(x, y, w, h, topo, max_mon);
    }
    false
}

/// ¿El HWND cubre el escritorio virtual ahora? Apta para el hilo de poll.
#[cfg(windows)]
fn overlay_hwnd_covers_virtual() -> bool {
    let hwnd = OVERLAY_HWND.load(Ordering::Acquire);
    let Some(outer) = hwnd_outer_rect(hwnd) else {
        return true;
    };
    let topo = display_topo();
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire)).max(0.01);
    hwnd_rect_covers_topo(outer, &topo, scale)
}

#[cfg(windows)]
fn rect_matches_topo(x: i32, y: i32, w: i32, h: i32, topo: &DisplayTopo, scale: f64) -> bool {
    extent_matches(x, topo.x, scale)
        && extent_matches(y, topo.y, scale)
        && extent_matches(w, topo.w as i32, scale)
        && extent_matches(h, topo.h as i32, scale)
}

fn start_display_watch(app: AppHandle) {
    let (tx, rx) = sync_channel::<()>(1);
    if DISPLAY_TX.set(tx.clone()).is_err() {
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
    // Tras hibernar el segundo monitor suele aparecer segundos después, sin
    // otro `WM_DISPLAYCHANGE`. Comparar topología es barato.
    //
    // Un overlay icónico también entra acá: si Windows lo minimiza con la
    // topología ya asentada no llega ningún mensaje y el resto de la condición
    // nunca se cumple, así que la restauración no se dispararía sola.
    std::thread::Builder::new()
        .name("atic-display-poll".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            #[cfg(windows)]
            {
                let topo = display_topo();
                let recorded = APPLIED_TOPO.lock().ok().and_then(|g| *g);
                let uncovered = !overlay_hwnd_covers_virtual();
                let tries = COVER_TRIES.load(Ordering::Relaxed);
                let apply_age = unix_ms().saturating_sub(LAST_APPLY_MS.load(Ordering::Relaxed));
                // Tras una llamada, el HWND queda recortado a un monitor y
                // `COVER_TRIES` ya gastó el presupuesto: hay que volver a
                // pedir cobertura, pero no cada 2 s (loop sobre `main`).
                if uncovered && tries >= 8 && apply_age > 15_000 {
                    COVER_TRIES.store(0, Ordering::Relaxed);
                    COVER_RETRY.store(true, Ordering::Relaxed);
                }
                if recorded != Some(topo)
                    || COVER_RETRY.load(Ordering::Relaxed)
                    || overlay_is_iconic()
                    || (uncovered && tries < 8)
                {
                    let _ = tx.try_send(());
                }
            }
            #[cfg(not(windows))]
            {
                let _ = &tx;
            }
        })
        .ok();
}

#[cfg(windows)]
fn orig_overlay_wndproc() -> Option<
    unsafe extern "system" fn(
        windows_sys::Win32::Foundation::HWND,
        u32,
        windows_sys::Win32::Foundation::WPARAM,
        windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT,
> {
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
        CallWindowProcW, HTCLIENT, HTTRANSPARENT, MINMAXINFO, WM_DEVICECHANGE, WM_DISPLAYCHANGE,
        WM_DPICHANGED, WM_GETMINMAXINFO, WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_NCHITTEST,
        WM_POWERBROADCAST,
    };

    if msg == WM_NCHITTEST {
        let (sx, sy) = lparam_screen_point(lparam);
        if overlay_eats_physical(sx, sy) {
            return HTCLIENT as isize;
        }
        return HTTRANSPARENT as isize;
    }

    if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
        let (sx, sy) = lparam_screen_point(lparam);
        if !overlay_eats_physical(sx, sy) {
            dispatch_wheel_to_main(msg, wparam);
            return 0;
        }
    }

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

    // Hibernar / desbloquear / colgar una llamada: a veces no hay
    // `WM_DISPLAYCHANGE` cuando el segundo monitor reaparece. El poll cubre
    // el resto. El eco de nuestro `SetWindowPos` no debe resetear reintentos.
    if msg == WM_DISPLAYCHANGE
        || msg == WM_DPICHANGED
        || msg == WM_POWERBROADCAST
        || msg == WM_DEVICECHANGE
    {
        if !display_event_is_echo() {
            COVER_TRIES.store(0, Ordering::Relaxed);
            COVER_RETRY.store(true, Ordering::Relaxed);
        }
        if let Some(tx) = DISPLAY_TX.get() {
            let _ = tx.try_send(());
        }
    }

    match orig_overlay_wndproc() {
        Some(orig_fn) => CallWindowProcW(Some(orig_fn), hwnd, msg, wparam, lparam),
        None => 0,
    }
}

/// Le hace creer a `main` que giró la rueda, sin pasar por Win32.
///
/// Reenviar `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` con `PostMessageW` no sirve:
/// WebView2/Chromium ignora ese mensaje inyectado, así que la rueda sonaba
/// pero `ToolRail` nunca se enteraba. En cambio, disparar un `WheelEvent` de
/// verdad en el DOM de `main` es indistinguible de un scroll real para
/// `onWindowWheel` — no lee posición, solo `deltaY`/`deltaX` y `event.target`
/// (que acá es `window`, así que `regionStillScrolls` no lo confunde con una
/// lista con scroll propio).
#[cfg(windows)]
fn dispatch_wheel_to_main(msg: u32, wparam: windows_sys::Win32::Foundation::WPARAM) {
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL;

    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    // HIWORD(wParam), con signo: notch positivo = aleja al usuario.
    let notches = (wparam >> 16) as u16 as i16 as f64 / 120.0;
    let (delta_y, delta_x) = if msg == WM_MOUSEWHEEL {
        (-notches * 100.0, 0.0)
    } else {
        (0.0, notches * 100.0)
    };
    let script = format!(
        "window.dispatchEvent(new WheelEvent('wheel',{{deltaY:{delta_y},deltaX:{delta_x},bubbles:true,cancelable:true}}))"
    );
    let _ = main.eval(&script);
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
    restack(app, Restack::Front);
}

/// Al armar por hover: topmost respecto de otras apps, pero debajo de las
/// ventanas de trabajo (anotar). Si no, pasar cerca de la pill sube TODAS las
/// flotantes encima del editor aunque no las estés tocando.
fn raise_for_pointer(app: &AppHandle) {
    restack(app, Restack::BelowWork);
}

/// El editor de anotaciones acaba de mostrarse: el overlay no debe taparlo.
pub fn yield_to_work_windows(app: &AppHandle) {
    if CAPTURING.load(Ordering::Acquire) {
        return;
    }
    tuck_below_work_windows(app);
}

enum Restack {
    Front,
    BelowWork,
}

fn restack(app: &AppHandle, _how: Restack) {
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
        if on {
            tuck_below_work_windows(app);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, on, _how);
    }
}

/// Ventanas que tienen que quedar encima del overlay (lámina a pantalla completa).
fn work_windows_above_overlay() -> &'static [&'static str] {
    &[crate::annotate::ANNOTATE_LABEL]
}

fn tuck_below_work_windows(app: &AppHandle) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        let Some(overlay) = app.get_webview_window(LABEL) else {
            return;
        };
        let Ok(overlay_hwnd) = overlay.hwnd() else {
            return;
        };
        for label in work_windows_above_overlay() {
            let Some(window) = app.get_webview_window(*label) else {
                continue;
            };
            if !window.is_visible().unwrap_or(false) {
                continue;
            }
            let Ok(above) = window.hwnd() else {
                continue;
            };
            // SAFETY: ambos HWND los da Tauri y viven mientras vivan las ventanas.
            // `hWndInsertAfter` = la ventana que queda ENCIMA del overlay.
            unsafe {
                SetWindowPos(
                    overlay_hwnd.0 as _,
                    above.0 as _,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
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
    // `ignore_cursor_events` pone `WS_EX_TRANSPARENT` en el HWND de tao. El
    // hijo `WRY_WEBVIEW` no lo hereda: la rueda del mouse sigue yendo a
    // Chromium (lámina a pantalla completa) y la ventana de Reuniones nunca
    // ve el evento. En Chrome/`pnpm dev` esa lámina no existe.
    #[cfg(windows)]
    sync_webview_child_transparent(window, on);
}

/// `WS_EX_TRANSPARENT` del hijo WebView2, alineado al click-through del padre.
#[cfg(windows)]
fn sync_webview_child_transparent(window: &tauri::WebviewWindow, on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TRANSPARENT,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let child = wry_webview_child(hwnd.0 as _);
    if child.is_null() {
        return;
    }
    // SAFETY: hijo `WRY_WEBVIEW` del overlay; solo se toca el bit transparente.
    unsafe {
        let ex = GetWindowLongPtrW(child, GWL_EXSTYLE);
        let bit = WS_EX_TRANSPARENT as isize;
        let next = if on { ex | bit } else { ex & !bit };
        if next != ex {
            SetWindowLongPtrW(child, GWL_EXSTYLE, next);
        }
    }
}

#[cfg(windows)]
fn wry_webview_child(
    parent: windows_sys::Win32::Foundation::HWND,
) -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW;
    let mut class: Vec<u16> = "WRY_WEBVIEW".encode_utf16().collect();
    class.push(0);
    // SAFETY: parent es un HWND de Tauri; FindWindowEx solo busca el hijo.
    unsafe {
        FindWindowExW(
            parent,
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        )
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
    let _ = APP_HANDLE.set(app.clone());
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
            // 30 s y no 8: con `vite dev` el frontend tarda ~8 s en reportar y
            // el timeout corto destruía una ventana SANA. La recreada salía
            // recortada (covers=false) con los hit-rects corridos: pill y
            // escritorio muertos a los segundos de abrir.
            for _ in 0..300 {
                let css_w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
                if css_w > 1.0 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            // Último chequeo antes de destruir: el reporte pudo llegar entre
            // la última muestra y este punto (así se perdía por 78 ms).
            if f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire)) > 1.0 {
                return;
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
                            if POINTER_GESTURE.load(Ordering::Acquire) {
                                raise(&app);
                            } else {
                                raise_for_pointer(&app);
                            }
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
        // Guardar antes de podar: quien arrastra desde el propio overlay los
        // republica al soltar, pero el estante es otra ventana y no puede, y
        // sin ellos el overlay queda muerto al mouse.
        if let Ok(mut saved) = ITEM_DRAG_SAVED_RECTS.lock() {
            if let Ok(mut guard) = HIT_RECTS.lock() {
                saved.clone_from(&guard);
                guard.retain(|r| is_ole_drop_target(&r.id));
            }
        }
        ARMED.store(false, Ordering::SeqCst);
        if let Some(window) = app.get_webview_window(LABEL) {
            set_click_through(&window, true);
            CLICK_THROUGH.store(true, Ordering::SeqCst);
        }
        #[cfg(windows)]
        reevaluate_arm();
    } else {
        // Devolver lo podado. Si el overlay ya republicó por su cuenta, su
        // envío es más nuevo que esta copia y pisa a esta enseguida.
        if let Ok(mut saved) = ITEM_DRAG_SAVED_RECTS.lock() {
            if !saved.is_empty() {
                if let Ok(mut guard) = HIT_RECTS.lock() {
                    *guard = std::mem::take(&mut *saved);
                }
            }
        }
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
    overlay_css_from_physical(cx, cy)
}

/// Cursor en CSS del overlay, o `None` fuera de Windows / sin HWND.
pub fn cursor_css_point() -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        cursor_overlay_css()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Punto de pantalla (p. ej. `lParam` de `WM_NCHITTEST`) → CSS del overlay.
#[cfg(windows)]
fn overlay_css_from_physical(cx: i32, cy: i32) -> Option<(f64, f64)> {
    let (ox, oy) = client_origin_physical()?;
    Some(physical_client_to_css(
        f64::from(cx) - ox,
        f64::from(cy) - oy,
    ))
}

/// Coordenadas de pantalla empaquetadas en `lParam` (signed 16-bit).
pub(crate) fn lparam_screen_point(lparam: isize) -> (i32, i32) {
    let x = lparam as i16 as i32;
    let y = (lparam >> 16) as i16 as i32;
    (x, y)
}

/// ¿Este punto tiene que quedárselo el overlay (pill / float / gesto)?
///
/// Fuera de esas zonas la lámina es a pantalla completa: si come el hit, la
/// rueda no llega a `main` aunque Atic se vea debajo.
#[cfg(windows)]
fn overlay_eats_physical(cx: i32, cy: i32) -> bool {
    if CAPTURING.load(Ordering::Acquire) {
        return false;
    }
    if POINTER_GESTURE.load(Ordering::Acquire) {
        return true;
    }
    let Some((x, y)) = overlay_css_from_physical(cx, cy) else {
        return false;
    };
    let Ok(rects) = HIT_RECTS.try_lock() else {
        return false;
    };
    let over_hit = rects.iter().any(|r| r.contains(x, y, ARM_MARGIN));
    let has_drag = rects.iter().any(|r| r.id == "drag");
    drop(rects);
    // Mismo caso especial que `should_arm`: el rect "drag" cubre toda la
    // pantalla mientras dura un arrastre. Si se lo comiera igual sobre
    // `main`, `main` nunca recibiría el movimiento que dispara
    // `yield_to_main` y el arrastre quedaría pegado para siempre — junto con
    // el resto de la ventana, porque a partir de acá decide el hit-test.
    if has_drag && cursor_over_visible_main() {
        return false;
    }
    over_hit
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

/// Dónde está el cursor, en CSS de la ventana que pregunta.
#[tauri::command]
pub fn window_cursor(window: tauri::WebviewWindow) -> Option<OverlayPoint> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::ScreenToClient;

        let (sx, sy) = crate::floating::cursor_position()?;
        let hwnd = window.hwnd().ok()?;
        let mut pt = POINT { x: sx, y: sy };
        // SAFETY: HWND de esta webview; ScreenToClient solo escribe `pt`.
        if unsafe { ScreenToClient(hwnd.0 as _, &mut pt) } == 0 {
            return None;
        }
        let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
        Some(OverlayPoint {
            x: f64::from(pt.x) / scale,
            y: f64::from(pt.y) / scale,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
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

/// ¿El CSS que mandó el frontend describe el cliente actual?
///
/// Recuadro chico al boot: CSS 1551, cliente 3840 → no. Tras una llamada:
/// CSS 3840 (escritorio viejo), cliente recortado 1920 → tampoco. En ambos
/// casos hay que caer a `phys/scale` o los dos monitores se pintan en uno.
fn css_viewport_usable(css_w: f64, css_h: f64, client_w: f64, client_h: f64) -> bool {
    if css_w <= 1.0 || css_h <= 1.0 || client_w <= 1.0 || client_h <= 1.0 {
        return false;
    }
    let wr = css_w / client_w;
    let hr = css_h / client_h;
    // 0.45 cubre DPI 200% (CSS = cliente físico / 2) y rechaza 1551/3840.
    // 1.2 rechaza un innerWidth de dos monitores sobre un HWND de uno.
    (0.45..=1.2).contains(&wr) && (0.45..=1.2).contains(&hr)
}

fn pick_css_viewport(
    css_w: f64,
    css_h: f64,
    client_w: f64,
    client_h: f64,
    phys_w: f64,
    phys_h: f64,
    scale: f64,
) -> (f64, f64) {
    let fallback = || {
        let s = scale.max(0.01);
        (phys_w / s, phys_h / s)
    };
    if client_w <= 1.0 || client_h <= 1.0 {
        if css_w > 1.0 && css_h > 1.0 {
            return (css_w, css_h);
        }
        return fallback();
    }
    if css_viewport_usable(css_w, css_h, client_w, client_h) {
        return (css_w, css_h);
    }
    fallback()
}

fn css_viewport_size(phys_w: f64, phys_h: f64) -> (f64, f64) {
    let w = f64::from_bits(CSS_VIEW_W_BITS.load(Ordering::Acquire));
    let h = f64::from_bits(CSS_VIEW_H_BITS.load(Ordering::Acquire));
    let scale = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire)).max(0.01);
    #[cfg(windows)]
    {
        let (cw, ch) = client_size_physical().unwrap_or((0.0, 0.0));
        return pick_css_viewport(w, h, cw, ch, phys_w, phys_h, scale);
    }
    #[cfg(not(windows))]
    {
        if w > 1.0 && h > 1.0 {
            return (w, h);
        }
        (phys_w / scale, phys_h / scale)
    }
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
        // HWND recortado (un monitor) con stored del escritorio dual: si se
        // usa stored, el centro cae en el canto visible y "no hay 2ª pantalla".
        if client_w > 1.0 && stored_w > client_w * 1.35 {
            return (client_w, client_h);
        }
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
    /// Monitor principal de Windows. La pill arranca en su canto de arriba.
    pub primary: bool,
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
/// `set_focusable` reescribe el ex-style y pisa `ignore_cursor_events`.
/// Después hay que rearmar: si no, el overlay queda opaco a pantalla completa.
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
        // El overlay nace sin foco y puede haber recibido el ajuste de WebView2
        // antes de que su controlador estuviera listo. Reintentar aquí cubre el
        // instante real en que la consola flotante empieza a recibir teclado.
        crate::webview_tweaks::disable_browser_accelerator_keys(&window);
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
        // `set_focusable` reescribe el ex-style; reconciliar el armado con el
        // cursor real. Arma solo si está sobre un hit-rect, y si no, repone el
        // click-through — sin forzar estados que rompan el clic siguiente.
        reevaluate_arm();
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
    let stored = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire));
    let scale = if stored > 1.001 {
        stored
    } else {
        window.scale_factor().unwrap_or(1.0)
    };
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
                    primary: m.is_primary,
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
        // `true` = "sigue como estabas". Devolver `false` cortaría todos los
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
        let stored = f64::from_bits(OVERLAY_SCALE_BITS.load(Ordering::Acquire));
        let scale = stored.max(window.scale_factor().unwrap_or(1.0));
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
        css_viewport_usable, desired_click_through, extent_matches, lparam_screen_point,
        map_client_to_css, map_css_to_client, pick_css_viewport, resolve_physical_extent,
        should_arm,
    };

    #[test]
    fn lparam_packs_negative_virtual_screen() {
        // Monitor a la izquierda: x = -1920, y = 540.
        let lparam =
            ((540i32 as u16 as isize) << 16) | ((-1920i32 as i16 as u16 as isize) & 0xFFFF);
        let (x, y) = lparam_screen_point(lparam);
        assert_eq!(x, -1920);
        assert_eq!(y, 540);
    }

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
    fn physical_extent_distrusts_stored_when_hwnd_is_clipped() {
        let (pw, ph) = resolve_physical_extent(3840.0, 1080.0, 1920.0, 1080.0, 1920.0, 1080.0, 1.0);
        assert!((pw - 1920.0).abs() < 0.01);
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

    #[test]
    fn extent_matches_physical_or_dip() {
        assert!(extent_matches(3840, 3840, 1.25));
        assert!(extent_matches(3072, 3840, 1.25));
        assert!(extent_matches(-1536, -1920, 1.25));
        assert!(!extent_matches(1920, 3840, 1.25));
        assert!(!extent_matches(0, 3840, 1.0));
    }

    #[test]
    fn css_viewport_rejects_boot_letterbox_and_clipped_hwnd() {
        // Recuadro chico WebView2 vs escritorio dual.
        assert!(!css_viewport_usable(1551.0, 864.0, 3840.0, 1080.0));
        // Tras una llamada: innerWidth de dos monitores, HWND de uno.
        assert!(!css_viewport_usable(3840.0, 1080.0, 1920.0, 1080.0));
        // DPI 125%: CSS DIP, cliente físico.
        assert!(css_viewport_usable(3072.0, 864.0, 3840.0, 1080.0));
        assert!(css_viewport_usable(1920.0, 1080.0, 1920.0, 1080.0));
    }

    #[test]
    fn pick_css_falls_back_when_webview_lied() {
        let (w, h) = pick_css_viewport(3840.0, 1080.0, 1920.0, 1080.0, 3840.0, 1080.0, 1.0);
        assert!((w - 3840.0).abs() < 0.01);
        assert!((h - 1080.0).abs() < 0.01);
        let (w, h) = pick_css_viewport(3072.0, 864.0, 3072.0, 864.0, 3840.0, 1080.0, 1.25);
        assert!((w - 3072.0).abs() < 0.01);
        assert!((h - 864.0).abs() < 0.01);
    }
}
