//! Prototipo de la pill de Atic en GPUI: tab acoplado a cualquier borde o gota
//! flotante, tira de herramientas al pasar el cursor, rueda con gotas
//! líquidas y el panel del Clipboard.

// En release, sin consola: el log va a archivo (`init_log`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod appearance;
mod settings;
mod app_icon;
mod board;
/// La calculadora del lanzador de Atic.
use atic_calc::calc;
mod agent_prompts;
mod agents;
mod agents_settings;
mod color;
mod capture;
mod clip_image;
mod clipboard;
mod clipboard_owner;
mod config_watch;
mod customize;
mod emoji;
mod flip;
mod flip_board;
mod flip_export;
mod flip_pages;
mod general_settings;
mod geometry;
mod dictation;
mod dictation_settings;
mod hang;
mod history;
mod housekeeping;
mod hover;
mod i18n;
mod liquid;
mod peeks;
mod phone;
mod phone_settings;
use hover::HoverExt;
mod pill_settings;
mod pill_tools;
use pill_tools::{Layout, Page, StripId};
mod launcher;
mod meetings;
mod meter;
mod secrets;
mod shelf;
mod shortcuts_settings;
mod snippets;
mod space;
mod system;
mod media;
// Copiado de Atic: lo que la pill no usa todavía se queda igual que allá.
#[allow(dead_code)]
mod quota;
mod tray;
mod updater;
mod usage;
mod text_area;
mod text_input;
mod paste_queue;
mod paths;
mod platform;
use platform::{drag, glass, hotkeys, ocr, paste, privacy, running, tray_icon, win};
mod single_instance;

use std::borrow::Cow;
use std::f32::consts::{PI, TAU};
use std::time::{Duration, Instant};

use anyhow::Result;
use gpui::{
    canvas, div, img, point, prelude::*, px, rgb, size, svg, App, Application, AssetSource, Bounds,
    BoxShadow, ClipboardItem, Context, Corners, Entity, Focusable, Hsla, MouseButton,
    MouseDownEvent, MouseUpEvent, SharedString, Subscription, StyledImage, TransformationMatrix, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};

use anim::{cubic_bezier, ease_back_out, ease_island, ease_smooth_out, lerp, segment, Tween};
use snippets::{SnippetEvent, SnippetsPanel};
use clipboard::{ClipboardPanel, Content, PanelEvent, Picture, BAND_H, PANEL_W};

use geometry::{Edge, Rect};

// Medidas de `pillStage.ts`, `pillPlan.ts` y `wheelGeometry.ts`.
const TAB_LENGTH: f32 = 124.0;
const TAB_THICK: f32 = 40.0;
const TAB_RADIUS: f32 = 22.0;
const DISC_R: f32 = 26.0;
const MARK_SIZE: f32 = 32.0;
const TOOL_W: f32 = 44.0;
const TOOL_GAP: f32 = 2.0;
const TOOL_ICON: f32 = 16.0;
/// Desde el borde hasta el centro de la rueda cuando gotea del tab.
const WHEEL_DEPTH: f32 = TAB_THICK + 20.0 + 126.0;
const WHEEL_BOX: f32 = 252.0;
const WHEEL_RING: f32 = 0.28 * 232.0;
const WHEEL_BLOB_R: f32 = 28.0;
const WHEEL_CORE_R: f32 = 29.0;
const WHEEL_ICON: f32 = 21.0;
const WHEEL_HIT_R: f32 = 126.0;

// Línea de tiempo de la rueda, en ms. Más lenta que la real (110 ms + 16 ms
// por gota) para que el efecto líquido se alcance a ver.
const CORE_DROP_MS: f32 = 300.0;
const BLOB_START_MS: f32 = 150.0;
const BLOB_STAGGER_MS: f32 = 24.0;
const BLOB_MS: f32 = 300.0;
const ICON_DELAY_MS: f32 = 180.0;
const ICON_MS: f32 = 140.0;
const CLOSE_SPEED: f32 = 2.5;

/// Esquinas de la burbuja de vista previa y del lanzador centrado.
const PANEL_CORNER: f32 = 20.0;
/// Distancia a la que se corta el cuello de la burbuja.
const PANEL_NECK_REACH: f32 = 12.0;
// Notch: una herramienta no sale de la pill sino que la pill misma se estira
// hasta su tamaño, en el borde donde está acoplada (arriba, al centro). La
// franja de `TAB_THICK` de arriba del panel queda para la marca y el contenido
// va debajo, en el mismo cuerpo. En un costado el panel es alto y angosto,
// pegado a ese borde (`geometry::notch_rect`).
const NOTCH_MORPH_MS: f32 = 300.0;
/// El panel en un costado: más angosto que arriba y tan alto como pida su
/// contenido, hasta casi todo el alto del área de trabajo (`SIDE_PANEL_MARGIN`
/// arriba y abajo).
const SIDE_PANEL_W: f32 = 380.0;
const SIDE_PANEL_MARGIN: f32 = 12.0;
/// El ancho con el que el editor mide sus filas. Es el del panel más ancho
/// (arriba), para que la grilla no cambie de forma según el borde.
pub(crate) const CUSTOMIZE_W: f32 = PANEL_W;
/// El margen del contenido del panel dentro de su ancho.
pub(crate) const CUSTOMIZE_INSET: f32 = 12.0;
/// Un costado más bajo que esto no tiene notch: la pill vuela al de arriba.
const NOTCH_MIN_SIDE_H: f32 = 420.0;
/// El bloque que sale al lado de la columna en un costado (vistazos, aviso,
/// letra, dictado, uso): angosto, con el contenido en columna.
pub(crate) const SIDE_W: f32 = 300.0;
/// La esquina cóncava donde ese bloque se une a la columna.
const SIDE_JOIN_R: f32 = 14.0;
/// Hueco a la izquierda del buscador donde va la marca (`clipboard::MARK_GAP`).
const MARK_GAP: f32 = 40.0;
const NOTCH_CONTENT_START_MS: f32 = 190.0;
const NOTCH_CONTENT_MS: f32 = 130.0;
const NOTCH_OPEN_MS: f32 = NOTCH_CONTENT_START_MS + NOTCH_CONTENT_MS;
const NOTCH_CLOSE_SPEED: f32 = 1.8;
/// Vuelo de la gota hasta el notch de arriba (y de vuelta) cuando la pill
/// flota, o cuando el panel no cabe en el costado donde está.
const FLIGHT_MS: f32 = 340.0;
const FLIGHT_BACK_MS: f32 = 300.0;

// Vista previa: una burbuja que se desprende del costado del notch, como la
// semilla del panel flotante. Así la lista no salta y caben textos largos.
const PREVIEW_W: f32 = 320.0;
const PREVIEW_H: f32 = 220.0;
const PREVIEW_GAP: f32 = 14.0;
const PREVIEW_SEED: f32 = 36.0;
const PREVIEW_PAD: f32 = 12.0;

// Vistazo del Clipboard: el cursor sobre su herramienta estira el notch hacia
// abajo con las últimas copiadas, bajo la tira. Clic pega en la app de atrás
// (el overlay no roba el foco), arrastrar lleva a otra app y «Ver todo» sigue
// estirando el mismo notch hasta el panel.
const PEEK_COUNT: usize = 3;
const PEEK_DELAY: Duration = Duration::from_millis(350);
const PEEK_GRACE: Duration = Duration::from_millis(150);
/// Lo de arriba del vistazo del portapapeles: el título, como en los de
/// las otras herramientas (`peeks.rs`).
const PEEK_PAD: f32 = 30.0;
const PEEK_ROW: f32 = 36.0;
const PEEK_FOOTER: f32 = 30.0;
const PEEK_BOTTOM: f32 = 6.0;
const PEEK_DRAG: f32 = 6.0;

const PASTE_FOCUS_DELAY: Duration = Duration::from_millis(220);
const PASTE_KEY_DELAY: Duration = Duration::from_millis(80);
/// Índices en `pill_tools::CATALOG`. El nivel de cada una lo elige el editor
/// (`pill_tools.rs`), así que ninguno de estos dice dónde se ve: solo cuál es.
const REUNIONES_TOOL: usize = 0;
pub(crate) const CLIPBOARD_TOOL: usize = 1;
pub(crate) const TEXTOS_TOOL: usize = 2;
pub(crate) const AGENTES_TOOL: usize = 3;
pub(crate) const SISTEMA_TOOL: usize = 4;
/// «Ahora suena»: también en pausa o sin nada sonando, para reanudar.
pub(crate) const MEDIA_TOOL: usize = 9;
/// Largo del tab con algo sonando: carátula a la izquierda, onda a la derecha.
const LIVE_LENGTH: f32 = 212.0;
const LIVE_ART: f32 = 24.0;
/// En pausa, la carátula queda atenuada con un ▶ este rato: para reanudar
/// sin buscar el reproductor.
const PAUSED_ART_FOR: Duration = Duration::from_secs(10 * 60);
/// Margen entre el borde del tab y lo que va en sus extremos (carátula,
/// reloj, onda).
const SLOT_PAD: f32 = 9.0;
/// La onda: cuatro barras de 3 px separadas 6.
const BARS_W: f32 = 22.0;
/// La carátula como insignia de la gota flotante.
const LIVE_BADGE: f32 = 20.0;
/// El punto de privacidad: micrófono (naranja) o cámara (verde).
const PRIVACY_MIC: u32 = 0xf0a020;
const PRIVACY_CAM: u32 = 0x4cd964;

/// Qué muestra el notch abierto: todas las herramientas con panel comparten
/// el mismo cuerpo y cambiar de una a otra solo cambia el contenido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotchTool {
    Clipboard,
    Textos,
    Apps,
    Agentes,
    Sistema,
    Media,
    /// El editor de los niveles de la pill (`customize.rs`).
    Personalizar,
}

/// La gota en camino hacia el notch o de vuelta a su lugar.
#[derive(Clone, Copy)]
struct Flight {
    from: (f32, f32),
    to: (f32, f32),
    started: Instant,
    back: bool,
}

impl Flight {
    fn progress(&self, now: Instant) -> f32 {
        let total = if self.back { FLIGHT_BACK_MS } else { FLIGHT_MS };
        (now.duration_since(self.started).as_secs_f32() * 1000.0 / total).clamp(0.0, 1.0)
    }

    /// Centro de la gota: sube en arco (primero se despega, después acelera
    /// hacia el borde) para que se lea como un salto y no como un deslizamiento.
    fn center(&self, now: Instant) -> (f32, f32) {
        let t = ease_smooth_out(self.progress(now));
        let (x, y) = (lerp(self.from.0, self.to.0, t), lerp(self.from.1, self.to.1, t));
        let lift = (t * PI).sin() * 0.12 * distance(self.from, self.to).min(400.0);
        // Se curva hacia afuera del trayecto (a la izquierda del sentido de avance).
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let len = dx.hypot(dy).max(1.0);
        (x - dy / len * lift * 0.5, y + dx / len * lift * 0.5)
    }
}

/// Movimiento con el botón apretado a partir del cual es arrastre y no clic.
const DRAG_THRESHOLD: f32 = 4.0;
/// Presión larga sobre una ficha de la tira: abre el editor de los niveles.
/// El mismo plazo que en Atic (`STRIP_HOLD_MS`).
const STRIP_HOLD_MS: f32 = 480.0;
/// Lo que espera la cara antes de empezar a volverse engranaje: un clic
/// normal no alcanza a mostrarlo.
const GEAR_DELAY_MS: f32 = 120.0;
/// Cuánto tarda la piel de GPUI en llegar a la pantalla después del vidrio
/// (unos dos cuadros).
const GLASS_LAG: Duration = Duration::from_millis(34);
/// Cada cuánto se mira si el monitor cambió de resolución, escala o barra de
/// tareas (arrastrando, en cada cuadro).
const SCREEN_POLL: Duration = Duration::from_millis(500);
/// El sondeo del cursor: ~60/s con algo cerca o en marcha, ~20/s en calma.
const TICK_ACTIVE: Duration = Duration::from_millis(16);
const TICK_CALM: Duration = Duration::from_millis(50);
/// A cuántos px de la pill el sondeo vuelve a ir rápido: a 50 ms por vuelta,
/// un cursor rápido recorre menos que esto antes de llegar.
const CALM_DISTANCE: f32 = 160.0;
/// "Aplastón" al acoplarse: el grosor baja a 86 % al 38 % del tramo.
const SEAT_MS: f32 = 125.0;
const SEAT_DEPTH: f32 = 0.14;
/// Muro líquido que se forma al acercar la gota a un borde (`INFLUENCE`).
const WALL_REACH: f32 = 24.0;
const WALL_R: f32 = 60.0;

const STRIP_LEAVE_GRACE: Duration = Duration::from_millis(150);
const WHEEL_HOVER_DELAY: Duration = Duration::from_millis(180);
const WHEEL_HOVER_GRACE: Duration = Duration::from_millis(150);
const WHEEL_CLICK_GRACE: Duration = Duration::from_millis(400);
const BLINK_MS: f32 = 140.0;
/// Opacidad del tinte de la pill sobre el vidrio, por omisión (se cambia en
/// Apariencia).
const GLASS_TINT: f32 = 0.6;
/// El tinte cuando el notch muestra algo para leer (un panel, la bandeja,
/// la letra, el dictado, el uso): casi opaco, para que el texto se lea sobre
/// cualquier fondo. Es el mismo en todo el cuerpo: nada pinta su propio
/// fondo encima (se notaba el corte y unas partes eran de vidrio y otras no).
/// Por omisión: se cambia en Apariencia.
const CONTENT_TINT: f32 = 0.93;

fn wheel_open_ms(gajos: usize) -> f32 {
    BLOB_START_MS + BLOB_STAGGER_MS * (gajos.max(1) - 1) as f32 + ICON_DELAY_MS + ICON_MS
}

/// El largo del tab con la tira abierta. Es el del paso que se está viendo,
/// menos nunca que el del tab en reposo.
fn strip_open_length(gajos: usize) -> f32 {
    TAB_LENGTH.max(MARK_SIZE + 12.0 + gajos as f32 * (TOOL_W + TOOL_GAP))
}

/// El largo del paso más largo que puede verse. Lo que cuelga del tab (los
/// vistazos, el uso) se mide con esto, no con el paso de ahora.
fn strip_max_length() -> f32 {
    strip_open_length(pill_tools::STRIP_MAX)
}

fn wheel_angle(index: usize, gajos: usize) -> f32 {
    index as f32 / gajos.max(1) as f32 * TAU - PI / 2.0
}

/// Centro y radio.
type Circle = ((f32, f32), f32);

/// Traza de eventos con `PILL_DEBUG=1`.
fn debug(message: impl FnOnce() -> String) {
    if std::env::var_os("PILL_DEBUG").is_some() {
        eprintln!("[pill] {}", message());
    }
}

/// El log a consola y a `pill.<fecha>.log` en la carpeta de logs de Atic, con
/// los pánicos. Los avisos y errores de GPUI (que usa `log`) también llegan:
/// sin eso, un error al dibujar deja la pill invisible sin decir por qué.
#[must_use = "si se descarta el guard, las últimas líneas antes del cierre se pierden"]
fn init_log() -> Option<atic_core::diagnostics::WorkerGuard> {
    let dir = paths::logs_dir().unwrap_or_else(|| std::env::temp_dir().join("atic-logs"));
    atic_core::diagnostics::init(&dir, "pill", "info,gpui=warn")
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

struct Palette {
    skin: Hsla,
    text: Hsla,
    muted: Hsla,
}

impl Palette {
    /// Tema "atic" oscuro.
    fn dark() -> Self {
        Self {
            skin: rgb(0x1a1a18).into(),
            text: rgb(0xf0f0ea).into(),
            muted: rgb(0xa8a89e).into(),
        }
    }
}

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/circle-dot.svg" => include_bytes!("../assets/icons/circle-dot.svg"),
            "icons/clipboard.svg" => include_bytes!("../assets/icons/clipboard.svg"),
            "icons/text-align-start.svg" => include_bytes!("../assets/icons/text-align-start.svg"),
            "icons/square-terminal.svg" => include_bytes!("../assets/icons/square-terminal.svg"),
            "icons/cpu.svg" => include_bytes!("../assets/icons/cpu.svg"),
            "icons/crop.svg" => include_bytes!("../assets/icons/crop.svg"),
            "icons/pencil.svg" => include_bytes!("../assets/icons/pencil.svg"),
            "icons/pipette.svg" => include_bytes!("../assets/icons/pipette.svg"),
            "icons/ellipsis.svg" => include_bytes!("../assets/icons/ellipsis.svg"),
            "icons/x.svg" => include_bytes!("../assets/icons/x.svg"),
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/import.svg" => include_bytes!("../assets/icons/import.svg"),
            "icons/layers.svg" => include_bytes!("../assets/icons/layers.svg"),
            "icons/type.svg" => include_bytes!("../assets/icons/type.svg"),
            "icons/mic-vocal.svg" => include_bytes!("../assets/icons/mic-vocal.svg"),
            "icons/image.svg" => include_bytes!("../assets/icons/image.svg"),
            "icons/star.svg" => include_bytes!("../assets/icons/star.svg"),
            "icons/pin.svg" => include_bytes!("../assets/icons/pin.svg"),
            "icons/flip.svg" => include_bytes!("../assets/icons/flip.svg"),
            "icons/plus.svg" => include_bytes!("../assets/icons/plus.svg"),
            "icons/lock.svg" => include_bytes!("../assets/icons/lock.svg"),
            "icons/highlighter.svg" => include_bytes!("../assets/icons/highlighter.svg"),
            "icons/arrow-up-right.svg" => include_bytes!("../assets/icons/arrow-up-right.svg"),
            "icons/arrow-up.svg" => include_bytes!("../assets/icons/arrow-up.svg"),
            "icons/square.svg" => include_bytes!("../assets/icons/square.svg"),
            "icons/circle.svg" => include_bytes!("../assets/icons/circle.svg"),
            "icons/eraser.svg" => include_bytes!("../assets/icons/eraser.svg"),
            "icons/trash.svg" => include_bytes!("../assets/icons/trash.svg"),
            "icons/undo.svg" => include_bytes!("../assets/icons/undo.svg"),
            "icons/redo.svg" => include_bytes!("../assets/icons/redo.svg"),
            "icons/check.svg" => include_bytes!("../assets/icons/check.svg"),
            "icons/folder.svg" => include_bytes!("../assets/icons/folder.svg"),
            "icons/copy.svg" => include_bytes!("../assets/icons/copy.svg"),
            "icons/scan-text.svg" => include_bytes!("../assets/icons/scan-text.svg"),
            "icons/external-link.svg" => include_bytes!("../assets/icons/external-link.svg"),
            "icons/volume-2.svg" => include_bytes!("../assets/icons/volume-2.svg"),
            "icons/volume-1.svg" => include_bytes!("../assets/icons/volume-1.svg"),
            "icons/volume-x.svg" => include_bytes!("../assets/icons/volume-x.svg"),
            "icons/sun.svg" => include_bytes!("../assets/icons/sun.svg"),
            "icons/sun-dim.svg" => include_bytes!("../assets/icons/sun-dim.svg"),
            "icons/coffee.svg" => include_bytes!("../assets/icons/coffee.svg"),
            "icons/moon.svg" => include_bytes!("../assets/icons/moon.svg"),
            "icons/battery-charging.svg" => include_bytes!("../assets/icons/battery-charging.svg"),
            "icons/battery-full.svg" => include_bytes!("../assets/icons/battery-full.svg"),
            "icons/battery-medium.svg" => include_bytes!("../assets/icons/battery-medium.svg"),
            "icons/battery-low.svg" => include_bytes!("../assets/icons/battery-low.svg"),
            "icons/monitor.svg" => include_bytes!("../assets/icons/monitor.svg"),
            "icons/laptop.svg" => include_bytes!("../assets/icons/laptop.svg"),
            "icons/audio-lines.svg" => include_bytes!("../assets/icons/audio-lines.svg"),
            "icons/activity.svg" => include_bytes!("../assets/icons/activity.svg"),
            "icons/skip-back.svg" => include_bytes!("../assets/icons/skip-back.svg"),
            "icons/skip-forward.svg" => include_bytes!("../assets/icons/skip-forward.svg"),
            "icons/play.svg" => include_bytes!("../assets/icons/play.svg"),
            "icons/pause.svg" => include_bytes!("../assets/icons/pause.svg"),
            "icons/mic.svg" => include_bytes!("../assets/icons/mic.svg"),
            "icons/mic-off.svg" => include_bytes!("../assets/icons/mic-off.svg"),
            "icons/headphones.svg" => include_bytes!("../assets/icons/headphones.svg"),
            "icons/speaker.svg" => include_bytes!("../assets/icons/speaker.svg"),
            "icons/video.svg" => include_bytes!("../assets/icons/video.svg"),
            "icons/circle-check.svg" => include_bytes!("../assets/icons/circle-check.svg"),
            "icons/rotate-ccw.svg" => include_bytes!("../assets/icons/rotate-ccw.svg"),
            "icons/rotate-cw.svg" => include_bytes!("../assets/icons/rotate-cw.svg"),
            "icons/chevron-down.svg" => include_bytes!("../assets/icons/chevron-down.svg"),
            "icons/chevron-up.svg" => include_bytes!("../assets/icons/chevron-up.svg"),
            "icons/agents/claude.svg" => include_bytes!("../assets/icons/agents/claude.svg"),
            "icons/agents/openai.svg" => include_bytes!("../assets/icons/agents/openai.svg"),
            "icons/agents/opencode.svg" => include_bytes!("../assets/icons/agents/opencode.svg"),
            "icons/agents/cursor.svg" => include_bytes!("../assets/icons/agents/cursor.svg"),
            "icons/agents/antigravity.svg" => {
                include_bytes!("../assets/icons/agents/antigravity.svg")
            }
            "icons/agents/grok.svg" => include_bytes!("../assets/icons/agents/grok.svg"),
            "icons/mail.svg" => include_bytes!("../assets/icons/mail.svg"),
            "icons/sparkles.svg" => include_bytes!("../assets/icons/sparkles.svg"),
            "icons/settings-2.svg" => include_bytes!("../assets/icons/settings-2.svg"),
            other => match flip_board::icon(other) {
                Some(bytes) => bytes,
                None => return Ok(None),
            },
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// Generador pseudoaleatorio mínimo para los parpadeos; no amerita una crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Dónde vive la pill cuando nadie la arrastra.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Home {
    /// Tab pegado a un borde, centrado en `along`.
    Docked { edge: Edge, along: f32 },
    /// Gota de 52 px centrada en `center`.
    Floating { center: (f32, f32) },
}

