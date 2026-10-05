//! Grabar desde la ventana de Reuniones, como `start_capture`/`stop_capture`
//! de `apps/desktop/src-tauri/src/state.rs`: la misma configuración de Atic
//! (`config.json`: pistas, micrófono, supresión de ruido, subtítulos), los
//! mismos WAV (`recordings/<id>/{mic,system}.wav`) y la misma fila en la base.
//!
//! Abrir y cerrar el audio (enumerar dispositivos, esperar a que el stream
//! arranque, cerrar los WAV, escribir la base) va fuera del hilo de UI. Lo que
//! se mueve mientras se graba (cronómetro, niveles, el punto que late) pide
//! cuadros a ~20 fps solo mientras hay grabación; quieto, nada.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use atic_audio::{CaptureConfig, CaptureEvent, CaptureHandle, CaptureSession, CaptureSummary};
use atic_core::{Recording, RecordingStatus};
use chrono::{DateTime, Local, Utc};
use gpui::{
    actions, div, prelude::*, px, AnyElement, App, ClickEvent, Context, FontWeight,
    KeyBinding, SharedString,
};

use super::data::Paths;
use super::live::{self, Plan};
use super::{chime, hsla, text, MeetingsView};
use super::{BLUE, FAINT, ITEM, LILAC, MUTED, R_PANEL, RED, SURFACE_ON, TEXT};
use crate::hover::{self, HoverExt};

actions!(meetings, [ToggleRecording]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("ctrl-r", ToggleRecording, Some(super::KEY_CONTEXT))]);
}

/// Cada cuánto se redibuja mientras se graba: basta para que los medidores
/// se sientan vivos sin redibujar la ventana entera a 60 fps una hora.
const FRAME: Duration = Duration::from_millis(50);
/// Margen de la tarjeta dentro del panel de la lista: su radio es el del
/// panel menos este margen, para que las esquinas queden paralelas.
const CARD_INSET: f32 = 8.0;
const R_INNER: f32 = R_PANEL - CARD_INSET;
/// Segmentos de cada medidor.
const SEGMENTS: usize = 22;
/// Subtítulos a la vista (más la parcial).
const CAPTION_LINES: usize = 3;
/// Bajo este RMS de pico una pista se considera muda (el umbral de Atic).
const SILENT_RMS: f32 = 0.0015;

/// Cómo va la grabación de esta ventana.
#[derive(Default)]
pub struct Recorder {
    phase: Phase,
    /// Lo que quedó de la última grabación (pista muda) o por qué no empezó.
    notice: Option<Notice>,
    /// Los niveles a la vista, suavizados (0..1).
    shown: (f32, f32),
    ticking: bool,
    _quit: Option<gpui::Subscription>,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    /// El micrófono es Bluetooth y abrirlo pasa los audífonos a manos
    /// libres: se pregunta antes de grabar.
    Confirm(String),
    Starting,
    Recording(Box<Active>),
    /// Cerrando los WAV y escribiendo la base; el cronómetro queda quieto.
    Stopping(Duration),
}

struct Notice {
    text: String,
    error: bool,
}

/// Lo que la captura publica desde su hilo: niveles RMS y avisos no fatales.
#[derive(Default)]
struct Levels {
    mic: AtomicU32,
    system: AtomicU32,
    warning: Mutex<Option<String>>,
}

/// Una grabación en curso. Si se suelta sin `finish` (se cerró la ventana o
/// la app grabando), se detiene y se guarda igual: el audio no se pierde.
struct Active {
    handle: Option<CaptureHandle>,
    recording: Recording,
    paths: Paths,
    started: Instant,
    levels: Arc<Levels>,
    tracks: Tracks,
    live: Option<live::Worker>,
    /// Aviso de la tarjeta: subtítulos que no corren, Bluetooth aceptado.
    note: Option<String>,
    beep: Option<Beep>,
}

#[derive(Clone)]
struct Beep {
    stop_voice: String,
    output: String,
}

struct Saved {
    id: String,
    warning: Option<&'static str>,
}

enum StartError {
    Bluetooth(String),
    Failed(String),
}

