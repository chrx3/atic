//! Exportar: el diálogo de exportación del editor y el trabajo que le pasa al
//! exportador (`atic_capture::stream`). Plan en `docs/EDITOR_PULIDO.md` (ola 2).
//!
//! - **Diálogo** (`export_overlay`): formato (MP4 o GIF), proporción, resolución,
//!   calidad, códec y cuadros por segundo, con un resumen vivo (duración, tamaño del
//!   cuadro y peso estimado) y el destino (nombre editable y «Guardar como…»). Todo
//!   lo elegido va a `project.export`, así se recuerda.
//! - **Exportar** corre en otro hilo con avance y cancelar. Lo que se escribe va a un
//!   temporal junto al destino y se renombra al terminar: un archivo a medias nunca
//!   queda con el nombre bueno. El GIF pasa primero por un MP4 temporal chico (en
//!   `.atic`) y de ahí a GIF; el avance cuenta 0–0,5 el video y 0,5–1 el GIF.
//! - **`job`** arma el `Export` del exportador desde el proyecto; también lo usa
//!   «Ver cómo queda» (`preview`: más chico y rápido, con todo lo demás igual).
//!
//! Las cuentas (opciones, peso estimado, nombres, avance) son funciones puras con
//! tests al final.

use std::path::{Path, PathBuf};
use std::time::Instant;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use atic_capture::stream as cap;
use gpui::{
    div, prelude::*, px, svg, AnyElement, Context, Entity, Focusable, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window,
};

use super::fluid;
use super::palette::{Ed, T_BODY, T_CONTROL, T_SMALL, T_TITLE};
use super::scene::{cursor_px_of, SceneInputs};
use super::RecordingEditor;
use crate::edit_project::{Aspect, Backdrop, Codec, ExportSettings, Format, Quality, TextAt};
use crate::hover::{mix, HoverExt};
use crate::i18n::{t, tf};
use crate::text_input::{Changed, TextInput};

const DIALOG_W: f32 = 540.0;
/// El margen de la ventana alrededor del diálogo.
pub(super) const DIALOG_MARGIN: f32 = 24.0;
/// Cuánto baja el borde de arriba de la tarjeta (parte del alto de la ventana): queda fijo, y al cambiar
/// de contenido solo se mueve el borde de abajo.
const DIALOG_TOP_SHARE: f32 = 0.12;
/// El resumen de resultados no se redondea más que esto (en Expressive el radio de control lo vuelve una píldora).
const SUMMARY_RADIUS_MAX: f32 = 14.0;
const LABEL_W: f32 = 116.0;
pub(super) const SCRIM_ALPHA: f32 = 0.45;
/// El lado del cuadrito que muestra la proporción en sus opciones.
const ASPECT_BOX: f32 = 18.0;
const ASPECT_H: f32 = 48.0;
/// Lo más largo de un nombre de archivo (en bytes, no en letras: un emoji o una letra
/// china pesan 3 o 4): con el prefijo y la terminación de los temporales sigue cabiendo
/// en los 255 bytes de un nombre.
const MAX_STEM_BYTES: usize = 120;
pub(super) const SHADOW_ALPHA: f32 = 0.4;

/// La palomita de «Listo»: su lado, y desde qué parte crece al entrar.
const DONE_ICON: f32 = 22.0;
const DONE_FROM: f32 = 0.7;
/// El anillo del pulso del botón Exportar: su opacidad inicial.
const PULSE_ALPHA: f32 = 0.45;

/// «Ver cómo queda»: el alto, los cuadros por segundo y la calidad de la vista previa.
const PREVIEW_HEIGHT: u32 = 540;
const PREVIEW_FPS: u32 = 30;

/// Los altos que se ofrecen además del original.
const HEIGHTS: [u32; 3] = [1080, 720, 480];
const VIDEO_FPS: [u32; 2] = [30, 60];
/// En un GIF, `ExportSettings::height` guarda el ancho máximo y `fps` sus cuadros por segundo
/// (`ExportSettings` no tiene campos aparte; al cambiar de formato se guarda cada par).
const GIF_WIDTHS: [u32; 3] = [480, 640, 800];
const GIF_FPS: [u32; 3] = [10, 15, 20];
const GIF_WIDTH_DEFAULT: u32 = 640;
const GIF_FPS_DEFAULT: u32 = 15;
const VIDEO_FPS_DEFAULT: u32 = 60;
/// El GIF y el video temporal que lo precede: calidad alta para que la paleta del GIF parta de buena imagen.
const GIF_STAGE_QUALITY: cap::Quality = cap::Quality::High;

/// Bits por segundo del audio del exportador (AAC).
const AUDIO_BPS: f64 = 160_000.0;
/// Un GIF más largo que esto se avisa: pesa mucho y se ve a saltos.
const GIF_LONG_SECS: f64 = 60.0;
/// Peso grueso de un GIF de pantalla: bytes por píxel y por cuadro (sin diferencias entre cuadros).
const GIF_BYTES_PER_PIXEL: f64 = 0.25;
/// Los cuadros más que esto no los hace el exportador de GIF (`export_gif_macos.rs`).
const GIF_MAX_FRAMES: f64 = 1500.0;
const GIF_MAX_FPS: u32 = 30;
/// Menos que esto de un tramo con velocidad no cuenta.
const MIN_SPAN: f64 = 0.01;
/// El fondo del marco si la imagen elegida ya no está: un degradado neutro.
pub(super) const FALLBACK_BACKDROP: (u32, u32) = (0x232526, 0x414345);
/// Probar nombres «(editada 2)», «(editada 3)»… hasta aquí.
const NAME_TRIES: usize = 10_000;

/// Dónde queda el borde de arriba de un diálogo en una ventana de `window_h` px de alto.
pub(super) fn dialog_top(window_h: f32) -> f32 {
    (window_h * DIALOG_TOP_SHARE).max(DIALOG_MARGIN)
}

// --- Estado ---------------------------------------------------------------------------

/// Lo que pasa con la exportación: en curso o terminada (bien o mal).
enum Run {
    Running { progress: Arc<AtomicU64>, cancel: Arc<AtomicBool>, format: Format, name: String },
    Done { path: PathBuf, bytes: u64 },
    Failed { error: String },
}

/// El estado del diálogo de exportación.
pub(super) struct ExportUi {
    open: bool,
    /// El nombre del archivo, sin la terminación.
    name: Entity<TextInput>,
    /// El nombre lo escribió quien edita (si no, se rehace al cambiar de formato).
    name_edited: bool,
    /// Otra carpeta que la de la grabación («Guardar como…»).
    dir: Option<PathBuf>,
    /// El `height` y el `fps` del otro formato, para el vaivén MP4 ↔ GIF.
    stash: Option<(Option<u32>, u32)>,
    run: Option<Run>,
    /// Un aviso corto bajo las acciones («Copiado»).
    note: Option<SharedString>,
    /// El paso que muestra el diálogo (`step_of`) y desde cuándo: lo nuevo entra con un fundido.
    step: (u8, Instant),
    /// Cuántas exportaciones se han iniciado: el avance suavizado y «cancelando» son de cada una.
    run_id: u64,
    /// Cuándo terminó una exportación con el diálogo oculto: el botón de arriba da un pulso.
    pulse_at: Option<Instant>,
    /// El destino que ya existe y espera un segundo clic («Reemplazar»). Se olvida
    /// al cambiar el nombre, la carpeta o el formato.
    confirm: Option<PathBuf>,
}

impl ExportUi {
    pub(super) fn new(pal: &Ed, cx: &mut Context<RecordingEditor>) -> Self {
        let name = cx.new(|cx| TextInput::new(t("pill.editor.exportName"), pal.text, pal.sub, pal.accent, cx));
        // Escribir el nombre lo vuelve «suyo»: ya no se rehace solo.
        cx.subscribe(&name, |view: &mut RecordingEditor, _, _: &Changed, cx| {
            view.export_ui.name_edited = true;
            view.export_ui.note = None;
            // Otro nombre, otro destino: la confirmación de «Reemplazar» ya no vale.
            view.export_ui.confirm = None;
            cx.notify();
        })
        .detach();
        Self {
            open: false,
            name,
            name_edited: false,
            dir: None,
            stash: None,
            run: None,
            note: None,
            step: (0, Instant::now()),
            run_id: 0,
            pulse_at: None,
            confirm: None,
        }
    }
}

/// Qué paso muestra el diálogo: 0 los ajustes, 1 exportando, 2 listo, 3 falló. Cuando cambia, el
/// contenido nuevo entra con un fundido (y el anterior se va de golpe: nunca dos copias a la vez).
fn step_of(run: &Option<Run>) -> u8 {
    match run {
        None => 0,
        Some(Run::Running { .. }) => 1,
        Some(Run::Done { .. }) => 2,
        Some(Run::Failed { .. }) => 3,
    }
}

