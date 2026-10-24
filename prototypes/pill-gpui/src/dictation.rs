//! Dictado en el notch: grabar el micrófono, transcribir y pegar en la app
//! donde estabas, como `apps/desktop/src-tauri/src/dictation.rs` de Atic.
//!
//! El motor es el de Atic sin Tauri: `atic-audio` graba (con RNNoise),
//! `atic-transcribe` manda el audio a Groq y `atic-core` da la configuración
//! (`config.json`) y la clave de Groq (el llavero), las mismas que eligió el
//! usuario en Ajustes de Atic. En Mac se transcribe por omisión con la voz de
//! macOS, en el equipo y sin llave (`platform::apple`), y Apple Intelligence
//! puede pulir el texto antes de pegarlo. El Whisper local no está.
//!
//! En el notch: mientras escuchas, el tab se ensancha y baja una franja con
//! un punto rojo, el tiempo y la onda del micrófono; después dice
//! «Transcribiendo…» y, al pegar, muestra lo pegado. El atajo es el de Atic
//! (`dictation_shortcut`, por omisión mantener para hablar); un clic en la
//! franja termina el dictado.
//!
//! Con la voz de macOS el dictado va en vivo: el audio del micrófono se le
//! pasa a `atic-apple stream` mientras se graba, la franja muestra lo que va
//! reconociendo (lo provisorio más tenue) y al soltar el texto ya está casi
//! listo. Si el streaming falla, se transcribe el WAV como siempre.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, SharedString};

use crate::anim::segment;
use crate::paste::Target;
use crate::Pill;

/// El ancho del tab dictando (el de un panel, como los vistazos) y el alto
/// de la franja bajo él.
pub(crate) const DICT_W: f32 = crate::PANEL_W;
pub(crate) const DICT_H: f32 = 46.0;
/// Cuánto se queda a la vista el resultado («Pegado…») y un error.
/// El resultado muestra lo pegado: se deja tiempo para leerlo.
const DONE_FOR: Duration = Duration::from_millis(3200);
const ERROR_FOR: Duration = Duration::from_millis(3200);
/// Menos que esto no se manda a transcribir.
const MIN_SECS: f64 = 0.4;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// Abriendo el micrófono.
    Starting,
    Listening,
    Transcribing,
    /// Lo que se pegó.
    Pasted(String),
    Error(String),
}

struct Active {
    handle: atic_audio::CaptureHandle,
    wav: PathBuf,
    dir: PathBuf,
    /// El dictado en vivo con la voz de macOS, si va con ese motor.
    live: Option<LiveFeed>,
}

/// Lo reconocido en vivo mientras se dicta (solo con la voz de macOS).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveText {
    /// Lo que el reconocimiento ya no va a cambiar.
    pub confirmed: String,
    /// Lo provisorio: puede cambiar con lo que sigue.
    pub volatile: String,
    /// Sube con cada resultado, para redibujar solo cuando hay texto nuevo.
    rev: u64,
}

struct Inner {
    phase: Phase,
    since: Instant,
    active: Option<Active>,
    /// La app donde estabas al empezar: ahí se pega si al final el foco
    /// quedó en la pill.
    target: Option<Target>,
    /// El texto listo para pegar (lo toma la pill, que es quien pega).
    ready: Option<(String, Option<Target>)>,
    /// Se soltó el atajo antes de que el micrófono terminara de abrir.
    stop_early: bool,
}

/// El dictado, compartido entre la pill y sus hilos.
#[derive(Clone)]
pub struct Dictation {
    inner: Arc<Mutex<Inner>>,
    /// Nivel del micrófono (0..1, bits de `f32`), para la onda.
    level: Arc<AtomicU32>,
    /// Lo que se va reconociendo en vivo.
    live: Arc<Mutex<LiveText>>,
    /// La última versión de `live` que vio la pill.
    live_seen: Arc<AtomicU64>,
}

