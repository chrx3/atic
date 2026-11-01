//! La línea de tiempo del editor: la regla con las marcas de tiempo, las
//! pistas (video, audio, zoom, velocidad, difuminado y texto) con su nombre a
//! la izquierda, el cabezal, el imán y el rango para cortar.
//!
//! La **vista** (`View`) es la parte del video que se ve: con zoom, un píxel
//! son pocos milisegundos y se puede cortar con precisión en una grabación de
//! veinte minutos. Todo lo que va en `x` sale de ella (`time_x`, `time_at`,
//! `px_per_sec`) y lo que cae fuera no se dibuja.
//!
//! Dibuja (`render`) y, en el `impl RecordingEditor` de abajo, maneja la rueda,
//! los botones de zoom y el imán. El arrastre y los clics los maneja `mod.rs`,
//! que usa las mismas medidas de aquí (`time_at`, `lane_y`) para saber qué hay
//! bajo el puntero. Si cambias una medida, cambia aquí y nada más.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{canvas, div, fill, img, point, prelude::*, px, size, svg, App, Bounds, ClickEvent, Context, Div, Hsla, ObjectFit, RenderImage, ScrollWheelEvent, SharedString, Stateful, Window};

use super::palette::{Ed, T_SMALL};
use super::{Grab, Item, RecordingEditor, LEVELS_PER_SEC};
use super::fluid;
use crate::anim::lerp;
use crate::hover::{HoverExt, HoverFx};
use crate::i18n::t;

/// La columna de los nombres de las pistas, a la izquierda.
pub const GUTTER_W: f32 = 92.0;
/// El margen a la derecha del área de tiempo.
pub const PAD_R: f32 = 16.0;
/// La regla de arriba, con las marcas de tiempo.
pub const RULER_H: f32 = 26.0;
/// La barrita bajo la regla que muestra qué parte del total se ve.
const OVERVIEW_H: f32 = 3.0;
/// Entre la regla y la primera pista: la barrita y los clics del registro.
const TOP_GAP: f32 = 14.0;
pub const VIDEO_H: f32 = 56.0;
pub const AUDIO_H: f32 = 28.0;
pub const LANE_H: f32 = 24.0;
pub const LANE_GAP: f32 = 5.0;
/// Abajo, después de la última pista.
pub const PAD_B: f32 = 10.0;

/// Entre una marca mayor de la regla y la siguiente, al menos esto (píxeles).
const MIN_MAJOR_PX: f64 = 70.0;

/// Lo menos que muestra la vista (segundos).
pub const MIN_VIEW: f64 = 1.0;
/// Dónde queda el cabezal (parte del ancho de la vista, desde la izquierda)
/// cuando la vista lo sigue mientras suena.
pub const FOLLOW_AT: f64 = 0.2;
/// Cuánto acercan o alejan los botones y las teclas.
pub const ZOOM_STEP: f64 = 1.5;
/// Zoom con la rueda: `exp(delta × esto)` por evento.
const WHEEL_ZOOM: f64 = 0.006;
/// Una línea de rueda (ratón sin gesto fino), en píxeles.
const WHEEL_LINE: f32 = 40.0;

/// El imán: a cuántos píxeles se pega un valor.
pub const SNAP_PX: f32 = 8.0;
/// La guía del imán: su opacidad (el color es el del texto, no el ámbar de los clics y del recorte) y la
/// muesca de la regla (lado, en px).
const SNAP_GUIDE_ALPHA: f32 = 0.9;
const SNAP_NOTCH: f32 = 5.0;
/// Un bloque más angosto que esto (px) no lleva etiqueta; si cabe, lo que no entra se corta con «…».
const BLOCK_LABEL_MIN_W: f32 = 28.0;
/// Los clics del registro (puntos sobre la regla): su lado, más chico que antes porque el ámbar ya es del recorte.
const CLICK_DOT: f32 = 4.0;
/// La etiqueta de la regla se apaga a esto de opacidad cuando el cabezal está a menos de `HEAD_CLEARANCE` px.
const LABEL_NEAR_HEAD_ALPHA: f32 = 0.3;
const HEAD_CLEARANCE: f32 = 24.0;
/// Bajo este nivel (0–1), el audio es silencio: no hay señal (típico del permiso del micrófono sin dar).
const SILENCE_LEVEL: f32 = 0.01;
/// La tecla que agrega cada bloque (zoom, velocidad, difuminado, texto): para la pista de una pista vacía.
const LANE_KEYS: [&str; 4] = ["Z", "V", "B", "T"];
/// La pista tenue de una pista vacía: opacidad y distancia al borde izquierdo.
const HINT_ALPHA: f32 = 0.85;
const HINT_LEFT: f32 = 10.0;

/// Las barras de la forma de onda: una cada `WAVE_PITCH` píxeles, de `WAVE_BAR` de ancho.
const WAVE_PITCH: f32 = 2.0;
const WAVE_BAR: f32 = 1.5;
/// Aire arriba y abajo de la forma de onda.
const WAVE_PAD: f32 = 6.0;

/// Los botones de zoom, arriba a la derecha de la regla.
const VIEW_BTN: f32 = 20.0;
const VIEW_BTN_GAP: f32 = 2.0;
const VIEW_BTN_TOP: f32 = 3.0;
const VIEW_BTNS_W: f32 = VIEW_BTN * 3.0 + VIEW_BTN_GAP * 2.0;

/// Las pistas, de arriba abajo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Video,
    Audio,
    Zoom,
    Speed,
    Blur,
    Text,
}

/// Las pistas en el orden en que se dibujan.
pub const LANES: [Lane; 6] = [Lane::Video, Lane::Audio, Lane::Zoom, Lane::Speed, Lane::Blur, Lane::Text];

/// Cuánto mide una pista.
pub fn lane_h(lane: Lane) -> f32 {
    match lane {
        Lane::Video => VIDEO_H,
        Lane::Audio => AUDIO_H,
        Lane::Zoom | Lane::Speed | Lane::Blur | Lane::Text => LANE_H,
    }
}

/// Dónde empieza (y cuánto mide) cada pista, desde arriba de la línea de tiempo.
pub fn lane_y(lane: Lane) -> (f32, f32) {
    let mut y = RULER_H + TOP_GAP;
    for l in LANES {
        if l == lane {
            return (y, lane_h(l));
        }
        y += lane_h(l) + LANE_GAP;
    }
    (y, 0.0)
}

/// El alto total.
pub fn height() -> f32 {
    let (y, h) = lane_y(Lane::Text);
    y + h + PAD_B
}

/// La pista bajo `y` (desde arriba de la línea de tiempo), si hay una.
pub fn lane_at(y: f32) -> Option<Lane> {
    LANES.into_iter().find(|lane| {
        let (top, h) = lane_y(*lane);
        y >= top - LANE_GAP / 2.0 && y < top + h + LANE_GAP / 2.0
    })
}

/// ¿Está (`x`, `y`), desde la esquina de la línea de tiempo, sobre los botones de zoom?
pub fn on_view_buttons(x: f32, y: f32, width: f32) -> bool {
    let right = width - PAD_R;
    y < RULER_H && x >= right - VIEW_BTNS_W - 4.0 && x <= right + 4.0
}

// --- La vista: qué parte del video se ve ---------------------------------------------

