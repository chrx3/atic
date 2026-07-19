//! Catálogo y descarga bajo demanda de modelos GGML de Whisper.
//!
//! Los modelos NO se empaquetan con la app: se descargan del repositorio
//! oficial de whisper.cpp en Hugging Face cuando el usuario los elige.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Result, TranscribeError};

/// Metadatos de un modelo descargable.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    /// Identificador estable (usado en config y comandos).
    pub id: &'static str,
    /// Nombre legible para la UI.
    pub display_name: &'static str,
    /// Nombre del archivo local.
    pub file_name: &'static str,
    /// URL de descarga.
    pub url: &'static str,
    /// Tamaño aproximado en bytes (para la barra de progreso).
    pub approx_size_bytes: u64,
}

/// Catálogo de modelos ofrecidos.
/// Default de producto: `base` para dictado y reuniones; es una única
/// descarga pequeña y rápida. Modelos superiores son opt-in.
pub const CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: "base",
        display_name: "Base — rápido y ligero (~148 MB)",
        file_name: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        approx_size_bytes: 147_951_465,
    },
    ModelInfo {
        id: "small",
        display_name: "Small — más precisión (~466 MB)",
        file_name: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        approx_size_bytes: 487_601_967,
    },
    ModelInfo {
        id: "medium",
        display_name: "Medium — más preciso, más CPU (~1.5 GB)",
        file_name: "ggml-medium.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        approx_size_bytes: 1_533_763_059,
    },
    ModelInfo {
        id: "large-v3-turbo",
        display_name: "Large v3 Turbo — máxima calidad, mucha CPU (~1.6 GB)",
        file_name: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        approx_size_bytes: 1_624_555_275,
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn model_path(models_dir: &Path, info: &ModelInfo) -> PathBuf {
    models_dir.join(info.file_name)
}

/// Un modelo se considera descargado si su archivo final existe. La descarga
/// escribe primero a `.part` y renombra al terminar, así que la existencia del
/// archivo final implica que está completo.
pub fn is_downloaded(models_dir: &Path, info: &ModelInfo) -> bool {
    model_path(models_dir, info).exists()
}

/// Descarga el modelo mostrando progreso. Escribe a un temporal y renombra al
/// final para no dejar archivos a medias si se interrumpe.
pub fn download(
    models_dir: &Path,
    info: &ModelInfo,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<()> {
    std::fs::create_dir_all(models_dir)?;
    let dest = model_path(models_dir, info);
    let tmp = dest.with_extension("part");

    let client = reqwest::blocking::Client::builder().build()?;
    let mut resp = client.get(info.url).send()?.error_for_status()?;
    let total = resp.content_length().unwrap_or(info.approx_size_bytes);

    let mut file = std::fs::File::create(&tmp)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, &dest)?;
    Ok(())
}

/// Devuelve la ruta del modelo si está descargado, o un error claro si falta.
pub fn require_downloaded(models_dir: &Path, id: &str) -> Result<PathBuf> {
    let info = find(id).ok_or_else(|| TranscribeError::UnknownModel(id.to_string()))?;
    if !is_downloaded(models_dir, info) {
        return Err(TranscribeError::ModelNotDownloaded(id.to_string()));
    }
    Ok(model_path(models_dir, info))
}