impl Dictation {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                phase: Phase::Idle,
                since: Instant::now(),
                active: None,
                target: None,
                ready: None,
                stop_early: false,
            })),
            level: Default::default(),
            live: Default::default(),
            live_seen: Default::default(),
        }
    }

    /// Lo reconocido en vivo hasta ahora (vacío si no hay o no aplica).
    pub fn live_text(&self) -> LiveText {
        self.live.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// ¿Llegó texto en vivo desde la última vez que se preguntó?
    fn live_changed(&self) -> bool {
        let rev = self.live.lock().map(|l| l.rev).unwrap_or(0);
        self.live_seen.swap(rev, Ordering::Relaxed) != rev
    }

    fn clear_live(&self) {
        if let Ok(mut live) = self.live.lock() {
            let rev = live.rev;
            *live = LiveText { rev: rev + 1, ..Default::default() };
        }
    }

    /// La fase y desde cuándo. Pasado el tiempo, el resultado o el error
    /// vuelven solos a reposo.
    pub fn phase(&self) -> (Phase, Instant) {
        let Ok(mut inner) = self.inner.lock() else {
            return (Phase::Idle, Instant::now());
        };
        let expired = match inner.phase {
            Phase::Pasted(_) => inner.since.elapsed() > DONE_FOR,
            Phase::Error(_) => inner.since.elapsed() > ERROR_FOR,
            _ => false,
        };
        if expired {
            inner.phase = Phase::Idle;
            inner.since = Instant::now();
        }
        (inner.phase.clone(), inner.since)
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Escuchando (o abriendo el micrófono para escuchar).
    pub fn recording(&self) -> bool {
        matches!(self.phase().0, Phase::Starting | Phase::Listening)
    }

    fn set(&self, phase: Phase) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.phase = phase;
            inner.since = Instant::now();
        }
    }

    /// El texto transcrito, para pegarlo. Una sola vez.
    pub fn take_ready(&self) -> Option<(String, Option<Target>)> {
        self.inner.lock().ok()?.ready.take()
    }

    /// Empezar a escuchar. El micrófono se abre en otro hilo: la pill no
    /// espera al dispositivo.
    pub fn start(&self, target: Option<Target>) {
        {
            let Ok(mut inner) = self.inner.lock() else { return };
            if matches!(inner.phase, Phase::Starting | Phase::Listening | Phase::Transcribing) {
                return;
            }
            inner.phase = Phase::Starting;
            inner.since = Instant::now();
            inner.target = target;
            inner.stop_early = false;
        }
        self.clear_live();
        let this = self.clone();
        std::thread::Builder::new()
            .name("dictado".into())
            .spawn(move || match this.open() {
                Ok(active) => {
                    let early = {
                        let Ok(mut inner) = this.inner.lock() else { return };
                        inner.active = Some(active);
                        inner.phase = Phase::Listening;
                        inner.since = Instant::now();
                        inner.stop_early
                    };
                    if early {
                        this.stop();
                    }
                }
                Err(message) => this.set(Phase::Error(message)),
            })
            .ok();
    }

    /// Valida la configuración y abre el micrófono.
    fn open(&self) -> Result<Active, String> {
        let (cfg, dirs) = config()?;
        let engine = pick_engine(&cfg.dictation_backend, apple_ready())?;
        if engine == Engine::Groq {
            groq_key()?;
        }
        if engine == Engine::Parakeet {
            if !crate::local_stt::ready() {
                return Err(crate::i18n::t("pill.localStt.missing").into());
            }
            crate::local_stt::preload();
        }
        let dir = dirs
            .data_dir()
            .join("dictation")
            .join(format!("gpui-{}", chrono::Utc::now().timestamp_millis()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let wav = dir.join("mic.wav");
        let mic = if cfg.dictation_mic_device_id.trim().is_empty() {
            cfg.mic_device_id.clone()
        } else {
            cfg.dictation_mic_device_id.clone()
        };
        // Atic fuerza RNNoise «medium» al dictar; acá se respeta lo elegido en
        // Ajustes. Whisper tolera bien el ruido y los artefactos del filtro le
        // bajan la precisión (`PILL_DICTATION_NOISE` lo fuerza para comparar).
        let noise = std::env::var("PILL_DICTATION_NOISE").unwrap_or_else(|_| cfg.noise_suppression.clone());
        // La voz de macOS se abre antes que el micrófono: carga el modelo
        // mientras el dispositivo arranca. Si no abre, se dicta sin vivo.
        let (stt_tap, live) = match (engine == Engine::Apple).then(|| LiveFeed::open(&self.live, cfg.whisper_language().as_deref())) {
            Some(Ok((tap, feed))) => (Some(tap), Some(feed)),
            Some(Err(error)) => {
                tracing::warn!(%error, "dictado en vivo: no abrió la voz de macOS; queda transcribir al final");
                (None, None)
            }
            None => (None, None),
        };
        let (tx, rx) = mpsc::channel::<atic_audio::CaptureEvent>();
        let handle = atic_audio::CaptureSession::start(
            atic_audio::CaptureConfig {
                mic_wav: wav.clone(),
                system_wav: dir.join("system.wav"),
                capture_mic: true,
                capture_system: false,
                noise_suppression: noise,
                mic_device_id: mic,
                output_device_id: cfg.output_device_id.clone(),
                stt_tap,
                english: false,
            },
            tx,
        )
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&dir);
            e.to_ui(false)
        })?;
        let level = self.level.clone();
        std::thread::Builder::new()
            .name("dictado-nivel".into())
            .spawn(move || {
                for event in rx {
                    match event {
                        atic_audio::CaptureEvent::Levels { mic, .. } => {
                            level.store(mic.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed)
                        }
                        atic_audio::CaptureEvent::Error(message) => eprintln!("[dictado] aviso: {message}"),
                    }
                }
                level.store(0f32.to_bits(), Ordering::Relaxed);
            })
            .ok();
        Ok(Active { handle, wav, dir, live })
    }

    /// Terminar: cerrar el micrófono, transcribir y dejar el texto listo.
    pub fn stop(&self) {
        let (active, target) = {
            let Ok(mut inner) = self.inner.lock() else { return };
            if inner.phase == Phase::Starting {
                inner.stop_early = true;
                return;
            }
            let Some(active) = inner.active.take() else { return };
            inner.phase = Phase::Transcribing;
            inner.since = Instant::now();
            (active, inner.target)
        };
        let this = self.clone();
        std::thread::Builder::new()
            .name("dictado-texto".into())
            .spawn(move || {
                let stopping = Instant::now();
                let summary = active.handle.stop();
                let stop_ms = stopping.elapsed().as_millis();
                // El micrófono ya cerró: lo que queda es que la voz de macOS
                // termine lo último. Se cierra siempre, aunque no se use.
                let live = active.live.map(LiveFeed::finish);
                let result = (|| {
                    if !summary.mic_written || !active.wav.exists() {
                        return Err(crate::i18n::t("pill.dictation.errNoAudio").to_string());
                    }
                    if summary.duration_secs < MIN_SECS {
                        return Err(crate::i18n::t("pill.dictation.errTooShort").to_string());
                    }
                    let (cfg, _) = config()?;
                    let language = cfg.whisper_language();
                    let started = Instant::now();
                    let engine = pick_engine(&cfg.dictation_backend, apple_ready())?;
                    let model = std::env::var("PILL_DICTATION_MODEL").unwrap_or(cfg.dictation_groq_model.clone());
                    let with_groq = || groq(&groq_key()?, &active.wav, language.as_deref(), &model);
                    let (text, used) = match engine {
                        Engine::Groq => (with_groq()?, "groq"),
                        Engine::Parakeet => {
                            let samples = atic_transcribe::decode::load_wav_mono_16k(&active.wav).map_err(|e| e.to_string())?;
                            (crate::local_stt::transcribe(&samples)?, "parakeet")
                        }
                        // Si la voz de macOS falla y hay llave de Groq, se dicta igual.
                        // Lo que ya reconoció en vivo; si no hay, el WAV entero.
                        Engine::Apple => match live {
                            Some(Ok(text)) if !text.trim().is_empty() => (text, "apple-vivo"),
                            other => {
                                match other {
                                    Some(Err(error)) => tracing::warn!(%error, "dictado en vivo falló; transcribo el WAV"),
                                    Some(Ok(_)) => tracing::info!("dictado en vivo vino vacío; transcribo el WAV"),
                                    None => {}
                                }
                                match apple_transcribe(&active.wav, language.as_deref()) {
                                    Ok(text) => (text, "apple-wav"),
                                    Err(error) => {
                                        tracing::warn!(%error, "la voz de macOS no transcribió; pruebo con Groq");
                                        (with_groq().map_err(|_| error)?, "groq")
                                    }
                                }
                            }
                        },
                    };
                    let transcribe_ms = started.elapsed().as_millis() as u64;
                    let text = text.trim().to_string();
                    // Pulir solo si quedó texto; si Apple Intelligence falla, va el de antes.
                    let polishing = Instant::now();
                    let text = if cfg.dictation_polish && !text.is_empty() {
                        apple_polish(&text).unwrap_or_else(|error| {
                            tracing::warn!(%error, "Apple Intelligence no pulió el dictado");
                            text
                        })
                    } else {
                        text
                    };
                    // Al registro (la pill de dev descarta stderr): cerrar el micrófono,
                    // transcribir y pulir son las esperas antes de pegar.
                    tracing::info!(
                        audio_s = format!("{:.1}", summary.duration_secs),
                        cerrar_mic_ms = stop_ms as u64,
                        transcribir_ms = transcribe_ms,
                        pulir_ms = cfg.dictation_polish.then(|| polishing.elapsed().as_millis() as u64),
                        motor = used,
                        "dictado transcrito"
                    );
                    if text.is_empty() {
                        return Err(crate::i18n::t("pill.dictation.errNoSpeech").to_string());
                    }
                    Ok(text)
                })();
                let _ = std::fs::remove_dir_all(&active.dir);
                match result {
                    Ok(text) => {
                        if let Ok(mut inner) = this.inner.lock() {
                            inner.ready = Some((text.clone(), target));
                        }
                        this.set(Phase::Pasted(text));
                    }
                    Err(message) => this.set(Phase::Error(message)),
                }
            })
            .ok();
    }
}

