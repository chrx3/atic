//! Subtítulos mientras se graba, como `src-tauri/src/live.rs`: el tap de PCM de
//! la captura va a un motor en vivo. Es una confirmación de que el audio entra,
//! no un documento: nunca se guardan; lo que queda es la transcripción del
//! archivo, hecha después.
//!
//! Dos motores: Groq (`atic_transcribe::LiveEngine`, Whisper en la nube por
//! ventanas de ~6 s) y el modelo local (`local_stt`, Parakeet: una línea por
//! frase, apenas hay una pausa), sin que el audio salga del equipo. Quien eligió
//! el local nunca cae en Groq.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use atic_audio::{AudioTapChunk, CaptureTrack};
use atic_core::{Segment, Speaker};
use atic_transcribe::{LiveEngine, LivePcmChunk, LiveSttBackend, LiveUpdate};

use super::text;
use crate::i18n::t;
use crate::local_stt::LinePace;

/// Las líneas que se guardan en memoria; la tarjeta muestra solo las últimas.
const KEEP: usize = 24;

/// Con qué motor corren los subtítulos.
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Groq { key: String, model: String },
    /// El modelo local (`local_stt`), con el largo de línea que se eligió.
    Local(LinePace),
}

/// Qué hacer con los subtítulos según la configuración.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Off,
    Run(Backend),
    /// Activados pero sin poder correr: el porqué, para una nota discreta.
    Unavailable(&'static str),
}

/// En qué está el modelo local de este equipo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalModel {
    /// Mac Intel: no hay Parakeet.
    Unsupported,
    /// Falta bajarlo (Ajustes → Reuniones).
    Missing,
    Ready,
}

impl LocalModel {
    pub fn current() -> Self {
        match (crate::local_stt::SUPPORTED, crate::local_stt::ready()) {
            (false, _) => Self::Unsupported,
            (true, false) => Self::Missing,
            (true, true) => Self::Ready,
        }
    }
}

/// Si el motor guardado es Groq. Sin Parakeet (Mac Intel), todo lo demás
/// también lo es: lo mismo que muestra Ajustes y que hacen el dictado y las
/// reuniones.
pub fn uses_groq(engine: &str, local: LocalModel) -> bool {
    engine.eq_ignore_ascii_case("groq") || local == LocalModel::Unsupported
}

pub fn plan(enabled: bool, engine: &str, key: Option<String>, model: &str, local: LocalModel, lines: &str) -> Plan {
    if !enabled {
        return Plan::Off;
    }
    if !uses_groq(engine, local) {
        return match local {
            LocalModel::Ready => Plan::Run(Backend::Local(LinePace::parse(lines))),
            _ => Plan::Unavailable(t("pill.meetings.live.noModel")),
        };
    }
    match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(key) => Plan::Run(Backend::Groq {
            key,
            model: atic_transcribe::normalize_groq_whisper_model(model).to_string(),
        }),
        None => Plan::Unavailable(t("pill.meetings.live.noKey")),
    }
}

/// La llave de Groq del llavero (se lee fuera del hilo de UI).
pub fn groq_key() -> Option<String> {
    atic_core::secrets::get_secret(atic_core::SecretKind::GroqApiKey).ok().flatten()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub me: bool,
    pub text: String,
}

/// Lo dicho hasta ahora: las líneas firmes y la parcial (que Whisper todavía
/// puede corregir, por eso se dibuja atenuada).
#[derive(Default, Debug)]
pub struct Captions {
    pub lines: VecDeque<Line>,
    pub partial: Option<Line>,
    pub error: Option<String>,
}

impl Captions {
    fn apply(&mut self, update: LiveUpdate) {
        match update {
            LiveUpdate::Partial(segment) => self.partial = line(&segment),
            LiveUpdate::Final(segment) => {
                self.partial = None;
                if let Some(line) = line(&segment) {
                    if self.lines.len() == KEEP {
                        self.lines.pop_front();
                    }
                    self.lines.push_back(line);
                }
            }
        }
    }
}

