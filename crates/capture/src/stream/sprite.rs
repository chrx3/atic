//! Los cursores que dibuja el editor de grabaciones, como mapas de bits: las mismas formas
//! de `export_macos.rs` (flecha, mano, I-beam, cruz, punto y anillo), pero armadas acá sin
//! Core Image ni nada del sistema.
//!
//! En macOS la flecha de [`CursorStyle::System`] es la de `NSCursor`; acá no hay, así que es
//! la flecha dibujada en tono claro.

use std::sync::OnceLock;

use super::types::{CursorShape, CursorStyle};

/// Los píxeles de `bgra` por punto: 4× de supermuestreo.
const SCALE: f64 = 4.0;

/// El alto en puntos de la flecha dibujada aquí (el de referencia).
const DRAWN_ARROW_H: f64 = 21.0;

/// Un cursor ya dibujado, listo para pegarse en el cuadro.
pub(super) struct Sprite {
    /// BGRA con alfa premultiplicado, `width*height*4` bytes, fila por fila arriba->abajo.
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Cuántos píxeles de `bgra` hay por punto (supermuestreo; usa 4).
    pub scale: f64,
    /// Medidas en puntos (`width/scale`, `height/scale`) y punto caliente en puntos desde
    /// arriba a la izquierda.
    pub size: (f64, f64),
    pub hot: (f64, f64),
}

impl Sprite {
    /// El alfa del píxel `(px, py)`.
    #[cfg(test)]
    fn alpha(&self, px: u32, py: u32) -> u8 {
        self.bgra[((py * self.width + px) * 4 + 3) as usize]
    }
}

/// De qué color se dibuja un cursor a mano: blanco con borde negro, o negro con borde blanco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Light,
    Dark,
}

impl Tone {
    /// El tono (0–255) del relleno y del borde.
    fn values(self) -> (u32, u32) {
        match self {
            Tone::Light => (255, 0),
            Tone::Dark => (0, 255),
        }
    }
}

/// Un cursor dibujado a mano de `w × h` puntos, a 4×: `fill(x, y)` dice si el punto (en
/// puntos, desde arriba a la izquierda) va en el color de relleno y `border(x, y)` si va en
/// el del borde alrededor (según el `tone`). `hot` es su punta.
///
/// Cada píxel de salida se muestrea 2×2 (bordes suaves): el alfa es la cobertura —cuántas
/// muestras caen dentro— y el color es la mezcla de los dos tonos según esas muestras, que
/// nunca pasa del alfa (queda premultiplicado). Es la misma cuenta que `raster_sprite`.
fn raster(w: usize, h: usize, hot: (f64, f64), tone: Tone, fill: &dyn Fn(f64, f64) -> bool, border: &dyn Fn(f64, f64) -> bool) -> Sprite {
    let (fill_tone, border_tone) = tone.values();
    let (pw, ph) = (w * 4, h * 4);
    let mut bgra = vec![0u8; pw * ph * 4];
    for py in 0..ph {
        for px in 0..pw {
            // Muestras 2×2 por píxel, a la misma posición que en macOS.
            let (mut inner, mut outer) = (0u32, 0u32);
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let (x, y) = ((px as f64 + sx) / SCALE, (py as f64 + sy) / SCALE);
                if fill(x, y) {
                    inner += 1;
                } else if border(x, y) {
                    outer += 1;
                }
            }
            let a = (inner + outer) * 255 / 4;
            let v = (inner * fill_tone + outer * border_tone) / 4;
            let i = (py * pw + px) * 4;
            // BGRA premultiplicado: el gris ya viene pesado por la cobertura.
            bgra[i..i + 4].copy_from_slice(&[v as u8, v as u8, v as u8, a as u8]);
        }
    }
    Sprite { bgra, width: pw as u32, height: ph as u32, scale: SCALE, size: (w as f64, h as f64), hot }
}

/// La distancia del punto `(x, y)` al segmento `a`–`b`.
fn segment_distance((x, y): (f64, f64), (ax, ay): (f64, f64), (bx, by): (f64, f64)) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 0.0 { 0.0 } else { (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0) };
    ((x - (ax + t * dx)).powi(2) + (y - (ay + t * dy)).powi(2)).sqrt()
}