/// El contexto que recibe Whisper: las palabras que no conoce (sin esto
/// «Atic» sale «Ati») y una frase con buena puntuación, que imita. Whisper
/// lee solo los últimos ~224 tokens.
const PROMPT: &str = "Dictado en español de Chile, con tildes y puntuación.     Vocabulario: Atic, GPUI, notch, pill, Claude, Claude Code, Codex, Groq, Tauri,     Svelte, Rust, commit, push, deploy, prompt.";

/// Groq Whisper como `atic_transcribe::transcribe_groq`, más el `prompt`.
/// Va acá para no tocar el crate de Atic mientras su `tauri dev` corre.
fn groq(key: &str, wav: &std::path::Path, language: Option<&str>, model: &str) -> Result<String, String> {
    let bytes = std::fs::read(wav).map_err(|e| e.to_string())?;
    let part = reqwest::blocking::multipart::Part::bytes(bytes)
        .file_name("mic.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "json")
        .text("temperature", "0")
        .text("prompt", PROMPT)
        .part("file", part);
    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .bearer_auth(key)
        .multipart(form)
        .send()
        .map_err(|_| crate::i18n::t("pill.dictation.errGroqUnreachable").to_string())?;
    let status = response.status();
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if !status.is_success() {
        let detail = body["error"]["message"].as_str().unwrap_or(crate::i18n::t("pill.dictation.errUnknown"));
        return Err(crate::i18n::tf("pill.dictation.errGroqStatus", &[("status", &status.as_u16()), ("detail", &detail)]));
    }
    Ok(body["text"].as_str().unwrap_or_default().to_string())
}

/// Con qué se transcribe el dictado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Engine {
    Groq,
    /// La voz de macOS, en el equipo.
    Apple,
    /// Parakeet en la CPU, sin internet (`local_stt.rs`).
    Parakeet,
}

/// El motor según la configuración. En Mac, «local» (lo que dejaba la Atic de
/// Tauri, sin Whisper en la pill) también va con la voz de macOS.
fn pick_engine(backend: &str, apple: bool) -> Result<Engine, String> {
    match backend {
        "groq" => Ok(Engine::Groq),
        "parakeet" => Ok(Engine::Parakeet),
        "apple" | "local" if apple => Ok(Engine::Apple),
        "apple" if cfg!(target_os = "macos") => Err(crate::i18n::t("pill.dictation.errNoHelper").into()),
        _ => Err(crate::i18n::t("pill.dictation.errPickEngine").into()),
    }
}

#[cfg(target_os = "macos")]
fn apple_ready() -> bool {
    crate::platform::apple::installed()
}

#[cfg(not(target_os = "macos"))]
fn apple_ready() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn apple_transcribe(wav: &std::path::Path, language: Option<&str>) -> Result<String, String> {
    crate::platform::apple::transcribe(wav, language, false).map(|t| t.text)
}

#[cfg(not(target_os = "macos"))]
fn apple_transcribe(_: &std::path::Path, _: Option<&str>) -> Result<String, String> {
    Err(crate::i18n::t("pill.dictation.errMacOnly").into())
}

