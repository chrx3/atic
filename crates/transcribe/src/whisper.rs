//! Backend de transcripción basado en whisper-rs (whisper.cpp).

use std::path::Path;
use std::sync::{Arc, Mutex, Once};

use atic_core::{Segment, Speaker};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::decode::WHISPER_RATE;
use crate::error::{Result, TranscribeError};

/// Tamaño de cada trozo enviado a Whisper (10 min @ 16 kHz).
const CHUNK_SAMPLES: usize = 10 * 60 * WHISPER_RATE as usize;
/// Solape entre trozos para no cortar palabras en el borde.
const OVERLAP_SAMPLES: usize = 2 * WHISPER_RATE as usize;
const OVERLAP_MS: i64 = (OVERLAP_SAMPLES as i64 * 1000) / WHISPER_RATE as i64;

/// Perfil de decode: reuniones priorizan calidad; dictado prioriza latencia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranscribeMode {
    /// Beam search + reintentos de temperatura (mejor para reuniones largas).
    #[default]
    Meeting,
    /// Greedy sin fallback de temperatura (dictado corto casi instantáneo).
    Dictation,
}

/// Redirige logs de whisper.cpp/GGML a `tracing` (feature `tracing_backend`).
static INIT_LOGGING: Once = Once::new();

fn init_logging() {
    INIT_LOGGING.call_once(whisper_rs::install_logging_hooks);
}

/// Backend compilado vía features de Cargo (`metal` / `cuda` / `vulkan`).
/// Si el dispositivo no está disponible, whisper.cpp puede caer a CPU.
fn compiled_whisper_backend() -> &'static str {
    if cfg!(feature = "metal") {
        "Metal"
    } else if cfg!(feature = "cuda") {
        "CUDA"
    } else if cfg!(feature = "vulkan") {
        "Vulkan"
    } else {
        "CPU"
    }
}

fn whisper_use_gpu() -> bool {
    cfg!(feature = "metal") || cfg!(feature = "cuda") || cfg!(feature = "vulkan")
}

/// ¿El texto es un marcador de silencio / vacío de Whisper?
fn is_silence_marker(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    // Normaliza: quita espacios internos y pasa a minúsculas.
    let compact: String = t
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        compact.as_str(),
        "[silence]"
            | "(silence)"
            | "[silence]."
            | "(silence)."
            | "[blank_audio]"
            | "[blankaudio]"
            | "[inaudible]"
            | "(inaudible)"
            | "[music]"
            | "(music)"
            | "[applause]"
            | "(applause)"
            | "silence"
    ) || compact.starts_with("[silence")
        || compact.starts_with("(silence")
        || compact.starts_with("[music")
        || compact.starts_with("(music")
        || compact.starts_with("[blank")
}

/// Modelo de Whisper cargado en memoria, reutilizable entre pistas.
pub struct WhisperModel {
    ctx: WhisperContext,
}

impl WhisperModel {
    /// Carga un modelo GGML desde disco.
    pub fn load(model_path: &Path) -> Result<Self> {
        init_logging();
        let path = model_path.to_str().ok_or(TranscribeError::InvalidPath)?;
        let backend = compiled_whisper_backend();
        let use_gpu = whisper_use_gpu();
        tracing::info!(
            backend,
            use_gpu,
            path,
            "Cargando modelo Whisper (backend compilado)"
        );
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(path, params)?;
        Ok(Self { ctx })
    }

