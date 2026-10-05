//! Silueta de la piel: tab acoplado, gotas y los "cuellos" que las unen
//! mientras se separan (metaball de dos círculos).
//!
//! Todo se agrega a un solo path con regla nonzero y el mismo sentido de giro:
//! si cada forma se pinta por separado, el antialias de un borde que cae sobre
//! otra forma deja ver el fondo como una raya oscura.

use std::f32::consts::PI;

use gpui::{point, px, FillOptions, FillRule, Path, PathBuilder, PathStyle, Pixels, Point};

use crate::geometry::Rect;

fn pt((x, y): (f32, f32)) -> Point<Pixels> {
    point(px(x), px(y))
}

pub struct Silhouette {
    builder: PathBuilder,
}

impl Silhouette {
    pub fn new() -> Self {
        let style = PathStyle::Fill(FillOptions::default().with_fill_rule(FillRule::NonZero));
        Self {
            builder: PathBuilder::fill().with_style(style),
        }
    }

    pub fn build(self) -> Option<Path<Pixels>> {
        self.builder.build().ok()
    }

    /// Círculo en sentido horario (en pantalla, con y hacia abajo).
    pub fn circle(&mut self, (cx, cy): (f32, f32), r: f32) {
        if r <= 0.05 {
            return;
        }
        let b = &mut self.builder;
        b.move_to(pt((cx + r, cy)));
        b.arc_to(point(px(r), px(r)), px(0.), false, true, pt((cx - r, cy)));
        b.arc_to(point(px(r), px(r)), px(0.), false, true, pt((cx + r, cy)));
        b.close();
    }

    /// Un contorno cerrado ya armado (el tab de un costado con su bloque,
    /// `geometry::side_outline`), en el mismo sentido que lo demás.
    pub fn polygon(&mut self, points: &[(f32, f32)]) {
        let Some((&first, rest)) = points.split_first() else {
            return;
        };
        let b = &mut self.builder;
        b.move_to(pt(first));
        for &p in rest {
            b.line_to(pt(p));
        }
        b.close();
    }

    /// Rectángulo redondeado en sentido horario.
    pub fn rounded_rect(&mut self, left: f32, top: f32, width: f32, height: f32, radius: f32) {
        let r = radius.min(width / 2.0).min(height / 2.0);
        let (right, bottom) = (left + width, top + height);
        let corner = point(px(r), px(r));
        let b = &mut self.builder;
        b.move_to(pt((left + r, top)));
        b.line_to(pt((right - r, top)));
        b.arc_to(corner, px(0.), false, true, pt((right, top + r)));
        b.line_to(pt((right, bottom - r)));
        b.arc_to(corner, px(0.), false, true, pt((right - r, bottom)));
        b.line_to(pt((left + r, bottom)));
        b.arc_to(corner, px(0.), false, true, pt((left, bottom - r)));
        b.line_to(pt((left, top + r)));
        b.arc_to(corner, px(0.), false, true, pt((left + r, top)));
        b.close();
    }