#[cfg(target_os = "macos")]
fn apple_polish(text: &str) -> Result<String, String> {
    crate::platform::apple::polish(text)
}

#[cfg(not(target_os = "macos"))]
fn apple_polish(text: &str) -> Result<String, String> {
    Ok(text.to_string())
}

/// Cuánto se espera, ya cerrado el micrófono, a que la voz de macOS termine.
/// Normalmente son decenas de ms; si se pasa, se transcribe el WAV.
#[cfg(target_os = "macos")]
const LIVE_FINISH_WAIT: Duration = Duration::from_secs(4);

/// El puente entre la grabación y la voz de macOS en vivo: un hilo recibe los
/// bloques del micrófono (el `stt_tap` de `atic-audio`, que nunca frena la
/// captura) y se los pasa al ayudante en mono.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct LiveFeed {
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    thread: std::thread::JoinHandle<crate::platform::apple::LiveStream>,
}

impl LiveFeed {
    #[cfg(target_os = "macos")]
    fn open(
        live: &Arc<Mutex<LiveText>>,
        language: Option<&str>,
    ) -> Result<(mpsc::SyncSender<atic_audio::AudioTapChunk>, Self), String> {
        let opened = Instant::now();
        let first = AtomicBool::new(false);
        let shared = live.clone();
        let mut stream = crate::platform::apple::LiveStream::start(language, move |confirmed, volatile| {
            if !first.swap(true, Ordering::Relaxed) {
                tracing::info!(ms = opened.elapsed().as_millis() as u64, "dictado en vivo: primer parcial");
            }
            if let Ok(mut text) = shared.lock() {
                text.confirmed = confirmed.to_string();
                text.volatile = volatile.to_string();
                text.rev += 1;
            }
        })?;
        // ~10 ms por bloque: unos 5 s de holgura antes de perder audio.
        let (tx, rx) = mpsc::sync_channel::<atic_audio::AudioTapChunk>(512);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("dictado-vivo".into())
            .spawn(move || {
                let mut push = |chunk: atic_audio::AudioTapChunk| {
                    if chunk.track == atic_audio::CaptureTrack::Mic {
                        stream.push(chunk.sample_rate, &crate::platform::apple::downmix(&chunk.samples, chunk.channels));
                    }
                };
                loop {
                    // Al cerrar el micrófono ya no llega nada nuevo: se vacía la cola y listo.
                    if stopping.load(Ordering::Relaxed) {
                        while let Ok(chunk) = rx.try_recv() {
                            push(chunk);
                        }
                        break;
                    }
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(chunk) => push(chunk),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                stream
            })
            .map_err(|e| e.to_string())?;
        Ok((tx, Self { stop, thread }))
    }

    #[cfg(not(target_os = "macos"))]
    fn open(_: &Arc<Mutex<LiveText>>, _: Option<&str>) -> Result<(mpsc::SyncSender<atic_audio::AudioTapChunk>, Self), String> {
        Err(crate::i18n::t("pill.dictation.errMacOnly").into())
    }

    /// Ya cerrado el micrófono: manda lo último y espera el texto final.
    #[cfg(target_os = "macos")]
    fn finish(self) -> Result<String, String> {
        let waiting = Instant::now();
        self.stop.store(true, Ordering::Relaxed);
        let stream = self.thread.join().map_err(|_| crate::i18n::t("pill.dictation.errLiveThread").to_string())?;
        if !stream.healthy() {
            return Err(crate::i18n::t("pill.dictation.errNotAllAudio").into());
        }
        let text = stream.finish(LIVE_FINISH_WAIT);
        tracing::info!(ms = waiting.elapsed().as_millis() as u64, ok = text.is_ok(), "dictado en vivo: cerrado");
        text
    }

    #[cfg(not(target_os = "macos"))]
    fn finish(self) -> Result<String, String> {
        self.stop.store(true, Ordering::Relaxed);
        Err(crate::i18n::t("pill.dictation.errMacOnly").into())
    }
}

/// Lo último del texto en vivo que cabe en una línea de `chars` caracteres,
/// cortado en una palabra y con «…» delante si se cortó. Devuelve la parte
/// confirmada y la provisoria por separado, para pintarlas distinto.
fn live_tail(confirmed: &str, volatile: &str, chars: usize) -> (String, String) {
    let confirmed = confirmed.trim_start();
    let volatile = if confirmed.is_empty() { volatile.trim_start() } else { volatile };
    let total = confirmed.chars().count() + volatile.chars().count();
    if total <= chars {
        return (confirmed.to_string(), volatile.to_string());
    }
    // Se botan caracteres del comienzo hasta que quepa «…» más el resto.
    let mut skip = total - chars.saturating_sub(1);
    let joined: String = confirmed.chars().chain(volatile.chars()).collect();
    // Hasta el próximo espacio, para no partir una palabra (si hay uno cerca).
    if let Some(space) = joined.chars().skip(skip).take(16).position(char::is_whitespace) {
        skip += space + 1;
    }
    let confirmed_len = confirmed.chars().count();
    let head: String = confirmed.chars().skip(skip).collect();
    let tail: String = volatile.chars().skip(skip.saturating_sub(confirmed_len)).collect();
    if head.is_empty() {
        // Todo lo que quedó es provisorio: el «…» va con eso.
        (String::new(), format!("…{}", tail.trim_start()))
    } else {
        (format!("…{head}"), tail)
    }
}

fn config() -> Result<(atic_core::Config, atic_core::AppDirs), String> {
    let dirs = atic_core::AppDirs::new().map_err(|e| e.to_string())?;
    Ok((atic_core::Config::load(&dirs.config_path()), dirs))
}

fn groq_key() -> Result<String, String> {
    match atic_core::secrets::get_secret(atic_core::SecretKind::GroqApiKey) {
        Ok(Some(key)) if !key.trim().is_empty() => Ok(key.trim().to_string()),
        _ => Err(crate::i18n::t("pill.dictation.errNoGroqKey").into()),
    }
}

/// Lo más que se escucha en un dictado.
const MAX_LISTENING: Duration = Duration::from_secs(5 * 60);

/// Botón lateral «atrás» del mouse (`MouseX1`).
pub(crate) const VK_XBUTTON1: i32 = 0x05;
/// Botón lateral «adelante» del mouse (`MouseX2`).
pub(crate) const VK_XBUTTON2: i32 = 0x06;

/// El atajo de Atic (`Alt+X`, `CmdOrCtrl+Shift+D`…) como teclas virtuales.
/// Se sondea, como el de la rueda: la ventana de la pill nunca tiene el foco.
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    /// Teclas virtuales; las de Alt, Ctrl, Shift y Win, con sus códigos genéricos.
    pub(crate) keys: Vec<i32>,
    pub push_to_talk: bool,
}

