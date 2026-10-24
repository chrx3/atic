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

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

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
    state() == Download::Ready
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
    let dir = dir().ok_or("No se encontró la carpeta de datos de Atic.")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear la carpeta del modelo: {e}"))?;
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
            .map_err(|_| "No se pudo bajar el modelo. Revisa la conexión e intenta de nuevo.".to_string())?;
        let mut file = std::fs::File::create(&part).map_err(|e| format!("No se pudo escribir el modelo: {e}"))?;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut got = 0u64;
        loop {
            let n = response
                .read(&mut buffer)
                .map_err(|_| "Se cortó la descarga del modelo. Intenta de nuevo.".to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n]).map_err(|e| format!("No se pudo escribir el modelo: {e}"))?;
            got += n as u64;
            set(Download::Downloading { done: before + got.min(size), total });
        }
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        if got != size {
            let _ = std::fs::remove_file(&part);
            return Err("La descarga del modelo llegó incompleta. Intenta de nuevo.".into());
        }
        std::fs::rename(&part, &path).map_err(|e| format!("No se pudo guardar el modelo: {e}"))?;
        before += size;
    }
    Ok(())
}

/// Carga el modelo en otro hilo si está bajado y todavía no se cargó: al
/// empezar a dictar, así el primer texto no espera la carga.
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

fn load_into(slot: &mut Option<ParakeetModel>) -> Result<(), String> {
    if slot.is_some() {
        return Ok(());
    }
    if !on_disk() {
        return Err("Falta bajar el modelo local: Ajustes → Dictado.".into());
    }
    let dir = dir().ok_or("No se encontró la carpeta del modelo.")?;
    let started = std::time::Instant::now();
    let loaded = ParakeetModel::load(&dir, &Quantization::Int8).map_err(|e| format!("No se pudo cargar el modelo local: {e}"))?;
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
pub fn transcribe_segments(samples_16k_mono: &[f32]) -> Result<(String, Vec<(f32, f32, String)>), String> {
    let mut slot = MODEL.lock().unwrap_or_else(|e| e.into_inner());
    load_into(&mut slot)?;
    let Some(model) = slot.as_mut() else {
        return Err("No se pudo cargar el modelo local.".into());
    };
    let params = ParakeetParams { timestamp_granularity: Some(TimestampGranularity::Segment), ..Default::default() };
    let result = model.transcribe_with(samples_16k_mono, &params).map_err(|e| format!("El modelo local no transcribió: {e}"))?;
    let segments = result
        .segments
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.start, s.end, s.text))
        .collect();
    Ok((result.text.trim().to_string(), segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_descarga_suma_lo_que_se_anuncia() {
        assert_eq!((total() as f64 / 1e6).round() as u64, DOWNLOAD_MB);
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
}
