//! Imágenes que van al agente como content multimodal, no como ruta en el texto.
//!
//! La vista adjunta rutas absolutas; cada adaptador las lee acá y arma el bloque
//! que su protocolo entiende (Anthropic `image`, Codex `attachments`, ACP
//! `ContentBlock::Image`).

use std::path::Path;

use base64::Engine;

/// Tope por archivo: más grande hincha el stdin y el contexto del modelo.
const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// MIME de una imagen que los tres backends aceptan, o `None` si no es imagen.
pub fn image_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

/// Bytes en base64 listos para un bloque de imagen.
pub fn read_image_base64(path: &Path) -> Result<(String, String), String> {
    let mime = image_mime(path)
        .ok_or_else(|| format!("no es una imagen soportada: {}", path.display()))?
        .to_string();
    let meta = std::fs::metadata(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "{} pesa más de {} MiB",
            path.display(),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok((mime, data))
}

/// Bloque Anthropic / Claude Code: `{ type: image, source: { type: base64, … } }`.
pub fn claude_image_block(path: &Path) -> Result<serde_json::Value, String> {
    let (mime, data) = read_image_base64(path)?;
    Ok(serde_json::json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": mime,
            "data": data,
        }
    }))
}

/// Adjunto Codex: `{ type: image, url: "data:…;base64,…" }`.
pub fn codex_image_attachment(path: &Path) -> Result<serde_json::Value, String> {
    let (mime, data) = read_image_base64(path)?;
    Ok(serde_json::json!({
        "type": "image",
        "url": format!("data:{mime};base64,{data}"),
    }))
}

/// Quita del texto las rutas que ya van embebidas como imagen.
pub fn strip_embedded_paths(text: &str, paths: &[String]) -> String {
    let mut out = text.to_string();
    for p in paths {
        out = out.replace(p, "");
    }
    out.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mime_por_extension() {
        assert_eq!(image_mime(Path::new("a.PNG")), Some("image/png"));
        assert_eq!(image_mime(Path::new("a.jpeg")), Some("image/jpeg"));
        assert_eq!(image_mime(Path::new("a.rs")), None);
    }

    #[test]
    fn lee_png_minimo() {
        // PNG 1×1 transparente.
        let bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let dir = std::env::temp_dir().join(format!(
            "atic-img-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dot.png");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        let block = claude_image_block(&path).unwrap();
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert!(!block["source"]["data"].as_str().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn strip_deja_el_texto() {
        let t = strip_embedded_paths("mira\nC:\\a.png\nporfa", &["C:\\a.png".into()]);
        assert_eq!(t, "mira\nporfa");
    }
}
