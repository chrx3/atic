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
/// Las variantes cuantizadas q5_0/q5_1 son el mismo modelo con menos disco y
/// RAM (y algo menos de CPU por ancho de banda); la pérdida de precisión es
/// mínima. Mismo motor y mismo formato, así que se descargan aparte.
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
        id: "small-q5_1",
        display_name: "Small q5_1 — misma precisión, menos disco (~181 MB)",
        file_name: "ggml-small-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        approx_size_bytes: 190_085_487,
    },
    ModelInfo {
        id: "medium",
        display_name: "Medium — más preciso, más CPU (~1.5 GB)",
        file_name: "ggml-medium.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        approx_size_bytes: 1_533_763_059,
    },
    ModelInfo {
        id: "medium-q5_0",
        display_name: "Medium q5_0 — misma precisión, menos disco (~514 MB)",
        file_name: "ggml-medium-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin",
        approx_size_bytes: 539_212_467,
    },
    ModelInfo {
        id: "large-v3-turbo",
        display_name: "Large v3 Turbo — máxima calidad, mucha CPU (~1.6 GB)",
        file_name: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        approx_size_bytes: 1_624_555_275,
    },
    ModelInfo {
        id: "large-v3-turbo-q5_0",
        display_name: "Large v3 Turbo q5_0 — máxima calidad, un tercio del disco (~547 MB)",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        approx_size_bytes: 574_041_195,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalogo_sin_ids_ni_archivos_duplicados() {
        let mut ids = HashSet::new();
        let mut files = HashSet::new();
        for info in CATALOG {
            assert!(ids.insert(info.id), "id duplicado: {}", info.id);
            assert!(
                files.insert(info.file_name),
                "archivo duplicado: {}",
                info.file_name
            );
            assert!(
                info.url.starts_with("https://"),
                "url insegura: {}",
                info.url
            );
            assert!(info.approx_size_bytes > 0, "tamaño inválido: {}", info.id);
            assert_eq!(find(info.id).map(|m| m.id), Some(info.id));
        }
    }

    #[test]
    fn variantes_cuantizadas_pesan_menos_que_su_modelo_base() {
        for (quant, base) in [
            ("small-q5_1", "small"),
            ("medium-q5_0", "medium"),
            ("large-v3-turbo-q5_0", "large-v3-turbo"),
        ] {
            let q = find(quant).expect("existe la variante cuantizada");
            let b = find(base).expect("existe el modelo base");
            assert!(
                q.approx_size_bytes < b.approx_size_bytes,
                "{quant} debería pesar menos que {base}"
            );
        }
    }
}
