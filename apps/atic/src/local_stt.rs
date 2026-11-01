//! El motor local de voz: Parakeet TDT 0.6B v3 (NVIDIA, CC-BY-4.0) en int8,
//! con ONNX Runtime en la CPU (`transcribe-rs`). Dicta y transcribe reuniones
//! sin internet ni llave, en español, inglés y otras 23 lenguas europeas; no
//! resume. Es el valor `"parakeet"` de `dictation_backend` y `meeting_backend`
//! («local» ya era la voz de macOS en Mac).
//!
//! El modelo no viene con Atic: se baja una vez (≈ 671 MB) a la carpeta de
//! modelos, de la conversión a ONNX de `istupakov` en Hugging Face, fijada a
//! una revisión. Cada archivo baja a un `.part` y se renombra al terminar: uno
//! a medias nunca pasa por listo. Se carga la primera vez que se usa y queda
//! en memoria.
//!
//! En Mac Intel no hay ONNX Runtime precompilado: no hay Parakeet
//! (`SUPPORTED`), el selector no lo ofrece y el motor local es la voz de macOS.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
use transcribe_rs::onnx::Quantization;
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
use transcribe_rs::transcriber::{Transcriber, VadChunked, VadChunkedConfig};
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
use transcribe_rs::vad::{EnergyVad, SmoothedVad};
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
use transcribe_rs::TranscribeOptions;

/// Si este equipo puede usar Parakeet (todos menos Mac Intel).
pub const SUPPORTED: bool = !cfg!(all(target_os = "macos", target_arch = "x86_64"));

/// El modelo, como se le nombra al usuario.
pub const NAME: &str = "Parakeet TDT 0.6B v3";

/// Cuántos idiomas entiende (los europeos de Parakeet v3), para los textos.
pub const LANGUAGES: u32 = 25;

/// Lo que pesa la descarga, para los textos («Bajar modelo · 671 MB»).
pub const DOWNLOAD_MB: u64 = 671;

/// La revisión del repositorio: los archivos no cambian bajo los pies.
const REVISION: &str = "8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce";
const REPO: &str = "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve";
/// La carpeta dentro de la de modelos de Atic.
const FOLDER: &str = "parakeet-tdt-0.6b-v3-int8";
/// Los archivos y su tamaño exacto (también sirve de control al terminar).
const FILES: [(&str, u64); 4] = [
    ("encoder-model.int8.onnx", 652_183_999),
    ("decoder_joint-model.int8.onnx", 18_202_004),
    ("nemo128.onnx", 139_764),
    ("vocab.txt", 93_939),
];
/// La frecuencia que espera el modelo.
pub const RATE: u32 = 16_000;

/// En qué va la descarga del modelo.
#[derive(Clone, Debug, PartialEq)]
pub enum Download {
    Idle,
    Downloading { done: u64, total: u64 },
    Ready,
    Failed(String),
}

static STATE: Mutex<Option<Download>> = Mutex::new(None);
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
static MODEL: Mutex<Option<ParakeetModel>> = Mutex::new(None);

fn total() -> u64 {
    FILES.iter().map(|(_, size)| size).sum()
}

fn dir() -> Option<PathBuf> {
    atic_core::AppDirs::new().ok().map(|dirs| dirs.models_dir().join(FOLDER))
}

/// Están los cuatro archivos, completos.
fn on_disk() -> bool {
    dir().is_some_and(|dir| {
        FILES
            .iter()
            .all(|(name, size)| std::fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == *size))
    })
}

pub fn state() -> Download {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    // Lo bajado por otro lado (o borrado a mano) se nota al mirar; un error
    // queda a la vista hasta el próximo intento.
    if !matches!(*state, Some(Download::Downloading { .. })) {
        if on_disk() {
            *state = Some(Download::Ready);
        } else if !matches!(*state, Some(Download::Failed(_))) {
            *state = Some(Download::Idle);
        }
    }
    state.clone().unwrap_or(Download::Idle)
}

pub fn ready() -> bool {
    SUPPORTED && state() == Download::Ready
}

fn set(next: Download) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(next);
}