impl Shortcut {
    fn dictation(cfg: &atic_core::Config) -> Option<Self> {
        let mut shortcut = Self::parse(&cfg.dictation_shortcut)?;
        shortcut.push_to_talk = cfg.dictation_mode != "toggle";
        Some(shortcut)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut keys = Vec::new();
        for part in text.split('+').map(|p| p.trim().to_ascii_lowercase()) {
            let vk = match part.as_str() {
                "alt" | "option" => 0x12,
                // En macOS «CmdOrCtrl» es Comando, la tecla Win de Windows.
                "cmdorctrl" | "commandorcontrol" if cfg!(target_os = "macos") => 0x5B,
                "ctrl" | "control" | "cmdorctrl" | "commandorcontrol" => 0x11,
                "shift" => 0x10,
                "super" | "win" | "meta" | "cmd" | "command" => 0x5B,
                "space" => 0x20,
                // Los botones laterales del mouse, como los escribe Atic.
                "mousex1" => VK_XBUTTON1,
                "mousex2" => VK_XBUTTON2,
                p if p.len() == 1 && p.as_bytes()[0].is_ascii_alphanumeric() => p.to_ascii_uppercase().as_bytes()[0] as i32,
                p if p.starts_with('f') && p[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
                    0x6F + p[1..].parse::<i32>().ok()?
                }
                _ => return None,
            };
            keys.push(vk);
        }
        (!keys.is_empty()).then_some(Self { keys, push_to_talk: true })
    }

    /// Recién apretado: sus teclas abajo y ningún otro modificador. AltGr es
    /// Ctrl+Alt para Windows: sin esto, AltGr+Q (la `@` en los teclados
    /// latinos) abría también un atajo Alt+Q.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn down(&self) -> bool {
        exact(&self.keys, async_down)
    }

    /// Sigue apretado: sus teclas abajo, aunque se sume otro modificador.
    /// Para no cortar el dictado mantenido por rozar Shift.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn held(&self) -> bool {
        self.keys.iter().all(|&vk| async_down(vk))
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn down(&self) -> bool {
        false
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn held(&self) -> bool {
        false
    }
}

#[cfg(windows)]
fn async_down(vk: i32) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { GetAsyncKeyState(vk) < 0 }
}

#[cfg(target_os = "macos")]
fn async_down(vk: i32) -> bool {
    crate::platform::keys::vk_down(vk)
}

/// Las teclas de `keys` abajo y los modificadores que no son del atajo,
/// arriba. Win cuenta con las dos teclas (izquierda y derecha).
pub(crate) fn exact(keys: &[i32], down: impl Fn(i32) -> bool) -> bool {
    const MODIFIERS: [(i32, &[i32]); 4] = [(0x10, &[0x10]), (0x11, &[0x11]), (0x12, &[0x12]), (0x5B, &[0x5B, 0x5C])];
    keys.iter().all(|&vk| down(vk))
        && MODIFIERS
            .iter()
            .filter(|(modifier, _)| !keys.contains(modifier))
            .all(|(_, vks)| vks.iter().all(|&vk| !down(vk)))
}

/// Los atajos vigentes (dictado y rueda) y si los tiene la app de Atic,
/// releídos cada 5 s en su propio hilo (leer la config y listar procesos no va
/// en un cuadro). Atic los tiene si está abierta y la pill no es la nativa
/// (`native_pill`): con la nativa, Atic ya no los registra.
#[derive(Clone, Default)]
pub struct Watch {
    shortcut: Arc<Mutex<Option<Shortcut>>>,
    wheel: Arc<Mutex<Option<Shortcut>>>,
    /// Alt+Z no es de ningún atajo de Atic: la rueda puede usarlo mientras
    /// Atic tiene los suyos.
    wheel_fallback_free: Arc<std::sync::atomic::AtomicBool>,
    atic: Arc<std::sync::atomic::AtomicBool>,
    /// Ya se miró una vez: antes no se sabe de quién son los atajos.
    checked: Arc<std::sync::atomic::AtomicBool>,
}

impl Watch {
    pub fn spawn() -> Self {
        let watch = Self::default();
        let shared = watch.clone();
        std::thread::Builder::new()
            .name("dictado-atajo".into())
            .spawn(move || {
                let mut was_atic = false;
                loop {
                    let cfg = config().ok().map(|(cfg, _)| cfg);
                    let shortcut = cfg.as_ref().and_then(Shortcut::dictation);
                    let wheel = cfg.as_ref().and_then(|cfg| Shortcut::parse(&cfg.pill_radial_shortcut));
                    let native = cfg.as_ref().is_some_and(|cfg| cfg.native_pill);
                    let atic = !native && atic_running();
                    if atic && !was_atic {
                        eprintln!("[atajos] la app de Atic está abierta: los atajos quedan para ella");
                    }
                    was_atic = atic;
                    if let Ok(mut slot) = shared.shortcut.lock() {
                        *slot = shortcut;
                    }
                    if let Ok(mut slot) = shared.wheel.lock() {
                        *slot = wheel;
                    }
                    let fallback_free = cfg.as_ref().is_some_and(|cfg| !uses_wheel_fallback(cfg));
                    shared.wheel_fallback_free.store(fallback_free, Ordering::Relaxed);
                    shared.atic.store(atic, Ordering::Relaxed);
                    shared.checked.store(true, Ordering::Release);
                    std::thread::sleep(Duration::from_secs(5));
                }
            })
            .ok();
        watch
    }

