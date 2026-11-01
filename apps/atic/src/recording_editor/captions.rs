//! Subtítulos automáticos del editor: la sección «Subtítulos» de la barra
//! lateral y el trabajo que los genera. Todo pasa en este equipo, sin red.
//!
//! - **Motor:** el de voz que Atic ya tiene en el equipo. La pill no compila el
//!   Whisper de `atic-transcribe` (sin su feature `local` no carga), así que se
//!   usa lo mismo que Reuniones: el modelo local (`local_stt`, Parakeet) o la voz
//!   de macOS (`platform::apple`). Va primero el que eligió el usuario en
//!   `meeting_backend` y, si no está listo, el otro; Groq nunca, porque nada de
//!   esto debe salir a internet. El idioma es el de Ajustes (`whisper_language`).
//! - **Audio:** `extract_wav_16k` deja un WAV temporal que se lee de a trozos
//!   (una grabación de horas no cabe entera en memoria) y se borra siempre.
//! - **Resultado:** cada subtítulo es un `Text { caption: true, .. }` normal de
//!   la pista Texto: se elige, se mueve y se corrige ahí. Los tiempos son del
//!   video original; el exportador los mapea con cortes y velocidades.
//!
//! Lo puro (partir y agrupar frases, evitar solapes, recortar, el estilo) está
//! aparte, sin GPUI ni motor de voz, y con tests.

use std::cell::Cell;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{div, prelude::*, px, Context, Hsla, SharedString};

use super::palette::{Ed, T_SMALL};
use super::{RecordingEditor, Tone};
use crate::edit_project::{Text, TextAt};
use crate::i18n::{t, tf};

/// Lo más que cabe en una línea del subtítulo, en caracteres.
const MAX_LINE_CHARS: usize = 42;
/// Líneas por subtítulo.
const MAX_LINES: usize = 2;
/// Lo menos y lo más que dura un subtítulo en pantalla, en segundos.
const MIN_SECS: f64 = 1.2;
const MAX_SECS: f64 = 6.0;
/// Aire entre un subtítulo y el siguiente.
const GAP_SECS: f64 = 0.05;
/// Lo menos que se deja visible un subtítulo cuando el habla va muy seguida.
const MIN_VISIBLE: f64 = 0.4;
/// Un subtítulo que al recortar al largo del video queda con menos que esto, se descarta.
const MIN_KEPT: f64 = 0.1;

/// Cuánto audio se le pasa al motor de una vez: trozos de 30 s no piden más
/// memoria de la cuenta en una grabación de horas.
const CHUNK_SECS: usize = 30;
/// La cabecera RIFF que deja `extract_wav_16k` (PCM mono de 16 bits).
const WAV_HEADER: u64 = 44;
/// Cada cuánto se redibuja el avance.
const TICK: Duration = Duration::from_millis(250);
/// Cuánto vale lo que se supo del motor antes de volver a mirar (la comprobación
/// toca el disco y se hace al dibujar).
const ENGINE_TTL: Duration = Duration::from_secs(3);

/// Qué hace el trabajo ahora (lo lee la sección).
const PHASE_AUDIO: u8 = 0;
/// Transcribiendo con avance conocido.
const PHASE_SPEECH: u8 = 1;
/// Transcribiendo sin avance (la voz de macOS contesta al final).
const PHASE_SPEECH_OPEN: u8 = 2;

/// Los tamaños del estilo: S, M y L, como múltiplo del tamaño base del texto.
const SIZE_SMALL: f64 = 0.8;
const SIZE_MEDIUM: f64 = 1.0;
const SIZE_LARGE: f64 = 1.3;

// --- Lo puro --------------------------------------------------------------------

/// Un subtítulo listo: cuándo sale y qué dice.
#[derive(Clone, Debug, PartialEq)]
struct Caption {
    start: f64,
    end: f64,
    text: String,
}

