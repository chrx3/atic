//! El editor de grabaciones: una ventana propia que se abre con **Editar** en
//! el estante (`shelf.rs`). Plan y decisiones en `docs/EDITOR_GRABACIONES.md`
//! y `docs/EDITOR_PULIDO.md`.
//!
//! Como un editor de video: la vista previa arriba, los ajustes a la derecha
//! (`style_panel.rs`) y la línea de tiempo abajo (`timeline.rs`), con la regla,
//! las pistas y el cabezal. Sigue el estilo de Apariencia (`palette.rs`): Atic,
//! Material 3 Expressive o Liquid Glass. Este archivo es el núcleo: el estado, la
//! vista previa, el mouse, las teclas y lo que engancha a los paneles hijos.
//!
//! - **Vista previa:** AVPlayer reproduce el original (con su audio) y cada
//!   cuadro pasa por el mismo `Composer` que la exportación (`scene.rs`: Core Image dibuja
//!   el video con la cámara, el desenfoque de movimiento, el cursor, las ondas, los textos,
//!   las zonas difuminadas, el marco, las esquinas, la sombra y el fondo) y el resultado se
//!   muestra con una sola `gpui::surface`, sin pasar por la CPU. Lo que sigue siendo de GPUI
//!   son las capas interactivas encima (el recuadro del zoom y del difuminado, sus manijas).
//!   El cuadro tiene la proporción de salida (`project.export.aspect`) y el video entra
//!   entero y centrado, con las mismas cuentas del exportador (`canvas_of`). Componer
//!   cuesta: se mide y se adapta la escala, y quieta no se recompone (`scene::Governor`, `Shot`).
//! - **Lo hecho** vive en `edit_project.rs` y se guarda solo, con deshacer y
//!   rehacer (`History`). La cámara, el cursor y las ondas salen del proyecto y
//!   del tiempo, no del cuadro anterior (`transiciones-fluidas`, regla 3): la
//!   vista previa y lo exportado son lo mismo (`scene.rs`, un solo camino).
//! - **Movimiento** (`fluid.rs`): las transiciones de la interfaz (diálogo de exportar, píldora y
//!   panel de las pestañas, controles, bloques de la línea de tiempo, avisos). Todas salen del
//!   estado (abierto/cerrado, elegido, posición), se pueden interrumpir, respetan «Reducir
//!   movimiento» y no tocan el `Composer` ni la cámara.
//! - **Exportar** (`export_panel.rs`) deja siempre otro archivo, «(editada)», con
//!   `atic_capture::stream::export` (Core Image y el codificador de hardware).

mod agent_bridge;
mod assistant;
mod captions;
mod export_panel;
mod fluid;
mod keys;
mod palette;
mod scene;
mod style_panel;
mod timeline;
pub(crate) mod wallpapers;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use atic_capture::stream::VideoPlayer;
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, size, svg, App, Bounds,
    ClickEvent, Context, Div, Entity, FocusHandle, Focusable, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, RenderImage, SharedString, Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::edit_project::{Backdrop, Blur, Focus, History, Look, Motion, Project, Text, TextAt};
use crate::hover::{mix, HoverExt};
use crate::i18n::t;
use crate::kit::chrome;
use crate::screen_cursor::Track;
use crate::text_input::{Changed, TextInput};
use captions::CaptionsUi;
use export_panel::ExportUi;
use palette::{Ed, T_BODY, T_CONTROL, T_SMALL, T_TITLE};
use style_panel::StyleUi;
use timeline::Lane;

const TOP_H: f32 = 46.0;
const SIDEBAR_W: f32 = 296.0;
const GAP: f32 = 12.0;
const BUTTON: f32 = 32.0;
/// Agarrar la orilla de un bloque, en píxeles.
const EDGE: f32 = 7.0;
/// Lo menos que dura un bloque.
const MIN_LEN: f64 = 0.3;
/// Cuántos píxeles hay que mover un bloque tomado para que cuente como arrastre.
const DRAG_SLOP: f32 = 3.0;
/// Cuánto se siguen pidiendo cuadros tras un salto de tiempo, para que llegue el cuadro nuevo.
const SETTLE: Duration = Duration::from_millis(450);
/// Lo que dura «Ver cómo queda»: unos segundos desde el cabezal.
const PREVIEW_LEN: f64 = 4.0;
/// Niveles de audio por segundo (`atic_capture::stream::audio_levels`): la forma de onda
/// de la línea de tiempo, los silencios y los subtítulos salen de ahí.
pub(super) const LEVELS_PER_SEC: u32 = 20;
/// Lo que dura un aviso que no es un error, en segundos; con una acción («Deshacer»), más: da tiempo a usarla.
const NOTICE_SECS: u64 = 4;
const NOTICE_ACTION_SECS: u64 = 6;
/// «Guardado»: cuánto se queda tras guardar y lo que tarda en entrar y salir.
const SAVED_SHOW: Duration = Duration::from_millis(1600);
const SAVED_FADE: Duration = Duration::from_millis(120);
/// Lo que dura lo nuevo que se agrega en el cabezal (velocidad, difuminado, texto, zoom).
const NEW_LEN: f64 = 3.0;
/// La velocidad de un tramo nuevo y la de los silencios que se aceleran.
const SPEED_NEW: f64 = 2.0;
const SPEED_IDLE: f64 = 4.0;
/// Lo menos que dura un silencio para proponerlo (segundos).
const IDLE_MIN: f64 = 2.0;
/// El recuadro de un difuminado nuevo: x, y, ancho y alto (0–1 del video).
const BLUR_NEW: [f64; 4] = [0.35, 0.40, 0.30, 0.15];
/// Lo menos que mide cada lado de un difuminado (parte del video).
const BLUR_MIN: f64 = 0.02;
/// La esquina de un recuadro se toma a esta distancia (puntos).
const CORNER_HIT: f32 = 14.0;
/// Un botón apagado (deshacer sin pasos) se sigue leyendo: su ícono pierde algo de fuerza, no desaparece.
const DISABLED_ALPHA: f32 = 0.8;
/// El interruptor: pista de `SWITCH_W × SWITCH_H` con la perilla de `SWITCH_KNOB` a `SWITCH_PAD` del borde.
const SWITCH_W: f32 = 30.0;
const SWITCH_H: f32 = 18.0;
const SWITCH_KNOB: f32 = 14.0;
const SWITCH_PAD: f32 = 2.0;
/// El botón de arriba cambia de texto («Exportar», «Exportando 42 %», «Listo»): ancho fijo.
const EXPORT_BTN_W: f32 = 152.0;
/// El aviso de abajo de la barra de arriba no pasa de este ancho.
const NOTICE_MAX_W: f32 = 560.0;
/// La hoja de atajos: su ancho, lo más que sube desde arriba (parte del alto) y el cuadrito de cada tecla.
const HELP_W: f32 = 380.0;
const KEY_CAP_H: f32 = 20.0;
/// El pulso del botón Exportar al terminar con el diálogo oculto: lo que dura y hasta cuántos px crece.
const PULSE_LEN: Duration = Duration::from_millis(600);
const PULSE_SPREAD: f32 = 7.0;
/// Fuera de un tramo con velocidad, el reproductor va a 1× y con su volumen.
const NORMAL_RATE: f64 = 1.0;
/// Al elegir una velocidad o un difuminado, el cabezal va a esta distancia de su inicio (o a su mitad).
const INSIDE_LEAD: f64 = 0.5;
/// Desde este ancho de la vista previa, las herramientas de abajo llevan su nombre.
const TOOLS_LABELS_MIN_W: f32 = 720.0;
/// Un silencio ya cortado a esto (segundos) de otro, cuenta como cortado.
const MIN_SPAN_SLACK: f64 = 0.05;

/// Abre el editor de `path`, o trae al frente el que ya lo tiene.
pub fn open(path: PathBuf, cx: &mut App) {
    let existing = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<RecordingEditor>())
        .find(|handle| handle.read(cx).is_ok_and(|view| view.path == path));
    if let Some(handle) = existing {
        if handle.update(cx, |_, window, _| crate::win::bring_to_front(window)).is_ok() {
            return;
        }
    }
    // Lo que dejó una sesión que murió con «Ver cómo queda» abierto.
    if let Some(leftover) = preview_path(&path) {
        let _ = std::fs::remove_file(leftover);
    }
    let track = crate::screen_cursor::load(&path);
    // Con «Cursor suave», se edita la grabación sin cursor y el cursor lo dibuja el editor.
    let cursor_drawn = track.as_ref().is_some_and(|t| t.cursor_hidden);
    let source = crate::screen_cursor::master_path(&path).filter(|m| cursor_drawn && m.exists()).unwrap_or_else(|| path.clone());
    let player = match VideoPlayer::open(&source) {
        Ok(player) => {
            player.seek(0.0);
            player
        }
        Err(error) => {
            tracing::warn!(%error, path = %source.display(), "editor: no se pudo abrir el video");
            return;
        }
    };
    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let options = WindowOptions {
        titlebar: Some(chrome::titlebar(&title, TOP_H)),
        window_min_size: Some(size(px(960.), px(640.))),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1320.), px(860.)), cx))),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| RecordingEditor::new(path, source, title, player, track, cursor_drawn, window, cx))
    });
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => tracing::warn!(%error, "editor: no se pudo abrir la ventana"),
    }
}

/// Lo que se puede elegir en la línea de tiempo.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Zoom(usize),
    Text(usize),
    Cut(usize),
    Speed(usize),
    Blur(usize),
}

/// Qué se arrastra.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    /// El cabezal; si sonaba, sigue al soltar.
    Head(bool),
    /// Un bloque entero, tomado a `offset` segundos de su inicio.
    Move(Item, f64),
    Start(Item),
    End(Item),
    TrimStart,
    TrimEnd,
    /// Un rango marcado sobre el video (Shift + arrastrar), tomado desde este segundo:
    /// al soltar queda como un corte.
    Range(f64),
    /// El recuadro de un zoom en la vista previa, tomado a (`dx`, `dy`) de su centro.
    Aim(usize, f64, f64),
    /// La esquina del recuadro: cambia cuánto se acerca.
    AimSize(usize),
    /// El recuadro de un difuminado, tomado a (`dx`, `dy`) de su esquina de arriba a la izquierda.
    BlurMove(usize, f64, f64),
    /// La esquina de abajo a la derecha del recuadro de un difuminado: cambia su tamaño.
    BlurSize(usize),
}

/// «Ver cómo queda»: unos segundos exportados de verdad (con el desenfoque de
/// movimiento, que la vista previa en vivo no puede dibujar).
enum Preview {
    Rendering { progress: Arc<AtomicU64>, cancel: Arc<AtomicBool> },
    Ready(VideoPlayer),
}

/// Lo que ofrece un aviso a la derecha de su texto.
#[derive(Clone, Debug, PartialEq)]
enum NoticeAction {
    /// «Deshacer»: vuelve el último paso (borrar un bloque, cortar silencios…).
    Undo,
    /// «Mostrar»: revela en Finder lo que se exportó.
    Reveal(PathBuf),
    /// «Ver»: abre el diálogo de exportar (una exportación falló mientras estaba oculto).
    ShowExport,
}

impl NoticeAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Undo => t("pill.editor.noticeUndo"),
            Self::Reveal(_) => t("pill.editor.noticeReveal"),
            Self::ShowExport => t("pill.editor.noticeShow"),
        }
    }
}

/// Un aviso bajo la barra de arriba. Los que no son un error se van solos.
struct Notice {
    text: String,
    error: bool,
    at: Instant,
    action: Option<NoticeAction>,
}

/// Dónde queda el `.preview.mp4` de «Ver cómo queda» (al lado del registro del mouse).
fn preview_path(video: &Path) -> Option<PathBuf> {
    crate::screen_cursor::track_path(video).map(|p| p.with_extension("preview.mp4"))
}

pub struct RecordingEditor {
    /// La grabación (la de afuera): de aquí sale el nombre de lo exportado.
    path: PathBuf,
    /// Lo que se reproduce y exporta: la grabación o, con «Cursor suave», la sin cursor.
    source: PathBuf,
    title: String,
    player: VideoPlayer,
    duration: f64,
    track: Option<Track>,
    clicks: Vec<u32>,
    /// El editor dibuja el cursor (la grabación no lo tiene).
    cursor_drawn: bool,
    project: Project,
    /// Deshacer y rehacer.
    history: History,
    motion: Arc<Motion>,
    selected: Option<Item>,
    grab: Option<Grab>,
    thumbs: Vec<Arc<RenderImage>>,
    /// La parte del video que muestra la línea de tiempo (zoom y desplazamiento).
    view: timeline::View,
    /// De dónde viene la vista cuando un botón o atajo la está deslizando hacia `view` (y desde cuándo).
    view_anim: Option<(timeline::View, Instant)>,
    /// El nivel del audio (0–1) a `LEVELS_PER_SEC` por segundo; vacío mientras carga o si no hay audio.
    levels: Vec<f32>,
    /// Ya se leyeron los niveles (vacíos = el video no tiene audio).
    levels_ready: bool,
    /// Donde se pegó lo que se arrastra en la línea de tiempo (el imán).
    snap: Option<f64>,
    /// El último salto de un corte (adónde y cuándo), para no repetirlo mientras llega.
    last_skip: Option<(f64, Instant)>,
    /// El último tiempo visto y hasta cuándo seguir pidiendo cuadros tras un salto: el cuadro nuevo
    /// llega unos milisegundos después y, en pausa, nadie más redibuja para mostrarlo.
    seen_time: f64,
    settle_until: Option<Instant>,
    /// Dónde se apretó el mouse en la línea de tiempo y si el arrastre ya cuenta (más de `DRAG_SLOP`).
    press_x: f32,
    armed: bool,
    /// El rango que se marca sobre el video para cortarlo (extremos, en segundos).
    range: Option<(f64, f64)>,
    text_field: Entity<TextInput>,
    export_ui: ExportUi,
    style_ui: StyleUi,
    captions_ui: CaptionsUi,
    preview: Option<Preview>,
    /// El aviso de abajo de la barra de arriba (un error de guardado, un proyecto dañado…).
    notice: Option<Notice>,
    /// Los tramos con velocidad (inicio, fin, ritmo; segundos del original), ya recortados a lo que queda.
    spans: Vec<(f64, f64, f64)>,
    /// Lo último que se le pidió al reproductor: (velocidad, mudo). Solo se repite si cambia.
    playback: (f64, bool),
    /// El video dentro de la vista previa (x, y, ancho, alto desde su esquina).
    video_rect: (f32, f32, f32, f32),
    focus: FocusHandle,
    backdrop: Option<bool>,
    /// Píxeles por punto de la pantalla de la ventana.
    scale: f32,
    /// Dónde quedaron la línea de tiempo y la vista previa (las dejan los canvas).
    lanes: Rc<Cell<Option<Bounds<Pixels>>>>,
    stage: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// El compositor de la vista previa, su última salida y lo que tarda (`scene.rs`).
    compose: scene::Compose,
    /// Las animaciones de la interfaz (`fluid.rs`): diálogo, pestañas, controles, bloques, avisos.
    fluid: fluid::Fluid,
    /// El bloque que se acaba de agregar y cuándo: entra con un fundido y una escala.
    born: Option<(Item, Instant)>,
    /// Lo último que mostraron la guía del imán, el rango y el aviso: se quedan dibujados
    /// mientras se desvanecen, cuando ya no hay nada en el estado.
    snap_last: Cell<Option<f64>>,
    range_last: Cell<Option<(f64, f64)>>,
    notice_last: RefCell<Option<(SharedString, Option<NoticeAction>, bool)>>,
    /// Se está navegando con el teclado (Tab, flechas sobre una opción): solo entonces el foco muestra su
    /// anillo. Un clic del mouse lo apaga (`keys.rs`).
    kbd_nav: bool,
    /// La hoja de atajos («?») está abierta.
    help_open: bool,
    /// Cuándo se guardó el proyecto por última vez (el «Guardado» de arriba).
    saved_at: Option<Instant>,
    /// El último clic sobre la vista previa (cuándo y si quedó sonando): su eco, un ícono de reproducir o pausar.
    echo: Option<(Instant, bool)>,
    /// El acercamiento del último zoom que cubrió el cabezal: el indicador «2,4×» se queda con él mientras se desvanece.
    zoom_last: Cell<f64>,
    /// El audio se leyó y es puro silencio (la grabación no recibió señal: suele ser el permiso del micrófono).
    levels_silent: bool,
    /// El registro de los agentes de IA (`agent_bridge.rs`): se da de baja al cerrar la ventana.
    agent: Option<agent_bridge::Link>,
    /// El chat con un agente de IA (pestaña «Asistente», `assistant.rs`).
    assistant: Entity<assistant::Assistant>,
}

