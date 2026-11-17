//! Vistazos de las herramientas de la tira.
//!
//! Como `toolPeek` de la pill web: quedarse sobre una herramienta del notch
//! baja una franja con lo último de esa herramienta, usable ahí mismo:
//! - **Color**: los últimos colores; pasar por uno muestra su código y un
//!   clic lo copia.
//! - **Capturas**: las últimas (en Expressive, un carrusel héroe que se
//!   recorre de lado); un clic copia la imagen y arrastrarla (hacia abajo,
//!   en el carrusel) la suelta como archivo en otra app.
//! - **Textos**: los tres guardados más recientes; un clic los pega donde
//!   estabas.
//! - **Flip**: las páginas del tablero en miniatura (en Expressive, un
//!   carrusel multi-browse); un clic abre esa.
//! - **Sistema**: CPU, memoria y lo que más consume.
//! - **Reuniones**: grabar (o la llamada detectada), lo que se graba y las
//!   últimas reuniones con su estado (ver `meetings/pill.rs`).
//!
//! Aparece a los 450 ms (120 ms si ya había otro abierto: pasar de una
//! herramienta a la vecina), se queda mientras el cursor esté en la
//! herramienta o en el vistazo y se va 400 ms después de salir. Clipboard y
//! Agentes tienen los suyos (`peek` en `main.rs` y `usage.rs`); entre los
//! tres hay relevo: con uno abierto, el de la herramienta vecina baja a los
//! 120 ms y el anterior se recoge recién entonces, así el notch cambia de
//! forma en vez de cerrarse y volver a abrirse.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    div, img, prelude::*, px, rgb, svg, AnyElement, ClipboardItem, Context, Hsla, MouseButton,
    MouseDownEvent, ObjectFit, Window,
};

use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::{corner, Morph, Scheme, ShapeName, Spring};

use crate::anim::{ease_island, ease_smooth_out, segment, Anim, SpringKind, SpringTrack, Tween};
use crate::geometry::Rect;
use crate::hover::{hover_fx_cx, m3_layer, m3_press_radii, m3_press_radius, HoverExt, M3_GROUP_RADII};
use crate::i18n::t;
use crate::theme::SmallText;
use crate::usage::Drawer;
use crate::Pill;

const SHOW_DELAY: Duration = Duration::from_millis(450);
/// Sobre la carátula del tab en reposo: el vistazo de Ahora suena baja antes
/// (no hay tira que recorrer, el cursor ya está donde quiere).
const ART_DELAY: Duration = Duration::from_millis(280);
/// Con otro vistazo abierto: pasar a la herramienta vecina.
pub(crate) const SWITCH_DELAY: Duration = Duration::from_millis(120);
const GRACE: Duration = Duration::from_millis(400);
/// Tras copiar, el «Copiado» se ve un momento y el vistazo se va.
const COPIED_FOR: Duration = Duration::from_millis(650);
/// M3 Expressive: en vez de eso, un snackbar dentro del vistazo con lo copiado
/// y «Deshacer». Se queda `SNACK_FOR`, sale con `EFFECTS_FAST` y recién ahí el
/// vistazo se va. Entra subiendo `SNACK_RISE` con `SPATIAL_FAST`, que se
/// asienta en `SNACK_IN` (mientras, pide cuadros).
const SNACK_FOR: Duration = Duration::from_secs(3);
const SNACK_IN: Duration = Duration::from_millis(600);
const SNACK_RISE: f32 = 16.0;
/// Su alto (el de M3); en un vistazo bajo, el compacto.
const SNACK_H: f32 = 48.0;
const SNACK_COMPACT_H: f32 = 36.0;
/// El blanco mínimo de un clic en Expressive: lo dibujado puede ser más chico.
const HIT_M3: f32 = 40.0;
/// Lo que hay que mover el cursor con el botón apretado para que sea
/// arrastre y no clic (como en la web).
const DRAG_START: f32 = 4.0;

/// El carrusel: pasar al vistazo de al lado corre los dos juntos de lado, con
/// el mismo progreso, sin fundido (cada página es dueña de lo suyo). El alto
/// no cambia: todas las páginas toman el de la más alta.
const SLIDE_S: f32 = 0.34;
/// Lo que hay que deslizar de lado con dos dedos para pasar de página.
const SWIPE_STEP: f32 = 70.0;
/// Lo que recorre un gesto antes de decidir si es de lado o vertical.
const AXIS_LOCK: f32 = 8.0;
/// Los puntos y las flechas del pie, que no se corren con la página.
const NAV_DOT: f32 = 6.0;
const NAV_ARROW: f32 = 26.0;
/// Una entrada ya terminada: al deslizar, la página llega armada (la cascada
/// del título y las filas repetida en cada página se veía nerviosa).
const ENTERED: Duration = Duration::from_secs(3);

const SIDE: f32 = 14.0;
const TITLE_H: f32 = 30.0;
const FOOTER_H: f32 = 32.0;
const BOTTOM: f32 = 6.0;
const EMPTY_H: f32 = 30.0;
/// En Expressive, el estado vacío: forma, «Sin X», «<Verbo> Y» y la acción.
const EMPTY_M3_H: f32 = 64.0;
/// Grabando la pantalla: la fila del reloj y la de los botones, las de Reuniones.
const RECORDING_H: f32 = 30.0 + 40.0;
/// «Ahora suena», como el panel sin la portada, la barra ni el volumen (así
/// el paso al panel con el scroll no cambia de cara): título y artista
/// grandes y debajo los controles redondos, centrados.
const MEDIA_TITLE_H: f32 = 46.0;
const MEDIA_GAP: f32 = 8.0;
const MEDIA_CONTROLS_H: f32 = 50.0;
const MEDIA_BUTTON: f32 = 38.0;
/// El margen del panel (no el de los vistazos): el título no se corre de lado.
const MEDIA_SIDE: f32 = 16.0;
/// Bajo la fila, el verso que suena y el siguiente (como la letra que
/// cuelga del tab, que este vistazo reemplaza).
const MEDIA_LYRICS_H: f32 = 64.0;
const SWATCH: f32 = 30.0;
const TEXT_ROW_H: f32 = 40.0;
/// Una tarea pendiente en el vistazo de Textos, y el alto de su cuerpo: tres
/// filas, o el contador con sus dos botones.
const TASK_ROW_H: f32 = 30.0;
const TASKS_BODY_H: f32 = 3.0 * TASK_ROW_H + 8.0;
/// En un costado, el contador va arriba de la lista.
const TASKS_COUNTER_H: f32 = 84.0;
const METER_H: f32 = 24.0;
const APP_ROW_H: f32 = 24.0;
const SECTION_H: f32 = 22.0;
const SHOT_GAP: f32 = 8.0;
const SHOT_LABEL_H: f32 = 18.0;
const PAGE_GAP: f32 = 8.0;
const PAGE_LABEL_H: f32 = 30.0;
const MAX_PAGES: usize = 4;
// En el bloque angosto de un costado lo que arriba va en fila va en columna:
// una fila por color, por captura o por página, con su miniatura al lado.
const COLOR_ROW_H: f32 = 28.0;
const SIDE_COLORS: usize = 6;
const SHOT_ROW_H: f32 = 56.0;
const SHOT_THUMB: (f32, f32) = (80.0, 50.0);
const PAGE_ROW_H: f32 = 56.0;
const PAGE_THUMB: (f32, f32) = (64.0, 48.0);
/// Pizarra: los botones de herramienta, los colores y la fila de la última
/// captura.
const BOARD_BTN: f32 = 34.0;
const BOARD_BTN_GAP: f32 = 6.0;
const BOARD_COLOR_H: f32 = 26.0;
const BOARD_GAP: f32 = 8.0;
/// Pomodoro: los modos, el tiempo grande con su barra y los botones.
const POMO_MODES_H: f32 = 26.0;
const POMO_TIME_H: f32 = 52.0;
const POMO_BAR_H: f32 = 4.0;
const POMO_BUTTONS_H: f32 = 40.0;
const POMO_GAP: f32 = 10.0;
/// Computadores: la fila del estado con el interruptor, el código para
/// vincular (si hay uno vigente), un aviso y un equipo por fila con el dibujo
/// de los lados a la derecha (en un costado, debajo).
const KVM_STATUS_H: f32 = 30.0;
const KVM_CODE_H: f32 = 86.0;
const KVM_NOTE_H: f32 = 28.0;
const KVM_WHO_H: f32 = 36.0;
const KVM_SHARE_H: f32 = 32.0;
const KVM_PEER_GAP: f32 = 6.0;
/// Cuántos equipos caben en el vistazo (en un costado, uno); el resto, en el panel.
const KVM_PEERS: usize = 2;
/// Cada cuánto se vuelve a leer el estado mientras se ve.
const KVM_REFRESH: Duration = Duration::from_millis(500);

/// Las herramientas del vistazo de la Pizarra: todas menos el borrador, el
/// cuentagotas y elegir texto (abrir con ellos no sirve).
fn board_tools() -> impl Iterator<Item = &'static (crate::board::Tool, &'static str, &'static str)> {
    use crate::board::Tool;
    crate::board::TOOLS.iter().filter(|(tool, ..)| !matches!(tool, Tool::Eraser | Tool::Picker | Tool::Select | Tool::Crop | Tool::SelectText))
}

/// Cuántas filas de herramientas caben en `width` (en un costado, dos).
fn board_tool_rows(width: f32) -> usize {
    let per_row = (((width - 2.0 * SIDE + BOARD_BTN_GAP) / (BOARD_BTN + BOARD_BTN_GAP)).floor() as usize).max(1);
    board_tools().count().div_ceil(per_row)
}

/// Los ajustes rápidos van como las fichas de Sistema (Silencio, Café…).
const QUICK_TILE_H: f32 = 58.0;
const QUICK_GAP: f32 = 8.0;

// M3 Expressive: lo que cambia de medida (cada vistazo con su héroe). En Atic
// siguen las de arriba.
/// Color: la tarjeta del color que se ve, sobre la fila de muestras.
const COLOR_HERO_H: f32 = 64.0;
const COLOR_M3_GAP: f32 = 12.0;
/// Capturas: el carrusel héroe (la grande y la siguiente asomando).
const SHOTS_M3_H: f32 = 160.0;
/// Flip: el carrusel multi-browse de páginas, con su resumen encima.
const PAGES_M3_H: f32 = 140.0;
/// Los carruseles de M3 de Capturas (héroe) y Flip (multi-browse): las
/// medidas y la física del `Carousel` de gpui-m3, hechos a mano porque aquí
/// apretar una captura también la arrastra a otra app.
const REEL_RADIUS: f32 = 28.0;
const REEL_GAP: f32 = 8.0;
const REEL_SMALL: f32 = 56.0;
/// La rueda quieta este rato (s): se acomoda en un ítem.
const REEL_WHEEL_IDLE: f32 = 0.12;
/// Más allá de los extremos, el arrastre sigue a esta fracción del cursor
/// (hasta tantos ítems).
const REEL_RUBBER: f32 = 0.35;
const REEL_OVERSCROLL: f32 = 4.0;
/// Sin eventos del trackpad este rato (s), el gesto terminó: el que sigue es otro.
const REEL_GESTURE_GAP: f32 = 0.15;
/// Arrastrando el carrusel de capturas, alejarse esto en vertical suelta la
/// captura apretada como archivo.
const SHOT_DRAG_OUT: f32 = 36.0;
/// Cuántas capturas y páginas se recorren.
const REEL_SHOTS: usize = 8;
const REEL_PAGES: usize = 12;
/// Tareas: la tarjeta del contador y la lista agrupada, del mismo alto.
const TASKS_M3_H: f32 = 118.0;
const TASK_M3_ROW_H: f32 = 36.0;
const TASKS_M3_COUNTER_W: f32 = 176.0;
/// Pomodoro: la forma con el tiempo adentro, y a su lado los modos y botones.
const POMO_M3_H: f32 = 124.0;
const POMO_M3_PLAY: f32 = 56.0;
/// Sistema: las tarjetas de CPU y memoria.
const SYS_CARD_H: f32 = 64.0;
const SYS_CARD_GAP: f32 = 8.0;
/// Celular: filas de lista agrupada (sin celular vinculado, el estado vacío
/// invita a vincular).
const PHONE_M3_ROW_H: f32 = 44.0;
/// Entre las filas de una lista agrupada.
const LIST_M3_GAP: f32 = 2.0;
/// Acciones rápidas: el botón (redondo; encendido, cuadrado redondeado) y su
/// etiqueta debajo.
const QUICK_M3_BUTTON: f32 = 48.0;
const QUICK_M3_TILE_H: f32 = QUICK_M3_BUTTON + 6.0 + 16.0;

/// La cara del notch no es una herramienta de la tira, pero también tiene
/// vistazo: los ajustes rápidos.
pub(crate) const FACE: usize = usize::MAX;

/// Lo que más se hace con Atic, a un clic desde la cara: ver las novedades,
/// buscar (e instalar) una versión nueva, cambiar el idioma, el tema de las
/// ventanas y el color de la pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaceAction {
    WhatsNew,
    Update,
    Language,
    Theme,
    Color,
    /// El modo no molestar (`tab_size.rs`).
    Quiet,
    /// Editar el notch cerrado: tamaño, orden y lo que muestra.
    EditNotch,
}

const FACE_ACTIONS: [FaceAction; 7] = [
    FaceAction::WhatsNew,
    FaceAction::Update,
    FaceAction::Language,
    FaceAction::Theme,
    FaceAction::Color,
    FaceAction::Quiet,
    FaceAction::EditNotch,
];

/// Cuántas fichas van por fila: todas arriba; en un costado, de a dos.
fn face_per_row(narrow: bool) -> usize {
    if narrow { 2 } else { FACE_ACTIONS.len() }
}

/// El tema de las ventanas como lo muestra la cara: los finos de Atic
/// (sepia, midnight…) cuentan como su lado, como en Ajustes.
fn theme_side(ui_theme: &str) -> &'static str {
    match ui_theme {
        "system" | "custom" | "" => "system",
        other if crate::theme::wants_light(other) => "light",
        _ => "dark",
    }
}

/// El que sigue al apretar: sistema → claro → oscuro → sistema.
fn next_theme(ui_theme: &str) -> &'static str {
    match theme_side(ui_theme) {
        "system" => "light",
        "light" => "dark",
        _ => "system",
    }
}

/// Los tres vistazos de la tira.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeekKind {
    Clipboard,
    Usage,
    Tool,
}

/// La herramienta tiene algún vistazo (aquí, Clipboard o Agentes).
pub(crate) fn any_peek(tool: usize) -> bool {
    tool == crate::CLIPBOARD_TOOL || tool == crate::AGENTES_TOOL || has_peek(tool)
}

/// El panel del notch que abre el vistazo de `tool`, si abre uno.
fn peek_panel(tool: usize) -> Option<crate::NotchTool> {
    match tool {
        t if t == crate::TEXTOS_TOOL => Some(crate::NotchTool::Textos),
        t if t == crate::SISTEMA_TOOL => Some(crate::NotchTool::Sistema),
        t if t == crate::MEDIA_TOOL => Some(crate::NotchTool::Media),
        t if t == crate::CELULAR_TOOL => Some(crate::NotchTool::Celular),
        t if t == crate::REUNIONES_TOOL => Some(crate::NotchTool::Reuniones),
        // Computadores vive dentro del Celular (`Pill::open_computers`).
        t if t == crate::COMPUTADORES_TOOL => Some(crate::NotchTool::Celular),
        _ => None,
    }
}

/// Las herramientas con vistazo aquí.
fn has_peek(tool: usize) -> bool {
    [
        // Portapapeles y Agentes tenían sus vistazos aparte; ahora son páginas
        // del carrusel, con su contenido de siempre (`hosted_rect`).
        crate::CLIPBOARD_TOOL,
        crate::AGENTES_TOOL,
        crate::POMODORO_TOOL,
        crate::TEXTOS_TOOL,
        crate::SISTEMA_TOOL,
        crate::REUNIONES_TOOL,
        crate::capture::TOOL,
        crate::color::TOOL,
        crate::flip::TOOL,
        crate::MEDIA_TOOL,
        crate::board::TOOL,
        crate::CELULAR_TOOL,
        crate::COMPUTADORES_TOOL,
    ]
    .contains(&tool)
}

#[derive(Clone)]
struct Shot {
    path: PathBuf,
    /// «18:10», o «ayer 18:10».
    label: String,
    /// Una grabación de pantalla (mp4/mov) en vez de una captura.
    video: bool,
}

/// Los carruseles de M3 de los vistazos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ReelKind {
    /// Capturas: héroe.
    Shots,
    /// Flip: multi-browse.
    Pages,
}

/// Un carrusel de M3 de un vistazo. Es dueño de los gestos que empiezan
/// sobre él (rueda, trackpad de lado, arrastre); el resto del vistazo es del
/// carrusel de vistazos (`peek_swipe`).
struct Reel {
    /// El desplazamiento en ítems (1.0: el segundo en el lugar del foco). Al
    /// soltar se acomoda con `Spatial`; arrastrando salta al cursor.
    pos: SpringTrack,
    /// Arrastrando: la x del cursor y `pos` al empezar, y las últimas
    /// posiciones (el impulso al soltar).
    drag: Option<ReelDrag>,
    /// Rueda o trackpad de lado: `pos` al empezar y el último evento. Quieta
    /// `REEL_WHEEL_IDLE`, se acomoda.
    wheel: Option<(f32, Instant)>,
    /// Un gesto del trackpad que empezó sobre el carrusel.
    touch: Option<ReelTouch>,
    /// El largo de la tira del último cuadro (mide el paso en px).
    container: std::cell::Cell<f32>,
}

struct ReelDrag {
    x0: f32,
    pos0: f32,
    samples: Vec<(Instant, f32)>,
}

#[derive(Clone, Copy)]
struct ReelTouch {
    /// Lo recorrido de lado y vertical hasta decidir el eje, y el eje (`true`
    /// de lado).
    travel: (f32, f32),
    axis: Option<bool>,
    at: Instant,
}

/// Un ítem de un carrusel apretado: soltar sin moverse es clic; moverse de
/// lado arrastra el carrusel y (en Capturas) en vertical suelta la captura
/// como archivo.
#[derive(Clone)]
struct ReelPress {
    kind: ReelKind,
    index: usize,
    origin: (f32, f32),
    path: Option<PathBuf>,
    /// Estaba en el foco (o, en Flip, se lee): el clic actúa. Si asomaba, el
    /// clic lo trae al foco.
    act: bool,
}

impl Reel {
    fn new() -> Self {
        Self {
            pos: SpringTrack::new(0.0, SpringKind::Spatial),
            drag: None,
            wheel: None,
            touch: None,
            container: std::cell::Cell::new(crate::PANEL_W - SIDE * 2.0),
        }
    }

    fn reset(&mut self) {
        self.pos.snap(0.0);
        self.drag = None;
        self.wheel = None;
        self.touch = None;
    }

    /// Algo se mueve o espera acomodarse: hay que pedir cuadros.
    fn busy(&self, now: Instant) -> bool {
        self.drag.is_some() || self.wheel.is_some() || self.pos.is_running(now)
    }

    /// Px que corre un ítem.
    fn step(&self, hero: bool) -> f32 {
        (reel_keylines(hero, self.container.get())[0] + REEL_GAP).max(1.0)
    }
}

/// El ancho de cada ítem según su distancia al foco (como `keylines` del
/// `Carousel` de gpui-m3): héroe, el grande y uno chico asomando;
/// multi-browse, grande, mediano y chico.
fn reel_keylines(hero: bool, container: f32) -> Vec<f32> {
    let small = REEL_SMALL.min(container / 8.0);
    if hero {
        vec![(container - REEL_GAP - small).max(0.0), small]
    } else {
        let large = (container - 2.0 * REEL_GAP - small).max(0.0) / 1.6;
        vec![large, large * 0.6, small]
    }
}

/// El ancho de un ítem `distance` ítems después del foco (negativo: antes).
fn reel_extent(widths: &[f32], distance: f32) -> f32 {
    let last = widths.len() - 1;
    if distance <= -1.0 {
        0.0
    } else if distance < 0.0 {
        widths[0] * (distance + 1.0)
    } else if distance >= last as f32 {
        widths[last]
    } else {
        let i = distance.floor() as usize;
        let f = distance - i as f32;
        widths[i] + (widths[i + 1] - widths[i]) * f
    }
}

/// Los ítems a la vista: (índice, inicio, ancho) desde el borde de la tira.
fn reel_slots(hero: bool, container: f32, count: usize, offset: f32) -> Vec<(usize, f32, f32)> {
    let mut slots = Vec::new();
    if count == 0 || container <= 0.0 {
        return slots;
    }
    let widths = reel_keylines(hero, container);
    let mut i = offset.floor() as isize - 1;
    let mut x = 0.0;
    while i < count as isize && x < container {
        let extent = reel_extent(&widths, i as f32 - offset);
        if i >= 0 && extent > 0.5 {
            slots.push((i as usize, x, extent));
        }
        // El hueco crece con el ítem: uno que aparece no empuja a los demás.
        x += extent + REEL_GAP * (extent / REEL_GAP).min(1.0);
        i += 1;
    }
    slots
}

/// Hasta dónde se corre: héroe, el último en el foco; multi-browse, el
/// último en el lugar mediano (en el chico no se leería).
fn reel_max(hero: bool, count: usize) -> f32 {
    if hero { count.saturating_sub(1) as f32 } else { count.saturating_sub(2) as f32 }
}

/// Al soltar: el ítem más cercano, lanzado a lo más 1,5 ítems por la
/// velocidad (ítems por segundo).
fn reel_snap_target(offset: f32, velocity: f32, max: f32) -> f32 {
    let thrown = (velocity * 0.12).clamp(-1.5, 1.5);
    (offset + thrown).round().clamp(0.0, max.max(0.0))
}

/// La rueda se acomoda en el ítem hacia donde giró, aunque sea poco.
fn reel_wheel_target(from: f32, offset: f32, max: f32) -> f32 {
    let net = offset - from;
    let target = if net > 1e-3 {
        (offset - 1e-3).ceil()
    } else if net < -1e-3 {
        (offset + 1e-3).floor()
    } else {
        offset.round()
    };
    target.clamp(0.0, max.max(0.0))
}

/// Más allá de un extremo, el arrastre sigue a una fracción del cursor.
fn reel_rubber(raw: f32, max: f32) -> f32 {
    let max = max.max(0.0);
    if raw < 0.0 {
        raw * REEL_RUBBER
    } else if raw > max {
        max + (raw - max) * REEL_RUBBER
    } else {
        raw
    }
}

/// Una fila del vistazo de Celular.
#[derive(Clone)]
struct PhoneGlance {
    app: String,
    icon: Option<std::sync::Arc<gpui::Image>>,
    title: String,
    detail: String,
}

#[derive(Default)]
struct Data {
    colors: Vec<u32>,
    shots: Vec<Shot>,
    texts: Vec<(String, String)>,
    /// Las tareas pendientes, lo primero del vistazo de Textos, y las hechas
    /// hoy del total del día.
    tasks: Vec<crate::tasks::Task>,
    tasks_today: (usize, usize),
    pages: Vec<crate::flip_board::PageGlance>,
    /// El tema de las ventanas (`ui_theme` de config.json), para la cara.
    theme: String,
    /// Se pidió buscar una versión nueva y todavía no empieza: la ficha ya
    /// dice «Buscando…».
    update_asked: bool,
    /// Con qué empieza la próxima pizarra, y la última captura para rayarla.
    board: crate::board::Prefs,
    last_shot: Option<Shot>,
}

pub(crate) struct ToolPeek {
    /// Cuánto bajó (0 a 1).
    amount: Anim,
    /// El alto del contenido: cambia con transición al pasar de una
    /// herramienta a otra.
    height: Anim,
    /// Lo que se muestra.
    tool: Option<usize>,
    /// Cuándo cambió lo que se muestra: el contenido nuevo entra fundiéndose.
    shown_at: Instant,
    /// Al pasar de una herramienta a otra con el vistazo abierto: la que se
    /// va y su `shown_at`. Se apaga mientras entra la nueva (antes se borraba
    /// de un cuadro a otro).
    prev: Option<(usize, Instant)>,
    /// Pasando de página en el carrusel: hacia dónde (1 la siguiente, -1 la
    /// anterior) y desde cuándo. Sin esto, el relevo es un fundido.
    slide: Option<(f32, Instant)>,
    /// La herramienta con que se abrió (la del hover o la carátula): en un
    /// costado el bloque sale a su altura y ahí se queda mientras se recorre
    /// el carrusel (antes subía y bajaba con cada página).
    anchor: Option<usize>,
    /// La herramienta bajo el cursor y desde cuándo.
    hover: Option<(usize, Instant)>,
    leave_at: Option<Instant>,
    data: Data,
    /// Lo que se acaba de copiar (clave del elemento) y cuándo.
    copied: Option<(String, Instant)>,
    /// En Expressive, el snackbar de lo copiado.
    snack: Option<Snack>,
    /// El color bajo el cursor: la fila de abajo muestra su código.
    hovered_color: Option<u32>,
    /// Una captura apretada: dónde empezó. Soltar sin moverse la copia;
    /// moverse la arrastra.
    press: Option<(PathBuf, (f32, f32))>,
    /// En Expressive, los carruseles de Capturas y Flip, y el ítem apretado.
    shots_reel: Reel,
    pages_reel: Reel,
    reel_press: Option<ReelPress>,
    /// CPU y memoria de Sistema (0 a 1): entre lecturas se deslizan al
    /// valor nuevo en vez de saltar.
    cpu: Tween,
    ram: Tween,
    /// El alto del cuerpo de Reuniones: depende de la grabadora, que se lee
    /// en cada sondeo (`meetings::peek_height`).
    meetings_h: f32,
    /// Grabando la pantalla: el de Capturas muestra los controles.
    recording: bool,
    /// Se recogió deslizando sobre la carátula: no vuelve a bajar hasta que
    /// el cursor salga de ella.
    art_dismissed: bool,
    /// Lo que suena trae letra sincronizada (y se muestra).
    has_lyrics: bool,
    /// Hay una pista (sonando o en pausa) para el vistazo de «Ahora suena».
    has_track: bool,
    /// Lo último del celular (app, título y texto), para su vistazo.
    phone: Vec<PhoneGlance>,
    /// Hay algún celular vinculado: sin ninguno, el vistazo invita a vincular.
    phone_paired: bool,
    /// Las filas del portapapeles y el alto del de Agentes, que miden sus
    /// páginas: los lee la pill en cada sondeo.
    clip_rows: usize,
    usage_h: f32,
    /// Computadores (`computers::status`) y cuándo se leyó.
    kvm: crate::computers::Status,
    kvm_at: Option<Instant>,
    /// La pill va en M3 Expressive: los vistazos con héroe miden otra cosa
    /// (`content_height`). Se lee en cada sondeo; `false` en los tests.
    m3: bool,
}

/// El snackbar de M3 tras copiar algo en el vistazo (`SNACK_FOR`).
struct Snack {
    text: String,
    /// Lo que había en el portapapeles antes de copiar: «Deshacer» lo
    /// devuelve. `None` si no se puede devolver tal cual (una imagen, archivos,
    /// nada): sin acción.
    undo: Option<String>,
    /// Desde cuándo entra (copiar otra cosa con él a la vista no lo repite).
    at: Instant,
    /// Hasta cuándo se queda (copiar otra cosa lo alarga).
    until: Instant,
    /// Desde cuándo se va.
    gone_at: Option<Instant>,
}

/// El reloj de la entrada del vistazo: cada cosa aparece un poco después
/// de la anterior y los números cuentan desde cero, como en el de Agentes.
#[derive(Clone, Copy)]
struct Motion {
    /// Segundos desde que se mostró este contenido.
    t: f32,
    /// M3 Expressive: la entrada va con el resorte de efectos y respeta
    /// «reducir movimiento».
    m3: bool,
}

impl Motion {
    /// El elemento `i` de una lista: cae desde un poco más arriba y se
    /// funde, en cascada.
    fn enter(&self, i: usize) -> f32 {
        if self.m3 {
            // Escalonado corto con `EFFECTS` (sin sobrepaso: también es la
            // opacidad). Sale del reloj de la entrada (`shown_at`), así que al
            // deslizar el carrusel la página llega armada. Con movimiento
            // reducido, `spring_progress` da 1 al tiro.
            let delay = 0.06 + i as f32 * 0.035;
            return crate::anim::spring_progress(SpringKind::Effects, Duration::from_secs_f32((self.t - delay).max(0.0)));
        }
        ease_smooth_out(segment(self.t, 0.10 + i as f32 * 0.055, 0.42))
    }

    /// Los números y las barras: de cero a su valor.
    fn count(&self) -> f32 {
        if self.m3 && crate::anim::reduced_motion() {
            return 1.0;
        }
        ease_smooth_out(segment(self.t, 0.12, 0.75))
    }
}

/// El título letra por letra: cada una cae a su sitio tras la anterior.
pub(crate) fn falling(text: &'static str, t: f32) -> impl IntoElement {
    div().flex().children(text.chars().enumerate().map(move |(i, ch)| {
        let e = ease_smooth_out(segment(t, 0.06 + i as f32 * 0.016, 0.32));
        // Un espacio solo se colapsaría: el duro mantiene el ancho.
        let ch = if ch == ' ' { '\u{00a0}' } else { ch };
        div()
            .relative()
            .top(px(-6.0 * (1.0 - e)))
            .opacity(e)
            .child(ch.to_string())
    }))
}