/// La forma de la pill en este cuadro.
#[derive(Clone, Copy)]
enum PillShape {
    Tab {
        edge: Edge,
        rect: Rect,
        along: f32,
        thick: f32,
        /// En un costado, el bloque al lado de la columna con lo que cuelga
        /// del tab (`geometry::side_block`). Arriba lo que cuelga va dentro de
        /// `rect`.
        block: Option<Rect>,
    },
    Disc {
        center: (f32, f32),
    },
}

#[derive(Clone, Copy, PartialEq)]
enum WheelOpener {
    Click,
    Hover,
    Shortcut,
}

#[derive(Clone, Copy, PartialEq)]
enum PressTarget {
    Mark,
    /// La carátula de lo que suena: abre Ahora suena.
    Art,
    /// Una ficha de la tira, del nivel que sea: herramienta, «Más», «Atrás» o
    /// «Personalizar».
    Tool(StripId),
    /// Un contador de la bandeja: abre Agentes.
    Tray,
    Body,
}

/// Dónde cae un punto dentro del vistazo.
#[derive(Clone, Copy, PartialEq)]
enum PeekHit {
    Row(usize),
    Footer,
}

/// Botón apretado sobre la pill: todavía puede ser clic o arrastre.
struct Press {
    cursor: (f32, f32),
    /// Posición de la pill al empezar (centro de la gota, o `along` del tab
    /// en `.0`).
    anchor: (f32, f32),
    target: PressTarget,
    dragging: bool,
}

struct Pill {
    overlay: Option<win::Overlay>,
    /// El vidrio esmerilado detrás de la pill (`glass.rs`).
    glass: Option<glass::Glass>,
    scale_factor: f32,
    cursor: Option<(f32, f32)>,
    born: Instant,
    last_frame: Instant,
    ticks: u64,
    /// Nada cerca ni en marcha: el sondeo va lento (`tick_every`).
    calm: bool,
    fps: (Instant, u32),

    monitor: Rect,
    work: Rect,
    dockable: Vec<Edge>,
    /// El monitor, la escala y la esquina de la ventana con que se calcularon
    /// `monitor` y `work`.
    screen: Option<(win::Screen, f32, Option<(i32, i32)>)>,
    screen_checked: Instant,
    /// La ventana va camino al monitor guardado: al llegar, `home` se vuelve
    /// a leer del archivo en vez de reubicarse desde el principal.
    restore_home: bool,
    /// El monitor al que va la ventana, mientras no llega.
    moving_to: Option<win::Screen>,
    home: Home,
    press: Option<Press>,
    seat_started: Option<Instant>,
    hover_since: Option<Instant>,

    strip: Tween,
    strip_leave_at: Option<Instant>,
    strip_hover: Vec<Tween>,
    strip_pulse: Option<(usize, Instant)>,
    /// Qué herramientas están a la vista, cuáles detrás de «Más» y cuáles
    /// fuera (`pill_tools.rs`). Lo elige el editor del notch.
    layout: Layout,
    /// Qué paso de la tira se está viendo. Cambia al tocar «Más», «Atrás» o
    /// «Personalizar».
    strip_page: Page,
    /// El cambio de paso en curso, mientras se anima.
    page_switch: Option<PageSwitch>,
    /// El reloj de la presión larga sobre una ficha: abre el editor.
    hold: Option<(PressTarget, Instant)>,

    wheel_target_open: bool,
    wheel_time: f32,
    /// El nivel que muestra la rueda: el suyo, aparte del de la tira.
    wheel_page: Page,
    /// Cuánto se corrió el brote de los gajos al cambiar de nivel: vuelven a
    /// salir del núcleo sin que este se recoja.
    wheel_bloom: f32,
    wheel_opener: WheelOpener,
    wheel_leave_at: Option<Instant>,
    wheel_hover: Vec<Tween>,
    shortcut_was_down: bool,

    panel: Entity<ClipboardPanel>,
    /// Qué herramienta ocupa el notch (abierto o cerrándose).
    notch_tool: NotchTool,
    /// El lanzador (`launcher.rs`): otro contenido del mismo notch.
    launcher: Entity<launcher::LauncherPanel>,
    /// Agentes (`agents.rs`).
    agents: Entity<agents::AgentsPanel>,
    /// Sistema (`system.rs`).
    system: Entity<system::SystemPanel>,
    /// El editor de los niveles de la pill (`customize.rs`).
    customize: Entity<customize::CustomizePanel>,
    /// Lo que suena (`media.rs`) y su panel.
    media: media::Media,
    media_panel: Entity<media::MediaPanel>,
    /// Quién usa el micrófono o la cámara (`privacy.rs`).
    privacy: privacy::Privacy,
    /// La grabadora de Reuniones: la misma de la ventana (`meetings::Studio`).
    studio: Entity<meetings::Studio>,
    /// Grabando: el reloj que muestra el tab. Cambia una vez por segundo.
    rec_clock: Option<String>,
    /// El tab con lo que suena: 0 normal, 1 con carátula (y onda si suena).
    live: Tween,
    /// Lo que suena está sonando (no en pausa).
    music_playing: bool,
    /// Desde cuándo está en pausa lo que sonaba.
    paused_since: Option<Instant>,
    /// El largo que necesita el tab para que quepa lo que muestra (`fit_length`).
    fit: Tween,
    /// La letra que cuelga del tab con lo que suena (`hang.rs`).
    hang: Tween,
    /// El dictado (`dictation.rs`): el motor, su franja en el notch, el
    /// atajo de Atic y si estaba apretado en el cuadro anterior.
    dictation: dictation::Dictation,
    /// La cara hecha engranaje (0..1): crece mientras se mantiene presionada
    /// y, completa, al soltar abre los Ajustes.
    gear: Tween,
    dict: Tween,
    dict_watch: dictation::Watch,
    dict_key_was_down: bool,
    /// El vistazo de uso de los agentes (`usage.rs`), al pasar sobre Agentes.
    usage: usage::Usage,
    usage_peek: Tween,
    /// En un costado, la altura a la que sale el bloque de lo que cuelga.
    block_along: Tween,
    /// Los vistazos de Color, Capturas, Textos, Flip y Sistema (`peeks.rs`).
    tool_peek: peeks::ToolPeek,
    usage_hover_since: Option<Instant>,
    usage_leave_at: Option<Instant>,
    /// El nivel de la salida, suavizado para la onda.
    level: f32,
    /// La ventana del espacio de consolas (`space`), si está abierta.
    space: Option<gpui::WindowHandle<space::SpaceView>>,
    /// La gota volando hacia el notch o de vuelta.
    flight: Option<Flight>,
    /// Dónde vivía la pill antes de volar al notch; vuelve ahí al cerrarlo.
    away: Option<Home>,
    /// El lanzador en el centro de la pantalla (Ctrl+M), no en el notch.
    launcher_centered: bool,
    /// Los atajos globales de las herramientas (`hotkeys.rs`).
    hotkeys: std::sync::mpsc::Receiver<hotkeys::Action>,
    /// Lo pedido desde el ícono de la bandeja (`tray_icon.rs`).
    tray_icon: std::sync::mpsc::Receiver<tray_icon::Command>,
    /// Lo que pide el celular (`phone.rs`).
    phone: std::sync::mpsc::Receiver<phone::Command>,
    snippets: Entity<SnippetsPanel>,
    panel_open: bool,
    /// Alto del contenido del notch: sigue a lo que el panel necesita
    /// mostrar, así filtrar lo encoge o lo estira.
    notch_h: Tween,
    /// Burbuja de vista previa: 0 pegada al notch, 1 desprendida.
    preview: Tween,
    /// Lo que muestra; se queda mientras la burbuja vuelve al notch.
    preview_entry: Option<clipboard::Entry>,
    /// La bandeja: contadores del tab y vistazo de Agentes (`tray.rs`).
    tray: tray::Banner,
    /// Vistazo: 0 cerrado, 1 estirado con las últimas copiadas.
    peek: Tween,
    peek_entries: Vec<clipboard::Entry>,
    peek_hover_since: Option<Instant>,
    /// Cuándo bajó el vistazo: el reloj de su entrada (letras, filas).
    peek_shown_at: Instant,
    peek_leave_at: Option<Instant>,
    peek_hovered: Option<PeekHit>,
    /// Botón apretado en el vistazo: clic pega, mover arrastra.
    peek_press: Option<(PeekHit, (f32, f32))>,
    /// `PILL_OPEN=peek`: el vistazo queda abierto un rato al arrancar.
    demo_peek_until: Option<Instant>,
    /// La mira de Capturas, encima de todo mientras dura (`capture.rs`).
    capture: Option<Entity<capture::CaptureView>>,
    capture_pending: bool,
    /// El flip: la ventana del frente dada vuelta (`flip.rs`).
    flip: Option<Entity<flip::FlipView>>,
    flip_events: Option<Subscription>,
    flip_target: Option<paste::Target>,
    capture_events: Option<Subscription>,
    /// La pizarra, sobre la misma pantalla congelada (`board.rs`).
    board: Option<Entity<board::BoardView>>,
    board_events: Option<Subscription>,
    /// El cuentagotas de Color, encima de todo mientras dura (`color.rs`).
    color: Option<Entity<color::ColorView>>,
    color_events: Option<Subscription>,
    /// Algo que cuelga del notch un rato con la burbuja (la captura recién
    /// hecha) y hasta cuándo.
    toast: Option<(clipboard::Entry, Instant)>,
    /// La ventana estirada a todas las pantallas para la mira (`capture.rs`).
    span: Option<capture::Span>,
    /// Las capturas recientes, apiladas abajo a la derecha (`shelf.rs`).
    shelves: Vec<shelf::Shelf>,
    /// El id de la última foto del estante.
    shelf_seq: u64,
    panel_time: f32,
    paste_target: Option<paste::Target>,

    eyes: (f32, f32),
    next_blink: Instant,
    blink_started: Option<Instant>,
    double_blink: bool,
    rng: Rng,
    _subscriptions: Vec<Subscription>,
}

/// Dónde gotea la rueda: desde qué punto, unida a qué círculo de la pill y
/// con qué centro final.
struct WheelPlace {
    source: (f32, f32),
    source_r: f32,
    anchor: Circle,
    center: (f32, f32),
}

/// Geometría del panel en un instante de su línea de tiempo.
#[derive(Clone, Copy)]
struct PanelShape {
    rect: Rect,
    content: f32,
    /// El panel es el cuerpo estirado del notch: no tiene silueta, sombra ni
    /// cuello propios.
    notch: bool,
    /// El borde del notch: el contenido se recorta con las esquinas del lado
    /// que mira al escritorio.
    edge: Edge,
    /// Círculo de la pill y círculo del panel que une el cuello.
    anchor: Circle,
    near: Circle,
}

/// Geometría de la burbuja de vista previa.
#[derive(Clone, Copy)]
struct PreviewShape {
    rect: Rect,
    content: f32,
    anchor: Circle,
    near: Circle,
    shadow: f32,
}

/// Todo lo que se dibuja en un cuadro, calculado antes de pintar.
struct Frame {
    palette: Palette,
    skin: Hsla,
    pill: PillShape,
    mark: (f32, f32),
    eyes: (f32, f32),
    blink: f32,
    /// Cuánto de la cara es engranaje (0..1).
    gear: f32,
    strip_tools: Vec<IconDraw>,
    core: Option<(f32, f32, f32)>,
    core_anchor: Circle,
    blobs: Vec<(f32, f32, f32)>,
    wheel_icons: Vec<IconDraw>,
    wheel_shadow: Option<(f32, f32, f32, f32)>,
    panel: Option<PanelShape>,
    preview: Option<PreviewShape>,
    walls: Vec<Circle>,
    /// En un costado con su bloque: la columna y el bloque en un solo
    /// contorno (`geometry::side_outline`).
    outline: Option<Vec<(f32, f32)>>,
    /// La pill es de vidrio: tinte translúcido sobre el blur de `glass.rs`.
    glass: bool,
    /// Cuánto tapa ese tinte: más con contenido (`content_amount`), parejo
    /// en todo el tab para que no se vea un corte bajo la franja.
    tint: f32,
    /// Lo que suena: cuánto se ve (0..1), la carátula y dónde va.
    live: f32,
    art: Option<std::sync::Arc<gpui::RenderImage>>,
    art_center: Option<(f32, f32)>,
    /// El cursor está sobre la carátula o la onda: la carátula crece un poco,
    /// para que se note que abre algo.
    art_hover: bool,
    /// Nivel de la salida (0..1) y el reloj, para la onda.
    level: f32,
    time: f32,
    /// Color del punto de privacidad, si algo usa el micrófono o la cámara.
    privacy: Option<u32>,
    /// Grabando una reunión: el reloj. Va donde iría la carátula.
    recording: Option<String>,
    /// En pausa: carátula atenuada con ▶ y sin onda.
    paused: bool,
    /// La onda a la derecha (`bars_shown`).
    bars: bool,
}

struct IconDraw {
    path: &'static str,
    center: (f32, f32),
    size: f32,
    color: Hsla,
}

/// El cambio de paso de la tira («Más», «Atrás»), con el ritmo de los paneles
/// de Atic: las fichas que se van salen en el primer tramo, el tab se estira
/// entre medio y las nuevas entran al final, escalonadas, desde el lado al
/// que se avanza.
const PAGE_SWITCH_MS: f32 = 620.0;
/// Hasta dónde (de 0 a 1) salen las fichas viejas.
const PAGE_OUT_END: f32 = 0.42;
/// El tramo en que el tab cambia de largo.
const PAGE_LENGTH_START: f32 = 0.14;
const PAGE_LENGTH_SPAN: f32 = 0.72;
/// Desde dónde entran las nuevas, cuánto dura cada una y su escalón máximo.
const PAGE_IN_START: f32 = 0.48;
const PAGE_IN_SPAN: f32 = 0.34;
const PAGE_IN_STAGGER: f32 = 0.03;
/// Cuánto se corren las fichas al salir y al entrar.
const PAGE_SLIDE: f32 = 8.0;

fn ease_page(t: f32) -> f32 {
    cubic_bezier(0.45, 0.0, 0.2, 1.0, t)
}

struct PageSwitch {
    at: Instant,
    /// Las fichas del paso que se deja, para dibujarlas saliendo.
    from: Vec<StripId>,
    /// El largo del tab abierto al empezar.
    from_length: f32,
    /// 1 hacia «Más» (las nuevas llegan desde el final), -1 de vuelta.
    dir: f32,
}

impl PageSwitch {
    fn progress(&self, now: Instant) -> f32 {
        (now.duration_since(self.at).as_secs_f32() * 1000.0 / PAGE_SWITCH_MS).clamp(0.0, 1.0)
    }
}

impl Pill {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        let seed = now.elapsed().as_nanos() as u64 ^ 0x9e37_79b9_7f4a_7c15;
        let panel = cx.new(ClipboardPanel::new);
        let snippets = cx.new(SnippetsPanel::new);
        let launcher = cx.new(launcher::LauncherPanel::new);
        let agents = cx.new(agents::AgentsPanel::new);
        let media = media::Media::start();
        let phone = phone::start(media.clone());
        let dict_watch = dictation::Watch::spawn();
        let hotkeys = hotkeys::spawn(dict_watch.clone());
        // Agentes lleva un mini reproductor al pie con lo mismo que suena.
        agents.update(cx, |panel, _| panel.media = Some(media.clone()));
        let privacy = privacy::Privacy::start();
        // Un solo hilo para hablar con el audio y las pantallas: lo usan
        // Sistema y Ahora suena (volumen y salida).
        let backend = system::os::Backend::start();
        let media_panel = cx.new(|cx| media::MediaPanel::new(media.clone(), backend.clone(), cx));
        let system = cx.new(|cx| system::SystemPanel::new(privacy.clone(), backend, cx));
        let customize = cx.new(customize::CustomizePanel::new);
        let studio = meetings::studio(cx);
        let subscriptions = vec![
            // La grabadora avisa a ~20 fps mientras graba. El tab solo cambia
            // cuando cambia el segundo; el vistazo de Reuniones, con sus
            // medidores, cada vez.
            cx.observe(&studio, |pill, studio, cx| {
                let s = studio.read(cx);
                // El celular muestra el cronómetro y puede detenerla.
                phone::set_recording(s.recording().then(|| s.elapsed()).flatten().map(|elapsed| {
                    chrono::Utc::now().timestamp_millis() - elapsed.as_millis() as i64
                }));
                let clock = matches!(s.stage(), meetings::Stage::Recording | meetings::Stage::Stopping)
                    .then(|| s.elapsed().map(meetings::stopwatch))
                    .flatten();
                let peek = pill.tool_peek_owner() == Some(REUNIONES_TOOL);
                if clock != pill.rec_clock || peek {
                    pill.rec_clock = clock;
                    cx.notify();
                }
            }),
            cx.subscribe(&customize, |pill, _, event: &customize::CustomizeEvent, cx| match event {
                customize::CustomizeEvent::Close => pill.close_panel(true, cx),
                customize::CustomizeEvent::Placed(layout) => pill.layout_changed(layout.clone(), cx),
            }),
            cx.subscribe(&media_panel, |pill, _, event: &media::MediaEvent, cx| match event {
                media::MediaEvent::Close => pill.close_panel(true, cx),
            }),
            cx.subscribe(&system, |pill, _, event: &system::SystemEvent, cx| match event {
                system::SystemEvent::Close => pill.close_panel(true, cx),
                system::SystemEvent::Action(action) => {
                    let action = *action;
                    pill.close_panel(false, cx);
                    if let Err(error) = launcher::system_action(action) {
                        eprintln!("sistema: {error}");
                    }
                }
            }),
            cx.subscribe(&agents, |pill, _, event: &agents::AgentsEvent, cx| {
                pill.agents_event(event, cx)
            }),
            cx.subscribe_in(
                &launcher,
                window,
                |pill, _, event: &launcher::LauncherEvent, window, cx| {
                    pill.launcher_event(event, window, cx)
                },
            ),
            cx.subscribe_in(&panel, window, |pill, _, event: &PanelEvent, window, cx| match event {
                PanelEvent::Paste(entry) => pill.paste(entry.clone(), cx),
                PanelEvent::Drag(entry) => pill.start_drag(entry.clone(), cx),
                PanelEvent::Draw(entry) => pill.draw_entry(entry, window, cx),
                PanelEvent::Close => pill.close_panel(true, cx),
            }),
            cx.subscribe(&snippets, |pill, _, event: &SnippetEvent, cx| match event {
                SnippetEvent::Paste(text) => pill.paste_text(text.clone(), cx),
                SnippetEvent::Close => pill.close_panel(true, cx),
            }),
            // Clic fuera del panel: la ventana pierde el foco y el panel se cierra,
            // salvo que esté fijado.
            cx.observe_window_activation(window, |pill, window, cx| {
                if !window.is_window_active() && pill.panel_open && !pill.panel_pinned(cx) {
                    pill.close_panel(false, cx);
                }
            }),
        ];

        let scale_factor = window.scale_factor();
        let viewport = window.viewport_size();
        let overlay = win::Overlay::attach(window);
        let viewport = Rect::new(
            0.0,
            0.0,
            f32::from(viewport.width),
            f32::from(viewport.height),
        );
        let (monitor, work, dockable) = overlay
            .as_ref()
            .and_then(|overlay| screen_geometry(overlay, scale_factor))
            .unwrap_or((viewport, viewport, geometry::dockable_edges(&viewport, &viewport)));
        let screen = overlay.as_ref().and_then(|overlay| {
            Some((overlay.screen()?, scale_factor, overlay.origin()))
        });
        // Si la pill quedó en otro monitor, la ventana se va allá y, al
        // llegar, `refresh_screen` vuelve a leer dónde estaba. Si no, igual
        // se calza al suyo: GPUI la abre en lógicos y puede quedar corrida.
        let mut restore_home = false;
        let mut moving_to = None;
        if let Some((overlay, (current, _, _))) = overlay.as_ref().zip(screen) {
            let saved = saved_screen().filter(|saved| *saved != current);
            restore_home = saved.is_some();
            moving_to = saved;
            overlay.move_to(&saved.unwrap_or(current));
        }
        let home = load_home(&work, &dockable).unwrap_or_else(|| default_home(&work));