impl Drop for ExportUi {
    /// Cerrar la ventana corta lo que esté exportando.
    fn drop(&mut self) {
        if let Some(Run::Running { cancel, .. }) = &self.run {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

// --- Lo puro: opciones, tamaños, nombres, avance ----------------------------------------

pub(super) fn rgb_of(color: u32) -> [f64; 3] {
    [((color >> 16) & 0xff) as f64 / 255.0, ((color >> 8) & 0xff) as f64 / 255.0, (color & 0xff) as f64 / 255.0]
}

fn cap_codec(codec: Codec) -> cap::Codec {
    match codec {
        Codec::H264 => cap::Codec::H264,
        Codec::Hevc => cap::Codec::Hevc,
    }
}

fn cap_quality(quality: Quality) -> cap::Quality {
    match quality {
        Quality::Low => cap::Quality::Low,
        Quality::Standard => cap::Quality::Standard,
        Quality::High => cap::Quality::High,
        Quality::Max => cap::Quality::Max,
    }
}

fn ext_of(format: Format) -> &'static str {
    match format {
        Format::Mp4 => "mp4",
        Format::Gif => "gif",
    }
}

/// El ancho máximo de un GIF (guardado en `height`); uno que no sea de la lista cae al de siempre.
fn gif_width(settings: &ExportSettings) -> u32 {
    settings.height.filter(|w| GIF_WIDTHS.contains(w)).unwrap_or(GIF_WIDTH_DEFAULT)
}

fn gif_fps(settings: &ExportSettings) -> u32 {
    if GIF_FPS.contains(&settings.fps) {
        settings.fps
    } else {
        GIF_FPS_DEFAULT
    }
}

/// `height` y `fps` con que nace cada formato.
fn format_defaults(format: Format) -> (Option<u32>, u32) {
    match format {
        Format::Mp4 => (ExportSettings::default().height, VIDEO_FPS_DEFAULT),
        Format::Gif => (Some(GIF_WIDTH_DEFAULT), GIF_FPS_DEFAULT),
    }
}

/// Pasa a `to` guardando el `height` y el `fps` del formato que se deja (y recuperando los que
/// había dejado el otro): ir de MP4 a GIF y volver no pierde lo elegido.
fn switch_format(settings: &mut ExportSettings, stash: &mut Option<(Option<u32>, u32)>, to: Format) {
    if settings.format == to {
        return;
    }
    let other = stash.take().unwrap_or_else(|| format_defaults(to));
    *stash = Some((settings.height, settings.fps));
    (settings.height, settings.fps) = other;
    settings.format = to;
}

/// El alto que deja el ancho dentro de `max_width` con la proporción `ratio` (par).
fn gif_stage_height(max_width: u32, (rw, rh): (u32, u32)) -> u32 {
    let h = max_width as u64 * rh.max(1) as u64 / rw.max(1) as u64;
    ((h.min(u32::MAX as u64) as u32) & !1).max(2)
}

/// La proporción pedida al exportador: `None` si es la del original.
fn aspect_option(settings: &ExportSettings, source: (u32, u32)) -> Option<(u32, u32)> {
    (settings.aspect != Aspect::Source).then(|| settings.aspect.ratio(source))
}

/// El alto de salida que cuenta: uno que no baja del original es el original.
fn normalized_height(height: Option<u32>, source_h: u32) -> Option<u32> {
    height.filter(|h| *h < source_h)
}

/// El cuadro de salida del exportador (`output_size` en `export_macos.rs`): el alto sin pasar
/// del original ni de `max_height`, y el ancho de la proporción; los dos pares.
fn out_dims((src_w, src_h): (u32, u32), max_height: Option<u32>, aspect: Option<(u32, u32)>) -> (u32, u32) {
    let src_h = src_h.max(2);
    let out_h = (src_h.min(max_height.unwrap_or(u32::MAX).max(2))) & !1;
    let out_w = match aspect {
        Some((aw, ah)) if aw > 0 && ah > 0 => (out_h as u64 * aw as u64 / ah as u64) as u32,
        _ if out_h == src_h & !1 => src_w,
        _ => (out_h as u64 * src_w as u64 / src_h as u64) as u32,
    };
    ((out_w & !1).max(2), out_h.max(2))
}

/// El cuadro que sale del exportar con estos ajustes (el del GIF, ya acotado a su ancho).
pub(super) fn render_dims(settings: &ExportSettings, source: (u32, u32)) -> (u32, u32) {
    let aspect = aspect_option(settings, source);
    match settings.format {
        Format::Mp4 => out_dims(source, settings.height, aspect),
        Format::Gif => out_dims(source, Some(gif_stage_height(gif_width(settings), settings.aspect.ratio(source))), aspect),
    }
}

/// Las opciones del exportador. `preview`: la vista previa de unos segundos (chica, a 30
/// cuadros y de calidad baja; lo demás igual). Un GIF exporta primero un video chico (del
/// ancho que pide, a sus cuadros por segundo) del que se saca el GIF.
fn export_options(settings: &ExportSettings, source: (u32, u32), preview: bool) -> cap::ExportOptions {
    let aspect = aspect_option(settings, source);
    if preview {
        return cap::ExportOptions { codec: cap_codec(settings.codec), quality: cap::Quality::Low, max_height: Some(PREVIEW_HEIGHT), fps: PREVIEW_FPS, aspect };
    }
    match settings.format {
        Format::Mp4 => cap::ExportOptions {
            codec: cap_codec(settings.codec),
            quality: cap_quality(settings.quality),
            max_height: settings.height,
            fps: settings.fps,
            aspect,
        },
        Format::Gif => cap::ExportOptions {
            codec: cap::Codec::H264,
            quality: GIF_STAGE_QUALITY,
            max_height: Some(gif_stage_height(gif_width(settings), settings.aspect.ratio(source))),
            fps: gif_fps(settings),
            aspect,
        },
    }
}

/// Los tramos con velocidad que caen dentro de `segments` (los de «Ver cómo queda» son un
/// pedazo del proyecto), recortados a ellos.
fn clip_speeds(spans: &[(f64, f64, f64)], segments: &[(f64, f64)]) -> Vec<cap::SpeedSpan> {
    spans
        .iter()
        .flat_map(|&(a, b, rate)| {
            segments.iter().filter_map(move |&(sa, sb)| {
                let (start, end) = (a.max(sa), b.min(sb));
                (end - start > MIN_SPAN).then_some(cap::SpeedSpan { start, end, rate })
            })
        })
        .collect()
}

fn quality_factor(quality: Quality) -> f64 {
    match quality {
        Quality::Low => 0.07,
        Quality::Standard => 0.12,
        Quality::High => 0.20,
        Quality::Max => 0.35,
    }
}

/// Bits por segundo del video: la fórmula del exportador (`bitrate` en `export_macos.rs`).
fn video_bitrate((w, h): (u32, u32), quality: Quality, codec: Codec) -> f64 {
    let codec_factor = if codec == Codec::Hevc { 0.6 } else { 1.0 };
    (w as f64 * h as f64 * 30.0 * quality_factor(quality) * codec_factor).round().clamp(2_000_000.0, 60_000_000.0)
}

/// Cuántos cuadros saca el exportador de GIF en `seconds` a `fps` (`plan` en `export_gif_macos.rs`:
/// pasos de centésimas y un tope de cuadros).
fn gif_frames(seconds: f64, fps: u32) -> f64 {
    let step = (100.0 / fps.clamp(1, GIF_MAX_FPS) as f64).round().max(1.0);
    (seconds.max(0.0) * 100.0 / step).ceil().clamp(1.0, GIF_MAX_FRAMES)
}

/// El peso estimado del archivo, en bytes: video más audio, o una cuenta gruesa para el GIF.
fn estimate_bytes(settings: &ExportSettings, source: (u32, u32), seconds: f64) -> u64 {
    let (w, h) = render_dims(settings, source);
    let seconds = seconds.max(0.0);
    let bytes = match settings.format {
        Format::Mp4 => (video_bitrate((w, h), settings.quality, settings.codec) + AUDIO_BPS) * seconds / 8.0,
        Format::Gif => w as f64 * h as f64 * GIF_BYTES_PER_PIXEL * gif_frames(seconds, gif_fps(settings)),
    };
    bytes.round() as u64
}

/// «38 MB», «1,2 GB», «820 KB» (los mil del Finder).
fn human_bytes(bytes: u64) -> String {
    human_bytes_in(bytes, crate::i18n::english())
}

/// `human_bytes` con el punto decimal del inglés o la coma del español.
fn human_bytes_in(bytes: u64, english: bool) -> String {
    let b = bytes as f64;
    let number = |value: f64, decimals: usize| {
        let text = format!("{value:.decimals$}");
        if english { text } else { text.replace('.', ",") }
    };
    if b >= 1e9 {
        format!("{} GB", number(b / 1e9, 1))
    } else if b >= 1e8 {
        format!("{} MB", number(b / 1e6, 0))
    } else if b >= 1e6 {
        format!("{} MB", number(b / 1e6, 1))
    } else if b >= 1e3 {
        format!("{} KB", number(b / 1e3, 0))
    } else {
        format!("{bytes} B")
    }
}

/// «1:05» o «1:02:03».
fn clock(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// El video que sale del editor.
    Video,
    /// El GIF que se saca de ese video.
    Gif,
}

/// El porcentaje entero que se muestra de un avance de 0 a 1 (nunca pasa de 100 ni baja de 0).
fn percent(p: f64) -> u32 {
    (p.clamp(0.0, 1.0) * 100.0).round() as u32
}

/// El avance de todo (0–1) cuando la etapa `stage` va en `p`: en un GIF, la mitad es el video.
fn overall(format: Format, stage: Stage, p: f64) -> f64 {
    let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
    match (format, stage) {
        (Format::Gif, Stage::Video) => p * 0.5,
        (Format::Gif, Stage::Gif) => 0.5 + p * 0.5,
        _ => p,
    }
}

/// El nombre del archivo que se escribió, sin terminación ni letras que rompen una ruta;
/// `None` si no queda nada.
fn clean_stem(text: &str, ext: &str) -> Option<String> {
    let replaced: String = text.chars().map(|c| if matches!(c, '/' | '\\' | ':') || c.is_control() { '-' } else { c }).collect();
    let mut stem = replaced.trim();
    // Quien escribe «demo.mp4» no quiere «demo.mp4.mp4».
    let dot = ext.len() + 1;
    if stem.len() >= dot && stem.is_char_boundary(stem.len() - dot) {
        let (head, tail) = stem.split_at(stem.len() - dot);
        if tail.starts_with('.') && tail[1..].eq_ignore_ascii_case(ext) {
            stem = head.trim_end();
        }
    }
    // Un nombre con punto al comienzo queda oculto.
    let stem = truncate_bytes(stem.trim_start_matches('.').trim(), MAX_STEM_BYTES).trim_end().to_string();
    (!stem.is_empty()).then_some(stem)
}

/// Lo que cabe de `s` en `max` bytes, sin partir un carácter por la mitad.
fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// «Nombre (editada)», o «Nombre (editada 2)»… el primero que no existe en `dir` con `ext`.
fn unique_stem(dir: &Path, recording: &str, edited: &str, ext: &str, exists: impl Fn(&Path) -> bool) -> String {
    let mut stem = format!("{recording} ({edited})");
    let mut n = 2;
    while n < NAME_TRIES && exists(dir.join(format!("{stem}.{ext}")).as_path()) {
        stem = format!("{recording} ({edited} {n})");
        n += 1;
    }
    stem
}

/// Donde se escribe mientras se exporta: `.nombre.atic-part.ext`, junto al destino (así el
/// renombrado final no cruza de disco).
fn part_path(out: &Path) -> PathBuf {
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = out.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    out.with_file_name(format!(".{stem}.atic-part.{ext}"))
}

/// El MP4 intermedio de un GIF: `.atic/<grabación>.gif-tmp.mp4`.
fn gif_tmp_path(recording: &Path) -> PathBuf {
    // Único por exportación: dos ventanas con la misma grabación no se pisan el temporal.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let stem = recording.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    recording.parent().map(PathBuf::from).unwrap_or_default().join(".atic").join(format!("{stem}.gif-tmp-{}-{n}.mp4", std::process::id()))
}

/// La carpeta con `~` en vez de la de la casa.
fn short_dir(dir: &Path) -> String {
    let text = dir.to_string_lossy();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => format!("~{}", &text[home.len()..]),
        _ => text.into_owned(),
    }
}

pub(super) fn backdrop_of(backdrop: Backdrop, image: Option<&Path>) -> cap::Backdrop {
    match backdrop {
        Backdrop::Gradient(a, b) => cap::Backdrop::Gradient(rgb_of(a), rgb_of(b)),
        Backdrop::Solid(c) => cap::Backdrop::Solid(rgb_of(c)),
        Backdrop::None => cap::Backdrop::Solid([0.0; 3]),
        // Si la imagen se movió o se borró, el degradado neutro: el marco no se rompe.
        Backdrop::Image => match image.and_then(super::wallpapers::resolve) {
            Some(path) => cap::Backdrop::Image(path),
            None => cap::Backdrop::Gradient(rgb_of(FALLBACK_BACKDROP.0), rgb_of(FALLBACK_BACKDROP.1)),
        },
    }
}

pub(super) fn text_spec(text: &crate::edit_project::Text) -> cap::TextSpec {
    cap::TextSpec {
        text: text.text.clone(),
        at: match text.at {
            TextAt::Top => cap::TextAt::Top,
            TextAt::Center => cap::TextAt::Center,
            TextAt::Bottom => cap::TextAt::Bottom,
        },
        size: text.size,
        color: rgb_of(text.color),
        boxed: text.boxed,
        bold: text.bold,
        caption: text.caption,
    }
}

// --- Exportar en otro hilo ----------------------------------------------------------------

/// Adónde va lo que se exporta y por dónde pasa.
struct Plan {
    out: PathBuf,
    /// Lo que se escribe mientras tanto (se renombra a `out` al terminar).
    part: PathBuf,
    /// El GIF: el MP4 intermedio, el ancho máximo y los cuadros por segundo.
    gif: Option<(PathBuf, u32, u32)>,
    /// Archivos que no se pueden reemplazar nunca: la grabación que se ve y el
    /// máster del que sale la imagen. Se vuelve a comprobar aquí, al publicar,
    /// porque entre escribir el nombre y terminar la exportación pasa tiempo.
    keep: Vec<PathBuf>,
}

/// Si `a` y `b` son el mismo archivo. Compara por ruta canónica (la del padre si
/// la hoja no existe todavía), así «./x.mp4», «x.mp4» y la ruta absoluta del mismo
/// archivo dan lo mismo.
fn same_file(a: &Path, b: &Path) -> bool {
    fn canon(path: &Path) -> Option<PathBuf> {
        if let Ok(full) = std::fs::canonicalize(path) {
            return Some(full);
        }
        let parent = std::fs::canonicalize(path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))).ok()?;
        Some(parent.join(path.file_name()?))
    }
    match (canon(a), canon(b)) {
        (Some(full_a), Some(full_b)) => full_a == full_b,
        // Sin poder resolverlas (una carpeta que todavía no existe), queda la comparación literal.
        _ => a == b,
    }
}

