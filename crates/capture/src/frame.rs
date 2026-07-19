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
}