/// Baja el modelo en otro hilo. Si ya está o ya se está bajando, no hace nada.
pub fn start_download() {
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*state, Some(Download::Downloading { .. })) || on_disk() {
            if on_disk() {
                *state = Some(Download::Ready);
            }
            return;
        }
        *state = Some(Download::Downloading { done: 0, total: total() });
    }
    let spawned = std::thread::Builder::new().name("modelo-local".into()).spawn(|| {
        let next = match download() {
            Ok(()) => Download::Ready,
            Err(error) => {
                tracing::warn!(%error, "modelo local: no se pudo bajar");
                Download::Failed(error)
            }
        };
        set(next);
    });
    if let Err(error) = spawned {
        set(Download::Failed(error.to_string()));
    }
}

fn download() -> Result<(), String> {
    let dir = dir().ok_or(crate::i18n::t("pill.localStt.noDataDir"))?;
    std::fs::create_dir_all(&dir).map_err(|e| crate::i18n::tf("pill.localStt.createDirFailed", &[("error", &e)]))?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // Sin tope total: 650 MB en una conexión lenta tardan.
        .timeout(None)
        .build()
        .map_err(|e| e.to_string())?;
    let total = total();
    let mut before = 0u64;
    for (name, size) in FILES {
        let path = dir.join(name);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == size) {
            before += size;
            set(Download::Downloading { done: before, total });
            continue;
        }
        let part = dir.join(format!("{name}.part"));
        let mut response = client
            .get(format!("{REPO}/{REVISION}/{name}"))
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| crate::i18n::t("pill.localStt.downloadFailed").to_string())?;
        let mut file = std::fs::File::create(&part).map_err(|e| crate::i18n::tf("pill.localStt.writeFailed", &[("error", &e)]))?;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut got = 0u64;
        loop {
            let n = response
                .read(&mut buffer)
                .map_err(|_| crate::i18n::t("pill.localStt.downloadCut").to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n]).map_err(|e| crate::i18n::tf("pill.localStt.writeFailed", &[("error", &e)]))?;
            got += n as u64;
            set(Download::Downloading { done: before + got.min(size), total });
        }
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        if got != size {
            let _ = std::fs::remove_file(&part);
            return Err(crate::i18n::t("pill.localStt.incomplete").into());
        }
        std::fs::rename(&part, &path).map_err(|e| crate::i18n::tf("pill.localStt.saveFailed", &[("error", &e)]))?;
        before += size;
    }
    Ok(())
}

/// Carga el modelo en otro hilo si está bajado y todavía no se cargó: al
/// empezar a dictar, así el primer texto no espera la carga.
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
pub fn preload() {
    if !on_disk() || MODEL.lock().map_or(true, |m| m.is_some()) {
        return;
    }
    let _ = std::thread::Builder::new().name("modelo-local-carga".into()).spawn(|| {
        if let Err(error) = load_into(&mut MODEL.lock().unwrap_or_else(|e| e.into_inner())) {
            tracing::warn!(%error, "modelo local: no cargó");
        }
    });
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub fn preload() {}

#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
fn load_into(slot: &mut Option<ParakeetModel>) -> Result<(), String> {
    if slot.is_some() {
        return Ok(());
    }
    if !on_disk() {
        return Err(crate::i18n::t("pill.localStt.missing").into());
    }
    let dir = dir().ok_or(crate::i18n::t("pill.localStt.noModelDir"))?;
    let started = std::time::Instant::now();
    let loaded = ParakeetModel::load(&dir, &Quantization::Int8).map_err(|e| crate::i18n::tf("pill.localStt.loadFailedWith", &[("error", &e)]))?;
    tracing::info!(ms = started.elapsed().as_millis() as u64, "modelo local cargado");
    *slot = Some(loaded);
    Ok(())
}

/// Transcribe audio mono a 16 kHz. Carga el modelo la primera vez (unos
/// segundos) y lo deja en memoria para las siguientes.
pub fn transcribe(samples_16k_mono: &[f32]) -> Result<String, String> {
    Ok(transcribe_segments(samples_16k_mono)?.0)
}

/// Como `transcribe`, con las frases y su tiempo en segundos (para Reuniones).
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
pub fn transcribe_segments(samples_16k_mono: &[f32]) -> Result<(String, Vec<(f32, f32, String)>), String> {
    let mut slot = MODEL.lock().unwrap_or_else(|e| e.into_inner());
    load_into(&mut slot)?;
    let Some(model) = slot.as_mut() else {
        return Err(crate::i18n::t("pill.localStt.loadFailed").into());
    };
    let params = ParakeetParams { timestamp_granularity: Some(TimestampGranularity::Segment), ..Default::default() };
    let result = model.transcribe_with(samples_16k_mono, &params).map_err(|e| crate::i18n::tf("pill.localStt.transcribeFailed", &[("error", &e)]))?;
    let segments = result
        .segments
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.start, s.end, s.text))
        .collect();
    Ok((result.text.trim().to_string(), segments))
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub fn transcribe_segments(_samples_16k_mono: &[f32]) -> Result<(String, Vec<(f32, f32, String)>), String> {
    Err(crate::i18n::t("pill.localStt.missing").into())
}