/// Cuánto se ve el «Guardado» (0–1) a `elapsed` de haber guardado: entra en `SAVED_FADE`, se queda hasta
/// `SAVED_SHOW` y se retira en otros `SAVED_FADE`.
fn saved_alpha(elapsed: Duration) -> f32 {
    let fade = SAVED_FADE.as_secs_f32();
    let t = elapsed.as_secs_f32();
    let show = SAVED_SHOW.as_secs_f32();
    if t < fade {
        t / fade
    } else if t < show {
        1.0
    } else {
        (1.0 - (t - show) / fade).max(0.0)
    }
}

/// ¿Está entrando o saliendo? (solo entonces hace falta pedir cuadros)
fn saved_animating(elapsed: Duration) -> bool {
    elapsed < SAVED_FADE || (elapsed >= SAVED_SHOW && elapsed < SAVED_SHOW + SAVED_FADE)
}

fn clock(secs: f64) -> String {
    crate::meetings::stopwatch(Duration::from_secs_f64(secs.max(0.0)))
}

/// «0:07.3»: el tiempo del cabezal, con décimas.
fn clock_tenths(secs: f64) -> String {
    let tenths = ((secs.max(0.0) * 10.0).floor() as u64) % 10;
    format!("{}.{tenths}", clock(secs))
}

/// El rectángulo (x, y, ancho, alto) de un cuadro de proporción `aspect` (ancho / alto)
/// dentro de un área de `bw × bh`, centrado y con `margin` alrededor.
fn fit((bw, bh): (f32, f32), aspect: f32, margin: f32) -> (f32, f32, f32, f32) {
    let (bw, bh) = (bw - margin * 2.0, bh - margin * 2.0);
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 };
    let h = (bw / aspect).min(bh).max(0.0);
    let w = h * aspect;
    (margin + (bw - w) / 2.0, margin + (bh - h) / 2.0, w, h)
}

fn hex(color: u32) -> Hsla {
    rgb(color).into()
}

// --- El cuadro de salida y dónde cae el video (las cuentas del exportador) ------------------

/// El margen del marco en píxeles (`padding` es parte del lado corto del cuadro): lo mismo
/// que `frame_margin` de `export_macos.rs`, que es privada allá.
fn frame_margin(padding: f64, out_w: f64, out_h: f64) -> f64 {
    padding.clamp(0.0, 0.3) * out_w.min(out_h)
}

/// Dónde va el video dentro del cuadro de salida: `[x, y, ancho, alto]` en píxeles, con el
/// origen abajo a la izquierda (como Core Image). Lo mismo que `content_rect` de
/// `export_macos.rs`: tiene la proporción del original, entra entero y queda centrado, con
/// `margin` por los lados que lo limitan.
fn content_rect((out_w, out_h): (f64, f64), (src_w, src_h): (f64, f64), margin: f64) -> [f64; 4] {
    let (src_w, src_h) = (src_w.max(1.0), src_h.max(1.0));
    let margin = margin.max(0.0);
    // Una proporción casi igual a la del cuadro: el margen es el mismo de siempre.
    let same = ((src_w * out_h) / (out_w * src_h) - 1.0).abs() < 0.005;
    let (w, h) = if same {
        let w = out_w - margin * 2.0;
        (w, w * out_h / out_w)
    } else if src_w * out_h > out_w * src_h {
        let w = out_w - margin * 2.0;
        (w, w * src_h / src_w)
    } else {
        let h = out_h - margin * 2.0;
        (h * src_w / src_h, h)
    };
    let w = w.round().clamp(2.0, out_w);
    let h = h.round().clamp(2.0, out_h);
    [((out_w - w) / 2.0).floor(), ((out_h - h) / 2.0).floor(), w, h]
}

/// El cuadro de salida y dónde cae el video dentro de él.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Canvas {
    /// El cuadro de salida en píxeles (ancho, alto): el del exportador.
    out: (u32, u32),
    /// El video dentro del cuadro: x, y, ancho y alto (0–1 del cuadro, desde arriba a la izquierda).
    video: [f32; 4],
}

impl Canvas {
    /// Ancho sobre alto del cuadro.
    fn aspect(&self) -> f32 {
        self.out.0 as f32 / self.out.1.max(1) as f32
    }
}

/// El cuadro de `out` píxeles (el que da `export_panel::render_dims`) con el video de
/// `source` dentro, con el margen del marco; sin marco, margen 0 (y el resto, negro).
fn canvas_of(out: (u32, u32), source: (u32, u32), look: Look) -> Canvas {
    let (ow, oh) = (out.0.max(2) as f64, out.1.max(2) as f64);
    let margin = if look.framed() { frame_margin(look.padding, ow, oh) } else { 0.0 };
    let [x, y, w, h] = content_rect((ow, oh), (source.0 as f64, source.1 as f64), margin);
    Canvas {
        out: (ow as u32, oh as u32),
        video: [(x / ow) as f32, ((oh - y - h) / oh) as f32, (w / ow) as f32, (h / oh) as f32],
    }
}

// --- El fondo del marco -----------------------------------------------------------------------

/// El fondo que se usa si la imagen elegida ya no está (el mismo que el del exportador).
fn fallback_backdrop() -> Backdrop {
    Backdrop::Gradient(export_panel::FALLBACK_BACKDROP.0, export_panel::FALLBACK_BACKDROP.1)
}

/// El fondo del marco para un cuadro de `w × h`, de esquina a esquina como al
/// exportar: en CSS, el ángulo cuya dirección es la diagonal.
fn backdrop_fill(backdrop: Backdrop, w: f32, h: f32) -> gpui::Background {
    match backdrop {
        Backdrop::None => gpui::black().into(),
        Backdrop::Solid(c) => hex(c).into(),
        Backdrop::Gradient(a, b) => {
            let angle = 180.0 - (w / h.max(1.0)).atan().to_degrees();
            linear_gradient(angle, linear_color_stop(hex(a), 0.0), linear_color_stop(hex(b), 1.0))
        }
        Backdrop::Image => backdrop_fill(fallback_backdrop(), w, h),
    }
}

// --- El difuminado y la velocidad en la vista previa ------------------------------------------

/// El recuadro `rect` (x, y, ancho, alto; 0–1 del video) movido para que su esquina de
/// arriba a la izquierda quede en `at` menos lo que se agarró (`grab`), sin salirse del video.
fn blur_moved(rect: [f64; 4], at: (f64, f64), grab: (f64, f64)) -> [f64; 4] {
    let [_, _, w, h] = rect;
    if ![at.0, at.1, grab.0, grab.1].iter().all(|v| v.is_finite()) {
        return rect;
    }
    [(at.0 - grab.0).clamp(0.0, (1.0 - w).max(0.0)), (at.1 - grab.1).clamp(0.0, (1.0 - h).max(0.0)), w, h]
}

/// El recuadro con su esquina de abajo a la derecha en `at`: cada lado entre `BLUR_MIN` y lo
/// que queda hasta el borde del video.
fn blur_resized(rect: [f64; 4], at: (f64, f64)) -> [f64; 4] {
    if !(at.0.is_finite() && at.1.is_finite()) {
        return rect;
    }
    let x = rect[0].clamp(0.0, 1.0 - BLUR_MIN);
    let y = rect[1].clamp(0.0, 1.0 - BLUR_MIN);
    // `max(BLUR_MIN)`: con el redondeo, `1 - x` podría quedar apenas bajo el mínimo (y `clamp` entra en pánico).
    [x, y, (at.0 - x).clamp(BLUR_MIN, (1.0 - x).max(BLUR_MIN)), (at.1 - y).clamp(BLUR_MIN, (1.0 - y).max(BLUR_MIN))]
}

/// El `item` si todavía existe en `project`: tras deshacer o rehacer, sus índices pueden
/// no valer.
fn valid_item(item: Option<Item>, project: &Project) -> Option<Item> {
    item.filter(|item| match *item {
        Item::Zoom(i) => i < project.zooms.len(),
        Item::Text(i) => i < project.texts.len(),
        Item::Cut(i) => i < project.cuts.len(),
        Item::Speed(i) => i < project.speeds.len(),
        Item::Blur(i) => i < project.blurs.len(),
    })
}

/// Cómo suena el reproductor en `t`: (velocidad, mudo). Dentro de un tramo con velocidad,
/// a su ritmo y sin audio (el exportado queda en silencio ahí); fuera, normal.
fn playback_at(spans: &[(f64, f64, f64)], t: f64) -> (f64, bool) {
    spans.iter().find(|(a, b, _)| t >= *a && t < *b).map_or((NORMAL_RATE, false), |&(_, _, rate)| (rate, true))
}

/// El ícono del eco de un clic sobre la vista previa: su lado (px).
const ECHO_SIZE: f32 = 44.0;

/// Cómo se ve el eco `elapsed` después del clic: (opacidad, escala). Entra en `ECHO_IN` (de `ECHO_FROM` a 1), se queda
/// `ECHO_HOLD` y sale en `ECHO_OUT`; con movimiento reducido no cambia de tamaño.
fn echo_state(elapsed: Duration, reduced: bool) -> (f32, f32) {
    let (t_in, hold) = (fluid::ECHO_IN, fluid::ECHO_IN + fluid::ECHO_HOLD);
    let alpha = if elapsed < t_in {
        elapsed.as_secs_f32() / t_in.as_secs_f32()
    } else if elapsed < hold {
        1.0
    } else {
        1.0 - ((elapsed - hold).as_secs_f32() / fluid::ECHO_OUT.as_secs_f32())
    };
    let grown = if reduced { 1.0 } else { crate::anim::lerp(fluid::ECHO_FROM, 1.0, (elapsed.as_secs_f32() / t_in.as_secs_f32()).min(1.0)) };
    (alpha.clamp(0.0, 1.0), grown)
}

/// El tono de una acción de panel (`RecordingEditor::action`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Normal,
    /// Borra algo: texto e ícono en el color de «cortar».
    Danger,
}

/// Cuánto se ve el relleno de una acción en reposo y con el mouse encima (alfa de `control`). Con vidrio
/// (Liquid Glass) se sube el relleno en vez de dibujar un borde: un borde oscuro en un vidrio claro se ve mal.
fn action_fill(glass: bool) -> (f32, f32) {
    if glass {
        (0.7, 1.0)
    } else {
        (0.55, 0.95)
    }
}

/// El borde fino de una acción en reposo: solo fuera del vidrio.
fn action_border(pal: &Ed) -> Option<Hsla> {
    (!pal.glass).then(|| pal.border.opacity(ACTION_BORDER))
}

/// El borde de una acción y el tinte de una acción destructiva al pasar el mouse.
const ACTION_BORDER: f32 = 0.6;
const ACTION_DANGER_HOVER: f32 = 0.14;

/// El cuadrito de una tecla (en un botón): `S`, `Z`…
fn key_cap(key: &'static str, color: Hsla, fill: Hsla) -> Div {
    div().h(px(KEY_CAP_H - 4.0)).px(px(4.)).flex().items_center().rounded(px(4.)).bg(fill.opacity(0.6)).text_size(px(10.)).text_color(color).child(key)
}