    /// Rectángulo con un radio por esquina, en orden: arriba-izquierda,
    /// arriba-derecha, abajo-derecha, abajo-izquierda. Radio 0 = esquina viva.
    pub fn rounded_rect_corners(&mut self, rect: &Rect, radii: [f32; 4]) {
        let limit = (rect.w / 2.0).min(rect.h / 2.0);
        let [tl, tr, br, bl] = radii.map(|r| r.clamp(0.0, limit));
        let (left, top, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
        let b = &mut self.builder;
        b.move_to(pt((left + tl, top)));
        b.line_to(pt((right - tr, top)));
        if tr > 0.0 {
            b.arc_to(
                point(px(tr), px(tr)),
                px(0.),
                false,
                true,
                pt((right, top + tr)),
            );
        }
        b.line_to(pt((right, bottom - br)));
        if br > 0.0 {
            b.arc_to(
                point(px(br), px(br)),
                px(0.),
                false,
                true,
                pt((right - br, bottom)),
            );
        }
        b.line_to(pt((left + bl, bottom)));
        if bl > 0.0 {
            b.arc_to(
                point(px(bl), px(bl)),
                px(0.),
                false,
                true,
                pt((left, bottom - bl)),
            );
        }
        b.line_to(pt((left, top + tl)));
        if tl > 0.0 {
            b.arc_to(
                point(px(tl), px(tl)),
                px(0.),
                false,
                true,
                pt((left + tl, top)),
            );
        }
        b.close();
    }

    /// Puente entre dos círculos que se estira y se corta al alejarse, como dos
    /// gotas que se separan.
    pub fn neck(&mut self, c1: (f32, f32), r1: f32, c2: (f32, f32), r2: f32) {
        self.neck_within(c1, r1, c2, r2, r2 * 1.5);
    }

    /// Como `neck`, pero el puente se corta cuando el espacio entre los dos
    /// círculos supera `max_gap`.
    pub fn neck_within(&mut self, c1: (f32, f32), r1: f32, c2: (f32, f32), r2: f32, max_gap: f32) {
        const SPREAD: f32 = 0.5;
        const HANDLE: f32 = 2.4;

        let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
        let d = (dx * dx + dy * dy).sqrt();
        let max_distance = r1 + r2 + max_gap;
        // Muy superpuestas, la unión de círculos ya es continua y el puente se
        // cruzaría consigo mismo (un lóbulo giraría al revés y abriría un hueco).
        let min_distance = r1.max(r2);
        if r1 <= 0.5 || r2 <= 0.5 || d > max_distance || d <= min_distance {
            return;
        }

        let (u1, u2) = if d < r1 + r2 {
            (
                ((r1 * r1 + d * d - r2 * r2) / (2.0 * r1 * d))
                    .clamp(-1.0, 1.0)
                    .acos(),
                ((r2 * r2 + d * d - r1 * r1) / (2.0 * r2 * d))
                    .clamp(-1.0, 1.0)
                    .acos(),
            )
        } else {
            (0.0, 0.0)
        };

        let between = dy.atan2(dx);
        let max_spread = ((r1 - r2) / d).clamp(-1.0, 1.0).acos();
        let angle1 = between + u1 + (max_spread - u1) * SPREAD;
        let angle2 = between - u1 - (max_spread - u1) * SPREAD;
        let angle3 = between + PI - u2 - (PI - u2 - max_spread) * SPREAD;
        let angle4 = between - PI + u2 + (PI - u2 - max_spread) * SPREAD;

        let on = |c: (f32, f32), r: f32, a: f32| (c.0 + r * a.cos(), c.1 + r * a.sin());
        let p1 = on(c1, r1, angle1);
        let p2 = on(c1, r1, angle2);
        let p3 = on(c2, r2, angle3);
        let p4 = on(c2, r2, angle4);

        let total = r1 + r2;
        let p1p3 = ((p3.0 - p1.0).powi(2) + (p3.1 - p1.1).powi(2)).sqrt();
        let handle = (SPREAD * HANDLE).min(p1p3 / total) * (d * 2.0 / total).min(1.0);
        let (h1r, h2r) = (r1 * handle, r2 * handle);

        let h1 = on(p1, h1r, angle1 - PI / 2.0);
        let h2 = on(p2, h1r, angle2 + PI / 2.0);
        let h3 = on(p3, h2r, angle3 + PI / 2.0);
        let h4 = on(p4, h2r, angle4 - PI / 2.0);

        // Mismo sentido de giro que los círculos; si no, la regla nonzero
        // restaría el puente en vez de sumarlo.
        let b = &mut self.builder;
        if signed_area(&[p1, p3, p4, p2]) >= 0.0 {
            b.move_to(pt(p1));
            b.cubic_bezier_to(pt(p3), pt(h1), pt(h3));
            b.line_to(pt(p4));
            b.cubic_bezier_to(pt(p2), pt(h4), pt(h2));
        } else {
            b.move_to(pt(p2));
            b.cubic_bezier_to(pt(p4), pt(h2), pt(h4));
            b.line_to(pt(p3));
            b.cubic_bezier_to(pt(p1), pt(h3), pt(h1));
        }
        b.close();
    }
}

/// Positiva en sentido horario en pantalla (y hacia abajo).
fn signed_area(points: &[(f32, f32)]) -> f32 {
    let mut area = 0.0;
    for (index, a) in points.iter().enumerate() {
        let b = points[(index + 1) % points.len()];
        area += a.0 * b.1 - b.0 * a.1;
    }
    area / 2.0
}

/// Elipse rellena suelta, para los ojos de la marca.
pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path<Pixels>> {
    if rx <= 0.05 || ry <= 0.05 {
        return None;
    }
    let mut builder = PathBuilder::fill();
    builder.move_to(pt((cx + rx, cy)));
    builder.arc_to(
        point(px(rx), px(ry)),
        px(0.),
        false,
        true,
        pt((cx - rx, cy)),
    );
    builder.arc_to(
        point(px(rx), px(ry)),
        px(0.),
        false,
        true,
        pt((cx + rx, cy)),
    );
    builder.close();
    builder.build().ok()
}