/// Parte `text` en líneas de a lo más `max` caracteres, por palabras. Una
/// palabra más larga que `max` queda sola en su línea.
fn wrap_words(text: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut len = 0usize;
    for word in text.split_whitespace() {
        let w = word.chars().count();
        if len > 0 && len + 1 + w > max {
            lines.push(std::mem::take(&mut line));
            len = 0;
        }
        if len > 0 {
            line.push(' ');
            len += 1;
        }
        line.push_str(word);
        len += w;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Lo que Whisper y compañía anotan sobre el silencio o los sonidos
/// («[música]», «(risas)», «...»): no es habla y no se subtitula.
fn is_noise(text: &str) -> bool {
    let t = text.trim();
    if !t.chars().any(char::is_alphanumeric) {
        return true;
    }
    (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')')) || (t.starts_with('*') && t.ends_with('*'))
}

/// Parte una frase de `start` a `end` en subtítulos de `MAX_LINES` líneas de
/// `MAX_LINE_CHARS`. Si no cabe en uno, lo divide proporcional al largo de cada
/// parte, de modo que los tramos se siguen sin huecos de `start` a `end`. El
/// texto de cada parte va con espacios: el exportador y la vista previa lo
/// parten según el ancho real del cuadro.
fn split_segment(start: f64, end: f64, text: &str) -> Vec<(f64, f64, String)> {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lines = wrap_words(&clean, MAX_LINE_CHARS);
    let pages: Vec<String> = lines.chunks(MAX_LINES).map(|group| group.join(" ")).collect();
    let weights: Vec<usize> = pages.iter().map(|page| page.chars().filter(|c| !c.is_whitespace()).count().max(1)).collect();
    let total: usize = weights.iter().sum();
    let span = (end - start).max(0.0);
    let mut at = start;
    let mut spent = 0usize;
    pages
        .into_iter()
        .zip(weights)
        .map(|(page, weight)| {
            spent += weight;
            let to = if spent == total { start + span } else { start + span * spent as f64 / total as f64 };
            let part = (at, to, page);
            at = to;
            part
        })
        .collect()
}

/// De las frases del motor (inicio, fin, texto; en segundos del video) a
/// subtítulos de lectura cómoda: de `MIN_SECS` a `MAX_SECS` cada uno, sin pisarse
/// (con `GAP_SECS` de aire) y recortados al largo `duration` del video. Si el
/// habla va tan seguida que no hay espacio, un subtítulo queda más corto (nunca
/// menos de `MIN_VISIBLE`) antes que solapado.
fn build_captions(segments: &[(f64, f64, String)], duration: f64) -> Vec<Caption> {
    let mut ordered: Vec<(f64, f64, &str)> = segments
        .iter()
        .filter(|(_, _, text)| !is_noise(text))
        .map(|(a, b, text)| {
            let start = a.max(0.0);
            (start, b.max(start), text.as_str())
        })
        .collect();
    ordered.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut pages: Vec<Caption> = ordered
        .iter()
        .flat_map(|&(a, b, text)| split_segment(a, b, text))
        .map(|(start, end, text)| Caption { start, end, text })
        .collect();
    // Ni muy cortos para leerse ni tan largos que se queden pegados.
    for page in &mut pages {
        page.end = page.end.clamp(page.start + MIN_SECS, page.start + MAX_SECS);
    }
    // Sin solapes: se recorta el final del anterior antes de empujar el inicio del siguiente.
    let mut out: Vec<Caption> = Vec::with_capacity(pages.len());
    let mut free = 0.0f64;
    for i in 0..pages.len() {
        let start = pages[i].start.max(free);
        let mut end = pages[i].end.max(start + MIN_VISIBLE);
        if let Some(next) = pages.get(i + 1) {
            let limit = next.start - GAP_SECS;
            if end > limit {
                end = limit.max(start + MIN_VISIBLE);
            }
        }
        free = end + GAP_SECS;
        out.push(Caption { start, end, text: std::mem::take(&mut pages[i].text) });
    }
    // Al largo del video.
    out.into_iter()
        .filter_map(|mut caption| {
            caption.end = caption.end.min(duration);
            (caption.end - caption.start >= MIN_KEPT).then_some(caption)
        })
        .collect()
}

/// Deja los subtítulos solo en los tramos `spans` (lo que queda tras recortar y
/// cortar): cada uno se achica a su pedazo más largo dentro de lo que queda, y
/// el que cae entero en un corte se va.
fn clip_to_spans(captions: Vec<Caption>, spans: &[(f64, f64)]) -> Vec<Caption> {
    captions
        .into_iter()
        .filter_map(|caption| {
            let (start, end) = spans
                .iter()
                .map(|&(a, b)| (caption.start.max(a), caption.end.min(b)))
                .filter(|(a, b)| b - a >= MIN_VISIBLE)
                .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))?;
            Some(Caption { start, end, text: caption.text })
        })
        .collect()
}

/// El avance (0–1) como porcentaje entero para mostrar.
fn percent(progress: f64) -> u32 {
    if progress.is_nan() {
        0
    } else {
        (progress.clamp(0.0, 1.0) * 100.0).round() as u32
    }
}

/// PCM de 16 bits little-endian a muestras de −1 a 1. Un byte suelto al final se ignora.
fn pcm_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CaptionSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl CaptionSize {
    const ALL: [CaptionSize; 3] = [CaptionSize::Small, CaptionSize::Medium, CaptionSize::Large];

    fn factor(self) -> f64 {
        match self {
            CaptionSize::Small => SIZE_SMALL,
            CaptionSize::Medium => SIZE_MEDIUM,
            CaptionSize::Large => SIZE_LARGE,
        }
    }

    fn label(self) -> &'static str {
        match self {
            CaptionSize::Small => "S",
            CaptionSize::Medium => "M",
            CaptionSize::Large => "L",
        }
    }

    /// El tamaño más cercano a un múltiplo cualquiera (un proyecto editado a mano).
    fn of(factor: f64) -> Self {
        Self::ALL.into_iter().min_by(|a, b| (a.factor() - factor).abs().total_cmp(&(b.factor() - factor).abs())).unwrap_or_default()
    }
}

/// El estilo que comparten todos los subtítulos.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CaptionStyle {
    /// Arriba en vez de abajo.
    top: bool,
    size: CaptionSize,
    /// Con píldora detrás.
    boxed: bool,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self { top: false, size: CaptionSize::Medium, boxed: true }
    }
}

fn style_of(text: &Text) -> CaptionStyle {
    CaptionStyle { top: text.at == TextAt::Top, size: CaptionSize::of(text.size), boxed: text.boxed }
}