fn line(segment: &Segment) -> Option<Line> {
    (!text::is_junk(&segment.text)).then(|| Line {
        me: segment.speaker == Speaker::Me,
        text: segment.text.trim().to_string(),
    })
}

/// El hilo de los subtítulos. Termina solo cuando la captura suelta el tap;
/// `cancel` corta antes la ventana que esté esperando (como en Atic: al
/// detener no se completa la vista previa).
pub struct Worker {
    cancel: Arc<AtomicBool>,
    pub captions: Arc<Mutex<Captions>>,
}

impl Worker {
    pub fn spawn(tap: Receiver<AudioTapChunk>, language: Option<String>, backend: Backend) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let captions = Arc::new(Mutex::new(Captions::default()));
        let (cancel_bg, captions_bg) = (cancel.clone(), captions.clone());
        std::thread::Builder::new().name("meetings-live".into()).spawn(move || {
            // Qué hace cada bloque del tap, y si un error lo corta: una red
            // caída vuelve sola, un modelo local que no carga no.
            let local = matches!(backend, Backend::Local(_));
            let mut push: Box<dyn FnMut(AudioTapChunk) -> Result<Vec<LiveUpdate>, String>> = match backend {
                Backend::Groq { key, model } => {
                    let mut engine = LiveEngine::new(language);
                    Box::new(move |chunk| {
                        let groq = LiveSttBackend::Groq { api_key: &key, model: &model };
                        engine.push(groq, to_live(chunk)).map_err(|e| e.to_ui(false))
                    })
                }
                Backend::Local(pace) => {
                    // Parakeet detecta el idioma solo: `language` no le sirve.
                    let mut phrases = LocalPhrases::new(pace);
                    Box::new(move |chunk| phrases.push(chunk))
                }
            };
            while let Ok(chunk) = tap.recv() {
                if cancel_bg.load(Ordering::Relaxed) {
                    break;
                }
                let result = push(chunk);
                let Ok(mut captions) = captions_bg.lock() else {
                    break;
                };
                match result {
                    Ok(updates) => {
                        // Un error anterior (red caída un momento) se olvida
                        // cuando vuelve a entrar texto.
                        if !updates.is_empty() {
                            captions.error = None;
                        }
                        updates.into_iter().for_each(|u| captions.apply(u));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "subtítulos en vivo");
                        captions.error = Some(error);
                        if local {
                            break;
                        }
                    }
                }
            }
        })?;
        Ok(Self { cancel, captions })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Las frases del modelo local, una pista por hablante.
struct LocalPhrases {
    me: crate::local_stt::LivePhrases,
    others: crate::local_stt::LivePhrases,
}

impl LocalPhrases {
    fn new(pace: LinePace) -> Self {
        Self { me: crate::local_stt::LivePhrases::new(pace), others: crate::local_stt::LivePhrases::new(pace) }
    }

    fn push(&mut self, chunk: AudioTapChunk) -> Result<Vec<LiveUpdate>, String> {
        let (speaker, track) = match chunk.track {
            CaptureTrack::Mic => (Speaker::Me, &mut self.me),
            CaptureTrack::System => (Speaker::Others, &mut self.others),
        };
        let mono = atic_transcribe::decode::pcm_to_mono_16k(&chunk.samples, chunk.channels, chunk.sample_rate);
        // Las líneas no guardan el tiempo (los subtítulos nunca se guardan).
        let at = chunk.start_ms;
        Ok(track
            .feed(&mono)?
            .into_iter()
            .map(|text| LiveUpdate::Final(Segment { start_ms: at, end_ms: at, speaker, speaker_name: None, text }))
            .collect())
    }
}