/// Si `(x, y)` cae en alguna de las cápsulas `(desde, hasta, radio)` agrandadas en `grow`.
fn in_capsules(capsules: &[((f64, f64), (f64, f64), f64)], x: f64, y: f64, grow: f64) -> bool {
    capsules.iter().any(|&(a, b, r)| segment_distance((x, y), a, b) <= r + grow)
}

/// La flecha de siempre: blanca con borde negro, a 4× (medidas en puntos: 14 × 21, con la
/// punta arriba a la izquierda).
fn drawn_arrow(tone: Tone) -> Sprite {
    // El contorno en puntos, desde la punta.
    let outline: [(f64, f64); 7] = [(1.0, 1.0), (1.0, 17.0), (5.0, 13.2), (7.8, 19.5), (10.2, 18.4), (7.5, 12.3), (12.6, 12.3)];
    let inside = |poly: &[(f64, f64)], x: f64, y: f64| {
        let mut odd = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                odd = !odd;
            }
            j = i;
        }
        odd
    };
    // El borde: el mismo contorno, un poco más grande alrededor de su centro.
    let grow = |(x, y): (f64, f64), by: f64| -> (f64, f64) {
        let (cx, cy) = (5.5, 11.0);
        let (dx, dy) = (x - cx, y - cy);
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        (x + dx / len * by, y + dy / len * by)
    };
    let border: Vec<(f64, f64)> = outline.iter().map(|p| grow(*p, 1.1)).collect();
    raster(14, DRAWN_ARROW_H as usize, (1.0, 1.0), tone, &|x, y| inside(&outline, x, y), &|x, y| inside(&border, x, y))
}

/// La mano de los vínculos (24 × 23 puntos): el índice arriba, los otros dedos y la palma;
/// la punta del índice es su punto caliente.
fn drawn_hand(tone: Tone) -> Sprite {
    // Cápsulas (desde, hasta, radio): índice, medio, anular, meñique, palma y pulgar.
    let shape: [((f64, f64), (f64, f64), f64); 6] = [
        ((8.0, 3.8), (8.0, 13.0), 1.9),
        ((11.8, 8.0), (11.8, 13.0), 1.9),
        ((15.6, 9.0), (15.6, 13.0), 1.9),
        ((19.4, 10.5), (19.4, 13.0), 1.9),
        ((9.0, 16.5), (17.5, 16.5), 4.5),
        ((6.0, 15.5), (3.2, 12.0), 1.8),
    ];
    raster(24, 23, (8.0, 2.0), tone, &|x, y| in_capsules(&shape, x, y, 0.0), &|x, y| in_capsules(&shape, x, y, 1.0))
}

/// El cursor de texto (12 × 22 puntos): una barra con remates arriba y abajo; el punto
/// caliente es su centro.
fn drawn_ibeam(tone: Tone) -> Sprite {
    // Cápsulas finas: la barra y los dos remates.
    let shape: [((f64, f64), (f64, f64), f64); 3] = [((6.0, 3.0), (6.0, 19.0), 1.0), ((3.2, 2.6), (8.8, 2.6), 0.9), ((3.2, 19.4), (8.8, 19.4), 0.9)];
    raster(12, 22, (6.0, 11.0), tone, &|x, y| in_capsules(&shape, x, y, 0.0), &|x, y| in_capsules(&shape, x, y, 1.0))
}

/// La cruz de precisión (22 × 22 puntos); el punto caliente es su centro.
fn drawn_cross(tone: Tone) -> Sprite {
    let shape: [((f64, f64), (f64, f64), f64); 2] = [((3.0, 11.0), (19.0, 11.0), 1.0), ((11.0, 3.0), (11.0, 19.0), 1.0)];
    raster(22, 22, (11.0, 11.0), tone, &|x, y| in_capsules(&shape, x, y, 0.0), &|x, y| in_capsules(&shape, x, y, 1.0))
}

/// Si `(x, y)` está a una distancia de `(cx, cy)` entre `from` y `to`.
fn in_annulus((x, y): (f64, f64), (cx, cy): (f64, f64), from: f64, to: f64) -> bool {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    d >= from && d <= to
}

