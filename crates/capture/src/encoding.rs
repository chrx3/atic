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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_buffer_size() {
        let err = bgra_to_png(2, 2, &[0u8; 8]).unwrap_err();
        assert!(matches!(err, Error::BufferSize { expected: 16, got: 8 }));
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
}