        let glass = glass::enabled().then(glass::Glass::new).flatten();
        let mut pill = Self {
            overlay,
            glass,
            scale_factor,
            cursor: None,
            born: now,
            last_frame: now,
            ticks: 0,
            calm: false,
            fps: (now, 0),
            monitor,
            work,
            dockable,
            screen,
            screen_checked: now,
            restore_home,
            moving_to,
            home,
            press: None,
            seat_started: None,
            hover_since: None,
            strip: Tween::new(0.0, Duration::from_millis(240), ease_island),
            strip_leave_at: None,
            // Como `.p-island-tool` en la web: crece con un leve rebote.
            // El hover es por posición en el paso que se está viendo, así que
            // hay uno por ficha del paso más largo.
            strip_hover: (0..pill_tools::STRIP_MAX)
                .map(|_| Tween::new(0.0, Duration::from_millis(240), ease_island))
                .collect(),
            strip_pulse: None,
            layout: pill_tools::load(),
            strip_page: Page::Ring,
            page_switch: None,
            hold: None,
            wheel_target_open: false,
            wheel_time: 0.0,
            wheel_page: Page::Ring,
            wheel_bloom: 0.0,
            wheel_opener: WheelOpener::Click,
            wheel_leave_at: None,
            wheel_hover: (0..pill_tools::STRIP_MAX)
                .map(|_| Tween::new(0.0, Duration::from_millis(120), ease_smooth_out))
                .collect(),
            shortcut_was_down: false,
            notch_h: Tween::new(
                panel.read(cx).desired_height(),
                Duration::from_millis(280),
                ease_smooth_out,
            ),
            preview: Tween::new(0.0, Duration::from_millis(280), ease_smooth_out),
            preview_entry: None,
            peek: Tween::new(0.0, Duration::from_millis(260), ease_island),
            peek_entries: Vec::new(),
            peek_hover_since: None,
            peek_shown_at: now,
            peek_leave_at: None,
            peek_hovered: None,
            peek_press: None,
            demo_peek_until: std::env::var("PILL_OPEN")
                .is_ok_and(|tool| tool == "peek")
                .then(|| now + Duration::from_secs(12)),
            capture: None,
            capture_pending: false,
            flip: None,
            flip_events: None,
            flip_target: None,
            capture_events: None,
            board: None,
            board_events: None,
            color: None,
            color_events: None,
            toast: None,
            span: None,
            shelves: Vec::new(),
            shelf_seq: 0,
            panel,
            notch_tool: NotchTool::Clipboard,
            launcher,
            agents,
            system,
            customize,
            media,
            media_panel,
            privacy,
            studio,
            rec_clock: None,
            tray: tray::Banner::new(),
            live: Tween::new(0.0, Duration::from_millis(320), ease_island),
            music_playing: false,
            paused_since: None,
            fit: Tween::new(TAB_LENGTH, Duration::from_millis(320), ease_island),
            hang: Tween::new(0.0, Duration::from_millis(320), ease_island),
            dictation: dictation::Dictation::new(),
            gear: Tween::new(0.0, Duration::from_millis(260), ease_island),
            dict: Tween::new(0.0, Duration::from_millis(320), ease_island),
            dict_watch,
            dict_key_was_down: false,
            usage: usage::Usage::default(),
            usage_peek: Tween::new(0.0, Duration::from_millis(300), ease_island),
            block_along: Tween::new(0.0, Duration::from_millis(240), ease_island),
            tool_peek: peeks::ToolPeek::new(),
            usage_hover_since: None,
            usage_leave_at: None,
            level: 0.0,
            space: None,
            flight: None,
            away: None,
            launcher_centered: false,
            hotkeys,
            tray_icon: tray_icon::spawn(),
            phone,
            snippets,
            panel_open: false,
            panel_time: 0.0,
            paste_target: None,
            eyes: (0.0, 0.0),
            next_blink: now,
            blink_started: None,
            double_blink: false,
            rng: Rng(seed | 1),
            _subscriptions: subscriptions,
        };
        pill.schedule_blink(now);

        // `PILL_OPEN=clipboard` abre el panel al arrancar: para revisar el
        // diseño con una captura sin tener que mover el mouse.
        let open_on_start = std::env::var("PILL_OPEN").ok();
        let notch_on_start = open_on_start.as_deref().and_then(notch_tool_named);
        // `PILL_OPEN_SEQ=clipboard,textos,cerrar`: abre esas herramientas una
        // tras otra con el notch abierto (cada `PILL_OPEN_STEP_MS`, 1500 por
        // omisión) y «cerrar» lo cierra: para ver el cambio de contenido y
        // el cierre en un borde sin teclas ni clics.
        if let Ok(seq) = std::env::var("PILL_OPEN_SEQ") {
            let step = std::env::var("PILL_OPEN_STEP_MS")
                .ok()
                .and_then(|ms| ms.parse().ok())
                .unwrap_or(1500);
            cx.spawn_in(window, async move |this, cx| {
                for name in seq.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    cx.background_executor()
                        .timer(Duration::from_millis(step))
                        .await;
                    let name = name.to_string();
                    let alive = this.update_in(cx, |pill, window, cx| match notch_tool_named(&name) {
                        Some(tool) => pill.open_notch(tool, window, cx),
                        None => pill.close_panel(true, cx),
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        let emoji_on_start = open_on_start.as_deref() == Some("emoji");
        if let Some(tool) = notch_on_start {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let _ = this.update_in(cx, |pill, window, cx| {
                    pill.open_notch(tool, window, cx);
                    if emoji_on_start {
                        pill.launcher.update(cx, |launcher, cx| launcher.open_emoji(cx));
                    }
                });
            })
            .detach();
        }
        // `PILL_OPEN=capture` o `board`: la mira o la pizarra al arrancar.
        if let Some(tool) = open_on_start.filter(|tool| {
            tool == "capture" || tool == "board" || tool == "flip" || tool == "shelf" || tool == "space"
        }) {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let _ = this.update_in(cx, |pill, window, cx| {
                    if tool == "space" {
                        pill.open_space(None, cx)
                    } else if tool == "board" {
                        pill.start_board(window, cx)
                    } else if tool == "flip" {
                        pill.start_flip(window, cx)
                    } else if tool == "shelf" {
                        pill.demo_shelf(cx)
                    } else {
                        pill.start_capture(false, window, cx)
                    }
                });
            })
            .detach();
        }

        // Sondeo del cursor global: la ventana deja pasar los clics casi siempre,
        // así que no recibe eventos de mouse mientras el cursor está afuera.
        cx.spawn_in(window, async move |this, cx| {
            let mut every = TICK_ACTIVE;
            loop {
                cx.background_executor().timer(every).await;
                match this.update_in(cx, |pill, window, cx| {
                    pill.tick(window, cx);
                    pill.tick_every()
                }) {
                    Ok(next) => every = next,
                    Err(_) => break,
                }
            }
        })
        .detach();

        pill
    }

    fn schedule_blink(&mut self, now: Instant) {
        let seconds = 2.6 + self.rng.next() * 6.4;
        self.next_blink = now + Duration::from_secs_f32(seconds);
    }

    // --- Forma de la pill -------------------------------------------------

    fn seat_scale(&self, now: Instant) -> f32 {
        let Some(started) = self.seat_started else {
            return 1.0;
        };
        let t = (now.duration_since(started).as_secs_f32() * 1000.0 / SEAT_MS).clamp(0.0, 1.0);
        let amount = if t < 0.38 {
            ease_smooth_out(t / 0.38)
        } else {
            1.0 - ease_smooth_out((t - 0.38) / 0.62)
        };
        1.0 - SEAT_DEPTH * amount
    }

    /// El largo del tab con la tira abierta: el del paso que se ve o, mientras
    /// se cambia de paso, camino a él.
    fn strip_open_now(&self, now: Instant) -> f32 {
        let target = strip_open_length(self.strip_slots().len());
        match &self.page_switch {
            Some(switch) => {
                let t = segment(switch.progress(now), PAGE_LENGTH_START, PAGE_LENGTH_SPAN);
                lerp(switch.from_length, target, ease_page(t))
            }
            None => target,
        }
    }

    fn tab_length(&self, now: Instant) -> f32 {
        let open = self.strip_open_now(now);
        let strip = lerp(TAB_LENGTH, open, self.strip.value(now));
        let live = lerp(TAB_LENGTH, LIVE_LENGTH, self.live.value(now));
        strip.max(live).max(self.fit.value(now)).max(TAB_LENGTH * 0.96)
    }

    /// Lo que va en el tab en reposo: a la izquierda el reloj de la grabación
    /// o la carátula; junto a la cara, los contadores de la bandeja; a la
    /// derecha la onda y el punto de privacidad. La cara va al centro, así que
    /// el tab mide el doble del lado más largo.
    fn fit_length(&self) -> f32 {
        let (left_chip, right_chip) = tray::chip_reach(self.tray.chips);
        let left_slot = match &self.rec_clock {
            // El punto (8) y el reloj, en Cascadia Mono de 11 px.
            Some(clock) => Some(15.0 + clock.chars().count() as f32 * 7.0),
            None => (self.live.target() == 1.0).then_some(LIVE_ART),
        };
        let privacy = !self.privacy.uses().is_empty();
        let right_slot = match (self.bars_shown(), privacy) {
            (true, true) => Some(BARS_W + 12.0),
            (true, false) => Some(BARS_W),
            (false, true) => Some(8.0),
            (false, false) => None,
        };
        let side = |chip: f32, slot: Option<f32>| {
            (MARK_SIZE / 2.0).max(chip) + slot.map_or(0.0, |w| 8.0 + w + SLOT_PAD)
        };
        (2.0 * side(left_chip, left_slot).max(side(right_chip, right_slot))).max(TAB_LENGTH)
    }

    /// La onda: solo con música sonando, y no grabando ni dictando (cada uno
    /// trae lo suyo; dos ondas confunden).
    fn bars_shown(&self) -> bool {
        self.music_playing && self.rec_clock.is_none() && !self.dictation_shown()
    }

    /// Cuánto se ve la actividad en vivo (la tira abierta la tapa).
    fn live_shown(&self, now: Instant) -> f32 {
        let strip = self.strip.value(now).clamp(0.0, 1.0);
        (self.live.value(now) * (1.0 - strip)).clamp(0.0, 1.0)
    }

    /// Centro de la carátula en el tab.
    /// Centro de la carátula: al principio del tab (a la izquierda, o arriba
    /// en los bordes laterales), o como insignia abajo a la derecha de la gota.
    fn art_center(&self, now: Instant) -> Option<(f32, f32)> {
        let mark = self.mark_center(now);
        Some(match self.shape(now) {
            PillShape::Tab { edge, rect, .. } if edge.is_vertical() => {
                (mark.0, rect.y + 9.0 + LIVE_ART / 2.0)
            }
            PillShape::Tab { rect, .. } => (rect.x + 9.0 + LIVE_ART / 2.0, mark.1),
            PillShape::Disc { center } => (center.0 + DISC_R * 0.68, center.1 + DISC_R * 0.68),
        })
    }

    /// La carátula o la onda: todo el tab menos la marca; en la gota, la
    /// insignia de la carátula.
    fn over_art(&self, p: (f32, f32), now: Instant) -> bool {
        if self.live_shown(now) <= 0.5 || self.rec_clock.is_some() {
            return false;
        }
        match self.shape(now) {
            PillShape::Disc { .. } => self
                .art_center(now)
                .is_some_and(|c| distance(p, c) <= LIVE_BADGE / 2.0 + 4.0),
            PillShape::Tab { .. } => {
                self.over_pill(p, now, 0.0) && !self.over_mark(p, now) && self.tray_chip_at(p, now).is_none()
            }
        }
    }

    /// Cuánto se estiró el notch hacia el tamaño de la herramienta (0..1, con
    /// el leve sobrepaso de `--ease-island`).
    fn notch_morph(&self) -> f32 {
        if !self.at_notch() || !self.panel_visible() || self.centered_launcher() {
            return 0.0;
        }
        ease_island(segment(self.panel_time, 0.0, NOTCH_MORPH_MS))
    }

    /// La pill está acoplada donde el notch se abre en su lugar: arriba
    /// siempre; en otro borde, si el panel cabe ahí (`geometry::notch_fits`).
    fn at_notch(&self) -> bool {
        self.flight.is_none()
            && match self.home {
                Home::Docked { edge, .. } => {
                    geometry::notch_fits(edge, &self.work, SIDE_PANEL_W, NOTCH_MIN_SIDE_H)
                }
                Home::Floating { .. } => false,
            }
    }

    /// Acoplada a cualquier borde y quieta: ahí salen la tira, los vistazos,
    /// la letra y el dictado (en la gota no hay franja de donde colgarlos).
    pub(crate) fn docked_still(&self) -> bool {
        self.flight.is_none() && matches!(self.home, Home::Docked { .. })
    }

    /// Acoplada a un costado: lo que cuelga del tab va en un bloque angosto
    /// al lado de la columna, con su contenido en columna (`SIDE_W`).
    pub(crate) fn side_drawers(&self) -> bool {
        matches!(self.home, Home::Docked { edge, .. } if edge.is_vertical())
    }

    /// El ancho del panel en `edge`: arriba el de siempre, en un costado
    /// angosto.
    fn panel_w(edge: Edge) -> f32 {
        if edge.is_vertical() {
            SIDE_PANEL_W
        } else {
            PANEL_W
        }
    }

    /// Dónde puede estirarse el notch en `edge`: el área de trabajo; en un
    /// costado, sin los márgenes de arriba y abajo (el panel alto no toca las
    /// esquinas).
    fn notch_area(&self, edge: Edge) -> Rect {
        if edge.is_vertical() {
            let m = SIDE_PANEL_MARGIN;
            Rect::new(self.work.x, self.work.y + m, self.work.w, self.work.h - m * 2.0)
        } else {
            self.work
        }
    }

    /// El alto máximo del panel: arriba el de cada herramienta; en un
    /// costado, casi todo el alto del área de trabajo.
    pub(crate) fn panel_max_height(&self) -> Option<f32> {
        self.side_drawers().then(|| self.work.h - SIDE_PANEL_MARGIN * 2.0)
    }

    /// A qué altura sale el bloque de un costado: junto a la herramienta del
    /// vistazo abierto (con la tira abierta), o junto a la marca.
    fn block_anchor(&self, now: Instant) -> Option<f32> {
        if !self.side_drawers() {
            return None;
        }
        let tool = if let Some(tool) = self.tool_peek_owner() {
            Some(tool)
        } else if self.peek.target() == 1.0 {
            Some(CLIPBOARD_TOOL)
        } else if self.usage_peek.target() == 1.0 || self.tray.is_open() {
            Some(AGENTES_TOOL)
        } else {
            None
        };
        // `tool` es su lugar en el catálogo; en la tira va donde la puso el
        // editor, o en otro paso (entonces el bloque sale junto a la marca).
        let index = tool.and_then(|tool| {
            self.strip_slots()
                .iter()
                .position(|&slot| slot == StripId::Tool(tool))
        });
        match index {
            Some(index) if self.strip.target() == 1.0 => Some(self.strip_tool_along(index, now)),
            _ => self.mark_along(now),
        }
    }

    /// Lleva el bloque de un costado a la altura que toca. Recién abierto
    /// aparece ahí mismo; abierto, se desliza (mismo tiempo y curva que el
    /// alto de los vistazos). Mientras se recoge se queda donde estaba.
    fn update_block(&mut self, now: Instant) {
        let opening = self.peek.target() == 1.0
            || self.tool_peek_owner().is_some()
            || self.usage_peek.target() == 1.0
            || self.tray.is_open()
            || self.hang.target() == 1.0
            || self.dict.target() == 1.0;
        let Some(anchor) = self.block_anchor(now).filter(|_| opening) else {
            return;
        };
        if self.drawers_stack(now) < 1.0 {
            self.block_along.snap(anchor);
        } else if (self.block_along.target() - anchor).abs() > 0.5 {
            self.block_along.set(anchor, now);
        }
    }

    /// Adónde vuela la gota cuando el notch no se puede abrir donde está:
    /// arriba, al centro del área de trabajo.
    fn notch_home(&self) -> Home {
        Home::Docked {
            edge: Edge::Top,
            along: self.work.x + self.work.w / 2.0,
        }
    }

    /// Largo y grosor del notch estirado en `edge` (la franja es parte del
    /// panel): el panel va de pie, así que a un costado se mide al revés.
    fn notch_size(&self, edge: Edge, now: Instant) -> (f32, f32) {
        edge.extent(Self::panel_w(edge), self.notch_h.value(now))
    }

    fn shape(&self, now: Instant) -> PillShape {
        if let Some(flight) = &self.flight {
            return PillShape::Disc {
                center: flight.center(now),
            };
        }
        match self.home {
            Home::Docked { edge, along } => {
                let morph = self.notch_morph();
                let (full_length, full_thick) = self.notch_size(edge, now);
                let base = self.tab_length(now);
                // El vistazo del portapapeles va al ancho del tab (arriba es el
                // de la tira abierta: no lo cambia).
                let peek = self.peek.value(now);
                // El vistazo de la bandeja ensancha el tab y baja unas filas.
                let (tray, tray_h) = self.tray_stretch(now);
                // La letra cuelga del tab igual: más ancho y una franja abajo.
                let hang = self.hang_amount(now);
                // El dictado también: su franja con la onda del micrófono.
                let dict = self.dict_amount(now);
                // Y el vistazo de uso de los agentes, al ancho de un panel.
                let usage = self.usage_amount(now);
                // Y el vistazo de una herramienta, al ancho de la tira.
                let tool = self.tool_peek_amount(now);
                let (length, thick, block) = if edge.is_vertical() {
                    // En un costado lo que cuelga no ensancha la columna: sale
                    // en un bloque angosto al lado, a la altura de la
                    // herramienta bajo el cursor, que mide solo lo que ocupa
                    // su contenido (puesto en columna). Crece desde la
                    // columna a medida que bajan las franjas.
                    let open = [peek, tray, hang, dict, usage, tool]
                        .into_iter()
                        .fold(0.0, |open: f32, a| open + (1.0 - open) * a.max(0.0));
                    let (w, h) = (SIDE_W * open, self.drawers_stack(now));
                    let band = TAB_THICK * self.seat_scale(now);
                    let block = (w > 1.0 && h > 1.0 && morph <= 0.0).then(|| {
                        geometry::side_block(edge, &self.work, band, self.block_along.value(now), w, h)
                    });
                    (base, band, block)
                } else {
                    let widen = |w: f32| {
                        let w = w + (base.max(w) - w) * peek;
                        let w = Self::hang_length(self.tray_length(w, tray), hang);
                        let w = Self::dict_length(w, dict);
                        let w = Self::usage_length(w, usage);
                        Self::tool_peek_length(w, tool)
                    };
                    let usage_h = if usage > 0.0 { self.usage_height() * usage } else { 0.0 };
                    let tool_h = if tool > 0.0 { self.tool_peek_height(now) * tool } else { 0.0 };
                    let stack = self.peek_height() * peek
                        + tray_h * tray
                        + hang::HANG_H * hang
                        + dictation::DICT_H * dict
                        + usage_h
                        + tool_h;
                    // El aplastón de acoplado achica la franja; con el vistazo
                    // abierto vuelve a su grosor.
                    let band = lerp(TAB_THICK * self.seat_scale(now), TAB_THICK, peek);
                    (widen(base), band + stack, None)
                };
                let length = lerp(length, full_length, morph);
                let thick = lerp(thick, full_thick, morph);
                // Al abrirse la tira crece hacia los dos lados; si no cabe, se
                // corre para no salirse del borde (estirándose hacia el panel,
                // también de los márgenes de un costado).
                let area = if morph > 0.0 { self.notch_area(edge) } else { self.work };
                let along = geometry::clamp_along(edge, &area, along, length);
                PillShape::Tab {
                    edge,
                    rect: edge.tab_rect(&self.work, along, length, thick),
                    along,
                    thick,
                    block,
                }
            }
            Home::Floating { center } => PillShape::Disc { center },
        }
    }

    /// Posición de la marca a lo largo del tab (o el centro de la gota).
    fn mark_along(&self, now: Instant) -> Option<f32> {
        match self.shape(now) {
            PillShape::Tab { edge, .. } => Some(edge.along_of(self.mark_center(now))),
            PillShape::Disc { .. } => None,
        }
    }

    fn mark_center(&self, now: Instant) -> (f32, f32) {
        match self.shape(now) {
            PillShape::Tab {
                edge,
                rect,
                along,
                thick,
                ..
            } => {
                // En reposo, al medio del tab; con la tira abierta, al
                // principio. Siempre en la franja junto al borde.
                let open = self.strip.value(now).clamp(0.0, 1.0);
                // El principio de la tira sola, sin lo que la ensancha
                // (vistazos, bandeja, uso): el tab crece hacia los dos lados y
                // las fichas se quedan donde están. Si se corrieran, la de
                // debajo del cursor perdería el hover y el vistazo se cerraría.
                let start = match self.home {
                    Home::Docked { along: home, .. } => {
                        let length = self.tab_length(now);
                        let area = if self.notch_morph() > 0.0 { self.notch_area(edge) } else { self.work };
                        geometry::clamp_along(edge, &area, home, length) - length / 2.0
                    }
                    Home::Floating { .. } => edge.along_of((rect.x, rect.y)),
                };
                let along = lerp(along, start + 6.0 + MARK_SIZE / 2.0, open);
                let resting = edge.point(&self.work, along, thick.min(TAB_THICK) / 2.0);
                // Con el notch abierto va a la izquierda de la franja de
                // arriba del panel (el hueco `MARK_GAP` de los buscadores), en
                // cualquier borde: el panel va de pie. Arriba esa franja es la
                // del tab y la marca solo se corre a lo largo.
                let morph = self.notch_morph().clamp(0.0, 1.0);
                let band = (rect.x + MARK_GAP / 2.0, rect.y + rect.h.min(TAB_THICK) / 2.0);
                (lerp(resting.0, band.0, morph), lerp(resting.1, band.1, morph))
            }
            PillShape::Disc { center } => center,
        }
    }

    fn strip_tool_along(&self, index: usize, now: Instant) -> f32 {
        let open = self.strip.value(now).max(0.0);
        let first = self.mark_along(now).unwrap_or_default() + MARK_SIZE / 2.0 + 6.0;
        first + index as f32 * (TOOL_W + TOOL_GAP) * open + TOOL_W * open / 2.0
    }

    fn over_pill(&self, p: (f32, f32), now: Instant, margin: f32) -> bool {
        match self.shape(now) {
            PillShape::Tab { rect, block, .. } => {
                rect.contains(p, margin) || block.is_some_and(|block| block.contains(p, margin))
            }
            PillShape::Disc { center } => distance(p, center) <= DISC_R + margin,
        }
    }