/// La parte del video que se ve: desde `start`, `len` segundos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub start: f64,
    pub len: f64,
}

/// Lo más chico y lo más grande que puede medir la vista para un video de `duration`.
fn len_bounds(duration: f64) -> (f64, f64) {
    let d = duration.max(0.001);
    (MIN_VIEW.min(d), d)
}

impl View {
    /// Todo el video.
    pub fn full(duration: f64) -> Self {
        Self { start: 0.0, len: duration.max(0.001) }
    }

    /// Lo mismo que `full`: el botón «ajustar».
    pub fn fit(duration: f64) -> Self {
        Self::full(duration)
    }

    pub fn end(&self) -> f64 {
        self.start + self.len
    }

    /// ¿La vista muestra todo el video?
    pub fn is_full(&self, duration: f64) -> bool {
        self.len >= duration.max(0.001) - 1e-6
    }

    /// Acota a [0, duración], con `len` entre `MIN_VIEW` (o la duración, si es menor) y la duración.
    pub fn clamped(self, duration: f64) -> Self {
        let (min, max) = len_bounds(duration);
        let len = if self.len.is_finite() { self.len } else { max };
        let len = len.clamp(min, max);
        let start = if self.start.is_finite() { self.start } else { 0.0 };
        Self { start: start.clamp(0.0, (max - len).max(0.0)), len }
    }

    /// Acerca (`factor` > 1) o aleja (`factor` < 1) alrededor del segundo `anchor`: ese
    /// segundo queda en el mismo lugar de la pantalla (salvo contra un borde).
    pub fn zoom_at(self, anchor: f64, factor: f64, duration: f64) -> Self {
        let cur = self.clamped(duration);
        let (min, max) = len_bounds(duration);
        let len = (cur.len / factor.max(1e-6)).clamp(min, max);
        let at = ((anchor - cur.start) / cur.len).clamp(0.0, 1.0);
        Self { start: anchor - at * len, len }.clamped(duration)
    }

    /// Corre la vista `dt` segundos (negativo, hacia atrás).
    pub fn pan(self, dt: f64, duration: f64) -> Self {
        Self { start: self.start + dt, len: self.len }.clamped(duration)
    }

    /// Si `t` queda fuera, la vista lo sigue y lo deja a `FOLLOW_AT` desde la izquierda.
    pub fn follow(self, t: f64, duration: f64) -> Self {
        let cur = self.clamped(duration);
        if t >= cur.start && t <= cur.end() {
            return cur;
        }
        Self { start: t - FOLLOW_AT * cur.len, len: cur.len }.clamped(duration)
    }
}

/// El ancho del área de tiempo para una línea de tiempo de `width` píxeles.
pub fn time_width(width: f32) -> f32 {
    (width - GUTTER_W - PAD_R).max(1.0)
}

/// Cuántos píxeles son un segundo con esta vista.
pub fn px_per_sec(view: View, width: f32) -> f64 {
    time_width(width) as f64 / view.len.max(1e-9)
}

/// La `x` (desde la izquierda de la línea de tiempo) del segundo `t`; sin acotar:
/// puede quedar fuera de la vista.
pub fn time_x(t: f64, view: View, width: f32) -> f32 {
    GUTTER_W + ((t - view.start) * px_per_sec(view, width)) as f32
}

/// El segundo bajo `x` (desde la izquierda de la línea de tiempo), acotado al video.
pub fn time_at(x: f32, view: View, duration: f64, width: f32) -> f64 {
    (view.start + (x - GUTTER_W) as f64 / px_per_sec(view, width)).clamp(0.0, duration.max(0.0))
}

/// Dónde empieza y cuánto mide (partes de 0 a 1 del ancho) lo que se ve del total.
pub fn overview(view: View, duration: f64) -> (f32, f32) {
    let d = duration.max(0.001);
    ((view.start / d).clamp(0.0, 1.0) as f32, (view.len / d).clamp(0.0, 1.0) as f32)
}

// --- El imán -------------------------------------------------------------------------

/// El candidato más cercano a `t` si queda a `threshold` segundos o menos.
pub fn snap_to(t: f64, candidates: &[f64], threshold: f64) -> Option<f64> {
    candidates
        .iter()
        .copied()
        .filter(|c| c.is_finite() && (c - t).abs() <= threshold)
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
}

/// Para un tramo de largo `len` que empieza en `start`: pega el borde (inicio o fin) que
/// quede más cerca de un candidato. Devuelve el nuevo inicio y el segundo donde se pegó.
pub fn snap_span(start: f64, len: f64, candidates: &[f64], threshold: f64) -> (f64, Option<f64>) {
    let end = start + len;
    match (snap_to(start, candidates, threshold), snap_to(end, candidates, threshold)) {
        (Some(a), Some(b)) => {
            if (a - start).abs() <= (b - end).abs() {
                (a, Some(a))
            } else {
                (b - len, Some(b))
            }
        }
        (Some(a), None) => (a, Some(a)),
        (None, Some(b)) => (b - len, Some(b)),
        (None, None) => (start, None),
    }
}

// --- La regla ------------------------------------------------------------------------

/// Una marca de la regla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    pub t: f64,
    /// Lleva etiqueta y línea guía.
    pub major: bool,
}

/// El paso de las marcas mayores (segundos) y en cuántas partes se divide.
const RULER_STEPS: [(f64, usize); 17] = [
    (0.1, 2),
    (0.2, 2),
    (0.5, 5),
    (1.0, 5),
    (2.0, 4),
    (5.0, 5),
    (10.0, 5),
    (15.0, 3),
    (30.0, 3),
    (60.0, 6),
    (120.0, 4),
    (300.0, 5),
    (600.0, 5),
    (900.0, 3),
    (1800.0, 3),
    (3600.0, 6),
    (7200.0, 4),
];

/// El paso menor con al menos `MIN_MAJOR_PX` píxeles entre marcas mayores.
pub fn ruler_step(px_per_sec: f64) -> (f64, usize) {
    RULER_STEPS.iter().copied().find(|(step, _)| step * px_per_sec >= MIN_MAJOR_PX).unwrap_or(RULER_STEPS[RULER_STEPS.len() - 1])
}

/// Las marcas que caen dentro de la vista (y del video), de menor a mayor.
pub fn ruler_ticks(view: View, duration: f64, px_per_sec: f64) -> Vec<Tick> {
    let (step, subs) = ruler_step(px_per_sec);
    let fine = step / subs as f64;
    let first = (view.start.max(0.0) / fine - 1e-9).ceil() as i64;
    let last = (view.end().min(duration.max(0.0)) / fine + 1e-9).floor() as i64;
    (first..=last).map(|k| Tick { t: k as f64 * fine, major: k % subs as i64 == 0 }).collect()
}

// --- La forma de onda ----------------------------------------------------------------

/// Agrupa los niveles (`per_second` por segundo) en `cols` columnas a lo ancho de la vista:
/// cada una vale el máximo de los niveles que caen en ella (0 si no hay audio ahí).
pub fn wave_columns(levels: &[f32], per_second: u32, view: View, cols: usize) -> Vec<f32> {
    let rate = per_second.max(1) as f64;
    let n = cols.max(1);
    (0..n)
        .map(|c| {
            let t0 = view.start + c as f64 * view.len / n as f64;
            let t1 = view.start + (c + 1) as f64 * view.len / n as f64;
            let lo = ((t0 * rate + 1e-9).floor().max(0.0)) as usize;
            let hi = (((t1 * rate - 1e-9).ceil().max(0.0)) as usize).max(lo + 1).min(levels.len());
            if lo >= hi {
                0.0
            } else {
                levels[lo..hi].iter().copied().fold(0.0f32, f32::max)
            }
        })
        .collect()
}