/// Borra los temporales al salir, también si falla, se cancela o hay un pánico.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Hace la exportación (en un hilo de fondo) y devuelve el peso del archivo.
fn run_export(source: &Path, job: cap::Export, plan: &Plan, progress: &AtomicU64, cancel: &AtomicBool) -> Result<u64, String> {
    let mut temps = vec![plan.part.clone()];
    temps.extend(plan.gif.iter().map(|(tmp, _, _)| tmp.clone()));
    let _cleanup = Cleanup(temps);
    for dir in [plan.out.parent(), plan.gif.as_ref().and_then(|(tmp, _, _)| tmp.parent())].into_iter().flatten() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // El destino no puede ser el video que se está exportando (ni su máster): un
    // `rename` encima destruiría el original y el proyecto quedaría apuntando a
    // tiempos que ya no existen.
    if let Some(kept) = plan.keep.iter().find(|kept| same_file(&plan.out, kept)) {
        return Err(tf("pill.editor.exportIsSource", &[("name", &kept.display().to_string())]));
    }
    match &plan.gif {
        None => cap::export(source, &plan.part, job)?,
        Some((tmp, width, fps)) => {
            cap::export(source, tmp, job)?;
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelada".into());
            }
            let on_progress = |p: f64| progress.store(overall(Format::Gif, Stage::Gif, p).to_bits(), Ordering::Relaxed);
            cap::export_gif(tmp, &plan.part, *width, *fps, &on_progress, cancel)?;
        }
    }
    std::fs::rename(&plan.part, &plan.out).map_err(|e| format!("no se pudo guardar {}: {e}", plan.out.display()))?;
    progress.store(1f64.to_bits(), Ordering::Relaxed);
    Ok(std::fs::metadata(&plan.out).map(|m| m.len()).unwrap_or(0))
}

// --- El editor: el botón, el diálogo y el trabajo ---------------------------------------------

impl RecordingEditor {
    /// ¿Hay una exportación corriendo? (el editor pide cuadros de animación por eso)
    pub(super) fn export_busy(&self) -> bool {
        matches!(self.export_ui.run, Some(Run::Running { .. }))
    }

    /// Para los agentes: cómo va la exportación (`idle`, `running`, `done` o `failed`).
    pub(super) fn export_status(&self) -> serde_json::Value {
        use serde_json::json;
        match &self.export_ui.run {
            None => json!({ "status": "idle" }),
            Some(Run::Running { progress, name, .. }) => json!({ "status": "running", "progress": f64::from_bits(progress.load(Ordering::Relaxed)), "name": name }),
            Some(Run::Done { path, bytes }) => json!({ "status": "done", "path": path, "bytes": bytes }),
            Some(Run::Failed { error }) => json!({ "status": "failed", "error": error }),
        }
    }

    /// Para los agentes: exporta con los ajustes del proyecto si no hay una exportación en curso ni hecha.
    pub(super) fn agent_export_start(&mut self, cx: &mut Context<Self>) {
        if self.export_ui.run.is_none() {
            self.export_default_name(cx);
            self.start_export(cx);
        }
    }

    /// ⌘E y el botón de arriba: abre el diálogo.
    pub(super) fn open_export(&mut self, cx: &mut Context<Self>) {
        self.player.pause();
        self.export_ui.open = true;
        if self.export_ui.run.is_none() {
            self.export_default_name(cx);
        }
        cx.notify();
    }

    /// El proyecto cambió: lo exportado antes ya no es lo que saldría. No toca una exportación en curso.
    pub(super) fn export_reset(&mut self) {
        if !self.export_busy() {
            self.export_ui.run = None;
        }
    }