impl ToolPeek {
    /// M3 Expressive: lo que baja y su alto van con el mismo resorte que las
    /// demás franjas que cuelgan (`Pill::restyle_motion`).
    pub(crate) fn restyle_motion(&mut self, expressive: bool, now: Instant) {
        self.amount.restyle(expressive, now);
        self.height.restyle(expressive, now);
    }

    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            amount: Anim::of(Tween::new(0.0, Duration::from_millis(crate::DRAWER_MS), ease_island), SpringKind::Spatial),
            height: Anim::of(Tween::new(0.0, Duration::from_millis(240), ease_island), SpringKind::Spatial),
            tool: None,
            shown_at: now,
            prev: None,
            slide: None,
            anchor: None,
            hover: None,
            leave_at: None,
            data: Data::default(),
            copied: None,
            snack: None,
            hovered_color: None,
            press: None,
            shots_reel: Reel::new(),
            pages_reel: Reel::new(),
            reel_press: None,
            cpu: Tween::new(0.0, Duration::from_millis(600), ease_smooth_out),
            ram: Tween::new(0.0, Duration::from_millis(600), ease_smooth_out),
            meetings_h: 0.0,
            recording: false,
            art_dismissed: false,
            has_lyrics: false,
            has_track: false,
            phone: Vec::new(),
            phone_paired: true,
            clip_rows: 0,
            usage_h: 0.0,
            kvm: Default::default(),
            kvm_at: None,
            m3: false,
        }
    }

    /// Pasa a mostrar `tool`. Con `dir` (y algo a la vista) las dos páginas
    /// se corren de lado; si no, la de antes se apaga encima.
    fn switch(&mut self, tool: usize, dir: f32, now: Instant) {
        self.prev = self.tool.filter(|_| self.amount.value(now) > 0.01).map(|old| (old, self.shown_at));
        self.slide = (dir != 0.0 && self.prev.is_some()).then_some((dir, now));
        self.shown_at = match self.slide {
            Some(_) => now.checked_sub(ENTERED).unwrap_or(now),
            None => now,
        };
        if let (Some(_), Some((old, at))) = (self.slide, self.prev) {
            self.prev = Some((old, at.min(now.checked_sub(ENTERED).unwrap_or(at))));
        }
        self.hovered_color = None;
        self.copied = None;
        self.dismiss_snack(now);
        self.tool = Some(tool);
    }

    /// El snackbar empieza a irse (si no se estaba yendo ya).
    fn dismiss_snack(&mut self, now: Instant) {
        if let Some(snack) = self.snack.as_mut() {
            snack.gone_at.get_or_insert(now);
        }
    }

    fn load(&mut self, tool: usize) {
        match tool {
            t if t == crate::color::TOOL => self.data.colors = recent_colors(),
            t if t == crate::capture::TOOL => {
                // Capturas y grabaciones juntas: las más nuevas (las que
                // recorre el carrusel de Expressive; las listas muestran 3).
                let modified = |path: &PathBuf| std::fs::metadata(path).and_then(|m| m.modified()).ok();
                let mut all: Vec<(Option<std::time::SystemTime>, PathBuf, bool)> = crate::flip_board::recent_captures()
                    .into_iter()
                    .take(REEL_SHOTS)
                    .map(|path| (modified(&path), path, false))
                    .chain(crate::screen_recording::recent_recordings(REEL_SHOTS).into_iter().map(|path| (modified(&path), path, true)))
                    .collect();
                all.sort_by(|a, b| b.0.cmp(&a.0));
                // Al bajar de nuevo, el carrusel parte por la más nueva.
                if self.amount.value(Instant::now()) <= 0.01 {
                    self.shots_reel.reset();
                }
                self.data.shots = all
                    .into_iter()
                    .take(REEL_SHOTS)
                    .map(|(_, path, video)| Shot {
                        label: shot_label(&path),
                        path,
                        video,
                    })
                    .collect()
            }
            t if t == crate::TEXTOS_TOOL => {
                self.data.texts = crate::snippets::texts().into_iter().take(3).collect();
                let tasks = crate::tasks::Tasks::load();
                self.data.tasks = tasks.ordered().into_iter().filter(|t| !t.done).take(3).cloned().collect();
                self.data.tasks_today = tasks.today();
            }
            t if t == crate::flip::TOOL => {
                if self.amount.value(Instant::now()) <= 0.01 {
                    self.pages_reel.reset();
                }
                self.data.pages = crate::flip_board::board_glance();
            }
            t if t == crate::board::TOOL => {
                self.data.board = crate::board::Prefs::load();
                self.data.last_shot = crate::flip_board::recent_captures()
                    .into_iter()
                    .next()
                    .map(|path| Shot { label: shot_label(&path), path, video: false });
            }
            FACE => {
                if let Ok(dirs) = atic_core::AppDirs::new() {
                    self.data.theme = atic_core::Config::load(&dirs.config_path()).ui_theme;
                }
            }
            t if t == crate::COMPUTADORES_TOOL => self.read_kvm(),
            _ => {}
        }
    }

    fn read_kvm(&mut self) {
        self.kvm = crate::computers::status();
        self.kvm_at = Some(Instant::now());
    }

    /// Los equipos que muestra el vistazo: el que usa este mouse y teclado
    /// primero, después los conectados; en un costado, solo uno.
    fn kvm_peers(&self, narrow: bool) -> Vec<crate::computers::PeerRow> {
        let s = &self.kvm;
        if !s.enabled || !s.running {
            return Vec::new();
        }
        let mut peers = s.peers.clone();
        peers.sort_by_key(|p| (!p.sharing, !p.connected));
        peers.truncate(if narrow { 1 } else { KVM_PEERS });
        peers
    }

    /// Lo que mide un equipo: arriba su nombre y el interruptor; el dibujo de
    /// los lados a la derecha o, en un costado, debajo.
    fn kvm_peer_h(narrow: bool) -> f32 {
        let (_, picker) = crate::computers_notch::side_picker_size(true);
        let rows = KVM_WHO_H + KVM_SHARE_H;
        if narrow { rows + picker + KVM_PEER_GAP * 2.0 } else { rows.max(picker) + KVM_PEER_GAP }
    }

    fn kvm_body_h(&self, narrow: bool) -> f32 {
        let s = &self.kvm;
        let mut h = KVM_STATUS_H;
        if s.pair_code.is_some() {
            h += KVM_CODE_H;
        }
        if s.notice.is_some() {
            h += KVM_NOTE_H;
        }
        if s.enabled && s.running && s.peers.is_empty() {
            h += EMPTY_H;
        }
        h + self.kvm_peers(narrow).len() as f32 * Self::kvm_peer_h(narrow)
    }

    /// El alto del contenido de cada vistazo, con su título y su pie. `narrow`:
    /// el bloque de un costado, con las filas en columna.
    fn content_height(&self, tool: usize, width: f32, narrow: bool) -> f32 {
        // Los de siempre, con sus medidas (y en Agentes, una franja abajo para
        // los puntos del carrusel, que no tapen sus tarjetas).
        if tool == crate::CLIPBOARD_TOOL {
            return crate::PEEK_PAD + self.clip_rows.max(1) as f32 * crate::PEEK_ROW + crate::PEEK_FOOTER + crate::PEEK_BOTTOM;
        }
        if tool == crate::AGENTES_TOOL {
            return self.usage_h + FOOTER_H;
        }
        let body = match tool {
            t if t == crate::color::TOOL => {
                if self.data.colors.is_empty() {
                    if self.m3 { EMPTY_M3_H } else { EMPTY_H }
                } else if narrow {
                    self.data.colors.len().min(SIDE_COLORS) as f32 * COLOR_ROW_H
                } else if self.m3 {
                    COLOR_HERO_H + COLOR_M3_GAP + SWATCH
                } else {
                    SWATCH + 10.0 + 22.0
                }
            }
            t if t == crate::capture::TOOL && self.recording => RECORDING_H,
            t if t == crate::capture::TOOL => {
                if self.data.shots.is_empty() {
                    if self.m3 { EMPTY_M3_H } else { EMPTY_H }
                } else if narrow {
                    self.data.shots.len().min(3) as f32 * SHOT_ROW_H
                } else if self.m3 {
                    SHOTS_M3_H
                } else {
                    shot_height(width) + SHOT_LABEL_H
                }
            }
            // Tareas: el contador y los botones a la izquierda, las pendientes
            // a la derecha (como siempre, aunque no haya ninguna).
            t if t == crate::TEXTOS_TOOL && self.m3 && narrow => TASKS_M3_H + 8.0 + TASKS_M3_H,
            t if t == crate::TEXTOS_TOOL && self.m3 => TASKS_M3_H,
            t if t == crate::TEXTOS_TOOL && narrow => TASKS_COUNTER_H + 8.0 + TASKS_BODY_H,
            t if t == crate::TEXTOS_TOOL => TASKS_BODY_H,
            t if t == crate::flip::TOOL && narrow => self.data.pages.len().clamp(1, MAX_PAGES) as f32 * PAGE_ROW_H,
            t if t == crate::flip::TOOL && self.m3 => PAGES_M3_H,
            t if t == crate::flip::TOOL => page_size(width).1 + PAGE_LABEL_H,
            t if t == crate::SISTEMA_TOOL && self.m3 => SYS_CARD_H + SYS_CARD_GAP + SECTION_H + APP_ROW_H * 3.0,
            t if t == crate::SISTEMA_TOOL => METER_H * 2.0 + 6.0 + SECTION_H + APP_ROW_H * 3.0,
            t if t == crate::POMODORO_TOOL && self.m3 => POMO_M3_H,
            t if t == crate::POMODORO_TOOL => POMO_MODES_H + POMO_GAP + POMO_TIME_H + POMO_BAR_H + POMO_GAP + POMO_BUTTONS_H,
            t if t == crate::COMPUTADORES_TOOL => self.kvm_body_h(narrow),
            t if t == crate::REUNIONES_TOOL => self.meetings_h,
            t if t == crate::CELULAR_TOOL && self.m3 => {
                if !self.phone_paired || self.phone.is_empty() {
                    EMPTY_M3_H
                } else {
                    let n = self.phone.len() as f32;
                    n * PHONE_M3_ROW_H + (n - 1.0) * LIST_M3_GAP
                }
            }
            t if t == crate::CELULAR_TOOL => {
                if self.phone.is_empty() {
                    EMPTY_H
                } else {
                    self.phone.len() as f32 * TEXT_ROW_H
                }
            }
            t if t == crate::board::TOOL => {
                let shot = if self.data.last_shot.is_some() { BOARD_GAP + SHOT_ROW_H } else { 0.0 };
                let rows = board_tool_rows(width) as f32;
                rows * BOARD_BTN + (rows - 1.0) * BOARD_BTN_GAP + BOARD_GAP + BOARD_COLOR_H + shot
            }
            FACE => {
                let rows = FACE_ACTIONS.len().div_ceil(face_per_row(narrow)) as f32;
                let tile = if self.m3 { QUICK_M3_TILE_H } else { QUICK_TILE_H };
                rows * tile + (rows - 1.0) * QUICK_GAP
            }
            t if t == crate::MEDIA_TOOL => {
                if self.has_track {
                    MEDIA_TITLE_H + MEDIA_GAP + MEDIA_CONTROLS_H + if self.has_lyrics { MEDIA_LYRICS_H } else { 0.0 }
                } else if self.m3 {
                    EMPTY_M3_H
                } else {
                    EMPTY_H
                }
            }
            _ => 0.0,
        };
        TITLE_H + body + FOOTER_H + BOTTOM
    }
}

fn shot_height(width: f32) -> f32 {
    let w = (width - SIDE * 2.0 - SHOT_GAP * 2.0) / 3.0;
    w * 10.0 / 16.0
}

/// El tamaño de la miniatura de una página (1200×900).
fn page_size(width: f32) -> (f32, f32) {
    let w = (width - SIDE * 2.0 - PAGE_GAP * (MAX_PAGES as f32 - 1.0)) / MAX_PAGES as f32;
    (w, w * 0.75)
}

/// La primera línea de una página de Flip, o qué tiene si no hay texto.
fn page_summary(page: &crate::flip_board::PageGlance) -> String {
    if page.summary.is_empty() {
        if !page.ink.is_empty() {
            t("pill.peeks.pageDrawing").to_string()
        } else if page.shapes.is_empty() {
            t("pill.peeks.pageEmpty").to_string()
        } else {
            t("pill.peeks.pageNoText").to_string()
        }
    } else {
        page.summary.clone()
    }
}

/// Una hoja de Flip en miniatura de `w`×`h`: sus bloques donde están y sus
/// trazos, dibujados hasta `drawn` (0 a 1).
fn page_marks(page: &crate::flip_board::PageGlance, (w, h): (f32, f32), text: Hsla, drawn: f32) -> Vec<AnyElement> {
    let shapes = page.shapes.iter().map(|&(x, y, bw, bh, kind)| {
        let fill: Hsla = match kind {
            1 => text.opacity(0.28),
            2 => Hsla::from(rgb(0x6fa3e0)).opacity(0.7),
            _ => text.opacity(0.5),
        };
        div()
            .absolute()
            .left(px(x * w))
            .top(px(y * h))
            .w(px((bw * w).max(3.0)))
            .h(px((bh * h).max(2.0)))
            .rounded(px(1.5))
            .bg(fill)
    });
    let ink: Vec<_> = page
        .ink
        .iter()
        .map(|(points, color)| {
            let n = ((points.len() as f32 * drawn).ceil() as usize).min(points.len());
            (points[..n].to_vec(), *color)
        })
        .filter(|(points, _)| points.len() >= 2)
        .collect();
    let strokes = gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            for (points, (r, g, b, a)) in &ink {
                let screen: Vec<gpui::Point<gpui::Pixels>> = points
                    .iter()
                    .map(|&(x, y)| {
                        gpui::point(
                            bounds.origin.x + bounds.size.width * x,
                            bounds.origin.y + bounds.size.height * y,
                        )
                    })
                    .collect();
                let options = lyon::tessellation::StrokeOptions::default()
                    .with_line_width(1.3)
                    .with_line_cap(lyon::tessellation::LineCap::Round)
                    .with_line_join(lyon::tessellation::LineJoin::Round);
                let mut builder = gpui::PathBuilder::stroke(px(1.3))
                    .with_style(gpui::PathStyle::Stroke(options));
                builder.add_polygon(&screen, false);
                if let Ok(path) = builder.build() {
                    let color = gpui::Rgba {
                        r: *r as f32 / 255.0,
                        g: *g as f32 / 255.0,
                        b: *b as f32 / 255.0,
                        a: *a as f32 / 255.0,
                    };
                    window.paint_path(path, color);
                }
            }
        },
    )
    .absolute()
    .size_full();
    let mut marks: Vec<AnyElement> = shapes.map(|shape| shape.into_any_element()).collect();
    marks.push(strokes.into_any_element());
    marks
}

/// Si el archivo es una grabación de pantalla.
fn is_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4") || e.eq_ignore_ascii_case("mov"))
}

/// «18:10» si es de hoy, «ayer 18:10», o «4 oct 18:10».
fn shot_label(path: &PathBuf) -> String {
    let Some(modified) = std::fs::metadata(path).and_then(|m| m.modified()).ok() else {
        return String::new();
    };
    let when: chrono::DateTime<chrono::Local> = modified.into();
    let today = chrono::Local::now().date_naive();
    let day = when.date_naive();
    if day == today {
        when.format("%H:%M").to_string()
    } else if today.pred_opt() == Some(day) {
        crate::i18n::tf("pill.clipboard.yesterdayAt", &[("time", &when.format("%H:%M"))])
    } else {
        use chrono::Datelike;
        let month = t("pill.peeks.monthsShort").split(',').nth(day.month0() as usize).unwrap_or_default();
        crate::i18n::tf("pill.peeks.shotDate", &[("day", &day.day()), ("month", &month), ("time", &when.format("%H:%M"))])
    }
}

/// Los colores tomados con Color y, después, los que pasaron por el
/// portapapeles (un `#hex` o un `rgb()` copiado): sin repetir, hasta 10.
fn recent_colors() -> Vec<u32> {
    let mut colors = crate::color::load_recents();
    let copied = crate::history::dir()
        .and_then(|dir| crate::history::load_if_changed(&dir, None))
        .map(|(_, entries)| entries)
        .unwrap_or_default();
    for entry in copied.iter().take(60) {
        if let crate::clipboard::Content::Color(_, color) = &entry.content {
            let c = color.to_rgb();
            let value = ((c.r * 255.0).round() as u32) << 16 | ((c.g * 255.0).round() as u32) << 8 | (c.b * 255.0).round() as u32;
            if !colors.contains(&value) {
                colors.push(value);
            }
        }
    }
    colors.truncate(10);
    colors
}

fn hex(color: u32) -> String {
    crate::color::format(color, crate::color::Format::Hex).to_uppercase()
}

impl Pill {
    /// Hay otro vistazo abierto (o bajando) que no es `kind`.
    pub(crate) fn other_peek_open(&self, kind: PeekKind) -> bool {
        (kind != PeekKind::Clipboard && self.peek.target() == 1.0)
            || (kind != PeekKind::Usage && self.usage_peek.target() == 1.0)
            || (kind != PeekKind::Tool && self.tool_peek.amount.target() == 1.0)
    }

    /// Baja el vistazo `kind`: los otros se recogen a la vez (relevo).
    pub(crate) fn hand_off_peeks(&mut self, kind: PeekKind, now: Instant) {
        if kind != PeekKind::Clipboard && self.peek.target() == 1.0 {
            self.peek.set(0.0, now);
            self.peek_leave_at = None;
        }
        if kind != PeekKind::Usage && self.usage_peek.target() == 1.0 {
            self.close_usage_peek(now);
        }
        if kind != PeekKind::Tool && self.tool_peek.amount.target() == 1.0 {
            self.tool_peek.amount.set(0.0, now);
            self.tool_peek.leave_at = None;
            self.tool_peek.copied = None;
            self.tool_peek.dismiss_snack(now);
        }
    }

    /// Cada sondeo: baja, cambia o recoge el vistazo según dónde esté el
    /// cursor. `on_face`: sobre la cara, que baja los ajustes rápidos.
    pub(crate) fn update_tool_peek(
        &mut self,
        now: Instant,
        cursor: Option<(f32, f32)>,
        strip_hovered: Option<usize>,
        on_face: bool,
        on_art: bool,
        docked: bool,
        cx: &mut Context<Self>,
    ) {
        // Con algo sonando, la carátula del tab en reposo también baja el de
        // Ahora suena, sin abrir la tira.
        if !on_art {
            self.tool_peek.art_dismissed = false;
        }
        // Con la pill como tab, «sobre la carátula» es sobre todo el tab, y el
        // vistazo que cuelga es parte de él: sobre el vistazo no cuenta, o
        // pasar de página en el carrusel lo devolvía a Ahora suena.
        let over_peek = cursor.is_some_and(|c| self.tool_peek_over(c, now));
        let on_art = on_art && !self.tool_peek.art_dismissed && !over_peek;
        let from_art = on_art
            || (self.tool_peek.tool == Some(crate::MEDIA_TOOL)
                && self.strip.target() != 1.0
                && self.live.target() == 1.0);
        // Transformándose en el panel de Ahora suena, se queda hasta que el
        // panel termina de abrirse.
        let morphing = self.media_peek_morph().is_some();
        // Dictando, el vistazo se cierra y no se abre (como el de uso).
        // Ya abierto y con el cursor encima, cualquier página del carrusel
        // vale aunque la tira esté cerrada (se abrió desde la carátula).
        let on_open_peek = self.tool_peek.amount.target() == 1.0 && cursor.is_some_and(|c| self.tool_peek_over(c, now));
        let usable = morphing
            || (docked
                && self.docked_still()
                && !self.panel_visible()
                && !self.dictation_shown()
                && (self.strip.target() == 1.0 || from_art || on_open_peek));
        let on_tool = strip_hovered
            .filter(|t| has_peek(*t))
            .or(on_face.then_some(FACE))
            .or(on_art.then_some(crate::MEDIA_TOOL))
            .filter(|_| usable);
        let open = self.tool_peek.amount.target() == 1.0;
        let relay = open || self.other_peek_open(PeekKind::Tool);

        // La herramienta bajo el cursor: los datos se piden apenas llega,
        // para que al bajar ya estén.
        match (on_tool, self.tool_peek.hover) {
            (Some(tool), Some((was, _))) if tool == was => {}
            (Some(tool), _) => {
                self.tool_peek.hover = Some((tool, now));
                self.tool_peek.load(tool);
                if tool == crate::SISTEMA_TOOL {
                    self.system.update(cx, |panel, cx| panel.set_peek(true, false, cx));
                }
                // Atic puede haber grabado o resumido algo mientras tanto.
                if tool == crate::REUNIONES_TOOL {
                    self.studio.update(cx, |studio, _| studio.refresh_recent());
                }
                if tool == crate::AGENTES_TOOL {
                    self.usage.refresh();
                }
            }
            (None, _) => self.tool_peek.hover = None,
        }

        // Mientras se despliega con el scroll, manda el gesto.
        if let Some((tool, since)) = self.tool_peek.hover.filter(|_| !self.scroll_scrub) {
            let wait = match (relay, on_art) {
                (true, _) => SWITCH_DELAY,
                (_, true) => ART_DELAY,
                _ => SHOW_DELAY,
            };
            if self.tool_peek.tool != Some(tool) || !open {
                if now.duration_since(since) >= wait {
                    // Recién abriendo: se leen todas las páginas del
                    // carrusel, que miden el alto común.
                    if !open {
                        for page in self.peek_pages() {
                            self.tool_peek.load(page);
                        }
                        if self.peek_pages().contains(&crate::AGENTES_TOOL) {
                            self.usage.refresh();
                        }
                    }
                    // Esas lecturas (disco frío la primera vez) pueden tardar cientos de ms. La
                    // animación cuenta desde ahora, no desde el cuadro que empezó antes: si no,
                    // el primer vistazo ya terminó de abrirse cuando se dibuja y aparece de golpe.
                    let now = Instant::now();
                    let fresh = self.tool_peek.amount.value(now) <= 0.01;
                    // Al bajar de nuevo, Color parte por el más reciente (en
                    // Expressive la tarjeta recuerda la última muestra pasada).
                    if fresh {
                        self.tool_peek.hovered_color = None;
                    }
                    if self.tool_peek.tool != Some(tool) {
                        // De una herramienta a la vecina en la tira: se corre
                        // hacia su lado, como en el carrusel.
                        let dir = self.peek_direction(self.tool_peek.tool, tool);
                        self.tool_peek.switch(tool, dir, now);
                        if tool == crate::AGENTES_TOOL {
                            self.usage.mark_opened();
                        }
                    } else if fresh && tool == crate::AGENTES_TOOL {
                        // Los anillos se llenan cada vez que baja de nuevo.
                        self.usage.mark_opened();
                    }
                    self.tool_peek.amount.set(1.0, now);
                    self.tool_peek.anchor = Some(tool);
                    self.hand_off_peeks(PeekKind::Tool, now);
                }
            }
        }

        // Una captura apretada: arrastre si el cursor se movió, clic (copia)
        // si se soltó sin moverse.
        if let Some((path, origin)) = self.tool_peek.press.clone() {
            if cursor.is_some_and(|c| (c.0 - origin.0).hypot(c.1 - origin.1) >= DRAG_START) {
                self.tool_peek.press = None;
                self.close_tool_peek(now, cx);
                self.drag_shot(path, cx);
            } else if !crate::win::left_button_down() {
                self.tool_peek.press = None;
                if is_video_path(&path) {
                    self.close_tool_peek(now, cx);
                    crate::screen_recording::open_recording(path, cx);
                } else {
                    let key = path.to_string_lossy().to_string();
                    self.copy_shot(path, key, cx);
                }
            }
        }
        // Los carruseles de Capturas y Flip (Expressive).
        self.update_reels(now, cursor, cx);

        // En Expressive, lo copiado se va con su snackbar.
        let snack_done = self.advance_snack(now, cx);

        // Quedarse o irse.
        if self.tool_peek.amount.target() == 1.0 {
            let on_peek = cursor.is_some_and(|c| self.tool_peek_over(c, now));
            // Sobre Clipboard o Agentes se espera el relevo: este se
            // recoge cuando aquel baja (`hand_off_peeks`).
            let relay_to = strip_hovered.is_some_and(|t| !has_peek(t) && any_peek(t));
            let copied_done = snack_done
                || (!self.tool_peek.m3
                    && self
                        .tool_peek
                        .copied
                        .as_ref()
                        .is_some_and(|(_, at)| now.duration_since(*at) >= COPIED_FOR));
            if !usable || copied_done {
                self.close_tool_peek(now, cx);
            } else if morphing || on_tool.is_some() || on_peek || relay_to || self.tool_peek.press.is_some() || self.tool_peek.reel_press.is_some() {
                self.tool_peek.leave_at = None;
            } else {
                let since = *self.tool_peek.leave_at.get_or_insert(now);
                if now.duration_since(since) >= GRACE {
                    self.close_tool_peek(now, cx);
                }
            }
        }

        if self.tool_peek.tool == Some(crate::SISTEMA_TOOL) {
            if let Some(procs) = self.system.read(cx).snapshot().procs.as_ref() {
                let ram = if procs.ram_total > 0 {
                    procs.ram_used as f32 / procs.ram_total as f32
                } else {
                    0.0
                };
                self.tool_peek.cpu.set((procs.cpu / 100.0).clamp(0.0, 1.0), now);
                self.tool_peek.ram.set(ram.clamp(0.0, 1.0), now);
            }
        }

        self.tool_peek.m3 = crate::appearance::current().pill == crate::appearance::PillStyle::Expressive;
        self.tool_peek.meetings_h = if self.tool_peek.m3 {
            crate::meetings::peek_height_m3(self.studio.read(cx))
        } else {
            crate::meetings::peek_height(self.studio.read(cx))
        };
        self.tool_peek.clip_rows = self.peek_entries.len();
        self.tool_peek.usage_h = self.usage_page_height();
        self.tool_peek.recording = self.screen_rec.active();
        self.tool_peek.has_track = self.media.track().is_some();
        self.tool_peek.has_lyrics = self.hang_verse().is_some();
        if self.tool_peek.tool == Some(crate::CELULAR_TOOL) {
            self.tool_peek.phone_paired = self.agents.read(cx).phone_paired();
            self.tool_peek.phone = self
                .agents
                .read(cx)
                .inbox()
                .ordered()
                .into_iter()
                .filter_map(|item| {
                    let phone = item.phone?;
                    Some(PhoneGlance { app: phone.app, icon: phone.icon, title: item.title, detail: item.detail })
                })
                .take(3)
                .collect();
        }
        // Computadores, si se ve o es página del carrusel abierto: el estado
        // cambia solo (el cursor pasa al otro, se conecta, llega un código).
        let kvm_page = self.tool_peek.tool == Some(crate::COMPUTADORES_TOOL)
            || (self.tool_peek.amount.target() == 1.0 && self.peek_pages().contains(&crate::COMPUTADORES_TOOL));
        if kvm_page && self.tool_peek.kvm_at.is_none_or(|at| now.duration_since(at) >= KVM_REFRESH) {
            self.tool_peek.read_kvm();
        }
        let narrow = self.side_drawers();
        let width = if narrow { crate::SIDE_W } else { self.tool_peek_width() };
        if let Some(tool) = self.tool_peek.tool {
            let pages = self.peek_pages();
            let open = self.tool_peek.amount.target() == 1.0 && self.tool_peek.amount.value(now) > 0.01;
            let h = if pages.contains(&tool) {
                let tallest = pages.iter().map(|&page| self.tool_peek.content_height(page, width, narrow)).fold(0.0, f32::max);
                // Abierto, el carrusel no se achica: los controles quedan
                // bajo el cursor aunque cambie lo de una página.
                if open { tallest.max(self.tool_peek.height.target()) } else { tallest }
            } else {
                self.tool_peek.content_height(tool, width, narrow)
            };
            // Si venía cerrado nace con su alto: partir del de la herramienta
            // anterior hacía que la franja subiera, bajara y volviera a subir.
            if self.tool_peek.height.target() == 0.0 || self.tool_peek.amount.value(now) <= 0.01 {
                self.tool_peek.height.snap(h);
            } else {
                self.tool_peek.height.set(h, now);
            }
        }
        // El vistazo se fue del todo: Sistema deja de medir procesos.
        if self.tool_peek.amount.value(now) <= 0.001 && self.tool_peek.amount.target() == 0.0 {
            if self.tool_peek.tool.take() == Some(crate::SISTEMA_TOOL) || self.tool_peek.hover.is_none() {
                let keep = self.panel_visible() && self.notch_tool == crate::NotchTool::Sistema;
                self.system.update(cx, |panel, cx| panel.set_peek(false, keep, cx));
            }
        }
    }

    /// La herramienta del vistazo abierto (o bajando), si hay uno: el bloque
    /// de un costado sale a su altura.
    /// Lo que ubica el bloque de un costado: la herramienta con que se abrió.
    pub(crate) fn tool_peek_anchor(&self) -> Option<usize> {
        (self.tool_peek.amount.target() == 1.0).then_some(self.tool_peek.anchor).flatten()
    }

    pub(crate) fn tool_peek_owner(&self) -> Option<usize> {
        (self.tool_peek.amount.target() == 1.0).then_some(self.tool_peek.tool).flatten()
    }

    /// Deslizar sobre la carátula: el vistazo de Ahora suena baja lo que se
    /// lleva deslizado (0 a 1), sin animar, siguiendo a los dedos.
    pub(crate) fn media_peek_follow(&mut self, amount: f32, now: Instant) {
        if self.tool_peek.tool != Some(crate::MEDIA_TOOL) || self.tool_peek.amount.target() == 0.0 {
            self.tool_peek.load(crate::MEDIA_TOOL);
            // Si todavía se ve (abrir el notch a mitad del scroll lo manda a
            // cerrar y el gesto lo retoma), su entrada no vuelve a empezar:
            // los controles y la letra se apagaban y volvían, un parpadeo.
            let visible = self.tool_peek.tool == Some(crate::MEDIA_TOOL) && self.tool_peek.amount.value(now) > 0.01;
            if !visible {
                self.tool_peek.shown_at = now;
            }
            self.tool_peek.copied = None;
            self.tool_peek.dismiss_snack(now);
            self.hand_off_peeks(PeekKind::Tool, now);
        }
        self.tool_peek.tool = Some(crate::MEDIA_TOOL);
        self.tool_peek.anchor = Some(crate::MEDIA_TOOL);
        self.tool_peek.art_dismissed = false;
        self.tool_peek.hover = Some((crate::MEDIA_TOOL, now));
        self.tool_peek.leave_at = None;
        self.tool_peek.amount.snap(amount.clamp(0.0, 1.0));
    }

    /// Con la letra colgando, el vistazo de Ahora suena no dibuja la suya: la
    /// que cuelga es la misma y viaja entre su lugar bajo la franja y el de
    /// los versos del vistazo (bajo los controles) a medida que baja o sube,
    /// así no hay dos letras ni una que se corta. Devuelve dónde va su borde
    /// de arriba. No al abrir el panel con el scroll (ahí la lleva el vistazo,
    /// `media_handoff`) ni en un costado.
    pub(crate) fn hang_rides_media_peek(&self, now: Instant) -> Option<f32> {
        let media = self.tool_peek_media_amount(now);
        if media <= 0.0 || !self.tool_peek.has_lyrics || self.panel_visible() || self.side_drawers() {
            return None;
        }
        let area = self.drawers_area(now)?;
        let rest = area.y + self.drawer_top(crate::usage::Drawer::Lyrics, now);
        // En el vistazo, bajo el verso quedan su franja, el pie y el margen.
        let below = MEDIA_LYRICS_H - 6.0 + FOOTER_H + BOTTOM;
        let there = area.y + self.drawers_stack(now) - below;
        Some((rest + (there - rest) * media).max(rest))
    }

    /// El vistazo bajando, abierto o siguiendo al scroll es el de Ahora suena.
    pub(crate) fn media_peek_shown(&self) -> bool {
        self.tool_peek.tool == Some(crate::MEDIA_TOOL)
    }