fn apply_style(text: &mut Text, style: CaptionStyle) {
    text.at = if style.top { TextAt::Top } else { TextAt::Bottom };
    text.size = style.size.factor();
    text.boxed = style.boxed;
}

fn caption_text(caption: Caption, style: CaptionStyle) -> Text {
    let mut text = Text::new(caption.start, caption.end, caption.text, TextAt::Bottom);
    text.caption = true;
    apply_style(&mut text, style);
    text
}

/// Con qué voz se transcribe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Engine {
    /// El modelo local de Atic (`local_stt`).
    Local,
    /// El reconocimiento de voz de macOS, en el equipo.
    Apple,
}

/// El motor que se usa: el que el usuario eligió para Reuniones si está listo y,
/// si no, el otro. Groq no cuenta (sale a internet): con `"groq"` va primero la
/// voz de macOS, como en Reuniones.
fn pick_engine(backend: &str, local_ready: bool, apple_ready: bool) -> Option<Engine> {
    let local = local_ready.then_some(Engine::Local);
    let apple = apple_ready.then_some(Engine::Apple);
    match backend {
        "parakeet" => local.or(apple),
        _ => apple.or(local),
    }
}

// --- El audio ---------------------------------------------------------------------

/// Un WAV mono de 16 bits leído de a trozos.
struct WavReader {
    file: std::fs::File,
    left: u64,
    total: u64,
}

impl WavReader {
    fn open(path: &Path) -> Result<Self, String> {
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; WAV_HEADER as usize];
        file.read_exact(&mut head).map_err(|e| e.to_string())?;
        if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
            return Err(t("pill.editor.captionsBadAudio").into());
        }
        let total = len.saturating_sub(WAV_HEADER) / 2;
        Ok(Self { file, left: total, total })
    }

    /// Hasta `max` muestras más, o `None` si ya no quedan.
    fn next(&mut self, max: usize) -> Result<Option<Vec<f32>>, String> {
        if self.left == 0 {
            return Ok(None);
        }
        let count = (self.left as usize).min(max.max(1));
        let mut bytes = vec![0u8; count * 2];
        self.file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        self.left -= count as u64;
        Ok(Some(pcm_to_f32(&bytes)))
    }
}

/// El WAV temporal: se borra al soltarlo, pase lo que pase.
struct TempWav(PathBuf);