    /// Las teclas con el diálogo abierto: Esc lo cierra (si no exporta), Intro exporta. Devuelve
    /// `true` si el diálogo está abierto: el editor no debe tratar la tecla como atajo.
    pub(super) fn export_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.export_ui.open {
            return false;
        }
        let typing = self.export_ui.name.read(cx).focus_handle(cx).is_focused(window);
        match event.keystroke.key.as_str() {
            "escape" => {
                if !self.export_busy() {
                    window.focus(&self.focus);
                    self.export_ui.open = false;
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "enter" => {
                if self.export_ui.run.is_none() {
                    self.start_export(cx);
                }
                cx.stop_propagation();
            }
            // Escribiendo el nombre, las teclas son del campo; si no, de nadie.
            _ if !typing => cx.stop_propagation(),
            _ => {}
        }
        true
    }

    /// El botón de arriba a la derecha: abre el diálogo, o muestra el avance o el resultado.
    pub(super) fn export_button(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let (icon, label, tip): (&'static str, SharedString, &'static str) = match &self.export_ui.run {
            Some(Run::Running { progress, .. }) => {
                let pct = percent(self.export_shown(f64::from_bits(progress.load(Ordering::Relaxed))));
                ("icons/download.svg", tf("pill.editor.exporting", &[("pct", &pct)]).into(), t("pill.editor.exportShowTip"))
            }
            Some(Run::Done { .. }) => ("icons/check.svg", t("pill.editor.exported").into(), t("pill.editor.exportDoneTip")),
            Some(Run::Failed { .. }) => ("icons/download.svg", t("pill.editor.exportFailed").into(), t("pill.editor.exportFailedTip")),
            None => ("icons/download.svg", t("pill.editor.export").into(), t("pill.editor.exportOpenTip")),
        };
        let button = self.primary_w(pal, "editor-export", icon, label, tip, Some(super::EXPORT_BTN_W), |view, _, cx| view.open_export(cx), cx);
        // Si la exportación terminó con el diálogo oculto, un anillo de acento se abre y se apaga una sola vez.
        let pulse = match self.export_ui.pulse_at {
            Some(at) if !self.fluid.reduced() => self.fluid.since(at, super::PULSE_LEN),
            _ => 1.0,
        };
        let ring = gpui::BoxShadow {
            color: pal.accent.opacity(PULSE_ALPHA * (1.0 - pulse)),
            offset: gpui::point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(super::PULSE_SPREAD * pulse),
        };
        div().flex_none().rounded(px(pal.r_chip.min(super::BUTTON / 2.0))).shadow(vec![ring]).child(button).into_any_element()
    }

    /// El avance mostrado (0–1): sigue a lo que informa el exportador con un deslizamiento corto, así la barra
    /// y el porcentaje no avanzan a escalones. Cada exportación tiene el suyo (la siguiente parte de 0).
    fn export_shown(&self, p: f64) -> f64 {
        let key = format!("export-progress-{}", self.export_ui.run_id);
        f64::from(self.fluid.glide(key, p.clamp(0.0, 1.0) as f32, fluid::PROGRESS_GLIDE, false))
    }

    /// El diálogo como capa absoluta sobre la ventana; `None` si está cerrado.
    pub(super) fn export_overlay(&mut self, pal: &Ed, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Abierto o cerrado manda: el fondo y la tarjeta van hacia ese destino y, si se abre y se
        // cierra rápido, siguen desde donde estaban. El overlay sigue en el árbol mientras se
        // desvanece (nada se quita de golpe).
        let open = self.export_ui.open;
        let scrim = self.fluid.toggle("export-scrim", open, fluid::SCRIM_IN, fluid::DIALOG_OUT);
        let card_in = self.fluid.toggle("export-card", open, fluid::CARD_IN, fluid::DIALOG_OUT);
        let step = step_of(&self.export_ui.run);
        if step != self.export_ui.step.0 {
            self.export_ui.step = (step, Instant::now());
        }
        if !open && scrim <= 0.0 && card_in <= 0.0 {
            return None;
        }
        let step_in = self.fluid.since(self.export_ui.step.1, fluid::STEP_FADE);
        let busy = self.export_busy();
        let (title, body, foot) = match &self.export_ui.run {
            None => (t("pill.editor.exportTitle"), self.export_settings_body(pal, cx), self.export_settings_footer(pal, cx)),
            Some(Run::Running { progress, cancel, format, name }) => {
                let p = self.export_shown(f64::from_bits(progress.load(Ordering::Relaxed)));
                let (format, name, cancelling) = (*format, name.clone(), cancel.load(Ordering::Relaxed));
                // «Cancelando» tiene su propio estado: la barra se apaga hacia `faint` y deja de ser del acento.
                let cancel_t = self.fluid.toggle(format!("export-cancelling-{}", self.export_ui.run_id), cancelling, fluid::CANCEL_FADE, fluid::CANCEL_FADE);
                (t("pill.editor.exportTitle"), self.export_running_body(pal, p, format, &name, cancelling, cancel_t), self.export_running_footer(pal, cancelling, cx))
            }
            Some(Run::Done { path, bytes }) => {
                let (path, bytes) = (path.clone(), *bytes);
                (t("pill.editor.exported"), self.export_done_body(pal, &path, bytes, cx), self.export_done_footer(pal, cx))
            }
            Some(Run::Failed { error }) => {
                let error = error.clone();
                (t("pill.editor.exportFailed"), self.export_failed_body(pal, &error), self.export_failed_footer(pal, cx))
            }
        };
        let close = (!busy).then(|| self.button(pal, "export-close", "icons/x.svg", None, t("pill.editor.exportCloseTip"), |view, _, cx| view.export_close(cx), cx));
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(18.))
            .pr(px(10.))
            .pt(px(14.))
            .pb(px(10.))
            .opacity(step_in)
            .child(svg().path("icons/video.svg").size(px(16.)).text_color(pal.sub))
            .child(div().flex_1().min_w_0().text_size(px(T_TITLE)).font_weight(gpui::FontWeight::SEMIBOLD).child(title))
            .children(close);
        // Opaco: sobre el fondo atenuado un vidrio o un panel translúcido no se leería.
        let mut card_bg = if pal.glass { pal.bg } else { pal.panel };
        card_bg.a = 1.0;
        let shadow = gpui::BoxShadow {
            color: gpui::black().opacity(SHADOW_ALPHA),
            offset: gpui::point(px(0.), px(10.)),
            blur_radius: px(30.),
            spread_radius: px(0.),
        };
        // La tarjeta entra con un fundido y un leve ascenso (GPUI no escala un `div`).
        let lift_px = self.fluid.lift(card_in, fluid::CARD_RISE);
        let card = div()
            .id("export-card")
            .relative()
            .top(px(lift_px))
            .opacity(card_in)
            .w(px(DIALOG_W))
            .max_w_full()
            .max_h_full()
            .flex()
            .flex_col()
            .rounded(px(pal.r_panel))
            .bg(card_bg)
            .border_1()
            .border_color(pal.border.opacity(0.6))
            .shadow(vec![shadow])
            .overflow_hidden()
            .text_color(pal.text)
            .font_family(pal.font)
            // Un clic adentro no cierra.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(div().flex_1().min_h_0().flex().flex_col().opacity(step_in).child(body))
            .child(div().flex_none().opacity(step_in).child(foot));
        Some(
            div()
                .id("export-overlay")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_start()
                .justify_center()
                // La tarjeta se ancla arriba: al cambiar de contenido (MP4 ↔ GIF, ajustes → avance → listo) solo se mueve
                // su borde de abajo, no el título ni la fila que el usuario tiene bajo el cursor.
                .pt(px(dialog_top(f32::from(window.viewport_size().height))))
                .px(px(DIALOG_MARGIN))
                .pb(px(DIALOG_MARGIN))
                .bg(gpui::black().opacity(SCRIM_ALPHA * scrim))
                .when(open, |el| el.on_mouse_down(MouseButton::Left, cx.listener(|view, _: &MouseDownEvent, _, cx| view.export_close(cx))))
                .child(card)
                // Mientras se desvanece no se puede apretar nada de la tarjeta.
                .when(!open, |el| el.child(div().absolute().inset_0().occlude()))
                .into_any_element(),
        )
    }

    /// Cierra el diálogo (no mientras exporta: ahí se oculta con su botón).
    fn export_close(&mut self, cx: &mut Context<Self>) {
        if self.export_busy() {
            return;
        }
        self.export_ui.open = false;
        cx.notify();
    }

    // --- Ajustes ---------------------------------------------------------------------------

