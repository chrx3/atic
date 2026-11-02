//! Conversiones de color para el codificador. Viven en este crate (y no en la pill) porque las
//! dependencias compilan optimizadas incluso en desarrollo: en la pill de dev la conversión de cada
//! cuadro sin optimizar dejaba la grabación de 1080p en 3 o 4 cuadros por segundo.

/// BGRA (filas de `pitch` bytes) a NV12 con BT.709 en rango limitado: el plano
/// Y entero y luego U y V intercalados, uno por cada 2×2 píxeles. `width` y
/// `height` son pares.
pub fn bgra_to_nv12(bgra: &[u8], pitch: usize, width: usize, height: usize, out: &mut Vec<u8>) {
    out.resize(width * height * 3 / 2, 0);
    let (luma, chroma) = out.split_at_mut(width * height);
    for (y, row) in luma.chunks_exact_mut(width).enumerate() {
        let src = &bgra[y * pitch..y * pitch + width * 4];
        for (dst, px) in row.iter_mut().zip(src.chunks_exact(4)) {
            let (b, g, r) = (px[0] as i32, px[1] as i32, px[2] as i32);
            *dst = (16 + ((47 * r + 157 * g + 16 * b + 128) >> 8)) as u8;
        }
    }
    for (y, row) in chroma.chunks_exact_mut(width).enumerate() {
        let top = &bgra[2 * y * pitch..];
        let bottom = &bgra[(2 * y + 1) * pitch..];
        for (x, uv) in row.chunks_exact_mut(2).enumerate() {
            let (mut b, mut g, mut r) = (0, 0, 0);
            for line in [top, bottom] {
                for px in line[x * 8..x * 8 + 8].chunks_exact(4) {
                    b += px[0] as i32;
                    g += px[1] as i32;
                    r += px[2] as i32;
                }
            }
            let (b, g, r) = (b / 4, g / 4, r / 4);
            uv[0] = (128 + ((-26 * r - 87 * g + 112 * b + 128) >> 8)).clamp(0, 255) as u8;
            uv[1] = (128 + ((112 * r - 102 * g - 10 * b + 128) >> 8)).clamp(0, 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, pitch: usize, [b, g, r]: [u8; 3]) -> Vec<u8> {
        let mut bgra = vec![0u8; pitch * height];
        for y in 0..height {
            for x in 0..width {
                bgra[y * pitch + x * 4..y * pitch + x * 4 + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        bgra
    }

    #[test]
    fn blanco_y_negro_en_rango_limitado() {
        let mut out = Vec::new();
        bgra_to_nv12(&solid(4, 2, 16, [255, 255, 255]), 16, 4, 2, &mut out);
        assert_eq!(out.len(), 12);
        assert!(out[..8].iter().all(|&y| y == 235), "{out:?}");
        assert!(out[8..].iter().all(|&c| (127..=129).contains(&c)), "{out:?}");

        bgra_to_nv12(&solid(4, 2, 16, [0, 0, 0]), 16, 4, 2, &mut out);
        assert!(out[..8].iter().all(|&y| y == 16));
        assert!(out[8..].iter().all(|&c| c == 128));
    }

    #[test]
    fn el_rojo_va_en_v_y_se_ignora_el_relleno_de_la_fila() {
        let mut out = Vec::new();
        // Filas de 24 bytes para 4 píxeles: los 8 de relleno no cuentan.
        let mut bgra = solid(4, 2, 24, [0, 0, 255]);
        for y in 0..2 {
            bgra[y * 24 + 16..y * 24 + 24].fill(200);
        }
        bgra_to_nv12(&bgra, 24, 4, 2, &mut out);
        assert!(out[..8].iter().all(|&y| y == 63), "{out:?}");
        let (u, v) = (out[8], out[9]);
        assert!(u < 110 && v > 230, "u {u}, v {v}");
        assert_eq!(&out[8..10], &out[10..12]);
    }
}
