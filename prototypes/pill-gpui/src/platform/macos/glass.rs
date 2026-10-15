//! Vidrio esmerilado detrás de la pill en macOS. Pendiente: irá con un
//! `NSVisualEffectView` en una ventana aparte, debajo del overlay, recortado
//! con la forma de la pill. Mientras tanto `Glass::new` no da vidrio y la pill
//! usa su piel sin él (lo mismo que en Windows sin transparencias).

use super::win::Handle;
use crate::geometry::Rect;

/// La forma del vidrio, en coordenadas lógicas del overlay.
#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    Rounded { rect: Rect, radii: [f32; 4] },
    Pair { a: (Rect, [f32; 4]), b: (Rect, [f32; 4]) },
    Circle { center: (f32, f32), r: f32 },
}

pub fn enabled() -> bool {
    std::env::var("PILL_GLASS").as_deref() != Ok("off")
}

pub fn backdrop_available() -> bool {
    false
}

pub struct Glass;

impl Glass {
    pub fn new() -> Option<Self> {
        None
    }

    pub fn hide(&mut self) {}

    pub fn place(&mut self, _shape: Shape, _above: Handle, _origin: (i32, i32), _scale: f32) {}
}