    /// Hay un vistazo bajando, abierto o siguiendo al scroll.
    pub(crate) fn tool_peek_down(&self) -> bool {
        self.tool_peek.amount.target() > 0.0
    }

    /// Al soltar el gesto: el vistazo termina de bajar o se recoge (y no
    /// vuelve a bajar solo hasta que el cursor salga de la carátula).
    pub(crate) fn media_peek_settle(&mut self, open: bool, now: Instant) {
        self.tool_peek.amount.set(if open { 1.0 } else { 0.0 }, now);
        if !open {
            self.tool_peek.art_dismissed = true;
            self.tool_peek.leave_at = None;
        }
    }

    /// Las páginas del carrusel de vistazos: las herramientas de la tira que
    /// tienen vistazo, a la vista y en «Más», en el orden de Personalizar.
    pub(crate) fn peek_pages(&self) -> Vec<usize> {
        self.layout.ring.iter().chain(&self.layout.more).copied().filter(|&tool| has_peek(tool)).collect()
    }

    /// Hacia qué lado queda `to` respecto de `from` en el carrusel (0 si
    /// alguna no está).
    fn peek_direction(&self, from: Option<usize>, to: usize) -> f32 {
        let pages = self.peek_pages();
        let at = |tool: usize| pages.iter().position(|&page| page == tool);
        match (from.and_then(at), at(to)) {
            (Some(a), Some(b)) if a != b => if b > a { 1.0 } else { -1.0 },
            _ => 0.0,
        }
    }

    /// Lleva el vistazo a `tool`, corriéndose hacia `dir`.
    fn peek_go(&mut self, tool: usize, dir: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.tool_peek.tool == Some(tool) || self.tool_peek.amount.target() != 1.0 {
            return;
        }
        self.tool_peek.load(tool);
        if tool == crate::SISTEMA_TOOL {
            self.system.update(cx, |panel, cx| panel.set_peek(true, false, cx));
        }
        if tool == crate::REUNIONES_TOOL {
            self.studio.update(cx, |studio, _| studio.refresh_recent());
        }
        if tool == crate::AGENTES_TOOL {
            self.usage.refresh();
            self.usage.mark_opened();
        }
        self.tool_peek.switch(tool, dir, now);
        self.tool_peek.leave_at = None;
        cx.notify();
    }

    /// La página de al lado (`dir` 1 o -1), dando la vuelta al final.
    fn peek_step(&mut self, dir: i32, cx: &mut Context<Self>) {
        let pages = self.peek_pages();
        let Some(i) = self.tool_peek.tool.and_then(|tool| pages.iter().position(|&page| page == tool)) else {
            return;
        };
        if pages.len() < 2 {
            return;
        }
        let next = (i as i32 + dir).rem_euclid(pages.len() as i32) as usize;
        self.peek_go(pages[next], dir as f32, cx);
    }

    /// Dos dedos de lado sobre el vistazo: pasa de página, una por gesto
    /// (la inercia que sigue no pasa otra). `true` si el gesto era para el
    /// carrusel: así no despliega Ahora suena. La rueda del mouse no cuenta.
    pub(crate) fn peek_swipe(&mut self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>) -> bool {
        let gpui::ScrollDelta::Pixels(delta) = event.delta else {
            return false;
        };
        if matches!(event.touch_phase, gpui::TouchPhase::Started) {
            self.swipe = crate::Swipe::default();
        }
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        let now = Instant::now();
        let p = (f32::from(event.position.x), f32::from(event.position.y));
        let on_carousel = self.peek_pages().len() > 1
            && self.tool_peek.tool.is_some_and(|tool| self.peek_pages().contains(&tool))
            && self.tool_peek_over(p, now);
        if !on_carousel {
            return false;
        }
        // El eje se decide una vez por gesto, tras unos píxeles: un gesto
        // vertical que empieza con un temblor de lado es para cerrar o abrir
        // el vistazo, no para el carrusel (antes se quedaba con sus primeros
        // eventos y el scroll no respondía).
        if self.swipe.axis.is_none() {
            self.swipe.travel.0 += dx.abs();
            self.swipe.travel.1 += dy.abs();
            let (h, v) = self.swipe.travel;
            if h + v >= AXIS_LOCK {
                self.swipe.axis = Some(h > v * 1.5);
            }
        }
        if self.swipe.axis != Some(true) {
            return false;
        }
        if !self.swipe.fired {
            self.swipe.sum += dx;
            if self.swipe.sum.abs() >= SWIPE_STEP {
                self.swipe.fired = true;
                // Los dedos hacia la izquierda traen la de la derecha, como
                // pasar una página.
                self.peek_step(if self.swipe.sum < 0.0 { 1 } else { -1 }, cx);
            }
        }
        true
    }