/// ¿Queda el segundo `t` en lo que se exporta? (dentro del recorte y fuera de los cortes)
pub fn is_kept(t: f64, trim: (f64, f64), cuts: &[(f64, f64)]) -> bool {
    t >= trim.0 && t <= trim.1 && !cuts.iter().any(|&(a, b)| t >= a && t < b)
}

// --- Lo que se dibuja ----------------------------------------------------------------

/// Un bloque de una pista (un zoom, un texto, una velocidad o un difuminado).
#[derive(Clone, Debug)]
pub struct Block {
    pub start: f64,
    pub end: f64,
    pub label: SharedString,
    pub selected: bool,
    /// Cuánto está elegido (0–1): el borde y el relleno transicionan en vez de saltar.
    pub sel: f32,
    /// Cuánto lleva de aparecer (0–1): un bloque nuevo entra con un fundido y una escala.
    pub appear: f32,
    /// La escala del bloque (de `BLOCK_FROM_SCALE` a 1; siempre 1 con movimiento reducido).
    pub scale: f32,
    /// Lo propuso el autofocus (lleva el ícono de destellos).
    pub auto: bool,
    /// Su id y su globito (el nombre completo y el rango: «2× · 0:42–0:50»).
    pub key: SharedString,
    pub tip: SharedString,
}

/// La pista de audio.
#[derive(Clone, Copy)]
pub enum Wave<'a> {
    /// Todavía se leen los niveles.
    Loading,
    /// El video no tiene audio.
    Silent,
    /// Tiene audio, pero sin señal (todo en silencio).
    Muted,
    /// Niveles (0–1) a `LEVELS_PER_SEC` por segundo.
    Levels(&'a [f32]),
}

/// Lo que se dibuja.
pub struct TimelineData<'a> {
    pub duration: f64,
    /// La parte del video que se ve.
    pub view: View,
    /// Dónde va el cabezal (segundos).
    pub now: f64,
    /// Lo que queda del inicio al final; lo de afuera se ve apagado.
    pub trim: (f64, f64),
    /// Tramos cortados (inicio, fin, cuánto está elegido 0–1, cuánto lleva de aparecer 0–1).
    pub cuts: Vec<(f64, f64, f32, f32)>,
    pub zooms: Vec<Block>,
    pub speeds: Vec<Block>,
    pub blurs: Vec<Block>,
    pub texts: Vec<Block>,
    /// Los clics del registro del mouse (segundos); basta con los de la vista.
    pub clicks: Vec<f64>,
    /// Miniaturas repartidas a lo largo del video (pueden faltar mientras cargan).
    pub thumbs: Vec<Arc<RenderImage>>,
    pub wave: Wave<'a>,
    /// Cuánto se ve la pista de cada pista vacía (zoom, velocidad, difuminado, texto; 0–1): se desvanece al crear el primer bloque.
    pub hints: [f32; 4],
    /// Un clic sobre el aviso de audio en silencio (abre los Ajustes).
    pub muted_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    /// Donde se pegó lo que se arrastra: una guía vertical cruza las pistas, y su opacidad (0–1:
    /// entra y sale con un fundido corto).
    pub snap: Option<(f64, f32)>,
    /// El rango que se marca (Shift + arrastrar sobre el video) para cortarlo, y su opacidad.
    pub range: Option<((f64, f64), f32)>,
    /// El ancho con que se dibujó la última vez (para la escala de la vista);
    /// 0 si todavía no se sabe.
    pub width: f32,
    pub labels: Labels,
}

/// Los nombres de las pistas, ya traducidos.
pub struct Labels {
    pub video: SharedString,
    pub audio: SharedString,
    pub zoom: SharedString,
    pub speed: SharedString,
    pub blur: SharedString,
    pub text: SharedString,
    /// Lo que dice la pista de audio si el video no tiene.
    pub no_audio: SharedString,
    /// Lo que dice si tiene audio pero sin señal.
    pub muted: SharedString,
    /// La pista de cada pista vacía (sin la tecla): zoom, velocidad, difuminado, texto.
    pub hints: [SharedString; 4],
}

/// ¿El audio es puro silencio? Vacío no cuenta (eso es un video sin audio).
pub fn is_silent(levels: &[f32]) -> bool {
    !levels.is_empty() && levels.iter().all(|&level| level < SILENCE_LEVEL)
}

/// ¿Cabe la etiqueta en un bloque de `width` px?
pub fn label_fits(width: f32) -> bool {
    width >= BLOCK_LABEL_MIN_W
}

/// ¿Queda la marca en `x` tan cerca del cabezal (en `head`) que lo taparía su burbuja?
pub fn near_head(x: f32, head: f32) -> bool {
    (x - head).abs() < HEAD_CLEARANCE
}

/// Un tiempo como «m:ss» (o «h:mm:ss» si pasa de una hora); con `tenths`, con décimas.
pub fn stamp(secs: f64, tenths: bool) -> String {
    let total = ((secs.max(0.0) + 1e-6) * 10.0).floor() as u64;
    let whole = total / 10;
    let (h, m, s) = (whole / 3600, whole / 60 % 60, whole % 60);
    let dec = if tenths { format!(".{}", total % 10) } else { String::new() };
    if h > 0 {
        format!("{h}:{m:02}:{s:02}{dec}")
    } else {
        format!("{m}:{s:02}{dec}")
    }
}

/// Un bloque (zoom, texto, velocidad o difuminado) dentro del área de tiempo; `None` si
/// cae fuera de la vista. `x_of` da la `x` de un segundo y `tw` el ancho del área.
fn block(b: &Block, color: Hsla, x_of: &dyn Fn(f64) -> f32, tw: f32, pal: &Ed) -> Option<Stateful<Div>> {
    let (x0, x1) = (x_of(b.start), x_of(b.end));
    if x1 < 0.0 || x0 > tw {
        return None;
    }
    let (left, right) = (x0.max(0.0), x1.min(tw));
    // Un bloque nuevo crece desde su centro (de `BLOCK_FROM_SCALE` a 1) mientras se funde.
    let width = (right - left).max(3.0);
    let (inset_x, inset_y) = (width * (1.0 - b.scale) / 2.0, LANE_H * (1.0 - b.scale) / 2.0);
    let mut el = div()
        .id(b.key.clone())
        .absolute()
        .top(px(inset_y))
        .bottom(px(inset_y))
        .left(px(left + inset_x))
        .w(px(width * b.scale))
        .opacity(b.appear)
        .rounded(px((pal.r_ctl - 2.0).max(2.0)))
        .overflow_hidden()
        .bg(color.opacity(lerp(0.30, 0.55, b.sel)))
        .border_1()
        .border_color(color.opacity(lerp(0.7, 1.0, b.sel)))
        .flex()
        .items_center()
        .px(px(8.))
        .font_family(pal.font)
        .text_size(px(T_SMALL))
        .text_color(pal.text)
        .whitespace_nowrap()
        .gap(px(4.))
        .tooltip(crate::hover::tip_text(b.tip.clone()))
        // Los del autofocus llevan su ícono (el del botón Autofocus).
        .when(b.auto && label_fits(width), |el| el.child(svg().path("icons/sparkles.svg").flex_none().size(px(11.)).text_color(pal.text)))
        // La etiqueta se corta con «…» y, en un bloque muy angosto, no va (el nombre completo está en el globito).
        .when(label_fits(width), |el| el.child(div().flex_1().min_w_0().truncate().child(b.label.clone())));
    if b.selected || b.sel > 0.01 {
        // Las orillas que indican que se puede estirar (solo las que se ven); se funden con la selección.
        let edge_alpha = 0.85 * b.sel;
        for left_edge in [true, false] {
            if (left_edge && x0 < 0.0) || (!left_edge && x1 > tw) {
                continue;
            }
            let edge = div().absolute().top(px(5.)).bottom(px(5.)).w(px(3.)).rounded(px(2.)).bg(gpui::white().opacity(edge_alpha));
            el = el.child(if left_edge { edge.left(px(2.)) } else { edge.right(px(2.)) });
        }
    }
    Some(el)
}