impl Active {
    /// Detiene la captura y deja la fila en la base. Una sola vez.
    fn finish(&mut self) -> Option<Result<Saved, String>> {
        let handle = self.handle.take()?;
        // Primero el audio: los subtítulos no tienen que demorar el cierre.
        let summary = handle.stop();
        self.live = None;
        let mut rec = self.recording.clone();
        rec.duration_secs = summary.duration_secs.round() as i64;
        if !summary.mic_written {
            rec.mic_path = None;
        }
        if !summary.system_written {
            rec.system_path = None;
        }
        rec.status = RecordingStatus::Recorded;
        let saved = self
            .paths
            .db()
            .and_then(|db| Ok(db.insert_recording(&rec)?))
            .map(|()| Saved { id: rec.id.clone(), warning: silent_warning(&summary) })
            .map_err(|e| e.to_string());
        if let Some(beep) = &self.beep {
            chime::play(chime::SoundAction::RecordingStop, &beep.stop_voice, &beep.output);
        }
        Some(saved)
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        if let Some(Err(error)) = self.finish() {
            eprintln!("reuniones: no se pudo guardar la grabación al cerrar: {error}");
        }
    }
}

// --- Lógica pura ------------------------------------------------------------------

/// El título por omisión, con la hora local (el de `Recording::new` usa UTC).
/// Mismo formato que Atic, para que la lista se lea pareja.
pub fn local_title(started: DateTime<Local>) -> String {
    format!("Grabación {}", started.format("%Y-%m-%d %H:%M"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tracks {
    pub mic: bool,
    pub system: bool,
}

/// Qué pistas grabar según `effective_record_tracks` (`mic`/`system`/`both`).
pub fn tracks(effective: &str) -> Tracks {
    Tracks {
        mic: matches!(effective, "both" | "mic"),
        system: matches!(effective, "both" | "system"),
    }
}

/// Las pistas que quedaron mudas, como avisa Atic (con menos de 2 s no se
/// juzga: no alcanzó a entrar nada).
pub fn silent_warning(summary: &CaptureSummary) -> Option<&'static str> {
    if summary.duration_secs < 2.0 {
        return None;
    }
    let mic = summary.mic_written && summary.mic_peak_rms < SILENT_RMS;
    let system = summary.system_written && summary.system_peak_rms < SILENT_RMS;
    Some(match (mic, system) {
        (true, true) => "Las dos pistas quedaron casi en silencio. Revisa el micrófono y la salida en Ajustes.",
        (true, false) => "El micrófono quedó casi en silencio. Revisa el micrófono en Ajustes.",
        (false, true) => "La pista de los demás quedó casi en silencio. Revisa que el audio del computador esté sonando.",
        (false, false) => return None,
    })
}

/// El cronómetro: «03:12» o, pasada la hora, «1:03:12».
pub fn stopwatch(elapsed: Duration) -> String {
    text::clock(elapsed.as_millis() as i64)
}

/// RMS lineal → 0..1 a oído: el habla vive entre −60 y −20 dB (como el
/// `Waveform` de Atic); lineal, el medidor quedaría casi plano.
pub fn meter(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 40.0).clamp(0.0, 1.0)
}

/// Sube rápido y cae despacio, como un vúmetro.
fn ease(shown: f32, target: f32) -> f32 {
    let k = if target > shown { 0.6 } else { 0.22 };
    shown + (target - shown) * k
}

fn first_upper(text: String) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

// --- Arrancar (fuera del hilo de UI) ----------------------------------------------

fn begin(paths: Paths, allow_bluetooth: bool) -> Result<Active, StartError> {
    let config = atic_core::Config::load(&paths.config_path());
    let tracks = tracks(config.effective_record_tracks());
    let mut note = None;
    if !allow_bluetooth {
        if let Ok(preflight) = atic_audio::audio_preflight(
            tracks.mic,
            tracks.system,
            &config.mic_device_id,
            &config.output_device_id,
            false,
        ) {
            if preflight.risk == "bluetooth_hands_free" {
                return Err(StartError::Bluetooth(preflight.message.unwrap_or_else(|| {
                    "El micrófono Bluetooth pasará los audífonos a manos libres.".into()
                })));
            }
        }
    } else {
        note = Some("Grabando con el micrófono Bluetooth en manos libres.".to_string());
    }

    let plan = live::plan(
        config.live_transcription,
        &config.live_engine,
        config.live_transcription.then(live::groq_key).flatten(),
        &config.live_groq_model,
    );

    let mut recording = Recording::new(Utc::now());
    recording.title = local_title(recording.started_at.with_timezone(&Local));
    let dir = paths.recording_dir(&recording.id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| StartError::Failed(format!("No se pudo crear la carpeta: {e}")))?;

    // El tap no bloquea: si los subtítulos se atrasan, se pierden trozos
    // de la vista previa, nunca del WAV.
    let (tap, tap_rx) = match plan {
        Plan::Groq { .. } => {
            let (tx, rx) = mpsc::sync_channel(512);
            (Some(tx), Some(rx))
        }
        _ => (None, None),
    };
    let (events, events_rx) = mpsc::channel();
    let handle = CaptureSession::start(
        CaptureConfig {
            mic_wav: dir.join("mic.wav"),
            system_wav: dir.join("system.wav"),
            capture_mic: tracks.mic,
            capture_system: tracks.system,
            noise_suppression: config.noise_suppression.clone(),
            mic_device_id: config.mic_device_id.clone(),
            output_device_id: config.output_device_id.clone(),
            stt_tap: tap,
            english: false,
        },
        events,
    )
    .map_err(|e| StartError::Failed(first_upper(e.to_ui(false))))?;

    // Los eventos de la captura, a atómicos que el render lee sin esperar.
    // El hilo termina solo cuando la captura suelta el canal.
    let levels = Arc::new(Levels::default());
    let levels_bg = levels.clone();
    std::thread::Builder::new()
        .name("meetings-levels".into())
        .spawn(move || {
            for event in events_rx {
                match event {
                    CaptureEvent::Levels { mic, system } => {
                        levels_bg.mic.store(mic.to_bits(), Ordering::Relaxed);
                        levels_bg.system.store(system.to_bits(), Ordering::Relaxed);
                    }
                    CaptureEvent::Error(message) => {
                        tracing::warn!(%message, "aviso de captura");
                        if let Ok(mut warning) = levels_bg.warning.lock() {
                            *warning = Some(first_upper(message));
                        }
                    }
                }
            }
        })
        .ok();

    let live = match (plan, tap_rx) {
        (Plan::Groq { key, model }, Some(rx)) => {
            match live::Worker::spawn(rx, config.whisper_language(), key, model) {
                Ok(worker) => Some(worker),
                Err(error) => {
                    note = Some(format!("Sin subtítulos: {error}"));
                    None
                }
            }
        }
        (Plan::Unavailable(why), _) => {
            note = Some(why.to_string());
            None
        }
        _ => None,
    };

    recording.mic_path = tracks.mic.then(|| "mic.wav".into());
    recording.system_path = tracks.system.then(|| "system.wav".into());
    let beep = config.beep_on_start.then(|| Beep {
        stop_voice: config.sound_recording_stop.clone(),
        output: config.output_device_id.clone(),
    });
    if config.beep_on_start {
        chime::play(
            chime::SoundAction::RecordingStart,
            &config.sound_recording_start,
            &config.output_device_id,
        );
    }
    Ok(Active {
        handle: Some(handle),
        recording,
        paths,
        started: Instant::now(),
        levels,
        tracks,
        live,
        note,
        beep,
    })
}

// --- La vista ---------------------------------------------------------------------

impl Recorder {
    pub fn recording(&self) -> bool {
        matches!(self.phase, Phase::Recording(_))
    }

    /// Un cuadro mientras se graba: los niveles a la vista se acercan a los
    /// reales. Devuelve si hay que seguir.
    fn tick(&mut self) -> bool {
        let Phase::Recording(active) = &self.phase else {
            self.shown = (0.0, 0.0);
            return false;
        };
        let read = |a: &AtomicU32| meter(f32::from_bits(a.load(Ordering::Relaxed)));
        let (mic, system) = (read(&active.levels.mic), read(&active.levels.system));
        self.shown = (ease(self.shown.0, mic), ease(self.shown.1, system));
        true
    }
}

impl MeetingsView {
    /// Lo que hay que hacer al abrir la ventana: si la app se cierra
    /// grabando, se guarda antes de salir.
    pub(super) fn install_recorder(&mut self, cx: &mut Context<Self>) {
        self.recorder._quit = Some(cx.on_app_quit(|view, _| {
            if let Phase::Recording(active) = &mut view.recorder.phase {
                active.finish();
            }
            async {}
        }));
    }

    fn can_record(&self) -> bool {
        self.source.as_ref().is_some_and(|s| s.paths().is_some())
    }

    pub(super) fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        match self.recorder.phase {
            Phase::Idle | Phase::Confirm(_) => self.start_recording(false, cx),
            Phase::Recording(_) => self.stop_recording(cx),
            Phase::Starting | Phase::Stopping(_) => {}
        }
    }

    fn start_recording(&mut self, allow_bluetooth: bool, cx: &mut Context<Self>) {
        if !matches!(self.recorder.phase, Phase::Idle | Phase::Confirm(_)) {
            return;
        }
        let Some(paths) = self.source.as_ref().and_then(|s| s.paths()).cloned() else {
            return;
        };
        self.recorder.phase = Phase::Starting;
        self.recorder.notice = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { begin(paths, allow_bluetooth) }).await;
            // Si la ventana ya no está, `result` se suelta aquí y `Active`
            // detiene y guarda lo poco que alcanzó a grabar.
            let _ = this.update(cx, |view, cx| view.started(result, cx));
        })
        .detach();
    }

    fn started(&mut self, result: Result<Active, StartError>, cx: &mut Context<Self>) {
        self.recorder.phase = match result {
            Ok(active) => Phase::Recording(Box::new(active)),
            Err(StartError::Bluetooth(message)) => Phase::Confirm(message),
            Err(StartError::Failed(text)) => {
                self.recorder.notice = Some(Notice { text, error: true });
                Phase::Idle
            }
        };
        if self.recorder.recording() && !self.recorder.ticking {
            self.recorder.ticking = true;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(FRAME).await;
                let go = this
                    .update(cx, |view, cx| {
                        let go = view.recorder.tick();
                        view.recorder.ticking = go;
                        cx.notify();
                        go
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            })
            .detach();
        }
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        let elapsed = match &self.recorder.phase {
            Phase::Recording(active) => active.started.elapsed(),
            _ => return,
        };
        let Phase::Recording(mut active) =
            std::mem::replace(&mut self.recorder.phase, Phase::Stopping(elapsed))
        else {
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { active.finish().unwrap_or(Err("ya estaba detenida".into())) })
                .await;
            let _ = this.update(cx, |view, cx| view.saved(result, cx));
        })
        .detach();
    }

    fn saved(&mut self, result: Result<Saved, String>, cx: &mut Context<Self>) {
        self.recorder.phase = Phase::Idle;
        match result {
            Ok(saved) => {
                self.recorder.notice =
                    saved.warning.map(|text| Notice { text: text.into(), error: false });
                self.reload(cx);
                self.after_recording(&saved.id, cx);
            }
            Err(error) => {
                self.recorder.notice = Some(Notice {
                    text: format!("El audio quedó en su carpeta, pero no se pudo guardar en la lista: {error}"),
                    error: true,
                });
            }
        }
        cx.notify();
    }

    /// Recién guardada una grabación: se elige y, si así está configurado
    /// (`auto_transcribe_after_recording`), se transcribe al tiro.
    pub(super) fn after_recording(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.items.iter().position(|r| r.id == id) {
            self.select(ix, cx);
        }
        let auto = self
            .source
            .as_ref()
            .and_then(|s| s.paths())
            .is_some_and(|p| atic_core::Config::load(&p.config_path()).auto_transcribe_after_recording);
        if auto {
            self.start_transcribe(id.to_string(), cx);
        }
    }

    // --- Dibujo -------------------------------------------------------------------

    /// El botón de la barra de arriba: «Grabar» o «Detener · 03:12».
    pub(super) fn record_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.can_record() {
            return None;
        }
        let (label, clock, live, busy) = match &self.recorder.phase {
            Phase::Idle | Phase::Confirm(_) => ("Grabar", None, false, false),
            Phase::Starting => ("Preparando…", None, false, true),
            Phase::Recording(active) => ("Detener", Some(stopwatch(active.started.elapsed())), true, false),
            Phase::Stopping(_) => ("Guardando…", None, true, true),
        };
        let pulse = self.pulse();
        let (rest, over) = if live {
            (hsla(RED).opacity(0.16), hsla(RED).opacity(0.24))
        } else {
            (hsla(ITEM), hsla(SURFACE_ON))
        };
        Some(
            div()
                .id("record")
                .h(px(30.))
                .pl(px(12.))
                .pr(px(14.))
                .mr(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .rounded(px(15.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .when(!busy, |el| {
                    el.cursor_pointer()
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_recording(cx)))
                        .tooltip(hover::tip(if live { "Detener (Ctrl+R)" } else { "Grabar (Ctrl+R)" }))
                })
                .when(busy, |el| el.text_color(hsla(MUTED)))
                .child(div().size(px(8.)).flex_none().rounded_full().bg(hsla(RED).opacity(pulse)))
                .child(label)
                // Cifras de ancho fijo: el botón no tiembla cada segundo.
                .when_some(clock, |el, clock| {
                    el.child(div().text_color(hsla(MUTED)).child("·"))
                        .child(div().font_family("Cascadia Mono").child(clock))
                })
                .hover_bg("record-fx", rest, over)
                .into_any_element(),
        )
    }

    /// El punto rojo late suave mientras se graba; si no, quieto y entero.
    fn pulse(&self) -> f32 {
        match &self.recorder.phase {
            Phase::Recording(active) => {
                let t = active.started.elapsed().as_secs_f32();
                0.55 + 0.45 * (0.5 + 0.5 * (t * std::f32::consts::TAU / 1.6).cos())
            }
            _ => 1.0,
        }
    }

    /// Lo de arriba de la lista: la grabación en curso, la pregunta del
    /// Bluetooth o el aviso de la última.
    pub(super) fn live_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = || {
            div()
                .flex_none()
                .m(px(CARD_INSET))
                .mb(px(0.))
                .p(px(16.))
                .rounded(px(R_INNER))
                .bg(hsla(SURFACE_ON))
                .flex()
                .flex_col()
                .gap(px(12.))
        };
        match &self.recorder.phase {
            Phase::Recording(active) => Some(self.recording_card(card(), active).into_any_element()),
            Phase::Starting | Phase::Stopping(_) => {
                let (title, clock) = match &self.recorder.phase {
                    Phase::Stopping(elapsed) => ("Guardando…", stopwatch(*elapsed)),
                    _ => ("Abriendo el audio…", stopwatch(Duration::ZERO)),
                };
                Some(
                    card()
                        .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(title))
                        .child(big_clock(clock, hsla(MUTED)))
                        .into_any_element(),
                )
            }
            Phase::Confirm(message) => Some(
                card()
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("¿Grabar con Bluetooth?"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(hsla(MUTED))
                            .child(SharedString::from(message.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(small_button(
                                "bt-go",
                                "Grabar igual",
                                true,
                                cx.listener(|v, _: &ClickEvent, _, cx| v.start_recording(true, cx)),
                            ))
                            .child(small_button(
                                "bt-cancel",
                                "Cancelar",
                                false,
                                cx.listener(|v, _: &ClickEvent, _, cx| {
                                    v.recorder.phase = Phase::Idle;
                                    cx.notify();
                                }),
                            )),
                    )
                    .into_any_element(),
            ),
            Phase::Idle => {
                let notice = self.recorder.notice.as_ref()?;
                Some(
                    card()
                        .py(px(12.))
                        .pr(px(10.))
                        .flex_row()
                        .items_start()
                        .gap(px(10.))
                        .child(
                            div()
                                .mt(px(6.))
                                .size(px(6.))
                                .flex_none()
                                .rounded_full()
                                .bg(hsla(if notice.error { RED } else { super::AMBER })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(hsla(super::BODY))
                                .child(SharedString::from(notice.text.clone())),
                        )
                        .child(hover::round_button(
                            "notice-close",
                            "icons/x.svg",
                            "",
                            false,
                            hsla(TEXT),
                            hsla(FAINT),
                            cx.listener(|v, _: &ClickEvent, _, cx| {
                                v.recorder.notice = None;
                                cx.notify();
                            }),
                        ))
                        .into_any_element(),
                )
            }
        }
    }

    fn recording_card(&self, card: gpui::Div, active: &Active) -> gpui::Div {
        let (mic, system) = self.recorder.shown;
        let warning = active.levels.warning.lock().ok().and_then(|w| w.clone());
        let note = warning.or_else(|| active.note.clone());
        let mut card = card
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(8.)).flex_none().rounded_full().bg(hsla(RED).opacity(self.pulse())))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child("Grabando"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(hsla(FAINT))
                            .child(SharedString::from(active.recording.title.clone())),
                    ),
            )
            .child(big_clock(stopwatch(active.started.elapsed()), hsla(TEXT)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(meter_row("Yo", mic, active.tracks.mic, BLUE))
                    .child(meter_row("Los demás", system, active.tracks.system, LILAC)),
            );
        if let Some(note) = note {
            card = card.child(div().text_size(px(12.)).line_height(px(17.)).text_color(hsla(FAINT)).child(note));
        }
        if let Some(worker) = &active.live {
            card = card.child(captions(worker));
        }
        card
    }

    /// Sin reuniones todavía: invita a grabar la primera.
    pub(super) fn invite(&self, cx: &mut Context<Self>) -> AnyElement {
        let can = self.can_record();
        div()
            .flex_1()
            .py(px(48.))
            .px(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child("Todavía no hay reuniones"))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .child(match (can, self.recorder.recording()) {
                        (true, true) => "Aparecerá aquí al detenerla.",
                        (true, false) => "Graba la primera; Ctrl+R también sirve.",
                        (false, _) => "Graba una desde Atic y aparecerá aquí.",
                    }),
            )
            .when(can && !self.recorder.recording(), |el| {
                el.child(div().pt(px(10.)).child(small_button(
                    "invite-record",
                    "Grabar",
                    true,
                    cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_recording(cx)),
                )))
            })
            .into_any_element()
    }
}

