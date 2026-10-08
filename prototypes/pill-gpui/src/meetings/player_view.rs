//! La barra del reproductor al pie del detalle y lo que la transcripción
//! necesita de él: saltar a la hora de un bloque y resaltar el que suena.
//!
//! La UI no pide cuadros cuando está quieta: mientras suena, un tick de ~30 fps
//! lee la posición del motor (un atómico) y se apaga al pausar.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use atic_core::Recording;
use gpui::{
    actions, canvas, div, fill, point, prelude::*, px, size, svg, Animation, AnimationExt, App,
    Bounds, ClickEvent, ClipboardItem, Context, DispatchPhase, ElementId, Font, FontWeight, Hsla,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    ScrollWheelEvent, SharedString, TextRun, Window,
};

use super::player::{self, Files, Player, Track};
use super::stretch;
use super::text::{self, Block};
use super::{
    hsla, MeetingsView, Tab, BLUE, BODY, FAINT, GREEN, INK, ITEM, KEY_CONTEXT, LILAC, MUTED, R_ITEM, SURFACE_ON,
    TEXT,
};
use crate::hover::{self, hover_fx, mix, HoverExt};

actions!(meetings, [PlayPause, Back5, Forward5, Back15, Forward15]);

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("space", PlayPause, context),
        KeyBinding::new("left", Back5, context),
        KeyBinding::new("right", Forward5, context),
        KeyBinding::new("shift-left", Back15, context),
        KeyBinding::new("shift-right", Forward15, context),
    ]);
    super::find::bind_keys(cx);
    super::speakers::bind_keys(cx);
}

/// Cada cuánto se lee la posición mientras suena.
const TICK: Duration = Duration::from_millis(33);
/// Tras mover la rueda, la transcripción deja de seguir al audio un rato.
const HANDS_OFF: Duration = Duration::from_secs(4);
const BAR_H: f32 = 52.0;
const WAVE_H: f32 = 30.0;
const WAVE_BAR: f32 = 2.0;
const WAVE_GAP: f32 = 2.0;
/// El fondo del bloque que suena.
const NOW: u32 = 0x262624;
/// El fondo tenue de un bloque con el cursor encima, y el del que suena con
/// el cursor encima (un poco más claro que `NOW`).
const BLOCK_OVER: u32 = 0x222220;
const NOW_OVER: u32 = 0x2b2b29;
/// El globito sobre la onda (el mismo gris de los tooltips).
const GLOBE: u32 = 0x2a2a27;
/// Cuánto dura el ✓ tras copiar un bloque.
const COPIED_FOR: Duration = Duration::from_millis(1200);

/// La velocidad elegida, para toda la sesión (bits de un `f32`; 0 es 1×):
/// cerrar y abrir la ventana o cambiar de reunión no la pierde.
static SPEED: AtomicU32 = AtomicU32::new(0);

fn session_speed() -> f32 {
    match SPEED.load(Ordering::Relaxed) {
        0 => 1.0,
        bits => f32::from_bits(bits),
    }
}

/// Lo que se sabe de las pistas de una grabación: primero las rutas y la
/// duración (cabeceras, al tiro); la onda llega después.
struct Info {
    /// La misma grabación con otra duración (terminó de grabarse): se relee.
    key: i64,
    files: Files,
    duration_ms: u64,
    mic: Vec<f32>,
    system: Vec<f32>,
    waves_at: Option<Instant>,
}

#[derive(Default)]
pub struct PlayerUi {
    engine: Player,
    /// La grabación que tiene el motor.
    loaded: Option<String>,
    /// La pista elegida a mano (si no, la por defecto).
    track: Option<Track>,
    info: HashMap<String, Info>,
    pending: Option<String>,
    ticking: bool,
    /// Arrastrando sobre la onda: dónde va (0..1).
    scrub: Option<f32>,
    wave_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// El bloque que suena y el anterior (que se apaga); `shift` cambia con
    /// cada paso y reinicia las transiciones.
    current: Option<usize>,
    previous: Option<usize>,
    shift: u64,
    block_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    wheel_at: Option<Instant>,
    follow: bool,
    /// Dónde está el cursor sobre la onda (0..1). No se borra al salir: el
    /// globito se desvanece en el último punto en vez de saltar.
    hover_at: Option<f32>,
    /// La pastilla entera: el globito no se sale de ella.
    bar_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// El bloque recién copiado (muestra ✓) y su turno, para que un aviso
    /// viejo no apague uno nuevo.
    copied: Option<(usize, u64)>,
    copies: u64,
}

/// Lo que muestra el globito de la onda: la hora y lo que se dice ahí.
struct Globe {
    time: SharedString,
    said: Option<(SharedString, bool, String)>,
}

/// El último bloque que empezó antes de `ms`.
pub fn block_at(starts: &[i64], ms: i64) -> Option<usize> {
    match starts.partition_point(|&s| s <= ms) {
        0 => None,
        n => Some(n - 1),
    }
}