impl TempWav {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Self(std::env::temp_dir().join(format!("atic-subtitulos-{}-{stamp}-{n}.wav", std::process::id())))
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// --- El trabajo -------------------------------------------------------------------

/// Por qué no salieron subtítulos.
#[derive(Clone, Debug, PartialEq)]
enum Failure {
    /// Ningún motor de voz listo en el equipo.
    NoEngine,
    NoAudio,
    NoSpeech,
    /// Se canceló: no se muestra.
    Cancelled,
    Other(String),
}

fn failure_text(error: &Failure) -> String {
    match error {
        Failure::NoEngine => t("pill.editor.captionsNoEngine").into(),
        Failure::NoAudio => t("pill.editor.captionsNoAudio").into(),
        Failure::NoSpeech => t("pill.editor.captionsNoSpeech").into(),
        Failure::Cancelled => String::new(),
        Failure::Other(detail) => tf("pill.editor.captionsFailed", &[("error", detail)]),
    }
}

/// Lo que se dice entre `.0` y `.1` segundos.
type Segment = (f64, f64, String);

/// El estado de la transcripción, para quien la pide sin mirar la sección (los agentes).
pub(super) enum Transcript {
    Idle,
    Running(f64),
    Ready(Vec<(f64, f64, String)>),
    Failed(String),
}

/// Lo que comparten la ventana y el hilo del trabajo.
#[derive(Clone)]
struct Control {
    /// El avance, de 0 a 1 (`f64::to_bits`).
    progress: Arc<AtomicU64>,
    phase: Arc<AtomicU8>,
    cancel: Arc<AtomicBool>,
}

impl Control {
    fn new() -> Self {
        Self { progress: Arc::new(AtomicU64::new(0f64.to_bits())), phase: Arc::new(AtomicU8::new(PHASE_AUDIO)), cancel: Arc::new(AtomicBool::new(false)) }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn check(&self) -> Result<(), Failure> {
        if self.cancelled() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// El trabajo en marcha. Soltarlo (cancelar, o cerrar la ventana) lo detiene.
struct Run {
    id: u64,
    ctl: Control,
    /// Pone los subtítulos en el proyecto al terminar (si no, solo guarda lo que se dice: lo pide un agente).
    apply: bool,
}

impl Drop for Run {
    fn drop(&mut self) {
        self.ctl.cancel.store(true, Ordering::Relaxed);
    }
}

/// El estado de la sección: lo que se ve mientras se genera y el estilo elegido.
#[derive(Default)]
pub(super) struct CaptionsUi {
    run: Option<Run>,
    /// Lo que se dice, con tiempos, de la última transcripción (para los agentes).
    segments: Option<Vec<Segment>>,
    next_id: u64,
    error: Option<Failure>,
    /// Solo lo que queda tras recortar y cortar (por defecto, todo el video).
    kept_only: bool,
    /// El estilo para los próximos subtítulos, mientras no haya ninguno.
    style: CaptionStyle,
    /// Si hay un motor de voz en el equipo, y cuándo se miró.
    engine: Cell<Option<(Instant, bool)>>,
}

/// El motor y el idioma que el usuario tiene en Ajustes.
fn speech_settings() -> (String, Option<String>) {
    match atic_core::AppDirs::new() {
        Ok(dirs) => {
            let cfg = atic_core::Config::load(&dirs.config_path());
            let language = cfg.whisper_language();
            (cfg.meeting_backend, language)
        }
        Err(_) => (String::new(), None),
    }
}

fn local_ready() -> bool {
    crate::local_stt::ready()
}

/// ¿Está la voz de macOS lista en el equipo? Lanza el ayudante: solo desde un hilo.
fn apple_ready(language: Option<&str>) -> bool {
    use crate::platform::apple;
    apple::installed() && matches!(apple::status(language).map(|s| s.speech), Ok(apple::Speech::Ready))
}

/// ¿Hay algún motor que probar? Barato (sin lanzar el ayudante): decide si la
/// sección ofrece el botón o manda a Ajustes.
fn engine_present() -> bool {
    local_ready() || crate::platform::apple::installed()
}

/// El modelo local, por trozos: sus frases con el tiempo del trozo sumado.
fn transcribe_local(wav: &Path, ctl: &Control) -> Result<Vec<(f64, f64, String)>, Failure> {
    let rate = crate::local_stt::RATE as u64;
    let mut reader = WavReader::open(wav).map_err(Failure::Other)?;
    let total = reader.total.max(1);
    let mut done = 0u64;
    let mut out = Vec::new();
    ctl.phase.store(PHASE_SPEECH, Ordering::Relaxed);
    while let Some(chunk) = reader.next(CHUNK_SECS * rate as usize).map_err(Failure::Other)? {
        ctl.check()?;
        let offset = done as f64 / rate as f64;
        let (text, phrases) = crate::local_stt::transcribe_segments(&chunk).map_err(Failure::Other)?;
        // El modelo a veces da el texto sin frases con tiempo (como en Reuniones): el trozo entero va de una.
        if phrases.is_empty() && !text.trim().is_empty() {
            out.push((offset, offset + chunk.len() as f64 / rate as f64, text.trim().to_string()));
        }
        out.extend(phrases.into_iter().map(|(a, b, text)| (offset + a as f64, offset + b as f64, text)));
        done += chunk.len() as u64;
        ctl.progress.store((done as f64 / total as f64).min(1.0).to_bits(), Ordering::Relaxed);
    }
    Ok(out)
}

/// La voz de macOS: todo el WAV de una vez (contesta al final, sin avance).
fn transcribe_apple(wav: &Path, language: Option<&str>, ctl: &Control) -> Result<Vec<(f64, f64, String)>, Failure> {
    ctl.phase.store(PHASE_SPEECH_OPEN, Ordering::Relaxed);
    let done = crate::platform::apple::transcribe(wav, language, true).map_err(Failure::Other)?;
    Ok(done.phrases.into_iter().map(|p| (p.start_ms as f64 / 1000.0, p.end_ms as f64 / 1000.0, p.text)).collect())
}

/// Del video a subtítulos: elige el motor, saca el audio y transcribe. `videos`
/// son los archivos donde buscar audio, en orden (la copia sin cursor puede no
/// traerlo y la grabación sí).
fn run_job(videos: &[PathBuf], duration: f64, ctl: &Control) -> Result<(Vec<Segment>, Vec<Caption>), Failure> {
    let (backend, language) = speech_settings();
    let local = local_ready();
    // Lanzar el ayudante de macOS cuesta: solo si puede ser el motor.
    let apple = !(backend == "parakeet" && local) && apple_ready(language.as_deref());
    let engine = pick_engine(&backend, local, apple).ok_or(Failure::NoEngine)?;
    ctl.check()?;
    let wav = TempWav::new();
    let mut failure = Failure::NoAudio;
    let mut extracted = false;
    for video in videos {
        match atic_capture::stream::extract_wav_16k(video, &wav.0) {
            Ok(()) => {
                extracted = true;
                break;
            }
            Err(error) if error.contains("no tiene audio") => failure = Failure::NoAudio,
            Err(error) => failure = Failure::Other(error),
        }
    }
    if !extracted {
        return Err(failure);
    }
    ctl.check()?;
    let segments = match engine {
        Engine::Local => transcribe_local(&wav.0, ctl)?,
        Engine::Apple => transcribe_apple(&wav.0, language.as_deref(), ctl)?,
    };
    ctl.check()?;
    let captions = build_captions(&segments, duration);
    // Sin voz no es un error aquí: quien pide subtítulos avisa (`apply_captions`) y quien pide
    // la transcripción (un agente) recibe una lista vacía.
    Ok((segments, captions))
}

/// `run_job` sin que un pánico del motor deje la sección esperando para siempre.
fn run_guarded(videos: &[PathBuf], duration: f64, ctl: &Control) -> Result<(Vec<Segment>, Vec<Caption>), Failure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(videos, duration, ctl)))
        .unwrap_or_else(|_| Err(Failure::Other(t("pill.editor.captionsCrashed").into())))
}

// --- La sección -------------------------------------------------------------------

fn note(color: Hsla, text: impl Into<SharedString>) -> gpui::AnyElement {
    div().text_size(px(T_SMALL)).text_color(color).child(text.into()).into_any_element()
}

fn progress_bar(pal: &Ed, progress: f64) -> gpui::AnyElement {
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(pal.control)
        .overflow_hidden()
        .child(div().h_full().w(gpui::relative(progress.clamp(0.0, 1.0) as f32)).rounded_full().bg(pal.accent))
        .into_any_element()
}

impl RecordingEditor {
    /// Los subtítulos que hay hoy en el proyecto.
    fn caption_count(&self) -> usize {
        self.project.texts.iter().filter(|text| text.caption).count()
    }