fn big_clock(clock: String, color: gpui::Hsla) -> impl IntoElement {
    div()
        .font_family("Cascadia Mono")
        .text_size(px(30.))
        .line_height(px(34.))
        .text_color(color)
        .child(clock)
}

/// Un medidor plano por segmentos; sin pista, la tira apagada.
fn meter_row(label: &'static str, level: f32, on: bool, color: u32) -> impl IntoElement {
    let lit = if on { (level * SEGMENTS as f32).round() as usize } else { 0 };
    let mut bar = div().flex_1().flex().items_center().gap(px(2.));
    for n in 0..SEGMENTS {
        bar = bar.child(
            div()
                .flex_1()
                .h(px(6.))
                .rounded(px(3.))
                .bg(if n < lit { hsla(color) } else { hsla(TEXT).opacity(0.07) }),
        );
    }
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .w(px(68.))
                .flex_none()
                .text_size(px(12.))
                .text_color(hsla(if on { MUTED } else { FAINT }))
                .child(label),
        )
        .child(if on {
            bar.into_any_element()
        } else {
            div().flex_1().text_size(px(12.)).text_color(hsla(FAINT)).child("No se graba").into_any_element()
        })
}

/// Las últimas líneas; la parcial, atenuada.
fn captions(worker: &live::Worker) -> impl IntoElement {
    let (lines, partial, error) = match worker.captions.lock() {
        Ok(c) => {
            let skip = c.lines.len().saturating_sub(CAPTION_LINES);
            (c.lines.iter().skip(skip).cloned().collect::<Vec<_>>(), c.partial.clone(), c.error.clone())
        }
        Err(_) => (Vec::new(), None, None),
    };
    let mut column = div().flex().flex_col().gap(px(6.)).pt(px(2.));
    if lines.is_empty() && partial.is_none() {
        column = column.child(
            div()
                .text_size(px(12.))
                .text_color(hsla(FAINT))
                .child(error.unwrap_or_else(|| "Los subtítulos aparecen a los pocos segundos.".into())),
        );
    }
    let row = |line: &live::Line, dim: bool| {
        div()
            .flex()
            .gap(px(8.))
            .text_size(px(13.))
            .line_height(px(19.))
            .child(
                div()
                    .w(px(40.))
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hsla(if line.me { BLUE } else { LILAC }).opacity(if dim { 0.6 } else { 1.0 }))
                    .child(if line.me { "Yo" } else { "Otros" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(hsla(if dim { FAINT } else { super::BODY }))
                    .child(SharedString::from(line.text.clone())),
            )
    };
    for line in &lines {
        column = column.child(row(line, false));
    }
    if let Some(partial) = &partial {
        column = column.child(row(partial, true));
    }
    column
}

fn small_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> impl IntoElement {
    let (rest, over) = if primary {
        (hsla(RED).opacity(0.18), hsla(RED).opacity(0.28))
    } else {
        (hsla(ITEM), hsla(0x343431))
    };
    div()
        .id(id)
        .h(px(28.))
        .px(px(13.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(on_click)
        .when(primary, |el| el.child(div().size(px(7.)).rounded_full().bg(hsla(RED))))
        .child(label)
        .hover_bg(id, rest, over)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn el_titulo_usa_la_hora_local() {
        let local = Local.with_ymd_and_hms(2026, 10, 4, 18, 42, 7).single().unwrap();
        assert_eq!(local_title(local), "Grabación 2026-10-04 18:42");
    }

    #[test]
    fn las_pistas_salen_de_la_config() {
        assert_eq!(tracks("both"), Tracks { mic: true, system: true });
        assert_eq!(tracks("mic"), Tracks { mic: true, system: false });
        assert_eq!(tracks("system"), Tracks { mic: false, system: true });
        // `effective_record_tracks` ya normaliza; con modo altavoces es «system».
        let mut config = atic_core::Config::default();
        config.record_tracks = "mic".into();
        assert_eq!(tracks(config.effective_record_tracks()), Tracks { mic: true, system: false });
    }

    #[test]
    fn avisa_de_las_pistas_mudas() {
        let mut summary = CaptureSummary {
            duration_secs: 5.0,
            mic_written: true,
            system_written: true,
            mic_peak_rms: 0.0004,
            system_peak_rms: 0.05,
        };
        assert!(silent_warning(&summary).unwrap().starts_with("El micrófono"));
        summary.system_peak_rms = 0.0;
        assert!(silent_warning(&summary).unwrap().starts_with("Las dos"));
        // Pista que no se grabó: no se juzga.
        summary.mic_written = false;
        assert!(silent_warning(&summary).unwrap().starts_with("La pista de los demás"));
        // Muy corta: tampoco.
        summary.duration_secs = 1.5;
        assert!(silent_warning(&summary).is_none());
        summary.duration_secs = 5.0;
        summary.system_peak_rms = 0.02;
        assert!(silent_warning(&summary).is_none());
    }

    #[test]
    fn el_cronometro() {
        assert_eq!(stopwatch(Duration::from_millis(999)), "00:00");
        assert_eq!(stopwatch(Duration::from_secs(192)), "03:12");
        assert_eq!(stopwatch(Duration::from_secs(3600 + 4 * 60 + 12)), "1:04:12");
    }

    #[test]
    fn el_medidor_es_logaritmico_y_acotado() {
        assert_eq!(meter(0.0), 0.0);
        assert_eq!(meter(0.001), 0.0); // −60 dB
        assert!((meter(0.01) - 0.5).abs() < 1e-4); // −40 dB
        assert_eq!(meter(0.5), 1.0);
        // Sube rápido, baja despacio.
        assert!(ease(0.0, 1.0) > 1.0 - ease(1.0, 0.0));
    }
}