/// El punto: un disco blanco con borde oscuro (14 × 14 puntos); el punto caliente es su
/// centro.
fn drawn_dot() -> Sprite {
    let c = (7.0, 7.0);
    raster(14, 14, c, Tone::Light, &|x, y| in_annulus((x, y), c, 0.0, 5.0), &|x, y| in_annulus((x, y), c, 0.0, 6.2))
}

/// El anillo: blanco, de centro vacío, con un borde oscuro fino por dentro y por fuera
/// (20 × 20 puntos); el punto caliente es su centro.
fn drawn_ring() -> Sprite {
    let c = (10.0, 10.0);
    raster(20, 20, c, Tone::Light, &|x, y| in_annulus((x, y), c, 5.6, 8.0), &|x, y| in_annulus((x, y), c, 4.8, 8.9))
}

/// Flecha, mano, I-beam y cruz de un mismo tono.
struct Shapes {
    arrow: Sprite,
    hand: Sprite,
    ibeam: Sprite,
    cross: Sprite,
}

impl Shapes {
    fn of(tone: Tone) -> Self {
        Self { arrow: drawn_arrow(tone), hand: drawn_hand(tone), ibeam: drawn_ibeam(tone), cross: drawn_cross(tone) }
    }

    fn pick(&self, shape: CursorShape) -> &Sprite {
        match shape {
            CursorShape::Arrow => &self.arrow,
            CursorShape::Hand => &self.hand,
            CursorShape::IBeam => &self.ibeam,
            CursorShape::Cross => &self.cross,
        }
    }
}

/// Los cursores de cada estilo y forma, armados una vez.
struct Sprites {
    system: Shapes,
    dark: Shapes,
    dot: Sprite,
    ring: Sprite,
}

impl Sprites {
    fn new() -> Self {
        let system = Shapes::of(Tone::Light);
        // La flecha negra se dibuja acá, al alto de la del sistema. En macOS el sistema
        // entrega la suya y esta se reescala por `system.arrow.height / DRAWN_ARROW_H`; acá
        // la del sistema es la dibujada, así que ese factor es 1.
        let dark = Shapes::of(Tone::Dark);
        Self { system, dark, dot: drawn_dot(), ring: drawn_ring() }
    }

    /// El cursor de `style` para `shape`. `Dot` y `Ring` valen para toda forma menos el
    /// I-beam, que conserva la suya.
    fn pick(&self, style: CursorStyle, shape: CursorShape) -> &Sprite {
        match (style, shape) {
            (CursorStyle::System, _) => self.system.pick(shape),
            (CursorStyle::Dark, _) => self.dark.pick(shape),
            (CursorStyle::Dot | CursorStyle::Ring, CursorShape::IBeam) => &self.system.ibeam,
            (CursorStyle::Dot, _) => &self.dot,
            (CursorStyle::Ring, _) => &self.ring,
            (CursorStyle::Arrow, _) => &self.system.arrow,
        }
    }

    /// El alto en puntos al que se mide `cursor_px`: el de la flecha del sistema.
    fn reference_height(&self) -> f64 {
        self.system.arrow.size.1.max(1.0)
    }
}

/// Los cursores de todos los estilos, armados la primera vez que se piden.
static SPRITES: OnceLock<Sprites> = OnceLock::new();

fn sprites() -> &'static Sprites {
    SPRITES.get_or_init(Sprites::new)
}

/// El sprite para ese estilo y forma.
pub(super) fn pick(style: CursorStyle, shape: CursorShape) -> &'static Sprite {
    sprites().pick(style, shape)
}