    fn over_mark(&self, p: (f32, f32), now: Instant) -> bool {
        match self.shape(now) {
            PillShape::Tab { edge, rect, .. } => {
                rect.contains(p, 0.0)
                    && (edge.along_of(p) - self.mark_along(now).unwrap_or_default()).abs()
                        <= MARK_SIZE / 2.0 + 2.0
            }
            PillShape::Disc { center } => distance(p, center) <= DISC_R,
        }
    }

    /// La ficha del paso que se está viendo bajo el cursor, si la hay.
    fn strip_slot_at(&self, p: (f32, f32), now: Instant) -> Option<(usize, StripId)> {
        let PillShape::Tab { edge, rect, .. } = self.shape(now) else {
            return None;
        };
        if self.strip.value(now) < 0.85 || !rect.contains(p, 0.0) {
            return None;
        }
        // Solo en la franja: lo que cuelga de ella (vistazos, uso, letra; al
        // lado de la columna en un costado) no es la herramienta de al lado.
        // Si no, cruzar los anillos del uso «pasaba» por otras herramientas
        // y cerraba el vistazo.
        if edge.depth_of(&self.work, p) > TAB_THICK {
            return None;
        }
        let along = edge.along_of(p);
        let slots = self.strip_slots();
        (0..slots.len())
            .find(|&index| {
                (along - self.strip_tool_along(index, now)).abs() <= (TOOL_W + TOOL_GAP) / 2.0
            })
            .map(|index| (index, slots[index]))
    }

    /// Las fichas del paso que se está viendo.
    fn strip_slots(&self) -> Vec<StripId> {
        pill_tools::strip_page(&self.layout, self.strip_page)
    }

    /// La herramienta bajo el cursor, sea del nivel que sea. `Más`, «Atrás» y
    /// «Personalizar» no son herramientas: no devuelven ninguna.
    fn strip_tool_at(&self, p: (f32, f32), now: Instant) -> Option<usize> {
        self.strip_slot_at(p, now).and_then(|(_, slot)| slot.tool())
    }

    // --- Rueda --------------------------------------------------------------

    fn wheel_place(&self, now: Instant) -> WheelPlace {
        let keep_inside = |center: (f32, f32)| {
            Rect::centered(center, WHEEL_BOX, WHEEL_BOX)
                .clamped_into(&self.work)
                .center()
        };
        match self.shape(now) {
            PillShape::Tab {
                edge, along, thick, ..
            } => WheelPlace {
                source: edge.point(&self.work, along, thick - 14.0),
                source_r: 10.0,
                anchor: (edge.point(&self.work, along, thick - 20.0), 20.0),
                center: keep_inside(edge.point(&self.work, along, WHEEL_DEPTH)),
            },
            PillShape::Disc { center } => WheelPlace {
                source: center,
                source_r: DISC_R,
                anchor: (center, DISC_R),
                center: keep_inside(center),
            },
        }
    }

    fn wheel_visible(&self) -> bool {
        self.wheel_target_open || self.wheel_time > 0.0
    }

    fn wheel_fully_open(&self) -> bool {
        self.wheel_target_open && self.wheel_time >= self.wheel_bloom + BLOB_START_MS + BLOB_MS
    }

    /// `None` fuera de la rueda, `Some(None)` en el centro, `Some(Some(i))` en
    /// el gajo número `i` del nivel a la vista.
    ///
    /// La rueda muestra solo el primer nivel (más la puerta a «Más»), como la
    /// tira antes de las dos páginas: el segundo nivel se abre desde la tira.
    fn wheel_target_at(&self, p: (f32, f32), now: Instant) -> Option<Option<usize>> {
        let center = self.wheel_place(now).center;
        let (dx, dy) = (p.0 - center.0, p.1 - center.1);
        let distance = dx.hypot(dy);
        if distance > WHEEL_HIT_R {
            return None;
        }
        if distance <= WHEEL_CORE_R {
            return Some(None);
        }
        let gajos = self.wheel_slots().len();
        let slice = TAU / gajos as f32;
        let angle = (dy.atan2(dx) + PI / 2.0 + slice / 2.0).rem_euclid(TAU);
        Some(Some((angle / slice) as usize % gajos))
    }

    /// Los gajos de la rueda: el nivel a la vista más la puerta a «Más» (o
    /// «Atrás»). «Personalizar» no: el editor solo se abre acoplada.
    fn wheel_slots(&self) -> Vec<StripId> {
        pill_tools::strip_page(&self.layout, self.wheel_page)
            .into_iter()
            .filter(|&slot| slot != StripId::Customize)
            .collect()
    }

    /// Cambia el nivel de la rueda sin cerrarla: los gajos nuevos brotan del
    /// núcleo como al abrirla.
    fn go_to_wheel_page(&mut self, page: Page, now: Instant) {
        self.wheel_page = page;
        self.wheel_bloom = (self.wheel_time - BLOB_START_MS).max(0.0);
        for tween in self.wheel_hover.iter_mut() {
            tween.set(0.0, now);
        }
    }

    fn open_wheel(&mut self, opener: WheelOpener) {
        if self.wheel_time <= 0.0 {
            self.wheel_page = Page::Ring;
            self.wheel_bloom = 0.0;
        }
        self.wheel_target_open = true;
        self.wheel_opener = opener;
        self.wheel_leave_at = None;
    }

    fn toggle_wheel(&mut self, opener: WheelOpener) {
        if self.wheel_target_open {
            self.close_wheel();
        } else {
            self.open_wheel(opener);
        }
    }

    fn close_wheel(&mut self) {
        // El brote corrido no se deshace: el cierre parte de la rueda entera.
        self.wheel_time = (self.wheel_time - self.wheel_bloom).max(0.0);
        self.wheel_bloom = 0.0;
        self.wheel_target_open = false;
        self.wheel_leave_at = None;
    }

    // --- Panel --------------------------------------------------------------

    fn panel_visible(&self) -> bool {
        self.panel_open || self.panel_time > 0.0
    }

    /// El lanzador centrado no estira el notch: es una tarjeta aparte.
    fn centered_launcher(&self) -> bool {
        self.notch_tool == NotchTool::Apps && self.launcher_centered
    }

    fn panel_shape(&self, now: Instant) -> Option<PanelShape> {
        if !self.panel_visible() {
            return None;
        }
        if self.centered_launcher() {
            // Tipo Spotlight: en el tercio de arriba, crece desde una franja.
            let grow = ease_island(segment(self.panel_time, 0.0, NOTCH_MORPH_MS));
            let height = self.notch_h.value(now);
            let w = lerp(PANEL_W * 0.92, PANEL_W, grow);
            let h = lerp(BAND_H, height, grow);
            let rect = Rect::new(
                self.work.x + (self.work.w - w) / 2.0,
                self.work.y + self.work.h * 0.2,
                w,
                h,
            );
            // Sin cuello: los dos círculos lejos para que no se unan.
            let far = ((rect.x, rect.y - 1000.0), 1.0);
            return Some(PanelShape {
                rect,
                content: segment(self.panel_time, NOTCH_CONTENT_START_MS, NOTCH_CONTENT_MS),
                notch: false,
                edge: Edge::Top,
                anchor: far,
                near: ((rect.x, rect.y), 1.0),
            });
        }
        // El notch: el contenido va donde termina, no donde va pasando; se
        // muestra recién cuando el cuerpo ya casi llegó. Mientras la gota
        // vuela todavía no hay notch.
        let Home::Docked { edge, along } = self.home else {
            return None;
        };
        if !self.at_notch() {
            return None;
        }
        let dummy = ((0.0, 0.0), 0.0);
        Some(PanelShape {
            rect: geometry::notch_rect(edge, &self.notch_area(edge), along, Self::panel_w(edge), self.notch_h.value(now)),
            content: segment(self.panel_time, NOTCH_CONTENT_START_MS, NOTCH_CONTENT_MS),
            notch: true,
            edge,
            anchor: dummy,
            near: dummy,
        })
    }

    fn peek_height(&self) -> f32 {
        let rows = self.peek_entries.len().max(1) as f32;
        PEEK_PAD + rows * PEEK_ROW + PEEK_FOOTER + PEEK_BOTTOM
    }

    /// Lo que el vistazo ocupa bajo la franja (al lado de la columna en un
    /// costado), con el notch estirado.
    fn peek_rect(&self, now: Instant) -> Option<Rect> {
        if self.panel_visible() || self.peek.value(now) <= 0.01 {
            return None;
        }
        self.drawers_area(now)
    }

    fn peek_hit(&self, p: (f32, f32), now: Instant) -> Option<PeekHit> {
        let rect = self.peek_rect(now)?;
        if self.peek.value(now) < 0.85 || !rect.contains(p, 0.0) {
            return None;
        }
        let y = p.1 - rect.y - PEEK_PAD;
        let rows = self.peek_entries.len();
        if y < 0.0 {
            return None;
        }
        let row = (y / PEEK_ROW) as usize;
        if row < rows {
            Some(PeekHit::Row(row))
        } else if y < rows.max(1) as f32 * PEEK_ROW + PEEK_FOOTER {
            Some(PeekHit::Footer)
        } else {
            None
        }
    }

    /// Pega sin abrir nada: el overlay no tiene el foco, así que la ventana
    /// activa es la app donde el usuario estaba.
    fn paste_from_peek(&mut self, entry: clipboard::Entry, cx: &mut Context<Self>) {
        let target = paste::foreground_target();
        self.peek.set(0.0, Instant::now());
        println!("vistazo → pegar {}", entry.key);
        if let Some(item) = clipboard_item(&entry) {
            cx.spawn(async move |_, cx| paste_into(target, item, cx).await)
                .detach();
        }
    }

    /// La burbuja nace metida en el costado del notch, crece y se separa
    /// hasta `PREVIEW_GAP`, donde el cuello se corta. Va a la derecha si cabe.
    fn preview_shape(&self, now: Instant) -> Option<PreviewShape> {
        let t = self.preview.value(now);
        if t <= 0.001 || self.preview_entry.is_none() {
            return None;
        }
        // Se desprende del cuerpo, o del bloque de un costado hacia el lado
        // libre.
        let body = match (self.home, self.shape(now)) {
            (Home::Docked { .. }, PillShape::Tab { rect, block, .. }) => block.unwrap_or(rect),
            _ => self.panel_shape(now)?.rect,
        };
        let right = self.work.right() - body.right() >= PREVIEW_W + PREVIEW_GAP + 8.0;
        let grow = (t / 0.7).min(1.0);
        let width = lerp(PREVIEW_SEED, PREVIEW_W, grow);
        let height = lerp(PREVIEW_SEED, PREVIEW_H, grow);
        // De metida a medias en el notch a separada.
        let gap = lerp(-PREVIEW_SEED / 2.0, PREVIEW_GAP, t);
        let top = (body.y + BAND_H).min(self.work.bottom() - height - 8.0);
        let x = if right {
            body.right() + gap
        } else {
            body.x - gap - width
        };
        let rect = Rect::new(x, top, width, height);
        // Con el notch más bajo que 40 px (encogiéndose, o el aplastón) el
        // rango se invierte: `clamp` entra en pánico con min > max.
        let neck_low = body.y + 20.0;
        let neck_y = (top + 24.0).clamp(neck_low, (body.bottom() - 20.0).max(neck_low));
        let near_r = PANEL_CORNER.min(width / 2.0).min(height / 2.0);
        let (anchor_x, near_x) = if right {
            (body.right() - 20.0, rect.x + near_r)
        } else {
            (body.x + 20.0, rect.right() - near_r)
        };
        Some(PreviewShape {
            rect,
            content: segment(t, 0.7, 0.3),
            anchor: ((anchor_x, neck_y), 20.0),
            near: ((near_x, neck_y), near_r),
            shadow: t,
        })
    }

    fn over_panel(&self, p: (f32, f32), now: Instant, margin: f32) -> bool {
        self.panel_shape(now)
            .is_some_and(|shape| shape.rect.contains(p, margin))
    }

