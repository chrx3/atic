//! Frame en memoria: píxeles BGRA con su posición física en el escritorio
//! virtual. Base de las capturas de monitor, región y ventana.

use crate::encoding::bgra_to_png;
use crate::error::Result;
use crate::geometry::Rect;

/// Imagen capturada en memoria. `bgra` es top-down y mide
/// `bounds.width * bounds.height * 4` bytes.
#[derive(Clone)]
pub struct Frame {
    pub bounds: Rect,
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn new(bounds: Rect, bgra: Vec<u8>) -> Self {
        debug_assert_eq!(
            bgra.len(),
            bounds.width as usize * bounds.height as usize * 4,
            "el buffer BGRA no coincide con las dimensiones del frame"
        );
        Self { bounds, bgra }
    }

    pub fn width(&self) -> u32 {
        self.bounds.width
    }

    pub fn height(&self) -> u32 {
        self.bounds.height
    }

    /// RGBA de un píxel en coords del buffer (origen 0,0 = esquina del PNG).
    pub fn pixel_rgba(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 {
            return None;
        }
        let px = x as u32;
        let py = y as u32;
        if px >= self.width() || py >= self.height() {
            return None;
        }
        let i = (py as usize * self.width() as usize + px as usize) * 4;
        Some([self.bgra[i + 2], self.bgra[i + 1], self.bgra[i], self.bgra[i + 3]])
    }

    /// Recorta una región (en coordenadas físicas del escritorio virtual) del
    /// frame. Devuelve `None` si la región no intersecta el frame.
    pub fn crop(&self, region: Rect) -> Option<Frame> {
        let inter = self.bounds.intersection(&region)?;
        if inter.is_empty() {
            return None;
        }
        let stride = self.bounds.width as usize * 4;
        let offset_x = (inter.x - self.bounds.x) as usize;
        let offset_y = (inter.y - self.bounds.y) as usize;
        let out_stride = inter.width as usize * 4;
        let mut out = vec![0u8; out_stride * inter.height as usize];
        for row in 0..inter.height as usize {
            let src_start = (offset_y + row) * stride + offset_x * 4;
            let dst_start = row * out_stride;
            out[dst_start..dst_start + out_stride]
                .copy_from_slice(&self.bgra[src_start..src_start + out_stride]);
        }
        Some(Frame::new(inter, out))
    }

    /// Codifica el frame a PNG en memoria.
    pub fn to_png(&self) -> Result<Vec<u8>> {
        bgra_to_png(self.width(), self.height(), &self.bgra)
    }

    /// Frame desde un PNG, anclado en coords físicas `(x, y)`.
    pub fn from_png(x: i32, y: i32, png: &[u8]) -> Result<Self> {
        let (width, height, bgra) = crate::encoding::png_to_bgra(png)?;
        Ok(Frame::new(Rect::new(x, y, width, height), bgra))
    }

    /// Prepara un frame impreso de una ventana transparente (overlay).
    ///
    /// `GetDIBits` / CapturePreview a veces dejan el alpha en 0 o 255 y pintan
    /// el vacío de negro o blanco. Si no hay alpha útil, el color mayoritario
    /// de los bordes (negro o blanco) se vuelve transparente.
    pub fn prepare_overlay_layer(&mut self) -> bool {
        let has_rgb = self
            .bgra
            .chunks_exact(4)
            .any(|px| px[0] > 8 || px[1] > 8 || px[2] > 8);
        if !has_rgb {
            return false;
        }
        let mut min_a = 255u8;
        let mut max_a = 0u8;
        for px in self.bgra.chunks_exact(4) {
            min_a = min_a.min(px[3]);
            max_a = max_a.max(px[3]);
        }
        if max_a > 0 && min_a < 255 {
            return true;
        }
        let mut black = 0usize;
        let mut white = 0usize;
        let mut other = 0usize;
        for px in self.bgra.chunks_exact(4) {
            let dark = px[0] <= 8 && px[1] <= 8 && px[2] <= 8;
            let light = px[0] >= 247 && px[1] >= 247 && px[2] >= 247;
            if dark {
                black += 1;
            } else if light {
                white += 1;
            } else {
                other += 1;
            }
        }
        if other == 0 {
            return false;
        }
        let knockout_white = white > black;
        for px in self.bgra.chunks_exact_mut(4) {
            let empty = if knockout_white {
                px[0] >= 247 && px[1] >= 247 && px[2] >= 247
            } else {
                px[0] <= 8 && px[1] <= 8 && px[2] <= 8
            };
            px[3] = if empty { 0 } else { 255 };
        }
        true
    }