    /// Transcribe una pista mono 16 kHz, etiquetando cada segmento con el
    /// hablante indicado. `language` en `None` activa la autodetección.
    /// `on_progress` recibe el avance 0..100 de esta pista.
    ///
    /// Audios largos se trocean (10 min + 2 s de solape) para evitar que el
    /// decoder degenere en silencio/alucinaciones en grabaciones >15 min.
    pub fn transcribe_track(
        &self,
        samples: &[f32],
        speaker: Speaker,
        language: Option<&str>,
        mode: TranscribeMode,
        on_progress: impl FnMut(i32) + Send + 'static,
    ) -> Result<Vec<Segment>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }

        // Un solo trozo: camino directo.
        if samples.len() <= CHUNK_SAMPLES {
            let progress = Arc::new(Mutex::new(on_progress));
            return self.transcribe_chunk(samples, speaker, language, mode, 0, progress);
        }

        let step = CHUNK_SAMPLES - OVERLAP_SAMPLES;
        let mut starts = Vec::new();
        let mut start = 0usize;
        while start < samples.len() {
            starts.push(start);
            if start + CHUNK_SAMPLES >= samples.len() {
                break;
            }
            start += step;
        }
        let chunk_count = starts.len().max(1) as f32;
        let progress = Arc::new(Mutex::new(on_progress));

        let mut all = Vec::new();
        for (idx, &chunk_start) in starts.iter().enumerate() {
            let chunk_end = (chunk_start + CHUNK_SAMPLES).min(samples.len());
            let chunk = &samples[chunk_start..chunk_end];
            let offset_ms = (chunk_start as i64 * 1000) / WHISPER_RATE as i64;
            let base = idx as f32 / chunk_count;

            let prog = progress.clone();
            let chunk_progress = Arc::new(Mutex::new(move |p: i32| {
                let global = ((base + (p as f32 / 100.0) / chunk_count) * 100.0) as i32;
                if let Ok(mut cb) = prog.lock() {
                    cb(global.clamp(0, 100));
                }
            }));

            let segs =
                self.transcribe_chunk(chunk, speaker, language, mode, offset_ms, chunk_progress)?;

            // En trozos siguientes, descartar la zona de solape ya cubierta.
            if idx == 0 {
                all.extend(segs);
            } else {
                for seg in segs {
                    let rel_start = seg.start_ms - offset_ms;
                    if rel_start < OVERLAP_MS {
                        continue;
                    }
                    all.push(seg);
                }
            }
        }
        if let Ok(mut cb) = progress.lock() {
            cb(100);
        }
        Ok(all)
    }

    fn transcribe_chunk(
        &self,
        samples: &[f32],
        speaker: Speaker,
        language: Option<&str>,
        mode: TranscribeMode,
        offset_ms: i64,
        on_progress: Arc<Mutex<impl FnMut(i32) + Send + 'static>>,
    ) -> Result<Vec<Segment>> {
        let mut state = self.ctx.create_state()?;

        let mut params = match mode {
            // Beam search mejora el reconocimiento de palabras frente a greedy,
            // a costa de más CPU. beam_size=5 es el default de whisper.cpp.
            TranscribeMode::Meeting => FullParams::new(SamplingStrategy::BeamSearch {
                beam_size: 5,
                patience: -1.0,
            }),
            // Dictado: greedy (best_of=1) = un solo pase, latencia mínima.
            TranscribeMode::Dictation => FullParams::new(SamplingStrategy::Greedy { best_of: 1 }),
        };
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Dictado no muestra marcas temporales: evitar que Whisper las genere
        // reduce tokens y acelera de forma notable frases muy cortas.
        params.set_no_timestamps(mode == TranscribeMode::Dictation);
        params.set_single_segment(mode == TranscribeMode::Dictation);
        params.set_print_special(false);
        params.set_suppress_blank(true);
        // Reduce tokens no-hablados ([Music], etc.) que ensucian el texto.
        params.set_suppress_nst(true);
        // Evita que el contexto de ventanas anteriores contamine trozos largos
        // (bucle de [silence] / alucinaciones en grabaciones >15 min).
        params.set_no_context(true);
        // Decodificación determinista: menos inventos con audio ruidoso.
        params.set_temperature(0.0);
        // Reuniones: reintentos con temperatura creciente si falla el umbral.
        // Dictado: sin fallback (evita 2–3× el tiempo en frases cortas).
        params.set_temperature_inc(match mode {
            TranscribeMode::Meeting => 0.2,
            TranscribeMode::Dictation => 0.0,
        });
        params.set_entropy_thold(2.4);
        params.set_logprob_thold(-1.0);
        // Dictado: umbral más bajo → descarta soplido/ventilador como no-habla.
        params.set_no_speech_thold(match mode {
            TranscribeMode::Meeting => 0.6,
            TranscribeMode::Dictation => 0.45,
        });
        // Preferir cortes en límites de palabra (reuniones). Dictado = 1 segmento.
        params.set_split_on_word(mode == TranscribeMode::Meeting);

        // Demasiados hilos empeoran latencia por contención; 8 basta en desktop.
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8) as i32)
            .unwrap_or(4);
        params.set_n_threads(threads);

        match language {
            Some(lang) if !lang.is_empty() && lang != "auto" => {
                // Forzar idioma: evita alucinaciones en inglés con audio ruidoso.
                params.set_language(Some(lang));
                params.set_detect_language(false);
                if lang == "es" {
                    params.set_initial_prompt(match mode {
                        TranscribeMode::Meeting => SPANISH_MEETING_PROMPT,
                        // Prompt corto: menos sesgo a jerga de reuniones.
                        TranscribeMode::Dictation => SPANISH_DICTATION_PROMPT,
                    });
                }
            }
            _ => {
                params.set_detect_language(true);
            }
        }

        let prog = on_progress.clone();
        params.set_progress_callback_safe(move |p| {
            if let Ok(mut cb) = prog.lock() {
                cb(p);
            }
        });

        state.full(params, samples)?;

        let n = state.full_n_segments();
        let mut segments = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n {
            let Some(seg) = state.get_segment(i) else {
                continue;
            };
            // Descarta tramos que Whisper marca como no-habla.
            let no_speech_cut = match mode {
                TranscribeMode::Meeting => 0.6,
                TranscribeMode::Dictation => 0.5,
            };
            if seg.no_speech_probability() > no_speech_cut {
                continue;
            }
            let text = seg.to_str()?.trim().to_string();
            if is_silence_marker(&text) {
                continue;
            }
            // Las marcas de tiempo vienen en centisegundos.
            let start_ms = seg.start_timestamp() * 10 + offset_ms;
            let end_ms = seg.end_timestamp() * 10 + offset_ms;
            segments.push(Segment {
                start_ms,
                end_ms,
                speaker,
                speaker_name: None,
                text,
            });
        }
        Ok(segments)
    }
}