    fn open_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_notch(NotchTool::Clipboard, window, cx);
    }

    /// Abre una herramienta en el notch. Con otra ya abierta solo cambia el
    /// contenido: el notch no se vuelve a abrir. Acoplada a un borde, el notch
    /// se estira ahí mismo; flotando (o si el panel no cabe en ese costado),
    /// primero vuela arriba (`tick` lleva el vuelo).
    pub(crate) fn open_notch(&mut self, tool: NotchTool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_wheel();
        self.peek.set(0.0, Instant::now());
        self.tray.close(Instant::now());
        if self.panel_open && self.notch_tool == tool {
            return;
        }
        if !self.panel_open {
            self.paste_target = paste::foreground_target();
        }
        self.notch_tool = tool;
        match tool {
            NotchTool::Clipboard => self.panel.update(cx, |panel, cx| panel.reset(cx)),
            NotchTool::Textos => self.snippets.update(cx, |panel, cx| panel.reset(cx)),
            NotchTool::Apps => self.launcher.update(cx, |panel, cx| panel.reset(cx)),
            NotchTool::Agentes => self.agents.update(cx, |panel, cx| panel.reset(cx)),
            NotchTool::Sistema => self.system.update(cx, |panel, cx| panel.reset(cx)),
            NotchTool::Media => self.media_panel.update(cx, |panel, cx| panel.reset(cx)),
            NotchTool::Personalizar => {}
        }
        self.sync_panel_limits(cx);
        self.notch_h.snap(self.panel_desired_height(cx));
        self.panel_open = true;
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(true);
        }
        let focus = self.panel_focus(cx);
        window.focus(&focus);
        cx.notify();
    }

    fn panel_desired_height(&self, cx: &Context<Self>) -> f32 {
        let desired = match self.notch_tool {
            NotchTool::Clipboard => self.panel.read(cx).desired_height(),
            NotchTool::Textos => self.snippets.read(cx).desired_height(),
            NotchTool::Apps => self.launcher.read(cx).desired_height(),
            NotchTool::Agentes => self.agents.read(cx).desired_height(),
            NotchTool::Sistema => self.system.read(cx).desired_height(),
            NotchTool::Media => self.media_panel.read(cx).desired_height(),
            NotchTool::Personalizar => self.customize.read(cx).desired_height(),
        };
        desired.min(self.panel_max_height().unwrap_or(f32::MAX))
    }

    /// Les dice a los paneles con lista hasta dónde pueden crecer: en un
    /// costado el panel es alto y angosto, y muestra todo lo que quepa en
    /// casi toda la altura (sin pasarse de lo que tiene: no queda espacio
    /// muerto abajo). El lanzador centrado sigue con su tope.
    fn sync_panel_limits(&mut self, cx: &mut Context<Self>) {
        let max = self.panel_max_height();
        let centered = self.launcher_centered;
        if self.panel.read(cx).max_height != max {
            self.panel.update(cx, |panel, _| panel.max_height = max);
        }
        if self.snippets.read(cx).max_height != max {
            self.snippets.update(cx, |panel, _| panel.max_height = max);
        }
        let launcher_max = max.filter(|_| !centered);
        if self.launcher.read(cx).max_height != launcher_max {
            self.launcher.update(cx, |panel, _| panel.max_height = launcher_max);
        }
        if self.agents.read(cx).max_height != max {
            self.agents.update(cx, |panel, _| panel.max_height = max);
        }
    }

    fn panel_pinned(&self, cx: &Context<Self>) -> bool {
        match self.notch_tool {
            NotchTool::Clipboard => self.panel.read(cx).pinned,
            NotchTool::Textos => self.snippets.read(cx).pinned,
            NotchTool::Apps => self.launcher.read(cx).pinned,
            NotchTool::Agentes => self.agents.read(cx).pinned,
            NotchTool::Sistema => self.system.read(cx).pinned,
            NotchTool::Media => self.media_panel.read(cx).pinned,
            NotchTool::Personalizar => false,
        }
    }

    fn panel_focus(&self, cx: &Context<Self>) -> gpui::FocusHandle {
        match self.notch_tool {
            NotchTool::Clipboard => self.panel.focus_handle(cx),
            NotchTool::Textos => self.snippets.focus_handle(cx),
            NotchTool::Apps => self.launcher.focus_handle(cx),
            NotchTool::Agentes => self.agents.focus_handle(cx),
            NotchTool::Sistema => self.system.focus_handle(cx),
            NotchTool::Media => self.media_panel.focus_handle(cx),
            NotchTool::Personalizar => self.customize.focus_handle(cx),
        }
    }

    /// La herramienta `index` de la tira o la rueda.
    fn run_tool(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.run_slot(StripId::Tool(index), window, cx)
    }

    /// Qué hace cada ficha de la tira. Las de segundo nivel son las mismas
    /// herramientas; solo cambian dónde están.
    fn run_slot(&mut self, slot: StripId, window: &mut Window, cx: &mut Context<Self>) {
        match slot {
            StripId::More => self.go_to_strip_page(Page::More),
            StripId::Back => self.go_to_strip_page(Page::Ring),
            StripId::Customize => self.open_customize(window, cx),
            StripId::Tool(CLIPBOARD_TOOL) => self.open_notch(NotchTool::Clipboard, window, cx),
            StripId::Tool(TEXTOS_TOOL) => self.open_notch(NotchTool::Textos, window, cx),
            StripId::Tool(AGENTES_TOOL) => self.open_notch(NotchTool::Agentes, window, cx),
            StripId::Tool(SISTEMA_TOOL) => self.open_notch(NotchTool::Sistema, window, cx),
            StripId::Tool(MEDIA_TOOL) => self.open_notch(NotchTool::Media, window, cx),
            StripId::Tool(capture::TOOL) => self.start_capture(false, window, cx),
            StripId::Tool(board::TOOL) => self.start_board(window, cx),
            StripId::Tool(flip::TOOL) => self.start_flip(window, cx),
            StripId::Tool(color::TOOL) => self.start_color(window, cx),
            StripId::Tool(REUNIONES_TOOL) => meetings::show(cx),
            StripId::Tool(_) => {}
        }
    }

    /// Un atajo global: abre su herramienta, o la cierra si ya está abierta.
    fn run_hotkey(&mut self, action: hotkeys::Action, window: &mut Window, cx: &mut Context<Self>) {
        use hotkeys::Action;
        // La mira, la pizarra, el color y el flip se cierran con Escape; otro
        // atajo encima los dejaría a medias.
        if self.capture.is_some() || self.board.is_some() || self.color.is_some() || self.flip.is_some() {
            return;
        }
        let notch = |pill: &mut Self, tool: NotchTool, window: &mut Window, cx: &mut Context<Self>| {
            if pill.panel_open && pill.notch_tool == tool {
                pill.close_panel(true, cx);
            } else {
                pill.open_notch(tool, window, cx);
            }
        };
        match action {
            Action::Launcher => self.toggle_launcher(window, cx),
            Action::Clipboard => notch(self, NotchTool::Clipboard, window, cx),
            Action::Snippets => notch(self, NotchTool::Textos, window, cx),
            Action::Capture => self.start_capture(true, window, cx),
            Action::Board => self.start_board(window, cx),
            Action::Color => self.start_color(window, cx),
            Action::Flip => self.start_flip(window, cx),
            Action::Agents => notch(self, NotchTool::Agentes, window, cx),
            Action::Record => self.studio.update(cx, |studio, cx| studio.toggle_recording(cx)),
            Action::Summon => self.summon_to_cursor(cx),
        }
    }

    /// Una entrada del menú de la bandeja.
    fn run_tray_command(&mut self, command: tray_icon::Command, window: &mut Window, cx: &mut Context<Self>) {
        use tray_icon::Command;
        match command {
            Command::OpenAtic => meetings::show(cx),
            Command::Settings => settings::open(cx),
            Command::Consoles => self.open_space(None, cx),
            Command::Capture => self.run_hotkey(hotkeys::Action::Capture, window, cx),
            Command::Summon => self.summon_to_cursor(cx),
            Command::Quit => cx.quit(),
        }
    }

    /// «Traer pill»: la gota queda bajo el cursor y ese es su lugar nuevo.
    /// Solo en el monitor donde ya está: llevar la ventana a otro es lo que
    /// hace el arrastre y aquí no se repite.
    fn summon_to_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(cursor) = self.cursor else {
            eprintln!("[atajos] traer pill: no se sabe dónde está el cursor");
            return;
        };
        println!("[atajos] traer pill → {cursor:?} (monitor {:?})", self.monitor);
        if !self.monitor.contains(cursor, 0.0) {
            eprintln!("[atajos] traer pill: el cursor está en otro monitor, todavía no se trae");
            return;
        }
        if self.panel_open {
            self.close_panel(false, cx);
        }
        let disc = Rect::centered(cursor, DISC_R * 2.0, DISC_R * 2.0).clamped_into(&self.monitor);
        self.home = Home::Floating { center: disc.center() };
        save_home(&self.home, &self.work, self.screen.as_ref().map(|(screen, _, _)| screen));
        cx.notify();
    }

    fn go_to_strip_page(&mut self, page: Page) {
        let now = Instant::now();
        if page != self.strip_page {
            self.page_switch = Some(PageSwitch {
                at: now,
                from: self.strip_slots(),
                from_length: self.strip_open_now(now),
                dir: if page == Page::More { 1.0 } else { -1.0 },
            });
        }
        self.strip_page = page;
        // Las fichas cambian de lugar: el hover y el pulso son de la posición,
        // no de la herramienta.
        for tween in self.strip_hover.iter_mut() {
            tween.set(0.0, now);
        }
        self.strip_pulse = None;
    }

    /// El editor de los niveles, como cara del notch.
    ///
    /// Solo acoplada: es donde vive la tira que se edita. La gota flotante
    /// no tiene tira.
    fn open_customize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.home, Home::Docked { .. }) {
            return;
        }
        self.open_notch(NotchTool::Personalizar, window, cx);
    }

    /// Deja la disposición como quedó y lo guarda en el acto: no hay
    /// «Guardar», la tira ya queda así.
    fn layout_changed(&mut self, layout: Layout, cx: &mut Context<Self>) {
        pill_tools::save(&layout);
        self.layout = layout;
        // Si ya no hay nada en «Más», el segundo paso no tiene a quién mostrar.
        if !pill_tools::more_open(&self.layout) && self.strip_page == Page::More {
            self.strip_page = Page::Ring;
        }
        self.customize.update(cx, |panel, cx| panel.reset(cx));
        cx.notify();
    }

    /// El vuelo al notch y de vuelta. Corre en cada sondeo: abrir solo marca
    /// `panel_open`, y aquí se decide si hay que ir arriba o volver.
    fn fly(&mut self, now: Instant, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(flight) = self.flight {
            if flight.progress(now) < 1.0 {
                return;
            }
            self.flight = None;
            if flight.back {
                if let Some(home) = self.away.take() {
                    self.home = home;
                }
            } else {
                self.home = self.notch_home();
                // Llegó: el foco va al panel que ahora sí está montado.
                if self.panel_open {
                    let focus = self.panel_focus(cx);
                    window.focus(&focus);
                }
            }
            if matches!(self.home, Home::Docked { .. }) {
                self.seat_started = Some(now);
            }
            cx.notify();
            return;
        }
        let wants_notch = self.panel_open && !self.centered_launcher();
        if wants_notch && !self.at_notch() {
            // Sale de donde esté (la gota, o un costado donde el panel no
            // cabe) hacia arriba.
            let from = self.mark_center(now);
            if self.away.is_none() {
                self.away = Some(self.home);
            }
            let Home::Docked { along, .. } = self.notch_home() else {
                return;
            };
            let to = Edge::Top.point(&self.work, along, TAB_THICK / 2.0);
            self.strip.set(0.0, now);
            self.flight = Some(Flight {
                from,
                to,
                started: now,
                back: false,
            });
            cx.notify();
        } else if !self.panel_visible() && self.at_notch() {
            let Some(home) = self.away else {
                return;
            };
            let to = match home {
                Home::Docked { edge, along } => edge.point(
                    &self.work,
                    geometry::clamp_along(edge, &self.work, along, TAB_LENGTH),
                    TAB_THICK / 2.0,
                ),
                Home::Floating { center } => center,
            };
            self.strip.set(0.0, now);
            self.flight = Some(Flight {
                from: self.mark_center(now),
                to,
                started: now,
                back: true,
            });
            cx.notify();
        }
    }

    fn agents_event(&mut self, event: &agents::AgentsEvent, cx: &mut Context<Self>) {
        match event {
            agents::AgentsEvent::Close => self.close_panel(true, cx),
            // La terminal ya tiene el foco: no se le devuelve a la app de antes.
            agents::AgentsEvent::Left => self.close_panel(false, cx),
            agents::AgentsEvent::Space => {
                self.close_panel(false, cx);
                self.open_space(None, cx);
            }
            agents::AgentsEvent::Open(open) => {
                self.close_panel(false, cx);
                self.open_space(Some(open.clone()), cx);
            }
        }
    }

    /// Muestra el espacio (lo abre si hace falta) y, si viene, abre una
    /// consola nueva en él.
    fn open_space(&mut self, open: Option<space::Open>, cx: &mut Context<Self>) {
        if let Some(handle) = self.space {
            let alive = handle
                .update(cx, |view, window, cx| {
                    window.activate_window();
                    if let Some(open) = open.clone() {
                        view.open(open, cx);
                    }
                })
                .is_ok();
            if alive {
                return;
            }
        }
        match space::open_window(open, cx) {
            Ok(handle) => self.space = Some(handle),
            Err(error) => eprintln!("espacio: no se pudo abrir la ventana: {error}"),
        }
    }

    /// Un texto ya armado (Textos): mismo pegado que el Clipboard.
    fn paste_text(&mut self, text: String, cx: &mut Context<Self>) {
        let target = self.paste_target;
        self.close_panel(false, cx);
        println!("pegar → texto de {} caracteres", text.chars().count());
        cx.spawn(async move |_, cx| {
            paste_into(target, ClipboardItem::new_string(text), cx).await
        })
        .detach();
    }

    fn close_panel(&mut self, restore_focus: bool, cx: &mut Context<Self>) {
        if !self.panel_open {
            return;
        }
        self.panel_open = false;
        // Sistema deja de releer el volumen y el brillo.
        self.system.update(cx, |panel, _| panel.active = false);
        self.media_panel.update(cx, |panel, _| panel.active = false);
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(false);
        }
        let target = self.paste_target.take();
        if restore_focus {
            if let Some(target) = target {
                paste::force_foreground(target);
            }
        }
        cx.notify();
    }

    fn paste(&mut self, entry: clipboard::Entry, cx: &mut Context<Self>) {
        let target = self.paste_target;
        self.close_panel(false, cx);
        let Some(item) = clipboard_item(&entry) else {
            return;
        };
        println!("pegar → entrada {}", entry.id);
        cx.spawn(async move |_, cx| paste_into(target, item, cx).await)
            .detach();
    }

    /// Texto como texto; imágenes como archivo PNG, igual que Atic.
    fn start_drag(&mut self, entry: clipboard::Entry, cx: &mut Context<Self>) {
        enum Payload {
            Text(String),
            Files(Vec<String>),
        }
        let payload = match &entry.content {
            Content::Text(text) => Payload::Text(text.to_string()),
            Content::Color(label, _) => Payload::Text(label.to_string()),
            // El PNG de Atic ya está en disco: se arrastra tal cual.
            Content::Image(Picture::File(path)) => {
                Payload::Files(vec![path.to_string_lossy().into_owned()])
            }
            Content::Image(Picture::Embedded(image)) => {
                match write_drag_image(entry.id, image.bytes()) {
                    Ok(path) => Payload::Files(vec![path]),
                    Err(error) => {
                        eprintln!("no se pudo preparar la imagen para arrastrar: {error}");
                        return;
                    }
                }
            }
        };
        // `DoDragDrop` corre un loop modal que sigue despachando mensajes a
        // GPUI. Desde el manejador del evento reentraría en la app mientras
        // está prestada; en una tarea aparte corre cuando ese préstamo terminó.
        cx.spawn(async move |_, cx| {
            let (outcome, text) = match payload {
                Payload::Text(text) => (drag::drag_text(&text), Some(text)),
                Payload::Files(paths) => (drag::drag_files(&paths), None),
            };
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    eprintln!("arrastre → entrada {}: {error}", entry.id);
                    return;
                }
            };
            // Se mira apenas se suelta, con el cursor todavía sobre el destino.
            let under_cursor = paste::target_under_cursor();
            let web_terminal = under_cursor.is_some_and(paste::needs_ctrl_shift_v);
            let fallback =
                paste::drop_needs_paste_fallback(outcome.dropped, outcome.effect, web_terminal);
            println!(
                "arrastre → entrada {}: soltado={} efecto={} respaldo={}",
                entry.id,
                outcome.dropped,
                outcome.effect,
                fallback && text.is_some()
            );
            if let (true, Some(text), Some(target)) = (fallback, text, under_cursor) {
                paste_into(Some(target), ClipboardItem::new_string(text), cx).await;
            }
        })
        .detach();
    }

    // --- Arrastrar la pill --------------------------------------------------

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // La mira, la pizarra y el flip atienden sus propios clics.
        if self.capture.is_some() || self.board.is_some() || self.flip.is_some() || self.color.is_some() {
            return;
        }
        // La gota en vuelo no se agarra.
        if self.flight.is_some() {
            return;
        }
        let now = Instant::now();
        // GPUI arma la posición con el último movimiento que vio, y mientras la
        // ventana dejaba pasar los clics no vio ninguno; el cursor global sí
        // está al día.
        let polled = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(self.scale_factor));
        let position = polled.unwrap_or((f32::from(event.position.x), f32::from(event.position.y)));

        debug(|| {
            format!(
                "down en {position:?} (evento {:?}) home={:?} sobre_pill={}",
                event.position,
                self.home,
                self.over_pill(position, now, 0.0)
            )
        });
        if self.panel_open && self.over_panel(position, now, 0.0) {
            // Los elementos del panel atienden sus propios clics.
            return;
        }
        if self.tray_mouse_down(position, now) {
            cx.notify();
            return;
        }
        if let Some(hit) = self.peek_hit(position, now) {
            self.peek_press = Some((hit, position));
            cx.notify();
            return;
        }
        if self.panel_open
            && matches!(self.home, Home::Docked { .. })
            && self.over_pill(position, now, 0.0)
        {
            // Con el notch abierto la franja no se arrastra; la marca lo cierra.
            if self.over_mark(position, now) {
                self.close_panel(true, cx);
            }
            cx.notify();
            return;
        }
        if self.over_pill(position, now, 0.0) {
            let target = if self.tray_chip_at(position, now).is_some() {
                PressTarget::Tray
            } else if self.over_art(position, now) {
                PressTarget::Art
            } else if self.over_mark(position, now) {
                PressTarget::Mark
            } else if let Some(slot) = self.strip_slot_at(position, now).map(|(_, slot)| slot) {
                PressTarget::Tool(slot)
            } else {
                PressTarget::Body
            };
            let anchor = match self.home {
                Home::Docked { along, .. } => (along, 0.0),
                Home::Floating { center } => center,
            };
            // La presión larga sobre una ficha abre el editor: el reloj corre
            // mientras el botón siga abajo y sin moverse.
            self.hold = match (target, self.home) {
                (PressTarget::Tool(StripId::Tool(_)), Home::Docked { .. }) => {
                    Some((target, now))
                }
                // Sobre la cara, la presión larga la vuelve engranaje (Ajustes).
                (PressTarget::Mark, _) => Some((target, now)),
                _ => None,
            };
            self.press = Some(Press {
                cursor: position,
                anchor,
                target,
                dragging: false,
            });
        } else if self.wheel_fully_open() {
            match self.wheel_target_at(position, now) {
                Some(Some(index)) => {
                    let slot = self.wheel_slots()[index];
                    println!("rueda → {}", slot.label());
                    match slot {
                        StripId::More => self.go_to_wheel_page(Page::More, now),
                        StripId::Back => self.go_to_wheel_page(Page::Ring, now),
                        _ => {
                            self.close_wheel();
                            self.run_slot(slot, window, cx);
                        }
                    }
                }
                Some(None) => self.close_wheel(),
                None => {}
            }
        }
        cx.notify();
    }

    /// El botón se soltó: si no hubo arrastre fue un clic; si lo hubo, la pill
    /// se acopla o queda flotando.
    fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tray_release(window, cx) {
            cx.notify();
            return;
        }
        if let Some((hit, _)) = self.peek_press.take() {
            match hit {
                PeekHit::Row(row) => {
                    if let Some(entry) = self.peek_entries.get(row).cloned() {
                        self.paste_from_peek(entry, cx);
                    }
                }
                PeekHit::Footer => self.open_panel(window, cx),
            }
            cx.notify();
            return;
        }
        let now = Instant::now();
        let Some(press) = self.press.take() else {
            return;
        };
        // Si la presión larga ya abrió el editor, `press` quedó en `None` y no
        // se llega acá: lo que se suelta antes del plazo es un clic normal.
        // La cara ya hecha engranaje abre los Ajustes al soltarla.
        let gear_ready = matches!(
            self.hold,
            Some((PressTarget::Mark, since)) if now.duration_since(since).as_secs_f32() * 1000.0 >= STRIP_HOLD_MS
        );
        self.hold = None;
        debug(|| {
            format!(
                "release arrastrando={} home={:?}",
                press.dragging, self.home
            )
        });
        if press.dragging {
            self.settle(now);
        } else {
            match press.target {
                PressTarget::Mark if gear_ready => {
                    self.close_wheel();
                    debug(|| "presión larga en la cara → Ajustes".into());
                    cx.defer(|cx| settings::open(cx));
                }
                // En Atic el clic en la gota no hace nada; la rueda se abre al
                // pasar el cursor. En el tab el clic en la marca la abre.
                PressTarget::Mark if matches!(self.home, Home::Docked { .. }) => {
                    self.toggle_wheel(WheelOpener::Click)
                }
                // En pausa, la carátula atenuada lleva un ▶: la reanuda.
                PressTarget::Art if !self.music_playing => self.media.control(media::Control::Toggle),
                PressTarget::Art => self.open_notch(NotchTool::Media, window, cx),
                PressTarget::Tray => self.open_notch(NotchTool::Agentes, window, cx),
                PressTarget::Tool(slot) => {
                    let index = self.strip_slots().iter().position(|&id| id == slot).unwrap_or(0);
                    println!("tira → {}", slot.label());
                    self.strip_pulse = Some((index, now));
                    self.run_slot(slot, window, cx);
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// Mueve la pill con el cursor. Devuelve si hay arrastre en curso.
    fn follow_drag(&mut self, now: Instant) -> bool {
        let Some(cursor) = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(self.scale_factor))
        else {
            return false;
        };
        let Some(press) = self.press.as_mut() else {
            return false;
        };
        if !press.dragging {
            if distance(cursor, press.cursor) < DRAG_THRESHOLD {
                return false;
            }
            press.dragging = true;
            // Se movió: el gesto es arrastre de la pill, no presión larga.
            self.hold = None;
            self.wheel_target_open = false;
            self.wheel_time = (self.wheel_time - self.wheel_bloom).max(0.0);
            self.wheel_bloom = 0.0;
            self.strip.set(0.0, now);
        }

        match self.home {
            Home::Docked { edge, .. } => {
                if edge.depth_of(&self.work, cursor) > geometry::UNDOCK_DISTANCE {
                    // Se suelta del borde: pasa a ser gota bajo el cursor.
                    self.home = Home::Floating { center: cursor };
                    press.cursor = cursor;
                    press.anchor = cursor;
                } else {
                    let along =
                        press.anchor.0 + edge.along_of(cursor) - edge.along_of(press.cursor);
                    self.home = Home::Docked {
                        edge,
                        along: geometry::clamp_along(edge, &self.work, along, TAB_LENGTH),
                    };
                }
            }
            Home::Floating { .. } => {
                let center = (
                    press.anchor.0 + cursor.0 - press.cursor.0,
                    press.anchor.1 + cursor.1 - press.cursor.1,
                );
                let disc =
                    Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0).clamped_into(&self.monitor);
                self.home = Home::Floating {
                    center: disc.center(),
                };
            }
        }
        true
    }

    /// Al soltar: cerca de un borde acoplable se pega con el aplastón; si no,
    /// queda flotando donde cayó.
    fn settle(&mut self, now: Instant) {
        if let Home::Floating { center } = self.home {
            let disc = Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0);
            debug(|| {
                format!(
                    "settle disco={disc:?} trabajo={:?} acoplables={:?}",
                    self.work, self.dockable
                )
            });
            if let Some(edge) = geometry::dock_candidate(&self.work, &disc, &self.dockable) {
                self.home = Home::Docked {
                    edge,
                    along: geometry::dock_along(edge, &self.work, center, TAB_LENGTH),
                };
                self.seat_started = Some(now);
            }
        }
        save_home(&self.home, &self.work, self.screen.as_ref().map(|(screen, _, _)| screen));
    }

    fn dragging(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.dragging)
    }

    /// Sigue al monitor: si se arrastra la pill a otro, la ventana se va con
    /// ella; y si el monitor cambia de resolución, escala o barra de tareas,
    /// rehace la geometría. GPUI ya escala el dibujo con el monitor (100 %,
    /// 125 %…), así que la pill se ve del mismo tamaño que el resto de
    /// Windows en cada pantalla.
    fn refresh_screen(&mut self, now: Instant, window: &Window) {
        let dragging = self.dragging();
        if !dragging && self.span.is_none() && now.duration_since(self.screen_checked) < SCREEN_POLL {
            return;
        }
        self.screen_checked = now;
        let Some(overlay) = self.overlay.as_ref() else {
            return;
        };
        // Estirada para la mira, la ventana no sigue al monitor; y volviendo,
        // hasta que llegue.
        match self.span {
            Some(capture::Span::Covering(_)) => return,
            Some(capture::Span::Restoring(home)) if !overlay.covers(&home) => return,
            Some(capture::Span::Restoring(_)) => self.span = None,
            None => {}
        }
        let Some(current) = overlay.screen() else {
            return;
        };
        if self.moving_to == Some(current) {
            self.moving_to = None;
        }
        if dragging {
            let target = overlay
                .screen_under_cursor()
                .filter(|s| *s != current && self.moving_to != Some(*s));
            if let Some(target) = target {
                overlay.move_to(&target);
                self.moving_to = Some(target);
            }
        }
        let scale_factor = window.scale_factor();
        let key = (current, scale_factor, overlay.origin());
        if self.screen == Some(key) {
            return;
        }
        let Some((monitor, work, dockable)) = screen_geometry(overlay, scale_factor) else {
            return;
        };
        // Otra resolución en el mismo monitor: la ventana vuelve a cubrirlo.
        if !dragging && self.moving_to.is_none() {
            overlay.move_to(&current);
        }
        debug(|| format!("pantalla {current:?} escala={scale_factor} trabajo={work:?}"));
        let cursor = overlay.cursor(scale_factor);
        let old_work = self.work;
        self.screen = Some(key);
        self.scale_factor = scale_factor;
        self.monitor = monitor;
        self.work = work;
        self.dockable = dockable;

        if let (Some(press), Some(cursor)) = (self.press.as_mut().filter(|p| p.dragging), cursor) {
            // Llegó a otro monitor arrastrándose: sigue como gota bajo el
            // cursor, ya en las coordenadas de la ventana nueva.
            self.home = Home::Floating { center: cursor };
            press.cursor = cursor;
            press.anchor = cursor;
        } else if std::mem::take(&mut self.restore_home) {
            self.home = load_home(&self.work, &self.dockable)
                .unwrap_or_else(|| default_home(&self.work));
        } else {
            self.home = parse_home(&home_line(&self.home, &old_work), &self.work, &self.dockable)
                .unwrap_or_else(|| default_home(&self.work));
        }
    }

    // --- Sondeo -------------------------------------------------------------

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.ticks += 1;
        self.refresh_screen(now, window);
        // Cada ~medio segundo: que ninguna ventana normal haya quedado encima.
        if self.ticks.is_multiple_of(30) && self.overlay.as_ref().is_some_and(|overlay| overlay.keep_topmost()) {
            debug(|| "la ventana había salido de «siempre visible»: vuelve arriba".into());
            // El vidrio quedó abajo con ella: escondido, el próximo cuadro lo
            // pone de nuevo justo debajo de la pill.
            if let Some(glass) = self.glass.as_mut() {
                glass.hide();
            }
            cx.notify();
        }
        let cursor = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.cursor(self.scale_factor));
        let moved = cursor != self.cursor;
        self.cursor = cursor;

        // Los atajos globales (llegan desde su propio hilo).
        while let Ok(action) = self.hotkeys.try_recv() {
            self.run_hotkey(action, window, cx);
        }
        while let Ok(phone::Command::StopRecording) = self.phone.try_recv() {
            self.studio.update(cx, |studio, cx| {
                if studio.recording() {
                    studio.toggle_recording(cx);
                }
            });
        }
        while let Ok(command) = self.tray_icon.try_recv() {
            self.run_tray_command(command, window, cx);
        }

        // Con la mira, la pizarra o el flip abiertos la ventana recibe todo;
        // lo demás espera.
        if self.capture.is_some() || self.board.is_some() || self.flip.is_some() || self.color.is_some() {
            self.calm = false;
            if let Some(overlay) = self.overlay.as_mut() {
                overlay.set_passthrough(false);
            }
            return;
        }

        // La presión larga sobre una ficha abre el editor de los niveles. El
        // plazo corre en cada cuadro: si la pill se movió, `hold` ya está en
        // `None` y esto no hace nada.
        if let Some((PressTarget::Tool(slot), since)) = self.hold {
            let elapsed = now.duration_since(since).as_secs_f32() * 1000.0;
            let still = self.press.as_ref().is_some_and(|press| !press.dragging);
            if elapsed >= STRIP_HOLD_MS && still {
                self.hold = None;
                self.press = None;
                debug(|| format!("presión larga → {}", slot.label()));
                self.run_slot(StripId::Customize, window, cx);
            }
        }
        // La presión larga sobre la cara: se vuelve engranaje mientras dura
        // (completo al plazo). Suelta o arrastrada, vuelve a ser cara.
        match self.hold {
            Some((PressTarget::Mark, since)) if self.press.as_ref().is_some_and(|press| !press.dragging) => {
                let elapsed = now.duration_since(since).as_secs_f32() * 1000.0;
                self.gear.snap(segment(elapsed, GEAR_DELAY_MS, STRIP_HOLD_MS - GEAR_DELAY_MS));
                cx.notify();
            }
            _ => {
                self.gear.set(0.0, now);
                if self.gear.is_running(now) {
                    cx.notify();
                }
            }
        }

        self.fly(now, window, cx);

        // Respaldo por si GPUI no ve el `mouse up` (llegó a otra ventana).
        if (self.press.is_some() || self.peek_press.is_some() || self.tray.press.is_some())
            && !win::left_button_down()
        {
            self.release(window, cx);
        }
        if let (Some((PeekHit::Row(row), origin)), Some(c)) = (self.peek_press, cursor) {
            if distance(c, origin) >= PEEK_DRAG {
                self.peek_press = None;
                if let Some(entry) = self.peek_entries.get(row).cloned() {
                    self.peek.set(0.0, now);
                    self.start_drag(entry, cx);
                }
            }
        }

        let shortcut_down = self.dict_watch.wheel_down();
        if shortcut_down && !self.shortcut_was_down {
            self.toggle_wheel(WheelOpener::Shortcut);
        }
        self.shortcut_was_down = shortcut_down;

        let dragging = self.dragging();
        let docked = matches!(self.home, Home::Docked { .. });
        let over_pill = cursor.is_some_and(|c| self.over_pill(c, now, 0.0));

        // Tab acoplado: la tira se abre al pasar el cursor. Con algo sonando,
        // solo desde la marca: la carátula y la onda abren Ahora suena, y si
        // la tira se abriera encima no se podrían tocar. Ya abierta, se
        // queda mientras el cursor siga en la pill.
        let notch_busy = docked && self.panel_visible();
        let live_on = self.live.target() == 1.0;
        // Los contadores y las filas del vistazo de la bandeja son atajos: no
        // abren la tira (que ensancharía el notch bajo el botón que se pulsa).
        let over_tray = cursor.is_some_and(|c| {
            self.tray_chip_at(c, now).is_some()
                || self.tray_over(c, now)
                || self.over_hang(c, now)
                || self.over_dict(c, now)
        });
        // Los atajos solo impiden *abrir* la tira: ya abierta, se queda. Si
        // no, bajar al aviso de la bandeja (bajo el uso de los agentes) la
        // cerraba, y con ella el vistazo de uso.
        let may_open = self.strip.target() == 1.0
            || ((!live_on || cursor.is_some_and(|c| self.over_mark(c, now))) && !over_tray);
        if docked && over_pill && !dragging && !notch_busy && may_open {
            self.strip.set(1.0, now);
            self.strip_leave_at = None;
        } else if self.strip.target() == 1.0 {
            let since = *self.strip_leave_at.get_or_insert(now);
            if !docked
                || dragging
                || notch_busy
                || now.duration_since(since) >= STRIP_LEAVE_GRACE
            {
                self.strip.set(0.0, now);
                self.strip_leave_at = None;
            }
        }

        // Gota flotante: la rueda se abre tras 180 ms con el cursor encima.
        // No mientras vuela ni con el notch abierto.
        let quiet = self.flight.is_none() && !self.panel_visible();
        if !docked && over_pill && !dragging && self.press.is_none() && quiet {
            let since = *self.hover_since.get_or_insert(now);
            if !self.wheel_target_open && now.duration_since(since) >= WHEEL_HOVER_DELAY {
                self.open_wheel(WheelOpener::Hover);
            }
        } else {
            self.hover_since = None;
        }

        let strip_slots = self.strip_slots();
        let strip_hovered = cursor
            .and_then(|c| self.strip_slot_at(c, now))
            .map(|(index, _)| index);
        let strip_tool = strip_hovered.and_then(|index| strip_slots[index].tool());

        // El vistazo: tras `PEEK_DELAY` sobre Clipboard; se queda mientras el
        // cursor esté en él o en la herramienta, y otra herramienta lo cierra.
        if self.peek.target() == 1.0 || strip_tool == Some(CLIPBOARD_TOOL) {
            self.peek_entries = self.panel.read(cx).recent(PEEK_COUNT);
        }
        let on_tool = strip_tool == Some(CLIPBOARD_TOOL) && !self.panel_visible();
        let on_peek = cursor.is_some_and(|c| {
            self.peek_rect(now)
                .is_some_and(|rect| rect.contains(c, 4.0))
        });
        if on_tool {
            let since = *self.peek_hover_since.get_or_insert(now);
            // Con otro vistazo abierto, el relevo es rápido (`peeks.rs`).
            let wait = if self.other_peek_open(peeks::PeekKind::Clipboard) {
                peeks::SWITCH_DELAY
            } else {
                PEEK_DELAY
            };
            if now.duration_since(since) >= wait && self.peek.target() != 1.0 {
                self.peek_shown_at = now;
                self.peek.set(1.0, now);
                self.hand_off_peeks(peeks::PeekKind::Clipboard, now);
            }
        } else {
            self.peek_hover_since = None;
        }
        if self.peek.target() == 1.0 {
            // Sobre otra herramienta con vistazo se espera el relevo; sobre
            // una sin vistazo, el margen de siempre.
            let relay_to = strip_tool.is_some_and(|tool| tool != CLIPBOARD_TOOL && peeks::any_peek(tool));
            if self.panel_visible() || !docked {
                self.peek.set(0.0, now);
            } else if on_tool || on_peek || relay_to || self.peek_press.is_some() {
                self.peek_leave_at = None;
            } else {
                let since = *self.peek_leave_at.get_or_insert(now);
                if now.duration_since(since) >= PEEK_GRACE {
                    self.peek.set(0.0, now);
                    self.peek_leave_at = None;
                }
            }
        }
        self.update_usage_peek(now, cursor, strip_tool, docked);
        self.update_tool_peek(now, cursor, strip_tool, docked, cx);
        self.update_block(now);
        if self.demo_peek_until.is_some_and(|until| now < until) && docked {
            self.peek_entries = self.panel.read(cx).recent(PEEK_COUNT);
            self.strip.set(1.0, now);
            self.peek.set(1.0, now);
        }
        self.peek_hovered = cursor.and_then(|c| self.peek_hit(c, now));
        self.tray_tick(now, cursor, cx);
        let wheel_hovered = if self.wheel_fully_open() {
            cursor.and_then(|c| self.wheel_target_at(c, now)).flatten()
        } else {
            None
        };
        for index in 0..pill_tools::STRIP_MAX {
            let strip_on = if strip_hovered == Some(index) {
                1.0
            } else {
                0.0
            };
            let wheel_on = if wheel_hovered == Some(index) {
                1.0
            } else {
                0.0
            };
            self.strip_hover[index].set(strip_on, now);
            self.wheel_hover[index].set(wheel_on, now);
        }

        let grace = match self.wheel_opener {
            WheelOpener::Click => Some(WHEEL_CLICK_GRACE),
            WheelOpener::Hover => Some(WHEEL_HOVER_GRACE),
            WheelOpener::Shortcut => None,
        };
        if let (true, Some(grace)) = (self.wheel_target_open, grace) {
            let near = cursor.is_some_and(|c| {
                self.wheel_target_at(c, now).is_some() || self.over_pill(c, now, 0.0)
            });
            if near {
                self.wheel_leave_at = None;
            } else {
                let since = *self.wheel_leave_at.get_or_insert(now);
                if now.duration_since(since) >= grace {
                    self.close_wheel();
                }
            }
        }

        let over_shelf = self.shelf_tick(cursor, cx);
        let interactive = self.press.is_some()
            || over_shelf
            || cursor.is_some_and(|c| {
                self.over_pill(c, now, 6.0)
                    || (self.wheel_target_open && self.wheel_target_at(c, now).is_some())
                    || (self.panel_open && self.over_panel(c, now, 6.0))
            });
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_passthrough(!interactive);
        }

        // Lo que suena: el tab se alarga con la carátula y la onda. Solo
        // acoplado arriba, con el notch cerrado.
        let track = self.media.track();
        let playing = track.as_ref().is_some_and(|t| t.playing);
        if playing || track.is_none() {
            self.paused_since = None;
        } else if self.paused_since.is_none() {
            self.paused_since = Some(now);
        }
        let paused = self.paused_since.is_some_and(|at| now.duration_since(at) < PAUSED_ART_FOR);
        self.music_playing = playing;
        // En cualquier borde y flotando; no con el notch abierto ni volando.
        let shown = !self.panel_visible() && self.flight.is_none() && !self.dragging();
        self.live.set(if (playing || paused) && shown { 1.0 } else { 0.0 }, now);
        self.fit.set(self.fit_length(), now);
        self.update_dictation(now, cx);
        self.update_hang(now);
        self.level += (self.media.level() - self.level) * 0.35;
        // Dictando, la onda del micrófono también pide ~30 cuadros/s.
        let live_on = self.live.value(now) > 0.01 || self.dict.value(now) > 0.01;

        // Fuera de las animaciones basta con ~20 cuadros/s para la respiración;
        // con la onda, ~30.
        // En calma el sondeo ya va lento: un cuadro de cada dos (~10/s) basta
        // para la respiración, que cambia menos de un tono por cuadro.
        let breath_every = if self.calm { 2 } else { 3 };
        if moved || self.ticks.is_multiple_of(breath_every) || (live_on && self.ticks.is_multiple_of(2)) {
            cx.notify();
        }
        self.calm = !live_on
            && !dragging
            && self.press.is_none()
            && self.peek_press.is_none()
            && self.tray.press.is_none()
            && self.hold.is_none()
            && self.flight.is_none()
            && self.span.is_none()
            && self.shelves.is_empty()
            && !self.panel_visible()
            && !self.wheel_target_open
            && self.wheel_time == 0.0
            && self.peek.target() == 0.0
            && self.tool_peek_amount(now) == 0.0
            && !cursor.is_some_and(|c| self.over_pill(c, now, CALM_DISTANCE));
    }

    /// Cada cuánto sondear: rápido con el cursor cerca o algo en marcha; si
    /// no, lento, para no gastar CPU ni batería en reposo. Lento sigue
    /// alcanzando a ver un atajo apretado (dura más de 50 ms).
    fn tick_every(&self) -> Duration {
        if self.calm {
            TICK_CALM
        } else {
            TICK_ACTIVE
        }
    }

    /// Avanza el estado dependiente del tiempo. Devuelve si hay una animación
    /// en curso que necesite el siguiente cuadro.
    fn advance(&mut self, now: Instant) -> bool {
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let dt_ms = dt * 1000.0;

        let dragging = self.follow_drag(now);

        let wheel_end = self.wheel_bloom + wheel_open_ms(self.wheel_slots().len());
        let wheel_moving = if self.wheel_target_open {
            self.wheel_time = (self.wheel_time + dt_ms).min(wheel_end);
            self.wheel_time < wheel_end
        } else {
            self.wheel_time = (self.wheel_time - dt_ms * CLOSE_SPEED).max(0.0);
            self.wheel_time > 0.0
        };

        // Mientras la gota vuela el notch espera: se estira recién arriba.
        let panel_moving = if self.flight.is_some() && self.panel_open {
            self.panel_time = 0.0;
            false
        } else if self.panel_open {
            self.panel_time = (self.panel_time + dt_ms).min(NOTCH_OPEN_MS);
            self.panel_time < NOTCH_OPEN_MS
        } else {
            self.panel_time = (self.panel_time - dt_ms * NOTCH_CLOSE_SPEED).max(0.0);
            self.panel_time > 0.0
        };

        if self
            .seat_started
            .is_some_and(|at| now.duration_since(at).as_secs_f32() * 1000.0 > SEAT_MS)
        {
            self.seat_started = None;
        }

        let target = match self.cursor {
            Some((x, y)) => {
                let (mx, my) = self.mark_center(now);
                let (dx, dy) = (x - mx, y - my);
                let distance = dx.hypot(dy).max(1.0);
                let reach = (distance / 240.0).min(1.0);
                (dx / distance * reach, dy / distance * reach)
            }
            None => (0.0, 0.0),
        };
        let follow = 1.0 - (-dt * 14.0).exp();
        self.eyes.0 += (target.0 - self.eyes.0) * follow;
        self.eyes.1 += (target.1 - self.eyes.1) * follow;
        let eyes_moving =
            (target.0 - self.eyes.0).abs() > 0.01 || (target.1 - self.eyes.1).abs() > 0.01;

        if self.blink_started.is_none() && now >= self.next_blink {
            self.blink_started = Some(now);
            self.double_blink = self.rng.next() < 0.22;
            self.schedule_blink(now);
        }
        if let Some(started) = self.blink_started {
            let elapsed = now.duration_since(started).as_secs_f32() * 1000.0;
            let total = if self.double_blink {
                BLINK_MS + 160.0
            } else {
                BLINK_MS
            };
            if elapsed >= total {
                self.blink_started = None;
            }
        }
        if self
            .strip_pulse
            .is_some_and(|(_, at)| now.duration_since(at) > Duration::from_millis(260))
        {
            self.strip_pulse = None;
        }
        if self.page_switch.as_ref().is_some_and(|switch| switch.progress(now) >= 1.0) {
            self.page_switch = None;
        }

        dragging
            || self.page_switch.is_some()
            || self.live.is_running(now)
            || self.hang.is_running(now)
            || self.dict.is_running(now)
            || self.usage_peek.is_running(now)
            || self.usage_animating(now)
            || self.tool_peek_animating(now)
            || self.block_along.is_running(now)
            // La entrada del vistazo del portapapeles (letras y filas).
            || (self.peek.target() == 1.0
                && now.duration_since(self.peek_shown_at) < Duration::from_millis(1200))
            || self.flight.is_some()
            || self.press.is_some()
            || wheel_moving
            || panel_moving
            || eyes_moving
            || self.seat_started.is_some()
            || self.blink_started.is_some()
            || self.strip_pulse.is_some()
            || self.strip.is_running(now)
            || self.notch_h.is_running(now)
            || self.preview.is_running(now)
            || self.peek.is_running(now)
            || self.tray_animating(now)
            || self.shelf_animating(now)
            || self.strip_hover.iter().any(|t| t.is_running(now))
            || self.wheel_hover.iter().any(|t| t.is_running(now))
    }

    fn blink_amount(&self, now: Instant) -> f32 {
        let Some(started) = self.blink_started else {
            return 1.0;
        };
        let elapsed = now.duration_since(started).as_secs_f32() * 1000.0;
        let closedness = |t: f32| {
            let p = (t / BLINK_MS).clamp(0.0, 1.0);
            (p * PI).sin()
        };
        let mut amount = closedness(elapsed);
        if self.double_blink {
            amount = amount.max(closedness(elapsed - 160.0));
        }
        1.0 - amount * 0.9
    }

    // --- Cuadro -------------------------------------------------------------

    /// Cuánto contenido muestra el tab (0..1): lo más abierto entre el panel
    /// del notch y las franjas que bajan. En reposo es vidrio.
    fn content_amount(&self, pill: PillShape, now: Instant) -> f32 {
        let PillShape::Tab { .. } = pill else {
            return 0.0;
        };
        // La letra y los contadores del tab se tapan como un panel solo si así
        // se eligió en Apariencia.
        let cover_hang = appearance::current().cover_hang;
        [
            self.notch_morph(),
            self.peek.value(now),
            if cover_hang { self.tray_stretch(now).0 } else { 0.0 },
            if cover_hang { self.hang_amount(now) } else { 0.0 },
            self.dict_amount(now),
            self.usage_amount(now),
            self.tool_peek_amount(now),
        ]
        .into_iter()
        .fold(0.0, f32::max)
        .clamp(0.0, 1.0)
    }

    fn frame(&self, now: Instant) -> Frame {
        let look = appearance::current();
        let palette = Palette::dark();
        let breath_t = now.duration_since(self.born).as_secs_f32() / 2.4 * TAU;
        let brightness = 1.04 - 0.04 * breath_t.cos();
        let mut skin = palette.skin;
        skin.l = (skin.l * brightness).min(1.0);

        let pill = self.shape(now);
        let open = self.strip.value(now);
        let reveal = open.clamp(0.0, 1.0);

        // Cambio de paso: cuánto salió cada ficha vieja (0..1) y cuánto entró
        // cada nueva, con el lado hacia el que se corren.
        let switch = self.page_switch.as_ref().map(|switch| (switch.progress(now), switch.dir));
        let page_in = |index: usize, count: usize| -> f32 {
            let Some((p, _)) = switch else {
                return 1.0;
            };
            let room = (1.0 - PAGE_IN_START - PAGE_IN_SPAN) / count.saturating_sub(1).max(1) as f32;
            let start = PAGE_IN_START + index as f32 * PAGE_IN_STAGGER.min(room);
            ease_page(segment(p, start, PAGE_IN_SPAN))
        };
        let slide = switch.map_or(0.0, |(_, dir)| dir * PAGE_SLIDE);

        let mut strip_tools: Vec<IconDraw> = match pill {
            PillShape::Tab { edge, thick, .. } if reveal > 0.02 => {
                let slots = self.strip_slots();
                let count = slots.len();
                slots
                    .into_iter()
                    .enumerate()
                    .map(|(index, slot)| {
                        let entered = page_in(index, count);
                        let mut hover = self.strip_hover[index].value(now);
                        // Con el vistazo abierto su herramienta queda encendida.
                        if slot.tool() == Some(CLIPBOARD_TOOL) {
                            hover = hover.max(self.peek.value(now).clamp(0.0, 1.0));
                        }
                        let pulse = match self.strip_pulse {
                            Some((pulsed, at)) if pulsed == index => {
                                let t = now.duration_since(at).as_secs_f32() / 0.26;
                                1.0 - 0.18 * (t.clamp(0.0, 1.0) * PI).sin()
                            }
                            _ => 1.0,
                        };
                        let scale = (0.4 + 0.6 * open.max(0.0))
                            * (1.0 + 0.14 * hover)
                            * pulse
                            * (0.8 + 0.2 * entered);
                        IconDraw {
                            path: slot.icon(),
                            center: edge.point(
                                &self.work,
                                self.strip_tool_along(index, now) + slide * (1.0 - entered),
                                thick.min(TAB_THICK) / 2.0,
                            ),
                            size: TOOL_ICON * scale,
                            // El rebote es solo del tamaño: el color no se pasa.
                            color: mix(palette.muted, palette.text, hover.clamp(0.0, 1.0))
                                .opacity(reveal * entered),
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        // Las del paso que se deja salen hacia atrás, en sus mismos lugares.
        if let (Some(switch), PillShape::Tab { edge, thick, .. }) = (&self.page_switch, pill) {
            let gone = ease_page(segment(switch.progress(now), 0.0, PAGE_OUT_END));
            if gone < 1.0 && reveal > 0.02 {
                for (index, slot) in switch.from.iter().enumerate() {
                    strip_tools.push(IconDraw {
                        path: slot.icon(),
                        center: edge.point(
                            &self.work,
                            self.strip_tool_along(index, now) - slide * gone,
                            thick.min(TAB_THICK) / 2.0,
                        ),
                        size: TOOL_ICON * (0.4 + 0.6 * open.max(0.0)) * (1.0 - 0.2 * gone),
                        color: palette.muted.opacity(reveal * (1.0 - gone)),
                    });
                }
            }
        }

        let place = self.wheel_place(now);
        let (core, blobs, wheel_icons, wheel_shadow) = self.wheel_frame(&place, &palette, now);

        // Muro líquido: al arrastrar la gota cerca de un borde acoplable, un
        // cuello la une con el borde, como aviso de que ahí se pega.
        let mut walls = Vec::new();
        if let (PillShape::Disc { center }, true) = (pill, self.dragging()) {
            for &edge in &self.dockable {
                let gap = edge.depth_of(&self.work, center) - DISC_R;
                if gap < WALL_REACH {
                    let along = edge.along_of(center);
                    walls.push((edge.point(&self.work, along, -WALL_R), WALL_R));
                }
            }
        }

        Frame {
            skin,
            pill,
            mark: self.mark_center(now),
            eyes: self.eyes,
            blink: self.blink_amount(now),
            gear: self.gear.value(now),
            strip_tools,
            core,
            core_anchor: place.anchor,
            blobs,
            wheel_icons,
            wheel_shadow,
            panel: self.panel_shape(now),
            preview: self.preview_shape(now),
            walls,
            outline: match pill {
                PillShape::Tab {
                    edge,
                    rect,
                    block: Some(block),
                    ..
                } => Some(geometry::side_outline(edge, &self.work, &rect, &block, TAB_RADIUS, SIDE_JOIN_R)),
                _ => None,
            },
            glass: self.glass.is_some() && glass::backdrop_available() && look.glass,
            tint: look.rest_tint + (look.read_tint - look.rest_tint) * self.content_amount(pill, now),
            live: self.live_shown(now),
            art: self.media.track().and_then(|t| t.art),
            art_center: self.art_center(now),
            art_hover: self.cursor.is_some_and(|c| self.over_art(c, now)),
            level: self.level,
            time: now.duration_since(self.born).as_secs_f32(),
            privacy: {
                let uses = self.privacy.uses();
                if uses.iter().any(|u| u.device == privacy::Device::Camera) {
                    Some(PRIVACY_CAM)
                } else if uses.is_empty() {
                    None
                } else {
                    Some(PRIVACY_MIC)
                }
            },
            // Con la tira o el notch abiertos la cara se corre al extremo:
            // el punto de la grabación caería encima.
            recording: self.rec_clock.clone().filter(|_| self.strip.value(now) < 0.5 && !self.panel_visible()),
            paused: !self.music_playing,
            bars: self.bars_shown(),
            palette,
        }
    }

    /// Lleva el vidrio a la forma de la pill en este cuadro, o lo esconde.
    fn update_glass(&mut self, now: Instant, shown: bool) {
        // El vidrio se ve al tiro y la piel de GPUI un par de cuadros
        // después: mientras crece, el vidrio se adelantaba y asomaba negro
        // fuera de ella. Va con lo que tienen en común la forma de ahora y la
        // de hace `GLASS_LAG`, así nunca se sale de la piel que está a la vista.
        let pill = match (self.shape(now), self.shape(now.checked_sub(GLASS_LAG).unwrap_or(now))) {
            (
                PillShape::Tab { edge, rect, along, thick, block },
                PillShape::Tab { edge: before_edge, rect: before_rect, block: before_block, .. },
            ) if edge == before_edge => PillShape::Tab {
                edge,
                rect: overlap(&rect, &before_rect),
                along,
                thick,
                block: block.zip(before_block).map(|(now, before)| overlap(&now, &before)),
            },
            (pill, _) => pill,
        };
        let (Some(glass), Some(overlay)) = (self.glass.as_mut(), self.overlay.as_ref()) else {
            return;
        };
        let Some(origin) = overlay
            .origin()
            .filter(|_| shown && glass::backdrop_available() && appearance::current().glass)
        else {
            glass.hide();
            return;
        };
        let shape = match pill {
            PillShape::Tab {
                edge,
                rect,
                block: Some(block),
                ..
            } => {
                // Dos piezas: la columna y el bloque. Las esquinas de la
                // columna que el bloque tapa van vivas (si no, quedaría un
                // rincón sin vidrio bajo la piel); el rincón cóncavo de la
                // unión no lleva vidrio, casi no se nota bajo el tinte.
                let r = TAB_RADIUS.min(rect.w.min(rect.h) / 2.0);
                let mut column = tab_corners(edge, r);
                let (top, bottom) = match edge {
                    Edge::Right => (0, 3),
                    _ => (1, 2),
                };
                if block.y <= rect.y + 1.0 {
                    column[top] = 0.0;
                }
                if block.bottom() >= rect.bottom() - 1.0 {
                    column[bottom] = 0.0;
                }
                // Las esquinas del bloque junto a la columna van vivas, salvo
                // las que se asoman más allá de ella: ahí la piel las redondea
                // y un vidrio de esquina viva se veía como un cuadro negro.
                let rb = TAB_RADIUS.min(block.w.min(block.h) / 2.0);
                let mut block_corners = tab_corners(edge, rb);
                let (inner_top, inner_bottom) = match edge {
                    Edge::Right => (1, 2),
                    _ => (0, 3),
                };
                if block.y < rect.y - 1.0 {
                    block_corners[inner_top] = rb;
                }
                if block.bottom() > rect.bottom() + 1.0 {
                    block_corners[inner_bottom] = rb;
                }
                glass::Shape::Pair {
                    a: (grow_outward(&rect, edge, 1.0), column),
                    b: (block, block_corners),
                }
            }
            PillShape::Tab { edge, rect, .. } => glass::Shape::Rounded {
                rect: grow_outward(&rect, edge, 1.0),
                radii: tab_corners(edge, TAB_RADIUS.min(rect.w.min(rect.h) / 2.0)),
            },
            PillShape::Disc { center } => glass::Shape::Circle { center, r: DISC_R },
        };
        glass.place(shape, overlay.hwnd(), origin, self.scale_factor);
    }

    #[allow(clippy::type_complexity)]
    fn wheel_frame(
        &self,
        place: &WheelPlace,
        palette: &Palette,
        now: Instant,
    ) -> (
        Option<(f32, f32, f32)>,
        Vec<(f32, f32, f32)>,
        Vec<IconDraw>,
        Option<(f32, f32, f32, f32)>,
    ) {
        if !self.wheel_visible() {
            return (None, Vec::new(), Vec::new(), None);
        }
        let time = self.wheel_time;

        // El núcleo gotea desde la pill hasta el centro de la rueda.
        let drop = segment(time, 0.0, CORE_DROP_MS);
        let travel = ease_back_out(drop);
        let core_x = lerp(place.source.0, place.center.0, travel);
        let core_y = lerp(place.source.1, place.center.1, travel);
        let core_r = lerp(place.source_r, WHEEL_CORE_R, ease_smooth_out(drop));

        let mut blobs = Vec::new();
        let mut icons = Vec::new();
        let mut extent = core_r;
        let slots = self.wheel_slots();
        for (index, slot) in slots.iter().enumerate() {
            let start = self.wheel_bloom + BLOB_START_MS + BLOB_STAGGER_MS * index as f32;
            let progress = segment(time, start, BLOB_MS);
            if progress <= 0.0 {
                continue;
            }
            let angle = wheel_angle(index, slots.len());
            let reach = WHEEL_RING * ease_back_out(progress);
            let radius = lerp(12.0, WHEEL_BLOB_R, ease_smooth_out(progress));
            let (bx, by) = (core_x + angle.cos() * reach, core_y + angle.sin() * reach);
            blobs.push((bx, by, radius));
            extent = extent.max(reach + radius);

            let shown = segment(time, start + ICON_DELAY_MS, ICON_MS);
            if shown > 0.0 {
                let hover = self.wheel_hover[index].value(now);
                let scale = lerp(0.35, 1.0, ease_smooth_out(shown)) * (1.0 + 0.05 * hover);
                icons.push(IconDraw {
                    path: slot.icon(),
                    center: (bx, by),
                    size: WHEEL_ICON * scale,
                    color: mix(palette.muted, palette.text, hover).opacity(shown),
                });
            }
        }
        let shadow_alpha = segment(time, 0.0, 200.0);
        (
            Some((core_x, core_y, core_r)),
            blobs,
            icons,
            Some((core_x, core_y, extent, shadow_alpha)),
        )
    }
}

/// La herramienta del notch por su nombre en `PILL_OPEN` y `PILL_OPEN_SEQ`.
fn notch_tool_named(name: &str) -> Option<NotchTool> {
    Some(match name {
        "clipboard" => NotchTool::Clipboard,
        "textos" => NotchTool::Textos,
        "agentes" => NotchTool::Agentes,
        "sistema" => NotchTool::Sistema,
        "media" => NotchTool::Media,
        // `apps`: el lanzador; `emoji`: el lanzador en su modo emoji.
        "apps" | "emoji" => NotchTool::Apps,
        "personalizar" => NotchTool::Personalizar,
        _ => return None,
    })
}

/// Mismo orden que `paste_clipboard_item` en Atic: dar el foco al destino,
/// esperar a que lo asiente, escribir el portapapeles, esperar y mandar el
/// atajo que esa app entiende.
async fn paste_into(target: Option<paste::Target>, item: ClipboardItem, cx: &mut gpui::AsyncApp) {
    if let Some(target) = target {
        paste::force_foreground(target);
    }
    cx.background_executor().timer(PASTE_FOCUS_DELAY).await;
    if cx.update(|cx| cx.write_to_clipboard(item)).is_err() {
        return;
    }
    cx.background_executor().timer(PASTE_KEY_DELAY).await;
    paste::send_paste_chord(target.is_some_and(paste::needs_ctrl_shift_v));
}

/// Las imágenes se arrastran como archivo: `CF_HDROP` lo entiende casi todo.
fn write_drag_image(id: usize, bytes: &[u8]) -> std::io::Result<String> {
    let dir = std::env::temp_dir().join("atic-gpui-drag");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("atic-imagen-{id}.png"));
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

// --- Posición guardada -------------------------------------------------------
//
// Atic guarda borde y posición relativa (0..1) para sobrevivir a cambios de
// resolución (`atic.pill.home`). Aquí, `home.txt` en la carpeta de la pill.

fn home_file() -> Option<std::path::PathBuf> {
    crate::paths::file("home.txt")
}

/// El monitor de la ventana en sus coordenadas lógicas: entero, sin la barra
/// de tareas y los bordes donde se puede acoplar (los exteriores sin barra).
fn screen_geometry(overlay: &win::Overlay, scale_factor: f32) -> Option<(Rect, Rect, Vec<Edge>)> {
    let screen = overlay.screen()?;
    let monitor = overlay.to_logical(&screen.monitor, scale_factor)?;
    let work = overlay.to_logical(&screen.work, scale_factor)?;
    let outer = screen.outer_edges();
    let dockable = geometry::dockable_edges(&monitor, &work)
        .into_iter()
        .filter(|edge| outer.contains(edge))
        .collect();
    Some((monitor, work, dockable))
}

/// `home` guardado como fracciones del área de trabajo, para que sobreviva a
/// otro monitor, otra resolución u otra escala.
fn home_line(home: &Home, work: &Rect) -> String {
    let fraction = |value: f32, start: f32, length: f32| ((value - start) / length).clamp(0.0, 1.0);
    match *home {
        Home::Docked { edge, along } => {
            let (start, end) = edge.span(work);
            format!("{edge:?} {}", fraction(along, start, end - start))
        }
        Home::Floating { center } => format!(
            "Floating {} {}",
            fraction(center.0, work.x, work.w),
            fraction(center.1, work.y, work.h)
        ),
    }
}

/// Tras el `home`, la esquina del monitor en píxeles físicos:
/// `Top 0.5 @ -1920 0`.
fn save_home(home: &Home, work: &Rect, screen: Option<&win::Screen>) {
    let Some(path) = home_file() else {
        return;
    };
    let mut line = home_line(home, work);
    if let Some(screen) = screen {
        line.push_str(&format!(" @ {} {}", screen.monitor.left, screen.monitor.top));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, line);
}

/// El monitor donde quedó la pill, si sigue conectado.
fn saved_screen() -> Option<win::Screen> {
    let text = std::fs::read_to_string(home_file()?).ok()?;
    let mut corner = text.split_once('@')?.1.split_whitespace();
    let x = corner.next()?.parse().ok()?;
    let y = corner.next()?.parse().ok()?;
    win::Screen::at_origin(x, y)
}

fn load_home(work: &Rect, dockable: &[Edge]) -> Option<Home> {
    parse_home(&std::fs::read_to_string(home_file()?).ok()?, work, dockable)
}

fn default_home(work: &Rect) -> Home {
    Home::Docked {
        edge: Edge::Top,
        along: work.x + work.w / 2.0,
    }
}

fn parse_home(text: &str, work: &Rect, dockable: &[Edge]) -> Option<Home> {
    let mut parts = text.split_whitespace();
    let kind = parts.next()?;
    let a: f32 = parts.next()?.parse().ok()?;
    if kind == "Floating" {
        let b: f32 = parts.next()?.parse().ok()?;
        return Some(Home::Floating {
            center: (work.x + a * work.w, work.y + b * work.h),
        });
    }
    let edge = match kind {
        "Top" => Edge::Top,
        "Bottom" => Edge::Bottom,
        "Left" => Edge::Left,
        "Right" => Edge::Right,
        _ => return None,
    };
    // Si ese borde ya no se puede usar (barra de tareas movida), vuelve al
    // lugar por defecto.
    if !dockable.contains(&edge) {
        return None;
    }
    let (start, end) = edge.span(work);
    Some(Home::Docked {
        edge,
        along: geometry::clamp_along(edge, work, start + a * (end - start), TAB_LENGTH),
    })
}

fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    Hsla {
        h: lerp(from.h, to.h, t),
        s: lerp(from.s, to.s, t),
        l: lerp(from.l, to.l, t),
        a: lerp(from.a, to.a, t),
    }
}

fn pill_shadow(alpha: f32) -> BoxShadow {
    BoxShadow {
        color: Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.0,
            a: 0.38 * alpha,
        },
        offset: point(px(0.), px(10.)),
        blur_radius: px(22.),
        spread_radius: px(0.),
    }
}

fn bounds_of(rect: &Rect) -> Bounds<gpui::Pixels> {
    Bounds::new(point(px(rect.x), px(rect.y)), size(px(rect.w), px(rect.h)))
}

/// Esquinas redondeadas del tab: solo las del lado que mira al escritorio.
fn tab_corners(edge: Edge, radius: f32) -> [f32; 4] {
    match edge {
        Edge::Top => [0.0, 0.0, radius, radius],
        Edge::Bottom => [radius, radius, 0.0, 0.0],
        Edge::Left => [0.0, radius, radius, 0.0],
        Edge::Right => [radius, 0.0, 0.0, radius],
    }
}

/// La parte común de dos rectángulos (vacía si no se tocan).
fn overlap(a: &Rect, b: &Rect) -> Rect {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    Rect::new(x, y, (a.right().min(b.right()) - x).max(0.0), (a.bottom().min(b.bottom()) - y).max(0.0))
}

/// El tab se extiende 1 px fuera de la pantalla para que el antialias no deje
/// una línea clara pegada al borde; la sombra, un radio entero.
fn grow_outward(rect: &Rect, edge: Edge, by: f32) -> Rect {
    match edge {
        Edge::Top => Rect::new(rect.x, rect.y - by, rect.w, rect.h + by),
        Edge::Bottom => Rect::new(rect.x, rect.y, rect.w, rect.h + by),
        Edge::Left => Rect::new(rect.x - by, rect.y, rect.w + by, rect.h),
        Edge::Right => Rect::new(rect.x, rect.y, rect.w + by, rect.h),
    }
}

impl Frame {
    fn paint(self, window: &mut Window, cx: &mut App) {
        // Sombras primero: así la de una gota no oscurece a su vecina. La de
        // vidrio no lleva: se vería por transparencia como una mancha.
        match self.pill {
            _ if self.glass => {}
            PillShape::Tab { edge, rect, block, .. } => {
                window.paint_shadows(
                    bounds_of(&grow_outward(&rect, edge, TAB_RADIUS)),
                    Corners::all(px(TAB_RADIUS)),
                    &[pill_shadow(1.0)],
                );
                if let Some(block) = block {
                    window.paint_shadows(bounds_of(&block), Corners::all(px(TAB_RADIUS)), &[pill_shadow(1.0)]);
                }
            }
            PillShape::Disc { center } => window.paint_shadows(
                bounds_of(&Rect::centered(center, DISC_R * 2.0, DISC_R * 2.0)),
                Corners::all(px(DISC_R)),
                &[pill_shadow(1.0)],
            ),
        }
        if let Some((x, y, r, alpha)) = self.wheel_shadow {
            window.paint_shadows(
                bounds_of(&Rect::centered((x, y), r * 2.0, r * 2.0)),
                Corners::all(px(r)),
                &[pill_shadow(alpha)],
            );
        }
        if let Some(panel) = self.panel.as_ref().filter(|panel| !panel.notch) {
            window.paint_shadows(
                bounds_of(&panel.rect),
                Corners::all(px(PANEL_CORNER)),
                &[pill_shadow(1.0)],
            );
        }

        if let Some(preview) = &self.preview {
            window.paint_shadows(
                bounds_of(&preview.rect),
                Corners::all(px(PANEL_CORNER)),
                &[pill_shadow(preview.shadow)],
            );
        }

        // La pill de vidrio va en su propia silueta, translúcida; la rueda y
        // la burbuja siguen opacas porque detrás no tienen blur.
        let mut pill_skin = liquid::Silhouette::new();
        let mut skin = liquid::Silhouette::new();
        let target = if self.glass { &mut pill_skin } else { &mut skin };
        match self.pill {
            PillShape::Tab { .. } if self.outline.is_some() => {
                target.polygon(self.outline.as_deref().unwrap_or_default());
            }
            PillShape::Tab { edge, rect, .. } => {
                let radius = TAB_RADIUS.min(rect.w.min(rect.h) / 2.0);
                target.rounded_rect_corners(
                    &grow_outward(&rect, edge, 1.0),
                    tab_corners(edge, radius),
                );
            }
            PillShape::Disc { center } => {
                target.circle(center, DISC_R);
                for &(wall, wall_r) in &self.walls {
                    target.neck_within(center, DISC_R, wall, wall_r, WALL_REACH);
                }
            }
        }
        if let Some(path) = pill_skin.build() {
            window.paint_path(path, self.skin.opacity(self.tint));
        }
        if let Some(panel) = self.panel.as_ref().filter(|panel| !panel.notch) {
            let r = &panel.rect;
            skin.rounded_rect(r.x, r.y, r.w, r.h, PANEL_CORNER);
            // El cuello une la pill con la cara del panel y se corta cuando el
            // panel se aleja más de 12 px.
            let (anchor, anchor_r) = panel.anchor;
            let (near, near_r) = panel.near;
            skin.neck_within(anchor, anchor_r, near, near_r, PANEL_NECK_REACH);
        }
        if let Some(preview) = &self.preview {
            let r = &preview.rect;
            skin.rounded_rect(r.x, r.y, r.w, r.h, PANEL_CORNER.min(r.w / 2.0).min(r.h / 2.0));
            let (anchor, anchor_r) = preview.anchor;
            let (near, near_r) = preview.near;
            skin.neck_within(anchor, anchor_r, near, near_r, PANEL_NECK_REACH);
        }
        if let Some((x, y, r)) = self.core {
            let (anchor, anchor_r) = self.core_anchor;
            skin.neck(anchor, anchor_r, (x, y), r);
            skin.circle((x, y), r);
            for &(bx, by, br) in &self.blobs {
                skin.neck((x, y), r, (bx, by), br);
                skin.circle((bx, by), br);
            }
        }
        if let Some(path) = skin.build() {
            window.paint_path(path, self.skin);
        }

        self.paint_mark(window);
        match &self.recording {
            Some(clock) => self.paint_recording(clock, window, cx),
            None => self.paint_live(window, cx),
        }
        self.paint_privacy(window);
        for icon in self.strip_tools.iter().chain(&self.wheel_icons) {
            let _ = window.paint_svg(
                bounds_of(&Rect::centered(icon.center, icon.size, icon.size)),
                icon.path.into(),
                TransformationMatrix::unit(),
                icon.color,
                cx,
            );
        }
    }

    /// Carátula a la izquierda y onda a la derecha del tab, con lo que suena.
    fn paint_live(&self, window: &mut Window, cx: &mut App) {
        if self.live <= 0.02 {
            return;
        }
        let Some(center) = self.art_center else {
            return;
        };
        let disc = matches!(self.pill, PillShape::Disc { .. });
        let base = if disc { LIVE_BADGE } else { LIVE_ART };
        let side = base * self.live * if self.art_hover { 1.15 } else { 1.0 };
        let bounds = bounds_of(&Rect::centered(center, side, side));
        let corner = if disc { side / 2.0 } else { 6. * self.live };
        if disc {
            // Un aro del color de la pill separa la insignia de la gota.
            window.paint_quad(
                gpui::fill(bounds_of(&Rect::centered(center, side + 4.0, side + 4.0)), self.skin)
                    .corner_radii(Corners::all(px(side / 2.0 + 2.0))),
            );
        }
        match self.art.clone() {
            Some(art) => {
                // En pausa, en gris.
                let _ = window.paint_image(bounds, Corners::all(px(corner)), art, 0, self.paused);
            }
            None => window.paint_quad(
                gpui::fill(bounds, self.palette.text.opacity(0.15 * self.live))
                    .corner_radii(Corners::all(px(corner))),
            ),
        }
        if self.paused {
            // Atenuada y con ▶: un clic la reanuda.
            window.paint_quad(
                gpui::fill(bounds, self.skin.opacity(0.45 * self.live)).corner_radii(Corners::all(px(corner))),
            );
            let icon = side * 0.6;
            let _ = window.paint_svg(
                bounds_of(&Rect::centered(center, icon, icon)),
                "icons/play.svg".into(),
                TransformationMatrix::unit(),
                self.palette.text.opacity(self.live),
                cx,
            );
        }
        if !self.bars {
            return;
        }
        let level = (self.level * 1.6).clamp(0.08, 1.0);
        match self.pill {
            PillShape::Disc { center } => {
                // En la gota no caben barras: un aro que late con el sonido.
                let r = DISC_R + 3.0 + 5.0 * level;
                if let Some(ring) = ring_path(center.0, center.1, r, 1.5) {
                    window.paint_path(ring, self.palette.text.opacity(0.35 * self.live * level.max(0.3)));
                }
            }
            PillShape::Tab { edge, rect, .. } => {
                // Cuatro barras que siguen el nivel real, cada una con su
                // vaivén: a la derecha del tab, o abajo en los laterales.
                let privacy = if self.privacy.is_some() { 12.0 } else { 0.0 };
                let (cx, cy) = if edge.is_vertical() {
                    (self.mark.0, rect.bottom() - 18.0 - privacy)
                } else {
                    (rect.right() - 26.0 - privacy, self.mark.1)
                };
                for i in 0..4 {
                    let phase = self.time * (6.0 + i as f32 * 1.7) + i as f32 * 1.3;
                    let wobble = 0.55 + 0.45 * phase.sin().abs();
                    let h = (4.0 + 14.0 * level * wobble) * self.live;
                    let x = cx + (i as f32 - 1.5) * 6.0 - 1.5;
                    window.paint_quad(
                        gpui::fill(
                            Bounds::new(point(px(x), px(cy - h / 2.0)), size(px(3.), px(h))),
                            self.palette.text.opacity(0.9 * self.live),
                        )
                        .corner_radii(Corners::all(px(1.5))),
                    );
                }
            }
        }
    }

    /// Grabando una reunión: un punto rojo y el reloj donde iría la carátula.
    /// En un costado o en la gota no cabe el reloj: solo el punto.
    fn paint_recording(&self, clock: &str, window: &mut Window, cx: &mut App) {
        let red: Hsla = rgb(meetings::RECORD_RED).into();
        let dot = |window: &mut Window, center: (f32, f32)| {
            window.paint_quad(
                gpui::fill(bounds_of(&Rect::centered(center, 8.0, 8.0)), red).corner_radii(Corners::all(px(4.))),
            );
        };
        match self.pill {
            PillShape::Tab { edge, rect, .. } if !edge.is_vertical() => {
                let y = self.mark.1;
                let size = 11.0;
                let run = gpui::TextRun {
                    len: clock.len(),
                    font: gpui::font("Cascadia Mono"),
                    color: self.palette.text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window
                    .text_system()
                    .shape_line(clock.to_string().into(), px(size), &[run], None);
                // Grabando, el tab toma el largo del de música (`tab_length`) y
                // el reloj va donde la carátula; si aun así no cabe antes de la
                // cara, solo el punto.
                let dot_x = rect.x + 16.0;
                dot(window, (dot_x, y));
                let text_x = dot_x + 8.0;
                if text_x + f32::from(line.width) > self.mark.0 - MARK_SIZE / 2.0 {
                    return;
                }
                let height = size * 1.3;
                let _ = line.paint(point(px(text_x), px(y - height / 2.0)), px(height), window, cx);
            }
            _ => {
                if let Some(center) = self.art_center {
                    dot(window, center);
                }
            }
        }
    }

    /// El punto de privacidad, como en macOS: arriba a la derecha de la pill.
    fn paint_privacy(&self, window: &mut Window) {
        let Some(color) = self.privacy else {
            return;
        };
        let center = match self.pill {
            PillShape::Tab { rect, edge, .. } => match edge {
                Edge::Top => (rect.right() - 13.0, self.mark.1),
                Edge::Bottom => (rect.right() - 13.0, self.mark.1),
                Edge::Left | Edge::Right => (self.mark.0, rect.bottom() - 13.0),
            },
            PillShape::Disc { center } => (center.0 + DISC_R * 0.68, center.1 - DISC_R * 0.68),
        };
        window.paint_quad(
            gpui::fill(bounds_of(&Rect::centered(center, 7.0, 7.0)), gpui::Hsla::from(rgb(color)))
                .corner_radii(Corners::all(px(3.5))),
        );
    }

    /// Marca de Atic: una "a" minúscula con ojos que siguen al cursor.
    fn paint_mark(&self, window: &mut Window) {
        paint_mark_at(window, self.mark, self.eyes, self.blink, self.gear, self.palette.text);
    }
}

/// Marca de Atic: una "a" minúscula con ojos que siguen al cursor.
///
/// Con `gear` > 0 se vuelve engranaje (presión larga sobre la cara): el palo
/// de la "a" se recoge hacia el centro, los ojos se juntan en el eje y del
/// anillo salen los dientes mientras gira.
fn paint_mark_at(
    window: &mut Window,
    mark: (f32, f32),
    eyes: (f32, f32),
    blink: f32,
    gear: f32,
    color: Hsla,
) {
    let unit = MARK_SIZE / 24.0;
    let origin = (mark.0 - 12.0 * unit, mark.1 - 12.0 * unit);
    let at = |x: f32, y: f32| (origin.0 + x * unit, origin.1 + y * unit);
    let stroke = 1.5 * unit;
    let gear = gear.clamp(0.0, 1.0);

    let (cx, cy) = at(12.0, 12.0);
    let ring_r = 5.5 * unit;
    if let Some(ring) = ring_path(cx, cy, ring_r, stroke) {
        window.paint_path(ring, color);
    }

    // El palo de la "a": se encoge hacia su centro y se apaga.
    let stem_left = 1.0 - segment(gear, 0.0, 0.6);
    if stem_left > 0.0 {
        let (sx, top) = at(17.5, 6.5);
        let (_, bottom) = at(17.5, 17.5);
        let middle = (top + bottom) / 2.0;
        let top = lerp(middle, top, stem_left);
        let bottom = lerp(middle, bottom, stem_left);
        let ink = color.opacity(stem_left);
        let mut stem = gpui::PathBuilder::stroke(px(stroke));
        stem.move_to(point(px(sx), px(top)));
        stem.line_to(point(px(sx), px(bottom)));
        if let Ok(path) = stem.build() {
            window.paint_path(path, ink);
        }
        for y in [top, bottom] {
            if let Some(cap) = liquid::ellipse(sx, y, stroke / 2.0, stroke / 2.0) {
                window.paint_path(cap, ink);
            }
        }
    }

    // Los dientes: ocho, que crecen desde el anillo mientras gira.
    let teeth = segment(gear, 0.25, 0.75);
    if teeth > 0.0 {
        let turn = gear * std::f32::consts::FRAC_PI_4;
        let inner = ring_r;
        let outer = ring_r + 2.6 * unit * teeth;
        let width = 2.4 * unit;
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::FRAC_PI_4 + turn;
            let (dx, dy) = (angle.cos(), angle.sin());
            let mut tooth = gpui::PathBuilder::stroke(px(width));
            tooth.move_to(point(px(cx + dx * inner), px(cy + dy * inner)));
            tooth.line_to(point(px(cx + dx * outer), px(cy + dy * outer)));
            if let Ok(path) = tooth.build() {
                window.paint_path(path, color);
            }
        }
    }

    // Los ojos se juntan en el centro y pasan a ser el eje del engranaje.
    for eye_x in [10.3, 13.7] {
        let x = lerp(eye_x + eyes.0 * 1.1, 12.0, gear);
        let y = lerp(11.6 + eyes.1 * 1.1, 12.0, gear);
        let (ex, ey) = at(x, y);
        let rx = lerp(0.88, 1.7, gear) * unit;
        let ry = lerp(1.32 * blink, 1.7, gear) * unit;
        if let Some(eye) = liquid::ellipse(ex, ey, rx, ry) {
            window.paint_path(eye, color);
        }
    }
}

fn ring_path(cx: f32, cy: f32, r: f32, width: f32) -> Option<gpui::Path<gpui::Pixels>> {
    let mut builder = gpui::PathBuilder::stroke(px(width));
    builder.move_to(point(px(cx + r), px(cy)));
    builder.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        true,
        point(px(cx - r), px(cy)),
    );
    builder.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        true,
        point(px(cx + r), px(cy)),
    );
    builder.close();
    builder.build().ok()
}