/// El texto en una línea y, si pasa de `max` caracteres, cortado en la
/// última palabra entera (salvo que eso deje muy poco) con «…».
pub fn snippet(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if cut[..space].chars().count() * 2 >= max => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches(|c: char| c.is_whitespace() || ",.;:-–—".contains(c)))
}

/// El mayor `n` en `0..=total` con `fits(n)`, si `fits` es monótona
/// (verdadera hasta cierto punto). Pocas pruebas para recortar al ancho.
pub fn longest_fit(total: usize, fits: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, total);
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// Dónde empieza algo de ancho `width` centrado en `x` sin salirse de
/// `min..max` (si no cabe, queda pegado a `min`).
pub fn centered_within(x: f32, width: f32, min: f32, max: f32) -> f32 {
    (x - width / 2.0).min(max - width).max(min)
}

/// Lo que se copia de un bloque.
pub fn copy_text(block: &Block) -> String {
    format!("{}: {}", block.label, block.text.trim())
}

/// `ms` + `delta`, dentro de la grabación.
pub fn nudge(ms: u64, delta_ms: i64, duration_ms: u64) -> u64 {
    let target = (ms as i64 + delta_ms).max(0) as u64;
    if duration_ms > 0 {
        target.min(duration_ms)
    } else {
        target
    }
}

impl MeetingsView {
    fn selected_recording(&self) -> Option<&Recording> {
        self.selected.and_then(|ix| self.items.get(ix))
    }

    fn player_info(&self) -> Option<&Info> {
        let rec = self.selected_recording()?;
        self.player.info.get(&rec.id).filter(|i| !i.files.is_empty())
    }