    /// Los puntos (uno por página, la actual más larga) y las flechas, a la
    /// derecha del pie. Quedan fijos: no se corren con la página.
    /// En el bloque angosto de un costado va sin flechas y más apretado (se
    /// pasa con los puntos o deslizando): junto al link del pie no cabía.
    fn peek_nav(&self, tool: usize, narrow: bool, c: &Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pages = self.peek_pages();
        let current = pages.iter().position(|&page| page == tool)?;
        if pages.len() < 2 {
            return None;
        }
        let (text, muted, m3) = (c.text, c.muted, c.m3.is_some());
        // En Expressive el blanco del clic es de 40 y lo dibujado sigue igual:
        // el margen negativo deja el pie como estaba.
        let inset = (HIT_M3 - NAV_ARROW) / 2.0;
        let arrow = |id: &'static str, dir: i32| {
            div()
                .id(id)
                .map(|el| if m3 { el.size(px(HIT_M3)).m(px(-inset)) } else { el.size(px(NAV_ARROW)).rounded_full() })
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.peek_step(dir, cx);
                    }),
                )
                .fx(id, move |el, h| {
                    let fg = h.mix(muted, text);
                    // En Expressive, la capa de estado de M3 sobre el relleno.
                    let bg = if m3 { m3_layer(text.opacity(0.07), text, h) } else { text.opacity(0.07 + 0.09 * h.t) };
                    let glyph = svg()
                        .path(crate::icons::themed("icons/chevron-right.svg"))
                        .size(px(14.))
                        .text_color(fg)
                        .when(dir < 0, |el| {
                            el.with_transformation(gpui::Transformation::rotate(gpui::radians(std::f32::consts::PI)))
                        });
                    if m3 {
                        el.child(
                            div()
                                .size(px(NAV_ARROW))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(bg)
                                .child(glyph),
                        )
                    } else {
                        el.bg(bg).child(glyph)
                    }
                })
        };
        let arrows = (!narrow).then(|| (arrow("peek-prev", -1), arrow("peek-next", 1)));
        // En Expressive (arriba) cada punto es un blanco de 40 de alto y, sin
        // hueco entre ellos, toma el `gap` como relleno: quedan donde estaban.
        let dots_m3 = m3 && !narrow;
        let (dot, pad) = if narrow {
            (NAV_DOT - 1.0, 2.0)
        } else if dots_m3 {
            (NAV_DOT, 5.0)
        } else {
            (NAV_DOT, 3.0)
        };
        let dots: Vec<_> = pages
            .iter()
            .enumerate()
            .map(|(i, &page)| {
                let on = i == current;
                div()
                    .id(("peek-dot", i))
                    // Un blanco más grande que el punto, para atinarle (en un
                    // costado, en columna).
                    .map(|el| {
                        if narrow {
                            el.w(px(12.)).py(px(pad)).justify_center()
                        } else if dots_m3 {
                            el.h(px(HIT_M3)).my(px(-inset)).px(px(pad)).items_center()
                        } else {
                            el.h(px(NAV_ARROW)).px(px(pad)).items_center()
                        }
                    })
                    .flex()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let dir = if i > current { 1.0 } else { -1.0 };
                            pill.peek_go(page, dir, cx);
                        }),
                    )
                    .fx(("peek-dot", i), move |el, h| {
                        el.child(
                            div()
                                .map(|el| {
                                    let long = if on { dot * 2.8 } else { dot };
                                    if narrow { el.w(px(dot)).h(px(long)) } else { el.h(px(dot)).w(px(long)) }
                                })
                                .rounded_full()
                                .bg(text.opacity(if on { 0.9 } else { 0.28 + 0.3 * h.t })),
                        )
                    })
            })
            .collect();
        let (prev, next) = arrows.unzip();
        Some(
            div()
                .flex()
                .when(narrow, |el| el.flex_col())
                .items_center()
                .gap(px(if narrow { 0.0 } else { 4.0 }))
                .children(prev)
                .map(|el| {
                    if dots_m3 {
                        el.child(div().mx(px(-2.)).flex().items_center().children(dots))
                    } else {
                        el.children(dots)
                    }
                })
                .children(next)
                .into_any_element(),
        )
    }

    /// Dónde está la página `page` del carrusel si está quieta y a la vista:
    /// el Portapapeles y Agentes atienden sus clics por posición, como cuando
    /// tenían su propio vistazo. Mientras se desliza, ninguna.
    pub(crate) fn hosted_rect(&self, page: usize, now: Instant) -> Option<Rect> {
        if self.panel_visible() || self.tool_peek.tool != Some(page) || self.tool_peek.amount.value(now) < 0.85 {
            return None;
        }
        let sliding = self
            .tool_peek
            .slide
            .is_some_and(|(_, at)| now.duration_since(at).as_secs_f32() < SLIDE_S);
        if sliding {
            return None;
        }
        let rect = self.tool_peek_rect(now)?;
        Some(Rect::new(rect.x, rect.y, rect.w, self.tool_peek_height(now)))
    }

    /// La página `page` se ve (o se está yendo): su contenido no se reinicia.
    pub(crate) fn peek_showing(&self, page: usize, now: Instant) -> bool {
        self.tool_peek.tool == Some(page) && self.tool_peek.amount.value(now) > 0.01
    }

    /// Se pegó o se arrastró algo del Portapapeles: su página se va.
    pub(crate) fn close_clipboard_page(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self.tool_peek.tool == Some(crate::CLIPBOARD_TOOL) {
            self.close_tool_peek(now, cx);
        }
    }

    fn close_tool_peek(&mut self, now: Instant, cx: &mut Context<Self>) {
        self.tool_peek.amount.set(0.0, now);
        self.tool_peek.leave_at = None;
        self.tool_peek.copied = None;
        self.tool_peek.dismiss_snack(now);
        let _ = cx;
    }

    /// Cuánto bajó (0 a 1): bajo la tira arriba, al lado de la columna en un
    /// costado.
    pub(crate) fn tool_peek_amount(&self, now: Instant) -> f32 {
        self.tool_peek.amount.value(now).clamp(0.0, 1.2)
    }

    /// Cuánto bajó el vistazo de Ahora suena (0 si es otro): tiñe la forma
    /// con la carátula difuminada.
    pub(crate) fn tool_peek_media_amount(&self, now: Instant) -> f32 {
        match self.tool_peek.tool {
            Some(t) if t == crate::MEDIA_TOOL => self.tool_peek.amount.value(now).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    pub(crate) fn tool_peek_height(&self, now: Instant) -> f32 {
        self.tool_peek.height.value(now)
    }

    /// El ancho del tab con el vistazo (`tool_peek_width`).
    pub(crate) fn tool_peek_length(length: f32, amount: f32, width: f32) -> f32 {
        length + (width.max(length) - length) * amount.clamp(0.0, 1.0)
    }

    /// El ancho del vistazo: el de un panel, igual en todas las herramientas
    /// (en Ahora suena, además, el mismo que crece al abrirlo con el scroll).
    pub(crate) fn tool_peek_width(&self) -> f32 {
        crate::PANEL_W
    }

    /// El panel se abre (o se cierra) desde su vistazo: con el scroll en
    /// Ahora suena, o con un clic en «Abrir …». El vistazo sigue ahí mientras
    /// el notch crece y se funde en el panel. Devuelve (cuánto se ve todavía,
    /// cuánto creció el notch).
    pub(crate) fn media_peek_morph(&self) -> Option<(f32, f32)> {
        let morphing = self.panel_visible()
            && self.panel_scrolled
            && self.tool_peek.tool.and_then(peek_panel) == Some(self.notch_tool)
            && self.tool_peek.amount.target() == 1.0;
        morphing.then(|| (1.0 - self.panel_content_amount(), self.notch_morph().clamp(0.0, 1.0)))
    }

    /// En esa transformación, lo que comparten el vistazo y el panel (título,
    /// controles y letra) se dibuja una sola vez: lo lleva el vistazo hasta
    /// que el notch llega a su tamaño, donde coincide con el del panel, y ahí
    /// pasa al panel. Devuelve (ya pasaron el título y los controles, cuánto
    /// pasó la letra): la del panel es otra (con los versos vecinos), así que
    /// esa se cruza en el último tramo, cuando ya casi están en el mismo lugar.
    /// Solo en Ahora suena: el vistazo de las demás se funde entero.
    pub(crate) fn media_handoff(&self) -> Option<(bool, f32)> {
        self.media_peek_morph()?;
        if self.notch_tool != crate::NotchTool::Media {
            return None;
        }
        let landed = self.panel_time >= crate::NOTCH_MORPH_MS;
        let lyric = segment(self.panel_time, crate::NOTCH_MORPH_MS * 0.6, crate::NOTCH_MORPH_MS * 0.4);
        Some((landed, lyric))
    }

    pub(crate) fn tool_peek_animating(&self, now: Instant) -> bool {
        // Deslizándose de página: la entrada de la nueva cuenta como hecha, así
        // que sin esto no se pedían cuadros y quedaba detenido a medio camino
        // (con la tira cerrada no hay otra animación que los pida).
        let sliding = self
            .tool_peek
            .slide
            .is_some_and(|(_, at)| now.duration_since(at).as_secs_f32() < SLIDE_S + 0.05);
        sliding
            || self.tool_peek.amount.is_running(now)
            || self.tool_peek.height.is_running(now)
            || (self.tool_peek.amount.value(now) > 0.01
                && now.duration_since(self.tool_peek.shown_at) < Duration::from_millis(1400))
            || self.tool_peek.cpu.is_running(now)
            || self.tool_peek.ram.is_running(now)
            || self.snack_animating(now)
            || self.tool_peek.shots_reel.busy(now)
            || self.tool_peek.pages_reel.busy(now)
    }

    pub(crate) fn tool_peek_rect(&self, now: Instant) -> Option<Rect> {
        if self.tool_peek.amount.value(now) <= 0.01 {
            return None;
        }
        self.drawer_rect(Drawer::Tool, now)
    }

    pub(crate) fn tool_peek_over(&self, p: (f32, f32), now: Instant) -> bool {
        self.tool_peek_rect(now).is_some_and(|r| r.contains(p, 4.0))
    }

    /// El vistazo dibujado bajo la tira.
    pub(crate) fn tool_peek_element(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Con el notch abierto (o abriéndose) manda el panel: el vistazo que
        // se recoge no queda encima (el de Ahora suena se veía doble). Salvo
        // el de Ahora suena abriendo el panel con el scroll: ese se corre
        // hasta donde quedan las mismas cosas en el panel y se funde en él.
        let morph = self.media_peek_morph();
        if self.panel_visible() && morph.is_none() {
            return None;
        }
        let mut area = self.tool_peek_rect(now)?;
        let tool = self.tool_peek.tool?;
        let (fade, slide) = match morph {
            // Ya pasado al panel no se dibuja: transparente igual se
            // quedaba con los clics de la barra y los botones del panel.
            Some(_) if self.media_handoff().is_some_and(|(landed, _)| landed) => return None,
            Some((fade, _)) if fade <= 0.02 && self.media_handoff().is_none() => return None,
            Some((fade, grow)) => {
                // Lo que se corre no se recorta en el vistazo sino en la
                // forma que crece, como el panel.
                if let crate::PillShape::Tab { rect, .. } = self.shape(now) {
                    area.h = area.h.max(rect.y + rect.h - area.y);
                }
                let panel_top = self.panel_shape(now).map_or(0.0, |shape| shape.rect.y) - area.y;
                (fade, Some((grow, panel_top)))
            }
            None => (1.0, None),
        };
        let amount = self.tool_peek.amount.value(now);
        // Ahora suena va sobre la carátula: texto claro en cualquier tema.
        let on_cover = tool == crate::MEDIA_TOOL && self.media.track().is_some_and(|t| t.backdrop.is_some());
        let c = if on_cover { Colors::cover() } else { Colors::current() };
        // El contenido se arma con el ancho al que llega (como su alto,
        // `content_height`), no con el del tab que todavía se ensancha: las
        // miniaturas de Capturas y Flip crecían cuadro a cuadro.
        let width = if self.side_drawers() { area.w } else { area.w.max(self.tool_peek_width()) };
        // En el bloque angosto de un costado: filas en columna y sin la
        // ayuda junto al título (no cabe).
        let narrow = self.side_drawers();
        // El contenido entra cuando la franja ya casi bajó, y al cambiar de
        // herramienta el nuevo se funde (la franja cambia de alto a la vez)
        // mientras el anterior se apaga encima.
        let reveal = segment(amount, 0.55, 0.45);
        let since = now.duration_since(self.tool_peek.shown_at).as_secs_f32();
        // Todas las páginas del carrusel miden lo mismo: el pie queda abajo.
        let column_h = self.tool_peek_height(now) - BOTTOM;
        let origin = (area.x, area.y);
        let column = self.peek_column(tool, self.tool_peek.shown_at, width, column_h, origin, narrow, &c, now, fade, slide, cx)?;
        // En el carrusel, la que se va y la que llega se corren juntas con el
        // mismo progreso; si no, la de antes se apaga encima.
        let carousel = self
            .tool_peek
            .slide
            .filter(|_| morph.is_none())
            .map(|(dir, at)| (dir, crate::ease_page(segment(now.duration_since(at).as_secs_f32(), 0.0, SLIDE_S))))
            .filter(|(_, p)| *p < 1.0);
        // En un costado las páginas se apilan: el carrusel corre en vertical.
        let span = if narrow { column_h } else { area.w };
        let (shift, switched) = match carousel {
            Some((dir, p)) => (dir * span * (1.0 - p), 1.0),
            None => (0.0, segment(since, 0.06, 0.18)),
        };
        let leaving = self
            .tool_peek
            .prev
            .filter(|(old, _)| *old != tool && (carousel.is_some() || since < 0.12) && morph.is_none())
            .and_then(|(old, at)| self.peek_column(old, at, width, column_h, origin, narrow, &c, now, fade, None, cx))
            .map(|column| {
                let leaving = div().id("tool-peek-leaving").absolute().w_full();
                match carousel {
                    Some((dir, p)) if narrow => leaving.left_0().top(px(-dir * span * p)).child(column),
                    Some((dir, p)) => leaving.top_0().left(px(-dir * span * p)).child(column),
                    None => leaving
                        .top_0()
                        .left_0()
                        .opacity(1.0 - ease_smooth_out(segment(since, 0.0, 0.12)))
                        .child(column),
                }
            });
        let nav = morph
            .is_none()
            .then(|| self.peek_nav(tool, narrow, &c, cx))
            .flatten()
            .map(|nav| {
                // En un costado, en columna en el margen de la izquierda (como
                // corre el carrusel); si no, a la derecha del pie.
                if narrow {
                    return div().absolute().left(px(2.)).top_0().h(px(column_h)).flex().items_center().child(nav);
                }
                div()
                    .absolute()
                    .right(px(SIDE - 6.0))
                    .top(px(column_h - FOOTER_H))
                    .h(px(FOOTER_H))
                    .flex()
                    .items_center()
                    .child(nav)
            });
        // En Expressive, lo copiado: encima del pie y de los puntos.
        let snack = c.m3.filter(|_| morph.is_none()).and_then(|s| self.snack_element(s, column_h, now, cx));
        Some(
            div()
                .id("tool-peek")
                .absolute()
                .left(px(area.x))
                .top(px(area.y))
                .w(px(area.w))
                .h(px(area.h))
                .overflow_hidden()
                .font_family(crate::fonts::UI)
                .text_color(c.text)
                // Un clic en el fondo no arrastra el notch. Mientras se funde
                // en el panel, el fondo deja pasar el clic al panel de abajo.
                // En la página del Portapapeles, una fila apretada se pega al
                // soltar o se arrastra, como en su vistazo de siempre.
                .when(morph.is_none(), |el| {
                    el.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pill, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let p = (f32::from(event.position.x), f32::from(event.position.y));
                            if let Some(hit) = pill.peek_hit(p, Instant::now()) {
                                pill.peek_press = Some((hit, p));
                                cx.notify();
                            }
                        }),
                    )
                })
                // Transformándose en el panel, solo la franja del título y el
                // pie se funden: lo demás lo sigue mostrando el vistazo hasta
                // que pasa al panel (`media_handoff`).
                .opacity(reveal)
                .child(
                    div()
                        .id("tool-peek-shown")
                        .relative()
                        .map(|el| if narrow { el.top(px(shift)) } else { el.left(px(shift)) })
                        .opacity(switched)
                        .child(column),
                )
                .children(leaving)
                .children(nav)
                .children(snack)
                .into_any_element(),
        )
    }

    /// El título, el contenido y el pie del vistazo de `tool`, con su entrada
    /// contada desde `shown_at`. Al cambiar de herramienta se dibujan dos: la
    /// que llega y, encima, la que se va (`ToolPeek::prev`).
    #[allow(clippy::too_many_arguments)]
    fn peek_column(
        &self,
        tool: usize,
        shown_at: Instant,
        width: f32,
        height: f32,
        // La esquina del vistazo en la ventana: Agentes anota ahí dónde
        // quedan sus fichas.
        origin: (f32, f32),
        narrow: bool,
        c: &Colors,
        now: Instant,
        fade: f32,
        slide: Option<(f32, f32)>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let c = *c;
        let m = Motion {
            t: now.duration_since(shown_at).as_secs_f32(),
            m3: c.m3.is_some(),
        };
        // Portapapeles y Agentes traen su vistazo entero (título, filas, pie).
        if tool == crate::CLIPBOARD_TOOL {
            let peek = crate::render_peek(
                &self.peek_entries,
                self.peek_hovered,
                Rect::new(0.0, 0.0, width, height),
                narrow,
                1.0,
                m.t,
                // En Expressive, los roles del esquema de la pill.
                &match c.m3 {
                    Some(s) => crate::Palette { skin: s.surface_container, text: s.on_surface, muted: s.on_surface_variant },
                    None => crate::Palette::dark(),
                },
            );
            return Some(div().relative().w_full().h(px(height)).child(peek));
        }
        if tool == crate::AGENTES_TOOL {
            let area = Rect::new(origin.0, origin.1, width, height - FOOTER_H);
            let usage = self.usage_view(area, origin, 1.0, cx);
            // Abajo, a la izquierda de los puntos: Atic Code, donde se
            // trabaja con los agentes.
            let open = crate::agents::open_code_button(
                "peek-open-code",
                c.text,
                cx.listener(|pill, _: &gpui::ClickEvent, _, cx| {
                    pill.close_tool_peek(Instant::now(), cx);
                    pill.open_code(cx);
                }),
            );
            return Some(
                div()
                    .relative()
                    .w_full()
                    .h(px(height))
                    .child(usage)
                    .child(div().absolute().left(px(SIDE)).bottom_0().h(px(FOOTER_H)).flex().items_center().child(open)),
            );
        }
        let lyric_handoff = self.media_handoff().map_or(0.0, |(_, lyric)| lyric);
        let (title, icon, body, footer) = match tool {
            t if t == crate::color::TOOL => (crate::i18n::t("pill.peeks.recentColors"), "icons/pipette.svg", self.color_body(narrow, &c, m, cx), crate::i18n::t("pill.peeks.pickColor")),
            t if t == crate::capture::TOOL && self.screen_rec.active() => {
                (crate::i18n::t("pill.peeks.recordingScreen"), "icons/scan.svg", self.recording_body(&c, now, cx), crate::i18n::t("pill.peeks.stopAndSave"))
            }
            t if t == crate::capture::TOOL => (crate::i18n::t("pill.peeks.capturesAndRecordings"), "icons/scan.svg", self.shots_body(width, narrow, &c, m, cx), crate::i18n::t("pill.peeks.newCapture")),
            t if t == crate::TEXTOS_TOOL => (crate::i18n::t("tools.snippets.label"), "icons/notebook-pen.svg", self.texts_body(narrow, &c, m, cx), crate::i18n::t("pill.peeks.seeAllTexts")),
            t if t == crate::flip::TOOL => (crate::i18n::t("pill.peeks.flipTitle"), "icons/notebook-tabs.svg", self.pages_body(width, narrow, &c, m, cx), crate::i18n::t("pill.peeks.openFlip")),
            t if t == crate::SISTEMA_TOOL => (crate::i18n::t("tools.system.label"), "icons/sliders-vertical.svg", self.system_body(&c, m, now, cx), crate::i18n::t("pill.peeks.openSystem")),
            t if t == crate::POMODORO_TOOL => {
                let action = if self.pomodoro.running() {
                    crate::i18n::t("pill.pomodoro.pause")
                } else if self.pomodoro.started() {
                    crate::i18n::t("pill.pomodoro.resume")
                } else {
                    crate::i18n::t("pill.pomodoro.start")
                };
                let body = match c.m3 {
                    Some(s) => self.pomodoro_body_m3(s, m, now, cx),
                    None => self.pomodoro_body(&c, now, cx),
                };
                (crate::i18n::t("tools.pomodoro.label"), "icons/timer.svg", body, action)
            }
            t if t == crate::REUNIONES_TOOL => {
                let ink = crate::meetings::Ink { text: c.text, muted: c.muted, faint: c.faint };
                // Tocar una reunión abre su resumen en el notch, como el clic
                // en la herramienta. Va después del evento: la ventana de la
                // pill está ocupada despachándolo.
                let open: crate::meetings::OpenMeeting = std::rc::Rc::new(|id, cx| {
                    cx.defer(move |cx| {
                        if let Some(pill) = cx.windows().into_iter().find_map(|w| w.downcast::<crate::Pill>()) {
                            let _ = pill.update(cx, |pill, window, cx| pill.open_meeting(id, window, cx));
                        }
                    });
                });
                // En Expressive, la última reunión en una tarjeta destacada.
                let body = match c.m3 {
                    Some(s) => crate::meetings::body_m3(&self.studio, s, open, cx),
                    None => crate::meetings::body_with(&self.studio, ink, open, cx),
                };
                (crate::i18n::t("tools.meetings.label"), crate::pill_tools::icon(crate::REUNIONES_TOOL), body, crate::i18n::t("pill.peeks.openMeetings"))
            }
            t if t == crate::MEDIA_TOOL => {
                (crate::i18n::t("pill.tools.nowPlaying"), crate::pill_tools::icon(crate::MEDIA_TOOL), self.media_body(&c, m, slide, lyric_handoff, cx), crate::i18n::t("pill.peeks.openNowPlaying"))
            }
            t if t == crate::board::TOOL => {
                (crate::i18n::t("tools.board.label"), crate::pill_tools::icon(crate::board::TOOL), self.board_body(&c, m, cx), crate::i18n::t("pill.peeks.openBoard"))
            }
            t if t == crate::CELULAR_TOOL => {
                (crate::i18n::t("settings.nav.phone"), crate::pill_tools::icon(crate::CELULAR_TOOL), self.phone_body(&c, m, cx), crate::i18n::t("pill.peeks.openPhone"))
            }
            t if t == crate::COMPUTADORES_TOOL => (
                crate::i18n::t("pill.kvm.nav"),
                crate::pill_tools::icon(crate::COMPUTADORES_TOOL),
                self.computers_body(narrow, &c, m, cx),
                crate::i18n::t("pill.peeks.openComputers"),
            ),
            FACE => (t("pill.peeks.quickTitle"), "icons/settings-2.svg", self.quick_body(narrow, &c, m, cx), t("pill.peeks.openSettings")),
            _ => return None,
        };
        // La cara dice qué versión de Atic corre: junto a «Actualizar».
        let face_version = format!("v{}", crate::updater::VERSION);
        let pomodoro_round = crate::i18n::tf(
            "pill.pomodoro.round",
            &[("n", &self.pomodoro.round()), ("of", &self.pomodoro.rounds())],
        );
        // Computadores: por qué borde pasa el cursor al otro, si se comparte.
        let kvm_edge = (tool == crate::COMPUTADORES_TOOL)
            .then(|| self.tool_peek.kvm.peers.iter().find(|p| p.sharing).and_then(|p| p.side))
            .flatten()
            .filter(|_| self.tool_peek.kvm.running)
            .map(|side| crate::i18n::tf("pill.kvm.peekEdge", &[("side", &crate::computers_settings::side_edge(side))]))
            .unwrap_or_default();
        // En Ahora suena, como en el panel: la app que suena.
        let source = (tool == crate::MEDIA_TOOL)
            .then(|| self.media.track().map(|t| crate::media::source_label(&t.source)))
            .flatten()
            .unwrap_or_default();
        let source_link = tool == crate::MEDIA_TOOL && !source.is_empty();
        let hint = match tool {
            t if t == crate::color::TOOL => crate::i18n::t("pill.peeks.hintColor"),
            t if t == crate::capture::TOOL && self.screen_rec.active() => crate::i18n::t("pill.peeks.hintShortcutToo"),
            t if t == crate::capture::TOOL => crate::i18n::t("pill.peeks.hintShot"),
            t if t == crate::TEXTOS_TOOL => crate::i18n::t("pill.peeks.hintTexts"),
            t if t == crate::flip::TOOL => crate::i18n::t("pill.peeks.hintFlip"),
            t if t == crate::REUNIONES_TOOL => crate::meetings::meet_tool_hint(),
            t if t == crate::MEDIA_TOOL => source.as_str(),
            t if t == crate::board::TOOL => crate::i18n::t("pill.peeks.hintBoard"),
            t if t == crate::CELULAR_TOOL => crate::i18n::t("pill.peeks.hintPhone"),
            t if t == crate::POMODORO_TOOL => pomodoro_round.as_str(),
            t if t == crate::COMPUTADORES_TOOL => kvm_edge.as_str(),
            FACE => face_version.as_str(),
            _ => "",
        };
        let open_tool = cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            pill.open_from_peek(tool, window, cx);
        });
        // Capturas: atajos a las carpetas de capturas y de grabaciones.
        let folders: Vec<AnyElement> = if tool == crate::capture::TOOL && !self.screen_rec.active() {
            let captures = crate::flip_board::notes_dir().and_then(|n| n.parent().map(|p| p.join("captures")));
            [
                ("peek-folder-captures", "icons/folder.svg", crate::i18n::t("pill.peeks.folderCaptures"), captures),
                ("peek-folder-recordings", "icons/video.svg", crate::i18n::t("pill.peeks.folderRecordings"), crate::screen_recording::recordings_dir()),
            ]
            .into_iter()
            .filter_map(|(id, icon, label, dir)| {
                let dir = dir?;
                let (faint, text, m3) = (c.faint, c.text, c.m3.is_some());
                // Con el carrusel a la derecha del pie no caben los nombres: van
                // solo los íconos, en botones redondos (antes se montaban sobre
                // los puntos).
                let compact = !narrow && self.peek_pages().len() >= 2;
                // En Expressive el redondo de 26 va dentro de un blanco de 40,
                // con margen negativo: el pie no se corre.
                let hit = compact && m3;
                Some(
                    div()
                        .id(id)
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .small_text(10.5)
                        .when(compact && !m3, |el| el.size(px(26.)).justify_center().rounded_full())
                        .when(hit, |el| el.size(px(HIT_M3)).m(px(-(HIT_M3 - 26.0) / 2.0)).justify_center())
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = crate::paths::reveal(&dir);
                        })
                        .fx((id, 1usize), move |el, h| {
                            let fg = h.mix(faint, text);
                            if hit {
                                return el.child(
                                    div()
                                        .size(px(26.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_full()
                                        .bg(m3_layer(text.opacity(0.07), text, h))
                                        .child(svg().path(crate::icons::themed(icon)).size(px(14.)).text_color(fg)),
                                );
                            }
                            el.text_color(fg)
                                .when(compact, |el| {
                                    el.bg(if m3 { m3_layer(text.opacity(0.07), text, h) } else { text.opacity(0.07 + 0.09 * h.t) })
                                })
                                .child(svg().path(crate::icons::themed(icon)).size(px(if compact { 14. } else { 11. })).text_color(fg))
                                .when(!compact, |el| el.child(label))
                        })
                        .into_any_element(),
                )
            })
            .collect()
        } else if tool == crate::COMPUTADORES_TOOL {
            // Junto a «Abrir Computadores»: Ajustes, donde se busca y se vincula.
            let (faint, text) = (c.faint, c.text);
            vec![div()
                .id("peek-kvm-settings")
                .flex()
                .items_center()
                .gap(px(4.))
                .small_text(10.5)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.close_tool_peek(Instant::now(), cx);
                        cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Computers, cx));
                    }),
                )
                .fx(("peek-kvm-settings", 1usize), move |el, h| {
                    let fg = h.mix(faint, text);
                    el.text_color(fg)
                        .child(svg().path(crate::icons::themed("icons/settings-2.svg")).size(px(11.)).text_color(fg))
                        .child(t("pill.kvm.inSettings"))
                })
                .into_any_element()]
        } else {
            Vec::new()
        };
        Some(
                    div()
                        .h(px(height))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(TITLE_H))
                                .px(px(SIDE))
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .opacity(fade)
                                // La cara es Atic: su marca y su nombre, y en tenue qué es esto.
                                .child(if tool == FACE {
                                    div().opacity(m.enter(0)).child(crate::kit::chrome::logo(15.)).into_any_element()
                                } else {
                                    // En Expressive el ícono lleva el acento.
                                    svg()
                                        .path(crate::icons::themed(icon))
                                        .size(px(if c.m3.is_some() { 16. } else { 13. }))
                                        .text_color(c.m3.map_or(c.muted, |s| s.primary))
                                        .opacity(m.enter(0))
                                        .into_any_element()
                                })
                                .child(
                                    div()
                                        .map(|el| match c.m3 {
                                            // Título enfatizado de la escala M3 Expressive.
                                            Some(_) => el.type_emphasized(type_scale::TITLE_MEDIUM),
                                            None => el.text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD),
                                        })
                                        .child(falling(if tool == FACE { "Atic" } else { title }, m.t)),
                                )
                                .when(tool == FACE, |el| {
                                    el.child(
                                        div()
                                            .map(|el| match c.m3 {
                                                Some(_) => el.type_style(type_scale::LABEL_LARGE),
                                                None => el.text_size(px(11.)),
                                            })
                                            .text_color(c.muted)
                                            .child(falling(title, m.t)),
                                    )
                                })
                                .child(div().flex_1())
                                .when(!narrow && !source_link, |el| {
                                    el.child(
                                        div()
                                            .map(|el| match c.m3 {
                                                Some(_) => el.type_style(type_scale::LABEL_SMALL),
                                                None => el.text_size(px(10.5)),
                                            })
                                            .text_color(c.faint)
                                            .opacity(ease_smooth_out(segment(m.t, 0.35, 0.4)))
                                            .child(hint.to_string()),
                                    )
                                })
                                // En Ahora suena la app que suena lleva a ella,
                                // como en el panel.
                                .when(!narrow && source_link, |el| {
                                    let (faint, text) = (c.faint, c.text);
                                    let m3 = c.m3;
                                    let label = hint.to_string();
                                    el.child(
                                        div()
                                            .id("peek-media-source")
                                            .flex()
                                            .items_center()
                                            .gap(px(3.))
                                            .px(px(6.))
                                            .h(px(20.))
                                            .rounded(px(10.))
                                            .small_text(10.5)
                                            .cursor_pointer()
                                            .opacity(ease_smooth_out(segment(m.t, 0.35, 0.4)))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                                                    cx.stop_propagation();
                                                    if let Some(track) = pill.media.track() {
                                                        crate::media::focus_source(&track);
                                                    }
                                                    cx.notify();
                                                }),
                                            )
                                            .fx("peek-media-source", move |el, h| {
                                                // En Expressive, un chip de asistencia con contorno.
                                                let (fg, bg) = match m3 {
                                                    Some(s) => (s.on_surface_variant, s.on_surface_variant.opacity(layer_alpha(h))),
                                                    None => (h.mix(faint, text), text.opacity(0.1 * h.t)),
                                                };
                                                el.text_color(fg)
                                                    .bg(bg)
                                                    .when_some(m3, |el, s| el.border_1().border_color(s.outline_variant))
                                                    .child(label.clone())
                                                    .child(svg().path(crate::icons::themed("icons/arrow-up-right.svg")).size(px(10.)).text_color(fg))
                                            }),
                                    )
                                }),
                        )
                        // Lo de Ahora suena pasa al panel (`media_handoff`);
                        // lo de las demás se funde con la franja y el pie.
                        .child(div().opacity(if self.media_handoff().is_some() { 1.0 } else { fade }).child(body))
                        // Lo que sobra en las páginas más bajas del carrusel:
                        // el pie queda abajo en todas.
                        .child(div().flex_1())
                        .child(
                            div()
                                .h(px(FOOTER_H))
                                .px(px(SIDE))
                                .flex()
                                .items_center()
                                // Transformándose en el panel, «Abrir …» ya no
                                // sirve: se va apenas empieza a crecer el notch.
                                .opacity(ease_smooth_out(segment(m.t, 0.30, 0.4)) * fade * slide.map_or(1.0, |(grow, _)| 1.0 - ease_smooth_out(segment(grow, 0.0, 0.3))))
                                .child(
                                div()
                                    .id("tool-peek-open")
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(11.))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, open_tool)
                                    // En Expressive, un botón tonal de M3 (la acción
                                    // del vistazo): contenedor secundario, etiqueta
                                    // LABEL_LARGE y capa de estado.
                                    .when_some(c.m3, |el, s| {
                                        el.h(px(28.))
                                            .px(px(12.))
                                            .gap(px(4.))
                                            .rounded_full()
                                            .type_style(type_scale::LABEL_LARGE)
                                            .bg(s.secondary_container)
                                    })
                                    .fx("tool-peek-open-fx", {
                                        let (faint, text, m3) = (c.faint, c.text, c.m3);
                                        move |el, h| {
                                            let fg = match m3 {
                                                Some(s) => s.on_secondary_container,
                                                None => h.mix(faint, text),
                                            };
                                            el.text_color(fg)
                                                .when_some(m3, |el, s| el.bg(state_layer(s.secondary_container, s.on_secondary_container, h)))
                                                .child(footer)
                                                .child(
                                                    svg()
                                                        .path(crate::icons::themed("icons/arrow-up-right.svg"))
                                                        .size(px(11.))
                                                        .text_color(fg)
                                                        .ml(px(2.0 * h.t)),
                                                )
                                        }
                                    }),
                            )
                            // Junto a «Nueva captura»: a la derecha del pie va
                            // el carrusel (`peek_nav`).
                            .when(!folders.is_empty(), |el| {
                                // Los blancos de 40 de Expressive no se pisan.
                                let gap = if c.m3.is_some() && !narrow && self.peek_pages().len() >= 2 { HIT_M3 - 26.0 } else { 6.0 };
                                el.child(div().ml(px(12.)).flex().items_center().gap(px(gap)).children(folders))
                            }),
                        ),
        )
    }

    /// Una reunión del vistazo: su resumen, en el notch de Reuniones.
    pub(crate) fn open_meeting(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tool_peek(Instant::now(), cx);
        self.tool_peek.amount.snap(0.0);
        self.open_notch(crate::NotchTool::Reuniones, window, cx);
        self.meetings_notch.update(cx, |panel, cx| panel.open_meeting(&id, cx));
    }

    fn open_from_peek(&mut self, tool: usize, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        // El pie del Pomodoro empieza o pausa ahí mismo: el vistazo se queda.
        if tool == crate::POMODORO_TOOL {
            self.toggle_pomodoro(cx);
            return;
        }
        // Si abre un panel del notch, el vistazo no se va de golpe: el notch
        // crece desde él y lo funde, como el de Ahora suena con el scroll.
        if let Some(panel) = peek_panel(tool).filter(|_| !self.panel_visible() && self.tool_peek.tool == Some(tool)) {
            if tool == crate::COMPUTADORES_TOOL {
                self.open_computers(window, cx);
            } else {
                self.open_notch(panel, window, cx);
            }
            self.panel_scrolled = true;
            return;
        }
        self.close_tool_peek(now, cx);
        self.tool_peek.amount.snap(0.0);
        match tool {
            t if t == crate::TEXTOS_TOOL => self.open_notch(crate::NotchTool::Textos, window, cx),
            t if t == crate::SISTEMA_TOOL => self.open_notch(crate::NotchTool::Sistema, window, cx),
            FACE => cx.defer(|cx| crate::settings::open(cx)),
            _ => self.run_tool(tool, window, cx),
        }
    }

    /// Copia un código de color y lo marca como copiado (en Expressive, con su
    /// snackbar y lo que había antes en el portapapeles, para «Deshacer»).
    fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        let undo = if self.tool_peek.m3 { clipboard_text(cx) } else { None };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        let label = crate::i18n::tf("pill.peeks.snackColor", &[("code", &text)]);
        self.mark_copied(text, label, undo, cx);
    }

    /// Marca `key` como copiado. En Expressive, además, el snackbar con `text`
    /// y, si `undo` trae lo de antes, «Deshacer». Con uno ya a la vista, ese
    /// se queda y se alarga: deshacer vuelve a lo que había antes de empezar
    /// a copiar.
    fn mark_copied(&mut self, key: String, text: String, undo: Option<String>, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.tool_peek.copied = Some((key, now));
        if self.tool_peek.m3 {
            if let Some(snack) = self.tool_peek.snack.as_mut().filter(|snack| snack.gone_at.is_none()) {
                snack.text = text;
                snack.until = now + SNACK_FOR;
            } else {
                self.tool_peek.snack = Some(Snack { text, undo, at: now, until: now + SNACK_FOR, gone_at: None });
            }
        }
        cx.notify();
    }

    /// «Deshacer»: devuelve lo que había en el portapapeles y el snackbar se
    /// va (el vistazo se queda).
    fn undo_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(before) = self.tool_peek.snack.as_ref().and_then(|snack| snack.undo.clone()) {
            cx.write_to_clipboard(ClipboardItem::new_string(before));
        }
        self.tool_peek.copied = None;
        self.tool_peek.dismiss_snack(Instant::now());
        cx.notify();
    }

    /// El snackbar: a su hora empieza a irse y, cuando terminó de salir, se
    /// borra. `true` si se fue solo con lo copiado todavía marcado: ahí el
    /// vistazo se recoge, como antes con el «Copiado» (tras «Deshacer», o si
    /// el vistazo ya se estaba cerrando, no).
    fn advance_snack(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        let Some(snack) = self.tool_peek.snack.as_mut() else {
            return false;
        };
        if snack.gone_at.is_none() && now >= snack.until {
            snack.gone_at = Some(now);
            cx.notify();
        }
        let gone = snack
            .gone_at
            .is_some_and(|at| crate::anim::spring_progress(SpringKind::EffectsFast, now.duration_since(at)) >= 1.0);
        if !gone {
            return false;
        }
        self.tool_peek.snack = None;
        cx.notify();
        self.tool_peek.copied.is_some()
    }

    /// Pide cuadros solo mientras el snackbar entra o sale; quieto, el cierre
    /// lo pone el sondeo (`advance_snack`).
    fn snack_animating(&self, now: Instant) -> bool {
        self.tool_peek.snack.as_ref().is_some_and(|snack| {
            snack.gone_at.is_some()
                || (!crate::anim::reduced_motion()
                    && now.duration_since(snack.at) < SNACK_IN.mul_f32(crate::anim::spring_time_scale()))
        })
    }

    /// El snackbar de lo copiado, abajo en el vistazo (sobre el pie), con el
    /// aspecto del `Toast` de gpui-m3 pero en el esquema de la pill y con el
    /// clic en `on_mouse_down`, como todo el vistazo: superficie inversa,
    /// esquinas S (XS el compacto) y «Deshacer» si se puede. Entra subiendo con
    /// `SPATIAL_FAST` y fundiéndose con `EFFECTS`; sale con `EFFECTS_FAST`.
    fn snack_element(&self, s: Scheme, column_h: f32, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snack = self.tool_peek.snack.as_ref()?;
        let since = now.duration_since(snack.at);
        // El resorte espacial sin recortar: se pasa apenas al subir.
        let rise = if crate::anim::reduced_motion() {
            1.0
        } else {
            SpringKind::SpatialFast.spring().step(since.as_secs_f32() / crate::anim::spring_time_scale())
        };
        let fade_in = crate::anim::spring_progress(SpringKind::Effects, since);
        let fade_out = snack
            .gone_at
            .map_or(1.0, |at| 1.0 - crate::anim::spring_progress(SpringKind::EffectsFast, now.duration_since(at)));
        let full = column_h >= 140.0;
        let height = if full { SNACK_H } else { SNACK_COMPACT_H };
        let top = column_h - height - 2.0 + SNACK_RISE * (1.0 - rise);
        let undo = snack.undo.is_some().then(|| {
            // El blanco del clic, de 40 (o el alto del compacto); el botón de
            // texto dibujado, de 32.
            div()
                .id("peek-snack-undo")
                .h(px(height.min(HIT_M3)))
                .flex()
                .flex_none()
                .items_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.undo_copy(cx);
                    }),
                )
                .fx("peek-snack-undo-fx", move |el, hv| {
                    el.child(
                        div()
                            .h(px(32.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .bg(s.inverse_primary.opacity(layer_alpha(hv)))
                            .type_style(type_scale::LABEL_LARGE)
                            .text_color(s.inverse_primary)
                            .child(t("pill.peeks.undo")),
                    )
                })
        });
        Some(
            div()
                .id("peek-snack")
                .absolute()
                .left(px(8.))
                .right(px(8.))
                .top(px(top))
                .h(px(height))
                .pl(px(16.))
                .pr(px(if undo.is_some() { 4. } else { 16. }))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(if full { corner::S } else { corner::XS })
                .bg(s.inverse_surface)
                .text_color(s.inverse_on_surface)
                .opacity(fade_in * fade_out)
                // Lo de abajo no se ilumina ni se copia con un clic en él.
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(div().flex_1().min_w_0().truncate().type_style(type_scale::BODY_MEDIUM).child(snack.text.clone()))
                .children(undo)
                .into_any_element(),
        )
    }

    fn is_copied(&self, key: &str) -> bool {
        self.tool_peek.copied.as_ref().is_some_and(|(k, _)| k == key)
    }

    // --- Grabando la pantalla ---------------------------------------------------

    /// El reloj y Detener, Pausar (o Reanudar) y Descartar.
    fn recording_body(&self, c: &Colors, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let ink = crate::meetings::Ink { text: c.text, muted: c.muted, faint: c.faint };
        let paused = self.screen_rec.paused();
        // El punto late como el de Reuniones; en pausa queda apagado.
        let pulse = if paused {
            0.35
        } else {
            0.65 + 0.35 * (now.duration_since(self.tool_peek.shown_at).as_secs_f32() * 3.0).cos().abs()
        };
        let pill = cx.entity();
        let (stop, pause, discard) = (pill.clone(), pill.clone(), pill);
        div()
            .flex()
            .flex_col()
            .w_full()
            .child(crate::meetings::head_row(
                if paused { t("pill.peeks.paused") } else { t("pill.peeks.recording") },
                self.screen_rec.clock(),
                pulse,
                ink,
            ))
            .child(
                crate::meetings::actions_row()
                    .child(crate::meetings::button("rec-stop", t("page.agents.chat.stop"), true, ink, move |cx| {
                        stop.update(cx, |pill, cx| pill.stop_screen_recording(cx))
                    }))
                    .child(crate::meetings::button(
                        "rec-pause",
                        if paused { t("pill.peeks.resume") } else { t("overlay.pause") },
                        false,
                        ink,
                        move |cx| pause.update(cx, |pill, cx| pill.toggle_screen_pause(cx)),
                    ))
                    .child(crate::meetings::button("rec-discard", t("page.meetings.discard"), false, ink, move |cx| {
                        discard.update(cx, |pill, cx| pill.discard_screen_recording(cx))
                    })),
            )
            .into_any_element()
    }

    // --- Color --------------------------------------------------------------

    fn color_body(&self, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.tool_peek.data.colors.clone();
        if colors.is_empty() {
            return match c.m3 {
                Some(s) => self.empty_state_m3(crate::color::TOOL, ShapeName::Flower, "icons/pipette.svg", t("pill.peeks.emptyColorsTitle"), t("pill.peeks.emptyColorsHint"), Some(t("pill.peeks.pickColor")), s, m, cx),
                None => empty(t("pill.peeks.emptyColors"), c),
            };
        }
        if narrow {
            return self.color_rows(colors, c, m, cx);
        }
        if let Some(s) = c.m3 {
            return self.color_body_m3(colors, s, m, cx);
        }
        let shown = self
            .tool_peek
            .hovered_color
            .filter(|h| colors.contains(h))
            .unwrap_or(colors[0]);
        let copied = colors.iter().find(|&&color| self.is_copied(&hex(color))).copied();
        let shown = copied.unwrap_or(shown);
        let text = c.text;
        let swatches = colors.into_iter().take(10).enumerate().map(|(i, color)| {
            let key = hex(color);
            let on = Some(color) == copied;
            div()
                .id(("peek-color", i))
                .size(px(SWATCH))
                .relative()
                .cursor_pointer()
                .on_hover(cx.listener(move |pill, hovered: &bool, _, cx| {
                    if *hovered {
                        pill.tool_peek.hovered_color = Some(color);
                    } else if pill.tool_peek.hovered_color == Some(color) {
                        pill.tool_peek.hovered_color = None;
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.copy_text(key.clone(), cx);
                    }),
                )
                .fx(("peek-color-fx", i), move |el, h| {
                    // Crece con el cursor (como `.op-swatch` de la web) sin
                    // mover a sus vecinas; el copiado queda grande. Al abrir,
                    // cada uno brota desde su centro tras el anterior.
                    let e = m.enter(i);
                    let grow = if on { 3.0 } else { 3.0 * h.t - 2.0 * h.press } - (1.0 - e) * SWATCH * 0.45;
                    let el = el.opacity(e);
                    el.child(
                        div()
                            .absolute()
                            .inset(px(-grow))
                            .rounded(px(SWATCH / 2.0 + grow))
                            .bg(rgb(color))
                            .border_1()
                            .border_color(text.opacity(0.18 + 0.3 * h.t)),
                    )
                })
                .into_any_element()
        });
        let ok = c.ok;
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().h(px(SWATCH)).flex().gap(px(9.)).children(swatches))
            .child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .opacity(m.enter(3))
                    .child(div().size(px(10.)).rounded(px(5.)).bg(rgb(shown)))
                    .child(div().child(hex(shown)))
                    .child(div().text_color(c.faint).child(rgb_text(shown)))
                    .when(copied.is_some(), |el| el.child(div().text_color(ok).child(format!("· {}", t("pill.peek.copied"))))),
            )
            .into_any_element()
    }

    /// Los colores en columna, para el bloque angosto de un costado: una fila
    /// por color con su código; clic lo copia.
    fn color_rows(&self, colors: Vec<u32>, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let (text, faint, ok, m3) = (c.text, c.faint, c.ok, c.m3.is_some());
        let rows = colors.into_iter().take(SIDE_COLORS).enumerate().map(|(i, color)| {
            let key = hex(color);
            let copied = self.is_copied(&key);
            let e = m.enter(i);
            div()
                .id(("peek-color-row", i))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(COLOR_ROW_H - 2.0))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(9.))
                .cursor_pointer()
                .text_size(px(12.))
                .on_mouse_down(MouseButton::Left, {
                    let key = key.clone();
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.copy_text(key.clone(), cx);
                    })
                })
                .child(
                    div()
                        .size(px(18.))
                        .flex_none()
                        .rounded(px(9.))
                        .bg(rgb(color))
                        .border_1()
                        .border_color(text.opacity(0.18)),
                )
                .child(div().child(key))
                .child(div().flex_1().min_w_0().truncate().text_color(faint).child(rgb_text(color)))
                .when(copied, |el| el.child(div().flex_none().text_color(ok).child(t("pill.peek.copied"))))
                .map(|el| match m3 {
                    true => el.fx(("peek-color-row-fx", i), move |el, h| el.bg(text.opacity(layer_alpha(h)))),
                    false => el.hover_bg(("peek-color-row-fx", i), text.opacity(0.0), text.opacity(0.08)),
                })
                .into_any_element()
        });
        div()
            .px(px(SIDE - 8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows)
            .into_any_element()
    }

    // --- Capturas -----------------------------------------------------------

    fn shots_body(&self, width: f32, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let shots = self.tool_peek.data.shots.clone();
        if shots.is_empty() {
            return match c.m3 {
                Some(s) => self.empty_state_m3(crate::capture::TOOL, ShapeName::Cookie4, "icons/scan.svg", t("pill.peeks.emptyShotsTitle"), t("pill.peeks.emptyShotsHint"), Some(t("pill.peeks.newCapture")), s, m, cx),
                None => empty(t("pill.peeks.emptyShots"), c),
            };
        }
        if let (Some(s), false) = (c.m3, narrow) {
            return self.shots_body_m3(shots, width, s, m, cx);
        }
        // Angosto: una captura por fila, con la miniatura a la izquierda.
        let h = if narrow { SHOT_THUMB.1 } else { shot_height(width) };
        let (text, faint, ok) = (c.text, c.faint, c.ok);
        // Las listas muestran 3; el resto es del carrusel de Expressive.
        let cells = shots.into_iter().take(3).enumerate().map(|(i, shot)| {
            let key = shot.path.to_string_lossy().to_string();
            let copied = self.is_copied(&key);
            let path = shot.path.clone();
            let has_thumb = !shot.video || crate::screen_cursor::thumb_path(&shot.path).is_some_and(|p| p.exists());
            div()
                .id(("peek-shot", i))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .when(narrow, |el| el.flex_row().items_center().gap(px(10.)).h(px(SHOT_ROW_H - 6.0)))
                .cursor_grab()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let at = (f32::from(event.position.x), f32::from(event.position.y));
                        pill.tool_peek.press = Some((path.clone(), at));
                    }),
                )
                .fx(("peek-shot-fx", i), move |el, hv| {
                    // La imagen se acerca un poco (`scale(1.04)` en la web) y
                    // su borde se enciende. Al abrir, cada una cae a su sitio
                    // tras la anterior, alejándose desde un poco más cerca.
                    let e = m.enter(i);
                    let zoom = 1.0 + 0.06 * hv.t - 0.03 * hv.press + 0.12 * (1.0 - e);
                    el.relative().top(px(-8.0 * (1.0 - e))).opacity(e).child(
                        div()
                            .h(px(h))
                            .w_full()
                            .when(narrow, |el| el.w(px(SHOT_THUMB.0)).flex_none())
                            .relative()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(text.opacity(0.06))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-(zoom - 1.0) * h / 2.0))
                                    .left(gpui::relative(-(zoom - 1.0) / 2.0))
                                    .w(gpui::relative(zoom))
                                    .h(px(h * zoom))
                                    .children(if shot.video {
                                        crate::screen_cursor::thumb_path(&shot.path).filter(|p| p.exists())
                                    } else {
                                        crate::clipboard::preview(&shot.path).map(|p| p.to_path_buf())
                                    }.map(|p| {
                                        crate::release::scoped(
                                            gpui::SharedString::from(format!("shot-{}", p.display())),
                                            img(p).size_full().object_fit(ObjectFit::Cover),
                                        )
                                    })),
                            )
                            .when(shot.video && !has_thumb, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(svg().path(crate::icons::themed("icons/video.svg")).size(px(20.)).text_color(faint)),
                                )
                            })
                            .when(shot.video, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .left(px(6.))
                                        .bottom(px(6.))
                                        .size(px(22.))
                                        .rounded_full()
                                        .bg(gpui::black().opacity(0.6))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(svg().path(crate::icons::themed("icons/play.svg")).size(px(11.)).text_color(gpui::white())),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(text.opacity(0.12 + 0.35 * hv.t)),
                            ),
                    )
                    .child(
                        div()
                            .h(px(SHOT_LABEL_H - 3.0))
                            .flex()
                            .when(!narrow, |el| el.justify_center())
                            .when(narrow, |el| el.items_center().text_size(px(12.)))
                            .small_text(10.5)
                            .text_color(if copied { ok } else { hv.mix(faint, text) })
                            .child(if copied { t("pill.peeks.copiedShot").to_string() } else { shot.label.clone() }),
                    )
                })
                .into_any_element()
        });
        div()
            .px(px(SIDE))
            .flex()
            .gap(px(SHOT_GAP))
            .when(narrow, |el| el.flex_col().gap(px(6.)))
            .children(cells)
            .into_any_element()
    }

    /// La captura como archivo hacia otra app. `DoDragDrop` corre un loop
    /// modal que sigue despachando a GPUI: en una tarea aparte, no dentro
    /// del manejador.
    fn drag_shot(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = path.to_string_lossy().into_owned();
        cx.spawn(async move |_, _| match crate::drag::drag_files(&[path]) {
            Ok(outcome) => println!("vistazo → arrastre de captura: soltado={}", outcome.dropped),
            Err(error) => eprintln!("vistazo: arrastre de captura: {error}"),
        })
        .detach();
    }

    fn copy_shot(&mut self, path: PathBuf, key: String, cx: &mut Context<Self>) {
        let undo = if self.tool_peek.m3 { clipboard_text(cx) } else { None };
        self.mark_copied(key, t("pill.peeks.snackShot").to_string(), undo, cx);
        cx.background_spawn(async move {
            let result = (|| -> Result<(), String> {
                let png = std::fs::read(&path).map_err(|e| e.to_string())?;
                let rgba = image::load_from_memory(&png).map_err(|e| e.to_string())?.to_rgba8();
                let (w, h) = rgba.dimensions();
                let mut bgra = rgba.into_raw();
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
                crate::clip_image::write(w, h, &bgra, &png)
            })();
            if let Err(error) = result {
                eprintln!("vistazo: no se pudo copiar la captura: {error}");
            }
        })
        .detach();
    }

    // --- Textos -------------------------------------------------------------

    /// La pista de ahora, aunque esté en pausa, como en el panel de Ahora
    /// suena: título y artista, y ±10 s / anterior / play-pausa / siguiente.
    /// `slide`: abriendo el panel con el scroll, cuánto creció el notch y
    /// dónde empieza el panel (desde arriba del vistazo); cada parte se corre
    /// hasta donde está en el panel.
    /// `lyric_handoff`: cuánto de la letra ya muestra el panel.
    fn media_body(&self, c: &Colors, m: Motion, slide: Option<(f32, f32)>, lyric_handoff: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(track) = self.media.track() else {
            // Desde aquí no hay nada que hacer sonar: sin acción.
            return match c.m3 {
                Some(s) => self.empty_state_m3(crate::MEDIA_TOOL, ShapeName::Clover4, crate::pill_tools::icon(crate::MEDIA_TOOL), t("pill.peeks.emptyMediaTitle"), t("pill.peeks.emptyMediaHint"), None, s, m, cx),
                None => empty(t("pill.peeks.emptyMedia"), c),
            };
        };
        let e = m.enter(0);
        let timeline = track.position.is_some();
        let (title_at, controls_at, verse_at) = crate::media::MediaPanel::morph_targets(timeline);
        let shift = |here: f32, there: f32| slide.map_or(0.0, |(grow, top)| (top + there - here) * grow);
        let controls_here = TITLE_H + MEDIA_TITLE_H + MEDIA_GAP;
        // La letra crece al tamaño de la del panel mientras se transforma.
        let grow = slide.map_or(0.0, |(grow, _)| grow);
        let verse_here = controls_here + MEDIA_CONTROLS_H + 6.0 + crate::hang::current_h(grow) / 2.0;
        let (title_dy, controls_dy, verse_dy) = (
            shift(TITLE_H, title_at),
            shift(controls_here, controls_at),
            shift(verse_here, verse_at),
        );
        // Los botones redondos del panel (`MediaPanel::round_button`): el de
        // play grande y claro, los demás transparentes con fondo al pasar.
        // En Expressive, los mismos `MediaControls` del panel.
        let control = |id: &'static str, icon: &'static str, badge: Option<&'static str>, big: bool, control: crate::media::Control| {
            let size = if big { MEDIA_CONTROLS_H } else { MEDIA_BUTTON };
            let cream: Hsla = rgb(0xf0f0ea).into();
            let fg: Hsla = if big { rgb(0x1a1a18).into() } else { cream };
            div()
                .id(id)
                .relative()
                .size(px(size))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(size / 2.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.media.control(control);
                        cx.notify();
                    }),
                )
                .children(badge.map(|text| {
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .pt(px(1.))
                        .text_size(px(7.5))
                        .text_color(fg)
                        .child(text)
                }))
                .fx((id, 1usize), move |el, h| {
                    let bg = if big {
                        h.mix(cream, cream.opacity(0.85))
                    } else {
                        h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.12 + 0.06 * h.press))
                    };
                    let icon_size = if big { 22. } else { 19. } * (1.0 - 0.08 * h.press);
                    el.bg(bg).child(svg().path(crate::icons::themed(icon)).size(px(icon_size)).text_color(fg))
                })
        };
        let header = div()
            .relative()
            .top(px(title_dy))
            .h(px(MEDIA_TITLE_H))
            .px(px(MEDIA_SIDE))
            .flex()
            .flex_col()
            .justify_end()
            .gap(px(2.))
            .map(|el| match c.m3 {
                // Los del panel (`media::m3_title`): en el relevo, idénticos.
                Some(s) => el.children(crate::media::m3_title(track.title.clone(), track.artist.clone(), &s)),
                None => el
                    .child(
                        div()
                            .truncate()
                            .text_size(px(18.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(c.text)
                            .child(track.title.clone()),
                    )
                    .child(div().truncate().text_size(px(13.)).text_color(c.text.opacity(0.8)).child(track.artist.clone())),
            });
        let controls = div()
            .relative()
            .top(px(controls_dy))
            .h(px(MEDIA_CONTROLS_H))
            .mt(px(MEDIA_GAP))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .opacity(m.enter(1));
        let controls = if let Some(s) = c.m3 {
            let media = self.media.clone();
            let pill = cx.entity().downgrade();
            controls.child(crate::media::MediaControls::new(
                "peek-media",
                track.playing,
                timeline,
                s,
                true,
                move |control, _, cx| {
                    media.control(control);
                    let _ = pill.update(cx, |_, cx| cx.notify());
                },
            ))
        } else {
            controls
                .when(timeline, |el| {
                    el.child(control("peek-media-back", "icons/rotate-ccw.svg", Some("10"), false, crate::media::Control::Jump(-crate::media::JUMP)))
                })
                .child(control("peek-media-prev", "icons/skip-back.svg", None, false, crate::media::Control::Previous))
                .child(control(
                    "peek-media-toggle",
                    if track.playing { "icons/pause.svg" } else { "icons/play.svg" },
                    None,
                    true,
                    crate::media::Control::Toggle,
                ))
                .child(control("peek-media-next", "icons/skip-forward.svg", None, false, crate::media::Control::Next))
                .when(timeline, |el| {
                    el.child(control("peek-media-forward", "icons/rotate-cw.svg", Some("10"), false, crate::media::Control::Jump(crate::media::JUMP)))
                })
        };
        let lyrics = self.tool_peek.has_lyrics.then(|| self.verse_lines(false, "peek", grow)).flatten().map(|lines| {
            // Si la lleva la letra que cuelga (`hang_rides_media_peek`), esta
            // solo cubre lo que a esa le falta para aparecer.
            let hang = self.hang_rides_media_peek(Instant::now()).map_or(0.0, |_| crate::anim::segment(self.hang.value(Instant::now()), 0.5, 0.5));
            div().relative().top(px(verse_dy)).h(px(MEDIA_LYRICS_H)).pt(px(6.)).px(px(18.0 + (crate::media::SIDE + 8.0 - 18.0) * grow)).opacity(m.enter(2) * (1.0 - lyric_handoff) * (1.0 - hang)).child(lines)
        });
        div()
            .relative()
            .top(px(-7.0 * (1.0 - e)))
            .opacity(e)
            .flex()
            .flex_col()
            .child(header)
            .child(controls)
            .children(lyrics)
            .into_any_element()
    }

    /// Los últimos avisos del celular: tocar uno abre «Celular».
    fn phone_body(&self, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        if let Some(s) = c.m3 {
            return self.phone_body_m3(s, m, cx);
        }
        let rows = self.tool_peek.phone.clone();
        // Sin celular vinculado: un clic abre el panel, que muestra el QR ahí mismo.
        if !self.tool_peek.phone_paired {
            let (text, muted) = (c.text, c.muted);
            return div()
                .id("peek-phone-pair")
                .h(px(EMPTY_H))
                .mx(px(SIDE - 8.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(10.))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        pill.open_from_peek(crate::CELULAR_TOOL, window, cx);
                    }),
                )
                .fx("peek-phone-pair-fx", move |el, h| {
                    let fg = h.mix(muted, text);
                    el.bg(text.opacity(0.08 * h.t))
                        .text_size(px(12.))
                        .text_color(fg)
                        .child(svg().path(crate::icons::themed("icons/plus.svg")).size(px(13.)).text_color(fg))
                        .child(t("pill.peeks.phoneUnpaired"))
                })
                .into_any_element();
        }
        if rows.is_empty() {
            return empty(t("pill.peeks.emptyPhone"), c);
        }
        let (text, faint) = (c.text, c.faint);
        let rows = rows.into_iter().enumerate().map(|(i, row)| {
            let e = m.enter(i);
            let lead = match row.icon {
                Some(icon) => gpui::img(icon).size(px(22.)).flex_none().rounded_full().into_any_element(),
                None => div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(text.opacity(0.10))
                    .child(svg().path(crate::icons::themed("icons/smartphone.svg")).size(px(12.)).text_color(text))
                    .into_any_element(),
            };
            let detail: String = row.detail.split_whitespace().collect::<Vec<_>>().join(" ");
            div()
                .id(("peek-phone", i))
                .relative()
                .top(px(-7.0 * (1.0 - e)))
                .opacity(e)
                .h(px(TEXT_ROW_H - 2.0))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(10.))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        pill.open_from_peek(crate::CELULAR_TOOL, window, cx);
                    }),
                )
                .child(lead)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(div().truncate().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child(row.title))
                        .child(div().truncate().text_size(px(11.)).text_color(faint).child(format!("{} · {detail}", row.app))),
                )
                .hover_bg(("peek-phone-fx", i), text.opacity(0.0), text.opacity(0.08))
                .into_any_element()
        });
        div()
            .px(px(SIDE - 8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows)
            .into_any_element()
    }

    /// Las tareas pendientes (un clic en la casilla la marca hecha), o, sin
    /// ninguna, los textos guardados.
    /// Las tareas, como una tarjeta de pendientes: a la izquierda las hechas
    /// hoy del total, «Agregar tarea» (abre Textos escribiendo una) y «Ver
    /// todas»; a la derecha las pendientes, que un clic marca hechas.
    fn texts_body(&self, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        if let Some(s) = c.m3 {
            return self.texts_body_m3(narrow, s, m, cx);
        }
        let (text, muted, faint, m3) = (c.text, c.muted, c.faint, c.m3);
        // En Expressive la casilla y lo hecho llevan el acento.
        let green: Hsla = m3.map_or(rgb(0x6cc48a).into(), |s| s.primary);
        let ink: Hsla = rgb(0x1a1a18).into();
        let (done, total) = self.tool_peek.data.tasks_today;
        let tasks = self.tool_peek.data.tasks.clone();
        let open_tasks = |id: &'static str, label: &'static str, icon: &'static str, primary: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded(px(13.))
                .text_size(px(11.5))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        pill.close_tool_peek(Instant::now(), cx);
                        pill.open_notch(crate::NotchTool::Textos, window, cx);
                    }),
                )
                .when(m3.is_some(), |el| el.font_weight(type_scale::LABEL_LARGE.weight))
                .fx(id, move |el, h| {
                    // En Expressive: «Agregar» es un botón lleno y «Ver todas»
                    // uno de texto en el acento, los dos con capa de estado.
                    if let Some(s) = m3 {
                        let (fg, bg) = if primary {
                            (s.on_primary, state_layer(s.primary, s.on_primary, h))
                        } else {
                            (s.primary, s.primary.opacity(0.08 * h.t + 0.02 * h.press))
                        };
                        return el.text_color(fg).bg(bg).child(svg().path(crate::icons::themed(icon)).size(px(12.)).text_color(fg)).child(label);
                    }
                    if primary {
                        el.text_color(ink)
                            .bg(text.opacity(0.88 + 0.12 * h.t))
                            .child(svg().path(crate::icons::themed(icon)).size(px(12.)).text_color(ink))
                            .child(label)
                    } else {
                        el.text_color(h.mix(muted, text))
                            .bg(text.opacity(0.07 * h.t))
                            .child(svg().path(crate::icons::themed(icon)).size(px(12.)).text_color(h.mix(muted, text)))
                            .child(label)
                    }
                })
        };
        let counter = div()
            .w(px(150.))
            .flex_none()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(6.))
            .opacity(m.enter(0))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(2.))
                    .child(
                        div()
                            .font_family(crate::fonts::MONO)
                            .text_size(px(28.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .when_some(m3, |el, s| el.text_color(s.primary))
                            .child(done.to_string()),
                    )
                    .child(div().font_family(crate::fonts::MONO).text_size(px(18.)).text_color(muted).child(format!("/{total}"))),
            )
            .child(div().text_size(px(11.)).text_color(muted).child(t("pill.tasks.doneToday")))
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(open_tasks("peek-task-add", t("pill.tasks.add"), "icons/plus.svg", true, cx))
                    .child(open_tasks("peek-task-all", t("pill.tasks.seeAll"), "icons/arrow-up-right.svg", false, cx)),
            );
        let list = if tasks.is_empty() {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .child(svg().path(crate::icons::themed("icons/check.svg")).size(px(18.)).text_color(faint))
                .child(div().text_size(px(11.5)).text_color(muted).child(t("pill.tasks.none")))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(tasks.into_iter().enumerate().map(|(i, task)| {
                    let e = m.enter(i + 1);
                    let id = task.id;
                    div()
                        .id(("peek-task", i))
                        .relative()
                        .top(px(-6.0 * (1.0 - e)))
                        .opacity(e)
                        .h(px(TASK_ROW_H))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        // Concéntrica con la caja de la lista (12 menos su margen de 4).
                        .rounded(px(8.))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                let mut all = crate::tasks::Tasks::load();
                                all.toggle(id);
                                pill.tool_peek.data.tasks.retain(|t| t.id != id);
                                pill.tool_peek.data.tasks_today = all.today();
                                cx.notify();
                            }),
                        )
                        .child(div().size(px(14.)).flex_none().rounded(px(4.)).border_1().border_color(green.opacity(0.8)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(task.text.clone()))
                        .children(task.meeting.as_ref().map(|(_, title)| {
                            div().max_w(px(90.)).flex_none().truncate().small_text(10.).text_color(faint).child(title.clone())
                        }))
                        .hover_bg(("peek-task-fx", i), text.opacity(0.0), text.opacity(0.08))
                        .into_any_element()
                }))
                .into_any_element()
        };
        // En el bloque angosto de un costado, uno sobre otro.
        let list = div()
            .p(px(4.))
            .rounded(px(12.))
            .bg(m3.map_or(text.opacity(0.05), |s| s.surface_container_high))
            .child(list);
        if narrow {
            return div()
                .px(px(SIDE))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(counter.h(px(TASKS_COUNTER_H)))
                .child(list.h(px(TASKS_BODY_H)))
                .into_any_element();
        }
        div()
            .h(px(TASKS_BODY_H))
            .px(px(SIDE))
            .flex()
            .gap(px(12.))
            .child(counter)
            .child(list.flex_1().min_w_0().h_full())
            .into_any_element()
    }

    // --- Pizarra -------------------------------------------------------------

    /// Las herramientas (un clic abre la pizarra con esa, ya con el color),
    /// los colores (eligen sin abrir) y la última captura para rayarla.
    fn board_body(&self, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        if let Some(s) = c.m3 {
            return self.board_body_m3(s, m, cx);
        }
        use crate::board::COLORS;
        let prefs = self.tool_peek.data.board;
        let (text, muted, faint) = (c.text, c.muted, c.faint);
        let ink: Hsla = rgb(prefs.color).into();
        // El borrador no: abrir una pizarra vacía para borrar no sirve.
        let tools = board_tools().enumerate().map(|(i, &(tool, icon, label))| {
            let on = prefs.tool == tool;
            let e = m.enter(i);
            div()
                .id(("peek-board-tool", i))
                .size(px(BOARD_BTN))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(BOARD_BTN / 2.0))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .cursor_pointer()
                .tooltip(crate::hover::tip(crate::i18n::t(label)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        pill.board_from_peek(tool, window, cx);
                    }),
                )
                .fx(("peek-board-tool-fx", i), move |el, h| {
                    let (rest, over) = if on { (0.16, 0.2) } else { (0.0, 0.09) };
                    // La herramienta de la última vez va del color elegido.
                    let fg = if on { ink } else { h.mix(muted, text) };
                    el.bg(h.mix(text.opacity(rest), text.opacity(over + 0.05 * h.press)))
                        .child(svg().path(icon).size(px(16. - 1.5 * h.press)).text_color(fg))
                })
                .into_any_element()
        });
        let colors = COLORS.iter().enumerate().map(|(i, &color)| {
            let on = prefs.color == color;
            let e = m.enter(i + 2);
            div()
                .id(("peek-board-color", i))
                .size(px(BOARD_COLOR_H))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(BOARD_COLOR_H / 2.0))
                .opacity(e)
                .cursor_pointer()
                .when(on, |el| el.border_2().border_color(text.opacity(0.85)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.tool_peek.data.board.color = color;
                        pill.tool_peek.data.board.save();
                        cx.notify();
                    }),
                )
                .fx(("peek-board-color-fx", i), move |el, h| {
                    let size = 14. + 3. * h.t - 2. * h.press;
                    el.child(
                        div()
                            .size(px(size))
                            .rounded(px(size / 2.))
                            .bg(rgb(color))
                            .border_1()
                            .border_color(text.opacity(0.25 + 0.2 * h.t)),
                    )
                })
                .into_any_element()
        });
        let shot = self.tool_peek.data.last_shot.clone().map(|shot| {
            let e = m.enter(4);
            let path = shot.path.clone();
            div()
                .id("peek-board-shot")
                .h(px(SHOT_ROW_H - 4.0))
                .mt(px(BOARD_GAP))
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(10.))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        pill.board_on_shot(path.clone(), window, cx);
                    }),
                )
                .child(
                    div()
                        .w(px(SHOT_THUMB.0 * 0.8))
                        .h(px(SHOT_THUMB.1 * 0.8))
                        .flex_none()
                        // Concéntrica con la fila (10 menos el margen de 6).
                        .rounded(px(4.))
                        .overflow_hidden()
                        .bg(text.opacity(0.06))
                        .children(crate::clipboard::preview(&shot.path).map(|p| {
                            crate::release::scoped(
                                gpui::SharedString::from(format!("shot-row-{}", p.display())),
                                img(p).size_full().object_fit(ObjectFit::Cover),
                            )
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(t("pill.peeks.scribbleLast")),
                        )
                        .child(div().truncate().text_size(px(11.)).text_color(faint).child(shot.label)),
                )
                .child(svg().path(crate::icons::themed("icons/pencil.svg")).size(px(14.)).text_color(muted))
                .hover_bg("peek-board-shot-fx", text.opacity(0.0), text.opacity(0.08))
                .into_any_element()
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .child(div().flex().flex_wrap().gap(px(BOARD_BTN_GAP)).children(tools))
            .child(div().h(px(BOARD_COLOR_H)).mt(px(BOARD_GAP)).flex().gap(px(6.)).children(colors))
            .children(shot)
            .into_any_element()
    }

    /// Abre la pizarra con `tool` y el color elegido en el vistazo.
    fn board_from_peek(&mut self, tool: crate::board::Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.tool_peek.data.board.tool = tool;
        self.tool_peek.data.board.save();
        self.close_tool_peek(Instant::now(), cx);
        self.tool_peek.amount.snap(0.0);
        self.start_board(window, cx);
    }

    /// La pizarra sobre la última captura, con lo elegido en el vistazo.
    fn board_on_shot(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.tool_peek.data.board.save();
        self.close_tool_peek(Instant::now(), cx);
        self.tool_peek.amount.snap(0.0);
        self.draw_file(&path, window, cx);
    }

    // --- Ajustes rápidos (la cara) ---------------------------------------------

    /// Una ficha por acción, como las de Sistema (Silencio, Café…): ícono y,
    /// abajo, cómo está (el idioma, el tema, el color…). La de Actualizar va
    /// clara cuando hay una versión nueva. En el bloque angosto de un costado
    /// van de a dos por fila.
    fn quick_body(&self, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        use crate::updater::Status;
        let (text, muted, m3) = (c.text, c.muted, c.m3);
        let fill: Hsla = rgb(0xe8e8e0).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let track = gpui::white().opacity(0.09);
        let status = crate::updater::status();
        let checking = self.tool_peek.data.update_asked || matches!(status, Status::Checking);
        let skin = crate::appearance::current().skin;
        let theme = theme_side(&self.tool_peek.data.theme);
        let tiles: Vec<AnyElement> = FACE_ACTIONS
            .iter()
            .enumerate()
            .map(|(i, &action)| {
                // El ícono, cómo está y qué hace un clic (el globito).
                let (icon, label, about): (&'static str, gpui::SharedString, &'static str) = match action {
                    FaceAction::WhatsNew => ("icons/sparkles.svg", t("pill.peeks.faceWhatsNew").into(), t("pill.peeks.faceWhatsNewHint")),
                    FaceAction::Update => match &status {
                        Status::Installing => ("icons/download.svg", t("pill.peeks.faceInstalling").into(), t("pill.peeks.faceInstallHint")),
                        Status::Available(update) => (
                            "icons/download.svg",
                            crate::i18n::tf("pill.peeks.faceInstall", &[("version", &update.version)]).into(),
                            t("pill.peeks.faceInstallHint"),
                        ),
                        _ if checking => ("icons/rotate-cw.svg", t("about.checking").into(), t("pill.peeks.faceCheckHint")),
                        Status::UpToDate => ("icons/circle-check.svg", t("pill.peeks.faceUpToDate").into(), t("pill.peeks.faceCheckHint")),
                        Status::Failed(_) => ("icons/rotate-cw.svg", t("pill.peeks.faceRetry").into(), t("pill.peeks.faceCheckHint")),
                        _ => ("icons/rotate-cw.svg", t("pill.peeks.faceCheck").into(), t("pill.peeks.faceCheckHint")),
                    },
                    FaceAction::Language => (
                        "icons/languages.svg",
                        if crate::i18n::english() { t("settings.language.en") } else { t("settings.language.es") }.into(),
                        t("pill.peeks.faceLanguageHint"),
                    ),
                    FaceAction::Theme => match theme {
                        "light" => ("icons/sun.svg", t("pill.settings.themeLight").into(), t("pill.peeks.faceThemeHint")),
                        "dark" => ("icons/moon.svg", t("pill.settings.themeDark").into(), t("pill.peeks.faceThemeHint")),
                        _ => ("icons/monitor.svg", t("pill.peeks.faceThemeSystem").into(), t("pill.peeks.faceThemeHint")),
                    },
                    FaceAction::Color => ("", crate::appearance::skin_name(skin).into(), t("pill.peeks.faceColorHint")),
                    FaceAction::Quiet => ("icons/eye-off.svg", t("pill.settings.quiet").into(), t("pill.peeks.faceQuietHint")),
                    FaceAction::EditNotch => ("icons/ruler.svg", t("pill.peeks.faceEditNotch").into(), t("pill.peeks.faceEditNotchHint")),
                };
                let on = (action == FaceAction::Update && matches!(status, Status::Available(_)))
                    || (action == FaceAction::Quiet && self.quiet_on());
                // En Expressive el tema oscuro también se ve elegido, y la
                // actualización disponible pide atención (contenedor terciario).
                let chosen = on || (m3.is_some() && action == FaceAction::Theme && theme == "dark");
                let attention = action == FaceAction::Update && on;
                let e = m.enter(i);
                let click = cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    pill.face_action(action, window, cx);
                });
                hover_fx_cx(("peek-quick-fx", i), move |h, window, cx| {
                    // En Expressive: un botón de ícono grande (24 px) con la
                    // etiqueta debajo; redondo, y encendido (o pidiendo atención)
                    // cuadrado redondeado, con el resorte SPATIAL_FAST; apretado
                    // se cuadra más.
                    if let Some(s) = m3 {
                        let lit = chosen || attention;
                        let (bg, fg) = if attention {
                            (s.tertiary_container, s.on_tertiary_container)
                        } else if chosen {
                            (s.secondary_container, s.on_secondary_container)
                        } else {
                            (s.surface_container_highest, s.on_surface)
                        };
                        let radius = m3_press_radius(h, if lit { 14.0 } else { QUICK_M3_BUTTON / 2.0 }, 10.0, window, cx);
                        let glyph = if action == FaceAction::Color {
                            div()
                                .size(px(24.))
                                .rounded_full()
                                .bg(skin.background(1.0))
                                .border_1()
                                .border_color(s.outline)
                                .into_any_element()
                        } else {
                            svg().path(crate::icons::themed(icon)).size(px(24.)).text_color(fg).into_any_element()
                        };
                        return div()
                            .id(("peek-quick", i))
                            .flex_1()
                            .min_w_0()
                            .h(px(QUICK_M3_TILE_H))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(6.))
                            .relative()
                            .top(px(-7.0 * (1.0 - e)))
                            .opacity(e)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, click)
                            // Clic secundario en «Color»: Apariencia, como en Atic.
                            .when(action == FaceAction::Color, |el| {
                                el.on_mouse_down(MouseButton::Right, |_, _, cx| {
                                    cx.stop_propagation();
                                    cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Appearance, cx));
                                })
                            })
                            .tooltip(crate::hover::tip(about))
                            .child(
                                div()
                                    .w(px(QUICK_M3_BUTTON + 4.0))
                                    .max_w_full()
                                    .h(px(QUICK_M3_BUTTON))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(radius))
                                    .bg(m3_layer(bg, fg, h))
                                    .child(glyph),
                            )
                            .child(
                                div()
                                    .max_w_full()
                                    .truncate()
                                    .type_style(type_scale::LABEL_SMALL)
                                    .text_color(if lit { s.on_surface } else { s.on_surface_variant })
                                    .child(label),
                            )
                            .into_any_element();
                    }
                    let _ = (window, cx);
                    // Encendida no cambia de color con el cursor: se aclara apenas.
                    let (surface, fg, label_fg, radius) = {
                        let surface = if on {
                            h.mix(fill, fill.opacity(0.86))
                        } else {
                            h.mix(track, gpui::white().opacity(0.14))
                        };
                        let fg = if on { ink } else { text };
                        (surface, fg, if on { ink } else { h.mix(muted, fg) }, 16.)
                    };
                    // Al apretar se hunde un poco, como en Sistema.
                    let inset = 2.5 * h.press;
                    // El color de la pill se muestra tal cual: una muestra de su piel.
                    let glyph = if action == FaceAction::Color {
                        div()
                            .size(px(18.))
                            .rounded_full()
                            .bg(skin.background(1.0))
                            .border_1()
                            .border_color(m3.map_or(gpui::white().opacity(0.28), |s| s.outline))
                            .into_any_element()
                    } else {
                        svg().path(crate::icons::themed(icon)).size(px(18.)).text_color(fg).into_any_element()
                    };
                    div()
                        .id(("peek-quick", i))
                        .flex_1()
                        .min_w_0()
                        .h(px(QUICK_TILE_H))
                        .relative()
                        .top(px(-7.0 * (1.0 - e)))
                        .opacity(e)
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, click)
                        // Clic secundario en «Color»: Apariencia, para elegir uno propio.
                        .when(action == FaceAction::Color, |el| {
                            el.on_mouse_down(MouseButton::Right, |_, _, cx| {
                                cx.stop_propagation();
                                cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Appearance, cx));
                            })
                        })
                        .tooltip(crate::hover::tip(about))
                        .child(
                            div()
                                .absolute()
                                .inset(px(inset))
                                .px(px(6.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(5.))
                                .rounded(px(radius - inset))
                                .bg(surface)
                                .child(glyph)
                                .child(
                                    div()
                                        .max_w_full()
                                        .truncate()
                                        .small_text(10.)
                                        .text_color(label_fg)
                                        .child(label),
                                ),
                        )
                        .into_any_element()
                })
                .into_any_element()
            })
            .collect();
        let per_row = face_per_row(narrow);
        let mut tiles = tiles.into_iter().peekable();
        let mut rows = Vec::new();
        while tiles.peek().is_some() {
            let mut row: Vec<AnyElement> = tiles.by_ref().take(per_row).collect();
            // La última fila corta no se estira: las fichas quedan del mismo ancho.
            while row.len() < per_row {
                row.push(div().flex_1().into_any_element());
            }
            rows.push(div().flex().gap(px(QUICK_GAP)).children(row));
        }
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .gap(px(QUICK_GAP))
            .children(rows)
            .into_any_element()
    }

    fn face_action(&mut self, action: FaceAction, _: &mut Window, cx: &mut Context<Self>) {
        use crate::updater::Status;
        let dirs = atic_core::AppDirs::new().ok();
        match action {
            FaceAction::WhatsNew => {
                self.close_tool_peek(Instant::now(), cx);
                cx.defer(|cx| crate::whats_new::open(cx));
            }
            FaceAction::Update => match crate::updater::status() {
                // Lo mismo que en Ajustes: descarga, comprueba y abre el
                // instalador; la pill se cierra y el instalador la reabre.
                Status::Available(update) => {
                    cx.spawn(async move |pill, cx| {
                        let restart = cx.background_spawn(async move { crate::updater::install(&update) }).await;
                        let _ = pill.update(cx, |_, cx| {
                            if restart {
                                cx.quit();
                            } else {
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                    // `install` pasa a «Instalando» en su hilo: se mira de nuevo enseguida.
                    cx.spawn(async move |pill, cx| {
                        cx.background_executor().timer(Duration::from_millis(150)).await;
                        let _ = pill.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                }
                Status::Checking | Status::Installing => {}
                _ => {
                    self.tool_peek.data.update_asked = true;
                    cx.spawn(async move |pill, cx| {
                        cx.background_spawn(async { crate::updater::check_now() }).await;
                        let _ = pill.update(cx, |pill, cx| {
                            pill.tool_peek.data.update_asked = false;
                            cx.notify();
                        });
                    })
                    .detach();
                }
            },
            FaceAction::Language => {
                let to = if crate::i18n::english() { "es" } else { "en" };
                if let Some(dirs) = &dirs {
                    crate::meetings::settings::save_config(&dirs.config_path(), |cfg| cfg.ui_language = to.into());
                }
                // Al tiro, sin esperar a que `config_watch` lo vea.
                crate::i18n::set_language(to);
                cx.refresh_windows();
            }
            FaceAction::Theme => {
                let to = next_theme(&self.tool_peek.data.theme);
                if let Some(dirs) = &dirs {
                    crate::meetings::settings::save_config(&dirs.config_path(), |cfg| cfg.ui_theme = to.into());
                }
                crate::theme::set_ui_theme(to);
                self.tool_peek.data.theme = to.into();
                cx.refresh_windows();
            }
            FaceAction::Color => {
                crate::appearance::next_skin();
                // La pill no se redibuja sola en reposo.
                cx.refresh_windows();
            }
            FaceAction::Quiet => {
                let on = !self.quiet_on();
                self.set_quiet(on, cx);
                cx.refresh_windows();
            }
            FaceAction::EditNotch => {
                self.close_tool_peek(Instant::now(), cx);
                self.start_resize(cx);
                cx.refresh_windows();
            }
        }
        cx.notify();
    }

    // --- Flip ---------------------------------------------------------------

    fn pages_body(&self, width: f32, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let pages = self.tool_peek.data.pages.clone();
        if let (Some(s), false) = (c.m3, narrow) {
            return self.pages_body_m3(pages, width, c.text, s, m, cx);
        }
        // Angosto: una página por fila, la miniatura a la izquierda.
        let (w, h) = if narrow { PAGE_THUMB } else { page_size(width) };
        let (text, muted, faint, m3) = (c.text, c.muted, c.faint, c.m3);
        let cells = pages.into_iter().take(MAX_PAGES).enumerate().map(|(i, page)| {
            let summary = page_summary(&page);
            div()
                .id(("peek-page", i))
                .w(px(w))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.))
                .when(narrow, |el| el.w_full().flex_row().items_center().gap(px(10.)).h(px(PAGE_ROW_H - 8.0)))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        crate::flip_board::OPEN_PAGE.store(i, std::sync::atomic::Ordering::Relaxed);
                        pill.open_from_peek(crate::flip::TOOL, window, cx);
                    }),
                )
                .fx_cx(("peek-page-fx", i), move |el, hv, window, cx| {
                    // Una hoja en miniatura con sus bloques donde están. Al
                    // abrir cae a su sitio y sus trazos se dibujan solos.
                    // En Expressive, una tarjeta de M3: esquinas M que se
                    // cuadran al apretar, capa de estado y el número en un
                    // distintivo (la primera, en el contenedor primario).
                    let radius = match m3 {
                        Some(_) => m3_press_radius(hv, 12.0, 6.0, window, cx),
                        None => 7.0,
                    };
                    let e = m.enter(i);
                    let drawn = ease_smooth_out(segment(m.t, 0.18 + i as f32 * 0.08, 0.9));
                    let el = el.relative().top(px(-8.0 * (1.0 - e))).opacity(e);
                    el.child(
                        div()
                            .w(px(w))
                            .h(px(h))
                            .flex_none()
                            .relative()
                            .rounded(px(radius))
                            .overflow_hidden()
                            .map(|el| match m3 {
                                Some(s) => el.bg(m3_layer(s.surface_container_high, s.on_surface, hv)),
                                None => el
                                    .bg(hv.mix(text.opacity(0.07), text.opacity(0.11)))
                                    .border_1()
                                    .border_color(text.opacity(0.10 + 0.3 * hv.t)),
                            })
                            .children(page_marks(&page, (w, h), text, drawn))
                            .child(match m3 {
                                Some(s) => {
                                    let (bg, fg) = if i == 0 { (s.primary_container, s.on_primary_container) } else { (s.secondary_container, s.on_secondary_container) };
                                    tabular(
                                        div()
                                            .absolute()
                                            .top(px(4.))
                                            .left(px(4.))
                                            .size(px(18.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .bg(bg)
                                            .text_color(fg)
                                            .type_style(type_scale::LABEL_SMALL)
                                            .child(format!("{}", i + 1)),
                                    )
                                }
                                None => div()
                                    .absolute()
                                    .top(px(4.))
                                    .left(px(6.))
                                    .text_size(px(9.5))
                                    .text_color(muted)
                                    .child(format!("{}", i + 1)),
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .when(narrow, |el| el.flex_1().min_w_0())
                            .flex()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .map(|el| match m3 {
                                        Some(s) => el
                                            .type_style(if narrow { type_scale::BODY_SMALL } else { type_scale::LABEL_MEDIUM })
                                            .text_color(hv.mix(s.on_surface_variant, s.on_surface)),
                                        None => el.text_size(px(if narrow { 12. } else { 10.5 })).text_color(hv.mix(faint, text)),
                                    })
                                    .child(summary),
                            ),
                    )
                })
                .into_any_element()
        });
        div()
            .px(px(SIDE))
            .flex()
            .gap(px(PAGE_GAP))
            .when(narrow, |el| el.flex_col().gap(px(8.)))
            .children(cells)
            .into_any_element()
    }

    /// Flip en Expressive: las páginas en un carrusel multi-browse de M3 (una
    /// grande, una mediana y una chica que asoma). Cada una es la hoja en
    /// miniatura con su número y, encima, su primera línea en un chip que se
    /// apaga al achicarse. Se recorre con la rueda, el trackpad de lado o
    /// arrastrando; un clic abre la página (la chica, la trae).
    fn pages_body_m3(&self, pages: Vec<crate::flip_board::PageGlance>, width: f32, text: Hsla, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let container = width - SIDE * 2.0;
        let reel = &self.tool_peek.pages_reel;
        reel.container.set(container);
        let natural = reel_keylines(false, container)[0];
        let pos = reel.pos.value(Instant::now());
        let h = PAGES_M3_H;
        let cells: Vec<AnyElement> = reel_slots(false, container, pages.len().min(REEL_PAGES), pos)
            .into_iter()
            .filter_map(|(i, start, extent)| {
                let page = pages.get(i)?.clone();
                let summary = page_summary(&page);
                // La grande y la mediana se leen y abren; la chica asoma.
                let act = extent >= natural * 0.5;
                let label = ((extent / natural.max(1.0) - 0.6) / 0.4).clamp(0.0, 1.0);
                let press = cx.listener(move |pill, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let origin = (f32::from(event.position.x), f32::from(event.position.y));
                    pill.tool_peek.reel_press = Some(ReelPress { kind: ReelKind::Pages, index: i, origin, path: None, act });
                });
                Some(
                    hover_fx_cx(("peek-page-fx", i), move |hv, window, cx| {
                        let radius = m3_press_radius(hv, REEL_RADIUS, 12.0, window, cx).min(extent / 2.0);
                        let e = m.enter(i);
                        let drawn = ease_smooth_out(segment(m.t, 0.18 + i as f32 * 0.08, 0.9));
                        let (badge_bg, badge_fg) = if i == 0 { (s.primary_container, s.on_primary_container) } else { (s.secondary_container, s.on_secondary_container) };
                        div()
                            .id(("peek-page", i))
                            .absolute()
                            .left(px(start))
                            .top(px(-8.0 * (1.0 - e)))
                            .w(px(extent))
                            .h(px(h))
                            .opacity(e)
                            .rounded(px(radius))
                            .overflow_hidden()
                            .bg(m3_layer(s.surface_container_high, s.on_surface, hv))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, press)
                            // La hoja va al tamaño del foco, desde el borde del
                            // ítem: al achicarse se recorta, no se deforma.
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .w(px(natural))
                                    .h(px(h))
                                    .children(page_marks(&page, (natural, h), text, drawn)),
                            )
                            .child(tabular(
                                div()
                                    .absolute()
                                    .top(px(10.))
                                    .left(px(10.))
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(badge_bg)
                                    .text_color(badge_fg)
                                    .type_style(type_scale::LABEL_MEDIUM)
                                    .child(format!("{}", i + 1)),
                            ))
                            .when(label > 0.01, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .left(px(10.))
                                        .bottom(px(10.))
                                        .max_w(px((extent - 20.0).max(0.0)))
                                        .h(px(24.))
                                        .px(px(8.))
                                        .flex()
                                        .items_center()
                                        .rounded_full()
                                        .opacity(label)
                                        .bg(s.surface_container_highest)
                                        .text_color(s.on_surface)
                                        .type_style(type_scale::LABEL_MEDIUM)
                                        .child(div().truncate().child(summary)),
                                )
                            })
                            .into_any_element()
                    })
                    .into_any_element(),
                )
            })
            .collect();
        self.reel_strip(ReelKind::Pages, container, h, cells, cx)
    }

    // --- Computadores --------------------------------------------------------

    /// Computadores: en qué anda (con el interruptor para prender o apagar),
    /// el código grande si otro equipo pide vincularse, y cada equipo con
    /// «Usar este mouse y teclado en él» y el dibujo de los lados. Buscar y
    /// vincular siguen en Ajustes (el pie lleva allá).
    fn computers_body(&self, narrow: bool, c: &Colors, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        if let Some(s) = c.m3 {
            return self.computers_body_m3(narrow, c, s, m, cx);
        }
        use crate::computers::{self, Headline, Notice};
        use crate::i18n::tf;
        use crate::phone_setup::{chip, switch, Ink};
        let s = &self.tool_peek.kvm;
        let ink = Ink { text: c.text, muted: c.muted, faint: c.faint, ok: c.ok, danger: rgb(0xe8826a).into() };
        let (text, muted, faint) = (c.text, c.muted, c.faint);
        let blue: Hsla = rgb(computers::TAB_BLUE).into();
        let (line, dot) = match s.headline() {
            Headline::Off => (t("pill.kvm.peekOff").to_string(), faint),
            Headline::Starting => (t("pill.kvm.starting").to_string(), faint),
            Headline::NoPeers => (t("pill.kvm.peekNone").to_string(), faint),
            Headline::Offline => (t("pill.kvm.peekOffline").to_string(), rgb(computers::TAB_GRAY).into()),
            Headline::Connected(name) => (tf("pill.kvm.peekConnected", &[("name", &name)]), c.ok),
            Headline::Away(name) => (tf("pill.kvm.peekAway", &[("name", &name)]), blue),
            Headline::Controlled(name) => (tf("pill.kvm.peekControlled", &[("name", &name)]), blue),
        };
        let on = s.enabled;
        let status = div()
            .h(px(KVM_STATUS_H))
            .px(px(SIDE))
            .flex()
            .items_center()
            .gap(px(8.))
            .opacity(m.enter(0))
            .child(div().size(px(7.)).flex_none().rounded_full().bg(dot))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(if on { text } else { muted }).child(line))
            .child(
                div()
                    .id("peek-kvm-power")
                    .flex_none()
                    .h(px(26.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .rounded(px(13.))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(t("pill.kvm.enable")))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            computers::set_enabled(!on);
                            pill.tool_peek.read_kvm();
                            cx.notify();
                        }),
                    )
                    .child(switch(on, ink))
                    .hover_bg("peek-kvm-power-fx", text.opacity(0.0), text.opacity(0.06)),
            );

        // Otro equipo pide vincularse: el código va grande, para escribirlo allá.
        let code = s.pair_code.as_ref().map(|(name, code)| {
            div()
                .h(px(KVM_CODE_H - 6.))
                .mx(px(SIDE))
                .mb(px(6.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(1.))
                .rounded(px(12.))
                .bg(text.opacity(0.08))
                .child(
                    div()
                        .max_w_full()
                        .px(px(10.))
                        .truncate()
                        .text_size(px(11.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(tf("pill.kvm.codeTitle", &[("name", name)])),
                )
                .child(
                    div()
                        .text_size(px(30.))
                        .line_height(px(38.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .font_family(crate::fonts::MONO)
                        .text_color(text)
                        .child(crate::computers_notch::spaced_code(code)),
                )
                .child(div().text_size(px(10.5)).text_color(muted).child(t("pill.kvm.codeHint")))
        });

        let notice = s.notice.as_ref().map(|notice| {
            let (line, color, permissions) = match notice {
                Notice::Permission => (t("pill.kvm.permissionShort").to_string(), ink.danger, cfg!(target_os = "macos")),
                Notice::Info(text) => (text.clone(), muted, false),
                Notice::Error(text) => (text.clone(), ink.danger, false),
            };
            div()
                .h(px(KVM_NOTE_H))
                .px(px(SIDE))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(color).child(line))
                .when(permissions, |el| {
                    el.child(chip("peek-kvm-permissions", None, t("pill.kvm.openPermissions"), true, false, ink, |_, _, cx| {
                        computers::dismiss_notice();
                        cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Permissions, cx));
                    }))
                })
        });

        // Prendido y sin equipos: vincular uno, en Ajustes.
        let pair = (s.enabled && s.running && s.peers.is_empty()).then(|| {
            div()
                .id("peek-kvm-pair")
                .h(px(EMPTY_H))
                .mx(px(SIDE - 8.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(10.))
                .cursor_pointer()
                .opacity(m.enter(1))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.close_tool_peek(Instant::now(), cx);
                        cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Computers, cx));
                    }),
                )
                .fx("peek-kvm-pair-fx", move |el, h| {
                    let fg = h.mix(muted, text);
                    el.bg(text.opacity(0.08 * h.t))
                        .text_size(px(12.))
                        .text_color(fg)
                        .child(svg().path(crate::icons::themed("icons/plus.svg")).size(px(13.)).text_color(fg))
                        .child(t("pill.kvm.peekPair"))
                })
        });

        // Tocar un lado vuelve a leer el estado, como el interruptor.
        let pill = cx.entity().downgrade();
        let after: std::rc::Rc<dyn Fn(&mut gpui::App)> = std::rc::Rc::new(move |cx| {
            let _ = pill.update(cx, |pill, cx| {
                pill.tool_peek.read_kvm();
                cx.notify();
            });
        });
        let peer_h = ToolPeek::kvm_peer_h(narrow);
        let peers = self.tool_peek.kvm_peers(narrow);
        let rows = peers.iter().enumerate().map(|(i, peer)| {
            let e = m.enter(i + 1);
            let state = match (peer.connected, peer.mac) {
                (true, true) => t("pill.kvm.connectedMac"),
                (true, false) => t("pill.kvm.connectedPc"),
                (false, _) => t("pill.kvm.offlineShort"),
            };
            let who = div()
                .h(px(KVM_WHO_H))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .relative()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(text.opacity(0.10))
                        .child(svg().path(crate::icons::themed(if peer.mac { "icons/laptop.svg" } else { "icons/monitor.svg" })).size(px(12.)).text_color(text))
                        // El punto de conectado, en la esquina.
                        .child(
                            div()
                                .absolute()
                                .right(px(-1.))
                                .bottom(px(-1.))
                                .size(px(8.))
                                .rounded_full()
                                .bg(if peer.connected { c.ok } else { faint }),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(div().truncate().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child(peer.name.clone()))
                        .child(div().truncate().text_size(px(10.5)).text_color(if peer.connected { muted } else { faint }).child(state)),
                );
            let sharing = peer.sharing;
            let id = peer.id.clone();
            let hint: gpui::SharedString = match (sharing, peer.side) {
                (true, Some(side)) => tf("pill.kvm.sharingHint", &[("side", &crate::computers_settings::side_edge(side))]).into(),
                _ => t("pill.kvm.shareHint").into(),
            };
            let share = div()
                .id(("peek-kvm-share", i))
                .h(px(KVM_SHARE_H))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(10.))
                .cursor_pointer()
                .tooltip(crate::hover::tip_text(hint))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        computers::set_sharing(&id, !sharing);
                        pill.tool_peek.read_kvm();
                        cx.notify();
                    }),
                )
                .child(svg().path(crate::icons::themed("icons/mouse-pointer-2.svg")).size(px(14.)).flex_none().text_color(if sharing { text } else { muted }))
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(t("pill.kvm.share")))
                .child(switch(sharing, ink))
                .hover_bg(("peek-kvm-share-fx", i), text.opacity(0.0), text.opacity(0.06));
            let picker = crate::computers_notch::side_picker(&format!("peek-kvm-side-{i}"), peer, true, ink, after.clone());
            let column = div().min_w_0().flex().flex_col().child(who).child(share);
            let drop_id = peer.id.clone();
            let row = div()
                .relative()
                .top(px(-7.0 * (1.0 - e)))
                .opacity(e)
                .h(px(peer_h))
                .mx(px(SIDE - 8.))
                .rounded(px(12.))
                // Soltar archivos sobre un equipo los manda a sus Descargas.
                .drag_over::<gpui::ExternalPaths>(move |style, _, _, _| style.bg(text.opacity(0.08)))
                .on_drop(cx.listener(move |pill, paths: &gpui::ExternalPaths, _, cx| {
                    crate::computers::send_files(&drop_id, paths.paths().to_vec());
                    pill.tool_peek.read_kvm();
                    cx.notify();
                }));
            if narrow {
                // En un costado no caben lado a lado: el dibujo va debajo.
                row.flex().flex_col().items_center().gap(px(KVM_PEER_GAP)).child(column.w_full()).child(picker)
            } else {
                row.flex().items_center().gap(px(10.)).child(column.flex_1()).child(picker)
            }
        });

        div()
            .flex()
            .flex_col()
            .child(status)
            .children(code)
            .children(notice)
            .children(pair)
            .children(rows.collect::<Vec<_>>())
            .into_any_element()
    }

    // --- Sistema ------------------------------------------------------------

    /// Pomodoro: los tres modos (el de ahora marcado), el tiempo que queda en
    /// grande con su barra, y reiniciar, empezar o pausar, y saltar.
    fn pomodoro_body(&self, c: &Colors, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        use crate::pomodoro::{format, Mode};
        let (text, muted, m3) = (c.text, c.muted, c.m3);
        let tomato: Hsla = rgb(crate::POMODORO_RED).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let pomo = &self.pomodoro;
        let current = pomo.mode();
        let modes = Mode::ALL.iter().map(|&mode| {
            let on = mode == current;
            let id = gpui::SharedString::from(format!("pomodoro-mode-{mode:?}"));
            div()
                .id(id.clone())
                .h(px(POMO_MODES_H))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(POMO_MODES_H / 2.0))
                .text_size(px(11.5))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.pomodoro.set_mode(mode);
                        pill.sync_pomodoro(Instant::now(), cx);
                        cx.notify();
                    }),
                )
                .child(mode.label())
                .fx(id, move |el, h| {
                    // En Expressive, chips de filtro de M3: el elegido en el
                    // contenedor secundario, los otros con contorno.
                    if let Some(s) = m3 {
                        return if on {
                            el.text_color(s.on_secondary_container).bg(state_layer(s.secondary_container, s.on_secondary_container, h))
                        } else {
                            el.text_color(s.on_surface_variant)
                                .bg(s.on_surface_variant.opacity(layer_alpha(h)))
                                .border_1()
                                .border_color(s.outline_variant)
                        };
                    }
                    if on {
                        el.text_color(text).bg(text.opacity(0.14))
                    } else {
                        el.text_color(h.mix(muted, text)).bg(text.opacity(0.07 * h.t))
                    }
                })
        });
        let button = |id: &'static str, icon: &'static str, big: bool, act: fn(&mut crate::Pill, &mut Context<crate::Pill>), cx: &mut Context<Self>| {
            let size = if big { POMO_BUTTONS_H } else { 32.0 };
            div()
                .id(id)
                .size(px(size))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        act(pill, cx);
                        pill.sync_pomodoro(Instant::now(), cx);
                        cx.notify();
                    }),
                )
                .fx(id, move |el, h| {
                    // En Expressive: play lleno (primary) y los otros tonales.
                    if let Some(s) = m3 {
                        let (bg, fg, side) = if big {
                            (state_layer(s.primary, s.on_primary, h), s.on_primary, 17.)
                        } else {
                            (state_layer(s.secondary_container, s.on_secondary_container, h), s.on_secondary_container, 14.)
                        };
                        return el.bg(bg).child(svg().path(crate::icons::themed(icon)).size(px(side)).text_color(fg));
                    }
                    if big {
                        el.bg(text.opacity(0.88 + 0.12 * h.t)).child(svg().path(crate::icons::themed(icon)).size(px(17.)).text_color(ink))
                    } else {
                        el.bg(text.opacity(0.06 + 0.08 * h.t)).child(svg().path(crate::icons::themed(icon)).size(px(14.)).text_color(h.mix(muted, text)))
                    }
                })
        };
        let play = if pomo.running() { "icons/pause.svg" } else { "icons/play.svg" };
        let progress = pomo.progress(now);
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .child(div().h(px(POMO_MODES_H)).flex().gap(px(4.)).children(modes.collect::<Vec<_>>()))
            .child(div().h(px(POMO_GAP)))
            .child(
                div()
                    .h(px(POMO_TIME_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(crate::fonts::MONO)
                    .text_size(px(38.))
                    .text_color(if pomo.running() { text } else { muted })
                    .child(format(pomo.remaining(now))),
            )
            .child(
                div()
                    .h(px(POMO_BAR_H))
                    .rounded_full()
                    .bg(m3.map_or(text.opacity(0.1), |s| s.surface_container_highest))
                    .child(div().h_full().w(gpui::relative(progress)).rounded_full().bg(tomato)),
            )
            .child(div().h(px(POMO_GAP)))
            .child(
                div()
                    .h(px(POMO_BUTTONS_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(16.))
                    .child(button("pomodoro-reset", "icons/rotate-ccw.svg", false, |pill, _| pill.pomodoro.reset(), cx))
                    .child(button("pomodoro-play", play, true, |pill, cx| pill.toggle_pomodoro(cx), cx))
                    .child(button("pomodoro-skip", "icons/skip-forward.svg", false, |pill, _| pill.pomodoro.skip(Instant::now()), cx)),
            )
            .into_any_element()
    }

    fn system_body(&self, c: &Colors, m: Motion, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        if let Some(s) = c.m3 {
            return self.system_body_m3(s, m, now, cx);
        }
        let snap = self.system.read(cx).snapshot().clone();
        let procs = snap.procs.clone();
        let ram_total = procs.as_ref().map(|p| p.ram_total).unwrap_or(0);
        let gb = |b: u64| b as f64 / 1024.0 / 1024.0 / 1024.0;
        // Cuentan desde cero al abrir y entre lecturas se deslizan.
        let count = m.count();
        let cpu = self.tool_peek.cpu.value(now) * 100.0 * count;
        let ram = self.tool_peek.ram.value(now) * count;
        let ram_used = (ram_total as f64 * ram as f64) as u64;
        // En Expressive, el indicador lineal de M3: relleno en el acento sobre
        // su pista, y la alerta en el aviso del esquema.
        let (warn, fine, track): (Hsla, Hsla, Hsla) = match c.m3 {
            Some(s) => (s.warning, s.primary, s.surface_container_highest),
            None => (rgb(0xd4a84b).into(), c.ok, c.text.opacity(0.12)),
        };
        let meter = |i: usize, label: &'static str, value: f32, shown: String, alert: f32| {
            let color = if value >= alert { warn } else { fine };
            let e = m.enter(i);
            div()
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(METER_H))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.))
                .child(div().w(px(66.)).text_color(c.muted).child(label))
                .child(
                    div()
                        .flex_1()
                        .h(px(5.))
                        .rounded(px(3.))
                        .bg(track)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(3.))
                                .bg(color)
                                .w(gpui::relative(value.clamp(0.02, 1.0))),
                        ),
                )
                .child(div().w(px(70.)).flex().justify_end().child(shown))
        };
        let measured = procs.is_some();
        let apps: Vec<_> = procs.map(|p| p.apps).unwrap_or_default();
        let mut apps = apps;
        apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        let rows = apps.into_iter().take(3).enumerate().map(|(i, app)| {
            let e = m.enter(i + 3);
            let icon = self.system.update(cx, |panel, cx| panel.app_image(app.path.as_ref(), cx));
            let ram = if app.ram >= 1 << 30 {
                format!("{:.1} GB", gb(app.ram))
            } else {
                format!("{} MB", app.ram >> 20)
            };
            div()
                .id(("peek-app", i))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(APP_ROW_H))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.))
                .child(match icon {
                    Some(icon) => img(icon).size(px(16.)).flex_none().into_any_element(),
                    None => div().size(px(16.)).flex_none().rounded(px(4.)).bg(c.text.opacity(0.10)).into_any_element(),
                })
                .child(div().flex_1().min_w_0().truncate().text_color(c.muted).child(app.name.clone()))
                .child(div().w(px(56.)).flex().justify_end().text_color(c.faint).child(ram))
                .child(div().w(px(44.)).flex().justify_end().child(format!("{:.0} %", app.cpu * count)))
                .into_any_element()
        });
        let battery = snap.battery.map(|(p, plugged)| {
            if plugged {
                crate::i18n::tf("pill.peeks.batteryPlugged", &[("p", &p)])
            } else {
                crate::i18n::tf("pill.peeks.battery", &[("p", &p)])
            }
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .child(meter(
                0,
                "CPU",
                cpu / 100.0,
                if measured { format!("{cpu:.0} %") } else { "—".into() },
                0.85,
            ))
            .child(meter(
                1,
                t("pill.peek.ram"),
                ram,
                if measured { crate::i18n::tf("pill.peeks.memUsed", &[("used", &format!("{:.1}", gb(ram_used))), ("total", &format!("{:.0}", gb(ram_total)))]) } else { "—".into() },
                0.90,
            ))
            .child(div().h(px(6.)))
            .child(
                div()
                    .h(px(SECTION_H))
                    .flex()
                    .items_center()
                    .small_text(10.5)
                    .text_color(c.faint)
                    .opacity(m.enter(2))
                    .child(t("pill.peeks.topConsumers"))
                    .child(div().flex_1())
                    .children(battery),
            )
            .children(rows)
            .into_any_element()
    }
}