/// Prompt corto en español: ancla el idioma y el vocabulario de reuniones
/// sin sesgar demasiado el contenido.
const SPANISH_MEETING_PROMPT: &str = "\
Transcripción literal de una reunión en español de Chile. \
Conserva nombres propios, siglas y términos técnicos. \
Ejemplos: reunión, agenda, acuerdo, pendiente, seguimiento, \
SQL, Python, Power BI, Looker, cloud, entry level.";

/// Dictado: ancla idioma sin vocabulario de reuniones.
const SPANISH_DICTATION_PROMPT: &str = "\
Dictado en español de Chile. Transcribe solo lo hablado, sin inventar.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_silence_markers() {
        assert!(is_silence_marker("[ Silence ]"));
        assert!(is_silence_marker("[silence]"));
        assert!(is_silence_marker("(silence)"));
        assert!(is_silence_marker("[Music]"));
        assert!(is_silence_marker("[blank_audio]"));
        assert!(is_silence_marker(""));
        assert!(!is_silence_marker("Hola mundo"));
    }

    #[test]
    fn spanish_prompt_is_anchored() {
        assert!(SPANISH_MEETING_PROMPT.contains("español"));
        assert!(SPANISH_MEETING_PROMPT.contains("SQL"));
        assert!(SPANISH_DICTATION_PROMPT.contains("español"));
        assert!(!SPANISH_DICTATION_PROMPT.contains("SQL"));
    }

    #[test]
    fn chunk_constants_are_sane() {
        const _: () = assert!(CHUNK_SAMPLES > OVERLAP_SAMPLES);
        assert_eq!(OVERLAP_MS, 2000);
        assert_eq!(CHUNK_SAMPLES, 10 * 60 * 16_000);
    }
}
