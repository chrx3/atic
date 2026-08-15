//! Codificación de frames BGRA (top-down) a PNG.
//!
//! GDI entrega los píxeles como BGRA de 32 bits con el canal alfa sin
//! significado (0). Se reordena a RGBA y se fuerza alfa opaco antes de
//! codificar, porque una captura de pantalla no tiene transparencia real.

use crate::error::{Error, Result};

/// Convierte un buffer BGRA top-down (`width * height * 4` bytes) a un PNG RGBA
/// opaco en memoria.
pub fn bgra_to_png(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(Error::InvalidDimensions(width, height));
    }
    let expected = width as usize * height as usize * 4;
    if bgra.len() != expected {
        return Err(Error::BufferSize {
            expected,
            got: bgra.len(),
        });
    }

    let mut rgba = vec![0u8; expected];
    for (src, dst) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        dst[0] = src[2]; // R ← B
        dst[1] = src[1]; // G
        dst[2] = src[0]; // B ← R
        dst[3] = 255; // alfa opaco
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer
            .write_image_data(&rgba)
            .map_err(|e| Error::Encode(e.to_string()))?;
    }
    Ok(out)
}

/// Lee (ancho, alto) de un PNG sin decodificar los píxeles.
pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    let reader = png::Decoder::new(bytes)
        .read_info()
        .map_err(|e| Error::Encode(e.to_string()))?;
    let info = reader.info();
    Ok((info.width, info.height))
}

/// Decodifica un PNG a RGBA de 8 bits (para copiar la imagen al portapapeles).
/// Asume PNGs RGBA como los que produce `bgra_to_png`.
pub fn png_to_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut reader = png::Decoder::new(bytes)
        .read_info()
        .map_err(|e| Error::Encode(e.to_string()))?;
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|e| Error::Encode(e.to_string()))?;
    buffer.truncate(info.buffer_size());
    Ok((info.width, info.height, buffer))
}

/// Decodifica un PNG a BGRA conservando el alpha (capa del overlay).
pub fn png_to_bgra(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|e| Error::Encode(e.to_string()))?;
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|e| Error::Encode(e.to_string()))?;
    buffer.truncate(info.buffer_size());
    let (width, height) = (info.width, info.height);
    let bgra = match info.color_type {
        png::ColorType::Rgba => {
            for px in buffer.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            buffer
        }
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(width as usize * height as usize * 4);
            for px in buffer.chunks_exact(3) {
                out.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(width as usize * height as usize * 4);
            for px in buffer.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(width as usize * height as usize * 4);
            for &g in &buffer {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        other => {
            return Err(Error::Encode(format!(
                "PNG con color no soportado: {other:?}"
            )));
        }
    };
    Ok((width, height, bgra))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_buffer_size() {
        let err = bgra_to_png(2, 2, &[0u8; 8]).unwrap_err();
        assert!(matches!(
            err,
            Error::BufferSize {
                expected: 16,
                got: 8
            }
        ));
    }

    #[test]
    fn rejects_zero_dimensions() {
        assert!(matches!(
            bgra_to_png(0, 4, &[]).unwrap_err(),
            Error::InvalidDimensions(0, 4)
        ));
    }

    #[test]
    fn encodes_and_roundtrips_pixels() {
        // 2x1: primer pixel azul puro (BGRA), segundo rojo puro.
        let bgra = vec![
            255, 0, 0, 0, // B=255 → azul
            0, 0, 255, 0, // R=255 → rojo
        ];
        let png_bytes = bgra_to_png(2, 1, &bgra).unwrap();

        let decoder = png::Decoder::new(png_bytes.as_slice());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();

        assert_eq!(info.width, 2);
        assert_eq!(info.height, 1);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        // Azul → RGBA (0,0,255,255).
        assert_eq!(&buf[0..4], &[0, 0, 255, 255]);
        // Rojo → RGBA (255,0,0,255).
        assert_eq!(&buf[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn reads_dimensions_and_decodes_back_to_rgba() {
        let bgra = vec![255, 0, 0, 0, 0, 0, 255, 0];
        let png_bytes = bgra_to_png(2, 1, &bgra).unwrap();

        assert_eq!(png_dimensions(&png_bytes).unwrap(), (2, 1));

        let (width, height, rgba) = png_to_rgba(&png_bytes).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(&rgba[0..4], &[0, 0, 255, 255]);
        assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn png_to_bgra_keeps_alpha() {
        let rgba = vec![10, 20, 30, 40, 0, 0, 0, 0];
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&rgba).unwrap();
        }
        let (width, height, bgra) = png_to_bgra(&out).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(&bgra[0..4], &[30, 20, 10, 40]);
        assert_eq!(&bgra[4..8], &[0, 0, 0, 0]);
    }
}