    /// El atajo, si la app de Atic no lo tiene. `PILL_DICTATION_KEY` (por
    /// ejemplo `Ctrl+Alt+D`) da uno propio, que vale aunque Atic esté abierta.
    fn shortcut(&self) -> Option<Shortcut> {
        if let Some(own) = std::env::var("PILL_DICTATION_KEY").ok().and_then(|k| Shortcut::parse(&k)) {
            let toggle = self.shortcut.lock().ok()?.as_ref().is_some_and(|s| !s.push_to_talk);
            return Some(Shortcut { push_to_talk: !toggle, ..own });
        }
        if self.atic_owns_shortcuts() {
            return None;
        }
        self.shortcut.lock().ok()?.clone()
    }

    /// Si ya se sabe de quién son los atajos (la primera revisión terminó).
    pub fn checked(&self) -> bool {
        self.checked.load(Ordering::Acquire)
    }

    /// Atic tiene los atajos: está abierta y la pill no es la nativa.
    pub fn atic_owns_shortcuts(&self) -> bool {
        self.atic.load(Ordering::Relaxed)
    }

    /// La rueda: el atajo de `config.json` si los atajos son de la pill; si
    /// no, Alt+Z, salvo que Atic lo use para otra cosa (sería doble). Sin
    /// atajo (el de omisión) no se abre con el teclado: se mantiene la pill.
    pub fn wheel_down(&self) -> bool {
        if crate::hotkeys::paused() {
            return false;
        }
        let Some(wheel) = self.wheel.lock().ok().and_then(|slot| slot.clone()) else {
            return false;
        };
        if self.atic_owns_shortcuts() {
            return self.wheel_fallback_free.load(Ordering::Relaxed) && crate::win::wheel_shortcut_down();
        }
        wheel.down()
    }
}

/// Algún atajo de Atic es Alt+Z, el que usa la rueda mientras Atic tiene los
/// suyos.
fn uses_wheel_fallback(cfg: &atic_core::Config) -> bool {
    const ALT_Z: [i32; 2] = [0x12, 'Z' as i32];
    [
        &cfg.global_shortcut,
        &cfg.dictation_shortcut,
        &cfg.summon_pill_shortcut,
        &cfg.pill_radial_shortcut,
        &cfg.clipboard_shortcut,
        &cfg.snippets_shortcut,
        &cfg.agents_shortcut,
        &cfg.screenshot_shortcut,
        &cfg.board_shortcut,
        &cfg.color_shortcut,
        &cfg.launcher_shortcut,
        &cfg.window_flip_shortcut,
    ]
    .into_iter()
    .filter_map(|text| Shortcut::parse(text))
    .any(|shortcut| {
        let mut keys = shortcut.keys;
        keys.sort_unstable();
        keys == ALT_Z
    })
}

/// La app de Atic (Tauri) corriendo: tiene el mismo atajo y dictaría dos
/// veces. Mientras esté abierta, el atajo es de ella.
#[cfg(target_os = "macos")]
fn atic_running() -> bool {
    // La app Tauri de Mac (`Atic.app`) o su `tauri dev` (`atic-desktop`).
    let found = |args: &[&str]| std::process::Command::new("pgrep").args(args).output().is_ok_and(|o| o.status.success());
    found(&["-f", "Atic.app/Contents/MacOS/"]) || found(&["-x", "atic-desktop"])
}

#[cfg(not(target_os = "macos"))]
fn atic_running() -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags_no_window()
        .output();
    out.is_ok_and(|o| {
        let list = String::from_utf8_lossy(&o.stdout).to_ascii_lowercase();
        list.contains("\"atic.exe\"") || list.contains("\"atic-desktop.exe\"")
    })
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x0800_0000);
        }
        self
    }
}

impl Pill {
    /// Lee el atajo y empieza o termina el dictado; pega lo transcrito.
    pub(crate) fn update_dictation(&mut self, now: Instant, cx: &mut Context<Self>) {
        let shortcut = self.dict_watch.shortcut();
        // Grabando un atajo en Ajustes: las teclas son para el editor.
        let down = !crate::hotkeys::paused()
            && shortcut.as_ref().is_some_and(|s| if self.dict_key_was_down { s.held() } else { s.down() });
        let push_to_talk = shortcut.as_ref().is_none_or(|s| s.push_to_talk);
        // Red de seguridad: un dictado que nadie cortó (se perdió el soltar
        // la tecla) no deja el micrófono abierto para siempre.
        let (phase, since) = self.dictation.phase();
        if phase == Phase::Listening && since.elapsed() >= MAX_LISTENING {
            tracing::warn!("dictado: pasó {} s escuchando, se corta solo", MAX_LISTENING.as_secs());
            self.dictation.stop();
        }
        if down && !self.dict_key_was_down {
            if self.dictation.recording() {
                self.dictation.stop();
            } else {
                self.dictation.start(crate::paste::dictation_target());
            }
        } else if !down && self.dict_key_was_down && push_to_talk && self.dictation.recording() {
            self.dictation.stop();
        }
        self.dict_key_was_down = down;

        // Se pega donde está el foco ahora si es otra app o una ventana de la pill
        // (Atic Code); si quedó en el notch, en la app donde se empezó a dictar. Sin ninguna, queda en
        // «pegar después».
        if let Some((text, target)) = self.dictation.take_ready() {
            match crate::paste::dictation_target().or(target) {
                Some(target) => {
                    let item = gpui::ClipboardItem::new_string(text);
                    cx.spawn(async move |_, cx| crate::paste_into(Some(target), item, cx).await)
                        .detach();
                }
                None => crate::paste_queue::enqueue(&text),
            }
        }

        let on = self.docked_still() && self.dictation.phase().0 != Phase::Idle;
        self.dict.set(if on { 1.0 } else { 0.0 }, now);
        // Texto nuevo del dictado en vivo: se ve al tiro, sin esperar el cuadro de la onda.
        if self.dictation.live_changed() {
            cx.notify();
        }
    }