    /// Antes de dibujar: al cambiar de reunión se detiene lo que sonaba, y se
    /// piden (una vez) las pistas de la elegida.
    pub(super) fn sync_player(&mut self, cx: &mut Context<Self>) {
        self.sync_find(cx);
        let rec = self.selected_recording().cloned();
        let id = rec.as_ref().map(|r| r.id.clone());
        if self.player.loaded.is_some() && self.player.loaded != id {
            self.player.engine.stop();
            self.player.loaded = None;
            self.player.track = None;
            self.player.scrub = None;
            self.player.hover_at = None;
            self.player.copied = None;
            self.set_current(None);
        }
        let (Some(rec), Some(paths)) = (rec, self.source.as_ref().and_then(|s| s.paths()).cloned())
        else {
            return;
        };
        let fresh = self
            .player
            .info
            .get(&rec.id)
            .is_some_and(|i| i.key == rec.duration_secs);
        if fresh || self.player.pending.as_deref() == Some(rec.id.as_str()) {
            return;
        }
        self.player.pending = Some(rec.id.clone());
        let dir = paths.recording_dir(&rec.id);
        let (id, key) = (rec.id.clone(), rec.duration_secs);
        let (mic, sys) = (rec.mic_path.clone(), rec.system_path.clone());
        cx.spawn(async move |this, cx| {
            let (files, duration_ms) = cx
                .background_spawn(async move {
                    let files = Files::resolve(&dir, mic.as_deref(), sys.as_deref());
                    let all: Vec<PathBuf> = files.paths(Track::Mix);
                    let duration = player::duration_ms(&all);
                    (files, duration)
                })
                .await;
            let wanted = files.clone();
            let ok = this.update(cx, |v, cx| {
                v.player.info.insert(
                    id.clone(),
                    Info { key, files, duration_ms, mic: Vec::new(), system: Vec::new(), waves_at: None },
                );
                if v.player.pending.as_deref() == Some(id.as_str()) {
                    v.player.pending = None;
                }
                cx.notify();
            });
            if ok.is_err() || wanted.is_empty() {
                return;
            }
            // La onda, que lee las pistas enteras, va después y aparte.
            let total = duration_ms as f64 / 1000.0;
            let (mut mic, mut system) = cx
                .background_spawn(async move {
                    let read = |p: &Option<PathBuf>| {
                        p.as_ref()
                            .and_then(|p| player::track_peaks(p, total).ok())
                            .map(|(peaks, _)| peaks)
                            .unwrap_or_default()
                    };
                    (read(&wanted.mic), read(&wanted.system))
                })
                .await;
            player::normalize(&mut [&mut mic, &mut system]);
            let _ = this.update(cx, |v, cx| {
                if let Some(info) = v.player.info.get_mut(&id).filter(|i| i.key == key) {
                    info.mic = mic;
                    info.system = system;
                    info.waves_at = Some(Instant::now());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn track_for(&self, info: &Info) -> Track {
        info.files.resolve_track(self.player.track.unwrap_or(Track::Mix))
    }

    fn is_loaded(&self) -> bool {
        self.player.loaded.is_some()
            && self.player.loaded.as_deref() == self.selected_recording().map(|r| r.id.as_str())
    }

    fn position_ms(&self) -> u64 {
        if self.is_loaded() {
            self.player.engine.position_ms()
        } else {
            0
        }
    }

    fn duration_ms(&self, info: &Info) -> u64 {
        match self.player.engine.duration_ms() {
            d if d > 0 && self.is_loaded() => d,
            _ if info.duration_ms > 0 => info.duration_ms,
            _ => self.selected_recording().map_or(0, |r| r.duration_secs.max(0) as u64 * 1000),
        }
    }

    /// Abre las pistas de la reunión elegida en el motor.
    fn load(&mut self, at_ms: u64, play: bool, cx: &mut Context<Self>) -> bool {
        let Some(info) = self.player_info() else {
            return false;
        };
        let paths = info.files.paths(self.track_for(info));
        let Some(id) = self.selected_recording().map(|r| r.id.clone()) else {
            return false;
        };
        self.player.engine.set_speed(session_speed());
        self.player.engine.load(paths, at_ms, play);
        self.player.loaded = Some(id);
        self.after_move(cx);
        true
    }

    fn play_pause(&mut self, cx: &mut Context<Self>) {
        if self.player_info().is_none() {
            cx.propagate();
            return;
        }
        if !self.is_loaded() {
            self.load(0, true, cx);
        } else if self.player.engine.playing() {
            self.player.engine.pause();
        } else {
            self.player.engine.play();
        }
        self.after_move(cx);
    }

    /// Salta a `ms` (y suena si `play`).
    fn seek_to(&mut self, ms: u64, play: bool, cx: &mut Context<Self>) {
        if !self.is_loaded() {
            self.load(ms, play, cx);
            return;
        }
        self.player.engine.seek(ms);
        if play && !self.player.engine.playing() {
            self.player.engine.play();
        }
        self.after_move(cx);
    }

    fn nudge(&mut self, delta_ms: i64, cx: &mut Context<Self>) {
        let Some(info) = self.player_info() else {
            cx.propagate();
            return;
        };
        let duration = self.duration_ms(info);
        let target = nudge(self.position_ms(), delta_ms, duration);
        let playing = self.player.engine.playing();
        self.seek_to(target, playing, cx);
    }

    fn set_track(&mut self, track: Track, cx: &mut Context<Self>) {
        self.player.track = Some(track);
        if self.is_loaded() {
            let (at, playing) = (self.player.engine.position_ms(), self.player.engine.playing());
            self.load(at, playing, cx);
        }
        cx.notify();
    }

    /// Tras cualquier cambio: se marca el bloque y, si suena, arranca el tick.
    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.player.follow = true;
        self.update_current();
        cx.notify();
        if self.player.ticking || !self.player.engine.playing() {
            return;
        }
        self.player.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            let go_on = this.update(cx, |v, cx| {
                let playing = v.player.engine.playing();
                v.update_current();
                v.follow_current();
                cx.notify();
                if !playing {
                    v.player.ticking = false;
                }
                playing
            });
            if !matches!(go_on, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    fn set_current(&mut self, next: Option<usize>) {
        if self.player.current != next {
            self.player.previous = self.player.current;
            self.player.current = next;
            self.player.shift += 1;
            self.player.block_bounds.set(None);
            self.player.follow = true;
        }
    }

    fn update_current(&mut self) {
        let next = match (&self.detail, self.is_loaded()) {
            (Some(detail), true) => {
                let starts: Vec<i64> = detail.blocks.iter().map(|b| b.start_ms).collect();
                let pos = self.player.engine.position_ms();
                // En pausa al principio no hay bloque «sonando».
                if pos == 0 && !self.player.engine.playing() {
                    None
                } else {
                    block_at(&starts, pos as i64)
                }
            }
            _ => None,
        };
        self.set_current(next);
    }

    /// Mantiene a la vista el bloque que suena, salvo que el usuario esté
    /// moviendo la rueda. Se acerca de a poco (un cuarto por tick).
    fn follow_current(&mut self) {
        if self.tab != Tab::Transcript || !self.player.follow {
            return;
        }
        if self.player.wheel_at.is_some_and(|t| t.elapsed() < HANDS_OFF) {
            return;
        }
        let Some(block) = self.player.block_bounds.get() else {
            return;
        };
        let view = self.detail_scroll.bounds();
        let offset = self.detail_scroll.offset();
        let max = self.detail_scroll.max_offset();
        let margin = px(24.);
        let visible = block.top() >= view.top() + margin && block.bottom() <= view.bottom() - margin;
        if visible {
            self.player.follow = false;
            return;
        }
        // Que quede a un tercio desde arriba.
        let target = (offset.y - (block.top() - view.top() - view.size.height / 3.0))
            .clamp(-max.height, px(0.));
        let step = (target - offset.y) * 0.3;
        let y = if step.abs() < px(1.) { target } else { offset.y + step };
        self.detail_scroll.set_offset(point(offset.x, y));
        // `block_bounds` es del cuadro anterior: se corrige con lo que se movió.
        self.player.block_bounds.set(Some(Bounds {
            origin: point(block.origin.x, block.origin.y + (y - offset.y)),
            size: block.size,
        }));
    }

    /// Para la raíz de la ventana: las teclas del reproductor.
    pub(super) fn player_actions(div: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        div.on_action(cx.listener(|v, _: &PlayPause, _, cx| v.play_pause(cx)))
            .on_action(cx.listener(|v, _: &Back5, _, cx| v.nudge(-5_000, cx)))
            .on_action(cx.listener(|v, _: &Forward5, _, cx| v.nudge(5_000, cx)))
            .on_action(cx.listener(|v, _: &Back15, _, cx| v.nudge(-15_000, cx)))
            .on_action(cx.listener(|v, _: &Forward15, _, cx| v.nudge(15_000, cx)))
            .map(|el| Self::find_actions(el, cx))
    }

    /// Para el ▶ de las filas (`row_peek`): suena desde el inicio la reunión
    /// elegida. `false` si sus pistas aún no se conocen; quien llama reintenta.
    pub(super) fn play_from_start(&mut self, cx: &mut Context<Self>) -> bool {
        if self.player_info().is_none() {
            return false;
        }
        self.seek_to(0, true, cx);
        true
    }

    /// Para el área con scroll del detalle: la rueda suelta el seguimiento.
    pub(super) fn player_wheel(&self, cx: &mut Context<Self>) -> Box<dyn Fn(&ScrollWheelEvent, &mut Window, &mut App)> {
        Box::new(cx.listener(|v, _: &ScrollWheelEvent, _, _| v.player.wheel_at = Some(Instant::now())))
    }

    // --- Transcripción --------------------------------------------------------------

    /// Un bloque de la transcripción: la hora salta ahí, y el que suena se
    /// resalta con un fondo que se funde. Con el cursor encima, el bloque se
    /// aclara apenas, la hora se vuelve ▶ (si hay audio) y aparece «Copiar».
    pub(super) fn transcript_block(&self, ix: usize, block: &Block, content: gpui::Div, cx: &mut Context<Self>) -> gpui::AnyElement {
        let playable = self.player_info().is_some();
        let start = block.start_ms.max(0) as u64;
        let is_now = self.player.current == Some(ix);
        let was_now = self.player.previous == Some(ix);
        let copied = self.player.copied.is_some_and(|(i, _)| i == ix);
        let shift = self.player.shift;
        let cell = is_now.then(|| self.player.block_bounds.clone());
        let time: SharedString = text::clock(block.start_ms).into();
        let seek = cx.listener(move |v, _: &ClickEvent, _, cx| v.seek_to(start, true, cx));
        let text_to_copy = copy_text(block);
        let copy = cx.listener(move |v, _: &ClickEvent, _, cx| v.copy_block(ix, text_to_copy.clone(), cx));
        let id = move |name: &'static str| ElementId::NamedInteger(name.into(), ix as u64);

        hover_fx(id("meeting-block"), move |h| {
            let over = h.over;
            // La hora y el ▶ ocupan el mismo lugar y se cruzan.
            let face = div()
                .relative()
                .child(div().opacity(if playable { 1.0 - over } else { 1.0 }).child(time))
                .when(playable && over > 0.0, |el| {
                    el.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .flex()
                            .items_center()
                            .opacity(over)
                            .child(svg().path("icons/play.svg").size(px(12.)).text_color(hsla(TEXT))),
                    )
                });
            let clock = div()
                .id(id("meeting-clock"))
                .ml(px(-7.))
                .px(px(7.))
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(11.))
                .font_family("Cascadia Mono")
                .text_size(px(12.))
                .child(face);
            let clock = if playable {
                clock
                    .cursor_pointer()
                    .tooltip(hover::tip("Escuchar desde aquí"))
                    .on_click(seek)
                    .fx(id("meeting-clock-fx"), move |el, h| {
                        el.bg(h.mix(hsla(ITEM).opacity(0.0), hsla(SURFACE_ON)))
                            .text_color(mix(hsla(if is_now { MUTED } else { FAINT }), hsla(TEXT), h.t))
                    })
                    .into_any_element()
            } else {
                clock.text_color(hsla(FAINT)).into_any_element()
            };
            // El lugar de «Copiar» queda reservado: el texto no se reacomoda
            // al pasar el cursor.
            let copy_button = div()
                .id(id("meeting-copy"))
                .size(px(26.))
                .mt(px(-4.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(13.))
                .opacity(over)
                .when(over > 0.0, |el| {
                    el.cursor_pointer()
                        .tooltip(hover::tip(if copied { "Copiado" } else { "Copiar" }))
                        .on_click(copy)
                })
                .fx(id("meeting-copy-fx"), move |el, b| {
                    let icon = if copied { "icons/check.svg" } else { "icons/copy.svg" };
                    let color = if copied { hsla(GREEN) } else { b.mix(hsla(FAINT), hsla(TEXT)) };
                    el.bg(mix(hsla(TEXT).opacity(0.0), hsla(TEXT).opacity(0.08 + 0.04 * b.press), b.t))
                        .child(svg().path(icon).size(px(13.)).text_color(color))
                });
            let row = div()
                .relative()
                .mx(px(-12.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(R_ITEM))
                .flex()
                .gap(px(14.))
                .child(div().w(px(56.)).flex_none().mt(px(-2.)).child(clock))
                .child(content)
                .child(copy_button);
            // El tenue del cursor y el del que suena se combinan: cada uno se
            // funde por su cuenta.
            let rest = mix(hsla(NOW).opacity(0.0), hsla(BLOCK_OVER), over);
            let now = mix(hsla(NOW), hsla(NOW_OVER), over);
            if let Some(cell) = cell {
                row.child(
                    canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .with_animation(
                    ElementId::NamedInteger("meeting-now-in".into(), shift),
                    Animation::new(Duration::from_millis(260)).with_easing(gpui::ease_out_quint()),
                    move |el, t| el.bg(mix(rest, now, t)),
                )
                .into_any_element()
            } else if was_now {
                row.with_animation(
                    ElementId::NamedInteger("meeting-now-out".into(), shift),
                    Animation::new(Duration::from_millis(420)).with_easing(gpui::ease_out_quint()),
                    move |el, t| el.bg(mix(now, rest, t)),
                )
                .into_any_element()
            } else {
                row.bg(rest).into_any_element()
            }
        })
        .into_any_element()
    }

    /// Copia «Hablante: texto» y deja el ✓ un rato en ese bloque.
    fn copy_block(&mut self, ix: usize, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.player.copies += 1;
        let turn = self.player.copies;
        self.player.copied = Some((ix, turn));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |v, cx| {
                if v.player.copied.is_some_and(|(_, t)| t == turn) {
                    v.player.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // --- La barra -------------------------------------------------------------------

    /// La pastilla al pie del detalle. Sin pistas en disco (datos de prueba,
    /// grabaciones viejas) no aparece.
    pub(super) fn player_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let info = self.player_info()?;
        let playing = self.is_loaded() && self.player.engine.playing();
        let duration = self.duration_ms(info);
        let position = match self.player.scrub {
            Some(f) => (f as f64 * duration as f64) as u64,
            None => self.position_ms().min(duration.max(1)),
        };
        let error = self.player.engine.error().filter(|_| self.is_loaded());
        let track = self.track_for(info);
        let options = info.files.options();
        let played = if duration > 0 { position as f32 / duration as f32 } else { 0.0 };

        let play = div()
            .id("player-play")
            .size(px(36.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.play_pause(cx)))
            .fx("player-play-fx", move |el, h| {
                el.bg(mix(hsla(0xe9e9e2), hsla(0xffffff), h.t * 0.8))
                    .child(
                        svg()
                            .path(if playing { "icons/pause.svg" } else { "icons/play.svg" })
                            .size(px(15. - 1.5 * h.press))
                            // El triángulo de lucide pesa a la izquierda: se centra a ojo.
                            .when(!playing, |el| el.ml(px(2.)))
                            .text_color(hsla(INK)),
                    )
            });
        let skip = |id: &'static str, icon: &'static str, tip: &'static str, delta: i64, cx: &mut Context<Self>| {
            hover::round_button(id, icon, tip, false, hsla(TEXT), hsla(MUTED), cx.listener(
                move |v, _: &ClickEvent, _, cx| v.nudge(delta, cx),
            ))
        };
        let time = |ms: u64, color: u32| {
            div()
                .flex_none()
                .font_family("Cascadia Mono")
                .text_size(px(12.))
                .text_color(hsla(color))
                .child(text::clock(ms as i64))
        };

        let bar_cell = self.player.bar_bounds.clone();
        let mut bar = div()
            .relative()
            .h(px(BAR_H))
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(8.))
            .pr(px(10.))
            .rounded(px(BAR_H / 2.))
            .bg(hsla(ITEM))
            .child(
                canvas(move |bounds, _, _| bar_cell.set(Some(bounds)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(play)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(px(2.))
                    .child(skip("player-back", "icons/rotate-ccw.svg", "15 s atrás (⇧←)", -15_000, cx))
                    .child(skip("player-fwd", "icons/rotate-cw.svg", "15 s adelante (⇧→)", 15_000, cx)),
            )
            .child(time(position, MUTED))
            .child(self.waveform(info, track, played, duration, cx))
            .child(time(duration, FAINT))
            .child(self.speed_button(cx));
        if let Some(error) = error {
            bar = bar.child(
                div()
                    .flex_none()
                    .max_w(px(220.))
                    .truncate()
                    .text_size(px(12.))
                    .text_color(hsla(super::RED))
                    .child(error),
            );
        }
        if options.len() > 1 {
            let mut seg = div().flex().flex_none().gap(px(2.)).p(px(3.)).rounded(px(15.)).bg(hsla(0x1f1f1d));
            for option in options {
                let on = option == track;
                let id: SharedString = format!("player-track-{}", option.label()).into();
                seg = seg.child(
                    div()
                        .id(ElementId::Name(id.clone()))
                        .h(px(24.))
                        .px(px(11.))
                        .flex()
                        .items_center()
                        .rounded(px(12.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .when(!on, |el| el.cursor_pointer())
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.set_track(option, cx)))
                        .child(option.label())
                        .fx(ElementId::Name(format!("{id}-fx").into()), move |el, h| {
                            if on {
                                el.bg(hsla(0x3a3a37)).text_color(hsla(TEXT))
                            } else {
                                el.bg(h.mix(hsla(0x3a3a37).opacity(0.0), hsla(SURFACE_ON)))
                                    .text_color(h.mix(hsla(MUTED), hsla(TEXT)))
                            }
                        }),
                );
            }
            bar = bar.child(seg);
        }
        Some(div().flex_none().px(px(12.)).pb(px(12.)).child(bar).into_any_element())
    }

    fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        let speed = stretch::next_speed(session_speed());
        SPEED.store(speed.to_bits(), Ordering::Relaxed);
        self.player.engine.set_speed(speed);
        cx.notify();
    }

    /// «1×»: cada clic pasa a la siguiente velocidad. Apagado a 1×; a otra
    /// queda encendido, para que se note que no es la normal.
    fn speed_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let speed = session_speed();
        let on = !stretch::is_normal(speed);
        div()
            .id("player-speed")
            // Ancho fijo: «1,25×» no empuja la onda.
            .w(px(48.))
            .h(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(13.))
            .cursor_pointer()
            .font_family("Cascadia Mono")
            .text_size(px(12.))
            .tooltip(hover::tip("Velocidad · clic para cambiar"))
            .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.cycle_speed(cx)))
            .child(stretch::speed_label(speed))
            .fx("player-speed-fx", move |el, h| {
                let rest = if on { hsla(0x3a3a37) } else { hsla(0x3a3a37).opacity(0.0) };
                el.bg(mix(rest, if on { hsla(0x444440) } else { hsla(SURFACE_ON) }, h.t))
                    .text_color(if on { hsla(TEXT) } else { h.mix(hsla(MUTED), hsla(TEXT)) })
            })
    }

    /// Soltar sobre la onda: salta ahí, sin cambiar si suena o no.
    fn scrub_end(&mut self, f: f32, duration: u64, cx: &mut Context<Self>) {
        if self.player.scrub.take().is_none() {
            return;
        }
        let playing = self.is_loaded() && self.player.engine.playing();
        self.seek_to((f as f64 * duration as f64) as u64, playing, cx);
    }

    /// La onda: barras finas con lo ya oído en claro. Clic o arrastre salta.
    fn waveform(&self, info: &Info, track: Track, played: f32, duration: u64, cx: &mut Context<Self>) -> impl IntoElement {
        // Las pistas que suenan; en «Todos», el máximo de las dos.
        let peaks: Arc<Vec<f32>> = Arc::new(match track {
            Track::Mic => info.mic.clone(),
            Track::System => info.system.clone(),
            Track::Mix => {
                let n = info.mic.len().max(info.system.len());
                (0..n)
                    .map(|i| {
                        info.mic.get(i).copied().unwrap_or(0.0).max(info.system.get(i).copied().unwrap_or(0.0))
                    })
                    .collect()
            }
        });
        let waves_at = info.waves_at;
        let cell = self.player.wave_bounds.clone();
        let dragging = self.player.scrub.is_some();
        let view = cx.entity().downgrade();
        let frac = |bounds: Option<Bounds<Pixels>>, x: Pixels| {
            bounds.map_or(0.0, |b| ((x - b.left()) / b.size.width).clamp(0.0, 1.0))
        };
        let down_cell = cell.clone();
        let up_cell = cell.clone();
        let move_cell = cell.clone();
        let bar = self.player.bar_bounds.clone();
        // Bajo el cursor (o donde se arrastra): la hora y lo que se dice ahí.
        let pointer = self.player.scrub.or(self.player.hover_at).filter(|_| duration > 0);
        let globe = pointer.map(|f| {
            let ms = (f as f64 * duration as f64) as i64;
            let said = self.detail.as_ref().and_then(|detail| {
                let starts: Vec<i64> = detail.blocks.iter().map(|b| b.start_ms).collect();
                let block = &detail.blocks[block_at(&starts, ms)?];
                // De sobra para una línea; el ancho exacto se ajusta al pintar.
                Some((SharedString::from(block.label.clone()), block.me, snippet(&block.text, 160)))
            });
            Globe { time: text::clock(ms).into(), said }
        });
        div()
            .id("player-wave")
            .flex_1()
            .min_w(px(60.))
            .h(px(WAVE_H))
            .cursor_pointer()
            // Solo se redibuja mientras el cursor se mueve encima.
            .on_mouse_move(cx.listener(move |v, e: &MouseMoveEvent, _, cx| {
                let f = frac(move_cell.get(), e.position.x);
                if v.player.hover_at != Some(f) {
                    v.player.hover_at = Some(f);
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |v, e: &MouseDownEvent, _, cx| {
                    if duration == 0 {
                        return;
                    }
                    v.player.scrub = Some(frac(down_cell.get(), e.position.x));
                    cx.notify();
                }),
            )
            // Un clic rápido puede soltarse antes del cuadro que escucha afuera.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |v, e: &MouseUpEvent, _, cx| {
                    let f = frac(up_cell.get(), e.position.x);
                    v.scrub_end(f, duration, cx);
                }),
            )
            // Arrastrar sigue aunque el cursor salga de la onda.
            .when(dragging, |el| {
                let (move_view, up_view) = (view.clone(), view);
                let bounds = self.player.wave_bounds.clone();
                let up_bounds = bounds.clone();
                el.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    let f = frac(bounds.get(), e.position.x);
                                    let _ = move_view.update(cx, |v, cx| {
                                        v.player.scrub = Some(f);
                                        cx.notify();
                                    });
                                }
                            });
                            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    let f = frac(up_bounds.get(), e.position.x);
                                    let _ = up_view.update(cx, |v, cx| v.scrub_end(f, duration, cx));
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_0(),
                )
            })
            .fx("player-wave-fx", move |el, h| {
                el.child(
                    canvas(
                        move |bounds, _, _| cell.set(Some(bounds)),
                        move |bounds, _, window, cx| {
                            // La onda entra fundiéndose cuando termina de leerse.
                            let fade = waves_at.map_or(0.0, |t| (t.elapsed().as_secs_f32() / 0.35).min(1.0));
                            if fade < 1.0 && waves_at.is_some() {
                                window.request_animation_frame();
                            }
                            let step = WAVE_BAR + WAVE_GAP;
                            let n = ((f32::from(bounds.size.width) + WAVE_GAP) / step).floor().max(1.0) as usize;
                            let bars = player::bucket(&peaks, n);
                            let head = bounds.left() + bounds.size.width * played;
                            let lit = hsla(TEXT).opacity(0.92);
                            let dim = mix(hsla(FAINT).opacity(0.55), hsla(MUTED).opacity(0.7), h.t);
                            let mid = bounds.top() + bounds.size.height / 2.0;
                            for i in 0..n {
                                // Sin onda todavía: una línea de puntos a ras.
                                let v = bars.get(i).copied().unwrap_or(0.0) * fade;
                                let height = (f32::from(bounds.size.height) * v).max(WAVE_BAR);
                                let x = bounds.left() + px(i as f32 * step);
                                let color = if x + px(WAVE_BAR / 2.) <= head { lit } else { dim };
                                window.paint_quad(
                                    fill(
                                        Bounds::new(point(x, mid - px(height / 2.)), size(px(WAVE_BAR), px(height))),
                                        color,
                                    )
                                    .corner_radii(px(WAVE_BAR / 2.)),
                                );
                            }
                            // La línea bajo el cursor y el globito, fundidos con el hover.
                            let shown = if dragging { 1.0 } else { h.over };
                            if let (Some(f), Some(globe)) = (pointer, globe.as_ref()) {
                                if shown > 0.0 {
                                    let x = (bounds.left() + bounds.size.width * f).round();
                                    window.paint_quad(fill(
                                        Bounds::new(point(x - px(0.5), bounds.top()), size(px(1.), bounds.size.height)),
                                        hsla(TEXT).opacity(0.6 * shown),
                                    ));
                                    paint_globe(globe, x, bounds, bar.get(), shown, window, cx);
                                }
                            }
                        },
                    )
                    .size_full(),
                )
            })
    }
}

/// El globito sobre la barra, centrado en `x` sin salirse de la pastilla:
/// la hora y, debajo, el hablante (en su color) y el comienzo de lo que dice,
/// recortado a una línea. Se pinta a mano para medir el texto en el mismo
/// cuadro y no saltar cuando cambia de ancho.
fn paint_globe(globe: &Globe, x: Pixels, wave: Bounds<Pixels>, bar: Option<Bounds<Pixels>>, alpha: f32, window: &mut Window, cx: &mut App) {
    const PAD_X: f32 = 10.;
    const PAD_Y: f32 = 7.;
    const LINE_H: f32 = 17.;
    const SIZE: f32 = 12.;
    let area = bar.unwrap_or(wave);
    let base = window.text_style().font();
    let run = |len: usize, font: Font, color: Hsla| TextRun {
        len,
        font,
        color: color.opacity(color.a * alpha),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let system = window.text_system().clone();
    let time_line = system.shape_line(
        globe.time.clone(),
        px(SIZE),
        &[run(globe.time.len(), gpui::font("Cascadia Mono"), hsla(TEXT))],
        None,
    );
    let room = (area.size.width - px(8. + 2. * PAD_X)).min(px(320.));
    let said = globe.said.as_ref().map(|(label, me, text)| {
        let name = Font { weight: FontWeight::SEMIBOLD, ..base.clone() };
        let shape = |n: usize| {
            let line: SharedString = format!("{label}  {}", snippet(text, n)).into();
            let runs = [
                run(label.len(), name.clone(), hsla(if *me { BLUE } else { LILAC })),
                run(line.len() - label.len(), base.clone(), hsla(BODY)),
            ];
            system.shape_line(line, px(SIZE), &runs, None)
        };
        let total = text.chars().count();
        let full = shape(total);
        if full.width <= room {
            full
        } else {
            shape(longest_fit(total, |n| shape(n).width <= room))
        }
    });
    let inner = said.as_ref().map_or(time_line.width, |l| l.width.max(time_line.width));
    let width = inner + px(2. * PAD_X);
    let lines = if said.is_some() { 2. } else { 1. };
    let height = px(2. * PAD_Y + LINE_H * lines);
    let left = centered_within(
        f32::from(x),
        f32::from(width),
        f32::from(area.left()) + 4.,
        f32::from(area.right()) - 4.,
    );
    // Encima de la pastilla, sin tapar los tiempos; entra subiendo un poco.
    let bottom = area.top() - px(8.) + px(3. * (1. - alpha));
    let origin = point(px(left), bottom - height);
    window.paint_quad(
        fill(Bounds::new(origin, size(width, height)), hsla(GLOBE).opacity(alpha))
            .corner_radii(px(if said.is_some() { 12. } else { 10. })),
    );
    let text_at = point(origin.x + px(PAD_X), origin.y + px(PAD_Y));
    let _ = time_line.paint(text_at, px(LINE_H), window, cx);
    if let Some(said) = said {
        let _ = said.paint(point(text_at.x, text_at.y + px(LINE_H)), px(LINE_H), window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_bloque_que_suena() {
        let starts = [0, 6_000, 14_500];
        assert_eq!(block_at(&starts, 0), Some(0));
        assert_eq!(block_at(&starts, 5_999), Some(0));
        assert_eq!(block_at(&starts, 6_000), Some(1));
        assert_eq!(block_at(&starts, 99_000), Some(2));
        assert_eq!(block_at(&[2_000], 1_000), None);
        assert_eq!(block_at(&[], 1_000), None);
    }

    #[test]
    fn saltar_no_se_sale_de_la_grabacion() {
        assert_eq!(nudge(3_000, -5_000, 60_000), 0);
        assert_eq!(nudge(58_000, 5_000, 60_000), 60_000);
        assert_eq!(nudge(10_000, 15_000, 60_000), 25_000);
        assert_eq!(nudge(10_000, 5_000, 0), 15_000);
    }

    #[test]
    fn el_globito_muestra_el_bloque_que_contiene_la_hora() {
        // Lo mismo que hace la onda: la hora bajo el cursor -> su bloque.
        let starts = [0, 6_000, 14_500];
        let at = |f: f64, duration: u64| block_at(&starts, (f * duration as f64) as i64);
        assert_eq!(at(0.0, 20_000), Some(0));
        assert_eq!(at(0.5, 20_000), Some(1));
        assert_eq!(at(1.0, 20_000), Some(2));
        // Antes del primer bloque no se dice nada.
        assert_eq!(block_at(&[3_000], 1_000), None);
    }

    #[test]
    fn el_recorte_corta_en_palabra_entera() {
        assert_eq!(snippet("  hola\n  mundo ", 40), "hola mundo");
        assert_eq!(snippet("vamos a revisar el presupuesto", 18), "vamos a revisar…");
        // Una palabra larguísima se corta igual.
        assert_eq!(snippet("supercalifragilístico", 6), "superc…");
        // Sin dejar comas colgando antes de «…».
        assert_eq!(snippet("bueno, entonces sí", 9), "bueno…");
        assert_eq!(snippet("", 5), "");
    }

    #[test]
    fn el_ajuste_al_ancho_busca_el_mayor_que_cabe() {
        assert_eq!(longest_fit(100, |n| n <= 37), 37);
        assert_eq!(longest_fit(100, |_| true), 100);
        assert_eq!(longest_fit(100, |n| n == 0), 0);
        assert_eq!(longest_fit(0, |_| true), 0);
    }

    #[test]
    fn el_globito_no_se_sale_de_la_barra() {
        assert_eq!(centered_within(100.0, 40.0, 0.0, 300.0), 80.0);
        assert_eq!(centered_within(5.0, 40.0, 0.0, 300.0), 0.0);
        assert_eq!(centered_within(295.0, 40.0, 0.0, 300.0), 260.0);
        // Más ancho que el espacio: pegado al comienzo.
        assert_eq!(centered_within(50.0, 400.0, 0.0, 300.0), 0.0);
    }

    #[test]
    fn copiar_lleva_el_hablante() {
        let block = Block { me: true, label: "Tú".into(), start_ms: 0, text: " hola \n".into() };
        assert_eq!(copy_text(&block), "Tú: hola");
    }
}