/// Qué tan largas salen las líneas de los subtítulos locales: cuánta pausa
/// espera el corte y cuánto aguanta una frase sin parar. Cortas salen antes
/// pero pueden partir una frase; largas esperan una pausa y salen más enteras.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LinePace {
    Short,
    #[default]
    Normal,
    Long,
}

impl LinePace {
    /// Del `live_local_lines` de la config; lo desconocido es el término medio.
    pub fn parse(value: &str) -> Self {
        match value {
            "short" => Self::Short,
            "long" => Self::Long,
            _ => Self::Normal,
        }
    }

    /// Ventanas de 30 ms de audio previo, ventanas de silencio para dar la
    /// frase por terminada, segundos máximos sin pausa y segundos hacia atrás
    /// donde buscar el momento más callado para cortar.
    #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
    fn windows(self) -> (usize, usize, f32, f32) {
        match self {
            Self::Short => (10, 8, 4.0, 1.0),
            Self::Normal => (15, 15, 8.0, 2.0),
            Self::Long => (15, 24, 12.0, 3.0),
        }
    }
}

/// Subtítulos en vivo con el modelo local, de una pista. Parakeet no escucha de
/// corrido: transcribe tramos enteros. Un detector de voz por energía corta el
/// audio en cada pausa (o a los 8 s si nadie para) y cada frase se decodifica
/// sola apenas termina, como en Atic para Android (ahí con Silero). La energía
/// no pide bajar un segundo modelo; el umbral es el del gate de `atic-audio`.
///
/// Una por pista: las dos comparten el modelo, que decodifica de a una frase.
#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
pub struct LivePhrases(VadChunked);

#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
impl LivePhrases {
    pub fn new(pace: LinePace) -> Self {
        // Ventanas de 30 ms; 2 seguidas para entrar a la voz. Con el ritmo
        // normal, 450 ms de cola antes de darla por terminada y 450 ms de
        // audio previo al inicio.
        let (prefill, hangover, max_secs, split_secs) = pace.windows();
        let vad = SmoothedVad::new(Box::new(EnergyVad::new(480, 0.008)), prefill, hangover, 2);
        let config = VadChunkedConfig {
            min_chunk_secs: 0.5,
            max_chunk_secs: max_secs,
            padding_secs: 0.25,
            // Una frase sin pausa se corta en el momento más callado de los
            // últimos segundos, no a media palabra.
            smart_split_search_secs: Some(split_secs),
            merge_separator: " ".into(),
        };
        Self(VadChunked::new(Box::new(vad), config, TranscribeOptions::default()))
    }

    /// Audio mono a 16 kHz → las frases que terminaron con él (sin las vacías).
    /// Carga el modelo la primera vez; sin `preload()` antes, eso son unos segundos.
    pub fn feed(&mut self, samples_16k_mono: &[f32]) -> Result<Vec<String>, String> {
        let mut slot = MODEL.lock().unwrap_or_else(|e| e.into_inner());
        load_into(&mut slot)?;
        let Some(model) = slot.as_mut() else {
            return Err(crate::i18n::t("pill.localStt.loadFailed").into());
        };
        let done = self
            .0
            .feed(model, samples_16k_mono)
            .map_err(|e| crate::i18n::tf("pill.localStt.transcribeFailed", &[("error", &e)]))?;
        Ok(done.into_iter().map(|r| r.text.trim().to_string()).filter(|text| !text.is_empty()).collect())
    }
}