    /// Dictando (en cualquier fase visible): la letra de la música se recoge.
    pub(crate) fn dictation_shown(&self) -> bool {
        self.dict.target() == 1.0
    }

    /// Cuánto se ve la franja del dictado (0 a 1): bajo el tab arriba, al
    /// lado en un costado.
    pub(crate) fn dict_amount(&self, now: Instant) -> f32 {
        self.dict.value(now).clamp(0.0, 1.2)
    }

    pub(crate) fn dict_length(length: f32, amount: f32) -> f32 {
        length + (DICT_W.max(length) - length) * amount
    }

    /// El cursor sobre la franja del dictado: no abre la tira.
    pub(crate) fn over_dict(&self, p: (f32, f32), now: Instant) -> bool {
        if self.dict.value(now) <= 0.05 {
            return false;
        }
        self.drawer_rect(crate::usage::Drawer::Dictation, now).is_some_and(|r| r.contains(p, 0.0))
    }

    pub(crate) fn dict_element(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        let amount = self.dict.value(now);
        if amount <= 0.05 {
            return None;
        }
        let area = self.drawer_rect(crate::usage::Drawer::Dictation, now)?;
        let palette = crate::Palette::dark();
        let (phase, since) = self.dictation.phase();
        let secs = since.elapsed().as_secs();
        let red = gpui::rgb(0xff5a52);
        let green = gpui::rgb(0x5ad08a);
        let (dot, label, muted): (Option<gpui::Hsla>, String, bool) = match &phase {
            Phase::Idle => (None, String::new(), true),
            Phase::Starting => (Some(gpui::Hsla::from(red).opacity(0.5)), crate::i18n::t("pill.dictation.phaseStarting").into(), true),
            Phase::Listening => (Some(red.into()), crate::i18n::tf("pill.dictation.phaseListening", &[("m", &(secs / 60)), ("s", &format!("{:02}", secs % 60))]), false),
            Phase::Transcribing => (None, crate::i18n::t("pill.dictation.phaseTranscribing").into(), false),
            // Lo que se pegó, para ver al tiro si entendió bien.
            Phase::Pasted(text) => (Some(green.into()), format!("«{text}»"), false),
            Phase::Error(message) => (None, message.clone(), true),
        };
        // El punto rojo late; transcribiendo, tres puntos que van y vienen.
        let t = now.elapsed_since_epoch();
        let pulse = 0.55 + 0.45 * (t * 4.0).sin().abs();
        let leading = match &phase {
            Phase::Transcribing => div()
                .flex()
                .gap(px(3.))
                .children((0..3).map(|i| {
                    let a = 0.3 + 0.7 * ((t * 5.0 - i as f32 * 0.6).sin() * 0.5 + 0.5);
                    div().size(px(5.)).rounded(px(3.)).bg(palette.text.opacity(a))
                }))
                .into_any_element(),
            _ => match dot {
                Some(color) => div()
                    .size(px(9.))
                    .rounded(px(5.))
                    .bg(if phase == Phase::Listening { color.opacity(pulse) } else { color })
                    .into_any_element(),
                None => div().into_any_element(),
            },
        };
        // Con la voz de macOS en vivo: lo que va entendiendo reemplaza al
        // «Escuchando» y a la onda. Una línea que muestra lo último; lo
        // provisorio, más tenue.
        let live = self.dictation.live_text();
        let live_on = matches!(phase, Phase::Listening | Phase::Transcribing)
            && !(live.confirmed.trim().is_empty() && live.volatile.trim().is_empty());
        let live_line = live_on.then(|| {
            // Lo que cabe: el ancho sin el margen ni el punto, a ~0,52 em por letra.
            let room = (area.w - 18.0 * 2.0 - 18.0 - 10.0).max(40.0);
            let chars = (room / (12.5 * 0.52)) as usize;
            let (confirmed, volatile) = live_tail(&live.confirmed, &live.volatile, chars);
            let split = confirmed.len();
            let text = SharedString::from(format!("{confirmed}{volatile}"));
            let len = text.len();
            let styled = gpui::StyledText::new(text).with_highlights((split < len).then(|| {
                (split..len, gpui::HighlightStyle { color: Some(palette.text.opacity(0.5)), ..Default::default() })
            }));
            // Pegada a la derecha: si la estimación se queda corta, se corta el comienzo.
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .justify_end()
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_size(px(12.5))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text)
                        .child(styled),
                )
        });
        // La onda del micrófono: barras que siguen el nivel, más altas al centro.
        let level = (self.dictation.level() * 3.0).min(1.0);
        let wave = (phase == Phase::Listening && !live_on).then(|| {
            div().flex().items_center().gap(px(3.)).h(px(22.)).children((0..9).map(|i| {
                let center = 1.0 - ((i as f32 - 4.0).abs() / 5.0);
                let wobble = 0.6 + 0.4 * ((t * 9.0 + i as f32 * 1.3).sin() * 0.5 + 0.5);
                let h = 3.0 + 17.0 * level * center * wobble;
                div().w(px(3.)).h(px(h)).rounded(px(2.)).bg(palette.text.opacity(0.85))
            }))
        });
        // Sin fondo propio: el tab entero se vuelve casi opaco (`CONTENT_TINT`).
        Some(
            div()
                .id("dictation-face")
                .absolute()
                .left(px(area.x))
                .top(px(area.y))
                .w(px(area.w))
                .h(px(area.h))
                .overflow_hidden()
                .opacity(segment(amount, 0.5, 0.5))
                .px(px(18.))
                .flex()
                .items_center()
                .gap(px(10.))
                .font_family(crate::fonts::UI)
                .cursor_pointer()
                // Un clic termina el dictado (escuchando) o descarta el aviso.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                        if pill.dictation.recording() {
                            pill.dictation.stop();
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(div().w(px(18.)).flex().justify_center().child(leading))
                .children(live_line)
                .children((!live_on).then(|| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_clamp(2)
                        .text_size(px(12.5))
                        .line_height(px(16.))
                        .font_weight(if muted { FontWeight::NORMAL } else { FontWeight::SEMIBOLD })
                        .text_color(if muted { palette.muted } else { palette.text })
                        .child(SharedString::from(label))
                }))
                .children(wave)
                .into_any_element(),
        )
    }
}

