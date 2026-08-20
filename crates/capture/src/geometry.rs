//! Geometría en píxeles físicos del escritorio virtual.
//!
//! Todo el motor trabaja internamente en coordenadas físicas; la conversión a
//! lógicas ocurre solo en el borde del frontend (ver `logical_to_physical`).

use serde::Serialize;

/// Rectángulo en píxeles físicos. `x`/`y` pueden ser negativos (monitores a la
/// izquierda o por encima del primario).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Construye desde bordes (left/top/right/bottom); si `right < left` o
    /// `bottom < top`, el lado resultante es 0.
    pub fn from_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        let width = (right - left).max(0) as u32;
        let height = (bottom - top).max(0) as u32;
        Self {
            x: left,
            y: top,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// `true` si el punto físico cae dentro del rectángulo (borde derecho e
    /// inferior exclusivos).
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Unión de rectángulos. Ignora vacíos; `None` si no hay ninguno útil.
    pub fn union_all(rects: impl IntoIterator<Item = Self>) -> Option<Self> {
        let mut iter = rects.into_iter().filter(|r| !r.is_empty());
        let first = iter.next()?;
        let mut left = first.x;
        let mut top = first.y;
        let mut right = first.right();
        let mut bottom = first.bottom();
        for rect in iter {
            left = left.min(rect.x);
            top = top.min(rect.y);
            right = right.max(rect.right());
            bottom = bottom.max(rect.bottom());
        }
        Some(Self::from_ltrb(left, top, right, bottom))
    }

    /// Intersección con otro rectángulo, o `None` si no se solapan.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            None
        } else {
            Some(Rect::from_ltrb(left, top, right, bottom))
        }
    }
}

/// Índice del primer rectángulo (en orden topmost-first) que contiene el punto.
///
/// La lista debe venir ordenada por z-index descendente (la ventana más al
/// frente primero), tal como la entrega `EnumWindows`.
pub fn topmost_index(rects: &[Rect], x: i32, y: i32) -> Option<usize> {
    rects.iter().position(|r| r.contains(x, y))
}

/// Convierte una coordenada lógica (del frontend) a física, dado el factor de
/// escala del monitor.
pub fn logical_to_physical(value: f64, scale: f64) -> i32 {
    (value * scale).round() as i32
}

/// Convierte una coordenada física a lógica.
pub fn physical_to_logical(value: i32, scale: f64) -> f64 {
    f64::from(value) / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_ltrb_clamps_inverted_edges() {
        let r = Rect::from_ltrb(10, 10, 5, 5);
        assert_eq!(r.width, 0);
        assert_eq!(r.height, 0);
        assert!(r.is_empty());
    }

    #[test]
    fn from_ltrb_supports_negative_origin() {
        let r = Rect::from_ltrb(-1920, -100, 0, 980);
        assert_eq!(r, Rect::new(-1920, -100, 1920, 1080));
        assert_eq!(r.right(), 0);
        assert_eq!(r.bottom(), 980);
    }

    #[test]
    fn contains_excludes_far_edges() {
        let r = Rect::new(0, 0, 100, 50);
        assert!(r.contains(0, 0));
        assert!(r.contains(99, 49));
        assert!(!r.contains(100, 49));
        assert!(!r.contains(50, 50));
        assert!(!r.contains(-1, 10));
    }

    #[test]
    fn union_all_covers_two_monitors() {
        let left = Rect::new(0, 0, 1920, 1080);
        let right = Rect::new(1920, 0, 1920, 1080);
        assert_eq!(
            Rect::union_all([left, right]),
            Some(Rect::new(0, 0, 3840, 1080))
        );
        assert_eq!(Rect::union_all([Rect::new(0, 0, 0, 10)]), None);
    }

    #[test]
    fn intersection_of_overlapping_rects() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        assert_eq!(a.intersection(&b), Some(Rect::new(50, 50, 50, 50)));
    }

    #[test]
    fn intersection_none_when_disjoint() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 20, 10, 10);
        assert_eq!(a.intersection(&b), None);
        // Bordes que solo se tocan no cuentan como intersección.
        let c = Rect::new(10, 0, 10, 10);
        assert_eq!(a.intersection(&c), None);
    }

    #[test]
    fn topmost_index_respects_z_order() {
        let rects = [
            Rect::new(10, 10, 100, 100), // topmost
            Rect::new(0, 0, 200, 200),   // detrás, más grande
        ];
        // El punto cae en ambos: gana la primera (topmost).
        assert_eq!(topmost_index(&rects, 50, 50), Some(0));
        // El punto solo cae en la de atrás.
        assert_eq!(topmost_index(&rects, 5, 5), Some(1));
        // Fuera de ambas.
        assert_eq!(topmost_index(&rects, 500, 500), None);
    }

    #[test]
    fn dpi_conversion_roundtrips() {
        assert_eq!(logical_to_physical(100.0, 1.5), 150);
        assert_eq!(logical_to_physical(100.0, 1.0), 100);
        assert!((physical_to_logical(150, 1.5) - 100.0).abs() < 1e-9);
    }
}