fn to_live(chunk: AudioTapChunk) -> LivePcmChunk {
    LivePcmChunk {
        speaker: match chunk.track {
            CaptureTrack::Mic => Speaker::Me,
            CaptureTrack::System => Speaker::Others,
        },
        start_ms: chunk.start_ms,
        sample_rate: chunk.sample_rate,
        channels: chunk.channels,
        samples: chunk.samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(speaker: Speaker, text: &str) -> Segment {
        Segment { start_ms: 0, end_ms: 1000, speaker, speaker_name: None, text: text.into() }
    }

    #[test]
    fn el_plan_respeta_motor_y_llave() {
        use LocalModel::*;
        assert_eq!(plan(false, "groq", Some("k".into()), "", Ready, "normal"), Plan::Off);
        assert!(matches!(plan(true, "groq", None, "", Ready, "normal"), Plan::Unavailable(_)));
        assert!(matches!(plan(true, "groq", Some("  ".into()), "", Ready, "normal"), Plan::Unavailable(_)));
        let Plan::Run(Backend::Groq { key, .. }) = plan(true, "Groq", Some(" k ".into()), "", Ready, "normal") else {
            panic!("debía usar Groq");
        };
        assert_eq!(key, "k");
    }

    #[test]
    fn el_motor_local_no_pasa_por_groq_ni_con_llave() {
        use LocalModel::*;
        // Aunque haya llave de Groq: quien eligió el local no manda su audio a la nube.
        assert_eq!(plan(true, "local", Some("k".into()), "", Ready, "normal"), Plan::Run(Backend::Local(LinePace::Normal)));
        assert!(matches!(plan(true, "local", Some("k".into()), "", Missing, "normal"), Plan::Unavailable(_)));
        assert_eq!(plan(false, "local", None, "", Ready, "normal"), Plan::Off);
    }

    #[test]
    fn el_motor_local_lleva_el_largo_de_linea_elegido() {
        use LocalModel::*;
        assert_eq!(plan(true, "local", None, "", Ready, "short"), Plan::Run(Backend::Local(LinePace::Short)));
        assert_eq!(plan(true, "local", None, "", Ready, "long"), Plan::Run(Backend::Local(LinePace::Long)));
        assert_eq!(plan(true, "local", None, "", Ready, "?"), Plan::Run(Backend::Local(LinePace::Normal)));
    }

    #[test]
    fn sin_parakeet_el_motor_guardado_es_groq() {
        use LocalModel::*;
        assert!(uses_groq("local", Unsupported));
        assert!(uses_groq("groq", Ready));
        assert!(!uses_groq("local", Ready));
        assert!(!uses_groq("local", Missing));
        assert!(matches!(plan(true, "local", Some("k".into()), "", Unsupported, "normal"), Plan::Run(Backend::Groq { .. })));
    }

    #[test]
    fn la_final_reemplaza_la_parcial_y_se_guardan_pocas() {
        let mut c = Captions::default();
        c.apply(LiveUpdate::Partial(seg(Speaker::Me, "hola qu")));
        assert_eq!(c.partial.as_ref().map(|l| l.text.as_str()), Some("hola qu"));
        c.apply(LiveUpdate::Final(seg(Speaker::Others, " hola, ¿qué tal? ")));
        assert!(c.partial.is_none());
        assert_eq!(c.lines.back(), Some(&Line { me: false, text: "hola, ¿qué tal?".into() }));
        for n in 0..KEEP + 5 {
            c.apply(LiveUpdate::Final(seg(Speaker::Me, &format!("línea {n}"))));
        }
        assert_eq!(c.lines.len(), KEEP);
        assert_eq!(c.lines.back().unwrap().text, format!("línea {}", KEEP + 4));
    }

    #[test]
    fn la_basura_de_whisper_no_entra() {
        let mut c = Captions::default();
        c.apply(LiveUpdate::Final(seg(Speaker::Me, "...")));
        c.apply(LiveUpdate::Partial(seg(Speaker::Me, "   ")));
        assert!(c.lines.is_empty() && c.partial.is_none());
    }
}