/// Segundos desde un punto fijo, para las animaciones continuas.
trait Elapsed {
    fn elapsed_since_epoch(&self) -> f32;
}

impl Elapsed for Instant {
    fn elapsed_since_epoch(&self) -> f32 {
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        self.duration_since(*EPOCH.get_or_init(Instant::now)).as_secs_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_texto_en_vivo_muestra_lo_ultimo() {
        // Cabe: queda tal cual, sin espacios al comienzo.
        assert_eq!(live_tail("Hola", " mundo", 40), ("Hola".into(), " mundo".into()));
        assert_eq!(live_tail("", "  hola", 40), (String::new(), "hola".into()));
        // No cabe: se corta el comienzo en una palabra, con «…».
        assert_eq!(
            live_tail("uno dos tres cuatro cinco seis siete ocho", " nueve", 20),
            ("…siete ocho".into(), " nueve".into())
        );
        // Solo queda parte de lo provisorio: el «…» va con eso.
        assert_eq!(live_tail("ab", " cdefgh ijklmnop", 10), (String::new(), "…ijklmnop".into()));
        // Con tildes no se parte un carácter.
        let (c, v) = live_tail("canción número único", "", 12);
        assert!(c.starts_with('…') && c.chars().count() <= 12 && v.is_empty());
    }

    #[test]
    fn elige_el_motor_del_dictado() {
        assert_eq!(pick_engine("groq", true), Ok(Engine::Groq));
        assert_eq!(pick_engine("apple", true), Ok(Engine::Apple));
        // «local» de la Atic de Tauri: en Mac, la voz de macOS.
        assert_eq!(pick_engine("local", true), Ok(Engine::Apple));
        // El modelo local va en cualquier sistema, con o sin la voz de macOS.
        assert_eq!(pick_engine("parakeet", true), Ok(Engine::Parakeet));
        assert_eq!(pick_engine("parakeet", false), Ok(Engine::Parakeet));
        // Sin el ayudante (o en Windows) no hay voz de macOS.
        assert!(pick_engine("apple", false).is_err());
        assert!(pick_engine("local", false).is_err());
    }

    #[test]
    fn lee_los_atajos_de_atic() {
        assert_eq!(Shortcut::parse("Alt+X").unwrap().keys, vec![0x12, 'X' as i32]);
        // En macOS «CmdOrCtrl» es Comando (la tecla Win de Windows).
        let cmd_or_ctrl = if cfg!(target_os = "macos") { 0x5B } else { 0x11 };
        assert_eq!(Shortcut::parse("CmdOrCtrl+Shift+D").unwrap().keys, vec![cmd_or_ctrl, 0x10, 'D' as i32]);
        assert_eq!(Shortcut::parse("Ctrl+F9").unwrap().keys, vec![0x11, 0x78]);
        assert!(Shortcut::parse("Alt+Flecha").is_none());
    }

    #[test]
    fn la_rueda_no_toma_alt_z_si_atic_lo_usa() {
        let mut cfg = atic_core::Config::default();
        assert!(!uses_wheel_fallback(&cfg), "por omisión ningún atajo es Alt+Z");
        cfg.pill_radial_shortcut = "Alt+Z".into();
        assert!(uses_wheel_fallback(&cfg));
        cfg.pill_radial_shortcut = "Alt+Q".into();
        cfg.color_shortcut = "Z+Alt".into();
        assert!(uses_wheel_fallback(&cfg));
        cfg.color_shortcut = "Alt+Shift+Z".into();
        assert!(!uses_wheel_fallback(&cfg));
    }

    #[test]
    fn altgr_no_dispara_un_atajo_con_alt() {
        let alt_q = Shortcut::parse("Alt+Q").unwrap().keys;
        let with = |pressed: &'static [i32]| move |vk| pressed.contains(&vk);
        assert!(exact(&alt_q, with(&[0x12, 'Q' as i32])));
        // AltGr+Q, la `@`: Windows la ve como Ctrl+Alt+Q.
        assert!(!exact(&alt_q, with(&[0x11, 0x12, 'Q' as i32])));
        assert!(!exact(&alt_q, with(&[0x10, 0x12, 'Q' as i32])));
        assert!(!exact(&alt_q, with(&[0x5C, 0x12, 'Q' as i32])));
        let ctrl_alt_d = Shortcut::parse("Ctrl+Alt+D").unwrap().keys;
        assert!(exact(&ctrl_alt_d, with(&[0x11, 0x12, 'D' as i32])));
    }
}