impl Render for Pill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scale_factor = window.scale_factor();
        let fullscreen = self.capture.is_some()
            || self.board.is_some()
            || self.color.is_some()
            || self.flip.is_some();
        self.update_glass(Instant::now(), !fullscreen);
        // La mira tapa todo, notch incluido: es lo que se está capturando.
        if let Some(view) = self.capture.clone() {
            return div().size_full().child(view).into_any_element();
        }
        if let Some(view) = self.board.clone() {
            return div().size_full().child(view).into_any_element();
        }
        if let Some(view) = self.color.clone() {
            return div().size_full().child(view).into_any_element();
        }
        if let Some(view) = self.flip.clone() {
            return div().size_full().child(view).into_any_element();
        }
        // Con la ventana todavía estirada o volviendo, la pill caería corrida.
        if self.span.is_some() {
            return div().size_full().into_any_element();
        }
        let now = Instant::now();
        let animating = self.advance(now);
        if animating {
            window.request_animation_frame();
        }
        if std::env::var_os("PILL_FPS").is_some() {
            self.fps.1 += 1;
            if now.duration_since(self.fps.0) >= Duration::from_secs(1) {
                eprintln!(
                    "fps {} animando={animating} rueda={:.0}ms",
                    self.fps.1, self.wheel_time
                );
                self.fps = (now, 0);
            }
        }
        self.sync_panel_limits(cx);
        let desired = self.panel_desired_height(cx);
        if self.panel_visible() {
            self.notch_h.set(desired, now);
        } else {
            self.notch_h.snap(desired);
        }
        self.launcher_centered = self.launcher.read(cx).centered();
        // El notch siempre está arriba: la marca va a la izquierda de la franja.
        let mark_gap = true;
        if self.panel.read(cx).mark_gap != mark_gap {
            self.panel.update(cx, |panel, _| panel.mark_gap = mark_gap);
        }
        // Centrado no hay marca a su izquierda: va la lupa.
        let launcher_gap = mark_gap && !self.launcher_centered;
        if self.launcher.read(cx).mark_gap != launcher_gap {
            self.launcher.update(cx, |panel, _| panel.mark_gap = launcher_gap);
        }
        if self.snippets.read(cx).mark_gap != mark_gap {
            self.snippets.update(cx, |panel, _| panel.mark_gap = mark_gap);
        }
        // La burbuja sale recién cuando el contenido del notch ya se ve.
        let content_shown = self
            .panel_shape(now)
            .is_some_and(|shape| shape.content >= 1.0);
        if self.toast.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.toast = None;
        }
        let wanted = if let Some((entry, _)) = &self.toast {
            Some(entry.clone())
        } else if self.panel_open && content_shown && self.notch_tool == NotchTool::Clipboard {
            self.panel.read(cx).preview().cloned()
        } else if !self.panel_visible() && self.peek.value(now) > 0.9 {
            match self.peek_hovered {
                Some(PeekHit::Row(row)) => self
                    .peek_entries
                    .get(row)
                    .filter(|entry| entry.worth_preview())
                    .cloned(),
                _ => None,
            }
        } else {
            None
        };
        match wanted {
            Some(entry) => {
                self.preview_entry = Some(entry);
                self.preview.set(1.0, now);
            }
            None => self.preview.set(0.0, now),
        }
        let frame = self.frame(now);
        let preview_content = frame
            .preview
            .as_ref()
            .zip(self.preview_entry.as_ref())
            .map(|(shape, entry)| render_preview(entry, shape, &frame.palette));
        let peek_content = self.peek_rect(now).map(|rect| {
            render_peek(
                &self.peek_entries,
                self.peek_hovered,
                rect,
                self.side_drawers(),
                segment(self.peek.value(now), 0.55, 0.45),
                now.duration_since(self.peek_shown_at).as_secs_f32(),
                &frame.palette,
            )
        });
        // Los contadores del tab, el vistazo de Agentes y la insignia de la tira.
        let tray_content = self.tray_elements(now, cx);
        // El contenido se monta apenas se abre (para que el buscador reciba el
        // foco) y aparece cuando el panel ya tiene su tamaño.
        let mark_on_top = self.panel_visible();
        let mark = (frame.mark, frame.eyes, frame.blink, frame.gear, frame.palette.text);
        let panel_content = frame
            .panel
            .as_ref()
            .map(|shape| (shape.rect, shape.content, shape.notch, shape.edge));

        div()
            .size_full()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pill, event: &MouseDownEvent, window, cx| {
                    pill.on_mouse_down(event, window, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pill, _: &MouseUpEvent, window, cx| pill.release(window, cx)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| frame.paint(window, cx),
                )
                .size_full(),
            )
            .when_some(panel_content, |root, (rect, alpha, notch, edge)| {
                // El contenido queda recortado a la forma del notch: mientras
                // crece (al desplegar algo) no se sale por abajo. Las esquinas
                // redondas son las del lado que mira al escritorio, como el
                // tab (arriba, las de abajo).
                let radius = TAB_RADIUS.min(rect.w.min(rect.h) / 2.0);
                let [tl, tr, br, bl] = tab_corners(edge, radius);
                root.child(
                    div()
                        .absolute()
                        .left(px(rect.x))
                        .top(px(rect.y))
                        .w(px(rect.w))
                        .h(px(rect.h))
                        .overflow_hidden()
                        .when(notch, |el| {
                            el.rounded_tl(px(tl))
                                .rounded_tr(px(tr))
                                .rounded_br(px(br))
                                .rounded_bl(px(bl))
                        })
                        .when(!notch, |el| el.rounded(px(PANEL_CORNER)))
                        .opacity(alpha)
                        .child(match self.notch_tool {
                            NotchTool::Clipboard => self.panel.clone().into_any_element(),
                            NotchTool::Textos => self.snippets.clone().into_any_element(),
                            NotchTool::Apps => self.launcher.clone().into_any_element(),
                            NotchTool::Agentes => self.agents.clone().into_any_element(),
                            NotchTool::Sistema => self.system.clone().into_any_element(),
                            NotchTool::Media => self.media_panel.clone().into_any_element(),
                            NotchTool::Personalizar => self.customize.clone().into_any_element(),
                        }),
                )
            })
            // La marca otra vez, encima del contenido del notch: un panel con
            // fondo (la portada de Ahora suena) la taparía.
            .when(mark_on_top, |root| {
                root.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            paint_mark_at(window, mark.0, mark.1, mark.2, mark.3, mark.4)
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .children(peek_content)
            .children(tray_content)
            .children(self.hang_element(now))
            .children(self.dict_element(now, cx))
            .children(self.usage_element(now, cx))
            .children(self.tool_peek_element(now, cx))
            .children(preview_content)
            .children(self.render_shelf(now, cx))
            .into_any_element()
    }
}