impl RecordingEditor {
    #[allow(clippy::too_many_arguments)]
    fn new(
        path: PathBuf,
        source: PathBuf,
        title: String,
        player: VideoPlayer,
        track: Option<Track>,
        cursor_drawn: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let pal = Ed::current();
        let duration = player.duration();
        let (mut project, load_error) = Project::load_checked(&path);
        // Los zooms de un proyecto de antes: sin duplicados apilados ni «respiraciones» (idempotente).
        project.tidy(duration);
        // Un proyecto dañado no se pierde en silencio: queda una copia y se avisa (hasta que se cierre el aviso).
        let notice = load_error.map(|error| {
            tracing::warn!(%error, "editor: el proyecto estaba dañado");
            Notice { text: t("pill.editor.projectCorrupt").to_string(), error: true, at: Instant::now(), action: None }
        });
        let motion = Arc::new(Motion::new(&project, track.as_ref(), duration));
        let spans = project.speed_spans(duration);
        let history = History::new(&project);
        let text_field = cx.new(|cx| TextInput::new(t("pill.editor.textPlaceholder"), pal.text, pal.sub, pal.accent, cx));
        // Escribir es un solo paso de deshacer por racha (misma clave dentro de 600 ms).
        cx.subscribe(&text_field, |view: &mut Self, field, _: &Changed, cx| {
            if let Some(Item::Text(i)) = view.selected {
                let value = field.read(cx).text().to_string();
                if let Some(text) = view.project.texts.get_mut(i) {
                    text.text = value;
                    view.changed_keyed("text", cx);
                }
            }
        })
        .detach();
        let export_ui = ExportUi::new(&pal, cx);
        let style_ui = StyleUi::new(&pal, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        // Las miniaturas, en otro hilo (decodificar cuadros sueltos tarda).
        let thumbs_of = source.clone();
        cx.spawn(async move |view, cx| {
            let frames = cx.background_executor().spawn(async move { atic_capture::stream::thumbnails(&thumbs_of, 28, 96) }).await;
            if let Ok(frames) = frames {
                let images: Vec<_> = frames.iter().filter_map(crate::capture::image_of).collect();
                let _ = view.update(cx, |view, cx| {
                    view.thumbs = images;
                    cx.notify();
                });
            }
        })
        .detach();
        // La forma de onda del audio, también en otro hilo (leerlo entero tarda).
        let levels_of = source.clone();
        cx.spawn(async move |view, cx| {
            let levels = cx.background_executor().spawn(async move { atic_capture::stream::audio_levels(&levels_of, LEVELS_PER_SEC) }).await;
            let levels = levels.unwrap_or_else(|error| {
                tracing::warn!(%error, "editor: no se pudo leer el nivel del audio");
                Vec::new()
            });
            let _ = view.update(cx, |view, cx| {
                view.levels_silent = timeline::is_silent(&levels);
                view.levels = levels;
                view.levels_ready = true;
                cx.notify();
            });
        })
        .detach();
        let agent = agent_bridge::attach(&path, &title, duration, window, cx);
        let assistant = {
            let (video, name) = (path.clone(), title.clone());
            cx.new(|cx| assistant::Assistant::new(video, name, cx))
        };
        Self {
            path,
            source,
            title,
            player,
            duration,
            clicks: track.as_ref().map(|t| t.clicks()).unwrap_or_default(),
            track,
            cursor_drawn,
            project,
            history,
            motion,
            selected: None,
            grab: None,
            thumbs: Vec::new(),
            view: timeline::View::full(duration),
            view_anim: None,
            levels: Vec::new(),
            levels_ready: false,
            snap: None,
            last_skip: None,
            seen_time: f64::NAN,
            settle_until: None,
            press_x: 0.0,
            armed: true,
            range: None,
            text_field,
            export_ui,
            style_ui,
            captions_ui: CaptionsUi::default(),
            preview: None,
            notice,
            spans,
            playback: (NORMAL_RATE, false),
            video_rect: (0.0, 0.0, 0.0, 0.0),
            focus,
            backdrop: None,
            scale: window.scale_factor(),
            lanes: Rc::new(Cell::new(None)),
            stage: Rc::new(Cell::new(None)),
            compose: scene::Compose::new(),
            fluid: fluid::Fluid::new(),
            born: None,
            snap_last: Cell::new(None),
            range_last: Cell::new(None),
            notice_last: RefCell::new(None),
            kbd_nav: false,
            echo: None,
            zoom_last: Cell::new(1.0),
            help_open: false,
            saved_at: None,
            levels_silent: false,
            agent,
            assistant,
        }
    }

    /// Algo cambió en el proyecto: queda un paso para deshacer, la cámara se recalcula y se guarda.
    fn changed(&mut self, cx: &mut Context<Self>) {
        // Una opción que ya estaba elegida: nada que anotar, calcular ni guardar.
        if self.history.unchanged(&self.project) {
            return;
        }
        self.history.record(&self.project, None);
        self.refresh(cx);
    }

    /// Como `changed`, pero una racha con la misma `key` (escribir, arrastrar) es un solo paso.
    /// Los ajustes que no mueven la cámara ni las velocidades (la exportación, el estilo de
    /// los textos…) no la recalculan: con una grabación de horas eso se notaba en cada clic.
    fn changed_keyed(&mut self, key: &'static str, cx: &mut Context<Self>) {
        const KEEPS_CAMERA: [&str; 6] = ["export", "captions-style", "text", "text-color", "backdrop-color", "blur-rect"];
        if self.history.unchanged(&self.project) {
            return;
        }
        self.history.record(&self.project, Some(key));
        self.refresh_with(!(KEEPS_CAMERA.contains(&key) || style_panel::keeps_camera(key)), cx);
    }

    /// El proyecto ya es el que va (cambió, se deshizo o se rehízo): se cierra «Ver cómo queda»
    /// (ya no es lo que saldría), se recalcula la cámara, se guarda y se avisa si no se pudo.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_with(true, cx);
    }

    /// `camera`: si hay que recalcular la cámara y las velocidades (lo caro).
    fn refresh_with(&mut self, camera: bool, cx: &mut Context<Self>) {
        self.close_preview();
        if camera {
            self.motion = Arc::new(Motion::new(&self.project, self.track.as_ref(), self.duration));
            self.spans = self.project.speed_spans(self.duration);
        }
        match self.project.save(&self.path) {
            Ok(()) => self.mark_saved(cx),
            Err(error) => {
                tracing::warn!(%error, "editor: no se pudo guardar el proyecto");
                self.say(crate::i18n::tf("pill.editor.saveFailed", &[("error", &error)]), true, cx);
            }
        }
        self.export_reset();
        cx.notify();
    }