// --- M3 Expressive ------------------------------------------------------------
//
// El interior de cada vistazo en M3 Expressive: un héroe por vistazo (la acción
// o el dato principal, en un contenedor tonal grande con forma expresiva), la
// escala tipográfica de M3 con números tabulares, capa de estado en todo lo
// clicable y esquinas que se cuadran al apretar (`SPATIAL_FAST`). Los clics son
// los de Atic (`on_mouse_down`); la entrada, `Motion::enter` (resorte de
// efectos desde `shown_at`, así el carrusel y el relevo al panel no la repiten).

impl Pill {
    /// El estado vacío de un vistazo: una forma de M3 quieta con su ícono, el
    /// título corto («Sin X») y debajo qué hacer («<Verbo> Y»), y a la derecha
    /// la acción, la misma del pie (`open_from_peek`), si hay una.
    #[allow(clippy::too_many_arguments)]
    fn empty_state_m3(
        &self,
        tool: usize,
        shape: ShapeName,
        icon: &'static str,
        title: &'static str,
        hint: &'static str,
        action: Option<&'static str>,
        s: Scheme,
        m: Motion,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let e = m.enter(0);
        let action = action.map(|label| {
            let run = cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                pill.open_from_peek(tool, window, cx);
            });
            hover_fx_cx(("peek-empty-action", tool), move |h, window, cx| {
                let radius = m3_press_radius(h, 16.0, 8.0, window, cx);
                // El blanco del clic, de 40; el botón dibujado, de 32.
                div()
                    .id(("peek-empty-action-hit", tool))
                    .h(px(HIT_M3))
                    .flex()
                    .flex_none()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, run)
                    .child(
                        div()
                            .h(px(32.))
                            .px(px(16.))
                            .flex()
                            .items_center()
                            .rounded(px(radius))
                            .bg(m3_layer(s.primary, s.on_primary, h))
                            .text_color(s.on_primary)
                            .type_style(type_scale::LABEL_LARGE)
                            .child(label),
                    )
                    .into_any_element()
            })
        });
        div()
            .h(px(EMPTY_M3_H))
            .mx(px(SIDE))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .rounded(corner::L)
            .bg(s.surface_container_high)
            .relative()
            .top(px(-6.0 * (1.0 - e)))
            .opacity(e)
            .child(
                gpui_m3::Shape::new(shape)
                    .size(px(40.))
                    .color(s.secondary_container)
                    .child(svg().path(crate::icons::themed(icon)).size(px(18.)).text_color(s.on_secondary_container)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().type_emphasized(type_scale::TITLE_SMALL).text_color(s.on_surface).child(title))
                    .child(div().truncate().type_style(type_scale::BODY_SMALL).text_color(s.on_surface_variant).child(hint)),
            )
            .children(action)
            .into_any_element()
    }

    /// Color: el color que se ve, en una tarjeta grande con su muestra como
    /// forma de M3 (un clic lo copia y la forma se vuelve galleta); debajo, los
    /// recientes como círculos, el de la tarjeta en `Cookie9`.
    fn color_body_m3(&self, colors: Vec<u32>, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let copied = colors.iter().find(|&&color| self.is_copied(&hex(color))).copied();
        let shown = copied
            .or(self.tool_peek.hovered_color.filter(|h| colors.contains(h)))
            .unwrap_or(colors[0]);
        let shown_hex = hex(shown);
        let copy = cx.listener({
            let key = shown_hex.clone();
            move |pill, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                pill.copy_text(key.clone(), cx);
            }
        });
        let e = m.enter(0);
        let hero = hover_fx_cx("peek-color-hero", move |h, window, cx| {
            let radius = m3_press_radius(h, 28.0, 16.0, window, cx);
            div()
                .id("peek-color-hero-card")
                .h(px(COLOR_HERO_H))
                .pl(px(9.))
                .pr(px(16.))
                .flex()
                .items_center()
                .gap(px(14.))
                .rounded(px(radius))
                .bg(m3_layer(s.surface_container_high, s.on_surface, h))
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, copy)
                .child(m3_swatch("peek-color-hero-shape", rgb(shown).into(), copied.is_some(), 46.0, s))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(tabular(div().type_emphasized(type_scale::TITLE_LARGE).text_color(s.on_surface).child(shown_hex)))
                        .child(tabular(div().type_style(type_scale::LABEL_MEDIUM).text_color(s.on_surface_variant).child(rgb_text(shown)))),
                )
                .child(if copied.is_some() {
                    div()
                        .h(px(28.))
                        .pl(px(8.))
                        .pr(px(12.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(4.))
                        .rounded(corner::S)
                        .bg(s.primary_container)
                        .text_color(s.on_primary_container)
                        .type_style(type_scale::LABEL_LARGE)
                        .child(svg().path(crate::icons::themed("icons/check.svg")).size(px(16.)).text_color(s.on_primary_container))
                        .child(t("pill.peek.copied"))
                        .into_any_element()
                } else {
                    svg()
                        .path(crate::icons::themed("icons/copy.svg"))
                        .size(px(18.))
                        .flex_none()
                        .text_color(h.mix(s.on_surface_variant, s.on_surface))
                        .into_any_element()
                })
                .into_any_element()
        });
        let swatches = colors.into_iter().take(10).enumerate().map(|(i, color)| {
            let key = hex(color);
            let on = color == shown;
            div()
                .id(("peek-color", i))
                .size(px(SWATCH))
                .relative()
                .cursor_pointer()
                // La tarjeta se queda con la última muestra que pasó el cursor:
                // así se puede subir a ella para copiarla.
                .on_hover(cx.listener(move |pill, hovered: &bool, _, cx| {
                    if *hovered && pill.tool_peek.hovered_color != Some(color) {
                        pill.tool_peek.hovered_color = Some(color);
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.copy_text(key.clone(), cx);
                    }),
                )
                .fx(("peek-color-fx", i), move |el, h| {
                    // Crece con el cursor sin mover a sus vecinas y apretada se
                    // achica; al abrir, brota desde su centro tras la anterior.
                    let e = m.enter(i + 1);
                    let size = SWATCH * (1.0 + 0.12 * h.t - 0.10 * h.press - 0.45 * (1.0 - e));
                    el.opacity(e).child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(m3_swatch(("peek-color-shape", i), rgb(color).into(), on, size.max(1.0), s)),
                    )
                })
                .into_any_element()
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .gap(px(COLOR_M3_GAP))
            .child(hero)
            .child(div().h(px(SWATCH)).flex().items_center().gap(px(9.)).children(swatches))
            .into_any_element()
    }

    /// Capturas: un carrusel héroe de M3 (la más nueva grande y la siguiente
    /// asomando), con la hora en un chip que se apaga al achicarse. Se recorre
    /// con la rueda, el trackpad de lado o arrastrando, y se acomoda al
    /// soltar. Un clic copia (o, si asomaba, la trae); arrastrarla hacia
    /// abajo la suelta como archivo.
    fn shots_body_m3(&self, shots: Vec<Shot>, width: f32, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let container = width - SIDE * 2.0;
        let reel = &self.tool_peek.shots_reel;
        reel.container.set(container);
        let natural = reel_keylines(true, container)[0];
        let pos = reel.pos.value(Instant::now());
        let cards: Vec<AnyElement> = reel_slots(true, container, shots.len().min(REEL_SHOTS), pos)
            .into_iter()
            .filter_map(|(i, start, extent)| {
                let shot = shots.get(i)?.clone();
                Some(self.shot_card_m3(i, shot, (start, extent), natural, s, m, cx))
            })
            .collect();
        self.reel_strip(ReelKind::Shots, container, SHOTS_M3_H, cards, cx)
    }

    /// La tira de un carrusel: recorta lo que sale por los lados y es dueña
    /// de la rueda y del trackpad que empiezan sobre ella.
    fn reel_strip(&self, kind: ReelKind, container: f32, h: f32, items: Vec<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mx(px(SIDE))
            .w(px(container))
            .h(px(h))
            .relative()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(move |pill, event: &gpui::ScrollWheelEvent, _, cx| {
                if pill.reel_wheel(kind, event) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(items)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn shot_card_m3(&self, i: usize, shot: Shot, (start, extent): (f32, f32), natural: f32, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let key = shot.path.to_string_lossy().to_string();
        let copied = self.is_copied(&key);
        let path = shot.path.clone();
        let h = SHOTS_M3_H;
        // En el foco actúa; la que asoma, con un clic, pasa al foco.
        let act = extent >= natural * 0.75;
        // El chip de la hora se apaga mientras se achica (como el pie de
        // foto del `Carousel` de gpui-m3).
        let label = ((extent / natural.max(1.0) - 0.6) / 0.4).clamp(0.0, 1.0);
        let thumb = if shot.video {
            crate::screen_cursor::thumb_path(&shot.path).filter(|p| p.exists())
        } else {
            crate::clipboard::preview(&shot.path).map(|p| p.to_path_buf())
        };
        let has_thumb = thumb.is_some();
        let press = cx.listener(move |pill, event: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            let origin = (f32::from(event.position.x), f32::from(event.position.y));
            pill.tool_peek.reel_press = Some(ReelPress { kind: ReelKind::Shots, index: i, origin, path: Some(path.clone()), act });
        });
        hover_fx_cx(("peek-shot-fx", i), move |hv, window, cx| {
            let radius = m3_press_radius(hv, REEL_RADIUS, 12.0, window, cx).min(extent / 2.0);
            let e = m.enter(i);
            let (chip_bg, chip_fg) = if copied { (s.primary, s.on_primary) } else { (s.surface_container_highest, s.on_surface) };
            div()
                .id(("peek-shot", i))
                .absolute()
                .left(px(start))
                .top(px(-8.0 * (1.0 - e)))
                .w(px(extent))
                .h(px(h))
                .opacity(e)
                .rounded(px(radius))
                .overflow_hidden()
                .bg(s.surface_container_high)
                .cursor_grab()
                .on_mouse_down(MouseButton::Left, press)
                // La imagen lleva las esquinas de la tarjeta (el recorte de
                // GPUI es rectangular) y sus medidas fijas: dentro del caché de
                // imágenes `size_full` no la limitaba y se salía de la tarjeta.
                // Cubre lo que mide la tarjeta: al achicarse se recorta al
                // centro, sin cambiar de tamaño (va a lo alto).
                .children(thumb.map(|p| {
                    div().absolute().left_0().top_0().w(px(extent)).h(px(h)).child(crate::release::scoped(
                        gpui::SharedString::from(format!("shot-{}", p.display())),
                        img(p).w(px(extent)).h(px(h)).object_fit(ObjectFit::Cover).rounded(px(radius)),
                    ))
                }))
                .when(shot.video && !has_thumb, |el| {
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(svg().path(crate::icons::themed("icons/video.svg")).size(px(24.)).text_color(s.on_surface_variant)),
                    )
                })
                // La capa de estado, encima de la imagen.
                .child(div().absolute().inset_0().rounded(px(radius)).bg(s.on_surface.opacity(0.08 * hv.t + 0.02 * hv.press)))
                .when(label > 0.01, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .bottom(px(10.))
                            .max_w(px((extent - 20.0).max(0.0)))
                            .h(px(24.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded_full()
                            .opacity(label)
                            .bg(chip_bg)
                            .text_color(chip_fg)
                            .type_style(type_scale::LABEL_MEDIUM)
                            .when(shot.video && !copied, |el| el.child(svg().path(crate::icons::themed("icons/play.svg")).size(px(10.)).flex_none().text_color(chip_fg)))
                            .when(copied, |el| el.child(svg().path(crate::icons::themed("icons/check.svg")).size(px(12.)).flex_none().text_color(chip_fg)))
                            .child(tabular(div().truncate().child(if copied { t("pill.peeks.copiedShot").to_string() } else { shot.label.clone() }))),
                    )
                })
                .into_any_element()
        })
        .into_any_element()
    }

    /// Cuántos ítems recorre el carrusel `kind` y si es el héroe.
    fn reel_count(&self, kind: ReelKind) -> (usize, bool) {
        match kind {
            ReelKind::Shots => (self.tool_peek.data.shots.len().min(REEL_SHOTS), true),
            ReelKind::Pages => (self.tool_peek.data.pages.len().min(REEL_PAGES), false),
        }
    }

    fn reel_mut(&mut self, kind: ReelKind) -> &mut Reel {
        match kind {
            ReelKind::Shots => &mut self.tool_peek.shots_reel,
            ReelKind::Pages => &mut self.tool_peek.pages_reel,
        }
    }

    /// La rueda o el trackpad sobre un carrusel. `true` si el gesto es suyo
    /// (no sigue al carrusel de vistazos ni al despliegue): la rueda del mouse
    /// siempre; el trackpad, si empezó sobre él y va de lado (uno vertical, o
    /// la inercia de uno que empezó afuera, sigue de largo).
    fn reel_wheel(&mut self, kind: ReelKind, event: &gpui::ScrollWheelEvent) -> bool {
        let now = Instant::now();
        let (count, hero) = self.reel_count(kind);
        let max = reel_max(hero, count);
        if max <= 0.0 {
            return false;
        }
        let reel = self.reel_mut(kind);
        if reel.drag.is_some() {
            return true;
        }
        let delta = match event.delta {
            gpui::ScrollDelta::Pixels(d) => {
                let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                let fresh = |at: Instant| now.duration_since(at).as_secs_f32() < REEL_GESTURE_GAP;
                if matches!(event.touch_phase, gpui::TouchPhase::Started) {
                    reel.touch = Some(ReelTouch { travel: (0.0, 0.0), axis: None, at: now });
                }
                // El fin del gesto sigue de largo: la pill anota que se soltó
                // (su inercia no despliega nada).
                if matches!(event.touch_phase, gpui::TouchPhase::Ended) {
                    reel.touch = None;
                    return false;
                }
                match reel.touch.as_mut().filter(|touch| fresh(touch.at)) {
                    Some(touch) => {
                        touch.at = now;
                        if touch.axis.is_none() {
                            touch.travel.0 += dx.abs();
                            touch.travel.1 += dy.abs();
                            let (h, v) = touch.travel;
                            if h + v >= AXIS_LOCK {
                                touch.axis = Some(h > v * 1.5);
                            }
                        }
                        if touch.axis != Some(true) {
                            return false;
                        }
                    }
                    // Sin gesto propio: la inercia del que se soltó aquí
                    // sigue; la de uno que empezó afuera, no.
                    None => {
                        reel.touch = None;
                        if !reel.wheel.is_some_and(|(_, at)| fresh(at)) {
                            return false;
                        }
                    }
                }
                dx
            }
            // La rueda del mouse: la vertical también corre el carrusel.
            gpui::ScrollDelta::Lines(d) => 40.0 * if d.x.abs() >= d.y.abs() { d.x } else { d.y },
        };
        let step = reel.step(hero);
        let now_pos = reel.pos.value(now);
        let from = reel.wheel.map_or(now_pos, |(from, _)| from);
        reel.pos.snap((now_pos - delta / step).clamp(0.0, max));
        reel.wheel = Some((from, now));
        // Lo que quede del gesto, aunque salga del carrusel, no pasa de página
        // en el carrusel de vistazos.
        self.swipe.fired = true;
        self.swipe.axis = Some(true);
        true
    }

    /// Cada sondeo: la rueda quieta se acomoda, el arrastre sigue al cursor y
    /// el ítem apretado se resuelve en clic, arrastre del carrusel o, en
    /// Capturas, arrastre del archivo a otra app.
    fn update_reels(&mut self, now: Instant, cursor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        for kind in [ReelKind::Shots, ReelKind::Pages] {
            let (count, hero) = self.reel_count(kind);
            let max = reel_max(hero, count);
            let reel = self.reel_mut(kind);
            if let Some((from, at)) = reel.wheel {
                if now.duration_since(at).as_secs_f32() >= REEL_WHEEL_IDLE {
                    reel.wheel = None;
                    let target = reel_wheel_target(from, reel.pos.value(now), max);
                    reel.pos.set(target, now);
                    cx.notify();
                }
            }
            // Llegó o se fue algo y quedó fuera de rango: vuelve.
            if reel.drag.is_none() && reel.wheel.is_none() {
                let target = reel.pos.target();
                let clamped = target.clamp(0.0, max.max(0.0));
                if (clamped - target).abs() > 1e-3 {
                    reel.pos.set(clamped, now);
                    cx.notify();
                }
            }
        }

        let Some(press) = self.tool_peek.reel_press.clone() else {
            return;
        };
        let kind = press.kind;
        let (count, hero) = self.reel_count(kind);
        let max = reel_max(hero, count);
        let held = crate::win::left_button_down();
        let (dx, dy) = cursor.map_or((0.0, 0.0), |c| (c.0 - press.origin.0, c.1 - press.origin.1));
        if self.reel_mut(kind).drag.is_none() {
            let moved = dx.hypot(dy) >= DRAG_START;
            if moved && kind == ReelKind::Shots && (dy.abs() > dx.abs() || max <= 0.0) {
                // Hacia abajo (o sin nada que recorrer): la captura sale como
                // archivo, como antes.
                self.tool_peek.reel_press = None;
                self.close_tool_peek(now, cx);
                if let Some(path) = press.path {
                    self.drag_shot(path, cx);
                }
                return;
            }
            if moved && max > 0.0 {
                let reel = self.reel_mut(kind);
                reel.wheel = None;
                let pos0 = reel.pos.value(now);
                reel.drag = Some(ReelDrag { x0: press.origin.0, pos0, samples: vec![(now, pos0)] });
            } else if !held {
                self.tool_peek.reel_press = None;
                self.reel_click(press, max, now, cx);
                return;
            } else {
                return;
            }
        }

        let reel = self.reel_mut(kind);
        let step = reel.step(hero);
        if let (Some(drag), Some(c)) = (reel.drag.as_mut(), cursor) {
            let raw = (drag.pos0 + (drag.x0 - c.0) / step).clamp(-REEL_OVERSCROLL, max + REEL_OVERSCROLL);
            let pos = reel_rubber(raw, max);
            drag.samples.push((now, pos));
            if drag.samples.len() > 8 {
                drag.samples.remove(0);
            }
            reel.pos.snap(pos);
            cx.notify();
        }
        let out = kind == ReelKind::Shots && dy.abs() >= SHOT_DRAG_OUT;
        if !held || out {
            let Some(drag) = reel.drag.take() else {
                return;
            };
            let pos = reel.pos.value(now);
            let velocity = drag
                .samples
                .iter()
                .find(|(at, _)| now.duration_since(*at).as_secs_f32() < 0.1)
                .filter(|(at, _)| *at < now)
                .map_or(0.0, |(at, p)| (pos - p) / now.duration_since(*at).as_secs_f32());
            // Saliendo hacia abajo no se lanza: queda en la más cercana.
            let target = reel_snap_target(pos, if out { 0.0 } else { velocity }, max);
            reel.pos.set(target, now);
            self.tool_peek.reel_press = None;
            if out {
                self.close_tool_peek(now, cx);
                if let Some(path) = press.path {
                    self.drag_shot(path, cx);
                }
            }
            cx.notify();
        }
    }

    /// Un ítem de un carrusel soltado sin moverse: si asomaba pasa al foco;
    /// si no, copia la captura (o abre la grabación) o abre la página.
    fn reel_click(&mut self, press: ReelPress, max: f32, now: Instant, cx: &mut Context<Self>) {
        if self.tool_peek.amount.target() != 1.0 {
            return;
        }
        if !press.act {
            self.reel_mut(press.kind).pos.set((press.index as f32).min(max), now);
            cx.notify();
            return;
        }
        match press.kind {
            ReelKind::Shots => {
                let Some(path) = press.path else {
                    return;
                };
                if is_video_path(&path) {
                    self.close_tool_peek(now, cx);
                    crate::screen_recording::open_recording(path, cx);
                } else {
                    let key = path.to_string_lossy().to_string();
                    self.copy_shot(path, key, cx);
                }
            }
            ReelKind::Pages => {
                crate::flip_board::OPEN_PAGE.store(press.index, std::sync::atomic::Ordering::Relaxed);
                // Abrir pide la ventana: después del sondeo.
                cx.defer(|cx| {
                    if let Some(pill) = cx.windows().into_iter().find_map(|w| w.downcast::<crate::Pill>()) {
                        let _ = pill.update(cx, |pill, window, cx| pill.open_from_peek(crate::flip::TOOL, window, cx));
                    }
                });
            }
        }
    }

    /// Tareas: el héroe a la izquierda, las hechas hoy del total en grande sobre
    /// el contenedor primario, con «Ver todas» (ícono) y «Agregar tarea» (lleno);
    /// a la derecha las pendientes como lista agrupada de M3 con su casilla (un
    /// clic la marca hecha).
    fn texts_body_m3(&self, narrow: bool, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let (done, total) = self.tool_peek.data.tasks_today;
        let tasks = self.tool_peek.data.tasks.clone();
        macro_rules! open_tasks {
            ($cx:expr) => {
                $cx.listener(|pill, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    pill.close_tool_peek(Instant::now(), cx);
                    pill.open_notch(crate::NotchTool::Textos, window, cx);
                })
            };
        }
        let (add, all) = (open_tasks!(cx), open_tasks!(cx));
        let add_button = hover_fx_cx("peek-task-add", move |h, window, cx| {
            let radius = m3_press_radius(h, 16.0, 8.0, window, cx);
            div()
                .id("peek-task-add-btn")
                .h(px(32.))
                .w_full()
                .px(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded(px(radius))
                .bg(m3_layer(s.primary, s.on_primary, h))
                .text_color(s.on_primary)
                .type_style(type_scale::LABEL_LARGE)
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, add)
                .child(svg().path(crate::icons::themed("icons/plus.svg")).size(px(16.)).flex_none().text_color(s.on_primary))
                .child(div().truncate().child(t("pill.tasks.add")))
                .into_any_element()
        });
        let all_button = hover_fx_cx("peek-task-all", move |h, window, cx| {
            let radius = m3_press_radius(h, 16.0, 8.0, window, cx);
            div()
                .id("peek-task-all-btn")
                .size(px(32.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(radius))
                .bg(m3_layer(s.primary_container, s.on_primary_container, h))
                .cursor_pointer()
                .tooltip(crate::hover::tip(t("pill.tasks.seeAll")))
                .on_mouse_down(MouseButton::Left, all)
                .child(svg().path(crate::icons::themed("icons/arrow-up-right.svg")).size(px(16.)).text_color(s.on_primary_container))
                .into_any_element()
        });
        let e0 = m.enter(0);
        let counter = div()
            .w(px(TASKS_M3_COUNTER_W))
            .flex_none()
            .h(px(TASKS_M3_H))
            .p(px(12.))
            .pl(px(16.))
            .flex()
            .flex_col()
            .justify_between()
            .rounded(corner::XL)
            .bg(s.primary_container)
            .text_color(s.on_primary_container)
            .relative()
            .top(px(-6.0 * (1.0 - e0)))
            .opacity(e0)
            .child(
                div()
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(tabular(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(2.))
                                    .child(div().type_emphasized(type_scale::HEADLINE_LARGE).child(done.to_string()))
                                    .child(div().type_style(type_scale::TITLE_LARGE).child(format!("/{total}"))),
                            ))
                            .child(div().truncate().type_style(type_scale::LABEL_MEDIUM).child(t("pill.tasks.doneToday"))),
                    )
                    .child(all_button),
            )
            .child(add_button);
        let n = tasks.len();
        // Sin pendientes, el estado vacío de los demás vistazos; su acción es
        // «Agregar tarea» de la tarjeta de al lado (dos iguales juntas sobraban).
        let list: AnyElement = if tasks.is_empty() {
            div()
                .size_full()
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(12.))
                .rounded(corner::L)
                .bg(s.surface_container_high)
                .opacity(m.enter(1))
                .child(
                    gpui_m3::Shape::new(ShapeName::Cookie9)
                        .size(px(40.))
                        .color(s.secondary_container)
                        .child(svg().path(crate::icons::themed("icons/check.svg")).size(px(18.)).text_color(s.on_secondary_container)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().type_emphasized(type_scale::TITLE_SMALL).text_color(s.on_surface).child(t("pill.tasks.none")))
                        .child(div().truncate().type_style(type_scale::BODY_SMALL).text_color(s.on_surface_variant).child(t("pill.peeks.emptyTasksHint"))),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(LIST_M3_GAP + 1.0))
                .children(tasks.into_iter().enumerate().map(|(i, task)| {
                    let e = m.enter(i + 1);
                    let id = task.id;
                    let toggle = cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let mut all = crate::tasks::Tasks::load();
                        all.toggle(id);
                        pill.tool_peek.data.tasks.retain(|t| t.id != id);
                        pill.tool_peek.data.tasks_today = all.today();
                        cx.notify();
                    });
                    let (first, last) = (i == 0, i + 1 == n);
                    hover_fx_cx(("peek-task-fx", i), move |h, window, cx| {
                        let (top, bottom) = m3_press_radii(h, first, last, M3_GROUP_RADII, window, cx);
                        // Apretada, la casilla ya se ve marcada: al soltar la
                        // tarea se va de la lista.
                        let pressed = h.press > 0.5;
                        div()
                            .id(("peek-task", i))
                            .h(px(TASK_M3_ROW_H))
                            .pl(px(4.))
                            .pr(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded_tl(px(top))
                            .rounded_tr(px(top))
                            .rounded_bl(px(bottom))
                            .rounded_br(px(bottom))
                            .bg(m3_layer(s.surface_container_high, s.on_surface, h))
                            .relative()
                            .top(px(-6.0 * (1.0 - e)))
                            .opacity(e)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, toggle)
                            // La casilla de M3 (esquinas de 2) en su capa de
                            // estado redonda.
                            .child(
                                div()
                                    .size(px(28.))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(s.primary.opacity(layer_alpha(h)))
                                    .child(
                                        div()
                                            .size(px(16.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(2.))
                                            .when(pressed, |el| {
                                                el.bg(s.primary).child(svg().path(crate::icons::themed("icons/check.svg")).size(px(12.)).text_color(s.on_primary))
                                            })
                                            .when(!pressed, |el| el.border_2().border_color(h.mix(s.on_surface_variant, s.primary))),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .type_style(type_scale::BODY_MEDIUM)
                                    .text_color(s.on_surface)
                                    .child(task.text.clone()),
                            )
                            .children(task.meeting.as_ref().map(|(_, title)| {
                                div()
                                    .max_w(px(96.))
                                    .flex_none()
                                    .truncate()
                                    .type_style(type_scale::LABEL_SMALL)
                                    .text_color(s.on_surface_variant)
                                    .child(title.clone())
                            }))
                            .into_any_element()
                    })
                }))
                .into_any_element()
        };
        if narrow {
            return div()
                .px(px(SIDE))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(counter.w_full())
                .child(div().h(px(TASKS_M3_H)).child(list))
                .into_any_element();
        }
        div()
            .h(px(TASKS_M3_H))
            .px(px(SIDE))
            .flex()
            .gap(px(12.))
            .child(counter)
            .child(div().flex_1().min_w_0().h_full().child(list))
            .into_any_element()
    }

    /// Pomodoro: el tiempo que queda dentro de una forma de M3 que se llena con
    /// lo que lleva (Sunny en foco, Cookie9 en las pausas, con resorte al
    /// cambiar, y gira apenas con el avance); a su lado los modos como grupo de
    /// botones conectado y abajo reiniciar, empezar o pausar (el grande:
    /// corriendo, cuadrado redondeado) y saltar.
    fn pomodoro_body_m3(&self, s: Scheme, m: Motion, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        use crate::pomodoro::{format, Mode};
        let pomo = &self.pomodoro;
        let current = pomo.mode();
        let running = pomo.running();
        let progress = pomo.progress(now);
        let left = format(pomo.remaining(now));
        let focus = current == Mode::Focus;
        let e0 = m.enter(0);
        let hero = hover_fx_cx("peek-pomo-hero", move |_, window, cx| {
            let t = gpui_m3::motion::animate_f32("peek-pomo-shape", Spring::SPATIAL, if focus { 0.0 } else { 1.0 }, window, cx);
            let morph = Morph {
                from: ShapeName::Sunny,
                to: ShapeName::Cookie9,
                t,
                // La vuelta sale de lo que lleva: no pide cuadros propios.
                rotation: progress * std::f32::consts::FRAC_PI_2,
                toward: None,
            };
            let fill = if focus { s.primary_container } else { s.tertiary_container };
            div()
                .size(px(POMO_M3_H))
                .flex_none()
                .relative()
                .opacity(e0)
                .child(div().absolute().inset_0().child(shape_fill(morph, s.surface_container_highest, fill, progress)))
                .child(
                    div().absolute().inset_0().flex().items_center().justify_center().child(tabular(
                        div()
                            .type_emphasized(type_scale::HEADLINE_MEDIUM)
                            .text_color(if running { s.on_surface } else { s.on_surface_variant })
                            .child(left),
                    )),
                )
                .into_any_element()
        });
        let count = Mode::ALL.len();
        let modes: Vec<AnyElement> = Mode::ALL
            .iter()
            .enumerate()
            .map(|(i, &mode)| {
                let on = mode == current;
                let pick = cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    pill.pomodoro.set_mode(mode);
                    pill.sync_pomodoro(Instant::now(), cx);
                    cx.notify();
                });
                let e = m.enter(i + 1);
                hover_fx_cx(gpui::SharedString::from(format!("pomodoro-mode-{mode:?}")), move |h, window, cx| {
                    // Grupo conectado: extremos redondos, chicas entre medio; el
                    // elegido, píldora; apretado, se cuadra.
                    let round = 16.0;
                    let target = if h.press > 0.5 {
                        [4.0, 4.0]
                    } else {
                        [if i == 0 || on { round } else { 4.0 }, if i + 1 == count || on { round } else { 4.0 }]
                    };
                    let [l, r] = gpui_m3::motion::animate("peek-pomo-mode-radii", Spring::SPATIAL_FAST, target, window, cx);
                    let (l, r) = (l.max(0.0), r.max(0.0));
                    let (base, fg) = if on {
                        (s.secondary_container, s.on_secondary_container)
                    } else {
                        (s.surface_container_high, s.on_surface_variant)
                    };
                    div()
                        .id(("pomodoro-mode-btn", i))
                        .h(px(32.))
                        .flex_1()
                        .min_w_0()
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_tl(px(l))
                        .rounded_bl(px(l))
                        .rounded_tr(px(r))
                        .rounded_br(px(r))
                        .bg(m3_layer(base, fg, h))
                        .text_color(fg)
                        .type_style(type_scale::LABEL_MEDIUM)
                        .opacity(e)
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, pick)
                        .child(div().truncate().child(mode.label()))
                        .into_any_element()
                })
                .into_any_element()
            })
            .collect();
        let button = |i: usize, id: &'static str, icon: &'static str, big: bool, act: fn(&mut crate::Pill, &mut Context<crate::Pill>), cx: &mut Context<Self>| {
            let click = cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                act(pill, cx);
                pill.sync_pomodoro(Instant::now(), cx);
                cx.notify();
            });
            let e = m.enter(i + 4);
            hover_fx_cx(id, move |h, window, cx| {
                let (w, hgt) = if big { (72.0, POMO_M3_PLAY) } else { (40.0, 40.0) };
                let rest = if big && running { 16.0 } else { hgt / 2.0 };
                let radius = m3_press_radius(h, rest, if big { 12.0 } else { 8.0 }, window, cx);
                let (base, fg) = if big { (s.primary, s.on_primary) } else { (s.secondary_container, s.on_secondary_container) };
                div()
                    .id(gpui::SharedString::from(format!("{id}-btn")))
                    .w(px(w))
                    .h(px(hgt))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius))
                    .bg(m3_layer(base, fg, h))
                    .opacity(e)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, click)
                    .child(svg().path(crate::icons::themed(icon)).size(px(if big { 24. } else { 18. })).text_color(fg))
                    .into_any_element()
            })
        };
        let play = if running { "icons/pause.svg" } else { "icons/play.svg" };
        div()
            .px(px(SIDE))
            .h(px(POMO_M3_H))
            .flex()
            .items_center()
            .gap(px(16.))
            .child(hero)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(div().flex().gap(px(2.)).children(modes))
                    .child(
                        div()
                            .h(px(POMO_M3_PLAY))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(12.))
                            .child(button(0, "pomodoro-reset", "icons/rotate-ccw.svg", false, |pill, _| pill.pomodoro.reset(), cx))
                            .child(button(1, "pomodoro-play", play, true, |pill, cx| pill.toggle_pomodoro(cx), cx))
                            .child(button(2, "pomodoro-skip", "icons/skip-forward.svg", false, |pill, _| pill.pomodoro.skip(Instant::now()), cx)),
                    ),
            )
            .into_any_element()
    }

    /// Sistema: la CPU es el héroe (contenedor primario; en alerta, el de error)
    /// con el porcentaje grande, y la memoria al lado en una tarjeta de
    /// superficie; los dos con el indicador lineal de M3. Debajo, lo que más
    /// consume.
    fn system_body_m3(&self, s: Scheme, m: Motion, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let snap = self.system.read(cx).snapshot().clone();
        let procs = snap.procs.clone();
        let ram_total = procs.as_ref().map(|p| p.ram_total).unwrap_or(0);
        let gb = |b: u64| b as f64 / 1024.0 / 1024.0 / 1024.0;
        let count = m.count();
        let cpu = self.tool_peek.cpu.value(now) * count;
        let ram = self.tool_peek.ram.value(now) * count;
        let ram_used = (ram_total as f64 * ram as f64) as u64;
        let measured = procs.is_some();
        let card = |i: usize, label: &'static str, value: f32, shown: String, alert: f32, hero: bool| {
            let high = value >= alert;
            let (bg, fg, bar, track) = match (hero, high) {
                (true, true) => (s.error_container, s.on_error_container, s.on_error_container, gpui_m3::theme::mix(s.on_error_container, s.error_container, 0.22)),
                (true, false) => (s.primary_container, s.on_primary_container, s.on_primary_container, gpui_m3::theme::mix(s.on_primary_container, s.primary_container, 0.22)),
                (false, true) => (s.surface_container_high, s.on_surface, s.warning, s.surface_container_highest),
                (false, false) => (s.surface_container_high, s.on_surface, s.primary, s.surface_container_highest),
            };
            let e = m.enter(i);
            div()
                .flex_1()
                .min_w_0()
                .h(px(SYS_CARD_H))
                .px(px(14.))
                .py(px(10.))
                .flex()
                .flex_col()
                .justify_between()
                .rounded(corner::L)
                .bg(bg)
                .text_color(fg)
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .child(div().flex_1().min_w_0().truncate().type_style(type_scale::LABEL_LARGE).child(label))
                        .child(tabular(
                            div()
                                .flex_none()
                                .map(|el| if hero { el.type_emphasized(type_scale::HEADLINE_SMALL) } else { el.type_emphasized(type_scale::TITLE_MEDIUM) })
                                .child(shown),
                        )),
                )
                // El indicador lineal de Expressive: lo hecho y la pista, con un
                // espacio entre medio.
                .child(
                    div()
                        .h(px(6.))
                        .flex()
                        .gap(px(4.))
                        .child(div().h_full().w(gpui::relative(value.clamp(0.02, 0.98))).rounded_full().bg(bar))
                        .child(div().h_full().flex_1().rounded_full().bg(track)),
                )
        };
        let mut apps: Vec<_> = procs.map(|p| p.apps).unwrap_or_default();
        apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        let rows = apps.into_iter().take(3).enumerate().map(|(i, app)| {
            let e = m.enter(i + 3);
            let icon = self.system.update(cx, |panel, cx| panel.app_image(app.path.as_ref(), cx));
            let ram = if app.ram >= 1 << 30 { format!("{:.1} GB", gb(app.ram)) } else { format!("{} MB", app.ram >> 20) };
            div()
                .relative()
                .top(px(-6.0 * (1.0 - e)))
                .opacity(e)
                .h(px(APP_ROW_H))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(match icon {
                    Some(icon) => img(icon).size(px(16.)).flex_none().into_any_element(),
                    None => div().size(px(16.)).flex_none().rounded(corner::XS).bg(s.surface_container_highest).into_any_element(),
                })
                .child(div().flex_1().min_w_0().truncate().type_style(type_scale::BODY_SMALL).text_color(s.on_surface).child(app.name.clone()))
                .child(tabular(div().w(px(60.)).flex().justify_end().type_style(type_scale::LABEL_MEDIUM).text_color(s.on_surface_variant).child(ram)))
                .child(tabular(div().w(px(44.)).flex().justify_end().type_style(type_scale::LABEL_MEDIUM).text_color(s.on_surface).child(format!("{:.0} %", app.cpu * count))))
                .into_any_element()
        });
        let battery = snap.battery.map(|(p, plugged)| {
            if plugged {
                crate::i18n::tf("pill.peeks.batteryPlugged", &[("p", &p)])
            } else {
                crate::i18n::tf("pill.peeks.battery", &[("p", &p)])
            }
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap(px(SYS_CARD_GAP))
                    .child(card(0, "CPU", cpu, if measured { format!("{:.0} %", cpu * 100.0) } else { "—".into() }, 0.85, true))
                    .child(card(
                        1,
                        t("pill.peek.ram"),
                        ram,
                        if measured {
                            crate::i18n::tf("pill.peeks.memUsed", &[("used", &format!("{:.1}", gb(ram_used))), ("total", &format!("{:.0}", gb(ram_total)))])
                        } else {
                            "—".into()
                        },
                        0.90,
                        false,
                    )),
            )
            .child(div().h(px(SYS_CARD_GAP)))
            .child(
                div()
                    .h(px(SECTION_H))
                    .flex()
                    .items_center()
                    .type_style(type_scale::LABEL_MEDIUM)
                    .text_color(s.on_surface_variant)
                    .opacity(m.enter(2))
                    .child(t("pill.peeks.topConsumers"))
                    .child(div().flex_1())
                    .children(battery.map(|b| tabular(div().child(b)))),
            )
            .children(rows)
            .into_any_element()
    }

    /// Pizarra: las herramientas como botones de ícono de M3 (la de la última
    /// vez, cuadrado redondeado en el contenedor secundario, con un punto del
    /// color elegido), los colores como formas (el elegido, galleta) y la última
    /// captura en una tarjeta.
    fn board_body_m3(&self, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        use crate::board::COLORS;
        let prefs = self.tool_peek.data.board;
        let ink: Hsla = rgb(prefs.color).into();
        let tools = board_tools().enumerate().map(|(i, &(tool, icon, label))| {
            let on = prefs.tool == tool;
            let pick = cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                pill.board_from_peek(tool, window, cx);
            });
            hover_fx_cx(("peek-board-tool-fx", i), move |h, window, cx| {
                let e = m.enter(i);
                let radius = m3_press_radius(h, if on { 10.0 } else { BOARD_BTN / 2.0 }, 6.0, window, cx);
                let (base, fg) = if on {
                    (s.secondary_container, s.on_secondary_container)
                } else {
                    (s.on_surface_variant.opacity(0.0), s.on_surface_variant)
                };
                div()
                    .id(("peek-board-tool", i))
                    .size(px(BOARD_BTN))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius))
                    .bg(m3_layer(base, fg, h))
                    .top(px(-6.0 * (1.0 - e)))
                    .opacity(e)
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(crate::i18n::t(label)))
                    .on_mouse_down(MouseButton::Left, pick)
                    .child(svg().path(icon).size(px(18.)).text_color(fg))
                    .when(on, |el| {
                        el.child(
                            div()
                                .absolute()
                                .right(px(3.))
                                .bottom(px(3.))
                                .size(px(9.))
                                .rounded_full()
                                .bg(ink)
                                .border_1()
                                .border_color(s.secondary_container),
                        )
                    })
                    .into_any_element()
            })
        });
        let colors = COLORS.iter().enumerate().map(|(i, &color)| {
            let on = prefs.color == color;
            let e = m.enter(i + 2);
            div()
                .id(("peek-board-color", i))
                .size(px(BOARD_COLOR_H))
                .relative()
                .opacity(e)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        pill.tool_peek.data.board.color = color;
                        pill.tool_peek.data.board.save();
                        cx.notify();
                    }),
                )
                .fx(("peek-board-color-fx", i), move |el, h| {
                    let size = (if on { 22. } else { 16. }) + 3. * h.t - 3. * h.press;
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(s.on_surface.opacity(layer_alpha(h)))
                            .child(m3_swatch(("peek-board-swatch", i), rgb(color).into(), on, size, s)),
                    )
                })
                .into_any_element()
        });
        let shot = self.tool_peek.data.last_shot.clone().map(|shot| {
            let e = m.enter(4);
            let path = shot.path.clone();
            let open = cx.listener(move |pill, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                pill.board_on_shot(path.clone(), window, cx);
            });
            let preview = crate::clipboard::preview(&shot.path).map(|p| p.to_path_buf());
            hover_fx_cx("peek-board-shot-fx", move |h, window, cx| {
                let radius = m3_press_radius(h, 16.0, 8.0, window, cx);
                // Concéntrica con la tarjeta (su radio menos el margen de 6).
                let inner = (radius - 6.0).max(4.0);
                div()
                    .id("peek-board-shot")
                    .h(px(SHOT_ROW_H - 4.0))
                    .mt(px(BOARD_GAP))
                    .pl(px(6.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(radius))
                    .bg(m3_layer(s.surface_container_high, s.on_surface, h))
                    .relative()
                    .top(px(-6.0 * (1.0 - e)))
                    .opacity(e)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, open)
                    .child(
                        div()
                            .w(px(SHOT_THUMB.0 * 0.8))
                            .h(px(SHOT_THUMB.1 * 0.8))
                            .flex_none()
                            .overflow_hidden()
                            .rounded(px(inner))
                            .bg(s.surface_container_highest)
                            .children(preview.map(|p| {
                                // Medidas fijas: con `size_full` la imagen tomaba su
                                // alto natural y se salía de la tarjeta.
                                crate::release::scoped(
                                    gpui::SharedString::from(format!("shot-row-{}", p.display())),
                                    img(p)
                                        .w(px(SHOT_THUMB.0 * 0.8))
                                        .h(px(SHOT_THUMB.1 * 0.8))
                                        .object_fit(ObjectFit::Cover)
                                        .rounded(px(inner)),
                                )
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().type_emphasized(type_scale::TITLE_SMALL).text_color(s.on_surface).child(t("pill.peeks.scribbleLast")))
                            .child(tabular(div().truncate().type_style(type_scale::LABEL_MEDIUM).text_color(s.on_surface_variant).child(shot.label))),
                    )
                    .child(
                        div()
                            .size(px(32.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(s.secondary_container)
                            .child(svg().path(crate::icons::themed("icons/pencil.svg")).size(px(16.)).text_color(s.on_secondary_container)),
                    )
                    .into_any_element()
            })
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .child(div().flex().flex_wrap().gap(px(BOARD_BTN_GAP)).children(tools))
            .child(div().h(px(BOARD_COLOR_H)).mt(px(BOARD_GAP)).flex().gap(px(6.)).children(colors))
            .children(shot)
            .into_any_element()
    }

    /// Celular: los últimos avisos como lista agrupada de M3 (un toque abre
    /// «Celular»); sin celular vinculado, el héroe es la invitación a vincular.
    fn phone_body_m3(&self, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.tool_peek.phone.clone();
        macro_rules! open {
            ($cx:expr) => {
                $cx.listener(|pill, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    pill.open_from_peek(crate::CELULAR_TOOL, window, cx);
                })
            };
        }
        // Sin celular vinculado: «Vincular» abre el panel, que muestra el QR ahí mismo.
        if !self.tool_peek.phone_paired {
            return self.empty_state_m3(crate::CELULAR_TOOL, ShapeName::Cookie9, "icons/smartphone.svg", t("pill.peeks.unpairedTitle"), t("pill.peeks.unpairedHint"), Some(t("pill.peeks.pair")), s, m, cx);
        }
        // Sin avisos, el pie ya abre Celular: sin acción.
        if rows.is_empty() {
            return self.empty_state_m3(crate::CELULAR_TOOL, ShapeName::Cookie6, "icons/smartphone.svg", t("pill.peeks.emptyPhoneTitle"), t("pill.peeks.emptyPhoneHint"), None, s, m, cx);
        }
        let n = rows.len();
        let rows = rows.into_iter().enumerate().map(|(i, row)| {
            let e = m.enter(i);
            let open = open!(cx);
            let detail: String = row.detail.split_whitespace().collect::<Vec<_>>().join(" ");
            let (first, last) = (i == 0, i + 1 == n);
            hover_fx_cx(("peek-phone-fx", i), move |h, window, cx| {
                let (top, bottom) = m3_press_radii(h, first, last, M3_GROUP_RADII, window, cx);
                let lead = match row.icon {
                    Some(icon) => gpui::img(icon).size(px(28.)).flex_none().rounded_full().into_any_element(),
                    None => div()
                        .size(px(28.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(s.secondary_container)
                        .child(svg().path(crate::icons::themed("icons/smartphone.svg")).size(px(14.)).text_color(s.on_secondary_container))
                        .into_any_element(),
                };
                div()
                    .id(("peek-phone", i))
                    .h(px(PHONE_M3_ROW_H))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded_tl(px(top))
                    .rounded_tr(px(top))
                    .rounded_bl(px(bottom))
                    .rounded_br(px(bottom))
                    .bg(m3_layer(s.surface_container_high, s.on_surface, h))
                    .relative()
                    .top(px(-7.0 * (1.0 - e)))
                    .opacity(e)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, open)
                    .child(lead)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().type_emphasized(type_scale::TITLE_SMALL).text_color(s.on_surface).child(row.title))
                            .child(div().truncate().type_style(type_scale::BODY_SMALL).text_color(s.on_surface_variant).child(format!("{} · {detail}", row.app))),
                    )
                    .into_any_element()
            })
        });
        div()
            .px(px(SIDE))
            .flex()
            .flex_col()
            .gap(px(LIST_M3_GAP))
            .children(rows)
            .into_any_element()
    }

    /// Computadores: el estado como chip de asistencia con el interruptor de M3,
    /// el código para vincular como héroe (contenedor terciario, grande), y cada
    /// equipo en una tarjeta con su forma (conectado, galleta en el contenedor
    /// primario) y «Usar este mouse y teclado en él» con su interruptor.
    fn computers_body_m3(&self, narrow: bool, c: &Colors, s: Scheme, m: Motion, cx: &mut Context<Self>) -> AnyElement {
        use crate::computers::{self, Headline, Notice};
        use crate::i18n::tf;
        use crate::phone_setup::{chip, Ink};
        let k = &self.tool_peek.kvm;
        let ink = Ink { text: c.text, muted: c.muted, faint: c.faint, ok: c.ok, danger: s.error };
        let blue: Hsla = rgb(computers::TAB_BLUE).into();
        let (line, dot) = match k.headline() {
            Headline::Off => (t("pill.kvm.peekOff").to_string(), s.outline),
            Headline::Starting => (t("pill.kvm.starting").to_string(), s.outline),
            Headline::NoPeers => (t("pill.kvm.peekNone").to_string(), s.outline),
            Headline::Offline => (t("pill.kvm.peekOffline").to_string(), rgb(computers::TAB_GRAY).into()),
            Headline::Connected(name) => (tf("pill.kvm.peekConnected", &[("name", &name)]), s.success),
            Headline::Away(name) => (tf("pill.kvm.peekAway", &[("name", &name)]), blue),
            Headline::Controlled(name) => (tf("pill.kvm.peekControlled", &[("name", &name)]), blue),
        };
        let on = k.enabled;
        let power = cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            computers::set_enabled(!on);
            pill.tool_peek.read_kvm();
            cx.notify();
        });
        let status = div()
            .h(px(KVM_STATUS_H))
            .px(px(SIDE))
            .flex()
            .items_center()
            .gap(px(8.))
            .opacity(m.enter(0))
            .child(div().flex_1().min_w_0().flex().child(m3_assist_chip(dot, line, s)))
            .child(hover_fx_cx("peek-kvm-power-fx", move |h, window, cx| {
                let radius = m3_press_radius(h, 14.0, 8.0, window, cx);
                div()
                    .id("peek-kvm-power")
                    .h(px(28.))
                    .px(px(4.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .rounded(px(radius))
                    .bg(s.on_surface.opacity(layer_alpha(h)))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(t("pill.kvm.enable")))
                    .on_mouse_down(MouseButton::Left, power)
                    .child(m3_switch(on, s))
                    .into_any_element()
            }));

        // Otro equipo pide vincularse: el código es el héroe, para escribirlo allá.
        let code = k.pair_code.as_ref().map(|(name, code)| {
            div()
                .h(px(KVM_CODE_H - 6.))
                .mx(px(SIDE))
                .mb(px(6.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .rounded(corner::XL)
                .bg(s.tertiary_container)
                .text_color(s.on_tertiary_container)
                .opacity(m.enter(1))
                .child(div().max_w_full().px(px(12.)).truncate().type_emphasized(type_scale::LABEL_LARGE).child(tf("pill.kvm.codeTitle", &[("name", name)])))
                .child(tabular(div().type_emphasized(type_scale::DISPLAY_SMALL).line_height(px(40.)).child(crate::computers_notch::spaced_code(code))))
                .child(div().type_style(type_scale::LABEL_SMALL).child(t("pill.kvm.codeHint")))
        });

        let notice = k.notice.as_ref().map(|notice| {
            let (line, color, permissions) = match notice {
                Notice::Permission => (t("pill.kvm.permissionShort").to_string(), s.error, cfg!(target_os = "macos")),
                Notice::Info(text) => (text.clone(), s.on_surface_variant, false),
                Notice::Error(text) => (text.clone(), s.error, false),
            };
            div()
                .h(px(KVM_NOTE_H))
                .px(px(SIDE))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().truncate().type_style(type_scale::BODY_SMALL).text_color(color).child(line))
                .when(permissions, |el| {
                    el.child(chip("peek-kvm-permissions", None, t("pill.kvm.openPermissions"), true, false, ink, |_, _, cx| {
                        computers::dismiss_notice();
                        cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Permissions, cx));
                    }))
                })
        });

        // Prendido y sin equipos: vincular uno, en Ajustes (un botón tonal).
        let pair = (k.enabled && k.running && k.peers.is_empty()).then(|| {
            let open = cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                pill.close_tool_peek(Instant::now(), cx);
                cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Computers, cx));
            });
            let e = m.enter(1);
            div().h(px(EMPTY_H)).px(px(SIDE)).flex().items_center().child(hover_fx_cx("peek-kvm-pair-fx", move |h, window, cx| {
                let radius = m3_press_radius(h, 14.0, 8.0, window, cx);
                div()
                    .id("peek-kvm-pair")
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(radius))
                    .bg(m3_layer(s.secondary_container, s.on_secondary_container, h))
                    .text_color(s.on_secondary_container)
                    .type_style(type_scale::LABEL_LARGE)
                    .opacity(e)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, open)
                    .child(svg().path(crate::icons::themed("icons/plus.svg")).size(px(16.)).text_color(s.on_secondary_container))
                    .child(t("pill.kvm.peekPair"))
                    .into_any_element()
            }))
        });

        let pill = cx.entity().downgrade();
        let after: std::rc::Rc<dyn Fn(&mut gpui::App)> = std::rc::Rc::new(move |cx| {
            let _ = pill.update(cx, |pill, cx| {
                pill.tool_peek.read_kvm();
                cx.notify();
            });
        });
        let peer_h = ToolPeek::kvm_peer_h(narrow);
        let peers = self.tool_peek.kvm_peers(narrow);
        let rows = peers.iter().enumerate().map(|(i, peer)| {
            let e = m.enter(i + 1);
            let state = match (peer.connected, peer.mac) {
                (true, true) => t("pill.kvm.connectedMac"),
                (true, false) => t("pill.kvm.connectedPc"),
                (false, _) => t("pill.kvm.offlineShort"),
            };
            let connected = peer.connected;
            let (shape_bg, shape_fg) = if connected { (s.primary_container, s.on_primary_container) } else { (s.surface_container_highest, s.on_surface_variant) };
            let who = div()
                .id(("peek-kvm-who", i))
                .h(px(KVM_WHO_H))
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(10.))
                // Conectado, la forma pasa de círculo a galleta (con resorte).
                .child(
                    gpui_m3::Shape::new(ShapeName::Circle)
                        .size(px(28.))
                        .color(shape_bg)
                        .breathe_on_hover(("peek-kvm-shape", i), ShapeName::Cookie4)
                        .hovered(connected)
                        .child(svg().path(crate::icons::themed(if peer.mac { "icons/laptop.svg" } else { "icons/monitor.svg" })).size(px(14.)).text_color(shape_fg)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().type_emphasized(type_scale::TITLE_SMALL).text_color(s.on_surface).child(peer.name.clone()))
                        .child(div().truncate().type_style(type_scale::LABEL_MEDIUM).text_color(s.on_surface_variant).child(state)),
                );
            let sharing = peer.sharing;
            let id = peer.id.clone();
            let hint: gpui::SharedString = match (sharing, peer.side) {
                (true, Some(side)) => tf("pill.kvm.sharingHint", &[("side", &crate::computers_settings::side_edge(side))]).into(),
                _ => t("pill.kvm.shareHint").into(),
            };
            let toggle = cx.listener(move |pill, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                computers::set_sharing(&id, !sharing);
                pill.tool_peek.read_kvm();
                cx.notify();
            });
            let share = hover_fx_cx(("peek-kvm-share-fx", i), move |h, window, cx| {
                let radius = m3_press_radius(h, 12.0, 6.0, window, cx);
                div()
                    .id(("peek-kvm-share", i))
                    .h(px(KVM_SHARE_H))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(radius))
                    .bg(s.on_surface.opacity(layer_alpha(h)))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip_text(hint))
                    .on_mouse_down(MouseButton::Left, toggle)
                    .child(svg().path(crate::icons::themed("icons/mouse-pointer-2.svg")).size(px(16.)).flex_none().text_color(if sharing { s.primary } else { s.on_surface_variant }))
                    .child(div().flex_1().min_w_0().truncate().type_style(type_scale::BODY_SMALL).text_color(s.on_surface).child(t("pill.kvm.share")))
                    .child(m3_switch(sharing, s))
                    .into_any_element()
            });
            let picker = crate::computers_notch::side_picker(&format!("peek-kvm-side-{i}"), peer, true, ink, after.clone());
            let column = div().min_w_0().flex().flex_col().child(who).child(share);
            let drop_id = peer.id.clone();
            let row = div()
                .relative()
                .top(px(-7.0 * (1.0 - e)))
                .opacity(e)
                .h(px(peer_h - KVM_PEER_GAP))
                .mb(px(KVM_PEER_GAP))
                .mx(px(SIDE))
                .px(px(4.))
                .rounded(corner::L)
                .bg(s.surface_container_high)
                // Soltar archivos sobre un equipo los manda a sus Descargas.
                .drag_over::<gpui::ExternalPaths>(move |style, _, _, _| style.bg(s.secondary_container))
                .on_drop(cx.listener(move |pill, paths: &gpui::ExternalPaths, _, cx| {
                    crate::computers::send_files(&drop_id, paths.paths().to_vec());
                    pill.tool_peek.read_kvm();
                    cx.notify();
                }));
            if narrow {
                row.flex().flex_col().items_center().gap(px(KVM_PEER_GAP)).child(column.w_full()).child(picker)
            } else {
                row.flex().items_center().gap(px(10.)).child(column.flex_1()).child(picker)
            }
        });

        div()
            .flex()
            .flex_col()
            .child(status)
            .children(code)
            .children(notice)
            .children(pair)
            .children(rows.collect::<Vec<_>>())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Colors {
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    ok: Hsla,
    /// El esquema de la pill en Expressive: con él, los vistazos usan sus roles
    /// (contenedores, acento, capa de estado). `None` en Atic.
    m3: Option<gpui_m3::Scheme>,
}