/// Lo que se escribe en el portapapeles al pegar una entrada.
fn clipboard_item(entry: &clipboard::Entry) -> Option<ClipboardItem> {
    Some(match &entry.content {
        Content::Text(text) => ClipboardItem::new_string(text.to_string()),
        Content::Color(label, _) => ClipboardItem::new_string(label.to_string()),
        Content::Image(picture) => match picture.load() {
            Ok(image) => ClipboardItem::new_image(&image),
            Err(error) => {
                eprintln!("no se pudo leer la imagen {}: {error}", entry.key);
                return None;
            }
        },
    })
}

/// Las filas del vistazo, dentro del notch estirado bajo la franja.
fn render_peek(
    entries: &[clipboard::Entry],
    hovered: Option<PeekHit>,
    rect: Rect,
    // En el bloque angosto de un costado la ayuda junto al título no cabe.
    narrow: bool,
    alpha: f32,
    // Segundos desde que bajó: el mismo reloj que los vistazos de
    // `peeks.rs`, así las letras y las filas se toman su tiempo.
    intro: f32,
    palette: &Palette,
) -> gpui::AnyElement {
    let now = chrono::Local::now();
    let text = palette.text;
    let rows = entries.iter().enumerate().map(|(index, entry)| {
        let leading = match &entry.content {
            Content::Image(picture) => div()
                .w(px(30.))
                .h(px(22.))
                .rounded(px(5.))
                .overflow_hidden()
                .child(
                    picture
                        .view()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover),
                )
                .into_any_element(),
            Content::Color(_, color) => div()
                .size(px(16.))
                .mx(px(7.))
                .rounded(px(4.))
                .bg(*color)
                .into_any_element(),
            Content::Text(_) => match entry.source_icon.clone() {
                Some(icon) => div()
                    .w(px(30.))
                    .flex()
                    .justify_center()
                    .child(img(icon).size(px(16.)))
                    .into_any_element(),
                None => div()
                    .w(px(30.))
                    .flex()
                    .justify_center()
                    .child(
                        svg()
                            .path("icons/type.svg")
                            .size(px(13.))
                            .text_color(palette.muted.opacity(0.7)),
                    )
                    .into_any_element(),
            },
        };
        let when = match history::day_of(entry.created_ms, now) {
            history::Day::Yesterday => i18n::tf(
                "pill.clipboard.yesterdayAt",
                &[("time", &history::short_when(entry.created_ms, now))],
            ),
            _ => history::short_when(entry.created_ms, now).to_string(),
        };
        // Las filas caen a su sitio una tras otra mientras el vistazo baja,
        // como en los de las otras herramientas (`peeks.rs`).
        let enter = anim::ease_smooth_out(segment(intro, 0.10 + index as f32 * 0.055, 0.42));
        div()
            .relative()
            .top(px(-7.0 * (1.0 - enter)))
            .opacity(enter)
            .h(px(PEEK_ROW))
            .mx(px(6.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(10.))
            .child(leading)
            .when(entry.secret, |el| {
                el.child(
                    svg()
                        .path("icons/lock.svg")
                        .size(px(11.))
                        .flex_none()
                        .text_color(palette.muted),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .when(entry.looks_like_code(), |el| {
                        el.font_family("Cascadia Mono").text_size(px(11.))
                    })
                    .child(entry.preview.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(palette.muted.opacity(0.7))
                    .child(when),
            )
            // El realce se funde (los clics los decide `peek_hit`; esto es
            // solo cómo se ve).
            .fx(("clip-peek-row-fx", index), move |el, h| {
                el.bg(h.mix(text.opacity(0.0), text.opacity(0.08)))
            })
            .lit(hovered == Some(PeekHit::Row(index)))
    });
    let faint: Hsla = rgb(0x8f8f86).into();
    let title = div()
        .h(px(PEEK_PAD))
        .flex_none()
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(7.))
        .child(
            svg()
                .path("icons/clipboard.svg")
                .size(px(13.))
                .text_color(palette.muted)
                .opacity(anim::ease_smooth_out(segment(intro, 0.10, 0.42))),
        )
        .child(
            div()
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(peeks::falling("Portapapeles", intro)),
        )
        .child(div().flex_1())
        .when(!narrow, |el| {
            el.child(
                div()
                    .text_size(px(10.5))
                    .text_color(faint)
                    .opacity(anim::ease_smooth_out(segment(intro, 0.35, 0.25)))
                    .child("clic pega · arrastra a otra app"),
            )
        });
    let footer_on = hovered == Some(PeekHit::Footer);
    let footer = div()
        .h(px(PEEK_FOOTER))
        .mx(px(6.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(10.))
        .text_size(px(11.))
        .opacity(anim::ease_smooth_out(segment(intro, 0.30, 0.3)))
        .fx("clip-peek-footer-fx", move |el, h| {
            let fg = h.mix(faint, text);
            el.text_color(fg)
                .child("Ver todo el portapapeles")
                .child(
                    svg()
                        .path("icons/arrow-up-right.svg")
                        .size(px(11.))
                        .text_color(fg)
                        .ml(px(5. + 2.0 * h.t)),
                )
        })
        .lit(footer_on);
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
        .overflow_hidden()
        .opacity(alpha)
        .flex()
        .flex_col()
        .font_family("Segoe UI")
        .text_color(palette.text)
        .child(title)
        .when(entries.is_empty(), |el| {
            el.child(
                div()
                    .h(px(PEEK_ROW))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child("Todavía no has copiado nada"),
            )
        })
        .children(rows)
        .child(footer)
        .into_any_element()
}

/// Lo de adentro de la burbuja: la imagen entera, el color o el texto
/// completo (con los secretos ocultos).
fn render_preview(
    entry: &clipboard::Entry,
    shape: &PreviewShape,
    palette: &Palette,
) -> gpui::AnyElement {
    let rect = shape.rect;
    let frame = div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(PREVIEW_W))
        .h(px(PREVIEW_H))
        .opacity(shape.content)
        .p(px(PREVIEW_PAD))
        .flex()
        .flex_col()
        .gap(px(8.))
        .font_family("Segoe UI")
        .text_color(palette.text);
    let caption = |text: String| {
        div()
            .flex_none()
            .text_size(px(10.))
            .text_color(palette.muted)
            .truncate()
            .child(text)
    };
    match &entry.content {
        Content::Image(picture) => frame
            .child(
                div().flex_1().min_h_0().rounded(px(10.)).overflow_hidden().child(
                    picture
                        .view()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Contain),
                ),
            )
            .child(caption(entry.preview.to_string())),
        Content::Color(label, color) => frame
            .child(div().flex_1().rounded(px(10.)).bg(*color))
            .child(div().text_size(px(14.)).child(label.clone())),
        Content::Text(_) => frame
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .when(entry.looks_like_code(), |el| {
                        el.font_family("Cascadia Mono").text_size(px(11.))
                    })
                    .child(entry.shown.clone()),
            )
            .when(entry.secret, |el| {
                el.child(caption(i18n::t("pill.clipboard.hiddenSecret").into()))
            }),
    }
    .into_any_element()
}