/// Una pista con el resalte suave al pasar el mouse: su fondo sube de `LANE_REST` a `LANE_HOVER`
/// de opacidad (`hover_fx`: ya pide cuadros solo mientras cambia).
fn lane_fx(lane: Lane, el: Div, pal: &Ed) -> HoverFx {
    let control = pal.control;
    el.fx(SharedString::from(format!("tl-lane-{lane:?}")), move |el, h| el.bg(control.opacity(lerp(fluid::LANE_REST, fluid::LANE_HOVER, h.t))))
}

/// La línea de tiempo entera, de `height()` de alto y todo el ancho que le den.
/// Va con `relative()`: `mod.rs` le agrega encima el canvas que mide, el manejo
/// del mouse y los botones de zoom.
pub fn render(data: &TimelineData, pal: &Ed) -> Div {
    let dur = data.duration.max(0.001);
    let view = data.view.clamped(dur);
    let width = if data.width > 0.0 { data.width } else { 900.0 };
    let tw = time_width(width);
    let ppx = px_per_sec(view, width);
    // La `x` de un segundo, desde el borde izquierdo del área de tiempo.
    let x_of = move |t: f64| time_x(t, view, width) - GUTTER_W;

    // El área de tiempo: todo lo que va en x se posiciona en píxeles desde su borde.
    let mut area = div().absolute().left(px(GUTTER_W)).right(px(PAD_R)).top_0().bottom_0();

    // Lo que se ve del total, bajo la regla (solo indicador).
    let (ov_left, ov_len) = overview(view, dur);
    area = area.child(
        div().absolute().left_0().right_0().top(px(RULER_H + 1.0)).h(px(OVERVIEW_H)).rounded_full().bg(pal.border.opacity(0.35)).child(
            div().absolute().top_0().bottom_0().left(px(ov_left * tw)).w(px((ov_len * tw).max(6.0))).rounded_full().bg(pal.muted.opacity(if view.is_full(dur) { 0.25 } else { 0.55 })),
        ),
    );

    // Marcas, etiquetas y líneas guía (solo las de la vista). La etiqueta que queda bajo la burbuja del cabezal se apaga.
    let tenths = ruler_step(ppx).0 < 1.0;
    let head_x = x_of(data.now);
    for tick in ruler_ticks(view, dur, ppx) {
        let x = x_of(tick.t);
        if x < -0.5 || x > tw + 0.5 {
            continue;
        }
        if tick.major {
            area = area
                .child(div().absolute().left(px(x)).top(px(RULER_H)).bottom_0().w(px(1.)).bg(pal.border.opacity(0.35)))
                .child(div().absolute().left(px(x)).top(px(RULER_H - 8.)).w(px(1.)).h(px(8.)).bg(pal.faint))
                .child(
                    div().absolute().left(px(x)).top(px(4.)).child(
                        div()
                            .ml(px(3.))
                            .font_family(pal.mono)
                            .text_size(px(10.))
                            .text_color(pal.sub.opacity(if near_head(x, head_x) { LABEL_NEAR_HEAD_ALPHA } else { 1.0 }))
                            .whitespace_nowrap()
                            .child(stamp(tick.t, tenths)),
                    ),
                );
        } else {
            area = area.child(div().absolute().left(px(x)).top(px(RULER_H - 4.)).w(px(1.)).h(px(4.)).bg(pal.faint.opacity(0.5)));
        }
    }

    // Las pistas.
    let lane_bg = |lane: Lane| {
        let (y, h) = lane_y(lane);
        div().absolute().left_0().right_0().top(px(y)).h(px(h)).rounded(px(pal.r_ctl)).bg(pal.control.opacity(0.6))
    };
    // Una pista vacía dice qué va ahí: «Z» y la acción, tenue; se desvanece al crear el primer bloque.
    let hint = |n: usize| {
        let alpha = data.hints[n];
        (alpha > 0.01).then(|| {
            div()
                .absolute()
                .left(px(HINT_LEFT))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .opacity(alpha * HINT_ALPHA)
                .font_family(pal.font)
                .text_size(px(T_SMALL))
                .text_color(pal.sub)
                .child(super::key_cap(LANE_KEYS[n], pal.sub, pal.control))
                .child(data.labels.hints[n].clone())
        })
    };

    // Video: miniaturas, lo recortado y los cortes dentro de un recorte redondeado.
    let (_, vh) = lane_y(Lane::Video);
    let band = |a: f64, b: f64| -> Option<Div> {
        let (x0, x1) = (x_of(a).max(0.0), x_of(b).min(tw));
        (x1 > x0).then(|| div().absolute().top_0().bottom_0().left(px(x0)).w(px(x1 - x0)).bg(gpui::black().opacity(0.6)))
    };
    let mut clip = div().absolute().top_0().bottom_0().left_0().right_0().rounded(px(pal.r_ctl)).overflow_hidden();
    // Las miniaturas se reparten por la vista: con zoom se ven más anchas.
    let n = data.thumbs.len().max(1) as f64;
    let thumb_len = dur / n;
    let thumb_w = (thumb_len * ppx) as f32;
    for (i, image) in data.thumbs.iter().enumerate() {
        let x0 = x_of(i as f64 * thumb_len);
        if x0 + thumb_w < 0.0 || x0 > tw {
            continue;
        }
        clip = clip.child(img(image.clone()).absolute().top_0().h_full().left(px(x0)).w(px(thumb_w + 0.5)).object_fit(ObjectFit::Cover));
    }
    clip = clip.children(band(0.0, data.trim.0)).children(band(data.trim.1, dur));
    for &(a, b, sel, appear) in &data.cuts {
        if let Some(cut) = band(a, b) {
            clip = clip.child(cut.border_1().border_color(pal.cut.opacity(lerp(0.55, 1.0, sel))).opacity(appear));
        }
    }
    let mut video = lane_bg(Lane::Video).child(clip);
    // El rango que se marca para cortar.
    if let Some(((a, b), alpha)) = data.range {
        let (a, b) = (a.min(b), a.max(b));
        let (x0, x1) = (x_of(a).max(0.0), x_of(b).min(tw));
        if x1 > x0 {
            video = video.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(x0))
                    .w(px(x1 - x0))
                    .rounded(px(3.))
                    .opacity(alpha)
                    .bg(pal.cut.opacity(0.35))
                    .border_1()
                    .border_color(pal.cut)
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .font_family(pal.mono)
                    .text_size(px(10.))
                    .text_color(pal.text)
                    .whitespace_nowrap()
                    .when(x1 - x0 > 44.0, |el| el.child(stamp(b - a, true))),
            );
        }
    }
    // Las manillas del recorte (si están a la vista).
    for t in [data.trim.0, data.trim.1] {
        let x = x_of(t);
        if x < -4.0 || x > tw + 4.0 {
            continue;
        }
        video = video.child(
            div().absolute().left(px(x)).top(px(-3.)).bottom(px(-3.)).w_0().child(
                div()
                    .absolute()
                    .left(px(-4.))
                    .top_0()
                    .bottom_0()
                    .w(px(8.))
                    .rounded(px(3.))
                    .bg(pal.trim)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().w(px(2.)).h(px(vh * 0.4)).rounded(px(1.)).bg(gpui::black().opacity(0.45))),
            ),
        );
    }
    area = area.child(lane_fx(Lane::Video, video, pal));

    // Audio: la forma de onda, una barra por columna de la vista.
    let audio = lane_bg(Lane::Audio).overflow_hidden();
    let audio = match data.wave {
        Wave::Loading => audio.child(div().absolute().left_0().right_0().top(px(AUDIO_H / 2.0 - 0.5)).h(px(1.)).bg(pal.faint.opacity(0.35))),
        Wave::Silent => audio
            .flex()
            .items_center()
            .justify_center()
            .font_family(pal.font)
            .text_size(px(T_SMALL))
            .text_color(pal.sub)
            .child(data.labels.no_audio.clone()),
        // Con audio pero sin señal: dice por qué se ve plano (no es «cargando»); un clic abre los Ajustes.
        Wave::Muted => audio.flex().items_center().justify_center().child({
            let message = div()
                .id("tl-audio-muted")
                .h_full()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .cursor_pointer()
                .font_family(pal.font)
                .text_size(px(T_SMALL))
                .text_color(pal.sub)
                .child(svg().path("icons/mic-off.svg").flex_none().size(px(12.)).text_color(pal.sub))
                .child(data.labels.muted.clone());
            match data.muted_click.clone() {
                Some(open) => message.on_click(move |event, window, cx| open(event, window, cx)),
                None => message,
            }
        }),
        Wave::Levels(levels) => {
            let cols = wave_columns(levels, LEVELS_PER_SEC, view, ((tw / WAVE_PITCH).ceil() as usize).max(1));
            let (on, off) = (pal.muted, pal.faint.opacity(0.4));
            let trim = data.trim;
            let cuts: Vec<(f64, f64)> = data.cuts.iter().map(|&(a, b, _, _)| (a, b)).collect();
            audio.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let h = f32::from(bounds.size.height);
                        let n = cols.len().max(1) as f64;
                        for (c, level) in cols.iter().enumerate() {
                            let t = view.start + (c as f64 + 0.5) * view.len / n;
                            let amp = (level.clamp(0.0, 1.0).powf(0.7) * (h - WAVE_PAD * 2.0)).max(WAVE_BAR);
                            let origin = point(bounds.origin.x + px(c as f32 * WAVE_PITCH), bounds.origin.y + px((h - amp) / 2.0));
                            let color = if is_kept(t, trim, &cuts) { on } else { off };
                            window.paint_quad(fill(Bounds::new(origin, size(px(WAVE_BAR), px(amp))), color));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
        }
    };
    area = area.child(lane_fx(Lane::Audio, audio, pal));

    let (speed_color, blur_color) = (pal.speed, pal.blur);
    area = area.child(lane_fx(Lane::Zoom, lane_bg(Lane::Zoom).children(hint(0)).children(data.zooms.iter().filter_map(|b| block(b, pal.zoom, &x_of, tw, pal))), pal));
    area = area.child(lane_fx(Lane::Speed, lane_bg(Lane::Speed).children(hint(1)).children(data.speeds.iter().filter_map(|b| block(b, speed_color, &x_of, tw, pal))), pal));
    area = area.child(lane_fx(Lane::Blur, lane_bg(Lane::Blur).children(hint(2)).children(data.blurs.iter().filter_map(|b| block(b, blur_color, &x_of, tw, pal))), pal));
    area = area.child(lane_fx(Lane::Text, lane_bg(Lane::Text).children(hint(3)).children(data.texts.iter().filter_map(|b| block(b, pal.text_lane, &x_of, tw, pal))), pal));

    // Los clics, justo bajo la barrita de la vista.
    let click_y = RULER_H + OVERVIEW_H + 3.0;
    area = area.children(data.clicks.iter().filter_map(|t| {
        let x = x_of(*t);
        (x >= -2.5 && x <= tw + 2.5).then(|| {
            div().absolute().left(px(x)).top(px(click_y)).w_0().h(px(CLICK_DOT)).child(div().absolute().left(px(-CLICK_DOT / 2.0)).top_0().size(px(CLICK_DOT)).rounded_full().bg(pal.click))
        })
    }));

    // La guía del imán: cruza todas las pistas en el segundo donde se pegó lo que se arrastra.
    if let Some((t, alpha)) = data.snap {
        let x = x_of(t);
        if x >= -1.0 && x <= tw + 1.0 {
            // Del color del texto (no del ámbar de los clics y del recorte), con una muesca en la regla.
            area = area.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(RULER_H - 6.))
                    .bottom_0()
                    .w_0()
                    .child(div().absolute().left(px(-0.5)).top_0().bottom_0().w(px(1.)).bg(pal.text.opacity(SNAP_GUIDE_ALPHA * alpha)))
                    .child(div().absolute().left(px(-SNAP_NOTCH / 2.0)).top(px(-SNAP_NOTCH / 2.0)).size(px(SNAP_NOTCH)).rounded_full().bg(pal.text.opacity(SNAP_GUIDE_ALPHA * alpha))),
            );
        }
    }

    // El cabezal: la línea y la burbuja con el tiempo.
    let hx = x_of(data.now);
    if hx >= -1.0 && hx <= tw + 1.0 {
        area = area.child(div().absolute().left(px(hx)).top(px(RULER_H - 6.)).bottom_0().w_0().child(
            div().absolute().left(px(-1.)).top_0().bottom_0().w(px(2.)).bg(pal.accent),
        ));
    }
    if hx >= -24.0 && hx <= tw + 24.0 {
        area = area.child(
            div().absolute().left(px(hx)).top(px(3.)).child(
                div().ml(px(-24.)).w(px(48.)).flex().justify_center().child(
                    div()
                        .px(px(5.))
                        .rounded(px(4.))
                        .bg(pal.accent)
                        .font_family(pal.mono)
                        .text_size(px(10.))
                        .text_color(pal.on_accent)
                        .whitespace_nowrap()
                        .text_center()
                        .child(stamp(data.now, true)),
                ),
            ),
        );
    }

    // La columna de los nombres.
    let name = |lane: Lane, icon: &'static str, text: &SharedString| {
        let (y, h) = lane_y(lane);
        div()
            .absolute()
            .left_0()
            .top(px(y))
            .h(px(h))
            .w(px(GUTTER_W))
            .pl(px(10.))
            .flex()
            .items_center()
            .gap(px(5.))
            .font_family(pal.font)
            .text_size(px(T_SMALL))
            .text_color(pal.sub)
            .child(svg().path(icon).flex_none().size(px(14.)).text_color(pal.sub))
            .child(div().whitespace_nowrap().child(text.clone()))
    };

    div()
        .relative()
        .w_full()
        .h(px(height()))
        .child(name(Lane::Video, "icons/video.svg", &data.labels.video))
        .child(name(Lane::Audio, "icons/audio-lines.svg", &data.labels.audio))
        .child(name(Lane::Zoom, "icons/magnify.svg", &data.labels.zoom))
        .child(name(Lane::Speed, "icons/gauge.svg", &data.labels.speed))
        .child(name(Lane::Blur, "icons/pixelate.svg", &data.labels.blur))
        .child(name(Lane::Text, "icons/type.svg", &data.labels.text))
        .child(area)
}