impl Colors {
    /// Los del estilo de la pill: en Atic, los de siempre; en Expressive, los
    /// roles del esquema de la pill (oscuro o claro).
    fn current() -> Self {
        Self::with(crate::pill_look::content_scheme())
    }

    /// Los de lo que va sobre una carátula: en Expressive, del esquema
    /// oscuro aunque el tema de la pill sea claro.
    fn cover() -> Self {
        Self::with(crate::pill_look::cover_scheme())
    }

    fn with(scheme: Option<gpui_m3::Scheme>) -> Self {
        match scheme {
            // El texto atenuado es texto: `outline` (3:1) es solo para bordes.
            Some(s) => Self { text: s.on_surface, muted: s.on_surface_variant, faint: s.on_surface_variant, ok: s.success, m3: Some(s) },
            None => {
                let palette = crate::Palette::dark();
                Self {
                    text: palette.text,
                    muted: palette.muted,
                    faint: rgb(0x8f8f86).into(),
                    ok: rgb(0x6faf88).into(),
                    m3: None,
                }
            }
        }
    }
}

/// La capa de estado de M3 sobre `base`: `on` al 8 % con el cursor encima y
/// al 10 % apretado (la presión reemplaza al hover, no se suma).
fn state_layer(base: Hsla, on: Hsla, h: crate::hover::Hover) -> Hsla {
    m3_layer(base, on, h)
}