    /// El estilo de los subtítulos: el del primero si ya hay (el proyecto manda),
    /// o el elegido para los próximos.
    fn caption_style(&self) -> CaptionStyle {
        self.project.texts.iter().find(|text| text.caption).map(style_of).unwrap_or(self.captions_ui.style)
    }

    /// ¿Hay un motor de voz en el equipo? Se vuelve a mirar cada pocos segundos.
    fn has_engine(&self) -> bool {
        let now = Instant::now();
        if let Some((at, present)) = self.captions_ui.engine.get() {
            if now.duration_since(at) < ENGINE_TTL {
                return present;
            }
        }
        let present = engine_present();
        self.captions_ui.engine.set(Some((now, present)));
        present
    }

    /// Cambia el estilo de todos los subtítulos a la vez (y el de los próximos).
    fn set_caption_style(&mut self, change: impl FnOnce(&mut CaptionStyle), cx: &mut Context<Self>) {
        let mut style = self.caption_style();
        change(&mut style);
        self.captions_ui.style = style;
        let mut any = false;
        for text in self.project.texts.iter_mut().filter(|text| text.caption) {
            apply_style(text, style);
            any = true;
        }
        if any {
            self.changed_keyed("captions-style", cx);
        } else {
            cx.notify();
        }
    }

    /// Los índices de los textos cambian al reemplazar o quitar subtítulos.
    fn forget_text_selection(&mut self) {
        if matches!(self.selected, Some(super::Item::Text(_))) {
            self.selected = None;
        }
    }

    pub(super) fn remove_captions(&mut self, cx: &mut Context<Self>) {
        if self.caption_count() == 0 {
            return;
        }
        self.project.texts.retain(|text| !text.caption);
        self.captions_ui.error = None;
        self.forget_text_selection();
        self.changed(cx);
    }

    fn cancel_captions(&mut self, cx: &mut Context<Self>) {
        // Soltar el trabajo lo cancela (`Run::drop`).
        self.captions_ui.run = None;
        cx.notify();
    }

    /// Genera los subtítulos del audio del video, en otro hilo: reemplazan a los
    /// anteriores y dejan intactos los textos normales.
    pub(super) fn start_captions(&mut self, cx: &mut Context<Self>) {
        self.start_job(true, cx);
    }

    /// Transcribe el audio sin tocar el proyecto: lo que se dice queda en `captions_ui.segments`.
    pub(super) fn start_transcript(&mut self, cx: &mut Context<Self>) {
        self.start_job(false, cx);
    }