/// Sin Parakeet (Mac Intel) no hay frases en vivo: `meetings::live` ni lo intenta.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub struct LivePhrases;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
impl LivePhrases {
    pub fn new(_pace: LinePace) -> Self {
        Self
    }

    pub fn feed(&mut self, _samples_16k_mono: &[f32]) -> Result<Vec<String>, String> {
        Err(crate::i18n::t("pill.localStt.missing").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_descarga_suma_lo_que_se_anuncia() {
        assert_eq!((total() as f64 / 1e6).round() as u64, DOWNLOAD_MB);
    }

    #[test]
    fn el_largo_de_linea_desconocido_es_el_normal() {
        assert_eq!(LinePace::parse("short"), LinePace::Short);
        assert_eq!(LinePace::parse("long"), LinePace::Long);
        assert_eq!(LinePace::parse("normal"), LinePace::Normal);
        assert_eq!(LinePace::parse("otro"), LinePace::Normal);
        assert_eq!(LinePace::parse(""), LinePace::default());
    }

    #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
    #[test]
    fn las_lineas_largas_esperan_mas_que_las_cortas() {
        let (_, short_wait, short_max, short_split) = LinePace::Short.windows();
        let (_, normal_wait, normal_max, _) = LinePace::Normal.windows();
        let (_, long_wait, long_max, _) = LinePace::Long.windows();
        assert!(short_wait < normal_wait && normal_wait < long_wait);
        assert!(short_max < normal_max && normal_max < long_max);
        // La búsqueda del corte cabe en lo que dura la línea y todas se arman.
        assert!(short_split < short_max);
        for pace in [LinePace::Short, LinePace::Normal, LinePace::Long] {
            let _ = LivePhrases::new(pace);
        }
    }

    /// Mide la velocidad con el modelo ya bajado: `PILL_STT_WAV=x.wav cargo
    /// test local_stt -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn transcribe_un_wav_de_verdad() {
        let Some(path) = std::env::var_os("PILL_STT_WAV") else { return };
        let samples = atic_transcribe::decode::load_wav_mono_16k(std::path::Path::new(&path)).unwrap();
        let secs = samples.len() as f32 / RATE as f32;
        let load = std::time::Instant::now();
        transcribe(&samples[..(RATE as usize).min(samples.len())]).unwrap();
        let load_ms = load.elapsed().as_millis();
        let started = std::time::Instant::now();
        let text = transcribe(&samples).unwrap();
        let took = started.elapsed().as_secs_f32();
        println!("carga+1s {load_ms} ms · audio {secs:.1} s · transcribir {took:.2} s · RTF {:.3}\n{text}", took / secs);
        assert!(!text.is_empty());
    }

    /// Los subtítulos en vivo con un WAV de verdad, a bloques de 20 ms como el
    /// tap de la captura: `PILL_STT_WAV=x.wav cargo test local_stt -- --ignored
    /// --nocapture live`. Muestra a qué segundo del audio sale cada frase;
    /// `PILL_STT_LINES=short|long` prueba los otros largos de línea.
    #[test]
    #[ignore]
    fn frases_en_vivo_de_un_wav_de_verdad() {
        let Some(path) = std::env::var_os("PILL_STT_WAV") else { return };
        let samples = atic_transcribe::decode::load_wav_mono_16k(std::path::Path::new(&path)).unwrap();
        let mut live = LivePhrases::new(LinePace::parse(&std::env::var("PILL_STT_LINES").unwrap_or_default()));
        let mut phrases = 0;
        for (n, block) in samples.chunks(320).enumerate() {
            for text in live.feed(block).unwrap() {
                phrases += 1;
                println!("[{:5.1} s] {text}", (n + 1) as f32 * 0.02);
            }
        }
        assert!(phrases > 0, "ninguna frase");
    }
}