/// La opacidad de esa capa cuando va sola, sobre un fondo transparente
/// (`on.opacity(layer_alpha(h))`): 8 % encima, 10 % apretado.
fn layer_alpha(h: crate::hover::Hover) -> f32 {
    crate::pill_look::PillTokens::layer_alpha(h.t, h.press)
}

fn empty(text: &'static str, c: &Colors) -> AnyElement {
    div()
        .h(px(EMPTY_H))
        .px(px(SIDE))
        .flex()
        .items_center()
        .map(|el| match c.m3 {
            Some(_) => el.type_style(type_scale::BODY_MEDIUM),
            None => el.text_size(px(12.)),
        })
        .text_color(c.muted)
        .child(text)
        .into_any_element()
}

/// Lo que hay en el portapapeles, si se puede devolver tal cual con
/// «Deshacer»: solo texto (una imagen o archivos copiados no).
fn clipboard_text(cx: &mut gpui::App) -> Option<String> {
    let item = cx.read_from_clipboard()?;
    let only_text = item.entries().iter().all(|entry| matches!(entry, gpui::ClipboardEntry::String(_)));
    if !only_text || !crate::clip_image::read_files().is_empty() {
        return None;
    }
    item.text()
}

fn rgb_text(color: u32) -> String {
    format!("rgb({}, {}, {})", (color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff)
}

// --- M3 Expressive: piezas del interior de los vistazos ------------------------
//
// Lo clicable sigue con su `on_mouse_down` (el vistazo no usa `Button` de
// gpui-m3): estas piezas solo dibujan con el aspecto de los componentes. Lo que
// se cuadra al apretar usa `hover_fx_cx` + `m3_press_radius` (`hover.rs`).

/// Números tabulares (`tnum`): el tiempo, los porcentajes y los contadores no
/// bailan de ancho al cambiar.
fn tabular<E: gpui::Styled>(mut el: E) -> E {
    el.text_style().get_or_insert_with(Default::default).font_features =
        Some(gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])));
    el
}