    /// Cambia los ajustes de exportación: se guardan con el proyecto y quedan para la próxima.
    fn export_set(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ExportSettings)) {
        change(&mut self.project.export);
        self.changed_keyed("export", cx);
    }

    fn export_format(&mut self, to: Format, cx: &mut Context<Self>) {
        let mut settings = self.project.export;
        switch_format(&mut settings, &mut self.export_ui.stash, to);
        self.project.export = settings;
        self.export_ui.confirm = None;
        self.export_default_name(cx);
        self.changed_keyed("export", cx);
    }

    /// La carpeta de destino: la de «Guardar como…» o, si no, junto a la grabación.
    fn export_dir(&self) -> PathBuf {
        self.export_ui.dir.clone().or_else(|| self.path.parent().map(PathBuf::from)).unwrap_or_default()
    }

    /// `Nombre (editada).mp4`, sin pisar uno anterior; solo si el nombre no es del usuario.
    fn export_default_name(&mut self, cx: &mut Context<Self>) {
        if self.export_ui.name_edited {
            return;
        }
        let recording = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = unique_stem(&self.export_dir(), &recording, t("pill.editor.edited"), ext_of(self.project.export.format), |p| p.exists());
        self.export_ui.name.update(cx, |field, cx| field.set_text(stem, cx));
    }

    /// Dónde se guarda: la carpeta y el nombre escrito con su terminación; `None` si no hay nombre.
    fn export_target(&self, cx: &gpui::App) -> Option<PathBuf> {
        let ext = ext_of(self.project.export.format);
        let stem = clean_stem(self.export_ui.name.read(cx).text(), ext)?;
        Some(self.export_dir().join(format!("{stem}.{ext}")))
    }

    /// Los archivos que exportar no puede pisar: la grabación que se ve y el máster
    /// del que sale la imagen (con «Cursor suave» son dos).
    fn export_keeps(&self) -> Vec<PathBuf> {
        let mut keep = vec![self.path.clone()];
        if self.source != self.path {
            keep.push(self.source.clone());
        }
        keep
    }

    /// Si `target` es la grabación o el máster: exportar ahí destruiría el original y
    /// dejaría el proyecto apuntando a otros tiempos.
    fn export_is_source(&self, target: &Path) -> bool {
        self.export_keeps().iter().any(|kept| same_file(target, kept))
    }

    /// Si `target` ya existe y todavía no se confirmó reemplazarlo.
    fn export_needs_confirm(&self, target: &Path) -> bool {
        !self.export_is_source(target) && target.exists() && self.export_ui.confirm.as_ref().map_or(true, |sure| !same_file(sure, target))
    }

    /// «Guardar como…»: el selector de archivos del sistema.
    fn export_save_as(&mut self, cx: &mut Context<Self>) {
        let ext = ext_of(self.project.export.format);
        let stem = clean_stem(self.export_ui.name.read(cx).text(), ext).unwrap_or_default();
        let picked = cx.prompt_for_new_path(&self.export_dir(), Some(&format!("{stem}.{ext}")));
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(path))) = picked.await else { return };
            let _ = view.update(cx, |view, cx| {
                let ext = ext_of(view.project.export.format);
                if let Some(dir) = path.parent() {
                    view.export_ui.dir = Some(dir.to_path_buf());
                }
                let stem = path.file_stem().map(|s| clean_stem(&s.to_string_lossy(), ext).unwrap_or_default()).unwrap_or_default();
                view.export_ui.name.update(cx, |field, cx| field.set_text(stem, cx));
                view.export_ui.name_edited = true;
                view.export_ui.confirm = None;
                cx.notify();
            });
        })
        .detach();
    }

    // --- Exportar ---------------------------------------------------------------------------

    fn start_export(&mut self, cx: &mut Context<Self>) {
        if self.export_busy() {
            return;
        }
        let Some(out) = self.export_target(cx) else {
            self.export_ui.run = Some(Run::Failed { error: t("pill.editor.exportNoName").into() });
            cx.notify();
            return;
        };
        // Nunca encima de la grabación ni del máster: el proyecto apunta a ese
        // archivo y su tiempo dejaría de corresponder.
        if self.export_is_source(&out) {
            let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.export_ui.run = Some(Run::Failed { error: tf("pill.editor.exportIsSource", &[("name", &name)]) });
            cx.notify();
            return;
        }
        // Un archivo que ya está se reemplaza solo tras un segundo clic: el nombre
        // puede ser de otra cosa.
        if self.export_needs_confirm(&out) {
            self.export_ui.confirm = Some(out);
            cx.notify();
            return;
        }
        self.export_ui.confirm = None;
        let segments = self.project.segments(self.duration);
        if segments.is_empty() {
            self.export_ui.run = Some(Run::Failed { error: t("pill.editor.exportNothing").into() });
            cx.notify();
            return;
        }
        self.player.pause();
        self.close_preview();
        self.export_ui.note = None;
        self.export_ui.pulse_at = None;
        self.export_ui.run_id += 1;
        let settings = self.project.export;
        let format = settings.format;
        let progress = Arc::new(AtomicU64::new(0f64.to_bits()));
        let cancel = Arc::new(AtomicBool::new(false));
        let mut job = self.job(segments, progress.clone(), cancel.clone(), false);
        // En un GIF, el video es solo la primera mitad.
        if format == Format::Gif {
            let progress = progress.clone();
            job.progress = Box::new(move |p: f64| progress.store(overall(Format::Gif, Stage::Video, p).to_bits(), Ordering::Relaxed));
        }
        let plan = Plan {
            part: part_path(&out),
            gif: (format == Format::Gif).then(|| (gif_tmp_path(&self.path), gif_width(&settings), gif_fps(&settings))),
            out: out.clone(),
            keep: self.export_keeps(),
        };
        let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.export_ui.run = Some(Run::Running { progress: progress.clone(), cancel: cancel.clone(), format, name });
        let source = self.source.clone();
        cx.spawn(async move |view, cx| {
            let (worker_progress, worker_cancel) = (progress.clone(), cancel.clone());
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Un pánico del exportador (objc2 verifica los selectores en debug) no debe
                    // dejar el diálogo esperando para siempre; los temporales se borran al desenrollar.
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_export(&source, job, &plan, &worker_progress, &worker_cancel)))
                        .unwrap_or_else(|_| Err(t("pill.editor.exportCrashed").to_string()))
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.export_ui.run = match result {
                    Ok(bytes) => Some(Run::Done { path: out, bytes }),
                    // Cancelar no es un error: se vuelve a los ajustes.
                    Err(_) if cancel.load(Ordering::Relaxed) => None,
                    Err(error) => {
                        tracing::warn!(%error, "editor: no se pudo exportar");
                        Some(Run::Failed { error })
                    }
                };
                view.export_default_name(cx);
                view.export_finished_hidden(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Si la exportación termina con el diálogo oculto, nada lo dice: un aviso (con «Mostrar» o «Ver») y un pulso del
    /// botón de arriba. Con el diálogo a la vista, el propio diálogo cambia de paso.
    fn export_finished_hidden(&mut self, cx: &mut Context<Self>) {
        if self.export_ui.open {
            return;
        }
        match &self.export_ui.run {
            Some(Run::Done { path, .. }) => {
                let (name, path) = (path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path.clone());
                self.export_ui.pulse_at = Some(Instant::now());
                self.say_with(tf("pill.editor.exportedNamed", &[("name", &name)]), false, Some(super::NoticeAction::Reveal(path)), cx);
            }
            Some(Run::Failed { .. }) => self.say_with(t("pill.editor.exportFailedNotice"), true, Some(super::NoticeAction::ShowExport), cx),
            _ => {}
        }
    }

    fn export_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(Run::Running { cancel, .. }) = &self.export_ui.run {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// De vuelta a los ajustes («Otra exportación», «Cambiar ajustes»); el nombre se rehace
    /// (lo recién exportado ya existe: sigue «(editada 2)»).
    fn export_again(&mut self, cx: &mut Context<Self>) {
        self.export_ui.run = None;
        self.export_ui.note = None;
        self.export_ui.name_edited = false;
        self.export_ui.confirm = None;
        self.export_default_name(cx);
        cx.notify();
    }

    fn export_copy(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.export_ui.note = Some(match crate::clip_image::write_files(&[path.to_string_lossy().into_owned()]) {
            Ok(()) => t("pill.editor.exportCopied").into(),
            Err(error) => tf("pill.editor.exportCopyFailed", &[("error", &error)]).into(),
        });
        cx.notify();
    }

    // --- El trabajo del exportador ---------------------------------------------------------

    /// Lo que se exporta de `segments`: el proyecto entero en Core Image. `preview`: la vista
    /// previa de «Ver cómo queda» (más chica, a 30 cuadros y de calidad baja; lo demás igual).
    /// La escena y el `Decor` salen de `scene.rs`, el mismo camino de la vista previa del editor.
    pub(super) fn job(&self, segments: Vec<(f64, f64)>, progress: Arc<AtomicU64>, cancel: Arc<AtomicBool>, preview: bool) -> cap::Export {
        let motion = self.motion.clone();
        let project = self.project.clone();
        let cursor_drawn = self.cursor_drawn;
        let source = self.player.size();
        let cursor_px = cursor_px_of(self.track.as_ref(), source.0, project.cursor.size);
        // De la escena solo se usan el marco, los textos, el cursor y el aspecto: el exportador
        // saca el tamaño del cuadro de `options`.
        let scene = SceneInputs { project: &project, motion: &motion, cursor_drawn, cursor_px, src: source }.scene_for(render_dims(&project.export, source));
        let speeds = clip_speeds(&project.speed_spans(self.duration), &segments);
        cap::Export {
            segments,
            speeds,
            texts: scene.texts,
            cursor_px: scene.cursor_px,
            frame: scene.frame,
            look: scene.look,
            options: export_options(&project.export, source, preview),
            decor: Box::new(move |src, _| SceneInputs { project: &project, motion: &motion, cursor_drawn, cursor_px, src: source }.decor_at(src)),
            progress: Box::new(move |p| progress.store(p.to_bits(), Ordering::Relaxed)),
            cancel,
        }
    }

    // --- El dibujo del diálogo ----------------------------------------------------------------

    /// Una opción con la proporción dibujada: un cuadrito con la forma real del cuadro.
    fn aspect_chip(&self, pal: &Ed, aspect: Aspect, source: (u32, u32), on: bool, cx: &mut Context<Self>) -> AnyElement {
        let (w, h) = aspect.ratio(source);
        let k = ASPECT_BOX / w.max(h).max(1) as f32;
        let (rw, rh) = ((w as f32 * k).max(4.0), (h as f32 * k).max(4.0));
        let (sel, hover) = (pal.sel, pal.hover);
        let id: SharedString = format!("export-aspect-{aspect:?}").into();
        let on_t = self.fluid.toggle(id.clone(), on, fluid::CHIP_FADE, fluid::CHIP_FADE);
        let color = mix(pal.sub, pal.text, on_t);
        let label: SharedString = match aspect {
            Aspect::Source => t("pill.editor.exportOriginal").into(),
            Aspect::Wide => "16:9".into(),
            Aspect::Vertical => "9:16".into(),
            Aspect::Square => "1:1".into(),
            Aspect::Portrait => "4:5".into(),
        };
        let el = div()
            .id(id.clone())
            .h(px(ASPECT_H))
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(5.))
            .rounded(px(pal.r_chip.min(14.0)))
            .cursor_pointer()
            .text_size(px(T_BODY))
            .text_color(color)
            .when(on, |el| el.font_weight(gpui::FontWeight::MEDIUM))
            .child(
                div().h(px(ASPECT_BOX)).flex().items_center().child(
                    div().w(px(rw)).h(px(rh)).rounded(px(2.)).border_1().border_color(color).bg(color.opacity(0.22 * on_t)),
                ),
            )
            .child(label);
        self.interactive(pal.accent, el, move |view, _, cx| view.export_set(cx, move |s| s.aspect = aspect), cx)
            .fx(SharedString::from(format!("{id}-fx")), move |el, h| el.bg(fluid::select_bg(h, on_t, hover, sel)))
            .into_any_element()
    }

    /// Los ajustes y, al final, el resumen y el destino.
    fn export_settings_body(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let s = self.project.export;
        let source = self.player.size();
        let mut rows: Vec<AnyElement> = Vec::new();

        rows.push(opt_row(
            pal,
            t("pill.editor.exportFormat"),
            [(Format::Mp4, "MP4"), (Format::Gif, "GIF")]
                .into_iter()
                .map(|(format, label)| self.chip(pal, format!("export-format-{label}").into(), label.into(), s.format == format, move |view, _, cx| view.export_format(format, cx), cx))
                .collect(),
        ));
        rows.push(opt_row(pal, t("pill.editor.exportAspect"), Aspect::ALL.into_iter().map(|a| self.aspect_chip(pal, a, source, s.aspect == a, cx)).collect()));

        match s.format {
            Format::Mp4 => {
                let current = normalized_height(s.height, source.1);
                let mut heights = vec![self.chip(pal, "export-height-source".into(), t("pill.editor.exportOriginal").into(), current.is_none(), |view, _, cx| view.export_set(cx, |s| s.height = None), cx)];
                // Solo las que bajan del original: una resolución mayor no agranda nada.
                for h in HEIGHTS.into_iter().filter(|h| *h < source.1) {
                    heights.push(self.chip(pal, format!("export-height-{h}").into(), format!("{h}p").into(), current == Some(h), move |view, _, cx| view.export_set(cx, move |s| s.height = Some(h)), cx));
                }
                rows.push(opt_row(pal, t("pill.editor.exportResolution"), heights));

                let qualities = [
                    (Quality::Low, t("pill.editor.exportQualityLow")),
                    (Quality::Standard, t("pill.editor.exportQualityStandard")),
                    (Quality::High, t("pill.editor.exportQualityHigh")),
                    (Quality::Max, t("pill.editor.exportQualityMax")),
                ];
                rows.push(opt_row(
                    pal,
                    t("pill.editor.exportQuality"),
                    qualities
                        .into_iter()
                        .map(|(q, label)| self.chip(pal, format!("export-quality-{q:?}").into(), label.into(), s.quality == q, move |view, _, cx| view.export_set(cx, move |s| s.quality = q), cx))
                        .collect(),
                ));

                rows.push(opt_row(
                    pal,
                    t("pill.editor.exportCodec"),
                    [(Codec::H264, "H.264"), (Codec::Hevc, "HEVC")]
                        .into_iter()
                        .map(|(c, label)| self.chip(pal, format!("export-codec-{c:?}").into(), label.into(), s.codec == c, move |view, _, cx| view.export_set(cx, move |s| s.codec = c), cx))
                        .collect(),
                ));
                rows.push(note(pal, if s.codec == Codec::Hevc { t("pill.editor.exportCodecHevcNote") } else { t("pill.editor.exportCodecH264Note") }));

                rows.push(opt_row(
                    pal,
                    t("pill.editor.exportFps"),
                    VIDEO_FPS
                        .into_iter()
                        .map(|f| self.chip(pal, format!("export-fps-{f}").into(), f.to_string().into(), s.fps == f, move |view, _, cx| view.export_set(cx, move |s| s.fps = f), cx))
                        .collect(),
                ));
            }
            Format::Gif => {
                let width = gif_width(&s);
                rows.push(opt_row(
                    pal,
                    t("pill.editor.exportMaxWidth"),
                    GIF_WIDTHS
                        .into_iter()
                        .map(|w| self.chip(pal, format!("export-gifw-{w}").into(), format!("{w} px").into(), width == w, move |view, _, cx| view.export_set(cx, move |s| s.height = Some(w)), cx))
                        .collect(),
                ));
                let fps = gif_fps(&s);
                rows.push(opt_row(
                    pal,
                    t("pill.editor.exportFps"),
                    GIF_FPS
                        .into_iter()
                        .map(|f| self.chip(pal, format!("export-giff-{f}").into(), f.to_string().into(), fps == f, move |view, _, cx| view.export_set(cx, move |s| s.fps = f), cx))
                        .collect(),
                ));
            }
        }

        // El resumen vivo.
        let seconds = self.project.out_length(self.duration);
        let (w, h) = render_dims(&s, source);
        let bytes = estimate_bytes(&s, source, seconds);
        let mut summary = div().flex().flex_col().gap(px(8.)).p(px(12.)).rounded(px(pal.r_ctl.min(SUMMARY_RADIUS_MAX))).bg(pal.control.opacity(0.45)).child(
            div()
                .flex()
                .gap(px(12.))
                .child(stat(pal, t("pill.editor.exportLength"), clock(seconds)))
                .child(stat(pal, t("pill.editor.exportDimensions"), format!("{w} × {h} px")))
                .child(stat(pal, t("pill.editor.exportEstimate"), format!("≈ {}", human_bytes(bytes)))),
        );
        if s.format == Format::Gif {
            if seconds > GIF_LONG_SECS {
                summary = summary.child(div().text_size(px(T_SMALL)).text_color(pal.click).child(t("pill.editor.exportGifLong")));
            }
            summary = summary.child(div().text_size(px(T_SMALL)).text_color(pal.sub).child(t("pill.editor.exportGifNote")));
        }

        // El destino.
        let ext = ext_of(s.format);
        let target = self.export_target(cx);
        let field = div()
            .flex_1()
            .min_w_0()
            .h(px(34.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(2.))
            .rounded(px(pal.r_ctl))
            .bg(pal.control.opacity(0.7))
            .border_1()
            .border_color(pal.border.opacity(0.6))
            .text_size(px(T_CONTROL))
            .child(self.export_ui.name.clone())
            .child(div().flex_none().text_color(pal.sub).child(format!(".{ext}")));
        let dir = self.export_dir();
        let folder = if self.export_ui.dir.is_some() {
            tf("pill.editor.exportIn", &[("dir", &short_dir(&dir))])
        } else {
            tf("pill.editor.exportInSame", &[("dir", &short_dir(&dir))])
        };
        let reset = self
            .export_ui
            .dir
            .is_some()
            .then(|| self.button(pal, "export-dir-reset", "icons/undo.svg", None, t("pill.editor.exportDirResetTip"), |view, _, cx| view.export_dir_reset(cx), cx));
        let warning = match &target {
            None => Some(t("pill.editor.exportNoName").to_string()),
            Some(path) if self.export_is_source(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Some(tf("pill.editor.exportIsSource", &[("name", &name)]))
            }
            Some(path) if path.exists() => Some(t("pill.editor.exportExists").to_string()),
            Some(_) => None,
        };
        let destination = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(heading(pal, t("pill.editor.exportDestination")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field)
                    .child(self.button(pal, "export-save-as", "icons/folder-open.svg", Some(t("pill.editor.exportSaveAs")), t("pill.editor.exportSaveAsTip"), |view, _, cx| view.export_save_as(cx), cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(svg().path("icons/folder.svg").flex_none().size(px(13.)).text_color(pal.faint))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(T_SMALL)).text_color(pal.sub).child(folder))
                    .children(reset),
            )
            .children(warning.map(|text| div().text_size(px(T_SMALL)).text_color(pal.click).child(text)));

        div()
            .id("export-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(18.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .children(rows)
            .child(heading(pal, t("pill.editor.exportSummary")))
            .child(summary)
            .child(destination)
            .into_any_element()
    }

    fn export_dir_reset(&mut self, cx: &mut Context<Self>) {
        self.export_ui.dir = None;
        self.export_ui.confirm = None;
        self.export_default_name(cx);
        cx.notify();
    }

    fn export_settings_footer(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        // Si el destino ya existe, el botón cambia a «Reemplazar»: el segundo clic
        // es la confirmación (y el nombre pudo ser de otra cosa).
        let replace = self.export_target(cx).is_some_and(|target| self.export_needs_confirm(&target));
        let (label, tip): (SharedString, &'static str) = if replace {
            (t("pill.editor.exportReplace").into(), t("pill.editor.exportReplaceTip"))
        } else {
            (t("pill.editor.export").into(), t("pill.editor.exportStartTip"))
        };
        footer(vec![
            self.button(pal, "export-cancel", "icons/x.svg", Some(t("pill.editor.exportClose")), t("pill.editor.exportCloseTip"), |view, _, cx| view.export_close(cx), cx),
            self.primary(pal, "export-start", "icons/download.svg", label, tip, |view, _, cx| view.start_export(cx), cx),
        ])
    }

    fn export_running_body(&self, pal: &Ed, p: f64, format: Format, name: &str, cancelling: bool, cancel_t: f32) -> AnyElement {
        let status = if cancelling {
            t("pill.editor.exportCancelling")
        } else if format == Format::Gif && p >= 0.5 {
            t("pill.editor.exportMakingGif")
        } else {
            t("pill.editor.exportPreparing")
        };
        let pct = percent(p);
        div()
            .flex_none()
            .px(px(18.))
            .pt(px(8.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(div().text_size(px(T_CONTROL)).child(status))
                    .child(div().text_size(px(T_CONTROL)).font_family(pal.mono).text_color(mix(pal.sub, pal.faint, cancel_t)).child(format!("{pct} %"))),
            )
            .child(
                div()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .bg(pal.control)
                    .overflow_hidden()
                    .child(div().h_full().w(gpui::relative(p.clamp(0.0, 1.0) as f32)).rounded_full().bg(mix(pal.accent, pal.faint, cancel_t))),
            )
            .child(div().truncate().text_size(px(T_SMALL)).text_color(pal.sub).child(name.to_string()))
            .into_any_element()
    }

    fn export_running_footer(&self, pal: &Ed, cancelling: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut buttons = vec![self.button(pal, "export-hide", "icons/x.svg", Some(t("pill.editor.exportHide")), t("pill.editor.exportHideTip"), |view, _, cx| {
            view.export_ui.open = false;
            cx.notify();
        }, cx)];
        if !cancelling {
            buttons.push(self.button(pal, "export-stop", "icons/rotate-ccw.svg", Some(t("pill.editor.exportCancel")), t("pill.editor.cancelTip"), |view, _, cx| view.export_cancel(cx), cx));
        }
        footer(buttons)
    }

    fn export_done_body(&self, pal: &Ed, path: &Path, bytes: u64, cx: &mut Context<Self>) -> AnyElement {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (reveal, open, copy) = (path.to_path_buf(), path.to_path_buf(), path.to_path_buf());
        div()
            .flex_none()
            .px(px(18.))
            .pt(px(4.))
            .pb(px(18.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        // El único momento de énfasis del diálogo: la palomita crece con un leve rebote, una vez.
                        div().size(px(DONE_ICON)).flex_none().flex().items_center().justify_center().child(
                            svg().path("icons/circle-check.svg").flex_none().size(px(DONE_ICON * self.fluid.grow(self.fluid.since_with(self.export_ui.step.1, fluid::DONE_POP, crate::anim::ease_back_out), DONE_FROM))).text_color(pal.accent),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_size(px(T_CONTROL)).font_weight(gpui::FontWeight::MEDIUM).child(name))
                            .child(div().text_size(px(T_SMALL)).text_color(pal.sub).child(human_bytes(bytes))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(self.primary(pal, "export-reveal", "icons/folder.svg", t("pill.editor.exportReveal").into(), t("pill.editor.revealTip"), move |_, _, _| {
                        let _ = crate::paths::reveal(&reveal);
                    }, cx))
                    .child(self.button(pal, "export-open", "icons/external-link.svg", Some(t("pill.editor.exportOpen")), t("pill.editor.exportOpenFileTip"), move |_, _, _| {
                        let _ = crate::paths::open(&open);
                    }, cx))
                    .child(self.button(pal, "export-copy", "icons/copy.svg", Some(t("pill.editor.exportCopy")), t("pill.editor.exportCopyTip"), move |view, _, cx| view.export_copy(&copy, cx), cx)),
            )
            .children(self.export_ui.note.clone().map(|note| div().text_size(px(T_SMALL)).text_color(pal.sub).child(note)))
            .into_any_element()
    }

    fn export_done_footer(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        footer(vec![
            self.button(pal, "export-again", "icons/rotate-ccw.svg", Some(t("pill.editor.exportAgain")), t("pill.editor.exportAgainTip"), |view, _, cx| view.export_again(cx), cx),
            self.button(pal, "export-done-close", "icons/x.svg", Some(t("pill.editor.exportClose")), t("pill.editor.exportCloseTip"), |view, _, cx| view.export_close(cx), cx),
        ])
    }

    fn export_failed_body(&self, pal: &Ed, error: &str) -> AnyElement {
        div()
            .flex_none()
            .px(px(18.))
            .pt(px(4.))
            .pb(px(18.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_size(px(T_CONTROL)).text_color(pal.sub).child(t("pill.editor.exportFailedHint")))
            .child(
                div()
                    .p(px(10.))
                    .rounded(px(pal.r_ctl))
                    .bg(pal.control.opacity(0.55))
                    .font_family(pal.mono)
                    .text_size(px(T_SMALL))
                    .text_color(pal.click)
                    .child(error.to_string()),
            )
            .into_any_element()
    }

    fn export_failed_footer(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        footer(vec![
            self.button(pal, "export-back", "icons/undo.svg", Some(t("pill.editor.exportBack")), t("pill.editor.exportBackTip"), |view, _, cx| view.export_again(cx), cx),
            self.primary(pal, "export-retry", "icons/rotate-cw.svg", t("pill.editor.exportRetry").into(), t("pill.editor.exportRetryTip"), |view, _, cx| {
                view.export_ui.run = None;
                view.start_export(cx);
            }, cx),
        ])
    }
}

// --- Piezas sueltas -----------------------------------------------------------------------

/// Una fila: el nombre a la izquierda y sus opciones a la derecha.
fn opt_row(pal: &Ed, label: &'static str, chips: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(div().w(px(LABEL_W)).flex_none().text_size(px(T_BODY)).text_color(pal.sub).child(label))
        .child(div().flex_1().min_w_0().flex().gap(px(4.)).p(px(3.)).rounded(px(pal.r_chip.min(17.0))).bg(pal.control.opacity(0.55)).children(chips))
        .into_any_element()
}

/// Una nota bajo una fila, alineada con sus opciones.
fn note(pal: &Ed, text: &'static str) -> AnyElement {
    div().pl(px(LABEL_W + 12.)).text_size(px(T_SMALL)).text_color(pal.sub).child(text).into_any_element()
}

fn heading(pal: &Ed, text: &'static str) -> AnyElement {
    div().pt(px(6.)).text_size(px(T_SMALL)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(pal.sub).child(text.to_uppercase()).into_any_element()
}

/// Un dato del resumen: su nombre y su valor.
fn stat(pal: &Ed, label: &'static str, value: String) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().text_size(px(T_SMALL)).text_color(pal.sub).child(label))
        .child(div().truncate().text_size(px(T_CONTROL)).font_weight(gpui::FontWeight::MEDIUM).text_color(pal.text).child(value))
        .into_any_element()
}

/// El pie del diálogo: los botones a la derecha.
fn footer(buttons: Vec<AnyElement>) -> AnyElement {
    div().flex_none().flex().items_center().justify_end().gap(px(6.)).px(px(14.)).pt(px(6.)).pb(px(14.)).children(buttons).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(format: Format) -> ExportSettings {
        ExportSettings { format, ..ExportSettings::default() }
    }

    #[test]
    fn el_mismo_archivo_se_reconoce_aunque_la_ruta_se_escriba_distinto() {
        let dir = std::env::temp_dir().join(format!("atic-export-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("toma.mp4");
        std::fs::write(&file, b"x").unwrap();
        assert!(same_file(&file, &dir.join("./toma.mp4")));
        assert!(same_file(&file, &dir.join(".").join("toma.mp4")));
        assert!(!same_file(&file, &dir.join("otra.mp4")));
        // Una que todavía no existe se compara por su carpeta y su nombre.
        assert!(same_file(&dir.join("nueva.mp4"), &dir.join("./nueva.mp4")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_ancho_del_gif_da_el_alto_par_de_la_proporcion() {
        assert_eq!(gif_stage_height(640, (16, 9)), 360);
        assert_eq!(gif_stage_height(800, (9, 16)), 1422);
        assert_eq!(gif_stage_height(480, (1, 1)), 480);
        assert_eq!(gif_stage_height(480, (4, 5)), 600);
        // Una proporción del original rara: siempre par y nunca menos de 2.
        assert_eq!(gif_stage_height(640, (3440, 1440)) % 2, 0);
        assert_eq!(gif_stage_height(0, (16, 9)), 2);
    }

    #[test]
    fn el_cuadro_de_salida_repite_el_del_exportador() {
        // Sin tope ni proporción: el del original tal cual.
        assert_eq!(out_dims((1920, 1080), None, None), (1920, 1080));
        // Un tope menor baja el alto y el ancho sigue la proporción del original.
        assert_eq!(out_dims((1920, 1080), Some(720), None), (1280, 720));
        // Nunca agranda.
        assert_eq!(out_dims((1280, 720), Some(1080), None), (1280, 720));
        // Vertical a partir de un original apaisado: el alto es el del original.
        assert_eq!(out_dims((1920, 1080), None, Some((9, 16))), (606, 1080));
        assert_eq!(out_dims((1920, 1080), Some(720), Some((1, 1))), (720, 720));
    }

    #[test]
    fn el_cuadro_de_un_gif_cabe_en_su_ancho() {
        let mut s = settings(Format::Gif);
        s.height = Some(640);
        assert_eq!(render_dims(&s, (1920, 1080)), (640, 360));
        s.aspect = Aspect::Vertical;
        let (w, h) = render_dims(&s, (1920, 1080));
        assert!(w <= 640, "{w}");
        assert_eq!(h, 1080);
        // Un original más angosto que el ancho pedido no se agranda.
        s.aspect = Aspect::Source;
        assert_eq!(render_dims(&s, (480, 270)), (480, 270));
    }

    #[test]
    fn las_opciones_de_un_mp4_salen_de_los_ajustes() {
        let s = ExportSettings { format: Format::Mp4, aspect: Aspect::Vertical, height: Some(720), quality: Quality::Max, codec: Codec::Hevc, fps: 30 };
        let o = export_options(&s, (1920, 1080), false);
        assert_eq!((o.codec, o.quality, o.max_height, o.fps, o.aspect), (cap::Codec::Hevc, cap::Quality::Max, Some(720), 30, Some((9, 16))));
        // La proporción del original no se pide.
        let o = export_options(&ExportSettings::default(), (1920, 1080), false);
        assert_eq!((o.codec, o.quality, o.max_height, o.fps, o.aspect), (cap::Codec::H264, cap::Quality::Standard, Some(1080), 60, None));
    }

    #[test]
    fn la_vista_previa_es_chica_y_baja_pero_conserva_lo_demas() {
        let s = ExportSettings { format: Format::Gif, aspect: Aspect::Square, height: Some(800), quality: Quality::Max, codec: Codec::Hevc, fps: 20 };
        let o = export_options(&s, (1920, 1080), true);
        assert_eq!((o.codec, o.quality, o.max_height, o.fps, o.aspect), (cap::Codec::Hevc, cap::Quality::Low, Some(540), 30, Some((1, 1))));
    }

    #[test]
    fn un_gif_exporta_primero_un_video_del_ancho_pedido() {
        let s = ExportSettings { format: Format::Gif, aspect: Aspect::Source, height: Some(800), quality: Quality::Low, codec: Codec::Hevc, fps: 20 };
        let o = export_options(&s, (1920, 1080), false);
        assert_eq!((o.codec, o.quality, o.max_height, o.fps, o.aspect), (cap::Codec::H264, GIF_STAGE_QUALITY, Some(450), 20, None));
        // Valores fuera de la lista (un `.edit.json` viejo o a mano) caen a los de siempre.
        let s = ExportSettings { format: Format::Gif, height: None, fps: 60, ..ExportSettings::default() };
        let o = export_options(&s, (1920, 1080), false);
        assert_eq!((o.max_height, o.fps), (Some(360), 15));
    }

    #[test]
    fn cambiar_de_formato_guarda_lo_elegido_en_cada_uno() {
        let mut s = ExportSettings { height: Some(720), fps: 30, ..ExportSettings::default() };
        let mut stash = None;
        switch_format(&mut s, &mut stash, Format::Gif);
        assert_eq!((s.format, s.height, s.fps), (Format::Gif, Some(640), 15));
        s.height = Some(800);
        switch_format(&mut s, &mut stash, Format::Mp4);
        assert_eq!((s.format, s.height, s.fps), (Format::Mp4, Some(720), 30));
        switch_format(&mut s, &mut stash, Format::Gif);
        assert_eq!((s.format, s.height, s.fps), (Format::Gif, Some(800), 15));
        // El mismo formato no toca nada.
        switch_format(&mut s, &mut stash, Format::Gif);
        assert_eq!((s.height, s.fps), (Some(800), 15));
    }

    #[test]
    fn las_velocidades_se_recortan_a_los_tramos_que_se_exportan() {
        let spans = [(2.0, 6.0, 2.0), (10.0, 12.0, 4.0)];
        // Los 4 s de «Ver cómo queda» desde el segundo 4.
        let out = clip_speeds(&spans, &[(4.0, 8.0)]);
        assert_eq!(out, vec![cap::SpeedSpan { start: 4.0, end: 6.0, rate: 2.0 }]);
        // Un tramo con velocidad que cruza un corte queda en dos.
        let out = clip_speeds(&[(1.0, 9.0, 3.0)], &[(0.0, 3.0), (5.0, 10.0)]);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].start, out[0].end, out[1].start, out[1].end), (1.0, 3.0, 5.0, 9.0));
        // Lo que no cae adentro, o es una pizca, no cuenta.
        assert!(clip_speeds(&spans, &[(7.0, 9.0)]).is_empty());
        assert!(clip_speeds(&[(5.995, 6.0, 2.0)], &[(0.0, 10.0)]).is_empty());
    }

    #[test]
    fn el_bitrate_sigue_la_formula_del_exportador() {
        // 1920 × 1080 × 30 × 0,12 = 7 464 960 bps.
        assert_eq!(video_bitrate((1920, 1080), Quality::Standard, Codec::H264), 7_464_960.0);
        // HEVC, 0,6 de eso.
        assert_eq!(video_bitrate((1920, 1080), Quality::Standard, Codec::Hevc), (7_464_960.0f64 * 0.6).round());
        // El tope de 60 Mbps y el piso de 2 Mbps.
        assert_eq!(video_bitrate((3840, 2160), Quality::Max, Codec::H264), 60_000_000.0);
        assert_eq!(video_bitrate((640, 360), Quality::Low, Codec::H264), 2_000_000.0);
        // Las calidades suben de a poco.
        let rates = [Quality::Low, Quality::Standard, Quality::High, Quality::Max].map(|q| video_bitrate((1920, 1080), q, Codec::H264));
        assert!(rates.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn el_peso_estimado_de_un_mp4_suma_el_audio() {
        let s = ExportSettings::default();
        // (7 464 960 + 160 000) bps durante 10 s = 9 531 200 bytes.
        assert_eq!(estimate_bytes(&s, (1920, 1080), 10.0), 9_531_200);
        assert_eq!(estimate_bytes(&s, (1920, 1080), 0.0), 0);
        // Un cuadro más chico pesa menos.
        let small = ExportSettings { height: Some(720), ..s };
        assert!(estimate_bytes(&small, (1920, 1080), 10.0) < estimate_bytes(&s, (1920, 1080), 10.0));
    }

    #[test]
    fn el_peso_estimado_de_un_gif_crece_con_los_cuadros() {
        let s = ExportSettings { format: Format::Gif, height: Some(640), fps: 15, ..ExportSettings::default() };
        // 640 × 360 × 0,25 = 57 600 bytes por cuadro; 15 fps cuenta cuadros de 7 centésimas
        // (14,3 por segundo): 10 s son 143 cuadros.
        assert_eq!(gif_frames(10.0, 15), 143.0);
        assert_eq!(estimate_bytes(&s, (1920, 1080), 10.0), 57_600 * 143);
        // Pasado el tope de cuadros del exportador, no crece más.
        assert_eq!(gif_frames(1000.0, 20), GIF_MAX_FRAMES);
    }

    #[test]
    fn el_avance_de_un_gif_reparte_mitad_y_mitad() {
        assert_eq!(overall(Format::Mp4, Stage::Video, 0.4), 0.4);
        assert_eq!(overall(Format::Gif, Stage::Video, 0.0), 0.0);
        assert_eq!(overall(Format::Gif, Stage::Video, 1.0), 0.5);
        assert_eq!(overall(Format::Gif, Stage::Gif, 0.0), 0.5);
        assert!((overall(Format::Gif, Stage::Gif, 0.4) - 0.7).abs() < 1e-12);
        assert_eq!(overall(Format::Gif, Stage::Gif, 1.0), 1.0);
        // Valores raros no rompen la barra.
        assert_eq!(overall(Format::Gif, Stage::Gif, 3.0), 1.0);
        assert_eq!(overall(Format::Mp4, Stage::Video, f64::NAN), 0.0);
    }

    #[test]
    fn el_nombre_escrito_se_limpia() {
        assert_eq!(clean_stem("demo", "mp4").as_deref(), Some("demo"));
        assert_eq!(clean_stem("  demo  ", "mp4").as_deref(), Some("demo"));
        // La terminación escrita a mano no se duplica (con otra, queda como parte del nombre).
        assert_eq!(clean_stem("demo.MP4", "mp4").as_deref(), Some("demo"));
        assert_eq!(clean_stem("demo.mp4", "gif").as_deref(), Some("demo.mp4"));
        // Lo que rompe una ruta se cambia; un punto al comienzo la ocultaría.
        assert_eq!(clean_stem("a/b\\c:d", "mp4").as_deref(), Some("a-b-c-d"));
        assert_eq!(clean_stem(".oculto", "mp4").as_deref(), Some("oculto"));
        assert_eq!(clean_stem("   ", "mp4"), None);
        assert_eq!(clean_stem(".mp4", "mp4"), None);
        assert_eq!(clean_stem("ñandú (editada)", "gif").as_deref(), Some("ñandú (editada)"));
        assert_eq!(clean_stem(&"x".repeat(300), "mp4").map(|s| s.len()), Some(MAX_STEM_BYTES));
        // Con emoji (4 bytes cada uno) el tope es de bytes y nunca parte un carácter.
        let long = clean_stem(&"😀".repeat(100), "mp4").unwrap();
        assert!(long.len() <= MAX_STEM_BYTES && long.chars().all(|c| c == '😀'));
        assert_eq!(truncate_bytes("añb", 2), "a");
    }

    #[test]
    fn el_nombre_por_defecto_no_pisa_uno_anterior() {
        let dir = Path::new("/v");
        let none = |_: &Path| false;
        assert_eq!(unique_stem(dir, "Grabación 1", "editada", "mp4", none), "Grabación 1 (editada)");
        let taken = |p: &Path| p == Path::new("/v/Grabación 1 (editada).mp4") || p == Path::new("/v/Grabación 1 (editada 2).mp4");
        assert_eq!(unique_stem(dir, "Grabación 1", "editada", "mp4", taken), "Grabación 1 (editada 3)");
        // Otra terminación es otro archivo.
        assert_eq!(unique_stem(dir, "Grabación 1", "editada", "gif", taken), "Grabación 1 (editada)");
        // Un `exists` que dice siempre que sí no cuelga.
        let _ = unique_stem(dir, "x", "editada", "mp4", |_| true);
    }

    #[test]
    fn los_temporales_quedan_junto_al_destino_y_a_la_grabacion() {
        assert_eq!(part_path(Path::new("/v/Grabación (editada).mp4")), Path::new("/v/.Grabación (editada).atic-part.mp4"));
        assert_eq!(part_path(Path::new("/v/x.gif")), Path::new("/v/.x.atic-part.gif"));
        let tmp = gif_tmp_path(Path::new("/v/Grabación 1.mp4"));
        assert_eq!(tmp.parent(), Some(Path::new("/v/.atic")));
        let name = tmp.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Grabación 1.gif-tmp-") && name.ends_with(".mp4"));
        assert_ne!(tmp, gif_tmp_path(Path::new("/v/Grabación 1.mp4")));
    }

    #[test]
    fn el_peso_y_el_tiempo_se_leen_bien() {
        assert_eq!(human_bytes_in(512, true), "512 B");
        assert_eq!(human_bytes_in(820_000, true), "820 KB");
        assert_eq!(human_bytes_in(1_500_000, true), "1.5 MB");
        assert_eq!(human_bytes_in(38_400_000, true), "38.4 MB");
        assert_eq!(human_bytes_in(412_000_000, false), "412 MB");
        assert_eq!(human_bytes_in(2_340_000_000, false), "2,3 GB");
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(3723.0), "1:02:03");
    }

    #[test]
    fn la_carpeta_se_acorta_con_la_casa() {
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                assert_eq!(short_dir(&Path::new(&home).join("Movies/Atic")), "~/Movies/Atic");
            }
        }
        assert_eq!(short_dir(Path::new("/Volumes/Disco/Atic")), "/Volumes/Disco/Atic");
    }
}