/// El alto de referencia (en puntos) de la flecha, contra el que se escala `Scene::cursor_px`.
pub(super) fn reference_height() -> f64 {
    sprites().reference_height()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPES: [CursorShape; 4] = [CursorShape::Arrow, CursorShape::Hand, CursorShape::IBeam, CursorShape::Cross];
    const STYLES: [CursorStyle; 5] = [CursorStyle::System, CursorStyle::Dot, CursorStyle::Ring, CursorStyle::Dark, CursorStyle::Arrow];

    fn same(a: &Sprite, b: &Sprite) -> bool {
        std::ptr::eq(a, b)
    }

    #[test]
    fn la_flecha_tiene_su_punta_dentro_y_opaca() {
        let arrow = pick(CursorStyle::System, CursorShape::Arrow);
        let (hx, hy) = arrow.hot;
        assert!(hx >= 0.0 && hy >= 0.0 && hx < arrow.size.0 && hy < arrow.size.1, "la punta {hx}, {hy} cae en {:?}", arrow.size);
        // Cerca de la punta (3 puntos): alguna muestra cubierta del todo.
        let (tx, ty) = ((hx * arrow.scale).round() as u32, (hy * arrow.scale).round() as u32);
        let reach = (3.0 * arrow.scale) as u32;
        let opaque = (0..=reach).any(|dy| (0..=reach).any(|dx| arrow.alpha(tx + dx, ty + dy) == 255));
        assert!(opaque, "la punta no quedó pintada");
    }

    #[test]
    fn el_bitmap_cuadra_con_su_medida_en_puntos() {
        for style in STYLES {
            for shape in SHAPES {
                let sprite = pick(style, shape);
                assert_eq!(sprite.width as f64, sprite.size.0 * sprite.scale, "{style:?} {shape:?}");
                assert_eq!(sprite.height as f64, sprite.size.1 * sprite.scale, "{style:?} {shape:?}");
                assert_eq!(sprite.bgra.len(), (sprite.width * sprite.height * 4) as usize, "{style:?} {shape:?}");
            }
        }
    }

    #[test]
    fn el_ibeam_es_mas_alto_que_ancho() {
        let ibeam = pick(CursorStyle::System, CursorShape::IBeam);
        assert!(ibeam.size.1 > ibeam.size.0);
        assert_eq!(ibeam.hot, (ibeam.size.0 / 2.0, ibeam.size.1 / 2.0));
    }

    #[test]
    fn el_punto_y_el_anillo_cambian_la_forma_menos_el_ibeam() {
        let ibeam = pick(CursorStyle::System, CursorShape::IBeam);
        assert!(same(pick(CursorStyle::Dot, CursorShape::IBeam), ibeam));
        assert!(same(pick(CursorStyle::Ring, CursorShape::IBeam), ibeam));
        for shape in [CursorShape::Arrow, CursorShape::Hand, CursorShape::Cross] {
            assert!(!same(pick(CursorStyle::Dot, shape), pick(CursorStyle::System, shape)), "{shape:?} con punto");
            assert!(!same(pick(CursorStyle::Ring, shape), pick(CursorStyle::System, shape)), "{shape:?} con anillo");
        }
    }

    #[test]
    fn el_estilo_flecha_da_la_flecha_para_toda_forma() {
        let arrow = pick(CursorStyle::System, CursorShape::Arrow);
        for shape in SHAPES {
            assert!(same(pick(CursorStyle::Arrow, shape), arrow), "{shape:?}");
        }
        // La oscura dibujada mide lo mismo que la clara: es el alto de referencia.
        let dark = pick(CursorStyle::Dark, CursorShape::Arrow);
        assert_eq!(dark.size, arrow.size);
        assert_eq!(reference_height(), arrow.size.1.max(1.0));
    }

    #[test]
    fn el_exterior_de_la_flecha_es_transparente() {
        let arrow = pick(CursorStyle::System, CursorShape::Arrow);
        let (w, h) = (arrow.width, arrow.height);
        // Las cuatro esquinas y un par de puntos lejos de la forma.
        for (px, py) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1), (w - 1, h / 2), (w / 2, 0)] {
            assert_eq!(arrow.alpha(px, py), 0, "({px}, {py})");
        }
        // La flecha clara es blanca y opaca en su relleno.
        assert!(arrow.bgra.chunks_exact(4).any(|p| p.iter().all(|&b| b == 255)));
    }

    #[test]
    fn los_bytes_estan_premultiplicados() {
        for style in STYLES {
            for shape in SHAPES {
                let sprite = pick(style, shape);
                for p in sprite.bgra.chunks_exact(4) {
                    assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "{style:?} {shape:?}: {p:?}");
                }
            }
        }
    }
}