    /// Anota que se guardó y avisa cuando toca retirar el «Guardado» (la entrada y la salida son fundidos cortos).
    fn mark_saved(&mut self, cx: &mut Context<Self>) {
        let at = Instant::now();
        self.saved_at = Some(at);
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(SAVED_SHOW).await;
            let _ = view.update(cx, |view, cx| {
                if view.saved_at == Some(at) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Un aviso bajo la barra de arriba. Un error se queda hasta que se cierre; lo demás se va a los 4 s.
    fn say(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.say_with(text, error, None, cx);
    }

    /// Como `say`, con una acción a la derecha («Deshacer», «Mostrar», «Ver»). Con acción dura más (`NOTICE_ACTION_SECS`).
    fn say_with(&mut self, text: impl Into<String>, error: bool, action: Option<NoticeAction>, cx: &mut Context<Self>) {
        let at = Instant::now();
        let secs = if action.is_some() { NOTICE_ACTION_SECS } else { NOTICE_SECS };
        self.notice = Some(Notice { text: text.into(), error, at, action });
        if !error {
            cx.spawn(async move |view, cx| {
                cx.background_executor().timer(Duration::from_secs(secs)).await;
                let _ = view.update(cx, |view, cx| {
                    // Si ya hay otro aviso, este no lo borra.
                    if view.notice.as_ref().is_some_and(|notice| notice.at == at && !notice.error) {
                        view.notice = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Lo que hace el botón de un aviso («Deshacer», «Mostrar», «Ver»): el aviso se va.
    fn run_notice_action(&mut self, action: NoticeAction, window: &mut Window, cx: &mut Context<Self>) {
        self.notice = None;
        match action {
            NoticeAction::Undo => self.undo(window, cx),
            NoticeAction::Reveal(path) => {
                let _ = crate::paths::reveal(&path);
            }
            NoticeAction::ShowExport => self.open_export(cx),
        }
        cx.notify();
    }

    fn now(&self) -> f64 {
        self.player.time()
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if let Some(Preview::Ready(player)) = &self.preview {
            if player.playing() {
                player.pause();
            } else {
                if player.time() >= player.duration() - 0.05 {
                    player.seek(0.0);
                }
                player.play();
            }
            cx.notify();
            return;
        }
        if self.player.playing() {
            self.player.pause();
        } else {
            let (from, to) = self.project.trim(self.duration);
            if self.now() < from || self.now() >= to - 0.05 {
                self.player.seek(from);
            }
            self.player.play();
        }
        cx.notify();
    }

    fn seek(&mut self, t: f64, cx: &mut Context<Self>) {
        self.player.seek(t.clamp(0.0, self.duration));
        cx.notify();
    }

    /// Si el cabezal está fuera de `span`, lo lleva adentro (a `lead` segundos del inicio o a
    /// su mitad, lo que quede antes): lo elegido se apunta viendo su contenido.
    fn bring_inside(&mut self, (a, b): (f64, f64), lead: f64) {
        let now = self.now();
        if now < a || now > b {
            self.player.seek(a + ((b - a) / 2.0).min(lead));
        }
    }

    fn select(&mut self, item: Option<Item>, window: &mut Window, cx: &mut Context<Self>) {
        // Algo nuevo elegido: su tarjeta entra con un fundido y la barra vuelve arriba para mostrarla.
        if item.is_some() && item != self.selected {
            self.style_ui.reveal_selection();
        }
        self.selected = item;
        // Un zoom, una velocidad o un difuminado se apuntan viendo su contenido: el cabezal va adentro.
        if let Some(span) = item.and_then(|item| self.span(item)) {
            match item {
                Some(Item::Zoom(_)) => self.bring_inside(span, crate::edit_project::ZOOM_RAMP),
                Some(Item::Speed(_) | Item::Blur(_)) => self.bring_inside(span, INSIDE_LEAD),
                _ => {}
            }
        }
        if let Some(Item::Text(i)) = item {
            let value = self.project.texts.get(i).map(|t| t.text.clone()).unwrap_or_default();
            self.text_field.update(cx, |field, cx| field.set_text(value, cx));
        } else {
            window.focus(&self.focus);
        }
        cx.notify();
    }

    // --- Herramientas ---------------------------------------------------------------

    /// Lo que se agrega en el cabezal: desde `now` hasta `NEW_LEN` segundos después (sin pasar
    /// del final). `None` si queda menos de `MIN_LEN`.
    fn new_span(&self, len: f64) -> Option<(f64, f64)> {
        let at = self.now();
        let end = (at + len).min(self.duration);
        (end - at >= MIN_LEN).then_some((at, end))
    }

    fn add_cut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, end)) = self.new_span(1.0) else { return };
        self.project.cuts.push((at, end));
        let i = self.project.cuts.len() - 1;
        self.changed(cx);
        self.born = Some((Item::Cut(i), Instant::now()));
        self.select(Some(Item::Cut(i)), window, cx);
    }

    fn add_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.now();
        // No duplica ni apila: si cae dentro de otro zoom empieza donde ese termina.
        match self.project.add_zoom(at, NEW_LEN, self.duration) {
            Some(i) => {
                self.changed(cx);
                self.born = Some((Item::Zoom(i), Instant::now()));
                self.select(Some(Item::Zoom(i)), window, cx);
            }
            None => self.say(t("pill.editor.noRoom"), false, cx),
        }
    }

    fn add_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, end)) = self.new_span(NEW_LEN) else { return };
        self.project.texts.push(Text::new(at, end, t("pill.editor.textDefault"), TextAt::Bottom));
        let i = self.project.texts.len() - 1;
        self.changed(cx);
        self.born = Some((Item::Text(i), Instant::now()));
        self.select(Some(Item::Text(i)), window, cx);
        let field = self.text_field.read(cx).focus_handle(cx);
        window.focus(&field);
    }

    fn add_speed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.now();
        match self.project.add_speed(at, at + NEW_LEN, SPEED_NEW) {
            Some(i) => {
                self.changed(cx);
                self.born = Some((Item::Speed(i), Instant::now()));
                self.select(Some(Item::Speed(i)), window, cx);
            }
            None => self.say(t("pill.editor.noRoom"), false, cx),
        }
    }

    fn add_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, end)) = self.new_span(NEW_LEN) else {
            self.say(t("pill.editor.noRoom"), false, cx);
            return;
        };
        self.project.blurs.push(Blur { start: at, end, rect: BLUR_NEW, pixelate: false });
        let i = self.project.blurs.len() - 1;
        self.changed(cx);
        self.born = Some((Item::Blur(i), Instant::now()));
        self.select(Some(Item::Blur(i)), window, cx);
    }

    fn autofocus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Sin el registro del mouse no hay dónde apuntar.
        if self.track.is_none() {
            self.say(t("pill.editor.noTrackNotice"), false, cx);
            return;
        }
        let duration = self.duration;
        if let Some(track) = &self.track {
            self.project.autofocus_track(track, duration);
        }
        self.changed(cx);
        self.select(None, window, cx);
    }

    /// Los tramos sin voz ni actividad. `None` mientras se lee el audio: sin él, el mouse solo
    /// daría como quietos tramos donde alguien habla.
    fn idle_spans(&mut self, cx: &mut Context<Self>) -> Option<Vec<(f64, f64)>> {
        if !self.levels_ready {
            self.say(t("pill.editor.idleWait"), false, cx);
            return None;
        }
        let spans = self.project.suggest_idle(&self.levels, LEVELS_PER_SEC, self.track.as_ref(), self.duration, IDLE_MIN);
        if spans.is_empty() {
            self.say(t("pill.editor.idleNone"), false, cx);
            return None;
        }
        Some(spans)
    }

    /// Pone más rápido lo que no tiene voz ni actividad.
    fn speed_up_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spans) = self.idle_spans(cx) else { return };
        let added = spans.into_iter().filter(|&(a, b)| self.project.add_speed(a, b, SPEED_IDLE).is_some()).count();
        if added == 0 {
            self.say(t("pill.editor.idleNone"), false, cx);
            return;
        }
        self.changed(cx);
        self.select(None, window, cx);
        self.say_with(crate::i18n::tf("pill.editor.idleSped", &[("count", &added)]), false, Some(NoticeAction::Undo), cx);
    }

    /// Quita lo que no tiene voz ni actividad.
    fn cut_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spans) = self.idle_spans(cx) else { return };
        let mut added = 0;
        for (a, b) in spans {
            // Lo que ya está cortado no se corta otra vez.
            let covered = self.project.cuts.iter().any(|&(c, d)| c.min(d) <= a + MIN_SPAN_SLACK && c.max(d) >= b - MIN_SPAN_SLACK);
            if !covered {
                self.project.cuts.push((a, b));
                added += 1;
            }
        }
        if added == 0 {
            self.say(t("pill.editor.idleNone"), false, cx);
            return;
        }
        self.changed(cx);
        self.select(None, window, cx);
        self.say_with(crate::i18n::tf("pill.editor.idleCut", &[("count", &added)]), false, Some(NoticeAction::Undo), cx);
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected {
            Some(Item::Zoom(i)) if i < self.project.zooms.len() => {
                self.project.zooms.remove(i);
            }
            Some(Item::Text(i)) if i < self.project.texts.len() => {
                self.project.texts.remove(i);
            }
            Some(Item::Cut(i)) if i < self.project.cuts.len() => {
                self.project.cuts.remove(i);
            }
            Some(Item::Speed(i)) if i < self.project.speeds.len() => {
                self.project.speeds.remove(i);
            }
            Some(Item::Blur(i)) if i < self.project.blurs.len() => {
                self.project.blurs.remove(i);
            }
            _ => return,
        }
        self.changed(cx);
        self.select(None, window, cx);
        self.say_with(t("pill.editor.deleted"), false, Some(NoticeAction::Undo), cx);
    }

    /// Un proyecto ya cambiado a otro (deshacer o rehacer): la selección vale si su índice sigue
    /// existiendo, y el campo de texto muestra lo del texto elegido.
    fn restore(&mut self, project: Project, window: &mut Window, cx: &mut Context<Self>) {
        self.project = project;
        self.selected = valid_item(self.selected, &self.project);
        match self.selected {
            Some(Item::Text(i)) => {
                let value = self.project.texts.get(i).map(|t| t.text.clone()).unwrap_or_default();
                self.text_field.update(cx, |field, cx| field.set_text(value, cx));
            }
            // Si el foco quedó en un campo de un elemento que ya no está, vuelve al editor.
            _ => window.focus(&self.focus),
        }
        self.refresh(cx);
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(project) = self.history.undo(&self.project) {
            self.restore(project, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(project) = self.history.redo(&self.project) {
            self.restore(project, window, cx);
        }
    }

    /// Borra todo lo hecho (un paso de deshacer: se puede volver).
    fn reset_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project = Project::default();
        self.selected = None;
        self.changed(cx);
        self.select(None, window, cx);
        self.say_with(t("pill.editor.resetDone"), false, Some(NoticeAction::Undo), cx);
    }

    fn span(&self, item: Item) -> Option<(f64, f64)> {
        match item {
            Item::Zoom(i) => self.project.zooms.get(i).map(|z| (z.start, z.end)),
            Item::Text(i) => self.project.texts.get(i).map(|t| (t.start, t.end)),
            Item::Cut(i) => self.project.cuts.get(i).copied(),
            Item::Speed(i) => self.project.speeds.get(i).map(|s| (s.start, s.end)),
            Item::Blur(i) => self.project.blurs.get(i).map(|b| (b.start, b.end)),
        }
    }

    fn set_span(&mut self, item: Item, (a, b): (f64, f64)) {
        match item {
            Item::Zoom(i) => {
                if let Some(z) = self.project.zooms.get_mut(i) {
                    (z.start, z.end, z.auto) = (a, b, false);
                }
            }
            Item::Text(i) => {
                if let Some(t) = self.project.texts.get_mut(i) {
                    (t.start, t.end) = (a, b);
                }
            }
            Item::Cut(i) => {
                if let Some(c) = self.project.cuts.get_mut(i) {
                    *c = (a, b);
                }
            }
            Item::Speed(i) => {
                if let Some(s) = self.project.speeds.get_mut(i) {
                    (s.start, s.end) = (a, b);
                }
            }
            Item::Blur(i) => {
                if let Some(x) = self.project.blurs.get_mut(i) {
                    (x.start, x.end) = (a, b);
                }
            }
        }
    }

    // --- La línea de tiempo (el mouse; el dibujo está en `timeline.rs`) -------------

    /// El tiempo bajo `x`, y cuántos segundos son un píxel (con la vista de la línea de tiempo).
    fn time_at(&self, x: Pixels) -> Option<(f64, f64)> {
        let bounds = self.lanes.get()?;
        let width = f32::from(bounds.size.width);
        let local = f32::from(x) - f32::from(bounds.origin.x);
        Some((timeline::time_at(local, self.view, self.duration, width), 1.0 / timeline::px_per_sec(self.view, width)))
    }

    /// Qué hay bajo el puntero al apretar.
    fn hit(&self, position: gpui::Point<Pixels>) -> Option<Grab> {
        let bounds = self.lanes.get()?;
        let (t, per_px) = self.time_at(position.x)?;
        let y = f32::from(position.y) - f32::from(bounds.origin.y);
        let edge = EDGE as f64 * per_px;
        let near = |(a, b): (f64, f64), item: Item| -> Option<Grab> {
            if (t - a).abs() <= edge {
                Some(Grab::Start(item))
            } else if (t - b).abs() <= edge {
                Some(Grab::End(item))
            } else if t > a && t < b {
                Some(Grab::Move(item, t - a))
            } else {
                None
            }
        };
        match timeline::lane_at(y)? {
            Lane::Video => {
                let (from, to) = self.project.trim(self.duration);
                if (t - from).abs() <= edge * 1.5 {
                    return Some(Grab::TrimStart);
                }
                if (t - to).abs() <= edge * 1.5 {
                    return Some(Grab::TrimEnd);
                }
                self.project.cuts.iter().enumerate().find_map(|(i, cut)| near(*cut, Item::Cut(i)))
            }
            // El audio no tiene nada que tomar: arrastrar ahí mueve el cabezal.
            Lane::Audio => None,
            Lane::Zoom => self.project.zooms.iter().enumerate().rev().find_map(|(i, z)| near((z.start, z.end), Item::Zoom(i))),
            Lane::Speed => self.project.speeds.iter().enumerate().rev().find_map(|(i, s)| near((s.start, s.end), Item::Speed(i))),
            Lane::Blur => self.project.blurs.iter().enumerate().rev().find_map(|(i, b)| near((b.start, b.end), Item::Blur(i))),
            Lane::Text => self.project.texts.iter().enumerate().rev().find_map(|(i, x)| near((x.start, x.end), Item::Text(i))),
        }
    }

    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.press_x = f32::from(event.position.x);
        self.armed = true;
        // Un deslizador o una pestaña que quedó enfocado no debe comerse las flechas del cabezal.
        window.focus(&self.focus);
        if let Some(bounds) = self.lanes.get() {
            let x = f32::from(event.position.x) - f32::from(bounds.origin.x);
            let y = f32::from(event.position.y) - f32::from(bounds.origin.y);
            // A la izquierda, sobre los nombres de las pistas, o sobre los botones de zoom: nada.
            if x < timeline::GUTTER_W || timeline::on_view_buttons(x, y, f32::from(bounds.size.width)) {
                return;
            }
            // Shift sobre el video: marca un rango para cortarlo.
            if event.modifiers.shift && timeline::lane_at(y) == Some(Lane::Video) {
                if let Some((t, per_px)) = self.time_at(event.position.x) {
                    let t = self.snap_value(t, Grab::Range(t), per_px, event.modifiers.alt);
                    self.grab = Some(Grab::Range(t));
                    self.range = Some((t, t));
                    self.select(None, window, cx);
                    return;
                }
            }
        }
        self.snap = None;
        let grab = self.hit(event.position);
        let item = match grab {
            Some(Grab::Move(item, _) | Grab::Start(item) | Grab::End(item)) => Some(item),
            _ => None,
        };
        if item.is_some() || grab.is_none() {
            self.select(item, window, cx);
        }
        self.grab = Some(grab.unwrap_or_else(|| {
            let playing = self.player.playing();
            self.player.pause();
            Grab::Head(playing)
        }));
        // Tomar un bloque no lo mueve: solo arrastrar, y con un clic que tiembla unos píxeles tampoco.
        self.armed = !matches!(self.grab, Some(Grab::Move(..) | Grab::Start(_) | Grab::End(_)));
        if matches!(self.grab, Some(Grab::Head(_) | Grab::TrimStart | Grab::TrimEnd)) {
            self.drag_to(event.position.x, event.modifiers.alt, cx);
        }
    }

    /// Arrastra lo que se tomó hasta `x`. El valor se pega a lo que haya cerca (el imán);
    /// con `alt` apretado, no.
    fn drag_to(&mut self, x: Pixels, alt: bool, cx: &mut Context<Self>) {
        let Some((t, per_px)) = self.time_at(x) else { return };
        let Some(grab) = self.grab else { return };
        let d = self.duration;
        match grab {
            Grab::Aim(..) | Grab::AimSize(_) | Grab::BlurMove(..) | Grab::BlurSize(_) => return,
            Grab::Head(_) => {
                let t = self.snap_value(t, grab, per_px, alt);
                self.seek(t, cx);
                return;
            }
            Grab::Range(from) => {
                let t = self.snap_value(t, grab, per_px, alt);
                self.range = Some((from, t));
            }
            Grab::Move(item, offset) => {
                if let Some((a, b)) = self.span(item) {
                    let len = b - a;
                    let room = (d - len).max(0.0);
                    let start = self.snap_move((t - offset).clamp(0.0, room), len, grab, per_px, alt).clamp(0.0, room);
                    self.set_span(item, (start, start + len));
                }
            }
            Grab::Start(item) => {
                if let Some((_, b)) = self.span(item) {
                    let t = self.snap_value(t, grab, per_px, alt);
                    self.set_span(item, (t.min(b - MIN_LEN).max(0.0), b));
                }
            }
            Grab::End(item) => {
                if let Some((a, _)) = self.span(item) {
                    let t = self.snap_value(t, grab, per_px, alt);
                    self.set_span(item, (a, t.max(a + MIN_LEN).min(d)));
                }
            }
            Grab::TrimStart => {
                let t = self.snap_value(t, grab, per_px, alt);
                let (_, to) = self.project.trim(d);
                self.project.trim = Some((t.min(to - MIN_LEN).max(0.0), to));
                self.player.seek(self.project.trim(d).0);
            }
            Grab::TrimEnd => {
                let t = self.snap_value(t, grab, per_px, alt);
                let (from, _) = self.project.trim(d);
                self.project.trim = Some((from, t.max(from + MIN_LEN).min(d)));
                self.player.seek(self.project.trim(d).1);
            }
        }
        // La cámara se recalcula al soltar; mientras, la vista previa usa la de antes.
        cx.notify();
    }

    // --- Apuntar un zoom o un difuminado en la vista previa -------------------------

    /// El zoom que se está apuntando: uno elegido, en pausa. La vista previa
    /// muestra entonces el cuadro entero con el recuadro del zoom encima.
    fn aiming(&self) -> Option<usize> {
        match self.selected {
            Some(Item::Zoom(i)) if self.preview.is_none() && !self.player.playing() && i < self.project.zooms.len() => Some(i),
            _ => None,
        }
    }

    /// El difuminado que se está editando: uno elegido, en pausa. También se ve el cuadro
    /// entero, con su recuadro para mover y cambiar de tamaño.
    fn blur_aiming(&self) -> Option<usize> {
        match self.selected {
            Some(Item::Blur(i)) if self.preview.is_none() && !self.player.playing() && i < self.project.blurs.len() => Some(i),
            _ => None,
        }
    }

    /// El recuadro del zoom `i` en el video entero (0–1): su centro y su lado.
    /// Siguiendo al mouse, donde lo tiene la cámara en ese momento.
    fn aim_rect(&self, i: usize) -> Option<(f64, f64, f64)> {
        let z = self.project.zooms.get(i)?;
        let size = 1.0 / z.scale.max(1.0);
        let half = size / 2.0;
        let (x, y) = match z.focus {
            Focus::At(x, y) => (x, y),
            Focus::Auto => {
                let [x, y, w, h] = self.motion.crop(self.now().clamp(z.start, z.end));
                (x + w / 2.0, y + h / 2.0)
            }
        };
        Some((x.clamp(half, 1.0 - half), y.clamp(half, 1.0 - half), size))
    }

    /// Un punto de la ventana en el video entero (0–1).
    fn stage_point(&self, p: gpui::Point<Pixels>) -> Option<(f64, f64)> {
        let bounds = self.stage.get()?;
        let (vx, vy, vw, vh) = self.video_rect;
        let x = (f32::from(p.x) - f32::from(bounds.origin.x) - vx) / vw.max(1.0);
        let y = (f32::from(p.y) - f32::from(bounds.origin.y) - vy) / vh.max(1.0);
        Some((x as f64, y as f64))
    }

    fn aim_press(&mut self, i: usize, p: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let (Some((x, y)), Some((ax, ay, size))) = (self.stage_point(p), self.aim_rect(i)) else { return };
        let half = size / 2.0;
        let (_, _, vw, vh) = self.video_rect;
        // La esquina de abajo a la derecha (con un blanco de unos 14 puntos).
        if ((x - ax - half) as f32 * vw).abs() < CORNER_HIT && ((y - ay - half) as f32 * vh).abs() < CORNER_HIT {
            self.grab = Some(Grab::AimSize(i));
            return;
        }
        // Adentro se toma donde se apretó; afuera, el recuadro salta ahí.
        let inside = (x - ax).abs() <= half && (y - ay).abs() <= half;
        let (dx, dy) = if inside { (x - ax, y - ay) } else { (0.0, 0.0) };
        self.grab = Some(Grab::Aim(i, dx, dy));
        self.aim_to(p, cx);
    }

    fn aim_to(&mut self, p: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some((x, y)) = self.stage_point(p) else { return };
        match self.grab {
            Some(Grab::Aim(i, dx, dy)) => {
                if let Some(z) = self.project.zooms.get_mut(i) {
                    let half = 0.5 / z.scale.max(1.0);
                    z.focus = Focus::At((x - dx).clamp(half, 1.0 - half), (y - dy).clamp(half, 1.0 - half));
                    z.auto = false;
                }
            }
            Some(Grab::AimSize(i)) => {
                let Some((ax, ay, _)) = self.aim_rect(i) else { return };
                if let Some(z) = self.project.zooms.get_mut(i) {
                    // Desde el centro: el lado es el doble de lo más lejos del mouse.
                    let half = (x - ax).abs().max((y - ay).abs()).max(0.01);
                    z.scale = ((0.5 / half).clamp(1.1, 4.0) * 20.0).round() / 20.0;
                    z.focus = Focus::At(ax, ay);
                    z.auto = false;
                }
            }
            Some(Grab::BlurMove(i, dx, dy)) => {
                if let Some(blur) = self.project.blurs.get_mut(i) {
                    blur.rect = blur_moved(blur.rect, (x, y), (dx, dy));
                }
            }
            Some(Grab::BlurSize(i)) => {
                if let Some(blur) = self.project.blurs.get_mut(i) {
                    blur.rect = blur_resized(blur.rect, (x, y));
                }
            }
            _ => return,
        }
        cx.notify();
    }

    /// Apretar sobre el recuadro del difuminado `i`: la esquina de abajo a la derecha cambia
    /// su tamaño; adentro lo mueve (tomado donde se apretó); afuera, el recuadro salta ahí
    /// con el puntero en su centro.
    fn blur_press(&mut self, i: usize, p: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let (Some((x, y)), Some(rect)) = (self.stage_point(p), self.project.blurs.get(i).map(|b| b.rect)) else { return };
        let [bx, by, bw, bh] = rect;
        let (_, _, vw, vh) = self.video_rect;
        if ((x - bx - bw) as f32 * vw).abs() < CORNER_HIT && ((y - by - bh) as f32 * vh).abs() < CORNER_HIT {
            self.grab = Some(Grab::BlurSize(i));
            return;
        }
        let inside = x >= bx && x <= bx + bw && y >= by && y <= by + bh;
        let (dx, dy) = if inside { (x - bx, y - by) } else { (bw / 2.0, bh / 2.0) };
        self.grab = Some(Grab::BlurMove(i, dx, dy));
        self.aim_to(p, cx);
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        self.snap = None;
        let range = self.range.take();
        match self.grab.take() {
            Some(Grab::Head(true)) => self.player.play(),
            Some(Grab::Head(false)) | None => {}
            // El rango marcado queda como un corte (si dura lo suficiente) y se elige.
            Some(Grab::Range(_)) => {
                if let Some((a, b)) = range.map(|(a, b)| (a.min(b), a.max(b))).filter(|(a, b)| b - a >= MIN_LEN) {
                    self.project.cuts.push((a, b));
                    let cut = Item::Cut(self.project.cuts.len() - 1);
                    self.selected = Some(cut);
                    self.born = Some((cut, Instant::now()));
                    self.changed(cx);
                }
            }
            // Un arrastre en la vista previa, aunque dure varios cuadros, es un solo paso.
            Some(Grab::Aim(..) | Grab::AimSize(_)) => self.changed_keyed("aim", cx),
            Some(Grab::BlurMove(..) | Grab::BlurSize(_)) => self.changed_keyed("blur-rect", cx),
            Some(_) => self.changed(cx),
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Con el diálogo de exportar abierto, las teclas son suyas.
        if self.export_key(event, window, cx) {
            return;
        }
        // La hoja de atajos: Esc o «?» la cierran; mientras está abierta, el resto de las teclas no hace nada.
        if self.help_open {
            let (key, mods) = (event.keystroke.key.as_str(), &event.keystroke.modifiers);
            if key == "escape" || keys::is_help_key(key, event.keystroke.key_char.as_deref(), mods) {
                self.help_open = false;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        // Escribiendo al asistente: Intro envía, Esc devuelve el foco, y ningún atajo se dispara.
        if assistant::on_key(&self.assistant, event, window, &self.focus, cx) {
            if event.keystroke.key == "escape" || event.keystroke.key == "enter" {
                cx.stop_propagation();
            }
            return;
        }
        // Escribiendo un color en hex (¡la A es un dígito!): ningún atajo se dispara.
        if self.style_ui.editing(window, cx) {
            if event.keystroke.key == "escape" || event.keystroke.key == "enter" {
                window.focus(&self.focus);
                cx.stop_propagation();
            } else if let Some(dir) = keys::tab_step(&event.keystroke.key, &event.keystroke.modifiers) {
                self.step_focus(dir, window, cx);
                cx.stop_propagation();
            }
            return;
        }
        // Escribiendo un texto, las teclas son del campo. Si el campo conserva el foco sin
        // que haya un texto elegido (se quitó o se rehizo el texto), el foco vuelve al editor
        // y la tecla sigue como atajo: si no, el teclado quedaba muerto.
        if self.text_field.read(cx).focus_handle(cx).is_focused(window) {
            if matches!(self.selected, Some(Item::Text(_))) {
                if event.keystroke.key == "escape" || event.keystroke.key == "enter" {
                    window.focus(&self.focus);
                    cx.stop_propagation();
                } else if let Some(dir) = keys::tab_step(&event.keystroke.key, &event.keystroke.modifiers) {
                    // Tab saca el foco del campo hacia el control siguiente (o el anterior, con Shift).
                    self.step_focus(dir, window, cx);
                    cx.stop_propagation();
                }
                return;
            }
            window.focus(&self.focus);
        }
        // Tab recorre los controles; «?» abre la hoja de atajos.
        if let Some(dir) = keys::tab_step(&event.keystroke.key, &event.keystroke.modifiers) {
            self.step_focus(dir, window, cx);
            cx.stop_propagation();
            return;
        }
        if keys::is_help_key(&event.keystroke.key, event.keystroke.key_char.as_deref(), &event.keystroke.modifiers) {
            self.help_open = true;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let shift = event.keystroke.modifiers.shift;
        let command = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
        if command {
            match event.keystroke.key.as_str() {
                "z" if shift => self.redo(window, cx),
                "z" => self.undo(window, cx),
                "y" => self.redo(window, cx),
                "e" => self.open_export(cx),
                // El resto con ⌘ no es del editor.
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        match event.keystroke.key.as_str() {
            "space" => self.toggle_play(cx),
            "left" => self.seek(self.now() - if shift { 5.0 } else { 1.0 }, cx),
            "right" => self.seek(self.now() + if shift { 5.0 } else { 1.0 }, cx),
            "home" => self.seek(self.project.trim(self.duration).0, cx),
            "end" => self.seek(self.project.trim(self.duration).1, cx),
            "s" => self.add_cut(window, cx),
            "z" => self.add_zoom(window, cx),
            "t" => self.add_text(window, cx),
            "v" => self.add_speed(window, cx),
            "b" => self.add_blur(window, cx),
            "a" => self.autofocus(window, cx),
            // La línea de tiempo: acercar, alejar y ajustar todo.
            "=" | "+" => self.zoom_step(timeline::ZOOM_STEP, cx),
            "-" => self.zoom_step(1.0 / timeline::ZOOM_STEP, cx),
            "0" => self.fit_view(cx),
            "backspace" | "delete" => self.delete_selected(window, cx),
            "escape" if self.preview.is_some() => {
                self.close_preview();
                cx.notify();
            }
            "escape" => self.select(None, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // --- Vista previa exportada ("Ver cómo queda") -------------------------------------

    /// «Ver cómo queda»: exporta unos segundos desde el cabezal y los muestra en
    /// la vista previa, tal como saldrán. Otra vez lo cancela o lo cierra.
    fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview.is_some() {
            self.close_preview();
            cx.notify();
            return;
        }
        // Dos escritores sobre la misma fuente a la vez no: la exportación tiene prioridad.
        if self.export_busy() {
            return;
        }
        let Some(out) = preview_path(&self.path) else { return };
        let from = self.now();
        let to = from + PREVIEW_LEN;
        let segments: Vec<_> = self
            .project
            .segments(self.duration)
            .into_iter()
            .filter_map(|(a, b)| {
                let (a, b) = (a.max(from), b.min(to));
                (b - a > 0.1).then_some((a, b))
            })
            .collect();
        if segments.is_empty() {
            return;
        }
        self.player.pause();
        let progress = Arc::new(AtomicU64::new(0f64.to_bits()));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = self.job(segments, progress.clone(), cancel.clone(), true);
        self.preview = Some(Preview::Rendering { progress, cancel: cancel.clone() });
        let source = self.source.clone();
        cx.spawn(async move |view, cx| {
            let target = out.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    if let Some(dir) = target.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    atic_capture::stream::export(&source, &target, job)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                // Si se canceló o cambió el proyecto mientras tanto, ya no va (y si nadie
                // pidió otra, su archivo tampoco).
                if cancel.load(Ordering::Relaxed) || !matches!(view.preview, Some(Preview::Rendering { .. })) {
                    if view.preview.is_none() {
                        let _ = std::fs::remove_file(&out);
                    }
                    return;
                }
                view.preview = match result.and_then(|()| VideoPlayer::open(&out)) {
                    Ok(player) => {
                        player.seek(0.0);
                        player.play();
                        Some(Preview::Ready(player))
                    }
                    Err(error) => {
                        tracing::warn!(%error, "editor: no se pudo preparar la vista previa");
                        let _ = std::fs::remove_file(&out);
                        None
                    }
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Cierra «Ver cómo queda» (o lo cancela) y borra su archivo temporal.
    fn close_preview(&mut self) {
        match self.preview.take() {
            Some(Preview::Rendering { cancel, .. }) => cancel.store(true, Ordering::Relaxed),
            // El reproductor se suelta antes de borrar el archivo que tiene abierto.
            Some(Preview::Ready(player)) => {
                player.pause();
                drop(player);
                self.remove_preview_file();
            }
            None => {}
        }
    }

    fn remove_preview_file(&self) {
        if let Some(path) = preview_path(&self.path) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// El reproductor a su ritmo: dentro de un tramo con velocidad, a esa velocidad y sin audio
    /// (lo exportado queda mudo ahí); fuera, normal. Solo habla con el reproductor si cambia.
    fn sync_playback(&mut self) {
        let wanted = if self.player.playing() { playback_at(&self.spans, self.now()) } else { (NORMAL_RATE, false) };
        if wanted != self.playback {
            self.player.set_rate(wanted.0);
            self.player.set_volume(if wanted.1 { 0.0 } else { 1.0 });
            self.playback = wanted;
        }
    }

    // --- Piezas -----------------------------------------------------------------------

    /// Un botón de ícono (y nombre, si hay), del estilo de la ventana: es el botón de las barras (arriba, el
    /// transporte, los diálogos), sin forma en reposo. Las acciones de un panel son `action`.
    #[allow(clippy::too_many_arguments)]
    fn button(
        &self,
        pal: &Ed,
        id: &'static str,
        icon: &'static str,
        label: Option<&'static str>,
        tip: &'static str,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.button_keyed(pal, id, icon, label, None, tip, act, cx)
    }

    /// Como `button`, con la tecla de su atajo como un cuadrito al final del nombre («Cortar» `S`).
    #[allow(clippy::too_many_arguments)]
    fn button_keyed(
        &self,
        pal: &Ed,
        id: &'static str,
        icon: &'static str,
        label: Option<&'static str>,
        key: Option<&'static str>,
        tip: &'static str,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (text, hover, sub, control) = (pal.text, pal.hover, pal.sub, pal.control);
        let el = div()
            .id(id)
            .h(px(BUTTON))
            .min_w(px(BUTTON))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .when(label.is_some(), |el| el.pl(px(8.)).pr(px(10.)))
            .rounded(px(pal.r_chip.min(BUTTON / 2.0)))
            .cursor_pointer()
            .text_size(px(T_CONTROL))
            .tooltip(keys::tip(tip));
        self.interactive(pal.accent, el, act, cx)
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                let fg = h.mix(text.opacity(0.85), text);
                el.bg(h.mix(hover.opacity(0.0), hover))
                    .text_color(fg)
                    .child(svg().path(icon).flex_none().size(px(16. - 1.5 * h.press)).text_color(fg))
                    .when_some(label, |el, label| el.child(label))
                    .when_some(key.filter(|_| label.is_some()), |el, key| el.child(key_cap(key, sub, control)))
            })
            .into_any_element()
    }

    /// Una acción de un panel (agregar, quitar, ver cómo queda): un botón con forma, con fondo y borde finos en
    /// reposo, a todo el ancho de su columna y el contenido a la izquierda. Es el nivel intermedio entre el
    /// botón de las barras (`button`, sin forma) y el principal (`primary`, con el acento). `Danger` pinta lo
    /// que borra con el color de «cortar» y lo tiñe al pasar el mouse.
    #[allow(clippy::too_many_arguments)]
    fn action(
        &self,
        pal: &Ed,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        tip: &'static str,
        tone: Tone,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (text, sub, control, cut) = (pal.text, pal.sub, pal.control, pal.cut);
        let (rest_a, hover_a) = action_fill(pal.glass);
        let border = action_border(pal);
        let radius = pal.r_ctl;
        let el = div()
            .id(id)
            .h(px(BUTTON))
            .w_full()
            .px(px(10.))
            .flex()
            .items_center()
            .justify_start()
            .gap(px(8.))
            .rounded(px(pal.r_ctl))
            .cursor_pointer()
            .text_size(px(T_CONTROL))
            .tooltip(keys::tip(tip));
        self.interactive(pal.accent, el, act, cx)
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                let (fg, icon_fg) = match tone {
                    Tone::Normal => (text, h.mix(sub, text)),
                    Tone::Danger => (cut, cut),
                };
                let rest = control.opacity(rest_a);
                let over = match tone {
                    Tone::Normal => control.opacity(hover_a),
                    Tone::Danger => cut.opacity(ACTION_DANGER_HOVER),
                };
                // El borde fino es una capa aparte (el borde del elemento es del anillo de foco) y se queda: lo que
                // cambia es el relleno (y el ícono, al apretar).
                el.relative()
                    .bg(mix(rest, over, h.t))
                    .text_color(fg)
                    .when_some(border, |el, c| el.child(div().absolute().inset_0().rounded(px(radius)).border_1().border_color(c)))
                    .child(svg().path(icon).flex_none().size(px(15. - 1.5 * h.press)).text_color(icon_fg))
                    .child(div().flex_1().min_w_0().truncate().child(label))
            })
            .into_any_element()
    }

    /// Un botón principal (relleno con el acento).
    fn primary(&self, pal: &Ed, id: &'static str, icon: &'static str, label: SharedString, tip: &'static str, act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.primary_w(pal, id, icon, label, tip, None, act, cx)
    }

    /// Como `primary`, con un ancho mínimo (`min_w`): un texto que cambia («Exportar» →
    /// «Exportando 42 %» → «Listo») no mueve lo que tiene al lado.
    #[allow(clippy::too_many_arguments)]
    fn primary_w(&self, pal: &Ed, id: &'static str, icon: &'static str, label: SharedString, tip: &'static str, min_w: Option<f32>, act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (accent, on) = (pal.accent, pal.on_accent);
        let el = div()
            .id(id)
            .h(px(BUTTON))
            .pl(px(10.))
            .pr(px(13.))
            .flex()
            .flex_none()
            .items_center()
            .when_some(min_w, |el, w| el.min_w(px(w)).justify_center())
            .gap(px(7.))
            .rounded(px(pal.r_chip.min(BUTTON / 2.0)))
            .cursor_pointer()
            .text_size(px(T_CONTROL))
            .font_weight(gpui::FontWeight::MEDIUM)
            .tooltip(keys::tip(tip));
        // Sobre el acento, el anillo es del color del texto del botón (con el acento no se vería).
        self.interactive(on, el, act, cx)
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                el.bg(accent.opacity(0.92 + 0.08 * h.t - 0.08 * h.press))
                    .text_color(on)
                    .child(svg().path(icon).size(px(15.)).text_color(on))
                    .child(label)
            })
            .into_any_element()
    }

    /// Una opción de un grupo (escala, posición, tamaño): marcada si está elegida.
    fn chip(&self, pal: &Ed, id: SharedString, label: SharedString, on: bool, act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (text, muted, sel, hover) = (pal.text, pal.sub, pal.sel, pal.hover);
        // El fondo y el color del texto se funden al elegirla; apretada, se acerca al de elegida.
        let on_t = self.fluid.toggle(id.clone(), on, fluid::CHIP_FADE, fluid::CHIP_FADE);
        let el = div()
            .id(id.clone())
            .h(px(28.))
            .px(px(8.))
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .rounded(px(pal.r_chip.min(14.0)))
            .text_size(px(T_BODY))
            .cursor_pointer()
            .when(on, |el| el.font_weight(gpui::FontWeight::MEDIUM))
            .child(label);
        self.interactive(pal.accent, el, act, cx)
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| el.bg(fluid::select_bg(h, on_t, hover, sel)).text_color(mix(muted, text, on_t)))
            .into_any_element()
    }

    /// Un interruptor con nombre: una fila con su descripción a la izquierda.
    fn switch_row(&self, pal: &Ed, id: &'static str, label: &'static str, on: bool, act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (accent, control, text, hover, muted, on_accent) = (pal.accent, pal.control, pal.text, pal.hover, pal.muted, pal.on_accent);
        // La perilla se desliza (y los colores la siguen); sin movimiento, salta.
        let on_t = self.fluid.glide(format!("{id}-switch"), if on { 1.0 } else { 0.0 }, fluid::SWITCH_SLIDE, false);
        let el = div()
            .id(id)
            .h(px(32.))
            .px(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(px(T_CONTROL))
            .text_color(text)
            .gap(px(10.));
        self.interactive(accent, el, act, cx)
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| {
                // Apretado, la perilla se alarga unos px hacia dentro (anclada al borde del lado en que está).
                let stretch = fluid::KNOB_PRESS * h.press;
                let x = crate::anim::lerp(SWITCH_PAD, SWITCH_W - SWITCH_PAD - SWITCH_KNOB - stretch, on_t);
                el.bg(mix(hover.opacity(0.0), hover, h.t))
                    .child(div().flex_1().min_w_0().child(label))
                    .child(
                        div()
                            .flex_none()
                            .relative()
                            .w(px(SWITCH_W))
                            .h(px(SWITCH_H))
                            .rounded_full()
                            .bg(mix(control, accent, on_t))
                            .child(div().absolute().left(px(x)).top(px(SWITCH_PAD)).w(px(SWITCH_KNOB + stretch)).h(px(SWITCH_KNOB)).rounded_full().bg(mix(muted, on_accent, on_t))),
                    )
            })
            .into_any_element()
    }

    /// Una sección de la barra lateral: su título (11, mayúsculas, apagado) y lo de adentro.
    fn section(pal: &Ed, title: &'static str, body: Vec<gpui::AnyElement>) -> Div {
        Self::group(pal, div().text_size(px(T_SMALL)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(pal.sub).child(title.to_uppercase()), body)
    }

    /// La primera sección de una pestaña: su título es el de la página (13, seminegrita, sin mayúsculas), para
    /// saber dónde se está; las demás secciones siguen en 11 mayúsculas.
    fn page(pal: &Ed, title: &'static str, body: Vec<gpui::AnyElement>) -> Div {
        Self::group(pal, div().text_size(px(T_CONTROL)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(pal.text).child(title), body)
    }

    fn group(pal: &Ed, heading: Div, body: Vec<gpui::AnyElement>) -> Div {
        div().flex().flex_col().gap(px(8.)).pb(px(12.)).mb(px(12.)).border_b_1().border_color(pal.border.opacity(0.35)).child(heading).children(body)
    }

    /// Un grupo de opciones con su nombre arriba (11 más tenue que las opciones) y el fondo de los grupos.
    fn row(pal: &Ed, label: &'static str, items: Vec<gpui::AnyElement>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(label))
            .child(div().flex().gap(px(4.)).p(px(3.)).rounded(px(pal.r_chip.min(17.0))).bg(pal.control.opacity(0.55)).children(items))
            .into_any_element()
    }

    /// La vista previa: el cuadro de salida tal cual lo exportaría (el `Composer` de la
    /// exportación lo compone: `scene.rs`), con las capas interactivas de GPUI encima.
    fn stage(&mut self, pal: &Ed, cx: &mut Context<Self>) -> gpui::AnyElement {
        let stage_bg = if pal.light { gpui::black().opacity(0.06) } else { gpui::black().opacity(0.35) };
        let area = self.stage.get().map(|b| (f32::from(b.size.width), f32::from(b.size.height)));
        // La insignia «Vista previa» entra con un fundido. (El video no se cruza con la vista en
        // vivo: `paint_surface` de GPUI no respeta la opacidad, así que el cambio de imagen es directo.)
        let badge_t = self.fluid.toggle("stage-badge", matches!(self.preview, Some(Preview::Ready(_))), fluid::OVERLAY_IN, fluid::OVERLAY_OUT);
        // «Ver cómo queda»: lo exportado, tal cual (ya trae el marco y el fondo).
        if let Some(Preview::Ready(player)) = &mut self.preview {
            let frame = player.frame();
            let (pw, ph) = player.size();
            let (ox, oy, ow, oh) = area.map(|area| fit(area, pw as f32 / ph.max(1) as f32, 18.0)).unwrap_or((0.0, 0.0, 0.0, 0.0));
            let slot = self.stage.clone();
            let badge = div()
                .absolute()
                .top(px(10.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .opacity(badge_t)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded_full()
                        .bg(gpui::black().opacity(0.6))
                        .text_size(px(T_BODY))
                        .text_color(gpui::white())
                        .child(svg().path("icons/play.svg").size(px(11.)).text_color(gpui::white()))
                        .child(t("pill.editor.previewBadge")),
                );
            return div()
                .id("editor-stage")
                .flex_1()
                .min_h_0()
                .relative()
                .rounded(px(pal.r_panel))
                .bg(stage_bg)
                .overflow_hidden()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(|view, _: &MouseDownEvent, _, cx| view.toggle_play(cx)))
                .child(canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full())
                .children(frame.map(|frame| gpui::surface(frame.0.clone()).absolute().left(px(ox)).top(px(oy)).w(px(ow)).h(px(oh))))
                .child(badge)
                .into_any_element();
        }
        // Apuntando un zoom o un difuminado: el cuadro entero, con su recuadro encima.
        let aim = self.aiming().and_then(|i| Some((i, self.aim_rect(i)?)));
        let blur_aim = self.blur_aiming();
        let frame = self.player.frame();
        let slot = self.stage.clone();
        let video_size = self.player.size();
        let look = self.project.look;
        // El cuadro de salida con la proporción elegida, centrado en el escenario.
        let full_out = export_panel::render_dims(&self.project.export, video_size);
        let (ox, oy, ow, oh) = area.map(|area| fit(area, canvas_of(full_out, video_size, look).aspect(), 18.0)).unwrap_or((0.0, 0.0, 0.0, 0.0));
        // Lo que suena o se arrastra se compone a la escala que aguanta el equipo; quieto, entero.
        let interactive = self.player.playing() || self.grab.is_some();
        let scale = if interactive { self.compose.scale() } else { 1.0 };
        let out = scene::preview_dims(full_out, (ow * self.scale, oh * self.scale), scale);
        // El video dentro del cuadro, entero y centrado, con el margen del marco: lo mismo que al exportar.
        let frame_of = canvas_of(out, video_size, look);
        let [fx, fy, fw, fh] = frame_of.video;
        let (vx, vy, vw, vh) = (ox + fx * ow, oy + fy * oh, fw * ow, fh * oh);
        self.video_rect = (vx, vy, vw, vh);
        let full_view = aim.is_some() || blur_aim.is_some();
        // La superficie: lo compuesto; si no se puede componer, el cuadro tal cual. Sin cuadro
        // todavía (o sin medir el escenario), el fondo oscuro del cuadro de salida.
        let picture = match (&frame, area.is_some()) {
            (Some(frame), true) => self.composed(frame, frame_of.out, full_view, interactive),
            _ => None,
        };
        let surface = match (picture, frame) {
            (Some(source), _) => Some(gpui::surface(source).absolute().left(px(ox)).top(px(oy)).w(px(ow)).h(px(oh)).object_fit(gpui::ObjectFit::Fill)),
            (None, Some(frame)) if area.is_some() => Some(gpui::surface(frame.0.clone()).absolute().left(px(vx)).top(px(vy)).w(px(vw)).h(px(vh)).object_fit(gpui::ObjectFit::Fill)),
            _ => None,
        };
        // El recuadro de un difuminado en la vista (el cuadro está entero: 0–1 del video).
        let blur_box = |rect: [f64; 4]| {
            let [bx, by, bw, bh] = rect;
            ((bx as f32) * vw, (by as f32) * vh, (bw as f32) * vw, (bh as f32) * vh)
        };
        // El recuadro que se edita: el borde con el acento y su esquina para cambiarle el tamaño.
        // Lo de adentro se ve tal cual lo deja el exportador (ya viene difuminado en la superficie).
        let blur_edit: Vec<gpui::AnyElement> = match blur_aim.and_then(|i| self.project.blurs.get(i)) {
            Some(blur) => {
                let (left, top, w, h) = blur_box(blur.rect);
                vec![
                    div().absolute().left(px(left)).top(px(top)).w(px(w)).h(px(h)).rounded(px(3.)).border_2().border_color(pal.accent).into_any_element(),
                    div()
                        .id("editor-blur-size")
                        .absolute()
                        .left(px(left + w - 7.0))
                        .top(px(top + h - 7.0))
                        .size(px(14.))
                        .rounded_full()
                        .bg(gpui::white())
                        .border_2()
                        .border_color(pal.accent)
                        .cursor(gpui::CursorStyle::ResizeUpLeftDownRight)
                        .into_any_element(),
                ]
            }
            None => Vec::new(),
        };
        // El recuadro del zoom: lo de afuera en sombra, el borde con el acento, la
        // escala y la esquina que la cambia.
        let aim_overlay: Vec<gpui::AnyElement> = match aim {
            Some((i, (ax, ay, size))) => {
                let (l, top, w, h) = (((ax - size / 2.0) as f32) * vw, ((ay - size / 2.0) as f32) * vh, size as f32 * vw, size as f32 * vh);
                let shade = gpui::black().opacity(0.5);
                let scale = self.project.zooms.get(i).map_or(2.0, |z| z.scale);
                vec![
                    div().absolute().left_0().top_0().w(px(vw)).h(px(top.max(0.0))).bg(shade).into_any_element(),
                    div().absolute().left_0().top(px(top + h)).w(px(vw)).h(px((vh - top - h).max(0.0))).bg(shade).into_any_element(),
                    div().absolute().left_0().top(px(top)).w(px(l.max(0.0))).h(px(h)).bg(shade).into_any_element(),
                    div().absolute().left(px(l + w)).top(px(top)).w(px((vw - l - w).max(0.0))).h(px(h)).bg(shade).into_any_element(),
                    div().absolute().left(px(l)).top(px(top)).w(px(w)).h(px(h)).rounded(px(4.)).border_2().border_color(pal.accent).into_any_element(),
                    div()
                        .absolute()
                        .left(px(l + 8.0))
                        .top(px(top + 8.0))
                        .px(px(7.))
                        .py(px(2.))
                        .rounded_full()
                        .bg(gpui::black().opacity(0.65))
                        .text_size(px(T_SMALL))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(gpui::white())
                        .child(format!("{scale}×"))
                        .into_any_element(),
                    div()
                        .id("editor-aim-size")
                        .absolute()
                        .left(px(l + w - 7.0))
                        .top(px(top + h - 7.0))
                        .size(px(14.))
                        .rounded_full()
                        .bg(gpui::white())
                        .border_2()
                        .border_color(pal.accent)
                        .cursor(gpui::CursorStyle::ResizeUpLeftDownRight)
                        .into_any_element(),
                ]
            }
            None => Vec::new(),
        };
        // Preparando «Ver cómo queda»: cuánto va.
        let rendering = match &self.preview {
            Some(Preview::Rendering { progress, .. }) => {
                let p = f64::from_bits(progress.load(Ordering::Relaxed));
                Some(
                    div().absolute().inset_0().flex().items_center().justify_center().child(
                        div()
                            .px(px(14.))
                            .py(px(7.))
                            .rounded_full()
                            .bg(gpui::black().opacity(0.65))
                            .text_size(px(T_CONTROL))
                            .text_color(gpui::white())
                            .child(crate::i18n::tf("pill.editor.previewing", &[("pct", &((p * 100.0).round() as u32))])),
                    ),
                )
            }
            _ => None,
        };
        let aiming = aim.map(|(i, _)| i);
        // Los recuadros de apuntar y «Preparando…» entran con un fundido corto.
        let overlay_t = self.fluid.toggle("stage-aim", !blur_edit.is_empty() || !aim_overlay.is_empty(), fluid::OVERLAY_IN, fluid::OVERLAY_OUT);
        let rendering_t = self.fluid.toggle("stage-rendering", rendering.is_some(), fluid::OVERLAY_IN, fluid::OVERLAY_OUT);
        // El zoom que cubre el cabezal, en una esquina (nunca mueve el video); entra y sale con un fundido.
        let now = self.now();
        let zoomed = self.project.zooms.iter().find(|z| now >= z.start && now <= z.end).map(|z| z.scale);
        if let Some(scale) = zoomed {
            self.zoom_last.set(scale);
        }
        let zoom_t = self.fluid.toggle("stage-zoom", zoomed.is_some() && !full_view && area.is_some(), fluid::OVERLAY_IN, fluid::OVERLAY_OUT);
        let zoom_chip = (zoom_t > 0.0).then(|| {
            let text = format!("{}×", style_panel::fmt_num(self.zoom_last.get(), !crate::i18n::english()));
            div()
                .absolute()
                .left(px(ox + 8.0))
                .top(px(oy + 8.0))
                .px(px(7.))
                .py(px(2.))
                .rounded_full()
                .opacity(zoom_t)
                .bg(gpui::black().opacity(0.55))
                .text_size(px(T_SMALL))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(gpui::white())
                .child(text)
        });
        // El eco del clic: el ícono de lo que quedó (reproducir o pausar) entra, se queda y sale.
        let echo = self.echo.and_then(|(at, playing)| {
            let elapsed = at.elapsed();
            let _ = self.fluid.since(at, fluid::ECHO_IN + fluid::ECHO_HOLD + fluid::ECHO_OUT);
            (elapsed < fluid::ECHO_IN + fluid::ECHO_HOLD + fluid::ECHO_OUT).then(|| {
                let (alpha, grown) = echo_state(elapsed, self.fluid.reduced());
                div().absolute().inset_0().flex().items_center().justify_center().opacity(alpha).child(
                    div()
                        .size(px(ECHO_SIZE * grown))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(gpui::black().opacity(0.5))
                        .child(svg().path(if playing { "icons/play.svg" } else { "icons/pause.svg" }).size(px(ECHO_SIZE * grown * 0.45)).text_color(gpui::white())),
                )
            })
        });
        div()
            .id("editor-stage")
            .flex_1()
            .min_h_0()
            .relative()
            .rounded(px(pal.r_panel))
            .bg(stage_bg)
            .overflow_hidden()
            .cursor(match (aiming.is_some() || blur_aim.is_some(), self.grab) {
                (true, Some(Grab::Aim(..) | Grab::BlurMove(..))) => gpui::CursorStyle::ClosedHand,
                (true, _) => gpui::CursorStyle::OpenHand,
                _ => gpui::CursorStyle::PointingHand,
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| match (aiming, blur_aim) {
                    (Some(i), _) => view.aim_press(i, event.position, cx),
                    (None, Some(i)) => view.blur_press(i, event.position, cx),
                    (None, None) => {
                        view.toggle_play(cx);
                        view.echo = Some((Instant::now(), view.player.playing()));
                    }
                }),
            )
            .child(canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full())
            // El cuadro de salida mientras no hay imagen (y debajo de ella).
            .child(div().absolute().left(px(ox)).top(px(oy)).w(px(ow)).h(px(oh)).bg(gpui::black()))
            .children(surface)
            // Las capas interactivas, sobre el video.
            .child(div().absolute().left(px(vx)).top(px(vy)).w(px(vw)).h(px(vh)).overflow_hidden().opacity(overlay_t).children(blur_edit).children(aim_overlay))
            .children(rendering.map(|el| el.opacity(rendering_t)))
            .children(zoom_chip)
            .children(echo)
            .into_any_element()
    }

    /// Un botón de ícono que se apaga (sin clic ni tooltip) cuando no se puede usar.
    #[allow(clippy::too_many_arguments)]
    fn icon_button(
        &self,
        pal: &Ed,
        id: &'static str,
        icon: &'static str,
        tip: &'static str,
        enabled: bool,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if enabled {
            return self.button(pal, id, icon, None, tip, act, cx);
        }
        div()
            .h(px(BUTTON))
            .min_w(px(BUTTON))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .child(svg().path(icon).flex_none().size(px(16.)).text_color(pal.faint.opacity(DISABLED_ALPHA)))
            .into_any_element()
    }

    /// El aviso bajo la barra de arriba, con su X; `None` si no hay (ni se está yendo).
    /// Es una capa encima del contenido: entra y sale deslizándose desde arriba con un
    /// fundido sin mover nada del layout (animar el alto reharía la vista previa en cada
    /// cuadro). Al irse se queda dibujado con lo último que dijo (`notice_last`).
    fn notice_bar(&self, pal: &Ed, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let live = self.notice.as_ref().map(|notice| (SharedString::from(notice.text.clone()), notice.action.clone(), notice.error));
        let shown = self.fluid.toggle("editor-notice", live.is_some(), fluid::NOTICE_IN, fluid::NOTICE_OUT);
        if live.is_some() {
            *self.notice_last.borrow_mut() = live.clone();
        }
        let (text, action, error) = live.or_else(|| self.notice_last.borrow().clone().filter(|_| shown > 0.0))?;
        let (icon, tint) = if error { ("icons/circle-x.svg", pal.cut) } else { ("icons/circle-check.svg", pal.sub) };
        let hover = pal.hover;
        let rise = self.fluid.lift(shown, fluid::NOTICE_RISE);
        // Opaco: encima de la vista previa, un fondo translúcido no se leería.
        let mut base = if pal.glass { pal.bg } else { pal.panel };
        base.a = 1.0;
        let mut fill = if error { mix(base, pal.cut, 0.14) } else { mix(base, pal.control, 0.8) };
        fill.a = 1.0;
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(TOP_H))
                .px(px(GAP))
                .pt(px(GAP / 4.0))
                .flex()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .top(px(-rise))
                        .opacity(shown)
                        .w_full()
                        .max_w(px(NOTICE_MAX_W))
                        .occlude()
                        .px(px(12.))
                        .py(px(5.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(pal.r_ctl))
                        .border_1()
                        .border_color(if error { pal.cut.opacity(0.5) } else { pal.border.opacity(0.5) })
                        .bg(fill)
                        .text_size(px(T_CONTROL))
                        .text_color(pal.text)
                        .child(svg().path(icon).flex_none().size(px(14.)).text_color(tint))
                        .child(div().flex_1().min_w_0().truncate().child(text))
                        .children(action.map(|action| {
                            let label = action.label();
                            div()
                                .id("editor-notice-action")
                                .h(px(22.))
                                .px(px(8.))
                                .flex()
                                .flex_none()
                                .items_center()
                                .rounded_full()
                                .cursor_pointer()
                                .text_size(px(T_BODY))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal.accent)
                                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.run_notice_action(action.clone(), window, cx)))
                                .child(label)
                                .hover_bg("editor-notice-action-fx", hover.opacity(0.0), hover)
                        }))
                        .child(
                            div()
                                .id("editor-notice-close")
                                .size(px(20.))
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .tooltip(keys::tip(t("pill.editor.noticeClose")))
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    view.notice = None;
                                    cx.notify();
                                }))
                                .child(svg().path("icons/x.svg").size(px(12.)).text_color(pal.sub))
                                .hover_bg("editor-notice-close-fx", hover.opacity(0.0), hover),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Bajo la vista previa: reproducir, el tiempo y las herramientas de la línea de tiempo.
    fn transport(&self, pal: &Ed, cx: &mut Context<Self>) -> impl IntoElement {
        let playing = self.player.playing();
        // Lo que dura de verdad lo exportado: con los cortes y las velocidades.
        let length = self.project.out_length(self.duration);
        // Con poco ancho, las herramientas quedan solo con su ícono (el nombre sale en su tooltip).
        let wide = self.stage.get().is_some_and(|b| f32::from(b.size.width) >= TOOLS_LABELS_MIN_W);
        let named = |label: &'static str| wide.then_some(label);
        // Con ancho, cada herramienta muestra también su tecla.
        let key = |key: &'static str| wide.then_some(key);
        let tools = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(self.button_keyed(pal, "editor-cut", "icons/scissors.svg", named(t("pill.editor.cut")), key("S"), t("pill.editor.cutTip"), |view, window, cx| view.add_cut(window, cx), cx))
            .child(self.button_keyed(pal, "editor-zoom", "icons/magnify.svg", named(t("pill.editor.zoom")), key("Z"), t("pill.editor.zoomTip"), |view, window, cx| view.add_zoom(window, cx), cx))
            .child(self.button_keyed(pal, "editor-text", "icons/type.svg", named(t("pill.editor.text")), key("T"), t("pill.editor.textTip"), |view, window, cx| view.add_text(window, cx), cx))
            .child(self.button_keyed(pal, "editor-speed", "icons/gauge.svg", named(t("pill.editor.laneSpeed")), key("V"), t("pill.editor.side.speedAddTip"), |view, window, cx| view.add_speed(window, cx), cx))
            .child(self.button_keyed(pal, "editor-blur-zone", "icons/pixelate.svg", named(t("pill.editor.laneBlur")), key("B"), t("pill.editor.side.blurAddTip"), |view, window, cx| view.add_blur(window, cx), cx));
        // El botón de reproducir, redondo y al centro.
        let (accent, on) = (pal.accent, pal.on_accent);
        let play = div()
            .id("editor-play")
            .size(px(38.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .tooltip(keys::tip(t("pill.editor.playTip")))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_play(cx)))
            .fx("editor-play-fx", move |el, h| {
                el.bg(accent.opacity(0.92 + 0.08 * h.t - 0.1 * h.press))
                    .child(svg().path(if playing { "icons/pause.svg" } else { "icons/play.svg" }).size(px(17. - 1.5 * h.press)).text_color(on))
            });
        let center = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(self.button(pal, "editor-back", "icons/skip-back.svg", None, t("pill.editor.backTip"), |view, _, cx| view.seek(view.now() - 5.0, cx), cx))
            .child(play)
            .child(self.button(pal, "editor-forward", "icons/skip-forward.svg", None, t("pill.editor.forwardTip"), |view, _, cx| view.seek(view.now() + 5.0, cx), cx));
        let time = div()
            .flex()
            .items_baseline()
            .gap(px(6.))
            .font_family(pal.mono)
            .child(div().text_size(px(T_TITLE)).text_color(pal.text).child(clock_tenths(self.now())))
            .child(div().text_size(px(T_BODY)).text_color(pal.sub).child(format!("/ {}", clock(self.duration))))
            .when((length - self.duration).abs() > 0.05, |el| {
                // «Salida 1:40»: lo que dura lo exportado (con los cortes y las velocidades), no lo que falta.
                el.child(div().text_size(px(T_SMALL)).text_color(pal.sub).font_family(pal.font).child(crate::i18n::tf("pill.editor.outLength", &[("time", &clock(length))])))
            });
        div()
            .flex_none()
            .h(px(52.))
            .flex()
            .items_center()
            .child(div().flex_1().flex().child(tools))
            .child(center)
            .child(div().flex_1().flex().justify_end().child(time))
    }
}

impl Render for RecordingEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        self.scale = window.scale_factor();
        let pal = Ed::current();
        // Las animaciones de la interfaz anotan, mientras se arma el cuadro, si alguna sigue en curso.
        self.fluid.begin_frame();
        let playing = self.player.playing();
        // Sonando: salta lo cortado y para al final del recorte.
        if playing && self.grab.is_none() {
            let now = self.now();
            match self.project.next_kept(now, self.duration) {
                Some(next) if next > now + 0.02 => {
                    // El salto tarda unos cuadros en llegar: no se pide otra vez el mismo.
                    let again = self.last_skip.is_some_and(|(to, at)| (to - next).abs() < 1e-6 && at.elapsed() < Duration::from_millis(400));
                    if !again {
                        self.player.seek(next);
                        self.last_skip = Some((next, Instant::now()));
                    }
                }
                None => self.player.pause(),
                _ => {}
            }
        }
        // Dentro de un tramo con velocidad, el reproductor va a ese ritmo y sin audio.
        self.sync_playback();
        let exporting = self.export_busy();
        // Sonando, arrastrando, exportando o esperando el primer cuadro (o el de
        // un salto): un cuadro por pasada; quieta, nada.
        let previewing = match &self.preview {
            Some(Preview::Ready(player)) => player.playing() || !player.has_frame(),
            Some(Preview::Rendering { .. }) => true,
            None => false,
        };
        // Un salto en pausa (clic en la línea de tiempo, flechas, elegir un bloque): el reproductor entrega
        // el cuadro nuevo un instante después; se siguen pidiendo cuadros un rato corto para mostrarlo.
        let at = self.now();
        if !(at - self.seen_time).abs().lt(&1e-3) {
            self.seen_time = at;
            self.settle_until = Some(Instant::now() + SETTLE);
        }
        let settling = self.settle_until.is_some_and(|until| Instant::now() < until);
        if playing || previewing || self.grab.is_some() || exporting || settling || !self.player.has_frame() || self.stage.get().is_none() {
            window.request_animation_frame();
        }

        // «Guardado»: un fundido corto al guardar y otro al retirarse; fuera de eso no pide cuadros.
        let saved_elapsed = self.saved_at.map(|at| at.elapsed());
        let saved = saved_elapsed.map_or(0.0, saved_alpha);
        if saved_elapsed.is_some_and(saved_animating) {
            window.request_animation_frame();
        }
        // Arriba: el nombre, el «Guardado», deshacer y rehacer, y Exportar.
        let top = div()
            .h(px(TOP_H))
            .flex_none()
            .pl(px(10.))
            .pr(px(14.))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(chrome::lights())
            .child(svg().path("icons/video.svg").size(px(15.)).text_color(pal.sub))
            .child(div().text_size(px(T_CONTROL)).font_weight(gpui::FontWeight::MEDIUM).text_color(pal.text).child(self.title.clone()))
            // El autoguardado ya no es mudo: «Guardado» tenue, sin color de acento (se reconoce, no se celebra).
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .opacity(saved)
                    .text_size(px(T_SMALL))
                    .text_color(pal.sub)
                    .child(svg().path("icons/check.svg").size(px(11.)).text_color(pal.sub))
                    .child(t("pill.editor.saved")),
            )
            .child(chrome::drag(TOP_H))
            .child(self.button(&pal, "editor-folder", "icons/folder.svg", None, t("pill.editor.folderTip"), |view, _, _| {
                let _ = crate::paths::reveal(&view.path);
            }, cx))
            .child(self.icon_button(&pal, "editor-undo-top", "icons/undo.svg", t("pill.editor.side.undoTip"), self.history.can_undo(), |view, window, cx| view.undo(window, cx), cx))
            .child(self.icon_button(&pal, "editor-redo-top", "icons/redo.svg", t("pill.editor.side.redoTip"), self.history.can_redo(), |view, window, cx| view.redo(window, cx), cx))
            .child(self.export_button(&pal, cx))
            .child(chrome::controls_colored(window.is_maximized(), TOP_H, pal.text, pal.hover));
        let notice = self.notice_bar(&pal, cx);

        let stage = self.stage(&pal, cx);
        let transport = self.transport(&pal, cx);
        let sidebar = self.sidebar_panel(&pal, cx);

        // La línea de tiempo.
        let lanes_slot = self.lanes.clone();
        let d = self.duration;
        // Sonando, la vista sigue al cabezal si se sale.
        if playing && self.grab.is_none() {
            self.view = self.view.follow(self.now(), d);
        }
        let view = self.shown_view().clamped(d);
        let wave = if !self.levels_ready {
            timeline::Wave::Loading
        } else if self.levels.is_empty() {
            timeline::Wave::Silent
        } else if self.levels_silent {
            // Con audio pero sin señal (−91 dB): casi siempre es el permiso del micrófono; no se ve como «cargando».
            timeline::Wave::Muted
        } else {
            timeline::Wave::Levels(&self.levels)
        };
        // La selección de un bloque transiciona su borde y relleno; uno recién agregado entra con un
        // fundido y una escala. Ninguna toca el cabezal: ese sigue 1:1 al arrastre y al tiempo.
        let (ui, born, selected_now) = (&self.fluid, self.born, self.selected);
        let item_motion = |item: Item| -> (bool, f32, f32, f32) {
            let selected = selected_now == Some(item);
            let sel = ui.toggle(format!("tl-sel-{item:?}"), selected, fluid::BLOCK_SELECT, fluid::BLOCK_SELECT);
            let appear = born.filter(|(b, _)| *b == item).map_or(1.0, |(_, at)| ui.since(at, fluid::BLOCK_IN));
            (selected, sel, appear, ui.grow(appear, fluid::BLOCK_FROM_SCALE))
        };
        let block = |start: f64, end: f64, label: SharedString, item: Item, auto: bool| {
            let (selected, sel, appear, scale) = item_motion(item);
            // El nombre completo y su rango van en el globito: el bloque corto lo corta con «…» o no lo muestra.
            let tip = SharedString::from(format!("{label} · {}–{}", timeline::stamp(start, false), timeline::stamp(end, false)));
            timeline::Block { start, end, label, selected, sel, appear, scale, auto, key: format!("tl-block-{item:?}").into(), tip }
        };
        // La guía del imán y el rango marcado entran y salen con un fundido corto: se quedan
        // dibujados donde estaban mientras se desvanecen.
        let snap_t = ui.toggle("tl-snap", self.snap.is_some(), fluid::GUIDE_FADE, fluid::GUIDE_FADE);
        if let Some(at) = self.snap {
            self.snap_last.set(Some(at));
        }
        let snap_shown = self.snap.or(self.snap_last.get()).filter(|_| snap_t > 0.0).map(|at| (at, snap_t));
        let range_t = ui.toggle("tl-range", self.range.is_some(), fluid::GUIDE_FADE, fluid::GUIDE_FADE);
        if let Some(r) = self.range {
            self.range_last.set(Some(r));
        }
        let range_shown = self.range.or(self.range_last.get()).filter(|_| range_t > 0.0).map(|r| (r, range_t));
        // Las pistas vacías dicen qué va ahí; la pista se desvanece al crear el primer bloque.
        let hint = |key: &'static str, empty: bool| ui.toggle(key, empty, fluid::HINT_FADE, fluid::HINT_FADE);
        let hints = [
            hint("tl-hint-zoom", self.project.zooms.is_empty()),
            hint("tl-hint-speed", self.project.speeds.is_empty()),
            hint("tl-hint-blur", self.project.blurs.is_empty()),
            hint("tl-hint-text", self.project.texts.is_empty()),
        ];
        let data = timeline::TimelineData {
            duration: d,
            view,
            now: self.now(),
            trim: self.project.trim(d),
            cuts: self
                .project
                .cuts
                .iter()
                .enumerate()
                .map(|(i, &(a, b))| {
                    let (_, sel, appear, _) = item_motion(Item::Cut(i));
                    (a.min(b), a.max(b), sel, appear)
                })
                .collect(),
            zooms: self
                .project
                .zooms
                .iter()
                .enumerate()
                .map(|(i, z)| block(z.start, z.end, format!("{}×", z.scale).into(), Item::Zoom(i), z.auto))
                .collect(),
            speeds: self
                .project
                .speeds
                .iter()
                .enumerate()
                .map(|(i, s)| block(s.start, s.end, format!("{}×", s.rate).into(), Item::Speed(i), false))
                .collect(),
            blurs: self
                .project
                .blurs
                .iter()
                .enumerate()
                .map(|(i, x)| {
                    let label = if x.pixelate { t("pill.editor.blockPixelate") } else { t("pill.editor.blockBlur") };
                    block(x.start, x.end, label.into(), Item::Blur(i), false)
                })
                .collect(),
            texts: self
                .project
                .texts
                .iter()
                .enumerate()
                .map(|(i, x)| block(x.start, x.end, x.text.clone().into(), Item::Text(i), false))
                .collect(),
            // Con horas de grabación, solo los clics de la vista.
            clicks: self.clicks.iter().map(|ms| *ms as f64 / 1000.0).filter(|t| *t >= view.start - 1.0 && *t <= view.end() + 1.0).collect(),
            thumbs: self.thumbs.clone(),
            wave,
            hints,
            muted_click: self.levels_silent.then(|| {
                let open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> = Rc::new(cx.listener(|_, _: &ClickEvent, _, cx| cx.defer(crate::settings::open)));
                open
            }),
            snap: snap_shown,
            range: range_shown,
            width: self.lanes.get().map_or(0.0, |b| f32::from(b.size.width)),
            labels: timeline::Labels {
                video: t("pill.editor.laneVideo").into(),
                audio: t("pill.editor.laneAudio").into(),
                zoom: t("pill.editor.zoom").into(),
                speed: t("pill.editor.laneSpeed").into(),
                blur: t("pill.editor.laneBlur").into(),
                text: t("pill.editor.text").into(),
                no_audio: t("pill.editor.noAudio").into(),
                muted: t("pill.editor.audioSilent").into(),
                hints: [t("pill.editor.hintZoom").into(), t("pill.editor.hintSpeed").into(), t("pill.editor.hintBlur").into(), t("pill.editor.hintText").into()],
            },
        };
        let lanes = timeline::render(&data, &pal)
            .id("editor-lanes")
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(|view, event: &MouseDownEvent, window, cx| view.press(event, window, cx)))
            .on_scroll_wheel(cx.listener(|view, event: &gpui::ScrollWheelEvent, _, cx| view.wheel(event, cx)))
            .child(canvas(move |bounds, _, _| lanes_slot.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full())
            .child(self.view_buttons(&pal, cx));
        let timeline_panel = pal.panel(div(), pal.r_panel).flex_none().px(px(8.)).pt(px(8.)).child(lanes);

        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(GAP))
            .child(div().flex_1().min_w_0().flex().flex_col().child(stage).child(transport))
            .child(sidebar);
        // El diálogo de exportar, encima de todo.
        let overlay = self.export_overlay(&pal, window, cx);
        let help = self.help_overlay(&pal, window, cx);
        // Solo mientras alguna animación de la interfaz corre se pide otro cuadro; quieta, nada.
        if self.fluid.animating() {
            window.request_animation_frame();
        }

        pal.root(div())
            .id("recording-editor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Un clic del mouse apaga el modo teclado: el anillo de foco ya no se muestra (`keys.rs`).
            .capture_any_mouse_down(cx.listener(|view, _: &MouseDownEvent, _, cx| view.mouse_nav(cx)))
            .size_full()
            .flex()
            .flex_col()
            .font_family(pal.font)
            .text_color(pal.text)
            // Arrastrar sigue aunque el cursor salga de la pista.
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    // Un control deslizante de la barra lateral que se arrastra es de ella.
                    if view.style_drag(event, cx) {
                        return;
                    }
                    match view.grab {
                        Some(Grab::Aim(..) | Grab::AimSize(_) | Grab::BlurMove(..) | Grab::BlurSize(_)) => view.aim_to(event.position, cx),
                        Some(_) => {
                            if !view.armed {
                                if (f32::from(event.position.x) - view.press_x).abs() < DRAG_SLOP {
                                    return;
                                }
                                view.armed = true;
                            }
                            view.drag_to(event.position.x, event.modifiers.alt, cx)
                        }
                        None => {}
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, cx| {
                let _ = view.style_release(cx);
                view.release(cx)
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, cx| {
                let _ = view.style_release(cx);
                view.release(cx)
            }))
            .child(top)
            .child(div().flex_1().min_h_0().px(px(GAP)).pb(px(GAP)).flex().flex_col().gap(px(GAP)).child(body).child(timeline_panel))
            .children(notice)
            .children(overlay)
            .children(help)
            .child(crate::glass_pieces::commit(pal.light))
    }
}