/// Un interruptor de M3 a la medida del vistazo (36×20): encendido, la pista en
/// el acento con la perilla grande; apagado, contorno y perilla chica.
fn m3_switch(on: bool, s: Scheme) -> gpui::Div {
    div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .px(px(3.))
        .flex()
        .items_center()
        .rounded_full()
        .when(on, |el| el.bg(s.primary).justify_end())
        .when(!on, |el| el.bg(s.surface_container_highest).border_2().border_color(s.outline))
        .child(div().size(px(if on { 14. } else { 10. })).flex_none().rounded_full().bg(if on { s.on_primary } else { s.outline }))
}

/// Un chip de asistencia de M3 (el estado de algo): contorno, un punto del
/// color del estado y el texto.
fn m3_assist_chip(dot: Hsla, label: impl Into<gpui::SharedString>, s: Scheme) -> gpui::Div {
    let label: gpui::SharedString = label.into();
    div()
        .h(px(28.))
        .min_w_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(corner::S)
        .border_1()
        .border_color(s.outline_variant)
        .type_style(type_scale::LABEL_LARGE)
        .text_color(s.on_surface)
        .child(div().size(px(8.)).flex_none().rounded_full().bg(dot))
        .child(div().min_w_0().truncate().child(label))
}

/// Una forma de M3 que se llena desde abajo: la pista entera y, recortada a
/// `progress` del alto, la misma forma en `fill`. `morph` puede estar a medio
/// camino entre dos formas. Se dibuja en el `paint`: no pide cuadros.
fn shape_fill(morph: Morph, track: Hsla, fill: Hsla, progress: f32) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            // Los puntos de la forma una vez: la pista y el relleno son la misma.
            let points = morph.points(bounds);
            let paint = |window: &mut Window, color: Hsla| {
                let mut builder = gpui::PathBuilder::fill();
                builder.add_polygon(&points, true);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            };
            paint(window, track);
            let progress = progress.clamp(0.0, 1.0);
            if progress <= 0.0 {
                return;
            }
            let h = bounds.size.height * progress;
            let mask = gpui::Bounds::new(
                gpui::point(bounds.origin.x, bounds.origin.y + bounds.size.height - h),
                gpui::size(bounds.size.width, h),
            );
            window.with_content_mask(Some(gpui::ContentMask { bounds: mask }), |window| paint(window, fill));
        },
    )
    .size_full()
}

/// Una muestra de color como forma de M3: círculo, y `Cookie9` si `on` (la que
/// muestra la tarjeta), con el resorte `SPATIAL` al cambiar. Lleva un borde
/// fino: un color oscuro no se pierde sobre la superficie oscura.
fn m3_swatch(id: impl Into<gpui::ElementId>, color: Hsla, on: bool, size: f32, s: Scheme) -> gpui::Div {
    let id = id.into();
    div().size(px(size)).flex_none().child(
        gpui_m3::Shape::new(ShapeName::Circle)
            .size(px(size))
            .color(s.outline_variant)
            .breathe_on_hover(gpui_m3::motion::key(&id, "ring"), ShapeName::Cookie9)
            .hovered(on)
            .child(
                gpui_m3::Shape::new(ShapeName::Circle)
                    .size(px((size - 2.0).max(1.0)))
                    .color(color)
                    .breathe_on_hover(gpui_m3::motion::key(&id, "fill"), ShapeName::Cookie9)
                    .hovered(on),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_las_herramientas_con_vistazo() {
        assert!(has_peek(crate::color::TOOL));
        assert!(has_peek(crate::capture::TOOL));
        // Portapapeles y Agentes también: son páginas del carrusel.
        assert!(has_peek(crate::CLIPBOARD_TOOL));
        assert!(has_peek(crate::AGENTES_TOOL));
        assert!(!has_peek(FACE));
        assert!(has_peek(crate::board::TOOL));
    }

    #[test]
    fn alto_segun_lo_que_hay() {
        let mut peek = ToolPeek::new();
        let empty = peek.content_height(crate::color::TOOL, 400.0, false);
        peek.data.colors = vec![0xff0000, 0x00ff00];
        assert!(peek.content_height(crate::color::TOOL, 400.0, false) > empty);
        // Textos es la tarjeta de tareas: mide lo mismo con o sin ellas.
        assert_eq!(
            peek.content_height(crate::TEXTOS_TOOL, 400.0, false),
            TITLE_H + TASKS_BODY_H + FOOTER_H + BOTTOM
        );
    }

    #[test]
    fn en_un_costado_las_filas_van_en_columna() {
        let mut peek = ToolPeek::new();
        // Diez colores: arriba en una fila; al costado, seis filas.
        peek.data.colors = (0..10).collect();
        assert_eq!(
            peek.content_height(crate::color::TOOL, 300.0, true),
            TITLE_H + SIDE_COLORS as f32 * COLOR_ROW_H + FOOTER_H + BOTTOM
        );
        // Tres capturas: una por fila.
        peek.data.shots = (0..3)
            .map(|i| Shot { path: PathBuf::from(format!("{i}.png")), label: String::new(), video: false })
            .collect();
        assert_eq!(
            peek.content_height(crate::capture::TOOL, 300.0, true),
            TITLE_H + 3.0 * SHOT_ROW_H + FOOTER_H + BOTTOM
        );
    }

    #[test]
    fn la_cara_baja_sus_acciones() {
        let peek = ToolPeek::new();
        assert_eq!(
            peek.content_height(FACE, 400.0, false),
            TITLE_H + QUICK_TILE_H + FOOTER_H + BOTTOM
        );
        // Al costado, de a dos: cuatro filas para las siete.
        assert_eq!(
            peek.content_height(FACE, 300.0, true),
            TITLE_H + 4.0 * QUICK_TILE_H + 3.0 * QUICK_GAP + FOOTER_H + BOTTOM
        );
        assert!(!has_peek(FACE), "la cara no es una ficha de la tira");
    }

    #[test]
    fn la_pizarra_crece_con_la_ultima_captura() {
        let mut peek = ToolPeek::new();
        let without = peek.content_height(crate::board::TOOL, 400.0, false);
        // Once herramientas: en 400 caben nueve por fila, así que van dos filas.
        assert_eq!(board_tool_rows(400.0), 2);
        assert_eq!(
            without,
            TITLE_H + 2.0 * BOARD_BTN + BOARD_BTN_GAP + BOARD_GAP + BOARD_COLOR_H + FOOTER_H + BOTTOM
        );
        // Una fila bien ancha las lleva todas.
        assert_eq!(board_tool_rows(1000.0), 1);
        peek.data.last_shot = Some(Shot { path: PathBuf::from("a.png"), label: String::new(), video: false });
        assert_eq!(peek.content_height(crate::board::TOOL, 400.0, false), without + BOARD_GAP + SHOT_ROW_H);
    }

    #[test]
    fn en_expressive_cada_vistazo_mide_su_heroe() {
        let mut peek = ToolPeek::new();
        peek.m3 = true;
        let frame = TITLE_H + FOOTER_H + BOTTOM;
        assert_eq!(peek.content_height(crate::TEXTOS_TOOL, 400.0, false), frame + TASKS_M3_H);
        assert_eq!(peek.content_height(crate::TEXTOS_TOOL, 300.0, true), frame + TASKS_M3_H * 2.0 + 8.0);
        assert_eq!(peek.content_height(FACE, 400.0, false), frame + QUICK_M3_TILE_H);
        assert_eq!(peek.content_height(crate::POMODORO_TOOL, 400.0, false), frame + POMO_M3_H);
        peek.data.colors = vec![0xff0000];
        assert_eq!(peek.content_height(crate::color::TOOL, 400.0, false), frame + COLOR_HERO_H + COLOR_M3_GAP + SWATCH);
        // Al costado, los colores siguen en filas.
        assert_eq!(peek.content_height(crate::color::TOOL, 300.0, true), frame + COLOR_ROW_H);
        // Sin celular vinculado o sin avisos, el estado vacío; con avisos, la lista agrupada.
        peek.phone_paired = false;
        assert_eq!(peek.content_height(crate::CELULAR_TOOL, 400.0, false), frame + EMPTY_M3_H);
        peek.phone_paired = true;
        assert_eq!(peek.content_height(crate::CELULAR_TOOL, 400.0, false), frame + EMPTY_M3_H);
        // Sin colores ni capturas, también: no se corta.
        peek.data.colors.clear();
        assert_eq!(peek.content_height(crate::color::TOOL, 400.0, false), frame + EMPTY_M3_H);
        assert_eq!(peek.content_height(crate::capture::TOOL, 400.0, false), frame + EMPTY_M3_H);
        peek.data.colors = vec![0xff0000];
        peek.phone_paired = true;
        peek.phone = (0..3)
            .map(|i| PhoneGlance { app: format!("{i}"), icon: None, title: String::new(), detail: String::new() })
            .collect();
        assert_eq!(peek.content_height(crate::CELULAR_TOOL, 400.0, false), frame + 3.0 * PHONE_M3_ROW_H + 2.0 * LIST_M3_GAP);
    }

    #[test]
    fn en_expressive_capturas_y_flip_son_carruseles() {
        let mut peek = ToolPeek::new();
        peek.m3 = true;
        let frame = TITLE_H + FOOTER_H + BOTTOM;
        // Ocho capturas: arriba, el alto del carrusel; al costado, tres filas.
        peek.data.shots = (0..REEL_SHOTS)
            .map(|i| Shot { path: PathBuf::from(format!("{i}.png")), label: String::new(), video: false })
            .collect();
        assert_eq!(peek.content_height(crate::capture::TOOL, 400.0, false), frame + SHOTS_M3_H);
        assert_eq!(peek.content_height(crate::capture::TOOL, 300.0, true), frame + 3.0 * SHOT_ROW_H);
        peek.data.pages = vec![crate::flip_board::PageGlance { summary: String::new(), shapes: Vec::new(), ink: Vec::new() }; 6];
        assert_eq!(peek.content_height(crate::flip::TOOL, 400.0, false), frame + PAGES_M3_H);
        assert_eq!(peek.content_height(crate::flip::TOOL, 300.0, true), frame + MAX_PAGES as f32 * PAGE_ROW_H);
    }

    #[test]
    fn el_carrusel_mide_como_el_de_gpui_m3() {
        let container = crate::PANEL_W - SIDE * 2.0;
        // Héroe: la grande y una chica asomando llenan la tira con su hueco.
        let hero = reel_slots(true, container, 8, 0.0);
        assert_eq!(hero.len(), 2);
        assert_eq!((hero[0].0, hero[0].1), (0, 0.0));
        assert!((hero[0].2 + REEL_GAP + hero[1].2 - container).abs() < 0.01);
        assert!(hero[1].2 <= REEL_SMALL);
        // A medio camino ninguna se monta en la siguiente.
        for offset in [0.3, 0.5, 2.7] {
            for pair in reel_slots(true, container, 8, offset).windows(2) {
                assert!(pair[0].1 + pair[0].2 <= pair[1].1 + 0.01);
            }
        }
        // Multi-browse: grande, mediana y chica.
        let browse = reel_slots(false, container, 5, 0.0);
        assert_eq!(browse.len(), 3);
        assert!(browse[0].2 > browse[1].2 && browse[1].2 > browse[2].2);
        // Héroe hasta la última en el foco; multi-browse, hasta la mediana.
        assert_eq!(reel_max(true, 8), 7.0);
        assert_eq!(reel_max(false, 5), 3.0);
        assert_eq!(reel_max(false, 1), 0.0);
        // Al soltar, la más cercana o hacia donde se lanzó.
        assert_eq!(reel_snap_target(1.4, 0.0, 7.0), 1.0);
        assert_eq!(reel_snap_target(1.4, 8.0, 7.0), 2.0);
        assert_eq!(reel_snap_target(6.8, 30.0, 7.0), 7.0);
        // La rueda avanza una aunque se gire poco.
        assert_eq!(reel_wheel_target(1.0, 1.1, 7.0), 2.0);
        assert_eq!(reel_wheel_target(1.0, 0.9, 7.0), 0.0);
        // Más allá de los extremos, a una fracción.
        assert_eq!(reel_rubber(-1.0, 3.0), -REEL_RUBBER);
        assert_eq!(reel_rubber(4.0, 3.0), 3.0 + REEL_RUBBER);
        assert_eq!(reel_rubber(2.0, 3.0), 2.0);
    }

    #[test]
    fn el_tema_da_la_vuelta() {
        assert_eq!(next_theme("system"), "light");
        assert_eq!(next_theme("light"), "dark");
        assert_eq!(next_theme("dark"), "system");
        // Los finos cuentan como su lado.
        assert_eq!(theme_side("sepia"), "light");
        assert_eq!(next_theme("midnight"), "system");
        assert_eq!(theme_side("custom"), "system");
    }

    #[test]
    fn codigos_de_color() {
        assert_eq!(rgb_text(0x1a2b3c), "rgb(26, 43, 60)");
    }
}