/// `--native` (lo pasa el instalador): la pill es la principal. Enciende
/// `native_pill` en `config.json` para que la app de Tauri deje su overlay,
/// sus atajos y lo demás en manos de la pill.
fn claim_native_pill() {
    let Ok(dirs) = atic_core::AppDirs::new() else {
        return;
    };
    let path = dirs.config_path();
    let mut cfg = atic_core::Config::load(&path);
    if cfg.native_pill {
        return;
    }
    cfg.native_pill = true;
    match cfg.save(&path) {
        Ok(()) => tracing::info!("native_pill encendido: la pill es la principal"),
        Err(error) => tracing::warn!(%error, "no se pudo encender native_pill"),
    }
}

/// Sin su ventana la app no sirve: deja el motivo en el log y sale, en vez de
/// entrar en pánico.
fn quit_on_error(what: &str, error: anyhow::Error, cx: &mut App) {
    tracing::error!(error = format!("{error:#}"), "{what}");
    cx.quit();
}

fn main() {
    // Vive hasta que `main` termina: soltarlo antes pierde las últimas líneas.
    let _log_guard = init_log();
    let Some(_instance) = single_instance::acquire() else {
        tracing::info!("ya hay una pill corriendo en esta sesión");
        // Abrirla otra vez (menú Inicio) abre Atic, como en Tauri. No cuando
        // la lanza la propia app de Tauri al arrancar: ya está abriéndose.
        if !std::env::args().any(|arg| arg == "--from-atic") {
            tray_icon::wake_running();
        }
        return;
    };
    // Antes de que cualquier herramienta lea sus preferencias.
    paths::migrate();
    if std::env::args().any(|arg| arg == "--native") {
        claim_native_pill();
    }
    i18n::init();
    clipboard_owner::start();
    paste_queue::start();
    housekeeping::spawn();
    config_watch::spawn();
    updater::spawn();
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        text_input::bind_keys(cx);
        text_area::bind_keys(cx);
        clipboard::bind_keys(cx);
        capture::bind_keys(cx);
        flip::bind_keys(cx);
        flip_board::bind_keys(cx);
        board::bind_keys(cx);
        launcher::bind_keys(cx);
        agents::bind_keys(cx);
        system::bind_keys(cx);
        media::bind_keys(cx);
        color::bind_keys(cx);
        snippets::bind_keys(cx);
        meetings::bind_keys(cx);
        customize::bind_keys(cx);
        // `MEETINGS_ALONE=1`: solo la ventana de Reuniones, sin la pill.
        if std::env::var_os("MEETINGS_ALONE").is_some() {
            if let Err(error) = meetings::open_window(cx) {
                quit_on_error("no se pudo abrir Reuniones", error, cx);
            }
            return;
        }
        // `SPACE_ALONE=1`: solo el espacio, sin la pill (para medirlo sin el
        // overlay compartiendo el hilo).
        if std::env::var_os("SPACE_ALONE").is_some() {
            if let Err(error) = space::open_window(None, cx) {
                quit_on_error("no se pudo abrir el espacio", error, cx);
            }
            return;
        }
        let display = cx.primary_display();
        let screen = display
            .as_ref()
            .map(|display| display.bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.))));
        // Una sola ventana transparente que cubre el monitor: la pill se mueve
        // dentro, como en Atic, y fuera de ella los clics la atraviesan.
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(screen)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.map(|display| display.id()),
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        if let Err(error) = cx.open_window(options, |window, cx| cx.new(|cx| Pill::new(window, cx))) {
            return quit_on_error("no se pudo abrir la ventana de la pill", error, cx);
        }
        // `PILL_OPEN=settings`: los Ajustes junto a la pill. La primera vez
        // también, con la bienvenida en General.
        let first_run = atic_core::AppDirs::new()
            .is_ok_and(|dirs| !atic_core::Config::load(&dirs.config_path()).onboarding_done);
        if first_run || matches!(std::env::var("PILL_OPEN").as_deref(), Ok("settings" | "appearance")) {
            settings::open(cx);
        }
    });
}