impl Drop for RecordingEditor {
    /// Cerrar la ventana deja al reproductor a 1× y con su volumen, corta «Ver cómo queda» y
    /// borra su archivo temporal.
    fn drop(&mut self) {
        self.player.pause();
        self.player.set_rate(NORMAL_RATE);
        self.player.set_volume(1.0);
        self.close_preview();
        self.remove_preview_file();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_project::{Aspect, ExportSettings, Zoom};

    fn framed() -> Look {
        Look { backdrop: Backdrop::Solid(0), padding: 0.08, radius: 0.02, shadow_amount: 0.5 }
    }

    fn unframed() -> Look {
        Look { backdrop: Backdrop::None, ..framed() }
    }

    /// El video en píxeles del cuadro de salida: x, y (desde arriba), ancho y alto.
    fn video_px(c: Canvas) -> [f64; 4] {
        let (w, h) = (c.out.0 as f64, c.out.1 as f64);
        [c.video[0] as f64 * w, c.video[1] as f64 * h, c.video[2] as f64 * w, c.video[3] as f64 * h]
    }

    fn assert_rect(got: [f64; 4], want: [f64; 4]) {
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 0.01, "{got:?} != {want:?}");
        }
    }

    fn settings(aspect: Aspect) -> ExportSettings {
        // Sin tope: estos tests son de proporciones, no del alto por defecto.
        ExportSettings { aspect, height: None, ..Default::default() }
    }

    // Los números salen de las fórmulas de `export_macos.rs` hechas a mano (`output_size`,
    // `frame_margin`, `content_rect`; el origen de ahí es abajo: aquí se pasa a arriba).

    #[test]
    fn el_cuadro_de_salida_es_el_del_exportador() {
        let video = (1920, 1080);
        assert_eq!(export_panel::render_dims(&settings(Aspect::Source), video), (1920, 1080));
        assert_eq!(export_panel::render_dims(&settings(Aspect::Wide), video), (1920, 1080));
        // 1080 × 9 / 16 = 607,5 → 607 → par: 606.
        assert_eq!(export_panel::render_dims(&settings(Aspect::Vertical), video), (606, 1080));
        assert_eq!(export_panel::render_dims(&settings(Aspect::Square), video), (1080, 1080));
        assert_eq!(export_panel::render_dims(&settings(Aspect::Portrait), video), (864, 1080));
        // Un video vertical en un cuadro ancho: 1920 × 16 / 9 = 3413,3 → 3412.
        assert_eq!(export_panel::render_dims(&settings(Aspect::Wide), (1080, 1920)), (3412, 1920));
    }

    #[test]
    fn video_16_9_en_cuadro_16_9_con_marco() {
        // margen 0,08 × 1080 = 86,4; ancho 1747,2 → 1747; alto 982,8 → 983; x = ⌊86,5⌋; y (abajo) = ⌊48,5⌋ = 48.
        let c = canvas_of((1920, 1080), (1920, 1080), framed());
        assert_eq!(c.out, (1920, 1080));
        assert_rect(video_px(c), [86.0, 49.0, 1747.0, 983.0]);
    }

    #[test]
    fn video_ancho_en_cuadro_cuadrado() {
        // Limita el ancho: 1080 − 172,8 = 907,2 → 907; alto 510,3 → 510; centrado a lo alto.
        let c = canvas_of((1080, 1080), (1920, 1080), framed());
        assert_rect(video_px(c), [86.0, 285.0, 907.0, 510.0]);
    }

    #[test]
    fn video_ancho_en_cuadro_vertical() {
        // margen 0,08 × 606 = 48,48; ancho 509,04 → 509; alto 286,3 → 286.
        let c = canvas_of((606, 1080), (1920, 1080), framed());
        assert_rect(video_px(c), [48.0, 397.0, 509.0, 286.0]);
    }

    #[test]
    fn video_ancho_en_cuadro_4_5() {
        // margen 0,08 × 864 = 69,12; ancho 725,76 → 726; alto 408,24 → 408.
        let c = canvas_of((864, 1080), (1920, 1080), framed());
        assert_rect(video_px(c), [69.0, 336.0, 726.0, 408.0]);
    }

    #[test]
    fn video_vertical_en_cuadro_ancho() {
        // Limita el alto: 1920 − 307,2 = 1612,8 → 1613; ancho 907,2 → 907.
        let c = canvas_of((3412, 1920), (1080, 1920), framed());
        assert_rect(video_px(c), [1252.0, 154.0, 907.0, 1613.0]);
    }

    #[test]
    fn sin_marco_el_margen_es_cero() {
        // Con la proporción del original llena el cuadro; con otra entra entero y centrado.
        assert_rect(video_px(canvas_of((1920, 1080), (1920, 1080), unframed())), [0.0, 0.0, 1920.0, 1080.0]);
        assert_rect(video_px(canvas_of((3412, 1920), (1080, 1920), unframed())), [1166.0, 0.0, 1080.0, 1920.0]);
    }

    #[test]
    fn el_margen_se_acota() {
        assert!((frame_margin(0.5, 1000.0, 500.0) - 150.0).abs() < 1e-9);
        assert_eq!(frame_margin(-1.0, 1000.0, 500.0), 0.0);
    }

    #[test]
    fn el_cuadro_cabe_centrado_en_el_area() {
        assert_eq!(fit((1000.0, 500.0), 1.0, 0.0), (250.0, 0.0, 500.0, 500.0));
        // Con margen: el cuadro 2:1 más grande en 1000 × 500 menos 18 por lado.
        assert_eq!(fit((1000.0, 500.0), 2.0, 18.0), (36.0, 18.0, 928.0, 464.0));
        // Una proporción rota no deja nada raro.
        let (_, _, w, h) = fit((1000.0, 500.0), f32::NAN, 0.0);
        assert!(w.is_finite() && h.is_finite() && w > 0.0);
    }

    fn close_rect(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    #[test]
    fn el_recuadro_se_mueve_sin_salirse() {
        let rect = [0.35, 0.40, 0.30, 0.15];
        // Tomado a (0,1; 0,05) de su esquina y llevado más allá del borde: queda pegado.
        assert!(close_rect(blur_moved(rect, (0.9, 0.9), (0.1, 0.05)), [0.7, 0.85, 0.3, 0.15]));
        assert!(close_rect(blur_moved(rect, (-1.0, -1.0), (0.0, 0.0)), [0.0, 0.0, 0.3, 0.15]));
        // Dentro, sigue al puntero sin cambiar de tamaño.
        assert!(close_rect(blur_moved(rect, (0.5, 0.5), (0.1, 0.05)), [0.4, 0.45, 0.3, 0.15]));
        // Un número roto lo deja como estaba.
        assert_eq!(blur_moved(rect, (f64::NAN, 0.5), (0.0, 0.0)), rect);
    }

    #[test]
    fn el_recuadro_cambia_de_tamano_con_minimo_y_sin_salirse() {
        let rect = [0.2, 0.2, 0.3, 0.3];
        assert!(close_rect(blur_resized(rect, (0.9, 0.2)), [0.2, 0.2, 0.7, BLUR_MIN]));
        assert!(close_rect(blur_resized(rect, (2.0, 0.5)), [0.2, 0.2, 0.8, 0.3]));
        assert!(close_rect(blur_resized(rect, (0.0, 0.0)), [0.2, 0.2, BLUR_MIN, BLUR_MIN]));
        // Pegado al borde de abajo a la derecha, no se sale del video.
        let [x, y, w, h] = blur_resized([0.999, 0.999, 0.3, 0.3], (2.0, 2.0));
        assert!(x + w <= 1.0 + 1e-9 && y + h <= 1.0 + 1e-9 && w >= BLUR_MIN && h >= BLUR_MIN);
        assert_eq!(blur_resized(rect, (f64::NAN, 0.5)), rect);
    }

    #[test]
    fn la_seleccion_sigue_valiendo_solo_si_su_indice_existe() {
        let mut project = Project::default();
        project.zooms.push(Zoom { start: 0.0, end: 1.0, scale: 2.0, focus: Focus::Auto, auto: false });
        project.cuts.push((1.0, 2.0));
        project.speeds.push(crate::edit_project::Speed { start: 0.0, end: 1.0, rate: 2.0 });
        project.blurs.push(Blur { start: 0.0, end: 1.0, rect: BLUR_NEW, pixelate: false });
        assert_eq!(valid_item(Some(Item::Zoom(0)), &project), Some(Item::Zoom(0)));
        assert_eq!(valid_item(Some(Item::Zoom(1)), &project), None);
        assert_eq!(valid_item(Some(Item::Cut(0)), &project), Some(Item::Cut(0)));
        assert_eq!(valid_item(Some(Item::Speed(0)), &project), Some(Item::Speed(0)));
        assert_eq!(valid_item(Some(Item::Blur(1)), &project), None);
        // No hay textos: ninguno vale.
        assert_eq!(valid_item(Some(Item::Text(0)), &project), None);
        assert_eq!(valid_item(None, &project), None);
        // Tras deshacerlo todo, nada de lo elegido existe.
        assert_eq!(valid_item(Some(Item::Zoom(0)), &Project::default()), None);
    }

    #[test]
    fn el_guardado_entra_se_queda_y_se_retira() {
        assert_eq!(saved_alpha(Duration::ZERO), 0.0);
        assert!((saved_alpha(SAVED_FADE / 2) - 0.5).abs() < 1e-3);
        assert_eq!(saved_alpha(SAVED_FADE * 3), 1.0);
        assert_eq!(saved_alpha(SAVED_SHOW - Duration::from_millis(1)), 1.0);
        assert!(saved_alpha(SAVED_SHOW + SAVED_FADE / 2) < 0.6);
        assert_eq!(saved_alpha(SAVED_SHOW + SAVED_FADE * 2), 0.0);
        // Solo se piden cuadros mientras entra o sale.
        assert!(saved_animating(Duration::from_millis(10)));
        assert!(!saved_animating(Duration::from_millis(800)));
        assert!(saved_animating(SAVED_SHOW + Duration::from_millis(10)));
        assert!(!saved_animating(SAVED_SHOW + SAVED_FADE * 2));
    }

    #[test]
    fn el_eco_del_clic_entra_crece_y_sale_sin_pasarse() {
        let total = fluid::ECHO_IN + fluid::ECHO_HOLD + fluid::ECHO_OUT;
        let (a0, s0) = echo_state(Duration::ZERO, false);
        assert_eq!(a0, 0.0);
        assert!((s0 - fluid::ECHO_FROM).abs() < 1e-3);
        let (a, s) = echo_state(fluid::ECHO_IN + Duration::from_millis(100), false);
        assert_eq!((a, s), (1.0, 1.0));
        assert_eq!(echo_state(total, false).0, 0.0);
        // Con movimiento reducido no cambia de tamaño.
        assert_eq!(echo_state(Duration::ZERO, true).1, 1.0);
    }

    #[test]
    fn un_aviso_con_accion_dice_que_ofrece() {
        assert_ne!(NoticeAction::Undo.label(), NoticeAction::ShowExport.label());
        assert_eq!(NoticeAction::Reveal(PathBuf::from("/tmp/a.mp4")), NoticeAction::Reveal(PathBuf::from("/tmp/a.mp4")));
    }

    #[test]
    fn el_reproductor_va_a_ritmo_solo_dentro_de_un_tramo_con_velocidad() {
        let spans = [(2.0, 4.0, 2.0), (6.0, 8.0, 4.0)];
        assert_eq!(playback_at(&spans, 1.0), (1.0, false));
        assert_eq!(playback_at(&spans, 2.0), (2.0, true));
        assert_eq!(playback_at(&spans, 3.99), (2.0, true));
        assert_eq!(playback_at(&spans, 4.0), (1.0, false));
        assert_eq!(playback_at(&spans, 7.0), (4.0, true));
        assert_eq!(playback_at(&[], 3.0), (1.0, false));
    }
}