// --- Vista, rueda, botones e imán del editor -----------------------------------------

/// La vista a medio camino entre `from` y `to`, con el mismo progreso `t` (0–1) para el inicio y el largo: la regla y
/// las pistas se mueven juntas, de una sola fuente.
pub fn lerp_view(from: View, to: View, t: f32) -> View {
    let t = t.clamp(0.0, 1.0) as f64;
    View { start: from.start + (to.start - from.start) * t, len: from.len + (to.len - from.len) * t }
}

impl RecordingEditor {
    /// La vista que se dibuja ahora: la de destino o, si se está deslizando hacia ella, un punto del camino.
    pub(super) fn shown_view(&self) -> View {
        match self.view_anim {
            Some((from, at)) if !self.fluid.reduced() => lerp_view(from, self.view, self.fluid.since(at, fluid::VIEW_GLIDE)),
            _ => self.view,
        }
    }

    /// Los botones y las teclas llevan la vista a `view` deslizándola desde donde esté ahora (si se aprieta otra vez
    /// a mitad de camino, sigue desde ahí).
    fn animate_view(&mut self, view: View, cx: &mut Context<Self>) {
        let from = self.shown_view();
        let next = view.clamped(self.duration);
        if next != self.view {
            self.view_anim = Some((from, std::time::Instant::now()));
            self.view = next;
            cx.notify();
        }
    }

    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        let next = view.clamped(self.duration);
        if next != self.view {
            self.view = next;
            cx.notify();
        }
    }

    /// Acerca (`factor` > 1) o aleja alrededor del segundo `anchor`.
    fn zoom_view(&mut self, anchor: f64, factor: f64, cx: &mut Context<Self>) {
        self.set_view(self.view.zoom_at(anchor, factor, self.duration), cx);
    }

    /// Los botones y las teclas: alrededor del cabezal si está a la vista; si no, del centro.
    pub(super) fn zoom_step(&mut self, factor: f64, cx: &mut Context<Self>) {
        let now = self.now();
        let anchor = if now >= self.view.start && now <= self.view.end() { now } else { self.view.start + self.view.len / 2.0 };
        self.animate_view(self.view.zoom_at(anchor, factor, self.duration), cx);
    }

    /// Toda la grabación en la vista.
    pub(super) fn fit_view(&mut self, cx: &mut Context<Self>) {
        self.animate_view(View::fit(self.duration), cx);
    }

    /// La rueda sobre la línea de tiempo: con ⌘ o Ctrl hace zoom alrededor del puntero;
    /// si no, desplaza (el gesto horizontal, Shift o la rueda vertical sola).
    pub(super) fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(bounds) = self.lanes.get() else { return };
        let width = f32::from(bounds.size.width);
        let delta = event.delta.pixel_delta(px(WHEEL_LINE));
        let (dx, dy) = (f32::from(delta.x) as f64, f32::from(delta.y) as f64);
        if event.modifiers.platform || event.modifiers.control {
            let d = if dy != 0.0 { dy } else { dx };
            if d == 0.0 {
                return;
            }
            let local = f32::from(event.position.x) - f32::from(bounds.origin.x);
            let anchor = time_at(local, self.view, self.duration, width);
            self.zoom_view(anchor, (d * WHEEL_ZOOM).exp(), cx);
        } else {
            let d = if dx.abs() > dy.abs() { dx } else { dy };
            if d == 0.0 {
                return;
            }
            self.set_view(self.view.pan(-d / px_per_sec(self.view, width), self.duration), cx);
        }
        cx.stop_propagation();
    }

    /// Un botón chico de la regla.
    fn view_button(&self, pal: &Ed, id: &'static str, icon: &'static str, tip: &'static str, act: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> gpui::AnyElement {
        let hover = pal.hover;
        div()
            .id(id)
            .size(px(VIEW_BTN))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .tooltip(super::keys::tip(tip))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| act(view, cx)))
            .child(svg().path(icon).flex_none().size(px(13.)).text_color(pal.muted))
            .hover_bg(SharedString::from(format!("{id}-fx")), hover.opacity(0.0), hover)
            .into_any_element()
    }

    /// Alejar, acercar y ajustar todo: arriba a la derecha de la regla.
    pub(super) fn view_buttons(&self, pal: &Ed, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .absolute()
            .right(px(PAD_R))
            .top(px(VIEW_BTN_TOP))
            .h(px(VIEW_BTN))
            .flex()
            .items_center()
            .gap(px(VIEW_BTN_GAP))
            .child(self.view_button(pal, "editor-view-out", "icons/minus.svg", t("pill.editor.timelineZoomOut"), |view, cx| view.zoom_step(1.0 / ZOOM_STEP, cx), cx))
            .child(self.view_button(pal, "editor-view-in", "icons/plus.svg", t("pill.editor.timelineZoomIn"), |view, cx| view.zoom_step(ZOOM_STEP, cx), cx))
            .child(self.view_button(pal, "editor-view-fit", "icons/fit-width.svg", t("pill.editor.timelineFit"), |view, cx| view.fit_view(cx), cx))
            .into_any_element()
    }

    /// Los segundos a los que se pega lo que se arrastra (`grab`), sin contar lo propio:
    /// el cabezal, los bordes del recorte (y 0 y la duración solo para el cabezal y el
    /// propio recorte: fuera del recorte no se exporta nada), los de los demás bloques de
    /// todas las pistas y los clics del registro que se ven.
    fn snap_points(&self, grab: Grab) -> Vec<f64> {
        let d = self.duration;
        let mut points = Vec::new();
        if matches!(grab, Grab::Head(_) | Grab::TrimStart | Grab::TrimEnd) {
            points.extend([0.0, d]);
        }
        // Arrastrar el recorte también mueve el cabezal: ahí no es un punto fijo.
        if !matches!(grab, Grab::Head(_) | Grab::TrimStart | Grab::TrimEnd) {
            points.push(self.now());
        }
        let (from, to) = self.project.trim(d);
        if grab != Grab::TrimStart {
            points.push(from);
        }
        if grab != Grab::TrimEnd {
            points.push(to);
        }
        let own = match grab {
            Grab::Move(item, _) | Grab::Start(item) | Grab::End(item) => Some(item),
            _ => None,
        };
        let mut edges = |item: Item, a: f64, b: f64| {
            if own != Some(item) {
                points.push(a);
                points.push(b);
            }
        };
        for (i, z) in self.project.zooms.iter().enumerate() {
            edges(Item::Zoom(i), z.start, z.end);
        }
        for (i, x) in self.project.texts.iter().enumerate() {
            edges(Item::Text(i), x.start, x.end);
        }
        for (i, s) in self.project.speeds.iter().enumerate() {
            edges(Item::Speed(i), s.start, s.end);
        }
        for (i, b) in self.project.blurs.iter().enumerate() {
            edges(Item::Blur(i), b.start, b.end);
        }
        for (i, &(a, b)) in self.project.cuts.iter().enumerate() {
            edges(Item::Cut(i), a, b);
        }
        // Solo los clics que se ven (con horas de video son decenas de miles y esto corre
        // en cada movimiento del mouse).
        let (lo, hi) = (self.view.start - 1.0, self.view.end() + 1.0);
        points.extend(self.clicks.iter().map(|ms| *ms as f64 / 1000.0).filter(|t| *t >= lo && *t <= hi));
        points
    }

    /// Pega `t` a lo que haya a menos de `SNAP_PX` píxeles (`per_px`: segundos por píxel);
    /// deja la guía en `self.snap`. Con `alt` no pega.
    pub(super) fn snap_value(&mut self, t: f64, grab: Grab, per_px: f64, alt: bool) -> f64 {
        self.snap = None;
        if alt {
            return t;
        }
        let points = self.snap_points(grab);
        match snap_to(t, &points, SNAP_PX as f64 * per_px) {
            Some(snapped) => {
                self.snap = Some(snapped);
                snapped
            }
            None => t,
        }
    }

    /// Como `snap_value`, para un bloque entero: pega el borde que quede más cerca.
    /// Devuelve el inicio.
    pub(super) fn snap_move(&mut self, start: f64, len: f64, grab: Grab, per_px: f64, alt: bool) -> f64 {
        self.snap = None;
        if alt {
            return start;
        }
        let points = self.snap_points(grab);
        let (start, guide) = snap_span(start, len, &points, SNAP_PX as f64 * per_px);
        self.snap = guide;
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn el_zoom_conserva_el_segundo_bajo_el_puntero() {
        let view = View { start: 10.0, len: 20.0 };
        let zoomed = view.zoom_at(15.0, 2.0, 100.0);
        assert!(close(zoomed.len, 10.0));
        assert!(close(zoomed.start, 12.5));
        // El 15 sigue al 25 % de la vista.
        assert!(close((15.0 - zoomed.start) / zoomed.len, (15.0 - view.start) / view.len));
        // Alejar lo conserva también.
        let out = zoomed.zoom_at(15.0, 0.5, 100.0);
        assert!(close(out.len, 20.0));
        assert!(close(out.start, 10.0));
    }

    #[test]
    fn el_zoom_contra_los_bordes_no_se_sale_del_video() {
        let acercado = View { start: 0.0, len: 100.0 }.zoom_at(0.0, 4.0, 100.0);
        assert!(close(acercado.start, 0.0) && close(acercado.len, 25.0));
        let al_final = View { start: 80.0, len: 20.0 }.zoom_at(100.0, 2.0, 100.0);
        assert!(close(al_final.len, 10.0) && close(al_final.start, 90.0));
    }

    #[test]
    fn los_limites_de_la_vista() {
        let full = View::full(100.0);
        // Acercar de más: un segundo.
        assert!(close(full.zoom_at(50.0, 1000.0, 100.0).len, MIN_VIEW));
        // Alejar de más: toda la duración.
        let out = View { start: 30.0, len: 10.0 }.zoom_at(35.0, 0.001, 100.0);
        assert!(close(out.len, 100.0) && close(out.start, 0.0));
        // Un video de medio segundo: la vista no pasa de la duración.
        let short = View::full(0.5).zoom_at(0.25, 4.0, 0.5);
        assert!(close(short.len, 0.5) && close(short.start, 0.0));
        // `clamped` acota inicio y largo.
        assert!(close(View { start: -5.0, len: 10.0 }.clamped(100.0).start, 0.0));
        assert!(close(View { start: 95.0, len: 10.0 }.clamped(100.0).start, 90.0));
        let huge = View { start: 3.0, len: 500.0 }.clamped(100.0);
        assert!(close(huge.len, 100.0) && close(huge.start, 0.0));
        assert!(View::full(100.0).is_full(100.0));
        assert!(!View { start: 0.0, len: 50.0 }.is_full(100.0));
    }

    #[test]
    fn la_vista_se_desliza_con_un_solo_progreso_para_inicio_y_largo() {
        let from = View { start: 10.0, len: 20.0 };
        let to = View { start: 0.0, len: 100.0 };
        assert_eq!(lerp_view(from, to, 0.0), from);
        assert_eq!(lerp_view(from, to, 1.0), to);
        let mid = lerp_view(from, to, 0.5);
        assert!(close(mid.start, 5.0) && close(mid.len, 60.0));
        // Fuera de 0–1 no se pasa.
        assert_eq!(lerp_view(from, to, 2.0), to);
        assert_eq!(lerp_view(from, to, -1.0), from);
    }

    #[test]
    fn solo_es_silencio_el_audio_sin_ninguna_senal() {
        assert!(is_silent(&[0.0, 0.001, 0.0]));
        assert!(!is_silent(&[0.0, 0.2, 0.0]));
        // Sin niveles no hay audio que revisar (eso es «sin audio»).
        assert!(!is_silent(&[]));
        assert!(!is_silent(&[SILENCE_LEVEL]));
    }

    #[test]
    fn las_etiquetas_de_los_bloques_cortos_se_omiten_y_la_de_la_regla_cede_al_cabezal() {
        assert!(!label_fits(BLOCK_LABEL_MIN_W - 1.0));
        assert!(label_fits(BLOCK_LABEL_MIN_W));
        assert!(near_head(100.0, 110.0));
        assert!(!near_head(100.0, 100.0 + HEAD_CLEARANCE));
    }

    #[test]
    fn desplazar_se_acota() {
        let view = View { start: 10.0, len: 20.0 };
        assert!(close(view.pan(5.0, 100.0).start, 15.0));
        assert!(close(view.pan(1000.0, 100.0).start, 80.0));
        assert!(close(view.pan(-1000.0, 100.0).start, 0.0));
        assert!(close(view.pan(5.0, 100.0).len, 20.0));
    }

    #[test]
    fn la_vista_sigue_al_cabezal() {
        let view = View { start: 0.0, len: 10.0 };
        // Adentro: no se mueve.
        assert_eq!(view.follow(5.0, 100.0), view);
        // Se sale por la derecha: queda al 20 % desde la izquierda.
        let next = view.follow(12.0, 100.0);
        assert!(close(next.start, 10.0) && close(next.len, 10.0));
        // Cerca del final del video: se acota.
        let end = view.follow(100.0, 100.0);
        assert!(close(end.start, 90.0));
        // Se sale por la izquierda (un salto atrás).
        let back = View { start: 50.0, len: 10.0 }.follow(1.0, 100.0);
        assert!(close(back.start, 0.0));
    }

    #[test]
    fn el_tiempo_y_la_x_van_y_vienen_con_la_vista() {
        let view = View { start: 10.0, len: 20.0 };
        // 400 px de área de tiempo: 20 px por segundo.
        let width = GUTTER_W + PAD_R + 400.0;
        assert!(close(px_per_sec(view, width), 20.0));
        assert!(close(time_x(15.0, view, width) as f64, GUTTER_W as f64 + 100.0));
        assert!(close(time_at(GUTTER_W + 100.0, view, 100.0, width), 15.0));
        // Fuera de la vista, `time_x` no se acota.
        assert!(time_x(5.0, view, width) < GUTTER_W);
        // `time_at` se acota al video.
        assert!(close(time_at(-10_000.0, view, 100.0, width), 0.0));
        assert!(close(time_at(10_000.0, view, 100.0, width), 100.0));
    }

    #[test]
    fn el_iman_elige_el_candidato_mas_cercano_dentro_del_umbral() {
        let c = [5.0, 10.0];
        assert_eq!(snap_to(5.3, &c, 0.5), Some(5.0));
        assert_eq!(snap_to(9.8, &c, 0.5), Some(10.0));
        assert_eq!(snap_to(7.0, &c, 0.5), None);
        assert_eq!(snap_to(5.3, &[5.0, 5.4], 0.5), Some(5.4));
        assert_eq!(snap_to(1.0, &[], 0.5), None);
    }

    #[test]
    fn el_iman_de_un_bloque_pega_el_borde_mas_cercano() {
        // El inicio está a 0,1 de 5 y el final a 0,15 de 7,05: gana el inicio.
        let (start, guide) = snap_span(4.9, 2.0, &[5.0, 7.05], 0.3);
        assert!(close(start, 5.0));
        assert_eq!(guide, Some(5.0));
        // Solo el final queda cerca: el bloque se corre para que termine ahí.
        let (start, guide) = snap_span(4.0, 3.0, &[7.1], 0.3);
        assert!(close(start, 4.1));
        assert_eq!(guide, Some(7.1));
        // Nada cerca: queda donde está y sin guía.
        assert_eq!(snap_span(1.0, 1.0, &[50.0], 0.3), (1.0, None));
    }

    #[test]
    fn la_onda_agrupa_los_niveles_por_columna() {
        let levels = [0.1, 0.9, 0.2, 0.4];
        let full = View { start: 0.0, len: 2.0 };
        // Dos niveles por segundo: una columna por segundo toma el máximo de sus dos niveles.
        assert_eq!(wave_columns(&levels, 2, full, 2), vec![0.9, 0.4]);
        assert_eq!(wave_columns(&levels, 2, full, 1), vec![0.9]);
        assert_eq!(wave_columns(&levels, 2, full, 4), vec![0.1, 0.9, 0.2, 0.4]);
        // Más columnas que niveles: cada nivel se repite.
        assert_eq!(wave_columns(&levels, 2, full, 8), vec![0.1, 0.1, 0.9, 0.9, 0.2, 0.2, 0.4, 0.4]);
        // Una vista que empieza a mitad.
        assert_eq!(wave_columns(&levels, 2, View { start: 1.0, len: 1.0 }, 1), vec![0.4]);
        // Pasado el final del audio, nada.
        assert_eq!(wave_columns(&levels, 2, View { start: 1.5, len: 1.0 }, 2), vec![0.4, 0.0]);
        assert_eq!(wave_columns(&[], 2, full, 3), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn lo_cortado_se_ve_apagado() {
        let cuts = [(4.0, 5.0)];
        assert!(!is_kept(1.0, (2.0, 8.0), &cuts));
        assert!(is_kept(3.0, (2.0, 8.0), &cuts));
        assert!(!is_kept(4.0, (2.0, 8.0), &cuts));
        assert!(!is_kept(4.5, (2.0, 8.0), &cuts));
        assert!(is_kept(5.0, (2.0, 8.0), &cuts));
        assert!(!is_kept(9.0, (2.0, 8.0), &cuts));
    }

    #[test]
    fn las_marcas_de_la_regla_se_adaptan_a_la_vista() {
        // 100 px por segundo: una marca mayor por segundo, con cinco partes.
        assert_eq!(ruler_step(100.0), (1.0, 5));
        assert_eq!(ruler_step(1000.0), (0.1, 2));
        assert_eq!(ruler_step(1.0), (120.0, 4));
        assert_eq!(ruler_step(0.5), (300.0, 5));
        // Con muy poco zoom, el paso más grande.
        assert_eq!(ruler_step(0.001), (7200.0, 4));
        // Todas las mayores quedan al menos a 70 px de la anterior.
        for ppx in [0.05, 0.3, 1.0, 4.0, 25.0, 100.0, 700.0] {
            let (step, _) = ruler_step(ppx);
            assert!(step * ppx >= MIN_MAJOR_PX || step == 7200.0, "ppx {ppx}");
        }
    }

    #[test]
    fn las_marcas_caen_dentro_de_la_vista() {
        let ticks = ruler_ticks(View { start: 0.0, len: 10.0 }, 10.0, 100.0);
        assert_eq!(ticks.len(), 51);
        assert_eq!(ticks.iter().filter(|t| t.major).count(), 11);
        assert!(ticks[0].major && close(ticks[0].t, 0.0));
        assert!(ticks[50].major && close(ticks[50].t, 10.0));
        // Una vista a mitad del video: solo lo que se ve.
        let mid = ruler_ticks(View { start: 30.0, len: 10.0 }, 100.0, 100.0);
        assert!(mid.iter().all(|t| t.t >= 30.0 - 1e-6 && t.t <= 40.0 + 1e-6));
        assert!(close(mid[0].t, 30.0) && close(mid[mid.len() - 1].t, 40.0));
        // Nunca pasa de la duración.
        let short = ruler_ticks(View { start: 0.0, len: 5.0 }, 5.0, 100.0);
        assert!(short.iter().all(|t| t.t <= 5.0 + 1e-6));
    }

    #[test]
    fn las_pistas_se_apilan_sin_pisarse() {
        let mut previous_end = RULER_H;
        for lane in LANES {
            let (y, h) = lane_y(lane);
            assert!(y > previous_end, "{lane:?}");
            previous_end = y + h;
            // El centro de cada pista cae en ella.
            assert_eq!(lane_at(y + h / 2.0), Some(lane));
        }
        assert!(height() > previous_end);
        assert_eq!(lane_at(0.0), None);
        assert!(lane_y(Lane::Video).1 > lane_y(Lane::Zoom).1);
    }

    #[test]
    fn la_barrita_muestra_lo_que_se_ve() {
        let (left, len) = overview(View { start: 25.0, len: 25.0 }, 100.0);
        assert!((left - 0.25).abs() < 1e-6 && (len - 0.25).abs() < 1e-6);
        let (left, len) = overview(View::full(60.0), 60.0);
        assert!(left.abs() < 1e-6 && (len - 1.0).abs() < 1e-6);
    }

    #[test]
    fn los_botones_de_zoom_no_pasan_por_la_regla() {
        let width = 900.0;
        assert!(on_view_buttons(width - PAD_R - 10.0, 10.0, width));
        assert!(!on_view_buttons(width - PAD_R - 10.0, RULER_H + 5.0, width));
        assert!(!on_view_buttons(GUTTER_W + 50.0, 10.0, width));
    }
}