    /// Pinta `src` encima, usando el canal alpha. Coordenadas físicas.
    pub fn blend_over(&mut self, src: &Frame) {
        let Some(inter) = self.bounds.intersection(&src.bounds) else {
            return;
        };
        if inter.is_empty() {
            return;
        }
        let dst_stride = self.bounds.width as usize * 4;
        let src_stride = src.bounds.width as usize * 4;
        let dst_ox = (inter.x - self.bounds.x) as usize;
        let dst_oy = (inter.y - self.bounds.y) as usize;
        let src_ox = (inter.x - src.bounds.x) as usize;
        let src_oy = (inter.y - src.bounds.y) as usize;
        for row in 0..inter.height as usize {
            let dst_row = (dst_oy + row) * dst_stride + dst_ox * 4;
            let src_row = (src_oy + row) * src_stride + src_ox * 4;
            for col in 0..inter.width as usize {
                let di = dst_row + col * 4;
                let si = src_row + col * 4;
                let sa = src.bgra[si + 3];
                if sa == 0 {
                    continue;
                }
                if sa == 255 {
                    self.bgra[di..di + 3].copy_from_slice(&src.bgra[si..si + 3]);
                    continue;
                }
                let a = sa as u16;
                let inv = 255 - a;
                self.bgra[di] =
                    ((src.bgra[si] as u16 * a + self.bgra[di] as u16 * inv) / 255) as u8;
                self.bgra[di + 1] =
                    ((src.bgra[si + 1] as u16 * a + self.bgra[di + 1] as u16 * inv) / 255) as u8;
                self.bgra[di + 2] =
                    ((src.bgra[si + 2] as u16 * a + self.bgra[di + 2] as u16 * inv) / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame 4x4 donde cada pixel codifica su posición en los canales B y G,
    /// para verificar que el recorte toma la región correcta.
    fn grid_4x4() -> Frame {
        let mut bgra = Vec::with_capacity(4 * 4 * 4);
        for y in 0..4u8 {
            for x in 0..4u8 {
                bgra.extend_from_slice(&[x, y, 0, 0]); // B=x, G=y
            }
        }
        Frame::new(Rect::new(0, 0, 4, 4), bgra)
    }

    #[test]
    fn pixel_rgba_reads_red_from_bgra() {
        // BGRA: azul, verde, rojo, alpha.
        let frame = Frame::new(Rect::new(0, 0, 1, 1), vec![10, 20, 30, 255]);
        assert_eq!(frame.pixel_rgba(0, 0), Some([30, 20, 10, 255]));
        assert_eq!(frame.pixel_rgba(-1, 0), None);
        assert_eq!(frame.pixel_rgba(1, 0), None);
    }

    #[test]
    fn crop_extracts_inner_region() {
        let frame = grid_4x4();
        let cropped = frame.crop(Rect::new(1, 1, 2, 2)).unwrap();
        assert_eq!(cropped.bounds, Rect::new(1, 1, 2, 2));
        // Esquina superior izquierda del recorte = pixel (1,1).
        assert_eq!(&cropped.bgra[0..4], &[1, 1, 0, 0]);
        // Esquina inferior derecha = pixel (2,2).
        let last = cropped.bgra.len() - 4;
        assert_eq!(&cropped.bgra[last..], &[2, 2, 0, 0]);
    }

    #[test]
    fn crop_clips_region_to_frame_bounds() {
        let frame = grid_4x4();
        // Región que se sale por la derecha/abajo: se recorta a 2x2 desde (2,2).
        let cropped = frame.crop(Rect::new(2, 2, 10, 10)).unwrap();
        assert_eq!(cropped.bounds, Rect::new(2, 2, 2, 2));
    }

    #[test]
    fn crop_returns_none_when_disjoint() {
        let frame = grid_4x4();
        assert!(frame.crop(Rect::new(100, 100, 10, 10)).is_none());
    }

    #[test]
    fn crop_respects_frame_origin() {
        // Frame que empieza en (-10, -10): recortar en coords físicas reales.
        let bgra = vec![0u8; 4 * 4 * 4];
        let frame = Frame::new(Rect::new(-10, -10, 4, 4), bgra);
        let cropped = frame.crop(Rect::new(-9, -9, 2, 2)).unwrap();
        assert_eq!(cropped.bounds, Rect::new(-9, -9, 2, 2));
    }

    #[test]
    fn blend_over_copies_opaque_pixels() {
        // Destino azul; overlay rojo de 1px desplazado a x=1.
        let mut dst = Frame::new(Rect::new(0, 0, 2, 1), vec![255, 0, 0, 255, 255, 0, 0, 255]);
        let src = Frame::new(Rect::new(1, 0, 1, 1), vec![0, 0, 255, 255]);
        dst.blend_over(&src);
        assert_eq!(&dst.bgra[0..4], &[255, 0, 0, 255]);
        assert_eq!(&dst.bgra[4..8], &[0, 0, 255, 255]);
    }

    #[test]
    fn blend_over_skips_transparent() {
        let mut dst = Frame::new(Rect::new(0, 0, 1, 1), vec![1, 2, 3, 255]);
        let src = Frame::new(Rect::new(0, 0, 1, 1), vec![9, 9, 9, 0]);
        dst.blend_over(&src);
        assert_eq!(&dst.bgra, &[1, 2, 3, 255]);
    }

    #[test]
    fn prepare_overlay_knockouts_near_black_when_alpha_missing() {
        let mut frame = Frame::new(Rect::new(0, 0, 2, 1), vec![0, 0, 0, 0, 40, 40, 40, 0]);
        assert!(frame.prepare_overlay_layer());
        assert_eq!(frame.bgra[3], 0);
        assert_eq!(frame.bgra[7], 255);
    }

    #[test]
    fn prepare_overlay_rejects_empty() {
        let mut frame = Frame::new(Rect::new(0, 0, 1, 1), vec![0, 0, 0, 0]);
        assert!(!frame.prepare_overlay_layer());
    }

    #[test]
    fn prepare_overlay_knockouts_white_fill() {
        let mut frame = Frame::new(
            Rect::new(0, 0, 2, 1),
            vec![255, 255, 255, 255, 40, 40, 40, 255],
        );
        assert!(frame.prepare_overlay_layer());
        assert_eq!(frame.bgra[3], 0);
        assert_eq!(frame.bgra[7], 255);
    }
}
