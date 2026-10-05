//! Subtítulos mientras se graba, como `apps/desktop/src-tauri/src/live.rs`:
//! el tap de PCM de la captura va a `atic_transcribe::LiveEngine` por ventanas
//! de ~6 s. Es una confirmación de que el audio entra, no un documento: nunca
//! se guardan; lo que queda es la transcripción del archivo, hecha después.
//!
//! Solo con Groq: el prototipo no trae el Whisper local (feature `local`), y
//! mandar a la nube el audio de quien eligió el motor local no corresponde.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use atic_audio::{AudioTapChunk, CaptureTrack};
use atic_core::{Segment, Speaker};
use atic_transcribe::{LiveEngine, LivePcmChunk, LiveSttBackend, LiveUpdate};

use super::text;

/// Las líneas que se guardan en memoria; la tarjeta muestra solo las últimas.
const KEEP: usize = 24;

/// Qué hacer con los subtítulos según la configuración.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Off,
    Groq { key: String, model: String },
    /// Activados pero sin poder correr: el porqué, para una nota discreta.
    Unavailable(&'static str),
}

pub fn plan(enabled: bool, engine: &str, key: Option<String>, model: &str) -> Plan {
    if !enabled {
        return Plan::Off;
    }
    if !engine.eq_ignore_ascii_case("groq") {
        return Plan::Unavailable("Sin subtítulos: el motor local no está en esta versión.");
    }
    match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(key) => Plan::Groq {
            key,
            model: atic_transcribe::normalize_groq_whisper_model(model).to_string(),
        },
        None => Plan::Unavailable("Sin subtítulos: falta la llave de Groq en Ajustes."),
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
    pub fn spawn(
        tap: Receiver<AudioTapChunk>,
        language: Option<String>,
        key: String,
        model: String,
    ) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let captions = Arc::new(Mutex::new(Captions::default()));
        let (cancel_bg, captions_bg) = (cancel.clone(), captions.clone());
        std::thread::Builder::new().name("meetings-live".into()).spawn(move || {
            let mut engine = LiveEngine::new(language);
            let backend = LiveSttBackend::Groq { api_key: &key, model: &model };
            while let Ok(chunk) = tap.recv() {
                if cancel_bg.load(Ordering::Relaxed) {
                    break;
                }
                let result = engine.push(backend, to_live(chunk));
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
                        captions.error = Some(error.to_ui(false));
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
        assert_eq!(plan(false, "groq", Some("k".into()), ""), Plan::Off);
        assert!(matches!(plan(true, "local", Some("k".into()), ""), Plan::Unavailable(_)));
        assert!(matches!(plan(true, "groq", None, ""), Plan::Unavailable(_)));
        assert!(matches!(plan(true, "groq", Some("  ".into()), ""), Plan::Unavailable(_)));
        assert!(matches!(plan(true, "Groq", Some(" k ".into()), ""), Plan::Groq { ref key, .. } if key == "k"));
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