    fn start_job(&mut self, apply: bool, cx: &mut Context<Self>) {
        if self.captions_ui.run.is_some() {
            return;
        }
        let ui = &mut self.captions_ui;
        ui.next_id += 1;
        let id = ui.next_id;
        let ctl = Control::new();
        ui.error = None;
        ui.run = Some(Run { id, ctl: ctl.clone(), apply });
        let kept_only = ui.kept_only;
        let duration = self.duration;
        let mut videos = vec![self.source.clone()];
        if self.path != self.source {
            videos.push(self.path.clone());
        }
        cx.spawn(async move |view, cx| {
            let result = cx.background_executor().spawn(async move { run_guarded(&videos, duration, &ctl) }).await;
            let _ = view.update(cx, |view, cx| view.finish_captions(id, kept_only, result, cx));
        })
        .detach();
        // El avance se redibuja solo mientras el trabajo sigue.
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(TICK).await;
            let going = view.update(cx, |view, cx| {
                let going = view.captions_ui.run.as_ref().is_some_and(|run| run.id == id);
                if going {
                    cx.notify();
                }
                going
            });
            if !going.unwrap_or(false) {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_captions(&mut self, id: u64, kept_only: bool, result: Result<(Vec<Segment>, Vec<Caption>), Failure>, cx: &mut Context<Self>) {
        // Si se canceló o se lanzó otro, este resultado ya no va.
        let Some(apply) = self.captions_ui.run.as_ref().filter(|run| run.id == id).map(|run| run.apply) else {
            return;
        };
        self.captions_ui.run = None;
        match result {
            Ok((segments, captions)) => {
                self.captions_ui.segments = Some(segments);
                if apply {
                    self.apply_captions(captions, kept_only, cx);
                } else {
                    cx.notify();
                }
            }
            Err(Failure::Cancelled) => cx.notify(),
            Err(error) => {
                tracing::warn!(?error, "editor: no se pudieron crear los subtítulos");
                self.captions_ui.error = Some(error);
                cx.notify();
            }
        }
    }

    /// Pone los subtítulos nuevos en lugar de los anteriores, ordenados por tiempo
    /// junto con los textos normales.
    fn apply_captions(&mut self, captions: Vec<Caption>, kept_only: bool, cx: &mut Context<Self>) {
        let captions = if kept_only { clip_to_spans(captions, &self.project.segments(self.duration)) } else { captions };
        // Sin nada nuevo, los de antes se quedan.
        if captions.is_empty() {
            self.captions_ui.error = Some(Failure::NoSpeech);
            cx.notify();
            return;
        }
        let style = self.caption_style();
        self.project.texts.retain(|text| !text.caption);
        self.project.texts.extend(captions.into_iter().map(|caption| caption_text(caption, style)));
        self.project.texts.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.forget_text_selection();
        self.changed(cx);
    }

    /// Cómo va la transcripción que pidió un agente.
    pub(super) fn transcript_status(&self) -> Transcript {
        let ui = &self.captions_ui;
        if let Some(run) = &ui.run {
            return Transcript::Running(f64::from_bits(run.ctl.progress.load(Ordering::Relaxed)));
        }
        match (&ui.segments, &ui.error) {
            (Some(segments), _) => Transcript::Ready(segments.clone()),
            (None, Some(error)) => Transcript::Failed(failure_text(error)),
            (None, None) => Transcript::Idle,
        }
    }

    /// La sección «Subtítulos» de la barra lateral.
    pub(super) fn captions_section(&self, pal: &Ed, cx: &mut Context<Self>) -> gpui::AnyElement {
        let ui = &self.captions_ui;
        let count = self.caption_count();
        let mut body = vec![note(pal.sub, t("pill.editor.captionsLocalNote"))];
        if let Some(run) = &ui.run {
            let phase = run.ctl.phase.load(Ordering::Relaxed);
            let progress = f64::from_bits(run.ctl.progress.load(Ordering::Relaxed));
            let label: String = match phase {
                PHASE_SPEECH => tf("pill.editor.captionsWorking", &[("pct", &percent(progress))]),
                PHASE_SPEECH_OPEN => t("pill.editor.captionsWorkingOpen").into(),
                _ => t("pill.editor.captionsPreparing").into(),
            };
            body.push(note(pal.text, label));
            if phase == PHASE_SPEECH {
                body.push(progress_bar(pal, progress));
            }
            body.push(self.action(pal, "editor-captions-cancel", "icons/x.svg", t("pill.editor.captionsCancel"), t("pill.editor.captionsCancelTip"), Tone::Normal, |view, _, cx| view.cancel_captions(cx), cx));
        } else if ui.error == Some(Failure::NoEngine) || !self.has_engine() {
            // Sin motor de voz: el camino a Ajustes en lugar del botón.
            body.push(note(pal.text, t("pill.editor.captionsNoEngine")));
            body.push(self.action(pal, "editor-captions-settings", "icons/settings-2.svg", t("pill.editor.captionsOpenSettings"), t("pill.editor.captionsOpenSettingsTip"), Tone::Normal, |_, _, cx| cx.defer(crate::settings::open), cx));
            if ui.error.is_some() {
                body.push(self.action(pal, "editor-captions-retry-engine", "icons/rotate-cw.svg", t("pill.editor.captionsRetry"), t("pill.editor.captionsRetryTip"), Tone::Normal, |view, _, cx| view.start_captions(cx), cx));
            }
        } else {
            if let Some(error) = &ui.error {
                body.push(note(pal.text, failure_text(error)));
            }
            let (icon, label, tip) = if ui.error.is_some() {
                ("icons/rotate-cw.svg", t("pill.editor.captionsRetry"), t("pill.editor.captionsRetryTip"))
            } else if count > 0 {
                ("icons/rotate-cw.svg", t("pill.editor.captionsRedo"), t("pill.editor.captionsRedoTip"))
            } else {
                ("icons/sparkles.svg", t("pill.editor.captionsGenerate"), t("pill.editor.captionsGenerateTip"))
            };
            body.push(self.primary(pal, "editor-captions-generate", icon, label.into(), tip, |view, _, cx| view.start_captions(cx), cx));
            body.push(self.switch_row(pal, "editor-captions-kept", t("pill.editor.captionsKeptOnly"), ui.kept_only, |view, _, cx| {
                view.captions_ui.kept_only = !view.captions_ui.kept_only;
                cx.notify();
            }, cx));
        }
        if count > 0 && ui.run.is_none() {
            let counted = if count == 1 { t("pill.editor.captionsCountOne").to_string() } else { tf("pill.editor.captionsCount", &[("count", &count)]) };
            body.push(note(pal.sub, counted));
            body.push(note(pal.sub, t("pill.editor.captionsEditHint")));
            body.push(self.action(pal, "editor-captions-remove", "icons/trash.svg", t("pill.editor.captionsRemove"), t("pill.editor.captionsRemoveTip"), Tone::Danger, |view, _, cx| view.remove_captions(cx), cx));
        }
        // El estilo, para todos a la vez.
        let style = self.caption_style();
        let places = vec![
            self.chip(pal, "editor-captions-bottom".into(), t("pill.editor.captionsBottom").into(), !style.top, |view, _, cx| view.set_caption_style(|s| s.top = false, cx), cx),
            self.chip(pal, "editor-captions-top".into(), t("pill.editor.captionsTop").into(), style.top, |view, _, cx| view.set_caption_style(|s| s.top = true, cx), cx),
        ];
        body.push(Self::row(pal, t("pill.editor.captionsPosition"), places));
        let sizes: Vec<_> = CaptionSize::ALL
            .into_iter()
            .map(|size| {
                self.chip(pal, format!("editor-captions-size-{}", size.label()).into(), size.label().into(), style.size == size, move |view, _, cx| view.set_caption_style(|s| s.size = size, cx), cx)
            })
            .collect();
        body.push(Self::row(pal, t("pill.editor.captionsSize"), sizes));
        body.push(self.switch_row(pal, "editor-captions-boxed", t("pill.editor.captionsBoxed"), style.boxed, |view, _, cx| view.set_caption_style(|s| s.boxed = !s.boxed, cx), cx));
        Self::page(pal, t("pill.editor.captions"), body).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, text: &str) -> (f64, f64, String) {
        (start, end, text.to_string())
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn parte_en_lineas_por_palabras() {
        assert_eq!(wrap_words("uno dos tres cuatro", 7), ["uno dos", "tres", "cuatro"]);
        assert_eq!(wrap_words("   ", 7), Vec::<String>::new());
        // Una palabra más larga que la línea queda sola.
        assert_eq!(wrap_words("a extraordinariamente b", 5), ["a", "extraordinariamente", "b"]);
    }

    #[test]
    fn una_frase_corta_es_un_solo_subtitulo() {
        assert_eq!(split_segment(2.0, 4.0, "  Hola   mundo "), [(2.0, 4.0, "Hola mundo".to_string())]);
    }

    #[test]
    fn una_frase_larga_se_divide_por_tiempo_proporcional_sin_huecos() {
        // 12 palabras de 9 letras: 4 por línea (39 caracteres), 3 líneas, 2 + 1.
        let text = vec!["abcdefghi"; 12].join(" ");
        let parts = split_segment(0.0, 9.0, &text);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].2.split(' ').count(), 8);
        assert_eq!(parts[1].2.split(' ').count(), 4);
        // Pesos 72 y 36 letras: el primero se lleva 2/3 de los 9 s.
        assert!(near(parts[0].0, 0.0) && near(parts[0].1, 6.0));
        assert!(near(parts[1].0, 6.0) && near(parts[1].1, 9.0));
        for (_, _, page) in &parts {
            assert!(page.chars().count() <= MAX_LINES * MAX_LINE_CHARS + 1);
            assert!(wrap_words(page, MAX_LINE_CHARS).len() <= MAX_LINES);
        }
    }

    #[test]
    fn el_ruido_no_se_subtitula() {
        let segments = [seg(0.0, 2.0, "[música]"), seg(2.0, 4.0, "..."), seg(3.0, 4.0, "(risas)"), seg(4.0, 6.0, "Hola")];
        let captions = build_captions(&segments, 60.0);
        assert_eq!(captions, [Caption { start: 4.0, end: 6.0, text: "Hola".into() }]);
    }

    #[test]
    fn frases_separadas_quedan_como_vienen() {
        let captions = build_captions(&[seg(1.0, 3.0, "Hola a todos"), seg(3.5, 6.0, "Bienvenidos")], 60.0);
        assert_eq!(captions.len(), 2);
        assert!(near(captions[0].start, 1.0) && near(captions[0].end, 3.0));
        assert!(near(captions[1].start, 3.5) && near(captions[1].end, 6.0));
    }

    #[test]
    fn un_subtitulo_dura_de_1_2_a_6_segundos() {
        let short = build_captions(&[seg(0.0, 0.3, "Sí")], 10.0);
        assert!(near(short[0].end - short[0].start, MIN_SECS));
        let long = build_captions(&[seg(0.0, 20.0, "Una frase corta")], 30.0);
        assert!(near(long[0].end - long[0].start, MAX_SECS));
    }

    #[test]
    fn los_subtitulos_no_se_pisan_y_dejan_aire() {
        // El primero se recorta antes de empujar al segundo.
        let captions = build_captions(&[seg(0.0, 3.0, "Primero"), seg(2.0, 5.0, "Segundo")], 60.0);
        assert_eq!(captions.len(), 2);
        assert!(near(captions[0].start, 0.0) && near(captions[0].end, 1.95));
        assert!(near(captions[1].start, 2.0) && near(captions[1].end, 5.0));
        assert!(near(captions[1].start - captions[0].end, GAP_SECS));
    }

    #[test]
    fn con_habla_muy_seguida_ninguno_se_solapa_ni_baja_del_minimo_visible() {
        let segments = [seg(0.0, 0.2, "Uno"), seg(0.3, 0.5, "Dos"), seg(0.6, 0.8, "Tres")];
        let captions = build_captions(&segments, 30.0);
        assert_eq!(captions.len(), 3);
        for pair in captions.windows(2) {
            assert!(pair[1].start >= pair[0].end + GAP_SECS - 1e-9, "{pair:?}");
        }
        for caption in &captions {
            assert!(caption.end - caption.start >= MIN_VISIBLE - 1e-9, "{caption:?}");
        }
    }

    #[test]
    fn lo_desordenado_se_ordena_y_los_espacios_se_juntan() {
        let captions = build_captions(&[seg(5.0, 7.0, "  segundo   texto "), seg(1.0, 3.0, "primero")], 60.0);
        assert_eq!(captions[0].text, "primero");
        assert_eq!(captions[1].text, "segundo texto");
    }

    #[test]
    fn se_recorta_al_largo_del_video() {
        let captions = build_captions(&[seg(8.0, 12.0, "Final")], 10.0);
        assert_eq!(captions.len(), 1);
        assert!(near(captions[0].start, 8.0) && near(captions[0].end, 10.0));
        // Uno que empieza a 0,05 s del final no alcanza a leerse: se va.
        assert!(build_captions(&[seg(9.95, 10.5, "Tarde")], 10.0).is_empty());
        // Uno que empieza después del final tampoco.
        assert!(build_captions(&[seg(11.0, 12.0, "Fuera")], 10.0).is_empty());
    }

    #[test]
    fn sin_frases_no_hay_subtitulos() {
        assert!(build_captions(&[], 10.0).is_empty());
    }

    #[test]
    fn solo_lo_que_queda_tras_los_cortes() {
        let captions = vec![
            Caption { start: 1.0, end: 3.0, text: "a".into() },
            Caption { start: 2.5, end: 4.5, text: "cortado".into() },
            Caption { start: 4.0, end: 6.0, text: "b".into() },
            Caption { start: 9.0, end: 12.0, text: "c".into() },
        ];
        // Quedan 0–2 y 5–10; 2–5 es un corte.
        let kept = clip_to_spans(captions, &[(0.0, 2.0), (5.0, 10.0)]);
        let spans: Vec<_> = kept.iter().map(|c| (c.start, c.end, c.text.as_str())).collect();
        assert_eq!(spans, [(1.0, 2.0, "a"), (5.0, 6.0, "b"), (9.0, 10.0, "c")]);
    }

    #[test]
    fn el_avance_se_muestra_como_porcentaje_entero() {
        assert_eq!(percent(0.0), 0);
        assert_eq!(percent(0.5), 50);
        assert_eq!(percent(0.499), 50);
        assert_eq!(percent(1.7), 100);
        assert_eq!(percent(-1.0), 0);
        assert_eq!(percent(f64::NAN), 0);
    }

    #[test]
    fn el_pcm_pasa_a_muestras_y_ignora_un_byte_suelto() {
        assert_eq!(pcm_to_f32(&[0x00, 0x40, 0x00, 0x80, 0xff]), [0.5, -1.0]);
    }

    #[test]
    fn el_wav_se_lee_de_a_trozos() {
        let path = std::env::temp_dir().join(format!("atic-subtitulos-test-{}.wav", std::process::id()));
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        bytes.resize(WAV_HEADER as usize, 0);
        for sample in [0i16, 16384, -16384, 8192, 0] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();
        let mut reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.total, 5);
        assert_eq!(reader.next(3).unwrap().unwrap(), [0.0, 0.5, -0.5]);
        assert_eq!(reader.next(3).unwrap().unwrap(), [0.25, 0.0]);
        assert!(reader.next(3).unwrap().is_none());
        std::fs::write(&path, b"no es un wav, ni de cerca, ni con cuarenta y cuatro bytes").unwrap();
        assert!(WavReader::open(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn el_tamano_va_al_mas_cercano() {
        assert_eq!(CaptionSize::of(0.8), CaptionSize::Small);
        assert_eq!(CaptionSize::of(1.0), CaptionSize::Medium);
        assert_eq!(CaptionSize::of(1.3), CaptionSize::Large);
        assert_eq!(CaptionSize::of(1.1), CaptionSize::Medium);
        assert_eq!(CaptionSize::of(2.5), CaptionSize::Large);
    }

    #[test]
    fn el_estilo_se_aplica_y_se_lee_de_vuelta() {
        let caption = Caption { start: 1.0, end: 2.0, text: "Hola".into() };
        let normal = caption_text(caption.clone(), CaptionStyle::default());
        assert!(normal.caption && normal.boxed);
        assert_eq!((normal.at, normal.size), (TextAt::Bottom, 1.0));
        let style = CaptionStyle { top: true, size: CaptionSize::Large, boxed: false };
        let mut text = caption_text(caption, CaptionStyle::default());
        apply_style(&mut text, style);
        assert_eq!((text.at, text.size, text.boxed), (TextAt::Top, 1.3, false));
        assert_eq!(style_of(&text), style);
        // El color y la negrita no son del estilo compartido.
        assert_eq!((text.color, text.bold), (0xffffff, true));
    }

    #[test]
    fn el_motor_elegido_va_primero_y_groq_nunca() {
        assert_eq!(pick_engine("parakeet", true, true), Some(Engine::Local));
        assert_eq!(pick_engine("parakeet", false, true), Some(Engine::Apple));
        assert_eq!(pick_engine("apple", true, true), Some(Engine::Apple));
        assert_eq!(pick_engine("apple", true, false), Some(Engine::Local));
        assert_eq!(pick_engine("groq", true, false), Some(Engine::Local));
        assert_eq!(pick_engine("groq", false, false), None);
        assert_eq!(pick_engine("local", false, false), None);
    }
}
